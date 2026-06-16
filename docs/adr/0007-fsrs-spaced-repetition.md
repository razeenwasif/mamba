# ADR 0007 — FSRS spaced-repetition scheduling

- **Status:** Accepted
- **Date:** 2026-06-16
- **Deciders:** Razeen Wasif
- **Tags:** architecture, scheduling, learning, fsrs
- **Relates to:** [ADR 0006](0006-vault-as-source-of-truth.md) (state lives in
  frontmatter)

## Context

Mamba's first scheduler was a fixed Leitner ladder: a problem resurfaced after
1, 3, 7, 16, 35, then 90 days, keyed only on its solve count. That ignores *when*
a card was actually reviewed (early vs. late) and treats every card as equally
hard. Modern spaced repetition (FSRS, as adopted by Anki) models memory per-card
and schedules markedly better, which directly serves Mamba's learning loop.

## Decision

**Adopt FSRS-4.5 as the scheduler, with state in the vault and optional
per-user weight optimisation.**

- **Per-card memory.** Each card learns a *stability* `S` and *difficulty* `D`.
  Recall probability follows a power forgetting curve
  `R(t) = (1 + FACTOR·t/S)^DECAY`; the next interval is the `t` at which `R`
  falls to the target retention (0.9). Elapsed-time-aware and per-card.
- **Grades from the verified outcome.** A first-try solve is `Good`, a
  multi-attempt solve `Hard`, and leaving a previously-solved card unsolved is a
  lapse (`Again`) — so the scheduler is driven by Mamba's *real* verdicts, not
  self-assessment.
- **State in note frontmatter** (`stability`/`difficulty_fsrs`/`reviewed`/`due`),
  per [ADR 0006] — no database. `due` is the authoritative next-review date,
  recomputed on every solve *and* lapse.
- **Hand-rolled, pure-Rust.** The 17-parameter FSRS-4.5 update is ~40 lines; the
  formulas are the contract and are covered by *behavioural* tests (success grows
  `S`, lapses shrink it, `R = 0.9` at `t = S`) rather than exact-value tests, so
  the scheduler stays correct across re-tuning.
- **Optional personalisation.** `mamba optimize` fits the weights to the user's
  `reviews.jsonl` by clamped numerical-gradient descent on the log-loss of
  predicted vs. actual recall, persisting them only if they beat the defaults.
  Until then, the published FSRS-4.5 defaults are used.
- **Backward compatible.** Notes written before FSRS (no memory) fall back to the
  Leitner ladder until their next solve.

## Alternatives Considered

| Alternative | Why rejected |
|---|---|
| **Keep the Leitner ladder** | Simple, but one-size-fits-all and elapsed-time-blind — exactly FSRS's advantage. |
| **SM-2 (classic Anki)** | Better than Leitner, but FSRS is the modern successor with a principled forgetting-curve model and a clean optimisation story. |
| **A third-party FSRS / ML crate (`fsrs-rs`, `candle`)** | Pulls a heavy dependency (and, for training, an autodiff/ML stack) into a tool whose whole ethos is pure-Rust and dependency-light; the update and a numerical-gradient fit are small to write directly. |
| **Self-graded reviews (Again/Hard/Good/Easy buttons)** | Cleaner FSRS signal, but Mamba's identity is *verified*, not self-assessed; deriving the grade from the engine's verdict keeps that. |

## Consequences

**Positive**
- Reviews are scheduled per-card and elapsed-time-aware; lapses collapse
  stability so forgotten material resurfaces quickly.
- No new runtime dependency; state stays in the human-readable vault.
- Optionally personalised to the user's own forgetting curve.

**Negative / accepted trade-offs**
- **Default weights aren't validated against the user** until `mamba optimize`
  runs (which needs a meaningful review history); the defaults are a sensible
  prior in the meantime.
- **`Easy` is not yet produced** (no "too easy" path), and the optimiser is a
  best-effort numerical fit, not the reference trainer — acceptable for a
  single-user tool, and revisitable.
