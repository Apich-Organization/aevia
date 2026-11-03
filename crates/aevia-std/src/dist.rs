//! The ten distributions of §8.7.1.
//!
//! A distribution is a plain record at run time, so it is a plain struct here:
//! `Normal<L>` is `{ mu: Real[L], sigma: Real[L] }` and the runtime reads the
//! fields before calling [`Distribution::pdf`]. Dimensions are the runtime's
//! business; these functions work on the coherent values.
//!
//! `pdf` and `cdf` are exact where they can be (`Uniform`, `Exponential`) and
//! use the special functions of [`crate::special`] where they cannot.
//! `sample` takes a uniform source instead of owning a generator, so the
//! interpreter's seeded RNG stays the single source of randomness (§6.10).

use crate::special::{ln_gamma, regularized_beta, regularized_gamma_p};
use std::f64::consts::PI;

/// One of the ten distributions §8.7.1 declares.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Distribution {
    /// `Normal<d> = { mu, sigma }`.
    Normal {
        /// The mean.
        mu: f64,
        /// The standard deviation.
        sigma: f64,
    },
    /// `Uniform<d> = { lo, hi }`.
    Uniform {
        /// The lower bound.
        lo: f64,
        /// The upper bound.
        hi: f64,
    },
    /// `Exponential<d> = { rate }`.
    Exponential {
        /// The rate, in events per unit.
        rate: f64,
    },
    /// `Gamma<d> = { shape, rate }`.
    Gamma {
        /// The shape `k`.
        shape: f64,
        /// The rate `λ`.
        rate: f64,
    },
    /// `Beta = { alpha, beta }`.
    Beta {
        /// The first shape.
        alpha: f64,
        /// The second shape.
        beta: f64,
    },
    /// `ChiSquared = { df }`.
    ChiSquared {
        /// The degrees of freedom.
        df: f64,
    },
    /// `StudentT = { df }`.
    StudentT {
        /// The degrees of freedom.
        df: f64,
    },
    /// `Poisson = { lambda }`.
    Poisson {
        /// The mean rate.
        lambda: f64,
    },
    /// `Binomial = { n, p }`.
    Binomial {
        /// The number of trials.
        n: u64,
        /// The success probability.
        p: f64,
    },
}

