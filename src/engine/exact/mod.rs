//! # Exact-arithmetic pipeline (number theory)
//!
//! Verifies answers that must be *exactly* right and may be astronomically
//! large — prime factorizations, modular powers, gcds — using arbitrary-
//! precision integers ([`num_bigint`]) so nothing ever overflows a machine
//! word. Two answer shapes are supported, selected by
//! [`AnswerKind`](crate::storage::problem::AnswerKind):
//!
//! * [`AnswerKind::Scalar`] — a single integer; compared exactly.
//! * [`AnswerKind::Factorization`] — a product of prime powers like
//!   `2^3 * 3^2 * 5`; the pipeline checks the product equals the target *and*
//!   that every base is prime (so writing the number unfactored is rejected).

use num_bigint::{BigInt, BigUint};
use num_integer::Integer;
use num_traits::{One, Zero};

use crate::engine::Verdict;
use crate::storage::problem::AnswerKind;

/// Verifies `input` against `expected` using exact big-integer arithmetic.
/// `kind` decides whether `expected` is a plain integer or a factorization
/// target.
pub fn verify(expected: &str, input: &str, kind: AnswerKind) -> Verdict {
    match kind {
        AnswerKind::Scalar => verify_scalar(expected, input),
        AnswerKind::Factorization => verify_factorization(expected, input),
    }
}

/// Exact equality of two arbitrary-precision integers (sign and magnitude).
fn verify_scalar(expected: &str, input: &str) -> Verdict {
    let want: BigInt = match expected.trim().parse() {
        Ok(n) => n,
        Err(e) => return Verdict::parse_error(format!("internal: expected '{expected}' ({e})"), None),
    };
    match input.trim().parse::<BigInt>() {
        Ok(got) if got == want => Verdict::Correct,
        Ok(got) => Verdict::incorrect(format!("got {got}, expected {want}")),
        Err(_) => Verdict::parse_error(
            format!("'{input}' is not an integer"),
            Some("enter a whole number, e.g. 42 (no commas, no spaces)".into()),
        ),
    }
}

/// Verifies that `input` is a valid prime factorization of the integer named
/// in `expected`: the product of the prime powers must equal the target and
/// every base must be prime.
fn verify_factorization(expected: &str, input: &str) -> Verdict {
    let target: BigUint = match expected.trim().parse() {
        Ok(n) => n,
        Err(e) => return Verdict::parse_error(format!("internal: target '{expected}' ({e})"), None),
    };

    let factors = match parse_factors(input) {
        Ok(f) => f,
        Err(e) => {
            return Verdict::parse_error(
                format!("couldn't parse factorization: {e}"),
                Some("write prime powers like 2^3 * 3^2 * 5 (a prime factorizes to itself)".into()),
            );
        }
    };

    // 1. The product of the claimed factors must reconstruct the target.
    let mut product = BigUint::one();
    for (base, exp) in &factors {
        product *= num_traits::pow(base.clone(), *exp as usize);
    }
    if product != target {
        return Verdict::incorrect(format!(
            "those factors multiply to {product}, but the number is {target}"
        ));
    }

    // 2. Every base must actually be prime — this is what rejects writing the
    //    composite number back out unfactored.
    for (base, _) in &factors {
        if base < &BigUint::from(2u32) || !is_probable_prime(base) {
            return Verdict::incorrect(format!("{base} is not prime — factor it further"));
        }
    }

    Verdict::Correct
}

