//! Dense linear algebra (§8.4).
//!
//! Matrices are row-major `&[f64]` with an explicit row and column count.
//!
//! Floating-point arithmetic and factorizations run in [`rssn::kernels::dense`].
//! This module validates and marshals Aevia's shapes and complex values. The
//! optimized determinant/solve/product family retains its blocked faer backend
//! in RSSN; eigendecompositions and SVD use RSSN's shared implementations.
//!
//! Where exactness matters the path does not come near either float backend:
//! `lu_exact` and `qr_exact` run over `BigRational` so an exact matrix keeps an
//! exact decomposition, which is the promise `Scalar<ρ,d>` makes when `ρ` is
//! `Real`. The rational adapters delegate to RSSN's exact dense module;
//! floating-point adapters accept only `&[f64]`.
//!
//! `svd` returns economy factors. For `k = min(rows, cols)`, `u` is `rows × k`,
//! `s` has `k` entries and `vt` is `k × cols`. Their product `u diag(s) vt`
//! reconstructs the matrix up to floating-point error.

use crate::Complex;
use crate::NumericError;
use num_rational::BigRational;
#[cfg(test)]
use num_traits::One;
use num_traits::Zero;
use std::alloc::Layout;

pub use rssn::kernels::contractions::ContractionError;
pub use rssn::kernels::contractions::contract_rectangular;

mod extended;
pub mod legacy;

pub use extended::MatrixNorm;
pub use extended::cond;
pub use extended::cond_norm;
pub use extended::eigvals_generalized;
pub use extended::expm;
pub use extended::logm;
pub use extended::lyapunov;
pub use extended::norm;
pub use extended::pinv;
pub use extended::pinv_tol;
pub use extended::qr_pivoted;
pub use extended::qz;
pub use extended::schur;
pub use extended::sqrtm;
pub use extended::sylvester;

#[cfg(test)]
use legacy::matrix;
use rssn::kernels::dense;
use rssn::kernels::dense::optimized;

fn dense_error(error: dense::DenseError) -> NumericError {
    match error {
        | dense::DenseError::Cancelled => NumericError::Cancelled,
        | dense::DenseError::Shape => {
            NumericError::ShapeMismatch("incompatible dense matrix dimensions".into())
        },
        | dense::DenseError::Singular => NumericError::Singular,
        | dense::DenseError::NoConvergence => NumericError::NotConverged,
        | dense::DenseError::NonFinite => {
            NumericError::Domain("dense operation requires finite inputs".into())
        },
    }
}

fn check_storage<T>(
    rows: usize,
    cols: usize,
    matrix: &[T],
) -> Result<(), NumericError> {
    if rows.checked_mul(cols) == Some(matrix.len()) {
        Ok(())
    } else {
        Err(NumericError::ShapeMismatch(
            "matrix shape does not match storage".into(),
        ))
    }
}

fn output_len<T>(
    rows: usize,
    cols: usize,
) -> Result<usize, NumericError> {
    rows.checked_mul(cols)
        .filter(|count| Layout::array::<T>(*count).is_ok())
        .ok_or_else(|| {
            NumericError::ShapeMismatch(format!("{rows}x{cols} result storage is too large"))
        })
}

fn filled<T: Clone>(
    count: usize,
    value: T,
) -> Result<Vec<T>, NumericError> {
    let mut out = Vec::new();
    out.try_reserve_exact(count)
        .map_err(|_| NumericError::ShapeMismatch("dense result storage is too large".into()))?;
    out.resize(count, value);
    Ok(out)
}

fn check_float_input(
    rows: usize,
    cols: usize,
    a: &[f64],
) -> Result<(), NumericError> {
    check_storage(rows, cols, a)?;
    if a.iter().any(|v| !v.is_finite()) {
        return Err(NumericError::Domain(
            "matrix contains non-finite entries".into(),
        ));
    }
    Ok(())
}

fn kernel_matrix(
    rows: usize,
    cols: usize,
    a: &[f64],
) -> Result<dense::Mat, NumericError> {
    check_float_input(rows, cols, a)?;
    Ok(dense::Mat {
        rows,
        cols,
        data: a.to_vec(),
    })
}

