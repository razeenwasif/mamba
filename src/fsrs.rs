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
//! The 17 weights default to FSRS-4.5's published defaults but can be
//! *personalised*: [`optimize`] fits them to a user's review log (minimising the
//! log-loss of predicted vs. actual recall), and `mamba optimize` persists the
//! result for the next run to [`set_weights`]. The formulas are the contract —
//! the tests assert behavioural properties (success grows `S`, lapses shrink it,
//! intervals lengthen) rather than exact numbers, so the scheduler stays correct
//! across re-tuning.

use std::path::Path;
use std::sync::OnceLock;

use serde::{Deserialize, Serialize};

/// How a graded review went. Mamba derives this from the verified outcome, the
/// attempt count, and the solve time: a *fast* first-try solve is `Easy`, any
/// other first-try is `Good`, a multi-attempt solve is `Hard`, and leaving a
/// known card unsolved is `Again`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Grade {
    Again = 1,
    Hard = 2,
    Good = 3,
    Easy = 4,
}

impl Grade {
    /// Maps a `1..=4` code (as stored in the review log) back to a grade.
    pub fn from_u8(n: u8) -> Option<Grade> {
        match n {
            1 => Some(Grade::Again),
            2 => Some(Grade::Hard),
            3 => Some(Grade::Good),
            4 => Some(Grade::Easy),
            _ => None,
        }
    }
}

/// A card's learned memory state, persisted in its vault note's frontmatter.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Memory {
    pub stability: f64,
    pub difficulty: f64,
}

/// The 17 FSRS-4.5 parameters.
pub type Weights = [f64; 17];

/// FSRS-4.5 default parameters.
pub const DEFAULT_WEIGHTS: Weights = [
    0.4072, 1.1829, 3.1262, 15.4722, 7.2102, 0.5316, 1.0651, 0.0234, 1.616, 0.1544, 1.0824, 1.9813,
    0.0953, 0.2975, 2.2042, 0.2407, 2.9466,
];

/// Per-weight `(min, max)` clamps that keep optimisation in a sane region.
const BOUNDS: [(f64, f64); 17] = [
    (0.01, 100.0),
    (0.01, 100.0),
    (0.01, 100.0),
    (0.01, 100.0),
    (1.0, 10.0),
    (0.1, 5.0),
    (0.01, 3.0),
    (0.001, 0.75),
    (0.0, 5.0),
    (0.0, 0.8),
    (0.01, 5.0),
    (0.1, 5.0),
    (0.01, 2.0),
    (0.01, 0.9),
    (0.01, 4.0),
    (0.0, 1.0),
    (1.0, 6.0),
];

/// The process-wide active weights, set once at startup (see [`set_weights`]).
static ACTIVE: OnceLock<Weights> = OnceLock::new();

/// The weights currently in effect (the defaults until [`set_weights`] runs).
pub fn weights() -> &'static Weights {
    ACTIVE.get_or_init(|| DEFAULT_WEIGHTS)
}

/// Installs personalised weights for the rest of the process. Call once at
/// startup, before any scheduling; a no-op if already set.
pub fn set_weights(w: Weights) {
    let _ = ACTIVE.set(w);
}

/// Exponent of the power forgetting curve.
const DECAY: f64 = -0.5;
/// `FACTOR = 0.9^(1/DECAY) − 1`, so retrievability is exactly 0.9 at `t = S`.
const FACTOR: f64 = 19.0 / 81.0;

/// Desired retention used when nothing else is specified (Anki's default).
pub const DEFAULT_RETENTION: f64 = 0.9;

/// The active target retention: `MAMBA_RETENTION` if set and sane, else the
/// default. Clamped to `[0.70, 0.97]` — outside that, FSRS intervals get
/// pathological (almost never reviewing, or reviewing constantly). Higher
/// retention means shorter intervals (study more, forget less).
pub fn retention() -> f64 {
    std::env::var("MAMBA_RETENTION")
        .ok()
        .and_then(|v| v.trim().parse::<f64>().ok())
        .filter(|r| r.is_finite())
        .map(|r| r.clamp(0.70, 0.97))
        .unwrap_or(DEFAULT_RETENTION)
}

fn clamp_difficulty(d: f64) -> f64 {
    d.clamp(1.0, 10.0)
}

/// Initial difficulty for a card first graded `g` under weights `w`.
fn init_difficulty(w: &Weights, g: i32) -> f64 {
    clamp_difficulty(w[4] - (w[5] * (g as f64 - 1.0)).exp() + 1.0)
}

/// Recall probability `t` days after a review of a card with stability `s`.
fn retrievability(s: f64, t: f64) -> f64 {
    (1.0 + FACTOR * t.max(0.0) / s).powf(DECAY)
}

/// The first-review memory state under weights `w`.
fn init_memory(w: &Weights, grade: Grade) -> Memory {
    let g = grade as i32;
    Memory {
        stability: w[(g - 1) as usize].max(0.1),
        difficulty: init_difficulty(w, g),
    }
}

