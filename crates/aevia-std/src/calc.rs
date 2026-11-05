//! Calculus and numerics that work on numbers, not on terms (§8.5, §8.6).
//!
//! The symbolic entry points (`diff`, `defint`, `limit`, `series`, `solve`) go
//! through the kernel and live in `aevia-core`; these are the numeric ones,
//! which take a closure over `f64` and never build a term.
//!
//! The two phases are kept apart on purpose. `numeric_deriv` is a finite
//! difference on `Float` data and says so; `grad_f`/`jacobian`/`hessian` are
//! their multi-dimensional forms; `quadrature` is adaptive Simpson with a
//! Richardson check. Everything iterative reports `NotConverged` rather than a
//! plausible-looking wrong answer.

use crate::NumericError;
use crate::linalg;

/// The relative step a finite difference uses.
///
/// `eps^(1/3)` balances truncation error (`h^2`) against cancellation
/// (`eps/h`), which is the optimum for a central difference.
fn step(x: f64) -> f64 {
    let scale = x.abs().max(1.0);
    f64::EPSILON.cbrt() * scale
}

/// The step a *second* difference uses.
///
/// A second difference subtracts three values that nearly cancel, so the
/// rounding noise is `eps/h^2` while the truncation error is `h^2`: the
/// balance is at `eps^(1/4)`, which is a hundred times larger than the first
/// difference's step. Using `eps^(1/3)` here loses the answer in the noise.
fn second_step(x: f64) -> f64 {
    let scale = x.abs().max(1.0);
    f64::EPSILON.sqrt().sqrt() * scale
}

/// The first derivative of `f` at `x`, by a central difference with one
/// Richardson extrapolation step.
pub fn derivative<F>(
    f: &mut F,
    x: f64,
) -> f64
where
    F: FnMut(f64) -> f64,
{
    let h = step(x);
    let coarse = (f(x + h) - f(x - h)) / (2.0 * h);
    let fine = (f(x + h / 2.0) - f(x - h / 2.0)) / h;
    // 4/3 * fine - 1/3 * coarse cancels the h^2 term.
    (4.0 * fine - coarse) / 3.0
}

/// The second derivative of `f` at `x`.
pub fn second_derivative<F>(
    f: &mut F,
    x: f64,
) -> f64
where
    F: FnMut(f64) -> f64,
{
    let h = second_step(x);
    let coarse = (f(x + h) - 2.0 * f(x) + f(x - h)) / (h * h);
    let fine = (f(x + h / 2.0) - 2.0 * f(x) + f(x - h / 2.0)) / (h * h / 4.0);
    (4.0 * fine - coarse) / 3.0
}

/// Adaptive Simpson quadrature of `f` over `[a, b]`.
///
/// # Errors
///
/// [`NumericError::NotConverged`] when the recursion limit is reached before
/// the tolerance is met, and [`NumericError::Domain`] when `b < a`.
pub fn quadrature<F>(
    f: &mut F,
    a: f64,
    b: f64,
) -> Result<f64, NumericError>
where
    F: FnMut(f64) -> f64,
{
    if !a.is_finite() || !b.is_finite() {
        return Err(NumericError::Domain(
            "quadrature needs finite bounds".to_string(),
        ));
    }
    if b < a {
        return Err(NumericError::Domain(
            "quadrature needs `a <= b`; swap the bounds and negate".to_string(),
        ));
    }
    if a == b {
        return Ok(0.0);
    }
    let mut fa = f(a);
    let mut fb = f(b);
    if !fa.is_finite() || !fb.is_finite() {
        // An endpoint singularity: step inside by a thousandth of the interval.
        let inset = (b - a) * 1e-6;
        fa = f(a + inset);
        fb = f(b - inset);
        if !fa.is_finite() || !fb.is_finite() {
            return Err(NumericError::Domain(
                "the integrand is not finite on the interval".to_string(),
            ));
        }
        return adaptive_simpson(f, a + inset, b - inset, fa, fb, 1e-12, 0);
    }
    adaptive_simpson(f, a, b, fa, fb, 1e-12, 0)
}

