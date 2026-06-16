# ADR 0008 — MCP server (verification engine as a tool)

- **Status:** Accepted
- **Date:** 2026-06-16
- **Deciders:** Razeen Wasif
- **Tags:** architecture, mcp, interop, agent
- **Relates to:** [ADR 0001](0001-universal-math-routing.md) (the engine being
  exposed), [ADR 0005](0005-local-ai-tutor-via-ollama.md) (no async runtime)

## Context

Mamba's verification engine is its most distinctive asset: it *checks* math
answers (algebraic equivalence, exact matrices, big-integer number theory) rather
than string- or LLM-matching them. Other AI agents have no reliable way to verify
their own math. Exposing the engine over the [Model Context Protocol] turns Mamba
into a reusable **verification backend** — a small surface, high leverage.

## Decision

**Ship `mamba mcp`: an MCP server over stdio, with a hand-rolled JSON-RPC loop.**

- **Transport: stdio, newline-delimited JSON-RPC 2.0.** `mamba mcp` reads one
  request per line from stdin and writes one response per line to stdout. The
  protocol surface is tiny — `initialize`, `tools/list`, `tools/call`, `ping` —
  so it is implemented directly (~200 lines) rather than via an SDK.
- **No async runtime, no SDK.** A synchronous stdin loop keeps the binary
  pure-Rust and dependency-light, the same reasoning as [ADR 0005]'s choice of
  blocking `ureq` over `reqwest`/`tokio`.
- **Tools.** `verify` (check an answer against an expected one — honours
  `answer_kind`, unit dimensional checks, and returns the near-miss hint),
  `solve` (verify against a bank problem by id), `get_problem` (the statement
  *without* the answer, so it stays a real exercise), `list_problems`, and
  `recommend_next`.
- **MCP conventions.** Tool results are the structured verdict as JSON text;
  tool-level failures return `isError` results (not JSON-RPC errors); the
  `initialize` reply echoes the client's `protocolVersion` for compatibility.

## Alternatives Considered

| Alternative | Why rejected |
|---|---|
| **The official Rust MCP SDK (`rmcp`)** | Pulls in `tokio` and a large async surface for a four-method server — against the synchronous, dependency-light design ([ADR 0005]). |
| **A REST/HTTP server** | Needs a port, a client, and an HTTP stack; MCP stdio is the native way agent hosts (Claude Code, frameworks) discover and call tools, with zero networking. |
| **No interop (TUI/CLI only)** | Leaves the engine — the unique asset — usable only from Mamba itself. |
| **Expose `expected` from `get_problem`** | Would leak answers; the engine is for *verifying*, so agents use `solve`/`verify` to check, not to read the key. |

## Consequences

**Positive**
- Any MCP client can use Mamba's real engine: `claude mcp add mamba -- mamba mcp`.
- Pure-Rust, synchronous, no new dependency; reuses `engine::verify` and the bank
  unchanged.
- Positions Mamba as infrastructure (a verification backend), not just an app.

**Negative / accepted trade-offs**
- **Hand-rolled protocol** must track the MCP spec by hand; mitigated by the
  minimal surface and an end-to-end stdio round-trip test.
- The server is **read-only** w.r.t. the vault (it verifies and recommends; it
  does not record solves), so an agent's checks don't affect the user's
  scheduling — intentional, but worth noting.

[Model Context Protocol]: https://modelcontextprotocol.io
