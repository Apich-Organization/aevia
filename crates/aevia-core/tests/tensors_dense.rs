//! Dense storage and the `f64` fast path (§9.13).
//!
//! Every assertion here is about a *value*: the storage class, the element
//! values, the dimension, or the result of a comparison — never a rendered
//! substring. `show`/`repr` are compared only where the requirement is that
//! the two storage classes render identically, and there the strings are
//! compared to each other, not to a literal.
//!
//! The differential tests are the point of the file: each operation is run
//! twice on the same program — once on the dense value, once on a boxed
//! mirror built from the same elements — and the two results must agree. For
//! every operation but `@` that agreement is bit for bit; `@` on two matrices
//! runs on a blocked kernel, which reassociates the inner sum, so there it is
//! agreement to rounding (see `matmul_agrees_with_the_boxed_path`).

#![allow(clippy::result_large_err)]

use aevia_core::value::{equal, repr, show, NumKind, Storage, TensorData, Value};
use aevia_core::{compile, Interp};
use aevia_syntax::Span;
use std::rc::Rc;
use std::time::Instant;

// --- harness ---------------------------------------------------------------

fn session(source: &str) -> Interp {
    let compilation = compile("dense.aev", source);
    assert!(
        !compilation.diagnostics.has_errors(),
        "compile errors: {:?}",
        compilation
            .diagnostics
            .items
            .iter()
            .map(|item| (&item.code, &item.message, item.span.start, item.span.end))
            .collect::<Vec<_>>()
    );
    let module = compilation.module.expect("module");
    let mut interp = Interp::new(&compilation.checked).with_buffer();
    let diagnostics = interp.load(&module);
    assert!(
        !diagnostics.has_errors(),
        "load errors: {:?}",
        diagnostics
            .items
            .iter()
            .map(|item| (&item.code, &item.message))
            .collect::<Vec<_>>()
    );
    interp
}

fn tensor_of(value: &Value) -> &TensorData {
    match value {
        Value::Tensor(tensor) => tensor,
        other => panic!("expected a tensor, found {}", other.type_name()),
    }
}

fn is_dense(value: &Value) -> bool {
    tensor_of(value).is_dense()
}

/// The same elements in boxed storage: the reference the dense path must
/// reproduce, not a different value.
fn boxed(value: &Value) -> Value {
    let tensor = tensor_of(value);
    Value::Tensor(Rc::new(TensorData {
        shape: tensor.shape.clone(),
        storage: Storage::Boxed(tensor.elements()),
        dim: tensor.dim.clone(),
        display: tensor.display.clone(),
    }))
}

/// Strict equality: shape, dimension, element kind and element *bits*.
///
/// `Value::equal` compares floats with `==`, which cannot tell `-0.0` from
/// `0.0`; a fast path that turned one into the other would still print
/// differently, so the bits are checked here.
fn assert_same(left: &Value, right: &Value, context: &str) {
    match (left, right) {
        (Value::Tensor(a), Value::Tensor(b)) => {
            assert_eq!(a.shape, b.shape, "{context}: shape");
            assert_eq!(a.dim, b.dim, "{context}: element dimension");
            assert_eq!(
                a.storage.len(),
                b.storage.len(),
                "{context}: element count"
            );
            for index in 0..a.storage.len() {
                let x = a.get_flat(index).expect("left element");
                let y = b.get_flat(index).expect("right element");
                assert_same(&x, &y, &format!("{context}[{index}]"));
            }
            assert_eq!(show(left), show(right), "{context}: show");
            assert_eq!(repr(left), repr(right), "{context}: repr");
        }
        (Value::Num(a), Value::Num(b)) => {
            assert_eq!(
                a.kind.type_name(),
                b.kind.type_name(),
                "{context}: numeric phase"
            );
            match (&a.kind, &b.kind) {
                (NumKind::Float(x), NumKind::Float(y)) => {
                    assert_eq!(x.to_bits(), y.to_bits(), "{context}: float bits");
                }
                (NumKind::Int(x), NumKind::Int(y)) => assert_eq!(x, y, "{context}: int"),
                (NumKind::Exact(x), NumKind::Exact(y)) => {
                    assert_eq!(x, y, "{context}: rational")
                }
                _ => assert_eq!(
                    a.kind.type_name(),
                    b.kind.type_name(),
                    "{context}: representation"
                ),
            }
            assert_eq!(a.dim, b.dim, "{context}: dimension");
            assert_eq!(repr(left), repr(right), "{context}: repr");
        }
        _ => {
            assert_eq!(
                equal(left, right),
                Ok(true),
                "{context}: values differ ({left:?} vs {right:?})"
            );
            assert_eq!(repr(left), repr(right), "{context}: repr");
        }
    }
}

