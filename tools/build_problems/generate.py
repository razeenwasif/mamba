#!/usr/bin/env python3
"""Mamba problem-bank generator.

Mass-authors `problems/*.json` files in Mamba's schema, computing each problem's
canonical ``expected`` answer arithmetically so it is *correct by construction*.
Pure Python standard library — no SymPy/NumPy — mirroring the self-contained
Python tooling in ``~/Kata``.

Every generated answer is emitted in Mamba's own input syntax (``^`` for powers,
explicit ``*``, ``a/b`` fractions). Run ``mamba --verify <out-dir>`` afterwards
(``make gen`` does both) to confirm the engine accepts every one end-to-end.

Usage:
    python3 tools/build_problems/generate.py [--count N] [--out DIR] [--seed S]
"""

from __future__ import annotations

import argparse
import json
import random
from fractions import Fraction
from math import comb, factorial, gcd, isqrt, lcm, perm
from pathlib import Path

# --------------------------------------------------------------------------
# Formatting helpers (produce Mamba-syntax strings)
# --------------------------------------------------------------------------


def format_polynomial(terms: list[tuple[int, str]]) -> str:
    """Render ``[(coeff, monomial), ...]`` as e.g. ``x^2 - x - 6``.

    ``monomial`` is ``""`` for a constant, ``"x"``, ``"x^2"``, …. Zero-coefficient
    terms are dropped; a unit coefficient on a monomial is elided.
    """
    parts: list[str] = []
    for coeff, mono in terms:
        if coeff == 0:
            continue
        mag = abs(coeff)
        if mono == "":
            token = str(mag)
        elif mag == 1:
            token = mono
        else:
            token = f"{mag}*{mono}"
        if not parts:
            parts.append(("-" if coeff < 0 else "") + token)
        else:
            parts.append(f"{'-' if coeff < 0 else '+'} {token}")
    return " ".join(parts) if parts else "0"


def signed(n: int) -> str:
    """`` + 2`` / `` - 3`` for inline prompt rendering of ``(x ± n)``."""
    return f"+ {n}" if n >= 0 else f"- {abs(n)}"


def monomial(power: int) -> str:
    """`` ``→constant, ``x``, ``x^k`` for an exponent."""
    if power == 0:
        return ""
    if power == 1:
        return "x"
    return f"x^{power}"


def frac(f: Fraction) -> str:
    """Mamba cell syntax for a fraction: ``n`` if integral, else ``n/d``."""
    return str(f.numerator) if f.denominator == 1 else f"{f.numerator}/{f.denominator}"


def factorize(n: int) -> dict[int, int]:
    """Prime factorization of ``n >= 2`` by trial division → ``{prime: exponent}``."""
    factors: dict[int, int] = {}
    d = 2
    while d * d <= n:
        while n % d == 0:
            factors[d] = factors.get(d, 0) + 1
            n //= d
        d += 1
    if n > 1:
        factors[n] = factors.get(n, 0) + 1
    return factors


def factor_string(factors: dict[int, int]) -> str:
    """`` {2:3, 3:2, 5:1} `` → ``2^3 * 3^2 * 5`` (Mamba factorization syntax)."""
    return " * ".join(
        f"{p}^{e}" if e > 1 else str(p) for p, e in sorted(factors.items())
    )


# --------------------------------------------------------------------------
# Templates — each returns a Mamba problem dict with a verified `expected`
# --------------------------------------------------------------------------


def t_expand_binomial(rng: random.Random, i: int) -> dict:
    a = rng.choice([n for n in range(-9, 10) if n != 0])
    b = rng.choice([n for n in range(-9, 10) if n != 0])
    expected = format_polynomial([(1, "x^2"), (a + b, "x"), (a * b, "")])
    return {
        "id": f"gen-expand-binomial-{i}",
        "title": "Expand a Binomial",
        "domain": "Symbolic",
        "topic": "Algebra",
        "difficulty": "Easy",
        "prompt": f"Expand into standard form:\n\n    (x {signed(a)})(x {signed(b)})",
        "expected": expected,
        "answer_kind": "Scalar",
        "hint": "use ^ for powers and * for products, e.g. x^2 - x - 6",
    }


def t_power_derivative(rng: random.Random, i: int) -> dict:
    c = rng.randint(2, 9)
    n = rng.randint(2, 5)
    expected = format_polynomial([(c * n, monomial(n - 1))])
    return {
        "id": f"gen-power-derivative-{i}",
        "title": "Derivative of a Power",
        "domain": "Symbolic",
        "topic": "Calculus",
        "difficulty": "Easy",
        "prompt": f"Differentiate with respect to x:\n\n    d/dx [ {c}x^{n} ]",
        "expected": expected,
        "answer_kind": "Scalar",
        "hint": "power rule: d/dx[a*x^n] = a*n*x^(n-1)",
    }


def t_determinant_2x2(rng: random.Random, i: int) -> dict:
    a, b, c, d = (rng.randint(-9, 9) for _ in range(4))
    det = a * d - b * c
    return {
        "id": f"gen-determinant-{i}",
        "title": "Determinant of a 2×2",
        "domain": "Tensor",
        "topic": "Linear Algebra",
        "difficulty": "Easy",
        "prompt": f"Find the determinant:\n\n    [[{a}, {b}], [{c}, {d}]]",
        "expected": str(det),
        "answer_kind": "Scalar",
        "hint": "for [[a,b],[c,d]], det = ad - bc",
    }


def t_transpose(rng: random.Random, i: int) -> dict:
    rows, cols = rng.randint(2, 3), rng.randint(2, 3)
    m = [[rng.randint(-9, 9) for _ in range(cols)] for _ in range(rows)]
    transposed = [[m[r][c] for r in range(rows)] for c in range(cols)]
    expected = "; ".join(" ".join(str(x) for x in row) for row in transposed)
    literal = "[" + ", ".join("[" + ", ".join(str(x) for x in row) + "]" for row in m) + "]"
    return {
        "id": f"gen-transpose-{i}",
        "title": "Transpose a Matrix",
        "domain": "Tensor",
        "topic": "Linear Algebra",
        "difficulty": "Medium",
        "prompt": f"Write the transpose A^T of:\n\n    {literal}",
        "expected": expected,
        "answer_kind": "Scalar",
        "hint": "rows separated by ';', elements by spaces, e.g. 1 4; 2 5",
    }


def t_inverse_2x2(rng: random.Random, i: int) -> dict:
    while True:
        a, b, c, d = (rng.randint(-6, 6) for _ in range(4))
        det = a * d - b * c
        if det != 0:
            break
    inv = [[Fraction(d, det), Fraction(-b, det)], [Fraction(-c, det), Fraction(a, det)]]
    expected = "; ".join(" ".join(frac(x) for x in row) for row in inv)
    return {
        "id": f"gen-inverse-{i}",
        "title": "Inverse of a 2×2",
        "domain": "Tensor",
        "topic": "Linear Algebra",
        "difficulty": "Hard",
        "prompt": f"Find the inverse:\n\n    [[{a}, {b}], [{c}, {d}]]\n\nEnter exact fractions.",
        "expected": expected,
        "answer_kind": "Scalar",
        "hint": "inverse of [[a,b],[c,d]] is (1/(ad-bc))*[[d,-b],[-c,a]]; use fractions",
    }


