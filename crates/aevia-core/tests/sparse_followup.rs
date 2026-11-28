//! Sparse follow-ups (§8.4.1): preconditioned `cg_with`, the Cholesky-first
//! `spsolve`, and slicing of sparse tensors.

#![allow(clippy::result_large_err)]

use aevia_core::Interp;
use aevia_core::Mode;
use aevia_core::compile;
use aevia_core::driver;
use std::path::PathBuf;

/// Runs `main` in both evaluators and returns the (agreeing) output.
fn run(source: &str) -> String {
    let compiled = compile("sparse_followup.aev", source);
    assert!(
        !compiled.diagnostics.has_errors(),
        "{:#?}",
        compiled.diagnostics.items
    );
    let module = compiled.module.expect("a module");
    let mut outputs = Vec::new();
    for mode in [Mode::Ast, Mode::Ir] {
        let mut interp = Interp::new(&compiled.checked).with_buffer().with_mode(mode);
        let diagnostics = interp.load(&module);
        assert!(
            !diagnostics.has_errors(),
            "{mode:?}: {:?}",
            diagnostics.items
        );
        if let Err(error) = interp.run_main() {
            panic!("{mode:?}: {}", error.diagnostic().message);
        }
        outputs.push(interp.captured());
    }
    assert_eq!(outputs[0], outputs[1], "AST and IR runs disagree");
    outputs.remove(0)
}

fn runtime_code(source: &str) -> String {
    let compiled = compile("sparse_followup_runtime.aev", source);
    assert!(
        !compiled.diagnostics.has_errors(),
        "{:#?}",
        compiled.diagnostics.items
    );
    let module = compiled.module.expect("a module");
    let mut interp = Interp::new(&compiled.checked).with_buffer();
    let diagnostics = interp.load(&module);
    assert!(!diagnostics.has_errors(), "{:?}", diagnostics.items);
    match interp.run_main() {
        | Ok(_) => panic!("the program was expected to fail"),
        | Err(error) => error.code.to_string(),
    }
}

/// A program that imports standard-library modules (`import LinearAlgebra.Sparse
/// { … }`) needs the module loader, so it is written to disk and run through
/// the driver.
struct Scratch(PathBuf);

