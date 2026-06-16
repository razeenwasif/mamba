# Changelog

All notable changes to Mamba are documented here. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and the project aims
to follow [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added
- **FSRS “Easy” grade for fast solves.** A first-try solve completed within 20 s
  now grades `Easy` (strong, quick recall) instead of `Good`, earning a longer
  interval — completing the grade signal (Easy / Good / Hard / Again) the
  scheduler derives from the verified outcome and solve time.
- **Configurable FSRS target retention** (`MAMBA_RETENTION` / `retention` in
  `config.toml`, default `0.9`, clamped to `0.70–0.97`). Higher retention →
  shorter intervals (study more, forget less); lower → longer intervals. Applied
  when the next-due date is computed on a solve or lapse.

## [0.7.0] — 2026-06-16

Scheduling and project hygiene: FSRS gains lapse capture and per-user
optimization, the curriculum is overridable, and the project gets CI and
caught-up architecture records.

### Added
- **Continuous integration.** A GitHub Actions workflow enforces the same gates
  as the local loop on every push and PR — `cargo fmt --check`, `cargo clippy -D
  warnings`, `cargo test`, and `mamba --verify` over the whole bank. The codebase
  is clippy-clean under `-D warnings`.
- **Architecture decision records 0006–0008** documenting the storage decision
  (vault as source of truth, no database), FSRS scheduling, and the MCP server.

### Changed
- **The default tutor model is now `gemma4:12b-it-qat`** (was `gemma4:e2b-it-qat`)
  — a more capable model for better hints, explanations, and generated problems.
  Set `MAMBA_OLLAMA_MODEL` (or `ollama_model` in the config file) to a lighter tag
  for faster replies on modest hardware.

### Added
- **Curriculum override file.** The prerequisite DAG behind `mamba next` and
  `mamba path` can now be customised in `~/.config/mamba/curriculum.toml` — a
  table mapping each topic to its prerequisites (`"Calculus" = ["Algebra"]`).
  Absent or malformed → the embedded default. The edges are loaded once at first
  use; `curriculum::prerequisites` now returns owned data sourced from either.
- **FSRS parameter optimization (`mamba optimize`).** The 17 FSRS weights are now
  *personalisable*: `mamba optimize` replays your `reviews.jsonl`, fits the
  weights by clamped numerical-gradient descent on the log-loss of predicted vs.
  actual recall, and — only if they explain your history better than the
  defaults — saves them to `fsrs-weights.json`, which is loaded at startup. The
  scheduler math is now parameterised by a weight vector (defaults unchanged);
  it reports the before/after log-loss and declines to tune on too little
  history. Pure-Rust, no ML dependency.
- **FSRS lapse capture.** Leaving a *previously-solved* card that you attempted
  but couldn't re-solve now records an FSRS `Again`, collapsing its stability so
  it resurfaces soon — the signal FSRS most needs. `vault::record_lapse` rewrites
  only the note's scheduling frontmatter (the solve history is untouched — a
  lapse is not a solve), and scheduling now keys off the precomputed `due` date
  (recomputed on solve *and* lapse) via `review::overdue_days`, with the Leitner
  ladder still the fallback for pre-FSRS notes.
- **Review-event log (`reviews.jsonl`).** Every graded review (solve or lapse)
  appends one line to an append-only log in the data dir — a rebuildable,
  plain-text corpus (the vault stays the source of truth) that `mamba optimize`
  will fit the scheduler weights to.

## [0.6.0] — 2026-06-16

A verification-and-learning release: the tutor now *generates* engine-verified
problems, wrong answers explain themselves, physics answers are unit-checked,
scheduling moves to FSRS, a prerequisite curriculum guides what's next, and an
MCP server exposes the engine to other agents.

### Added
- **MCP server (`mamba mcp`).** Mamba now speaks the Model Context Protocol over
  stdio, exposing its verification engine and bank as tools so *other* AI agents
  can check math against a real engine — "Mamba as a verification backend". Tools:
  `verify` (algebraic/matrix/exact equivalence with unit + near-miss diagnostics),
  `solve` (against a bank problem by id), `get_problem` (without leaking the
  answer), `list_problems`, and `recommend_next`. The JSON-RPC protocol surface
  (`initialize`/`tools/list`/`tools/call`/`ping`) is hand-rolled over
  stdin/stdout — no async runtime, no network, no SDK dependency. Register with
  `claude mcp add mamba -- mamba mcp`.
- **Prerequisite curriculum.** A new `curriculum` module encodes a topic
  prerequisite DAG (Algebra → Calculus → ODEs; Machine Learning → Deep Learning;
  …). A topic *unlocks* once a third of each prerequisite's problems are solved,
  and `mamba next` now prefers unlocked topics over locked ones — so it won't
  push you into Deep Learning before Machine Learning — while never dead-ending
  (a locked topic is deprioritised, not hidden). `mamba path` prints the whole
  curriculum in prerequisite order with each topic's mastery bar and lock status
  (`🔒 needs Machine Learning`).