def t_prime_factorization(rng: random.Random, i: int) -> dict:
    # A product of 2–3 small primes with small exponents → an interesting composite.
    primes = [2, 3, 5, 7, 11, 13]
    n = 1
    chosen: dict[int, int] = {}
    for p in rng.sample(primes, rng.randint(2, 3)):
        e = rng.randint(1, 3)
        chosen[p] = e
        n *= p**e
    expected = factor_string(chosen)
    return {
        "id": f"gen-factorization-{i}",
        "title": "Prime Factorization",
        "domain": "Exact",
        "topic": "Number Theory",
        "difficulty": "Medium",
        "prompt": f"Find the prime factorization of:\n\n    {n}",
        "expected": expected,
        "answer_kind": "Factorization",
        "hint": "format like 2^3 * 3^2 * 5",
    }


def t_modular_exponentiation(rng: random.Random, i: int) -> dict:
    base = rng.randint(2, 20)
    exp = rng.randint(10, 200)
    mod = rng.choice([7, 11, 13, 17, 19, 23, 97])
    expected = str(pow(base, exp, mod))
    return {
        "id": f"gen-modexp-{i}",
        "title": "Modular Exponentiation",
        "domain": "Exact",
        "topic": "Number Theory",
        "difficulty": "Hard",
        "prompt": f"Compute the least non-negative residue:\n\n    {base}^{exp} mod {mod}",
        "expected": expected,
        "answer_kind": "Scalar",
        "hint": "reduce the exponent with Fermat's little theorem; enter one integer",
    }


def t_gcd(rng: random.Random, i: int) -> dict:
    a = rng.randint(100, 9999)
    b = rng.randint(100, 9999)
    return {
        "id": f"gen-gcd-{i}",
        "title": "Greatest Common Divisor",
        "domain": "Exact",
        "topic": "Number Theory",
        "difficulty": "Easy",
        "prompt": f"Find the greatest common divisor:\n\n    gcd({a}, {b})",
        "expected": str(gcd(a, b)),
        "answer_kind": "Scalar",
        "hint": "the Euclidean algorithm; enter a single integer",
    }


def t_gradient_linear(rng: random.Random, i: int) -> dict:
    n = rng.randint(2, 3)
    a = [rng.choice([k for k in range(-6, 7) if k != 0]) for _ in range(n)]
    terms = " + ".join(f"{c}*x{j}" for j, c in enumerate(a, start=1)).replace("+ -", "- ")
    return {
        "id": f"gen-gradient-linear-{i}",
        "title": "Gradient of a Linear Form",
        "domain": "Tensor",
        "topic": "Convex Optimization",
        "difficulty": "Easy",
        "prompt": (
            f"Find the gradient ∇f of the linear function:\n\n    f(x) = {terms}\n\n"
            "Enter it as a column vector (one component per row)."
        ),
        "expected": "; ".join(str(c) for c in a),
        "answer_kind": "Scalar",
        "hint": "∇(aᵀx) = a; one component per row, e.g. 2; -3",
    }


def t_hessian_quadratic(rng: random.Random, i: int) -> dict:
    a = rng.randint(1, 5)
    c = rng.randint(1, 5)
    b = rng.choice([k for k in range(-4, 5) if k != 0])
    f = f"{a}*x1^2 + {b}*x1*x2 + {c}*x2^2".replace("+ -", "- ")
    return {
        "id": f"gen-hessian-quadratic-{i}",
        "title": "Hessian of a Quadratic",
        "domain": "Tensor",
        "topic": "Convex Optimization",
        "difficulty": "Medium",
        "prompt": f"Find the Hessian of:\n\n    f(x1, x2) = {f}",
        "expected": f"{2 * a} {b}; {b} {2 * c}",
        "answer_kind": "Scalar",
        "hint": "the matrix of second partials; rows separated by ';', e.g. 2 1; 1 4",
    }


def t_ridge_gradient(rng: random.Random, i: int) -> dict:
    lam = rng.randint(1, 3)
    t = rng.randint(1, 9)
    expected = format_polynomial([(1 + lam, "w"), (-t, "")])
    return {
        "id": f"gen-ridge-gradient-{i}",
        "title": "Ridge Regression Gradient",
        "domain": "Symbolic",
        "topic": "Machine Learning",
        "difficulty": "Medium",
        "prompt": f"Ridge objective f(w) = (1/2)*(w - {t})^2 + ({lam}/2)*w^2.\n\nFind f'(w).",
        "expected": expected,
        "answer_kind": "Scalar",
        "hint": "differentiate and combine like terms; format a*w - b",
    }


def t_lcm(rng: random.Random, i: int) -> dict:
    a, b = rng.randint(4, 60), rng.randint(4, 60)
    return {
        "id": f"gen-lcm-{i}",
        "title": "Least Common Multiple",
        "domain": "Exact",
        "topic": "Number Theory",
        "difficulty": "Easy",
        "prompt": f"Find the least common multiple:\n\n    lcm({a}, {b})",
        "expected": str(lcm(a, b)),
        "answer_kind": "Scalar",
        "hint": "lcm(a, b) = a*b / gcd(a, b); enter a single integer",
    }


def t_modular_inverse(rng: random.Random, i: int) -> dict:
    m = rng.choice([7, 11, 13, 17, 19, 23, 29, 31])  # prime → every a is invertible
    a = rng.randint(2, m - 1)
    return {
        "id": f"gen-modinverse-{i}",
        "title": "Modular Inverse",
        "domain": "Exact",
        "topic": "Number Theory",
        "difficulty": "Hard",
        "prompt": (
            f"Find the modular inverse of {a} modulo {m}\n"
            f"(the x in [0, {m}) with {a}*x ≡ 1 (mod {m}))."
        ),
        "expected": str(pow(a, -1, m)),
        "answer_kind": "Scalar",
        "hint": "use the extended Euclidean algorithm; enter one integer in [0, m)",
    }


def t_eigenvalues_triangular(rng: random.Random, i: int) -> dict:
    l1, l2 = rng.randint(-6, 6), rng.randint(-6, 6)
    r = rng.randint(-5, 5)
    evals = sorted([l1, l2])
    return {
        "id": f"gen-eigenvalues-{i}",
        "title": "Eigenvalues of a Triangular Matrix",
        "domain": "Tensor",
        "topic": "Linear Algebra",
        "difficulty": "Medium",
        "prompt": (
            f"Find the eigenvalues of the triangular matrix:\n\n    [[{l1}, {r}], [0, {l2}]]\n\n"
            "List them in increasing order (e.g. -1 3)."
        ),
        "expected": f"{evals[0]} {evals[1]}",
        "answer_kind": "Scalar",
        "hint": "the eigenvalues of a triangular matrix are its diagonal entries",
    }


