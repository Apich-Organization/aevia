//! Descriptive statistics (§8.7).
//!
//! Operations with an exact form preserve rational arithmetic for samples
//! of `Int` and `Real` values. Float quantiles and exact quantiles both use
//! Hyndman–Fan type 7. Covariance and variance use the sample divisor unless
//! the caller requests population normalization. Skewness and excess kurtosis
//! use population moments.

use crate::NumericError;
#[cfg(test)]
use num_bigint::BigInt;
use num_rational::BigRational;
#[cfg(test)]
use num_traits::One;

/// The median of an exact sample.
///
/// # Errors
///
/// [`NumericError::Empty`] for an empty sample.
pub fn median_exact(values: &mut [BigRational]) -> Result<BigRational, NumericError> {
    rssn::kernels::descriptive::median_exact(values).map_err(Into::into)
}

/// The median of a float sample.
///
/// # Errors
///
/// [`NumericError::Empty`] for an empty sample and
/// [`NumericError::Domain`] for non-finite values.
pub fn median(values: &mut [f64]) -> Result<f64, NumericError> {
    rssn::kernels::descriptive::median(values).map_err(Into::into)
}

/// The `q`-quantile of a sorted exact sample, `0 ≤ q ≤ 1`.
///
/// # Errors
///
/// [`NumericError::Empty`] for an empty sample and
/// [`NumericError::Domain`] when `q` is outside `[0, 1]`.
pub fn quantile_exact(
    sorted: &[BigRational],
    q: &BigRational,
) -> Result<BigRational, NumericError> {
    rssn::kernels::descriptive::quantile_exact(sorted, q).map_err(Into::into)
}

/// The `q`-quantile of a float sample, `0 ≤ q ≤ 1`.
///
/// # Errors
///
/// [`NumericError::Empty`] for an empty sample and
/// [`NumericError::Domain`] for non-finite values or a `q` outside `[0, 1]`.
pub fn quantile(
    values: &[f64],
    q: f64,
) -> Result<f64, NumericError> {
    rssn::kernels::descriptive::quantile(values, q).map_err(Into::into)
}

/// Type-7 quantiles of a sorted exact sample in probability order.
///
/// # Errors
///
/// Returns [`NumericError::Empty`] for an empty sample or
/// [`NumericError::Domain`] for a probability outside `[0, 1]`.
pub fn quantiles_exact(
    sorted: &[BigRational],
    probabilities: &[BigRational],
) -> Result<Vec<BigRational>, NumericError> {
    if sorted.is_empty() {
        return Err(NumericError::Empty);
    }
    probabilities
        .iter()
        .map(|probability| quantile_exact(sorted, probability))
        .collect()
}

/// Type-7 quantiles of a finite sample in probability order.
///
/// Multiple probabilities share one ordered copy of the observations.
/// Repeated probabilities are retained. An empty probability vector yields
/// an empty result when the sample is valid.
///
/// # Errors
///
/// Returns [`NumericError::Empty`] for an empty sample or
/// [`NumericError::Domain`] for non-finite observations and for probabilities
/// outside `[0, 1]`.
pub fn quantiles(
    values: &[f64],
    probabilities: &[f64],
) -> Result<Vec<f64>, NumericError> {
    rssn::kernels::descriptive::quantiles(values, probabilities).map_err(Into::into)
}

/// The arithmetic mean of a float sample.
///
/// Non-finite inputs follow IEEE addition in sample order.
///
/// # Errors
///
/// Returns [`NumericError::Empty`] for an empty sample.
pub fn mean(values: &[f64]) -> Result<f64, NumericError> {
    rssn::kernels::descriptive::mean(values).map_err(Into::into)
}

/// The variance of a float sample.
///
/// Use `population` to divide by the sample length. Otherwise, divide by one
/// less than the length for samples with two or more observations. A singleton
/// has zero variance.
///
/// # Errors
///
/// Returns [`NumericError::Empty`] for an empty sample and
/// [`NumericError::Domain`] for non-finite values or a result outside the
/// finite range.
pub fn var(
    values: &[f64],
    population: bool,
) -> Result<f64, NumericError> {
    rssn::kernels::descriptive::var(values, population).map_err(Into::into)
}

/// The standard deviation of a float sample.
///
/// Uses the same divisor as [`var`]. The standard deviation can be finite
/// even when the variance exceeds the float range.
///
/// # Errors
///
/// Returns [`NumericError::Empty`] for an empty sample and
/// [`NumericError::Domain`] for non-finite values or a result outside the
/// finite range.
pub fn std_dev(
    values: &[f64],
    population: bool,
) -> Result<f64, NumericError> {
    rssn::kernels::descriptive::std_dev(values, population).map_err(Into::into)
}

/// The sample covariance of two equally long samples.
///
/// Use `population` to divide by the sample length. Otherwise, divide by one
/// less than the length for samples with two or more observations. The
/// covariance of a pair of singleton samples is zero.
///
/// # Errors
///
/// Returns [`NumericError::ShapeMismatch`] for different lengths,
/// [`NumericError::Empty`] for empty samples, and [`NumericError::Domain`]
/// for non-finite values or a result outside the finite range.
pub fn cov(
    left: &[f64],
    right: &[f64],
    population: bool,
) -> Result<f64, NumericError> {
    rssn::kernels::descriptive::cov(left, right, population).map_err(Into::into)
}