fn adaptive_simpson<F>(
    f: &mut F,
    a: f64,
    b: f64,
    fa: f64,
    fb: f64,
    tolerance: f64,
    depth: u32,
) -> Result<f64, NumericError>
where
    F: FnMut(f64) -> f64,
{
    let middle = (a + b) / 2.0;
    let fm = f(middle);
    let whole = (b - a) / 6.0 * (fa + 4.0 * fm + fb);
    let left = (middle - a) / 6.0 * (fa + 4.0 * f(a + (middle - a) / 2.0) + fm);
    let right = (b - middle) / 6.0 * (fm + 4.0 * f(middle + (b - middle) / 2.0) + fb);
    if depth >= 40 {
        return Err(NumericError::NotConverged);
    }
    if (left + right - whole).abs() <= 15.0 * tolerance {
        return Ok(left + right + (left + right - whole) / 15.0);
    }
    let left_value = adaptive_simpson(f, a, middle, fa, fm, tolerance / 2.0, depth + 1)?;
    let right_value = adaptive_simpson(f, middle, b, fm, fb, tolerance / 2.0, depth + 1)?;
    Ok(left_value + right_value)
}

/// The gradient of a scalar field at `x`, by central differences.
pub fn grad<F>(
    f: &mut F,
    x: &[f64],
) -> Vec<f64>
where
    F: FnMut(&[f64]) -> f64,
{
    let mut out = vec![0.0; x.len()];
    let mut point = x.to_vec();
    for index in 0..x.len() {
        let h = step(x[index]);
        let original = point[index];
        point[index] = original + h;
        let forward = f(&point);
        point[index] = original - h;
        let backward = f(&point);
        point[index] = original;
        out[index] = (forward - backward) / (2.0 * h);
    }
    out
}

/// The Jacobian of a vector field at `x`: `(rows, cols, row-major entries)`.
pub fn jacobian<F>(
    f: &mut F,
    x: &[f64],
) -> (usize, usize, Vec<f64>)
where
    F: FnMut(&[f64]) -> Vec<f64>,
{
    let base = f(x);
    let rows = base.len();
    let cols = x.len();
    let mut out = vec![0.0; rows * cols];
    let mut point = x.to_vec();
    for column in 0..cols {
        let h = step(x[column]);
        let original = point[column];
        point[column] = original + h;
        let forward = f(&point);
        point[column] = original - h;
        let backward = f(&point);
        point[column] = original;
        for row in 0..rows {
            let ahead = forward.get(row).copied().unwrap_or(0.0);
            let behind = backward.get(row).copied().unwrap_or(0.0);
            out[row * cols + column] = (ahead - behind) / (2.0 * h);
        }
    }
    (rows, cols, out)
}

/// The Hessian of a scalar field at `x`.
pub fn hessian<F>(
    f: &mut F,
    x: &[f64],
) -> Vec<f64>
where
    F: FnMut(&[f64]) -> f64,
{
    let n = x.len();
    let mut out = vec![0.0; n * n];
    let mut point = x.to_vec();
    // Diagonal: a second difference along one axis.
    for index in 0..n {
        let h = second_step(x[index]);
        let original = point[index];
        point[index] = original + h;
        let plus = f(&point);
        point[index] = original - h;
        let minus = f(&point);
        point[index] = original;
        let centre = f(&point);
        out[index * n + index] = (plus - 2.0 * centre + minus) / (h * h);
    }
    // Off-diagonal: a mixed difference, which is symmetric.
    for row in 0..n {
        for column in row + 1..n {
            let h = second_step(x[row]);
            let k = second_step(x[column]);
            let (xr, xc) = (point[row], point[column]);
            point[row] = xr + h;
            point[column] = xc + k;
            let plus_plus = f(&point);
            point[row] = xr + h;
            point[column] = xc - k;
            let plus_minus = f(&point);
            point[row] = xr - h;
            point[column] = xc + k;
            let minus_plus = f(&point);
            point[row] = xr - h;
            point[column] = xc - k;
            let minus_minus = f(&point);
            point[row] = xr;
            point[column] = xc;
            let value = (plus_plus - plus_minus - minus_plus + minus_minus) / (4.0 * h * k);
            out[row * n + column] = value;
            out[column * n + row] = value;
        }
    }
    out
}

/// The divergence of a vector field at `x`.
pub fn divergence<F>(
    f: &mut F,
    x: &[f64],
) -> f64
where
    F: FnMut(&[f64]) -> Vec<f64>,
{
    let (rows, _, jacobian) = jacobian(f, x);
    (0..rows.min(x.len()))
        .map(|index| jacobian[index * x.len() + index])
        .sum()
}

