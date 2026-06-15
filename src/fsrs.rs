//! FSRS — the Free Spaced Repetition Scheduler (v4.5).
//!
//! A modern replacement for the fixed Leitner ladder in [`crate::review`]. Each
//! card carries two latent variables:
//!
//! * **stability** `S` — days until recall probability decays to the target
//!   retention; it *grows* with every successful review, more when the card was
//!   nearly forgotten (high information gain).
//! * **difficulty** `D` ∈ [1, 10] — the card's intrinsic hardness, nudged by how
//!   you grade it and gently reverting toward easy over time.
//!
//! Recall probability after `t` days follows a power forgetting curve
//! `R(t) = (1 + FACTOR·t/S)^DECAY`; the next interval is the `t` at which `R`
//! falls to the desired retention. Unlike the Leitner ladder this is
//! *elapsed-time aware* (reviewing early vs. late changes the update) and
//! *per-card* (difficulty is learned), which schedules reviews markedly better.
//!
//! The default weights are FSRS-4.5's published defaults; this module does not
//! (yet) re-optimize them from a user's history, so the constants are fixed. The
//! formulas are the contract — the tests assert behavioural properties (success
//! grows `S`, lapses shrink it, intervals lengthen) rather than exact numbers,
//! so the scheduler stays correct even if the weights are later tuned.

use serde::{Deserialize, Serialize};

/// How a graded review went. Mamba derives this from the verified outcome and
/// the attempt count (a first-try solve is `Good`, a multi-try solve is `Hard`,
/// a lapse is `Again`); `Easy` is reserved for a future "too easy" signal.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Grade {
    Again = 1,
    Hard = 2,
    Good = 3,
    Easy = 4,
}

/// A card's learned memory state, persisted in its vault note's frontmatter.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Memory {
    pub stability: f64,
    pub difficulty: f64,
}

/// FSRS-4.5 default parameters.
const W: [f64; 17] = [
    0.4072, 1.1829, 3.1262, 15.4722, 7.2102, 0.5316, 1.0651, 0.0234, 1.616, 0.1544, 1.0824, 1.9813,
    0.0953, 0.2975, 2.2042, 0.2407, 2.9466,
];

/// Exponent of the power forgetting curve.
const DECAY: f64 = -0.5;
/// `FACTOR = 0.9^(1/DECAY) − 1`, so retrievability is exactly 0.9 at `t = S`.
const FACTOR: f64 = 19.0 / 81.0;

/// Desired retention used when nothing else is specified (Anki's default).
pub const DEFAULT_RETENTION: f64 = 0.9;

fn clamp_difficulty(d: f64) -> f64 {
    d.clamp(1.0, 10.0)
}

/// Initial difficulty for a card first graded `g`.
fn init_difficulty(g: i32) -> f64 {
    clamp_difficulty(W[4] - (W[5] * (g as f64 - 1.0)).exp() + 1.0)
}

impl Memory {
    /// The memory state after the *first* review of a brand-new card.
    pub fn init(grade: Grade) -> Memory {
        let g = grade as i32;
        Memory {
            stability: W[(g - 1) as usize].max(0.1),
            difficulty: init_difficulty(g),
        }
    }

    /// Recall probability `t` days after the last review.
    pub fn retrievability(&self, t: f64) -> f64 {
        (1.0 + FACTOR * t.max(0.0) / self.stability).powf(DECAY)
    }

    /// Days until recall probability falls to `retention` (≥ 1).
    pub fn interval(&self, retention: f64) -> i64 {
        let ivl = self.stability / FACTOR * (retention.powf(1.0 / DECAY) - 1.0);
        (ivl.round() as i64).max(1)
    }

    /// The memory state after grading the card `grade`, `elapsed` days since the
    /// previous review.
    pub fn review(&self, grade: Grade, elapsed: f64) -> Memory {
        let g = grade as i32;
        let r = self.retrievability(elapsed);

        // Difficulty: nudge by the grade, then mean-revert toward "easy".
        let d_prime = self.difficulty - W[6] * (g as f64 - 3.0);
        let difficulty = clamp_difficulty(W[7] * init_difficulty(4) + (1.0 - W[7]) * d_prime);

        let stability = if g == 1 {
            // Lapse: stability collapses (but never below a small floor), scaled
            // by difficulty and how much was still remembered.
            let s = W[11]
                * self.difficulty.powf(-W[12])
                * ((self.stability + 1.0).powf(W[13]) - 1.0)
                * (W[14] * (1.0 - r)).exp();
            s.clamp(0.1, self.stability)
        } else {
            // Success: stability grows; the gain is largest when R was low
            // (you nearly forgot), and modulated by Hard/Easy.
            let hard = if g == 2 { W[15] } else { 1.0 };
            let easy = if g == 4 { W[16] } else { 1.0 };
            let gain = W[8].exp()
                * (11.0 - self.difficulty)
                * self.stability.powf(-W[9])
                * ((W[10] * (1.0 - r)).exp() - 1.0)
                * hard
                * easy;
            (self.stability * (1.0 + gain)).max(self.stability)
        };

        Memory {
            stability: stability.max(0.1),
            difficulty,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn first_review_sets_a_positive_state() {
        let m = Memory::init(Grade::Good);
        assert!(m.stability > 0.0);
        assert!((1.0..=10.0).contains(&m.difficulty));
        // A harder first grade leaves a lower stability than an easier one.
        assert!(Memory::init(Grade::Again).stability < Memory::init(Grade::Easy).stability);
    }

    #[test]
    fn retrievability_is_one_at_zero_and_target_at_the_interval() {
        let m = Memory::init(Grade::Good);
        assert!((m.retrievability(0.0) - 1.0).abs() < 1e-9);
        // At t = S the curve is exactly the 0.9 design point.
        assert!((m.retrievability(m.stability) - 0.9).abs() < 1e-6);
    }

    #[test]
    fn success_grows_stability_and_lengthens_intervals() {
        let m = Memory::init(Grade::Good);
        let i0 = m.interval(DEFAULT_RETENTION);
        // Review on time with Good → stability (and the next interval) grow.
        let m2 = m.review(Grade::Good, i0 as f64);
        assert!(m2.stability > m.stability);
        assert!(m2.interval(DEFAULT_RETENTION) >= i0);
        // Good grows stability more than Hard.
        let hard = m.review(Grade::Hard, i0 as f64);
        assert!(m2.stability > hard.stability);
    }

    #[test]
    fn a_lapse_shrinks_stability() {
        let m = Memory::init(Grade::Good).review(Grade::Good, 5.0);
        let lapsed = m.review(Grade::Again, m.stability);
        assert!(lapsed.stability < m.stability);
    }

    #[test]
    fn interval_is_at_least_one_day() {
        let weak = Memory {
            stability: 0.05,
            difficulty: 9.0,
        };
        assert!(weak.interval(DEFAULT_RETENTION) >= 1);
    }
}
