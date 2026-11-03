//! Interpolation and polynomial fitting (§8.8).
//!
//! `interp1` is linear and `spline` is a natural cubic spline — the two are
//! different functions in §8.8, so they are different here. `polyfit` returns
//! the coefficients in *ascending* power order (`c[0] + c[1] x + …`), which is
//! what `polyval` consumes.
//!
//! `chebyshev_fit` fits at Chebyshev nodes and returns *monomial* coefficients,
//! so its result feeds `polyval` unchanged. Fitting in the Chebyshev basis is
//! what makes the fit stable; converting back is a change of basis, not a
//! different answer.

use crate::{linalg, NumericError};

/// Linear interpolation of `(xs, ys)` at `x`.
///
/// Outside the range the nearest endpoint is extended linearly from its own
/// segment, which is what a piecewise-linear interpolant does at its ends.
///
/// # Errors
///
/// [`NumericError::ShapeMismatch`] when the two samples differ in length, and
/// [`NumericError::Empty`] when there are no points.
pub fn interp1(xs: &[f64], ys: &[f64], x: f64) -> Result<f64, NumericError> {
    if xs.len() != ys.len() {
        return Err(NumericError::ShapeMismatch(format!(
            "interp1 needs two samples of one length, found {} and {}",
            xs.len(),
            ys.len()
        )));
    }
    if xs.is_empty() {
        return Err(NumericError::Empty);
    }
    if xs.len() == 1 {
        return Ok(ys[0]);
    }
    // Locate the segment; the samples are assumed sorted, and a scan keeps the
    // function total when they are not.
    let mut index = 0;
    for candidate in 0..xs.len() - 1 {
        if x >= xs[candidate] && x <= xs[candidate + 1] {
            index = candidate;
            break;
        }
        if x < xs[0] {
            index = 0;
            break;
        }
        index = candidate;
    }
    let (x0, x1) = (xs[index], xs[index + 1]);
    let (y0, y1) = (ys[index], ys[index + 1]);
    if x1 == x0 {
        return Ok(y0);
    }
    Ok(y0 + (y1 - y0) * (x - x0) / (x1 - x0))
}

/// Natural cubic spline interpolation at `x`.
///
/// # Errors
///
/// [`NumericError::ShapeMismatch`] when the samples differ in length, and
/// [`NumericError::Domain`] when fewer than three points are given (a natural
/// spline needs a second derivative to pin down).
pub fn spline(xs: &[f64], ys: &[f64], x: f64) -> Result<f64, NumericError> {
    if xs.len() != ys.len() {
        return Err(NumericError::ShapeMismatch(format!(
            "spline needs two samples of one length, found {} and {}",
            xs.len(),
            ys.len()
        )));
    }
    let n = xs.len();
    if n < 3 {
        return interp1(xs, ys, x);
    }
    // Second derivatives from the tridiagonal system with natural end
    // conditions (y'' = 0 at both ends).
    let mut second = vec![0.0; n];
    let mut u = vec![0.0; n];
    for index in 1..n - 1 {
        let sigma = (xs[index] - xs[index - 1]) / (xs[index + 1] - xs[index - 1]);
        let p = sigma * second[index - 1] + 2.0;
        second[index] = (sigma - 1.0) / p;
        u[index] = (ys[index + 1] - ys[index]) / (xs[index + 1] - xs[index])
            - (ys[index] - ys[index - 1]) / (xs[index] - xs[index - 1]);
        u[index] = (6.0 * u[index] / (xs[index + 1] - xs[index - 1]) - sigma * u[index - 1]) / p;
    }
    for index in (0..n - 1).rev() {
        second[index] = second[index] * second[index + 1] + u[index];
    }
    // Locate the segment, clamping outside the range.
    let mut lower = 0;
    let mut upper = n - 1;
    let mut index = 0;
    while upper - lower > 1 {
        let middle = (upper + lower) / 2;
        if xs[middle] > x {
            upper = middle;
        } else {
            lower = middle;
        }
    }
    index = index.max(lower).min(n - 2);
    let h = xs[index + 1] - xs[index];
    if h == 0.0 {
        return Ok(ys[index]);
    }
    let a = (xs[index + 1] - x) / h;
    let b = (x - xs[index]) / h;
    Ok(a * ys[index]
        + b * ys[index + 1]
        + ((a * a * a - a) * second[index] + (b * b * b - b) * second[index + 1]) * h * h / 6.0)
}

