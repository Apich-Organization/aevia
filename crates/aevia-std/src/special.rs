//! Special functions (§8.2.2) and the distribution kernels that need them.
//!
//! Every function here is a real approximation with a documented accuracy
//! target of about 1e-8 relative, which is the accuracy of the classical
//! rational approximations they are built from. Nothing is symbolic: `gamma`
//! of an exact integer is handled by the runtime, because that is where
//! exactness can be kept.

// The Bessel approximations carry their published nine-digit constants (for
// example `0.636_619_772` for `2/pi`): the coefficients were fitted together with
// them, so replacing one by the exact value would change the approximation.
#![allow(clippy::approx_constant)]

use std::f64::consts::PI;

/// The natural logarithm of the Gamma function, for `x > 0`.
///
/// Lanczos with `g = 7`, `n = 9`, which is accurate to about 1e-13 relative
/// over the positive axis; reflection carries it to the negative one.
#[must_use]
pub fn ln_gamma(x: f64) -> f64 {
    if x < 0.5 {
        // Reflection: Gamma(x) Gamma(1-x) = pi / sin(pi x).
        return (PI / (PI * x).sin().abs()).ln() - ln_gamma(1.0 - x);
    }
    const COEFFICIENTS: [f64; 8] = [
        676.520_368_121_885_1,
        -1_259.139_216_722_402_8,
        771.323_428_777_653_1,
        -176.615_029_162_140_6,
        12.507_343_278_686_905,
        -0.138_571_095_265_720_12,
        9.984_369_578_019_572e-6,
        1.505_632_735_149_311_6e-7,
    ];
    let shifted = x - 1.0;
    let mut series = 0.999_999_999_999_809_9;
    let t = shifted + 7.5;
    for (index, coefficient) in COEFFICIENTS.iter().enumerate() {
        series += coefficient / (shifted + index as f64 + 1.0);
    }
    0.5 * (2.0 * PI).ln() + (shifted + 0.5) * t.ln() - t + series.ln()
}

/// The Gamma function.
#[must_use]
pub fn gamma(x: f64) -> f64 {
    if x < 0.5 {
        return PI / ((PI * x).sin() * gamma(1.0 - x));
    }
    ln_gamma(x).exp()
}

/// The Beta function `B(a, b)`.
#[must_use]
pub fn beta(
    a: f64,
    b: f64,
) -> f64 {
    (ln_gamma(a) + ln_gamma(b) - ln_gamma(a + b)).exp()
}

/// The error function, through the regularized incomplete Gamma function.
#[must_use]
pub fn erf(x: f64) -> f64 {
    if x == 0.0 {
        return 0.0;
    }

    if x > 0.0 {
        regularized_gamma_p(0.5, x * x)
    } else {
        -regularized_gamma_p(0.5, x * x)
    }
}

/// The complementary error function.
#[must_use]
pub fn erfc(x: f64) -> f64 {
    1.0 - erf(x)
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
    if x <= 0.0 || a <= 0.0 {
        return 0.0;
    }
    if x < a + 1.0 {
        // Series: P(a,x) = x^a e^-x / Gamma(a) * sum x^n / (a(a+1)...(a+n)).
        let mut term = 1.0 / a;
        let mut total = term;
        let mut n = 0.0;
        for _ in 0..1000 {
            n += 1.0;
            term *= x / (a + n);
            total += term;
            if term.abs() < total.abs() * 1e-16 {
                break;
            }
        }
        return total * (-x + a * x.ln() - ln_gamma(a)).exp();
    }
    // Continued fraction for Q(a, x), then complement.
    let mut b = x + 1.0 - a;
    let mut c = 1.0 / f64::MIN_POSITIVE;
    let mut d = 1.0 / b;
    let mut h = d;
    for index in 1..1000 {
        let index = index as f64;
        let an = -index * (index - a);
        b += 2.0;
        d = an * d + b;
        if d.abs() < f64::MIN_POSITIVE {
            d = f64::MIN_POSITIVE;
        }
        c = b + an / c;
        if c.abs() < f64::MIN_POSITIVE {
            c = f64::MIN_POSITIVE;
        }
        d = 1.0 / d;
        let delta = d * c;
        h *= delta;
        if (delta - 1.0).abs() < 1e-16 {
            break;
        }
    }
    (1.0 - h * (-x + a * x.ln() - ln_gamma(a)).exp()).clamp(0.0, 1.0)
}