/// The determinant of a square matrix.
///
/// # Errors
///
/// [`NumericError::NotSquare`] for a non-square matrix, [`NumericError::ShapeMismatch`]
/// for inconsistent storage, or [`NumericError::Domain`] for non-finite entries.
pub fn det(
    rows: usize,
    cols: usize,
    a: &[f64],
) -> Result<f64, NumericError> {
    if rows != cols {
        return Err(NumericError::NotSquare);
    }
    optimized::det(rows, cols, a).map_err(dense_error)
}

/// The inverse of a square matrix.
///
/// # Errors
///
/// [`NumericError::Singular`] when the matrix has no inverse. The same shape
/// and finite-input checks as [`det`] apply.
pub fn inv(
    rows: usize,
    cols: usize,
    a: &[f64],
) -> Result<Vec<f64>, NumericError> {
    if rows != cols {
        return Err(NumericError::NotSquare);
    }
    optimized::inv(rows, cols, a).map_err(dense_error)
}

/// Solves `A x = b` for a square `A`.
///
/// # Errors
///
/// [`NumericError::Singular`] when the system has no unique solution. The
/// shape and finite-input checks apply to both operands.
pub fn solve(
    rows: usize,
    cols: usize,
    a: &[f64],
    b: &[f64],
) -> Result<Vec<f64>, NumericError> {
    if rows != cols {
        return Err(NumericError::NotSquare);
    }
    optimized::solve(rows, cols, a, b).map_err(dense_error)
}

/// The product of a `rows × inner` matrix and an `inner × cols` one.
///
/// The dense matrix `@` fast path uses this blocked `faer` product.
/// [`legacy::matmul`] preserves sequential floating-point accumulation when the
/// accumulation order matters. A zero inner extent produces a zero-filled
/// matrix, and a zero output extent produces an empty buffer.
///
/// # Errors
///
/// [`NumericError::ShapeMismatch`] for inconsistent or unrepresentable storage,
/// and [`NumericError::Domain`] for non-finite entries.
pub fn matmul(
    rows: usize,
    inner: usize,
    cols: usize,
    a: &[f64],
    b: &[f64],
) -> Result<Vec<f64>, NumericError> {
    let count = output_len::<f64>(rows, cols)?;
    if rows == 0 || inner == 0 || cols == 0 {
        check_float_input(rows, inner, a)?;
        check_float_input(inner, cols, b)?;
        return filled(count, 0.0);
    }
    optimized::matmul(rows, inner, cols, a, b).map_err(dense_error)
}

/// The minimum-norm least-squares solution of `A x = b` (§8.4).
///
/// The thin SVD selects the minimum-norm solution for rectangular or
/// rank-deficient systems. Singular values at or below `1e-12` are discarded.
///
/// # Errors
///
/// Inconsistent or unrepresentable storage, non-finite input, or failure of
/// the factorization to converge.
pub fn lstsq(
    rows: usize,
    cols: usize,
    a: &[f64],
    b: &[f64],
) -> Result<Vec<f64>, NumericError> {
    let count = output_len::<f64>(cols, 1)?;
    if rows == 0 || cols == 0 {
        check_float_input(rows, cols, a)?;
        check_float_input(rows, 1, b)?;
        return filled(count, 0.0);
    }
    optimized::lstsq(rows, cols, a, b).map_err(dense_error)
}

