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

use crate::NumericError;
use rssn::kernels::interp_ext;

mod extended;
pub use extended::BSpline;
pub use extended::RbfKernel;
pub use extended::SplineBc;
pub use extended::akima;
pub use extended::bicubic;
pub use extended::bspline_eval;
pub use extended::cubic_spline;
pub use extended::delaunay_linear_2d;
pub use extended::interpn;
pub use extended::pade;
pub use extended::pchip;
pub use extended::rational_eval;
pub use extended::rbf_interp;
pub use extended::shepard_idw;

/// Linear interpolation of `(xs, ys)` at `x`.
///
/// Outside the range the nearest endpoint is extended linearly from its own
/// segment, which is what a piecewise-linear interpolant does at its ends.
///
/// # Errors
///
/// [`NumericError::ShapeMismatch`] when the two samples differ in length, and
/// [`NumericError::Empty`] when there are no points.
pub fn interp1(
    xs: &[f64],
    ys: &[f64],
    x: f64,
) -> Result<f64, NumericError> {
    interp_ext::interp1(xs, ys, x).map_err(extended::map_err)
}

/// Natural cubic spline interpolation at `x`.
///
/// # Errors
///
/// [`NumericError::ShapeMismatch`] when the samples differ in length, and
/// [`NumericError::Domain`] when fewer than three points are given (a natural
/// spline needs a second derivative to pin down).
pub fn spline(
    xs: &[f64],
    ys: &[f64],
    x: f64,
) -> Result<f64, NumericError> {
    interp_ext::spline(xs, ys, x).map_err(extended::map_err)
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
    interp_ext::interp2(xs, ys, grid, x, y).map_err(extended::map_err)
}

/// The coefficients of the least-squares polynomial of `degree`, ascending.
///
/// # Errors
///
/// [`NumericError::ShapeMismatch`] when the samples differ in length, and
/// [`NumericError::Domain`] when there are fewer points than coefficients.
pub fn polyfit(
    xs: &[f64],
    ys: &[f64],
    degree: usize,
) -> Result<Vec<f64>, NumericError> {
    interp_ext::polyfit(xs, ys, degree).map_err(extended::map_err)
}

/// Evaluates a polynomial given its coefficients in ascending order.
#[must_use]
pub fn polyval(
    coefficients: &[f64],
    x: f64,
) -> f64 {
    interp_ext::polyval(coefficients, x)
}

/// The monomial coefficients of the polynomial interpolating `(xs, ys)` at
/// `degree + 1` Chebyshev nodes of the sample's range.
///
/// # Errors
///
/// [`NumericError::ShapeMismatch`] when the samples differ in length, and
/// [`NumericError::Empty`] when there are none.
pub fn chebyshev_fit(
    xs: &[f64],
    ys: &[f64],
    degree: usize,
) -> Result<Vec<f64>, NumericError> {
    interp_ext::chebyshev_fit(xs, ys, degree).map_err(extended::map_err)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn close(
        a: f64,
        b: f64,
    ) -> bool {
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
