//! Extended linear algebra kernels: thin adapters over
//! `rssn::kernels::{dense, lowrank, matfun, schur}`.

#[cfg(test)]
use super::matmul;
use crate::Complex;
use crate::NumericError;

fn check_shape(
    rows: usize,
    cols: usize,
    a: &[f64],
) -> Result<(), NumericError> {
    if rows.checked_mul(cols) != Some(a.len()) {
        return Err(NumericError::ShapeMismatch(format!(
            "expected {rows}x{cols} ({}) entries, got {}",
            rows.saturating_mul(cols),
            a.len()
        )));
    }
    Ok(())
}
use rssn::kernels::dense::DenseError;
use rssn::kernels::dense::Mat as RssnMat;
use rssn::kernels::dense::{
    self,
};
use rssn::kernels::lowrank;
use rssn::kernels::matfun;
use rssn::kernels::schur as kschur;

pub use rssn::kernels::dense::MatrixNorm;

fn to_rssn_mat(
    rows: usize,
    cols: usize,
    a: &[f64],
) -> Result<RssnMat, NumericError> {
    check_shape(rows, cols, a)?;
    if a.iter().any(|v| !v.is_finite()) {
        return Err(NumericError::Domain(
            "matrix contains non-finite entries".into(),
        ));
    }
    Ok(RssnMat {
        rows,
        cols,
        data: a.to_vec(),
    })
}

fn map_dense_err(err: DenseError) -> NumericError {
    match err {
        | DenseError::Shape => NumericError::NotSquare,
        | DenseError::Singular => NumericError::Singular,
        | DenseError::NonFinite => {
            NumericError::Domain("matrix contains non-finite entries".into())
        },
        | DenseError::NoConvergence => NumericError::NotConverged,
    }
}

/// Computes the requested matrix norm (`One`, `Inf`, `Frobenius`, or `Spectral`).
///
/// # Errors
/// [`NumericError::ShapeMismatch`] or [`NumericError::NotConverged`].
pub fn norm(
    rows: usize,
    cols: usize,
    a: &[f64],
    kind: MatrixNorm,
) -> Result<f64, NumericError> {
    let m = to_rssn_mat(rows, cols, a)?;
    dense::norm(&m, kind).map_err(map_dense_err)
}

/// Spectral (2-norm) condition number `sigma_max / sigma_min` via `rssn::kernels::dense::cond`.
///
/// # Errors
/// [`NumericError::ShapeMismatch`] or [`NumericError::NotConverged`].
pub fn cond(
    rows: usize,
    cols: usize,
    a: &[f64],
) -> Result<f64, NumericError> {
    let m = to_rssn_mat(rows, cols, a)?;
    if rows == 0 || cols == 0 {
        return Ok(1.0);
    }
    dense::cond(&m).map_err(map_dense_err)
}

/// Condition number in the specified matrix norm (`||A|| * ||A^{-1}||` for
/// square matrices under `One`/`Inf`/`Frobenius`, or 2-norm for `Spectral`).
///
/// # Errors
/// [`NumericError::ShapeMismatch`], [`NumericError::NotSquare`], or [`NumericError::NotConverged`].
pub fn cond_norm(
    rows: usize,
    cols: usize,
    a: &[f64],
    kind: MatrixNorm,
) -> Result<f64, NumericError> {
    let m = to_rssn_mat(rows, cols, a)?;
    dense::cond_norm(&m, kind).map_err(map_dense_err)
}

/// Moore–Penrose pseudoinverse `A^+` (`cols × rows`) via `rssn::kernels::dense::pinv`.
///
/// # Errors
/// [`NumericError::ShapeMismatch`] or [`NumericError::NotConverged`].
pub fn pinv(
    rows: usize,
    cols: usize,
    a: &[f64],
) -> Result<Vec<f64>, NumericError> {
    let m = to_rssn_mat(rows, cols, a)?;
    if rows == 0 || cols == 0 {
        return Ok(vec![0.0; cols * rows]);
    }
    Ok(dense::pinv(&m).map_err(map_dense_err)?.data)
}

/// Moore–Penrose pseudoinverse `A^+` with explicit singular-value cutoff `rcond`.
///
/// # Errors
/// [`NumericError::ShapeMismatch`] or [`NumericError::NotConverged`].
pub fn pinv_tol(
    rows: usize,
    cols: usize,
    a: &[f64],
    rcond: f64,
) -> Result<Vec<f64>, NumericError> {
    let m = to_rssn_mat(rows, cols, a)?;
    Ok(dense::pinv_tol(&m, rcond).map_err(map_dense_err)?.data)
}

