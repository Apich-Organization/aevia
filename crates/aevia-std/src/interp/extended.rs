//! Extended interpolation and approximation kernels: thin adapters over
//! `rssn::kernels::interp_ext`.

use crate::NumericError;
use rssn::kernels::interp_ext::InterpError;
use rssn::kernels::interp_ext::{
    self,
};

pub use rssn::kernels::interp_ext::RbfKernel;
pub use rssn::kernels::interp_ext::SplineBc;

pub(super) fn map_err(err: InterpError) -> NumericError {
    match err {
        | InterpError::Empty => NumericError::Empty,
        | InterpError::ShapeMismatch(s) => NumericError::ShapeMismatch(s),
        | InterpError::Domain(s) => NumericError::Domain(s),
        | InterpError::Singular => NumericError::Singular,
    }
}

/// Evaluates a cubic spline with the specified boundary condition at `x`.
pub fn cubic_spline(
    xs: &[f64],
    ys: &[f64],
    x: f64,
    bc: SplineBc,
) -> Result<f64, NumericError> {
    interp_ext::cubic_spline(xs, ys, x, bc).map_err(map_err)
}

/// PCHIP (Piecewise Cubic Hermite Interpolating Polynomial) at `x`.
pub fn pchip(
    xs: &[f64],
    ys: &[f64],
    x: f64,
) -> Result<f64, NumericError> {
    interp_ext::pchip(xs, ys, x).map_err(map_err)
}

/// Akima 1D cubic spline interpolation at `x`.
pub fn akima(
    xs: &[f64],
    ys: &[f64],
    x: f64,
) -> Result<f64, NumericError> {
    interp_ext::akima(xs, ys, x).map_err(map_err)
}

/// Evaluates a B-spline of given `degree` at `x`.
pub fn bspline_eval(
    knots: &[f64],
    coeffs: &[f64],
    degree: usize,
    x: f64,
) -> Result<f64, NumericError> {
    interp_ext::bspline_eval(knots, coeffs, degree, x).map_err(map_err)
}

/// A fitted B-spline curve backed by `rssn::kernels::interp_ext::BSpline`.
#[derive(Clone, Debug)]
pub struct BSpline(interp_ext::BSpline);

impl BSpline {
    /// Fits a clamped B-spline of `degree` that interpolates `(xs[i], ys[i])`.
    pub fn interpolate(
        xs: &[f64],
        ys: &[f64],
        degree: usize,
    ) -> Result<Self, NumericError> {
        interp_ext::BSpline::interpolate(xs, ys, degree)
            .map(Self)
            .map_err(map_err)
    }

    /// Evaluates the fitted B-spline at `x`.
    pub fn eval(
        &self,
        x: f64,
    ) -> Result<f64, NumericError> {
        self.0.eval(x).map_err(map_err)
    }
}

/// Bicubic interpolation on a rectilinear 2D grid.
pub fn bicubic(
    xs: &[f64],
    ys: &[f64],
    grid: &[f64],
    x: f64,
    y: f64,
) -> Result<f64, NumericError> {
    interp_ext::bicubic(xs, ys, grid, x, y).map_err(map_err)
}

/// N-dimensional multilinear interpolation on a rectilinear grid.
pub fn interpn(
    axes: &[&[f64]],
    grid: &[f64],
    point: &[f64],
) -> Result<f64, NumericError> {
    interp_ext::interpn(axes, grid, point).map_err(map_err)
}

/// Radial Basis Function (RBF) scattered-data interpolation in `R^d`.
pub fn rbf_interp(
    points: &[&[f64]],
    values: &[f64],
    query: &[f64],
    kernel: RbfKernel,
) -> Result<f64, NumericError> {
    interp_ext::rbf_interp(points, values, query, kernel).map_err(map_err)
}

/// Shepard inverse-distance weighting (IDW) scattered-data interpolation in `R^d`.
pub fn shepard_idw(
    points: &[&[f64]],
    values: &[f64],
    query: &[f64],
    power: f64,
) -> Result<f64, NumericError> {
    interp_ext::shepard_idw(points, values, query, power).map_err(map_err)
}

/// 2D scattered-data linear interpolation over the Delaunay triangulation of `points`.
pub fn delaunay_linear_2d(
    points: &[(f64, f64)],
    values: &[f64],
    x: f64,
    y: f64,
) -> Result<f64, NumericError> {
    interp_ext::delaunay_linear_2d(points, values, x, y).map_err(map_err)
}

/// Computes the `[m / n]` Padé approximant `P(x) / Q(x)`.
pub fn pade(
    coeffs: &[f64],
    m: usize,
    n: usize,
) -> Result<(Vec<f64>, Vec<f64>), NumericError> {
    interp_ext::pade(coeffs, m, n).map_err(map_err)
}

