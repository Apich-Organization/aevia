//! Dense axis reductions (`sum(a, axis)`, §8.5 keepdims).
//!
//! The dense `f64` fast path must be an optimisation, not a second semantics:
//! dense buffers, contiguous windows, strided views, rank-1/0 and empty extents
//! all agree with the element-wise path, and the compensated sum is the same
//! one `value::kahan_sum` performs.

#![allow(clippy::result_large_err)]

use aevia_core::value::{Storage, TensorData, Value};
use aevia_core::{compile, Interp, Mode};
use aevia_syntax::Span;

fn interpreter() -> Interp {
    let compilation = compile("axis.aev", "fn main(): IO Unit = do\n    println(\"\")\n");
    assert!(!compilation.diagnostics.has_errors());
    let mut interp = Interp::new(&compilation.checked).with_buffer();
    let diagnostics = interp.load(&compilation.module.expect("a module"));
    assert!(!diagnostics.has_errors());
    interp
}

/// Runs a source program on one evaluator, returning the rendered values.
fn evaluate(mode: Mode, source: &str, names: &[&str]) -> Vec<String> {
    let compilation = compile("axis.aev", source);
    assert!(
        !compilation.diagnostics.has_errors(),
        "compile errors: {:?}",
        compilation
            .diagnostics
            .items
            .iter()
            .map(|item| (&item.code, &item.message))
            .collect::<Vec<_>>()
    );
    let module = compilation.module.expect("a module");
    let mut interp = Interp::new(&compilation.checked)
        .with_buffer()
        .with_mode(mode);
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
    names
        .iter()
        .map(|name| {
            aevia_core::value::show(&interp.eval_toplevel(name).expect("the name is bound"))
        })
        .collect()
}

/// Both evaluators must agree; returns the IR rendering.
fn evaluate_both(source: &str, names: &[&str]) -> Vec<String> {
    let ir = evaluate(Mode::Ir, source, names);
    let ast = evaluate(Mode::Ast, source, names);
    assert_eq!(ast, ir, "the two evaluators disagree on {names:?}");
    ir
}

fn call(interp: &mut Interp, name: &str, args: Vec<Value>) -> Value {
    interp
        .call_named(name, args, Span::default())
        .unwrap_or_else(|error| panic!("`{name}` failed: {} {}", error.code, error.message))
}

fn tensor_of(value: &Value) -> &TensorData {
    match value {
        Value::Tensor(tensor) => tensor,
        other => panic!("expected a tensor, found {}", other.type_name()),
    }
}

fn floats(tensor: &TensorData) -> Vec<f64> {
    tensor
        .elements()
        .iter()
        .map(|element| element.as_f64().expect("a number"))
        .collect()
}

fn dense(shape: Vec<usize>, values: Vec<f64>) -> Value {
    Value::Tensor(std::rc::Rc::new(TensorData {
        shape,
        storage: Storage::Dense(std::rc::Rc::new(values)),
        dim: aevia_dim::Dim::one(),
        display: None,
    }))
}

#[test]
fn dense_rank_two_reduces_each_axis_with_keepdims() {
    let source = "let m = [[1.0, 2.0, 3.0], [4.0, 5.0, 6.0]]\n\
                  let rows = sum(m, 1)\n\
                  let columns = sum(m, 0)\n";
    let rendered = evaluate_both(source, &["rows", "columns"]);
    assert_eq!(rendered[0], "[[6], [15]]", "axis 1 keeps the rank");
    assert_eq!(rendered[1], "[[5, 7, 9]]", "axis 0 keeps the rank");

    let mut interp = interpreter();
    let compilation = compile("axis.aev", source);
    let mut session = Interp::new(&compilation.checked).with_buffer();
    let diagnostics = session.load(&compilation.module.expect("a module"));
    assert!(!diagnostics.has_errors());
    let m = session.eval_toplevel("m").expect("m");
    let rows = call(&mut interp, "sum", vec![m.clone(), Value::int(1)]);
    assert_eq!(tensor_of(&rows).shape, vec![2, 1], "keepdims keeps the rank");
    assert_eq!(floats(tensor_of(&rows)), vec![6.0, 15.0]);
    let columns = call(&mut interp, "sum", vec![m, Value::int(0)]);
    assert_eq!(tensor_of(&columns).shape, vec![1, 3]);
    assert_eq!(floats(tensor_of(&columns)), vec![5.0, 7.0, 9.0]);
}