/// The Pearson correlation coefficient of two equally long samples.
///
/// Population and sample normalization give the same coefficient.
///
/// # Errors
///
/// Returns [`NumericError::ShapeMismatch`] for different lengths,
/// [`NumericError::Empty`] for empty samples, and [`NumericError::Domain`]
/// for non-finite values or a sample with no spread.
pub fn corr(
    left: &[f64],
    right: &[f64],
    population: bool,
) -> Result<f64, NumericError> {
    rssn::kernels::descriptive::corr(left, right, population).map_err(Into::into)
}

/// The skewness `m3 / m2^{3/2}`.
///
/// # Errors
///
/// [`NumericError::Empty`] for an empty sample, and [`NumericError::Domain`]
/// for a constant sample or non-finite values.
pub fn skewness(values: &[f64]) -> Result<f64, NumericError> {
    rssn::kernels::descriptive::skewness(values).map_err(Into::into)
}

/// The excess kurtosis `m4 / m2^2 - 3`.
///
/// # Errors
///
/// [`NumericError::Empty`] for an empty sample, and [`NumericError::Domain`]
/// for a constant sample or non-finite values.
pub fn kurtosis(values: &[f64]) -> Result<f64, NumericError> {
    rssn::kernels::descriptive::kurtosis(values).map_err(Into::into)
}

/// A uniform histogram with `bins` buckets.
///
/// Returns `(counts, edges)`. For a nonconstant sample, the `bins + 1`
/// edges span the sample range. An interior edge belongs to the bucket above
/// it, and the highest edge belongs to the last bucket. Edges can coincide
/// when a bucket is narrower than the representable spacing. Constant samples
/// use unit-width buckets starting at the value minus 0.5, with all observations
/// in the first bucket.
///
/// # Errors
///
/// [`NumericError::Empty`] for an empty sample and [`NumericError::Domain`]
/// for non-finite values, a zero bucket count, or buckets that cannot be allocated.
pub fn histogram(
    values: &[f64],
    bins: usize,
) -> Result<(Vec<i64>, Vec<f64>), NumericError> {
    rssn::kernels::descriptive::histogram(values, bins).map_err(Into::into)
}

/// The most frequent value of an exact sample.
///
/// Ties go to the smallest value, so the answer does not depend on the order of
/// the input.
///
/// # Errors
///
/// [`NumericError::Empty`] for an empty sample.
pub fn mode_exact(values: &[BigRational]) -> Result<BigRational, NumericError> {
    rssn::kernels::descriptive::mode_exact(values).map_err(Into::into)
}

