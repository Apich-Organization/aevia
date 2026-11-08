//! `f64` kernels for compressed sparse matrices (§8.4.1).
//!
//! Everything here takes the CSR arrays of a square-or-rectangular matrix
//! (`row_ptr`, `col_idx`, `values`) and a dense right-hand side. Nothing is
//! ever densified: the direct solver hands the entries to `faer`'s sparse LU
//! (fill-reducing COLAMD ordering, left-looking numeric factorisation), and
//! the iterative solver only needs the matrix-vector product.

use crate::NumericError;
use faer::linalg::solvers::Solve;
use faer::sparse::SparseColMat;
use faer::sparse::Triplet;

/// A borrowed CSR matrix.
#[derive(Clone, Copy, Debug)]
pub struct Csr<'a> {
    /// Number of rows.
    pub rows: usize,
    /// Number of columns.
    pub cols: usize,
    /// `rows + 1` offsets into `col_idx`/`values`.
    pub row_ptr: &'a [usize],
    /// Column of each stored entry.
    pub col_idx: &'a [usize],
    /// Value of each stored entry.
    pub values: &'a [f64],
}

impl Csr<'_> {
    /// `y = A x`.
    ///
    /// # Errors
    /// [`NumericError::ShapeMismatch`] when `x` does not have `cols` entries.
    pub fn matvec(
        &self,
        x: &[f64],
    ) -> Result<Vec<f64>, NumericError> {
        if x.len() != self.cols {
            return Err(NumericError::ShapeMismatch(format!(
                "matrix has {} columns, vector has {} entries",
                self.cols,
                x.len()
            )));
        }
        let mut y = vec![0.0; self.rows];
        for row in 0..self.rows {
            let mut acc = 0.0;
            for k in self.row_ptr[row]..self.row_ptr[row + 1] {
                acc += self.values[k] * x[self.col_idx[k]];
            }
            y[row] = acc;
        }
        Ok(y)
    }
}

/// Solves `A x = b` by sparse LU.
///
/// # Errors
/// [`NumericError::NotSquare`], [`NumericError::ShapeMismatch`] for a bad
/// right-hand side, and [`NumericError::Singular`] when the factorisation
/// fails or produces a non-finite solution.
pub fn lu_solve(
    a: &Csr<'_>,
    b: &[f64],
) -> Result<Vec<f64>, NumericError> {
    if a.rows != a.cols {
        return Err(NumericError::NotSquare);
    }
    if b.len() != a.rows {
        return Err(NumericError::ShapeMismatch(format!(
            "matrix has {} rows, right-hand side has {} entries",
            a.rows,
            b.len()
        )));
    }
    let n = a.rows;
    if n == 0 {
        return Ok(Vec::new());
    }
    let mut triplets = Vec::with_capacity(a.values.len());
    for row in 0..n {
        for k in a.row_ptr[row]..a.row_ptr[row + 1] {
            triplets.push(Triplet::new(row, a.col_idx[k], a.values[k]));
        }
    }
    let matrix = SparseColMat::<usize, f64>::try_new_from_triplets(n, n, &triplets)
        .map_err(|_| NumericError::Singular)?;
    let lu = matrix.sp_lu().map_err(|_| NumericError::Singular)?;
    let rhs = faer::Col::<f64>::from_fn(n, |i| b[i]);
    let solution = lu.solve(&rhs);
    let out: Vec<f64> = (0..n).map(|i| solution[i]).collect();
    if out.iter().any(|value| !value.is_finite()) {
        return Err(NumericError::Singular);
    }
    Ok(out)
}

/// The result of [`cg`].
#[derive(Clone, Debug)]
pub struct CgResult {
    /// The final iterate.
    pub x: Vec<f64>,
    /// Iterations performed.
    pub iterations: usize,
    /// The final relative residual `‖b − A x‖ / ‖b‖` (`‖r‖` when `b = 0`).
    pub residual: f64,
    /// Whether the relative residual reached the tolerance.
    pub converged: bool,
}

/// Unpreconditioned conjugate gradients for a symmetric positive-definite
/// matrix, starting from zero.
///
/// # Errors
/// [`NumericError::NotSquare`] and [`NumericError::ShapeMismatch`].
pub fn cg(
    a: &Csr<'_>,
    b: &[f64],
    tolerance: f64,
    max_iterations: usize,
) -> Result<CgResult, NumericError> {
    if a.rows != a.cols {
        return Err(NumericError::NotSquare);
    }
    if b.len() != a.rows {
        return Err(NumericError::ShapeMismatch(format!(
            "matrix has {} rows, right-hand side has {} entries",
            a.rows,
            b.len()
        )));
    }
    let n = a.rows;
    let dot = |x: &[f64], y: &[f64]| x.iter().zip(y).map(|(p, q)| p * q).sum::<f64>();
    let b_norm = dot(b, b).sqrt();
    let scale = if b_norm > 0.0 { b_norm } else { 1.0 };
    let mut x = vec![0.0; n];
    let mut r = b.to_vec();
    let mut p = r.clone();
    let mut rs = dot(&r, &r);
    let mut iterations = 0;
    while iterations < max_iterations && rs.sqrt() / scale > tolerance {
        let ap = a.matvec(&p)?;
        let denominator = dot(&p, &ap);
        if denominator <= 0.0 || !denominator.is_finite() {
            return Err(NumericError::NotPositiveDefinite);
        }
        let alpha = rs / denominator;
        for i in 0..n {
            x[i] += alpha * p[i];
            r[i] -= alpha * ap[i];
        }
        let next = dot(&r, &r);
        let beta = next / rs;
        for i in 0..n {
            p[i] = r[i] + beta * p[i];
        }
        rs = next;
        iterations += 1;
    }
    let residual = rs.sqrt() / scale;
    Ok(CgResult {
        x,
        iterations,
        residual,
        converged: residual <= tolerance,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tridiagonal(n: usize) -> (Vec<usize>, Vec<usize>, Vec<f64>) {
        let (mut ptr, mut idx, mut val) = (vec![0], Vec::new(), Vec::new());
        for i in 0..n {
            if i > 0 {
                idx.push(i - 1);
                val.push(-1.0);
            }
            idx.push(i);
            val.push(2.0);
            if i + 1 < n {
                idx.push(i + 1);
                val.push(-1.0);
            }
            ptr.push(idx.len());
        }
        (ptr, idx, val)
    }

    #[test]
    fn lu_and_cg_agree_on_the_laplacian() {
        let n = 50;
        let (ptr, idx, val) = tridiagonal(n);
        let a = Csr {
            rows: n,
            cols: n,
            row_ptr: &ptr,
            col_idx: &idx,
            values: &val,
        };
        let b = vec![1.0; n];
        let direct = lu_solve(&a, &b).expect("lu");
        let iterative = cg(&a, &b, 1e-12, 500).expect("cg");
        assert!(iterative.converged);
        for (p, q) in direct.iter().zip(&iterative.x) {
            assert!((p - q).abs() < 1e-8);
        }
        let back = a.matvec(&direct).expect("matvec");
        assert!(back.iter().all(|v| (v - 1.0).abs() < 1e-9));
    }
}