#[test]
fn a_strided_view_reduces_like_its_dense_copy() {
    // `m[0..2, 1..3]` is [[2,3],[5,6]]; a column slice is not contiguous, so the
    // kernel materialises it through `dense_flat` and must still agree.
    let source = "let m = [[1.0, 2.0, 3.0], [4.0, 5.0, 6.0]]\n\
                  let v = m[0..2, 1..3]\n\
                  let dense = [[2.0, 3.0], [5.0, 6.0]]\n\
                  let view_sum = sum(v, 0)\n\
                  let dense_sum = sum(dense, 0)\n";
    let rendered = evaluate_both(source, &["view_sum", "dense_sum"]);
    assert_eq!(rendered[0], rendered[1], "a view reduces like its copy");
    assert_eq!(rendered[0], "[[7, 9]]");

    let compilation = compile("axis.aev", source);
    let mut session = Interp::new(&compilation.checked).with_buffer();
    let diagnostics = session.load(&compilation.module.expect("a module"));
    assert!(!diagnostics.has_errors());
    let view_sum = session.eval_toplevel("view_sum").expect("view_sum");
    assert_eq!(floats(tensor_of(&view_sum)), vec![7.0, 9.0]);

    // A transposed view has the same rule.
    let source = "let m = [[1.0, 2.0, 3.0], [4.0, 5.0, 6.0]]\n\
                  let t = transpose(m)\n\
                  let flat = [[1.0, 4.0], [2.0, 5.0], [3.0, 6.0]]\n\
                  let a = sum(t, 0)\n\
                  let b = sum(flat, 0)\n";
    let rendered = evaluate_both(source, &["a", "b"]);
    assert_eq!(rendered[0], rendered[1]);
    assert_eq!(rendered[0], "[[6, 15]]");
}

#[test]
fn rank_one_and_rank_zero_axes() {
    let source = "let v = [1.0, 2.0, 3.0]\nlet total = sum(v, 0)\n";
    let rendered = evaluate_both(source, &["total"]);
    assert_eq!(rendered[0], "[6]");

    let compilation = compile("axis.aev", source);
    let mut session = Interp::new(&compilation.checked).with_buffer();
    let diagnostics = session.load(&compilation.module.expect("a module"));
    assert!(!diagnostics.has_errors());
    let total = session.eval_toplevel("total").expect("total");
    assert_eq!(tensor_of(&total).shape, vec![1]);
    assert_eq!(floats(tensor_of(&total)), vec![6.0]);

    // Axis out of range is a runtime `E0802` (the checker types the axis as an
    // `Int`, so it cannot see the value), and a non-tensor operand keeps
    // `E0302`.
    let mut interp = interpreter();
    let vector = Value::Tensor(std::rc::Rc::new(TensorData {
        shape: vec![3],
        storage: Storage::Dense(std::rc::Rc::new(vec![1.0, 2.0, 3.0])),
        dim: aevia_dim::Dim::one(),
        display: None,
    }));
    let error = interp
        .call_named(
            "sum",
            vec![vector, Value::int(1)],
            Span::default(),
        )
        .expect_err("axis 1 on a rank-1 tensor is E0802");
    assert_eq!(error.code, "E0802");
    let error = interp
        .call_named("sum", vec![Value::float(1.0), Value::int(0)], Span::default())
        .expect_err("a scalar is not a tensor");
    assert_eq!(error.code, "E0302");

    let mut interp = interpreter();
    let error = interp
        .call_named(
            "sum",
            vec![Value::Tensor(std::rc::Rc::new(TensorData {
                shape: Vec::new(),
                storage: Storage::Dense(std::rc::Rc::new(vec![1.0])),
                dim: aevia_dim::Dim::one(),
                display: None,
            })), Value::int(0)],
            Span::default(),
        )
        .expect_err("rank 0 has no axis 0");
    assert_eq!(error.code, "E0802");
}

#[test]
fn empty_extents_take_the_element_phase() {
    // A dense empty axis answers Float zeros (§6.6.6). `zeros(0, 3)` is a
    // 0×3 tensor, so axis 0 reduces to one row of three zeros.
    let source = "let e = zeros(0, 3)\nlet s = sum(e, 0)\n";
    let rendered = evaluate_both(source, &["s"]);
    assert_eq!(rendered[0], "[[0, 0, 0]]");

    let compilation = compile("axis.aev", source);
    let mut session = Interp::new(&compilation.checked).with_buffer();
    let diagnostics = session.load(&compilation.module.expect("a module"));
    assert!(!diagnostics.has_errors());
    let s = session.eval_toplevel("s").expect("s");
    assert_eq!(tensor_of(&s).shape, vec![1, 3]);
    assert_eq!(floats(tensor_of(&s)), vec![0.0, 0.0, 0.0]);

    // An exact empty axis answers the exact `0`, not a Float one.
    let mut interp = interpreter();
    let exact_empty = Value::Tensor(std::rc::Rc::new(TensorData {
        shape: vec![0],
        storage: Storage::Exact(Vec::new()),
        dim: aevia_dim::Dim::one(),
        display: None,
    }));
    let sum = call(&mut interp, "sum", vec![exact_empty, Value::int(0)]);
    let tensor = tensor_of(&sum);
    assert_eq!(tensor.shape, vec![1]);
    match tensor.get(&[0]) {
        Some(Value::Num(num)) => assert!(matches!(
            num.kind,
            aevia_core::value::NumKind::Exact(_) | aevia_core::value::NumKind::Int(_)
        )),
        other => panic!("expected an exact element, found {other:?}"),
    }
}

