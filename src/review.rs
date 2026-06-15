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

/// Whether a problem is due for review as of `today`, given its solve history.
/// A problem with no history (never solved) is due; otherwise it is due once
/// its interval has elapsed since the last solve.
pub fn is_due(history: Option<&ProblemHistory>, today: NaiveDate) -> bool {
    match history.and_then(|h| h.last_solved.map(|d| (h.solves, d))) {
        None => true, // never solved → a new card, always due
        Some((solves, last)) => (today - last).num_days() >= interval_days(solves),
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
                last_solved: None
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
