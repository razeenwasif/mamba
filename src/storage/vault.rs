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

use crate::fsrs::{self, Grade, Memory};
use crate::storage::problem::Problem;

/// Renders and writes the solved-problem note, returning the path written.
///
/// The file lands at `<vault>/<slug-of-title>.md`. The frontmatter and the
/// "Accepted Answer" always reflect the *latest* solve, but a `## History`
/// table **accumulates** one row per solve (date · attempts-to-solve · time) by
/// reading the prior note and appending to it. The vault directory is created
/// on demand.
///
/// `solved_on` is injected (rather than read from the clock here) so tests and
/// callers fully control the rendered frontmatter.
pub fn export(
    vault: &Path,
    problem: &Problem,
    answer: &str,
    attempts: u32,
    time_spent: Duration,
    solved_on: NaiveDate,
    tutor: Option<&str>,
) -> anyhow::Result<PathBuf> {
    fs::create_dir_all(vault)
        .with_context(|| format!("failed to create vault directory {}", vault.display()))?;

    let path = vault.join(format!("{}.md", slugify(&problem.title)));

    // Read the prior note (if any) for both the accumulated history and the
    // card's FSRS memory + last-review date, which drive the next interval.
    let prior = fs::read_to_string(&path).ok();
    let mut rows = prior
        .as_deref()
        .map(extract_history_rows)
        .unwrap_or_default();
    let prior_last = rows.last().and_then(|r| row_date(r));
    let prior_memory = prior.as_deref().and_then(parse_memory);

    // Grade the review from the outcome: a first-try solve is Good, a
    // multi-attempt solve is Hard (a lapse — Again — is recorded elsewhere).
    let grade = if attempts <= 1 {
        Grade::Good
    } else {
        Grade::Hard
    };
    let memory = match prior_memory {
        Some(m) => {
            let elapsed = prior_last.map_or(0, |d| (solved_on - d).num_days().max(0)) as f64;
            m.review(grade, elapsed)
        }
        None => Memory::init(grade),
    };

    rows.push(format!(
        "| {} | {} | {} |",
        solved_on.format("%Y-%m-%d"),
        attempts,
        format_duration(time_spent),
    ));

    let note = render_note(
        problem, answer, attempts, time_spent, solved_on, &rows, memory, tutor,
    );
    fs::write(&path, note).with_context(|| format!("failed to write {}", path.display()))?;
    Ok(path)
}

/// A peek at a problem's solve history, read back from its vault note for
/// display in the TUI and for scheduling.
#[derive(Debug, Clone, PartialEq)]
pub struct ProblemHistory {
    /// How many times the problem has been solved (history rows).
    pub solves: u32,
    /// The date of the most recent solve, if parseable.
    pub last_solved: Option<NaiveDate>,
    /// The card's FSRS memory state, if the note records it (notes written
    /// before FSRS have none — scheduling falls back to the Leitner ladder).
    pub memory: Option<Memory>,
}

/// Reads the solve history for `problem` from its vault note, or `None` if it
/// has never been solved (no note yet).
pub fn history(vault: &Path, problem: &Problem) -> Option<ProblemHistory> {
    let path = vault.join(format!("{}.md", slugify(&problem.title)));
    let content = fs::read_to_string(&path).ok()?;
    let rows = extract_history_rows(&content);
    if rows.is_empty() {
        return None;
    }
    let last_solved = rows.last().and_then(|row| row_date(row));
    Some(ProblemHistory {
        solves: rows.len() as u32,
        last_solved,
        memory: parse_memory(&content),
    })
}

/// The date in a history row's first cell, if parseable.
fn row_date(row: &str) -> Option<NaiveDate> {
    row.trim_start_matches("| ")
        .split('|')
        .next()
        .and_then(|cell| NaiveDate::parse_from_str(cell.trim(), "%Y-%m-%d").ok())
}

