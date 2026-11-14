//! Special functions (§8.2.2) and the distribution kernels that need them.
//!
//! Floating-point evaluation delegates to RSSN. Exact polynomial values remain
//! in the language adapter to preserve Aevia rational-value semantics.

#[cfg(test)]
use std::f64::consts::PI;

/// The natural logarithm of the Gamma function, for `x > 0`.
///
/// Lanczos with `g = 7`, `n = 9`, which is accurate to about 1e-13 relative
/// over the positive axis; reflection carries it to the negative one.
#[must_use]
pub fn ln_gamma(x: f64) -> f64 {
    rssn::kernels::special::ln_gamma_numerical(x)
}

/// The Gamma function.
#[must_use]
pub fn gamma(x: f64) -> f64 {
    rssn::kernels::special::gamma_numerical(x)
}

/// The Beta function `B(a, b)`.
#[must_use]
pub fn beta(
    a: f64,
    b: f64,
) -> f64 {
    rssn::kernels::special::beta_numerical(a, b)
}

/// The error function, through the regularized incomplete Gamma function.
#[must_use]
pub fn erf(x: f64) -> f64 {
    rssn::kernels::special::erf_numerical(x)
}

/// The complementary error function.
#[must_use]
pub fn erfc(x: f64) -> f64 {
    rssn::kernels::special::erfc_numerical(x)
}

/// `P(a, x)`: the regularized lower incomplete Gamma function.
///
/// Series below `x = a + 1` and a continued fraction above it, which is where
/// each expansion converges.
#[must_use]
pub fn regularized_gamma_p(
    a: f64,
    x: f64,
) -> f64 {
    rssn::kernels::special::regularized_lower_gamma(a, x)
}

/// `Q(a, x)`: the regularized upper incomplete Gamma function.
#[must_use]
pub fn regularized_gamma_q(
    a: f64,
    x: f64,
) -> f64 {
    rssn::kernels::special::regularized_upper_gamma(a, x)
}

/// `I_x(a, b)`: the regularized incomplete Beta function.
#[must_use]
pub fn regularized_beta(
    x: f64,
    a: f64,
    b: f64,
) -> f64 {
    rssn::kernels::special::regularized_beta(x, a, b)
}

/// The Riemann zeta function.
///
/// Euler–Maclaurin above one, the functional equation below it, and the pole at
/// one reported as infinity — `zeta(1)` has no value, and returning a large
/// number would be a wrong answer that looks right.
#[must_use]
pub fn zeta(s: f64) -> f64 {
    rssn::kernels::special::riemann_zeta(s)
}

/// The Bessel function of the first kind, order zero.
#[must_use]
pub fn bessel_j0(x: f64) -> f64 {
    rssn::kernels::special::bessel_j0(x)
}

/// The Bessel function of the first kind, order one.
#[must_use]
pub fn bessel_j1(x: f64) -> f64 {
    rssn::kernels::special::bessel_j1(x)
}

/// The Bessel function of the second kind, order zero.
#[must_use]
pub fn bessel_y0(x: f64) -> f64 {
    rssn::kernels::special::bessel_y0(x)
}

/// The Bessel function of the second kind, order one.
#[must_use]
pub fn bessel_y1(x: f64) -> f64 {
    rssn::kernels::special::bessel_y1(x)
}

/// The Bessel function of the first kind, order `n`.
///
/// Upward recurrence where it is stable (`n <= x`) and Miller's downward
/// recurrence where it is not, normalised against `J0`.
#[must_use]
pub fn bessel_jn(
    n: i32,
    x: f64,
) -> f64 {
    rssn::kernels::special_ext::bessel_jn(n, x)
}

/// The Bessel function of the second kind, order `n`.
#[must_use]
pub fn bessel_yn(
    n: i32,
    x: f64,
) -> f64 {
    rssn::kernels::special::bessel_y(f64::from(n), x)
}

/// The Legendre polynomial `P_n(x)`, by the three-term recurrence.
#[must_use]
pub fn legendre_p(
    n: i32,
    x: f64,
) -> f64 {
    if n < 0 {
        return f64::NAN;
    }
    rssn::kernels::special::legendre_p(n as u32, x)
}

/// The physicists' Hermite polynomial `H_n(x)`.
#[must_use]
pub fn hermite_h(
    n: i32,
    x: f64,
) -> f64 {
    if n < 0 {
        return f64::NAN;
    }
    rssn::kernels::special::hermite_h(n as u32, x)
}

