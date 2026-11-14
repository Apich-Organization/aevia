//! Number theory, combinatorics and elementary geometry (§8.16).
//!
//! Exact integer arithmetic throughout: `binomial(60, 30)` is a 17-digit
//! integer, not a float, and `factor` returns prime powers rather than a float
//! approximation of them.

use num_bigint::BigInt;
use num_traits::One;
use num_traits::Signed;
use num_traits::Zero;

/// The prime factorisation of `n`, as `(prime, exponent)` pairs in increasing
/// order.
///
/// Trial division up to 10^6, then Pollard's rho for the rest: a 64-bit input
/// is fully factored in milliseconds, and `n <= 1` has no factors.
#[must_use]
pub fn factor(n: i64) -> Vec<(i64, u32)> {
    let mut remaining = n.unsigned_abs();
    let mut out = Vec::new();
    if remaining <= 1 {
        return out;
    }
    let mut candidate = 2u64;
    while candidate * candidate <= remaining && candidate <= 1_000_000 {
        if remaining.is_multiple_of(candidate) {
            let mut exponent = 0;
            while remaining.is_multiple_of(candidate) {
                remaining /= candidate;
                exponent += 1;
            }
            out.push((candidate as i64, exponent));
        }
        candidate += if candidate == 2 { 1 } else { 2 };
    }
    if remaining > 1 {
        // The remainder is either prime or has two factors above 10^6.
        let mut rest = remaining;
        let mut stack = vec![rest];
        while let Some(value) = stack.pop() {
            if value == 1 {
                continue;
            }
            if is_prime(value) {
                push_factor(&mut out, value);
                continue;
            }
            let divisor = pollard_rho(value);
            stack.push(divisor);
            stack.push(value / divisor);
            rest = 1;
        }
        let _ = rest;
    }
    out.sort_by_key(|(prime, _)| *prime);
    // Merge duplicates that the rho path may have produced out of order.
    let mut merged: Vec<(i64, u32)> = Vec::with_capacity(out.len());
    for (prime, exponent) in out {
        match merged.last_mut() {
            | Some((last, count)) if *last == prime => *count += exponent,
            | _ => merged.push((prime, exponent)),
        }
    }
    merged
}

fn push_factor(
    out: &mut Vec<(i64, u32)>,
    prime: u64,
) {
    for entry in out.iter_mut() {
        if entry.0 as u64 == prime {
            entry.1 += 1;
            return;
        }
    }
    out.push((prime as i64, 1));
}

/// A deterministic Miller–Rabin primality test, exact for every `u64`.
#[must_use]
pub fn is_prime(n: u64) -> bool {
    if n < 2 {
        return false;
    }
    for small in [2u64, 3, 5, 7, 11, 13, 17, 19, 23, 29, 31, 37] {
        if n == small {
            return true;
        }
        if n.is_multiple_of(small) {
            return false;
        }
    }
    let mut d = n - 1;
    let mut r = 0u32;
    while d.is_multiple_of(2) {
        d /= 2;
        r += 1;
    }
    // This base set is a proven deterministic test for n < 2^64.
    for base in [2u64, 3, 5, 7, 11, 13, 17, 19, 23, 29, 31, 37] {
        let mut x = modular_power(base % n, d, n);
        if x == 1 || x == n - 1 {
            continue;
        }
        let mut composite = true;
        for _ in 1..r {
            x = mul_mod(x, x, n);
            if x == n - 1 {
                composite = false;
                break;
            }
        }
        if composite {
            return false;
        }
    }
    true
}

/// `base^exponent mod modulus`.
fn modular_power(
    mut base: u64,
    mut exponent: u64,
    modulus: u64,
) -> u64 {
    if modulus == 1 {
        return 0;
    }
    let mut result = 1u64;
    base %= modulus;
    while exponent > 0 {
        if exponent & 1 == 1 {
            result = mul_mod(result, base, modulus);
        }
        base = mul_mod(base, base, modulus);
        exponent >>= 1;
    }
    result
}

fn mul_mod(
    a: u64,
    b: u64,
    modulus: u64,
) -> u64 {
    u64::try_from((u128::from(a) * u128::from(b)) % u128::from(modulus)).unwrap_or(0)
}

/// Pollard's rho, for a composite with no small factor.
fn pollard_rho(n: u64) -> u64 {
    if n.is_multiple_of(2) {
        return 2;
    }
    let mut c = 1u64;
    loop {
        let mut x = 2u64;
        let mut y = 2u64;
        let mut divisor = 1u64;
        while divisor == 1 {
            x = (mul_mod(x, x, n) + c) % n;
            y = (mul_mod(y, y, n) + c) % n;
            y = (mul_mod(y, y, n) + c) % n;
            divisor = gcd_u64(x.abs_diff(y), n);
        }
        if divisor != n {
            return divisor;
        }
        c += 1;
    }
}