/// Parses `2^3 * 3^2 * 5` (or whitespace-separated `2 2 2 3 3 5`, or a bare
/// number) into `(base, exponent)` pairs.
fn parse_factors(src: &str) -> Result<Vec<(BigUint, u32)>, String> {
    let tokens: Vec<&str> = src
        .split(['*', '×', ' ', '\t', '\n'])
        .filter(|t| !t.is_empty())
        .collect();
    if tokens.is_empty() {
        return Err("no factors found".into());
    }

    tokens
        .into_iter()
        .map(|tok| {
            let (base_str, exp) = match tok.split_once('^') {
                Some((b, e)) => {
                    let exp: u32 = e
                        .trim()
                        .parse()
                        .map_err(|_| format!("'{e}' is not a valid exponent"))?;
                    if exp == 0 {
                        return Err(format!("exponent 0 in '{tok}' is meaningless"));
                    }
                    (b.trim(), exp)
                }
                None => (tok.trim(), 1),
            };
            let base: BigUint = base_str
                .parse()
                .map_err(|_| format!("'{base_str}' is not a non-negative integer"))?;
            Ok((base, exp))
        })
        .collect()
}

/// Deterministic Miller–Rabin primality test.
///
/// The first twelve primes as witnesses make this **deterministic** (no false
/// positives) for every `n < 3.3 × 10^24`, and an extremely strong
/// probabilistic test beyond that — more than enough for a practice tool.
fn is_probable_prime(n: &BigUint) -> bool {
    let two = BigUint::from(2u32);
    if n < &two {
        return false;
    }
    // Trial-divide by the small primes (also handles n being one of them).
    const WITNESSES: [u32; 12] = [2, 3, 5, 7, 11, 13, 17, 19, 23, 29, 31, 37];
    for &p in &WITNESSES {
        let bp = BigUint::from(p);
        if n == &bp {
            return true;
        }
        if (n % &bp).is_zero() {
            return false;
        }
    }

    // Write n - 1 = d · 2^s with d odd.
    let one = BigUint::one();
    let n_minus_one = n - &one;
    let mut d = n_minus_one.clone();
    let mut s = 0u32;
    while d.is_even() {
        d = &d / &two;
        s += 1;
    }

    // n is larger than every witness here, so each `a` is a valid base.
    'witness: for &a in &WITNESSES {
        let a = BigUint::from(a);
        let mut x = a.modpow(&d, n);
        if x == one || x == n_minus_one {
            continue;
        }
        for _ in 0..s.saturating_sub(1) {
            x = x.modpow(&two, n);
            if x == n_minus_one {
                continue 'witness;
            }
        }
        return false; // composite: no witness round hit n-1
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scalar_compares_exactly_even_for_huge_values() {
        let big = "123456789012345678901234567890";
        assert!(verify(big, big, AnswerKind::Scalar).is_correct());
        assert!(matches!(
            verify(big, "123", AnswerKind::Scalar),
            Verdict::Incorrect { .. }
        ));
    }

    #[test]
    fn prime_factorizes_to_itself() {
        // 104729 is the 10 000th prime.
        assert!(verify("104729", "104729", AnswerKind::Factorization).is_correct());
        // Writing a composite unfactored is rejected.
        assert!(matches!(
            verify("360", "360", AnswerKind::Factorization),
            Verdict::Incorrect { .. }
        ));
    }

    #[test]
    fn accepts_a_correct_composite_factorization() {
        // 360 = 2^3 · 3^2 · 5.
        assert!(verify("360", "2^3 * 3^2 * 5", AnswerKind::Factorization).is_correct());
        assert!(verify("360", "2 2 2 3 3 5", AnswerKind::Factorization).is_correct());
        // Right product, but a non-prime base must be rejected.
        assert!(matches!(
            verify("360", "8 * 9 * 5", AnswerKind::Factorization),
            Verdict::Incorrect { .. }
        ));
        // Wrong product entirely.
        assert!(matches!(
            verify("360", "2^3 * 3^2", AnswerKind::Factorization),
            Verdict::Incorrect { .. }
        ));
    }

    #[test]
    fn primality_test_matches_known_values() {
        let prime = |s: &str| is_probable_prime(&s.parse::<BigUint>().unwrap());
        assert!(prime("2"));
        assert!(prime("104729"));
        assert!(prime("2147483647")); // Mersenne prime M31
        assert!(!prime("1"));
        assert!(!prime("104730"));
        assert!(!prime("2147483649"));
    }
}
