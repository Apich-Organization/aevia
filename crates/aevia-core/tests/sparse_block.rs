//! Block CSR (`Bcsr`, spelled `Block` in types) sparse matrices (§8.4.1,
//! §13.9.5): construction with an explicit or automatic block size, the
//! padding rule, operations through the canonical entries, the block `@`
//! kernel, exactness, equality, rendering and errors.

#![allow(clippy::result_large_err)]

use aevia_core::Interp;
use aevia_core::Mode;
use aevia_core::compile;
use aevia_core::sparse::SparseData;
use aevia_core::sparse::SparseFormat;
use aevia_core::value::NumKind;
use aevia_core::value::Value;

/// Runs `main` in both evaluators and returns the (agreeing) output.
fn run(source: &str) -> String {
    let compiled = compile("sparse_block.aev", source);
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
    let compiled = compile("sparse_block_runtime.aev", source);
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

fn sparse_of(
    source: &str,
    name: &str,
) -> std::rc::Rc<SparseData> {
    let compiled = compile("sparse_block_value.aev", source);
    assert!(
        !compiled.diagnostics.has_errors(),
        "{:#?}",
        compiled.diagnostics.items
    );
    let module = compiled.module.expect("a module");
    let mut interp = Interp::new(&compiled.checked).with_buffer();
    let diagnostics = interp.load(&module);
    assert!(!diagnostics.has_errors(), "{:?}", diagnostics.items);
    match interp.eval_toplevel(name).expect("a top-level value") {
        | Value::Sparse(data) => data,
        | other => panic!("expected a sparse tensor, found {}", other.type_name()),
    }
}

/// A 4x4 matrix with two dense 2x2 blocks and one half-empty block.
const M4: &str =
    "[[1.0, 2.0, 0.0, 0.0], [3.0, 4.0, 0.0, 0.0], [0.0, 0.0, 5.0, 0.0], [0.0, 0.0, 0.0, 6.0]]";

#[test]
fn explicit_block_size_and_padding() {
    let a = sparse_of(&format!("let a = to_bcsr(from_dense({M4}), 2, 2)"), "a");
    assert_eq!(a.format, SparseFormat::Bcsr);
    assert_eq!(a.block, (2, 2));
    assert!(a.validate().is_ok());
    // Six logical nonzeros in two stored 2x2 blocks: 8 slots, 2 padding zeros.
    assert_eq!(a.nnz(), 6);
    assert_eq!(a.stored_len(), 8);
    assert_eq!(a.stored().len(), 6);
    let out = run(&format!(
        r#"
fn main(): IO Unit = do
    let d = {M4}
    let a = to_bcsr(from_dense(d), 2, 2)
    println("{{format_of(a)}} {{nnz(a)}} {{block_size(a)}} {{to_dense(a) == d}} {{a[1, 0]}} {{a[2, 3]}} {{a[3, 3]}}")
    println("{{block_size(from_dense(d))}} {{density(a)}}")
"#
    ));
    assert_eq!(out, "Bcsr 6 (2, 2) true 3.0 0.0 6.0\n(1, 1) 0.375\n");
}

#[test]
fn convert_and_from_dense_choose_the_block_size() {
    let out = run(&format!(
        r#"
fn main(): IO Unit = do
    let dense_blocks = [[1.0, 2.0, 0.0, 0.0], [3.0, 4.0, 0.0, 0.0], [0.0, 0.0, 5.0, 6.0], [0.0, 0.0, 7.0, 8.0]]
    let a = from_dense<Bcsr>(dense_blocks)
    let b = convert<Bcsr>(from_dense<Csc>(dense_blocks))
    let c = from_dense<Block>({M4})
    let d = convert<Bcsr>(from_dense<Coo>([[1.0, 0.0, 0.0], [0.0, 0.0, 2.0]]))
    println("{{format_of(a)}} {{block_size(a)}} {{block_size(b)}} {{block_size(c)}} {{block_size(d)}}")
    println("{{a == b}} {{to_dense(a) == dense_blocks}} {{format_of(c)}} {{format_of(d)}}")
    let e = speye<Bcsr>(4)
    let f = spdiag<Bcsr>([1.0, 2.0, 3.0])
    println("{{format_of(e)}} {{nnz(e)}} {{format_of(f)}} {{nnz(f)}} {{to_dense(e) == eye(4)}}")
"#
    ));
    // Two dense 2x2 blocks: fill 1. M4: 8 slots for 6 entries (1.33 <= 1.5).
    // The 2x3 matrix gains nothing from any block; the identity either.
    assert_eq!(
        out,
        "Bcsr (2, 2) (2, 2) (2, 2) (1, 1)
true true Bcsr Bcsr
Bcsr 4 Bcsr 3 true
"
    );
}

#[test]
fn block_spelling_is_the_same_type() {
    let out = run(&format!(
        r#"
fn takes(s: Sparse<Bcsr, [4, 4], Float>) = nnz(s)

fn main(): IO Unit = do
    let a: Sparse<Block, [4, 4], Float> = to_bcsr(from_dense({M4}), 2, 2)
    let b: Sparse<Bcsr, [4, 4], Float> = a
    println("{{takes(a)}} {{takes(b)}} {{a == b}}")
"#
    ));
    assert_eq!(out, "6 6 true\n");
}

#[test]
fn matmul_with_dense_equals_csr() {
    let out = run(&format!(
        r#"
fn main(): IO Unit = do
    let d = {M4}
    let c = from_dense(d)
    let b = to_bcsr(c, 2, 2)
    let b4 = to_bcsr(c, 4, 4)
    let v = [1.0, 2.0, 3.0, 4.0]
    let m = [[1.0, 0.0], [2.0, 1.0], [0.0, 3.0], [4.0, 5.0]]
    println("{{b @ v == c @ v}} {{b4 @ v == c @ v}} {{b @ m == c @ m}} {{transpose(m) @ b == transpose(m) @ c}} {{v @ b == v @ c}}")
    println("{{b @ v}}")
    println("{{to_dense(b @ b) == to_dense(c @ c)}} {{format_of(b @ b)}}")
"#
    ));
    assert_eq!(
        out,
        "true true true true true\n[5.0, 11.0, 15.0, 24.0]\ntrue Csr\n"
    );
}

#[test]
fn arithmetic_and_structure_through_entries() {
    let out = run(&format!(
        r#"
fn main(): IO Unit = do
    let c = from_dense({M4})
    let b = to_bcsr(c, 2, 2)
    let s = b + b
    let h = b * b
    let k = b * 2.0
    let n = -b
    let t = transpose(b)
    println("{{s == c + c}} {{h == c * c}} {{k == c * 2.0}} {{n == -c}} {{t == transpose(c)}}")
    println("{{format_of(s)}} {{format_of(h)}} {{format_of(k)}} {{format_of(n)}} {{format_of(t)}}")
    println("{{block_size(s)}} {{block_size(t)}} {{nnz(s)}} {{nnz(t)}}")
    println("{{sum(b)}} {{b == c}} {{c == b}} {{b == transpose(c)}}")
    let z = b - b
    println("{{nnz(z)}} {{z == c - c}}")
    println("{{to_dense(map_nonzero(fn x => x + 1.0, b)) == to_dense(map_nonzero(fn x => x + 1.0, c))}}")
"#
    ));
    assert_eq!(
        out,
        "true true true true true\nBcsr Bcsr Bcsr Bcsr Bcsr\n(2, 2) (2, 2) 6 6\n21.0 true true false\n0 true\ntrue\n"
    );
}

#[test]
fn solvers_accept_block_matrices() {
    let out = run(&format!(
        r#"
fn main(): IO Unit = do
    let spd = [[4.0, 1.0, 0.0, 0.0], [1.0, 3.0, 0.0, 0.0], [0.0, 0.0, 5.0, 2.0], [0.0, 0.0, 2.0, 6.0]]
    let c = from_dense(spd)
    let b = to_bcsr(c, 2, 2)
    let rhs = [1.0, 2.0, 3.0, 4.0]
    let x = spsolve(b, rhs)
    let r = cg(b, rhs, 1e-12, 50)
    println("{{norm(b @ x - rhs) < 1e-12}} {{r.converged}} {{norm(r.x - x) < 1e-9}} {{norm(x - spsolve(c, rhs)) < 1e-12}}")
    let i = to_bcsr(from_dense({M4}), 2, 2)
    println("{{nnz(kron(i, i))}} {{get(i, 1, 1)}}")
"#
    ));
    assert_eq!(out, "true true true true\n36 4.0\n");
}

#[test]
fn exact_elements_stay_exact() {
    let int = sparse_of(
        "let a = to_bcsr(from_dense([[1, 2, 0, 0], [3, 4, 0, 0]]), 2, 2)",
        "a",
    );
    assert!(!int.is_f64());
    assert!(matches!(int.zero.kind, NumKind::Int(_)));
    assert!(int.validate().is_ok());
    let real = sparse_of(
        "let a = to_bcsr(from_dense([[1/3, 0], [0, 2/3]]), 2, 2)",
        "a",
    );
    assert!(!real.is_f64());
    assert!(matches!(real.zero.kind, NumKind::Exact(_)));
    assert_eq!(real.nnz(), 2);
    assert_eq!(real.stored_len(), 4);
    assert!(real.validate().is_ok());
    let out = run(r#"
fn main(): IO Unit = do
    let a = to_bcsr(from_dense([[1/3, 0], [0, 2/3]]), 2, 2)
    let b = a * 3
    let p = a @ a
    println("{to_dense(b) == [[1, 0], [0, 2]]} {p[1, 1] == 4/9} {repr(a[0, 0])} {repr(a[0, 1])}")
    println("{repr((a @ [3, 3])[1])} {repr((a + a)[0, 0])}")
    let big = 9007199254740993
    let g = to_bcsr(from_dense([[big, 0], [0, 1]]), 2, 2)
    println("{repr((g * 2)[0, 0])} {repr((g @ [1, 1])[0])}")
    let i = to_bcsr(from_dense([[1, 2], [0, 0]]), 1, 2)
    println("{repr(i[0, 1])} {repr(i[1, 0])} {nnz(i)}")
"#);
    assert_eq!(
        out,
        "true true 1/3 0\n2 2/3\n18014398509481986 9007199254740993\n2 0 2\n"
    );
}

#[test]
fn padding_never_counts_as_an_entry() {
    let out = run(r#"
fn main(): IO Unit = do
    let a = to_bcsr(from_dense([[1.0, 0.0], [0.0, 0.0]]), 2, 2)
    println("{nnz(a)} {a[1, 1]} {sum(a)}")
    let b = a + to_bcsr(from_dense([[-1.0, 0.0], [0.0, 0.0]]), 2, 2)
    println("{nnz(b)} {b == from_dense([[0.0, 0.0], [0.0, 0.0]])}")
    let z = to_bcsr(from_dense([[0.0, 0.0], [0.0, 0.0]]), 2, 2)
    println("{nnz(z)} {block_size(z)}")
"#);
    assert_eq!(out, "1 0.0 1.0\n0 true\n0 (2, 2)\n");
}

#[test]
fn show_and_repr_round_trip() {
    let out = run(&format!(
        r#"
fn main(): IO Unit = do
    let a = to_bcsr(from_dense({M4}), 2, 2)
    println("{{a}}")
    let text = repr(a)
    println("{{text}}")
    let b = to_bcsr(sparse_coo([4, 4], [(0, 0), (0, 1), (1, 0), (1, 1), (2, 2), (3, 3)], [1.0, 2.0, 3.0, 4.0, 5.0, 6.0]), 2, 2)
    println("{{a == b}} {{repr(b) == text}} {{block_size(b)}}")
"#
    ));
    let lines: Vec<&str> = out.lines().collect();
    assert!(
        lines[0].starts_with("sparse(Bcsr, [4, 4], { (0, 0) = 1.0,"),
        "{}",
        lines[0]
    );
    assert!(
        lines[1].starts_with("to_bcsr(sparse_coo([4, 4],"),
        "{}",
        lines[1]
    );
    assert!(lines[1].ends_with(", 2, 2)"), "{}", lines[1]);
    assert_eq!(lines[2], "true true (2, 2)");
}

#[test]
fn invalid_block_sizes_are_e0802() {
    let bad = |expr: &str| {
        runtime_code(&format!(
            "fn main(): IO Unit = do\n    let a = {expr}\n    println(\"{{nnz(a)}}\")\n"
        ))
    };
    assert_eq!(
        bad("to_bcsr(from_dense([[1.0, 2.0], [3.0, 4.0]]), 0, 1)"),
        "E0802"
    );
    assert_eq!(
        bad("to_bcsr(from_dense([[1.0, 2.0], [3.0, 4.0]]), 1, 0)"),
        "E0802"
    );
    assert_eq!(
        bad("to_bcsr(from_dense([[1.0, 2.0, 3.0], [3.0, 4.0, 5.0]]), 2, 2)"),
        "E0802"
    );
    assert_eq!(
        bad("to_bcsr(from_dense([[1.0, 2.0], [3.0, 4.0]]), 3, 1)"),
        "E0802"
    );
    assert_eq!(
        bad("to_bcsr(from_dense([[1.0, 2.0], [3.0, 4.0]]), -1, 1)"),
        "E0802"
    );
    assert_eq!(
        bad("convert<Bcsr>(sparse_coo([2, 2, 2], [(0, 0, 0)], [1.0]))"),
        "E0802"
    );
    assert_eq!(bad("from_dense<Block>([1.0, 2.0])"), "E0802");
}

#[test]
fn block_format_is_no_longer_a_missing_capability() {
    let compiled = compile(
        "sparse_block_cap.aev",
        "fn main(): IO Unit = do\n    println(\"{format_of(from_dense<Block>([[1.0]]))}\")\n",
    );
    assert!(!compiled.diagnostics.has_errors());
    let out = run(
        "fn main(): IO Unit = do\n    println(\"{format_of(from_dense<Block>([[1.0]]))} {format_of(convert<BCSR>(speye(2)))}\")\n",
    );
    assert_eq!(out, "Bcsr Bcsr\n");
}

#[test]
fn flat_parts_hold_only_logical_nonzeros() {
    let a = sparse_of(&format!("let a = to_bcsr(from_dense({M4}), 2, 2)"), "a");
    let (flat, values) = a.to_flat_parts().expect("flat parts");
    assert_eq!(flat, vec![0, 1, 4, 5, 10, 15]);
    assert_eq!(values.len(), 6);
    let back =
        SparseData::from_flat_parts(SparseFormat::Bcsr, vec![4, 4], &flat, values, a.dim.clone())
            .expect("rebuilds");
    assert!(back.validate().is_ok());
    assert_eq!(back.nnz(), 6);
    assert_eq!(back.format, SparseFormat::Bcsr);
}