/// The eigenvalues and eigenvectors of a square matrix.
///
/// The eigenvectors are the columns of the returned matrix: the result is
/// `(values, vectors)` with `vectors` row-major `n × n`, and column `j` is the
/// eigenvector of `values[j]`.
///
/// # Errors
///
/// [`NumericError::NotSquare`] when the matrix is not square, and
/// [`NumericError::NotConverged`] when the eigenvalue iteration fails.
pub fn eig(
    rows: usize,
    cols: usize,
    a: &[f64],
) -> Result<(Vec<Complex>, Vec<Complex>), NumericError> {
    if rows != cols {
        return Err(NumericError::NotSquare);
    }
    let count = output_len::<Complex>(rows, cols)?;
    let result = dense::eigen(&kernel_matrix(rows, cols, a)?).map_err(dense_error)?;
    // Marshal RSSN's per-column eigenvectors into Aevia's row-major matrix.
    if result.values.len() != rows
        || result.vectors.len() != cols
        || result.vectors.iter().any(|vector| vector.len() != rows)
    {
        return Err(NumericError::ShapeMismatch(
            "unexpected eigenvector matrix shape".into(),
        ));
    }
    let mut vectors = filled(count, Complex::default())?;
    for (column, vector) in result.vectors.into_iter().enumerate() {
        for (row, value) in vectors.chunks_exact_mut(cols.max(1)).zip(vector) {
            let cell = row.get_mut(column).ok_or(NumericError::NotConverged)?;
            *cell = Complex::new(value.re, value.im);
        }
    }
    let values = result
        .values
        .into_iter()
        .map(|(re, im)| Complex::new(re, im))
        .collect();
    Ok((values, vectors))
}

/// The eigenvalues of a square matrix.
///
/// # Errors
///
/// [`NumericError::NotSquare`] when the matrix is not square.
pub fn eigvals(
    rows: usize,
    cols: usize,
    a: &[f64],
) -> Result<Vec<Complex>, NumericError> {
    if rows != cols {
        return Err(NumericError::NotSquare);
    }
    Ok(dense::eigenvalues(&kernel_matrix(rows, cols, a)?)
        .map_err(dense_error)?
        .into_iter()
        .map(|(re, im)| Complex::new(re, im))
        .collect())
}

/// The economy singular value decomposition (§8.4).
///
/// Returns `(u, s, vt)` with `u` of shape `rows × k`, `s` of length `k` and
/// `vt` of shape `k × cols`, where `k = min(rows, cols)`.
///
/// # Errors
///
/// [`NumericError::NotConverged`] when the iteration fails.
pub fn svd(
    rows: usize,
    cols: usize,
    a: &[f64],
) -> Result<(Vec<f64>, Vec<f64>, Vec<f64>), NumericError> {
    if rows == 0 || cols == 0 {
        check_storage(rows, cols, a)?;
        return Ok((Vec::new(), Vec::new(), Vec::new()));
    }
    let factors = dense::svd(&kernel_matrix(rows, cols, a)?).map_err(dense_error)?;
    Ok((factors.u.data, factors.s, factors.v.transpose().data))
}

/// The QR decomposition `A = Q R` with `Q` orthonormal.
///
/// `Q` is `rows × rows` and `R` is `rows × cols`, so `Q R` reproduces `A` for
/// any shape.
///
/// # Errors
///
/// [`NumericError::ShapeMismatch`] for inconsistent or unrepresentable
/// storage, and [`NumericError::Domain`] for non-finite entries.
pub fn qr(
    rows: usize,
    cols: usize,
    a: &[f64],
) -> Result<(Vec<f64>, Vec<f64>), NumericError> {
    let count = output_len::<f64>(rows, rows)?;
    if rows == 0 || cols == 0 {
        return empty_qr(rows, cols, a, count);
    }
    optimized::qr(rows, cols, a).map_err(dense_error)
}

fn empty_qr(
    rows: usize,
    cols: usize,
    a: &[f64],
    count: usize,
) -> Result<(Vec<f64>, Vec<f64>), NumericError> {
    check_storage(rows, cols, a)?;
    let mut q = filled(count, 0.0)?;
    for (index, row) in q.chunks_exact_mut(rows.max(1)).enumerate() {
        if let Some(diagonal) = row.get_mut(index) {
            *diagonal = 1.0;
        }
    }
    Ok((q, Vec::new()))
}

/// The LU decomposition with partial pivoting: `P A = L U`.
///
/// # Errors
///
/// [`NumericError::NotSquare`] when the matrix is not square.
pub fn lu(
    rows: usize,
    cols: usize,
    a: &[f64],
) -> Result<(Vec<f64>, Vec<f64>, Vec<f64>), NumericError> {
    if rows != cols {
        return Err(NumericError::NotSquare);
    }
    optimized::lu(rows, cols, a).map_err(dense_error)
}

