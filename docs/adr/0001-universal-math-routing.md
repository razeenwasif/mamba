# ADR 0001 — The Universal Math Routing Engine

- **Status:** Accepted
- **Date:** 2026-06-14
- **Deciders:** Razeen Wasif
- **Tags:** architecture, engine, mathematics, tui

## Context

Mamba is a local, terminal-based mathematics practice engine: browse a problem
bank, read a prompt, type an answer in the terminal, have it **mathematically
verified**, and archive solved problems as Markdown notes in a personal vault
(`~/OnyxVault/Mamba`). It is a sibling to `~/Kata` and reuses its proven shape —
a `ratatui` immediate-mode TUI, an `App` single source of truth, a declarative
JSON problem bank, and a Markdown/YAML vault exporter.

The hard part is *verification*. Unlike Kata (where a single Docker sandbox runs
code against fixed stdin/stdout test cases), correctness in mathematics is not
string equality and is not uniform across topics:

1. **Algebra/calculus** answers are *expressions*. `(x-3)(x+2)`, `x²-x-6`, and
   `x·x - x - 6` are the same answer; a string compare rejects two of the three.
   Correctness means **algebraic equivalence**.
2. **Linear algebra** answers are *scalars or matrices*. Correctness means
   **equal shape and equal elements within tolerance** — a fundamentally
   different operation, over a different data type.
3. **Number theory** answers are *exact integers*, often far beyond 64 bits
   (factorizations, modular powers, large gcds). Correctness means **exact
   equality with no overflow**, plus structural checks (e.g. "are these factors
   actually prime?").

Driving forces:

1. **Correctness per domain.** Each domain needs a verifier that models *its*
   notion of equality. One generic checker would be wrong for at least two of
   the three.
2. **Single-binary distribution.** Install/update must be `cargo install` or
   copying one file — no Python/SymPy runtime, no external CAS process.
3. **Extensibility.** Adding a fourth domain (probability, geometry, …) must be
   a local, additive change — not a rewrite of the input path.
4. **Tool-family consistency.** State/render separation, JSON problems, and the
   Markdown vault must match Kata so the two tools feel like one toolkit.

## Decision

**Adopt a Universal Routing Engine: one total dispatch point, three specialized
pipelines, one embedded crate per pipeline.**

The problem's `domain` field (a three-variant enum) is the routing key. A single
function — `engine::verify(&Problem, input) -> Verdict` — matches on it and
hands the answer to exactly one pipeline. Each pipeline owns its own parsing,
normalization, and comparison, and returns a uniform `Verdict`
(`Correct` | `Incorrect{detail}` | `ParseError{message, hint}`).

| Domain     | Pipeline (`src/engine/…`) | Embedded crate | Verifies by |
|------------|---------------------------|----------------|-------------|
| `Symbolic` | `symbolic/`               | **`meval`**    | Algebraic equivalence via randomized **numeric probing** — parse both sides to an AST, evaluate at several well-separated points, accept iff they agree everywhere (Schwartz–Zippel: distinct low-degree expressions can't agree at many points). |
| `Tensor`   | `tensor/`                 | **`ndarray`**  | Parse both sides to `Array2<f64>`; compare **shape first, then element-wise** within tolerance. Scalars are 1×1 matrices, so determinants and full matrices share one path. |
| `Exact`    | `exact/`                  | **`num-bigint`** (+`num-integer`/`num-traits`) | Arbitrary-precision integer comparison. For factorizations: reconstruct the product **and** verify each base is prime (deterministic Miller–Rabin). Nothing overflows. |

### Why embed *multiple* specialized math crates

A single math dependency cannot serve all three domains well:

- A CAS alone (e.g. `symbolica`) is excellent for symbolic work but is the wrong
  (and heavyweight) tool for "compare two `f64` matrices" or "is this 30-digit
  integer prime?".
- A numerics crate (`ndarray`) has no notion of symbolic equivalence.
- A bignum crate (`num-bigint`) has no notion of expressions or matrices.

Each crate is **best-in-class for its domain, pulls in no async runtime, and is
pure Rust** (no BLAS/LAPACK, no C toolchain), so the single-binary goal holds.
The cost — a larger dependency tree — is bounded because each pipeline is
isolated behind the `verify` seam and compiled once.

### Why numeric probing (not a full CAS) for the symbolic pipeline today

True symbolic equivalence is undecidable in general and, in practice, drags in a
large CAS dependency and (for `symbolica` specifically) license activation.
Randomized numeric probing gives correct answers for the polynomial/calculus
problems Mamba poses, with a vanishing false-positive rate, in a few lines. The
pipeline boundary is a clean `compile(&str) -> impl Fn(f64)->f64` seam, so a real
CAS can be swapped in later — for exact, multivariate, or trig-identity
verification — **without touching the router or any other pipeline**.

## Alternatives Considered

| Alternative | Why rejected |
|---|---|
| **One generic verifier (normalize-and-string-compare)** | Wrong by construction: it rejects `(x-3)(x+2)` vs `x²-x-6`, mishandles matrix tolerance, and overflows on large integers. The whole point is per-domain equality. |
| **Shell out to Python + SymPy/NumPy** | Correct and powerful, but reintroduces a heavyweight runtime dependency Mamba exists to avoid; per-verify process spawn adds latency and a failure mode (interpreter missing). |
| **`symbolica` as the symbolic engine now** | A genuine CAS and the natural future upgrade, but adds license-activation friction and weight for problems numeric probing already verifies. Kept behind the pipeline seam for later. |
| **`ndarray-linalg` for determinants** | Pulls in a system BLAS/LAPACK backend, breaking the pure-Rust single-binary promise. We compute the few operations we need directly and compare with plain `ndarray`. |
| **`malachite` instead of `num-bigint`** | Faster for very large integers, but heavier to build; `num-bigint` is pure Rust, ubiquitous, and more than fast enough for a practice tool. A drop-in swap if profiling ever demands it. |
| **Trait-object plugin registry (`Box<dyn Verifier>`)** | More "extensible" on paper, but a closed three-variant enum + `match` is simpler, exhaustively checked by the compiler, and just as additive for the foreseeable domain count. Revisit if domains become user-pluggable at runtime. |

## Consequences

**Positive**
- Each domain is verified by the right mathematics, behind one tiny, total
  dispatch point — the compiler enforces that every domain is handled.
- Adding a domain is purely additive: one `Domain` variant, one `engine/<x>/`
  module, one `match` arm. No existing pipeline changes.
- Pure-Rust dependencies preserve the single static-binary install.
- `Verdict` carries actionable feedback (witnessed mismatch points, parse
  hints), which the verification console renders directly.
- State/render separation, JSON problems, and the Markdown vault match Kata.

**Negative / accepted trade-offs**
- **Larger dependency tree** (three math crates). Bounded by the pipeline
  isolation and acceptable for the capability gained.
- **Numeric probing is probabilistic and currently single-variable (`x`).** False
  positives are astronomically unlikely for the posed problems, but it is not a
  proof of identity; multivariate and exact symbolic checking await the CAS
  upgrade behind the existing seam.
- **Float tolerance in the tensor pipeline** means answers are compared up to
  `1e-6`; exact-rational matrix answers would need a rational backend (future
  work), mirroring the `num-bigint` choice on the exact side.
- **Authoring burden:** every problem must ship a canonical `expected` answer in
  its domain's input syntax. This is data, not code — adding a problem never
  recompiles the binary.
