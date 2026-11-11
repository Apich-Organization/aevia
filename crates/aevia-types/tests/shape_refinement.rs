//! §3.5.1 / §3.5.3: symbolic shape extents (`n + 1`, `n * m`), broadcasting,
//! rigid declared extents, `refine_shape` branches and sparse shape
//! propagation.

use aevia_syntax::lex;
use aevia_syntax::parse_module;
use aevia_types::check::check_module;

fn errors(source: &str) -> Vec<(String, String)> {
    let lexed = lex(0, source).expect("lex");
    let (module, parsed) = parse_module(0, source, &lexed);
    assert!(!parsed.has_errors(), "parse errors in {source}");
    check_module(0, &module)
        .diagnostics
        .items
        .iter()
        .filter(|item| item.severity == aevia_syntax::Severity::Error)
        .map(|item| (item.code.clone(), item.message.clone()))
        .collect()
}

fn ok(source: &str) {
    let found = errors(source);
    assert!(found.is_empty(), "{source}\n{found:?}");
}

fn fails(
    source: &str,
    code: &str,
    fragment: &str,
) {
    let found = errors(source);
    assert!(
        found.iter().any(|(c, m)| c == code && m.contains(fragment)),
        "{source}\nexpected {code} containing `{fragment}`, got {found:?}"
    );
}

#[test]
fn a_generic_function_can_call_another_with_a_symbolic_extent() {
    ok(
        "fn id_t<n>(x: Tensor<[n], Float>): Tensor<[n], Float> = x\n\
        fn g<m>(y: Tensor<[m], Float>): Tensor<[m], Float> = id_t(y)\n",
    );
    ok(
        "fn cat2<n, m>(x: Tensor<[n], Float>, y: Tensor<[m], Float>): Tensor<[n + m], Float> = concat(x, y)\n\
        fn g<a, b>(p: Tensor<[a], Float>, q: Tensor<[b], Float>): Tensor<[b + a], Float> = cat2(q, p)\n",
    );
}

#[test]
fn products_of_extents_are_symbolic_and_commute() {
    ok(
        "fn f<n, m>(a: Tensor<[n, m], Float>): Result<Tensor<[n * m], Float>, ShapeError> = reshape<[n * m]>(a)\n",
    );
    ok("fn f<n, m>(a: Tensor<[n * m], Float>): Tensor<[m * n], Float> = a\n");
    ok("fn f<n, m>(a: Tensor<[n + m], Float>): Tensor<[m + n], Float> = a\n");
    // A product that cannot be solved is E0304, naming the variables.
    fails(
        "fn f<n, m>(a: Tensor<[n * m], Float>): Tensor<[2], Float> = a\n",
        "E0304",
        "n * m",
    );
}

#[test]
fn a_solved_factor_turns_a_product_into_a_linear_equation() {
    // n = 2 from the first axis makes `n * m ~ 6` solvable: m = 3.
    ok(
        "fn f<m>(a: Tensor<[2, m], Float>): Result<Tensor<[6], Float>, ShapeError> = reshape<[6]>(a)\n",
    );
}

#[test]
fn broadcasting_follows_the_one_extent_rule() {
    ok("fn f(a: Tensor<[3, 1], Float>, b: Tensor<[1, 4], Float>): Tensor<[3, 4], Float> = a + b\n");
    ok("fn f(a: Tensor<[3, 1], Float>, b: Tensor<[1, 4], Float>): Tensor<[3, 4], Float> = a * b\n");
    ok("fn f<n>(a: Tensor<[n], Float>, b: Tensor<[1], Float>): Tensor<[n], Float> = a + b\n");
    ok(
        "fn f<n, m>(a: Tensor<[n, 1], Float>, b: Tensor<[1, m], Float>): Tensor<[n, m], Float> = a + b\n",
    );
    fails(
        "fn f(a: Tensor<[3, 2], Float>, b: Tensor<[3, 4], Float>): Tensor<[3, 4], Float> = a + b\n",
        "E0302",
        "shape mismatch",
    );
    fails(
        "fn f(a: Tensor<[3], Float>, b: Tensor<[3, 1], Float>): Tensor<[3], Float> = a + b\n",
        "E0302",
        "shape mismatch",
    );
}

