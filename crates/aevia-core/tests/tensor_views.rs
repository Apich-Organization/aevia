//! Strided views (§6.6.4, §9.5).
//!
//! A slice of an `f64` tensor is a *view*: an offset and one stride per
//! dimension over the buffer it already has. Two things have to be true for
//! that to be a representation choice rather than a second semantics:
//!
//! 1. **Sharing is real.** The view and its source must point at the same
//!    allocation, so `Rc::ptr_eq` on their buffers must hold and the buffer
//!    must still be the *source's* length, not the window's. A test that only
//!    compared values would pass against the old copying implementation.
//! 2. **A view and its copy are indistinguishable.** Same shape, same
//!    dimension, same elements, same rendering, same equality, and the same
//!    result from every operation — which is what the parity corpus checks by
//!    comparing `Value`s, not text.
//!
//! Assertions here are on values, shapes and storage classes. `show`/`repr`
//! appear only where the requirement *is* that the two classes render
//! identically, and there the two strings are compared to each other.

#![allow(clippy::result_large_err)]

use aevia_core::builtins::call_typed;
use aevia_core::value::{equal, repr, show, NumKind, Storage, TensorData, Value};
use aevia_core::{compile, Interp};
use aevia_syntax::ast::{NatAst, ShapeAst, ShapeItem, Ty, TyKind};
use aevia_syntax::Span;
use std::rc::Rc;

// --- harness ---------------------------------------------------------------