/// The Cholesky factor `L` of the symmetric matrix represented by the input's
/// lower triangle. For symmetric `A`, `A = L Lᵀ`.
///
/// # Errors
///
/// [`NumericError::NotPositiveDefinite`] when the lower triangle represents a
/// matrix that is not positive definite. The input must be square and finite.
pub fn cholesky(
    rows: usize,
    cols: usize,
    a: &[f64],
) -> Result<Vec<f64>, NumericError> {
    if rows != cols {
        return Err(NumericError::NotSquare);
    }
    dense::cholesky(&kernel_matrix(rows, cols, a)?)
        .map(|factor| factor.data)
        .map_err(|error| {
            match error {
                | dense::DenseError::Singular => NumericError::NotPositiveDefinite,
                | other => dense_error(other),
            }
        })
}

/// The lower triangle of a matrix, with the remaining entries zeroed.
/// Inconsistent dimensions return an empty buffer.
#[must_use]
pub fn tril(
    rows: usize,
    cols: usize,
    a: &[f64],
) -> Vec<f64> {
    if a.is_empty() || check_storage(rows, cols, a).is_err() {
        return Vec::new();
    }
    dense::reference::tril(rows, cols, a)
}

/// The upper triangle of a matrix, with the remaining entries zeroed.
/// Inconsistent dimensions return an empty buffer.
#[must_use]
pub fn triu(
    rows: usize,
    cols: usize,
    a: &[f64],
) -> Vec<f64> {
    if a.is_empty() || check_storage(rows, cols, a).is_err() {
        return Vec::new();
    }
    dense::reference::triu(rows, cols, a)
}

// ----- exact decompositions ------------------------------------------------

/// An exact LU decomposition with partial pivoting: `P A = L U`.
///
/// RSSN owns the exact rational arithmetic and returns valid factors even for
/// singular matrices with zero pivots.
///
/// # Errors
///
/// [`NumericError::ShapeMismatch`] for inconsistent or overflowing dimensions.
pub fn lu_exact(
    n: usize,
    a: &[BigRational],
) -> Result<(Vec<BigRational>, Vec<BigRational>, Vec<BigRational>), NumericError> {
    dense::exact::lu_exact(n, a).map_err(exact_error)
}

/// Exact rational QR in row-major storage, with `Q` of shape `rows × rows`
/// and `R` of shape `rows × cols`. Nonzero columns of `Q` are orthonormal.
/// Unused columns of `Q` are zero.
///
/// # Errors
/// Invalid dimensions or a norm requiring irrational arithmetic.
pub fn qr_exact(
    rows: usize,
    cols: usize,
    a: &[BigRational],
) -> Result<(Vec<BigRational>, Vec<BigRational>), NumericError> {
    let count = output_len::<BigRational>(rows, rows)?;
    if rows == 0 || cols == 0 {
        check_storage(rows, cols, a)?;
        return Ok((filled(count, BigRational::zero())?, Vec::new()));
    }
    dense::exact::qr_exact(rows, cols, a).map_err(exact_error)
}

/// The exact square root of a non-negative rational, when it has one.
#[must_use]
pub fn exact_sqrt(value: &BigRational) -> Option<BigRational> {
    dense::exact::exact_sqrt(value)
}