/// The curl of a three-dimensional vector field.
///
/// # Errors
///
/// [`NumericError::Domain`] when the field is not three-dimensional — the curl
/// of a two-dimensional field is a scalar that §8.5 does not declare, and
/// answering with one would silently change the type.
pub fn curl<F>(
    f: &mut F,
    x: &[f64],
) -> Result<Vec<f64>, NumericError>
where
    F: FnMut(&[f64]) -> Vec<f64>,
{
    if x.len() != 3 {
        return Err(NumericError::Domain(
            "`curl` is defined for a three-dimensional field".to_string(),
        ));
    }
    let (_, cols, jacobian) = jacobian(f, x);
    let at = |row: usize, column: usize| jacobian[row * cols + column];
    Ok(vec![
        at(2, 1) - at(1, 2),
        at(0, 2) - at(2, 0),
        at(1, 0) - at(0, 1),
    ])
}

/// The Laplacian of a scalar field at `x`: the trace of the Hessian.
pub fn laplacian<F>(
    f: &mut F,
    x: &[f64],
) -> f64
where
    F: FnMut(&[f64]) -> f64,
{
    let n = x.len();
    let hessian = hessian(f, x);
    (0..n).map(|index| hessian[index * n + index]).sum()
}

/// A root of `f` near `guess`, by Newton's method with a bisection fallback.
///
/// # Errors
///
/// [`NumericError::NotConverged`] when neither method finds a sign change or a
/// stationary point.
pub fn root<F>(
    f: &mut F,
    guess: f64,
) -> Result<f64, NumericError>
where
    F: FnMut(f64) -> f64,
{
    let mut x = guess;
    for _ in 0..100 {
        let value = f(x);
        if value == 0.0 {
            return Ok(x);
        }
        let slope = derivative(f, x);
        if slope.abs() < 1e-14 {
            break;
        }
        let next = x - value / slope;
        if !next.is_finite() {
            break;
        }
        if (next - x).abs() <= 1e-14 * x.abs().max(1.0) {
            return Ok(next);
        }
        x = next;
    }
    // Newton stalled: look for a sign change around the guess.
    let mut width = 1.0;
    for _ in 0..60 {
        let (low, high) = (guess - width, guess + width);
        let low_value = f(low);
        let high_value = f(high);
        if low_value.is_finite() && high_value.is_finite() && low_value * high_value < 0.0 {
            let mut low = low;
            let mut high = high;
            let mut low_value = low_value;
            for _ in 0..200 {
                let middle = (low + high) / 2.0;
                let value = f(middle);
                if value == 0.0 || (high - low) <= 1e-15 * middle.abs().max(1.0) {
                    return Ok(middle);
                }
                if low_value * value < 0.0 {
                    high = middle;
                } else {
                    low = middle;
                    low_value = value;
                }
            }
            return Ok((low + high) / 2.0);
        }
        width *= 2.0;
        if width > 1e9 {
            break;
        }
    }
    // No sign change: the root may be a touching one, where `f` never crosses
    // zero. Minimise `f^2` instead — `(x - 1)^2` is the standard example.
    for scale in [1.0, 10.0, 100.0, 1_000.0, 10_000.0] {
        let mut squared = |value: f64| {
            let evaluated = f(value);
            evaluated * evaluated
        };
        let candidate = golden_minimum(&mut squared, guess - scale, guess + scale);
        if f(candidate).abs() <= 1e-9 * (1.0 + candidate.abs()) {
            return Ok(candidate);
        }
    }
    Err(NumericError::NotConverged)
}

/// The minimum of a unimodal function on `[low, high]`, by golden-section
/// search.
fn golden_minimum<F>(
    f: &mut F,
    low: f64,
    high: f64,
) -> f64
where
    F: FnMut(f64) -> f64,
{
    const RATIO: f64 = 0.618_033_988_749_894_9;
    let mut low = low;
    let mut high = high;
    let mut left = high - RATIO * (high - low);
    let mut right = low + RATIO * (high - low);
    let mut left_value = f(left);
    let mut right_value = f(right);
    for _ in 0..200 {
        if (high - low).abs() <= 1e-15 * high.abs().max(1.0) {
            break;
        }
        if left_value < right_value {
            high = right;
            right = left;
            right_value = left_value;
            left = high - RATIO * (high - low);
            left_value = f(left);
        } else {
            low = left;
            left = right;
            left_value = right_value;
            right = low + RATIO * (high - low);
            right_value = f(right);
        }
    }
    (low + high) / 2.0
}