// --- 1. the rule for choosing dense ----------------------------------------

#[test]
fn a_float_tensor_is_dense_and_an_exact_one_is_not() {
    // `2.0` is a `Float`, so its buffer is lossless: dense.
    let mut interp = session("let b = [2.0]\n");
    let dense = interp.eval_toplevel("b").expect("b");
    assert!(is_dense(&dense), "a float tensor must be dense");
    let element = tensor_of(&dense).get_flat(0).expect("element");
    assert!(
        matches!(element.as_num().expect("number").kind, NumKind::Float(_)),
        "the dense element is a Float"
    );

    // `1/3` is an exact rational with no `f64` value. It must stay exact.
    let mut interp = session("let a = [1/3]\n");
    let exact = interp.eval_toplevel("a").expect("a");
    assert!(!is_dense(&exact), "1/3 must not become dense");
    let element = tensor_of(&exact).get_flat(0).expect("element");
    let rational = element
        .as_num()
        .expect("number")
        .to_rational()
        .expect("exact value");
    assert_eq!(rational.to_string(), "1/3", "the value must stay exact");
    assert_eq!(
        repr(&element),
        "1/3",
        "repr of the exact element names the rational"
    );

    // An `Int` tensor keeps its phase: dense would have to answer `3.0`
    // where the program wrote `3`.
    let mut interp = session("let c = [1, 2, 3]\n");
    let ints = interp.eval_toplevel("c").expect("c");
    assert!(!is_dense(&ints), "an Int tensor stays exact");
    assert_eq!(repr(&tensor_of(&ints).get_flat(0).expect("element")), "1");
}

#[test]
fn dense_storage_is_contiguous_and_matches_its_shape() {
    let mut interp = session("let m = [[1.0, 2.0, 3.0], [4.0, 5.0, 6.0]]\n");
    let matrix = interp.eval_toplevel("m").expect("m");
    let tensor = tensor_of(&matrix);
    assert!(tensor.is_dense());
    let data = tensor.dense().expect("dense buffer");
    assert_eq!(data, &[1.0, 2.0, 3.0, 4.0, 5.0, 6.0], "row-major order");
    assert_eq!(
        data.len(),
        TensorData::element_count(&tensor.shape),
        "the buffer covers the shape exactly"
    );
}

// --- 2. the two classes are indistinguishable ------------------------------

#[test]
fn dense_and_boxed_render_and_compare_identically() {
    let elements: Vec<Value> = [1.5, -2.0, 0.0, 1.0e300, -0.0, 3.25]
        .into_iter()
        .map(Value::float)
        .collect();
    let shape = vec![2, 3];
    let dense = Value::Tensor(Rc::new(TensorData {
        shape: shape.clone(),
        storage: Storage::Dense(Rc::new(vec![1.5, -2.0, 0.0, 1.0e300, -0.0, 3.25])),
        dim: aevia_dim::Dim::one(),
        display: None,
    }));
    let boxed = Value::Tensor(Rc::new(TensorData {
        shape,
        storage: Storage::Boxed(elements),
        dim: aevia_dim::Dim::one(),
        display: None,
    }));

    assert!(is_dense(&dense) && !is_dense(&boxed));
    assert_eq!(equal(&dense, &boxed), Ok(true), "structural equality");
    assert_eq!(show(&dense), show(&boxed), "show");
    assert_eq!(repr(&dense), repr(&boxed), "repr");
    // `-0` and `0` are distinct renderings and must survive either class.
    assert_eq!(
        repr(&tensor_of(&dense).get_flat(4).expect("element")),
        "-0.0",
        "a negative zero keeps its sign"
    );
    assert_same(&dense, &boxed, "dense vs boxed");
}

