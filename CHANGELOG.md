# Changelog

All notable changes to Mamba are documented here. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and the project aims
to follow [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added
- **Tutor shortcuts.** `Ctrl-E` after a wrong answer asks the tutor to explain
  what's going wrong (the console shows a `press ^E to ask the tutor why` hint);
  and typing an answer that **starts with `?`** sends the rest to the tutor as a
  one-shot question instead of verifying it (a bare `?` asks for a hint). Both
  open and focus the panel.
- **Local AI tutor (`Ctrl-A`).** A right-hand chat panel powered by a local
  [Ollama](https://ollama.com) model (default `gemma4:12b-it-qat`, overridable
  via `MAMBA_OLLAMA_MODEL`/`MAMBA_OLLAMA_URL`). The system prompt is scoped to
  the open problem and your current answer attempt, and the model is asked to
  prefer hints and method over the full answer. The default model is the light
  `gemma4:e2b-it-qat` for snappy hints. Replies stream token-by-token on
  a worker thread over an `mpsc` channel (the same pattern as verification), so
  the UI never blocks; failures (server down, model not pulled) surface in the
  panel and nothing else breaks. See
  [ADR 0005](docs/adr/0005-local-ai-tutor-via-ollama.md).

## [0.1.0] — 2026-06-15

The first tagged release. A single-binary terminal math practice engine with a
multi-domain verification engine, a 124-problem bank, a spaced-repetition study
loop, a Markdown vault, and a problem generator.

### Added
- **`mamba solve <id> <answer>` subcommand.** Verify a single problem's answer
  from the shell without the TUI — prints the verdict and exits `0` only when
  correct, so it's usable in scripts and CI. The answer may span several args
  (e.g. a matrix) and is rejoined. End-to-end CLI test included.
- **Probability & Statistics problems** (bank 103 → 124): combinations,
  permutations, and factorials (Exact, exercising arbitrary-precision integers);
  two-dice probabilities, expected value, discrete variance, Bayes' rule, the
  complement rule, and the sample mean (Tensor, exact `BigRational` fractions).
  Six new generator templates + three curated problems under new `Probability`
  and `Statistics` topics; all pass `mamba --verify`.
- **Spaced-repetition review mode (`r`).** Pressing `r` scans the vault and
  filters the navigator to the problems **due for review** — never-solved ones,
  and solved ones whose Leitner-style interval (1 → 3 → 7 → 16 → 35 → 90 days,
  growing with each solve) has elapsed. The pane title shows `Review · N due`,
  combines with the topic filter, and shows "✓ all caught up!" when nothing is
  due. New pure `review` module (`interval_days`, `is_due`).
- **Navigator topic filter (`t`).** Pressing `t` cycles the navigator through
  `all → topic₁ → … → topicₙ → all`, showing only the selected topic's problems
  (the pane title becomes `Topic · <name>`). Handy with 100+ problems for
  drilling into e.g. just Deep Learning or Convex Optimization.
- **In-TUI attempt-history peek.** When a problem opens (and after each solve),
  the verification console shows a `📒 solved N× · last YYYY-MM-DD` line read
  from the problem's vault note — at-a-glance recall of how often and how
  recently you've solved it. `vault::history` reads it back from the `## History`
  table.
- **Deep Learning problem topic** (bank 86 → 103): conv output length, linear/conv
  layer parameter counts, activation mean & (biased) variance for LayerNorm, plus
  cross-entropy derivative and BatchNorm normalization. Five new generator
  templates and two curated problems under a new `Deep Learning` topic spanning
  all three pipelines; all pass `mamba --verify`.
- **Richer ML/AI & convex-optimization problems** (bank 68 → 86, 41 ML/Convex):
  softmax Jacobian and softmax cross-entropy gradient (`p − y`), KKT optimum,
  positive-semidefinite checks (1/0), Lagrangian stationarity, gradient-descent
  steps, and deep-learning shape arithmetic (backprop weight-gradient and
  attention output shapes). Five new parametrized generator templates plus three
  curated problems; all pass `mamba --verify`.
- **Per-problem attempt history in the vault.** A solved problem's note now
  accumulates a `## History` table — one row per solve (date · attempts-to-solve
  · time) — by reading the prior note and appending, instead of overwriting it.
  The frontmatter gains `attempts` (latest) and `solves` (total). The latest
  accepted answer and frontmatter still reflect the most recent solve.
- **In-TUI progress overlay (`Ctrl-P`).** A modal, centered overlay shows live
  all-time progress computed from the vault (via the shared `progress` stats
  API): total solved, **per-domain, per-difficulty, and per-topic** bars,
  current/longest streak, and recent solves. `Esc`/`q`/`^P` close it. The
  `_progress.md` dashboard gained a matching `## By topic` section.
- **Topic sub-grouping in the navigator.** With the bank now at 68 problems, the
  sidebar groups problems by **topic within each domain** (e.g. Machine
  Learning, Convex Optimization, Number Theory under their domains), so they are
  browseable, not just findable via `/`. The bank now sorts by
  (domain, topic, difficulty, title).
- **More generator templates** (bank 56 → 68): least common multiple, modular
  inverse (`pow(a, -1, m)`), eigenvalues of a triangular matrix, and solving a
  2×2 linear system by Cramer's rule (exact-fraction solutions). All pass
  `mamba --verify`.
- **Machine-learning & convex-optimization problem sets.** 23 new problems
  (bank 25 → 56): activation/loss gradients (sigmoid, tanh, softplus, MSE,
  binary cross-entropy logit, L2/ridge), and convex-optimization staples
  (convexity via the 2nd derivative, log-barrier and exponential gradients,
  negative entropy, Hessian of a quadratic, gradient of a linear form, Gram
  matrix). These map onto the existing Symbolic and Tensor pipelines — the
  symbolic pipeline verifies `exp`/`ln`/`tanh` expressions by numeric probing.
  Authored as a curated `FIXED` set plus three new parametrized generator
  templates (linear-form gradient, quadratic Hessian, ridge gradient); the
  generator default count is now 3. All 56 problems pass `mamba --verify`.
- **Rolling progress dashboard.** On every correct answer, after the per-problem
  note is written, Mamba (re)renders `_progress.md` in the vault: total solved, a
  breakdown by domain and by difficulty, current and longest day streaks, and the
  most recent solves. The vault stays the single source of truth — the dashboard
  is aggregated by scanning the per-problem notes' frontmatter (`storage/progress.rs`),
  with no separate ledger. Best-effort: a dashboard write failure never sinks an
  already-saved solve.
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

### Changed
- **Tensor cells widened to arbitrary precision.** Exact matrix cells use
  `BigRational` (`Ratio<BigInt>`) instead of `Ratio<i64>`, so no numerator or
  denominator can ever overflow — fractions of any size compare exactly (resolves
  the limitation noted in [ADR 0004](docs/adr/0004-rational-tensor-cells.md)).

### Foundation (initial scaffold)
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