fn session(source: &str) -> Interp {
    let compilation = compile("views.aev", source);
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

/// The same tensor materialised: a fresh contiguous buffer, the value a
/// kernel that cannot walk strides would have to compute from.
fn materialised(value: &Value) -> Value {
    let tensor = tensor_of(value);
    Value::Tensor(Rc::new(tensor.to_dense().expect("an f64 tensor")))
}

/// The same elements in boxed storage, for the class-independence checks.
fn boxed(value: &Value) -> Value {
    let tensor = tensor_of(value);
    Value::Tensor(Rc::new(TensorData {
        shape: tensor.shape.clone(),
        storage: Storage::Boxed(tensor.elements()),
        dim: tensor.dim.clone(),
        display: tensor.display.clone(),
    }))
}

/// A tensor's elements as `f64`, in the tensor's own row-major order.
fn values(value: &Value) -> Vec<f64> {
    tensor_of(value)
        .elements()
        .iter()
        .map(|element| element.as_f64().expect("an f64 element"))
        .collect()
}

/// Strict equality: kind, shape, dimension and element *bits*.
///
/// `Value::equal` compares floats with `==`, which cannot tell `-0.0` from
/// `0.0`. A view that read the wrong slot would usually differ in value, but
/// "usually" is not the standard a representation has to meet.
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

const MATRIX: &str = "let m = [[1.0, 2.0, 3.0], [4.0, 5.0, 6.0], [7.0, 8.0, 9.0]]\n";

// --- 1. sharing is real ----------------------------------------------------

#[test]
fn a_slice_shares_the_source_buffer() {
    let mut interp = session(&format!("{MATRIX}let s = m[1..3, 0..3]\n"));
    let m = interp.eval_toplevel("m").expect("m");
    let s = interp.eval_toplevel("s").expect("s");

    assert!(tensor_of(&s).is_view(), "a slice is a view");
    let source = tensor_of(&m).buffer().expect("the source buffer");
    let window = tensor_of(&s).buffer().expect("the view buffer");
    assert!(
        Rc::ptr_eq(source, window),
        "the slice must share the source's allocation"
    );
    // The shared buffer is the whole source, so nothing was copied: a copying
    // slice would have a 6-element buffer of its own.
    assert_eq!(source.len(), 9, "the buffer is still the source's");
    assert_eq!(tensor_of(&s).storage.len(), 6, "the window is 2×3");

    // The window is the description the class claims: row 1 onwards, and the
    // source's row-major strides.
    assert_eq!(tensor_of(&s).offset(), 3, "row 1 of a 3×3");
    assert_eq!(tensor_of(&s).strides(), Some(&[3usize, 1][..]), "strides");
    assert_eq!(tensor_of(&s).shape, vec![2, 3], "shape");
}

#[test]
fn a_slice_of_a_view_is_a_view_of_the_same_buffer() {
    let mut interp = session(&format!(
        "{MATRIX}let s = m[1..3, 0..3]\nlet t = s[0..2, 1..3]\n"
    ));
    let m = interp.eval_toplevel("m").expect("m");
    let s = interp.eval_toplevel("s").expect("s");
    let t = interp.eval_toplevel("t").expect("t");

    assert!(tensor_of(&t).is_view(), "a slice of a view is a view");
    assert!(
        Rc::ptr_eq(
            tensor_of(&m).buffer().expect("source"),
            tensor_of(&t).buffer().expect("window")
        ),
        "the second slice still shares the original buffer"
    );
    assert!(
        Rc::ptr_eq(
            tensor_of(&s).buffer().expect("source"),
            tensor_of(&t).buffer().expect("window")
        ),
        "and it shares its parent's buffer"
    );
    // `s` starts at row 1; `t` adds one column, so the first element is the
    // `(1, 1)` element of the source.
    assert_eq!(tensor_of(&t).offset(), 4, "offset accumulates");
    assert_eq!(tensor_of(&t).strides(), Some(&[3usize, 1][..]), "strides");
    assert_eq!(tensor_of(&t).shape, vec![2, 2], "shape");
    assert_eq!(
        tensor_of(&t).get(&[0, 0]).and_then(|v| v.as_f64()),
        Some(5.0),
        "t[0, 0] is m[1, 1]"
    );
}

#[test]
fn indexing_the_leading_axes_is_a_view_too() {
    let mut interp = session(&format!("{MATRIX}let row = m[1]\n"));
    let m = interp.eval_toplevel("m").expect("m");
    let row = interp.eval_toplevel("row").expect("row");

    assert!(tensor_of(&row).is_view(), "m[1] is a view");
    assert!(
        Rc::ptr_eq(
            tensor_of(&m).buffer().expect("source"),
            tensor_of(&row).buffer().expect("row")
        ),
        "the row shares the matrix's buffer"
    );
    assert_eq!(tensor_of(&row).offset(), 3, "row 1 of a 3×3");
    assert_eq!(tensor_of(&row).shape, vec![3], "shape");
    for (index, expected) in [4.0, 5.0, 6.0].into_iter().enumerate() {
        assert_eq!(
            tensor_of(&row)
                .get(&[index as isize])
                .and_then(|value| value.as_f64()),
            Some(expected),
            "row[{index}]"
        );
    }
}

#[test]
fn a_transpose_permutes_strides_and_moves_no_data() {
    let mut interp = session(&format!("{MATRIX}let t = transpose(m)\n"));
    let m = interp.eval_toplevel("m").expect("m");
    let t = interp.eval_toplevel("t").expect("t");

    assert!(tensor_of(&t).is_view(), "a transpose is a view");
    assert!(
        Rc::ptr_eq(
            tensor_of(&m).buffer().expect("source"),
            tensor_of(&t).buffer().expect("transpose")
        ),
        "the transpose shares the matrix's buffer"
    );
    assert_eq!(tensor_of(&t).shape, vec![3, 3], "shape");
    assert_eq!(tensor_of(&t).strides(), Some(&[1usize, 3][..]), "strides");
    assert_eq!(tensor_of(&t).offset(), 0, "offset");
    // The strides are a permutation, not row-major, so a 3×3 transpose is not
    // contiguous and materialises at the kernel boundary.
    assert!(!tensor_of(&t).is_contiguous(), "a transpose is strided");
    for row in 0..3usize {
        for column in 0..3usize {
            assert_eq!(
                tensor_of(&t)
                    .get(&[row as isize, column as isize])
                    .and_then(|value| value.as_f64()),
                tensor_of(&m)
                    .get(&[column as isize, row as isize])
                    .and_then(|value| value.as_f64()),
                "t[{row}, {column}] == m[{column}, {row}]"
            );
        }
    }
}

// --- 2. a view and its copy are indistinguishable --------------------------

/// Slice, transpose and index programs. Each binds `p`, whose value must be
/// the same whether it is a view or a materialised copy.
const CORPUS: &[(&str, &str)] = &[
    (MATRIX, "m[1..3, 0..3]"),
    (MATRIX, "m[0..3, 1..2]"),
    (MATRIX, "m[0..2, 0..2]"),
    (MATRIX, "m[1..2, 1..2]"),
    (MATRIX, "m[2..3, 0..3]"),
    (MATRIX, "transpose(m)"),
    (MATRIX, "transpose(m)[0..2, 1..3]"),
    (MATRIX, "m[1..3, 0..3][0..2, 1..3]"),
    (MATRIX, "m[1]"),
    ("let v = [1.5, -2.25, 3.0, 0.5, 8.0]\n", "v[1..4]"),
    ("let v = [1.5, -2.25, 3.0, 0.5, 8.0]\n", "v[0..4][1..3]"),
    ("let v = [1.5, -2.25, 3.0, 0.5, 8.0]\n", "v[..]"),
];

#[test]
fn a_view_and_its_copy_are_the_same_value() {
    for (source, expression) in CORPUS {
        let mut interp = session(&format!("{source}let p = {expression}\n"));
        let view = interp.eval_toplevel("p").expect(expression);
        assert!(
            tensor_of(&view).is_view(),
            "`{expression}` is a view, not a copy"
        );
        let copy = materialised(&view);

        // Same `Value`: equality, shape, dimension, element kind and bits,
        // rendering and repr. This is the parity requirement.
        assert_eq!(equal(&view, &copy), Ok(true), "`{expression}`: equality");
        assert_eq!(
            tensor_of(&view).shape,
            tensor_of(&copy).shape,
            "`{expression}`: shape"
        );
        assert_eq!(
            tensor_of(&view).dim,
            tensor_of(&copy).dim,
            "`{expression}`: dimension"
        );
        assert_same(&view, &copy, expression);

        // The view is also equal to its own boxed mirror: the class is not
        // part of the value.
        assert_eq!(
            equal(&view, &boxed(&view)),
            Ok(true),
            "`{expression}`: boxed mirror"
        );
        // And every element read through a coordinate agrees.
        let shape = tensor_of(&view).shape.clone();
        let count = TensorData::element_count(&shape);
        for index in 0..count {
            let mut rest = index;
            let mut coordinates = vec![0isize; shape.len()];
            for axis in (0..shape.len()).rev() {
                let extent = shape[axis].max(1);
                coordinates[axis] = (rest % extent) as isize;
                rest /= extent;
            }
            let through_view = tensor_of(&view)
                .get(&coordinates)
                .expect("a view element");
            let through_copy = tensor_of(&copy)
                .get(&coordinates)
                .expect("a copy element");
            assert_same(
                &through_view,
                &through_copy,
                &format!("`{expression}` at {coordinates:?}"),
            );
        }
    }
}

#[test]
fn arithmetic_and_reductions_agree_between_a_view_and_its_copy() {
    let source = format!("{MATRIX}let s = m[1..3, 0..3]\nlet c = m[0..2, 0..2]\n");
    let mut interp = session(&source);
    let view = interp.eval_toplevel("s").expect("s");
    let copy = materialised(&view);
    let square = interp.eval_toplevel("c").expect("c");
    let square_copy = materialised(&square);

    for op in ["+", "-", "*", "/", "^"] {
        let from_view = interp
            .apply_binary(op, view.clone(), view.clone(), Span::default())
            .expect("view arithmetic");
        let from_copy = interp
            .apply_binary(op, copy.clone(), copy.clone(), Span::default())
            .expect("copy arithmetic");
        assert_same(&from_view, &from_copy, &format!("view {op} view"));

        let scaled_view = interp
            .apply_binary(op, view.clone(), Value::float(1.5), Span::default())
            .expect("view scalar arithmetic");
        let scaled_copy = interp
            .apply_binary(op, copy.clone(), Value::float(1.5), Span::default())
            .expect("copy scalar arithmetic");
        assert_same(&scaled_view, &scaled_copy, &format!("view {op} 1.5"));
    }

    for name in ["sum", "prod", "mean", "norm", "min_all", "max_all"] {
        let from_view = interp
            .call_named(name, vec![view.clone()], Span::default())
            .expect("view reduction");
        let from_copy = interp
            .call_named(name, vec![copy.clone()], Span::default())
            .expect("copy reduction");
        assert_same(&from_view, &from_copy, name);
    }

    // `@` on a strided view: the product of two square views, and the
    // product of their copies, must agree bit for bit.
    let from_view = interp
        .apply_binary("@", square.clone(), square.clone(), Span::default())
        .expect("view matmul");
    let from_copy = interp
        .apply_binary("@", square_copy.clone(), square_copy.clone(), Span::default())
        .expect("copy matmul");
    assert_same(&from_view, &from_copy, "view @ view");

    // A transpose of a view is still a view of the same buffer, and its
    // product with the copy is the copy's product.
    let transposed = interp
        .call_named("transpose", vec![square.clone()], Span::default())
        .expect("transpose of a view");
    assert!(
        tensor_of(&transposed).is_view(),
        "transpose of a view is a view"
    );
    assert!(
        Rc::ptr_eq(
            tensor_of(&square).buffer().expect("square"),
            tensor_of(&transposed).buffer().expect("transposed")
        ),
        "the transposed view shares the sliced buffer"
    );
    let transposed_copy = interp
        .call_named("transpose", vec![square_copy.clone()], Span::default())
        .expect("transpose of a copy");
    assert_same(&transposed, &transposed_copy, "transpose parity");
}

#[test]
fn a_contiguous_view_takes_the_f64_path_and_a_strided_one_materialises() {
    let mut interp = session(&format!("{MATRIX}let rows = m[1..3, 0..3]\nlet column = m[0..3, 1..2]\n"));
    let rows = interp.eval_toplevel("rows").expect("rows");
    let column = interp.eval_toplevel("column").expect("column");

    // A row slice of a row-major matrix is contiguous, so the buffer is
    // borrowed and `dense()` answers.
    assert!(tensor_of(&rows).is_contiguous(), "rows are contiguous");
    assert!(
        tensor_of(&rows).dense().is_some(),
        "a contiguous view exposes its window"
    );
    assert_eq!(
        tensor_of(&rows).dense().expect("window"),
        &[4.0, 5.0, 6.0, 7.0, 8.0, 9.0],
        "the window is the rows themselves"
    );

    // A column slice is not: `dense()` declines and `dense_flat` materialises
    // in logical order, which is the order every kernel expects.
    assert!(!tensor_of(&column).is_contiguous(), "a column is strided");
    assert!(
        tensor_of(&column).dense().is_none(),
        "a strided view does not pretend to be contiguous"
    );
    assert_eq!(
        tensor_of(&column)
            .dense_flat()
            .expect("materialised window")
            .to_vec(),
        vec![2.0, 5.0, 8.0],
        "materialising walks the strides in row-major order"
    );
}

#[test]
fn reshape_is_a_view_of_a_contiguous_window_and_a_copy_otherwise() {
    // §6.6.4 / §9.5: "reshape 在连续内存上返回视图，否则复制." The row window
    // `m[1..3, 0..3]` is contiguous, so the reshaped tensor must share the
    // parent's buffer; a column window is strided, so it is materialised.
    let mut interp = session(&format!(
        "{MATRIX}let s = m[1..3, 0..3]\nlet c = m[0..3, 1..2]\n"
    ));
    let view = interp.eval_toplevel("s").expect("s");
    assert!(tensor_of(&view).is_view(), "the operand is a view");
    assert!(
        tensor_of(&view).is_contiguous(),
        "the row window is contiguous"
    );
    let source = interp.eval_toplevel("m").expect("m");

    let ty = Ty::new(
        TyKind::ShapeArg(ShapeAst::Refined(vec![
            ShapeItem::Known(NatAst::Lit(3)),
            ShapeItem::Known(NatAst::Lit(2)),
        ])),
        Span::default(),
    );
    let reshaped = call_typed(&mut interp, "reshape", &[ty], vec![view], Span::default())
        .expect("a 2×3 window has six elements");
    let Value::Variant(result) = &reshaped else {
        panic!("reshape answers with a Result");
    };
    assert_eq!(result.index, 0, "the reshape succeeds");
    let Value::Tensor(payload) = &result.fields[0] else {
        panic!("the `Ok` payload is a tensor");
    };
    assert_eq!(payload.shape, vec![3, 2], "the new shape");
    assert!(
        Rc::ptr_eq(
            payload.buffer().expect("reshaped buffer"),
            tensor_of(&source).buffer().expect("source buffer")
        ),
        "a contiguous reshape is a view of the same buffer, not a copy"
    );
    assert_eq!(
        payload
            .elements()
            .iter()
            .map(|element| element.as_f64().unwrap_or(f64::NAN))
            .collect::<Vec<_>>(),
        vec![4.0, 5.0, 6.0, 7.0, 8.0, 9.0],
        "the elements keep the view's row-major order"
    );

    // The strided column window cannot be a view: no strides of the new shape
    // describe it, so it is copied in row-major (logical) order.
    let column = interp.eval_toplevel("c").expect("c");
    assert!(
        !tensor_of(&column).is_contiguous(),
        "the column window is strided"
    );
    let ty = Ty::new(
        TyKind::ShapeArg(ShapeAst::Refined(vec![
            ShapeItem::Known(NatAst::Lit(1)),
            ShapeItem::Known(NatAst::Lit(3)),
        ])),
        Span::default(),
    );
    let reshaped = call_typed(&mut interp, "reshape", &[ty], vec![column], Span::default())
        .expect("a 3-element column has three elements");
    let Value::Variant(result) = &reshaped else {
        panic!("reshape answers with a Result");
    };
    assert_eq!(result.index, 0, "the reshape succeeds");
    let Value::Tensor(payload) = &result.fields[0] else {
        panic!("the `Ok` payload is a tensor");
    };
    assert_eq!(payload.shape, vec![1, 3]);
    assert!(!payload.is_view(), "a strided reshape materialises");
    assert!(
        !Rc::ptr_eq(
            payload.buffer().expect("materialised buffer"),
            tensor_of(&source).buffer().expect("source buffer")
        ),
        "the copy is a fresh allocation"
    );
    assert_eq!(
        payload
            .elements()
            .iter()
            .map(|element| element.as_f64().unwrap_or(f64::NAN))
            .collect::<Vec<_>>(),
        vec![2.0, 5.0, 8.0],
        "the strided window is walked in row-major order"
    );
}

#[test]
fn a_view_keeps_the_element_dimension_and_display_unit() {
    // A dimensioned `f64` tensor is built here rather than written in source:
    // a unit-bearing decimal literal is an *exact* quantity (§6.3.2), so a
    // source-level tensor of them is not in the `f64` class at all.
    let metre = Rc::new(
        aevia_dim::UnitTable::si()
            .lookup("m")
            .expect("the metre is in the SI table"),
    );
    let whole = TensorData {
        shape: vec![4],
        storage: Storage::Dense(Rc::new(vec![1.0, 2.0, 3.0, 4.0])),
        dim: metre.dim.clone(),
        display: Some(metre.clone()),
    };
    let view = whole.slice_view(&[(1, 3)]).expect("a view");
    assert!(view.is_view(), "the slice is a view");
    assert_eq!(view.dim, whole.dim, "the view keeps the element dimension");
    assert_eq!(
        view.display.as_ref().map(|unit| unit.name.clone()),
        Some("m".to_string()),
        "the view keeps the display unit"
    );
    assert_eq!(
        view.elements()
            .iter()
            .map(|element| element.as_f64().unwrap_or(f64::NAN))
            .collect::<Vec<_>>(),
        vec![2.0, 3.0],
        "the window is the sliced elements"
    );

    // The reduction over the view names the same unit as the reduction over
    // its materialised copy, because both carry the same dimension and the
    // same display unit.
    let mut interp = Interp::new(&aevia_types::CheckedModule::default());
    let from_view = interp
        .call_named("sum", vec![Value::Tensor(Rc::new(view.clone()))], Span::default())
        .expect("sum of the view");
    let from_copy = interp
        .call_named(
            "sum",
            vec![Value::Tensor(Rc::new(view.to_dense().expect("a copy")))],
            Span::default(),
        )
        .expect("sum of the copy");
    assert_same(&from_view, &from_copy, "sum of a dimensioned view");
    assert_eq!(
        from_view.as_num().expect("a number").dim,
        whole.dim,
        "the total carries the element dimension"
    );
    // A one-element view keeps its display unit through the reduction, which
    // is what the boxed reduction does — it returns the element it started
    // from unchanged. A wider reduction drops it, in both classes.
    let single = whole.slice_view(&[(1, 2)]).expect("a one-element view");
    let total = interp
        .call_named("sum", vec![Value::Tensor(Rc::new(single))], Span::default())
        .expect("sum of a one-element view");
    assert_eq!(
        total
            .as_num()
            .expect("a number")
            .display
            .as_ref()
            .map(|unit| unit.name.clone()),
        Some("m".to_string()),
        "a one-element view keeps the display unit"
    );
}

// --- 3. a reversed axis is a view, not a copy ------------------------------

const VECTOR: &str = "let v = [1.5, -2.25, 3.0, 0.5, 8.0]\n";

#[test]
fn a_reversed_slice_has_the_reversed_values() {
    let mut interp = session(&format!("{VECTOR}let r = reverse(v)\n"));
    let v = interp.eval_toplevel("v").expect("v");
    let r = interp.eval_toplevel("r").expect("r");

    // The expected order is computed from `v`, not from `r`, so a view that
    // read the wrong slots could not agree with itself.
    let mut expected = values(&v);
    expected.reverse();
    assert_eq!(values(&r), expected, "reverse(v) is v backwards");
    assert_eq!(tensor_of(&r).shape, vec![5], "the shape is unchanged");
    assert_eq!(
        tensor_of(&r).get(&[0]).and_then(|value| value.as_f64()),
        Some(8.0),
        "r[0] is the last element of v"
    );
    assert_eq!(
        tensor_of(&r).get(&[-1]).and_then(|value| value.as_f64()),
        Some(1.5),
        "r[-1] is the first element of v"
    );

    // And it is the same value as its own materialised copy, bit for bit.
    assert_same(&r, &materialised(&r), "reverse(v)");
}

#[test]
fn a_reversed_slice_shares_the_buffer() {
    let mut interp = session(&format!("{VECTOR}let r = reverse(v)\n"));
    let v = interp.eval_toplevel("v").expect("v");
    let r = interp.eval_toplevel("r").expect("r");

    assert!(tensor_of(&r).is_view(), "a reversal is a view");
    let source = tensor_of(&v).buffer().expect("the source buffer");
    let window = tensor_of(&r).buffer().expect("the reversed buffer");
    assert!(
        Rc::ptr_eq(source, window),
        "the reversed slice must share the source's allocation, not copy it"
    );
    // A copying reversal would have a five-element buffer of its own; the
    // shared one is still the whole source.
    assert_eq!(source.len(), 5, "the buffer is still the source's");

    // The description the class claims: the last element, then a step of one
    // backwards. The stride stays a magnitude; the direction is the flag.
    assert_eq!(
        tensor_of(&r).offset(),
        4,
        "the first element is the last slot"
    );
    assert_eq!(
        tensor_of(&r).strides(),
        Some(&[1usize][..]),
        "stride magnitude"
    );
    assert_eq!(
        tensor_of(&r).reversed(),
        Some(&[true][..]),
        "the axis runs back"
    );

    // A reversed view is not contiguous even though its elements are adjacent:
    // they are in the wrong order, so it materialises rather than borrowing.
    assert!(
        !tensor_of(&r).is_contiguous(),
        "a reversed view is not contiguous"
    );
    assert!(
        tensor_of(&r).dense().is_none(),
        "a reversed view does not pretend to expose a window"
    );
    assert_eq!(
        tensor_of(&r)
            .dense_flat()
            .expect("materialised elements")
            .to_vec(),
        vec![8.0, 0.5, 3.0, -2.25, 1.5],
        "materialising walks the reversed axis in logical order"
    );
}

#[test]
fn a_double_reversal_returns_to_the_original_order() {
    let mut interp = session(&format!("{VECTOR}let rr = reverse(reverse(v))\n"));
    let v = interp.eval_toplevel("v").expect("v");
    let rr = interp.eval_toplevel("rr").expect("rr");

    assert_eq!(values(&rr), values(&v), "two reversals are the identity");
    assert_same(&v, &rr, "reverse(reverse(v))");
    // The offset walked to the end and back, and the axis runs forwards again,
    // so the window is exactly the source's.
    assert!(tensor_of(&rr).is_view(), "the result is still a view");
    assert!(
        Rc::ptr_eq(
            tensor_of(&v).buffer().expect("source"),
            tensor_of(&rr).buffer().expect("twice reversed")
        ),
        "a double reversal still shares the source buffer"
    );
    assert_eq!(tensor_of(&rr).offset(), 0, "the offset came back");
    assert_eq!(
        tensor_of(&rr).reversed(),
        Some(&[false][..]),
        "the axis runs forwards again"
    );
    assert!(
        tensor_of(&rr).is_contiguous(),
        "a double reversal of a vector is contiguous"
    );
}

#[test]
fn a_slice_of_a_reversed_view_stays_inside_the_buffer() {
    // Both ends are checked: `r[0..2]` starts at the last slot and walks down,
    // `r[3..5]` ends at slot 0 and must not step below it.
    let mut interp = session(&format!(
        "{VECTOR}let r = reverse(v)\nlet head = r[0..2]\nlet tail = r[3..5]\nlet one = r[4..5]\nlet all = r[0..5]\n"
    ));
    let v = interp.eval_toplevel("v").expect("v");
    let r = interp.eval_toplevel("r").expect("r");
    let head = interp.eval_toplevel("head").expect("head");
    let tail = interp.eval_toplevel("tail").expect("tail");
    let one = interp.eval_toplevel("one").expect("one");
    let all = interp.eval_toplevel("all").expect("all");

    for (name, value) in [
        ("head", &head),
        ("tail", &tail),
        ("one", &one),
        ("all", &all),
    ] {
        assert!(tensor_of(value).is_view(), "`{name}` is a view");
        assert!(
            Rc::ptr_eq(
                tensor_of(&v).buffer().expect("source"),
                tensor_of(value).buffer().expect(name)
            ),
            "`{name}` still shares the source buffer"
        );
    }

    assert_eq!(
        values(&head),
        vec![8.0, 0.5],
        "the first two of the reversal"
    );
    assert_eq!(tensor_of(&head).offset(), 4, "head starts at the last slot");
    assert_eq!(
        tensor_of(&head).reversed(),
        Some(&[true][..]),
        "a slice of a reversed axis stays reversed"
    );

    assert_eq!(
        values(&tail),
        vec![-2.25, 1.5],
        "the last two of the reversal are the first two of the source"
    );
    assert_eq!(tensor_of(&tail).offset(), 1, "tail starts at slot 1");
    assert_eq!(
        tensor_of(&tail).get(&[1]).and_then(|value| value.as_f64()),
        Some(1.5),
        "the last element of the window is slot 0, not slot -1"
    );

    assert_eq!(values(&one), vec![1.5], "the final element is slot 0");
    assert_eq!(tensor_of(&one).offset(), 0, "the window ends at slot 0");
    assert_eq!(values(&all), values(&r), "a full slice is the reversal");

    // A range that would leave the buffer is refused rather than clamped: the
    // runtime never computes a position outside the buffer.
    assert!(
        tensor_of(&r).slice_view(&[(0, 6)]).is_none(),
        "a range past the extent is not a view"
    );
}

#[test]
fn a_reversed_view_of_a_slice_stays_inside_the_buffer() {
    // The other order: slice first, then reverse. `s` starts at slot 1, so the
    // reversal starts at slot 3 and walks down to 1.
    let mut interp = session(&format!(
        "{VECTOR}let s = v[1..4]\nlet rs = reverse(s)\nlet rs2 = reverse(reverse(s))\n"
    ));
    let v = interp.eval_toplevel("v").expect("v");
    let s = interp.eval_toplevel("s").expect("s");
    let rs = interp.eval_toplevel("rs").expect("rs");
    let rs2 = interp.eval_toplevel("rs2").expect("rs2");

    assert!(tensor_of(&rs).is_view(), "a reversed slice is a view");
    assert!(
        Rc::ptr_eq(
            tensor_of(&v).buffer().expect("source"),
            tensor_of(&rs).buffer().expect("reversed slice")
        ),
        "a reversed slice of a view still shares the original buffer"
    );
    assert_eq!(tensor_of(&s).offset(), 1, "the slice starts at slot 1");
    assert_eq!(values(&s), vec![-2.25, 3.0, 0.5], "the slice's values");
    assert_eq!(values(&rs), vec![0.5, 3.0, -2.25], "reversed");
    assert_eq!(tensor_of(&rs).offset(), 3, "the reversal starts at slot 3");
    assert_eq!(
        tensor_of(&rs).reversed(),
        Some(&[true][..]),
        "the axis runs back"
    );
    assert_eq!(
        tensor_of(&rs).get(&[2]).and_then(|value| value.as_f64()),
        Some(-2.25),
        "the last element of the reversal is slot 1, not slot 0"
    );

    // Reversing a slice twice is the slice again, offset and direction alike.
    assert_eq!(values(&rs2), values(&s), "two reversals of a slice");
    assert_eq!(tensor_of(&rs2).offset(), 1, "the offset came back");
    assert_eq!(
        tensor_of(&rs2).reversed(),
        Some(&[false][..]),
        "the axis runs forwards again"
    );
}

#[test]
fn a_reversed_matrix_axis_flips_that_axis_only() {
    // `reverse` is rank-1 in the library (§8), so an axis flip is built through
    // the view API — the shape a convolution kernel or a fold needs. Column 1
    // of `m` runs downwards; the other axes are untouched.
    let mut interp = session(MATRIX);
    let m = interp.eval_toplevel("m").expect("m");
    let tensor = tensor_of(&m);
    let flipped = tensor.reversed_view(1).expect("a reversed view");

    assert!(flipped.is_view(), "the flip is a view");
    assert!(
        Rc::ptr_eq(
            tensor.buffer().expect("source"),
            flipped.buffer().expect("flip")
        ),
        "the flipped matrix shares the source buffer"
    );
    assert_eq!(flipped.shape, vec![3, 3], "the shape is unchanged");
    assert_eq!(flipped.offset(), 2, "the first element is column 2 of row 0");
    assert_eq!(flipped.strides(), Some(&[3usize, 1][..]), "strides");
    assert_eq!(
        flipped.reversed(),
        Some(&[false, true][..]),
        "only the second axis runs back"
    );
    assert!(!flipped.is_contiguous(), "a flipped axis is strided");

    // Row-major order of the flipped matrix, read through the view.
    let read: Vec<f64> = (0..9)
        .map(|index| {
            let (row, column) = (index / 3, index % 3);
            flipped
                .get(&[row as isize, column as isize])
                .and_then(|value| value.as_f64())
                .expect("an element")
        })
        .collect();
    assert_eq!(
        read,
        vec![3.0, 2.0, 1.0, 6.0, 5.0, 4.0, 9.0, 8.0, 7.0],
        "each row is read backwards, the rows themselves are not"
    );
    // The same matrix through its materialised copy: the class is not the
    // value.
    let flipped_value = Value::Tensor(Rc::new(flipped));
    assert_eq!(
        equal(&flipped_value, &materialised(&flipped_value)),
        Ok(true),
        "a flipped axis is the value its copy is"
    );

    // Slicing a flipped axis at either end stays inside the buffer. Logical
    // column 0 of the flip is buffer slot 2 of each row, so the *last* column
    // is the one that lands on slot 0 — the end that a non-negative bound
    // check would step past.
    let tensor = tensor_of(&m);
    let flipped = tensor.reversed_view(1).expect("a reversed view");
    let first = flipped.slice_view(&[(0, 3), (0, 1)]).expect("a column");
    assert_eq!(first.offset(), 2, "logical column 0 is buffer slot 2");
    assert_eq!(values(&Value::Tensor(Rc::new(first))), vec![3.0, 6.0, 9.0]);
    let last = flipped
        .slice_view(&[(0, 3), (2, 3)])
        .expect("a column");
    assert_eq!(last.offset(), 0, "logical column 2 is buffer slot 0");
    assert_eq!(values(&Value::Tensor(Rc::new(last))), vec![1.0, 4.0, 7.0]);
    assert!(
        flipped.slice_view(&[(0, 3), (0, 4)]).is_none(),
        "a column past the extent is not a view"
    );
}