/// The Laguerre polynomial `L_n(x)`.
#[must_use]
pub fn laguerre_l(
    n: i32,
    x: f64,
) -> f64 {
    if n < 0 {
        return f64::NAN;
    }
    rssn::kernels::special::laguerre_l(n as u32, x)
}

/// The Chebyshev polynomial of the first kind `T_n(x)`.
#[must_use]
pub fn chebyshev_t(
    n: i32,
    x: f64,
) -> f64 {
    if n < 0 {
        return f64::NAN;
    }
    rssn::kernels::special::chebyshev_t(n as u32, x)
}

/// The Chebyshev polynomial of the second kind `U_n(x)`.
#[must_use]
pub fn chebyshev_u(
    n: i32,
    x: f64,
) -> f64 {
    if n < 0 {
        return f64::NAN;
    }
    rssn::kernels::special::chebyshev_u(n as u32, x)
}

/// The modified Bessel function of the first kind, order 0: `I_0(x)`.
#[must_use]
pub fn bessel_i0(x: f64) -> f64 {
    rssn::kernels::special::bessel_i0(x)
}

/// The modified Bessel function of the first kind, order 1: `I_1(x)`.
#[must_use]
pub fn bessel_i1(x: f64) -> f64 {
    rssn::kernels::special::bessel_i1(x)
}

/// The modified Bessel function of the first kind, integer order `n`: `I_n(x)`.
#[must_use]
pub fn bessel_in(
    n: i32,
    x: f64,
) -> f64 {
    rssn::kernels::special_ext::bessel_in(n, x)
}

/// The modified Bessel function of the second kind, order 0: `K_0(x)` for `x > 0`.
#[must_use]
pub fn bessel_k0(x: f64) -> f64 {
    rssn::kernels::special::bessel_k0(x)
}

/// The modified Bessel function of the second kind, order 1: `K_1(x)` for `x > 0`.
#[must_use]
pub fn bessel_k1(x: f64) -> f64 {
    rssn::kernels::special::bessel_k1(x)
}

/// The modified Bessel function of the second kind, integer order `n`: `K_n(x)` for `x > 0`.
#[must_use]
pub fn bessel_kn(
    n: i32,
    x: f64,
) -> f64 {
    rssn::kernels::special::bessel_k(f64::from(n), x)
}

/// The Airy function of the first kind `Ai(x)`.
#[must_use]
pub fn airy_ai(x: f64) -> f64 {
    rssn::kernels::special::airy_ai(x)
}

/// The Airy function of the second kind `Bi(x)`.
#[must_use]
pub fn airy_bi(x: f64) -> f64 {
    rssn::kernels::special::airy_bi(x)
}

/// The digamma function `psi(x) = Gamma'(x) / Gamma(x)`.
#[must_use]
pub fn digamma(x: f64) -> f64 {
    rssn::kernels::special::digamma_numerical(x)
}

/// The polygamma function `psi^(n)(x)` of order `n >= 0`.
#[must_use]
pub fn polygamma(
    n: i32,
    x: f64,
) -> f64 {
    if n < 0 || x.is_nan() {
        return f64::NAN;
    }
    rssn::kernels::special::polygamma_numerical(n as u32, x)
}

/// Inverse error function `erfinv(y)` for `|y| <= 1`.
#[must_use]
pub fn erfinv(y: f64) -> f64 {
    if y.is_nan() || y.abs() > 1.0 {
        return f64::NAN;
    }
    if y == 1.0 {
        return f64::INFINITY;
    }
    if y == -1.0 {
        return f64::NEG_INFINITY;
    }
    rssn::kernels::special::inverse_erf_numerical(y)
}

/// Inverse complementary error function `erfcinv(y)` for `0 <= y <= 2`.
#[must_use]
pub fn erfcinv(y: f64) -> f64 {
    rssn::kernels::special::inverse_erfc(y)
}

/// Principal branch `W_0(x)` of the Lambert W function (`x >= -1/e`).
#[must_use]
pub fn lambert_w0(x: f64) -> f64 {
    rssn::kernels::special::lambert_w(x, true)
}

/// Secondary real branch `W_{-1}(x)` of the Lambert W function (`-1/e <= x < 0`).
#[must_use]
pub fn lambert_wm1(x: f64) -> f64 {
    rssn::kernels::special::lambert_w(x, false)
}