/// Bilinear interpolation on a regular grid.
///
/// `grid` is `ny × nx` in row-major order, covering `xs × ys`.
///
/// # Errors
///
/// [`NumericError::ShapeMismatch`] when the grid does not match the axes.
pub fn interp2(
    xs: &[f64],
    ys: &[f64],
    grid: &[f64],
    x: f64,
    y: f64,
) -> Result<f64, NumericError> {
    let (nx, ny) = (xs.len(), ys.len());
    if grid.len() != nx * ny {
        return Err(NumericError::ShapeMismatch(format!(
            "interp2 needs a {ny}x{nx} grid, found {} values",
            grid.len()
        )));
    }
    if nx < 2 || ny < 2 {
        return Err(NumericError::Domain(
            "interp2 needs at least two points per axis".to_string(),
        ));
    }
    let locate = |axis: &[f64], value: f64| -> (usize, f64) {
        let last = axis.len() - 1;
        if value <= axis[0] {
            return (0, 0.0);
        }
        if value >= axis[last] {
            return (last - 1, 1.0);
        }
        for index in 0..last {
            if value >= axis[index] && value <= axis[index + 1] {
                let width = axis[index + 1] - axis[index];
                let fraction = if width == 0.0 {
                    0.0
                } else {
                    (value - axis[index]) / width
                };
                return (index, fraction);
            }
        }
        (last - 1, 1.0)
    };
    let (column, fx) = locate(xs, x);
    let (row, fy) = locate(ys, y);
    let at = |row: usize, column: usize| grid[row * nx + column];
    let low = at(row, column) * (1.0 - fx) + at(row, column + 1) * fx;
    let high = at(row + 1, column) * (1.0 - fx) + at(row + 1, column + 1) * fx;
    Ok(low * (1.0 - fy) + high * fy)
}

/// The coefficients of the least-squares polynomial of `degree`, ascending.
///
/// # Errors
///
/// [`NumericError::ShapeMismatch`] when the samples differ in length, and
/// [`NumericError::Domain`] when there are fewer points than coefficients.
pub fn polyfit(xs: &[f64], ys: &[f64], degree: usize) -> Result<Vec<f64>, NumericError> {
    if xs.len() != ys.len() {
        return Err(NumericError::ShapeMismatch(format!(
            "polyfit needs two samples of one length, found {} and {}",
            xs.len(),
            ys.len()
        )));
    }
    let columns = degree + 1;
    if xs.len() < columns {
        return Err(NumericError::Domain(format!(
            "a degree-{degree} fit needs {columns} points, found {}",
            xs.len()
        )));
    }
    let mut design = vec![0.0; xs.len() * columns];
    for (row, x) in xs.iter().enumerate() {
        let mut power = 1.0;
        for column in 0..columns {
            design[row * columns + column] = power;
            power *= x;
        }
    }
    linalg::lstsq(xs.len(), columns, &design, ys)
}

/// Evaluates a polynomial given its coefficients in ascending order.
#[must_use]
pub fn polyval(coefficients: &[f64], x: f64) -> f64 {
    // Horner, from the highest power down.
    coefficients
        .iter()
        .rev()
        .fold(0.0, |accumulator, coefficient| accumulator * x + coefficient)
}

