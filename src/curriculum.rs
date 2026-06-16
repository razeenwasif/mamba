//! The curriculum: a prerequisite DAG over topics.
//!
//! Mathematics has a natural order — you meet algebra before calculus, calculus
//! before ODEs, machine learning before deep learning. This module encodes those
//! edges so the recommender ([`crate::recommend`]) can steer you down a sensible
//! path: a topic is *unlocked* once its prerequisites are sufficiently practised,
//! and `mamba next` prefers unlocked topics over locked ones (without ever
//! dead-ending — a locked topic is deprioritised, never hidden).
//!
//! The DAG is a small embedded default keyed by the bank's topic names. It is
//! pure data + pure functions, so it's trivially testable and easy to extend.

use std::collections::HashSet;

/// `(topic, its prerequisite topics)`. Topics absent here (e.g. `Algebra`,
/// `Graph Theory`) are roots with no prerequisites.
const EDGES: &[(&str, &[&str])] = &[
    ("Trigonometry", &["Algebra"]),
    ("Calculus", &["Algebra"]),
    ("Complex Numbers", &["Algebra"]),
    ("Linear Algebra", &["Algebra"]),
    ("Number Theory", &["Algebra"]),
    ("Probability", &["Algebra"]),
    ("Statistics", &["Probability"]),
    ("Hypothesis Testing", &["Statistics"]),
    ("Decompositions", &["Linear Algebra"]),
    ("ODEs", &["Calculus"]),
    ("Convex Optimization", &["Calculus", "Linear Algebra"]),
    (
        "Machine Learning",
        &["Calculus", "Linear Algebra", "Probability"],
    ),
    ("Deep Learning", &["Machine Learning"]),
    ("Linear Programming", &["Linear Algebra"]),
    ("Signals & DSP", &["Calculus", "Complex Numbers"]),
    ("Physics", &["Algebra", "Calculus"]),
    ("Engineering Math", &["Physics", "Complex Numbers"]),
];

/// Fraction of a prerequisite topic's problems that must be solved for it to
/// count as "met" (and so unlock its dependents).
pub const MET_THRESHOLD: f64 = 1.0 / 3.0;

/// The prerequisite topics of `topic` (empty for a root).
pub fn prerequisites(topic: &str) -> &'static [&'static str] {
    EDGES
        .iter()
        .find(|(t, _)| *t == topic)
        .map(|(_, prereqs)| *prereqs)
        .unwrap_or(&[])
}

/// Whether `topic` is unlocked: every prerequisite satisfies `met`.
pub fn is_unlocked(topic: &str, met: impl Fn(&str) -> bool) -> bool {
    prerequisites(topic).iter().all(|p| met(p))
}

/// The prerequisites of `topic` that are *not* yet met (for an explanatory
/// "locked until …" message).
pub fn missing_prerequisites(topic: &str, met: impl Fn(&str) -> bool) -> Vec<&'static str> {
    prerequisites(topic)
        .iter()
        .copied()
        .filter(|p| !met(p))
        .collect()
}

/// Orders `topics` so every prerequisite precedes its dependents (a stable
/// topological sort, alphabetical within a tier). Prerequisites not present in
/// `topics` are ignored, and an unexpected cycle degrades gracefully.
pub fn topo_order(topics: &[String]) -> Vec<String> {
    let present: HashSet<&str> = topics.iter().map(String::as_str).collect();
    let mut remaining: Vec<String> = topics.to_vec();
    remaining.sort();
    remaining.dedup();

    let mut done: HashSet<String> = HashSet::new();
    let mut ordered: Vec<String> = Vec::with_capacity(remaining.len());
    while !remaining.is_empty() {
        let ready: Vec<String> = remaining
            .iter()
            .filter(|t| {
                prerequisites(t)
                    .iter()
                    .all(|p| !present.contains(p) || done.contains(*p))
            })
            .cloned()
            .collect();
        if ready.is_empty() {
            ordered.extend(remaining.drain(..)); // cycle — emit the rest as-is
            break;
        }
        for t in &ready {
            done.insert(t.clone());
        }
        remaining.retain(|t| !ready.contains(t));
        ordered.extend(ready);
    }
    ordered
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roots_have_no_prerequisites() {
        assert!(prerequisites("Algebra").is_empty());
        assert_eq!(prerequisites("Calculus"), &["Algebra"]);
        assert_eq!(prerequisites("Deep Learning"), &["Machine Learning"]);
    }

    #[test]
    fn unlock_respects_prerequisites() {
        // Nothing met → Calculus is locked, Algebra (a root) is open.
        let nothing = |_: &str| false;
        assert!(is_unlocked("Algebra", nothing));
        assert!(!is_unlocked("Calculus", nothing));
        assert_eq!(missing_prerequisites("Calculus", nothing), vec!["Algebra"]);

        // Meet Algebra → Calculus unlocks; Deep Learning still needs ML.
        let algebra_only = |t: &str| t == "Algebra";
        assert!(is_unlocked("Calculus", algebra_only));
        assert!(!is_unlocked("Deep Learning", algebra_only));
    }

    #[test]
    fn topo_order_places_prerequisites_first() {
        let topics: Vec<String> = ["Deep Learning", "Algebra", "Calculus", "Machine Learning"]
            .iter()
            .map(|s| s.to_string())
            .collect();
        let order = topo_order(&topics);
        let pos = |t: &str| order.iter().position(|x| x == t).unwrap();
        assert!(pos("Algebra") < pos("Calculus"));
        assert!(pos("Calculus") < pos("Machine Learning"));
        assert!(pos("Machine Learning") < pos("Deep Learning"));
    }
}
