//! Sparse tensors (§8.4.1, §13.9.5): construction, formats, exactness,
//! dimensions, layout dispatch of `@`, solvers, contraction, equality and the
//! kernel bridge.
//!
//! Programs are compiled and run in both evaluators (AST and IR); the
//! assertions are about values (`==`, shapes, `NumKind`), not renderings.

#![allow(clippy::result_large_err)]

use aevia_core::Interp;
use aevia_core::Mode;
use aevia_core::compile;
use aevia_core::sparse::SparseData;
use aevia_core::sparse::SparseFormat;
use aevia_core::value::NumKind;
use aevia_core::value::Value;

/// Runs `main` of a program in both evaluators and returns the output of the
/// AST run (asserting the IR run agrees).
fn run(source: &str) -> String {
    let compiled = compile("sparse_test.aev", source);
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

/// The checker diagnostics' codes of a program that must not compile.
fn check_codes(source: &str) -> Vec<String> {
    let compiled = compile("sparse_check.aev", source);
    compiled
        .diagnostics
        .items
        .iter()
        .map(|d| d.code.to_string())
        .collect()
}

/// The runtime error code of `main`.
fn runtime_code(source: &str) -> String {
    let compiled = compile("sparse_runtime.aev", source);
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

/// The value bound to a top-level `let`, from an AST run.
fn top_level(
    source: &str,
    name: &str,
) -> Value {
    let compiled = compile("sparse_value.aev", source);
    assert!(
        !compiled.diagnostics.has_errors(),
        "{:#?}",
        compiled.diagnostics.items
    );
    let module = compiled.module.expect("a module");
    let mut interp = Interp::new(&compiled.checked).with_buffer();
    let diagnostics = interp.load(&module);
    assert!(!diagnostics.has_errors(), "{:?}", diagnostics.items);
    interp.eval_toplevel(name).expect("a top-level value")
}

fn sparse_of(
    source: &str,
    name: &str,
) -> std::rc::Rc<SparseData> {
    match top_level(source, name) {
        | Value::Sparse(data) => data,
        | other => panic!("expected a sparse tensor, found {}", other.type_name()),
    }
}

const DENSE: &str = "[[4.0, 0.0, 0.0], [1.0, 3.0, 0.0], [0.0, 2.0, 5.0]]";

// ---------------------------------------------------------------------------
// Construction, validation, formats
// ---------------------------------------------------------------------------

#[test]
fn from_dense_and_to_dense_round_trip() {
    let out = run(&format!(
        r#"
fn main(): IO Unit = do
    let d = {DENSE}
    let a = from_dense(d)
    println("{{nnz(a)}} {{format_of(a)}} {{to_dense(a) == d}} {{a[2, 1]}} {{a[0, 2]}} {{a[-1, -1]}}")
"#
    ));
    assert_eq!(out, "5 Csr true 2.0 0.0 5.0\n");
}

#[test]
fn every_format_round_trips_through_every_other() {
    let out = run(&format!(
        r#"
fn main(): IO Unit = do
    let d = {DENSE}
    let r = from_dense<Csr>(d)
    let c = convert<Csc>(r)
    let o = convert<Coo>(c)
    let r2 = convert<Csr>(o)
    let c2 = convert<Csc>(o)
    println("{{format_of(r)}} {{format_of(c)}} {{format_of(o)}} {{format_of(r2)}}")
    println("{{to_dense(c) == d}} {{to_dense(o) == d}} {{to_dense(r2) == d}} {{to_dense(c2) == d}}")
    println("{{r == c}} {{c == o}} {{o == r2}} {{nnz(c)}} {{nnz(o)}}")
    println("{{to_dense(from_dense<Csc>(d)) == d}} {{to_dense(from_dense<Coo>(d)) == d}}")
    println("{{c[1, 0]}} {{c[2, 1]}} {{c[0, 1]}} {{o[2, 2]}}")
"#
    ));
    assert_eq!(
        out,
        "Csr Csc Coo Csr\ntrue true true true\ntrue true true 5 5\ntrue true\n1.0 2.0 0.0 5.0\n"
    );
}

#[test]
fn constructors_build_the_same_matrix() {
    let out = run(&format!(
        r#"
fn main(): IO Unit = do
    let d = {DENSE}
    let a = sparse_coo([3, 3], [(0, 0), (1, 0), (1, 1), (2, 1), (2, 2)], [4.0, 1.0, 3.0, 2.0, 5.0])
    let b = sparse_csr(3, 3, [0, 1, 3, 5], [0, 0, 1, 1, 2], [4.0, 1.0, 3.0, 2.0, 5.0])
    let m = sparse_coo([3, 3], [[0, 0], [1, 0], [1, 1], [2, 1], [2, 2]], [4.0, 1.0, 3.0, 2.0, 5.0])
    println("{{to_dense(a) == d}} {{to_dense(b) == d}} {{to_dense(m) == d}} {{a == b}} {{format_of(a)}} {{format_of(b)}}")
"#
    ));
    assert_eq!(out, "true true true true Coo Csr\n");
}

#[test]
fn duplicates_are_summed_and_zeros_dropped() {
    let sparse = sparse_of(
        "let a = sparse_coo([2, 2], [(1, 1), (0, 0), (1, 1), (0, 1)], [1.0, 5.0, 2.0, 0.0])",
        "a",
    );
    assert_eq!(sparse.nnz(), 2);
    assert!(sparse.validate().is_ok());
    let stored = sparse.stored();
    assert_eq!(stored[1].0, vec![1, 1]);
    assert_eq!(stored[1].1.as_f64(), Some(3.0));
}

#[test]
fn construction_errors_are_reported() {
    let bad = |expr: &str| {
        runtime_code(&format!(
            "fn main(): IO Unit = do\n    let a = {expr}\n    println(\"{{nnz(a)}}\")\n"
        ))
    };
    assert_eq!(bad("sparse_coo([2, 2], [(0, 2)], [1.0])"), "E0802");
    assert_eq!(bad("sparse_coo([2, 2], [(0, 0), (1, 1)], [1.0])"), "E0802");
    assert_eq!(bad("sparse_coo([2, 2], [(-1, 0)], [1.0])"), "E0802");
    assert_eq!(bad("sparse_csr(2, 2, [0, 1], [0], [1.0])"), "E0802");
    assert_eq!(
        bad("sparse_csr(2, 2, [0, 2, 2], [1, 0], [1.0, 2.0])"),
        "E0802"
    );
    assert_eq!(
        bad("sparse_csr(2, 2, [0, 1, 2], [0, 2], [1.0, 2.0])"),
        "E0802"
    );
    assert_eq!(bad("sparse_csr(2, 2, [0, 1, 3], [0], [1.0])"), "E0802");
    assert_eq!(bad("from_dense<Csr>([1.0, 2.0])"), "E0802");
    // `Block`/`Bcsr` are real formats now (see `tests/sparse_block.rs`).
    assert_eq!(bad("from_dense<Block>([1.0, 2.0])"), "E0802");
    assert_eq!(
        bad("to_bcsr(from_dense([[1.0, 2.0], [3.0, 4.0]]), 3, 3)"),
        "E0802"
    );
    assert_eq!(
        bad("convert<Csr>(sparse_coo([2, 2, 2], [(0, 0, 0)], [1.0]))"),
        "E0802"
    );
}

#[test]
fn out_of_range_index_is_an_error() {
    let code = runtime_code(&format!(
        "fn main(): IO Unit = do\n    let a = from_dense({DENSE})\n    println(\"{{a[3, 0]}}\")\n"
    ));
    assert_eq!(code, "E0802");
}

#[test]
fn speye_spdiag_and_density() {
    let out = run(r#"
fn main(): IO Unit = do
    let i = speye(4)
    let d = spdiag([1.0, 2.0, 0.0, 4.0])
    println("{nnz(i)} {nnz(d)} {to_dense(i) == eye(4)} {density(i)} {d[3, 3]} {format_of(speye<Coo>(2))}")
"#);
    assert_eq!(out, "4 3 true 0.25 4.0 Coo\n");
}
#[path = "sparse/ops.rs"]
mod ops;
#[path = "sparse/tensors.rs"]
mod tensors;
