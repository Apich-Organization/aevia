//! Compatibility adapters to RSSN reference dense algebra.
//! Sequential multiplication preserves the historical first-product accumulation
//! order; all numerical work resides in the RSSN kernel.
use crate::NumericError;
#[cfg(test)]
use nalgebra::DMatrix;
use rssn::kernels::dense::reference;

/// A row-major slice as a `nalgebra` matrix, checking the shape.
#[cfg(test)]
pub(crate) fn matrix(
    rows: usize,
    cols: usize,
    a: &[f64],
) -> Result<DMatrix<f64>, NumericError> {
    if rows * cols != a.len() {
        return Err(NumericError::ShapeMismatch(format!(
            "{rows}x{cols} needs {} elements, found {}",
            rows * cols,
            a.len()
        )));
    }
    Ok(DMatrix::from_row_slice(rows, cols, a))
}

/// Compatibility `det` through RSSN's reference algebra.
///
/// # Errors
/// Invalid dimensions or numerical factorization failure.
pub fn det(
    rows: usize,
    cols: usize,
    a: &[f64],
) -> Result<f64, NumericError> {
    if rows != cols {
        return Err(NumericError::NotSquare);
    }
    reference::det(rows, cols, a).map_err(super::faer_backend::error)
}

/// Compatibility `inv` through RSSN's reference algebra.
///
/// # Errors
/// Invalid dimensions or numerical factorization failure.
pub fn inv(
    rows: usize,
    cols: usize,
    a: &[f64],
) -> Result<Vec<f64>, NumericError> {
    if rows != cols {
        return Err(NumericError::NotSquare);
    }
    reference::inv(rows, cols, a).map_err(super::faer_backend::error)
}

/// Compatibility `solve` through RSSN's reference algebra.
///
/// # Errors
/// Invalid dimensions or numerical factorization failure.
pub fn solve(
    rows: usize,
    cols: usize,
    a: &[f64],
    b: &[f64],
) -> Result<Vec<f64>, NumericError> {
    if rows != cols {
        return Err(NumericError::NotSquare);
    }
    reference::solve(rows, cols, a, b).map_err(super::faer_backend::error)
}

/// Compatibility `lu` through RSSN's reference algebra.
///
/// # Errors
/// Invalid dimensions or numerical factorization failure.
pub fn lu(
    rows: usize,
    cols: usize,
    a: &[f64],
) -> Result<(Vec<f64>, Vec<f64>, Vec<f64>), NumericError> {
    if rows != cols {
        return Err(NumericError::NotSquare);
    }
    reference::lu(rows, cols, a).map_err(super::faer_backend::error)
}

/// Compatibility `qr` through RSSN's reference algebra.
///
/// # Errors
/// Invalid dimensions or numerical factorization failure.
pub fn qr(
    rows: usize,
    cols: usize,
    a: &[f64],
) -> Result<(Vec<f64>, Vec<f64>), NumericError> {
    reference::qr(rows, cols, a).map_err(super::faer_backend::error)
}

/// Sequential kernel product retaining the historical floating-point order.
#[must_use]
pub fn matmul(
    rows: usize,
    inner: usize,
    cols: usize,
    a: &[f64],
    b: &[f64],
) -> Vec<f64> {
    reference::matmul(rows, inner, cols, a, b)
}
