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


# ---- Physics (mechanics, energy, waves) -----------------------------------


def t_newton_second_law(rng: random.Random, i: int) -> dict:
    m = rng.randint(2, 20)
    a = rng.randint(1, 12)
    return {
        "id": f"gen-phys-newton-2nd-{i}",
        "title": "Newton's Second Law",
        "domain": "Exact",
        "topic": "Physics",
        "difficulty": "Easy",
        "prompt": f"A net force accelerates a {m} kg mass at {a} m/s². Find the force (in newtons).",
        "expected": str(m * a),
        "answer_kind": "Scalar",
        "hint": "F = m·a",
    }


def t_kinematics_velocity(rng: random.Random, i: int) -> dict:
    u = rng.randint(0, 20)
    a = rng.randint(1, 10)
    t = rng.randint(1, 10)
    return {
        "id": f"gen-phys-velocity-{i}",
        "title": "Final Velocity",
        "domain": "Exact",
        "topic": "Physics",
        "difficulty": "Easy",
        "prompt": (
            f"A body starts at {u} m/s and accelerates uniformly at {a} m/s² for {t} s. "
            "Find its final velocity (m/s)."
        ),
        "expected": str(u + a * t),
        "answer_kind": "Scalar",
        "hint": "v = u + a·t",
    }


def t_kinematics_displacement(rng: random.Random, i: int) -> dict:
    u = rng.randint(0, 15)
    a = 2 * rng.randint(1, 6)  # even, so (1/2)·a·t² is an integer
    t = rng.randint(1, 8)
    s = u * t + (a * t * t) // 2
    return {
        "id": f"gen-phys-displacement-{i}",
        "title": "Displacement under Acceleration",
        "domain": "Exact",
        "topic": "Physics",
        "difficulty": "Medium",
        "prompt": (
            f"A body starts at {u} m/s and accelerates at {a} m/s² for {t} s. "
            "Find the displacement (m)."
        ),
        "expected": str(s),
        "answer_kind": "Scalar",
        "hint": "s = u·t + (1/2)·a·t²",
    }


