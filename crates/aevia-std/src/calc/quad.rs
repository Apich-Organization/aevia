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
    check_interval_order(a, b)
}

fn check_interval_order(
    a: f64,
    b: f64,
) -> Result<(), NumericError> {
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
/// The RSSN rule accepts finite, ordered bounds even when their full width
/// overflows. The result and the error estimate must remain finite.
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
    if !a.is_finite() || !b.is_finite() {
        return Err(NumericError::Domain(
            "Gauss–Kronrod quadrature requires finite bounds".into(),
        ));
    }
    check_interval_order(a, b)?;
    if a >= b {
        return Ok(0.0);
    }
    let cell = RefCell::new(f);
    let res = kquad::integrate(|x| (cell.borrow_mut())(x), a, b, tol.max(1e-14));
    if res.converged && res.value.is_finite() && res.error.is_finite() {
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gauss_kronrod_uses_the_rssn_half_width_when_the_full_width_overflows() {
        let limit = 1.7e308_f64;
        let sample = 1e-308_f64;
        let mut calls = 0;
        let mut callback = |x: f64| {
            assert!(x.is_finite() && x > -limit && x < limit);
            calls += 1;
            sample
        };
        let value = quad_gauss_kronrod(&mut callback, -limit, limit, 1e-12).unwrap();
        assert!((value - (2.0 * sample) * limit).abs() < 1e-12);
        assert_eq!(calls, 15);

        let mut never = |_| panic!("invalid or empty interval sampled");
        assert!(quad_gauss_legendre(&mut never, -limit, limit, 4).is_err());
        assert!(quad_gauss_kronrod(&mut never, limit, -limit, 1e-12).is_err());
        let empty = quad_gauss_kronrod(&mut never, 3.0, 3.0, 1e-12).unwrap();
        assert_eq!(empty.to_bits(), 0);
    }

    #[test]
    fn adaptive_steps_match_the_native_rule_and_analytic_integral() {
        let step = |x: f64| if x < 0.375 { 1.0 } else { 3.0 };
        let mut adapted = Vec::new();
        let mut native = Vec::new();
        let value = quad_gauss_kronrod(
            &mut |x| {
                adapted.push(x.to_bits());
                step(x)
            },
            0.0,
            1.0,
            1e-12,
        )
        .unwrap();
        let nodes = RefCell::new(&mut native);
        let reference = kquad::integrate(
            |x| {
                nodes.borrow_mut().push(x.to_bits());
                step(x)
            },
            0.0,
            1.0,
            1e-12,
        );
        assert_eq!(value.to_bits(), reference.value.to_bits());
        assert!((value - 2.25).abs() < 1e-12);
        assert_eq!(adapted, native);
    }
}
