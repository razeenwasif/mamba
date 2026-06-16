# ADR 0006 — Vault as source of truth (no database)

- **Status:** Accepted
- **Date:** 2026-06-16
- **Deciders:** Razeen Wasif
- **Tags:** architecture, storage, local-first
- **Relates to:** [ADR 0001](0001-universal-math-routing.md) (single-binary ethos),
  [ADR 0007](0007-fsrs-spaced-repetition.md) (where scheduling state lives)

## Context

Mamba accumulates several kinds of state: per-problem solve history, spaced-
repetition scheduling, semantic-search embeddings, a review-event log, and user
config. As these grew, the natural question arose — should Mamba adopt a real
database (SQLite) instead of files?

The constraint that decides it: the **Markdown vault is a feature, not an
implementation detail.** Each solve writes a human-readable note (YAML
frontmatter + the problem, the accepted answer, a history table, and the tutor
transcript) into the user's Onyx/Obsidian vault. That data being *plain text the
user owns* — browsable in a note app, diffable in git, portable — is a core part
of what distinguishes Mamba from a black-box flashcard tool.

## Decision

**Keep the Markdown vault as the single source of truth; do not add a database.**

- **Scheduling state lives in note frontmatter.** FSRS memory
  (`stability`/`difficulty_fsrs`/`reviewed`/`due`) is read from and written to
  each problem's `.md` note. No separate scheduling store to keep in sync.
- **Every other store is a *derived, rebuildable cache*, never the source of
  truth.** `_progress.md` (the dashboard), `embeddings.json` (semantic search),
  and `reviews.jsonl` (the FSRS optimisation corpus) are all reconstructable
  from the vault (or from re-embedding/re-deriving) and safe to delete.
- **Read by scanning.** The progress dashboard, the review filter, and the
  recommender scan the vault's notes and aggregate on demand. At the current
  scale (hundreds of problems, single-user, day-granularity scheduling) this is
  single-digit milliseconds — there is no measured performance problem to solve.
- **Plain-text, pure-Rust formats only** (Markdown/YAML, JSON, JSONL, TOML),
  consistent with the no-C-dependency stance of [ADR 0003](0003-multivariate-symbolic-probing.md).

## Alternatives Considered

| Alternative | Why rejected |
|---|---|
| **Embedded SQLite (`rusqlite`)** | Makes the data opaque (a binary blob, not a note you can open/diff), and bundles the SQLite **C** library — the same dependency stance [ADR 0003] rejected. No performance pain justifies it. |
| **Pure-Rust embedded KV (`redb`/`sled`)** | Avoids the C dependency but the store is still binary/opaque, losing the Obsidian-ready, git-friendly property that is the point. |
| **Database as the source of truth, vault as an export** | Inverts the model: the user no longer *owns* their history as files; an export can drift from the canonical store. |
| **Full event sourcing for everything** | Over-engineered for a single-user practice tool; the frontmatter already holds the only state scheduling needs at read time. |

## Consequences

**Positive**
- The headline "your practice history is plain Markdown you own" property holds.
- No database, no migrations, no C dependency; the binary stays single-file.
- Derived caches can be deleted/rebuilt freely; corruption is non-fatal.

**Negative / accepted trade-offs**
- **Read-by-scanning** rather than indexed queries. Fine now; if the bank ever
  reached tens of thousands of notes and aggregation became slow, the escape
  hatch is a *derived* index (rebuildable with a `reindex`), never moving the
  source of truth.
- **FSRS parameter optimisation needs the full review sequence**, which the
  frontmatter (latest state only) doesn't keep — so a `reviews.jsonl` *event
  log* was added. It is explicitly a derived corpus, not the source of truth,
  and is plain-text JSONL (not SQLite) to honour this decision.
