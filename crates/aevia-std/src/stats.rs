//! Descriptive statistics (§8.7).
//!
//! Every function has an exact form and a float form. The exact form is used
//! when every element of the sample is an `Int`/`Real`, because that is the
//! representation the signature says it returns: `median([1, 2, 4])` is the
//! `Int` `2`, not `2.0`.
//!
//! Definitions, since §8.7 names the functions without pinning them down:
//!
//! * `quantile` is the linear-interpolation quantile (the one `numpy` calls
//!   `linear`, Hyndman–Fan type 7), which is the common choice in science;
//! * `cov`/`var` divide by `n - 1` (the sample estimator) unless the caller
//!   asks for the population form;
//! * `skewness`/`kurtosis` are the moment ratios `m3/m2^{3/2}` and
//!   `m4/m2^2 - 3` (excess kurtosis), with the population divisor.

use crate::NumericError;
use num_bigint::BigInt;
use num_rational::BigRational;
use num_traits::One;
use num_traits::Signed;
use num_traits::ToPrimitive;
use num_traits::Zero;

fn sort_exact(values: &mut [BigRational]) {
    values.sort();
}

/// The median of an exact sample.
///
/// # Errors
///
/// [`NumericError::Empty`] for an empty sample.
pub fn median_exact(values: &mut [BigRational]) -> Result<BigRational, NumericError> {
    if values.is_empty() {
        return Err(NumericError::Empty);
    }
    sort_exact(values);
    let len = values.len();
    if len % 2 == 1 {
        return Ok(values[len / 2].clone());
    }
    Ok((values[len / 2 - 1].clone() + values[len / 2].clone())
        / BigRational::from_integer(BigInt::from(2)))
}

/// The median of a float sample.
///
/// # Errors
///
/// [`NumericError::Empty`] for an empty sample.
pub fn median(values: &mut [f64]) -> Result<f64, NumericError> {
    if values.is_empty() {
        return Err(NumericError::Empty);
    }
    values.sort_by(f64::total_cmp);
    let len = values.len();
    if len % 2 == 1 {
        return Ok(values[len / 2]);
    }
    Ok((values[len / 2 - 1] + values[len / 2]) / 2.0)
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
    if sorted.is_empty() {
        return Err(NumericError::Empty);
    }
    if q.is_negative() || *q > BigRational::one() {
        return Err(NumericError::Domain(
            "a quantile needs `0 <= q <= 1`".to_string(),
        ));
    }
    let position = q.clone() * BigRational::from_integer(BigInt::from(sorted.len() - 1));
    let lower = position.to_f64().unwrap_or(0.0).floor().max(0.0) as usize;
    let lower = lower.min(sorted.len() - 1);
    if lower + 1 >= sorted.len() {
        return Ok(sorted[lower].clone());
    }
    let fraction = position - BigRational::from_integer(BigInt::from(lower));
    let below = sorted[lower].clone();
    let above = sorted[lower + 1].clone();
    Ok(below.clone() + fraction * (above - below))
}

/// The `q`-quantile of a float sample, `0 ≤ q ≤ 1`.
///
/// # Errors
///
/// [`NumericError::Empty`] for an empty sample and
/// [`NumericError::Domain`] when `q` is outside `[0, 1]`.
pub fn quantile(
    values: &[f64],
    q: f64,
) -> Result<f64, NumericError> {
    if values.is_empty() {
        return Err(NumericError::Empty);
    }
    if !(0.0..=1.0).contains(&q) {
        return Err(NumericError::Domain(
            "a quantile needs `0 <= q <= 1`".to_string(),
        ));
    }
    let mut sorted = values.to_vec();
    sorted.sort_by(f64::total_cmp);
    let position = q * (sorted.len() - 1) as f64;
    let lower = position.floor().max(0.0) as usize;
    if lower + 1 >= sorted.len() {
        return Ok(sorted[lower]);
    }
    let fraction = position - lower as f64;
    Ok(sorted[lower] + fraction * (sorted[lower + 1] - sorted[lower]))
}

