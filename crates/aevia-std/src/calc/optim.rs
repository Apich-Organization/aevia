//! Optimization kernels: thin adapters over `rssn::kernels::optim` and
//! `rssn::kernels::rootfind`.

use crate::NumericError;
use rssn::kernels::optim::LpStatus;
use rssn::kernels::optim::Relation;
use rssn::kernels::optim::{
    self,
};
use rssn::kernels::rootfind;
use std::cell::Cell;
use std::cell::RefCell;

fn check_controls(
    x: &[f64],
    tol: f64,
) -> Result<(), NumericError> {
    if !tol.is_finite() || tol <= 0.0 || x.iter().any(|v| !v.is_finite()) {
        return Err(NumericError::Domain(
            "initial values and positive tolerance must be finite".into(),
        ));
    }
    Ok(())
}

/// Quasi-Newton BFGS minimization of `f` with user-supplied gradient `grad_f`.
///
/// # Errors
/// [`NumericError::Empty`], [`NumericError::ShapeMismatch`], or
/// [`NumericError::NotConverged`].
pub fn minimize_bfgs<F, G>(
    f: &mut F,
    grad_f: &mut G,
    x0: &[f64],
    tol: f64,
    max_iter: usize,
) -> Result<Vec<f64>, NumericError>
where
    F: FnMut(&[f64]) -> f64,
    G: FnMut(&[f64]) -> Vec<f64>,
{
    check_controls(x0, tol)?;
    if x0.is_empty() {
        return Err(NumericError::Empty);
    }
    let g0 = grad_f(x0);
    if g0.len() != x0.len() {
        return Err(NumericError::ShapeMismatch(format!(
            "gradient length {} does not match dimension {}",
            g0.len(),
            x0.len()
        )));
    }
    let fc = RefCell::new(f);
    let gc = RefCell::new(grad_f);
    let invalid_gradient = Cell::new(false);
    let g_closure = |x: &[f64]| {
        let g = (gc.borrow_mut())(x);
        if g.len() != x.len() || g.iter().any(|v| !v.is_finite()) {
            invalid_gradient.set(true);
            vec![f64::NAN; x.len()]
        } else {
            g
        }
    };
    let res = optim::bfgs(
        |x| (fc.borrow_mut())(x),
        Some(&g_closure),
        x0,
        tol.max(1e-12),
        max_iter,
    );
    if invalid_gradient.get() {
        return Err(NumericError::ShapeMismatch(
            "gradient must retain its dimension and finite values".into(),
        ));
    }
    if res.converged && res.fx.is_finite() && res.x.iter().all(|v| v.is_finite()) {
        Ok(res.x)
    } else {
        Err(NumericError::NotConverged)
    }
}

/// Limited-memory BFGS (L-BFGS) minimization of `f` with history size `m`.
///
/// # Errors
/// [`NumericError::Empty`], [`NumericError::ShapeMismatch`], or
/// [`NumericError::NotConverged`].
pub fn minimize_lbfgs<F, G>(
    f: &mut F,
    grad_f: &mut G,
    x0: &[f64],
    m: usize,
    tol: f64,
    max_iter: usize,
) -> Result<Vec<f64>, NumericError>
where
    F: FnMut(&[f64]) -> f64,
    G: FnMut(&[f64]) -> Vec<f64>,
{
    check_controls(x0, tol)?;
    if x0.is_empty() {
        return Err(NumericError::Empty);
    }
    let g0 = grad_f(x0);
    if g0.len() != x0.len() {
        return Err(NumericError::ShapeMismatch(format!(
            "gradient length {} does not match dimension {}",
            g0.len(),
            x0.len()
        )));
    }
    let fc = RefCell::new(f);
    let gc = RefCell::new(grad_f);
    let invalid_gradient = Cell::new(false);
    let g_closure = |x: &[f64]| {
        let g = (gc.borrow_mut())(x);
        if g.len() != x.len() || g.iter().any(|v| !v.is_finite()) {
            invalid_gradient.set(true);
            vec![f64::NAN; x.len()]
        } else {
            g
        }
    };
    let res = optim::lbfgs(
        |x| (fc.borrow_mut())(x),
        Some(&g_closure),
        x0,
        m.max(1),
        tol.max(1e-12),
        max_iter,
    );
    if invalid_gradient.get() {
        return Err(NumericError::ShapeMismatch(
            "gradient must retain its dimension and finite values".into(),
        ));
    }
    if res.converged && res.fx.is_finite() && res.x.iter().all(|v| v.is_finite()) {
        Ok(res.x)
    } else {
        Err(NumericError::NotConverged)
    }
}

/// Nonlinear Conjugate Gradient (Polak–Ribière+ with automatic steepest-descent
/// restart) for smooth unconstrained minimization.
///
/// # Errors
/// [`NumericError::Empty`], [`NumericError::ShapeMismatch`], or
/// [`NumericError::NotConverged`].
pub fn minimize_cg<F, G>(
    f: &mut F,
    grad_f: &mut G,
    x0: &[f64],
    tol: f64,
    max_iter: usize,
) -> Result<Vec<f64>, NumericError>
where
    F: FnMut(&[f64]) -> f64,
    G: FnMut(&[f64]) -> Vec<f64>,
{
    check_controls(x0, tol)?;
    if x0.is_empty() {
        return Err(NumericError::Empty);
    }
    let g0 = grad_f(x0);
    if g0.len() != x0.len() {
        return Err(NumericError::ShapeMismatch(format!(
            "gradient length {} does not match dimension {}",
            g0.len(),
            x0.len()
        )));
    }
    let fc = RefCell::new(f);
    let gc = RefCell::new(grad_f);
    let invalid_gradient = Cell::new(false);
    let g_closure = |x: &[f64]| {
        let g = (gc.borrow_mut())(x);
        if g.len() != x.len() || g.iter().any(|v| !v.is_finite()) {
            invalid_gradient.set(true);
            vec![f64::NAN; x.len()]
        } else {
            g
        }
    };
    let res = optim::nonlinear_cg(
        |x| (fc.borrow_mut())(x),
        Some(&g_closure),
        x0,
        tol.max(1e-12),
        max_iter,
    );
    if invalid_gradient.get() {
        return Err(NumericError::ShapeMismatch(
            "gradient must retain its dimension and finite values".into(),
        ));
    }
    if res.converged && res.fx.is_finite() && res.x.iter().all(|v| v.is_finite()) {
        Ok(res.x)
    } else {
        Err(NumericError::NotConverged)
    }
}