/// The most frequent value of a float sample; ties go to the smallest.
///
/// # Errors
///
/// [`NumericError::Empty`] for an empty sample and
/// [`NumericError::Domain`] for non-finite values.
pub fn mode(values: &[f64]) -> Result<f64, NumericError> {
    rssn::kernels::descriptive::mode(values).map_err(Into::into)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rational(value: i64) -> BigRational {
        BigRational::from_integer(BigInt::from(value))
    }

    #[test]
    fn the_median_of_an_odd_sample_is_the_middle_element() {
        let mut sample = vec![rational(3), rational(1), rational(2)];
        assert_eq!(median_exact(&mut sample), Ok(rational(2)));
    }

    #[test]
    fn the_median_of_an_even_sample_is_the_mean_of_the_middle_two() {
        let mut sample = vec![rational(1), rational(2), rational(4), rational(8)];
        assert_eq!(
            median_exact(&mut sample),
            Ok(BigRational::new(BigInt::from(3), BigInt::from(1)))
        );
    }

    #[test]
    fn an_empty_sample_has_no_median() {
        assert_eq!(median(&mut []), Err(NumericError::Empty));
    }

    #[test]
    fn the_median_of_a_float_sample_interpolates() {
        assert!((median(&mut [1.0, 2.0]).unwrap() - 1.5).abs() < 1e-12);
        assert!((median(&mut [5.0, 1.0, 3.0]).unwrap() - 3.0).abs() < 1e-12);
    }

    #[test]
    fn median_of_extreme_finite_values_avoids_intermediate_overflow() {
        assert_eq!(median(&mut [f64::MAX, f64::MAX]), Ok(f64::MAX));
        assert_eq!(median(&mut [-f64::MAX, f64::MAX]), Ok(0.0));
        assert_eq!(median(&mut [-f64::MAX, -f64::MAX]), Ok(-f64::MAX));
    }

    #[test]
    fn quantiles_interpolate_linearly() {
        let sample: Vec<BigRational> = (0..=4).map(rational).collect();
        // Type 7: position = q (n - 1) = 1 for the lower quartile.
        assert_eq!(
            quantile_exact(&sample, &BigRational::new(1.into(), 4.into())),
            Ok(rational(1))
        );
        assert_eq!(
            quantile_exact(&sample, &BigRational::new(1.into(), 2.into())),
            Ok(rational(2))
        );
        assert_eq!(
            quantile_exact(&sample, &BigRational::one()),
            Ok(rational(4))
        );
        assert_eq!(quantile(&[0.0, 10.0], 0.25), Ok(2.5));
    }

    #[test]
    fn interpolation_and_histogram_span_the_full_finite_range() {
        let sample = [-f64::MAX, 0.0, f64::MAX];
        assert_eq!(quantile(&[sample[0], sample[2]], 0.0), Ok(sample[0]));
        assert_eq!(quantile(&[sample[0], sample[2]], 0.5), Ok(0.0));
        assert_eq!(quantile(&[sample[0], sample[2]], 1.0), Ok(sample[2]));
        assert_eq!(histogram(&sample, 2), Ok((vec![1, 2], sample.to_vec())));

        let tiny = f64::from_bits(1);
        let (counts, edges) = histogram(&[tiny, tiny * 2.0], 4).unwrap();
        assert_eq!(edges.first(), Some(&tiny));
        assert_eq!(edges.last(), Some(&(tiny * 2.0)));
        assert_eq!(edges, vec![tiny, tiny, tiny * 2.0, tiny * 2.0, tiny * 2.0]);
        assert_eq!(counts, vec![0, 1, 0, 1]);
        assert!(matches!(
            histogram(&sample, usize::MAX),
            Err(NumericError::Domain(_))
        ));
        assert!(matches!(
            histogram(&sample, usize::MAX - 1),
            Err(NumericError::Domain(_))
        ));
    }

    #[test]
    fn scaled_moments_are_finite_across_large_and_small_scales() {
        let tiny = [1e-300, 2e-300, 3e-300];
        let large = [1e300, 2e300, 3e300];
        for population in [false, true] {
            let expected = if population { 2.0 / 3.0 } else { 1.0 };
            assert!((cov(&tiny, &large, population).unwrap() - expected).abs() < 1e-12);
            assert!((corr(&tiny, &large, population).unwrap() - 1.0).abs() < 1e-12);
        }
        assert_eq!(cov(&[f64::MAX; 2], &large[..2], false), Ok(0.0));
        // Disjoint extremes leave a nonzero product of a tiny value and
        // a large value even if the normalized tiny value rounds to zero.
        let mixed = [f64::MAX, -f64::MAX, 1e-100, -1e-100];
        let partner = [0.0, 0.0, f64::MAX, -f64::MAX];
        assert_eq!(cov(&mixed, &partner, true), Ok((1e-100 * f64::MAX) / 2.0));
        for scale in [f64::from_bits(1), f64::MAX] {
            assert_eq!(skewness(&[-scale, scale]), Ok(0.0));
            assert_eq!(kurtosis(&[-scale, scale]), Ok(-2.0));
        }

        let sample = [-f64::MAX, f64::MAX];
        assert!(matches!(var(&sample, true), Err(NumericError::Domain(_))));
        assert_eq!(std_dev(&sample, true), Ok(f64::MAX));
        assert!(matches!(
            std_dev(&sample, false),
            Err(NumericError::Domain(_))
        ));
        assert_eq!(
            std_dev(&[0.0, f64::from_bits(2)], true),
            Ok(f64::from_bits(1))
        );
        assert_eq!(var(&[0.0, 2.0], true), Ok(1.0));
        assert_eq!(var(&[0.0, 2.0], false), Ok(2.0));
    }

    #[test]
    fn float_mean_recovers_overflow_and_preserves_ieee_inputs() {
        assert_eq!(mean(&[f64::MAX; 7]), Ok(f64::MAX));
        assert_eq!(mean(&[-f64::MAX; 9]), Ok(-f64::MAX));
        assert_eq!(mean(&[f64::MAX, f64::MAX, -f64::MAX, -f64::MAX]), Ok(0.0));
        assert_eq!(mean(&[1e16, 3.0, -1e16]), Ok(1.0));
        assert_eq!(mean(&[-0.0]).unwrap().to_bits(), (-0.0f64).to_bits());
        assert_eq!(mean(&[f64::INFINITY, 1.0]), Ok(f64::INFINITY));
        assert!(mean(&[f64::INFINITY, f64::NEG_INFINITY]).unwrap().is_nan());
        assert!(mean(&[1.0, f64::NAN]).unwrap().is_nan());
        assert_eq!(mean(&[]), Err(NumericError::Empty));
    }

    #[test]
    fn finite_only_statistics_reject_any_non_finite_observation() {
        for unsuitable in [f64::NAN, f64::NEG_INFINITY, f64::INFINITY] {
            for mut sample in [[unsuitable, 1.0], [1.0, unsuitable]] {
                assert!(matches!(
                    histogram(&sample, 2),
                    Err(NumericError::Domain(_))
                ));
                assert!(matches!(
                    quantile(&sample, 0.0),
                    Err(NumericError::Domain(_))
                ));
                assert!(matches!(mode(&sample), Err(NumericError::Domain(_))));
                assert!(matches!(var(&sample, true), Err(NumericError::Domain(_))));
                assert!(matches!(
                    std_dev(&sample, true),
                    Err(NumericError::Domain(_))
                ));
                assert!(matches!(
                    cov(&sample, &[1.0; 2], false),
                    Err(NumericError::Domain(_))
                ));
                assert!(matches!(
                    corr(&[1.0; 2], &sample, false),
                    Err(NumericError::Domain(_))
                ));
                assert!(matches!(skewness(&sample), Err(NumericError::Domain(_))));
                assert!(matches!(kurtosis(&sample), Err(NumericError::Domain(_))));
                assert!(matches!(median(&mut sample), Err(NumericError::Domain(_))));
            }
        }
    }

    #[test]
    fn covariance_of_a_perfect_line_is_its_slope_squared() {
        let x = [1.0, 2.0, 3.0, 4.0];
        let y = [2.0, 4.0, 6.0, 8.0];
        // Sample covariance of x with 2x is 2 * var(x) = 2 * 5/3.
        assert!((cov(&x, &y, false).unwrap() - 10.0 / 3.0).abs() < 1e-12);
        assert!((corr(&x, &y, false).unwrap() - 1.0).abs() < 1e-12);
    }

    #[test]
    fn correlation_of_an_antitone_pair_is_minus_one() {
        let x = [1.0, 2.0, 3.0];
        let y = [3.0, 2.0, 1.0];
        assert!((corr(&x, &y, false).unwrap() + 1.0).abs() < 1e-12);
    }

    #[test]
    fn a_constant_sample_has_no_correlation() {
        assert!(matches!(
            corr(&[1.0, 1.0], &[1.0, 2.0], false),
            Err(NumericError::Domain(_))
        ));
    }

    #[test]
    fn a_symmetric_sample_has_zero_skewness() {
        assert!(skewness(&[1.0, 2.0, 3.0]).unwrap().abs() < 1e-12);
    }

    #[test]
    fn a_normal_sample_has_zero_excess_kurtosis() {
        // The two-point sample {0, 2} has m2 = 1 and m4 = 1, so g2 = -2.
        assert!((kurtosis(&[0.0, 2.0]).unwrap() + 2.0).abs() < 1e-12);
    }

    #[test]
    fn the_histogram_counts_every_element() {
        let (counts, edges) = histogram(&[0.0, 1.0, 2.0, 3.0, 4.0], 2).unwrap();
        // Edges are [0, 2, 4]; the interior edge belongs to the upper bucket
        // and the last bucket is closed, so 0 and 1 fall low and 2, 3, 4 high.
        assert_eq!(counts, vec![2, 3]);
        assert_eq!(edges.len(), 3);
        assert!((edges[0] - 0.0).abs() < 1e-12);
        assert!((edges[2] - 4.0).abs() < 1e-12);
        assert_eq!(counts.iter().sum::<i64>(), 5);
    }

    #[test]
    fn the_mode_takes_the_smallest_value_on_a_tie() {
        assert_eq!(
            mode_exact(&[rational(2), rational(1), rational(2), rational(1)]),
            Ok(rational(1))
        );
        assert_eq!(mode(&[3.0, 3.0, 1.0]), Ok(3.0));
    }
}