/// The sample covariance of two equally long samples.
///
/// # Errors
///
/// [`NumericError::ShapeMismatch`] when the lengths differ, and
/// [`NumericError::Empty`] when there are no elements.
pub fn cov(
    left: &[f64],
    right: &[f64],
    population: bool,
) -> Result<f64, NumericError> {
    if left.len() != right.len() {
        return Err(NumericError::ShapeMismatch(format!(
            "cov needs two samples of one length, found {} and {}",
            left.len(),
            right.len()
        )));
    }
    let count = left.len();
    if count == 0 {
        return Err(NumericError::Empty);
    }
    if count == 1 {
        return Ok(0.0);
    }
    let mean_left = left.iter().sum::<f64>() / count as f64;
    let mean_right = right.iter().sum::<f64>() / count as f64;
    let total: f64 = left
        .iter()
        .zip(right)
        .map(|(a, b)| (a - mean_left) * (b - mean_right))
        .sum();
    let divisor = if population {
        count as f64
    } else {
        (count - 1) as f64
    };
    Ok(total / divisor)
}

/// The Pearson correlation coefficient of two equally long samples.
///
/// # Errors
///
/// [`NumericError::ShapeMismatch`] when the lengths differ, and
/// [`NumericError::Domain`] when one sample has no spread.
pub fn corr(
    left: &[f64],
    right: &[f64],
    population: bool,
) -> Result<f64, NumericError> {
    let covariance = cov(left, right, population)?;
    let variance_left = cov(left, left, population)?;
    let variance_right = cov(right, right, population)?;
    let denominator = (variance_left * variance_right).sqrt();
    if denominator == 0.0 || !denominator.is_finite() {
        return Err(NumericError::Domain(
            "a correlation needs two samples with a spread".to_string(),
        ));
    }
    Ok((covariance / denominator).clamp(-1.0, 1.0))
}

/// The moment ratios of a sample: `(mean, m2, m3, m4)`.
fn moments(values: &[f64]) -> Option<(f64, f64, f64, f64)> {
    if values.is_empty() {
        return None;
    }
    let count = values.len() as f64;
    let mean = values.iter().sum::<f64>() / count;
    let mut m2 = 0.0;
    let mut m3 = 0.0;
    let mut m4 = 0.0;
    for value in values {
        let deviation = value - mean;
        m2 += deviation * deviation;
        m3 += deviation * deviation * deviation;
        m4 += deviation * deviation * deviation * deviation;
    }
    Some((mean, m2 / count, m3 / count, m4 / count))
}

/// The skewness `m3 / m2^{3/2}`.
///
/// # Errors
///
/// [`NumericError::Empty`] for an empty sample, and [`NumericError::Domain`]
/// when every element is the same (the ratio is undefined).
pub fn skewness(values: &[f64]) -> Result<f64, NumericError> {
    let (_, m2, m3, _) = moments(values).ok_or(NumericError::Empty)?;
    if m2 == 0.0 {
        return Err(NumericError::Domain(
            "skewness of a constant sample is undefined".to_string(),
        ));
    }
    Ok(m3 / m2.powf(1.5))
}

/// The excess kurtosis `m4 / m2^2 - 3`.
///
/// # Errors
///
/// [`NumericError::Empty`] for an empty sample, and [`NumericError::Domain`]
/// when every element is the same.
pub fn kurtosis(values: &[f64]) -> Result<f64, NumericError> {
    let (_, m2, _, m4) = moments(values).ok_or(NumericError::Empty)?;
    if m2 == 0.0 {
        return Err(NumericError::Domain(
            "kurtosis of a constant sample is undefined".to_string(),
        ));
    }
    Ok(m4 / (m2 * m2) - 3.0)
}