def t_kinetic_energy(rng: random.Random, i: int) -> dict:
    m = 2 * rng.randint(1, 10)  # even mass → (1/2)·m·v² is an integer
    v = rng.randint(1, 12)
    return {
        "id": f"gen-phys-ke-{i}",
        "title": "Kinetic Energy",
        "domain": "Exact",
        "topic": "Physics",
        "difficulty": "Medium",
        "prompt": f"Find the kinetic energy (in joules) of a {m} kg mass moving at {v} m/s.",
        "expected": str((m * v * v) // 2),
        "answer_kind": "Scalar",
        "hint": "KE = (1/2)·m·v²",
    }


def t_gravitational_pe(rng: random.Random, i: int) -> dict:
    m = rng.randint(1, 20)
    h = rng.randint(1, 20)
    g = 10
    return {
        "id": f"gen-phys-pe-{i}",
        "title": "Gravitational Potential Energy",
        "domain": "Exact",
        "topic": "Physics",
        "difficulty": "Easy",
        "prompt": (
            f"Find the gravitational potential energy (J) of a {m} kg mass raised {h} m. "
            "Take g = 10 m/s²."
        ),
        "expected": str(m * g * h),
        "answer_kind": "Scalar",
        "hint": "PE = m·g·h",
    }


def t_wave_speed(rng: random.Random, i: int) -> dict:
    f = rng.randint(2, 50)
    lam = rng.randint(1, 20)
    return {
        "id": f"gen-phys-wave-speed-{i}",
        "title": "Wave Speed",
        "domain": "Exact",
        "topic": "Physics",
        "difficulty": "Easy",
        "prompt": f"A wave of frequency {f} Hz has wavelength {lam} m. Find its speed (m/s).",
        "expected": str(f * lam),
        "answer_kind": "Scalar",
        "hint": "v = f·λ",
    }


def t_momentum(rng: random.Random, i: int) -> dict:
    m = rng.randint(1, 30)
    v = rng.randint(1, 25)
    return {
        "id": f"gen-phys-momentum-{i}",
        "title": "Linear Momentum",
        "domain": "Exact",
        "topic": "Physics",
        "difficulty": "Easy",
        "prompt": f"Find the momentum (kg·m/s) of a {m} kg mass moving at {v} m/s.",
        "expected": str(m * v),
        "answer_kind": "Scalar",
        "hint": "p = m·v",
    }


def t_projectile_velocity(rng: random.Random, i: int) -> dict:
    u = rng.randint(5, 40)
    return {
        "id": f"gen-phys-projectile-{i}",
        "title": "Velocity from Position",
        "domain": "Symbolic",
        "topic": "Physics",
        "difficulty": "Medium",
        "prompt": (
            f"A ball thrown upward has height s(t) = {u}*t - 5*t^2 (metres, g = 10 m/s²).\n\n"
            "Find its velocity v(t) = ds/dt."
        ),
        "expected": f"{u} - 10*t",
        "answer_kind": "Scalar",
        "hint": "differentiate term by term: d/dt[a*t] = a, d/dt[t^2] = 2*t",
    }


def t_shm_acceleration(rng: random.Random, i: int) -> dict:
    a = rng.randint(2, 6)
    w = rng.randint(2, 5)
    return {
        "id": f"gen-phys-shm-accel-{i}",
        "title": "SHM Acceleration",
        "domain": "Symbolic",
        "topic": "Physics",
        "difficulty": "Hard",
        "prompt": (
            f"A particle in simple harmonic motion has displacement x(t) = {a}*cos({w}*t).\n\n"
            "Find its acceleration a(t) = d²x/dt²."
        ),
        "expected": f"-{a * w * w}*cos({w}*t)",
        "answer_kind": "Scalar",
        "hint": "differentiate twice; note a(t) = -ω²·x(t)",
    }


# ---- Engineering Math (circuits, statics, materials) ----------------------


def t_ohms_law(rng: random.Random, i: int) -> dict:
    cur = rng.randint(1, 15)
    res = rng.randint(1, 30)
    return {
        "id": f"gen-eng-ohms-law-{i}",
        "title": "Ohm's Law",
        "domain": "Exact",
        "topic": "Engineering Math",
        "difficulty": "Easy",
        "prompt": f"A current of {cur} A flows through a {res} Ω resistor. Find the voltage (V).",
        "expected": str(cur * res),
        "answer_kind": "Scalar",
        "hint": "V = I·R",
    }


def t_power_dissipated(rng: random.Random, i: int) -> dict:
    cur = rng.randint(1, 8)
    res = rng.randint(1, 20)
    return {
        "id": f"gen-eng-power-{i}",
        "title": "Resistive Power",
        "domain": "Exact",
        "topic": "Engineering Math",
        "difficulty": "Medium",
        "prompt": f"A current of {cur} A flows through a {res} Ω resistor. Find the power dissipated (W).",
        "expected": str(cur * cur * res),
        "answer_kind": "Scalar",
        "hint": "P = I²·R",
    }


def t_resistors_series(rng: random.Random, i: int) -> dict:
    rs = [rng.randint(1, 50) for _ in range(3)]
    return {
        "id": f"gen-eng-series-r-{i}",
        "title": "Resistors in Series",
        "domain": "Exact",
        "topic": "Engineering Math",
        "difficulty": "Easy",
        "prompt": f"Find the total resistance (Ω) of {rs[0]} Ω, {rs[1]} Ω, and {rs[2]} Ω in series.",
        "expected": str(sum(rs)),
        "answer_kind": "Scalar",
        "hint": "series resistances add: R = R1 + R2 + R3",
    }


def t_resistors_parallel(rng: random.Random, i: int) -> dict:
    r1 = rng.randint(2, 12)
    r2 = rng.randint(2, 12)
    return {
        "id": f"gen-eng-parallel-r-{i}",
        "title": "Resistors in Parallel",
        "domain": "Tensor",
        "topic": "Engineering Math",
        "difficulty": "Medium",
        "prompt": (
            f"Find the equivalent resistance (Ω) of {r1} Ω and {r2} Ω in parallel, as an exact fraction."
        ),
        "expected": frac(Fraction(r1 * r2, r1 + r2)),
        "answer_kind": "Scalar",
        "hint": "1/R = 1/R1 + 1/R2, so R = R1·R2/(R1 + R2)",
    }


def t_rc_time_constant(rng: random.Random, i: int) -> dict:
    res = rng.randint(1, 20)
    cap = rng.randint(1, 20)
    return {
        "id": f"gen-eng-rc-tau-{i}",
        "title": "RC Time Constant",
        "domain": "Exact",
        "topic": "Engineering Math",
        "difficulty": "Easy",
        "prompt": (
            f"An RC circuit has R = {res} kΩ and C = {cap} µF. Find the time constant (in ms)."
        ),
        "expected": str(res * cap),
        "answer_kind": "Scalar",
        "hint": "τ = R·C  (kΩ × µF gives ms)",
    }


def t_capacitor_energy(rng: random.Random, i: int) -> dict:
    cap = 2 * rng.randint(1, 10)  # even, so (1/2)·C·V² is an integer
    v = rng.randint(1, 12)
    return {
        "id": f"gen-eng-cap-energy-{i}",
        "title": "Capacitor Energy",
        "domain": "Exact",
        "topic": "Engineering Math",
        "difficulty": "Medium",
        "prompt": f"Find the energy (J) stored in a {cap} F capacitor charged to {v} V.",
        "expected": str((cap * v * v) // 2),
        "answer_kind": "Scalar",
        "hint": "E = (1/2)·C·V²",
    }


def t_torque(rng: random.Random, i: int) -> dict:
    force = rng.randint(2, 50)
    d = rng.randint(1, 20)
    return {
        "id": f"gen-eng-torque-{i}",
        "title": "Moment of a Force",
        "domain": "Exact",
        "topic": "Engineering Math",
        "difficulty": "Easy",
        "prompt": (
            f"A {force} N force acts perpendicular to a lever {d} m from the pivot. "
            "Find the moment (N·m)."
        ),
        "expected": str(force * d),
        "answer_kind": "Scalar",
        "hint": "moment τ = F·d",
    }


def t_axial_stress(rng: random.Random, i: int) -> dict:
    area = rng.randint(2, 10)
    sigma = rng.randint(2, 20)
    force = sigma * area  # chosen so σ = F/A is an integer
    return {
        "id": f"gen-eng-stress-{i}",
        "title": "Axial Stress",
        "domain": "Exact",
        "topic": "Engineering Math",
        "difficulty": "Medium",
        "prompt": (
            f"An axial force of {force} N acts over a cross-section of {area} mm². "
            "Find the stress (in MPa, i.e. N/mm²)."
        ),
        "expected": str(sigma),
        "answer_kind": "Scalar",
        "hint": "σ = F/A",
    }


def t_hookes_law(rng: random.Random, i: int) -> dict:
    k = rng.randint(2, 50)
    x = rng.randint(1, 12)
    return {
        "id": f"gen-eng-hooke-{i}",
        "title": "Hooke's Law",
        "domain": "Exact",
        "topic": "Engineering Math",
        "difficulty": "Easy",
        "prompt": f"A spring of stiffness {k} N/m is stretched {x} m. Find the restoring force (N).",
        "expected": str(k * x),
        "answer_kind": "Scalar",
        "hint": "F = k·x",
    }


def t_impedance_series_rl(rng: random.Random, i: int) -> dict:
    res = rng.randint(1, 20)
    x = rng.randint(1, 20)  # inductive reactance X_L = ω·L
    return {
        "id": f"gen-eng-impedance-rl-{i}",
        "title": "Series RL Impedance",
        "domain": "Tensor",
        "topic": "Engineering Math",
        "difficulty": "Hard",
        "prompt": (
            f"A {res} Ω resistor is in series with an inductor of reactance X_L = {x} Ω.\n\n"
            "Write the impedance Z = R + jX_L as `real imag`."
        ),
        "expected": f"{res} {x}",
        "answer_kind": "Scalar",
        "hint": "series impedances add: Z = R + jX_L → enter `R X_L`",
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
    # Physics
    t_newton_second_law,
    t_kinematics_velocity,
    t_kinematics_displacement,
    t_kinetic_energy,
    t_gravitational_pe,
    t_wave_speed,
    t_momentum,
    t_projectile_velocity,
    t_shm_acceleration,
    # Engineering Math
    t_ohms_law,
    t_power_dissipated,
    t_resistors_series,
    t_resistors_parallel,
    t_rc_time_constant,
    t_capacitor_energy,
    t_torque,
    t_axial_stress,
    t_hookes_law,
    t_impedance_series_rl,
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
    # ---- Physics ----
    _p("phys-half-life", "Counting Half-Lives", "Exact", "Physics", "Easy",
       "A radioactive sample decays to 1/8 of its initial amount. How many half-lives have elapsed?",
       "3", "after n half-lives a fraction (1/2)^n remains; (1/2)^3 = 1/8"),
    _p("phys-thin-lens", "Thin-Lens Focal Length", "Tensor", "Physics", "Medium",
       "An object 2 m from a thin lens forms an image 3 m away. Using 1/f = 1/u + 1/v, "
       "find the focal length f (m) as an exact fraction.",
       "6/5", "1/f = 1/2 + 1/3 = 5/6, so f = 6/5"),
    _p("phys-shm-velocity", "SHM Velocity", "Symbolic", "Physics", "Hard",
       "A particle in simple harmonic motion has displacement x(t) = 3*sin(2*t). "
       "Find its velocity v(t) = dx/dt.",
       "6*cos(2*t)", "chain rule: d/dt[sin(ω t)] = ω·cos(ω t)"),
    # ---- Engineering Math ----
    _p("eng-decibel-power", "Power Ratio in Decibels", "Exact", "Engineering Math", "Medium",
       "An amplifier raises a signal's power by a factor of 1000. Express the gain in decibels "
       "using 10·log₁₀(ratio).",
       "30", "log₁₀(1000) = 3, so 10·3 = 30 dB"),
    _p("eng-voltage-divider", "Voltage Divider", "Tensor", "Engineering Math", "Medium",
       "A 5 V source drives R1 = 2 Ω and R2 = 2 Ω in series. Find the output voltage across R2, "
       "V_out = V_in·R2/(R1 + R2), as an exact fraction.",
       "5/2", "V_out = 5·2/(2 + 2) = 10/4 = 5/2"),
    _p("eng-rc-discharge", "RC Discharge Rate", "Symbolic", "Engineering Math", "Medium",
       "A discharging capacitor's voltage is V(t) = 10*exp(-t/2). Find the rate of change dV/dt.",
       "-5*exp(-t/2)", "d/dt[a·exp(k·t)] = a·k·exp(k·t), here k = −1/2"),
]


# --------------------------------------------------------------------------


# --------------------------------------------------------------------------
# Recaps — short preliminary reading per concept, injected by problem id.
# Keyed by the template concept (the `gen-<concept>-N` id minus the index) or by
# a fixed problem's full id. Shown in the description pane (toggle: `g`).
# --------------------------------------------------------------------------

RECAPS = {
    # Algebra & calculus
    "expand-binomial": "To expand (x + a)(x + b), distribute every pair (FOIL): "
        "x^2 + (a + b)x + ab. The linear coefficient is the sum of a and b; the constant is their product.",
    "power-derivative": "The power rule: d/dx[a·x^n] = a·n·x^(n-1) — multiply by the exponent, then drop it by one. "
        "It differentiates every polynomial term.",
    # Linear algebra
    "determinant": "For a 2×2 matrix [[a, b], [c, d]] the determinant is ad − bc. It scales signed area under the "
        "map and is nonzero exactly when the matrix is invertible.",
    "transpose": "The transpose Aᵀ reflects a matrix across its diagonal: entry (i, j) ↦ (j, i), turning m×n into "
        "n×m. (AB)ᵀ = BᵀAᵀ.",
    "inverse": "For [[a, b], [c, d]] with det = ad − bc ≠ 0, the inverse is (1/det)·[[d, −b], [−c, a]]: swap the "
        "diagonal, negate the off-diagonal, divide by the determinant.",
    "trace": "The trace of a square matrix is the sum of its diagonal entries. It equals the sum of the "
        "eigenvalues and is unchanged by similarity transforms.",
    "rank": "The rank is the number of linearly independent rows (or columns). A 2×2 matrix has rank 2 when its "
        "determinant is nonzero, rank 1 if singular but nonzero, and rank 0 only when all entries vanish.",
    "cholesky": "A symmetric positive-definite A factors uniquely as A = L·Lᵀ with L lower-triangular, positive "
        "diagonal. Solve row by row: l₁₁ = √A₁₁, l₂₁ = A₂₁/l₁₁, l₂₂ = √(A₂₂ − l₂₁^2).",
    "eigenvalues": "The eigenvalues of a triangular matrix are simply its diagonal entries. In general they are the "
        "roots of det(A − λI) = 0.",
    "linear-system": "A 2×2 system A·x = b is solved by Cramer's rule: each xⱼ = det(Aⱼ)/det(A), where Aⱼ replaces "
        "column j of A with b. It needs det(A) ≠ 0.",
    # Number theory
    "factorization": "Every integer > 1 factors uniquely into primes (Fundamental Theorem of Arithmetic). "
        "Trial-divide by 2, 3, 5, … up to √n; write the result as prime powers, e.g. 360 = 2^3 · 3^2 · 5.",
    "modexp": "Modular exponentiation a^b mod m is computed by repeated squaring. Fermat's little theorem helps: if "
        "m is prime and gcd(a, m) = 1 then a^(m-1) ≡ 1, so reduce the exponent mod (m − 1) first.",
    "gcd": "The greatest common divisor comes from the Euclidean algorithm: gcd(a, b) = gcd(b, a mod b), repeated "
        "until the remainder is 0.",
    "lcm": "The least common multiple satisfies lcm(a, b)·gcd(a, b) = a·b, so lcm = a·b / gcd(a, b).",
    "modinverse": "The modular inverse of a (mod m) is the x with a·x ≡ 1 (mod m). It exists iff gcd(a, m) = 1; the "
        "extended Euclidean algorithm finds it.",
    # Machine learning
    "gradient-linear": "For a linear function f(x) = aᵀx, the gradient is the constant vector ∇f = a — each partial "
        "∂f/∂xⱼ is just aⱼ.",
    "hessian-quadratic": "The Hessian H(i, j) = ∂^2f/∂xᵢ∂xⱼ is the matrix of second partials. For a quadratic it is "
        "constant; a function is convex where its Hessian is positive semidefinite.",
    "ridge-gradient": "Ridge regression adds an L2 penalty: f(w) = (1/2)(w − t)^2 + (λ/2)w^2, so "
        "f'(w) = (1 + λ)w − t. The penalty shrinks weights toward zero.",
    "matmul-grad-shape": "In Y = X·W, backprop gives dL/dW = Xᵀ·(dL/dY), which has the same shape as W. Gradients "
        "always match the shape of what they differentiate.",
    "attention-shape": "Scaled dot-product attention: scores Q·Kᵀ/√d are [n×m]; softmax over the m keys; times "
        "V [m×d] gives output [n×d] — the queries' shape.",
    "gd-step": "Gradient descent updates x ← x − α·∇f(x). For f = x^2, ∇f = 2x, so one step gives (1 − 2α)x. The "
        "step size α sets how far you descend.",
    "conv-output": "A 1-D convolution output length is floor((L − K + 2P)/S) + 1: padding P grows it, stride S "
        "shrinks it.",
    "layernorm-mean": "The mean μ = (Σ xᵢ)/n is the data's balance point. LayerNorm subtracts it so each layer's "
        "activations are centered.",
    "layernorm-var": "The biased variance σ^2 = (Σ xᵢ^2)/n − μ^2 is the mean squared deviation from the mean. "
        "LayerNorm divides by √(σ^2 + ε) to unit-scale.",
    "linear-params": "A dense layer ℝ^a → ℝ^b has a·b weights plus b biases, i.e. a·b + b learnable parameters.",
    "conv-params": "A Conv2D with C_in channels, C_out filters and a K×K kernel has C_in·K·K weights + 1 bias per "
        "filter, times C_out: C_out·(C_in·K^2 + 1).",
    # Convex optimization
    "psd-check": "A symmetric 2×2 [[a, b], [b, c]] is positive semidefinite iff a ≥ 0, c ≥ 0 and ac − b^2 ≥ 0. PSD "
        "matrices are exactly the Hessians of convex quadratics.",
    "lagrangian": "For min f(x) s.t. g(x) = 0, the Lagrangian is L = f + λg, and ∂L/∂x = 0 is a necessary (KKT) "
        "condition. Here ∂/∂x[a·x^2 + λ(x − c)] = 2a·x + λ.",
    "box-lp": "Minimizing c·x over a box 0 ≤ xⱼ ≤ uⱼ is solved coordinate-wise: xⱼ = uⱼ where cⱼ < 0, else 0. The "
        "optimum sits at a corner.",
    "lp-standard-vars": "Standard form turns each ≤-constraint into an equality with one nonnegative slack "
        "variable. With n decision variables and m constraints, the total is n + m.",
    "lp-objective": "An LP objective at a point is the dot product c·x = Σ cⱼxⱼ; for a bounded LP the optimum is "
        "attained at a vertex of the feasible region.",
    # Probability & statistics
    "combination": "C(n, k) = n!/(k!(n − k)!) counts unordered choices of k from n — the permutation count divided "
        "by k! because order doesn't matter.",
    "permutation": "P(n, k) = n!/(n − k)! counts ordered arrangements of k items from n: n·(n−1)···(n−k+1).",
    "factorial": "n! = n·(n−1)···1 is the number of orderings of n distinct items, with 0! = 1; it outgrows any "
        "exponential.",
    "dice-sum": "For two fair dice the 36 ordered outcomes are equally likely, so P(sum = s) = (pairs summing to "
        "s)/36 — peaking at s = 7.",
    "expected-value": "The expected value E[X] = Σ pᵢxᵢ is the probability-weighted average — the long-run mean. "
        "With weights wᵢ, E[X] = (Σ wᵢxᵢ)/(Σ wᵢ).",
    "variance": "Var(X) = E[X^2] − E[X]^2 measures spread: the mean of the squares minus the square of the mean. "
        "It is always ≥ 0.",
    "z-score": "A z-score (x − μ)/σ rescales a value to standard-deviation units from the mean: z = 0 is average, "
        "|z| = 2 is two SDs out.",
    "standard-error": "The standard error of the mean is SE = σ/√n: the sample mean's spread shrinks with √n, so "
        "quadrupling n halves the SE.",
    "t-test-df": "A one-sample t-test estimates the variance from the data, costing one degree of freedom, so "
        "df = n − 1.",
    # Signals & DSP
    "conv-length": "Linearly convolving signals of lengths M and N gives length M + N − 1 — the supports add minus "
        "the one shared sample.",
    "nyquist": "The Nyquist frequency is half the sampling rate, f_s/2. By the sampling theorem, only frequencies "
        "below it are representable; higher ones alias.",
    "fft-stages": "A radix-2 FFT of size N = 2^k splits into log₂(N) = k stages of butterflies, giving "
        "O(N log N) instead of O(N^2).",
    "convolution": "Linear convolution y[n] = Σ x[k]·h[n − k] slides one signal across the other, multiplying and "
        "summing — how an LTI system's output is built from its impulse response.",
    # Graph theory
    "complete-edges": "The complete graph K_n joins every pair of its n vertices, giving C(n, 2) = n(n − 1)/2 "
        "edges — the most a simple graph can have.",
    "cayley": "Cayley's formula counts the labelled spanning trees of K_n as n^(n-2); the count grows explosively "
        "with n.",
    "walks": "Powers of the adjacency matrix count walks: (A^k)[i][j] is the number of length-k walks from i to j, "
        "so A^2 gives length-2 walk counts.",
    # Complex numbers
    "complex-mul": "Multiply complex numbers by distributing with i^2 = −1: (a + bi)(c + di) = (ac − bd) + "
        "(ad + bc)i.",
    "power-of-i": "Powers of i cycle with period 4: i = i, i^2 = −1, i^3 = −i, i^4 = 1, then repeat — so i^n depends "
        "only on n mod 4.",
    "modulus": "The modulus |a + bi| = √(a^2 + b^2) is the distance from the origin in the complex plane "
        "(Pythagoras), and is multiplicative: |zw| = |z||w|.",
    # ODEs
    "ode-roots": "A linear ODE y'' + b·y' + c·y = 0 has characteristic equation r^2 + b·r + c = 0; its roots give "
        "the exponential modes e^(r·t).",
    "ode-equilibrium": "An equilibrium of y' = f(y) is where f(y) = 0. For y' = a·y + b it is y* = −b/a — the value "
        "the system settles toward when a < 0.",
    "exp-growth": "The ODE y' = k·y has solution y(t) = y(0)·exp(k·t): exponential growth when k > 0, decay when "
        "k < 0 — the rate of change is proportional to the amount.",
    # Curated ML problems
    "ml-sigmoid-derivative": "The logistic sigmoid σ(x) = 1/(1 + exp(−x)) squashes ℝ into (0, 1). Its derivative is "
        "self-referential: σ'(x) = σ(x)(1 − σ(x)), maximal (1/4) at x = 0.",
    "ml-tanh-derivative": "tanh(x) maps ℝ into (−1, 1) and has derivative 1 − tanh(x)^2. Like the sigmoid it "
        "saturates for large |x|, which can slow learning.",
    "ml-softplus-derivative": "Softplus ln(1 + exp(x)) is a smooth ReLU; its derivative is exactly the sigmoid "
        "1/(1 + exp(−x)).",
    "ml-mse-gradient": "Mean-squared error L = (1/2)(y − x)^2 has gradient dL/dx = x − y (prediction minus target); "
        "descending it moves the prediction toward the target.",
    "ml-bce-logit-gradient": "With a sigmoid output p = σ(z) and binary cross-entropy, the gradient w.r.t. the logit "
        "collapses to dL/dz = p − y.",
    "ml-log-likelihood-derivative": "Log-likelihoods are built from ln; the basic derivative d/dx ln(x) = 1/x "
        "(x > 0) underlies maximum-likelihood gradients.",
    "ml-l2-gradient": "An L2 penalty R(w) = (λ/2)w^2 has gradient λw — weight decay, a force toward zero "
        "proportional to the weight.",
    "ml-gram-matrix": "The Gram matrix XᵀX collects all pairwise inner products of X's columns; it is symmetric "
        "PSD and appears in least squares and kernels.",
    "ml-softmax-jacobian": "Softmax s has Jacobian J(i, j) = sᵢ(δᵢⱼ − sⱼ): diagonal sᵢ(1 − sᵢ), off-diagonal −sᵢsⱼ. "
        "At equal logits s = (1/2, 1/2).",
    "ml-softmax-ce-gradient": "Softmax with cross-entropy has the famous gradient dL/dz = softmax(z) − y — predicted "
        "probabilities minus the one-hot truth.",
    # Curated convex / DL problems
    "convex-quartic-second-derivative": "A function is convex where f'' ≥ 0. For x^4, f'' = 12x^2 ≥ 0 everywhere, so "
        "it is convex.",
    "convex-log-barrier-gradient": "Interior-point methods replace x > 0 by the barrier −ln(x), which blows up at "
        "the boundary; its gradient is −1/x.",
    "convex-exp-derivative": "exp is convex (second derivative > 0) and is its own derivative: d/dx exp(x) = exp(x).",
    "convex-negentropy-derivative": "Negative entropy x·ln(x) (x > 0) is convex; by the product rule its derivative "
        "is ln(x) + 1.",
    "convex-kkt-optimum": "The KKT conditions extend Lagrange multipliers to inequalities. For min (1/2)x^2 s.t. "
        "x ≥ 1 the constraint is active, so x* = 1.",
    "dl-cross-entropy-derivative": "For target y = 1 the loss is −ln(p) in the predicted probability p; dL/dp = −1/p "
        "is large when p is small (confidently wrong).",
    "dl-batchnorm-normalize": "BatchNorm standardizes activations as (x − μ)/√(σ^2 + ε) using the batch statistics, "
        "then scales and shifts.",
    # Curated probability / statistics problems
    "prob-bayes-disease": "Bayes' rule updates beliefs: P(D|+) = P(+|D)P(D)/P(+). A rare condition makes most "
        "positives false — the base-rate effect.",
    "prob-three-coins": "For 'at least one' events use the complement: P(≥1 head) = 1 − P(all tails).",
    "stats-chi-square-df": "A chi-square goodness-of-fit test over k categories has k − 1 degrees of freedom (the "
        "counts must sum to n).",
    "stats-two-sample-df": "A pooled two-sample t-test estimates one variance from both groups, costing two "
        "degrees of freedom: df = n₁ + n₂ − 2.",
    # Curated graph problem
    "graph-cycle-coloring": "The chromatic number is the fewest colours so adjacent vertices differ. A cycle Cₙ "
        "needs 2 colours if n is even and 3 if odd.",
    # Physics
    "phys-newton-2nd": "Newton's second law: the net force equals mass times acceleration, F = m·a. Force is in "
        "newtons (N) when m is in kg and a in m/s².",
    "phys-velocity": "For constant acceleration, velocity grows linearly: v = u + a·t — start speed plus "
        "acceleration times elapsed time. One of the SUVAT equations.",
    "phys-displacement": "With constant acceleration the distance covered is s = u·t + (1/2)·a·t²: the steady part "
        "u·t plus the area of the velocity ramp. A SUVAT staple.",
    "phys-ke": "Kinetic energy is the work needed to reach a speed: KE = (1/2)·m·v². It scales with the *square* of "
        "speed, so doubling v quadruples the energy.",
    "phys-pe": "Gravitational potential energy near the surface is PE = m·g·h — the work to lift a mass m through a "
        "height h against gravity g (≈ 9.8, often 10, m/s²).",
    "phys-wave-speed": "A wave's speed is frequency times wavelength: v = f·λ. It ties how often crests pass (f) to "
        "how far apart they are (λ).",
    "phys-momentum": "Linear momentum p = m·v measures 'quantity of motion'. It is conserved in collisions, which is "
        "why it matters more than speed alone.",
    "phys-projectile": "Velocity is the time-derivative of position: v(t) = ds/dt. Differentiate term by term — "
        "d/dt[a·t] = a and d/dt[t²] = 2t — so s = u·t − 5t² gives v = u − 10t.",
    "phys-shm-accel": "In simple harmonic motion x(t) = A·cos(ω t), differentiating twice gives acceleration "
        "a = −A·ω²·cos(ω t) = −ω²·x — always pointing back toward equilibrium.",
    "phys-half-life": "Each half-life halves the amount, so after n half-lives a fraction (1/2)ⁿ remains. Invert it: "
        "1/8 = (1/2)³ means 3 half-lives.",
    "phys-thin-lens": "The thin-lens equation 1/f = 1/u + 1/v links object distance u, image distance v, and focal "
        "length f. Add the reciprocals, then invert for f.",
    "phys-shm-velocity": "Velocity is the derivative of displacement. For x = A·sin(ω t), the chain rule gives "
        "v = A·ω·cos(ω t): the speed peaks as the particle crosses equilibrium.",
    # Engineering Math
    "eng-ohms-law": "Ohm's law relates voltage, current, and resistance: V = I·R. It is the workhorse of circuit "
        "analysis — volts equal amps times ohms.",
    "eng-power": "Power dissipated in a resistor is P = I²·R (equivalently V²/R or V·I). It grows with the *square* "
        "of current — why thick wires carry heavy loads.",
    "eng-series-r": "Resistors in series share the same current and their resistances add: R = R₁ + R₂ + R₃. The "
        "total always exceeds the largest one.",
    "eng-parallel-r": "Resistors in parallel share the same voltage; conductances add, so 1/R = 1/R₁ + 1/R₂, giving "
        "R = R₁·R₂/(R₁ + R₂) for two. The total is below the smallest one.",
    "eng-rc-tau": "An RC circuit charges/discharges with time constant τ = R·C — the time to reach ~63%. Handily, "
        "kΩ × µF gives milliseconds.",
    "eng-cap-energy": "A charged capacitor stores energy E = (1/2)·C·V², analogous to (1/2)·k·x² for a spring. It "
        "grows with the square of voltage.",
    "eng-torque": "The moment (torque) of a perpendicular force about a pivot is τ = F·d — force times lever arm. "
        "Longer levers multiply turning effect.",
    "eng-stress": "Axial stress is force per unit area: σ = F/A. In N/mm² it reads directly as MPa; it normalizes "
        "the load by cross-section, so it (not raw force) drives deformation.",
    "eng-hooke": "Hooke's law: a spring's restoring force is proportional to its extension, F = k·x, with stiffness "
        "k. Valid in the elastic (small-x) regime.",
    "eng-impedance-rl": "In AC circuits, impedance generalizes resistance to complex numbers: a series R and "
        "inductor give Z = R + jX_L, with reactance X_L = ω·L. Magnitude √(R² + X_L²) sets the current.",
    "eng-decibel-power": "Decibels express ratios logarithmically: gain = 10·log₁₀(P_out/P_in) for power. A ×1000 "
        "ratio is 30 dB; every ×10 adds 10 dB.",
    "eng-voltage-divider": "Two series resistors split the source voltage in proportion: V_out = V_in·R₂/(R₁ + R₂). "
        "The larger resistor drops the larger share.",
    "eng-rc-discharge": "A discharging RC capacitor follows V(t) = V₀·exp(−t/τ). Differentiating, dV/dt = "
        "−(V₀/τ)·exp(−t/τ): the rate is steepest at the start and decays with the voltage.",
}

# Aliases: fixed problems that share a concept with a template recap.
RECAPS["convex-hessian-quadratic"] = RECAPS["hessian-quadratic"]
RECAPS["convex-gradient-linear"] = RECAPS["gradient-linear"]
RECAPS["stats-sample-mean"] = RECAPS["layernorm-mean"]
RECAPS["dsp-convolution"] = RECAPS["convolution"]
RECAPS["dsp-nyquist-cd"] = RECAPS["nyquist"]
RECAPS["lp-box-min"] = RECAPS["box-lp"]
RECAPS["decomp-cholesky-3x3"] = RECAPS["cholesky"]
RECAPS["complex-i-cubed"] = RECAPS["power-of-i"]
RECAPS["ode-exp-decay"] = RECAPS["exp-growth"]


# Expanded recaps for the hardest problems: more depth, a worked mini-example,
# and a reference. Used in place of the base recap when difficulty == "Hard".
RECAPS_HARD = {
    "cholesky": "A symmetric positive-definite A factors uniquely as A = L·Lᵀ with L lower-triangular and "
        "positive diagonal — the 'matrix square root' used to solve systems and sample Gaussians.\n\n"
        "Solve column by column: l₁₁ = √A₁₁; then l_{i1} = A_{i1}/l₁₁; and l_{jj} = √(A_{jj} − Σ_{k<j} l_{jk}²).\n\n"
        "Example — A = [[4, 2], [2, 5]]: l₁₁ = √4 = 2, l₂₁ = 2/2 = 1, l₂₂ = √(5 − 1) = 2, so L = [[2, 0], [1, 2]] "
        "(check L·Lᵀ ✓).\n\nReference: Trefethen & Bau, Numerical Linear Algebra, Lecture 23.",
    "inverse": "For a 2×2 [[a, b], [c, d]] with det = ad − bc ≠ 0, the inverse is (1/det)·[[d, −b], [−c, a]]: "
        "swap the diagonal, negate the off-diagonal, divide by the determinant. A·A⁻¹ = I.\n\n"
        "Example — A = [[4, 7], [2, 6]]: det = 24 − 14 = 10, so A⁻¹ = (1/10)[[6, −7], [−2, 4]] = "
        "[[3/5, −7/10], [−1/5, 2/5]].\n\nExact fractions avoid rounding. Reference: Strang, Introduction to "
        "Linear Algebra, §2.5.",
    "linear-system": "A 2×2 system A·x = b is solved by Cramer's rule: xⱼ = det(Aⱼ)/det(A), where Aⱼ replaces "
        "column j of A with b. It needs det(A) ≠ 0.\n\nExample — x + 2y = 5, 3x + 4y = 6: det(A) = 4 − 6 = −2; "
        "x = (5·4 − 2·6)/−2 = −4; y = (1·6 − 5·3)/−2 = 9/2.\n\nFor larger systems use Gaussian elimination "
        "(Cramer's is O(n!)). Reference: Strang, Introduction to Linear Algebra.",
    "cayley": "Cayley's formula: K_n has exactly n^(n-2) labelled spanning trees — a count that explodes with n.\n\n"
        "It follows from the Prüfer code, a bijection between labelled trees on n vertices and length-(n−2) strings "
        "over {1,…,n}; there are n^(n−2) of them.\n\nExample — K₄ has 4² = 16 spanning trees.\n\nReference: Aigner & "
        "Ziegler, Proofs from THE BOOK (Cayley's formula).",
    "convolution": "Linear convolution y[n] = Σₖ x[k]·h[n−k] flips h, slides it across x, and sums the overlap — "
        "exactly the output of an LTI system with impulse response h. Length is len(x)+len(h)−1.\n\n"
        "Example — x = [1, 2, 3], h = [1, 1]: y = [1, 1+2, 2+3, 3] = [1, 3, 5, 3].\n\nConvolution in time is "
        "multiplication in frequency (the convolution theorem), which the FFT exploits. Reference: Oppenheim & "
        "Schafer, Discrete-Time Signal Processing, ch. 2.",
    "modexp": "Modular exponentiation a^b mod m uses square-and-multiply in O(log b) steps, never forming a^b. "
        "Fermat's little theorem helps for prime m with gcd(a, m) = 1: a^(m-1) ≡ 1, so reduce the exponent "
        "mod (m − 1).\n\nExample — 7^128 mod 13: 128 ≡ 8 (mod 12), so it is 7^8; squaring, 7²≡10, 7⁴≡9, 7⁸≡81≡3 "
        "(mod 13).\n\nReference: CLRS, Introduction to Algorithms, §31.6.",
    "modinverse": "The modular inverse of a (mod m) is the x with a·x ≡ 1 (mod m); it exists iff gcd(a, m) = 1. "
        "The extended Euclidean algorithm finds (x, y) with a·x + m·y = 1, so x mod m is the inverse. For prime m, "
        "Fermat gives a⁻¹ ≡ a^(m-2).\n\nExample — 3⁻¹ mod 7: 3·5 = 15 ≡ 1 (mod 7), so it is 5.\n\nReference: "
        "CLRS §31.4.",
    "ml-bce-logit-gradient": "For a sigmoid output p = σ(z) and binary cross-entropy "
        "L = −[y·ln p + (1−y)·ln(1−p)], the gradient w.r.t. the logit z simplifies to dL/dz = σ(z) − y = p − y — the "
        "σ′ = σ(1−σ) factors cancel the denominators.\n\nExample — y = 1, z = 0: p = 1/2, so dL/dz = −1/2 (push z "
        "up).\n\nThis 'predicted minus target' form is why logits are what you backprop. Reference: Goodfellow, "
        "Bengio & Courville, Deep Learning, §6.2.",
    "ml-softmax-jacobian": "For softmax s = softmax(z), the Jacobian is J(i, j) = sᵢ(δᵢⱼ − sⱼ): diagonal "
        "sᵢ(1 − sᵢ), off-diagonal −sᵢsⱼ. It is symmetric and each row sums to 0 (softmax outputs sum to 1).\n\n"
        "Example — z = [0, 0] → s = [1/2, 1/2], so J = [[1/4, −1/4], [−1/4, 1/4]].\n\nWith cross-entropy it collapses "
        "to the p − y gradient. Reference: Bishop, Pattern Recognition and ML, §4.3.4.",
    "prob-bayes-disease": "Bayes' rule inverts conditionals: P(D|+) = P(+|D)·P(D) / P(+), with "
        "P(+) = P(+|D)P(D) + P(+|¬D)P(¬D). A low base rate makes most positives false — the base-rate fallacy.\n\n"
        "Example — P(D)=0.01, P(+|D)=0.9, P(+|¬D)=0.1: P(+) = 0.009 + 0.099 = 0.108, so P(D|+) = 0.009/0.108 = 1/12 "
        "≈ 8%.\n\nReference: Blitzstein & Hwang, Introduction to Probability, ch. 2.",
    "variance": "Variance Var(X) = E[(X − μ)²] = E[X²] − E[X]² is the mean squared deviation from the mean: the "
        "average of the squares minus the square of the average. Always ≥ 0; √Var (the standard deviation) restores "
        "the original units.\n\nExample — [1, 2, 3] equal-weight: μ = 2, E[X²] = 14/3, Var = 14/3 − 4 = 2/3.\n\nFor "
        "a sample, dividing by n−1 (Bessel) is unbiased. Reference: Wasserman, All of Statistics, §3.",
    "phys-shm-accel": "Simple harmonic motion is x(t) = A·cos(ω t): a restoring force proportional to displacement. "
        "Differentiate once for velocity v = −A·ω·sin(ω t), again for acceleration a = −A·ω²·cos(ω t) = −ω²·x — "
        "always toward equilibrium, with magnitude set by ω².\n\n"
        "Example — x(t) = 3·cos(2t): v = −6·sin(2t), a = −12·cos(2t) (= −4·x, since ω² = 4).\n\n"
        "The defining relation a = −ω²·x is what makes the motion sinusoidal with angular frequency ω. "
        "Reference: Kleppner & Kolenkow, An Introduction to Mechanics, ch. 11.",
    "phys-shm-velocity": "Velocity is the time-derivative of displacement. For SHM x(t) = A·sin(ω t), the chain "
        "rule gives v(t) = A·ω·cos(ω t): the speed is greatest (A·ω) as the particle passes through equilibrium "
        "and zero at the turning points.\n\n"
        "Example — x(t) = 3·sin(2t): v(t) = 6·cos(2t); peak speed 6 at t = 0.\n\n"
        "Energy check: max KE = (1/2)m(Aω)² equals the max PE = (1/2)kA² since k = mω². "
        "Reference: Kleppner & Kolenkow, An Introduction to Mechanics, ch. 11.",
    "eng-impedance-rl": "In AC steady state, impedance Z = R + jX generalizes resistance to complex numbers. A "
        "resistor contributes R (real); an inductor contributes a positive reactance X_L = ω·L (imaginary), a "
        "capacitor a negative one X_C = −1/(ω·C). In series they add.\n\n"
        "Example — R = 3 Ω in series with X_L = 4 Ω: Z = 3 + j4, magnitude |Z| = √(3² + 4²) = 5 Ω and phase "
        "arctan(4/3) ≈ 53°, so the current lags the voltage.\n\n"
        "The magnitude sets the current (I = V/|Z|); the phase sets the power factor. Reference: Irwin & Nelms, "
        "Basic Engineering Circuit Analysis, ch. 8.",
}


RECAPS_HARD["decomp-cholesky-3x3"] = RECAPS_HARD["cholesky"]
RECAPS_HARD["dsp-convolution"] = RECAPS_HARD["convolution"]


def recap_for(problem_id: str, difficulty: str = "") -> str | None:
    """Looks up a problem's recap by its concept key (the id minus the `gen-`
    prefix and trailing index for templates; the full id for fixed problems).
    Hard problems get the expanded recap (worked example + reference) when one
    exists."""
    key = problem_id[4:].rsplit("-", 1)[0] if problem_id.startswith("gen-") else problem_id
    if difficulty == "Hard" and key in RECAPS_HARD:
        return RECAPS_HARD[key]
    return RECAPS.get(key)


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

    missing_recaps: set[str] = set()

    def write(problem: dict) -> None:
        if not problem.get("recap"):
            recap = recap_for(problem["id"], problem.get("difficulty", ""))
            if recap:
                problem["recap"] = recap
            else:
                missing_recaps.add(problem["id"])
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
    if missing_recaps:
        print(f"⚠ {len(missing_recaps)} problem(s) without a recap:", ", ".join(sorted(missing_recaps)))
    print("verify them with:  mamba --verify", args.out)


if __name__ == "__main__":
    main()
