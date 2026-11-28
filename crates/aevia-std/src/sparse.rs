//! Typed CSR adapters to RSSN sparse kernels (§8.4.1).
//! Matrix products, factorizations, iterations and preconditioners reside in RSSN.
use crate::NumericError;
pub use native::CgResult;
pub use native::Krylov;
pub use native::PrecondCgResult;
pub use native::PrecondKind;
use rssn::kernels::sparse_native as native;

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
    const fn native(&self) -> native::Csr<'_> {
        native::Csr {
            rows: self.rows,
            cols: self.cols,
            row_ptr: self.row_ptr,
            col_idx: self.col_idx,
            values: self.values,
        }
    }

    /// Matrix-vector product through the shared CSR kernel.
    ///
    /// # Errors
    /// Invalid matrix storage or incompatible vector length.
    pub fn matvec(
        &self,
        x: &[f64],
    ) -> Result<Vec<f64>, NumericError> {
        self.native().matvec(x).map_err(Into::into)
    }
}

/// RSSN `lu_solve` adapter with the established Aevia result layout.
///
/// # Errors
/// Invalid dimensions, sparse data, controls, or numerical factorization failure.
pub fn lu_solve(
    a: &Csr<'_>,
    b: &[f64],
) -> Result<Vec<f64>, NumericError> {
    native::lu_solve(&a.native(), b).map_err(Into::into)
}

/// RSSN `cholesky_solve` adapter with the established Aevia result layout.
///
/// # Errors
/// Invalid dimensions, sparse data, controls, or numerical factorization failure.
pub fn cholesky_solve(
    a: &Csr<'_>,
    b: &[f64],
) -> Result<Vec<f64>, NumericError> {
    native::cholesky_solve(&a.native(), b).map_err(Into::into)
}

/// RSSN `solve_auto` adapter with the established Aevia result layout.
///
/// # Errors
/// Invalid dimensions, sparse data, controls, or numerical factorization failure.
pub fn solve_auto(
    a: &Csr<'_>,
    b: &[f64],
) -> Result<(Vec<f64>, &'static str), NumericError> {
    native::solve_auto(&a.native(), b).map_err(Into::into)
}

/// RSSN `cg` adapter with the established Aevia result layout.
///
/// # Errors
/// Invalid dimensions, sparse data, controls, or numerical factorization failure.
pub fn cg(
    a: &Csr<'_>,
    b: &[f64],
    tolerance: f64,
    max_iterations: usize,
) -> Result<CgResult, NumericError> {
    native::cg(&a.native(), b, tolerance, max_iterations).map_err(Into::into)
}

/// RSSN `cg_precond` adapter with the established Aevia result layout.
///
/// # Errors
/// Invalid dimensions, sparse data, controls, or numerical factorization failure.
pub fn cg_precond(
    a: &Csr<'_>,
    b: &[f64],
    tolerance: f64,
    max_iterations: usize,
    kind: PrecondKind,
    block: Option<(usize, usize)>,
) -> Result<PrecondCgResult, NumericError> {
    native::cg_precond(&a.native(), b, tolerance, max_iterations, kind, block).map_err(Into::into)
}

/// RSSN `krylov_precond` adapter with the established Aevia result layout.
///
/// # Errors
/// Invalid dimensions, sparse data, controls, or numerical factorization failure.
pub fn krylov_precond(
    a: &Csr<'_>,
    b: &[f64],
    tolerance: f64,
    max_iterations: usize,
    method: Krylov,
    kind: PrecondKind,
    block: Option<(usize, usize)>,
) -> Result<PrecondCgResult, NumericError> {
    native::krylov_precond(
        &a.native(),
        b,
        tolerance,
        max_iterations,
        method,
        kind,
        block,
    )
    .map_err(Into::into)
}

fn validate_csr(a: &Csr<'_>) -> Result<(), NumericError> {
    if a.rows.checked_add(1) != Some(a.row_ptr.len())
        || a.col_idx.len() != a.values.len()
        || a.row_ptr.first() != Some(&0)
        || a.row_ptr.last() != Some(&a.values.len())
        || a.row_ptr.array_windows().any(|&[lo, hi]| lo > hi)
        || a.col_idx.iter().any(|&column| column >= a.cols)
    {
        return Err(NumericError::ShapeMismatch("invalid CSR arrays".into()));
    }
    if a.values.iter().any(|v| !v.is_finite()) {
        return Err(NumericError::Domain(
            "sparse matrix contains non-finite entries".into(),
        ));
    }
    Ok(())
}