/// A solution of the square system `f(x) = 0`, by Newton's method.
///
/// # Errors
///
/// [`NumericError::NotConverged`] when the iteration does not settle, and
/// [`NumericError::Singular`] when the Jacobian vanishes.
pub fn solve_system<F>(
    f: &mut F,
    start: &[f64],
) -> Result<Vec<f64>, NumericError>
where
    F: FnMut(&[f64]) -> Vec<f64>,
{
    let n = start.len();
    let mut x = start.to_vec();
    for _ in 0..100 {
        let value = f(&x);
        if value.len() != n {
            return Err(NumericError::ShapeMismatch(format!(
                "the system returns {} value(s) for {n} unknown(s)",
                value.len()
            )));
        }
        if value.iter().all(|entry| entry.abs() < 1e-14) {
            return Ok(x);
        }
        let (rows, cols, jacobian) = jacobian(f, &x);
        if rows != n || cols != n {
            return Err(NumericError::ShapeMismatch(
                "the system is not square".to_string(),
            ));
        }
        let step = linalg::solve(n, n, &jacobian, &value)?;
        let mut delta = 0.0f64;
        for index in 0..n {
            x[index] -= step[index];
            delta = delta.max(step[index].abs());
        }
        if delta <= 1e-14 * x.iter().fold(1.0f64, |acc, entry| acc.max(entry.abs())) {
            return Ok(x);
        }
    }
    Err(NumericError::NotConverged)
}

/// A local minimum of `f`, by the Nelder–Mead simplex method.
///
/// Derivative-free, which is why it is the default: `f` is a closure and its
/// gradient is only available numerically.
///
/// # Errors
///
/// [`NumericError::Empty`] when the start point is empty, and
/// [`NumericError::NotConverged`] when the simplex does not shrink.
pub fn minimize<F>(
    f: &mut F,
    start: &[f64],
) -> Result<Vec<f64>, NumericError>
where
    F: FnMut(&[f64]) -> f64,
{
    let n = start.len();
    if n == 0 {
        return Err(NumericError::Empty);
    }
    let mut simplex: Vec<(f64, Vec<f64>)> = Vec::with_capacity(n + 1);
    simplex.push((f(start), start.to_vec()));
    for index in 0..n {
        let mut point = start.to_vec();
        point[index] += if point[index] == 0.0 {
            0.000_25
        } else {
            0.05 * point[index]
        };
        simplex.push((f(&point), point));
    }
    let mut iterations = 0;
    loop {
        simplex.sort_by(|a, b| a.0.total_cmp(&b.0));
        iterations += 1;
        if iterations > 4000 {
            return Err(NumericError::NotConverged);
        }
        let best = simplex[0].0;
        let spread = simplex
            .iter()
            .map(|(value, _)| (value - best).abs())
            .fold(0.0f64, f64::max);
        if spread <= 1e-12 * best.abs().max(1.0) {
            return Ok(simplex[0].1.clone());
        }
        // Centroid of everything but the worst vertex.
        let mut centroid = vec![0.0; n];
        for (_, point) in simplex.iter().take(n) {
            for index in 0..n {
                centroid[index] += point[index] / n as f64;
            }
        }
        let reflected: Vec<f64> = (0..n)
            .map(|index| centroid[index] + (centroid[index] - simplex[n].1[index]))
            .collect();
        let reflected_value = f(&reflected);
        if reflected_value < simplex[0].0 {
            let expanded: Vec<f64> = (0..n)
                .map(|index| centroid[index] + 2.0 * (reflected[index] - centroid[index]))
                .collect();
            let expanded_value = f(&expanded);
            if expanded_value < reflected_value {
                simplex[n] = (expanded_value, expanded);
            } else {
                simplex[n] = (reflected_value, reflected);
            }
            continue;
        }
        if reflected_value < simplex[n - 1].0 {
            simplex[n] = (reflected_value, reflected);
            continue;
        }
        let contracted: Vec<f64> = (0..n)
            .map(|index| centroid[index] + 0.5 * (simplex[n].1[index] - centroid[index]))
            .collect();
        let contracted_value = f(&contracted);
        if contracted_value < simplex[n].0 {
            simplex[n] = (contracted_value, contracted);
            continue;
        }
        // Shrink towards the best vertex.
        let best_point = simplex[0].1.clone();
        for entry in simplex.iter_mut().skip(1) {
            for index in 0..n {
                entry.1[index] = best_point[index] + 0.5 * (entry.1[index] - best_point[index]);
            }
            entry.0 = f(&entry.1);
        }
    }
}

