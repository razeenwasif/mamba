# ADR 0004 — Hybrid exact-rational / approximate tensor cells

- **Status:** Accepted
- **Date:** 2026-06-15
- **Deciders:** Razeen Wasif
- **Tags:** architecture, engine, tensor, linear-algebra
- **Relates to:** [ADR 0001](0001-universal-math-routing.md) (Universal Routing Engine)

## Context

ADR 0001 shipped the tensor pipeline comparing every matrix cell as an `f64`
within a `1e-6` tolerance, and explicitly listed *"exact-rational matrix answers
would need a rational backend (future work)"* as an accepted limitation.

That limitation bites for the natural linear-algebra answers that are exact
fractions — a `2×2` inverse is `(1/det)·adj`, giving entries like `3/5`, `-7/10`.
Forcing the user to type decimal approximations (`0.6`, `-0.7`) and accepting
them only within tolerance is both less precise (a near-miss could slip through)
and worse UX (you can't type the fraction you computed). At the same time, some
answers genuinely *are* irrational (eigenvalues, norms), where an exact
representation is impossible and tolerance is the only sane comparison.

## Decision

**Parse each cell into a hybrid [`Cell`] — exact rational *or* approximate
float — and compare accordingly.** (`num-rational`, pure-Rust, MIT/Apache — fits
the single-binary, no-C-toolchain constraints.)

- **Parsing by notation:**
  - an integer (`10`) or an `a/b` fraction (`-7/10`) → `Cell::Exact(Ratio<i64>)`,
  - a decimal or scientific literal (`0.5`, `1.41421356`) → `Cell::Approx(f64)`.
- **Comparison:**
  - both `Exact` → exact `==` (so `2/6` equals `1/3`, and an inverse verifies with
    **zero rounding error**),
  - otherwise → agreement within the existing float tolerance (so an
    author-stored irrational like `1.41421356` still matches a close decimal).

Notation carries the intent: a fraction asks to be checked exactly; a decimal
admits it is an approximation. Integers subsume the old behaviour, so every
existing problem keeps passing. Scalars remain `1×1` matrices, so determinants,
transposes, products, and inverses all share one comparison path.

## Alternatives Considered

| Alternative | Why rejected |
|---|---|
| **Keep float-only (status quo)** | Can't express exact fractions; near-misses can pass tolerance; users can't type the fraction they computed. |
| **Exact-rational only** | Can't represent irrational answers (eigenvalues, norms) at all; would reject any decimal. The hybrid keeps both. |
| **Treat decimals as exact rationals too** (`0.3333` → `3333/10000`) | Then `0.3333 ≠ 1/3` exactly would reject reasonable decimal entry, and long decimals overflow `i64`. Decimals-as-approximate is simpler and matches user intent. |
| **Arbitrary-precision rationals (`BigRational`)** | Unneeded for the matrix sizes a practice tool poses; `Ratio<i64>` is lighter. Easy to widen later if a problem needs it. |

## Consequences

**Positive**
- Exact-fraction matrix answers (inverses, solved systems) verify with no
  rounding error; equivalent fractions (`6/10` = `3/5`) are accepted.
- Irrational/decimal answers still work via tolerance — nothing regressed.
- Backward compatible: integer matrices are exact `Ratio`s and compare as before.
- One pure-Rust dependency (`num-rational`), consistent with the design.

**Negative / accepted trade-offs**
- Fraction numerator/denominator are `i64`; pathologically large fractions would
  overflow (not a concern for practice problems; widen to `BigRational` if ever).
- A decimal approximation of an exact fraction (`0.3333` for `1/3`) is judged
  *incorrect* — intentional, since the exact answer is the fraction; the problem
  hint tells users to enter fractions.
- The exact-vs-approximate split is decided by *notation*, not value, so `0.5`
  takes the tolerance path even though it is exactly `1/2` (harmless: it still
  matches `1/2`).