fn to_rssn_csr(a: &Csr<'_>) -> Result<rssn::kernels::krylov::Csr, NumericError> {
    validate_csr(a)?;
    Ok(rssn::kernels::krylov::Csr {
        rows: a.rows,
        cols: a.cols,
        indptr: a.row_ptr.to_vec(),
        indices: a.col_idx.to_vec(),
        values: a.values.to_vec(),
    })
}

fn prepare_iterative_system(
    a: &Csr<'_>,
    b: &[f64],
) -> Result<Option<rssn::kernels::krylov::Csr>, NumericError> {
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
    if a.rows == 0 {
        validate_csr(a)?;
        return Ok(None);
    }
    to_rssn_csr(a).map(Some)
}

const fn empty_iterative_solution() -> CgResult {
    CgResult {
        x: Vec::new(),
        iterations: 0,
        residual: 0.0,
        converged: true,
    }
}

/// Minimal Residual (MINRES) method for symmetric indefinite systems `A x = b`
/// starting from zero (Paige–Saunders algorithm).
///
/// # Errors
/// [`NumericError::NotSquare`] for a rectangular matrix,
/// [`NumericError::ShapeMismatch`] for malformed CSR storage or an incompatible
/// right-hand side, and [`NumericError::Domain`] for non-finite matrix values.
pub fn minres(
    a: &Csr<'_>,
    b: &[f64],
    tolerance: f64,
    max_iterations: usize,
) -> Result<CgResult, NumericError> {
    let Some(csr) = prepare_iterative_system(a, b)? else {
        return Ok(empty_iterative_solution());
    };
    let result = rssn::kernels::krylov_ext::minres(
        |x, y| csr.matvec(x, y),
        |x, y| y.copy_from_slice(x),
        b,
        None,
        tolerance,
        max_iterations,
    );
    Ok(CgResult {
        x: result.x,
        iterations: result.iterations,
        residual: result.residual,
        converged: result.converged,
    })
}

/// Which part of the spectrum a sparse eigensolver targets.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SpectrumPart {
    /// Eigenvalues of largest modulus.
    LargestMagnitude,
    /// Eigenvalues of smallest modulus.
    SmallestMagnitude,
    /// Largest real part (largest algebraic for symmetric matrices).
    LargestReal,
    /// Smallest real part (smallest algebraic for symmetric matrices).
    SmallestReal,
    /// Largest absolute imaginary part.
    LargestImag,
}

impl From<SpectrumPart> for rssn::kernels::eigs::Which {
    fn from(part: SpectrumPart) -> Self {
        match part {
            | SpectrumPart::LargestMagnitude => Self::LargestMagnitude,
            | SpectrumPart::SmallestMagnitude => Self::SmallestMagnitude,
            | SpectrumPart::LargestReal => Self::LargestReal,
            | SpectrumPart::SmallestReal => Self::SmallestReal,
            | SpectrumPart::LargestImag => Self::LargestImag,
        }
    }
}

/// Result of a sparse eigensolver (`eigs_sym`, `eigs`, `eigs_shift_invert`).
#[derive(Clone, Debug)]
pub struct SparseEigsResult {
    /// Eigenvalues `(re, im)`.
    pub values: Vec<crate::Complex>,
    /// Real parts of the eigenvectors.
    pub vectors_re: Vec<Vec<f64>>,
    /// Imaginary parts of the eigenvectors.
    pub vectors_im: Vec<Vec<f64>>,
    /// Residual norms `‖A x − λ x‖`.
    pub residuals: Vec<f64>,
    /// Whether all requested eigenpairs converged.
    pub converged: bool,
    /// Number of sparse matrix-vector products performed.
    pub matvecs: usize,
}

fn map_eigs_result(r: rssn::kernels::eigs::EigsResult) -> SparseEigsResult {
    SparseEigsResult {
        values: r
            .values
            .into_iter()
            .map(|(re, im)| crate::Complex::new(re, im))
            .collect(),
        vectors_re: r.vectors_re,
        vectors_im: r.vectors_im,
        residuals: r.residuals,
        converged: r.converged,
        matvecs: r.matvecs,
    }
}

