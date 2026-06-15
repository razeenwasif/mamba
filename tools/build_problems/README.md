# Problem-bank generator

`generate.py` mass-authors `problems/*.json` files in Mamba's schema, computing
each problem's canonical `expected` answer **arithmetically** so it is *correct by
construction*. It is pure Python standard library — no SymPy/NumPy — mirroring the
self-contained Python tooling in `~/Kata`.

## Use

```sh
make gen                                   # generate into ./problems, then verify
# or directly:
python3 tools/build_problems/generate.py --count 5 --out problems --seed 0
mamba --verify problems                    # end-to-end check against the engine
```

Flags:

| flag | default | meaning |
|------|---------|---------|
| `--count N` | `2` | problems generated per template |
| `--out DIR` | `<repo>/problems` | output directory |
| `--seed S` | `0` | RNG seed — deterministic, so re-running overwrites the same files |

Generated files are named `gen-<template>-<index>.json`, so they sit alongside
the hand-authored bank and are easy to spot (and `git rm 'problems/gen-*.json'`
to clear).

## Templates

The generator emits a curated `FIXED` set once each — the canonical
machine-learning and convex-optimization problems (sigmoid/tanh/softplus/MSE/
cross-entropy gradients, log-barrier, Hessians, Gram matrix, …) — plus these
parametrized templates `--count` times each:

| template | domain | answer computed by |
|----------|--------|--------------------|
| `expand-binomial` | Symbolic | `(x+a)(x+b)` → expanded quadratic |
| `power-derivative` | Symbolic | power rule `d/dx[c·xⁿ]` |
| `ridge-gradient` | Symbolic | `(1+λ)·w − t` (ML) |
| `determinant` | Tensor | `ad − bc` |
| `transpose` | Tensor | index swap |
| `inverse` | Tensor | `(1/det)·adj`, exact `Fraction`s |
| `gradient-linear` | Tensor | `∇(aᵀx) = a` (convex opt) |
| `hessian-quadratic` | Tensor | `[[2a, b], [b, 2c]]` (convex opt) |
| `eigenvalues` | Tensor | diagonal of a triangular matrix |
| `linear-system` | Tensor | Cramer's rule, exact `Fraction`s |
| `matmul-grad-shape` | Tensor | backprop weight-gradient shape (ML) |
| `attention-shape` | Tensor | attention output shape (ML) |
| `gd-step` | Tensor | gradient-descent update `x(1−2α)` (ML) |
| `lagrangian` | Symbolic | `∂L/∂x = 2a·x + λ` (convex opt) |
| `psd-check` | Exact | 2×2 PSD test → `1`/`0` (convex opt) |
| `layernorm-mean` | Tensor | activation mean (DL) |
| `layernorm-variance` | Tensor | biased activation variance (DL) |
| `dice-sum` | Tensor | two-dice P(sum) fraction (Probability) |
| `expected-value` | Tensor | weighted E[X] fraction (Statistics) |
| `variance` | Tensor | discrete Var(X) fraction (Statistics) |
| `combination` | Exact | `C(n, k)` (Probability) |
| `permutation` | Exact | `P(n, k)` (Probability) |
| `factorial` | Exact | `n!` (Probability) |
| `factorization` | Exact | trial division → `2^3 * 3^2 * 5` |
| `modexp` | Exact | `pow(base, exp, mod)` |
| `gcd` | Exact | Euclid (`math.gcd`) |
| `lcm` | Exact | `a*b / gcd(a, b)` |
| `modular-inverse` | Exact | `pow(a, -1, m)` |
| `conv-output` | Exact | `floor((L−K+2P)/S)+1` (DL) |
| `linear-params` | Exact | `a·b + b` (DL) |
| `conv-params` | Exact | `C_out·(C_in·K² + 1)` (DL) |

The `FIXED` curated problems are authored as literal dicts in `generate.py`; the
materialized JSON in `problems/` is the committed, browseable data.

## Why answers are trustworthy

Two independent guarantees:

1. **Correct by construction** — the generator computes the answer with exact
   arithmetic (Python `int`/`Fraction`), never guesses.
2. **Verified end-to-end** — `mamba --verify` feeds every problem's `expected`
   back through the real engine (the same path `tests/bank_is_solvable.rs`
   guards), catching any mismatch between the emitted string and Mamba's input
   syntax. The exit code is non-zero on any failure, so it gates CI and `make gen`.

## Adding a template

Write a `t_<name>(rng, i) -> dict` returning a problem dict (see existing ones),
add it to `TEMPLATES`, and emit `expected` in **Mamba syntax**: `^` for powers,
explicit `*`, `*` between variables, `a/b` fractions, and the factored form
(`2^3 * 3^2 * 5`) for `Factorization` problems. Then `make gen` and check the
verify output.

`prompt` may be written in plain ASCII: `^`/`_` become super/subscripts and a
line that is wholly `[[a, b], [c, d]]` renders as a bracketed matrix (see
`src/ui/formula.rs`). No need to hand-embed Unicode.