// ----- exact forms ---------------------------------------------------------
//
// `mean`/`var`/`cov` are typed `div_rep(ρ)`, which is `Real` when the input is
// `Int`/`Real`, so an exact sample must give an exact answer: the mean of
// `[1, 2]` is the rational `3/2`, not `1.5` (§6.3.2).

/// The arithmetic mean of an exact sample.
///
/// # Errors
///
/// [`NumericError::Empty`] for an empty sample.
pub fn mean_exact(values: &[BigRational]) -> Result<BigRational, NumericError> {
    rssn::kernels::descriptive::mean_exact(values).map_err(Into::into)
}

/// The variance of an exact sample.
///
/// # Errors
///
/// [`NumericError::Empty`] for an empty sample.
pub fn var_exact(
    values: &[BigRational],
    population: bool,
) -> Result<BigRational, NumericError> {
    rssn::kernels::descriptive::var_exact(values, population).map_err(Into::into)
}

/// The covariance of two exact samples.
///
/// # Errors
///
/// [`NumericError::ShapeMismatch`] when the lengths differ, and
/// [`NumericError::Empty`] when there are no elements.
pub fn cov_exact(
    left: &[BigRational],
    right: &[BigRational],
    population: bool,
) -> Result<BigRational, NumericError> {
    rssn::kernels::descriptive::cov_exact(left, right, population).map_err(Into::into)
}

#[cfg(test)]
mod exact_stats_tests {
    use super::*;

    fn rational(value: i64) -> BigRational {
        BigRational::from_integer(BigInt::from(value))
    }

    #[test]
    fn exact_quantile_positions_near_an_integer_do_not_round_through_float() {
        let denominator = BigInt::one() << 100usize;
        let q = BigRational::new(&denominator - 1, &denominator * 2);
        let expected = BigRational::new(&denominator - 1, denominator);
        assert_eq!(
            quantile_exact(&[rational(0), rational(1), rational(100)], &q),
            Ok(expected)
        );
    }

