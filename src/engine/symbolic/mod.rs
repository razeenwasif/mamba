//! # Symbolic pipeline (algebra & calculus)
//!
//! Verifies that a user's expression is **algebraically equivalent** to the
//! expected one — `(x-3)(x+2)` ≡ `x^2 - x - 6`, `(x+y)^2` ≡ `x^2 + 2*x*y + y^2`,
//! `sin(x)^2 + cos(x)^2` ≡ `1` — across **any number of variables**, rather than
//! merely textually.
//!
//! ## How: multivariate numeric probing
//!
//! Two expressions are equal as functions iff they agree everywhere. By the
//! [Schwartz–Zippel lemma], two *distinct* rational expressions of bounded
//! degree agree on only a thin set of points, so evaluating both at several
//! well-separated points — over *all* their free variables at once — and finding
//! them equal every time is overwhelming evidence of equivalence. False
//! positives are astronomically unlikely for the expressions a practice tool
//! poses, and the method naturally covers multivariate polynomials, rational
//! functions, and trigonometric identities (`meval` supplies `sin`/`cos`/…).
//!
//! This stays dependency-light, pure-Rust, and MIT-compatible — see ADR 0003 for
//! why we did *not* embed a proprietary CAS (`symbolica`). This module is the
//! equivalence *seam* for the router; a true CAS could still slot in here later
//! for exact, proof-grade verification without touching anything else.
//!
//! [Schwartz–Zippel lemma]: https://en.wikipedia.org/wiki/Schwartz%E2%80%93Zippel_lemma

use std::collections::BTreeSet;

use meval::{Context, Expr};

use crate::engine::Verdict;

/// Number of probe rounds. More than the single-variable case once the space is
/// multi-dimensional, yet still trivially cheap.
const ROUNDS: usize = 12;

/// Absolute + relative tolerance for the floating-point agreement test.
const EPSILON: f64 = 1e-7;

/// Per-round base points — well-separated, none at a small-integer root, so a
/// wrong answer is extremely unlikely to coincide at every point. Deterministic
/// (not random) so verification is perfectly reproducible.
const ROUND_BASE: [f64; ROUNDS] = [
    -2.3, -1.1, -0.37, 0.71, 1.29, 2.93, 4.13, 5.61, -3.7, 0.53, 3.31, 1.87,
];

