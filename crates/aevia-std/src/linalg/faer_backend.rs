//! Adapters to RSSN's optimized dense kernels.
use crate::NumericError;
use rssn::kernels::dense::DenseError;
use rssn::kernels::dense::optimized;

pub(super) fn error(error: DenseError) -> NumericError {
    match error {
        | DenseError::Shape => {
            NumericError::ShapeMismatch("incompatible dense matrix dimensions".into())
        },
        | DenseError::Singular => NumericError::Singular,
        | DenseError::NoConvergence => NumericError::NotConverged,
        | DenseError::NonFinite => {
            NumericError::Domain("dense operation requires finite inputs".into())
        },
    }
}

pub(super) fn det(
    rows: usize,
    cols: usize,
    a: &[f64],
) -> Result<f64, NumericError> {
    optimized::det(rows, cols, a).map_err(error)
}
pub(super) fn inv(
    rows: usize,
    cols: usize,
    a: &[f64],
) -> Result<Vec<f64>, NumericError> {
    optimized::inv(rows, cols, a).map_err(error)
}
pub(super) fn solve(
    rows: usize,
    cols: usize,
    a: &[f64],
    b: &[f64],
) -> Result<Vec<f64>, NumericError> {
    optimized::solve(rows, cols, a, b).map_err(error)
}
pub(super) fn lu(
    rows: usize,
    cols: usize,
    a: &[f64],
) -> Result<(Vec<f64>, Vec<f64>, Vec<f64>), NumericError> {
    optimized::lu(rows, cols, a).map_err(error)
}
pub(super) fn qr(
    rows: usize,
    cols: usize,
    a: &[f64],
) -> Result<(Vec<f64>, Vec<f64>), NumericError> {
    optimized::qr(rows, cols, a).map_err(error)
}
pub(super) fn matmul(
    rows: usize,
    inner: usize,
    cols: usize,
    a: &[f64],
    b: &[f64],
) -> Result<Vec<f64>, NumericError> {
    optimized::matmul(rows, inner, cols, a, b).map_err(error)
}