/// The post-review memory state under weights `w` — the FSRS-4.5 update.
fn review_memory(w: &Weights, mem: Memory, grade: Grade, elapsed: f64) -> Memory {
    let g = grade as i32;
    let r = retrievability(mem.stability, elapsed);

    // Difficulty: nudge by the grade, then mean-revert toward "easy".
    let d_prime = mem.difficulty - w[6] * (g as f64 - 3.0);
    let difficulty = clamp_difficulty(w[7] * init_difficulty(w, 4) + (1.0 - w[7]) * d_prime);

    let stability = if g == 1 {
        // Lapse: stability collapses (but never below a small floor), scaled by
        // difficulty and how much was still remembered.
        let s = w[11]
            * mem.difficulty.powf(-w[12])
            * ((mem.stability + 1.0).powf(w[13]) - 1.0)
            * (w[14] * (1.0 - r)).exp();
        s.clamp(0.1, mem.stability)
    } else {
        // Success: stability grows; the gain is largest when R was low (you
        // nearly forgot), modulated by Hard/Easy.
        let hard = if g == 2 { w[15] } else { 1.0 };
        let easy = if g == 4 { w[16] } else { 1.0 };
        let gain = w[8].exp()
            * (11.0 - mem.difficulty)
            * mem.stability.powf(-w[9])
            * ((w[10] * (1.0 - r)).exp() - 1.0)
            * hard
            * easy;
        (mem.stability * (1.0 + gain)).max(mem.stability)
    };

    Memory {
        stability: stability.max(0.1),
        difficulty,
    }
}

/// A first-try solve completed within this many seconds counts as `Easy`
/// (strong, quick recall) rather than `Good`.
pub const EASY_SECONDS: u64 = 20;

/// The grade a *solve* earns from its outcome: a fast first-try solve is `Easy`,
/// any other first-try solve `Good`, and a multi-attempt solve `Hard`. (A lapse
/// — `Again` — comes from leaving a known card unsolved.)
pub fn grade_for_solve(attempts: u32, seconds: u64) -> Grade {
    if attempts <= 1 {
        if seconds <= EASY_SECONDS {
            Grade::Easy
        } else {
            Grade::Good
        }
    } else {
        Grade::Hard
    }
}

impl Memory {
    /// The memory state after the *first* review of a brand-new card.
    pub fn init(grade: Grade) -> Memory {
        init_memory(weights(), grade)
    }

    /// Recall probability `t` days after the last review.
    pub fn retrievability(&self, t: f64) -> f64 {
        retrievability(self.stability, t)
    }

    /// Days until recall probability falls to `retention` (≥ 1).
    pub fn interval(&self, retention: f64) -> i64 {
        let ivl = self.stability / FACTOR * (retention.powf(1.0 / DECAY) - 1.0);
        (ivl.round() as i64).max(1)
    }

    /// The memory state after grading the card `grade`, `elapsed` days since the
    /// previous review.
    pub fn review(&self, grade: Grade, elapsed: f64) -> Memory {
        review_memory(weights(), *self, grade, elapsed)
    }
}

// -- Parameter optimisation --------------------------------------------------

/// One card's review history: `(elapsed_days_since_previous, grade)` per review,
/// in order (the first review's elapsed is ignored).
pub type Sequence = Vec<(f64, Grade)>;

/// Mean log-loss of predicted vs. actual recall over `sequences` under `w`:
/// for each review after a card's first, predict recall from the replayed
/// memory state and score it against whether the card was actually recalled
/// (grade ≥ Hard). Lower = better-calibrated weights. `0.0` if there's nothing
/// to score.
pub fn loss(w: &Weights, sequences: &[Sequence]) -> f64 {
    let (mut total, mut n) = (0.0f64, 0usize);
    for seq in sequences {
        let Some((_, first)) = seq.first() else {
            continue;
        };
        let mut mem = init_memory(w, *first);
        for &(elapsed, grade) in &seq[1..] {
            let p = retrievability(mem.stability, elapsed).clamp(1e-6, 1.0 - 1e-6);
            let actual = if grade as i32 >= 2 { 1.0 } else { 0.0 };
            total += -(actual * p.ln() + (1.0 - actual) * (1.0 - p).ln());
            n += 1;
            mem = review_memory(w, mem, grade, elapsed);
        }
    }
    if n == 0 { 0.0 } else { total / n as f64 }
}

/// Number of scorable reviews (those after each card's first) in `sequences` —
/// a proxy for how trustworthy an optimisation over them is.
pub fn scorable_reviews(sequences: &[Sequence]) -> usize {
    sequences.iter().map(|s| s.len().saturating_sub(1)).sum()
}

