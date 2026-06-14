# ADR 0003 — Multivariate symbolic verification by numeric probing (not a CAS)

- **Status:** Accepted
- **Date:** 2026-06-15
- **Deciders:** Razeen Wasif
- **Tags:** architecture, engine, symbolic, licensing
- **Relates to:** [ADR 0001](0001-universal-math-routing.md) (Universal Routing Engine)

## Context

[ADR 0001](0001-universal-math-routing.md) shipped the symbolic pipeline as a
**single-variable** numeric-probe checker (`meval`, binding only `x`) and named
a true CAS — `symbolica` — as the intended future upgrade for *multivariate*,
*exact*, and *trigonometric-identity* verification. This ADR is that upgrade
decision.

Two things forced a closer look before adding `symbolica`:

1. **License.** `symbolica` publishes to crates.io with `license: unknown` — it
   is **not** an OSI/permissive license but the proprietary *Symbolica License*:
   free only for hobbyist-scale, non-commercial use, gated behind a **license
   key**, and restricted beyond that. Mamba is **MIT-licensed**; embedding a
   proprietary, key-gated dependency into the default build is a license
   mismatch and an install-time obligation we don't want to impose on a user
   running a personal practice tool.
2. **Build weight.** `symbolica` pulls in `rug` (GMP/MPFR) and `mimalloc` — **C
   libraries** requiring a C toolchain (m4, a C compiler) — which breaks ADR
   0001's *single static binary, pure-Rust, no-C-toolchain* constraint.

Meanwhile the actual user-facing goal — verify answers in **more than one
variable** (and ideally trig/rational forms) — does **not** require a CAS.

## Decision

**Generalize the existing numeric-probe checker to be multivariate, and keep it
pure-Rust on `meval`. Do not embed `symbolica`.**

- **Free-variable detection.** Scan the (implicit-multiplication-normalized)
  source for maximal identifier runs, excluding function calls (an identifier
  immediately followed by `(`) and the built-in constants `pi`/`e`. The variable
  set is the **union** over both expressions, so an answer that wrongly drops or
  adds a variable is still caught.
- **Multivariate probing.** For each of `ROUNDS` (12) rounds, bind every free
  variable to a deterministic value via `meval`'s `Context`, then evaluate both
  sides with `eval_with_context` and compare within tolerance. Distinct
  variables get **distinct** values in each round (a golden-ratio stride), so
  `x*y` is distinguished from `x+y`; the assignment moves each round.
  [Schwartz–Zippel] makes agreement at all rounds overwhelming evidence of
  equivalence.
- **Bonus coverage for free.** Because `meval`'s default context supplies
  `sin`/`cos`/`tan`/`exp`/`ln`/… and `pi`/`e`, the same machinery verifies
  **trigonometric identities** (`sin(x)^2 + cos(x)^2 ≡ 1`) and rational forms —
  no extra code.
- **Authoring rule.** Variable–variable products must be written with an explicit
  `*` (`2*x*y`, not `2xy`), because `xy` is genuinely ambiguous with a two-letter
  variable name. The single-variable implicit cases (`2x`, `3x^2`, `(x-3)(x+2)`)
  still normalize automatically. Problem hints state this.

The pipeline boundary is unchanged: `symbolic::verify(expected, input) ->
Verdict`. This module remains the **equivalence seam** — a true CAS can still be
dropped in here later, behind the same signature, if exact/proof-grade
verification is ever needed and an acceptably-licensed engine exists.

## Alternatives Considered

| Alternative | Why rejected |
|---|---|
| **Embed `symbolica`** | Proprietary, key-gated license incompatible with MIT; drags in GMP + mimalloc (C), breaking the pure-Rust single-binary promise. Capability not required to meet the goal. |
| **Stay single-variable** | Fails the actual requirement (multivariate answers like `(x+y)^2`). |
| **Insert `*` between adjacent letters (`xy` → `x*y`)** | Ambiguous: it would also shatter legitimate multi-letter names (`theta` → `t*h*e*t*a`). Rejected in favour of the explicit-`*` authoring rule. |
| **Random probe points** | Marginally stronger per-round, but non-reproducible. Deterministic, well-separated points are reproducible and already astronomically safe at 12 rounds. |
| **A pure-Rust CAS instead of `symbolica`** | No mature pure-Rust CAS offers reliable equivalence checking today; numeric probing is the pragmatic, correct-in-practice choice. |

## Consequences

**Positive**
- Multivariate algebraic equivalence works now (`(x+y)^2 ≡ x^2 + 2*x*y + y^2`),
  plus trig identities and rational forms — at no dependency cost.
- Stays pure-Rust, MIT-clean, single-binary; no license key, no C toolchain.
- The CAS upgrade path is still open behind the unchanged `verify` seam.

**Negative / accepted trade-offs**
- Verification remains **probabilistic**, not a symbolic proof of identity (false
  positives astronomically unlikely for the posed expressions, but non-zero in
  principle).
- Float tolerance (`1e-7`) means exotic exact-but-near-coincident expressions are
  out of scope; this is the same trade-off the tensor pipeline accepts.
- Variable–variable products need an explicit `*`; surfaced via problem hints and
  the parser's hint message.
- Free-variable detection is a lexical heuristic; it treats `pi`/`e` as constants
  and anything before `(` as a function, which is correct for the supported
  grammar but would need revisiting if user-defined functions were added.

[Schwartz–Zippel]: https://en.wikipedia.org/wiki/Schwartz%E2%80%93Zippel_lemma