#[test]
fn two_declared_extents_are_not_silently_merged() {
    fails(
        "fn f<n, m>(a: Tensor<[n], Float>, b: Tensor<[m], Float>): Tensor<[n], Float> = a + b\n",
        "E0302",
        "`n` and `m`",
    );
    fails(
        "fn f<n, k, m>(a: Tensor<[n, k], Float>, b: Tensor<[m, k], Float>): Tensor<[n, m], Float> = a @ b\n",
        "E0302",
        "`k` and `m`",
    );
    fails(
        "fn f<n>(a: Tensor<[n + 1], Float>): Tensor<[n], Float> = a\n",
        "E0302",
        "n + 1 = n",
    );
}

#[test]
fn shape_mismatch_diagnostics_use_source_names() {
    fails(
        "fn f<n>(a: Tensor<[n, 2], Float>, b: Tensor<[n, 3], Float>): Tensor<[n, 2], Float> = a + b\n",
        "E0302",
        "[n, 2]",
    );
}

#[test]
fn refine_shape_branches_on_the_shape() {
    ok(
        "fn f(a: Tensor<2, Float>): Float =\n    match refine_shape<[3, 3]>(a) with\n    | Some(sq) -> (sq @ sq)[0, 0]\n    | None -> 0.0\n",
    );
    // Inside the `Some` arm the extents are static: indexing out of bounds fails.
    fails(
        "fn f(a: Tensor<1, Float>): Float =\n    match refine_shape<[3]>(a) with\n    | Some(b) -> b[5]\n    | None -> 0.0\n",
        "E0802",
        "out of bounds",
    );
    fails(
        "fn f(a: Tensor<1, Float>): Float =\n    match refine_shape<[3, 3]>(a) with\n    | Some(b) -> 1.0\n    | None -> 0.0\n",
        "E0302",
        "rank",
    );
    fails(
        "fn f<n>(a: Tensor<1, Float>): Float =\n    match refine_shape<[n]>(a) with\n    | Some(b) -> 1.0\n    | None -> 0.0\n",
        "E0304",
        "no run-time value",
    );
}

#[test]
fn sparse_shapes_are_propagated_and_checked() {
    ok(
        "fn f<n, k, m>(a: Sparse<Csr, [n, k], Float>, b: Sparse<Csr, [k, m], Float>): Sparse<Csr, [n, m], Float> = a @ b\n",
    );
    ok(
        "fn f<n, k>(a: Sparse<Csr, [n, k], Float>, x: Tensor<[k], Float>): Tensor<[n], Float> = a @ x\n",
    );
    ok("fn f<n, k>(a: Sparse<Csr, [n, k], Float>): Sparse<Csr, [k, n], Float> = transpose(a)\n");
    ok(
        "fn f<n>(a: Sparse<Csr, [n, n], Float>, b: Tensor<[n], Float>): Tensor<[n], Float> = spsolve(a, b)\n",
    );
    ok(
        "fn f(a: Sparse<Csr, [2, 3], Float>, b: Sparse<Csr, [4, 5], Float>): Sparse<Csr, [8, 15], Float> = kron(a, b)\n",
    );
    fails(
        "fn f(a: Sparse<Csr, [2, 3], Float>, b: Sparse<Csr, [4, 5], Float>): Sparse<Csr, [2, 5], Float> = a @ b\n",
        "E0802",
        "left columns",
    );
    fails(
        "fn f(a: Sparse<Csr, [2, 3], Float>, x: Tensor<[4], Float>): Tensor<[2], Float> = a @ x\n",
        "E0802",
        "left columns",
    );
    fails(
        "fn f(a: Sparse<Csr, [2, 3], Float>, b: Sparse<Csr, [3, 2], Float>): Sparse<Csr, [2, 3], Float> = a + b\n",
        "E0302",
        "shape mismatch",
    );
}