/// A local maximum of `f`.
///
/// # Errors
///
/// As [`minimize`].
pub fn maximize<F>(
    f: &mut F,
    start: &[f64],
) -> Result<Vec<f64>, NumericError>
where
    F: FnMut(&[f64]) -> f64,
{
    let mut negated = |point: &[f64]| -f(point);
    minimize(&mut negated, start)
}

/// A least-squares solution of `f(x) ≈ 0`, by the Gauss–Newton method.
///
/// # Errors
///
/// [`NumericError::NotConverged`] when the iteration does not settle.
pub fn least_squares<F>(
    f: &mut F,
    start: &[f64],
) -> Result<Vec<f64>, NumericError>
where
    F: FnMut(&[f64]) -> Vec<f64>,
{
    let n = start.len();
    let mut x = start.to_vec();
    for _ in 0..200 {
        let residual = f(&x);
        let (rows, cols, jacobian) = jacobian(f, &x);
        if rows == 0 || cols != n {
            return Err(NumericError::ShapeMismatch(
                "the residual has the wrong shape".to_string(),
            ));
        }
        // Solve the normal equations through a least-squares solve of `J d = r`.
        let step = linalg::lstsq(rows, cols, &jacobian, &residual)?;
        let mut delta = 0.0f64;
        for index in 0..n {
            x[index] -= step[index];
            delta = delta.max(step[index].abs());
        }
        if delta <= 1e-14 * x.iter().fold(1.0f64, |acc, entry| acc.max(entry.abs())) {
            return Ok(x);
        }
    }
    Err(NumericError::NotConverged)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn close(
        a: f64,
        b: f64,
        tolerance: f64,
    ) -> bool {
        (a - b).abs() <= tolerance
    }

    #[test]
    fn the_derivative_of_a_polynomial() {
        let mut f = |x: f64| x * x * x;
        assert!(close(derivative(&mut f, 2.0), 12.0, 1e-6));
    }

    #[test]
    fn the_second_derivative_of_a_parabola_is_its_curvature() {
        let mut f = |x: f64| 3.0 * x * x;
        assert!(close(second_derivative(&mut f, 1.0), 6.0, 1e-5));
    }

    #[test]
    fn quadrature_integrates_a_sine_over_a_period() {
        let mut f = |x: f64| x.sin();
        assert!(close(
            quadrature(&mut f, 0.0, std::f64::consts::PI).unwrap(),
            2.0,
            1e-10
        ));
    }

    #[test]
    fn quadrature_integrates_a_polynomial_exactly() {
        let mut f = |x: f64| x * x;
        assert!(close(quadrature(&mut f, 0.0, 3.0).unwrap(), 9.0, 1e-9));
    }

    #[test]
    fn the_gradient_of_a_quadratic_form() {
        let mut f = |x: &[f64]| x[0] * x[0] + 2.0 * x[1] * x[1];
        let gradient = grad(&mut f, &[1.0, 2.0]);
        assert!(close(gradient[0], 2.0, 1e-6));
        assert!(close(gradient[1], 8.0, 1e-6));
    }

    #[test]
    fn the_jacobian_of_a_linear_map_is_the_map() {
        let mut f = |x: &[f64]| vec![2.0 * x[0] + 3.0 * x[1], -x[0] + 4.0 * x[1]];
        let (rows, cols, entries) = jacobian(&mut f, &[1.0, 1.0]);
        assert_eq!((rows, cols), (2, 2));
        assert!(close(entries[0], 2.0, 1e-6));
        assert!(close(entries[1], 3.0, 1e-6));
        assert!(close(entries[2], -1.0, 1e-6));
        assert!(close(entries[3], 4.0, 1e-6));
    }

    #[test]
    fn the_hessian_of_a_quadratic_form_is_its_matrix() {
        let mut f = |x: &[f64]| x[0] * x[0] + x[0] * x[1] + 2.0 * x[1] * x[1];
        let hessian = hessian(&mut f, &[0.5, -0.5]);
        assert!(close(hessian[0], 2.0, 1e-4));
        assert!(close(hessian[1], 1.0, 1e-4));
        assert!(close(hessian[2], 1.0, 1e-4));
        assert!(close(hessian[3], 4.0, 1e-4));
    }

    #[test]
    fn divergence_and_laplacian_of_a_radial_field() {
        // div(x, y) = 2 and lap(x^2 + y^2) = 4.
        let mut field = |x: &[f64]| vec![x[0], x[1]];
        assert!(close(divergence(&mut field, &[1.0, 2.0]), 2.0, 1e-6));
        let mut potential = |x: &[f64]| x[0] * x[0] + x[1] * x[1];
        assert!(close(laplacian(&mut potential, &[1.0, 2.0]), 4.0, 1e-4));
    }

    #[test]
    fn the_curl_of_a_rotation_is_constant() {
        let mut field = |x: &[f64]| vec![-x[1], x[0], 0.0];
        let curl = curl(&mut field, &[1.0, 1.0, 0.0]).unwrap();
        assert!(close(curl[2], 2.0, 1e-6));
        assert!(close(curl[0], 0.0, 1e-6));
    }

    #[test]
    fn the_curl_of_a_plane_field_is_refused() {
        let mut field = |x: &[f64]| vec![x[0], x[1]];
        assert!(matches!(
            curl(&mut field, &[1.0, 1.0]),
            Err(NumericError::Domain(_))
        ));
    }

    #[test]
    fn newton_finds_the_root_of_a_cubic() {
        let mut f = |x: f64| x * x * x - 2.0;
        let value = root(&mut f, 1.0).unwrap();
        assert!(close(value, 2.0f64.cbrt(), 1e-10));
    }

    #[test]
    fn newton_falls_back_to_bisection_at_a_stationary_point() {
        // f(x) = (x - 1)^2 has a root at 1 but a vanishing derivative there.
        let mut f = |x: f64| (x - 1.0) * (x - 1.0);
        let value = root(&mut f, 0.5).unwrap();
        assert!(close(value, 1.0, 1e-6));
    }

    #[test]
    fn a_non_linear_system_is_solved() {
        // x^2 + y^2 = 1, x = y.
        let mut f = |x: &[f64]| vec![x[0] * x[0] + x[1] * x[1] - 1.0, x[0] - x[1]];
        let solution = solve_system(&mut f, &[0.6, 0.7]).unwrap();
        let expected = 1.0 / 2.0f64.sqrt();
        assert!(close(solution[0], expected, 1e-9));
        assert!(close(solution[1], expected, 1e-9));
    }

    #[test]
    fn nelder_mead_finds_the_minimum_of_a_paraboloid() {
        let mut f = |x: &[f64]| (x[0] - 1.0) * (x[0] - 1.0) + (x[1] + 2.0) * (x[1] + 2.0);
        let point = minimize(&mut f, &[0.0, 0.0]).unwrap();
        assert!(close(point[0], 1.0, 1e-4));
        assert!(close(point[1], -2.0, 1e-4));
    }

    #[test]
    fn nelder_mead_finds_a_maximum() {
        let mut f = |x: &[f64]| -(x[0] * x[0]) - (x[1] - 3.0) * (x[1] - 3.0);
        let point = maximize(&mut f, &[1.0, 1.0]).unwrap();
        assert!(close(point[0], 0.0, 1e-4));
        assert!(close(point[1], 3.0, 1e-4));
    }

    #[test]
    fn gauss_newton_fits_an_overdetermined_system() {
        // Fit a line through (0,1), (1,3), (2,5) with residuals that vanish.
        let mut f = |p: &[f64]| {
            vec![
                p[0] + p[1] * 0.0 - 1.0,
                p[0] + p[1] * 1.0 - 3.0,
                p[0] + p[1] * 2.0 - 5.0,
            ]
        };
        let solution = least_squares(&mut f, &[0.0, 0.0]).unwrap();
        assert!(close(solution[0], 1.0, 1e-8));
        assert!(close(solution[1], 2.0, 1e-8));
    }
}
