//! Probability distributions, backed by `rssn::kernels::distributions`.

use crate::NumericError;
use rssn::kernels::distributions::DistError;
use rssn::kernels::distributions::Distribution as RssnDist;
use rssn::kernels::distributions::MultivariateDistribution as RssnMvDist;

fn map_dist_err(err: DistError) -> NumericError {
    match err {
        | DistError::Empty => NumericError::Empty,
        | DistError::ShapeMismatch(s) => NumericError::ShapeMismatch(s),
        | DistError::Domain(s) => NumericError::Domain(s),
        | DistError::NotPositiveDefinite => NumericError::NotPositiveDefinite,
    }
}

/// One of the univariate distributions.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Distribution {
    /// `Normal<d> = { mu, sigma }`.
    Normal { mu: f64, sigma: f64 },
    /// `Uniform<d> = { lo, hi }`.
    Uniform { lo: f64, hi: f64 },
    /// `Exponential<d> = { rate }`.
    Exponential { rate: f64 },
    /// `Gamma<d> = { shape, rate }`.
    Gamma { shape: f64, rate: f64 },
    /// `Beta = { alpha, beta }`.
    Beta { alpha: f64, beta: f64 },
    /// `ChiSquared = { df }`.
    ChiSquared { df: f64 },
    /// `StudentT = { df }`.
    StudentT { df: f64 },
    /// `Poisson = { lambda }`.
    Poisson { lambda: f64 },
    /// `Binomial = { n, p }`.
    Binomial { n: u64, p: f64 },
    /// `LogNormal<d> = { mu, sigma }`.
    LogNormal { mu: f64, sigma: f64 },
    /// `Weibull<d> = { shape, scale }`.
    Weibull { shape: f64, scale: f64 },
    /// `Cauchy<d> = { loc, scale }`.
    Cauchy { loc: f64, scale: f64 },
    /// `F = { df1, df2 }` (Snedecor's F-distribution).
    F { df1: f64, df2: f64 },
    /// `NegativeBinomial = { r, p }`.
    NegativeBinomial { r: f64, p: f64 },
    /// `Geometric = { p }`.
    Geometric { p: f64 },
}

impl Distribution {
    fn to_rssn(self) -> RssnDist {
        match self {
            | Self::Normal { mu, sigma } => RssnDist::Normal { mu, sigma },
            | Self::Uniform { lo, hi } => RssnDist::Uniform { lo, hi },
            | Self::Exponential { rate } => RssnDist::Exponential { rate },
            | Self::Gamma { shape, rate } => RssnDist::Gamma { shape, rate },
            | Self::Beta { alpha, beta } => RssnDist::Beta { alpha, beta },
            | Self::ChiSquared { df } => RssnDist::ChiSquared { df },
            | Self::StudentT { df } => RssnDist::StudentT { df },
            | Self::Poisson { lambda } => RssnDist::Poisson { lambda },
            | Self::Binomial { n, p } => RssnDist::Binomial { n, p },
            | Self::LogNormal { mu, sigma } => RssnDist::LogNormal { mu, sigma },
            | Self::Weibull { shape, scale } => RssnDist::Weibull { shape, scale },
            | Self::Cauchy { loc, scale } => RssnDist::Cauchy { loc, scale },
            | Self::F { df1, df2 } => RssnDist::F { df1, df2 },
            | Self::NegativeBinomial { r, p } => RssnDist::NegativeBinomial { r, p },
            | Self::Geometric { p } => RssnDist::Geometric { p },
        }
    }

    #[must_use]
    pub fn pdf(
        &self,
        x: f64,
    ) -> f64 {
        self.to_rssn().pdf(x)
    }

    #[must_use]
    pub fn cdf(
        &self,
        x: f64,
    ) -> f64 {
        self.to_rssn().cdf(x)
    }

    #[must_use]
    pub fn mean(&self) -> f64 {
        self.to_rssn().mean()
    }

