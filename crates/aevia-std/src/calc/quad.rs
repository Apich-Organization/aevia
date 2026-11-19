//! Quadrature kernels: thin adapters over `rssn::kernels::quadrature`.

use crate::NumericError;
use rssn::kernels::quadrature as kquad;
use std::cell::RefCell;

fn check_finite_interval(
    a: f64,
    b: f64,
) -> Result<(), NumericError> {
    if !a.is_finite() || !b.is_finite() || !(b - a).is_finite() {
        return Err(NumericError::Domain(
            "quadrature requires finite bounds with a representable interval length".to_string(),
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

fn check_rule_size(
    order: usize,
    nodes: usize,
) -> Result<(), NumericError> {
    if order == 0
        || nodes
            .checked_mul(std::mem::size_of::<f64>())
            .is_none_or(|bytes| isize::try_from(bytes).is_err())
    {
        return Err(NumericError::Domain(
            "invalid or unrepresentable quadrature order".into(),
        ));
    }
    Ok(())
}

const fn finite_integral(value: f64) -> Result<f64, NumericError> {
    if value.is_finite() {
        Ok(value)
    } else {
        Err(NumericError::NotConverged)
    }
}

/// Computes `n`-point Gauss–Legendre nodes and weights on `[-1, 1]`.
///
/// # Errors
/// [`NumericError::Domain`] when the order is zero or cannot fit the node and weight arrays.
pub fn gauss_legendre_nodes_weights(n: usize) -> Result<(Vec<f64>, Vec<f64>), NumericError> {
    check_rule_size(n, n)?;
    Ok(kquad::gauss_legendre_rule(n))
}

/// `n`-point Gauss–Legendre quadrature of `f` over `[a, b]`.
///
/// # Errors
/// Returns a domain error for invalid order or interval, or a convergence error
/// when the integral is non-finite.
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
    check_rule_size(n, n)?;
    if a >= b {
        return Ok(0.0);
    }
    let cell = RefCell::new(f);
    finite_integral(kquad::gauss_legendre(|x| (cell.borrow_mut())(x), a, b, n))
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
    if a >= b {
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
/// Returns a domain error for invalid order or interval, or a convergence error
/// when the integral is non-finite.
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
    // RSSN rounds odd panels up and includes both endpoints.
    let nodes = n.max(2).div_ceil(2).saturating_mul(2).saturating_add(1);
    check_rule_size(n, nodes)?;
    if a >= b {
        return Ok(0.0);
    }
    let cell = RefCell::new(f);
    finite_integral(kquad::clenshaw_curtis(|x| (cell.borrow_mut())(x), a, b, n))
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
    if a >= b {
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
    if a >= b {
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
    if a.is_nan()
        || b.is_nan()
        || (a.is_infinite() && a.is_sign_positive())
        || (b.is_infinite() && b.is_sign_negative())
        || b < a
    {
        return Err(NumericError::Domain(
            "improper quadrature requires a <= b and non-NaN bounds".to_string(),
        ));
    }
    if a >= b {
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