// --- 3. slicing and get_flat agree -----------------------------------------

#[test]
fn dense_slice_agrees_with_get_flat_on_a_non_contiguous_sequence() {
    let source = "let m = [[1.0, 2.0, 3.0], [4.0, 5.0, 6.0], [7.0, 8.0, 9.0]]\n\
                  let column = m[0..3, 1..2]\n\
                  let block = m[1..3, 1..3]\n\
                  let rows = m[1..3, 0..3]\n";
    let mut interp = session(source);
    let matrix = interp.eval_toplevel("m").expect("m");

    for (name, rows, columns) in [
        ("column", (0usize, 3usize), (1usize, 2usize)),
        ("block", (1, 3), (1, 3)),
        ("rows", (1, 3), (0, 3)),
    ] {
        let slice = interp.eval_toplevel(name).expect(name);
        let sliced = tensor_of(&slice);
        // §9.5: a slice is a view over the source's buffer, not a copy. The
        // storage class is the observable part of that claim; the values are
        // checked below against the source coordinates.
        assert!(sliced.is_view(), "`{name}` is a view");
        assert!(
            Rc::ptr_eq(
                sliced.buffer().expect("a buffer"),
                tensor_of(&matrix).buffer().expect("the source buffer")
            ),
            "`{name}` shares the source buffer"
        );
        assert_eq!(
            sliced.shape,
            vec![rows.1 - rows.0, columns.1 - columns.0],
            "`{name}` shape"
        );
        // Every element is the element `get_flat` reports at the same
        // coordinate of the source — including the strided column, whose
        // consecutive output elements come from consecutive rows.
        for (offset, element) in sliced.elements().iter().enumerate() {
            let row = rows.0 + offset / (columns.1 - columns.0);
            let column = columns.0 + offset % (columns.1 - columns.0);
            let expected = tensor_of(&matrix)
                .get(&[row as isize, column as isize])
                .expect("source element");
            assert_same(element, &expected, &format!("{name}[{offset}]"));
        }
        // The slice is also equal, element for element, to its own boxed
        // mirror — the class is an implementation detail.
        assert_eq!(
            equal(&slice, &boxed(&slice)),
            Ok(true),
            "`{name}` equals its boxed mirror"
        );
        assert_eq!(show(&slice), show(&boxed(&slice)), "`{name}` show");
        // A row slice of a row-major matrix is contiguous, so the `f64`
        // kernels can still borrow it; a column slice is not, and materialises
        // at the kernel boundary.
        assert_eq!(
            sliced.is_contiguous(),
            name == "rows",
            "`{name}` contiguity"
        );
    }
}

// --- 4. differential: elementwise arithmetic -------------------------------

const OPERANDS: &str = "let a = [1.5, -2.25, 3.0, 0.5]\n\
                        let b = [0.25, 4.0, -1.5, 2.0]\n\
                        let s = 0.75\n";