/// Matrix exponential `exp(A)` via `rssn::kernels::matfun::expm`.
///
/// # Errors
/// [`NumericError::NotSquare`] or [`NumericError::ShapeMismatch`].
pub fn expm(
    rows: usize,
    cols: usize,
    a: &[f64],
) -> Result<Vec<f64>, NumericError> {
    let m = to_rssn_mat(rows, cols, a)?;
    if rows != cols {
        return Err(NumericError::NotSquare);
    }
    if rows == 0 {
        return Ok(Vec::new());
    }
    Ok(matfun::expm(&m).map_err(map_dense_err)?.data)
}

/// Principal matrix logarithm `log(A)` via `rssn::kernels::matfun::logm`.
///
/// # Errors
/// [`NumericError::NotSquare`], [`NumericError::ShapeMismatch`], or [`NumericError::NotConverged`].
pub fn logm(
    rows: usize,
    cols: usize,
    a: &[f64],
) -> Result<Vec<f64>, NumericError> {
    let m = to_rssn_mat(rows, cols, a)?;
    if rows != cols {
        return Err(NumericError::NotSquare);
    }
    if rows == 0 {
        return Ok(Vec::new());
    }
    Ok(matfun::logm(&m).map_err(map_dense_err)?.data)
}

/// Principal matrix square root `sqrt(A)` via `rssn::kernels::matfun::sqrtm`.
///
/// # Errors
/// [`NumericError::NotSquare`], [`NumericError::ShapeMismatch`], or [`NumericError::NotConverged`].
pub fn sqrtm(
    rows: usize,
    cols: usize,
    a: &[f64],
) -> Result<Vec<f64>, NumericError> {
    let m = to_rssn_mat(rows, cols, a)?;
    if rows != cols {
        return Err(NumericError::NotSquare);
    }
    if rows == 0 {
        return Ok(Vec::new());
    }
    Ok(matfun::sqrtm(&m).map_err(map_dense_err)?.data)
}

/// Real Schur decomposition `A = Q T Qᵀ` via `rssn::kernels::schur::schur`.
///
/// # Errors
/// [`NumericError::NotSquare`] or [`NumericError::ShapeMismatch`].
pub fn schur(
    rows: usize,
    cols: usize,
    a: &[f64],
) -> Result<(Vec<f64>, Vec<f64>), NumericError> {
    let m = to_rssn_mat(rows, cols, a)?;
    let s = kschur::schur(&m).map_err(map_dense_err)?;
    Ok((s.q.data, s.t.data))
}

/// Rank-revealing QR decomposition with column pivoting: `A P = Q R` via
/// `rssn::kernels::lowrank::qr_pivoted`.
///
/// # Errors
/// [`NumericError::ShapeMismatch`].
pub fn qr_pivoted(
    rows: usize,
    cols: usize,
    a: &[f64],
    tol: f64,
) -> Result<(Vec<f64>, Vec<f64>, Vec<usize>, usize), NumericError> {
    let m = to_rssn_mat(rows, cols, a)?;
    if rows == 0 || cols == 0 {
        let mut q = vec![0.0; rows * rows];
        for i in 0..rows {
            q[i * rows + i] = 1.0;
        }
        return Ok((q, vec![0.0; rows * cols], (0..cols).collect(), 0));
    }
    let pqr = lowrank::qr_pivoted(&m).map_err(|e| NumericError::ShapeMismatch(e.to_string()))?;
    let rank = pqr.rank(if tol < 0.0 { None } else { Some(tol) });
    Ok((pqr.q.data, pqr.r.data, pqr.perm, rank))
}

/// Solves the continuous-time Sylvester equation `A X + X B = C` via
/// `rssn::kernels::schur::sylvester`.
///
/// # Errors
/// [`NumericError::ShapeMismatch`] or [`NumericError::Singular`].
pub fn sylvester(
    n: usize,
    m: usize,
    a: &[f64],
    b: &[f64],
    c: &[f64],
) -> Result<Vec<f64>, NumericError> {
    let ma = to_rssn_mat(n, n, a)?;
    let mb = to_rssn_mat(m, m, b)?;
    let mc = to_rssn_mat(n, m, c)?;
    Ok(kschur::sylvester(&ma, &mb, &mc)
        .map_err(map_dense_err)?
        .data)
}

/// Solves the continuous-time Lyapunov equation `A X + X Aᵀ = Q` via
/// `rssn::kernels::schur::lyapunov`.
///
/// # Errors
/// [`NumericError::ShapeMismatch`] or [`NumericError::Singular`].
pub fn lyapunov(
    n: usize,
    a: &[f64],
    q: &[f64],
) -> Result<Vec<f64>, NumericError> {
    let ma = to_rssn_mat(n, n, a)?;
    let mq = to_rssn_mat(n, n, q)?;
    Ok(kschur::lyapunov(&ma, &mq).map_err(map_dense_err)?.data)
}