/// Complete elliptic integral of the first kind `K(k)` for `|k| < 1`.
#[must_use]
pub fn elliptic_k(k: f64) -> f64 {
    if k.is_nan() || k.abs() > 1.0 {
        return f64::NAN;
    }
    rssn::kernels::special_ext::elliptic_k(k * k)
}

/// Complete elliptic integral of the second kind `E(k)` for `|k| <= 1`.
#[must_use]
pub fn elliptic_e(k: f64) -> f64 {
    if k.is_nan() || k.abs() > 1.0 {
        return f64::NAN;
    }
    rssn::kernels::special_ext::elliptic_e(k * k)
}

/// Incomplete elliptic integral of the first kind `F(phi, k)` for `|k| <= 1`.
#[must_use]
pub fn elliptic_f(
    phi: f64,
    k: f64,
) -> f64 {
    if phi.is_nan() || k.is_nan() || k.abs() > 1.0 {
        return f64::NAN;
    }
    rssn::kernels::special_ext::elliptic_f(phi, k * k)
}

/// Incomplete elliptic integral of the second kind `E(phi, k)` for `|k| <= 1`.
#[must_use]
pub fn elliptic_e_inc(
    phi: f64,
    k: f64,
) -> f64 {
    if phi.is_nan() || k.is_nan() || k.abs() > 1.0 {
        return f64::NAN;
    }
    rssn::kernels::special_ext::elliptic_e_inc(phi, k * k)
}

/// Kummer's confluent hypergeometric function `M(a, b, z) = 1F1(a; b; z)`.
#[must_use]
pub fn hypergeometric_1f1(
    a: f64,
    b: f64,
    z: f64,
) -> f64 {
    rssn::kernels::special_ext::hyp1f1(a, b, z)
}