/// `Q(a, x)`: the regularized upper incomplete Gamma function.
#[must_use]
pub fn regularized_gamma_q(
    a: f64,
    x: f64,
) -> f64 {
    1.0 - regularized_gamma_p(a, x)
}

/// `I_x(a, b)`: the regularized incomplete Beta function.
#[must_use]
pub fn regularized_beta(
    x: f64,
    a: f64,
    b: f64,
) -> f64 {
    if x <= 0.0 {
        return 0.0;
    }
    if x >= 1.0 {
        return 1.0;
    }
    let front =
        (ln_gamma(a + b) - ln_gamma(a) - ln_gamma(b) + a * x.ln() + b * (1.0 - x).ln()).exp();
    if x < (a + 1.0) / (a + b + 2.0) {
        front * beta_continued_fraction(x, a, b) / a
    } else {
        1.0 - front * beta_continued_fraction(1.0 - x, b, a) / b
    }
}

/// Lentz's continued fraction for the incomplete Beta function.
fn beta_continued_fraction(
    x: f64,
    a: f64,
    b: f64,
) -> f64 {
    const TINY: f64 = 1e-300;
    let mut c = 1.0;
    let mut d = 1.0 - (a + b) * x / (a + 1.0);
    if d.abs() < TINY {
        d = TINY;
    }
    d = 1.0 / d;
    let mut h = d;
    for index in 1..1000 {
        let m = index as f64;
        let m2 = 2.0 * m;
        // Even step.
        let mut aa = m * (b - m) * x / ((a + m2 - 1.0) * (a + m2));
        d = 1.0 + aa * d;
        if d.abs() < TINY {
            d = TINY;
        }
        c = 1.0 + aa / c;
        if c.abs() < TINY {
            c = TINY;
        }
        d = 1.0 / d;
        h *= d * c;
        // Odd step.
        aa = -(a + m) * (a + b + m) * x / ((a + m2) * (a + m2 + 1.0));
        d = 1.0 + aa * d;
        if d.abs() < TINY {
            d = TINY;
        }
        c = 1.0 + aa / c;
        if c.abs() < TINY {
            c = TINY;
        }
        d = 1.0 / d;
        let delta = d * c;
        h *= delta;
        if (delta - 1.0).abs() < 1e-15 {
            break;
        }
    }
    h
}

/// The Riemann zeta function.
///
/// Euler–Maclaurin above one, the functional equation below it, and the pole at
/// one reported as infinity — `zeta(1)` has no value, and returning a large
/// number would be a wrong answer that looks right.
#[must_use]
pub fn zeta(s: f64) -> f64 {
    if s == 1.0 {
        return f64::INFINITY;
    }
    if s < 0.0 {
        // zeta(s) = 2^s pi^(s-1) sin(pi s / 2) Gamma(1-s) zeta(1-s).
        let reflected = 2f64.powf(s) * PI.powf(s - 1.0) * (PI * s / 2.0).sin() * gamma(1.0 - s);
        return reflected * zeta(1.0 - s);
    }
    if s == 0.0 {
        return -0.5;
    }
    // Euler–Maclaurin: sum to N, then the tail expansion
    //   sum_k B_2k/(2k)! * s(s+1)…(s+2k-2) * N^(-s-2k+1).
    let n = 20usize;
    let mut total = 0.0;
    for k in 1..n {
        total += (k as f64).powf(-s);
    }
    let n = n as f64;
    total += n.powf(1.0 - s) / (s - 1.0);
    total += 0.5 * n.powf(-s);
    let bernoulli = [
        1.0 / 6.0,
        -1.0 / 30.0,
        1.0 / 42.0,
        -1.0 / 30.0,
        5.0 / 66.0,
        -691.0 / 2730.0,
    ];
    let mut rising = s;
    let mut factorial = 2.0;
    let mut power = n.powf(-s - 1.0);
    for (index, coefficient) in bernoulli.iter().enumerate() {
        total += coefficient * rising / factorial * power;
        let k = (index + 1) as f64;
        rising *= (s + 2.0 * k - 1.0) * (s + 2.0 * k);
        factorial *= (2.0 * k + 1.0) * (2.0 * k + 2.0);
        power /= n * n;
    }
    total
}

