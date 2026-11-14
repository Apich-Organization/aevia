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

/// Whether `a` is exactly symmetric (pattern and values) with a positive
/// diagonal: the cheap necessary conditions for a Cholesky attempt.
fn looks_spd(a: &Csr<'_>) -> bool {
    if a.rows != a.cols || a.rows == 0 {
        return false;
    }
    let entry = |i: usize, j: usize| -> Option<f64> {
        let row = &a.col_idx[a.row_ptr[i]..a.row_ptr[i + 1]];
        row.binary_search(&j)
            .ok()
            .map(|p| a.values[a.row_ptr[i] + p])
    };
    for i in 0..a.rows {
        match entry(i, i) {
            | Some(d) if d > 0.0 => {},
            | _ => return false,
        }
        for k in a.row_ptr[i]..a.row_ptr[i + 1] {
            let j = a.col_idx[k];
            if j != i && entry(j, i) != Some(a.values[k]) {
                return false;
            }
        }
    }
    true
}

/// Solves `A x = b` by sparse Cholesky (`A = L Lᵀ`, lower triangle read).
///
/// # Errors
/// [`NumericError::NotPositiveDefinite`] when `A` is not symmetric positive
/// definite (or the factorisation breaks down), plus the shape errors of
/// [`lu_solve`].
pub fn cholesky_solve(
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
    if !looks_spd(a) {
        return Err(NumericError::NotPositiveDefinite);
    }
    let n = a.rows;
    let mut triplets = Vec::with_capacity(a.values.len());
    for row in 0..n {
        for k in a.row_ptr[row]..a.row_ptr[row + 1] {
            triplets.push(Triplet::new(row, a.col_idx[k], a.values[k]));
        }
    }
    let matrix = SparseColMat::<usize, f64>::try_new_from_triplets(n, n, &triplets)
        .map_err(|_| NumericError::NotPositiveDefinite)?;
    let llt = matrix
        .sp_cholesky(faer::Side::Lower)
        .map_err(|_| NumericError::NotPositiveDefinite)?;
    let rhs = faer::Col::<f64>::from_fn(n, |i| b[i]);
    let solution = llt.solve(&rhs);
    let out: Vec<f64> = (0..n).map(|i| solution[i]).collect();
    if out.iter().any(|value| !value.is_finite()) {
        return Err(NumericError::NotPositiveDefinite);
    }
    Ok(out)
}

/// Solves `A x = b`: sparse Cholesky when `A` is symmetric positive
/// definite, otherwise (or when the factorisation fails) sparse LU. The
/// second component names the factorisation used (`"Cholesky"` or `"LU"`).
///
/// # Errors
/// As [`lu_solve`].
pub fn solve_auto(
    a: &Csr<'_>,
    b: &[f64],
) -> Result<(Vec<f64>, &'static str), NumericError> {
    if let Ok(x) = cholesky_solve(a, b) {
        return Ok((x, "Cholesky"));
    }
    lu_solve(a, b).map(|x| (x, "LU"))
}

/// The preconditioner a caller asks [`cg_precond`] for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PrecondKind {
    /// No preconditioner.
    Identity,
    /// Diagonal scaling.
    Jacobi,
    /// Inverted diagonal blocks (rssn `Bsr::block_jacobi`).
    BlockJacobi,
    /// Incomplete LU with the sparsity pattern (block ILU(0) for a block
    /// operand).
    Ilu0,
    /// Incomplete Cholesky with the sparsity pattern.
    Ic0,
    /// The strongest factorisation that succeeds.
    Auto,
}

/// The result of [`cg_precond`].
#[derive(Clone, Debug)]
pub struct PrecondCgResult {
    /// The final iterate.
    pub x: Vec<f64>,
    /// Iterations performed.
    pub iterations: usize,
    /// The relative residual `‖b − A x‖ / ‖b‖`.
    pub residual: f64,
    /// Whether the tolerance was reached.
    pub converged: bool,
    /// The preconditioner actually used: `"Identity"`, `"Jacobi"`,
    /// `"BlockJacobi"`, `"BlockIlu0"`, `"Ilu0"` or `"Ic0"`.
    pub precond: &'static str,
}

enum Built {
    Identity,
    Jacobi(Vec<f64>),
    Block(rssn::kernels::bsr::BlockJacobi),
    BlockIlu(rssn::kernels::bsr::Bilu0),
    Ilu(rssn::kernels::krylov::Ilu0),
    Ic(rssn::kernels::krylov::Ic0),
}