/// Gauss hypergeometric function `2F1(a, b; c; z)`.
#[must_use]
pub fn hypergeometric_2f1(
    a: f64,
    b: f64,
    c: f64,
    z: f64,
) -> f64 {
    rssn::kernels::special_ext::hyp2f1(a, b, c, z)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn close(
        a: f64,
        b: f64,
        tolerance: f64,
    ) -> bool {
        (a - b).abs() <= tolerance * b.abs().max(1.0)
    }

    #[test]
    fn gamma_of_an_integer_is_a_factorial() {
        assert!(close(gamma(5.0), 24.0, 1e-12));
        assert!(close(gamma(0.5), PI.sqrt(), 1e-12));
        assert!(close(gamma(0.5) * gamma(0.5), PI, 1e-12));
    }

    #[test]
    fn ln_gamma_is_consistent_with_gamma() {
        assert!(close(ln_gamma(10.0), 12.801_827_480_077_02, 1e-12));
    }

    #[test]
    fn the_error_function_matches_known_values() {
        assert!(close(erf(0.0), 0.0, 1e-12));
        assert!(close(erf(1.0), 0.842_700_792_949_714_9, 1e-10));
        assert!(close(erfc(1.0), 0.157_299_207_050_285_1, 1e-9));
        // The series is antisymmetric.
        assert!(close(erf(-2.0), -erf(2.0), 1e-12));
    }

    #[test]
    fn the_incomplete_beta_is_a_cdf() {
        // I_x(1,1) = x.
        assert!(close(regularized_beta(0.25, 1.0, 1.0), 0.25, 1e-12));
        // I_x(2,2) = 3x^2 - 2x^3.
        assert!(close(
            regularized_beta(0.3, 2.0, 2.0),
            3.0 * 0.09 - 2.0 * 0.027,
            1e-12
        ));
    }

    #[test]
    fn zeta_matches_known_values() {
        assert!(close(zeta(2.0), PI * PI / 6.0, 1e-8));
        assert!(close(zeta(4.0), PI.powi(4) / 90.0, 1e-8));
        assert!(close(zeta(-1.0), -1.0 / 12.0, 1e-8));
        assert!(zeta(1.0).is_infinite());
    }

    #[test]
    fn bessel_functions_match_known_values() {
        // The rational approximations of Abramowitz & Stegun 9.4 carry about
        // 1e-8 absolute error, which is the accuracy the module promises.
        assert!(close(bessel_j0(0.0), 1.0, 1e-8));
        assert!(close(bessel_j0(1.0), 0.765_197_686_579_827_1, 1e-7));
        assert!(close(bessel_j1(1.0), 0.440_050_585_744_099_9, 1e-7));
        assert!(close(bessel_y0(1.0), 0.088_256_964_240_892_3, 1e-7));
        assert!(close(bessel_y1(1.0), -0.781_212_818_464_764_2, 1e-7));
        assert!(close(bessel_jn(2, 1.0), 0.114_903_282_237_762_2, 1e-6));
        assert!(close(bessel_jn(5, 10.0), -0.234_061_507_770_158_5, 1e-6));
    }

    #[test]
    fn the_classical_polynomials_satisfy_their_recurrences() {
        assert!(close(legendre_p(2, 0.5), -0.125, 1e-12));
        assert!(close(legendre_p(3, 0.5), -0.4375, 1e-12));
        assert!(close(hermite_h(3, 0.5), -5.0, 1e-12));
        assert!(close(laguerre_l(2, 0.5), 0.125, 1e-12));
        assert!(close(chebyshev_t(3, 0.5), -1.0, 1e-12));
        assert!(close(chebyshev_u(2, 0.5), 0.0, 1e-12));
    }

    #[test]
    fn modified_bessel_functions_match_known_values() {
        assert!(close(bessel_i0(0.0), 1.0, 1e-8));
        assert!(close(bessel_i0(1.0), 1.266_065_877_752_008_4, 1e-7));
        assert!(close(bessel_i1(1.0), 0.565_159_103_992_485, 1e-7));
        assert!(close(bessel_in(3, 2.0), 0.212_739_959_241_484_6, 1e-6));
        assert!(close(bessel_i0(5.0), 27.239_871_823_604_442, 1e-6));

        assert!(close(bessel_k0(1.0), 0.421_024_438_240_708_3, 1e-7));
        assert!(close(bessel_k1(1.0), 0.601_907_230_197_234_6, 1e-7));
        assert!(close(bessel_kn(2, 1.0), 1.624_838_898_635_177_5, 1e-6));
        assert!(close(bessel_k0(5.0), 0.003_691_098_334_042_594, 1e-6));
    }

    #[test]
    fn airy_functions_match_known_values_and_continuity() {
        assert!(close(airy_ai(0.0), 0.355_028_053_887_817_2, 1e-12));
        assert!(close(airy_bi(0.0), 0.614_926_627_446_000_7, 1e-12));
        assert!(close(airy_ai(1.0), 0.135_292_416_312_881_4, 1e-12));
        assert!(close(airy_bi(1.0), 1.207_423_594_952_871_3, 1e-12));
        assert!(close(airy_ai(-2.0), 0.227_407_428_201_685_57, 1e-12));
        assert!(close(airy_bi(-2.0), -0.412_302_587_956_398_5, 1e-12));
        // Continuity across |x| = 4.5 boundary (asymptotic vs Taylor).
        assert!(close(airy_ai(4.499), airy_ai(4.501), 5e-3));
        assert!(close(airy_ai(6.0), 9.947_694_314_536_517e-6, 1e-8));
        assert!(close(airy_ai(-6.0), -0.329_145_173_489_739_2, 1e-7));
    }

    #[test]
    fn digamma_and_polygamma_match_known_values() {
        use std::f64::consts::EULER_GAMMA;
        assert!(close(digamma(1.0), -EULER_GAMMA, 1e-12));
        assert!(close(digamma(2.0), 1.0 - EULER_GAMMA, 1e-12));
        assert!(close(digamma(0.5), -EULER_GAMMA - 2.0 * 2.0f64.ln(), 1e-12));
        // Trigamma at 1 is pi^2 / 6.
        assert!(close(polygamma(1, 1.0), PI * PI / 6.0, 1e-11));
        assert!(close(polygamma(1, 0.5), PI * PI / 2.0, 1e-11));
        // Tetragamma at 1 is -2 * zeta(3).
        assert!(close(polygamma(2, 1.0), -2.404_113_806_319_188_5, 1e-10));
        // Reflection for negative non-integer.
        assert!(close(
            digamma(-0.5),
            digamma(1.5) - PI / (-0.5 * PI).tan(),
            1e-12
        ));
    }

    #[test]
    fn inverse_error_functions_invert_erf_and_erfc() {
        for x in [-2.5, -1.0, -0.3, 0.0, 0.3, 1.0, 2.5] {
            let y = erf(x);
            assert!(close(erfinv(y), x, 1e-10), "erfinv at x={x}");
        }
        for x in [-1.5, 0.2, 1.0, 3.0] {
            let y = erfc(x);
            assert!(close(erfcinv(y), x, 1e-9), "erfcinv at x={x}");
        }
    }

    #[test]
    fn lambert_w_satisfies_defining_equation() {
        assert!(close(lambert_w0(0.0), 0.0, 1e-15));
        assert!(close(lambert_w0(1.0), 0.567_143_290_409_783_8, 1e-14));
        for x in [-0.3, -0.1, 0.5, 2.0, 10.0, 100.0] {
            let w = lambert_w0(x);
            assert!(close(w * w.exp(), x, 1e-12), "W0({x})");
        }
        for x in [-0.35, -0.2, -0.05, -1e-6] {
            let w = lambert_wm1(x);
            assert!(w <= -1.0);
            assert!(close(w * w.exp(), x, 1e-12), "W-1({x})");
        }
    }

    #[test]
    fn elliptic_integrals_match_known_values() {
        assert!(close(elliptic_k(0.0), PI / 2.0, 1e-14));
        assert!(close(elliptic_e(0.0), PI / 2.0, 1e-14));
        assert!(close(elliptic_e(1.0), 1.0, 1e-14));
        let k = 0.5f64.sqrt();
        assert!(close(elliptic_k(k), 1.854_074_677_301_371_9, 1e-12));
        assert!(close(elliptic_e(k), 1.350_643_881_047_675_5, 1e-12));
        // Incomplete integrals at pi/2 agree with complete integrals.
        assert!(close(elliptic_f(PI / 2.0, k), elliptic_k(k), 1e-12));
        assert!(close(elliptic_e_inc(PI / 2.0, k), elliptic_e(k), 1e-12));
        assert!(close(elliptic_f(0.5, 0.0), 0.5, 1e-14));
    }

    #[test]
    fn hypergeometric_functions_match_special_cases() {
        // 1F1(a; a; z) = exp(z).
        assert!(close(
            hypergeometric_1f1(2.5, 2.5, 1.5),
            1.5f64.exp(),
            1e-12
        ));
        assert!(close(
            hypergeometric_1f1(1.5, 1.5, -3.0),
            (-3.0f64).exp(),
            1e-12
        ));
        // 2F1(1, 1; 2; z) = -ln(1 - z) / z.
        for z in [-2.0f64, -0.5, 0.3, 0.85] {
            let expected = -(1.0 - z).ln() / z;
            assert!(
                close(hypergeometric_2f1(1.0, 1.0, 2.0, z), expected, 1e-10),
                "2F1 at z={z}"
            );
        }
        // Gauss's theorem at z = 1: 2F1(0.5, 0.5; 2.0; 1) = 4 / pi.
        assert!(close(
            hypergeometric_2f1(0.5, 0.5, 2.0, 1.0),
            4.0 / PI,
            1e-11
        ));
    }
}