    #[test]
    fn the_exact_mean_of_a_small_sample() {
        let sample = vec![rational(1), rational(2)];
        assert_eq!(
            mean_exact(&sample),
            Ok(BigRational::new(BigInt::from(3), BigInt::from(2)))
        );
        assert_eq!(mean_exact(&[]), Err(NumericError::Empty));
    }

    #[test]
    fn the_exact_variance_of_one_two_three() {
        // Sample variance of 1, 2, 3 is 1.
        let sample = vec![rational(1), rational(2), rational(3)];
        assert_eq!(var_exact(&sample, false), Ok(rational(1)));
        // The population variance is 2/3.
        assert_eq!(
            var_exact(&sample, true),
            Ok(BigRational::new(BigInt::from(2), BigInt::from(3)))
        );
    }

    #[test]
    fn the_exact_covariance_of_two_lines() {
        let x = vec![rational(1), rational(2), rational(3)];
        let y = vec![rational(2), rational(4), rational(6)];
        // The deviations are (-1, 0, 1) and (-2, 0, 2), so the sum of products
        // is 4: the population covariance is 4/3 and the sample one is 2.
        assert_eq!(
            cov_exact(&x, &y, true),
            Ok(BigRational::new(BigInt::from(4), BigInt::from(3)))
        );
        assert_eq!(cov_exact(&x, &y, false), Ok(rational(2)));
    }
}

// ----- hypothesis testing, regression, PCA, rank correlations, KDE, resampling -----

pub use rssn::kernels::stats_ext::BootstrapResult;
pub use rssn::kernels::stats_ext::JackknifeResult;
pub use rssn::kernels::stats_ext::LinearRegressionResult;
pub use rssn::kernels::stats_ext::LogisticRegressionResult;
pub use rssn::kernels::stats_ext::PcaResult;
pub use rssn::kernels::stats_ext::TestResult;

fn map_stats_err(err: rssn::kernels::stats_ext::StatsExtError) -> NumericError {
    match err {
        | rssn::kernels::stats_ext::StatsExtError::Empty => NumericError::Empty,
        | rssn::kernels::stats_ext::StatsExtError::ShapeMismatch(s) => {
            NumericError::ShapeMismatch(s)
        },
        | rssn::kernels::stats_ext::StatsExtError::Domain(s) => NumericError::Domain(s),
        | rssn::kernels::stats_ext::StatsExtError::Singular => NumericError::Singular,
        | rssn::kernels::stats_ext::StatsExtError::NotConverged => NumericError::NotConverged,
    }
}

/// Tests a sample mean against the hypothesized mean `mu0`.
///
/// # Errors
/// Rejects samples with fewer than two observations, non-finite data, or zero variance.
pub fn t_test_one_sample(
    sample: &[f64],
    mu0: f64,
) -> Result<TestResult, NumericError> {
    rssn::kernels::stats_ext::t_test_one_sample(sample, mu0).map_err(map_stats_err)
}

/// Compares two independent sample means with equal-variance or Welch's t-test.
///
/// # Errors
/// Rejects samples with fewer than two observations or non-finite data, or zero standard error.
pub fn t_test_two_sample(
    a: &[f64],
    b: &[f64],
    equal_var: bool,
) -> Result<TestResult, NumericError> {
    rssn::kernels::stats_ext::t_test_two_sample(a, b, equal_var).map_err(map_stats_err)
}

/// Applies a one-sample t-test to paired differences.
///
/// # Errors
/// Rejects mismatched lengths or differences with non-finite data or zero variance.
pub fn t_test_paired(
    a: &[f64],
    b: &[f64],
) -> Result<TestResult, NumericError> {
    rssn::kernels::stats_ext::t_test_paired(a, b).map_err(map_stats_err)
}

/// Compares observed counts with expected counts using a chi-squared statistic.
///
/// # Errors
/// Rejects mismatched lengths, invalid counts, or nonpositive expected counts.
pub fn chi_squared_goodness_of_fit(
    observed: &[f64],
    expected: &[f64],
) -> Result<TestResult, NumericError> {
    rssn::kernels::stats_ext::chi_squared_goodness_of_fit(observed, expected).map_err(map_stats_err)
}

/// Tests independence in a row-major contingency table.
///
/// # Errors
/// Rejects invalid table dimensions or counts and zero expected frequencies.
pub fn chi_squared_independence(
    rows: usize,
    cols: usize,
    table: &[f64],
) -> Result<TestResult, NumericError> {
    rssn::kernels::stats_ext::chi_squared_independence(rows, cols, table).map_err(map_stats_err)
}

/// Tests whether independent group means agree using one-way ANOVA.
///
/// # Errors
/// Rejects invalid groups or zero within-group variance.
pub fn anova_one_way(groups: &[&[f64]]) -> Result<TestResult, NumericError> {
    rssn::kernels::stats_ext::anova_one_way(groups).map_err(map_stats_err)
}