#[test]
fn elementwise_arithmetic_agrees_with_the_boxed_path() {
    let mut interp = session(OPERANDS);
    let a = interp.eval_toplevel("a").expect("a");
    let b = interp.eval_toplevel("b").expect("b");
    let s = interp.eval_toplevel("s").expect("s");
    assert!(is_dense(&a) && is_dense(&b), "the corpus is dense");

    for op in ["+", "-", "*", "/", "^"] {
        let dense = interp
            .apply_binary(op, a.clone(), b.clone(), Span::default())
            .expect("dense tensor op");
        let reference = interp
            .apply_binary(op, boxed(&a), boxed(&b), Span::default())
            .expect("boxed tensor op");
        assert_same(&dense, &reference, &format!("a {op} b"));

        // tensor ⊗ scalar and scalar ⊗ tensor
        let right = interp
            .apply_binary(op, a.clone(), s.clone(), Span::default())
            .expect("dense tensor op scalar");
        let right_ref = interp
            .apply_binary(op, boxed(&a), s.clone(), Span::default())
            .expect("boxed tensor op scalar");
        assert_same(&right, &right_ref, &format!("a {op} s"));

        if op != "^" {
            let left = interp
                .apply_binary(op, s.clone(), a.clone(), Span::default())
                .expect("dense scalar op tensor");
            let left_ref = interp
                .apply_binary(op, s.clone(), boxed(&a), Span::default())
                .expect("boxed scalar op tensor");
            assert_same(&left, &left_ref, &format!("s {op} a"));
        }
    }
}

#[test]
fn broadcast_and_mixed_storage_still_take_the_boxed_path() {
    // Broadcasting is not the fast path, and must still be right: the dense
    // result is compared with the boxed one.
    let source = "let m = [[1.0, 2.0], [3.0, 4.0]]\n\
                  let v = [[10.0, 20.0]]\n";
    let mut interp = session(source);
    let m = interp.eval_toplevel("m").expect("m");
    let v = interp.eval_toplevel("v").expect("v");
    let dense = interp
        .apply_binary("+", m.clone(), v.clone(), Span::default())
        .expect("broadcast");
    let reference = interp
        .apply_binary("+", boxed(&m), boxed(&v), Span::default())
        .expect("broadcast reference");
    assert_same(&dense, &reference, "broadcast");

    // A tensor that mixes phases is boxed, and the same expression agrees.
    // It is built here rather than written in source because a literal has a
    // single element type.
    let mixed = Value::Tensor(Rc::new(TensorData {
        shape: vec![2],
        storage: Storage::Boxed(vec![Value::float(1.0), Value::int(2)]),
        dim: aevia_dim::Dim::one(),
        display: None,
    }));
    assert!(!is_dense(&mixed), "a mixed tensor is boxed");
    let dense = interp
        .apply_binary("+", mixed.clone(), mixed.clone(), Span::default())
        .expect("mixed add");
    let reference = interp
        .apply_binary("+", boxed(&mixed), boxed(&mixed), Span::default())
        .expect("mixed add reference");
    assert_same(&dense, &reference, "mixed add");
}

// --- 5. differential: reductions -------------------------------------------

#[test]
fn reductions_agree_with_the_boxed_path() {
    let mut interp = session(OPERANDS);
    let a = interp.eval_toplevel("a").expect("a");

    for name in ["sum", "mean", "norm", "prod"] {
        let dense = interp
            .call_named(name, vec![a.clone()], Span::default())
            .expect("dense reduction");
        let reference = interp
            .call_named(name, vec![boxed(&a)], Span::default())
            .expect("boxed reduction");
        assert_same(&dense, &reference, name);
    }

    for order in [1_i64, 2, i64::MAX] {
        let dense = interp
            .call_named(
                "norm",
                vec![a.clone(), Value::int(order)],
                Span::default(),
            )
            .expect("dense norm");
        let reference = interp
            .call_named("norm", vec![boxed(&a), Value::int(order)], Span::default())
            .expect("boxed norm");
        assert_same(&dense, &reference, &format!("norm order {order}"));
    }

    // A one-element tensor keeps its display unit through `sum`, exactly as
    // the boxed reduction (which returns its first element) does.
    let mut interp = session("let one = [2.0 km]\n");
    let one = interp.eval_toplevel("one").expect("one");
    let dense = interp
        .call_named("sum", vec![one.clone()], Span::default())
        .expect("dense sum");
    let reference = interp
        .call_named("sum", vec![boxed(&one)], Span::default())
        .expect("boxed sum");
    assert_same(&dense, &reference, "sum of one element");
}