- **FSRS spaced-repetition scheduling.** A new `fsrs` module implements the
  Free Spaced Repetition Scheduler (v4.5): each card learns a *stability* and
  *difficulty*, and the next interval follows a power forgetting curve — so
  reviews are elapsed-time-aware and per-card, not a fixed Leitner ladder. The
  grade is derived from the verified outcome (a first-try solve is *Good*, a
  multi-attempt solve *Hard*). State persists in each note's frontmatter
  (`stability`/`difficulty_fsrs`/`due`), keeping the Markdown vault the single
  source of truth — no database. `r` review and `mamba next` use it; notes
  written before FSRS fall back to the old ladder until their next solve.

- **Dimensional analysis (units) for physics & engineering.** Problems now
  declare the physical `unit` their answer carries (a new optional `Problem`
  field; injected by the generator like recaps). When you append a unit, the
  router checks it is *dimensionally* consistent with the expected one before
  comparing values — `30 N` and `30 kg*m/s^2` both pass, `30 kg` is rejected
  (*"wrong units: expected N (force), but got kg (mass)"*). A new
  `engine::units` module parses base/derived SI units, compound forms (`m/s^2`,
  `N·m`), and SI prefixes. It's a dimensional check (not a unit conversion), so
  an omitted or unrecognized unit is lenient and SI-prefix slips aren't policed.
- **Near-miss diagnostics.** A wrong answer now carries an optional diagnosis of
  *how* it's wrong, shown as a `💡` line in the console (and `mamba solve`): off
  by a sign, off by a constant factor, the reciprocal, the transpose (or a
  row/column vector entered the wrong way), swapped real/imaginary parts, or off
  by one. Each pipeline grew a `diagnose` step that runs only on the Incorrect
  path — the symbolic one reuses the probe core to test the answer against the
  negative/rescaled/reciprocal of the expected expression, so it works for whole
  expressions, not just numbers. `Verdict::Incorrect` gained a `hint` field.
- **Verified problem generation (`Ctrl-G`).** A new `generate` module turns the
  tutor into a trustworthy problem factory — the one move only a tool with *both*
  an LLM and a verifier can make. The pipeline (off-thread, like the other
  workers): the local model **authors** a problem and its answer as JSON in the
  vein of the open one; the engine **parse-gates** the answer (must be valid
  Mamba syntax for its domain, with retries); then the model **independently
  re-solves** the problem and that solution is checked against the stated answer
  *through the engine* (algebraic equivalence). The result opens in a new
  *generated mode* to attempt, badged `✓ cross-verified` or `⚠ unverified`, and
  `Ctrl-S` saves a keeper into the bank (JSON + the live, sorted navigator).

### Changed
- **`Ctrl-G` now generates an engine-verified problem** instead of seeding a
  chat-only tutor exercise — it produces a real, attemptable, cross-checked
  problem (see above). `Problem` and its enums gained `Serialize` so generated
  problems can be written back to the bank.
- `vault::ProblemHistory` gained an optional `memory` (FSRS state), and `review`
  exposes `interval_for` — the one place "how long until this resurfaces" lives.

## [0.5.0] — 2026-06-16

Two new subject areas across all difficulties: Physics and Engineering Math push
the bank to 262 problems.

### Added
- **Physics and Engineering Math topics** (bank 199 → 262). Two new subject
  areas spanning all difficulties: mechanics, energy, waves, kinematics, and SHM
  for **Physics** (Newton's second law, SUVAT velocity/displacement, kinetic and
  potential energy, momentum, wave speed, symbolic velocity-from-position, SHM
  acceleration/velocity, half-life, thin-lens); and circuits, statics, and
  materials for **Engineering Math** (Ohm's law, resistive power, series/parallel
  resistors, RC time constant, capacitor energy, moment of a force, axial stress,
  Hooke's law, series-RL complex impedance, decibel gain, voltage divider, RC
  discharge rate). Nineteen new generator templates + six curated problems; each
  carries a recap (the Hard ones — SHM and RL impedance — get the expanded recap
  with a worked example and reference), and all pass `mamba --verify`.

## [0.4.0] — 2026-06-16

A full study loop and a sharper tutor: a study-session mode, an adaptive "next
problem", a tutor that generates fresh practice problems, semantic search by
meaning, a `mamba stats` dashboard, and an optional config file.

### Added
- **`mamba stats` shell command.** Prints the progress dashboard the `Ctrl-P`
  overlay shows — total solved out of the bank, current/longest day streak,
  per-domain and per-difficulty counts, a `solved/total` topic-mastery view with
  bars (least-mastered first), and the most recent solves — straight to the
  terminal, reading the same vault as the TUI.
- **Optional config file (`~/.config/mamba/config.toml`).** A new `config`
  module reads persistent defaults for the vault, problem bank, theme, and Ollama
  tutor/embedding models, so the `MAMBA_*` variables don't have to be exported
  each shell. Precedence is environment variable > config file > built-in
  default; a malformed file is reported and ignored, never fatal.