/// Thick-restart Lanczos eigensolver for the `k` wanted eigenpairs of a
/// symmetric sparse matrix (`rssn::kernels::eigs::eigs_sym`).
///
/// # Errors
/// [`NumericError::NotSquare`] or [`NumericError::Domain`] when `k == 0` or `k > rows`.
pub fn eigs_sym(
    a: &Csr<'_>,
    k: usize,
    which: SpectrumPart,
    tol: f64,
    max_restarts: usize,
) -> Result<SparseEigsResult, NumericError> {
    if a.rows != a.cols {
        return Err(NumericError::NotSquare);
    }
    if k == 0 || k > a.rows {
        return Err(NumericError::Domain(format!(
            "eigs_sym requires 1 <= k <= {}, got {k}",
            a.rows
        )));
    }
    let csr = to_rssn_csr(a)?;
    let opts = rssn::kernels::eigs::EigsOptions {
        ncv: 0,
        tol,
        max_restarts,
        seed: 0x5eed,
    };
    let res =
        rssn::kernels::eigs::eigs_sym(|x, y| csr.matvec(x, y), a.rows, k, which.into(), &opts)
            .map_err(|_| NumericError::NotConverged)?;
    Ok(map_eigs_result(res))
}

/// Implicitly restarted Arnoldi eigensolver for the `k` wanted eigenpairs of a
/// general (possibly nonsymmetric) sparse matrix (`rssn::kernels::eigs::eigs`).
///
/// # Errors
/// [`NumericError::NotSquare`] or [`NumericError::Domain`] when `k == 0` or `k > rows`.
pub fn eigs(
    a: &Csr<'_>,
    k: usize,
    which: SpectrumPart,
    tol: f64,
    max_restarts: usize,
) -> Result<SparseEigsResult, NumericError> {
    if a.rows != a.cols {
        return Err(NumericError::NotSquare);
    }
    if k == 0 || k > a.rows {
        return Err(NumericError::Domain(format!(
            "eigs requires 1 <= k <= {}, got {k}",
            a.rows
        )));
    }
    let csr = to_rssn_csr(a)?;
    let opts = rssn::kernels::eigs::EigsOptions {
        ncv: 0,
        tol,
        max_restarts,
        seed: 0x5eed,
    };
    let res = rssn::kernels::eigs::eigs(|x, y| csr.matvec(x, y), a.rows, k, which.into(), &opts)
        .map_err(|_| NumericError::NotConverged)?;
    Ok(map_eigs_result(res))
}

/// Shift-invert sparse eigensolver for the `k` eigenvalues of `A` closest to
/// `sigma`, factoring `(A − σ I)` with sparse LU.
///
/// # Errors
/// [`NumericError::NotSquare`], [`NumericError::Domain`], or [`NumericError::Singular`]
/// if `A − σ I` is singular.
pub fn eigs_shift_invert(
    a: &Csr<'_>,
    k: usize,
    sigma: f64,
    symmetric: bool,
    tol: f64,
    max_restarts: usize,
) -> Result<SparseEigsResult, NumericError> {
    if a.rows != a.cols {
        return Err(NumericError::NotSquare);
    }
    if k == 0 || k > a.rows {
        return Err(NumericError::Domain(format!(
            "eigs_shift_invert requires 1 <= k <= {}, got {k}",
            a.rows
        )));
    }
    let csr = to_rssn_csr(a)?;
    let opts = rssn::kernels::eigs::EigsOptions {
        ncv: 0,
        tol,
        max_restarts,
        seed: 0x5eed,
    };
    let res = rssn::kernels::eigs::eigs_csr_shift_invert(&csr, k, sigma, symmetric, &opts)
        .map_err(|error| {
            match error {
                | rssn::kernels::eigs::ShiftInvertError::Invalid => {
                    NumericError::Domain("invalid shift-invert parameters".into())
                },
                | rssn::kernels::eigs::ShiftInvertError::Singular => NumericError::Singular,
                | rssn::kernels::eigs::ShiftInvertError::Eigen(_) => NumericError::NotConverged,
            }
        })?;
    Ok(map_eigs_result(res))
}