/// The `@` differential.
///
/// `matrix @ matrix` now runs on `aevia_std::linalg::matmul`, a blocked kernel
/// that reassociates the inner sum; the boxed path is still the sequential
/// loop. Bit-equality between the two is therefore no longer a property the
/// operator has, and asserting it would assert that the kernel is not used.
///
/// Two decisions are recorded here rather than left implicit:
///
/// * The assertion is *kept* at bit equality for the operands below, because
///   every product and every partial sum in them is exactly representable
///   (`1 * 0.5 + 2 * 2` and friends): no order of summation rounds, so a
///   reassociation that changed a bit would be a wrong answer, not a rounding.
///   That is the same invariant `aevia_std`'s
///   `matmul_is_exact_on_an_exactly_representable_product` pins.
/// * The general contract — same shape, same dimension, same numeric phase,
///   agreement to a relative tolerance — is asserted by
///   `matmul_reassociates_but_agrees` below, at a size where the kernel really
///   does sum in a different order.
///
/// The alternative, gating the kernel on operand size so this test keeps
/// running the loop, was rejected: `@`'s last bits would then depend on the
/// size of its inputs, and the fast path would be the one path no test covers.
#[test]
fn matmul_agrees_with_the_boxed_path() {
    let source = "let m = [[1.0, 2.0], [3.0, 4.0]]\n\
                  let v = [5.0, 6.0]\n\
                  let w = [1.5, -2.0]\n\
                  let n = [[0.5, 1.0], [2.0, -1.0]]\n";
    let mut interp = session(source);
    let m = interp.eval_toplevel("m").expect("m");
    let v = interp.eval_toplevel("v").expect("v");
    let w = interp.eval_toplevel("w").expect("w");
    let n = interp.eval_toplevel("n").expect("n");

    for (left, right, label) in [
        (m.clone(), v.clone(), "matrix @ vector"),
        (m.clone(), n.clone(), "matrix @ matrix"),
        (v.clone(), n.clone(), "vector @ matrix"),
        (v.clone(), w.clone(), "dot"),
    ] {
        let dense = interp
            .apply_binary("@", left.clone(), right.clone(), Span::default())
            .expect("dense matmul");
        let reference = interp
            .apply_binary("@", boxed(&left), boxed(&right), Span::default())
            .expect("boxed matmul");
        assert_same(&dense, &reference, label);
    }
}

/// A deterministic pseudo-random matrix, so the tolerance below covers a shape
/// and a value distribution rather than one hand-picked case.
fn pseudo_random(rows: usize, cols: usize, seed: u64) -> Vec<f64> {
    let mut state = seed.wrapping_mul(6_364_136_223_846_793_005).wrapping_add(1);
    (0..rows * cols)
        .map(|_| {
            state = state
                .wrapping_mul(6_364_136_223_846_793_005)
                .wrapping_add(1_442_695_040_888_963_407);
            let unit = ((state >> 11) as f64) / ((1u64 << 53) as f64);
            unit * 8.0 - 4.0
        })
        .collect()
}

fn dense_tensor(rows: usize, cols: usize, values: Vec<f64>) -> Value {
    Value::Tensor(Rc::new(TensorData {
        shape: vec![rows, cols],
        storage: Storage::Dense(Rc::new(values)),
        dim: aevia_dim::Dim::one(),
        display: None,
    }))
}

