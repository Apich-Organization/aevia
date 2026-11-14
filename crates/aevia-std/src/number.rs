//! Number theory, combinatorics and elementary geometry (§8.16).
//!
//! Exact integer arithmetic throughout: `binomial(60, 30)` is a 17-digit
//! integer, not a float, and `factor` returns prime powers rather than a float
//! approximation of them.

use num_bigint::BigInt;
#[cfg(test)]
use num_traits::One;
#[cfg(test)]
use num_traits::Zero;
pub use rssn::kernels::number_exact::binomial;
pub use rssn::kernels::number_exact::factor;
pub use rssn::kernels::number_exact::gcd;
pub use rssn::kernels::number_exact::is_negative;
pub use rssn::kernels::number_exact::is_prime;
pub use rssn::kernels::number_exact::lcm;
pub use rssn::kernels::number_exact::permutations;
pub use rssn::kernels::number_exact::primes;

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
