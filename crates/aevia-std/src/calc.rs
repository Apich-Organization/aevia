//! Calculus and numerics that work on numbers, not on terms (§8.5, §8.6).
//!
//! The symbolic entry points (`diff`, `defint`, `limit`, `series`, `solve`) go
//! through the kernel and live in `aevia-core`; these are the numeric ones,
//! which take a closure over `f64` and never build a term.
//!
//! The two phases are kept apart on purpose. `numeric_deriv` is a finite
//! difference on `Float` data and says so; `grad_f`/`jacobian`/`hessian` are
//! their multi-dimensional forms; `quadrature` uses adaptive Gauss–Kronrod with an
//! embedded error estimate. Everything iterative reports `NotConverged` rather than a
//! plausible-looking wrong answer.

use crate::NumericError;
use rssn::kernels::calculus as kcalc;
use rssn::kernels::optim as koptim;
use rssn::kernels::rootfind as kroots;
use std::cell::RefCell;

pub mod ad;
mod optim;
mod quad;
mod roots;

pub use ad::Dual;
pub use ad::Tape;
pub use ad::Var;
pub use ad::ad_derivative;
pub use ad::ad_grad;
pub use ad::ad_jacobian;
pub use ad::ad_reverse_grad;
pub use optim::least_squares_lm;
pub use optim::linprog;
pub use optim::minimize_bfgs;
pub use optim::minimize_box;
pub use optim::minimize_cg;
pub use optim::minimize_lbfgs;
pub use quad::gauss_legendre_nodes_weights;
pub use quad::quad_clenshaw_curtis;
pub use quad::quad_gauss_kronrod;
pub use quad::quad_gauss_legendre;
pub use quad::quad_improper;
pub use quad::quad_romberg;
pub use quad::quad_tanh_sinh;
pub use roots::poly_roots;
pub use roots::root_brent;
pub use roots::root_halley;
pub use roots::solve_system_broyden;
pub use roots::solve_system_dogleg;

/// The first derivative of `f` at `x`, by a central difference with one
/// Richardson extrapolation step.
pub fn derivative<F>(
    f: &mut F,
    x: f64,
) -> f64
where
    F: FnMut(f64) -> f64,
{
    kcalc::richardson_derivative(f, x)
}

/// The second derivative of `f` at `x`.
pub fn second_derivative<F>(
    f: &mut F,
    x: f64,
) -> f64
where
    F: FnMut(f64) -> f64,
{
    kcalc::second_derivative(f, x)
}

/// Adaptive Gauss–Kronrod quadrature of `f` over `[a, b]`.
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
    quad_gauss_kronrod(f, a, b, 1e-12)
}

/// The gradient of a scalar field at `x`, by central differences.
pub fn grad<F>(
    f: &mut F,
    x: &[f64],
) -> Vec<f64>
where
    F: FnMut(&[f64]) -> f64,
{
    let f = RefCell::new(f);
    kcalc::gradient(|x| (f.borrow_mut())(x), x)
}

/// The Jacobian of a vector field at `x`: `(rows, cols, row-major entries)`.
pub fn jacobian<F>(
    f: &mut F,
    x: &[f64],
) -> (usize, usize, Vec<f64>)
where
    F: FnMut(&[f64]) -> Vec<f64>,
{
    let f = RefCell::new(f);
    let callback = |x: &[f64]| (f.borrow_mut())(x);
    let fx = callback(x);
    let j = kroots::numerical_jacobian(&callback, x, &fx);
    (j.rows, j.cols, j.data)
}

/// The Hessian of a scalar field at `x`.
pub fn hessian<F>(
    f: &mut F,
    x: &[f64],
) -> Vec<f64>
where
    F: FnMut(&[f64]) -> f64,
{
    let f = RefCell::new(f);
    kcalc::hessian(|x| (f.borrow_mut())(x), x)
}

/// The divergence of a vector field at `x`.
pub fn divergence<F>(
    f: &mut F,
    x: &[f64],
) -> f64
where
    F: FnMut(&[f64]) -> Vec<f64>,
{
    let f = RefCell::new(f);
    kcalc::divergence(|x| (f.borrow_mut())(x), x)
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
            "`curl` requires a three-dimensional field".into(),
        ));
    }
    let f = RefCell::new(f);
    kcalc::curl(|x| (f.borrow_mut())(x), x)
        .ok_or_else(|| NumericError::ShapeMismatch("curl requires three field components".into()))
}