/// The `@` differential where the blocked kernel reassociates: the answers
/// agree to rounding, and the shape, dimension and numeric phase still agree
/// exactly.
#[test]
fn matmul_reassociates_but_agrees() {
    let left = dense_tensor(24, 19, pseudo_random(24, 19, 1));
    let right = dense_tensor(19, 17, pseudo_random(19, 17, 2));
    let mut interp = session("");
    let dense = interp
        .apply_binary("@", left.clone(), right.clone(), Span::default())
        .expect("dense matmul");
    let reference = interp
        .apply_binary("@", boxed(&left), boxed(&right), Span::default())
        .expect("boxed matmul");

    let (fast, slow) = (tensor_of(&dense), tensor_of(&reference));
    assert_eq!(fast.shape, vec![24, 17], "shape");
    assert_eq!(fast.dim, slow.dim, "element dimension");
    assert_eq!(fast.storage.len(), slow.storage.len(), "element count");
    for index in 0..fast.storage.len() {
        let x = fast.get_flat(index).expect("dense element");
        let y = slow.get_flat(index).expect("boxed element");
        let (Value::Num(x), Value::Num(y)) = (&x, &y) else {
            panic!("at {index}: expected two scalars");
        };
        assert_eq!(x.kind.type_name(), y.kind.type_name(), "at {index}: phase");
        let (NumKind::Float(x), NumKind::Float(y)) = (&x.kind, &y.kind) else {
            panic!("at {index}: expected two floats");
        };
        let scale = x.abs().max(y.abs()).max(1.0);
        assert!(
            (x - y).abs() <= 1e-12 * scale,
            "at {index}: {x} vs {y}"
        );
    }
}

// --- 6. differential: a program corpus -------------------------------------

/// `(left, operator, right)` programs: the existing tensor tests plus the
/// operations `examples/tensors.aev` performs.
const CORPUS: &[(&str, &str, &str)] = &[
    ("[[1.0, 2.0], [3.0, 4.0]]", "+", "[[5.0, 6.0], [7.0, 8.0]]"),
    ("[[1.0, 2.0], [3.0, 4.0]]", "-", "[[5.0, 6.0], [7.0, 8.0]]"),
    ("[[1.0, 2.0], [3.0, 4.0]]", "*", "[[5.0, 6.0], [7.0, 8.0]]"),
    ("[[1.0, 2.0], [3.0, 4.0]]", "/", "[[5.0, 6.0], [7.0, 8.0]]"),
    ("[1.0, 2.0, 3.0, 4.0]", "+", "[4.0, 3.0, 2.0, 1.0]"),
    ("[1.0, 2.0, 3.0, 4.0]", "*", "[0.5, 0.5, 0.5, 0.5]"),
    ("[1.0, 2.0]", "+", "[3.0, 4.0]"),
    ("[5.0, 6.0]", "*", "[5.0, 6.0]"),
];

#[test]
fn program_corpus_matches_the_boxed_path() {
    for (left, op, right) in CORPUS {
        let source = format!("let a = {left}\nlet b = {right}\n");
        let mut interp = session(&source);
        let a = interp.eval_toplevel("a").expect("a");
        let b = interp.eval_toplevel("b").expect("b");
        assert!(is_dense(&a) && is_dense(&b), "{left} and {right} are dense");

        let dense = interp
            .apply_binary(op, a.clone(), b.clone(), Span::default())
            .expect("dense");
        let reference = interp
            .apply_binary(op, boxed(&a), boxed(&b), Span::default())
            .expect("boxed");
        assert_same(&dense, &reference, &format!("{left} {op} {right}"));
        assert_eq!(
            equal(&dense, &reference),
            Ok(true),
            "{left} {op} {right}: equality"
        );
    }
}

#[test]
fn the_example_tensor_program_agrees_with_the_boxed_path() {
    // The same operands and reductions `examples/tensors.aev` uses, with the
    // dense result compared against the boxed one.
    let mut interp = session("let m = [[1.0, 2.0], [3.0, 4.0]]\nlet v = [5.0, 6.0]\n");
    let m = interp.eval_toplevel("m").expect("m");
    let v = interp.eval_toplevel("v").expect("v");

    let dense = interp
        .apply_binary("@", m.clone(), v.clone(), Span::default())
        .expect("M @ v");
    let reference = interp
        .apply_binary("@", boxed(&m), boxed(&v), Span::default())
        .expect("M @ v boxed");
    assert_same(&dense, &reference, "M @ v");
    assert_eq!(
        equal(&dense, &Value::Tensor(Rc::new(TensorData {
            shape: vec![2],
            storage: Storage::Dense(Rc::new(vec![17.0, 39.0])),
            dim: aevia_dim::Dim::one(),
            display: None,
        }))),
        Ok(true),
        "M @ v is [17, 39]"
    );

    for name in ["sum", "mean", "norm"] {
        let dense = interp
            .call_named(name, vec![v.clone()], Span::default())
            .expect("reduction");
        let reference = interp
            .call_named(name, vec![boxed(&v)], Span::default())
            .expect("boxed reduction");
        assert_same(&dense, &reference, name);
    }

    // `shape(m)` is exact storage, not dense, and unchanged.
    let shape = interp
        .call_named("shape", vec![m.clone()], Span::default())
        .expect("shape");
    assert!(!is_dense(&shape), "`shape` is an exact Int tensor");
    assert_eq!(repr(&shape), "[2, 2]");
}

