//! The `faer` backend for the `f64` linear-algebra kernels.
//!
//! `faer` is column-major and takes its own matrix type, so every entry point
//! copies the caller's row-major slice into a `Mat`. That copy is O(n²) against
//! an O(n³) kernel and is not the thing worth optimising.
//!
//! Everything here is `f64`. There is no generic element type and no strided
//! entry point: a non-contiguous operand cannot be expressed, so it cannot be
//! read with the wrong stride. A view is materialised by the caller before it
//! reaches this module.

use crate::NumericError;
use faer::Accum;
use faer::Mat;
use faer::get_global_parallelism;
use faer::linalg::solvers::PartialPivLu;
use faer::linalg::solvers::Qr;
use faer::linalg::solvers::Solve;

/// A row-major slice as a `faer` matrix, checking the shape.
pub(super) fn matrix(
    rows: usize,
    cols: usize,
    a: &[f64],
) -> Result<Mat<f64>, NumericError> {
    let expected = rows * cols;
    if expected != a.len() {
        return Err(NumericError::ShapeMismatch(format!(
            "{rows}x{cols} needs {expected} elements, found {}",
            a.len()
        )));
    }
    let mut out = Mat::<f64>::zeros(rows, cols);
    for row in 0..rows {
        for column in 0..cols {
            out[(row, column)] = a[row * cols + column];
        }
    }
    Ok(out)
}

/// A `faer` matrix as a row-major `Vec`.
fn to_row_major(matrix: faer::MatRef<'_, f64>) -> Vec<f64> {
    let (rows, cols) = matrix.shape();
    let mut out = vec![0.0; rows * cols];
    for row in 0..rows {
        for column in 0..cols {
            out[row * cols + column] = matrix[(row, column)];
        }
    }
    out
}

/// `true` when the `U` factor has a zero on its diagonal, i.e. the matrix the
/// factorisation was taken of is exactly singular.
///
/// An exactly singular matrix is the case the old path reported
/// [`NumericError::Singular`] for, and a nearly singular one is not: the
/// answer is large, not absent, and refusing it would be a semantic change.
fn is_singular(
    u: faer::MatRef<'_, f64>,
    n: usize,
) -> bool {
    (0..n).any(|index| u[(index, index)] == 0.0)
}

/// The determinant of a square matrix.
pub(super) fn det(
    rows: usize,
    cols: usize,
    a: &[f64],
) -> Result<f64, NumericError> {
    if rows != cols {
        return Err(NumericError::NotSquare);
    }
    Ok(matrix(rows, cols, a)?.as_ref().determinant())
}

/// The inverse of a square matrix.
pub(super) fn inv(
    rows: usize,
    cols: usize,
    a: &[f64],
) -> Result<Vec<f64>, NumericError> {
    if rows != cols {
        return Err(NumericError::NotSquare);
    }
    let matrix = matrix(rows, cols, a)?;
    let lu = PartialPivLu::new(matrix.as_ref());
    if is_singular(lu.U(), rows) {
        return Err(NumericError::Singular);
    }
    let mut inverse = Mat::<f64>::identity(rows, rows);
    lu.solve_in_place(inverse.as_mut());
    Ok(to_row_major(inverse.as_ref()))
}

/// `A x = b` for a square `A`.
pub(super) fn solve(
    rows: usize,
    cols: usize,
    a: &[f64],
    b: &[f64],
) -> Result<Vec<f64>, NumericError> {
    if rows != cols {
        return Err(NumericError::NotSquare);
    }
    let matrix = matrix(rows, cols, a)?;
    if b.len() != rows {
        return Err(NumericError::ShapeMismatch(format!(
            "expected {rows} entries, found {}",
            b.len()
        )));
    }
    let lu = PartialPivLu::new(matrix.as_ref());
    if is_singular(lu.U(), rows) {
        return Err(NumericError::Singular);
    }
    let mut x = Mat::<f64>::zeros(rows, 1);
    for (row, value) in b.iter().enumerate() {
        x[(row, 0)] = *value;
    }
    lu.solve_in_place(x.as_mut());
    Ok((0..rows).map(|row| x[(row, 0)]).collect())
}

/// `P A = L U` with partial pivoting.
///
/// `faer` reports the row permutation as a pair of index arrays; the `P` this
/// crate promises is the permutation *matrix*, so it is materialised from the
/// forward array as `P[i][forward[i]] = 1`, which is `(P A)[i] = A[forward[i]]`.
pub(super) fn lu(
    rows: usize,
    cols: usize,
    a: &[f64],
) -> Result<(Vec<f64>, Vec<f64>, Vec<f64>), NumericError> {
    if rows != cols {
        return Err(NumericError::NotSquare);
    }
    let matrix = matrix(rows, cols, a)?;
    let lu = PartialPivLu::new(matrix.as_ref());
    let (forward, _) = lu.P().arrays();
    let mut p = vec![0.0; rows * rows];
    for (row, &source) in forward.iter().enumerate() {
        p[row * rows + source] = 1.0;
    }
    Ok((p, to_row_major(lu.L()), to_row_major(lu.U())))
}

/// `A = Q R` with `Q` orthonormal and `rows × rows`.
pub(super) fn qr(
    rows: usize,
    cols: usize,
    a: &[f64],
) -> Result<(Vec<f64>, Vec<f64>), NumericError> {
    let matrix = matrix(rows, cols, a)?;
    let qr = Qr::new(matrix.as_ref());
    let q = qr.compute_Q();
    Ok((to_row_major(q.as_ref()), to_row_major(qr.R())))
}

/// `A B` for `A` of shape `rows × inner` and `B` of `inner × cols`.
pub(super) fn matmul(
    rows: usize,
    inner: usize,
    cols: usize,
    a: &[f64],
    b: &[f64],
) -> Result<Vec<f64>, NumericError> {
    let lhs = matrix(rows, inner, a)?;
    let rhs = matrix(inner, cols, b)?;
    let mut out = Mat::<f64>::zeros(rows, cols);
    faer::linalg::matmul::matmul(
        out.as_mut(),
        Accum::Replace,
        lhs.as_ref(),
        rhs.as_ref(),
        1.0,
        get_global_parallelism(),
    );
    Ok(to_row_major(out.as_ref()))
}