/// The Laplacian of a scalar field at `x`: the trace of the Hessian.
pub fn laplacian<F>(
    f: &mut F,
    x: &[f64],
) -> f64
where
    F: FnMut(&[f64]) -> f64,
{
    let f = RefCell::new(f);
    kcalc::laplacian(|x| (f.borrow_mut())(x), x)
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
    kroots::root_near(f, guess).map_err(roots::map_root_err)
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
    if start.is_empty() {
        return Err(NumericError::Empty);
    }
    let f = RefCell::new(f);
    kroots::newton_system::<_, fn(&[f64]) -> rssn::kernels::dense::Mat>(
        |x| (f.borrow_mut())(x),
        None,
        start,
        1e-12,
        100,
    )
    .map(|s| s.x)
    .map_err(roots::map_root_err)
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
    if start.is_empty() {
        return Err(NumericError::Empty);
    }
    let f = RefCell::new(f);
    let result = koptim::nelder_mead(|x| (f.borrow_mut())(x), start, 0.05, 1e-12, 4000);
    if result.converged && result.fx.is_finite() {
        Ok(result.x)
    } else {
        Err(NumericError::NotConverged)
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
    let mut negated = |x: &[f64]| -f(x);
    minimize(&mut negated, start)
}

/// A least-squares solution of `f(x) ≈ 0`, by Levenberg–Marquardt.
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
    least_squares_lm(f, start, 1e-12, 200)
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

    #[test]
    fn extended_quadrature_methods_agree() {
        // Smooth integral: int_0^pi sin(x) dx = 2
        let mut s = |x: f64| x.sin();
        let pi = std::f64::consts::PI;
        assert!(close(
            quad_gauss_legendre(&mut s, 0.0, pi, 8).unwrap(),
            2.0,
            1e-12
        ));
        assert!(close(
            quad_gauss_kronrod(&mut s, 0.0, pi, 1e-12).unwrap(),
            2.0,
            1e-12
        ));
        assert!(close(
            quad_clenshaw_curtis(&mut s, 0.0, pi, 16).unwrap(),
            2.0,
            1e-12
        ));
        assert!(close(
            quad_romberg(&mut s, 0.0, pi, 1e-12, 12).unwrap(),
            2.0,
            1e-11
        ));

        // Endpoint singularity: int_0^1 1 / sqrt(x) dx = 2
        let mut sing = |x: f64| 1.0 / x.sqrt();
        assert!(close(
            quad_tanh_sinh(&mut sing, 0.0, 1.0, 1e-10, 10).unwrap(),
            2.0,
            1e-9
        ));

        // Improper integrals:
        // int_0^inf exp(-x) dx = 1
        let mut exp_neg = |x: f64| (-x).exp();
        assert!(close(
            quad_improper(&mut exp_neg, 0.0, f64::INFINITY, 1e-10).unwrap(),
            1.0,
            1e-9
        ));
        // int_-inf^inf exp(-x^2) dx = sqrt(pi)
        let mut gauss = |x: f64| (-x * x).exp();
        assert!(close(
            quad_improper(&mut gauss, f64::NEG_INFINITY, f64::INFINITY, 1e-9).unwrap(),
            pi.sqrt(),
            1e-8
        ));
    }

    #[test]
    fn extended_root_finders_and_poly_roots() {
        let mut f = |x: f64| x.cos() - x;
        let r_brent = root_brent(&mut f, 0.0, 1.0, 1e-12, 100).unwrap();
        assert!(close(r_brent.cos(), r_brent, 1e-12));

        let mut df = |x: f64| -x.sin() - 1.0;
        let mut d2f = |x: f64| -x.cos();
        let r_halley = root_halley(&mut f, &mut df, &mut d2f, 0.5, 1e-13, 30).unwrap();
        assert!(close(r_halley, r_brent, 1e-12));

        // Nonlinear system: x^2 + y^2 = 1, x - y = 0
        let mut sys = |x: &[f64]| vec![x[0] * x[0] + x[1] * x[1] - 1.0, x[0] - x[1]];
        let expected = 1.0 / 2.0_f64.sqrt();
        let broyden = solve_system_broyden(&mut sys, &[0.6, 0.8], 1e-10, 50).unwrap();
        assert!(close(broyden[0], expected, 1e-9));
        assert!(close(broyden[1], expected, 1e-9));

        let dogleg = solve_system_dogleg(&mut sys, &[0.6, 0.8], 1e-10, 50).unwrap();
        assert!(close(dogleg[0], expected, 1e-9));
        assert!(close(dogleg[1], expected, 1e-9));

        // Polynomial roots: (x - 1)(x - 2)(x^2 + 1) = x^4 - 3x^3 + 3x^2 - 3x + 2
        let roots = poly_roots(&[2.0, -3.0, 3.0, -3.0, 1.0]).unwrap();
        assert_eq!(roots.len(), 4);
        for z in &roots {
            let p_val = crate::Complex::new(2.0, 0.0);
            let mut acc = crate::Complex::new(1.0, 0.0);
            for &c in &[-3.0, 3.0, -3.0] {
                acc = acc.mul(*z);
                acc.re += c;
            }
            acc = acc.mul(*z);
            acc.re += p_val.re;
            assert!(acc.abs() < 1e-11, "non-root: {z:?}, |p(z)| = {}", acc.abs());
        }
    }

    #[test]
    fn extended_optimizers_and_linprog() {
        // Rosenbrock 2D: min at (1, 1)
        let mut rosen = |x: &[f64]| (1.0 - x[0]).powi(2) + 100.0 * (x[1] - x[0] * x[0]).powi(2);
        let mut rosen_g = |x: &[f64]| {
            vec![
                -2.0 * (1.0 - x[0]) - 400.0 * x[0] * (x[1] - x[0] * x[0]),
                200.0 * (x[1] - x[0] * x[0]),
            ]
        };

        let x_bfgs = minimize_bfgs(&mut rosen, &mut rosen_g, &[-1.2, 1.0], 1e-8, 200).unwrap();
        assert!(close(x_bfgs[0], 1.0, 1e-6) && close(x_bfgs[1], 1.0, 1e-6));

        let x_lbfgs = minimize_lbfgs(&mut rosen, &mut rosen_g, &[-1.2, 1.0], 7, 1e-8, 500).unwrap();
        assert!(close(x_lbfgs[0], 1.0, 1e-6) && close(x_lbfgs[1], 1.0, 1e-6));

        let x_cg = minimize_cg(&mut rosen, &mut rosen_g, &[-1.2, 1.0], 1e-6, 1000).unwrap();
        assert!(close(x_cg[0], 1.0, 1e-4) && close(x_cg[1], 1.0, 1e-4));

        // Box constrained: min (x0 - 3)^2 + (x1 + 2)^2 s.t. x in [0, 2] x [-1, 1] -> (2, -1)
        let mut quad_f = |x: &[f64]| (x[0] - 3.0).powi(2) + (x[1] + 2.0).powi(2);
        let mut quad_g = |x: &[f64]| vec![2.0 * (x[0] - 3.0), 2.0 * (x[1] + 2.0)];
        let x_box = minimize_box(
            &mut quad_f,
            &mut quad_g,
            &[0.5, 0.5],
            &[0.0, -1.0],
            &[2.0, 1.0],
            1e-9,
            100,
        )
        .unwrap();
        assert!(close(x_box[0], 2.0, 1e-8) && close(x_box[1], -1.0, 1e-8));

        // Levenberg-Marquardt: exponential fit y = a * exp(b * t)
        let ts = [0.0_f64, 1.0, 2.0, 3.0];
        let ys: Vec<f64> = ts.iter().map(|&t| 2.5 * (-0.4 * t).exp()).collect();
        let mut res = |p: &[f64]| {
            ts.iter()
                .zip(&ys)
                .map(|(&t, &y)| p[0] * (p[1] * t).exp() - y)
                .collect()
        };
        let p_lm = least_squares_lm(&mut res, &[1.0, -0.1], 1e-10, 100).unwrap();
        assert!(close(p_lm[0], 2.5, 1e-8) && close(p_lm[1], -0.4, 1e-8));

        // Simplex LP: min -x0 - 2*x1 s.t. x0 + x1 <= 4, x0 <= 2, x1 <= 3, x >= 0
        // Optimal at (1, 3), obj = -7
        let (x_lp, obj) = linprog(
            &[-1.0, -2.0],
            &[1.0, 1.0, 1.0, 0.0, 0.0, 1.0],
            &[4.0, 2.0, 3.0],
        )
        .unwrap();
        assert!(close(x_lp[0], 1.0, 1e-10) && close(x_lp[1], 3.0, 1e-10));
        assert!(close(obj, -7.0, 1e-10));
    }

    #[test]
    fn automatic_differentiation_forward_and_reverse() {
        // Forward 1D: f(x) = sin(x^2) * exp(x), f'(x) = (2x cos(x^2) + sin(x^2)) exp(x)
        let x = 0.7_f64;
        let (val, d) = ad_derivative(&mut |z: Dual| (z * z).sin() * z.exp(), x);
        let exact_val = (x * x).sin() * x.exp();
        let exact_d = (2.0 * x * (x * x).cos() + (x * x).sin()) * x.exp();
        assert!(close(val, exact_val, 1e-14));
        assert!(close(d, exact_d, 1e-14));

        // Forward vs Reverse gradient of f(x, y, z) = x * y + sin(y * z) + exp(x - z)
        let pt = [1.2, -0.5, 0.8];
        let g_fwd = ad_grad(
            &mut |v: &[Dual]| v[0] * v[1] + (v[1] * v[2]).sin() + (v[0] - v[2]).exp(),
            &pt,
        )
        .unwrap();
        let (r_val, g_rev) = ad_reverse_grad(
            &mut |v: &[Var]| {
                let t1 = &v[0] * &v[1];
                let t2 = (&v[1] * &v[2]).sin();
                let t3 = (&v[0] - &v[2]).exp();
                &(&t1 + &t2) + &t3
            },
            &pt,
        )
        .unwrap();
        let exact_f = pt[0] * pt[1] + (pt[1] * pt[2]).sin() + (pt[0] - pt[2]).exp();
        assert!(close(r_val, exact_f, 1e-14));
        for i in 0..3 {
            assert!(close(g_fwd[i], g_rev[i], 1e-14));
        }

        // Forward Jacobian: [x*y, x^2 + sin(y)]
        let jac = ad_jacobian(
            &mut |v: &[Dual]| vec![v[0] * v[1], v[0] * v[0] + v[1].sin()],
            &[2.0, 0.5],
        )
        .unwrap();
        assert!(close(jac[0], 0.5, 1e-14));
        assert!(close(jac[1], 2.0, 1e-14));
        assert!(close(jac[2], 4.0, 1e-14));
        assert!(close(jac[3], 0.5_f64.cos(), 1e-14));
    }
    #[test]
    fn quadrature_rejects_invalid_controls_and_honors_budget() {
        let mut f = |x: f64| x.exp();
        for tol in [0.0, -1.0, f64::NAN, f64::INFINITY] {
            assert!(matches!(
                quad_gauss_kronrod(&mut f, 0.0, 1.0, tol),
                Err(NumericError::Domain(_))
            ));
        }
        assert!(matches!(
            quad_tanh_sinh(&mut f, 0.0, 1.0, 1e-12, 0),
            Err(NumericError::NotConverged)
        ));
        assert!(quad_improper(&mut f, f64::INFINITY, f64::INFINITY, 1e-10).is_err());
        assert!(quad_romberg(&mut f, 0.0, 1.0, 1e-10, usize::MAX).is_err());
    }

    #[test]
    fn tanh_sinh_integrates_endpoint_singularity_without_truncation() {
        let mut f = |x: f64| 1.0 / x.sqrt();
        let value = quad_tanh_sinh(&mut f, 0.0, 1.0, 1e-8, 12).unwrap();
        assert!((value - 2.0).abs() < 1e-7, "{value}");
        let mut invalid = |_: f64| f64::NAN;
        assert!(quad_tanh_sinh(&mut invalid, 0.0, 1.0, 1e-8, 2).is_err());
    }

    #[test]
    fn curl_rejects_wrong_output_dimension() {
        assert!(matches!(
            curl(&mut |_: &[f64]| vec![0.0; 2], &[0.0; 3]),
            Err(NumericError::ShapeMismatch(_))
        ));
    }
    #[test]
    fn optimizer_rejects_malformed_gradient_and_nan_bounds() {
        let mut f = |x: &[f64]| x.iter().map(|v| v * v).sum();
        let mut calls = 0;
        let mut bad = |_: &[f64]| {
            calls += 1;
            if calls == 1 {
                vec![1.0]
            } else {
                Vec::new()
            }
        };
        assert!(minimize_bfgs(&mut f, &mut bad, &[1.0], 1e-8, 10).is_err());
        assert!(
            minimize_box(
                &mut f,
                &mut |x: &[f64]| x.to_vec(),
                &[0.0],
                &[f64::NAN],
                &[1.0],
                1e-8,
                10
            )
            .is_err()
        );
    }

    #[test]
    fn automatic_differentiation_rejects_foreign_output_tape() {
        let other = Tape::new();
        let mut foreign = |_: &[Var]| other.var(3.0);
        assert!(matches!(
            ad_reverse_grad(&mut foreign, &[1.0]),
            Err(NumericError::Domain(_))
        ));
    }
    #[test]
    fn dogleg_does_not_report_a_nonzero_stationary_residual_as_a_root() {
        assert!(solve_system_dogleg(&mut |_: &[f64]| vec![1.0], &[0.0], 1e-9, 10).is_err());
    }
}