// --- 7. the fast paths are guarded by storage class ------------------------

#[test]
fn the_fast_paths_decline_non_dense_storage() {
    use aevia_syntax::ast::BinOp;

    let dense = Value::Tensor(Rc::new(TensorData {
        shape: vec![2],
        storage: Storage::Dense(Rc::new(vec![1.0, 2.0])),
        dim: aevia_dim::Dim::one(),
        display: None,
    }));
    let boxed = boxed(&dense);
    let (Value::Tensor(dense_data), Value::Tensor(boxed_data)) = (&dense, &boxed) else {
        panic!("both are tensors");
    };

    assert!(dense_data.dense().is_some());
    assert!(boxed_data.dense().is_none());
    assert!(dense_data.dense_sum().is_some());
    assert!(boxed_data.dense_sum().is_none());
    assert!(dense_data.dense_prod().is_some());
    assert!(boxed_data.dense_prod().is_none());
    assert!(
        dense_data.dense_binary(BinOp::Add, dense_data).is_some(),
        "dense + dense takes the fast path"
    );
    assert!(
        dense_data.dense_binary(BinOp::Add, boxed_data).is_none(),
        "dense + boxed does not"
    );
    assert!(
        boxed_data.dense_binary(BinOp::Add, boxed_data).is_none(),
        "boxed + boxed does not"
    );
    // An exact tensor never exposes a buffer, even when every value is an
    // integer that `f64` could hold.
    let exact = Value::Tensor(Rc::new(TensorData {
        shape: vec![2],
        storage: Storage::Exact(vec![
            num_rational::BigRational::from_integer(1.into()),
            num_rational::BigRational::from_integer(2.into()),
        ]),
        dim: aevia_dim::Dim::one(),
        display: None,
    }));
    let (Value::Tensor(exact_data),) = (&exact,) else {
        panic!("a tensor");
    };
    assert!(exact_data.dense().is_none());
    assert!(exact_data.dense_sum().is_none());
}

// --- 8. measurement --------------------------------------------------------