def t_linear_system(rng: random.Random, i: int) -> dict:
    while True:
        a, b, c, d = (rng.randint(-6, 6) for _ in range(4))
        det = a * d - b * c
        if det != 0:
            break
    b1, b2 = rng.randint(-8, 8), rng.randint(-8, 8)
    # Cramer's rule — exact rationals.
    x1 = Fraction(b1 * d - b * b2, det)
    x2 = Fraction(a * b2 - b1 * c, det)
    return {
        "id": f"gen-linear-system-{i}",
        "title": "Solve a 2×2 System",
        "domain": "Tensor",
        "topic": "Linear Algebra",
        "difficulty": "Hard",
        "prompt": (
            f"Solve the system for (x1, x2):\n\n"
            f"    {a}*x1 + {b}*x2 = {b1}\n    {c}*x1 + {d}*x2 = {b2}\n\n"
            "Enter the solution as a column (x1; x2)."
        ),
        "expected": f"{frac(x1)}; {frac(x2)}",
        "answer_kind": "Scalar",
        "hint": "Cramer's rule or elimination; exact fractions are accepted",
    }


def t_psd_check(rng: random.Random, i: int) -> dict:
    a = rng.randint(-3, 5)
    c = rng.randint(-3, 5)
    b = rng.randint(-3, 3)
    psd = a >= 0 and c >= 0 and a * c - b * b >= 0
    return {
        "id": f"gen-psd-check-{i}",
        "title": "Positive Semidefinite?",
        "domain": "Exact",
        "topic": "Convex Optimization",
        "difficulty": "Medium",
        "prompt": (
            f"Is the symmetric matrix\n\n    [[{a}, {b}], [{b}, {c}]]\n\n"
            "positive semidefinite? Answer 1 for yes, 0 for no."
        ),
        "expected": "1" if psd else "0",
        "answer_kind": "Scalar",
        "hint": "2×2 symmetric is PSD iff a ≥ 0, c ≥ 0, and ac − b² ≥ 0",
    }


def t_lagrangian_stationarity(rng: random.Random, i: int) -> dict:
    a = rng.randint(1, 5)
    c = rng.randint(-6, 6)
    sign = "-" if c < 0 else "+"
    return {
        "id": f"gen-lagrangian-{i}",
        "title": "Lagrangian Stationarity",
        "domain": "Symbolic",
        "topic": "Convex Optimization",
        "difficulty": "Medium",
        "prompt": (
            f"For the Lagrangian\n\n    L(x, lambda) = {a}*x^2 + lambda*(x {sign} {abs(c)})\n\n"
            "write the stationarity condition ∂L/∂x (as an expression in x and lambda)."
        ),
        "expected": f"{2 * a}*x + lambda",
        "answer_kind": "Scalar",
        "hint": "differentiate L with respect to x; the constant term drops out",
    }


def t_matmul_grad_shape(rng: random.Random, i: int) -> dict:
    n, k, m = rng.randint(2, 64), rng.randint(2, 64), rng.randint(2, 64)
    return {
        "id": f"gen-matmul-grad-shape-{i}",
        "title": "Backprop Gradient Shape",
        "domain": "Tensor",
        "topic": "Machine Learning",
        "difficulty": "Easy",
        "prompt": (
            f"In a linear layer Y = X·W with X of shape [{n}×{k}] and W of shape [{k}×{m}], "
            "the weight gradient is dL/dW = Xᵀ·(dL/dY).\n\nWhat is its shape? Enter `rows cols`."
        ),
        "expected": f"{k} {m}",
        "answer_kind": "Scalar",
        "hint": "dL/dW has the same shape as W",
    }


def t_attention_shape(rng: random.Random, i: int) -> dict:
    n, m, d = rng.randint(2, 32), rng.randint(2, 32), rng.randint(2, 64)
    return {
        "id": f"gen-attention-shape-{i}",
        "title": "Attention Output Shape",
        "domain": "Tensor",
        "topic": "Machine Learning",
        "difficulty": "Medium",
        "prompt": (
            f"Scaled dot-product attention with Q [{n}×{d}], K [{m}×{d}], V [{m}×{d}]:\n\n"
            "    softmax(Q·Kᵀ / √d)·V\n\nWhat is the output shape? Enter `rows cols`."
        ),
        "expected": f"{n} {d}",
        "answer_kind": "Scalar",
        "hint": "scores are [n×m]; multiplying by V [m×d] gives [n×d]",
    }


def t_gradient_descent_step(rng: random.Random, i: int) -> dict:
    xk = rng.choice([k for k in range(-9, 10) if k != 0])
    alpha = rng.choice([Fraction(1, 10), Fraction(1, 4), Fraction(1, 2)])
    nxt = Fraction(xk) * (1 - 2 * alpha)  # x - α·(2x)
    return {
        "id": f"gen-gd-step-{i}",
        "title": "Gradient Descent Step",
        "domain": "Tensor",
        "topic": "Machine Learning",
        "difficulty": "Medium",
        "prompt": (
            f"Gradient descent on f(x) = x^2 (so ∇f = 2x).\n\n"
            f"From x_k = {xk} with step size α = {frac(alpha)}, compute the next iterate "
            "x_k − α·∇f(x_k)."
        ),
        "expected": frac(nxt),
        "answer_kind": "Scalar",
        "hint": "x_k − α·2·x_k; an exact fraction or a decimal is accepted",
    }


def t_conv_output_length(rng: random.Random, i: int) -> dict:
    length = rng.randint(8, 64)
    kernel = rng.randint(1, 7)
    padding = rng.randint(0, 3)
    stride = rng.randint(1, 4)
    out = (length - kernel + 2 * padding) // stride + 1
    return {
        "id": f"gen-conv-output-{i}",
        "title": "Conv Output Length",
        "domain": "Exact",
        "topic": "Deep Learning",
        "difficulty": "Medium",
        "prompt": (
            f"A 1-D convolution has input length L = {length}, kernel K = {kernel}, "
            f"padding P = {padding}, stride S = {stride}.\n\n"
            "Output length = floor((L − K + 2P) / S) + 1. Compute it."
        ),
        "expected": str(out),
        "answer_kind": "Scalar",
        "hint": "floor division; enter one integer",
    }


def t_layernorm_mean(rng: random.Random, i: int) -> dict:
    xs = [rng.randint(-6, 6) for _ in range(rng.randint(3, 4))]
    mean = Fraction(sum(xs), len(xs))
    return {
        "id": f"gen-layernorm-mean-{i}",
        "title": "Activation Mean",
        "domain": "Tensor",
        "topic": "Deep Learning",
        "difficulty": "Easy",
        "prompt": f"Compute the mean μ of the activations\n\n    x = {xs}",
        "expected": frac(mean),
        "answer_kind": "Scalar",
        "hint": "μ = (Σ xᵢ) / n; an exact fraction or a decimal is accepted",
    }


def t_layernorm_variance(rng: random.Random, i: int) -> dict:
    xs = [rng.randint(-6, 6) for _ in range(rng.randint(3, 4))]
    n = len(xs)
    s = sum(xs)
    sumsq = sum(x * x for x in xs)
    var = Fraction(n * sumsq - s * s, n * n)  # biased (1/n) variance
    return {
        "id": f"gen-layernorm-var-{i}",
        "title": "Activation Variance",
        "domain": "Tensor",
        "topic": "Deep Learning",
        "difficulty": "Medium",
        "prompt": (
            f"Compute the biased (1/n) variance σ² of the activations\n\n    x = {xs}\n\n"
            "(σ² = (1/n)·Σ (xᵢ − μ)²)"
        ),
        "expected": frac(var),
        "answer_kind": "Scalar",
        "hint": "σ² = (Σ xᵢ²)/n − μ²; an exact fraction or a decimal is accepted",
    }


