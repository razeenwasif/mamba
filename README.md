# 🐍 Mamba

A local, single-binary, terminal-based **mathematics practice engine**. Browse
problems, type an answer, and have it *mathematically verified* — then archive
solves as Markdown notes in a local vault. Sibling tool to [`~/Kata`](../Kata).

Mamba's defining idea is the **Universal Routing Engine**: a problem's domain
routes your answer to one of three specialized verification pipelines, each
backed by a crate chosen for that kind of mathematics.

| Domain     | Verifies                          | Pipeline / crate            |
|------------|-----------------------------------|-----------------------------|
| `Symbolic` | algebra & calculus (equivalence)  | `engine/symbolic` · `meval` |
| `Tensor`   | linear algebra (matrices/scalars) | `engine/tensor` · `ndarray` |
| `Exact`    | number theory (exact integers)    | `engine/exact` · `num-bigint` |

Verification runs on a **worker thread** (delivered back over an `mpsc` channel),
so even a slow check never freezes the UI — the console shows `Verifying…` while
input, scrolling, and quit stay live.

Design rationale: [ADR 0001 — Universal Math Routing](docs/adr/0001-universal-math-routing.md)
· [ADR 0002 — Off-thread Verification](docs/adr/0002-off-thread-verification.md)
· [ADR 0003 — Multivariate Symbolic](docs/adr/0003-multivariate-symbolic-probing.md)
· [ADR 0004 — Rational Tensor Cells](docs/adr/0004-rational-tensor-cells.md).

## Highlights

- **124+ problems** across Algebra, Calculus, Trigonometry, Linear Algebra,
  Number Theory, **Machine Learning, Convex Optimization, Deep Learning,
  Probability, and Statistics** — all verified end-to-end by `mamba --verify`.
- **Mathematical verification, not string matching**: multivariate algebraic
  equivalence (incl. `exp`/`ln`/trig), exact `BigRational` matrices, and
  arbitrary-precision number theory with Miller–Rabin primality.
- **A real study loop**: a `/` fuzzy finder, `t` topic filter, `r`
  spaced-repetition review, an `s` **study session** over the due queue (with a
  live progress HUD and an end-of-session summary), an `n` **adaptive “next
  problem”** (`mamba next`) that blends due-ness, topic mastery, and difficulty,
  a `^P` progress overlay (streaks, per-domain / per-difficulty bars, and a
  `solved/total` **topic-mastery** view), and a per-problem attempt-history peek.
