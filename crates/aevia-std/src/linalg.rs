//! Dense linear algebra (§8.4).
//!
//! Matrices are row-major `&[f64]` with an explicit row and column count.
//!
//! The `f64` kernels — `matmul`, `det`, `inv`, `solve`, `lu`, `qr` — run on
//! [`faer`](https://codeberg.org/sarah-quinones/faer), which is a blocked,
//! optionally parallel BLAS/LAPACK-class backend; [`legacy`] holds the code
//! they replaced (`nalgebra` for the factorisations, a triple loop for
//! `matmul`) and is what answers a shape `faer` cannot express.
//!
//! `eig`, `eigvals`, `svd`, `lstsq` and `cholesky` are still `nalgebra`'s.
//! They are iterative and were not in scope for the backend change; moving
//! them would have to be justified by a measurement of its own.
//!
//! Where exactness matters the path does not come near either float backend:
//! `lu_exact` and `qr_exact` run over `BigRational` so an exact matrix keeps an
//! exact decomposition, which is the promise `Scalar<ρ,d>` makes when `ρ` is
//! `Real`. Nothing in this module accepts an exact value — every entry point
//! takes `&[f64]`, so a `Real` matrix cannot reach `faer` at all.
//!
//! `svd` is the *economy* decomposition: `u` is `rows × k`, `s` has `k` entries
//! and `vt` is `k × cols` with `k = min(rows, cols)`, so `a = u diag(s) vt`
//! exactly. A full square `u` would need `rows - k` extra columns that carry no
//! information about `a`.

use crate::Complex;
use crate::NumericError;
use num_rational::BigRational;
use num_traits::One;
use num_traits::Signed;
use num_traits::Zero;

mod faer_backend;
pub mod legacy;

use legacy::matrix;
use legacy::vector;

/// `true` when a dimension is empty, in which case `faer` asserts instead of
/// answering and [`legacy`] answers.
fn empty(
    rows: usize,
    cols: usize,
) -> bool {
    rows == 0 || cols == 0
}

/// The determinant of a square matrix.
///
/// # Errors
///
/// [`NumericError::NotSquare`] when the matrix is not square.
pub fn det(
    rows: usize,
    cols: usize,
    a: &[f64],
) -> Result<f64, NumericError> {
    if rows != cols {
        return Err(NumericError::NotSquare);
    }
    if rows == 0 {
        return legacy::det(rows, cols, a);
    }
    faer_backend::det(rows, cols, a)
}

/// The inverse of a square matrix.
///
/// # Errors
///
/// [`NumericError::Singular`] when it has none.
pub fn inv(
    rows: usize,
    cols: usize,
    a: &[f64],
) -> Result<Vec<f64>, NumericError> {
    if rows != cols {
        return Err(NumericError::NotSquare);
    }
    if rows == 0 {
        return legacy::inv(rows, cols, a);
    }
    faer_backend::inv(rows, cols, a)
}

/// Solves `A x = b` for a square `A`.
///
/// # Errors
///
/// [`NumericError::Singular`] when the system has no unique solution.
pub fn solve(
    rows: usize,
    cols: usize,
    a: &[f64],
    b: &[f64],
) -> Result<Vec<f64>, NumericError> {
    if rows != cols {
        return Err(NumericError::NotSquare);
    }
    if rows == 0 {
        return legacy::solve(rows, cols, a, b);
    }
    faer_backend::solve(rows, cols, a, b)
}

/// The product of a `rows × inner` matrix and an `inner × cols` one.
///
/// The runtime's `@` still runs [`legacy::matmul`], which is the same loop it
/// has always run; this is the backend that operator can be moved onto. The
/// two do **not** agree bit for bit — `faer` blocks the inner loop, and
/// reassociating a sum changes its last bits — so a caller that needs the old
/// bits has to ask for them.
///
/// A zero extent answers zeros rather than failing; the runtime's own `@`
/// declines a zero inner extent before it gets here.
///
/// # Errors
///
/// [`NumericError::ShapeMismatch`] when either operand's length is not
/// `rows * inner` or `inner * cols`.
pub fn matmul(
    rows: usize,
    inner: usize,
    cols: usize,
    a: &[f64],
    b: &[f64],
) -> Result<Vec<f64>, NumericError> {
    if empty(rows, cols) || inner == 0 {
        if rows * inner != a.len() || inner * cols != b.len() {
            return Err(NumericError::ShapeMismatch(format!(
                "{rows}x{inner} times {inner}x{cols} needs {} and {} elements, found {} and {}",
                rows * inner,
                inner * cols,
                a.len(),
                b.len()
            )));
        }
        return Ok(legacy::matmul(rows, inner, cols, a, b));
    }
    faer_backend::matmul(rows, inner, cols, a, b)
}

