//! End-to-end: `mamba solve <id> <answer>` verifies one problem from the shell,
//! exiting 0 on a correct answer and non-zero otherwise.

use std::process::{Command, Output};

fn mamba(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_mamba"))
        .args(args)
        .env("MAMBA_PROBLEMS", "problems") // tests run from the crate root
        .output()
        .expect("failed to run the mamba binary")
}

#[test]
fn solve_accepts_correct_rejects_wrong_and_unknown() {
    // expand-a-binomial: (x-3)(x+2) ≡ x^2 - x - 6.
    let ok = mamba(&["solve", "expand-a-binomial", "(x-3)(x+2)"]);
    assert!(
        ok.status.success(),
        "expected success; stdout={}",
        String::from_utf8_lossy(&ok.stdout)
    );
    assert!(String::from_utf8_lossy(&ok.stdout).contains("Correct"));

    // A wrong answer exits non-zero.
    let bad = mamba(&["solve", "expand-a-binomial", "x^2 + x - 6"]);
    assert!(!bad.status.success());
    assert!(String::from_utf8_lossy(&bad.stdout).contains("Incorrect"));

    // A matrix answer spanning several args is rejoined.
    let matrix = mamba(&["solve", "determinant-2x2", "10"]);
    assert!(matrix.status.success());

    // An unknown id exits non-zero (2).
    let missing = mamba(&["solve", "no-such-problem", "1"]);
    assert!(!missing.status.success());
}