/// The Bessel function of the first kind, order zero.
#[must_use]
pub fn bessel_j0(x: f64) -> f64 {
    let ax = x.abs();
    if ax < 8.0 {
        let y = x * x;
        let numerator = 57_568_490_574.0
            + y * (-13_362_590_354.0
                + y * (651_619_640.7
                    + y * (-11_214_424.18 + y * (77_392.330_17 + y * (-184.905_245_6)))));
        let denominator = 57_568_490_411.0
            + y * (1_029_532_985.0
                + y * (9_494_680.718 + y * (59_272.648_53 + y * (267.853_271_2 + y))));
        numerator / denominator
    } else {
        let z = 8.0 / ax;
        let y = z * z;
        let xx = ax - 0.785_398_164;
        let first = 1.0
            + y * (-0.001_098_628_627
                + y * (0.000_027_345_104_07
                    + y * (-0.000_002_073_370_639 + y * 0.000_000_209_388_721_1)));
        let second = -0.015_624_999_95
            + y * (0.000_143_048_876_5
                + y * (-0.000_006_911_147_651
                    + y * (0.000_000_762_109_516_1 - y * 0.000_000_093_493_515_2)));
        (0.636_619_772 / ax).sqrt() * (xx.cos() * first - z * xx.sin() * second)
    }
}

/// The Bessel function of the first kind, order one.
#[must_use]
pub fn bessel_j1(x: f64) -> f64 {
    let ax = x.abs();
    if ax < 8.0 {
        let y = x * x;
        let numerator = x
            * (72_362_614_232.0
                + y * (-7_895_059_235.0
                    + y * (242_396_853.1
                        + y * (-2_972_611.439 + y * (15_704.482_60 + y * (-30.160_366_06))))));
        let denominator = 144_725_228_442.0
            + y * (2_300_535_178.0
                + y * (18_583_304.74 + y * (99_447.433_94 + y * (376.999_139_7 + y))));
        numerator / denominator
    } else {
        let z = 8.0 / ax;
        let y = z * z;
        let xx = ax - 2.356_194_491;
        let first = 1.0
            + y * (0.001_831_05
                + y * (-0.000_035_163_964_96
                    + y * (0.000_002_457_520_174 + y * (-0.000_000_240_337_019))));
        let second = 0.046_874_999_95
            + y * (-0.000_200_269_087_3
                + y * (0.000_008_449_199_096
                    + y * (-0.000_000_882_289_87 + y * 0.000_000_105_787_412)));
        let answer = (0.636_619_772 / ax).sqrt() * (xx.cos() * first - z * xx.sin() * second);
        if x < 0.0 { -answer } else { answer }
    }
}

/// The Bessel function of the second kind, order zero.
#[must_use]
pub fn bessel_y0(x: f64) -> f64 {
    if x < 8.0 {
        let y = x * x;
        let numerator = -2_957_821_389.0
            + y * (7_062_834_065.0
                + y * (-512_359_803.6
                    + y * (10_879_881.29 + y * (-86_327.927_57 + y * 228.462_273_3))));
        let denominator = 40_076_544_269.0
            + y * (745_249_964.8
                + y * (7_189_466.438 + y * (47_447.264_70 + y * (226.103_024_4 + y))));
        numerator / denominator + 0.636_619_772 * bessel_j0(x) * x.ln()
    } else {
        let z = 8.0 / x;
        let y = z * z;
        let xx = x - 0.785_398_164;
        let first = 1.0
            + y * (-0.001_098_628_627
                + y * (0.000_027_345_104_07
                    + y * (-0.000_002_073_370_639 + y * 0.000_000_209_388_721_1)));
        let second = -0.015_624_999_95
            + y * (0.000_143_048_876_5
                + y * (-0.000_006_911_147_651
                    + y * (0.000_000_762_109_516_1 + y * (-0.000_000_093_494_515_2))));
        (0.636_619_772 / x).sqrt() * (xx.sin() * first + z * xx.cos() * second)
    }
}