impl Distribution {
    /// The probability density (or mass) at `x`.
    #[must_use]
    pub fn pdf(&self, x: f64) -> f64 {
        match *self {
            Self::Normal { mu, sigma } => {
                if sigma <= 0.0 {
                    return f64::NAN;
                }
                let z = (x - mu) / sigma;
                (-0.5 * z * z).exp() / (sigma * (2.0 * PI).sqrt())
            }
            Self::Uniform { lo, hi } => {
                if hi <= lo {
                    f64::NAN
                } else if x >= lo && x <= hi {
                    1.0 / (hi - lo)
                } else {
                    0.0
                }
            }
            Self::Exponential { rate } => {
                if rate <= 0.0 {
                    f64::NAN
                } else if x < 0.0 {
                    0.0
                } else {
                    rate * (-rate * x).exp()
                }
            }
            Self::Gamma { shape, rate } => {
                if shape <= 0.0 || rate <= 0.0 {
                    return f64::NAN;
                }
                if x < 0.0 {
                    return 0.0;
                }
                if x == 0.0 {
                    return if shape < 1.0 {
                        f64::INFINITY
                    } else if shape == 1.0 {
                        rate
                    } else {
                        0.0
                    };
                }
                ((shape - 1.0) * x.ln() - rate * x + shape * rate.ln() - ln_gamma(shape)).exp()
            }
            Self::Beta { alpha, beta } => {
                if alpha <= 0.0 || beta <= 0.0 {
                    return f64::NAN;
                }
                if !(0.0..=1.0).contains(&x) {
                    return 0.0;
                }
                let log = (alpha - 1.0) * x.ln()
                    + (beta - 1.0) * (1.0 - x).ln()
                    + ln_gamma(alpha + beta)
                    - ln_gamma(alpha)
                    - ln_gamma(beta);
                log.exp()
            }
            Self::ChiSquared { df } => {
                if df <= 0.0 {
                    return f64::NAN;
                }
                Self::Gamma {
                    shape: df / 2.0,
                    rate: 0.5,
                }
                .pdf(x)
            }
            Self::StudentT { df } => {
                if df <= 0.0 {
                    return f64::NAN;
                }
                (ln_gamma((df + 1.0) / 2.0)
                    - ln_gamma(df / 2.0)
                    - 0.5 * (df * PI).ln()
                    - (df + 1.0) / 2.0 * (1.0 + x * x / df).ln())
                .exp()
            }
            Self::Poisson { lambda } => {
                if lambda < 0.0 {
                    return f64::NAN;
                }
                if x < 0.0 || x.fract() != 0.0 {
                    return 0.0;
                }
                if lambda == 0.0 {
                    return if x == 0.0 { 1.0 } else { 0.0 };
                }
                (-lambda + x * lambda.ln() - ln_gamma(x + 1.0)).exp()
            }
            Self::Binomial { n, p } => {
                if !(0.0..=1.0).contains(&p) {
                    return f64::NAN;
                }
                if x < 0.0 || x.fract() != 0.0 || x > n as f64 {
                    return 0.0;
                }
                let k = x;
                (ln_gamma(n as f64 + 1.0) - ln_gamma(k + 1.0) - ln_gamma(n as f64 - k + 1.0)
                    + k * p.ln()
                    + (n as f64 - k) * (1.0 - p).ln())
                .exp()
            }
        }
    }

    /// The cumulative distribution function at `x`.
    #[must_use]
    pub fn cdf(&self, x: f64) -> f64 {
        match *self {
            Self::Normal { mu, sigma } => {
                if sigma <= 0.0 {
                    return f64::NAN;
                }
                let z = (x - mu) / sigma;
                0.5 * crate::special::erfc(-z / std::f64::consts::SQRT_2)
            }
            Self::Uniform { lo, hi } => {
                if hi <= lo {
                    return f64::NAN;
                }
                ((x - lo) / (hi - lo)).clamp(0.0, 1.0)
            }
            Self::Exponential { rate } => {
                if rate <= 0.0 {
                    return f64::NAN;
                }
                if x < 0.0 {
                    0.0
                } else {
                    -(-rate * x).exp_m1()
                }
            }
            Self::Gamma { shape, rate } => {
                if shape <= 0.0 || rate <= 0.0 {
                    return f64::NAN;
                }
                if x <= 0.0 {
                    0.0
                } else {
                    regularized_gamma_p(shape, rate * x)
                }
            }
            Self::Beta { alpha, beta } => regularized_beta(x, alpha, beta),
            Self::ChiSquared { df } => {
                if df <= 0.0 {
                    return f64::NAN;
                }
                if x <= 0.0 {
                    0.0
                } else {
                    regularized_gamma_p(df / 2.0, x / 2.0)
                }
            }
            Self::StudentT { df } => {
                if df <= 0.0 {
                    return f64::NAN;
                }
                let tail = 0.5 * regularized_beta(df / (df + x * x), df / 2.0, 0.5);
                if x >= 0.0 { 1.0 - tail } else { tail }
            }
            Self::Poisson { lambda } => {
                if lambda < 0.0 {
                    return f64::NAN;
                }
                if x < 0.0 {
                    return 0.0;
                }
                // P(X <= k) = Q(k+1, lambda).
                1.0 - regularized_gamma_p((x.floor() + 1.0).max(0.0), lambda)
            }
            Self::Binomial { n, p } => {
                if !(0.0..=1.0).contains(&p) {
                    return f64::NAN;
                }
                if x < 0.0 {
                    return 0.0;
                }
                if x >= n as f64 {
                    return 1.0;
                }
                // P(X <= k) = I_{1-p}(n-k, k+1).
                regularized_beta(1.0 - p, n as f64 - x.floor(), x.floor() + 1.0)
            }
        }
    }