/// Fits the weights to `sequences` by clamped numerical-gradient descent on
/// [`loss`], starting from the defaults. Returns the best weights found (never
/// worse than the defaults). Best-effort: with little data it stays near the
/// defaults, which is the safe behaviour.
pub fn optimize(sequences: &[Sequence], iterations: usize) -> Weights {
    const EPS: f64 = 1e-4;
    const LR: f64 = 0.05;

    let clamp = |w: &mut Weights| {
        for i in 0..17 {
            w[i] = w[i].clamp(BOUNDS[i].0, BOUNDS[i].1);
        }
    };

    let mut w = DEFAULT_WEIGHTS;
    let mut best = w;
    let mut best_loss = loss(&w, sequences);
    for _ in 0..iterations {
        let base = loss(&w, sequences);
        let mut grad = [0.0f64; 17];
        for i in 0..17 {
            let mut wp = w;
            wp[i] += EPS;
            grad[i] = (loss(&wp, sequences) - base) / EPS;
        }
        for i in 0..17 {
            w[i] -= LR * grad[i];
        }
        clamp(&mut w);
        let l = loss(&w, sequences);
        if l < best_loss {
            best_loss = l;
            best = w;
        }
    }
    best
}

/// Persists weights as JSON to `path` (creating the parent directory).
pub fn save_weights(path: &Path, w: &Weights) -> anyhow::Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(path, serde_json::to_string(w)?)?;
    Ok(())
}

/// Loads weights from `path`, or `None` if absent/malformed.
pub fn load_weights(path: &Path) -> Option<Weights> {
    let text = std::fs::read_to_string(path).ok()?;
    serde_json::from_str(&text).ok()
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

    #[test]
    fn solve_grade_reflects_attempts_and_speed() {
        // Fast first try → Easy; slower first try → Good; retries → Hard.
        assert_eq!(grade_for_solve(1, 5), Grade::Easy);
        assert_eq!(grade_for_solve(1, EASY_SECONDS), Grade::Easy); // boundary
        assert_eq!(grade_for_solve(1, EASY_SECONDS + 1), Grade::Good);
        assert_eq!(grade_for_solve(3, 2), Grade::Hard); // speed can't rescue retries
        // Easy earns a longer initial interval than Good.
        assert!(
            Memory::init(Grade::Easy).interval(DEFAULT_RETENTION)
                > Memory::init(Grade::Good).interval(DEFAULT_RETENTION)
        );
    }

    #[test]
    fn grade_round_trips_through_u8() {
        for g in [Grade::Again, Grade::Hard, Grade::Good, Grade::Easy] {
            assert_eq!(Grade::from_u8(g as u8), Some(g));
        }
        assert_eq!(Grade::from_u8(0), None);
    }

    #[test]
    fn loss_is_lower_when_recall_matches_the_prediction() {
        // A card reviewed at short, easy intervals and always recalled should be
        // better explained (lower loss) than the same schedule with a surprise
        // lapse the model didn't see coming.
        let recalled: Sequence = vec![(0.0, Grade::Good), (1.0, Grade::Good), (1.0, Grade::Good)];
        let lapsed: Sequence = vec![(0.0, Grade::Good), (1.0, Grade::Again), (1.0, Grade::Again)];
        let w = DEFAULT_WEIGHTS;
        assert!(loss(&w, &[recalled]) < loss(&w, &[lapsed]));
    }

    #[test]
    fn optimize_never_increases_the_loss() {
        // A handful of mixed sequences; the optimiser must not return weights
        // worse than the defaults it starts from.
        let sequences = vec![
            vec![(0.0, Grade::Good), (3.0, Grade::Good), (7.0, Grade::Hard)],
            vec![(0.0, Grade::Hard), (2.0, Grade::Again), (1.0, Grade::Good)],
            vec![(0.0, Grade::Good), (5.0, Grade::Good)],
        ];
        let baseline = loss(&DEFAULT_WEIGHTS, &sequences);
        let tuned = optimize(&sequences, 20);
        assert!(loss(&tuned, &sequences) <= baseline + 1e-9);
        // Every weight stays inside its bounds.
        for i in 0..17 {
            assert!(tuned[i] >= BOUNDS[i].0 - 1e-9 && tuned[i] <= BOUNDS[i].1 + 1e-9);
        }
        assert_eq!(scorable_reviews(&sequences), 5);
    }

    #[test]
    fn retention_clamps_and_defaults() {
        // The pure clamp logic (independent of the live env var).
        let resolve = |raw: &str| {
            raw.trim()
                .parse::<f64>()
                .ok()
                .filter(|r| r.is_finite())
                .map(|r| r.clamp(0.70, 0.97))
                .unwrap_or(DEFAULT_RETENTION)
        };
        assert_eq!(resolve("0.85"), 0.85);
        assert_eq!(resolve("0.5"), 0.70); // clamped up
        assert_eq!(resolve("0.999"), 0.97); // clamped down
        assert_eq!(resolve("nonsense"), DEFAULT_RETENTION);
        // The default is used when the env var is unset (the common case).
        assert!((0.70..=0.97).contains(&retention()));
    }

    #[test]
    fn weights_save_and_load_round_trip() {
        let path = std::env::temp_dir().join(format!("mamba-fsrs-{}.json", std::process::id()));
        let _ = std::fs::remove_file(&path);
        let mut w = DEFAULT_WEIGHTS;
        w[0] = 0.5;
        save_weights(&path, &w).unwrap();
        assert_eq!(load_weights(&path), Some(w));
        let _ = std::fs::remove_file(&path);
    }
}
