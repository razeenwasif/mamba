//! The vault exporter: turns a correctly-solved problem into a structured
//! Markdown note with YAML frontmatter, written into the user's OnyxVault.
//!
//! The output is deliberately plain Markdown + YAML so it renders natively in
//! Onyx, Obsidian, or any other local-first knowledge tool — matching the
//! tracking style of `~/Kata`.

use std::fs;
use std::path::{Path, PathBuf};
use std::time::Duration;

use anyhow::Context;
use chrono::NaiveDate;

use crate::storage::problem::Problem;

/// Renders and writes the solved-problem note, returning the path written.
///
/// The file lands at `<vault>/<slug-of-title>.md` and is overwritten on
/// re-solve, so the vault always holds the *latest* accepted answer. The vault
/// directory is created on demand.
///
/// `solved_on` is injected (rather than read from the clock here) so tests and
/// callers fully control the rendered frontmatter.
pub fn export(
    vault: &Path,
    problem: &Problem,
    answer: &str,
    time_spent: Duration,
    solved_on: NaiveDate,
) -> anyhow::Result<PathBuf> {
    fs::create_dir_all(vault)
        .with_context(|| format!("failed to create vault directory {}", vault.display()))?;

    let path = vault.join(format!("{}.md", slugify(&problem.title)));
    let note = render_note(problem, answer, time_spent, solved_on);
    fs::write(&path, note).with_context(|| format!("failed to write {}", path.display()))?;
    Ok(path)
}

/// Renders the full note: YAML frontmatter, the problem statement, and the
/// accepted answer in a fenced block.
fn render_note(
    problem: &Problem,
    answer: &str,
    time_spent: Duration,
    solved_on: NaiveDate,
) -> String {
    format!(
        "---\n\
         title: {title}\n\
         topic: {topic}\n\
         domain: {domain}\n\
         difficulty: {difficulty}\n\
         timestamp: {timestamp}\n\
         time_spent: {time_spent}\n\
         tags: [mamba, math]\n\
         ---\n\
         \n\
         # {raw_title}\n\
         \n\
         ## Problem\n\
         \n\
         {prompt}\n\
         \n\
         ## Accepted Answer\n\
         \n\
         ```text\n\
         {answer}\n\
         ```\n",
        title = yaml_string(&problem.title),
        topic = yaml_string(&problem.topic),
        domain = problem.domain,
        difficulty = problem.difficulty,
        timestamp = solved_on.format("%Y-%m-%d"),
        time_spent = yaml_string(&format_duration(time_spent)),
        raw_title = problem.title,
        prompt = problem.prompt.trim_end(),
        answer = answer.trim_end(),
    )
}

/// Quotes and escapes a string for safe inclusion as a YAML scalar value.
/// Always double-quotes — simpler and safer than deciding when quoting is
/// optional.
fn yaml_string(s: &str) -> String {
    format!("\"{}\"", s.replace('\\', "\\\\").replace('"', "\\\""))
}

/// Lowercases the title and collapses every non-alphanumeric run into a single
/// `-`, yielding a filesystem- and wiki-link-safe file name.
fn slugify(s: &str) -> String {
    let mut slug = String::with_capacity(s.len());
    let mut last_was_dash = true; // suppress a leading dash
    for c in s.chars() {
        if c.is_ascii_alphanumeric() {
            slug.push(c.to_ascii_lowercase());
            last_was_dash = false;
        } else if !last_was_dash {
            slug.push('-');
            last_was_dash = true;
        }
    }
    if slug.ends_with('-') {
        slug.pop();
    }
    slug
}

/// Formats a duration as a compact human-readable string: `"45s"`,
/// `"12m 03s"`, or `"1h 07m"`.
fn format_duration(d: Duration) -> String {
    let total = d.as_secs();
    let (h, m, s) = (total / 3600, (total % 3600) / 60, total % 60);
    match (h, m) {
        (0, 0) => format!("{s}s"),
        (0, _) => format!("{m}m {s:02}s"),
        _ => format!("{h}h {m:02}m"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::storage::problem::{Difficulty, Domain};

    fn fixture() -> Problem {
        Problem {
            id: "expand-a-binomial".into(),
            title: "Expand a Binomial".into(),
            domain: Domain::Symbolic,
            topic: "Algebra".into(),
            difficulty: Difficulty::Easy,
            prompt: "Expand (x - 3)(x + 2).".into(),
            expected: "x^2 - x - 6".into(),
            answer_kind: Default::default(),
            hint: None,
        }
    }

    #[test]
    fn slugify_collapses_punctuation_and_case() {
        assert_eq!(slugify("Expand a Binomial"), "expand-a-binomial");
        assert_eq!(slugify("  Determinant (2×2)!  "), "determinant-2-2");
    }

    #[test]
    fn format_duration_picks_the_right_granularity() {
        assert_eq!(format_duration(Duration::from_secs(45)), "45s");
        assert_eq!(format_duration(Duration::from_secs(12 * 60 + 3)), "12m 03s");
        assert_eq!(
            format_duration(Duration::from_secs(3600 + 7 * 60)),
            "1h 07m"
        );
    }

    #[test]
    fn rendered_note_has_frontmatter_and_fenced_answer() {
        let date = NaiveDate::from_ymd_opt(2026, 6, 14).unwrap();
        let note = render_note(&fixture(), "x^2 - x - 6", Duration::from_secs(125), date);

        assert!(note.starts_with("---\n"));
        assert!(note.contains("title: \"Expand a Binomial\""));
        assert!(note.contains("topic: \"Algebra\""));
        assert!(note.contains("domain: Symbolic"));
        assert!(note.contains("difficulty: Easy"));
        assert!(note.contains("timestamp: 2026-06-14"));
        assert!(note.contains("time_spent: \"2m 05s\""));
        assert!(note.contains("```text\nx^2 - x - 6\n```"));
    }
}