/// Verifies `input` against `expected` by multivariate algebraic (numeric-probe)
/// equivalence over every free variable appearing in either expression.
pub fn verify(expected: &str, input: &str) -> Verdict {
    let expected_src = normalize_implicit_mul(expected);
    let input_src = normalize_implicit_mul(input);

    // The expected answer is authored by us; a parse failure here is a bank bug,
    // surfaced plainly rather than blamed on the user.
    let expected_expr: Expr = match expected_src.parse() {
        Ok(e) => e,
        Err(e) => {
            return Verdict::parse_error(
                format!("internal: expected answer failed to parse ({e})"),
                None,
            );
        }
    };
    // The user's answer: a parse failure is *their* syntax error — hint them.
    let input_expr: Expr = match input_src.parse() {
        Ok(e) => e,
        Err(e) => {
            return Verdict::parse_error(
                format!("couldn't parse expression: {e}"),
                Some(
                    "use explicit operators where unambiguous, e.g. 2*x, x^2, \
                     (x-1)*(x+1), and * between variables (2*x*y, not 2xy)"
                        .into(),
                ),
            );
        }
    };

    // Free variables = the union over both sides, so a wrong answer that drops
    // or introduces a variable is caught (it must agree as `y` varies too).
    let mut vars = BTreeSet::new();
    collect_variables(&expected_src, &mut vars);
    collect_variables(&input_src, &mut vars);
    let vars: Vec<String> = vars.into_iter().collect();

    let mut compared = 0usize;
    for round in 0..ROUNDS {
        // A fresh context per round, with each free variable bound to its
        // deterministic probe value. `Context::new` supplies pi/e and the
        // trig/transcendental functions.
        let mut ctx = Context::new();
        for (vi, name) in vars.iter().enumerate() {
            ctx.var(name.as_str(), probe_value(vi, round));
        }

        let (a, b) = match (
            expected_expr.eval_with_context(&ctx),
            input_expr.eval_with_context(&ctx),
        ) {
            (Ok(a), Ok(b)) => (a, b),
            // A fully-bound expression should always evaluate; an error means an
            // identifier we failed to classify. Ours is internal; theirs is a
            // syntax problem.
            (Err(e), _) => {
                return Verdict::parse_error(
                    format!("internal: expected answer didn't evaluate ({e})"),
                    None,
                );
            }
            (_, Err(e)) => {
                return Verdict::parse_error(
                    format!("couldn't evaluate expression: {e}"),
                    Some("check your function and variable names".into()),
                );
            }
        };

        // Skip points where either side is undefined (e.g. a pole); a valid
        // expression still agrees on the finite points.
        if !a.is_finite() || !b.is_finite() {
            continue;
        }
        compared += 1;
        if (a - b).abs() > EPSILON * (1.0 + a.abs()) {
            let detail = format!(
                "not equivalent: at {} your answer gives {b:.4}, expected {a:.4}",
                format_point(&vars, round)
            );
            return Verdict::incorrect_maybe(detail, diagnose(expected, input));
        }
    }

    if compared == 0 {
        // Both sides were non-finite at every probe point — can't conclude.
        return Verdict::parse_error(
            "could not evaluate either expression at the probe points",
            Some("avoid divisions that are undefined across the domain".into()),
        );
    }

    Verdict::Correct
}

/// Near-miss diagnosis for a wrong symbolic answer: is the input a *recognized
/// transform* of the expected expression? Reuses [`equiv`] (the probe core,
/// without diagnostics, so there's no recursion) to test the answer against the
/// negative, simple rescalings, and the reciprocal of the expected one.
fn diagnose(expected: &str, input: &str) -> Option<String> {
    if equiv(&format!("-({expected})"), input) {
        return Some("you're off by a sign — your answer is the negative of the expected one".into());
    }
    for (k, label) in [
        ("2", "a factor of 2"),
        ("1/2", "a factor of 1/2"),
        ("3", "a factor of 3"),
        ("1/3", "a factor of 1/3"),
        ("10", "a factor of 10"),
        ("1/10", "a factor of 1/10"),
    ] {
        if equiv(&format!("({k})*({expected})"), input) {
            return Some(format!("off by {label} — rescale your answer"));
        }
    }
    if equiv(&format!("1/({expected})"), input) {
        return Some("that's the reciprocal of the expected answer".into());
    }
    None
}

/// Probe-based equivalence as a plain boolean — the diagnostic core. Unlike
/// [`verify`] it produces no witness and runs no diagnostics, so it is safe to
/// call from [`diagnose`] without recursing.
fn equiv(expected: &str, input: &str) -> bool {
    let e = normalize_implicit_mul(expected);
    let i = normalize_implicit_mul(input);
    let (Ok(ee), Ok(ie)) = (e.parse::<Expr>(), i.parse::<Expr>()) else {
        return false;
    };
    let mut vars = BTreeSet::new();
    collect_variables(&e, &mut vars);
    collect_variables(&i, &mut vars);
    let vars: Vec<String> = vars.into_iter().collect();

    let mut compared = 0usize;
    for round in 0..ROUNDS {
        let mut ctx = Context::new();
        for (vi, name) in vars.iter().enumerate() {
            ctx.var(name.as_str(), probe_value(vi, round));
        }
        let (Ok(a), Ok(b)) = (ee.eval_with_context(&ctx), ie.eval_with_context(&ctx)) else {
            return false;
        };
        if !a.is_finite() || !b.is_finite() {
            continue;
        }
        compared += 1;
        if (a - b).abs() > EPSILON * (1.0 + a.abs()) {
            return false;
        }
    }
    compared > 0
}