def t_linear_param_count(rng: random.Random, i: int) -> dict:
    a, b = rng.randint(2, 256), rng.randint(2, 256)
    return {
        "id": f"gen-linear-params-{i}",
        "title": "Linear Layer Parameters",
        "domain": "Exact",
        "topic": "Deep Learning",
        "difficulty": "Easy",
        "prompt": (
            f"A linear layer maps ℝ^{a} → ℝ^{b} (with a bias).\n\n"
            "How many learnable parameters does it have?"
        ),
        "expected": str(a * b + b),
        "answer_kind": "Scalar",
        "hint": "weights a·b plus b biases",
    }


def t_conv_param_count(rng: random.Random, i: int) -> dict:
    cin, cout = rng.randint(1, 16), rng.randint(1, 16)
    k = rng.choice([1, 3, 5, 7])
    return {
        "id": f"gen-conv-params-{i}",
        "title": "Conv Layer Parameters",
        "domain": "Exact",
        "topic": "Deep Learning",
        "difficulty": "Medium",
        "prompt": (
            f"A Conv2D has {cin} input channels, {cout} filters, and a {k}×{k} kernel "
            "(with a bias per filter).\n\nHow many learnable parameters does it have?"
        ),
        "expected": str(cout * (cin * k * k + 1)),
        "answer_kind": "Scalar",
        "hint": "each filter: C_in·K·K weights + 1 bias; times C_out filters",
    }


def t_combination(rng: random.Random, i: int) -> dict:
    n = rng.randint(6, 20)
    k = rng.randint(2, n - 2)
    return {
        "id": f"gen-combination-{i}",
        "title": "Combinations",
        "domain": "Exact",
        "topic": "Probability",
        "difficulty": "Easy",
        "prompt": f"How many ways to choose {k} items from {n} (order doesn't matter)?\n\n    C({n}, {k})",
        "expected": str(comb(n, k)),
        "answer_kind": "Scalar",
        "hint": "C(n, k) = n! / (k!·(n−k)!); enter one integer",
    }


def t_permutation(rng: random.Random, i: int) -> dict:
    n = rng.randint(6, 16)
    k = rng.randint(2, n - 1)
    return {
        "id": f"gen-permutation-{i}",
        "title": "Permutations",
        "domain": "Exact",
        "topic": "Probability",
        "difficulty": "Medium",
        "prompt": f"How many ordered arrangements of {k} items from {n}?\n\n    P({n}, {k})",
        "expected": str(perm(n, k)),
        "answer_kind": "Scalar",
        "hint": "P(n, k) = n! / (n−k)!; enter one integer",
    }


def t_factorial(rng: random.Random, i: int) -> dict:
    n = rng.randint(8, 25)  # large enough to exercise arbitrary precision
    return {
        "id": f"gen-factorial-{i}",
        "title": "Factorial",
        "domain": "Exact",
        "topic": "Probability",
        "difficulty": "Easy",
        "prompt": f"Compute the factorial:\n\n    {n}!",
        "expected": str(factorial(n)),
        "answer_kind": "Scalar",
        "hint": "n! = n·(n−1)···1; enter the full integer",
    }


def t_two_dice_sum(rng: random.Random, i: int) -> dict:
    s = rng.randint(2, 12)
    count = 6 - abs(s - 7)  # number of (a, b) with a + b = s, a,b ∈ 1..6
    prob = Fraction(count, 36)
    return {
        "id": f"gen-dice-sum-{i}",
        "title": "Two-Dice Probability",
        "domain": "Tensor",
        "topic": "Probability",
        "difficulty": "Medium",
        "prompt": f"Two fair dice are rolled. What is P(sum = {s})?\n\nGive an exact fraction.",
        "expected": frac(prob),
        "answer_kind": "Scalar",
        "hint": "favorable outcomes over 36; reduce the fraction",
    }


def t_expected_value(rng: random.Random, i: int) -> dict:
    vals = rng.sample(range(-5, 10), 3)
    weights = [rng.randint(1, 5) for _ in vals]
    total = sum(weights)
    ev = Fraction(sum(v * w for v, w in zip(vals, weights)), total)
    return {
        "id": f"gen-expected-value-{i}",
        "title": "Expected Value",
        "domain": "Tensor",
        "topic": "Statistics",
        "difficulty": "Medium",
        "prompt": (
            f"A random variable X takes values {vals} with relative weights {weights}.\n\n"
            "Find E[X] as an exact fraction."
        ),
        "expected": frac(ev),
        "answer_kind": "Scalar",
        "hint": "E[X] = Σ wᵢ·vᵢ / Σ wᵢ",
    }


def t_variance_discrete(rng: random.Random, i: int) -> dict:
    vals = rng.sample(range(-4, 8), 3)
    weights = [rng.randint(1, 4) for _ in vals]
    total = sum(weights)
    ex = Fraction(sum(v * w for v, w in zip(vals, weights)), total)
    ex2 = Fraction(sum(v * v * w for v, w in zip(vals, weights)), total)
    var = ex2 - ex * ex
    return {
        "id": f"gen-variance-{i}",
        "title": "Discrete Variance",
        "domain": "Tensor",
        "topic": "Statistics",
        "difficulty": "Hard",
        "prompt": (
            f"X takes values {vals} with relative weights {weights}.\n\n"
            "Find Var(X) = E[X²] − E[X]² as an exact fraction."
        ),
        "expected": frac(var),
        "answer_kind": "Scalar",
        "hint": "compute E[X²] and E[X] from the weights, then subtract the square",
    }


def _convolve(x: list[int], h: list[int]) -> list[int]:
    """Linear convolution of two integer sequences."""
    y = [0] * (len(x) + len(h) - 1)
    for i, xi in enumerate(x):
        for j, hj in enumerate(h):
            y[i + j] += xi * hj
    return y


def _matmul(a: list[list[int]], b: list[list[int]]) -> list[list[int]]:
    """Integer matrix product."""
    return [[sum(a[i][k] * b[k][j] for k in range(len(b))) for j in range(len(b[0]))] for i in range(len(a))]


def _mat_literal(rows: list[list]) -> str:
    """`[[a, b], [c, d]]` ASCII form for a prompt (rendered as a bracketed matrix)."""
    return "[" + ", ".join("[" + ", ".join(str(x) for x in row) + "]" for row in rows) + "]"


def _mat_answer(rows: list[list]) -> str:
    """`a b; c d` answer form for the tensor pipeline."""
    return "; ".join(" ".join(str(x) for x in row) for row in rows)


# ---- Signals & DSP --------------------------------------------------------


def t_conv_length(rng: random.Random, i: int) -> dict:
    m, n = rng.randint(3, 64), rng.randint(3, 64)
    return {
        "id": f"gen-conv-length-{i}",
        "title": "Convolution Length",
        "domain": "Exact",
        "topic": "Signals & DSP",
        "difficulty": "Easy",
        "prompt": (
            f"Two finite signals of length {m} and {n} are linearly convolved.\n\n"
            "What is the length of the result?"
        ),
        "expected": str(m + n - 1),
        "answer_kind": "Scalar",
        "hint": "M + N − 1",
    }