#[test]
fn dimensions_carry_through_the_reduction() {
    let source = "let d = [1.0 m, 2.0 m]\nlet s = sum(d, 0)\n";
    let rendered = evaluate_both(source, &["s"]);
    assert!(
        rendered[0].contains("m"),
        "the reduced element keeps its dimension, found {:?}",
        rendered[0]
    );
}

#[test]
fn dense_and_boxed_float_classes_agree_bit_for_bit() {
    // 0.1 is not exactly representable: plain sequential addition and a
    // compensated sum differ, so this pins that the classes use the same fold.
    let values = vec![0.1, 0.2, 0.3, 0.4, 0.1, 0.2, 0.3, 0.4];
    let dense_tensor = dense(vec![2, 4], values.clone());
    let boxed_tensor = Value::Tensor(std::rc::Rc::new(TensorData {
        shape: vec![2, 4],
        storage: Storage::Boxed(values.into_iter().map(Value::float).collect()),
        dim: aevia_dim::Dim::one(),
        display: None,
    }));
    let mut interp = interpreter();
    for axis in [0, 1] {
        let dense_sum = call(
            &mut interp,
            "sum",
            vec![dense_tensor.clone(), Value::int(axis)],
        );
        let boxed_sum = call(
            &mut interp,
            "sum",
            vec![boxed_tensor.clone(), Value::int(axis)],
        );
        assert_eq!(
            floats(tensor_of(&dense_sum)),
            floats(tensor_of(&boxed_sum)),
            "axis {axis}: dense and boxed Float agree bit-for-bit"
        );
    }
}

#[test]
fn the_dense_path_preserves_compensation() {
    // The whole-tensor sum uses the compensated fold; the axis path must too.
    // Sequential addition of 1e16 + 1 - 1e16 gives 0; Neumaier keeps the 1.
    let mut interp = interpreter();
    let values = vec![1e16, 1.0, -1e16];
    let tensor = dense(vec![3], values);
    let reduced = call(&mut interp, "sum", vec![tensor, Value::int(0)]);
    assert_eq!(
        floats(tensor_of(&reduced)),
        vec![1.0],
        "the compensated sum keeps the small term"
    );
}

#[test]
fn the_dense_axis_path_does_not_box_per_element() {
    // Structural check used by the allocation gate: a 512×512 dense axis-0 sum
    // produces 512 outputs; per-element allocation would show up as > 262 144
    // allocations, while the fast path is O(outputs).
    let values = vec![0.5f64; 512 * 512];
    let mut interp = interpreter();
    let tensor = dense(vec![512, 512], values);
    let reduced = call(&mut interp, "sum", vec![tensor, Value::int(0)]);
    assert_eq!(tensor_of(&reduced).shape, vec![1, 512]);

    // Timing note: the dense fast path vs the same values in `Boxed` storage
    // (the element-wise path, which this fast path replaces for `f64`).
    let flat = vec![0.5f64; 512 * 512];
    let dense_tensor = dense(vec![512, 512], flat.clone());
    let boxed_tensor = Value::Tensor(std::rc::Rc::new(TensorData {
        shape: vec![512, 512],
        storage: Storage::Boxed(flat.into_iter().map(Value::float).collect()),
        dim: aevia_dim::Dim::one(),
        display: None,
    }));
    for (label, tensor) in [("dense", dense_tensor), ("boxed", boxed_tensor)] {
        let start = std::time::Instant::now();
        for _ in 0..3 {
            let _ = call(&mut interp, "sum", vec![tensor.clone(), Value::int(0)]);
        }
        let elapsed = start.elapsed();
        eprintln!(
            "timing: {label} sum(axis=0) 512×512 ≈ {:.2} ms per call (debug build)",
            elapsed.as_secs_f64() * 1e3 / 3.0
        );
    }
}