/// The Bessel function of the second kind, order one.
#[must_use]
pub fn bessel_y1(x: f64) -> f64 {
    if x < 8.0 {
        let y = x * x;
        let numerator = x
            * (-0.490_060_494_3e13
                + y * (0.127_527_439_0e13
                    + y * (-0.515_343_813_9e11
                        + y * (0.734_926_455_1e9
                            + y * (-0.423_792_272_6e7 + y * 0.851_193_793_5e4)))));
        let denominator = 0.249_958_057_0e14
            + y * (0.424_441_966_4e12
                + y * (0.373_365_036_7e10
                    + y * (0.224_590_400_2e8
                        + y * (0.102_042_605_0e6 + y * (0.354_963_288_5e3 + y)))));
        numerator / denominator + 0.636_619_772 * (bessel_j1(x) * x.ln() - 1.0 / x)
    } else {
        let z = 8.0 / x;
        let y = z * z;
        let xx = x - 2.356_194_491;
        let first = 1.0
            + y * (0.001_831_05
                + y * (-0.000_035_163_964_96
                    + y * (0.000_002_457_520_174 + y * (-0.000_000_240_337_019))));
        let second = 0.046_874_999_95
            + y * (-0.000_200_269_087_3
                + y * (0.000_008_449_199_096
                    + y * (-0.000_000_882_289_87 + y * 0.000_000_105_787_412)));
        (0.636_619_772 / x).sqrt() * (xx.sin() * first + z * xx.cos() * second)
    }
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
    let n = n.unsigned_abs();
    match n {
        | 0 => bessel_j0(x),
        | 1 => bessel_j1(x),
        | _ => {
            let ax = x.abs();
            if ax == 0.0 {
                return 0.0;
            }
            if ax > f64::from(n) {
                let mut previous = bessel_j0(x);
                let mut current = bessel_j1(x);
                for order in 1..n {
                    let next = 2.0 * f64::from(order) / x * current - previous;
                    previous = current;
                    current = next;
                }
                current
            } else {
                // Miller: start from a large order with an arbitrary seed and
                // renormalise, which is stable downwards.
                let start = n + 20 + (ax as u32).min(1_000);
                let mut next = 0.0;
                let mut current = 1e-30;
                let mut previous;
                let mut answer = 0.0;
                for order in (1..=start).rev() {
                    previous = f64::from(order) * 2.0 / ax * current - next;
                    next = current;
                    current = previous;
                    if current.abs() > 1e100 {
                        current *= 1e-100;
                        next *= 1e-100;
                        answer *= 1e-100;
                    }
                    if order == n {
                        answer = next;
                    }
                }
                let scale = bessel_j0(ax) / current;
                answer
                    * scale
                    * if x < 0.0 && n % 2 == 1 {
                        -1.0
                    } else {
                        1.0
                    }
            }
        },
    }
}

/// The Bessel function of the second kind, order `n`.
#[must_use]
pub fn bessel_yn(
    n: i32,
    x: f64,
) -> f64 {
    let n = n.unsigned_abs();
    match n {
        | 0 => bessel_y0(x),
        | 1 => bessel_y1(x),
        | _ => {
            let mut previous = bessel_y0(x);
            let mut current = bessel_y1(x);
            for order in 1..n {
                let next = 2.0 * f64::from(order) / x * current - previous;
                previous = current;
                current = next;
            }
            current
        },
    }
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
    let mut previous = 1.0;
    if n == 0 {
        return previous;
    }
    let mut current = x;
    for order in 1..n {
        let next = ((2.0 * f64::from(order) + 1.0) * x * current - f64::from(order) * previous)
            / (f64::from(order) + 1.0);
        previous = current;
        current = next;
    }
    current
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
    let mut previous = 1.0;
    if n == 0 {
        return previous;
    }
    let mut current = 2.0 * x;
    for order in 1..n {
        let next = 2.0 * x * current - 2.0 * f64::from(order) * previous;
        previous = current;
        current = next;
    }
    current
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
    let mut previous = 1.0;
    if n == 0 {
        return previous;
    }
    let mut current = 1.0 - x;
    for order in 1..n {
        let next = ((2.0 * f64::from(order) + 1.0 - x) * current - f64::from(order) * previous)
            / (f64::from(order) + 1.0);
        previous = current;
        current = next;
    }
    current
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
    let mut previous = 1.0;
    if n == 0 {
        return previous;
    }
    let mut current = x;
    for _ in 1..n {
        let next = 2.0 * x * current - previous;
        previous = current;
        current = next;
    }
    current
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
    let mut previous = 1.0;
    if n == 0 {
        return previous;
    }
    let mut current = 2.0 * x;
    for _ in 1..n {
        let next = 2.0 * x * current - previous;
        previous = current;
        current = next;
    }
    current
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