fn gcd_u64(
    mut a: u64,
    mut b: u64,
) -> u64 {
    while b != 0 {
        let remainder = a % b;
        a = b;
        b = remainder;
    }
    a
}

/// The primes below `limit`, by the sieve of Eratosthenes.
#[must_use]
pub fn primes(limit: i64) -> Vec<i64> {
    if limit <= 2 {
        return Vec::new();
    }
    let bound = limit as usize;
    let mut sieve = vec![true; bound];
    sieve[0] = false;
    sieve[1] = false;
    let mut candidate = 2usize;
    while candidate * candidate < bound {
        if sieve[candidate] {
            let mut multiple = candidate * candidate;
            while multiple < bound {
                sieve[multiple] = false;
                multiple += candidate;
            }
        }
        candidate += 1;
    }
    sieve
        .iter()
        .enumerate()
        .filter_map(|(index, prime)| {
            if *prime {
                Some(index as i64)
            } else {
                None
            }
        })
        .collect()
}

/// The binomial coefficient `n choose k`, exactly.
///
/// Returns zero when `k > n` and when either argument is negative, which is the
/// convention combinatorics uses for an impossible selection.
#[must_use]
pub fn binomial(
    n: i64,
    k: i64,
) -> BigInt {
    if k < 0 || n < 0 || k > n {
        return BigInt::zero();
    }
    let k = k.min(n - k);
    let mut result = BigInt::one();
    for step in 0..k {
        result = result * BigInt::from(n - step) / BigInt::from(step + 1);
    }
    result
}

/// The number of permutations of `n` things, `n!`.
#[must_use]
pub fn permutations(n: i64) -> BigInt {
    if n < 0 {
        return BigInt::zero();
    }
    let mut result = BigInt::one();
    for step in 2..=n {
        result *= BigInt::from(step);
    }
    result
}

/// The greatest common divisor, non-negative.
#[must_use]
pub fn gcd(
    a: i64,
    b: i64,
) -> i64 {
    let mut a = a.unsigned_abs();
    let mut b = b.unsigned_abs();
    while b != 0 {
        let remainder = a % b;
        a = b;
        b = remainder;
    }
    a as i64
}

/// The least common multiple, zero when either argument is zero.
#[must_use]
pub fn lcm(
    a: i64,
    b: i64,
) -> i64 {
    if a == 0 || b == 0 {
        return 0;
    }
    let divisor = gcd(a, b);
    if divisor == 0 {
        return 0;
    }
    (a / divisor).saturating_mul(b).abs()
}

/// True when `value` is negative, for the sign tests of §8.1.3.
#[must_use]
pub fn is_negative(value: &BigInt) -> bool {
    value.is_negative()
}

fn map_num_err(err: rssn::kernels::number_ext::NumberExtError) -> crate::NumericError {
    match err {
        | rssn::kernels::number_ext::NumberExtError::Empty => crate::NumericError::Empty,
        | rssn::kernels::number_ext::NumberExtError::ShapeMismatch(s) => {
            crate::NumericError::ShapeMismatch(s)
        },
        | rssn::kernels::number_ext::NumberExtError::Domain(s) => crate::NumericError::Domain(s),
    }
}

/// The extended greatest common divisor `(g, x, y)` such that
/// `a * x + b * y == g` and `g >= 0`.
#[must_use]
pub fn xgcd(
    a: i64,
    b: i64,
) -> (i128, i128, i128) {
    rssn::kernels::number_ext::xgcd(a, b)
}

/// Modular exponentiation: `base^exponent mod modulus`, in `[0, modulus)`.
///
/// # Errors
///
/// [`crate::NumericError::Domain`] when `modulus <= 0`.
pub fn mod_pow(
    base: i64,
    exponent: u64,
    modulus: i64,
) -> Result<i64, crate::NumericError> {
    rssn::kernels::number_ext::mod_pow(base, exponent, modulus).map_err(map_num_err)
}

/// The modular multiplicative inverse of `a` modulo `modulus`, in `[0, modulus)`.
///
/// # Errors
///
/// [`crate::NumericError::Domain`] when `modulus <= 0` or `gcd(a, modulus) != 1`.
pub fn mod_inv(
    a: i64,
    modulus: i64,
) -> Result<i64, crate::NumericError> {
    rssn::kernels::number_ext::mod_inv(a, modulus).map_err(map_num_err)
}

