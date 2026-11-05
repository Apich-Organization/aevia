//! The previous implementations, kept reachable.
//!
//! `det`, `inv`, `solve`, `lu` and `qr` were `nalgebra` calls; `matmul` is the
//! hand-written triple loop the runtime's `@` operator runs today
//! (`crates/aevia-core/src/value.rs`, `dense_matmul`). They are still here for
//! three reasons:
//!
//! * a shape `faer` cannot express — an empty dimension, where `faer` asserts
//!   rather than answering — is delegated to them;
//! * they are the reference the parity tests compare against;
//! * `matmul` is the "before" number in the benchmark, and it is the
//!   accumulation order the boxed tensor path produces bit for bit, so it
//!   cannot be replaced by a blocked kernel without changing results.
//!
//! Everything here is `f64`-only and contiguous: the arguments are row-major
//! slices with an explicit shape, so there is no stride to get wrong.

use crate::NumericError;
use nalgebra::DMatrix;
use nalgebra::DVector;

/// A row-major slice as a `nalgebra` matrix, checking the shape.
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

/// A slice as a `nalgebra` column vector, checking the length.
pub(crate) fn vector(
    len: usize,
    b: &[f64],
) -> Result<DVector<f64>, NumericError> {
    if len != b.len() {
        return Err(NumericError::ShapeMismatch(format!(
            "expected {len} entries, found {}",
            b.len()
        )));
    }
    Ok(DVector::from_row_slice(b))
}

/// A matrix as a row-major `Vec`, which is what this crate promises.
pub(crate) fn to_row_major(m: &DMatrix<f64>) -> Vec<f64> {
    let mut out = vec![0.0; m.nrows() * m.ncols()];
    for row in 0..m.nrows() {
        for column in 0..m.ncols() {
            out[row * m.ncols() + column] = m[(row, column)];
        }
    }
    out
}

/// The determinant, by `nalgebra`'s LU.
pub fn det(
    rows: usize,
    cols: usize,
    a: &[f64],
) -> Result<f64, NumericError> {
    if rows != cols {
        return Err(NumericError::NotSquare);
    }
    Ok(matrix(rows, cols, a)?.determinant())
}

/// The inverse, by `nalgebra`'s LU.
pub fn inv(
    rows: usize,
    cols: usize,
    a: &[f64],
) -> Result<Vec<f64>, NumericError> {
    if rows != cols {
        return Err(NumericError::NotSquare);
    }
    let inverse = matrix(rows, cols, a)?
        .try_inverse()
        .ok_or(NumericError::Singular)?;
    Ok(to_row_major(&inverse))
}

/// `A x = b`, by `nalgebra`'s LU.
pub fn solve(
    rows: usize,
    cols: usize,
    a: &[f64],
    b: &[f64],
) -> Result<Vec<f64>, NumericError> {
    if rows != cols {
        return Err(NumericError::NotSquare);
    }
    let matrix = matrix(rows, cols, a)?;
    let rhs = vector(rows, b)?;
    let lu = matrix.lu();
    let solution = lu.solve(&rhs).ok_or(NumericError::Singular)?;
    Ok(solution.as_slice().to_vec())
}

/// `P A = L U`, by `nalgebra`'s LU.
pub fn lu(
    rows: usize,
    cols: usize,
    a: &[f64],
) -> Result<(Vec<f64>, Vec<f64>, Vec<f64>), NumericError> {
    if rows != cols {
        return Err(NumericError::NotSquare);
    }
    let matrix = matrix(rows, cols, a)?;
    let lu = matrix.lu();
    // `p` is a permutation *sequence*; the matrix it represents is the identity
    // with its rows permuted, which is the `P` of `P A = L U`.
    let mut p = DMatrix::<f64>::identity(rows, rows);
    lu.p().permute_rows(&mut p);
    let l = lu.l();
    let u = lu.u();
    let mut p_out = vec![0.0; rows * rows];
    for row in 0..rows {
        for column in 0..rows {
            p_out[row * rows + column] = p[(row, column)];
        }
    }
    let mut l_out = vec![0.0; rows * rows];
    for row in 0..rows {
        for column in 0..rows {
            l_out[row * rows + column] = l[(row, column)];
        }
    }
    let mut u_out = vec![0.0; rows * cols];
    for row in 0..rows {
        for column in 0..cols {
            u_out[row * cols + column] = u[(row, column)];
        }
    }
    Ok((p_out, l_out, u_out))
}

/// `A = Q R`, by `nalgebra`'s Householder QR.
pub fn qr(
    rows: usize,
    cols: usize,
    a: &[f64],
) -> Result<(Vec<f64>, Vec<f64>), NumericError> {
    let matrix = matrix(rows, cols, a)?;
    let qr = matrix.qr();
    let q = qr.q();
    let r = qr.r();
    let mut q_out = vec![0.0; rows * rows];
    for row in 0..rows {
        for column in 0..rows.min(q.ncols()) {
            q_out[row * rows + column] = q[(row, column)];
        }
    }
    let mut r_out = vec![0.0; rows * cols];
    for row in 0..rows.min(r.nrows()) {
        for column in 0..cols {
            r_out[row * cols + column] = r[(row, column)];
        }
    }
    Ok((q_out, r_out))
}

/// The hand-written `@` for `f64` matrices, exactly as the runtime runs it.
///
/// The accumulation order is the runtime's — the first product *is* the
/// accumulator, not `0.0 + product` — so this and
/// `TensorData::dense_matmul` agree bit for bit. That is also why it cannot be
/// replaced by a blocked kernel: a blocked sum reassociates.
#[must_use]
pub fn matmul(
    rows: usize,
    inner: usize,
    cols: usize,
    a: &[f64],
    b: &[f64],
) -> Vec<f64> {
    let mut data = vec![0.0; rows * cols];
    for row in 0..rows {
        for column in 0..cols {
            if inner == 0 {
                break;
            }
            let mut total = a[row * inner] * b[column];
            for k in 1..inner {
                total += a[row * inner + k] * b[k * cols + column];
            }
            data[row * cols + column] = total;
        }
    }
    data
}