// ----- exact forms of the classical polynomials ----------------------------
//
// §8.2.2 types these as `Scalar<ρ,1> -> Scalar<ρ,1>`, so an exact argument must
// give an exact answer: `legendre_p(2, 1/2)` is `-1/8`, not `-0.125`. They are
// polynomials with rational coefficients, so this is exactness the algorithm
// genuinely permits.

use num_bigint::BigInt;
use num_rational::BigRational;
use num_traits::One;

fn rational(value: i64) -> BigRational {
    BigRational::from_integer(BigInt::from(value))
}

/// `P_n(x)` over the rationals.
#[must_use]
pub fn legendre_p_exact(
    n: u32,
    x: &BigRational,
) -> BigRational {
    if n == 0 {
        return BigRational::one();
    }
    let mut previous = BigRational::one();
    let mut current = x.clone();
    for order in 1..n {
        let k = rational(i64::from(order));
        let next = ((rational(2) * k.clone() + BigRational::one()) * x.clone() * current.clone()
            - k.clone() * previous.clone())
            / (k + BigRational::one());
        previous = current;
        current = next;
    }
    current
}

/// `H_n(x)` over the rationals.
#[must_use]
pub fn hermite_h_exact(
    n: u32,
    x: &BigRational,
) -> BigRational {
    if n == 0 {
        return BigRational::one();
    }
    let mut previous = BigRational::one();
    let mut current = rational(2) * x.clone();
    for order in 1..n {
        let next = rational(2) * x.clone() * current.clone()
            - rational(2 * i64::from(order)) * previous.clone();
        previous = current;
        current = next;
    }
    current
}