/// Times `xs + xs` on 100k elements through both paths.
///
/// Run with `cargo test --release -p aevia-core --test tensors_dense --
/// --ignored --nocapture`. The boxed measurement is the *old* code path: the
/// fast path is skipped for a boxed operand, so one binary measures both.
#[test]
#[ignore = "measurement harness"]
fn bench_dense_vs_boxed_100k() {
    let mut interp = session("let xs = linspace(0.0, 1.0, 100000)\n");
    let xs = interp.eval_toplevel("xs").expect("xs");
    assert!(is_dense(&xs), "the benchmark operand is dense");
    let mirror = boxed(&xs);
    const ROUNDS: usize = 20;

    let mut measure = |operand: &Value, label: &str| {
        // Warm up: the first pass touches cold allocations on both sides.
        for _ in 0..2 {
            let result = interp
                .apply_binary("+", operand.clone(), operand.clone(), Span::default())
                .expect("add");
            std::hint::black_box(&result);
        }
        let start = Instant::now();
        for _ in 0..ROUNDS {
            let result = interp
                .apply_binary("+", operand.clone(), operand.clone(), Span::default())
                .expect("add");
            std::hint::black_box(&result);
        }
        let elapsed = start.elapsed();
        let per_op = elapsed.as_secs_f64() * 1e3 / ROUNDS as f64;
        println!("{label}: {per_op:.3} ms per `xs + xs` (100k elements)");
        per_op
    };

    let dense = measure(&xs, "dense");
    let boxed = measure(&mirror, "boxed");
    println!(
        "speedup: {:.1}x (dense {dense:.3} ms, boxed {boxed:.3} ms)",
        boxed / dense
    );
    assert!(dense < boxed, "the fast path must not be slower");

    let mut reduce = |operand: &Value, label: &str| {
        for _ in 0..2 {
            let result = interp
                .call_named("sum", vec![operand.clone()], Span::default())
                .expect("sum");
            std::hint::black_box(&result);
        }
        let start = Instant::now();
        for _ in 0..ROUNDS {
            let result = interp
                .call_named("sum", vec![operand.clone()], Span::default())
                .expect("sum");
            std::hint::black_box(&result);
        }
        let per_op = start.elapsed().as_secs_f64() * 1e3 / ROUNDS as f64;
        println!("{label}: {per_op:.3} ms per `sum(xs)` (100k elements)");
        per_op
    };
    let dense_sum = reduce(&xs, "dense sum");
    let boxed_sum = reduce(&mirror, "boxed sum");
    println!(
        "sum speedup: {:.1}x (dense {dense_sum:.3} ms, boxed {boxed_sum:.3} ms)",
        boxed_sum / dense_sum
    );
    assert!(dense_sum < boxed_sum, "the reduction path must not be slower");
}

/// The loop `@` ran before the dispatcher, verbatim, so one binary can measure
/// both sides of the change.
fn sequential_matmul(rows: usize, inner: usize, cols: usize, a: &[f64], b: &[f64]) -> Vec<f64> {
    let mut data = vec![0.0; rows * cols];
    for row in 0..rows {
        for column in 0..cols {
            let mut total = a[row * inner] * b[column];
            for step in 1..inner {
                total += a[row * inner + step] * b[step * cols + column];
            }
            data[row * cols + column] = total;
        }
    }
    data
}

/// Times `@` on 256×256, before and after the move onto the dispatcher.
///
/// Run with `RAYON_NUM_THREADS=2 cargo test --release -p aevia-core --test
/// tensors_dense -- --ignored --nocapture bench_matmul_256`. `RAYON_NUM_THREADS`
/// is the project's rule (§3.5): `faer` defaults to every core, which is slower
/// on a machine already shared with other builds.
#[test]
#[ignore = "measurement harness"]
fn bench_matmul_256() {
    const SIZE: usize = 256;
    const ROUNDS: usize = 15;
    let a = pseudo_random(SIZE, SIZE, 1);
    let b = pseudo_random(SIZE, SIZE, 2);
    let left = dense_tensor(SIZE, SIZE, a.clone());
    let right = dense_tensor(SIZE, SIZE, b.clone());
    let mut interp = session("");

    fn timed(label: &str, rounds: usize, mut run: impl FnMut()) -> f64 {
        for _ in 0..2 {
            run();
        }
        let start = Instant::now();
        for _ in 0..rounds {
            run();
        }
        let per_op = start.elapsed().as_secs_f64() * 1e3 / rounds as f64;
        println!("{label}: {per_op:.3} ms per 256x256 `@`");
        per_op
    }

    let after = timed("after  (dispatcher)", ROUNDS, || {
        let result = interp
            .apply_binary("@", left.clone(), right.clone(), Span::default())
            .expect("@");
        std::hint::black_box(&result);
    });
    let before = timed("before (sequential loop)", ROUNDS, || {
        std::hint::black_box(sequential_matmul(SIZE, SIZE, SIZE, &a, &b));
    });
    println!("speedup: {:.1}x", before / after);
    assert!(after < before, "the dispatcher must not be slower");
}