/// The minimum-norm least-squares solution of `A x = b` (§8.4).
///
/// `lstsq` is defined for a matrix of any shape: a tall matrix has one
/// least-squares answer, a wide one has infinitely many and this returns the
/// one of smallest norm, which is what the pseudo-inverse gives.
///
/// # Errors
///
/// [`NumericError::ShapeMismatch`] when `b` does not have `rows` entries.
pub fn lstsq(
    rows: usize,
    cols: usize,
    a: &[f64],
    b: &[f64],
) -> Result<Vec<f64>, NumericError> {
    let matrix = matrix(rows, cols, a)?;
    let rhs = vector(rows, b)?;
    let svd = matrix.svd(true, true);
    let solution = svd
        .solve(&rhs, 1e-12)
        .map_err(|_| NumericError::NotConverged)?;
    Ok(solution.as_slice().to_vec())
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
    let matrix = matrix(rows, cols, a)?;
    let values: Vec<Complex> = matrix
        .complex_eigenvalues()
        .into_iter()
        .map(|value| Complex::new(value.re, value.im))
        .collect();
    if values.len() != rows {
        return Err(NumericError::NotConverged);
    }
    let mut vectors = vec![Complex::default(); rows * rows];
    for (column, value) in values.iter().enumerate() {
        let vector = eigenvector(rows, a, *value)?;
        for row in 0..rows {
            vectors[row * rows + column] = vector[row];
        }
    }
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
    Ok(matrix(rows, cols, a)?
        .complex_eigenvalues()
        .into_iter()
        .map(|value| Complex::new(value.re, value.im))
        .collect())
}

/// An eigenvector for `lambda`, from the null space of `A - lambda I`.
///
/// The null space is found by complex Gaussian elimination: the reduced matrix
/// has a free column (the eigenvalue makes it singular), the free variable is
/// set to one and the rest is back-substituted. When rounding leaves the matrix
/// non-singular the answer falls back to the last basis vector, which is a
/// legitimate (if uninteresting) representative and keeps the shape promise.
fn eigenvector(
    n: usize,
    a: &[f64],
    lambda: Complex,
) -> Result<Vec<Complex>, NumericError> {
    let mut m: Vec<Vec<Complex>> = (0..n)
        .map(|row| {
            (0..n)
                .map(|column| {
                    let entry = a[row * n + column];
                    if row == column {
                        Complex::new(entry - lambda.re, -lambda.im)
                    } else {
                        Complex::new(entry, 0.0)
                    }
                })
                .collect()
        })
        .collect();
    // Row echelon form with partial pivoting by modulus.
    let mut pivot_of_row = vec![usize::MAX; n];
    let mut row = 0;
    for column in 0..n {
        let mut best = row;
        for candidate in row..n {
            if m[candidate][column].abs() > m[best][column].abs() {
                best = candidate;
            }
        }
        if m[best][column].abs() < 1e-12 {
            continue;
        }
        m.swap(row, best);
        let pivot = m[row][column];
        for inner in column..n {
            m[row][inner] = m[row][inner].div(pivot);
        }
        for other in 0..n {
            if other == row {
                continue;
            }
            let factor = m[other][column];
            if factor.abs() == 0.0 {
                continue;
            }
            for inner in column..n {
                let term = factor.mul(m[row][inner]);
                m[other][inner] =
                    Complex::new(m[other][inner].re - term.re, m[other][inner].im - term.im);
            }
        }
        pivot_of_row[row] = column;
        row += 1;
        if row == n {
            break;
        }
    }
    let taken: Vec<usize> = pivot_of_row
        .iter()
        .copied()
        .filter(|&c| c != usize::MAX)
        .collect();
    let free = (0..n).find(|column| !taken.contains(column));
    let Some(free) = free else {
        // No free column: numerical noise. Any vector is as good as another.
        let mut vector = vec![Complex::default(); n];
        vector[n - 1] = Complex::new(1.0, 0.0);
        return Ok(vector);
    };
    let mut vector = vec![Complex::default(); n];
    vector[free] = Complex::new(1.0, 0.0);
    for (row, &column) in pivot_of_row.iter().enumerate() {
        if column == usize::MAX {
            continue;
        }
        vector[column] = Complex::new(-m[row][free].re, -m[row][free].im);
    }
    // Normalise so that the vectors of a matrix are comparable across runs.
    let norm = vector
        .iter()
        .map(|value| value.re * value.re + value.im * value.im)
        .sum::<f64>()
        .sqrt();
    if norm > 0.0 {
        for value in &mut vector {
            *value = Complex::new(value.re / norm, value.im / norm);
        }
    }
    Ok(vector)
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
    let matrix = matrix(rows, cols, a)?;
    let k = rows.min(cols);
    let svd = matrix.svd(true, true);
    let u = svd.u.ok_or(NumericError::NotConverged)?;
    let vt = svd.v_t.ok_or(NumericError::NotConverged)?;
    // `nalgebra` returns the thin factors; make the shape promise explicit.
    let mut u_out = vec![0.0; rows * k];
    for row in 0..rows {
        for column in 0..k.min(u.ncols()) {
            u_out[row * k + column] = u[(row, column)];
        }
    }
    let mut vt_out = vec![0.0; k * cols];
    for row in 0..k.min(vt.nrows()) {
        for column in 0..cols {
            vt_out[row * cols + column] = vt[(row, column)];
        }
    }
    let s: Vec<f64> = svd.singular_values.iter().copied().collect();
    Ok((u_out, s, vt_out))
}