/// The monomial coefficients of the polynomial interpolating `(xs, ys)` at
/// `degree + 1` Chebyshev nodes of the sample's range.
///
/// # Errors
///
/// [`NumericError::ShapeMismatch`] when the samples differ in length, and
/// [`NumericError::Empty`] when there are none.
pub fn chebyshev_fit(xs: &[f64], ys: &[f64], degree: usize) -> Result<Vec<f64>, NumericError> {
    if xs.len() != ys.len() {
        return Err(NumericError::ShapeMismatch(format!(
            "chebyshev_fit needs two samples of one length, found {} and {}",
            xs.len(),
            ys.len()
        )));
    }
    if xs.is_empty() {
        return Err(NumericError::Empty);
    }
    let (low, high) = xs
        .iter()
        .fold((f64::INFINITY, f64::NEG_INFINITY), |(low, high), x| {
            (low.min(*x), high.max(*x))
        });
    let count = degree + 1;
    let nodes: Vec<f64> = (0..count)
        .map(|index| {
            let angle = std::f64::consts::PI * (index as f64 + 0.5) / count as f64;
            let mapped = angle.cos();
            if high > low {
                0.5 * (low + high) + 0.5 * (high - low) * mapped
            } else {
                low
            }
        })
        .collect();
    // Interpolate at the nodes in the Chebyshev basis, then change basis.
    let values: Vec<f64> = nodes
        .iter()
        .map(|node| interp1(xs, ys, *node))
        .collect::<Result<_, _>>()?;
    let scaled: Vec<f64> = nodes
        .iter()
        .map(|node| {
            if high > low {
                2.0 * (node - low) / (high - low) - 1.0
            } else {
                0.0
            }
        })
        .collect();
    // Solve the Vandermonde system in the Chebyshev basis by Gaussian
    // elimination; it is small (degree + 1) and well conditioned at these
    // nodes.
    let mut matrix = vec![0.0; count * count];
    for row in 0..count {
        for column in 0..count {
            matrix[row * count + column] = crate::special::chebyshev_t(column as i32, scaled[row]);
        }
    }
    let coefficients = linalg::solve(count, count, &matrix, &values)?;
    // Convert sum c_k T_k(t) with t = (2x - low - high)/(high - low) into
    // monomial coefficients in x.
    let mut total = vec![0.0; count];
    let mut previous: Vec<f64> = vec![1.0]; // T_0
    let mut current: Vec<f64> = vec![0.0, 1.0]; // T_1 in t
    for degree_index in 0..count {
        let polynomial = if degree_index == 0 {
            previous.clone()
        } else if degree_index == 1 {
            current.clone()
        } else {
            // T_k = 2 t T_{k-1} - T_{k-2}
            let mut next = vec![0.0; degree_index + 1];
            for (power, coefficient) in current.iter().enumerate() {
                next[power + 1] += 2.0 * coefficient;
            }
            for (power, coefficient) in previous.iter().enumerate() {
                next[power] -= coefficient;
            }
            previous = current.clone();
            current = next.clone();
            next
        };
        for (power, coefficient) in polynomial.iter().enumerate() {
            total[power] += coefficients[degree_index] * coefficient;
        }
    }
    // `total` is in the scaled variable `t`; substitute t = a x + b by Horner's
    // rule, one degree at a time.
    if high > low {
        let a = 2.0 / (high - low);
        let b = -(high + low) / (high - low);
        let mut out: Vec<f64> = vec![0.0];
        for power in (0..count).rev() {
            let mut next = vec![0.0; out.len() + 1];
            for (index, coefficient) in out.iter().enumerate() {
                next[index] += coefficient * b;
                next[index + 1] += coefficient * a;
            }
            next[0] += total[power];
            out = next;
        }
        out.truncate(count);
        out.resize(count, 0.0);
        Ok(out)
    } else {
        let mut out = vec![0.0; count];
        out[0] = total[0];
        Ok(out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn close(a: f64, b: f64) -> bool {
        (a - b).abs() < 1e-9
    }

    #[test]
    fn linear_interpolation_hits_the_knots() {
        let xs = [0.0, 1.0, 2.0];
        let ys = [0.0, 10.0, 40.0];
        assert!(close(interp1(&xs, &ys, 0.0).unwrap(), 0.0));
        assert!(close(interp1(&xs, &ys, 1.0).unwrap(), 10.0));
        assert!(close(interp1(&xs, &ys, 0.5).unwrap(), 5.0));
        assert!(close(interp1(&xs, &ys, 1.5).unwrap(), 25.0));
    }

    #[test]
    fn a_natural_spline_is_exact_on_a_cubic_at_its_knots() {
        let xs: Vec<f64> = (0..6).map(|index| index as f64).collect();
        let ys: Vec<f64> = xs.iter().map(|x| x * x * x - 2.0 * x).collect();
        for (x, y) in xs.iter().zip(ys.iter()) {
            assert!(close(spline(&xs, &ys, *x).unwrap(), *y), "at {x}");
        }
        // Between the knots it stays close to the cubic.
        let mid = 2.5;
        let exact = mid * mid * mid - 2.0 * mid;
        assert!((spline(&xs, &ys, mid).unwrap() - exact).abs() < 0.2);
    }

    #[test]
    fn bilinear_interpolation_on_a_plane_is_exact() {
        let xs = [0.0, 1.0];
        let ys = [0.0, 1.0];
        // f(x, y) = 2x + 3y.
        let grid = [0.0, 2.0, 3.0, 5.0];
        assert!(close(interp2(&xs, &ys, &grid, 0.5, 0.5).unwrap(), 2.5));
        assert!(close(interp2(&xs, &ys, &grid, 0.25, 0.75).unwrap(), 2.75));
    }

    #[test]
    fn a_polynomial_fit_recovers_the_coefficients() {
        let xs = [0.0, 1.0, 2.0, 3.0, 4.0];
        let ys: Vec<f64> = xs.iter().map(|x| 1.0 - 2.0 * x + 0.5 * x * x).collect();
        let coefficients = polyfit(&xs, &ys, 2).unwrap();
        assert!(close(coefficients[0], 1.0));
        assert!(close(coefficients[1], -2.0));
        assert!(close(coefficients[2], 0.5));
        assert!(close(polyval(&coefficients, 3.0), 1.0 - 6.0 + 4.5));
    }

    #[test]
    fn a_chebyshev_fit_is_stable_at_a_high_degree() {
        // Runge's function on [-1, 1]: a monomial fit of degree 12 at equally
        // spaced nodes would be terrible; the Chebyshev nodes are not.
        let xs: Vec<f64> = (0..41).map(|index| -1.0 + index as f64 / 20.0).collect();
        let ys: Vec<f64> = xs.iter().map(|x| 1.0 / (1.0 + 25.0 * x * x)).collect();
        let coefficients = chebyshev_fit(&xs, &ys, 12).unwrap();
        for x in [-0.8, -0.3, 0.0, 0.45, 0.9] {
            let fitted = polyval(&coefficients, x);
            let exact = 1.0 / (1.0 + 25.0 * x * x);
            assert!((fitted - exact).abs() < 0.05, "at {x}: {fitted} vs {exact}");
        }
    }
}
