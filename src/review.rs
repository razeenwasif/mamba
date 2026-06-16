//! Spaced-repetition scheduling: decides which problems are *due for review*
//! from their vault solve history.
//!
//! A simple Leitner-style ladder — the more times a problem has been solved, the
//! longer until it resurfaces. A never-solved problem is always due (it's new),
//! and a problem is due once `today - last_solved` reaches its interval. The
//! logic is pure (history in, boolean out) so it's trivially testable; the
//! file I/O that reads the history lives in [`crate::storage::vault::history`].

use chrono::NaiveDate;

use crate::storage::vault::ProblemHistory;

/// Days until a problem with `solves` successful solves should resurface. A
/// roughly-doubling ladder, capped so long-mastered problems still recur.
pub fn interval_days(solves: u32) -> i64 {
    match solves {
        0 => 0,
        1 => 1,
        2 => 3,
        3 => 7,
        4 => 16,
        5 => 35,
        _ => 90,
    }
}

/// Whether a problem is due for review as of `today`. A never-solved problem is
/// due; an FSRS-scheduled one is due on/after its precomputed [`due`] date
/// (recomputed on every solve *and* lapse); a pre-FSRS note falls back to the
/// Leitner ladder over its last solve.
///
/// [`due`]: ProblemHistory::due
pub fn is_due(history: Option<&ProblemHistory>, today: NaiveDate) -> bool {
    let Some(h) = history else {
        return true; // never solved → a new card, always due
    };
    if let Some(due) = h.due {
        return today >= due;
    }
    match h.last_solved {
        None => true,
        Some(last) => (today - last).num_days() >= interval_days(h.solves),
    }
}

/// How many days past due a card is (negative if not yet due). The shared
/// scheduling signal behind the [`crate::recommend`] ranking; uses the FSRS
/// `due` date when present, else the Leitner ladder.
pub fn overdue_days(history: Option<&ProblemHistory>, today: NaiveDate) -> i64 {
    let Some(h) = history else {
        return 0;
    };
    if let Some(due) = h.due {
        return (today - due).num_days();
    }
    match h.last_solved {
        Some(last) => (today - last).num_days() - interval_days(h.solves),
        None => 0,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn day(d: u32) -> NaiveDate {
        NaiveDate::from_ymd_opt(2026, 6, d).unwrap()
    }

    fn solved(solves: u32, last: NaiveDate) -> ProblemHistory {
        ProblemHistory {
            solves,
            last_solved: Some(last),
            memory: None,
            due: None,
        }
    }

    #[test]
    fn interval_grows_then_caps() {
        assert_eq!(interval_days(1), 1);
        assert_eq!(interval_days(3), 7);
        assert_eq!(interval_days(5), 35);
        assert_eq!(interval_days(99), 90);
    }

    #[test]
    fn never_solved_is_always_due() {
        assert!(is_due(None, day(15)));
        assert!(is_due(
            Some(&ProblemHistory {
                solves: 0,
                last_solved: None,
                memory: None,
                due: None,
            }),
            day(15)
        ));
    }

    #[test]
    fn due_only_after_the_interval_elapses() {
        // Solved once on the 14th → interval 1 day.
        assert!(!is_due(Some(&solved(1, day(14))), day(14))); // same day
        assert!(is_due(Some(&solved(1, day(14))), day(15))); // +1 day → due
        // Solved three times on the 1st → interval 7 days.
        assert!(!is_due(Some(&solved(3, day(1))), day(7)));
        assert!(is_due(Some(&solved(3, day(1))), day(8)));
    }
}