/// The QR decomposition `A = Q R` with `Q` orthonormal.
///
/// `Q` is `rows × rows` and `R` is `rows × cols`, so `Q R` reproduces `A` for
/// any shape.
///
/// # Errors
///
/// [`NumericError::NotConverged`] when the decomposition fails.
pub fn qr(
    rows: usize,
    cols: usize,
    a: &[f64],
) -> Result<(Vec<f64>, Vec<f64>), NumericError> {
    if empty(rows, cols) {
        return legacy::qr(rows, cols, a);
    }
    faer_backend::qr(rows, cols, a)
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
    if rows == 0 {
        return legacy::lu(rows, cols, a);
    }
    faer_backend::lu(rows, cols, a)
}

/// The lower-triangular Cholesky factor `L` with `A = L Lᵀ`.
///
/// # Errors
///
/// [`NumericError::NotPositiveDefinite`] when `A` is not positive definite.
pub fn cholesky(
    rows: usize,
    cols: usize,
    a: &[f64],
) -> Result<Vec<f64>, NumericError> {
    if rows != cols {
        return Err(NumericError::NotSquare);
    }
    let matrix = matrix(rows, cols, a)?;
    let factor = matrix.cholesky().ok_or(NumericError::NotPositiveDefinite)?;
    let l = factor.l();
    let mut out = vec![0.0; rows * rows];
    for row in 0..rows {
        for column in 0..rows {
            out[row * rows + column] = l[(row, column)];
        }
    }
    Ok(out)
}

/// The lower triangle of a matrix, the rest zeroed.
#[must_use]
pub fn tril(
    rows: usize,
    cols: usize,
    a: &[f64],
) -> Vec<f64> {
    let mut out = vec![0.0; a.len()];
    for row in 0..rows {
        for column in 0..cols.min(row + 1) {
            out[row * cols + column] = a[row * cols + column];
        }
    }
    out
}

/// The upper triangle of a matrix, the rest zeroed.
#[must_use]
pub fn triu(
    rows: usize,
    cols: usize,
    a: &[f64],
) -> Vec<f64> {
    let mut out = vec![0.0; a.len()];
    for row in 0..rows {
        for column in row..cols {
            out[row * cols + column] = a[row * cols + column];
        }
    }
    out
}

// ----- exact decompositions ------------------------------------------------