/// Reads a `key: value` scalar from a note's leading YAML frontmatter.
fn frontmatter_value<'a>(content: &'a str, key: &str) -> Option<&'a str> {
    let mut lines = content.lines();
    if lines.next()?.trim() != "---" {
        return None;
    }
    for line in lines {
        if line.trim() == "---" {
            break;
        }
        if let Some(rest) = line.strip_prefix(key).and_then(|r| r.strip_prefix(':')) {
            return Some(rest.trim());
        }
    }
    None
}

/// Reconstructs the FSRS [`Memory`] from a note's `stability`/`difficulty`
/// frontmatter, or `None` for a pre-FSRS note.
fn parse_memory(content: &str) -> Option<Memory> {
    let stability = frontmatter_value(content, "stability")?.parse().ok()?;
    // `difficulty_fsrs` is distinct from the problem's `difficulty` (Easy/…).
    let difficulty = frontmatter_value(content, "difficulty_fsrs")?
        .parse()
        .ok()?;
    Some(Memory {
        stability,
        difficulty,
    })
}

/// Extracts the data rows of a prior note's `## History` table — lines that
/// begin with `| ` whose first cell parses as a `YYYY-MM-DD` date (so the
/// header and separator rows are skipped). Order is preserved.
fn extract_history_rows(content: &str) -> Vec<String> {
    content
        .lines()
        .filter(|line| line.starts_with("| "))
        .filter(|line| {
            line.trim_start_matches("| ")
                .split('|')
                .next()
                .is_some_and(|cell| NaiveDate::parse_from_str(cell.trim(), "%Y-%m-%d").is_ok())
        })
        .map(str::to_string)
        .collect()
}