/// Evaluates a rational function `P(x) / Q(x)`.
pub fn rational_eval(
    p: &[f64],
    q: &[f64],
    x: f64,
) -> Result<f64, NumericError> {
    interp_ext::rational_eval(p, q, x).map_err(map_err)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn close(
        a: f64,
        b: f64,
        tol: f64,
    ) -> bool {
        (a - b).abs() < tol
    }

    #[test]
    fn pchip_preserves_monotonicity() {
        let xs = [0.0, 1.0, 2.0, 3.0, 4.0];
        let ys = [0.0, 0.0, 1.0, 1.0, 1.0];
        for k in 0..40 {
            let x = k as f64 * 0.1;
            let v = pchip(&xs, &ys, x).unwrap();
            assert!((-1e-12..=1.0 + 1e-12).contains(&v), "overshoot at {x}: {v}");
        }
    }

    #[test]
    fn akima_and_cubic_splines_reproduce_polynomials() {
        let xs = [0.0, 1.0, 2.0, 3.0, 4.0];
        let f = |x: f64| x * x * x - 2.0 * x + 1.0;
        let df = |x: f64| 3.0 * x * x - 2.0;
        let ys: Vec<f64> = xs.iter().map(|&x| f(x)).collect();

        // Not-a-knot and clamped splines are exact on cubics!
        for x in [0.4, 1.7, 2.5, 3.3] {
            assert!(close(
                cubic_spline(&xs, &ys, x, SplineBc::NotAKnot).unwrap(),
                f(x),
                1e-10
            ));
            assert!(close(
                cubic_spline(&xs, &ys, x, SplineBc::Clamped(df(0.0), df(4.0))).unwrap(),
                f(x),
                1e-10
            ));
        }

        // Akima is exact on quadratics.
        let q_ys: Vec<f64> = xs.iter().map(|&x| x * x - 2.0 * x + 1.0).collect();
        for x in [0.4, 1.7, 2.5, 3.3] {
            assert!(close(
                akima(&xs, &q_ys, x).unwrap(),
                x * x - 2.0 * x + 1.0,
                1e-10
            ));
        }
    }

    #[test]
    fn bspline_interpolates_data() {
        let xs = [0.0_f64, 0.5, 1.2, 2.0, 3.1, 4.0];
        let ys: Vec<f64> = xs.iter().map(|&x| (x * 0.8).sin()).collect();
        let sp = BSpline::interpolate(&xs, &ys, 3).unwrap();
        for (&x, &y) in xs.iter().zip(&ys) {
            assert!(close(sp.eval(x).unwrap(), y, 1e-10));
        }
    }

    #[test]
    fn bicubic_and_interpn_match_analytical_functions() {
        let xs = [0.0, 1.0, 2.0, 3.0];
        let ys = [0.0, 1.0, 2.0, 3.0];
        let mut grid = Vec::new();
        for &y in &ys {
            for &x in &xs {
                grid.push(x * x + 2.0 * x * y - y * y);
            }
        }
        let val = bicubic(&xs, &ys, &grid, 1.4, 2.3).unwrap();
        let exact = 1.4 * 1.4 + 2.0 * 1.4 * 2.3 - 2.3 * 2.3;
        assert!(close(val, exact, 1e-10));

        // 3D multilinear: f(x,y,z) = x + 2y + 3z
        let ax = [0.0, 2.0];
        let ay = [0.0, 2.0];
        let az = [0.0, 2.0];
        let mut g3 = Vec::new();
        for &x in &ax {
            for &y in &ay {
                for &z in &az {
                    g3.push(x + 2.0 * y + 3.0 * z);
                }
            }
        }
        let v3 = interpn(&[&ax, &ay, &az], &g3, &[0.5, 1.0, 1.5]).unwrap();
        assert!(close(v3, 0.5 + 2.0 + 4.5, 1e-12));
    }

    #[test]
    fn scattered_data_interpolants_work() {
        let pts_raw = [[0.0, 0.0], [1.0, 0.0], [0.0, 1.0], [1.0, 1.0], [0.5, 0.5]];
        let pts: Vec<&[f64]> = pts_raw.iter().map(|p| p.as_slice()).collect();
        let vals: Vec<f64> = pts_raw
            .iter()
            .map(|p| 2.0 * p[0] + 3.0 * p[1] + 1.0)
            .collect();

        let rbf_v = rbf_interp(&pts, &vals, &[0.25, 0.75], RbfKernel::Multiquadric(1.0)).unwrap();
        assert!(close(rbf_v, 2.0 * 0.25 + 3.0 * 0.75 + 1.0, 0.15));

        let idw_v = shepard_idw(&pts, &vals, &[0.5, 0.5], 2.0).unwrap();
        assert!(close(idw_v, 3.5, 1e-12));

        let pts_2d: Vec<(f64, f64)> = pts_raw.iter().map(|p| (p[0], p[1])).collect();
        let del_v = delaunay_linear_2d(&pts_2d, &vals, 0.25, 0.75).unwrap();
        assert!(close(del_v, 2.0 * 0.25 + 3.0 * 0.75 + 1.0, 1e-10));
    }

    #[test]
    fn pade_approximates_exponential() {
        // Taylor series of exp(x): 1, 1, 1/2, 1/6, 1/24
        let c = [1.0, 1.0, 0.5, 1.0 / 6.0, 1.0 / 24.0];
        let (p, q) = pade(&c, 2, 2).unwrap();
        for x in [-0.5_f64, 0.0, 0.5, 1.0] {
            let approx = rational_eval(&p, &q, x).unwrap();
            let tol = if x.abs() <= 0.5 { 2e-4 } else { 5e-3 };
            assert!(
                close(approx, x.exp(), tol),
                "at {x}: {approx} vs {}",
                x.exp()
            );
        }
    }
    #[test]
    fn invalid_knots_and_excessive_grid_rank_are_rejected() {
        assert!(pchip(&[0.0, f64::NAN], &[0.0, 1.0], 0.5).is_err());
        assert!(cubic_spline(&[0.0, 0.0, 1.0], &[0.0, 1.0, 2.0], 0.5, SplineBc::Natural).is_err());
        let axis = [0.0];
        let axes = vec![axis.as_slice(); usize::BITS as usize];
        assert!(interpn(&axes, &[1.0], &vec![0.0; axes.len()]).is_err());
    }
}