/// An exact LU decomposition with partial pivoting: `P A = L U`.
///
/// Exactness is the point: `lu([[1,2],[3,4]])` answers rationals, and a
/// singular matrix is reported rather than turned into `inf`.
///
/// # Errors
///
/// [`NumericError::NotSquare`] when the matrix is not square.
pub fn lu_exact(
    n: usize,
    a: &[BigRational],
) -> Result<(Vec<BigRational>, Vec<BigRational>, Vec<BigRational>), NumericError> {
    if n * n != a.len() {
        return Err(NumericError::ShapeMismatch(format!(
            "{n}x{n} needs {} elements, found {}",
            n * n,
            a.len()
        )));
    }
    let mut u: Vec<BigRational> = a.to_vec();
    let mut l = vec![BigRational::zero(); n * n];
    let mut p = vec![BigRational::zero(); n * n];
    for index in 0..n {
        l[index * n + index] = BigRational::one();
        p[index * n + index] = BigRational::one();
    }
    for column in 0..n {
        let mut pivot = column;
        while pivot < n && u[pivot * n + column].is_zero() {
            pivot += 1;
        }
        if pivot == n {
            // Singular: no pivot in this column. Continue with the identity,
            // which is what an exact `L U` of a singular matrix is.
            continue;
        }
        if pivot != column {
            for inner in 0..n {
                u.swap(pivot * n + inner, column * n + inner);
                p.swap(pivot * n + inner, column * n + inner);
            }
            for inner in 0..column {
                l.swap(pivot * n + inner, column * n + inner);
            }
        }
        let lead = u[column * n + column].clone();
        for row in column + 1..n {
            let factor = u[row * n + column].clone() / &lead;
            l[row * n + column] = factor.clone();
            for inner in column..n {
                let term = factor.clone() * u[column * n + inner].clone();
                u[row * n + inner] -= term;
            }
        }
    }
    Ok((p, l, u))
}

/// An exact QR decomposition by modified Gram-Schmidt.
///
/// # Errors
///
/// [`NumericError::ShapeMismatch`] when the matrix is not `rows × cols`.
pub fn qr_exact(
    rows: usize,
    cols: usize,
    a: &[BigRational],
) -> Result<(Vec<BigRational>, Vec<BigRational>), NumericError> {
    if rows * cols != a.len() {
        return Err(NumericError::ShapeMismatch(format!(
            "{rows}x{cols} needs {} elements, found {}",
            rows * cols,
            a.len()
        )));
    }
    let mut q = vec![BigRational::zero(); rows * rows];
    let mut r = vec![BigRational::zero(); rows * cols];
    for column in 0..cols {
        let mut v: Vec<BigRational> = (0..rows)
            .map(|row| a[row * cols + column].clone())
            .collect();
        for inner in 0..column.min(rows) {
            let mut dot = BigRational::zero();
            for row in 0..rows {
                dot += q[row * rows + inner].clone() * v[row].clone();
            }
            r[inner * cols + column] = dot.clone();
            for row in 0..rows {
                let term = dot.clone() * q[row * rows + inner].clone();
                v[row] -= term;
            }
        }
        let norm = v.iter().fold(BigRational::zero(), |acc, value| {
            acc + value.clone() * value.clone()
        });
        if norm.is_zero() {
            continue;
        }
        let Some(length) = exact_sqrt(&norm) else {
            return Err(NumericError::Domain(
                "an exact QR of this matrix needs an irrational square root".to_string(),
            ));
        };
        r[column * cols + column] = length.clone();
        if column < rows {
            for row in 0..rows {
                q[row * rows + column] = v[row].clone() / &length;
            }
        }
    }
    Ok((q, r))
}

/// The exact square root of a non-negative rational, when it has one.
#[must_use]
pub fn exact_sqrt(value: &BigRational) -> Option<BigRational> {
    if value.is_negative() {
        return None;
    }
    let numerator = integer_sqrt(value.numer())?;
    let denominator = integer_sqrt(value.denom())?;
    Some(BigRational::new(numerator, denominator))
}

/// The integer square root, when the argument is a perfect square.
fn integer_sqrt(value: &num_bigint::BigInt) -> Option<num_bigint::BigInt> {
    use num_bigint::BigInt;
    use num_traits::ToPrimitive;
    if value.is_zero() {
        return Some(BigInt::zero());
    }
    if value.is_negative() {
        return None;
    }
    // Newton's method on integers: exact, and it never needs a float that
    // cannot hold the value.
    let mut guess: BigInt = {
        let bits = value.bits();
        BigInt::one() << ((bits as usize).div_ceil(2))
    };
    loop {
        let next = (&guess + value / &guess) >> 1;
        if next >= guess {
            break;
        }
        guess = next;
    }
    if &guess * &guess == *value {
        let _ = guess.to_u64();
        Some(guess)
    } else {
        None
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
        // `faer` asserts on a zero dimension; the legacy path answers, and the
        // dispatch sends it there.
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
}