/// A uniform histogram with `bins` buckets.
///
/// Returns `(counts, edges)`; `edges` has `bins + 1` entries spanning the
/// sample's range, and a value on an interior edge belongs to the bucket above
/// it, so the last bucket is closed on both ends.
///
/// # Errors
///
/// [`NumericError::Empty`] for an empty sample and [`NumericError::Domain`]
/// when `bins` is zero.
pub fn histogram(
    values: &[f64],
    bins: usize,
) -> Result<(Vec<i64>, Vec<f64>), NumericError> {
    if values.is_empty() {
        return Err(NumericError::Empty);
    }
    if bins == 0 {
        return Err(NumericError::Domain(
            "a histogram needs a bucket".to_string(),
        ));
    }
    let mut low = f64::INFINITY;
    let mut high = f64::NEG_INFINITY;
    for value in values {
        low = low.min(*value);
        high = high.max(*value);
    }
    if !low.is_finite() || !high.is_finite() {
        return Err(NumericError::Domain(
            "a histogram needs finite values".to_string(),
        ));
    }
    let width = if high > low {
        (high - low) / bins as f64
    } else {
        1.0
    };
    let start = if high > low { low } else { low - 0.5 };
    let edges: Vec<f64> = (0..=bins)
        .map(|index| start + width * index as f64)
        .collect();
    let mut counts = vec![0i64; bins];
    for value in values {
        let mut index = ((value - start) / width).floor() as isize;
        if index < 0 {
            index = 0;
        }
        if index >= bins as isize {
            index = bins as isize - 1;
        }
        counts[index as usize] += 1;
    }
    Ok((counts, edges))
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
    if values.is_empty() {
        return Err(NumericError::Empty);
    }
    let mut sorted = values.to_vec();
    sort_exact(&mut sorted);
    let mut best = sorted[0].clone();
    let mut best_count = 1usize;
    let mut current = sorted[0].clone();
    let mut current_count = 1usize;
    for value in &sorted[1..] {
        if *value == current {
            current_count += 1;
        } else {
            current = value.clone();
            current_count = 1;
        }
        if current_count > best_count {
            best_count = current_count;
            best = current.clone();
        }
    }
    Ok(best)
}

/// The most frequent value of a float sample; ties go to the smallest.
///
/// # Errors
///
/// [`NumericError::Empty`] for an empty sample.
pub fn mode(values: &[f64]) -> Result<f64, NumericError> {
    if values.is_empty() {
        return Err(NumericError::Empty);
    }
    let mut sorted = values.to_vec();
    sorted.sort_by(f64::total_cmp);
    let mut best = sorted[0];
    let mut best_count = 1usize;
    let mut current = sorted[0];
    let mut current_count = 1usize;
    for value in &sorted[1..] {
        if *value == current {
            current_count += 1;
        } else {
            current = *value;
            current_count = 1;
        }
        if current_count > best_count {
            best_count = current_count;
            best = current;
        }
    }
    Ok(best)
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
    if values.is_empty() {
        return Err(NumericError::Empty);
    }
    let total = values
        .iter()
        .fold(BigRational::zero(), |acc, value| acc + value.clone());
    Ok(total / BigRational::from_integer(BigInt::from(values.len())))
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
    if values.is_empty() {
        return Err(NumericError::Empty);
    }
    let mean = mean_exact(values)?;
    let total = values.iter().fold(BigRational::zero(), |acc, value| {
        let deviation = value.clone() - mean.clone();
        acc + deviation.clone() * deviation
    });
    let count = if population || values.len() == 1 {
        values.len()
    } else {
        values.len() - 1
    };
    Ok(total / BigRational::from_integer(BigInt::from(count)))
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
    if left.len() != right.len() {
        return Err(NumericError::ShapeMismatch(format!(
            "cov needs two samples of one length, found {} and {}",
            left.len(),
            right.len()
        )));
    }
    if left.is_empty() {
        return Err(NumericError::Empty);
    }
    let mean_left = mean_exact(left)?;
    let mean_right = mean_exact(right)?;
    let total = left
        .iter()
        .zip(right)
        .fold(BigRational::zero(), |acc, (a, b)| {
            acc + (a.clone() - mean_left.clone()) * (b.clone() - mean_right.clone())
        });
    let count = if population || left.len() == 1 {
        left.len()
    } else {
        left.len() - 1
    };
    Ok(total / BigRational::from_integer(BigInt::from(count)))
}