    /// The mean, when it exists.
    #[must_use]
    pub fn mean(&self) -> f64 {
        match *self {
            Self::Normal { mu, .. } => mu,
            Self::Uniform { lo, hi } => (lo + hi) / 2.0,
            Self::Exponential { rate } => 1.0 / rate,
            Self::Gamma { shape, rate } => shape / rate,
            Self::Beta { alpha, beta } => alpha / (alpha + beta),
            Self::ChiSquared { df } => df,
            Self::StudentT { df } => {
                if df > 1.0 { 0.0 } else { f64::NAN }
            }
            Self::Poisson { lambda } => lambda,
            Self::Binomial { n, p } => n as f64 * p,
        }
    }

    /// The variance, when it exists.
    #[must_use]
    pub fn variance(&self) -> f64 {
        match *self {
            Self::Normal { sigma, .. } => sigma * sigma,
            Self::Uniform { lo, hi } => (hi - lo) * (hi - lo) / 12.0,
            Self::Exponential { rate } => 1.0 / (rate * rate),
            Self::Gamma { shape, rate } => shape / (rate * rate),
            Self::Beta { alpha, beta } => {
                alpha * beta / ((alpha + beta) * (alpha + beta) * (alpha + beta + 1.0))
            }
            Self::ChiSquared { df } => 2.0 * df,
            Self::StudentT { df } => {
                if df > 2.0 {
                    df / (df - 2.0)
                } else {
                    f64::NAN
                }
            }
            Self::Poisson { lambda } => lambda,
            Self::Binomial { n, p } => n as f64 * p * (1.0 - p),
        }
    }

    /// The `u`-quantile, `0 ≤ u ≤ 1`: the inverse of [`Distribution::cdf`].
    ///
    /// This is what `sample` uses at run time, because the interpreter's RNG
    /// hands out one uniform per action (§6.10) and an inverse-CDF sample needs
    /// exactly one. Continuous distributions invert by bisection on the CDF,
    /// which converges to machine precision in sixty steps and needs no
    /// distribution-specific algebra; discrete ones walk their mass function.
    ///
    /// # Errors
    ///
    /// [`crate::NumericError::Domain`] when `u` is outside `[0, 1]` or a
    /// parameter is unusable.
    pub fn quantile(&self, u: f64) -> Result<f64, crate::NumericError> {
        if !(0.0..=1.0).contains(&u) {
            return Err(crate::NumericError::Domain(
                "a quantile needs `0 <= u <= 1`".to_string(),
            ));
        }
        match *self {
            Self::Poisson { lambda } => {
                if lambda < 0.0 {
                    return Err(crate::NumericError::Domain(
                        "a Poisson rate cannot be negative".to_string(),
                    ));
                }
                let mut cumulative = 0.0;
                let mut count = 0.0;
                // The mass beyond `lambda + 40 sqrt(lambda) + 40` is below any
                // double, so the walk terminates.
                let limit = lambda + 40.0 * lambda.sqrt() + 40.0;
                while count <= limit {
                    cumulative += self.pdf(count);
                    if cumulative >= u {
                        return Ok(count);
                    }
                    count += 1.0;
                }
                Ok(count)
            }
            Self::Binomial { n, .. } => {
                let mut cumulative = 0.0;
                for count in 0..=n {
                    cumulative += self.pdf(count as f64);
                    if cumulative >= u {
                        return Ok(count as f64);
                    }
                }
                Ok(n as f64)
            }
            _ => {
                let mut low = self.support_low();
                let mut high = self.support_high();
                if !low.is_finite() || !high.is_finite() {
                    // A wide bracket, then bisection: the CDF is monotone, so
                    // the answer is bracketed as soon as `cdf(high) >= u`.
                    low = -1.0;
                    high = 1.0;
                    let mut guard = 0;
                    while self.cdf(low) > u && guard < 200 {
                        low *= 2.0;
                        guard += 1;
                    }
                    guard = 0;
                    while self.cdf(high) < u && guard < 200 {
                        high *= 2.0;
                        guard += 1;
                    }
                }
                for _ in 0..200 {
                    let middle = (low + high) / 2.0;
                    if self.cdf(middle) < u {
                        low = middle;
                    } else {
                        high = middle;
                    }
                    if (high - low) <= 1e-15 * middle.abs().max(1.0) {
                        break;
                    }
                }
                Ok((low + high) / 2.0)
            }
        }
    }