/// Compares an empirical distribution against the supplied cumulative distribution.
///
/// # Errors
/// Rejects empty or non-finite data and invalid cumulative probabilities.
pub fn ks_test_one_sample(
    sample: &[f64],
    cdf: &dyn Fn(f64) -> f64,
) -> Result<TestResult, NumericError> {
    rssn::kernels::stats_ext::ks_test_one_sample(sample, cdf).map_err(map_stats_err)
}

/// Compares two empirical distributions by their largest cumulative difference.
///
/// # Errors
/// Rejects empty or non-finite samples.
pub fn ks_test_two_sample(
    a: &[f64],
    b: &[f64],
) -> Result<TestResult, NumericError> {
    rssn::kernels::stats_ext::ks_test_two_sample(a, b).map_err(map_stats_err)
}

/// Tests for a rank shift between two independent samples.
///
/// # Errors
/// Rejects empty or non-finite samples, or an entirely tied combined sample.
pub fn mann_whitney_u(
    a: &[f64],
    b: &[f64],
) -> Result<TestResult, NumericError> {
    rssn::kernels::stats_ext::mann_whitney_u(a, b).map_err(map_stats_err)
}

/// Fits ordinary least squares to a row-major design matrix.
///
/// # Errors
/// Rejects invalid shapes or values and failed decomposition.
pub fn ols(
    rows: usize,
    cols: usize,
    x: &[f64],
    y: &[f64],
    fit_intercept: bool,
) -> Result<LinearRegressionResult, NumericError> {
    rssn::kernels::stats_ext::ols(rows, cols, x, y, fit_intercept).map_err(map_stats_err)
}

/// Fits L2-regularized least squares to a row-major design matrix.
///
/// # Errors
/// Rejects invalid shapes, values, or regularization strength and singular systems.
pub fn ridge(
    rows: usize,
    cols: usize,
    x: &[f64],
    y: &[f64],
    alpha: f64,
    fit_intercept: bool,
) -> Result<LinearRegressionResult, NumericError> {
    rssn::kernels::stats_ext::ridge(rows, cols, x, y, alpha, fit_intercept).map_err(map_stats_err)
}

/// Fits L1-regularized least squares by coordinate descent.
///
/// # Errors
/// Rejects invalid shapes, values, or solver parameters.
#[allow(clippy::too_many_arguments)] // Preserve matrix shape, data, and convergence controls.
pub fn lasso(
    rows: usize,
    cols: usize,
    x: &[f64],
    y: &[f64],
    alpha: f64,
    fit_intercept: bool,
    max_iter: usize,
    tol: f64,
) -> Result<LinearRegressionResult, NumericError> {
    rssn::kernels::stats_ext::lasso(rows, cols, x, y, alpha, fit_intercept, max_iter, tol)
        .map_err(map_stats_err)
}

/// Fits a binary logistic model to a row-major design matrix.
///
/// # Errors
/// Rejects invalid shapes, values, labels, or solver parameters and singular systems.
pub fn logistic_regression(
    rows: usize,
    cols: usize,
    x: &[f64],
    y: &[f64],
    fit_intercept: bool,
    max_iter: usize,
    tol: f64,
) -> Result<LogisticRegressionResult, NumericError> {
    rssn::kernels::stats_ext::logistic_regression(rows, cols, x, y, fit_intercept, max_iter, tol)
        .map_err(map_stats_err)
}

/// Computes principal components of a row-major observation matrix.
///
/// # Errors
/// Rejects invalid shapes, values, or component counts and decomposition failures.
pub fn pca(
    rows: usize,
    cols: usize,
    data: &[f64],
    n_components: Option<usize>,
) -> Result<PcaResult, NumericError> {
    rssn::kernels::stats_ext::pca(rows, cols, data, n_components).map_err(map_stats_err)
}

/// Computes Spearman rank correlation using average ranks for ties.
///
/// # Errors
/// Rejects incompatible, undersized, non-finite, or constant samples.
pub fn spearman(
    x: &[f64],
    y: &[f64],
) -> Result<f64, NumericError> {
    rssn::kernels::stats_ext::spearman(x, y).map_err(map_stats_err)
}

/// Computes Kendall's tau-b rank correlation, accounting for ties.
///
/// # Errors
/// Rejects incompatible, undersized, non-finite, or degenerate samples.
pub fn kendall(
    x: &[f64],
    y: &[f64],
) -> Result<f64, NumericError> {
    rssn::kernels::stats_ext::kendall(x, y).map_err(map_stats_err)
}

/// Estimates a Gaussian kernel density at the requested points.
///
/// # Errors
/// Rejects invalid samples, evaluation points, or bandwidth.
pub fn kde(
    samples: &[f64],
    eval_points: &[f64],
    bandwidth: Option<f64>,
) -> Result<Vec<f64>, NumericError> {
    rssn::kernels::stats_ext::kde(samples, eval_points, bandwidth).map_err(map_stats_err)
}