/// The Chinese Remainder Theorem: solves `x ≡ remainders[i] (mod moduli[i])`.
///
/// # Errors
///
/// [`crate::NumericError`] on mismatched lengths, empty input, non-positive modulus,
/// inconsistent congruences, or `i64` overflow.
pub fn crt(
    remainders: &[i64],
    moduli: &[i64],
) -> Result<(i64, i64), crate::NumericError> {
    rssn::kernels::number_ext::crt(remainders, moduli).map_err(map_num_err)
}

/// Euler's totient function `phi(n)`: the count of integers in `1..=n` coprime to `n`.
#[must_use]
pub fn totient(n: i64) -> i64 {
    if n <= 0 {
        return 0;
    }
    rssn::kernels::number_theory::phi(n as u64) as i64
}

/// The Möbius function `mu(n)`.
#[must_use]
pub fn mobius(n: i64) -> i64 {
    rssn::kernels::number_ext::mobius(n)
}

/// The integer partition function `p(n)`.
#[must_use]
pub fn partition_count(n: i64) -> BigInt {
    rssn::kernels::number_ext::partition_count(n)
}

/// Unsigned Stirling numbers of the first kind `c(n, k)`.
#[must_use]
pub fn stirling_first(
    n: i64,
    k: i64,
) -> BigInt {
    rssn::kernels::number_ext::stirling_first(n, k)
}

/// Signed Stirling numbers of the first kind `s(n, k) = (-1)^(n - k) c(n, k)`.
#[must_use]
pub fn stirling_first_signed(
    n: i64,
    k: i64,
) -> BigInt {
    rssn::kernels::number_ext::stirling_first_signed(n, k)
}

/// Stirling numbers of the second kind `S(n, k)`.
#[must_use]
pub fn stirling_second(
    n: i64,
    k: i64,
) -> BigInt {
    rssn::kernels::number_ext::stirling_second_exact(n, k)
}

