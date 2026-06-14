# ADR 0002 — Off-thread (worker) verification

- **Status:** Accepted
- **Date:** 2026-06-15
- **Deciders:** Razeen Wasif
- **Tags:** architecture, concurrency, engine, tui

## Context

[ADR 0001](0001-universal-math-routing.md) established the Universal Routing
Engine: `engine::verify(&Problem, input) -> Verdict`. The first version called it
**synchronously** from `App::submit_answer` on the Enter key, so the immediate-
mode event loop blocked until verification returned.

Today's pipelines are fast (sub-millisecond probing, matrix compares, and
big-integer checks), so the block is imperceptible. But the architecture is
explicitly designed to host *heavier* verifiers behind the same seam — a real
CAS for symbolic identities, factorization of large semiprimes, exact-rational
linear algebra. Any of those can take hundreds of milliseconds to seconds, and a
synchronous call would freeze the entire UI (no scrolling, no quit, no redraw on
resize) for that whole time. We want the responsiveness guarantee to hold
*before* we add a slow pipeline, not after it bites.

Constraints:

- Keep the **single static binary** and the **synchronous, runtime-free**
  character of the app — no `tokio`, no async colouring of the call graph.
- Preserve the **immediate-mode invariant**: `ui::draw` is a pure projection of
  `App`, and all `App` mutation happens on the main thread in `on_key` / the
  loop. The render must never touch a half-updated state.

## Decision

**Run `engine::verify` on a detached worker thread and deliver its `Verdict`
back over an `mpsc` channel, drained once per main-loop tick** — the same
streaming-poll shape Kata uses for its agent.

- `submit_answer` clones the open `Problem` and the trimmed answer, spawns a
  `std::thread`, and stores the `Receiver<Verdict>` in a `VerifyJob` (which also
  captures the problem's open-time for the `time_spent` frontmatter). The worker
  computes the verdict and `send`s it; it never touches `App`.
- The main loop calls `App::poll_verify()` before each draw. `try_recv` is
  non-blocking: `Empty` ⇒ keep showing `Verifying…`; a value ⇒ record the
  verdict and, on success, export — **all App mutation stays on the main
  thread**. `Disconnected` (a panicked worker) surfaces as an error verdict
  rather than a permanent spinner.
- **One verification in flight at a time.** While a job exists, further Enter
  presses are ignored, and the loop polls at 16 ms instead of 100 ms so the
  result lands within a frame.
- **Navigation cancels.** Opening another problem drops the `VerifyJob`; the
  worker's `send` then fails harmlessly and the stale result is discarded, so a
  late verdict can never attach to the wrong problem.

Why threads and a channel (not async): verification is CPU-bound, fire-and-
forget, and one-at-a-time. A thread + `mpsc` models that exactly in the standard
library, with no executor, no `async fn` propagation, and no new dependency. The
channel boundary also keeps the worker incapable of mutating shared state, which
preserves the immediate-mode invariant by construction.

## Alternatives Considered

| Alternative | Why rejected |
|---|---|
| **Keep it synchronous** | Fine for today's fast pipelines, but a single slow future verifier freezes input, redraw, and quit. We want the guarantee before the slow pipeline lands. |
| **Deferred-blocking (paint `Verifying…`, then block — Kata's original sandbox style)** | Still blocks the UI during the call; only the *first* frame is responsive. A worker thread is barely more code and never blocks. |
| **`tokio` / async runtime** | Colours the whole call graph `async`, adds a heavyweight dependency and an executor, and buys nothing for a CPU-bound, one-at-a-time task. Contradicts the runtime-free design. |
| **A persistent worker + job queue** | Useful if we ever verify many answers concurrently (batch grading), but overkill for interactive single-answer submission. The `VerifyJob` seam leaves room to add it later. |

## Consequences

**Positive**
- The UI stays responsive (scroll, navigate, quit, resize) regardless of how
  long a verification takes — the responsiveness contract now holds for any
  future pipeline.
- The immediate-mode invariant is preserved: workers return data, the main
  thread owns all state changes.
- No new dependencies; the binary stays single-file and synchronous in spirit.
- Adds a real `Verifying…` state and `VERIFYING` badge, and correct
  cancellation-on-navigation semantics.

**Negative / accepted trade-offs**
- A thread spawn per submission. Negligible (microseconds) against any
  verification worth running off-thread; acceptable for interactive use.
- Results now arrive up to one poll interval late (≤16 ms while verifying).
  Imperceptible, and the cost of never blocking.
- Tests must pump `poll_verify` to observe a verdict (a small `submit_blocking`
  helper), since submission is no longer synchronous.
- Only one verification runs at a time by design; concurrent/batch verification
  would need the persistent-worker extension noted above.
