//! Exactness, dimensions, layout dispatch, elementwise operations, solvers.

use super::*;

// ---------------------------------------------------------------------------
// Exactness
// ---------------------------------------------------------------------------

#[test]
fn float_storage_is_the_f64_class_and_exact_storage_stays_boxed() {
    let float = sparse_of("let a = from_dense([[1.0, 0.0], [0.0, 2.0]])", "a");
    assert!(float.is_f64());
    let int = sparse_of("let a = from_dense([[1, 0], [0, 2]])", "a");
    assert!(!int.is_f64());
    assert!(matches!(int.zero.kind, NumKind::Int(_)));
    let real = sparse_of("let a = from_dense([[1/3, 0], [0, 2/3]])", "a");
    assert!(!real.is_f64());
    assert!(matches!(real.zero.kind, NumKind::Exact(_)));
    for data in [&float, &int, &real] {
        assert!(data.validate().is_ok());
    }
}

#[test]
fn exact_values_are_never_rounded() {
    let out = run(r#"
fn main(): IO Unit = do
    let a = from_dense([[1/3, 0], [0, 2/3]])
    let b = a * 3
    let c = a + a
    let p = a @ a
    println("{to_dense(b) == [[1, 0], [0, 2]]} {c[0, 0] == 2/3} {p[1, 1] == 4/9} {a[0, 1] == 0}")
    println("{a[0, 0] + a[0, 0] + a[0, 0] == 1} {repr(a[0, 0])} {repr(a[0, 1])}")
    let i = from_dense([[1, 0], [0, 2]])
    println("{repr(i[1, 1])} {repr(i[0, 1])} {repr((i @ i)[1, 1])}")
"#);
    assert_eq!(out, "true true true true\ntrue 1/3 0\n2 0 4\n");
}

#[test]
fn large_integers_survive() {
    let out = run(r#"
fn main(): IO Unit = do
    let big = 9007199254740993
    let a = from_dense([[big, 0], [0, 1]])
    println("{repr((a * 2)[0, 0])} {repr((a @ a)[0, 0])}")
"#);
    assert_eq!(
        out,
        "18014398509481986 81129638414606699710187514626049
"
    );
}

// ---------------------------------------------------------------------------
// Dimensions
// ---------------------------------------------------------------------------

#[test]
fn dimensions_propagate_through_sparse_operations() {
    let out = run(r#"
fn main(): IO Unit = do
    let a = from_dense([[2.0 m, 0.0 m], [0.0 m, 1.0 m]])
    let v = [1.0 s, 2.0 s]
    let x = a @ v
    let area = a @ a
    let scaled = a * 3.0 s
    println("{x[1] == 2.0 m*s} {area[0, 0] == 4.0 m^2} {scaled[0, 0] == 6.0 m*s}")
    println("{(a + a)[1, 1] == 2.0 m} {(a * a)[0, 0] == 4.0 m^2} {(a / 2.0 s)[0, 0] == 1.0 m/s}")
    println("{sum(a) == 3.0 m} {transpose(a)[0, 0] == 2.0 m}")
"#);
    assert_eq!(out, "true true true\ntrue true true\ntrue true\n");
}

#[test]
fn dimension_errors_are_reported_at_check_time() {
    let codes = check_codes(
        r#"
let a = from_dense([[1.0 m]])
let b = from_dense([[1.0 s]])
let c = a + b
"#,
    );
    assert!(codes.iter().any(|c| c == "E0401"), "{codes:?}");
    let codes = check_codes(
        r#"
let a = from_dense([[1.0 m]])
let r = a == from_dense([[1.0 s]])
"#,
    );
    assert!(!codes.is_empty(), "{codes:?}");
    let codes = check_codes(
        r#"
let a = from_dense([[1.0 m]])
let r = a + 1.0 m
"#,
    );
    assert!(!codes.is_empty(), "{codes:?}");
    let codes = check_codes(
        r#"
let k = from_dense([[2.0 N/m]])
let u: Tensor<1, Float[N]> = spsolve(k, [1.0 N])
"#,
    );
    assert!(!codes.is_empty(), "{codes:?}");
}

#[test]
fn sparse_and_tensor_do_not_unify_implicitly() {
    let codes = check_codes(
        r#"
fn takes_dense(t: Tensor<[2, 2], Float>) = t
let a = from_dense([[1.0, 0.0], [0.0, 1.0]])
let r = takes_dense(a)
"#,
    );
    assert!(
        !codes.is_empty(),
        "a Sparse was accepted where a Tensor is expected"
    );
    let codes = check_codes(
        r#"
fn takes_sparse(s: Sparse<Csr, [2, 2], Float>) = s
let r = takes_sparse([[1.0, 0.0], [0.0, 1.0]])
"#,
    );
    assert!(
        !codes.is_empty(),
        "a Tensor was accepted where a Sparse is expected"
    );
    let codes = check_codes(
        r#"
fn takes_sparse(s: Sparse<Csr, [2, 2], Float>) = to_dense(s)
let r = takes_sparse(from_dense([[1.0, 0.0], [0.0, 1.0]]))
let q = takes_sparse(from_dense<Coo>([[1.0, 0.0], [0.0, 1.0]]))
"#,
    );
    // The format is part of the type: a `Coo` is not a `Csr`.
    assert_eq!(codes.len(), 1, "{codes:?}");
    let codes = check_codes(
        r#"
fn takes_sparse(s: Sparse<Csr, [2, 2], Float>) = to_dense(s)
let r = takes_sparse(convert<Csr>(from_dense<Coo>([[1.0, 0.0], [0.0, 1.0]])))
"#,
    );
    assert!(codes.is_empty(), "{codes:?}");
}

// ---------------------------------------------------------------------------
// Layout dispatch of `@`
// ---------------------------------------------------------------------------

#[test]
fn matmul_dispatches_on_all_four_layouts() {
    let out = run(&format!(
        r#"
fn main(): IO Unit = do
    let d = {DENSE}
    let e = [[1.0, 2.0, 0.0], [0.0, 1.0, 1.0], [3.0, 0.0, 1.0]]
    let x = [1.0, -1.0, 2.0]
    let a = from_dense(d)
    let b = from_dense<Csc>(e)
    let dd = d @ e
    println("{{a @ e == dd}} {{d @ b == dd}}")
    println("{{to_dense(a @ b) == dd}} {{format_of(a @ b)}} {{a @ x == d @ x}} {{x @ a == x @ d}} {{x @ b == x @ e}}")
    println("{{matmul(a, e) == dd}} {{to_dense(matmul(a, b)) == dd}}")
    let o = from_dense<Coo>(d)
    println("{{to_dense(o @ b) == dd}} {{to_dense(a @ o) == d @ d}}")
"#
    ));
    assert_eq!(
        out,
        "true true\ntrue Csr true true true\ntrue true\ntrue true\n"
    );
}

#[test]
fn matmul_with_exact_elements_is_exact() {
    let out = run(r#"
fn main(): IO Unit = do
    let a = from_dense([[1, 0, 2], [0, 3, 0]])
    let d = [[1, 2], [3, 4], [5, 6]]
    let v = [1, 2, 3]
    println("{a @ d == [[11, 14], [9, 12]]} {a @ v == [7, 6]} {to_dense(a @ from_dense(d)) == [[11, 14], [9, 12]]}")
    println("{[1, 2] @ a == [1, 6, 2]} {from_dense(d) @ [1, 1] == [3, 7, 11]}")
"#);
    assert_eq!(out, "true true true\ntrue true\n");
}

#[test]
fn matmul_shape_mismatch_is_e0802() {
    let code = runtime_code(&format!(
        "fn main(): IO Unit = do\n    let a = from_dense({DENSE})\n    println(\"{{a @ [1.0, 2.0]}}\")\n"
    ));
    assert_eq!(code, "E0802");
}

// ---------------------------------------------------------------------------
// Elementwise, transpose, kron, reductions
// ---------------------------------------------------------------------------

#[test]
fn elementwise_operations_and_hadamard() {
    let out = run(&format!(
        r#"
fn main(): IO Unit = do
    let d = {DENSE}
    let e = [[1.0, 2.0, 0.0], [0.0, 1.0, 1.0], [3.0, 0.0, 1.0]]
    let a = from_dense(d)
    let b = from_dense<Csc>(e)
    println("{{to_dense(a + b) == d + e}} {{to_dense(a - b) == d - e}} {{to_dense(a * b) == d * e}} {{format_of(a + b)}} {{format_of(b + a)}}")
    println("{{to_dense(a * e) == d * e}} {{to_dense(e * a) == d * e}} {{a + e == d + e}} {{e - a == e - d}}")
    println("{{to_dense(a * 2.0) == d * 2.0}} {{to_dense(2.0 * a) == d * 2.0}} {{to_dense(a / 2.0) == d / 2.0}} {{to_dense(-a) == -d}}")
    println("{{nnz(a - a)}} {{nnz(a * from_dense(e))}} {{a == a}} {{a != b}} {{a == from_dense<Coo>(d)}}")
"#
    ));
    assert_eq!(
        out,
        "true true true Csr Csc\ntrue true true true\ntrue true true true\n0 3 true true true\n"
    );
}

#[test]
fn transpose_kron_and_reductions() {
    let out = run(&format!(
        r#"
fn main(): IO Unit = do
    let d = {DENSE}
    let a = from_dense(d)
    println("{{to_dense(transpose(a)) == transpose(d)}} {{format_of(transpose(from_dense<Csc>(d)))}} {{transpose(transpose(a)) == a}}")
    let k = kron(from_dense([[1.0, 2.0], [0.0, 1.0]]), from_dense([[0.0, 1.0], [1.0, 0.0]]))
    let expect = [[0.0, 1.0, 0.0, 2.0], [1.0, 0.0, 2.0, 0.0], [0.0, 0.0, 0.0, 1.0], [0.0, 0.0, 1.0, 0.0]]
    println("{{to_dense(k) == expect}} {{nnz(k)}} {{format_of(k)}}")
    println("{{sum(a)}} {{sum(a, 0) == sum(d, 0)}} {{sum(a, 1) == sum(d, 1)}}")
    let m = map_nonzero(fn x => x * 2.0, a)
    println("{{to_dense(m) == d * 2.0}} {{length(nonzeros(a))}} {{nonzeros(a)[1]}}")
"#
    ));
    assert_eq!(
        out,
        "true Csc true\ntrue 6 Csr\n15.0 true true\ntrue 5 ([1, 0], 1.0)\n"
    );
}

// ---------------------------------------------------------------------------
// Solvers
// ---------------------------------------------------------------------------

#[test]
fn spsolve_f64_and_exact() {
    let out = run(&format!(
        r#"
fn main(): IO Unit = do
    let d = {DENSE}
    let b = [4.0, 5.0, 7.0]
    let a = from_dense(d)
    let x = spsolve(a, b)
    let residual = norm(a @ x - b)
    println("{{residual < 1e-12}}")
    let ai: Sparse<Csr, [2, 2], Real> = from_dense([[2, 0], [1, 3]])
    let bi: Tensor<[2], Real> = [1, 1]
    let xi = spsolve(ai, bi)
    println("{{xi == [1/2, 1/6]}} {{repr(xi[1])}}")
"#
    ));
    assert_eq!(out, "true\ntrue 1/6\n");
}

#[test]
fn spsolve_reports_a_singular_matrix() {
    let code = runtime_code(
        "fn main(): IO Unit = do\n    let x = spsolve(from_dense([[1.0, 2.0], [2.0, 4.0]]), [1.0, 1.0])\n    println(\"{x}\")\n",
    );
    assert_eq!(code, "E0806");
    let code = runtime_code(
        "fn main(): IO Unit = do\n    let x = spsolve(from_dense([[1, 2], [2, 4]]), [1, 1])\n    println(\"{x}\")\n",
    );
    assert_eq!(code, "E0806");
}

#[test]
fn spsolve_dimension_is_b_over_a() {
    let out = run(r#"
fn main(): IO Unit = do
    let k = from_dense([[2.0 N/m, -1.0 N/m], [-1.0 N/m, 2.0 N/m]])
    let f = [1.0 N, 0.0 N]
    let u = spsolve(k, f)
    println("{u[0] > 0.6 m} {u[0] < 0.7 m}")
"#);
    assert_eq!(out, "true true\n");
}

/// The 1-D Laplacian (Dirichlet) as a `Coo` sum, in the language itself.
const LAPLACIAN: &str = r#"
fn laplacian(n: Int) =
    let ii = range(0, n)
    let jj = range(0, n - 1)
    let diag = sparse_coo([n, n], map(fn i => (i, i), ii), map(fn i => 2.0, ii))
    let up = sparse_coo([n, n], map(fn i => (i, i + 1), jj), map(fn i => -1.0, jj))
    diag + up + transpose(up)
"#;

#[test]
fn cg_converges_on_a_laplacian() {
    let out = run(&format!(
        r#"{LAPLACIAN}
fn main(): IO Unit = do
    let n = 20
    let a = laplacian(n)
    let b = map(fn i => 1.0, range(0, n))
    let r = cg(a, b, 1e-12, 200)
    let x = spsolve(a, b)
    println("{{r.converged}} {{r.iterations <= 20}} {{norm(r.x - x) < 1e-8}} {{nnz(a)}}")
"#
    ));
    assert_eq!(out, "true true true 58\n");
}

#[test]
fn cg_reports_non_convergence_and_requires_floats() {
    let out = run(&format!(
        r#"{LAPLACIAN}
fn main(): IO Unit = do
    let n = 50
    let r = cg(laplacian(n), map(fn i => 1.0, range(0, n)), 1e-14, 3)
    println("{{r.converged}} {{r.iterations}}")
"#
    ));
    assert_eq!(out, "false 3\n");
    let codes = check_codes(
        "let m: Sparse<Csr, [2, 2], Int> = from_dense([[2, 0], [0, 2]])
let r = cg(m, [1.0, 1.0], 1e-9, 10)",
    );
    assert!(!codes.is_empty(), "cg accepted exact right-hand sides");
}