/// Real QZ (generalized Schur) decomposition of `(A, B)` via
/// `rssn::kernels::schur::qz`.
///
/// # Errors
/// [`NumericError::ShapeMismatch`] or [`NumericError::NotConverged`].
pub fn qz(
    n: usize,
    a: &[f64],
    b: &[f64],
) -> Result<
    (
        Vec<f64>,
        Vec<f64>,
        Vec<f64>,
        Vec<f64>,
        Vec<Complex>,
        Vec<f64>,
    ),
    NumericError,
> {
    let ma = to_rssn_mat(n, n, a)?;
    let mb = to_rssn_mat(n, n, b)?;
    let res = kschur::qz(&ma, &mb).map_err(map_dense_err)?;
    let alpha = res
        .alpha
        .into_iter()
        .map(|c| Complex::new(c.re, c.im))
        .collect();
    Ok((
        res.s.data, res.t.data, res.q.data, res.z.data, alpha, res.beta,
    ))
}

/// Generalized eigenvalues `lambda_i = alpha_i / beta_i` of `A v = lambda B v`.
///
/// # Errors
/// [`NumericError::ShapeMismatch`] or [`NumericError::NotConverged`].
pub fn eigvals_generalized(
    n: usize,
    a: &[f64],
    b: &[f64],
) -> Result<Vec<Complex>, NumericError> {
    let ma = to_rssn_mat(n, n, a)?;
    let mb = to_rssn_mat(n, n, b)?;
    let eigs = kschur::eigvals_generalized(&ma, &mb).map_err(map_dense_err)?;
    Ok(eigs.into_iter().map(|c| Complex::new(c.re, c.im)).collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn close(
        a: f64,
        b: f64,
        tol: f64,
    ) -> bool {
        (a - b).abs() < tol
    }

    #[test]
    fn pinv_recovers_inverse_and_least_squares_pseudoinverse() {
        let a = [1.0, 2.0, 3.0, 4.0, 5.0, 6.0];
        let p = pinv(3, 2, &a).unwrap();
        let ap = matmul(3, 2, 3, &a, &p).unwrap();
        let apa = matmul(3, 3, 2, &ap, &a).unwrap();
        for (x, y) in a.iter().zip(apa.iter()) {
            assert!(close(*x, *y, 1e-10));
        }
    }

    #[test]
    fn norms_and_cond_match_known_values() {
        let a = [1.0, -2.0, 3.0, 4.0];
        assert!(close(norm(2, 2, &a, MatrixNorm::One).unwrap(), 6.0, 1e-12));
        assert!(close(norm(2, 2, &a, MatrixNorm::Inf).unwrap(), 7.0, 1e-12));
        assert!(close(
            norm(2, 2, &a, MatrixNorm::Frobenius).unwrap(),
            30.0_f64.sqrt(),
            1e-12
        ));
        let id = [1.0, 0.0, 0.0, 1.0];
        assert!(close(cond(2, 2, &id).unwrap(), 1.0, 1e-12));
        let singular = [1.0, 2.0, 2.0, 4.0];
        assert!(cond(2, 2, &singular).unwrap().is_infinite());
    }

    #[test]
    fn expm_logm_sqrtm_satisfy_identities() {
        let a = [2.0, 1.0, 0.0, 3.0];
        let e = expm(2, 2, &a).unwrap();
        let le = logm(2, 2, &e).unwrap();
        for (x, y) in a.iter().zip(le.iter()) {
            assert!(close(*x, *y, 1e-9), "expm/logm mismatch: {x} vs {y}");
        }
        let spd = [4.0, 1.0, 1.0, 3.0];
        let s = sqrtm(2, 2, &spd).unwrap();
        let s2 = matmul(2, 2, 2, &s, &s).unwrap();
        for (x, y) in spd.iter().zip(s2.iter()) {
            assert!(close(*x, *y, 1e-9), "sqrtm^2 mismatch: {x} vs {y}");
        }
    }

    #[test]
    fn schur_reconstructs_matrix() {
        let a = [0.0, 1.0, -2.0, 3.0, 1.0, 0.0, 4.0, -1.0, 2.0];
        let (q, t) = schur(3, 3, &a).unwrap();
        let mut qt = vec![0.0; 9];
        for i in 0..3 {
            for j in 0..3 {
                qt[i * 3 + j] = q[j * 3 + i];
            }
        }
        let tmp = matmul(3, 3, 3, &q, &t).unwrap();
        let rec = matmul(3, 3, 3, &tmp, &qt).unwrap();
        for (x, y) in a.iter().zip(rec.iter()) {
            assert!(close(*x, *y, 1e-10));
        }
    }

    #[test]
    fn qr_pivoted_reveals_rank() {
        let a = [1.0, 2.0, 3.0, 2.0, 4.0, 6.0, 1.0, 0.0, 1.0];
        let (q, r, perm, rank) = qr_pivoted(3, 3, &a, -1.0).unwrap();
        assert_eq!(rank, 2);
        let qr_prod = matmul(3, 3, 3, &q, &r).unwrap();
        for i in 0..3 {
            for j in 0..3 {
                assert!(close(qr_prod[i * 3 + j], a[i * 3 + perm[j]], 1e-10));
            }
        }
    }

    #[test]
    fn sylvester_and_lyapunov_solve_equations() {
        let a = [-2.0, 1.0, 0.0, -3.0];
        let b = [-1.0, 0.5, 0.0, -4.0];
        let x_true = [1.0, 2.0, -1.0, 3.0];
        let ax = matmul(2, 2, 2, &a, &x_true).unwrap();
        let xb = matmul(2, 2, 2, &x_true, &b).unwrap();
        let c: Vec<f64> = ax.iter().zip(xb.iter()).map(|(u, v)| u + v).collect();
        let x = sylvester(2, 2, &a, &b, &c).unwrap();
        for (u, v) in x.iter().zip(x_true.iter()) {
            assert!(close(*u, *v, 1e-10));
        }

        let q = [-4.0, 0.0, 0.0, -6.0];
        let xl = lyapunov(2, &a, &q).unwrap();
        let mut at = vec![0.0; 4];
        for i in 0..2 {
            for j in 0..2 {
                at[i * 2 + j] = a[j * 2 + i];
            }
        }
        let axl = matmul(2, 2, 2, &a, &xl).unwrap();
        let xlat = matmul(2, 2, 2, &xl, &at).unwrap();
        for k in 0..4 {
            assert!(close(axl[k] + xlat[k], q[k], 1e-10));
        }
    }

    #[test]
    fn qz_decomposes_and_finds_generalized_eigenvalues() {
        let a = [4.0, 1.0, 2.0, 3.0];
        let b = [2.0, 0.0, 0.0, 1.0];
        let (s, t, q, z, _, _) = qz(2, &a, &b).unwrap();
        let mut zt = vec![0.0; 4];
        for i in 0..2 {
            for j in 0..2 {
                zt[i * 2 + j] = z[j * 2 + i];
            }
        }
        let rec_a = matmul(2, 2, 2, &matmul(2, 2, 2, &q, &s).unwrap(), &zt).unwrap();
        let rec_b = matmul(2, 2, 2, &matmul(2, 2, 2, &q, &t).unwrap(), &zt).unwrap();
        for k in 0..4 {
            assert!(close(rec_a[k], a[k], 1e-10));
            assert!(close(rec_b[k], b[k], 1e-10));
        }
        let eigs = eigvals_generalized(2, &a, &b).unwrap();
        let mut res: Vec<f64> = eigs.iter().map(|c| c.re).collect();
        res.sort_by(f64::total_cmp);
        assert!(close(res[0], (5.0 - 5.0_f64.sqrt()) / 2.0, 1e-9));
        assert!(close(res[1], (5.0 + 5.0_f64.sqrt()) / 2.0, 1e-9));
    }
    #[test]
    fn matrix_functions_reject_empty_rectangular_shapes_and_invalid_cutoffs() {
        assert_eq!(expm(0, 2, &[]), Err(NumericError::NotSquare));
        assert_eq!(logm(0, 2, &[]), Err(NumericError::NotSquare));
        assert_eq!(sqrtm(0, 2, &[]), Err(NumericError::NotSquare));
        assert!(pinv_tol(1, 1, &[1.0], f64::NAN).is_err());
        assert!(norm(1, 1, &[f64::NAN], MatrixNorm::One).is_err());
    }
    #[test]
    fn qz_preserves_singular_pencils_and_infinite_eigenvalues() {
        // det(A-lambda B) = 6 - 3 lambda: finite eigenvalue 2 and infinity.
        let a = [2.0, 1.0, 0.0, 3.0];
        let b = [1.0, 0.0, 0.0, 0.0];
        let (s, t, q, z, alpha, beta) = qz(2, &a, &b).unwrap();
        assert!(beta.contains(&0.0));
        let eigen = eigvals_generalized(2, &a, &b).unwrap();
        assert!(eigen.iter().any(|v| v.re.is_infinite()));
        assert!(eigen.iter().any(|v| (v.re - 2.0).abs() < 1e-12));
        assert!(alpha.iter().all(|v| v.re.is_finite()));
        let zt = [z[0], z[2], z[1], z[3]];
        let ra = matmul(2, 2, 2, &matmul(2, 2, 2, &q, &s).unwrap(), &zt).unwrap();
        let rb = matmul(2, 2, 2, &matmul(2, 2, 2, &q, &t).unwrap(), &zt).unwrap();
        for i in 0..4 {
            assert!((ra[i] - a[i]).abs() < 1e-12);
            assert!((rb[i] - b[i]).abs() < 1e-12);
        }
    }
}