- **A local AI tutor** (`^A`): a streaming chat with a local
  [Ollama](https://ollama.com) model, scoped to the open problem — private,
  offline, no API keys.
- **A Markdown vault**: each solve writes a note (with a `## History` table) and
  refreshes a rolling `_progress.md` dashboard — local-first, Obsidian-ready.
- **Batteries included**: a pure-Python generator (`make gen`) mass-authors
  problems with answers correct by construction, plus a scriptable
  `mamba solve <id> <answer>`.

## Install (run `mamba` from anywhere)

```sh
make install        # = cargo install --path . --force, then syncs the problem
                    #   bank into ~/.local/share/mamba/problems
```

This builds in release, drops `mamba` onto your PATH (`~/.cargo/bin`), and
installs the problem bank to the platform data dir, so the binary works from any
directory:

```sh
mamba               # launch the TUI from anywhere
mamba --version
mamba --help
mamba --check       # load the problem bank and report (no TUI) — a health check
mamba --verify      # run every problem's canonical answer through the engine
mamba solve <id> "<answer>"   # verify one answer from the shell (exit 0 iff correct)
mamba review        # list the problems due for review (spaced repetition)
mamba next          # recommend the single best problem to attempt next
```

Keys: `/` fuzzy-find · `n` next recommended problem · `t` cycle topic filter ·
`r` spaced-repetition review · `s` study session (work the due queue) · `g`
toggle the recap · `^A` AI tutor · `⇥` cycle focus · `↵` open/submit · `j`/`k`
navigate & scroll · `^P` progress/stats overlay · `esc` return to navigator ·
`^Q` quit. The sidebar groups problems by topic within each domain.

Tutor shortcuts: after a wrong answer, `^E` asks the tutor to explain it; `^G`
asks it to generate a fresh, similar practice problem to try (a tutor exercise —
the model poses and grades it, so the engine-verified bank stays ground truth);
and an answer that starts with `?` is sent to the tutor as a question (a bare `?`
asks for a hint) instead of being verified.

Full reference — keys, CLI, answer syntax, and config — is in
[**QUICKGUIDE.md**](QUICKGUIDE.md), which is also embedded in the tutor's
context, so you can just ask it "which key opens the stats overlay?".

### From a checkout, without installing

```sh
make run            # = cargo run --release  (uses ./problems)
```

## Development workflow

The standing loop for every feature:

```sh
edit  →  update docs (README + CHANGELOG)  →  make test  →  make install  →  git commit  →  git push
```

`make install` after each feature keeps the on-PATH `mamba` current. `make help`
lists all targets (`install`, `build`, `test`, `run`, `check`, `fmt`, `clean`).

## How verification works

- **Symbolic** — parses both your answer and the canonical one, then checks
  *algebraic equivalence* by evaluating at several probe points over **every free
  variable** (so `(x-3)(x+2)` ≡ `x^2-x-6`, `(x+y)^2` ≡ `x^2 + 2*x*y + y^2`, and
  `sin(x)^2 + cos(x)^2` ≡ `1` all pass). Write `*` between variables (`2*x*y`,
  not `2xy`). See [ADR 0003](docs/adr/0003-multivariate-symbolic-probing.md).
- **Tensor** — parses `1 2; 3 4`-style input into a matrix and compares shape
  then elements (a scalar is a 1×1 matrix). Cells are **exact rationals** when
  written as integers/fractions (so `3/5 -7/10; -1/5 2/5` verifies an inverse
  with no rounding, and `6/10` = `3/5`) and fall back to float tolerance for
  decimals. See [ADR 0004](docs/adr/0004-rational-tensor-cells.md).
- **Exact** — arbitrary-precision integers; factorizations are checked by
  reconstructing the product *and* verifying each factor is prime
  (deterministic Miller–Rabin).

## Layout

```
src/
├── main.rs              # thin binary + immediate-mode event loop
├── lib.rs               # module tree + architecture docs
├── app.rs               # App: single source of truth + on_key router
├── engine/
│   ├── mod.rs           # the Universal Router (verify) + Verdict
│   ├── symbolic/        # meval  — algebraic equivalence (numeric probe)
│   ├── tensor/          # ndarray — matrix/scalar comparison
│   └── exact/           # num-bigint — exact arithmetic + primality
├── storage/
│   ├── problem.rs       # Domain/Problem model + JSON loader
│   ├── vault.rs         # Markdown + YAML vault exporter (per-problem notes)
│   └── progress.rs      # rolling _progress.md dashboard (streaks, breakdowns)
└── ui/
    ├── mod.rs           # pure render: navigator · problem · console · answer bar
    ├── formula.rs       # ASCII → typeset math (^/_ scripts, [[…]] matrices)
    └── theme.rs         # role-based palette (MAMBA_THEME)
problems/                # declarative JSON problem bank (data, not code)
docs/adr/                # architecture decision records
tools/build_problems/    # pure-Python problem-bank generator (see its README)
```

## Generating problems

```sh
make gen                 # mass-generate problems, then verify them end-to-end
```

The generator (`tools/build_problems/generate.py`) computes each answer with
exact arithmetic (correct by construction) and emits it in Mamba syntax;
`mamba --verify` then feeds every `expected` back through the engine. See
[`tools/build_problems/README.md`](tools/build_problems/README.md).

## Environment

- `MAMBA_PROBLEMS` — override the problem-bank directory.
- `MAMBA_VAULT` — override the export vault (default `~/OnyxVault/Mamba`).
- `MAMBA_THEME` — `venom` (default), `midnight`, or `slate`.
- `MAMBA_OLLAMA_MODEL` — tutor model tag (default `gemma4:e2b-it-qat`).
- `MAMBA_OLLAMA_URL` — Ollama server URL (default `http://localhost:11434`).

## Tests

```sh
cargo test               # unit tests for all three pipelines + the router,
                         # plus a guard that every shipped problem accepts its
                         # own canonical answer
```
