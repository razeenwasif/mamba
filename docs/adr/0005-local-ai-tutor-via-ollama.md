# ADR 0005 — Local AI tutor via Ollama

- **Status:** Accepted
- **Date:** 2026-06-15
- **Deciders:** Razeen Wasif
- **Tags:** architecture, agent, ai, tui
- **Relates to:** [ADR 0002](0002-off-thread-verification.md) (off-thread workers)

## Context

Mamba verifies answers but doesn't *teach*. A tutor that can give a hint,
explain the method, or say why an attempt is wrong — scoped to the open problem
— turns the tool from a checker into a study aid. The constraints that shaped
the rest of Mamba apply here too: **local-first, offline-capable, no API keys,
no async runtime, and the UI must never block.**

## Decision

**Embed a streaming chat client for a local [Ollama] server**, mirroring the
verification worker's design.

- **Local models only.** Mamba talks to `http://localhost:11434` (Ollama). No
  cloud API, no key, no data leaving the machine — consistent with the
  Markdown-vault, single-binary ethos. Default model `gemma4:12b-it-qat`;
  both the URL and model are overridable via `MAMBA_OLLAMA_URL` /
  `MAMBA_OLLAMA_MODEL`.
- **Off-thread streaming.** `agent::stream_chat` spawns a worker thread that
  POSTs to `/api/chat` (`stream: true`) and sends each token back over an
  `mpsc` channel as an `AgentEvent` (`Token` / `Done` / `Error`). The main loop
  drains it each tick via `App::poll_agent` — the same shape as `poll_verify`,
  so the immediate-mode UI keeps redrawing while the model generates.
- **`ureq` (blocking HTTP), not `reqwest`/`tokio`.** The request already runs on
  its own thread, so a blocking client is simpler and keeps the app free of an
  async runtime — the same reasoning Kata used.
- **Scoped, hint-first prompting.** The system prompt includes the open
  problem's statement, difficulty/domain/topic, and the user's current answer
  attempt, and instructs the model to prefer hints and method over giving the
  full answer. A right-hand panel (`Ctrl-A`) shows the streaming conversation.
- **Graceful degradation.** If Ollama isn't running or the model isn't pulled,
  the worker sends an `Error` event that's shown in the panel — the rest of
  Mamba is unaffected. The dependency is entirely optional.

## Alternatives Considered

| Alternative | Why rejected |
|---|---|
| **A cloud LLM API (OpenAI/Anthropic/…)** | Needs an API key, a network round-trip to a third party, and sends the user's work off-machine — against the local-first design. |
| **`reqwest` + `tokio` streaming** | Pulls an async runtime into an otherwise-synchronous app for no benefit; the worker thread already gives concurrency. |
| **Bundle/run a model in-process (`llama.cpp` bindings, Candle)** | Much larger build, GPU/runtime complexity, and model-weight management — Ollama already solves serving and the user runs it. |
| **No tutor (verification only)** | Simpler, but misses the teaching loop the attempt-history and review features point toward. |

## Consequences

**Positive**
- Private, offline, key-free help scoped to the current problem.
- Reuses the proven worker-thread + `mpsc` + tick-drain pattern; the UI never
  blocks while the model streams.
- Model/URL are configurable; any Ollama model works.
- Optional and self-contained — failure surfaces in the panel, nothing else
  breaks.

**Negative / accepted trade-offs**
- **Runtime dependency on Ollama** for the tutor (only). Mitigated by graceful
  degradation and the clear error message.
- **One new dependency** (`ureq`, pure-Rust). Acceptable; no async runtime.
- The tutor's quality is the local model's quality; the qat 12B default is a
  balance of capability and footprint, swappable via `MAMBA_OLLAMA_MODEL`.
- A live integration test exists but is `#[ignore]`d (needs a running server),
  so CI doesn't depend on Ollama.

[Ollama]: https://ollama.com
