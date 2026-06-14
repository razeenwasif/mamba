//! Integration guard: every problem shipped in `./problems` must (a) load and
//! (b) have a canonical `expected` answer that its own routed pipeline accepts.
//!
//! This catches the most insidious authoring bug — a problem whose stored
//! answer is itself wrong or mis-typed for its domain — at `cargo test` time,
//! before it ever reaches a user.

use std::path::Path;

use mamba::engine::{self, Verdict};
use mamba::storage::problem::load_all;

#[test]
fn every_shipped_problem_accepts_its_own_expected_answer() {
    let problems = load_all(Path::new("problems")).expect("the problem bank should load");
    assert!(!problems.is_empty(), "expected a non-empty problem bank");

    for p in &problems {
        let verdict = engine::verify(p, &p.expected);
        assert_eq!(
            verdict,
            Verdict::Correct,
            "problem '{}' ({}) rejected its own expected answer {:?}: {:?}",
            p.id,
            p.domain,
            p.expected,
            verdict
        );
    }
}

#[test]
fn covers_all_three_domains() {
    use mamba::storage::problem::Domain;
    let problems = load_all(Path::new("problems")).unwrap();
    for domain in [Domain::Symbolic, Domain::Tensor, Domain::Exact] {
        assert!(
            problems.iter().any(|p| p.domain == domain),
            "the bootstrap bank should include at least one {domain} problem"
        );
    }
}