impl Built {
    fn apply(
        &self,
        r: &[f64],
        z: &mut [f64],
    ) {
        match self {
            | Self::Identity => z.copy_from_slice(r),
            | Self::Jacobi(inv) => {
                for ((zi, ri), di) in z.iter_mut().zip(r).zip(inv) {
                    *zi = ri * di;
                }
            },
            | Self::Block(p) => p.apply(r, z),
            | Self::BlockIlu(p) => p.apply(r, z),
            | Self::Ilu(p) => p.apply(r, z),
            | Self::Ic(p) => p.apply(r, z),
        }
    }

    const fn name(&self) -> &'static str {
        match self {
            | Self::Identity => "Identity",
            | Self::Jacobi(_) => "Jacobi",
            | Self::Block(_) => "BlockJacobi",
            | Self::BlockIlu(_) => "BlockIlu0",
            | Self::Ilu(_) => "Ilu0",
            | Self::Ic(_) => "Ic0",
        }
    }
}

/// The square block shape the block preconditioners use: the operand's own
/// block when it is square, else the automatic choice.
fn square_block(
    csr: &rssn::kernels::krylov::Csr,
    hint: Option<(usize, usize)>,
) -> (usize, usize) {
    match hint {
        | Some((r, c)) if r == c && csr.rows.is_multiple_of(r) => (r, c),
        | _ => rssn::kernels::bsr::choose_block_size(csr, &[(4, 4), (3, 3), (2, 2), (1, 1)]),
    }
}

fn build_preconditioner(
    csr: &rssn::kernels::krylov::Csr,
    kind: PrecondKind,
    block: Option<(usize, usize)>,
) -> Built {
    use rssn::kernels::bsr::Bsr;
    let jacobi = || Built::Jacobi(csr.jacobi_inverse());
    let blocked = || {
        let (br, bc) = square_block(csr, block);
        Bsr::from_csr(csr, br, bc).ok()
    };
    match kind {
        | PrecondKind::Identity => Built::Identity,
        | PrecondKind::Jacobi => jacobi(),
        | PrecondKind::BlockJacobi => {
            blocked()
                .and_then(|b| b.block_jacobi().ok())
                .map_or_else(jacobi, Built::Block)
        },
        | PrecondKind::Ilu0 => {
            if block.is_some() {
                blocked()
                    .and_then(|b| b.bilu0().ok())
                    .map(Built::BlockIlu)
                    .or_else(|| csr.ilu0().map(Built::Ilu))
                    .unwrap_or_else(jacobi)
            } else {
                csr.ilu0().map_or_else(jacobi, Built::Ilu)
            }
        },
        | PrecondKind::Ic0 => csr.ic0().map_or_else(jacobi, Built::Ic),
        | PrecondKind::Auto => {
            if block.is_some()
                && let Some(b) = blocked()
            {
                if let Ok(p) = b.bilu0() {
                    return Built::BlockIlu(p);
                }
                if let Ok(p) = b.block_jacobi() {
                    return Built::Block(p);
                }
            }
            let auto = rssn::kernels::krylov::auto_preconditioner(csr);
            match auto.kind() {
                | rssn::kernels::krylov::AutoPreconditionerKind::Ic0 => {
                    csr.ic0().map_or_else(jacobi, Built::Ic)
                },
                | rssn::kernels::krylov::AutoPreconditionerKind::Ilu0 => {
                    csr.ilu0().map_or_else(jacobi, Built::Ilu)
                },
                | rssn::kernels::krylov::AutoPreconditionerKind::Jacobi => jacobi(),
            }
        },
    }
}

/// Preconditioned conjugate gradients from a zero start.
///
/// `block` is the block shape of a block-CSR operand (`None` otherwise); it
/// selects rssn's block Jacobi / block ILU(0) for those kinds. A
/// factorisation that breaks down falls back to Jacobi, and the kind that was
/// really applied is reported in the result.
///
/// # Errors
/// [`NumericError::NotSquare`] and [`NumericError::ShapeMismatch`].
pub fn cg_precond(
    a: &Csr<'_>,
    b: &[f64],
    tolerance: f64,
    max_iterations: usize,
    kind: PrecondKind,
    block: Option<(usize, usize)>,
) -> Result<PrecondCgResult, NumericError> {
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
    let csr = rssn::kernels::krylov::Csr {
        rows: a.rows,
        cols: a.cols,
        indptr: a.row_ptr.to_vec(),
        indices: a.col_idx.to_vec(),
        values: a.values.to_vec(),
    };
    let built = build_preconditioner(&csr, kind, block);
    let result = rssn::kernels::krylov::cg(
        |x, y| csr.matvec(x, y),
        |r, z| built.apply(r, z),
        b,
        None,
        tolerance,
        max_iterations,
    );
    Ok(PrecondCgResult {
        x: result.x,
        iterations: result.iterations,
        residual: result.residual,
        converged: result.converged,
        precond: built.name(),
    })
}