/// Solves a symmetric positive-definite sparse system `A x = b` using smoothed-aggregation
/// Algebraic Multigrid (`rssn::kernels::multigrid::Amg`) as a standalone solver.
///
/// # Errors
/// [`NumericError::NotSquare`], [`NumericError::ShapeMismatch`], or
/// [`NumericError::NotPositiveDefinite`] if AMG setup fails.
pub fn amg_solve(
    a: &Csr<'_>,
    b: &[f64],
    tolerance: f64,
    max_cycles: usize,
) -> Result<CgResult, NumericError> {
    let Some(csr) = prepare_iterative_system(a, b)? else {
        return Ok(empty_iterative_solution());
    };
    let opts = rssn::kernels::multigrid::AmgOptions::default();
    let amg = rssn::kernels::multigrid::Amg::build(&csr, &opts)
        .map_err(|_| NumericError::NotPositiveDefinite)?;
    let res = amg.solve(b, None, tolerance, max_cycles);
    let residual = res.history.last().copied().unwrap_or(0.0);
    Ok(CgResult {
        x: res.x,
        iterations: res.history.len().saturating_sub(1),
        residual,
        converged: res.converged,
    })
}

/// Solves a general (possibly nonsymmetric) sparse system `A x = b` using
/// Ruge–Stüben Algebraic Multigrid (`rssn::kernels::amg_general::AmgGeneral`).
///
/// # Errors
/// [`NumericError::NotSquare`], [`NumericError::ShapeMismatch`], or
/// [`NumericError::Domain`] if AMG setup fails.
pub fn amg_general_solve(
    a: &Csr<'_>,
    b: &[f64],
    tolerance: f64,
    max_cycles: usize,
) -> Result<CgResult, NumericError> {
    let Some(csr) = prepare_iterative_system(a, b)? else {
        return Ok(empty_iterative_solution());
    };
    let opts = rssn::kernels::amg_general::AmgGeneralOptions::default();
    let amg = rssn::kernels::amg_general::AmgGeneral::build(&csr, &opts)
        .map_err(|_| NumericError::Domain("AMG setup requires positive diagonal".to_string()))?;
    let res = amg.solve(b, None, tolerance, max_cycles);
    let residual = res.history.last().copied().unwrap_or(0.0);
    Ok(CgResult {
        x: res.x,
        iterations: res.history.len().saturating_sub(1),
        residual,
        converged: res.converged,
    })
}

