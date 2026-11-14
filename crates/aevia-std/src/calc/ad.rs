//! Automatic Differentiation (AD): forward-mode dual numbers (`Dual`) and
//! tape-based reverse-mode AD (`Tape`, `Var`), backed by `rssn::kernels::autodiff`.

use crate::NumericError;
use rssn::kernels::autodiff::AdError;
use rssn::kernels::autodiff::{
    self,
};

pub use rssn::kernels::autodiff::Dual;
pub use rssn::kernels::autodiff::Tape;
pub use rssn::kernels::autodiff::Var;

fn map_ad_err(err: AdError) -> NumericError {
    match err {
        | AdError::Empty => NumericError::Empty,
        | AdError::ForeignTape => {
            NumericError::Domain("AD result belongs to a different tape".into())
        },
        | AdError::ShapeMismatch { expected, got } => {
            NumericError::ShapeMismatch(format!(
                "ad_jacobian expected {expected} outputs, got {got}"
            ))
        },
    }
}

/// Exact first derivative of a scalar function `f(Dual) -> Dual` at `x` via
/// forward-mode automatic differentiation.
pub fn ad_derivative<F>(
    f: &mut F,
    x: f64,
) -> (f64, f64)
where
    F: FnMut(Dual) -> Dual,
{
    autodiff::forward_derivative(f, x)
}

/// Exact gradient of `f(&[Dual]) -> Dual` at `x` via forward-mode AD.
///
/// # Errors
/// [`NumericError::Empty`] when `x` is empty.
pub fn ad_grad<F>(
    f: &mut F,
    x: &[f64],
) -> Result<Vec<f64>, NumericError>
where
    F: FnMut(&[Dual]) -> Dual,
{
    autodiff::forward_grad(f, x).map_err(map_ad_err)
}

/// Exact Jacobian of `f(&[Dual]) -> Vec<Dual>` at `x` via forward-mode AD.
///
/// Returns row-major `m × n` Jacobian.
///
/// # Errors
/// [`NumericError::Empty`] when `x` is empty or `f` returns an empty vector.
pub fn ad_jacobian<F>(
    f: &mut F,
    x: &[f64],
) -> Result<Vec<f64>, NumericError>
where
    F: FnMut(&[Dual]) -> Vec<Dual>,
{
    autodiff::forward_jacobian(f, x).map_err(map_ad_err)
}

/// Exact gradient of `f(&[Var]) -> Var` at `x` in a single reverse-mode pass.
///
/// # Errors
/// [`NumericError::Empty`] when `x` is empty.
pub fn ad_reverse_grad<F>(
    f: &mut F,
    x: &[f64],
) -> Result<(f64, Vec<f64>), NumericError>
where
    F: FnMut(&[Var]) -> Var,
{
    autodiff::reverse_grad(f, x).map_err(map_ad_err)
}