/// Which Krylov method [`krylov_precond`] runs.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Krylov {
    /// Restarted GMRES(`restart`) with right preconditioning: any square
    /// nonsingular matrix; the residual norm never increases.
    Gmres {
        /// The restart length (the Krylov basis size before restarting).
        restart: usize,
    },
    /// BiCGSTAB: any square nonsingular matrix, short recurrences (constant
    /// memory), with a possibly irregular residual history.
    Bicgstab,
}

/// Preconditioned GMRES or BiCGSTAB from a zero start, for a **general**
/// (possibly nonsymmetric) square matrix.
///
/// The preconditioners are those of [`cg_precond`] except that `Ic0` and the
/// symmetric `Auto` choice need a symmetric matrix: here `Ic0` is replaced by
/// `Ilu0` and `Auto` tries `Ilu0` then `Jacobi`. A factorisation that breaks
/// down falls back to Jacobi, and the kind actually applied is reported. The
/// iteration is sequential (rssn's Krylov kernels), so the result does not
/// depend on the thread count.
///
/// # Errors
/// [`NumericError::NotSquare`] and [`NumericError::ShapeMismatch`].
pub fn krylov_precond(
    a: &Csr<'_>,
    b: &[f64],
    tolerance: f64,
    max_iterations: usize,
    method: Krylov,
    kind: PrecondKind,
    block: Option<(usize, usize)>,
) -> Result<PrecondCgResult, NumericError> {
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
    let csr = rssn::kernels::krylov::Csr {
        rows: a.rows,
        cols: a.cols,
        indptr: a.row_ptr.to_vec(),
        indices: a.col_idx.to_vec(),
        values: a.values.to_vec(),
    };
    let kind = match kind {
        | PrecondKind::Ic0 => PrecondKind::Ilu0,
        | PrecondKind::Auto if block.is_none() => PrecondKind::Ilu0,
        | other => other,
    };
    let built = build_preconditioner(&csr, kind, block);
    let apply = |x: &[f64], y: &mut [f64]| csr.matvec(x, y);
    let precondition = |r: &[f64], z: &mut [f64]| built.apply(r, z);
    let result = match method {
        | Krylov::Gmres { restart } => {
            rssn::kernels::krylov::gmres(
                apply,
                precondition,
                b,
                None,
                restart.max(1),
                tolerance,
                max_iterations,
            )
        },
        | Krylov::Bicgstab => {
            rssn::kernels::krylov::bicgstab(apply, precondition, b, None, tolerance, max_iterations)
        },
    };
    Ok(PrecondCgResult {
        x: result.x,
        iterations: result.iterations,
        residual: result.residual,
        converged: result.converged,
        precond: built.name(),
    })
}

fn to_rssn_csr(a: &Csr<'_>) -> Result<rssn::kernels::krylov::Csr, NumericError> {
    if a.rows.checked_add(1) != Some(a.row_ptr.len())
        || a.col_idx.len() != a.values.len()
        || a.row_ptr.first() != Some(&0)
        || a.row_ptr.last() != Some(&a.values.len())
        || a.row_ptr.windows(2).any(|pair| pair[0] > pair[1])
        || a.col_idx.iter().any(|&column| column >= a.cols)
    {
        return Err(NumericError::ShapeMismatch("invalid CSR arrays".into()));
    }
    if a.values.iter().any(|v| !v.is_finite()) {
        return Err(NumericError::Domain(
            "sparse matrix contains non-finite entries".into(),
        ));
    }
    Ok(rssn::kernels::krylov::Csr {
        rows: a.rows,
        cols: a.cols,
        indptr: a.row_ptr.to_vec(),
        indices: a.col_idx.to_vec(),
        values: a.values.to_vec(),
    })
}

/// Minimal Residual (MINRES) method for symmetric indefinite systems `A x = b`
/// starting from zero (Paige–Saunders algorithm).
///
/// # Errors
/// [`NumericError::NotSquare`] and [`NumericError::ShapeMismatch`].
pub fn minres(
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
    if a.rows == 0 {
        return Ok(CgResult {
            x: Vec::new(),
            iterations: 0,
            residual: 0.0,
            converged: true,
        });
    }
    let csr = to_rssn_csr(a)?;
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
        return Ok(CgResult {
            x: Vec::new(),
            iterations: 0,
            residual: 0.0,
            converged: true,
        });
    }
    let csr = to_rssn_csr(a)?;
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
        return Ok(CgResult {
            x: Vec::new(),
            iterations: 0,
            residual: 0.0,
            converged: true,
        });
    }
    let csr = to_rssn_csr(a)?;
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
        return Ok(CgResult {
            x: Vec::new(),
            iterations: 0,
            residual: 0.0,
            converged: true,
        });
    }
    let csr = to_rssn_csr(a)?;
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