    /// The lower end of the support, or `-inf`.
    fn support_low(&self) -> f64 {
        match *self {
            Self::Uniform { lo, .. } => lo,
            Self::Exponential { .. } | Self::Gamma { .. } | Self::ChiSquared { .. } => 0.0,
            Self::Beta { .. } => 0.0,
            _ => f64::NEG_INFINITY,
        }
    }

    /// The upper end of the support, or `+inf`.
    fn support_high(&self) -> f64 {
        match *self {
            Self::Uniform { hi, .. } => hi,
            Self::Beta { .. } => 1.0,
            _ => f64::INFINITY,
        }
    }

    /// One sample, from a uniform source on `[0, 1)`.
    ///
    /// # Errors
    ///
    /// [`crate::NumericError::Domain`] for a parameter the sampler cannot use.
    pub fn sample(
        &self,
        uniform: &mut dyn FnMut() -> f64,
    ) -> Result<f64, crate::NumericError> {
        let one = |u: f64| if u <= 0.0 { f64::MIN_POSITIVE } else { u };
        Ok(match *self {
            Self::Normal { mu, sigma } => {
                // Box-Muller; the second uniform is discarded.
                let u1 = one(uniform());
                let u2 = uniform();
                mu + sigma * (-2.0 * u1.ln()).sqrt() * (2.0 * PI * u2).cos()
            }
            Self::Uniform { lo, hi } => lo + (hi - lo) * uniform(),
            Self::Exponential { rate } => -one(uniform()).ln() / rate,
            Self::Gamma { shape, rate } => gamma_sample(shape, uniform)? / rate,
            Self::Beta { alpha, beta } => {
                let x = gamma_sample(alpha, uniform)?;
                let y = gamma_sample(beta, uniform)?;
                if x + y == 0.0 { 0.5 } else { x / (x + y) }
            }
            Self::ChiSquared { df } => 2.0 * gamma_sample(df / 2.0, uniform)?,
            Self::StudentT { df } => {
                let z = Self::Normal { mu: 0.0, sigma: 1.0 }.sample(uniform)?;
                let v = Self::ChiSquared { df }.sample(uniform)?;
                z / (v / df).sqrt()
            }
            Self::Poisson { lambda } => {
                if lambda <= 0.0 {
                    0.0
                } else if lambda < 30.0 {
                    // Knuth's product of uniforms.
                    let limit = (-lambda).exp();
                    let mut product = one(uniform());
                    let mut count = 0.0;
                    while product > limit {
                        count += 1.0;
                        product *= one(uniform());
                    }
                    count
                } else {
                    // Normal approximation with a continuity correction.
                    (Self::Normal {
                        mu: lambda,
                        sigma: lambda.sqrt(),
                    }
                    .sample(uniform)?)
                    .round()
                }
            }
            Self::Binomial { n, p } => {
                let mut successes = 0.0;
                for _ in 0..n {
                    if uniform() < p {
                        successes += 1.0;
                    }
                }
                successes
            }
        })
    }
}

