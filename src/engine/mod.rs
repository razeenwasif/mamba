//! # The Universal Routing Engine
//!
//! Mamba's defining idea: a single entry point, [`verify`], inspects a
//! problem's [`Domain`] and dispatches the user's raw answer string to exactly
//! one of three specialized verification pipelines, each backed by a crate
//! chosen for that kind of mathematics:
//!
//! | Domain               | Pipeline           | Crate         | Verifies by                    |
//! |----------------------|--------------------|---------------|--------------------------------|
//! | [`Domain::Symbolic`] | [`symbolic`]       | `meval`       | algebraic (numeric-probe) equiv|
//! | [`Domain::Tensor`]   | [`tensor`]         | `ndarray`     | element-wise matrix comparison |
//! | [`Domain::Exact`]    | [`exact`]          | `num-bigint`  | exact big-integer comparison   |
//!
//! Each pipeline exposes a `verify(expected, input, ...) -> Verdict` function
//! and owns its own parsing, normalization, and comparison. The router below
//! is therefore tiny and total: one `match` arm per domain. Adding a domain is
//! a new [`Domain`] variant, a new module, and one arm here — nothing else in
//! the app changes.

pub mod exact;
pub mod symbolic;
pub mod tensor;
pub mod units;

use crate::storage::problem::{Domain, Problem};

/// The outcome of verifying a user's answer. The UI ([`crate::ui`]) renders
/// each variant differently in the verification console, and a [`Verdict::Correct`]
/// triggers the vault export in [`crate::app`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Verdict {
    /// The answer is mathematically equivalent to the expected one.
    Correct,
    /// The answer parsed cleanly but is wrong; `detail` explains how (e.g.
    /// "shapes differ: got 2×3, expected 3×2"). `hint` is an optional *near-miss
    /// diagnosis* — a recognized relationship to the right answer (off by a sign,
    /// a factor, the transpose, …) that turns a bare ✗ into a lesson.
    Incorrect {
        detail: String,
        hint: Option<String>,
    },
    /// The answer could not be parsed in this domain's syntax. `message` is
    /// the parser error; `hint` is an optional nudge toward the right syntax.
    ParseError {
        message: String,
        hint: Option<String>,
    },
}

impl Verdict {
    /// Convenience for the common "couldn't parse, here's a hint" case.
    pub(crate) fn parse_error(message: impl Into<String>, hint: Option<String>) -> Self {
        Verdict::ParseError {
            message: message.into(),
            hint,
        }
    }

    /// Convenience for a wrong-but-well-formed answer (no near-miss diagnosis).
    pub(crate) fn incorrect(detail: impl Into<String>) -> Self {
        Verdict::Incorrect {
            detail: detail.into(),
            hint: None,
        }
    }

    /// A wrong answer with an *optional* near-miss diagnosis — the shape a
    /// pipeline produces after running its diagnostics (which may find nothing).
    pub(crate) fn incorrect_maybe(detail: impl Into<String>, hint: Option<String>) -> Self {
        Verdict::Incorrect {
            detail: detail.into(),
            hint,
        }
    }

    /// `true` only for [`Verdict::Correct`].
    pub fn is_correct(&self) -> bool {
        matches!(self, Verdict::Correct)
    }
}

/// **The Universal Router.** Routes `input` to the pipeline that matches
/// `problem.domain` and returns its [`Verdict`].
///
/// This is the entire dispatch surface of the engine: a total `match` over the
/// three domains. Each arm hands the pipeline only what it needs — the
/// canonical `expected` answer, the user's `input`, and (for the exact
/// pipeline) how to interpret them.
pub fn verify(problem: &Problem, input: &str) -> Verdict {
    let input = input.trim();
    if input.is_empty() {
        return Verdict::parse_error(
            "empty answer",
            Some("type an answer, then press Enter".into()),
        );
    }

    // Optional dimensional gate: if the problem declares a unit and the user
    // appended one, it must be dimensionally consistent; then we verify the
    // numeric part (the unit stripped). A missing or unknown unit is lenient.
    let input = match &problem.unit {
        Some(unit) => match units::check(unit, input) {
            Ok(value) => value,
            Err(detail) => return Verdict::incorrect(detail),
        },
        None => input,
    };

    match problem.domain {
        // Algebra / calculus → symbolic equivalence.
        Domain::Symbolic => symbolic::verify(&problem.expected, input),

        // Linear algebra → tensor (matrix) comparison.
        Domain::Tensor => tensor::verify(&problem.expected, input),

        // Number theory → exact arbitrary-precision arithmetic. The
        // `answer_kind` selects plain-integer vs. prime-factorization checking.
        Domain::Exact => exact::verify(&problem.expected, input, problem.answer_kind),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::storage::problem::{AnswerKind, Difficulty, Domain};

    fn problem(domain: Domain, expected: &str, answer_kind: AnswerKind) -> Problem {
        Problem {
            id: "t".into(),
            title: "T".into(),
            domain,
            topic: "X".into(),
            difficulty: Difficulty::Easy,
            prompt: "p".into(),
            expected: expected.into(),
            answer_kind,
            hint: None,
            recap: None,
            unit: None,
        }
    }

    #[test]
    fn router_dispatches_each_domain_to_the_right_pipeline() {
        // Symbolic: algebraically-equal but textually different.
        let sym = problem(Domain::Symbolic, "x^2 - x - 6", AnswerKind::Scalar);
        assert!(verify(&sym, "(x - 3)*(x + 2)").is_correct());

        // Tensor: same matrix, different whitespace.
        let ten = problem(Domain::Tensor, "1 4; 2 5; 3 6", AnswerKind::Scalar);
        assert!(verify(&ten, "1 4;2 5;3 6").is_correct());

        // Exact: a 10 000th-prime factorization (the number is itself prime).
        let exa = problem(Domain::Exact, "104729", AnswerKind::Factorization);
        assert!(verify(&exa, "104729").is_correct());
    }

    #[test]
    fn empty_input_is_a_parse_error_before_routing() {
        let sym = problem(Domain::Symbolic, "x", AnswerKind::Scalar);
        assert!(matches!(verify(&sym, "   "), Verdict::ParseError { .. }));
    }

    #[test]
    fn unit_gate_accepts_consistent_and_rejects_inconsistent_units() {
        let mut p = problem(Domain::Exact, "30", AnswerKind::Scalar);
        p.unit = Some("N".into());
        assert!(verify(&p, "30").is_correct()); // no unit → lenient
        assert!(verify(&p, "30 N").is_correct()); // the declared unit
        assert!(verify(&p, "30 kg*m/s^2").is_correct()); // dimensionally equal
        match verify(&p, "30 kg") {
            Verdict::Incorrect { detail, .. } => assert!(detail.contains("wrong units")),
            other => panic!("expected a unit mismatch, got {other:?}"),
        }
        // The right units with the wrong number is still wrong.
        assert!(matches!(verify(&p, "31 N"), Verdict::Incorrect { .. }));
    }
}
