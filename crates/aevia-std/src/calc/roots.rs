//! Root-finding and polynomial root kernels: thin adapters over
//! `rssn::kernels::rootfind`.

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
    let fc = RefCell::new(f);
    let dfc = RefCell::new(df);
    let d2fc = RefCell::new(d2f);
    rootfind::halley(
        |x| (fc.borrow_mut())(x),
        |x| (dfc.borrow_mut())(x),
        |x| (d2fc.borrow_mut())(x),
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
