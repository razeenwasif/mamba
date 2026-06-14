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
from math import gcd
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
        "prompt": f"Find the determinant:\n\n    ⎡ {a:>3} {b:>3} ⎤\n    ⎣ {c:>3} {d:>3} ⎦",
        "expected": str(det),
        "answer_kind": "Scalar",
        "hint": "for [[a,b],[c,d]], det = ad - bc",
    }


def t_transpose(rng: random.Random, i: int) -> dict:
    rows, cols = rng.randint(2, 3), rng.randint(2, 3)
    m = [[rng.randint(-9, 9) for _ in range(cols)] for _ in range(rows)]
    transposed = [[m[r][c] for r in range(rows)] for c in range(cols)]
    expected = "; ".join(" ".join(str(x) for x in row) for row in transposed)
    body = "\n".join("    " + " ".join(f"{x:>3}" for x in row) for row in m)
    return {
        "id": f"gen-transpose-{i}",
        "title": "Transpose a Matrix",
        "domain": "Tensor",
        "topic": "Linear Algebra",
        "difficulty": "Medium",
        "prompt": f"Write the transpose Aᵀ of:\n\n{body}",
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
        "prompt": (
            f"Find the inverse:\n\n    ⎡ {a:>3} {b:>3} ⎤\n    ⎣ {c:>3} {d:>3} ⎦\n\n"
            "Enter exact fractions."
        ),
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


TEMPLATES = [
    t_expand_binomial,
    t_power_derivative,
    t_determinant_2x2,
    t_transpose,
    t_inverse_2x2,
    t_prime_factorization,
    t_modular_exponentiation,
    t_gcd,
]


# --------------------------------------------------------------------------


def main() -> None:
    parser = argparse.ArgumentParser(description="Generate Mamba problem JSON files.")
    parser.add_argument("--count", type=int, default=2, help="problems per template")
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

    written = 0
    for template in TEMPLATES:
        for i in range(args.count):
            problem = template(rng, i)
            path = args.out / f"{problem['id']}.json"
            path.write_text(json.dumps(problem, indent=2, ensure_ascii=False) + "\n")
            written += 1

    print(
        f"generated {written} problems "
        f"({len(TEMPLATES)} templates × {args.count}) → {args.out}"
    )
    print("verify them with:  mamba --verify", args.out)


if __name__ == "__main__":
    main()