fn map_num_to_stats_err(err: NumericError) -> rssn::kernels::stats_ext::StatsExtError {
    match err {
        | NumericError::Empty => rssn::kernels::stats_ext::StatsExtError::Empty,
        | NumericError::ShapeMismatch(s) => {
            rssn::kernels::stats_ext::StatsExtError::ShapeMismatch(s)
        },
        | NumericError::Domain(s) => rssn::kernels::stats_ext::StatsExtError::Domain(s),
        | NumericError::Singular => rssn::kernels::stats_ext::StatsExtError::Singular,
        | NumericError::NotConverged => rssn::kernels::stats_ext::StatsExtError::NotConverged,
        | other => rssn::kernels::stats_ext::StatsExtError::Domain(format!("{other:?}")),
    }
}

/// Evaluates a statistic on resampled data with replacement.
///
/// # Errors
/// Rejects invalid sample or resampling parameters and propagates statistic errors.
pub fn bootstrap(
    samples: &[f64],
    n_resamples: usize,
    statistic: &dyn Fn(&[f64]) -> Result<f64, NumericError>,
    uniform: &mut dyn FnMut() -> f64,
) -> Result<BootstrapResult, NumericError> {
    let wrapped = |s: &[f64]| statistic(s).map_err(map_num_to_stats_err);
    rssn::kernels::stats_ext::bootstrap(samples, n_resamples, &wrapped, uniform)
        .map_err(map_stats_err)
}

/// Evaluates a statistic on leave-one-out samples.
///
/// # Errors
/// Rejects insufficient data and propagates statistic errors.
pub fn jackknife(
    samples: &[f64],
    statistic: &dyn Fn(&[f64]) -> Result<f64, NumericError>,
) -> Result<JackknifeResult, NumericError> {
    let wrapped = |s: &[f64]| statistic(s).map_err(map_num_to_stats_err);
    rssn::kernels::stats_ext::jackknife(samples, &wrapped).map_err(map_stats_err)
}

#[cfg(test)]
mod advanced_stats_tests {
    use super::*;
    use crate::dist::Distribution;

    fn lcg(seed: u64) -> impl FnMut() -> f64 {
        let mut state = seed;
        move || {
            state = state
                .wrapping_mul(6_364_136_223_846_793_005)
                .wrapping_add(1);
            ((state >> 11) as f64) / ((1u64 << 53) as f64)
        }
    }

    #[test]
    fn t_tests_match_analytical_values() {
        let s = [1.0, 2.0, 3.0, 4.0, 5.0];
        let res = t_test_one_sample(&s, 3.0).unwrap();
        assert!(res.statistic.abs() < 1e-12);
        assert!((res.p_value - 1.0).abs() < 1e-12);
        assert_eq!(res.df, Some(4.0));

        let a = [1.0, 2.0, 3.0, 4.0];
        let b = [10.0, 11.0, 12.0, 13.0];
        let two = t_test_two_sample(&a, &b, true).unwrap();
        assert!(two.statistic < -5.0);
        assert!(two.p_value < 1e-3);

        let paired = t_test_paired(&[1.0, 2.0, 3.0], &[2.0, 4.0, 6.0]).unwrap();
        assert!(paired.statistic < 0.0);
        assert_eq!(paired.df, Some(2.0));
    }

    #[test]
    fn chi_squared_and_anova_detect_differences() {
        let gof = chi_squared_goodness_of_fit(&[25.0, 25.0, 25.0, 25.0], &[25.0, 25.0, 25.0, 25.0])
            .unwrap();
        assert_eq!(gof.statistic, 0.0);
        assert!((gof.p_value - 1.0).abs() < 1e-12);

        let ind = chi_squared_independence(2, 2, &[10.0, 20.0, 20.0, 40.0]).unwrap();
        assert!(ind.statistic.abs() < 1e-12);
        assert!((ind.p_value - 1.0).abs() < 1e-12);

        let g1 = [1.0, 2.0, 3.0];
        let g2 = [10.0, 11.0, 12.0];
        let g3 = [20.0, 21.0, 22.0];
        let anova = anova_one_way(&[&g1, &g2, &g3]).unwrap();
        assert!(anova.statistic > 100.0);
        assert!(anova.p_value < 1e-4);
    }

    #[test]
    fn ks_and_mann_whitney_behave_properly() {
        let u_dist = Distribution::Uniform { lo: 0.0, hi: 1.0 };
        let sample = [0.1, 0.3, 0.5, 0.7, 0.9];
        let ks1 = ks_test_one_sample(&sample, &|x| u_dist.cdf(x)).unwrap();
        assert!((ks1.statistic - 0.1).abs() < 1e-12);
        assert!(ks1.p_value > 0.5);

        let ks2 = ks_test_two_sample(&[1.0, 2.0, 3.0], &[10.0, 11.0, 12.0]).unwrap();
        assert!((ks2.statistic - 1.0).abs() < 1e-12);

        let mw = mann_whitney_u(&[1.0, 2.0, 3.0], &[4.0, 5.0, 6.0]).unwrap();
        assert_eq!(mw.statistic, 0.0);
        assert!(mw.p_value < 0.1);
    }

