//! Root-finding and polynomial root kernels backed by RSSN.
//!
//! The explicit Brent adapter uses the caller's interval and budget. The
//! nearby-root solver used by Solve searches for its own interval in RSSN.

use crate::Complex;
use crate::NumericError;
use rssn::kernels::rootfind::RootError;
use rssn::kernels::rootfind::{
    self,
};
use std::cell::RefCell;

pub(super) fn map_root_err(err: RootError) -> NumericError {
    match err {
        | RootError::NotBracketed => {
            NumericError::Domain("root_brent requires opposite signs at bracket endpoints".into())
        },
        | RootError::Singular => NumericError::Singular,
        | RootError::NoConvergence => NumericError::NotConverged,
        | RootError::Invalid => NumericError::Domain("invalid root-finding input".into()),
    }
}

/// Brent's method (Van Wijngaarden–Dekker–Brent) for a root of `f` in `[a, b]`.
///
/// # Errors
/// [`NumericError::Domain`] if `f(a)` and `f(b)` do not bracket a root, or
/// [`NumericError::NotConverged`].
pub fn root_brent<F>(
    f: &mut F,
    a: f64,
    b: f64,
    tol: f64,
    max_iter: usize,
) -> Result<f64, NumericError>
where
    F: FnMut(f64) -> f64,
{
    let cell = RefCell::new(f);
    rootfind::brent(|x| (cell.borrow_mut())(x), a, b, tol, max_iter).map_err(map_root_err)
}

/// Halley's third-order root-finding method given `f`, `f'`, and `f''`.
///
/// # Errors
/// [`NumericError::NotConverged`] or [`NumericError::Singular`].
pub fn root_halley<F, DF, D2F>(
    f: &mut F,
    df: &mut DF,
    d2f: &mut D2F,
    x0: f64,
    tol: f64,
    max_iter: usize,
) -> Result<f64, NumericError>
where
    F: FnMut(f64) -> f64,
    DF: FnMut(f64) -> f64,
    D2F: FnMut(f64) -> f64,
{
    let function = RefCell::new(f);
    let first_derivative = RefCell::new(df);
    let second_derivative = RefCell::new(d2f);
    rootfind::halley(
        |x| (function.borrow_mut())(x),
        |x| (first_derivative.borrow_mut())(x),
        |x| (second_derivative.borrow_mut())(x),
        x0,
        tol,
        max_iter,
    )
    .map_err(map_root_err)
}

/// Broyden's good quasi-Newton method for solving `f(x) = 0`.
///
/// # Errors
/// [`NumericError::Empty`], [`NumericError::ShapeMismatch`], or
/// [`NumericError::NotConverged`].
pub fn solve_system_broyden<F>(
    f: &mut F,
    x0: &[f64],
    tol: f64,
    max_iter: usize,
) -> Result<Vec<f64>, NumericError>
where
    F: FnMut(&[f64]) -> Vec<f64>,
{
    if x0.is_empty() {
        return Err(NumericError::Empty);
    }
    let cell = RefCell::new(f);
    let sol =
        rootfind::broyden(|x| (cell.borrow_mut())(x), x0, tol, max_iter).map_err(map_root_err)?;
    Ok(sol.x)
}

/// Trust-region dogleg method for solving `f(x) = 0`.
///
/// # Errors
/// [`NumericError::Empty`], [`NumericError::ShapeMismatch`], or
/// [`NumericError::NotConverged`].
pub fn solve_system_dogleg<F>(
    f: &mut F,
    x0: &[f64],
    tol: f64,
    max_iter: usize,
) -> Result<Vec<f64>, NumericError>
where
    F: FnMut(&[f64]) -> Vec<f64>,
{
    if x0.is_empty() {
        return Err(NumericError::Empty);
    }
    let cell = RefCell::new(f);
    let sol =
        rootfind::dogleg(|x| (cell.borrow_mut())(x), x0, tol, max_iter).map_err(map_root_err)?;
    Ok(sol.x)
}

/// Finds all complex roots of a polynomial `c[0] + c[1] x + ... + c[d] x^d`
/// (ascending power order) via `rssn::kernels::rootfind::polynomial_roots_aberth`.
///
/// # Errors
/// [`NumericError::Empty`] when `coeffs` is empty, or [`NumericError::Domain`]
/// when the polynomial is constant (`degree == 0`).
pub fn poly_roots(coeffs: &[f64]) -> Result<Vec<Complex>, NumericError> {
    if coeffs.is_empty() {
        return Err(NumericError::Empty);
    }
    let roots = rootfind::polynomial_roots_aberth(coeffs).map_err(map_root_err)?;
    Ok(roots
        .into_iter()
        .map(|(re, im)| Complex::new(re, im))
        .collect())
}

#[cfg(test)]
mod tests {
    use super::super::root;
    use super::NumericError;

    #[test]
    fn nearby_root_search_keeps_tiny_residual_signs_and_its_probe_order() {
        let mut visited = Vec::new();
        let result = root(
            &mut |x| {
                visited.push(x);
                (x - 2.5) * 1e-200
            },
            0.0,
        )
        .unwrap();
        assert_eq!(result.to_bits(), 2.5_f64.to_bits());
        assert_eq!(
            &visited[5..],
            &[-1.0, 1.0, -2.0, 2.0, -4.0, 4.0, 0.0, 2.0, 3.0, 2.5],
        );
    }

    #[test]
    fn nearby_root_search_handles_exact_endpoints_and_invalid_interiors() {
        let mut visited = Vec::new();
        let result = root(
            &mut |x| {
                visited.push(x);
                (x - 2.0) * 1e-200
            },
            0.0,
        )
        .unwrap();
        assert_eq!(result.to_bits(), 2.0_f64.to_bits());
        assert_eq!(&visited[5..], &[-1.0, 1.0, -2.0, 2.0]);

        let mut blocked = |x: f64| {
            if x.to_bits() == 3.0_f64.to_bits() {
                f64::NAN
            } else {
                (x - 2.5) * 1e-200
            }
        };
        assert_eq!(
            root(&mut blocked, 0.0),
            Err(NumericError::Domain("invalid root-finding input".into())),
        );
    }

    #[test]
    fn a_nonfinite_difference_sample_cannot_report_a_converged_zero_step() {
        let bad_probe = (f64::EPSILON.cbrt() * 0.5).to_bits();
        let mut residual = |x: f64| {
            assert!(x.is_finite());
            if x.to_bits() == bad_probe {
                f64::INFINITY
            } else {
                x - 0.25
            }
        };
        let result = root(&mut residual, 0.0).unwrap();
        assert_eq!(result.to_bits(), 0.25_f64.to_bits());
    }
}
