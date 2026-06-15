//! The problem bank: declarative JSON problem definitions loaded from disk.
//!
//! Each problem is one `.json` file in the `problems/` directory. Keeping
//! problems as *data* (rather than code) means adding a problem — or a whole
//! new topic — never requires recompiling the binary, exactly as in `~/Kata`.

use std::fs;
use std::path::Path;

use anyhow::Context;
use serde::Deserialize;

/// The three mathematical domains Mamba can serve and verify. This is the
/// **routing key**: [`crate::engine::verify`] matches on it to pick a
/// pipeline, so adding a domain is a single new arm there plus a module.
///
/// Serialized as a bare string in JSON (`"domain": "Symbolic"`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Deserialize)]
pub enum Domain {
    /// Polynomial algebra & calculus. Answers are *expressions*, verified for
    /// algebraic equivalence by the symbolic pipeline (numeric probing).
    Symbolic,
    /// Linear algebra. Answers are scalars or matrices, verified element-wise
    /// by the tensor pipeline (`ndarray`).
    Tensor,
    /// Number theory & exact arithmetic. Answers are arbitrary-precision
    /// integers (or factorizations), verified exactly by the exact pipeline
    /// (`num-bigint`).
    Exact,
}

impl std::fmt::Display for Domain {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Domain::Symbolic => "Symbolic",
            Domain::Tensor => "Tensor",
            Domain::Exact => "Exact",
        })
    }
}

/// How a domain's pipeline should *interpret* the answer strings. Defaults to
/// [`AnswerKind::Scalar`]; only the exact pipeline currently varies on it
/// (plain integer vs. prime factorization), but the field is domain-agnostic
/// so future pipelines can reuse it.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Deserialize)]
pub enum AnswerKind {
    /// A single value: an expression, a matrix/scalar, or one big integer.
    #[default]
    Scalar,
    /// A prime factorization, e.g. `2^3 * 3^2 * 5`. The exact pipeline checks
    /// that the product equals the target *and* that every base is prime.
    Factorization,
}

/// Problem difficulty. Variant order defines sort order (`Easy < Medium <
/// Hard`), which the navigator relies on for grouping within a domain.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Deserialize)]
pub enum Difficulty {
    Easy,
    Medium,
    Hard,
}

impl std::fmt::Display for Difficulty {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Difficulty::Easy => "Easy",
            Difficulty::Medium => "Medium",
            Difficulty::Hard => "Hard",
        })
    }
}

/// A complete problem as loaded from a `problems/*.json` file.
#[derive(Debug, Clone, Deserialize)]
pub struct Problem {
    /// Stable identifier (kebab-case), unique across the bank.
    pub id: String,
    /// Human-readable title, e.g. `"Expand a Binomial"`.
    pub title: String,
    /// The domain — the routing key for [`crate::engine::verify`].
    pub domain: Domain,
    /// Topic shown as the second-level navigator header, e.g. `"Calculus"`.
    pub topic: String,
    /// Difficulty bucket used for sorting and color-coding.
    pub difficulty: Difficulty,
    /// Full prompt: statement and any worked context. May contain Unicode
    /// formula approximations (superscripts, matrix brackets) rendered as-is.
    pub prompt: String,
    /// The canonical correct answer, written in the domain's own input syntax
    /// (an expression, a `;`-delimited matrix, or an integer). The user's
    /// input is checked *against* this by the routed pipeline.
    pub expected: String,
    /// How the pipeline should interpret [`expected`](Problem::expected) and
    /// the user's input. Optional in JSON; defaults to [`AnswerKind::Scalar`].
    #[serde(default)]
    pub answer_kind: AnswerKind,
    /// Optional one-line syntax nudge surfaced in the verification console,
    /// e.g. `"rows separated by ';', e.g. 1 2; 3 4"`.
    #[serde(default)]
    pub hint: Option<String>,
    /// Optional preliminary reading — a short recap of the subject matter shown
    /// in the description pane (toggle with `g`). May use ASCII formula markers.
    #[serde(default)]
    pub recap: Option<String>,
}

/// Loads every `*.json` problem in `dir`, sorted by (domain, difficulty,
/// topic, title) so the navigator can group by domain then topic in a single
/// pass.
///
/// Fails fast with the offending file name if any definition is malformed — a
/// silently skipped problem is harder to debug than a startup error.
pub fn load_all(dir: &Path) -> anyhow::Result<Vec<Problem>> {
    let entries = fs::read_dir(dir)
        .with_context(|| format!("problem directory not found: {}", dir.display()))?;

    let mut problems = Vec::new();
    for entry in entries {
        let path = entry?.path();
        if path.extension().is_none_or(|ext| ext != "json") {
            continue;
        }
        let raw = fs::read_to_string(&path)
            .with_context(|| format!("failed to read {}", path.display()))?;
        let problem: Problem = serde_json::from_str(&raw)
            .with_context(|| format!("invalid problem definition: {}", path.display()))?;
        problems.push(problem);
    }

    problems.sort_by(|a, b| {
        (a.domain, &a.topic, a.difficulty, &a.title).cmp(&(
            b.domain,
            &b.topic,
            b.difficulty,
            &b.title,
        ))
    });
    Ok(problems)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn difficulty_and_domain_order_as_declared() {
        assert!(Difficulty::Easy < Difficulty::Hard);
        assert!(Domain::Symbolic < Domain::Exact);
    }

    #[test]
    fn answer_kind_defaults_to_scalar_when_absent() {
        let json = r#"{
            "id":"t","title":"T","domain":"Symbolic","topic":"Algebra",
            "difficulty":"Easy","prompt":"p","expected":"x"
        }"#;
        let p: Problem = serde_json::from_str(json).unwrap();
        assert_eq!(p.answer_kind, AnswerKind::Scalar);
    }

    #[test]
    fn load_all_sorts_by_domain_then_difficulty_and_rejects_malformed() {
        let dir = tempdir();
        let good = |id: &str, domain: &str, difficulty: &str| {
            format!(
                r#"{{"id":"{id}","title":"{id}","domain":"{domain}","topic":"X",
                    "difficulty":"{difficulty}","prompt":"p","expected":"1"}}"#
            )
        };
        fs::write(dir.join("b.json"), good("b", "Exact", "Easy")).unwrap();
        fs::write(dir.join("a.json"), good("a", "Symbolic", "Hard")).unwrap();
        fs::write(dir.join("notes.txt"), "ignored").unwrap();

        let problems = load_all(&dir).unwrap();
        assert_eq!(problems.len(), 2);
        // Symbolic sorts before Exact regardless of difficulty.
        assert_eq!(problems[0].id, "a");

        fs::write(dir.join("bad.json"), "{ not json").unwrap();
        let err = load_all(&dir).unwrap_err();
        assert!(format!("{err:#}").contains("bad.json"));
    }

    /// A throwaway unique temp dir without pulling in the `tempfile` crate.
    fn tempdir() -> std::path::PathBuf {
        let mut p = std::env::temp_dir();
        p.push(format!("mamba-test-{}", std::process::id()));
        let _ = fs::remove_dir_all(&p);
        fs::create_dir_all(&p).unwrap();
        p
    }
}