/// The `n`-th Catalan number `C_n = binomial(2n, n) / (n + 1)`.
#[must_use]
pub fn catalan(n: i64) -> BigInt {
    rssn::kernels::number_ext::catalan_exact(n)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn factorisation_of_a_small_composite() {
        assert_eq!(factor(12), vec![(2, 2), (3, 1)]);
        assert_eq!(factor(97), vec![(97, 1)]);
        assert_eq!(factor(1), vec![]);
        assert_eq!(factor(0), vec![]);
        assert_eq!(factor(-360), vec![(2, 3), (3, 2), (5, 1)]);
    }

    #[test]
    fn factorisation_of_a_large_semiprime() {
        // 1_000_003 * 1_000_033, both prime; trial division alone would take
        // 10^6 steps, rho finishes immediately.
        let product = 1_000_003i64 * 1_000_033i64;
        let factors = factor(product);
        assert_eq!(factors.len(), 2);
        assert!(is_prime(1_000_003));
        assert!(is_prime(1_000_033));
    }

    #[test]
    fn primality_matches_a_table() {
        let primes_below_twenty = [2u64, 3, 5, 7, 11, 13, 17, 19];
        for n in 0..20u64 {
            assert_eq!(is_prime(n), primes_below_twenty.contains(&n), "at {n}");
        }
        assert!(is_prime(1_000_000_007));
        assert!(!is_prime(1_000_000_007 * 3));
    }

    #[test]
    fn the_sieve_lists_the_primes_below_a_bound() {
        assert_eq!(primes(20), vec![2, 3, 5, 7, 11, 13, 17, 19]);
        assert_eq!(primes(2), Vec::<i64>::new());
        assert_eq!(primes(30).len(), 10);
    }

    #[test]
    fn binomial_coefficients_are_exact() {
        assert_eq!(binomial(5, 2), BigInt::from(10));
        assert_eq!(binomial(10, 0), BigInt::one());
        assert_eq!(binomial(2, 5), BigInt::zero());
        assert_eq!(binomial(60, 30).to_string(), "118264581564861424");
    }

    #[test]
    fn permutations_are_factorials() {
        assert_eq!(permutations(0), BigInt::one());
        assert_eq!(permutations(5), BigInt::from(120));
        assert_eq!(permutations(20).to_string(), "2432902008176640000");
    }

    #[test]
    fn gcd_and_lcm_are_non_negative() {
        assert_eq!(gcd(-12, 18), 6);
        assert_eq!(lcm(-4, 6), 12);
        assert_eq!(lcm(0, 5), 0);
    }

    #[test]
    fn xgcd_satisfies_bezout_identity() {
        for (a, b) in [(240, 46), (-12, 18), (0, 5), (7, 0), (17, 13)] {
            let (g, x, y) = xgcd(a, b);
            assert_eq!(g, i128::from(gcd(a, b)));
            assert_eq!(i128::from(a) * x + i128::from(b) * y, g);
        }
    }

    #[test]
    fn mod_pow_and_mod_inv_work() {
        assert_eq!(mod_pow(2, 10, 1000).unwrap(), 24);
        assert_eq!(mod_pow(-3, 3, 7).unwrap(), 1); // (-3)^3 = -27 ≡ 1 (mod 7)
        assert!(mod_pow(2, 3, 0).is_err());

        let inv = mod_inv(3, 11).unwrap();
        assert_eq!(inv, 4);
        assert_eq!((3 * inv) % 11, 1);
        assert_eq!(mod_inv(-3, 11).unwrap(), 7);
        assert!(mod_inv(6, 9).is_err());
    }

    #[test]
    fn crt_solves_coprime_and_compatible_systems() {
        // x ≡ 2 (mod 3), x ≡ 3 (mod 5), x ≡ 2 (mod 7) => x = 23 (mod 105)
        assert_eq!(crt(&[2, 3, 2], &[3, 5, 7]).unwrap(), (23, 105));
        // Compatible non-coprime: x ≡ 1 (mod 4), x ≡ 3 (mod 6) => x = 9 (mod 12)
        assert_eq!(crt(&[1, 3], &[4, 6]).unwrap(), (9, 12));
        // Inconsistent: x ≡ 1 (mod 4), x ≡ 2 (mod 6)
        assert!(crt(&[1, 2], &[4, 6]).is_err());
        assert!(crt(&[1], &[2, 3]).is_err());
    }

    #[test]
    fn totient_and_mobius_match_known_values() {
        let expected_phi = [0, 1, 1, 2, 2, 4, 2, 6, 4, 6, 4, 10, 4];
        for (n, &phi) in expected_phi.iter().enumerate() {
            assert_eq!(totient(n as i64), phi, "phi({n})");
        }
        // mu(1)=1, mu(2)=-1, mu(3)=-1, mu(4)=0, mu(5)=-1, mu(6)=1, mu(12)=0, mu(30)=-1
        assert_eq!(mobius(1), 1);
        assert_eq!(mobius(2), -1);
        assert_eq!(mobius(4), 0);
        assert_eq!(mobius(6), 1);
        assert_eq!(mobius(12), 0);
        assert_eq!(mobius(30), -1);
    }

    #[test]
    fn partitions_stirling_and_catalan_match_tables() {
        let expected_p = [1, 1, 2, 3, 5, 7, 11, 15, 22, 30, 42];
        for (n, &val) in expected_p.iter().enumerate() {
            assert_eq!(partition_count(n as i64), BigInt::from(val), "p({n})");
        }
        assert_eq!(partition_count(50), BigInt::from(204_226u64));

        // Unsigned Stirling 1st kind: c(4, 2) = 11, c(5, 3) = 35
        assert_eq!(stirling_first(0, 0), BigInt::one());
        assert_eq!(stirling_first(4, 2), BigInt::from(11));
        assert_eq!(stirling_first(5, 3), BigInt::from(35));
        assert_eq!(stirling_first_signed(4, 2), BigInt::from(11));
        assert_eq!(stirling_first_signed(4, 1), BigInt::from(-6));

        // Stirling 2nd kind: S(4, 2) = 7, S(5, 3) = 25
        assert_eq!(stirling_second(0, 0), BigInt::one());
        assert_eq!(stirling_second(4, 2), BigInt::from(7));
        assert_eq!(stirling_second(5, 3), BigInt::from(25));

        // Catalan: 1, 1, 2, 5, 14, 42, 132
        let expected_c = [1, 1, 2, 5, 14, 42, 132];
        for (n, &val) in expected_c.iter().enumerate() {
            assert_eq!(catalan(n as i64), BigInt::from(val), "C_{n}");
        }
    }
}

#[cfg(test)]
mod kernel_regressions {
    use super::*;

    #[test]
    fn extreme_signed_inputs_preserve_bezout_and_modular_inverse() {
        for (a, b) in [(i64::MIN, 0), (i64::MIN, -1), (i64::MIN, i64::MAX)] {
            let (g, x, y) = xgcd(a, b);
            assert!(g >= 0);
            assert_eq!(i128::from(a) * x + i128::from(b) * y, g);
        }
        assert_eq!(mod_inv(i64::MIN, i64::MAX).unwrap(), i64::MAX - 1);
        assert!(crt(&[0, 0, 0], &[i64::MAX, i64::MAX - 1, i64::MAX - 2]).is_err());
        assert_eq!(stirling_first_signed(i64::MIN, 1), BigInt::from(0));
    }
}
