//! # Symbolic pipeline (algebra & calculus)
//!
//! Verifies that a user's expression is **algebraically equivalent** to the
//! expected one — `(x-3)(x+2)` ≡ `x^2 - x - 6`, `6*x` ≡ `6x` — rather than
//! merely textually equal.
//!
//! ## Why numeric probing instead of a full CAS
//!
//! Two single-variable expressions are equal as functions iff they agree
//! everywhere. We can't test "everywhere", but the [Schwartz–Zippel lemma]
//! tells us that two *distinct* rational expressions of bounded degree agree
//! on only finitely many points, so evaluating both at a handful of
//! well-separated probe points and finding them equal every time is
//! overwhelming evidence of equivalence — false positives are astronomically
//! unlikely for the low-degree polynomials a practice tool poses.
//!
//! This keeps the pipeline dependency-light (`meval` parses + evaluates) and
//! license-clean. The [`EquivalenceChecker`]-shaped seam means a true CAS
//! (e.g. `symbolica`) can be dropped in later for exact, multivariate, or
//! trigonometric-identity verification without touching the router.
//!
//! [Schwartz–Zippel lemma]: https://en.wikipedia.org/wiki/Schwartz%E2%80%93Zippel_lemma

use meval::Expr;

use crate::engine::Verdict;

/// Deterministic probe points: well-separated, none at a small-integer root,
/// so a wrong answer is extremely unlikely to coincide at every point.
/// Deterministic (not random) so verification is perfectly reproducible.
const PROBE_POINTS: [f64; 8] = [-2.3, -1.1, -0.37, 0.71, 1.29, 2.93, 4.13, 5.61];

/// Absolute + relative tolerance for the floating-point agreement test.
const EPSILON: f64 = 1e-7;

/// Verifies `input` against `expected` by algebraic (numeric-probe)
/// equivalence over the single variable `x`.
pub fn verify(expected: &str, input: &str) -> Verdict {
    // The expected answer is authored by us; a parse failure here is a bank
    // bug, surfaced plainly rather than blamed on the user.
    let expected_fn = match compile(expected) {
        Ok(f) => f,
        Err(e) => {
            return Verdict::parse_error(
                format!("internal: expected answer failed to parse ({e})"),
                None,
            );
        }
    };

    // The user's answer: a parse failure is *their* syntax error — hint them.
    let input_fn = match compile(input) {
        Ok(f) => f,
        Err(e) => {
            return Verdict::parse_error(
                format!("couldn't parse expression: {e}"),
                Some("use explicit operators where unambiguous, e.g. 2*x, x^2, (x-1)*(x+1)".into()),
            );
        }
    };

    let mut compared = 0usize;
    for &x in &PROBE_POINTS {
        let (a, b) = (expected_fn(x), input_fn(x));
        // Skip points where either side is undefined (e.g. a pole); a valid
        // expression still agrees on the finite points.
        if !a.is_finite() || !b.is_finite() {
            continue;
        }
        compared += 1;
        if (a - b).abs() > EPSILON * (1.0 + a.abs()) {
            return Verdict::incorrect(format!(
                "not equivalent: at x = {x} your answer gives {b:.4}, expected {a:.4}"
            ));
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

/// Normalizes implicit multiplication, parses with `meval`, and binds the
/// single variable `x`, returning an evaluator closure.
fn compile(src: &str) -> Result<impl Fn(f64) -> f64, String> {
    let normalized = normalize_implicit_mul(src);
    let expr: Expr = normalized
        .parse()
        .map_err(|e| format!("{e}"))?;
    expr.bind("x").map_err(|e| format!("{e}"))
}

/// Inserts an explicit `*` for the implicit multiplication mathematicians
/// write but parsers reject, so `3x`, `2(x+1)`, and `(x-3)(x+2)` all parse.
///
/// Conservative by design — it inserts `*` only at boundaries that are
/// unambiguous for the polynomial domain:
/// * a digit or `)` immediately followed by a letter, `(`, or digit-after-`)`,
/// * `)` immediately followed by `(`.
///
/// It deliberately does *not* split `letter(` (so `sin(x)` keeps working) or
/// `letter`-`digit` runs, which a future multivariate upgrade can refine.
fn normalize_implicit_mul(src: &str) -> String {
    let chars: Vec<char> = src.chars().collect();
    let mut out = String::with_capacity(src.len() + 8);

    for i in 0..chars.len() {
        let c = chars[i];
        out.push(c);
        let Some(&next) = chars.get(i + 1) else {
            continue;
        };
        // Skip whitespace boundaries — meval handles spaces, and inserting a
        // `*` next to one would be redundant.
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
    fn accepts_equivalent_expansions() {
        assert!(verify("x^2 - x - 6", "(x-3)(x+2)").is_correct());
        assert!(verify("x^2 - x - 6", "x*x - x - 6").is_correct());
        assert!(verify("6*x", "6x").is_correct());
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
        assert!(matches!(
            verify("x^2", "x^^2"),
            Verdict::ParseError { .. }
        ));
    }
}