impl Scratch {
    fn new(name: &str) -> Self {
        let mut path = std::env::temp_dir();
        path.push(format!(
            "aevia-sparse-followup-{name}-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&path);
        std::fs::create_dir_all(&path).expect("create scratch directory");
        Self(path)
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// Loads the program through the module loader and returns its error codes.
fn driver_errors(
    name: &str,
    source: &str,
) -> Vec<String> {
    let scratch = Scratch::new(name);
    let entry = scratch.0.join("main.aev");
    std::fs::write(&entry, source).expect("write program");
    let program = driver::load(&entry);
    program
        .diagnostics
        .items
        .iter()
        .filter(|item| item.code.starts_with('E'))
        .map(|item| format!("{}: {}", item.code, item.message))
        .collect()
}

/// Loads and runs `main` through the module loader, returning its output.
fn run_driver(
    name: &str,
    source: &str,
) -> String {
    let scratch = Scratch::new(name);
    let entry = scratch.0.join("main.aev");
    std::fs::write(&entry, source).expect("write program");
    let program = driver::load(&entry);
    assert!(
        !program.diagnostics.has_errors(),
        "the program does not load: {:?}",
        program
            .diagnostics
            .items
            .iter()
            .map(|item| (&item.code, &item.message))
            .collect::<Vec<_>>()
    );
    let module = program.entry().expect("the entry module");
    let mut interp = Interp::new(&module.checked).with_buffer();
    let diagnostics = driver::run_modules(&mut interp, &program);
    assert!(
        !diagnostics.has_errors(),
        "the program does not run: {:?}",
        diagnostics
            .items
            .iter()
            .map(|item| (&item.code, &item.message))
            .collect::<Vec<_>>()
    );
    interp.run_main().expect("`main` runs");
    interp.captured()
}

/// `sparse_coo(...)` source of the 5-point Laplacian on an `m x m` grid.
fn poisson_source(m: usize) -> String {
    let n = m * m;
    let (mut coords, mut values) = (Vec::new(), Vec::new());
    for y in 0..m {
        for x in 0..m {
            let i = y * m + x;
            let mut push = |j: usize, v: &str| {
                coords.push(format!("({i}, {j})"));
                values.push(v.to_string());
            };
            if y > 0 {
                push(i - m, "-1.0");
            }
            if x > 0 {
                push(i - 1, "-1.0");
            }
            push(i, "4.0");
            if x + 1 < m {
                push(i + 1, "-1.0");
            }
            if y + 1 < m {
                push(i + m, "-1.0");
            }
        }
    }
    format!(
        "sparse_coo([{n}, {n}], [{}], [{}])",
        coords.join(", "),
        values.join(", ")
    )
}

/// A right-hand side literal of length `n` with a varied pattern.
fn rhs_source(n: usize) -> String {
    let items: Vec<String> = (0..n).map(|i| format!("{}.0", 1 + (i * 7) % 5)).collect();
    format!("[{}]", items.join(", "))
}

const IMPORT: &str = "import LinearAlgebra.Sparse { Precond, Identity, Jacobi, BlockJacobi, Ilu0, Ic0, Auto, cg_with }";

#[test]
fn cg_with_every_preconditioner() {
    let poisson = poisson_source(4);
    let rhs36 = rhs_source(16);
    let out = run_driver(
        "every",
        &format!(
            r#"{IMPORT}

fn main(): IO Unit = do
    let a = convert<Csr>({poisson})
    let b = {rhs36}
    let plain = cg(a, b, 1e-10, 500)
    let id = cg_with(a, b, {{ tol = 1e-10, maxiter = 500, precond = Identity }})
    let jac = cg_with(a, b, {{ tol = 1e-10, maxiter = 500, precond = Jacobi }})
    let blk = cg_with(a, b, {{ tol = 1e-10, maxiter = 500, precond = BlockJacobi }})
    let ilu = cg_with(a, b, {{ tol = 1e-10, maxiter = 500, precond = Ilu0 }})
    let ic = cg_with(a, b, {{ tol = 1e-10, maxiter = 500, precond = Ic0 }})
    let auto = cg_with(a, b, {{ tol = 1e-10, maxiter = 500, precond = Auto }})
    println("{{id.precond}} {{jac.precond}} {{blk.precond}} {{ilu.precond}} {{ic.precond}} {{auto.precond}}")
    println("{{id.converged}} {{ic.converged}} {{auto.converged}} {{plain.converged}}")
    println("{{ilu.converged}} {{blk.converged}} {{auto.iterations == ic.iterations}}")
    let x = spsolve(a, b)
    println("{{norm(plain.x - x) < 1e-8}} {{norm(ic.x - x) < 1e-8}} {{norm(blk.x - x) < 1e-8}} {{norm(jac.x - x) < 1e-8}}")
"#
        ),
    );
    assert_eq!(
        out,
        "Identity Jacobi BlockJacobi Ilu0 Ic0 Ic0\ntrue true true true\ntrue true true\ntrue true true true\n"
    );
}

#[test]
fn cg_with_uses_block_preconditioners_on_bcsr() {
    let poisson = poisson_source(4);
    let rhs36 = rhs_source(16);
    let out = run_driver(
        "bcsr",
        &format!(
            r#"{IMPORT}

fn main(): IO Unit = do
    let a = to_bcsr({poisson}, 2, 2)
    let b = {rhs36}
    let blk = cg_with(a, b, {{ tol = 1e-10, maxiter = 500, precond = BlockJacobi }})
    let ilu = cg_with(a, b, {{ tol = 1e-10, maxiter = 500, precond = Ilu0 }})
    let auto = cg_with(a, b, {{ tol = 1e-10, maxiter = 500, precond = Auto }})
    let ic = cg_with(a, b, {{ tol = 1e-10, maxiter = 500, precond = Ic0 }})
    let id = cg_with(a, b, {{ tol = 1e-10, maxiter = 500, precond = Identity }})
    println("{{format_of(a)}} {{blk.precond}} {{ilu.precond}} {{auto.precond}} {{ic.precond}}")
    println("{{ilu.converged}} {{blk.converged}}")
    let x = spsolve(a, b)
    println("{{norm(blk.x - x) < 1e-8}} {{norm(ilu.x - x) < 1e-8}} {{norm(auto.x - x) < 1e-8}}")
"#
        ),
    );
    assert_eq!(
        out,
        "Bcsr BlockJacobi BlockIlu0 BlockIlu0 Ic0\ntrue true\ntrue true true\n"
    );
}

#[test]
fn cg_with_reports_non_convergence_and_rejects_bad_input() {
    let poisson = poisson_source(4);
    let rhs36 = rhs_source(16);
    let out = run_driver(
        "nonconv",
        &format!(
            r#"{IMPORT}

fn main(): IO Unit = do
    let a = convert<Csr>({poisson})
    let b = {rhs36}
    let r = cg_with(a, b, {{ tol = 1e-14, maxiter = 2, precond = Jacobi }})
    println("{{r.converged}} {{r.iterations}} {{r.residual > 1e-14}}")
"#
        ),
    );
    assert_eq!(out, "false 2 true\n");
    // Exact (Int) elements are a type error: `cg_with` iterates in floating point.
    let errors = driver_errors(
        "int",
        &format!(
            r#"{IMPORT}

fn main(): IO Unit = do
    let m: Sparse<Csr, [2, 2], Int> = from_dense([[2, 0], [0, 2]])
    let r = cg_with(m, [1.0, 1.0], {{ tol = 1e-8, maxiter = 5, precond = Auto }})
    println("{{r.converged}}")
"#
        ),
    );
    assert!(!errors.is_empty());
}

#[test]
fn spsolve_takes_the_cholesky_path_on_spd_and_still_solves_the_rest() {
    let poisson = poisson_source(4);
    let rhs64 = rhs_source(16);
    let out = run(&format!(
        r#"fn main(): IO Unit = do
    let a = convert<Csr>({poisson})
    let b = {rhs64}
    let x = spsolve(a, b)
    let r = cg(a, b, 1e-13, 1000)
    println("{{norm(a @ x - b) < 1e-10}} {{norm(x - r.x) < 1e-8}}")
    let sym = from_dense([[1.0, 2.0], [2.0, 1.0]])
    let y = spsolve(sym, [3.0, 3.0])
    println("{{norm(sym @ y - [3.0, 3.0]) < 1e-12}} {{y[0]}}")
    let gen = from_dense([[2.0, 1.0], [0.0, 3.0]])
    println("{{norm(gen @ spsolve(gen, [1.0, 2.0]) - [1.0, 2.0]) < 1e-12}}")
"#
    ));
    assert_eq!(out, "true true\ntrue 1.0\ntrue\n");
}

#[test]
fn slicing_rows_columns_and_ranges() {
    let out = run(r#"fn main(): IO Unit = do
    let d = [[1.0, 0.0, 2.0, 0.0], [0.0, 3.0, 0.0, 4.0], [5.0, 0.0, 0.0, 6.0], [0.0, 7.0, 8.0, 0.0]]
    let a = from_dense(d)
    let blk = a[1..3, 0..2]
    let row = a[2, :]
    let col = a[:, 3]
    println("{to_dense(blk) == [[0.0, 3.0], [5.0, 0.0]]} {format_of(blk)} {nnz(blk)}")
    println("{to_dense(row) == [5.0, 0.0, 0.0, 6.0]} {format_of(row)} {to_dense(col) == [0.0, 4.0, 6.0, 0.0]}")
    println("{to_dense(a[1.., ..2]) == [[0.0, 3.0], [5.0, 0.0], [0.0, 7.0]]} {to_dense(a[-1, :]) == [0.0, 7.0, 8.0, 0.0]}")
    println("{to_dense(a[::2, :]) == [[1.0, 0.0, 2.0, 0.0], [5.0, 0.0, 0.0, 6.0]]} {to_dense(a[::-1, 0..1]) == [[0.0], [5.0], [0.0], [1.0]]}")
    println("{to_dense(a[0..4, 0..4]) == d} {nnz(a[0..0, :])}")
    let s = a[1..3, 1..4]
    println("{shape(s)} {s[0, 0]} {s[1, 2]} {s[1, 0]}")
"#);
    assert_eq!(
        out,
        "true Csr 2\ntrue Coo true\ntrue true\ntrue true\ntrue 0\n[2, 3] 3.0 6.0 0.0\n"
    );
}

#[test]
fn slicing_keeps_formats_and_exactness() {
    let out = run(r#"fn main(): IO Unit = do
    let d = [[1, 0, 2, 0], [0, 3, 0, 4], [5, 0, 0, 6], [0, 7, 8, 0]]
    let c = convert<Csc>(from_dense(d))
    let b = to_bcsr(from_dense(d), 2, 2)
    let o = convert<Coo>(from_dense(d))
    println("{format_of(c[1..4, 0..2])} {format_of(b[0..2, 1..4])} {format_of(o[0..2, 0..2])} {format_of(b[1, :])}")
    println("{to_dense(b[2..4, 0..4]) == [[5, 0, 0, 6], [0, 7, 8, 0]]} {repr(b[3, 2])} {repr(c[0..2, 2..3][0, 0])}")
    let t = sparse_coo([2, 3, 4], [(0, 1, 2), (1, 2, 3), (1, 0, 0)], [1/2, 2/3, 3/4])
    let plane = t[1, :, :]
    let line = t[1, 2, :]
    println("{format_of(plane)} {shape(plane)} {nnz(plane)} {repr(plane[2, 3])} {shape(line)} {repr(line[3])}")
    println("{shape(t[0..2, 1..3, 2..4])} {nnz(t[0..2, 1..3, 2..4])}")
"#);
    assert_eq!(
        out,
        "Csc Bcsr Coo Coo\ntrue 8 2\nCoo [3, 4] 2 2/3 [4] 2/3\n[2, 2, 2] 2\n"
    );
}

#[test]
fn slicing_errors() {
    let bad = |expr: &str| {
        runtime_code(&format!(
            "fn main(): IO Unit = do\n    let a = from_dense([[1.0, 2.0], [3.0, 4.0]])\n    println(\"{{nnz({expr})}}\")\n"
        ))
    };
    assert_eq!(bad("a[5, :]"), "E0802");
    assert_eq!(bad("a[2..1, :]"), "E0802");
    let compiled = compile(
        "sparse_slice_arity.aev",
        "fn main(): IO Unit = do\n    let a = from_dense([[1.0, 2.0], [3.0, 4.0]])\n    println(\"{nnz(a[0..1])}\")\n",
    );
    assert!(compiled.diagnostics.has_errors());
}