/// Conjugate gradients preconditioned with smoothed-aggregation AMG.
///
/// # Errors
/// [`NumericError::NotSquare`], [`NumericError::ShapeMismatch`], or
/// [`NumericError::NotPositiveDefinite`] if AMG setup fails.
pub fn cg_amg(
    a: &Csr<'_>,
    b: &[f64],
    tolerance: f64,
    max_iterations: usize,
) -> Result<CgResult, NumericError> {
    let Some(csr) = prepare_iterative_system(a, b)? else {
        return Ok(empty_iterative_solution());
    };
    let opts = rssn::kernels::multigrid::AmgOptions::default();
    let amg = rssn::kernels::multigrid::Amg::build(&csr, &opts)
        .map_err(|_| NumericError::NotPositiveDefinite)?;
    let apply = |x: &[f64], y: &mut [f64]| csr.matvec(x, y);
    let precondition = |r: &[f64], z: &mut [f64]| amg.precondition(r, z);
    let res = rssn::kernels::krylov::cg(apply, precondition, b, None, tolerance, max_iterations);
    Ok(CgResult {
        x: res.x,
        iterations: res.iterations,
        residual: res.residual,
        converged: res.converged,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    type IterativeSolver = fn(&Csr<'_>, &[f64], f64, usize) -> Result<CgResult, NumericError>;

    const ITERATIVE_SOLVERS: [(&str, IterativeSolver); 4] = [
        ("minres", minres),
        ("amg_solve", amg_solve),
        ("amg_general_solve", amg_general_solve),
        ("cg_amg", cg_amg),
    ];

    #[test]
    fn empty_iterative_systems_require_canonical_csr_storage() {
        let empty = Csr {
            rows: 0,
            cols: 0,
            row_ptr: &[0],
            col_idx: &[],
            values: &[],
        };
        let malformed = [
            Csr {
                row_ptr: &[],
                ..empty
            },
            Csr {
                row_ptr: &[1],
                ..empty
            },
            Csr {
                row_ptr: &[0, 0],
                ..empty
            },
            Csr {
                col_idx: &[0],
                ..empty
            },
            Csr {
                values: &[f64::NAN],
                ..empty
            },
            Csr {
                row_ptr: &[1],
                col_idx: &[0],
                values: &[1.0],
                ..empty
            },
        ];
        for (name, solve) in ITERATIVE_SOLVERS {
            let report = solve(&empty, &[], 1e-10, 0).unwrap();
            assert!(
                report.x.is_empty() && report.converged,
                "{name}: {report:?}"
            );
            assert_eq!(report.iterations, 0, "{name}");
            assert_eq!(report.residual.to_bits(), 0.0_f64.to_bits(), "{name}");
            for bad in malformed {
                let error = solve(&bad, &[], 1e-10, 0).unwrap_err();
                assert_eq!(error, bad.matvec(&[]).unwrap_err(), "{name}: {bad:?}");
                assert!(
                    matches!(error, NumericError::ShapeMismatch(_)),
                    "{name}: {bad:?}"
                );
            }
        }
    }

    #[test]
    fn iterative_systems_preserve_preflight_error_order() {
        let malformed = Csr {
            rows: 0,
            cols: 0,
            row_ptr: &[],
            col_idx: &[],
            values: &[],
        };
        let rectangular = Csr { cols: 1, ..malformed };
        let non_finite = Csr {
            rows: 1,
            cols: 1,
            row_ptr: &[0, 1],
            col_idx: &[0],
            values: &[f64::INFINITY],
        };
        for (name, solve) in ITERATIVE_SOLVERS {
            assert_eq!(
                solve(&rectangular, &[1.0], 1e-10, 1).unwrap_err(),
                NumericError::NotSquare,
                "{name}"
            );
            assert_eq!(
                solve(&malformed, &[1.0], 1e-10, 1).unwrap_err(),
                NumericError::ShapeMismatch(
                    "matrix has 0 rows, right-hand side has 1 entries".into()
                ),
                "{name}"
            );
            assert!(
                matches!(
                    solve(&non_finite, &[1.0], 1e-10, 1),
                    Err(NumericError::Domain(_))
                ),
                "{name}"
            );
        }
    }

    #[test]
    fn iterative_diagonal_solves_match_componentwise_reference() {
        let diagonal = [2.0, 4.0, 8.0, 16.0];
        let a = Csr {
            rows: 4,
            cols: 4,
            row_ptr: &[0, 1, 2, 3, 4],
            col_idx: &[0, 1, 2, 3],
            values: &diagonal,
        };
        let b = [2.0, -8.0, 4.0, 64.0];
        for (name, solve) in ITERATIVE_SOLVERS {
            let report = solve(&a, &b, 1e-10, 30).unwrap();
            assert!(report.converged, "{name}: {report:?}");
            assert_eq!(report.x.len(), b.len(), "{name}");
            for ((value, rhs), scale) in report.x.iter().zip(b).zip(diagonal) {
                let expected = rhs / scale;
                assert!(
                    (value - expected).abs() < 1e-9,
                    "{name}: {value} vs {expected}"
                );
            }
        }
    }

    #[test]
    fn kernel_validates_borrowed_csr_before_indexing_and_zero_block_hints() {
        let malformed = Csr {
            rows: 1,
            cols: 1,
            row_ptr: &[0, 2],
            col_idx: &[0],
            values: &[1.0],
        };
        assert!(matches!(
            malformed.matvec(&[1.0]),
            Err(NumericError::ShapeMismatch(_))
        ));
        assert!(matches!(
            lu_solve(&malformed, &[1.0]),
            Err(NumericError::ShapeMismatch(_))
        ));
        let valid = Csr {
            rows: 2,
            cols: 2,
            row_ptr: &[0, 1, 2],
            col_idx: &[0, 1],
            values: &[2.0, 3.0],
        };
        let result = cg_precond(
            &valid,
            &[2.0, 3.0],
            1e-12,
            10,
            PrecondKind::BlockJacobi,
            Some((0, 0)),
        )
        .unwrap();
        assert!(result.converged);
    }

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

    fn poisson2d(m: usize) -> (Vec<usize>, Vec<usize>, Vec<f64>) {
        let (mut ptr, mut idx, mut val) = (vec![0], Vec::new(), Vec::new());
        for y in 0..m {
            for x in 0..m {
                let i = y * m + x;
                if y > 0 {
                    idx.push(i - m);
                    val.push(-1.0);
                }
                if x > 0 {
                    idx.push(i - 1);
                    val.push(-1.0);
                }
                idx.push(i);
                val.push(4.0);
                if x + 1 < m {
                    idx.push(i + 1);
                    val.push(-1.0);
                }
                if y + 1 < m {
                    idx.push(i + m);
                    val.push(-1.0);
                }
                ptr.push(idx.len());
            }
        }
        (ptr, idx, val)
    }

    #[test]
    fn cholesky_matches_lu_on_2d_poisson() {
        let m = 12;
        let (ptr, idx, val) = poisson2d(m);
        let n = m * m;
        let a = Csr {
            rows: n,
            cols: n,
            row_ptr: &ptr,
            col_idx: &idx,
            values: &val,
        };
        let b: Vec<f64> = (0..n).map(|i| 1.0 + (i % 5) as f64).collect();
        let lu = lu_solve(&a, &b).expect("lu");
        let llt = cholesky_solve(&a, &b).expect("cholesky");
        for (p, q) in lu.iter().zip(&llt) {
            assert!((p - q).abs() < 1e-10);
        }
        let (auto, how) = solve_auto(&a, &b).expect("auto");
        assert_eq!(how, "Cholesky");
        assert!(auto.iter().zip(&lu).all(|(p, q)| (p - q).abs() < 1e-10));
    }

    #[test]
    fn indefinite_matrices_fall_back_to_lu() {
        let (ptr, idx, val) = (vec![0, 2, 4], vec![0, 1, 0, 1], vec![1.0, 2.0, 2.0, 1.0]);
        let a = Csr {
            rows: 2,
            cols: 2,
            row_ptr: &ptr,
            col_idx: &idx,
            values: &val,
        };
        assert_eq!(
            cholesky_solve(&a, &[1.0, 1.0]),
            Err(NumericError::NotPositiveDefinite)
        );
        let (x, how) = solve_auto(&a, &[3.0, 3.0]).expect("lu");
        assert_eq!(how, "LU");
        assert!((x[0] - 1.0).abs() < 1e-12 && (x[1] - 1.0).abs() < 1e-12);
    }

    #[test]
    fn every_preconditioner_converges_on_poisson() {
        let m = 10;
        let (ptr, idx, val) = poisson2d(m);
        let n = m * m;
        let a = Csr {
            rows: n,
            cols: n,
            row_ptr: &ptr,
            col_idx: &idx,
            values: &val,
        };
        let b = vec![1.0; n];
        let plain = cg_precond(&a, &b, 1e-10, 500, PrecondKind::Identity, None).expect("cg");
        assert!(plain.converged);
        assert_eq!(plain.precond, "Identity");
        for (kind, name, block) in [
            (PrecondKind::Jacobi, "Jacobi", None),
            (PrecondKind::BlockJacobi, "BlockJacobi", None),
            (PrecondKind::Ilu0, "Ilu0", None),
            (PrecondKind::Ic0, "Ic0", None),
            (PrecondKind::Auto, "Ic0", None),
            (PrecondKind::Ilu0, "BlockIlu0", Some((2, 2))),
            (PrecondKind::Auto, "BlockIlu0", Some((2, 2))),
        ] {
            let r = cg_precond(&a, &b, 1e-10, 500, kind, block).expect("cg");
            assert!(r.converged, "{name}");
            assert_eq!(r.precond, name);
            assert!(r.iterations <= plain.iterations, "{name}");
            assert!(r.x.iter().zip(&plain.x).all(|(p, q)| (p - q).abs() < 1e-6));
        }
    }

    /// A nonsymmetric, diagonally dominant convection-diffusion-like matrix.
    fn convection(n: usize) -> (Vec<usize>, Vec<usize>, Vec<f64>) {
        let (mut ptr, mut idx, mut val) = (vec![0], Vec::new(), Vec::new());
        for i in 0..n {
            if i > 0 {
                idx.push(i - 1);
                val.push(-1.6);
            }
            idx.push(i);
            val.push(4.0);
            if i + 1 < n {
                idx.push(i + 1);
                val.push(-0.4);
            }
            ptr.push(idx.len());
        }
        (ptr, idx, val)
    }

    #[test]
    fn gmres_and_bicgstab_solve_a_nonsymmetric_system() {
        let n = 300;
        let (ptr, idx, val) = convection(n);
        let a = Csr {
            rows: n,
            cols: n,
            row_ptr: &ptr,
            col_idx: &idx,
            values: &val,
        };
        let b: Vec<f64> = (0..n).map(|i| 1.0 + (i % 7) as f64).collect();
        let reference = lu_solve(&a, &b).unwrap();
        for method in [Krylov::Gmres { restart: 40 }, Krylov::Bicgstab] {
            for kind in [
                PrecondKind::Identity,
                PrecondKind::Jacobi,
                PrecondKind::Ilu0,
            ] {
                let result = krylov_precond(&a, &b, 1e-12, 1000, method, kind, None).unwrap();
                assert!(result.converged, "{method:?} {kind:?}: {}", result.residual);
                let error = result
                    .x
                    .iter()
                    .zip(&reference)
                    .map(|(x, r)| (x - r).abs())
                    .fold(0.0, f64::max);
                assert!(error < 1e-9, "{method:?} {kind:?}: error {error}");
                assert_eq!(
                    result.precond,
                    match kind {
                        | PrecondKind::Identity => "Identity",
                        | PrecondKind::Jacobi => "Jacobi",
                        | _ => "Ilu0",
                    }
                );
            }
        }
        // ILU(0) of a tridiagonal matrix is its exact LU, so it converges in
        // one iteration where the unpreconditioned method needs many.
        let plain = krylov_precond(
            &a,
            &b,
            1e-12,
            1000,
            Krylov::Gmres { restart: 40 },
            PrecondKind::Identity,
            None,
        )
        .unwrap();
        let ilu = krylov_precond(
            &a,
            &b,
            1e-12,
            1000,
            Krylov::Gmres { restart: 40 },
            PrecondKind::Ilu0,
            None,
        )
        .unwrap();
        assert!(
            ilu.iterations < plain.iterations,
            "{} vs {}",
            ilu.iterations,
            plain.iterations
        );
        // `Ic0` and `Auto` are replaced by ILU(0) for a general matrix.
        let auto = krylov_precond(
            &a,
            &b,
            1e-12,
            1000,
            Krylov::Bicgstab,
            PrecondKind::Auto,
            None,
        )
        .unwrap();
        assert_eq!(auto.precond, "Ilu0");
    }

    #[test]
    fn krylov_solvers_reject_bad_shapes_and_report_non_convergence() {
        let (ptr, idx, val) = convection(10);
        let square = Csr {
            rows: 10,
            cols: 10,
            row_ptr: &ptr,
            col_idx: &idx,
            values: &val,
        };
        assert!(matches!(
            krylov_precond(
                &square,
                &[1.0; 9],
                1e-8,
                10,
                Krylov::Bicgstab,
                PrecondKind::Identity,
                None
            ),
            Err(NumericError::ShapeMismatch(_))
        ));
        let wide = Csr {
            rows: 10,
            cols: 11,
            ..square
        };
        assert!(matches!(
            krylov_precond(
                &wide,
                &[1.0; 10],
                1e-8,
                10,
                Krylov::Gmres { restart: 5 },
                PrecondKind::Identity,
                None
            ),
            Err(NumericError::NotSquare)
        ));
        // Too few iterations is `converged = false`, not an error.
        let (ptr, idx, val) = convection(200);
        let big = Csr {
            rows: 200,
            cols: 200,
            row_ptr: &ptr,
            col_idx: &idx,
            values: &val,
        };
        let result = krylov_precond(
            &big,
            &[1.0; 200],
            1e-14,
            2,
            Krylov::Gmres { restart: 1 },
            PrecondKind::Identity,
            None,
        )
        .unwrap();
        assert!(!result.converged);
    }

    #[test]
    fn minres_solves_symmetric_indefinite_system() {
        // Diagonal + tridiagonal shifted to be indefinite: diag = [-4, -1, 2, 5, 8] with -0.5 off-diagonals.
        let n = 5;
        let (mut ptr, mut idx, mut val) = (vec![0], Vec::new(), Vec::new());
        let diag = [-4.0, -1.0, 2.0, 5.0, 8.0];
        for i in 0..n {
            if i > 0 {
                idx.push(i - 1);
                val.push(-0.5);
            }
            idx.push(i);
            val.push(diag[i]);
            if i + 1 < n {
                idx.push(i + 1);
                val.push(-0.5);
            }
            ptr.push(idx.len());
        }
        let a = Csr {
            rows: n,
            cols: n,
            row_ptr: &ptr,
            col_idx: &idx,
            values: &val,
        };
        let b = [1.0, -2.0, 3.0, -1.0, 4.0];
        let direct = lu_solve(&a, &b).unwrap();
        let res = minres(&a, &b, 1e-12, 100).unwrap();
        assert!(res.converged, "minres residual: {}", res.residual);
        for (p, q) in direct.iter().zip(&res.x) {
            assert!((p - q).abs() < 1e-8);
        }
    }

    #[test]
    fn sparse_eigs_finds_extremal_and_shifted_eigenvalues() {
        let n = 20;
        let (ptr, idx, val) = tridiagonal(n);
        let a = Csr {
            rows: n,
            cols: n,
            row_ptr: &ptr,
            col_idx: &idx,
            values: &val,
        };
        // Largest eigenvalue of n-point 1D Laplacian is 4 * cos^2(pi / (2*(n+1)))
        let sym = eigs_sym(&a, 2, SpectrumPart::LargestReal, 1e-9, 200).unwrap();
        assert!(sym.converged);
        let expected_max = 4.0
            * (std::f64::consts::PI / (2.0 * (n as f64 + 1.0)))
                .cos()
                .powi(2);
        assert!((sym.values[0].re - expected_max).abs() < 1e-7);

        let general = eigs(&a, 1, SpectrumPart::LargestMagnitude, 1e-9, 200).unwrap();
        assert!(general.converged);
        assert!((general.values[0].re - expected_max).abs() < 1e-7);

        // Shift-invert near 2.0
        let shifted = eigs_shift_invert(&a, 2, 2.01, true, 1e-9, 200).unwrap();
        assert!(shifted.converged);
        assert!((shifted.values[0].re - 2.0).abs() < 0.2);
    }

    #[test]
    fn amg_solvers_solve_2d_poisson() {
        let m = 12;
        let n = m * m;
        let (ptr, idx, val) = poisson2d(m);
        let a = Csr {
            rows: n,
            cols: n,
            row_ptr: &ptr,
            col_idx: &idx,
            values: &val,
        };
        let b = vec![1.0; n];
        let direct = cholesky_solve(&a, &b).unwrap();

        let amg = amg_solve(&a, &b, 1e-10, 100).unwrap();
        assert!(amg.converged);
        for (p, q) in direct.iter().zip(&amg.x) {
            assert!((p - q).abs() < 1e-6);
        }

        let amg_gen = amg_general_solve(&a, &b, 1e-10, 100).unwrap();
        assert!(amg_gen.converged);
        for (p, q) in direct.iter().zip(&amg_gen.x) {
            assert!((p - q).abs() < 1e-6);
        }

        let pcg_amg = cg_amg(&a, &b, 1e-10, 100).unwrap();
        assert!(pcg_amg.converged);
        for (p, q) in direct.iter().zip(&pcg_amg.x) {
            assert!((p - q).abs() < 1e-6);
        }
    }
    #[test]
    fn shift_invert_subtracts_shift_once_when_csr_contains_duplicate_diagonals() {
        let a = Csr {
            rows: 3,
            cols: 3,
            row_ptr: &[0, 2, 3, 4],
            col_idx: &[0, 0, 1, 2],
            values: &[1.0, 1.0, 5.0, 9.0],
        };
        let result = eigs_shift_invert(&a, 1, 1.8, true, 1e-10, 30).unwrap();
        assert!(result.converged);
        assert!((result.values[0].re - 2.0).abs() < 1e-10);
        assert!(result.residuals[0] < 1e-10);
        let malformed = Csr {
            rows: 1,
            cols: 1,
            row_ptr: &[0, 2],
            col_idx: &[0],
            values: &[1.0],
        };
        assert!(minres(&malformed, &[1.0], 1e-8, 10).is_err());
    }
}
