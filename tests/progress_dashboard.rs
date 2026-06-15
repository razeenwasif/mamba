//! End-to-end: exporting solves then refreshing the dashboard produces a
//! `_progress.md` with correct totals, breakdowns, and streaks — the same path
//! `App::export_solution` drives after a correct answer.

use std::time::Duration;

use chrono::NaiveDate;
use mamba::storage::problem::{AnswerKind, Difficulty, Domain, Problem};
use mamba::storage::{progress, vault};

fn problem(id: &str, title: &str, domain: Domain, difficulty: Difficulty) -> Problem {
    Problem {
        id: id.into(),
        title: title.into(),
        domain,
        topic: "Demo".into(),
        difficulty,
        prompt: "p".into(),
        expected: "1".into(),
        answer_kind: AnswerKind::Scalar,
        hint: None,
        recap: None,
    }
}

#[test]
fn solving_problems_builds_a_progress_dashboard() {
    let dir = std::env::temp_dir().join(format!("mamba-progress-it-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);

    let day = |d: u32| NaiveDate::from_ymd_opt(2026, 6, d).unwrap();
    let solves = [
        (
            problem("a", "Expand a Binomial", Domain::Symbolic, Difficulty::Easy),
            day(14),
        ),
        (
            problem("b", "Determinant", Domain::Tensor, Difficulty::Easy),
            day(15),
        ),
        (
            problem("c", "Modular Power", Domain::Exact, Difficulty::Hard),
            day(15),
        ),
    ];
    for (p, d) in &solves {
        vault::export(&dir, p, "answer", 1, Duration::from_secs(90), *d, None).unwrap();
    }

    let path = progress::update(&dir, day(15)).unwrap();
    let dashboard = std::fs::read_to_string(&path).unwrap();
    println!("\n----- {} -----\n{dashboard}", path.display());

    assert!(dashboard.contains("**3 solved**"));
    assert!(dashboard.contains("current streak **2**")); // the 14th and 15th
    // Per-domain / per-difficulty rows exist regardless of column padding.
    let row = |label: &str, count: usize| {
        dashboard.lines().any(|l| {
            l.starts_with(&format!("- {label}"))
                && l.split_whitespace().last() == Some(count.to_string().as_str())
        })
    };
    assert!(row("Symbolic", 1));
    assert!(row("Tensor", 1));
    assert!(row("Exact", 1));
    assert!(row("Easy", 2));
    assert!(row("Hard", 1));
    // The dashboard lists recent solves and never counts itself.
    assert!(dashboard.contains("Expand a Binomial  [Symbolic · Easy]"));

    let _ = std::fs::remove_dir_all(&dir);
}
