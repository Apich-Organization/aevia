//! Quadrature kernels: thin adapters over `rssn::kernels::quadrature`.

use crate::NumericError;
use rssn::kernels::quadrature as kquad;
use std::cell::RefCell;

fn check_finite_interval(
    a: f64,
    b: f64,
) -> Result<(), NumericError> {
    if !a.is_finite() || !b.is_finite() {
        return Err(NumericError::Domain(
            "quadrature requires finite bounds".to_string(),
        ));
    }
    if b < a {
        return Err(NumericError::Domain(
            "quadrature requires a <= b".to_string(),
        ));
    }
    Ok(())
}

fn check_tol(tol: f64) -> Result<(), NumericError> {
    if tol.is_finite() && tol > 0.0 {
        Ok(())
    } else {
        Err(NumericError::Domain(
            "tolerance must be finite and positive".into(),
        ))
    }
}

/// Computes `n`-point Gauss–Legendre nodes and weights on `[-1, 1]`.
///
/// # Errors
/// [`NumericError::Domain`] when `n == 0`.
pub fn gauss_legendre_nodes_weights(n: usize) -> Result<(Vec<f64>, Vec<f64>), NumericError> {
    if n == 0 {
        return Err(NumericError::Domain(
            "Gauss–Legendre order n must be >= 1".to_string(),
        ));
    }
    Ok(kquad::gauss_legendre_rule(n))
}

/// `n`-point Gauss–Legendre quadrature of `f` over `[a, b]`.
///
/// # Errors
/// [`NumericError::Domain`] when `n == 0`, `b < a`, or bounds are non-finite.
pub fn quad_gauss_legendre<F>(
    f: &mut F,
    a: f64,
    b: f64,
    n: usize,
) -> Result<f64, NumericError>
where
    F: FnMut(f64) -> f64,
{
    check_finite_interval(a, b)?;
    if n == 0 {
        return Err(NumericError::Domain(
            "Gauss–Legendre order n must be >= 1".to_string(),
        ));
    }
    if a == b {
        return Ok(0.0);
    }
    let cell = RefCell::new(f);
    Ok(kquad::gauss_legendre(|x| (cell.borrow_mut())(x), a, b, n))
}

/// Adaptive 15-point Gauss–Kronrod (G7–K15) quadrature of `f` over `[a, b]`.
///
/// # Errors
/// [`NumericError::Domain`] or [`NumericError::NotConverged`].
pub fn quad_gauss_kronrod<F>(
    f: &mut F,
    a: f64,
    b: f64,
    tol: f64,
) -> Result<f64, NumericError>
where
    F: FnMut(f64) -> f64,
{
    check_tol(tol)?;
    check_finite_interval(a, b)?;
    if a == b {
        return Ok(0.0);
    }
    let cell = RefCell::new(f);
    let res = kquad::integrate(|x| (cell.borrow_mut())(x), a, b, tol.max(1e-14));
    if res.converged && res.value.is_finite() {
        Ok(res.value)
    } else {
        Err(NumericError::NotConverged)
    }
}

/// `n`-panel Clenshaw–Curtis quadrature of `f` over `[a, b]`.
///
/// # Errors
/// [`NumericError::Domain`] when `n == 0`, `b < a`, or bounds are non-finite.
pub fn quad_clenshaw_curtis<F>(
    f: &mut F,
    a: f64,
    b: f64,
    n: usize,
) -> Result<f64, NumericError>
where
    F: FnMut(f64) -> f64,
{
    check_finite_interval(a, b)?;
    if n == 0 {
        return Err(NumericError::Domain(
            "Clenshaw–Curtis order n must be >= 1".to_string(),
        ));
    }
    if a == b {
        return Ok(0.0);
    }
    let cell = RefCell::new(f);
    Ok(kquad::clenshaw_curtis(|x| (cell.borrow_mut())(x), a, b, n))
}

/// Romberg quadrature of `f` over `[a, b]`.
///
/// # Errors
/// [`NumericError::Domain`] or [`NumericError::NotConverged`].
pub fn quad_romberg<F>(
    f: &mut F,
    a: f64,
    b: f64,
    tol: f64,
    max_levels: usize,
) -> Result<f64, NumericError>
where
    F: FnMut(f64) -> f64,
{
    check_tol(tol)?;
    check_finite_interval(a, b)?;
    if a == b {
        return Ok(0.0);
    }
    if !(2..=24).contains(&max_levels) {
        return Err(NumericError::Domain(
            "Romberg requires 2 to 24 refinement levels".into(),
        ));
    }
    let cell = RefCell::new(f);
    let res = kquad::romberg(|x| (cell.borrow_mut())(x), a, b, tol.max(1e-14), max_levels);
    if !res.value.is_finite() {
        return Err(NumericError::Domain(
            "non-finite value in Romberg quadrature".to_string(),
        ));
    }
    if res.converged {
        Ok(res.value)
    } else {
        Err(NumericError::NotConverged)
    }
}

/// Tanh–Sinh (double-exponential) quadrature of `f` over `[a, b]`.
///
/// # Errors
/// [`NumericError::Domain`] or [`NumericError::NotConverged`].
pub fn quad_tanh_sinh<F>(
    f: &mut F,
    a: f64,
    b: f64,
    tol: f64,
    max_levels: usize,
) -> Result<f64, NumericError>
where
    F: FnMut(f64) -> f64,
{
    check_tol(tol)?;
    check_finite_interval(a, b)?;
    if a == b {
        return Ok(0.0);
    }
    let cell = RefCell::new(f);
    let res =
        kquad::tanh_sinh_with_levels(|x| (cell.borrow_mut())(x), a, b, tol.max(1e-14), max_levels);
    if res.converged && res.value.is_finite() {
        Ok(res.value)
    } else {
        Err(NumericError::NotConverged)
    }
}

/// Improper-interval quadrature over `[a, b]` where `a` may be `-inf` and/or
/// `b` may be `+inf`.
///
/// # Errors
/// [`NumericError::Domain`] when `b < a` or bounds are `NaN`, or [`NumericError::NotConverged`].
pub fn quad_improper<F>(
    f: &mut F,
    a: f64,
    b: f64,
    tol: f64,
) -> Result<f64, NumericError>
where
    F: FnMut(f64) -> f64,
{
    check_tol(tol)?;
    if a.is_nan() || b.is_nan() || a == f64::INFINITY || b == f64::NEG_INFINITY || b < a {
        return Err(NumericError::Domain(
            "improper quadrature requires a <= b and non-NaN bounds".to_string(),
        ));
    }
    if a == b {
        return Ok(0.0);
    }
    let tol = tol.max(1e-14);
    let cell = RefCell::new(f);
    let res = match (a.is_finite(), b.is_finite()) {
        | (true, true) => kquad::integrate(|x| (cell.borrow_mut())(x), a, b, tol),
        | (true, false) => kquad::integrate_semi_infinite(|x| (cell.borrow_mut())(x), a, tol),
        | (false, true) => kquad::integrate_semi_infinite(|x| (cell.borrow_mut())(-x), -b, tol),
        | (false, false) => kquad::integrate_infinite(|x| (cell.borrow_mut())(x), tol),
    };
    if res.converged && res.value.is_finite() {
        Ok(res.value)
    } else {
        Err(NumericError::NotConverged)
    }
}
