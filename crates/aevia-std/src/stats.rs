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
use num_traits::{One, Signed, ToPrimitive, Zero};

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
    let lower = position
        .to_f64()
        .unwrap_or(0.0)
        .floor()
        .max(0.0) as usize;
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
pub fn quantile(values: &[f64], q: f64) -> Result<f64, NumericError> {
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
pub fn cov(left: &[f64], right: &[f64], population: bool) -> Result<f64, NumericError> {
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
pub fn corr(left: &[f64], right: &[f64], population: bool) -> Result<f64, NumericError> {
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
pub fn histogram(values: &[f64], bins: usize) -> Result<(Vec<i64>, Vec<f64>), NumericError> {
    if values.is_empty() {
        return Err(NumericError::Empty);
    }
    if bins == 0 {
        return Err(NumericError::Domain("a histogram needs a bucket".to_string()));
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
    let edges: Vec<f64> = (0..=bins).map(|index| start + width * index as f64).collect();
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
        assert_eq!(
            quantile(&[0.0, 10.0], 0.25),
            Ok(2.5)
        );
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
    let total = left.iter().zip(right).fold(BigRational::zero(), |acc, (a, b)| {
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