/// Marsaglia–Tsang for `Gamma(shape, 1)`.
fn gamma_sample(
    shape: f64,
    uniform: &mut dyn FnMut() -> f64,
) -> Result<f64, crate::NumericError> {
    if shape <= 0.0 {
        return Err(crate::NumericError::Domain(
            "a Gamma shape must be positive".to_string(),
        ));
    }
    if shape < 1.0 {
        // Boost by one and scale by U^(1/shape).
        let boosted = gamma_sample(shape + 1.0, uniform)?;
        let u = uniform().max(f64::MIN_POSITIVE);
        return Ok(boosted * u.powf(1.0 / shape));
    }
    let d = shape - 1.0 / 3.0;
    let c = 1.0 / (9.0 * d).sqrt();
    loop {
        let mut x;
        let mut v;
        loop {
            let normal = Distribution::Normal {
                mu: 0.0,
                sigma: 1.0,
            }
            .sample(uniform)?;
            x = normal;
            v = 1.0 + c * x;
            if v > 0.0 {
                break;
            }
        }
        v = v * v * v;
        let u = uniform().max(f64::MIN_POSITIVE);
        if u.ln() < 0.5 * x * x + d - d * v + d * v.ln() {
            return Ok(d * v);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn close(a: f64, b: f64, tolerance: f64) -> bool {
        (a - b).abs() <= tolerance
    }

    #[test]
    fn the_normal_density_integrates_to_its_cdf() {
        let normal = Distribution::Normal {
            mu: 0.0,
            sigma: 1.0,
        };
        assert!(close(normal.pdf(0.0), 1.0 / (2.0 * PI).sqrt(), 1e-12));
        assert!(close(normal.cdf(0.0), 0.5, 1e-12));
        assert!(close(normal.cdf(1.96), 0.975_002_104_851_780_1, 1e-8));
        assert!(close(normal.cdf(-1.96), 0.024_997_895_148_219_9, 1e-8));
    }

    #[test]
    fn the_uniform_density_is_the_reciprocal_of_its_width() {
        let uniform = Distribution::Uniform { lo: 0.0, hi: 4.0 };
        assert!(close(uniform.pdf(1.0), 0.25, 1e-12));
        assert!(close(uniform.pdf(5.0), 0.0, 1e-12));
        assert!(close(uniform.cdf(2.0), 0.5, 1e-12));
    }

    #[test]
    fn the_exponential_cdf_is_one_minus_the_survival() {
        let exponential = Distribution::Exponential { rate: 2.0 };
        assert!(close(exponential.cdf(0.5), 1.0 - (-1.0f64).exp(), 1e-12));
        assert!(close(exponential.pdf(0.0), 2.0, 1e-12));
        assert!(close(exponential.mean(), 0.5, 1e-12));
    }

    #[test]
    fn the_gamma_cdf_matches_the_chi_squared_of_two_degrees() {
        // Gamma(1, 1/2) is ChiSquared(2).
        let gamma = Distribution::Gamma {
            shape: 1.0,
            rate: 0.5,
        };
        let chi = Distribution::ChiSquared { df: 2.0 };
        for x in [0.5, 1.0, 3.0] {
            assert!(close(gamma.cdf(x), chi.cdf(x), 1e-10), "at {x}");
        }
    }

    #[test]
    fn the_student_t_with_many_degrees_approaches_the_normal() {
        let student = Distribution::StudentT { df: 1_000_000.0 };
        let normal = Distribution::Normal {
            mu: 0.0,
            sigma: 1.0,
        };
        for x in [-1.0, 0.0, 1.5] {
            assert!(close(student.cdf(x), normal.cdf(x), 1e-4), "at {x}");
        }
    }

    #[test]
    fn the_beta_cdf_of_a_uniform_is_the_identity() {
        let beta = Distribution::Beta {
            alpha: 1.0,
            beta: 1.0,
        };
        for x in [0.1, 0.5, 0.9] {
            assert!(close(beta.cdf(x), x, 1e-12), "at {x}");
        }
    }

    #[test]
    fn a_poisson_cdf_is_one_at_a_large_argument() {
        let poisson = Distribution::Poisson { lambda: 3.0 };
        assert!(close(poisson.cdf(100.0), 1.0, 1e-12));
        assert!(close(poisson.pdf(0.0), (-3.0f64).exp(), 1e-12));
        // The masses sum to one.
        let total: f64 = (0..60).map(|k| poisson.pdf(k as f64)).sum();
        assert!(close(total, 1.0, 1e-10));
    }

    #[test]
    fn a_binomial_cdf_reaches_one_at_n() {
        let binomial = Distribution::Binomial { n: 10, p: 0.5 };
        assert!(close(binomial.pdf(5.0), 252.0 / 1024.0, 1e-12));
        assert!(close(binomial.cdf(10.0), 1.0, 1e-12));
        assert!(close(binomial.mean(), 5.0, 1e-12));
        assert!(close(binomial.variance(), 2.5, 1e-12));
    }

    #[test]
    fn sampling_stays_inside_the_support() {
        // A deterministic source: the same uniforms every time.
        let mut state = 0u64;
        let mut uniform = move || {
            state = state.wrapping_mul(6_364_136_223_846_793_005).wrapping_add(1);
            ((state >> 11) as f64) / ((1u64 << 53) as f64)
        };
        let uniform_dist = Distribution::Uniform { lo: 2.0, hi: 3.0 };
        for _ in 0..100 {
            let value = uniform_dist.sample(&mut uniform).unwrap();
            assert!((2.0..=3.0).contains(&value));
        }
        let poisson = Distribution::Poisson { lambda: 4.0 };
        for _ in 0..100 {
            let value = poisson.sample(&mut uniform).unwrap();
            assert!(value >= 0.0 && value.fract() == 0.0);
        }
    }
}

#[cfg(test)]
mod quantile_tests {
    use super::*;

    #[test]
    fn the_quantile_inverts_the_cdf() {
        let cases = [
            Distribution::Normal { mu: 2.0, sigma: 0.5 },
            Distribution::Uniform { lo: -1.0, hi: 3.0 },
            Distribution::Exponential { rate: 1.5 },
            Distribution::Gamma { shape: 2.5, rate: 1.0 },
            Distribution::Beta { alpha: 2.0, beta: 5.0 },
            Distribution::ChiSquared { df: 4.0 },
            Distribution::StudentT { df: 7.0 },
        ];
        for distribution in cases {
            for u in [0.05, 0.3, 0.5, 0.9, 0.99] {
                let x = distribution.quantile(u).unwrap();
                let back = distribution.cdf(x);
                assert!(
                    (back - u).abs() < 1e-6,
                    "{distribution:?} at {u}: cdf({x}) = {back}"
                );
            }
        }
    }

    #[test]
    fn a_discrete_quantile_lands_on_a_mass_point() {
        let poisson = Distribution::Poisson { lambda: 3.0 };
        for u in [0.1, 0.5, 0.9] {
            let value = poisson.quantile(u).unwrap();
            assert_eq!(value.fract(), 0.0);
            assert!(poisson.cdf(value) >= u - 1e-12);
            assert!(value == 0.0 || poisson.cdf(value - 1.0) < u);
        }
        let binomial = Distribution::Binomial { n: 8, p: 0.4 };
        let median = binomial.quantile(0.5).unwrap();
        assert_eq!(median.fract(), 0.0);
        assert!((0.0..=8.0).contains(&median));
    }

    #[test]
    fn a_quantile_outside_the_unit_interval_is_refused() {
        let normal = Distribution::Normal { mu: 0.0, sigma: 1.0 };
        assert!(normal.quantile(1.5).is_err());
        assert!(normal.quantile(-0.1).is_err());
    }
}
