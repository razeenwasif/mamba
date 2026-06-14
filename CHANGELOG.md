# Changelog

All notable changes to Mamba are documented here. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and the project aims
to follow [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added
- **Off-thread verification.** `engine::verify` now runs on a detached worker
  thread; the result is delivered over an `mpsc` channel and drained each
  main-loop tick (`App::poll_verify`), so a slow check never blocks input,
  scrolling, or quit. Adds a `Verifying…` console state and a `VERIFYING` badge,
  one-verification-at-a-time, a 16 ms busy-poll, and cancellation when the user
  navigates to another problem. See
  [ADR 0002](docs/adr/0002-off-thread-verification.md).
- **Installable binary on PATH.** `make install` builds in release, installs
  `mamba` to `~/.cargo/bin`, and syncs the problem bank into the platform data
  dir (`~/.local/share/mamba/problems`) so the TUI runs from any directory.
- **CLI flags** (handled before the terminal is touched, so they are
  scriptable): `-V`/`--version`, `-h`/`--help`, and `--check` (a non-interactive
  health check that loads the resolved problem bank and reports the count + path).
- **`Makefile`** with `install`, `build`, `test`, `run`, `check`, `fmt`,
  `clean`, and `help` targets — `make install` is the after-every-feature
  command.

## [0.1.0] — 2026-06-14

### Added
- **Universal Routing Engine** (`engine::verify`): a total `match` over the
  problem `Domain` dispatching to one of three specialized pipelines, each
  returning a uniform `Verdict` (`Correct` | `Incorrect` | `ParseError`).
  - **Symbolic** (`meval`): algebraic equivalence by randomized numeric probing.
  - **Tensor** (`ndarray`): shape-then-element matrix/scalar comparison.
  - **Exact** (`num-bigint`): arbitrary-precision integer comparison and
    factorization checking with deterministic Miller–Rabin primality.
- **Ratatui TUI** mirroring `~/Kata`: domain-grouped navigator, problem pane,
  bottom-anchored answer bar, and a verification console; role-based theme
  palette (`MAMBA_THEME`: venom/midnight/slate).
- **Declarative JSON problem bank** with a bootstrap set of six problems (two
  each across Symbolic, Tensor, Exact).
- **Markdown + YAML vault exporter** to `~/OnyxVault/Mamba` on a correct answer.
- **ADR 0001** — the Universal Math Routing Engine design.
- Unit tests for all three pipelines + the router, and an integration guard
  (`tests/bank_is_solvable.rs`) asserting every shipped problem accepts its own
  canonical answer.