    #[test]
    fn regression_methods_recover_known_models() {
        // y = 2*x1 - 3*x2 + 1
        let x = [1.0, 0.0, 0.0, 1.0, 1.0, 1.0, 2.0, 1.0, 1.0, 2.0, 2.0, 2.0];
        let y = [3.0, -2.0, 0.0, 2.0, -3.0, -1.0];
        let fit = ols(6, 2, &x, &y, true).unwrap();
        assert!((fit.coefficients[0] - 2.0).abs() < 1e-10);
        assert!((fit.coefficients[1] + 3.0).abs() < 1e-10);
        assert!((fit.intercept - 1.0).abs() < 1e-10);
        assert!((fit.r_squared - 1.0).abs() < 1e-10);

        let r_fit = ridge(6, 2, &x, &y, 0.0, true).unwrap();
        assert!((r_fit.coefficients[0] - 2.0).abs() < 1e-10);
        assert!((r_fit.coefficients[1] + 3.0).abs() < 1e-10);

        let l_fit = lasso(6, 2, &x, &y, 0.001, true, 5000, 1e-10).unwrap();
        assert!((l_fit.coefficients[0] - 2.0).abs() < 0.05);
        assert!((l_fit.coefficients[1] + 3.0).abs() < 0.05);

        // Non-separable 1D logistic regression (finite unique MLE)
        let lx = [-2.0, -1.0, -0.5, 0.5, 1.0, 2.0];
        let ly = [0.0, 0.0, 1.0, 0.0, 1.0, 1.0];
        let log_fit = logistic_regression(6, 1, &lx, &ly, true, 100, 1e-8).unwrap();
        assert!(log_fit.coefficients[0] > 0.5);
        assert!(log_fit.intercept.abs() < 1e-8);
    }

    #[test]
    fn pca_captures_dominant_axis() {
        // Points along y = 2x
        let data = [-2.0, -4.0, -1.0, -2.0, 0.0, 0.0, 1.0, 2.0, 2.0, 4.0];
        let res = pca(5, 2, &data, Some(1)).unwrap();
        assert_eq!(res.n_components, 1);
        assert!((res.explained_variance_ratio[0] - 1.0).abs() < 1e-12);
        assert_eq!(res.transformed.len(), 5);
    }

    #[test]
    fn rank_correlations_handle_monotone_and_ties() {
        let x = [1.0, 2.0, 2.0, 4.0];
        let y = [10.0, 20.0, 20.0, 40.0];
        assert!((spearman(&x, &y).unwrap() - 1.0).abs() < 1e-12);
        assert!((kendall(&x, &y).unwrap() - 1.0).abs() < 1e-12);

        let z = [4.0, 3.0, 2.0, 1.0];
        assert!((spearman(&[1.0, 2.0, 3.0, 4.0], &z).unwrap() + 1.0).abs() < 1e-12);
        assert!((kendall(&[1.0, 2.0, 3.0, 4.0], &z).unwrap() + 1.0).abs() < 1e-12);
    }

    #[test]
    fn kde_evaluates_positive_density() {
        let samples = [0.0, 0.5, 1.0, 1.5, 2.0];
        let dens = kde(&samples, &[0.0, 1.0, 10.0], None).unwrap();
        assert_eq!(dens.len(), 3);
        assert!(dens[1] > dens[0]);
        assert!(dens[0] > dens[2]);
    }

    #[test]
    fn bootstrap_and_jackknife_estimate_se_of_mean() {
        let data = [1.0, 2.0, 3.0, 4.0, 5.0];
        let mut rng = lcg(123);
        let boot = bootstrap(
            &data,
            500,
            &|s| Ok(s.iter().sum::<f64>() / s.len() as f64),
            &mut rng,
        )
        .unwrap();
        assert!((boot.estimate - 3.0).abs() < 1e-12);
        assert!(boot.ci_low < boot.estimate && boot.estimate < boot.ci_high);

        let jack = jackknife(&data, &|s| Ok(s.iter().sum::<f64>() / s.len() as f64)).unwrap();
        assert!((jack.estimate - 3.0).abs() < 1e-12);
        assert!(jack.bias.abs() < 1e-12);
        // True SE of mean for [1,2,3,4,5] is sqrt(2.5 / 5) = sqrt(0.5)
        assert!((jack.std_error - 0.5_f64.sqrt()).abs() < 1e-12);
    }
}

#[cfg(test)]
mod kernel_regressions {
    use super::*;

    #[test]
    fn nonfinite_samples_and_invalid_cdfs_are_rejected() {
        assert!(ks_test_two_sample(&[f64::NAN], &[1.0]).is_err());
        assert!(ks_test_one_sample(&[0.0], &|_| f64::NAN).is_err());
        assert!(spearman(&[1.0, f64::INFINITY], &[1.0, 2.0]).is_err());
        assert!(ols(2, 1, &[1.0, f64::NAN], &[1.0, 2.0], false).is_err());
        assert!(bootstrap(&[1.0, 2.0], 2, &|s| Ok(s[0]), &mut || 1.0).is_err());
    }

    #[test]
    fn shape_products_do_not_overflow() {
        assert!(pca(usize::MAX, 2, &[], None).is_err());
        assert!(ols(usize::MAX, 2, &[], &[], false).is_err());
        assert!(chi_squared_independence(usize::MAX, 2, &[]).is_err());
    }
}