/// The deterministic probe value for variable `var_index` in round `round`.
/// Different variables get *different* values within a round (so `x*y` is
/// distinguished from `x+y`), and the assignment moves each round.
fn probe_value(var_index: usize, round: usize) -> f64 {
    // A golden-ratio stride keeps distinct variables at distinct, irrational-ish
    // offsets; scaling by the round spreads them differently on each pass.
    const GOLDEN: f64 = 0.618_033_988_75;
    ROUND_BASE[round] + (var_index as f64 + 1.0) * GOLDEN * (round as f64 * 0.5 + 1.0)
}

/// Renders a round's probe assignment for a witness message, e.g.
/// `(x = 0.7100, y = 2.3456)`.
fn format_point(vars: &[String], round: usize) -> String {
    if vars.is_empty() {
        return "every point".into();
    }
    let parts: Vec<String> = vars
        .iter()
        .enumerate()
        .map(|(vi, name)| format!("{name} = {:.4}", probe_value(vi, round)))
        .collect();
    format!("({})", parts.join(", "))
}

/// Collects the free variables in `src` into `out`: maximal identifier runs that
/// are neither a function call (immediately followed by `(`) nor a built-in
/// constant (`pi`, `e`). Multi-letter names (`theta`) are single variables;
/// juxtaposed variables must be `*`-separated (`x*y`, not `xy`) — see the
/// normalizer.
fn collect_variables(src: &str, out: &mut BTreeSet<String>) {
    let chars: Vec<char> = src.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        if chars[i].is_ascii_alphabetic() || chars[i] == '_' {
            let start = i;
            while i < chars.len() && (chars[i].is_ascii_alphanumeric() || chars[i] == '_') {
                i += 1;
            }
            let ident: String = chars[start..i].iter().collect();
            // Look past spaces for a '(' — that marks a function call, not a var.
            let mut j = i;
            while j < chars.len() && chars[j].is_whitespace() {
                j += 1;
            }
            let is_call = chars.get(j) == Some(&'(');
            if !is_call && !is_constant(&ident) {
                out.insert(ident);
            }
        } else {
            i += 1;
        }
    }
}

/// Built-in constants `meval`'s default context provides — never variables.
fn is_constant(name: &str) -> bool {
    matches!(name, "pi" | "e")
}