    #[must_use]
    pub fn variance(&self) -> f64 {
        self.to_rssn().variance()
    }

    pub fn quantile(
        &self,
        u: f64,
    ) -> Result<f64, NumericError> {
        self.to_rssn().quantile(u).map_err(map_dist_err)
    }

    pub fn sample(
        &self,
        uniforms: &mut impl FnMut() -> f64,
    ) -> Result<f64, NumericError> {
        self.to_rssn().sample(uniforms).map_err(map_dist_err)
    }
}

/// Multivariate probability distributions.
#[derive(Clone, Debug, PartialEq)]
pub enum MultivariateDistribution {
    /// `MultivariateNormal = { mu, cov }`.
    MultivariateNormal { mu: Vec<f64>, cov: Vec<f64> },
    /// `Dirichlet = { alpha }`.
    Dirichlet { alpha: Vec<f64> },
}

impl MultivariateDistribution {
    fn to_rssn(&self) -> RssnMvDist {
        match self {
            | Self::MultivariateNormal { mu, cov } => {
                RssnMvDist::MultivariateNormal {
                    mu: mu.clone(),
                    cov: cov.clone(),
                }
            },
            | Self::Dirichlet { alpha } => RssnMvDist::Dirichlet { alpha: alpha.clone() },
        }
    }

    #[must_use]
    pub fn dim(&self) -> usize {
        match self {
            | Self::MultivariateNormal { mu, .. } => mu.len(),
            | Self::Dirichlet { alpha } => alpha.len(),
        }
    }

    pub fn log_pdf(
        &self,
        x: &[f64],
    ) -> Result<f64, NumericError> {
        self.to_rssn().log_pdf(x).map_err(map_dist_err)
    }

    pub fn pdf(
        &self,
        x: &[f64],
    ) -> Result<f64, NumericError> {
        self.to_rssn().pdf(x).map_err(map_dist_err)
    }

    pub fn mean(&self) -> Result<Vec<f64>, NumericError> {
        self.to_rssn().mean().map_err(map_dist_err)
    }

    /// The row-major covariance matrix.
    pub fn cov(&self) -> Result<Vec<f64>, NumericError> {
        self.to_rssn().cov().map_err(map_dist_err)
    }