/// Renders the full note: YAML frontmatter, the problem statement, the latest
/// accepted answer, and the accumulated solve history.
fn render_note(
    problem: &Problem,
    answer: &str,
    attempts: u32,
    time_spent: Duration,
    solved_on: NaiveDate,
    history_rows: &[String],
    memory: Memory,
    tutor: Option<&str>,
) -> String {
    let mut note = format!(
        "---\n\
         title: {title}\n\
         topic: {topic}\n\
         domain: {domain}\n\
         difficulty: {difficulty}\n\
         timestamp: {timestamp}\n\
         time_spent: {time_spent}\n\
         attempts: {attempts}\n\
         solves: {solves}\n\
         stability: {stability:.4}\n\
         difficulty_fsrs: {difficulty_fsrs:.4}\n\
         due: {due}\n\
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
         ```\n\
         \n\
         ## History\n\
         \n\
         | Date | Attempts | Time |\n\
         |------|----------|------|\n\
         {history}\n",
        title = yaml_string(&problem.title),
        topic = yaml_string(&problem.topic),
        domain = problem.domain,
        difficulty = problem.difficulty,
        timestamp = solved_on.format("%Y-%m-%d"),
        time_spent = yaml_string(&format_duration(time_spent)),
        attempts = attempts,
        solves = history_rows.len(),
        stability = memory.stability,
        difficulty_fsrs = memory.difficulty,
        due = (solved_on + chrono::Duration::days(memory.interval(fsrs::DEFAULT_RETENTION)))
            .format("%Y-%m-%d"),
        raw_title = problem.title,
        prompt = problem.prompt.trim_end(),
        answer = answer.trim_end(),
        history = history_rows.join("\n"),
    );

    // Append the tutor transcript for the solve, if there was a conversation.
    if let Some(transcript) = tutor.map(str::trim).filter(|t| !t.is_empty()) {
        note.push_str(&format!("\n## Tutor\n\n{transcript}\n"));
    }
    note
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
            recap: None,
            unit: None,
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
    fn rendered_note_has_frontmatter_answer_and_history() {
        let date = NaiveDate::from_ymd_opt(2026, 6, 14).unwrap();
        let rows = vec!["| 2026-06-14 | 2 | 2m 05s |".to_string()];
        let note = render_note(
            &fixture(),
            "x^2 - x - 6",
            2,
            Duration::from_secs(125),
            date,
            &rows,
            Memory::init(Grade::Good),
            Some("**you:** hint?\n\n**tutor:** factor it"),
        );

        assert!(note.starts_with("---\n"));
        assert!(note.contains("title: \"Expand a Binomial\""));
        assert!(note.contains("domain: Symbolic"));
        assert!(note.contains("timestamp: 2026-06-14"));
        assert!(note.contains("time_spent: \"2m 05s\""));
        assert!(note.contains("attempts: 2"));
        assert!(note.contains("solves: 1"));
        assert!(note.contains("stability: "));
        assert!(note.contains("difficulty_fsrs: "));
        assert!(note.contains("due: "));
        assert!(note.contains("```text\nx^2 - x - 6\n```"));
        assert!(note.contains("## History"));
        assert!(note.contains("| 2026-06-14 | 2 | 2m 05s |"));
        // The tutor transcript is appended when present.
        assert!(note.contains("## Tutor"));
        assert!(note.contains("**tutor:** factor it"));
    }

    #[test]
    fn extract_history_rows_keeps_only_dated_rows() {
        let note = render_note(
            &fixture(),
            "x",
            1,
            Duration::from_secs(30),
            NaiveDate::from_ymd_opt(2026, 6, 14).unwrap(),
            &["| 2026-06-14 | 1 | 30s |".to_string()],
            Memory::init(Grade::Good),
            None,
        );
        // Round-trips: the header and separator are dropped, the data row kept.
        assert_eq!(
            extract_history_rows(&note),
            vec!["| 2026-06-14 | 1 | 30s |"]
        );
    }

    #[test]
    fn export_accumulates_history_across_solves() {
        let dir = std::env::temp_dir().join(format!("mamba-vault-hist-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        let day = |d: u32| NaiveDate::from_ymd_opt(2026, 6, d).unwrap();

        export(
            &dir,
            &fixture(),
            "x^2 - x - 6",
            3,
            Duration::from_secs(90),
            day(14),
            None,
        )
        .unwrap();
        let path = export(
            &dir,
            &fixture(),
            "x*x - x - 6",
            1,
            Duration::from_secs(40),
            day(15),
            None,
        )
        .unwrap();

        let note = fs::read_to_string(&path).unwrap();
        assert!(note.contains("solves: 2"));
        assert!(note.contains("attempts: 1")); // latest solve
        assert!(note.contains("| 2026-06-14 | 3 |"));
        assert!(note.contains("| 2026-06-15 | 1 |"));
        // Latest accepted answer is the second one.
        assert!(note.contains("```text\nx*x - x - 6\n```"));

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn history_reads_solve_count_and_last_date() {
        let dir = std::env::temp_dir().join(format!("mamba-vault-hist2-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        let day = |d: u32| NaiveDate::from_ymd_opt(2026, 6, d).unwrap();

        assert_eq!(history(&dir, &fixture()), None); // never solved → no note
        export(
            &dir,
            &fixture(),
            "x",
            2,
            Duration::from_secs(60),
            day(14),
            None,
        )
        .unwrap();
        export(
            &dir,
            &fixture(),
            "x",
            1,
            Duration::from_secs(30),
            day(16),
            None,
        )
        .unwrap();

        let h = history(&dir, &fixture()).unwrap();
        assert_eq!(h.solves, 2);
        assert_eq!(h.last_solved, Some(day(16)));

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn fsrs_state_persists_and_strengthens_across_solves() {
        let dir = std::env::temp_dir().join(format!("mamba-vault-fsrs-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        let day = |d: u32| NaiveDate::from_ymd_opt(2026, 6, d).unwrap();
        let solve = |d: u32| {
            export(
                &dir,
                &fixture(),
                "x^2 - x - 6",
                1,
                Duration::from_secs(30),
                day(d),
                None,
            )
            .unwrap()
        };

        solve(1);
        let first = history(&dir, &fixture()).unwrap().memory.expect("memory");
        // A second on-time solve must read the prior state and grow stability.
        solve(1 + first.interval(crate::fsrs::DEFAULT_RETENTION) as u32);
        let second = history(&dir, &fixture()).unwrap().memory.expect("memory");
        assert!(second.stability > first.stability);

        let _ = fs::remove_dir_all(&dir);
    }
}