fn exact_error(error: dense::exact::ExactError) -> NumericError {
    match error {
        | dense::exact::ExactError::Shape => NumericError::ShapeMismatch(error.to_string()),
        | dense::exact::ExactError::IrrationalNorm => NumericError::Domain(error.to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn close(
        a: f64,
        b: f64,
    ) -> bool {
        (a - b).abs() < 1e-9
    }

    /// A deterministic pseudo-random matrix, so a parity test covers a shape
    /// and a value distribution rather than one hand-picked case.
    fn pseudo_random(
        rows: usize,
        cols: usize,
        seed: u64,
    ) -> Vec<f64> {
        let mut state = seed.wrapping_mul(6_364_136_223_846_793_005).wrapping_add(1);
        (0..rows * cols)
            .map(|_| {
                state = state
                    .wrapping_mul(6_364_136_223_846_793_005)
                    .wrapping_add(1_442_695_040_888_963_407);
                let unit = ((state >> 11) as f64) / ((1u64 << 53) as f64);
                // Spread over [-4, 4) and keep every entry away from zero, so
                // the matrices are well conditioned but not trivially so.
                unit * 8.0 - 4.0 + 0.25
            })
            .collect()
    }

    #[test]
    fn empty_inputs_do_not_hide_overflowing_output_shapes() {
        assert!(matmul(usize::MAX, 0, 2, &[], &[]).is_err());
        assert!(qr(usize::MAX, 0, &[]).is_err());
    }

    #[test]
    fn empty_inputs_check_output_byte_capacity() {
        let max_floats = isize::MAX.unsigned_abs() / size_of::<f64>();
        let too_many_rows = 1 << (usize::BITS / 2 - 1);
        for result in [
            matmul(max_floats + 1, 0, 1, &[], &[]).map(|_| ()),
            lstsq(0, max_floats + 1, &[], &[]).map(|_| ()),
            qr(too_many_rows, 0, &[]).map(|_| ()),
            qr_exact(too_many_rows, 0, &[]).map(|_| ()),
            legacy::qr(too_many_rows, 0, &[]).map(|_| ()),
        ] {
            assert!(matches!(result, Err(NumericError::ShapeMismatch(_))));
        }
    }

    #[test]
    fn empty_outputs_skip_unused_extents() {
        assert_eq!(matmul(0, usize::MAX, 0, &[], &[]), Ok(vec![]));
        assert_eq!(svd(0, usize::MAX, &[]), Ok((vec![], vec![], vec![])));
        assert_eq!(qr_exact(0, usize::MAX, &[]), Ok((vec![], vec![])));
        assert_eq!(qr(2, 0, &[]), Ok((vec![1.0, 0.0, 0.0, 1.0], vec![])));
        assert_eq!(lstsq(0, 3, &[], &[]), Ok(vec![0.0; 3]));
        assert!(tril(usize::MAX, 0, &[]).is_empty());
    }

    #[test]
    fn compatibility_buffers_reject_inconsistent_storage() {
        assert!(tril(2, 2, &[1.0]).is_empty());
        assert!(triu(2, 2, &[1.0]).is_empty());
        assert!(legacy::matmul(2, 2, 2, &[1.0], &[1.0; 4]).is_empty());
    }

    #[test]
    fn determinant_of_a_two_by_two_is_exact_in_floats() {
        assert!(close(det(2, 2, &[1.0, 2.0, 3.0, 4.0]).unwrap(), -2.0));
    }

    #[test]
    fn inverse_multiplies_back_to_the_identity() {
        let a = [4.0, 7.0, 2.0, 6.0];
        let inverse = inv(2, 2, &a).unwrap();
        let product = matrix(2, 2, &a).unwrap() * matrix(2, 2, &inverse).unwrap();
        for (row, column) in [(0, 0), (0, 1), (1, 0), (1, 1)] {
            let expected = if row == column { 1.0 } else { 0.0 };
            assert!(close(product[(row, column)], expected));
        }
    }

    #[test]
    fn a_singular_matrix_has_no_inverse() {
        assert_eq!(
            inv(2, 2, &[1.0, 2.0, 2.0, 4.0]),
            Err(NumericError::Singular)
        );
    }

    #[test]
    fn qr_reproduces_the_matrix() {
        let a = [12.0, -51.0, 4.0, 6.0, 167.0, -68.0, -4.0, 24.0, -41.0];
        let (q, r) = qr(3, 3, &a).unwrap();
        let product = matrix(3, 3, &q).unwrap() * matrix(3, 3, &r).unwrap();
        for row in 0..3 {
            for column in 0..3 {
                assert!(
                    close(product[(row, column)], a[row * 3 + column]),
                    "at ({row}, {column})"
                );
            }
        }
    }

    #[test]
    fn lu_reconstructs_through_the_permutation() {
        let a = [0.0, 5.0, 2.0, 1.0, 3.0, 7.0, 4.0, 6.0, 8.0];
        let (p, l, u) = lu(3, 3, &a).unwrap();
        let left = matrix(3, 3, &p).unwrap() * matrix(3, 3, &a).unwrap();
        let right = matrix(3, 3, &l).unwrap() * matrix(3, 3, &u).unwrap();
        for index in 0..9 {
            assert!(close(left.as_slice()[index], right.as_slice()[index]));
        }
    }

    #[test]
    fn cholesky_of_a_positive_definite_matrix() {
        let a = [4.0, 12.0, -16.0, 12.0, 37.0, -43.0, -16.0, -43.0, 98.0];
        let l = cholesky(3, 3, &a).unwrap();
        let product = matrix(3, 3, &l).unwrap() * matrix(3, 3, &l).unwrap().transpose();
        for row in 0..3 {
            for column in 0..3 {
                assert!(close(product[(row, column)], a[row * 3 + column]));
            }
        }
        assert_eq!(
            cholesky(2, 2, &[1.0, 2.0, 2.0, 1.0]),
            Err(NumericError::NotPositiveDefinite)
        );
    }

    #[test]
    fn svd_factors_a_rectangular_matrix() {
        let a = [1.0, 2.0, 3.0, 4.0, 5.0, 6.0];
        let (u, s, vt) = svd(2, 3, &a).unwrap();
        assert_eq!(u.len(), 2 * 2);
        assert_eq!(s.len(), 2);
        assert_eq!(vt.len(), 2 * 3);
        // `u diag(s) vt` is `a`.
        let mut rebuilt = [0.0; 6];
        for row in 0..2 {
            for column in 0..3 {
                let mut total = 0.0;
                for inner in 0..2 {
                    total += u[row * 2 + inner] * s[inner] * vt[inner * 3 + column];
                }
                rebuilt[row * 3 + column] = total;
            }
        }
        for index in 0..6 {
            assert!(close(rebuilt[index], a[index]), "at {index}");
        }
        assert!(s[0] > s[1]);
    }

    #[test]
    fn eigenvalues_of_a_symmetric_matrix_are_real() {
        let a = [2.0, 1.0, 1.0, 2.0];
        let values = eigvals(2, 2, &a).unwrap();
        let mut reals: Vec<f64> = values.iter().map(|value| value.re).collect();
        reals.sort_by(f64::total_cmp);
        assert!(close(reals[0], 1.0));
        assert!(close(reals[1], 3.0));
        assert!(values.iter().all(|value| value.im.abs() < 1e-12));
    }

    #[test]
    fn a_rotation_has_complex_eigenvalues() {
        let a = [0.0, -1.0, 1.0, 0.0];
        let values = eigvals(2, 2, &a).unwrap();
        assert!(values.iter().any(|value| close(value.im, 1.0)));
        assert!(values.iter().any(|value| close(value.im, -1.0)));
    }

    #[test]
    fn an_eigenvector_satisfies_the_eigen_equation() {
        let a = [2.0, 1.0, 1.0, 2.0];
        let (values, vectors) = eig(2, 2, &a).unwrap();
        for (column, value) in values.iter().enumerate() {
            for row in 0..2 {
                let mut left = Complex::default();
                for inner in 0..2 {
                    left.re += a[row * 2 + inner] * vectors[inner * 2 + column].re;
                    left.im += a[row * 2 + inner] * vectors[inner * 2 + column].im;
                }
                let right = value.mul(vectors[row * 2 + column]);
                assert!((left.re - right.re).abs() < 1e-9);
                assert!((left.im - right.im).abs() < 1e-9);
            }
        }
    }

    #[test]
    fn least_squares_fits_a_line() {
        // y = 1 + 2x at x = 0, 1, 2.
        let a = [1.0, 0.0, 1.0, 1.0, 1.0, 2.0];
        let b = [1.0, 3.0, 5.0];
        let solution = lstsq(3, 2, &a, &b).unwrap();
        assert!(close(solution[0], 1.0));
        assert!(close(solution[1], 2.0));
    }

    #[test]
    fn exact_lu_and_qr_stay_rational() {
        let a: Vec<BigRational> = [1, 2, 3, 4]
            .iter()
            .map(|value| BigRational::from_integer((*value).into()))
            .collect();
        let (_, l, u) = lu_exact(2, &a).unwrap();
        assert_eq!(l[2], BigRational::from_integer(3.into()));
        assert_eq!(u[3], BigRational::from_integer((-2).into()));

        // A Pythagorean triple: the QR of [[3,0],[0,4]] is itself.
        let b: Vec<BigRational> = [3, 0, 0, 4]
            .iter()
            .map(|value| BigRational::from_integer((*value).into()))
            .collect();
        let (q, r) = qr_exact(2, 2, &b).unwrap();
        assert_eq!(q[0], BigRational::one());
        assert_eq!(r[3], BigRational::from_integer(4.into()));
    }

    #[test]
    fn exact_sqrt_finds_perfect_squares_only() {
        assert_eq!(
            exact_sqrt(&BigRational::new(9.into(), 4.into())),
            Some(BigRational::new(3.into(), 2.into()))
        );
        assert_eq!(exact_sqrt(&BigRational::from_integer(2.into())), None);
    }

    #[test]
    fn exact_kernel_handles_dependent_wide_columns_and_large_squares() {
        let data: Vec<_> = [0, 3, 0, 0, 0, 4]
            .into_iter()
            .map(|value| BigRational::from_integer(value.into()))
            .collect();
        let (q, r) = qr_exact(2, 3, &data).unwrap();
        for row in 0..2 {
            for column in 0..3 {
                let rebuilt = (0..2)
                    .map(|k| &q[row * 2 + k] * &r[k * 3 + column])
                    .fold(BigRational::from_integer(0.into()), |a, b| a + b);
                assert_eq!(rebuilt, data[row * 3 + column]);
            }
        }
        let root = num_bigint::BigInt::one() << 200;
        assert_eq!(
            exact_sqrt(&BigRational::from_integer(&root * &root)),
            Some(BigRational::from_integer(root))
        );
        assert!(lu_exact(usize::MAX, &[]).is_err());
        assert!(qr_exact(usize::MAX, 0, &[]).is_err());
    }

    #[test]
    fn reference_kernel_retains_signed_zero_and_sequential_accumulation() {
        let result = legacy::matmul(1, 1, 1, &[-0.0], &[1.0]);
        assert_eq!(result[0].to_bits(), (-0.0_f64).to_bits());
        assert_eq!(
            legacy::matmul(1, 3, 1, &[1e16, 1.0, -1e16], &[1.0; 3]),
            vec![0.0]
        );
        assert_eq!(legacy::qr(3, 0, &[]), Ok((vec![0.0; 9], vec![])));
    }

    // --- the `faer` backend against the implementation it replaced ---------

    #[test]
    fn matmul_agrees_with_the_hand_written_loop() {
        // The blocked kernel reassociates the inner sum, so the two agree to a
        // relative tolerance rather than bit for bit; that tolerance is
        // documented on `matmul` and is what a float kernel can promise.
        let a = pseudo_random(24, 19, 1);
        let b = pseudo_random(19, 17, 2);
        let fast = matmul(24, 19, 17, &a, &b).unwrap();
        let reference = legacy::matmul(24, 19, 17, &a, &b);
        for index in 0..fast.len() {
            let scale = reference[index].abs().max(1.0);
            assert!(
                (fast[index] - reference[index]).abs() <= 1e-12 * scale,
                "at {index}: {} vs {}",
                fast[index],
                reference[index]
            );
        }
    }

    #[test]
    fn matmul_is_exact_on_an_exactly_representable_product() {
        // Every partial sum here is an integer below 2^53, so the blocked
        // kernel and the loop must agree *exactly*: a reassociation that
        // changed a value would be a wrong answer, not a rounding.
        let a = [1.0, 2.0, 3.0, 4.0, 5.0, 6.0];
        let b = [7.0, 8.0, 9.0, 10.0, 11.0, 12.0];
        let product = matmul(2, 3, 2, &a, &b).unwrap();
        assert_eq!(product, [58.0, 64.0, 139.0, 154.0]);
        assert_eq!(product, legacy::matmul(2, 3, 2, &a, &b));
    }

    #[test]
    fn the_factorisations_agree_with_the_old_implementations() {
        let a = pseudo_random(12, 12, 3);
        let b = pseudo_random(12, 1, 4);

        let old_det = legacy::det(12, 12, &a).unwrap();
        let new_det = det(12, 12, &a).unwrap();
        assert!(
            (new_det - old_det).abs() <= 1e-9 * old_det.abs().max(1.0),
            "det: {new_det} vs {old_det}"
        );

        let old_inverse = legacy::inv(12, 12, &a).unwrap();
        let new_inverse = inv(12, 12, &a).unwrap();
        for index in 0..old_inverse.len() {
            assert!(
                (new_inverse[index] - old_inverse[index]).abs()
                    <= 1e-9 * old_inverse[index].abs().max(1.0),
                "inv at {index}"
            );
        }

        let old_solution = legacy::solve(12, 12, &a, &b).unwrap();
        let new_solution = solve(12, 12, &a, &b).unwrap();
        for index in 0..old_solution.len() {
            assert!(
                (new_solution[index] - old_solution[index]).abs()
                    <= 1e-9 * old_solution[index].abs().max(1.0),
                "solve at {index}"
            );
        }

        // `lu` and `qr` are compared through what they promise, not entry by
        // entry: the factors of one pivoting strategy are not the other's.
        let (p, l, u) = lu(12, 12, &a).unwrap();
        let left = matrix(12, 12, &p).unwrap() * matrix(12, 12, &a).unwrap();
        let right = matrix(12, 12, &l).unwrap() * matrix(12, 12, &u).unwrap();
        for index in 0..left.len() {
            assert!(
                (left.as_slice()[index] - right.as_slice()[index]).abs() <= 1e-9,
                "lu at {index}"
            );
        }

        let (q, r) = qr(12, 12, &a).unwrap();
        let product = matrix(12, 12, &q).unwrap() * matrix(12, 12, &r).unwrap();
        for row in 0..12 {
            for column in 0..12 {
                assert!(
                    (product[(row, column)] - a[row * 12 + column]).abs() <= 1e-9,
                    "qr at ({row}, {column})"
                );
            }
        }
    }

    #[test]
    fn an_empty_matrix_is_still_answered() {
        // The RSSN optimized adapter handles zero dimensions before invoking
        // a faer factorization.
        assert_eq!(det(0, 0, &[]), Ok(1.0));
        assert_eq!(inv(0, 0, &[]), Ok(Vec::new()));
        assert_eq!(
            lu(0, 0, &[]).map(|(p, l, u)| (p.len(), l.len(), u.len())),
            Ok((0, 0, 0))
        );
        assert_eq!(qr(0, 0, &[]).map(|(q, r)| (q.len(), r.len())), Ok((0, 0)));
        assert_eq!(matmul(2, 0, 3, &[], &[]), Ok(vec![0.0; 6]));
    }

    #[test]
    fn a_shape_mismatch_is_reported_rather_than_read() {
        assert!(matches!(
            matmul(2, 2, 2, &[1.0, 2.0, 3.0], &[1.0, 2.0, 3.0, 4.0]),
            Err(NumericError::ShapeMismatch(_))
        ));
        assert!(matches!(
            solve(2, 2, &[1.0, 0.0, 0.0, 1.0], &[1.0]),
            Err(NumericError::ShapeMismatch(_))
        ));
    }
    #[test]
    fn shared_kernel_eigenvectors_remain_valid_for_uniformly_tiny_matrices() {
        let a = [2e-20, 1e-20, 1e-20, 2e-20];
        let (values, vectors) = eig(2, 2, &a).unwrap();
        for column in 0..2 {
            for row in 0..2 {
                let first = vectors[column].mul(Complex::new(a[row * 2], 0.0));
                let second = vectors[2 + column].mul(Complex::new(a[row * 2 + 1], 0.0));
                let rhs = values[column].mul(vectors[row * 2 + column]);
                assert!(
                    (first.re + second.re - rhs.re).hypot(first.im + second.im - rhs.im) < 1e-32
                );
            }
        }
        assert_eq!(
            lstsq(2, 3, &[1.0, 0.0, 0.0, 0.0, 1.0, 0.0], &[2.0, 3.0]).unwrap(),
            vec![2.0, 3.0, 0.0]
        );
        let (u, s, vt) = svd(0, 3, &[]).unwrap();
        assert!(u.is_empty() && s.is_empty() && vt.is_empty());
    }
}