    pub fn sample(
        &self,
        uniforms: &mut impl FnMut() -> f64,
    ) -> Result<Vec<f64>, NumericError> {
        self.to_rssn().sample(uniforms).map_err(map_dist_err)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::f64::consts::PI;

    fn close(
        a: f64,
        b: f64,
        tolerance: f64,
    ) -> bool {
        (a - b).abs() <= tolerance
    }

    #[test]
    fn the_normal_density_integrates_to_its_cdf() {
        let normal = Distribution::Normal { mu: 0.0, sigma: 1.0 };
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
        let normal = Distribution::Normal { mu: 0.0, sigma: 1.0 };
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
            state = state
                .wrapping_mul(6_364_136_223_846_793_005)
                .wrapping_add(1);
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
    use std::f64::consts::PI;

    #[test]
    fn the_quantile_inverts_the_cdf() {
        let cases = [
            Distribution::Normal { mu: 2.0, sigma: 0.5 },
            Distribution::Uniform { lo: -1.0, hi: 3.0 },
            Distribution::Exponential { rate: 1.5 },
            Distribution::Gamma {
                shape: 2.5,
                rate: 1.0,
            },
            Distribution::Beta {
                alpha: 2.0,
                beta: 5.0,
            },
            Distribution::ChiSquared { df: 4.0 },
            Distribution::StudentT { df: 7.0 },
            Distribution::LogNormal { mu: 0.2, sigma: 0.5 },
            Distribution::Weibull {
                shape: 2.0,
                scale: 1.5,
            },
            Distribution::Cauchy {
                loc: -1.0,
                scale: 0.7,
            },
            Distribution::F { df1: 5.0, df2: 8.0 },
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

    #[test]
    fn lognormal_matches_exp_of_normal() {
        let ln_dist = Distribution::LogNormal { mu: 0.5, sigma: 0.4 };
        let normal = Distribution::Normal { mu: 0.5, sigma: 0.4 };
        for x in [0.5, 1.5, 3.0] {
            assert!((ln_dist.cdf(x) - normal.cdf(x.ln())).abs() < 1e-12);
        }
        assert!((ln_dist.mean() - (0.5 + 0.08_f64).exp()).abs() < 1e-12);
        assert_eq!(ln_dist.pdf(0.0), 0.0);
        assert_eq!(ln_dist.cdf(-1.0), 0.0);
    }

    #[test]
    fn weibull_shape_one_is_exponential() {
        let w = Distribution::Weibull {
            shape: 1.0,
            scale: 2.0,
        };
        let e = Distribution::Exponential { rate: 0.5 };
        for x in [0.0, 0.5, 2.0, 5.0] {
            assert!((w.pdf(x) - e.pdf(x)).abs() < 1e-12);
            assert!((w.cdf(x) - e.cdf(x)).abs() < 1e-12);
        }
        assert!((w.mean() - e.mean()).abs() < 1e-10);
        assert!((w.variance() - e.variance()).abs() < 1e-10);
    }

    #[test]
    fn cauchy_has_median_at_loc_and_undefined_moments() {
        let c = Distribution::Cauchy { loc: 2.0, scale: 3.0 };
        assert!((c.cdf(2.0) - 0.5).abs() < 1e-12);
        assert!((c.cdf(5.0) - 0.75).abs() < 1e-12);
        assert!(c.mean().is_nan());
        assert!(c.variance().is_nan());
    }

    #[test]
    fn f_distribution_df1_one_equals_squared_t() {
        let f = Distribution::F { df1: 1.0, df2: 5.0 };
        let t = Distribution::StudentT { df: 5.0 };
        for x in [0.5, 1.0, 2.5] {
            let p_f = f.cdf(x);
            let p_t = 2.0 * t.cdf(x.sqrt()) - 1.0;
            assert!((p_f - p_t).abs() < 1e-10, "at {x}: {p_f} vs {p_t}");
        }
        assert!((f.mean() - 5.0 / 3.0).abs() < 1e-12);
        assert!((f.variance() - (2.0 * 25.0 * 4.0) / (1.0 * 9.0 * 1.0)).abs() < 1e-12);
    }

    #[test]
    fn negative_binomial_and_geometric_consistency() {
        let nb = Distribution::NegativeBinomial { r: 1.0, p: 0.3 };
        let g = Distribution::Geometric { p: 0.3 };
        for k in 0..8 {
            assert!((nb.pdf(k as f64) - g.pdf(k as f64)).abs() < 1e-12);
            assert!((nb.cdf(k as f64) - g.cdf(k as f64)).abs() < 1e-12);
        }
        assert!((g.mean() - 0.7 / 0.3).abs() < 1e-12);
        assert!((g.variance() - 0.7 / 0.09).abs() < 1e-12);
        let q = nb.quantile(0.6).unwrap();
        assert!(nb.cdf(q) >= 0.6 - 1e-12);
        assert!(q == 0.0 || nb.cdf(q - 1.0) < 0.6);
    }

    #[test]
    fn multivariate_normal_pdf_and_sampling() {
        let mvn = MultivariateDistribution::MultivariateNormal {
            mu: vec![1.0, -2.0],
            cov: vec![2.0, 0.5, 0.5, 1.0],
        };
        assert_eq!(mvn.dim(), 2);
        assert_eq!(mvn.mean().unwrap(), vec![1.0, -2.0]);
        let p = mvn.pdf(&[1.0, -2.0]).unwrap();
        // det = 2*1 - 0.25 = 1.75; 1 / (2*pi*sqrt(1.75))
        let expected = 1.0 / (2.0 * PI * 1.75_f64.sqrt());
        assert!((p - expected).abs() < 1e-12);

        let mut state = 42u64;
        let mut uniform = move || {
            state = state
                .wrapping_mul(6_364_136_223_846_793_005)
                .wrapping_add(1);
            ((state >> 11) as f64) / ((1u64 << 53) as f64)
        };
        let s = mvn.sample(&mut uniform).unwrap();
        assert_eq!(s.len(), 2);
        assert!(s.iter().all(|v| v.is_finite()));
    }

    #[test]
    fn dirichlet_moments_and_simplex_sampling() {
        let dir = MultivariateDistribution::Dirichlet {
            alpha: vec![2.0, 3.0, 5.0],
        };
        let mean = dir.mean().unwrap();
        assert!((mean[0] - 0.2).abs() < 1e-12);
        assert!((mean[1] - 0.3).abs() < 1e-12);
        assert!((mean[2] - 0.5).abs() < 1e-12);

        // Uniform Dirichlet(1, 1, 1) has density 2! = 2 on the 2-simplex.
        let unif_dir = MultivariateDistribution::Dirichlet {
            alpha: vec![1.0, 1.0, 1.0],
        };
        assert!((unif_dir.pdf(&[0.2, 0.3, 0.5]).unwrap() - 2.0).abs() < 1e-12);
        assert_eq!(unif_dir.pdf(&[0.2, 0.3, 0.6]).unwrap(), 0.0);

        let mut state = 7u64;
        let mut uniform = move || {
            state = state
                .wrapping_mul(6_364_136_223_846_793_005)
                .wrapping_add(1);
            ((state >> 11) as f64) / ((1u64 << 53) as f64)
        };
        for _ in 0..20 {
            let s = dir.sample(&mut uniform).unwrap();
            assert_eq!(s.len(), 3);
            assert!(s.iter().all(|&v| v >= 0.0));
            assert!((s.iter().sum::<f64>() - 1.0).abs() < 1e-12);
        }
    }
}

#[cfg(test)]
mod kernel_regressions {
    use super::*;

    #[test]
    fn quantiles_preserve_support_endpoints_and_large_discrete_tails() {
        let normal = Distribution::Normal { mu: 0.0, sigma: 1.0 };
        assert_eq!(normal.quantile(0.0).unwrap(), f64::NEG_INFINITY);
        assert_eq!(normal.quantile(1.0).unwrap(), f64::INFINITY);
        let geometric = Distribution::Geometric { p: 1e-8 };
        let q = geometric.quantile(0.9).unwrap();
        assert!(q > 1_000_000.0);
        assert!(geometric.cdf(q) >= 0.9);
        assert!(geometric.cdf(q - 1.0) < 0.9);
        assert_eq!(geometric.quantile(1.0).unwrap(), f64::INFINITY);
        let beta = Distribution::Beta {
            alpha: 2.0,
            beta: 3.0,
        };
        assert_eq!(beta.cdf(-1.0), 0.0);
        assert_eq!(beta.cdf(2.0), 1.0);
    }

    #[test]
    fn invalid_parameters_and_covariances_are_rejected() {
        for distribution in [
            Distribution::Normal { mu: 0.0, sigma: -1.0 },
            Distribution::Gamma {
                shape: f64::NAN,
                rate: 1.0,
            },
            Distribution::NegativeBinomial { r: 2.0, p: f64::NAN },
        ] {
            assert!(distribution.quantile(0.5).is_err());
            assert!(distribution.sample(&mut || 0.5).is_err());
            assert!(distribution.pdf(1.0).is_nan());
        }
        let distribution = MultivariateDistribution::MultivariateNormal {
            mu: vec![0.0, 0.0],
            cov: vec![1.0, 10.0, 0.0, 1.0],
        };
        assert_eq!(
            distribution.pdf(&[0.0, 0.0]),
            Err(NumericError::NotPositiveDefinite)
        );
    }
}