/// `L_n(x)` over the rationals.
#[must_use]
pub fn laguerre_l_exact(
    n: u32,
    x: &BigRational,
) -> BigRational {
    if n == 0 {
        return BigRational::one();
    }
    let mut previous = BigRational::one();
    let mut current = BigRational::one() - x.clone();
    for order in 1..n {
        let k = rational(i64::from(order));
        let next = ((rational(2) * k.clone() + BigRational::one() - x.clone()) * current.clone()
            - k.clone() * previous.clone())
            / (k + BigRational::one());
        previous = current;
        current = next;
    }
    current
}

/// `T_n(x)` over the rationals.
#[must_use]
pub fn chebyshev_t_exact(
    n: u32,
    x: &BigRational,
) -> BigRational {
    if n == 0 {
        return BigRational::one();
    }
    let mut previous = BigRational::one();
    let mut current = x.clone();
    for _ in 1..n {
        let next = rational(2) * x.clone() * current.clone() - previous.clone();
        previous = current;
        current = next;
    }
    current
}

/// `U_n(x)` over the rationals.
#[must_use]
pub fn chebyshev_u_exact(
    n: u32,
    x: &BigRational,
) -> BigRational {
    if n == 0 {
        return BigRational::one();
    }
    let mut previous = BigRational::one();
    let mut current = rational(2) * x.clone();
    for _ in 1..n {
        let next = rational(2) * x.clone() * current.clone() - previous.clone();
        previous = current;
        current = next;
    }
    current
}

#[cfg(test)]
mod exact_tests {
    use super::*;
    use num_traits::ToPrimitive;

    fn half() -> BigRational {
        BigRational::new(BigInt::from(1), BigInt::from(2))
    }

    #[test]
    fn the_exact_polynomials_agree_with_the_float_ones() {
        for n in 0..8 {
            let exact = legendre_p_exact(n, &half());
            let approximate = legendre_p(n as i32, 0.5);
            assert!((exact.to_f64().unwrap() - approximate).abs() < 1e-12);
            let exact = hermite_h_exact(n, &half());
            assert!((exact.to_f64().unwrap() - hermite_h(n as i32, 0.5)).abs() < 1e-12);
            let exact = laguerre_l_exact(n, &half());
            assert!((exact.to_f64().unwrap() - laguerre_l(n as i32, 0.5)).abs() < 1e-12);
            let exact = chebyshev_t_exact(n, &half());
            assert!((exact.to_f64().unwrap() - chebyshev_t(n as i32, 0.5)).abs() < 1e-12);
            let exact = chebyshev_u_exact(n, &half());
            assert!((exact.to_f64().unwrap() - chebyshev_u(n as i32, 0.5)).abs() < 1e-12);
        }
    }

    #[test]
    fn legendre_p_two_at_a_half_is_minus_one_eighth() {
        assert_eq!(
            legendre_p_exact(2, &half()),
            BigRational::new(BigInt::from(-1), BigInt::from(8))
        );
        assert!(hermite_h_exact(0, &half()).is_one());
        assert!(laguerre_l_exact(0, &half()).is_one());
    }
}

#[cfg(test)]
mod kernel_regressions {
    use rssn::kernels::special_ext::carlson_rd;
    use rssn::kernels::special_ext::carlson_rf;

    #[test]
    fn carlson_invalid_domains_terminate_and_extreme_scales_stay_accurate() {
        for (x, y, z) in [
            (-1.0, 1.0, 1.0),
            (f64::NAN, 1.0, 1.0),
            (0.0, 0.0, 1.0),
            (f64::INFINITY, 1.0, 1.0),
        ] {
            assert!(carlson_rf(x, y, z).is_nan());
            assert!(carlson_rd(x, y, z).is_nan());
        }
        assert!(carlson_rd(1.0, 1.0, 0.0).is_nan());
        for x in [1e-300_f64, 1.0, 1e308] {
            assert!((carlson_rf(x, x, x) * x.sqrt() - 1.0).abs() < 1e-14);
        }
        assert!((carlson_rd(1e200, 1e200, 1e200) / 1e-300 - 1.0).abs() < 1e-14);
    }
}