#[cfg(test)]
mod exact_stats_tests {
    use super::*;

    fn rational(value: i64) -> BigRational {
        BigRational::from_integer(BigInt::from(value))
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

pub fn t_test_one_sample(
    sample: &[f64],
    mu0: f64,
) -> Result<TestResult, NumericError> {
    rssn::kernels::stats_ext::t_test_one_sample(sample, mu0).map_err(map_stats_err)
}

pub fn t_test_two_sample(
    a: &[f64],
    b: &[f64],
    equal_var: bool,
) -> Result<TestResult, NumericError> {
    rssn::kernels::stats_ext::t_test_two_sample(a, b, equal_var).map_err(map_stats_err)
}

pub fn t_test_paired(
    a: &[f64],
    b: &[f64],
) -> Result<TestResult, NumericError> {
    rssn::kernels::stats_ext::t_test_paired(a, b).map_err(map_stats_err)
}

pub fn chi_squared_goodness_of_fit(
    observed: &[f64],
    expected: &[f64],
) -> Result<TestResult, NumericError> {
    rssn::kernels::stats_ext::chi_squared_goodness_of_fit(observed, expected).map_err(map_stats_err)
}

pub fn chi_squared_independence(
    rows: usize,
    cols: usize,
    table: &[f64],
) -> Result<TestResult, NumericError> {
    rssn::kernels::stats_ext::chi_squared_independence(rows, cols, table).map_err(map_stats_err)
}

pub fn anova_one_way(groups: &[&[f64]]) -> Result<TestResult, NumericError> {
    rssn::kernels::stats_ext::anova_one_way(groups).map_err(map_stats_err)
}

pub fn ks_test_one_sample(
    sample: &[f64],
    cdf: &dyn Fn(f64) -> f64,
) -> Result<TestResult, NumericError> {
    rssn::kernels::stats_ext::ks_test_one_sample(sample, cdf).map_err(map_stats_err)
}

pub fn ks_test_two_sample(
    a: &[f64],
    b: &[f64],
) -> Result<TestResult, NumericError> {
    rssn::kernels::stats_ext::ks_test_two_sample(a, b).map_err(map_stats_err)
}

pub fn mann_whitney_u(
    a: &[f64],
    b: &[f64],
) -> Result<TestResult, NumericError> {
    rssn::kernels::stats_ext::mann_whitney_u(a, b).map_err(map_stats_err)
}

pub fn ols(
    rows: usize,
    cols: usize,
    x: &[f64],
    y: &[f64],
    fit_intercept: bool,
) -> Result<LinearRegressionResult, NumericError> {
    rssn::kernels::stats_ext::ols(rows, cols, x, y, fit_intercept).map_err(map_stats_err)
}

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

pub fn pca(
    rows: usize,
    cols: usize,
    data: &[f64],
    n_components: Option<usize>,
) -> Result<PcaResult, NumericError> {
    rssn::kernels::stats_ext::pca(rows, cols, data, n_components).map_err(map_stats_err)
}

pub fn spearman(
    x: &[f64],
    y: &[f64],
) -> Result<f64, NumericError> {
    rssn::kernels::stats_ext::spearman(x, y).map_err(map_stats_err)
}

pub fn kendall(
    x: &[f64],
    y: &[f64],
) -> Result<f64, NumericError> {
    rssn::kernels::stats_ext::kendall(x, y).map_err(map_stats_err)
}

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
