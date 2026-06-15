//! "What should I do next?" — a single adaptive recommendation.
//!
//! Where [`crate::review`] answers *which* problems are due, this module answers
//! the sharper question of *which one to do right now*. It blends three signals,
//! all read from the vault's solve history:
//!
//! 1. **Spaced repetition** — anything due is preferred, most-overdue first, so
//!    learned material doesn't decay while new cards pile up.
//! 2. **Topic mastery** — ties break toward the weakest topic (fewest
//!    `solved / total`), steering practice at what you know least.
//! 3. **Difficulty** — within a topic, the easier card comes first to build
//!    momentum before the hard ones.
//!
//! When nothing is due, it still suggests one — *getting ahead* in the weakest
//! topic — so `mamba next` (and the in-TUI `n` key) always has an answer. The
//! ranking is pure (problems + history in, one index out) so it's testable
//! without a terminal.

use std::collections::HashMap;
use std::path::Path;

use chrono::NaiveDate;

use crate::review;
use crate::storage::problem::Problem;
use crate::storage::vault::{self, ProblemHistory};

/// A recommended next problem: an index into the problem slice plus a short,
/// human-readable rationale (shown by `mamba next` and the in-TUI flash).
#[derive(Debug, Clone)]
pub struct Pick {
    /// Index into the `problems` slice passed to [`next`].
    pub index: usize,
    /// One line explaining *why* this problem was chosen.
    pub reason: String,
}

/// Per-topic mastery as `(solved, total)`. A problem counts as solved once it
/// has at least one recorded solve in the vault.
fn topic_mastery(
    problems: &[Problem],
    histories: &[Option<ProblemHistory>],
) -> HashMap<String, (usize, usize)> {
    let mut mastery: HashMap<String, (usize, usize)> = HashMap::new();
    for (p, h) in problems.iter().zip(histories) {
        let entry = mastery.entry(p.topic.clone()).or_insert((0, 0));
        entry.1 += 1;
        if h.as_ref().is_some_and(|h| h.solves >= 1) {
            entry.0 += 1;
        }
    }
    mastery
}

/// How many days a card is overdue as of `today`: positive when past its
/// scheduled review, zero on the due date, negative when not yet due. A
/// never-solved card is treated as "due today" (`0`), so genuinely overdue
/// reviews outrank brand-new cards.
fn overdue_days(history: Option<&ProblemHistory>, today: NaiveDate) -> i64 {
    match history.and_then(|h| h.last_solved.map(|d| (h.solves, d))) {
        None => 0,
        Some((solves, last)) => (today - last).num_days() - review::interval_days(solves),
    }
}