/// Damped nonlinear least squares via Levenberg–Marquardt: minimizes `0.5 ||r(x)||^2`.
///
/// # Errors
/// [`NumericError::Empty`] or [`NumericError::NotConverged`].
pub fn least_squares_lm<F>(
    residuals: &mut F,
    x0: &[f64],
    tol: f64,
    max_iter: usize,
) -> Result<Vec<f64>, NumericError>
where
    F: FnMut(&[f64]) -> Vec<f64>,
{
    check_controls(x0, tol)?;
    if x0.is_empty() {
        return Err(NumericError::Empty);
    }
    let rc = RefCell::new(residuals);
    let sol = rootfind::levenberg_marquardt(|x| (rc.borrow_mut())(x), x0, tol.max(1e-12), max_iter)
        .map_err(|_| NumericError::NotConverged)?;
    Ok(sol.x)
}

/// Box-constrained minimization (`lower <= x <= upper`) using projected L-BFGS-B
/// style steps.
///
/// # Errors
/// [`NumericError::Empty`], [`NumericError::ShapeMismatch`], [`NumericError::Domain`]
/// when `lower[i] > upper[i]`, or [`NumericError::NotConverged`].
pub fn minimize_box<F, G>(
    f: &mut F,
    grad_f: &mut G,
    x0: &[f64],
    lower: &[f64],
    upper: &[f64],
    tol: f64,
    max_iter: usize,
) -> Result<Vec<f64>, NumericError>
where
    F: FnMut(&[f64]) -> f64,
    G: FnMut(&[f64]) -> Vec<f64>,
{
    check_controls(x0, tol)?;
    let n = x0.len();
    if n == 0 {
        return Err(NumericError::Empty);
    }
    if lower.len() != n || upper.len() != n {
        return Err(NumericError::ShapeMismatch(
            "bounds must match x0 length".to_string(),
        ));
    }
    for i in 0..n {
        if lower[i].is_nan() || upper[i].is_nan() || lower[i] > upper[i] {
            return Err(NumericError::Domain(format!("lower[{i}] > upper[{i}]")));
        }
    }
    let fc = RefCell::new(f);
    let gc = RefCell::new(grad_f);
    let invalid_gradient = Cell::new(false);
    let g_closure = |x: &[f64]| {
        let g = (gc.borrow_mut())(x);
        if g.len() != x.len() || g.iter().any(|v| !v.is_finite()) {
            invalid_gradient.set(true);
            vec![f64::NAN; x.len()]
        } else {
            g
        }
    };
    let res = optim::lbfgs_b(
        |x| (fc.borrow_mut())(x),
        Some(&g_closure),
        x0,
        lower,
        upper,
        10,
        tol.max(1e-12),
        max_iter,
    );
    if invalid_gradient.get() {
        return Err(NumericError::ShapeMismatch(
            "gradient must retain its dimension and finite values".into(),
        ));
    }
    if res.converged && res.fx.is_finite() && res.x.iter().all(|v| v.is_finite()) {
        Ok(res.x)
    } else {
        Err(NumericError::NotConverged)
    }
}

/// Solves a standard-form linear program via `rssn::kernels::optim::simplex`:
/// `minimize c^T x` subject to `A x <= b`, `x >= 0`, where `A` is `m × n` row-major
/// and `b >= 0`.
///
/// Returns `(x, optimal_value)`.
///
/// # Errors
/// [`NumericError::ShapeMismatch`], [`NumericError::Domain`] when `b` has negative
/// entries or the problem is unbounded, or [`NumericError::NotConverged`].
pub fn linprog(
    c: &[f64],
    a: &[f64],
    b: &[f64],
) -> Result<(Vec<f64>, f64), NumericError> {
    let m = b.len();
    let n = c.len();
    if n == 0 || m == 0 {
        return Err(NumericError::Empty);
    }
    if m.checked_mul(n) != Some(a.len()) {
        return Err(NumericError::ShapeMismatch(
            "linprog dimensions do not match".to_string(),
        ));
    }
    if c.iter().chain(a).chain(b).any(|v| !v.is_finite()) {
        return Err(NumericError::Domain(
            "linear program data must be finite".into(),
        ));
    }
    if b.iter().any(|&bi| bi < 0.0) {
        return Err(NumericError::Domain(
            "linprog requires b >= 0 in standard form".to_string(),
        ));
    }
    let rows: Vec<Vec<f64>> = (0..m).map(|i| a[i * n..(i + 1) * n].to_vec()).collect();
    let rels = vec![Relation::Le; m];
    match optim::simplex(c, &rows, &rels, b) {
        | LpStatus::Optimal { x, value } => Ok((x, value)),
        | LpStatus::Unbounded => Err(NumericError::Domain("linear program is unbounded".into())),
        | LpStatus::Infeasible => Err(NumericError::NotConverged),
    }
}