def t_nyquist(rng: random.Random, i: int) -> dict:
    fs = rng.choice([8000, 16000, 32000, 44100 - 100, 48000, 96000])  # even rates
    return {
        "id": f"gen-nyquist-{i}",
        "title": "Nyquist Frequency",
        "domain": "Exact",
        "topic": "Signals & DSP",
        "difficulty": "Easy",
        "prompt": f"A signal is sampled at f_s = {fs} Hz.\n\nWhat is the Nyquist frequency (in Hz)?",
        "expected": str(fs // 2),
        "answer_kind": "Scalar",
        "hint": "the Nyquist frequency is f_s / 2",
    }


def t_fft_stages(rng: random.Random, i: int) -> dict:
    k = rng.randint(3, 12)
    return {
        "id": f"gen-fft-stages-{i}",
        "title": "Radix-2 FFT Stages",
        "domain": "Exact",
        "topic": "Signals & DSP",
        "difficulty": "Medium",
        "prompt": f"A radix-2 FFT has size N = {2**k}.\n\nHow many butterfly stages does it use?",
        "expected": str(k),
        "answer_kind": "Scalar",
        "hint": "log₂(N) stages",
    }


def t_convolution_sequence(rng: random.Random, i: int) -> dict:
    x = [rng.randint(-4, 4) for _ in range(rng.randint(2, 3))]
    h = [rng.randint(-4, 4) for _ in range(rng.randint(2, 3))]
    y = _convolve(x, h)
    return {
        "id": f"gen-convolution-{i}",
        "title": "Convolve Two Signals",
        "domain": "Tensor",
        "topic": "Signals & DSP",
        "difficulty": "Hard",
        "prompt": (
            f"Compute the linear convolution of\n\n    x = {x}\n    h = {h}\n\n"
            "Enter the result as a space-separated row."
        ),
        "expected": " ".join(str(v) for v in y),
        "answer_kind": "Scalar",
        "hint": "y[n] = Σ x[k]·h[n−k]; length is len(x)+len(h)−1",
    }


# ---- Linear Programming ---------------------------------------------------


def t_box_lp_min(rng: random.Random, i: int) -> dict:
    n = rng.randint(2, 3)
    c = [rng.choice([k for k in range(-5, 6) if k != 0]) for _ in range(n)]
    u = [rng.randint(1, 6) for _ in range(n)]
    obj = " + ".join(f"{ci}*x{j}" for j, ci in enumerate(c, start=1)).replace("+ -", "- ")
    # For a minimization over [0, u_i], pick x_i = u_i where c_i < 0, else 0.
    minimum = sum(ci * ui for ci, ui in zip(c, u) if ci < 0)
    return {
        "id": f"gen-box-lp-{i}",
        "title": "Box-Constrained LP",
        "domain": "Exact",
        "topic": "Linear Programming",
        "difficulty": "Medium",
        "prompt": (
            f"Minimize  {obj}\nsubject to  0 ≤ x_j ≤ u_j  with  u = {u}.\n\n"
            "What is the minimum objective value?"
        ),
        "expected": str(minimum),
        "answer_kind": "Scalar",
        "hint": "each x_j is at a bound: u_j if c_j < 0, else 0",
    }


def t_lp_standard_vars(rng: random.Random, i: int) -> dict:
    n, m = rng.randint(2, 8), rng.randint(1, 6)
    return {
        "id": f"gen-lp-standard-vars-{i}",
        "title": "Standard-Form Variables",
        "domain": "Exact",
        "topic": "Linear Programming",
        "difficulty": "Easy",
        "prompt": (
            f"An LP has {n} decision variables and {m} inequality (≤) constraints.\n\n"
            "Converting to standard form (adding slack variables), how many variables in total?"
        ),
        "expected": str(n + m),
        "answer_kind": "Scalar",
        "hint": "one slack per ≤-constraint: n + m",
    }


def t_lp_objective(rng: random.Random, i: int) -> dict:
    n = rng.randint(2, 3)
    c = [rng.randint(-5, 6) for _ in range(n)]
    x = [rng.randint(0, 6) for _ in range(n)]
    return {
        "id": f"gen-lp-objective-{i}",
        "title": "Objective Value",
        "domain": "Exact",
        "topic": "Linear Programming",
        "difficulty": "Easy",
        "prompt": f"For c = {c} and the feasible point x = {x}, compute the objective c·x.",
        "expected": str(sum(ci * xi for ci, xi in zip(c, x))),
        "answer_kind": "Scalar",
        "hint": "c·x = Σ cⱼ·xⱼ",
    }


# ---- Hypothesis Testing ---------------------------------------------------


def t_t_test_df(rng: random.Random, i: int) -> dict:
    n = rng.randint(5, 60)
    return {
        "id": f"gen-t-test-df-{i}",
        "title": "t-Test Degrees of Freedom",
        "domain": "Exact",
        "topic": "Hypothesis Testing",
        "difficulty": "Easy",
        "prompt": f"A one-sample t-test uses n = {n} observations.\n\nHow many degrees of freedom?",
        "expected": str(n - 1),
        "answer_kind": "Scalar",
        "hint": "df = n − 1",
    }


def t_z_score(rng: random.Random, i: int) -> dict:
    sigma = rng.randint(2, 9)
    mu = rng.randint(-5, 10)
    x = rng.randint(-5, 20)
    return {
        "id": f"gen-z-score-{i}",
        "title": "Z-Score",
        "domain": "Tensor",
        "topic": "Hypothesis Testing",
        "difficulty": "Medium",
        "prompt": (
            f"A value x = {x} comes from a distribution with mean μ = {mu} and "
            f"standard deviation σ = {sigma}.\n\nFind the z-score (x − μ)/σ as an exact fraction."
        ),
        "expected": frac(Fraction(x - mu, sigma)),
        "answer_kind": "Scalar",
        "hint": "z = (x − μ)/σ",
    }


def t_standard_error(rng: random.Random, i: int) -> dict:
    n = rng.choice([4, 9, 16, 25, 36, 49, 64])  # perfect squares → exact SE
    sigma = rng.randint(2, 24)
    se = Fraction(sigma, isqrt(n))
    return {
        "id": f"gen-standard-error-{i}",
        "title": "Standard Error",
        "domain": "Tensor",
        "topic": "Hypothesis Testing",
        "difficulty": "Medium",
        "prompt": (
            f"A sample of n = {n} observations has population σ = {sigma}.\n\n"
            "Find the standard error SE = σ/√n as an exact fraction."
        ),
        "expected": frac(se),
        "answer_kind": "Scalar",
        "hint": "SE = σ/√n; √n is exact since n is a perfect square",
    }


# ---- Linear-algebra decompositions ----------------------------------------


def t_trace(rng: random.Random, i: int) -> dict:
    rows = [[rng.randint(-9, 9) for _ in range(3)] for _ in range(3)]
    return {
        "id": f"gen-trace-{i}",
        "title": "Matrix Trace",
        "domain": "Exact",
        "topic": "Decompositions",
        "difficulty": "Easy",
        "prompt": f"Find the trace of\n\n    {_mat_literal(rows)}",
        "expected": str(sum(rows[k][k] for k in range(3))),
        "answer_kind": "Scalar",
        "hint": "the trace is the sum of the diagonal (= sum of eigenvalues)",
    }


def t_rank_2x2(rng: random.Random, i: int) -> dict:
    a, b, c, d = (rng.randint(-4, 4) for _ in range(4))
    det = a * d - b * c
    rank = 2 if det != 0 else (1 if any(v != 0 for v in (a, b, c, d)) else 0)
    return {
        "id": f"gen-rank-{i}",
        "title": "Matrix Rank",
        "domain": "Exact",
        "topic": "Decompositions",
        "difficulty": "Medium",
        "prompt": f"What is the rank of\n\n    {_mat_literal([[a, b], [c, d]])}?",
        "expected": str(rank),
        "answer_kind": "Scalar",
        "hint": "a 2×2 has rank 2 if its determinant is nonzero",
    }


def t_cholesky_2x2(rng: random.Random, i: int) -> dict:
    l11, l22 = rng.randint(1, 5), rng.randint(1, 5)
    l21 = rng.randint(-4, 4)
    a = [[l11 * l11, l11 * l21], [l11 * l21, l21 * l21 + l22 * l22]]
    return {
        "id": f"gen-cholesky-{i}",
        "title": "Cholesky Factor",
        "domain": "Tensor",
        "topic": "Decompositions",
        "difficulty": "Hard",
        "prompt": (
            f"Find the lower-triangular Cholesky factor L (positive diagonal) with L·Lᵀ = A:\n\n"
            f"    A = {_mat_literal(a)}"
        ),
        "expected": _mat_answer([[l11, 0], [l21, l22]]),
        "answer_kind": "Scalar",
        "hint": "L = [[l11, 0], [l21, l22]]: l11 = √A₁₁, l21 = A₂₁/l11, l22 = √(A₂₂ − l21²)",
    }


# ---- Graph theory ---------------------------------------------------------


def t_complete_edges(rng: random.Random, i: int) -> dict:
    n = rng.randint(3, 20)
    return {
        "id": f"gen-complete-edges-{i}",
        "title": "Edges in a Complete Graph",
        "domain": "Exact",
        "topic": "Graph Theory",
        "difficulty": "Easy",
        "prompt": f"How many edges does the complete graph K_{n} have?",
        "expected": str(n * (n - 1) // 2),
        "answer_kind": "Scalar",
        "hint": "C(n, 2) = n(n−1)/2",
    }


def t_cayley(rng: random.Random, i: int) -> dict:
    n = rng.randint(4, 18)
    return {
        "id": f"gen-cayley-{i}",
        "title": "Spanning Trees of K_n",
        "domain": "Exact",
        "topic": "Graph Theory",
        "difficulty": "Hard",
        "prompt": f"By Cayley's formula, how many labelled spanning trees does K_{n} have?",
        "expected": str(n ** (n - 2)),
        "answer_kind": "Scalar",
        "hint": "Cayley's formula: n^(n−2)",
    }


def t_adjacency_walks(rng: random.Random, i: int) -> dict:
    # Symmetric 0/1 adjacency matrix, zero diagonal (simple graph).
    a = [[0, 0, 0], [0, 0, 0], [0, 0, 0]]
    for r in range(3):
        for c in range(r + 1, 3):
            a[r][c] = a[c][r] = rng.randint(0, 1)
    return {
        "id": f"gen-walks-{i}",
        "title": "Walks of Length 2",
        "domain": "Tensor",
        "topic": "Graph Theory",
        "difficulty": "Medium",
        "prompt": (
            f"For the adjacency matrix\n\n    A = {_mat_literal(a)}\n\ncompute A² "
            "(entry (i,j) counts walks of length 2)."
        ),
        "expected": _mat_answer(_matmul(a, a)),
        "answer_kind": "Scalar",
        "hint": "A² is the matrix product of A with itself",
    }


# ---- Complex numbers ------------------------------------------------------


def t_complex_multiply(rng: random.Random, i: int) -> dict:
    a, b, c, d = (rng.randint(-6, 6) for _ in range(4))
    return {
        "id": f"gen-complex-mul-{i}",
        "title": "Complex Multiplication",
        "domain": "Tensor",
        "topic": "Complex Numbers",
        "difficulty": "Medium",
        "prompt": (
            f"Multiply ({a} + {b}i)({c} + {d}i). Enter the result as `real imag`."
        ),
        "expected": f"{a * c - b * d} {a * d + b * c}",
        "answer_kind": "Scalar",
        "hint": "(a+bi)(c+di) = (ac−bd) + (ad+bc)i",
    }


def t_power_of_i(rng: random.Random, i: int) -> dict:
    n = rng.randint(2, 99)
    val = [(1, 0), (0, 1), (-1, 0), (0, -1)][n % 4]
    return {
        "id": f"gen-power-of-i-{i}",
        "title": "Powers of i",
        "domain": "Tensor",
        "topic": "Complex Numbers",
        "difficulty": "Easy",
        "prompt": f"Compute i^{n}. Enter it as `real imag`.",
        "expected": f"{val[0]} {val[1]}",
        "answer_kind": "Scalar",
        "hint": "i cycles with period 4: 1, i, −1, −i",
    }


def t_modulus(rng: random.Random, i: int) -> dict:
    a0, b0, m = rng.choice([(3, 4, 5), (6, 8, 10), (5, 12, 13), (8, 15, 17), (7, 24, 25), (20, 21, 29)])
    a = a0 * rng.choice([-1, 1])
    b = b0 * rng.choice([-1, 1])
    return {
        "id": f"gen-modulus-{i}",
        "title": "Complex Modulus",
        "domain": "Exact",
        "topic": "Complex Numbers",
        "difficulty": "Medium",
        "prompt": f"Find the modulus |{a} + {b}i|.",
        "expected": str(m),
        "answer_kind": "Scalar",
        "hint": "|a + bi| = √(a² + b²)",
    }


# ---- Ordinary differential equations --------------------------------------


def t_ode_char_roots(rng: random.Random, i: int) -> dict:
    r1, r2 = sorted(rng.sample(range(-5, 6), 2))
    b, c = -(r1 + r2), r1 * r2  # y'' + b y' + c y = 0 has roots r1, r2
    ode = f"y'' + {b}*y' + {c}*y = 0".replace("+ -", "- ")
    return {
        "id": f"gen-ode-roots-{i}",
        "title": "Characteristic Roots",
        "domain": "Tensor",
        "topic": "ODEs",
        "difficulty": "Medium",
        "prompt": (
            f"Find the roots of the characteristic equation of\n\n    {ode}\n\n"
            "Enter them smaller-first as `r1 r2`."
        ),
        "expected": f"{r1} {r2}",
        "answer_kind": "Scalar",
        "hint": "the characteristic equation is r² + b·r + c = 0",
    }


def t_ode_equilibrium(rng: random.Random, i: int) -> dict:
    a = rng.choice([k for k in range(-5, 6) if k != 0])
    b = rng.randint(-8, 8)
    return {
        "id": f"gen-ode-equilibrium-{i}",
        "title": "ODE Equilibrium",
        "domain": "Tensor",
        "topic": "ODEs",
        "difficulty": "Medium",
        "prompt": f"Find the equilibrium of y' = {a}*y + {b} (the y with y' = 0), as an exact fraction.",
        "expected": frac(Fraction(-b, a)),
        "answer_kind": "Scalar",
        "hint": "set a·y + b = 0 and solve for y",
    }


def t_exp_growth(rng: random.Random, i: int) -> dict:
    k = rng.choice([k for k in range(-4, 5) if k != 0])
    y0 = rng.randint(1, 9)
    return {
        "id": f"gen-exp-growth-{i}",
        "title": "Exponential Growth",
        "domain": "Symbolic",
        "topic": "ODEs",
        "difficulty": "Medium",
        "prompt": f"Solve y' = {k}*y with y(0) = {y0}. Give y(t).",
        "expected": f"{y0}*exp({k}*t)",
        "answer_kind": "Scalar",
        "hint": "y(t) = y(0)·e^(k·t)",
    }


TEMPLATES = [
    t_expand_binomial,
    t_power_derivative,
    t_determinant_2x2,
    t_transpose,
    t_inverse_2x2,
    t_prime_factorization,
    t_modular_exponentiation,
    t_gcd,
    t_lcm,
    t_modular_inverse,
    t_gradient_linear,
    t_hessian_quadratic,
    t_ridge_gradient,
    t_eigenvalues_triangular,
    t_linear_system,
    t_psd_check,
    t_lagrangian_stationarity,
    t_matmul_grad_shape,
    t_attention_shape,
    t_gradient_descent_step,
    t_conv_output_length,
    t_layernorm_mean,
    t_layernorm_variance,
    t_linear_param_count,
    t_conv_param_count,
    t_combination,
    t_permutation,
    t_factorial,
    t_two_dice_sum,
    t_expected_value,
    t_variance_discrete,
    t_conv_length,
    t_nyquist,
    t_fft_stages,
    t_convolution_sequence,
    t_box_lp_min,
    t_lp_standard_vars,
    t_lp_objective,
    t_t_test_df,
    t_z_score,
    t_standard_error,
    t_trace,
    t_rank_2x2,
    t_cholesky_2x2,
    t_complete_edges,
    t_cayley,
    t_adjacency_walks,
    t_complex_multiply,
    t_power_of_i,
    t_modulus,
    t_ode_char_roots,
    t_ode_equilibrium,
    t_exp_growth,
]


def _p(id, title, domain, topic, difficulty, prompt, expected, hint, answer_kind="Scalar"):
    """Compact constructor for the curated, non-random `FIXED` problems."""
    return {
        "id": id,
        "title": title,
        "domain": domain,
        "topic": topic,
        "difficulty": difficulty,
        "prompt": prompt,
        "expected": expected,
        "answer_kind": answer_kind,
        "hint": hint,
    }


# Curated, canonical problems emitted once each (no randomization). Heavy on the
# ML/AI calculus and convex-optimization staples; the answers are in Mamba's own
# input syntax and verified end-to-end by `mamba --verify`.
FIXED = [
    # ---- Machine Learning · activation & loss gradients (Symbolic) ----
    _p("ml-sigmoid-derivative", "Sigmoid Derivative", "Symbolic", "Machine Learning", "Medium",
       "The logistic sigmoid is σ(x) = 1/(1+exp(-x)). Differentiate it:\n\n    d/dx [ 1/(1+exp(-x)) ]",
       "exp(-x)/(1+exp(-x))^2", "use exp(...); the equivalent s*(1-s) form is accepted"),
    _p("ml-tanh-derivative", "Tanh Derivative", "Symbolic", "Machine Learning", "Easy",
       "Differentiate the tanh activation:\n\n    d/dx [ tanh(x) ]",
       "1 - tanh(x)^2", "a standard identity in terms of tanh(x)"),
    _p("ml-softplus-derivative", "Softplus Derivative", "Symbolic", "Machine Learning", "Medium",
       "Softplus is f(x) = ln(1 + exp(x)). Find f'(x).",
       "1/(1+exp(-x))", "it equals the sigmoid; exp(x)/(1+exp(x)) is accepted"),
    _p("ml-mse-gradient", "MSE Gradient", "Symbolic", "Machine Learning", "Easy",
       "Squared error for one sample (x is the prediction, y the target):\n\n    L = (1/2)*(y - x)^2\n\nFind dL/dx.",
       "x - y", "chain rule — mind the sign"),
    _p("ml-bce-logit-gradient", "Cross-Entropy Logit Gradient", "Symbolic", "Machine Learning", "Hard",
       "With a sigmoid output and logit z, the binary cross-entropy gradient simplifies to σ(z) − y. "
       "Write σ(z) − y in terms of z and y.",
       "1/(1+exp(-z)) - y", "σ(z) = 1/(1+exp(-z))"),
    _p("ml-log-likelihood-derivative", "Log Term Derivative", "Symbolic", "Machine Learning", "Easy",
       "A log-likelihood contains ln(x). Find:\n\n    d/dx [ ln(x) ]",
       "1/x", "for x > 0"),
    _p("ml-l2-gradient", "L2 Regularizer Gradient", "Symbolic", "Machine Learning", "Easy",
       "The L2 (ridge) penalty is R(w) = (lambda/2)*w^2. Find dR/dw.",
       "lambda*w", "treat lambda as a constant; write lambda*w"),
    # ---- Convex Optimization (Symbolic) ----
    _p("convex-quartic-second-derivative", "Convexity via 2nd Derivative", "Symbolic", "Convex Optimization", "Medium",
       "A function is convex where its second derivative is ≥ 0. Compute:\n\n    d^2/dx^2 [ x^4 ]",
       "12*x^2", "differentiate twice"),
    _p("convex-log-barrier-gradient", "Log-Barrier Gradient", "Symbolic", "Convex Optimization", "Easy",
       "The log-barrier for x > 0 is φ(x) = -ln(x). Find φ'(x).",
       "-1/x", "for x > 0"),
    _p("convex-exp-derivative", "Exponential Derivative", "Symbolic", "Convex Optimization", "Easy",
       "exp is a canonical convex function. Find:\n\n    d/dx [ exp(x) ]",
       "exp(x)", "it is its own derivative"),
    _p("convex-negentropy-derivative", "Negative-Entropy Derivative", "Symbolic", "Convex Optimization", "Medium",
       "Negative entropy is f(x) = x*ln(x) for x > 0. Find f'(x).",
       "ln(x) + 1", "product rule"),
    # ---- ML / Convex (Tensor) ----
    _p("ml-gram-matrix", "Gram Matrix", "Tensor", "Machine Learning", "Medium",
       "For the data matrix\n\n    X = [[1, 2], [3, 4]]\n\ncompute the Gram matrix X^T X.",
       "10 14; 14 20", "X^T X is symmetric; rows separated by ';'"),
    _p("convex-hessian-quadratic", "Hessian of a Quadratic", "Tensor", "Convex Optimization", "Medium",
       "Find the Hessian of:\n\n    f(x1, x2) = x1^2 + x1*x2 + 2*x2^2",
       "2 1; 1 4", "the matrix of second partials"),
    _p("convex-gradient-linear", "Gradient of a Linear Form", "Tensor", "Convex Optimization", "Easy",
       "Find the gradient ∇f of:\n\n    f(x) = 2*x1 - 3*x2 + 5*x3\n\nEnter it as a column vector.",
       "2; -3; 5", "∇(aᵀx) = a; one component per row"),
    # ---- Richer ML / Convex staples ----
    _p("ml-softmax-jacobian", "Softmax Jacobian", "Tensor", "Machine Learning", "Hard",
       "The softmax Jacobian is J(i,j) = s_i*(δ(i,j) − s_j). At logits z = [0, 0] "
       "(so s = [1/2, 1/2]), compute J.",
       "1/4 -1/4; -1/4 1/4", "diagonal: s_i*(1−s_i); off-diagonal: −s_i*s_j; exact fractions"),
    _p("ml-softmax-ce-gradient", "Softmax Cross-Entropy Gradient", "Tensor", "Machine Learning", "Medium",
       "For softmax + cross-entropy with logits z = [0, 0] and one-hot target y = [1, 0], the "
       "gradient is dL/dz = softmax(z) − y. Compute it as a column.",
       "-1/2; 1/2", "the famous result dL/dz = p − y, with p = [1/2, 1/2]"),
    _p("convex-kkt-optimum", "KKT Optimum", "Exact", "Convex Optimization", "Medium",
       "Minimize (1/2)*x^2 subject to x ≥ 1. By the KKT conditions, what is the optimal x?",
       "1", "the constraint is active at the optimum"),
    # ---- Deep Learning ----
    _p("dl-cross-entropy-derivative", "Cross-Entropy Derivative", "Symbolic", "Deep Learning", "Easy",
       "For a target y = 1, the cross-entropy loss is L = -ln(p) in the prediction p. Find dL/dp.",
       "-1/p", "for 0 < p < 1"),
    _p("dl-batchnorm-normalize", "BatchNorm Normalize", "Tensor", "Deep Learning", "Easy",
       "BatchNorm normalizes with (x − μ)/√(σ² + ε). For x = 2, μ = 1, σ² = 4, ε = 0, compute it.",
       "1/2", "√4 = 2, so (2 − 1)/2"),
    # ---- Probability & Statistics ----
    _p("prob-bayes-disease", "Bayes' Rule", "Tensor", "Probability", "Hard",
       "A disease affects 1% of people. A test is 90% sensitive (P(+|D) = 9/10) with a 10% "
       "false-positive rate (P(+|¬D) = 1/10). Given a positive test, find P(D|+) as an exact fraction.",
       "1/12", "P(D|+) = P(+|D)P(D) / P(+); the base rate dominates"),
    _p("prob-three-coins", "At Least One Head", "Tensor", "Probability", "Easy",
       "Three fair coins are flipped. What is P(at least one head)? Give an exact fraction.",
       "7/8", "use the complement: 1 − P(all tails)"),
    _p("stats-sample-mean", "Sample Mean", "Tensor", "Statistics", "Easy",
       "Find the mean of the sample\n\n    [2, 4, 4, 4, 5, 5, 7, 9]",
       "5", "sum the values and divide by the count"),
    # ---- Signals & DSP ----
    _p("dsp-convolution", "Linear Convolution", "Tensor", "Signals & DSP", "Medium",
       "Compute the linear convolution\n\n    [1, 2, 3] * [1, 1]\n\nEnter the result as a row.",
       "1 3 5 3", "y[n] = Σ x[k]·h[n−k]; the result has length 4"),
    _p("dsp-nyquist-cd", "CD Nyquist Frequency", "Exact", "Signals & DSP", "Easy",
       "CD audio is sampled at 44100 Hz. What is the Nyquist frequency (in Hz)?",
       "22050", "f_s / 2"),
    # ---- Linear Programming ----
    _p("lp-box-min", "LP Minimum over a Box", "Exact", "Linear Programming", "Medium",
       "Minimize  -2*x1 + 3*x2  subject to  0 ≤ x1 ≤ 4, 0 ≤ x2 ≤ 5.\n\n"
       "What is the minimum objective value?",
       "-8", "x1 = 4 (its coefficient is negative), x2 = 0"),
    # ---- Hypothesis Testing ----
    _p("stats-chi-square-df", "Chi-Square Degrees of Freedom", "Exact", "Hypothesis Testing", "Easy",
       "A chi-square goodness-of-fit test is run over 6 categories. How many degrees of freedom?",
       "5", "df = k − 1 for k categories"),
    _p("stats-two-sample-df", "Two-Sample t-Test df", "Exact", "Hypothesis Testing", "Medium",
       "A pooled two-sample t-test has groups of sizes n1 = 10 and n2 = 12. How many degrees of freedom?",
       "20", "df = n1 + n2 − 2"),
    # ---- Decompositions ----
    _p("decomp-cholesky-3x3", "Cholesky of a 3×3", "Tensor", "Decompositions", "Hard",
       "Find the lower-triangular Cholesky factor L (positive diagonal) with L·Lᵀ = A:\n\n"
       "    A = [[4, 2, 2], [2, 10, 4], [2, 4, 6]]",
       "2 0 0; 1 3 0; 1 1 2", "solve row by row: l11 = 2, then the first column, then l22, …"),
    # ---- Graph Theory ----
    _p("graph-cycle-coloring", "Chromatic Number of a Cycle", "Exact", "Graph Theory", "Medium",
       "What is the chromatic number of the cycle graph C_5 (a pentagon)?",
       "3", "an odd cycle needs 3 colours; an even cycle needs 2"),
    # ---- Complex Numbers ----
    _p("complex-i-cubed", "i Cubed", "Tensor", "Complex Numbers", "Easy",
       "Compute i^3. Enter it as `real imag`.",
       "0 -1", "i^3 = i^2·i = −i"),
    # ---- ODEs ----
    _p("ode-exp-decay", "Exponential Decay", "Symbolic", "ODEs", "Easy",
       "Solve y' = -3*y with y(0) = 5. Give y(t).",
       "5*exp(-3*t)", "y(t) = y(0)·e^(k·t) with k = −3"),
]


# --------------------------------------------------------------------------


def main() -> None:
    parser = argparse.ArgumentParser(description="Generate Mamba problem JSON files.")
    parser.add_argument("--count", type=int, default=3, help="problems per template")
    parser.add_argument(
        "--out",
        type=Path,
        default=Path(__file__).resolve().parents[2] / "problems",
        help="output directory (default: <repo>/problems)",
    )
    parser.add_argument("--seed", type=int, default=0, help="RNG seed (deterministic)")
    args = parser.parse_args()

    args.out.mkdir(parents=True, exist_ok=True)
    rng = random.Random(args.seed)

    def write(problem: dict) -> None:
        path = args.out / f"{problem['id']}.json"
        path.write_text(json.dumps(problem, indent=2, ensure_ascii=False) + "\n")

    # Curated problems are emitted once; parametrized templates `--count` times.
    for problem in FIXED:
        write(problem)
    for template in TEMPLATES:
        for i in range(args.count):
            write(template(rng, i))

    written = len(FIXED) + len(TEMPLATES) * args.count
    print(
        f"generated {written} problems "
        f"({len(FIXED)} curated + {len(TEMPLATES)} templates × {args.count}) → {args.out}"
    )
    print("verify them with:  mamba --verify", args.out)


if __name__ == "__main__":
    main()