/// Recommends the single best problem to attempt next, or `None` if the bank is
/// empty. See the module docs for the ranking.
pub fn next(problems: &[Problem], vault: &Path, today: NaiveDate) -> Option<Pick> {
    if problems.is_empty() {
        return None;
    }

    let histories: Vec<Option<ProblemHistory>> =
        problems.iter().map(|p| vault::history(vault, p)).collect();
    let mastery = topic_mastery(problems, &histories);

    // Topic weakness as parts-per-thousand mastered (lower = weaker = higher
    // priority); kept integral so it can sit in an `Ord` sort key.
    let weakness = |topic: &str| -> i64 {
        match mastery.get(topic) {
            Some(&(solved, total)) if total > 0 => (solved as i64 * 1000) / total as i64,
            _ => 1000,
        }
    };

    let due: Vec<usize> = (0..problems.len())
        .filter(|&i| review::is_due(histories[i].as_ref(), today))
        .collect();

    // Recommend from the due set; if nothing's due, get ahead across the whole
    // bank (weakest topic first, ignoring overdue-ness).
    let ahead = due.is_empty();
    let pool: Vec<usize> = if ahead {
        (0..problems.len()).collect()
    } else {
        due
    };

    // Sort key (ascending → best first):
    //   1. most overdue first (reviews before new cards) — skipped when ahead
    //   2. weakest topic first
    //   3. easiest difficulty first
    //   4. unsolved before solved (matters mainly in "ahead" mode)
    //   5. title, for a stable, deterministic tiebreak
    let key = |i: usize| {
        let h = histories[i].as_ref();
        let solved = h.is_some_and(|h| h.solves >= 1);
        let overdue = if ahead { 0 } else { overdue_days(h, today) };
        (
            -overdue,
            weakness(&problems[i].topic),
            problems[i].difficulty,
            solved as u8,
            problems[i].title.clone(),
        )
    };
    let index = pool.into_iter().min_by(|&a, &b| key(a).cmp(&key(b)))?;

    let problem = &problems[index];
    let (solved, total) = mastery.get(&problem.topic).copied().unwrap_or((0, 1));
    let history = histories[index].as_ref();
    let reason = if ahead {
        format!(
            "ahead of schedule — practice ‘{}’, your weakest topic ({solved}/{total} mastered)",
            problem.topic
        )
    } else if history.and_then(|h| h.last_solved).is_none() {
        format!(
            "new card in ‘{}’ ({solved}/{total} mastered)",
            problem.topic
        )
    } else {
        let h = history.unwrap();
        let last = h.last_solved.unwrap();
        let od = overdue_days(history, today);
        let when = if od <= 0 {
            "due today".to_string()
        } else {
            format!("{od}d overdue")
        };
        let plural = if h.solves == 1 { "" } else { "s" };
        format!(
            "review — {when} (last solved {last}, {} solve{plural})",
            h.solves
        )
    };

    Some(Pick { index, reason })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::storage::problem::{AnswerKind, Difficulty, Domain};

    fn problem(id: &str, topic: &str, difficulty: Difficulty) -> Problem {
        Problem {
            id: id.into(),
            title: id.into(),
            domain: Domain::Symbolic,
            topic: topic.into(),
            difficulty,
            prompt: "p".into(),
            expected: "x".into(),
            answer_kind: AnswerKind::Scalar,
            hint: None,
            recap: None,
        }
    }

    fn day(d: u32) -> NaiveDate {
        NaiveDate::from_ymd_opt(2026, 6, d).unwrap()
    }

    /// A scratch vault unique to this test (the `tag` keeps parallel tests from
    /// stomping each other's directory).
    fn scratch(tag: &str) -> std::path::PathBuf {
        let v = std::env::temp_dir().join(format!("mamba-recommend-{}-{tag}", std::process::id()));
        let _ = std::fs::remove_dir_all(&v);
        v
    }

    #[test]
    fn empty_bank_has_no_recommendation() {
        assert!(next(&[], &scratch("empty"), day(15)).is_none());
    }

    #[test]
    fn an_overdue_review_beats_a_new_card() {
        let vault = scratch("overdue");
        let reviewed = problem("reviewed", "Algebra", Difficulty::Easy);
        let fresh = problem("fresh", "Algebra", Difficulty::Easy);
        // Solved once on the 1st → interval 1 day, so by the 15th it's 13d overdue.
        vault::export(
            &vault,
            &reviewed,
            "x",
            1,
            std::time::Duration::from_secs(1),
            day(1),
            None,
        )
        .unwrap();

        let pick = next(&[reviewed, fresh], &vault, day(15)).expect("a pick");
        assert_eq!(
            pick.index, 0,
            "the overdue review should win over the new card"
        );
        assert!(pick.reason.contains("overdue"), "{}", pick.reason);
        let _ = std::fs::remove_dir_all(&vault);
    }

    #[test]
    fn among_new_cards_the_weakest_topic_wins() {
        let vault = scratch("weakest");
        // Algebra: one of two solved (50% mastered). Calculus: zero of one (0%).
        let alg_done = problem("alg-done", "Algebra", Difficulty::Easy);
        let alg_new = problem("alg-new", "Algebra", Difficulty::Easy);
        let calc_new = problem("calc-new", "Calculus", Difficulty::Easy);
        vault::export(
            &vault,
            &alg_done,
            "x",
            1,
            std::time::Duration::from_secs(1),
            day(15),
            None,
        )
        .unwrap();

        // alg-done was solved today → not due; the two new cards are both due.
        let pick = next(&[alg_done, alg_new, calc_new], &vault, day(15)).expect("a pick");
        assert_eq!(
            pick.index, 2,
            "Calculus (0% mastered) is weaker than Algebra (50%)"
        );
        assert!(pick.reason.contains("new card"), "{}", pick.reason);
        let _ = std::fs::remove_dir_all(&vault);
    }

    #[test]
    fn nothing_due_suggests_getting_ahead() {
        let vault = scratch("ahead");
        let p = problem("solved", "Algebra", Difficulty::Easy);
        // Solved today → not due (interval 1 day), so the only suggestion is to
        // get ahead.
        vault::export(
            &vault,
            &p,
            "x",
            1,
            std::time::Duration::from_secs(1),
            day(15),
            None,
        )
        .unwrap();

        let pick = next(std::slice::from_ref(&p), &vault, day(15)).expect("a pick");
        assert_eq!(pick.index, 0);
        assert!(pick.reason.contains("ahead of schedule"), "{}", pick.reason);
        let _ = std::fs::remove_dir_all(&vault);
    }
}