/// Inserts an explicit `*` for the implicit multiplication mathematicians write
/// but parsers reject, so `3x`, `2(x+1)`, and `(x-3)(x+2)` all parse.
///
/// Conservative by design — it inserts `*` only at boundaries that are
/// unambiguous:
/// * a digit immediately followed by a letter or `(`,
/// * `)` immediately followed by a letter, digit, or `(`.
///
/// It deliberately does *not* split `letter(` (so `sin(x)` keeps working) nor
/// `letter`-`letter` runs (so `theta` stays one variable). Consequently
/// variable–variable products must be written `x*y`, not `xy`.
fn normalize_implicit_mul(src: &str) -> String {
    let chars: Vec<char> = src.chars().collect();
    let mut out = String::with_capacity(src.len() + 8);

    for i in 0..chars.len() {
        let c = chars[i];
        out.push(c);
        let Some(&next) = chars.get(i + 1) else {
            continue;
        };
        if next.is_whitespace() {
            continue;
        }
        let insert = match c {
            // `3x`, `3(`  →  `3*x`, `3*(`
            d if d.is_ascii_digit() => next.is_ascii_alphabetic() || next == '(',
            // `)x`, `)(`, `)3`  →  `)*x`, `)*(`, `)*3`
            ')' => next.is_ascii_alphanumeric() || next == '(',
            _ => false,
        };
        if insert {
            out.push('*');
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalizer_inserts_only_unambiguous_stars() {
        assert_eq!(normalize_implicit_mul("3x"), "3*x");
        assert_eq!(normalize_implicit_mul("(x-3)(x+2)"), "(x-3)*(x+2)");
        assert_eq!(normalize_implicit_mul("2(x+1)"), "2*(x+1)");
        assert_eq!(normalize_implicit_mul("3x^2"), "3*x^2");
        // Function calls and spaced input are left intact.
        assert_eq!(normalize_implicit_mul("x + 1"), "x + 1");
    }

    #[test]
    fn variable_collector_skips_functions_and_constants() {
        let mut v = std::collections::BTreeSet::new();
        collect_variables("sin(x) + 2*pi*y - e + theta", &mut v);
        let got: Vec<String> = v.into_iter().collect();
        assert_eq!(
            got,
            vec!["theta".to_string(), "x".to_string(), "y".to_string()]
        );
    }

    #[test]
    fn accepts_equivalent_expansions() {
        assert!(verify("x^2 - x - 6", "(x-3)(x+2)").is_correct());
        assert!(verify("x^2 - x - 6", "x*x - x - 6").is_correct());
        assert!(verify("6*x", "6x").is_correct());
    }

    #[test]
    fn accepts_multivariate_equivalence() {
        assert!(verify("x^2 + 2*x*y + y^2", "(x+y)^2").is_correct());
        assert!(verify("x^2 + 2*x*y + y^2", "(x+y)*(x+y)").is_correct());
        assert!(verify("a + b + c", "c + b + a").is_correct());
        assert!(verify("x^2 - y^2", "(x-y)*(x+y)").is_correct());
    }

    #[test]
    fn rejects_multivariate_difference_with_a_witness() {
        // x*y and x+y agree only on a thin set; distinct variable values expose it.
        assert!(matches!(verify("x*y", "x+y"), Verdict::Incorrect { .. }));
        assert!(matches!(
            verify("x^2 + 2*x*y + y^2", "x^2 + y^2"),
            Verdict::Incorrect { .. }
        ));
    }

    #[test]
    fn accepts_trigonometric_identity() {
        assert!(verify("1", "sin(x)^2 + cos(x)^2").is_correct());
        assert!(verify("sin(x)^2 + cos(x)^2", "1").is_correct());
    }

    #[test]
    fn rejects_wrong_answers_with_a_witness_point() {
        assert!(matches!(
            verify("x^2 - x - 6", "x^2 + x - 6"),
            Verdict::Incorrect { .. }
        ));
    }

    #[test]
    fn surfaces_user_syntax_errors_as_parse_errors() {
        assert!(matches!(verify("x^2", "x^^2"), Verdict::ParseError { .. }));
    }

    /// Pulls the near-miss hint out of an Incorrect verdict (panics otherwise).
    fn hint_of(v: Verdict) -> Option<String> {
        match v {
            Verdict::Incorrect { hint, .. } => hint,
            other => panic!("expected Incorrect, got {other:?}"),
        }
    }

    #[test]
    fn diagnoses_a_sign_error() {
        // Forgot to negate the derivative.
        let h = hint_of(verify("u - 10*t", "10*t - u")).expect("a hint");
        assert!(h.contains("sign"), "{h}");
    }

    #[test]
    fn diagnoses_a_constant_factor() {
        // Dropped the ½ in (1/2)·a·t² style answers → off by a factor of 2.
        let h = hint_of(verify("3*x^2", "6*x^2")).expect("a hint");
        assert!(h.contains("factor of 2"), "{h}");
    }

    #[test]
    fn diagnoses_a_reciprocal() {
        let h = hint_of(verify("x", "1/x")).expect("a hint");
        assert!(h.contains("reciprocal"), "{h}");
    }

    #[test]
    fn a_plainly_wrong_answer_has_no_misleading_hint() {
        assert_eq!(hint_of(verify("x^2 - x - 6", "x^2 + x - 6")), None);
    }
}
