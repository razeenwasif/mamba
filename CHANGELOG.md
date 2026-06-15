# Changelog

All notable changes to Mamba are documented here. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and the project aims
to follow [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added
- **Richer formula rendering.** A new `src/ui/formula.rs` turns plain-ASCII math
  markers in prompts into typeset terminal math: `^…` → superscripts
  (`x^2` → `x²`, `x^(n-1)` → `xⁿ⁻¹`, `A^T` → `Aᵀ`), `_…` → subscripts
  (`a_i` → `aᵢ`), and a line that is wholly a matrix literal `[[4, 7], [2, 6]]`
  → a Unicode-bracketed multi-line layout. Each script run is all-or-nothing
  (falls back to literal if a character has no Unicode form). Prompts can now be
  authored in ASCII — the generator and hand-authored problems use it.
- **Problem-bank generator + `mamba --verify`.** A pure-Python generator
  (`tools/build_problems/generate.py`) mass-authors problems across all three
  domains with answers that are *correct by construction* (exact `int`/`Fraction`
  arithmetic — no SymPy), emitted in Mamba's own input syntax. A new
  `mamba --verify [DIR]` CLI mode runs every problem's canonical answer back
  through the engine (PASS/FAIL, non-zero exit on failure), closing the loop
  end-to-end; `make gen` does both. The exact pipeline now derives a
  factorization's target from the **product of `expected`'s factors**, so
  composite factorization problems self-verify (not just primes).
- **Exact-rational tensor backend.** Matrix cells are now parsed as a hybrid
  [`Cell`]: an exact rational (`num-rational`) for integers/fractions, or an
  approximate float for decimals. Both-exact cells compare with `==` (so a `2×2`
  inverse with entries like `3/5`, `-7/10` verifies with no rounding error, and
  `6/10` equals `3/5`); a decimal on either side falls back to float tolerance.
  Backward compatible — integer matrices still pass. New bootstrap problem
  (Inverse of a 2×2). See
  [ADR 0004](docs/adr/0004-rational-tensor-cells.md).
- **Multivariate symbolic verification.** The symbolic pipeline now detects every
  free variable in both expressions and probes over all of them at once, so
  multi-variable answers verify by algebraic equivalence
  (`(x+y)^2 ≡ x^2 + 2*x*y + y^2`). The same machinery covers trigonometric
  identities (`sin(x)^2 + cos(x)^2 ≡ 1`) and rational forms via `meval`'s
  built-ins — at no dependency cost. Two new bootstrap problems (Expand a Perfect
  Square, Pythagorean Identity). See
  [ADR 0003](docs/adr/0003-multivariate-symbolic-probing.md), which also records
  why `symbolica` was **not** adopted (proprietary/key-gated license + GMP/mimalloc
  C deps, incompatible with Mamba's MIT pure-Rust single-binary design).
- **Fuzzy problem finder (`/`).** A Spotlight-style floating overlay (ported
  from `~/Kata`, `fuzzy-matcher`/SkimMatcherV2) filters the bank as you type,
  matching against title, topic, difficulty, and domain, ranked best-first.
  `↑`/`↓` move, `↵` opens the highlight and jumps to the answer bar, `esc`
  cancels and restores the navigator. `/` stays a literal character while typing
  an answer. Adds a live match-count badge in the top bar.
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