- **Semantic problem search (`/` then `Tab`).** The finder now has two modes:
  the existing fuzzy character match, and a new *semantic* mode that ranks
  problems by **meaning** using local `nomic-embed-text` embeddings — so “rate of
  change of a curve” surfaces the derivative problems even with no shared words.
  A new `semantic` module embeds each problem once (via the same local Ollama
  server as the tutor), caches the vectors to `embeddings.json` in the data dir
  (re-indexing automatically when a problem's text or the model changes), and
  ranks the query by cosine similarity on a worker thread so indexing never
  blocks the UI. New `MAMBA_EMBED_MODEL` env var; nothing leaves the machine.
- **“Generate a similar problem” tutor action (`Ctrl-G`).** One keystroke asks
  the local tutor to pose a fresh practice problem in the same vein as the open
  one — same type, topic, and difficulty, different numbers — for you to attempt
  and then have the tutor check. It's a *tutor exercise*, deliberately not added
  to the bank: Mamba's engine still owns verification, so this never introduces
  an unchecked answer.
- **Adaptive “next problem” (`n` / `mamba next`).** A new `recommend` module
  picks the single best problem to do right now, blending three signals from the
  vault: spaced repetition (most-overdue reviews first), per-topic mastery (ties
  break toward your weakest topic), and difficulty (easier first). When nothing
  is due it suggests *getting ahead* in the weakest topic, so there's always an
  answer. Press `n` in the TUI to jump straight to it (cursor moves, answer bar
  focused, rationale flashed in the console); `mamba next` prints the same pick
  with its `why` from the shell.
- **Study-session mode (`s`).** Press `s` to start a focused session over
  everything currently due for review: the navigator is filtered to that queue,
  a live `📚 solved/total · Nm` HUD shows progress, and each correct answer
  advances the counter. The session ends automatically when the queue is cleared
  (or with another `s`), printing a summary line to the console — e.g.
  `📚 Session ended — 4/6 solved in 12m`. If nothing is due, it says so instead
  of starting an empty session.
- **Per-problem recap (preliminary reading).** Every problem now carries a short
  recap of its subject matter — the key formula and idea — shown in the
  description pane under `📖 Recap` and toggled with `g` (on by default). Recaps
  are authored per concept in the generator (`RECAPS`, injected by problem id)
  and on the hand-authored bootstrap problems; a new optional `recap` field on
  the problem schema.
- **Expanded recaps for Hard problems.** Hard-difficulty problems get a longer
  recap (`RECAPS_HARD`) with a worked mini-example and a textbook reference —
  e.g. Cholesky, modular exponentiation, the softmax Jacobian, and Bayes' rule.

### Changed
- **The AI tutor panel is shown by default** (still toggled with `Ctrl-A`), so
  help is one keystroke (or a `?`-prefixed answer) away from the start.

## [0.3.0] — 2026-06-15

Bigger bank, sharper study tools: a `mamba review` shell command, a per-topic
mastery view, and five new subject areas push the bank to 199 problems across 18
topics.

### Added
- **Decompositions, Graph Theory, Complex Numbers, and ODEs topics** (bank
  159 → 199): matrix trace/rank and Cholesky factors; complete-graph edges,
  Cayley's spanning-tree count, and adjacency-matrix walk counts; complex
  multiplication, powers of i, and modulus; ODE characteristic roots,
  equilibria, and exponential solutions (`5*exp(-3*t)`). Twelve new generator
  templates + four curated problems; all pass `mamba --verify`.
- **Per-topic mastery view in the `Ctrl-P` overlay.** The topic section now shows
  `solved / total` coverage per topic (from the bank vs the vault) with a `█`/`░`
  bar — green when complete, yellow when started, dim when untouched — sorted
  **least-mastered first** so you can see what to study next.
- **Signals & DSP, Linear Programming, and Hypothesis Testing topics** (bank
  124 → 159): convolution length & sequences, Nyquist frequency, radix-2 FFT
  stages; box-constrained LP minima, standard-form variable counts, objective
  values; t-test/chi-square/two-sample degrees of freedom, z-scores, and exact
  standard errors. Ten new generator templates + four curated problems; all pass
  `mamba --verify`.
- **`mamba review` subcommand.** Lists the problems due for review from the
  shell — most-overdue first, each with its topic and `new` / `last DATE (N
  solves)` status — using the same spaced-repetition schedule and vault history
  as the TUI's `r` mode. Handy for a quick "what should I study?" without opening
  the TUI.

## [0.2.0] — 2026-06-15

The study-loop release: a local AI tutor, spaced-repetition review, a per-page
Quick Guide, and a 124-problem bank reaching into ML, convex optimization, deep
learning, probability, and statistics.

### Added
- **Quick Guide + tutor keymap recall.** A new `QUICKGUIDE.md` documents every
  keybinding, the CLI, the answer syntax, and configuration in one page. It's
  embedded into the tutor's system prompt at compile time, so you can ask the
  tutor "which key opens the stats overlay?" and it answers from the guide.
- **Tutor chat saved to the vault.** The tutor conversation is now scoped to the
  open problem (reset when you switch problems) and, on a correct solve, appended
  to that problem's vault note as a `## Tutor` section — so the help you got is
  archived alongside the solution.
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
