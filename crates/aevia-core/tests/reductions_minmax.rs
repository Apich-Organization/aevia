//! Whole-tensor `min_all`/`max_all` (§8.1) — dense fast path.
//!
//! The spec declares `min`/`max` only as *binary scalar* functions and the
//! tensor reductions only as whole-tensor `min_all`/`max_all`: there is no axis
//! form and no `argmin`/`argmax`, so none is invented here.

#![allow(clippy::result_large_err)]

use aevia_core::value::{Storage, TensorData, Value};
use aevia_core::{compile, Interp, Mode};
use aevia_syntax::Span;

fn interpreter() -> Interp {
    let compilation = compile("minmax.aev", "fn main(): IO Unit = do\n    println(\"\")\n");
    assert!(!compilation.diagnostics.has_errors());
    let mut interp = Interp::new(&compilation.checked).with_buffer();
    let diagnostics = interp.load(&compilation.module.expect("a module"));
    assert!(!diagnostics.has_errors());
    interp
}

fn evaluate(mode: Mode, source: &str, names: &[&str]) -> Vec<String> {
    let compilation = compile("minmax.aev", source);
    assert!(
        !compilation.diagnostics.has_errors(),
        "compile errors: {:?}",
        compilation
            .diagnostics
            .items
            .iter()
            .filter(|item| item.severity == aevia_syntax::Severity::Error)
            .map(|item| (&item.code, &item.message))
            .collect::<Vec<_>>()
    );
    let module = compilation.module.expect("a module");
    let mut interp = Interp::new(&compilation.checked)
        .with_buffer()
        .with_mode(mode);
    let diagnostics = interp.load(&module);
    assert!(!diagnostics.has_errors());
    names
        .iter()
        .map(|name| {
            aevia_core::value::show(&interp.eval_toplevel(name).expect("the name is bound"))
        })
        .collect()
}

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

fn dense(shape: Vec<usize>, values: Vec<f64>) -> Value {
    Value::Tensor(std::rc::Rc::new(TensorData {
        shape,
        storage: Storage::Dense(std::rc::Rc::new(values)),
        dim: aevia_dim::Dim::one(),
        display: None,
    }))
}

fn number(value: &Value) -> f64 {
    value.as_f64().expect("a number")
}

#[test]
fn dense_min_max_answer_the_whole_tensor_scalar() {
    let source = "let v = [3.0, 1.0, 2.0]\n\
                  let lo = min_all(v)\n\
                  let hi = max_all(v)\n";
    let rendered = evaluate_both(source, &["lo", "hi"]);
    assert_eq!(rendered[0], "1");
    assert_eq!(rendered[1], "3");

    let mut interp = interpreter();
    let tensor = dense(vec![3], vec![3.0, 1.0, 2.0]);
    assert_eq!(number(&call(&mut interp, "min_all", vec![tensor.clone()])), 1.0);
    assert_eq!(number(&call(&mut interp, "max_all", vec![tensor])), 3.0);
    // The result is a Scalar, not a rank-0 tensor.
    assert!(!matches!(
        call(&mut interp, "min_all", vec![dense(vec![1], vec![1.0])]),
        Value::Tensor(_)
    ));
}

#[test]
fn exact_tensors_keep_exact_ordering() {
    let exact = Value::Tensor(std::rc::Rc::new(TensorData {
        shape: vec![3],
        storage: Storage::Exact(
            [-3, 7, 2]
                .into_iter()
                .map(|value| num_rational::BigRational::from_integer(value.into()))
                .collect(),
        ),
        dim: aevia_dim::Dim::one(),
        display: None,
    }));
    let mut interp = interpreter();
    let lo = call(&mut interp, "min_all", vec![exact.clone()]);
    let hi = call(&mut interp, "max_all", vec![exact]);
    assert!(matches!(
        lo,
        Value::Num(aevia_core::value::NumVal {
            kind: aevia_core::value::NumKind::Exact(_),
            ..
        })
    ));
    assert_eq!(number(&lo), -3.0);
    assert_eq!(number(&hi), 7.0);

    // Dimensions carry through: the winning element keeps its unit.
    let source = "let v = [1.0 m, 0.5 m, 2.0 m]\nlet lo = min_all(v)\n";
    let rendered = evaluate_both(source, &["lo"]);
    assert!(
        rendered[0].contains('m'),
        "the minimum keeps its dimension, found {:?}",
        rendered[0]
    );
}

#[test]
fn nan_never_displaces_the_current_best() {
    // §6.6.6/§3.8: Float comparisons with NaN are false, so the element-wise
    // rule is "a NaN never wins; a leading NaN is sticky". The dense path must
    // reproduce it exactly.
    let mut interp = interpreter();
    for (values, expected_min, expected_max) in [
        (vec![f64::NAN, 1.0, 2.0], f64::NAN, f64::NAN),
        (vec![1.0, f64::NAN, 2.0], 1.0, 2.0),
        (vec![1.0, 2.0, f64::NAN], 1.0, 2.0),
        (vec![f64::NAN, f64::NAN], f64::NAN, f64::NAN),
    ] {
        let tensor = dense(vec![values.len()], values);
        let lo = number(&call(&mut interp, "min_all", vec![tensor.clone()]));
        let hi = number(&call(&mut interp, "max_all", vec![tensor]));
        if expected_min.is_nan() {
            assert!(lo.is_nan(), "min should be NaN, found {lo}");
        } else {
            assert_eq!(lo, expected_min);
        }
        if expected_max.is_nan() {
            assert!(hi.is_nan(), "max should be NaN, found {hi}");
        } else {
            assert_eq!(hi, expected_max);
        }
    }
}

#[test]
fn dense_and_boxed_float_classes_agree() {
    let values = vec![0.5, -1.25, 3.75, 0.0, -0.5, 2.0];
    let dense_tensor = dense(vec![2, 3], values.clone());
    let boxed = Value::Tensor(std::rc::Rc::new(TensorData {
        shape: vec![2, 3],
        storage: Storage::Boxed(values.into_iter().map(Value::float).collect()),
        dim: aevia_dim::Dim::one(),
        display: None,
    }));
    let mut interp = interpreter();
    for name in ["min_all", "max_all"] {
        let from_dense = call(&mut interp, name, vec![dense_tensor.clone()]);
        let from_boxed = call(&mut interp, name, vec![boxed.clone()]);
        assert_eq!(
            number(&from_dense).to_bits(),
            number(&from_boxed).to_bits(),
            "{name}: dense and boxed agree bit-for-bit"
        );
    }
}

#[test]
fn a_strided_view_answers_like_its_dense_copy() {
    // A reversed slice is a view: `dense_flat` materialises it once at the
    // kernel boundary and the reduction must still see the logical values.
    let source = "let v = [3.0, 1.0, 2.0]\n\
                  let r = reverse(v)\n\
                  let lo = min_all(r)\n\
                  let hi = max_all(r)\n";
    let rendered = evaluate_both(source, &["lo", "hi"]);
    assert_eq!(rendered[0], "1");
    assert_eq!(rendered[1], "3");

    // A column-major (strided) 3×3 view over a row-major buffer: the logical
    // matrix is [[0,3,6],[1,4,7],[2,5,8]], so min is 0 and max is 8.
    let mut interp = interpreter();
    let view = Value::Tensor(std::rc::Rc::new(TensorData {
        shape: vec![3, 3],
        storage: Storage::View(aevia_core::value::View {
            data: std::rc::Rc::new((0..9).map(f64::from).collect()),
            offset: 0,
            strides: vec![1, 3],
            reversed: vec![false, false],
            len: 9,
        }),
        dim: aevia_dim::Dim::one(),
        display: None,
    }));
    assert_eq!(number(&call(&mut interp, "min_all", vec![view.clone()])), 0.0);
    assert_eq!(number(&call(&mut interp, "max_all", vec![view])), 8.0);

    // The whole-tensor runtime accepts a rank-2 tensor even though the checker
    // scheme is rank-1 today (reported as a scheme gap to frontend): a dense
    // rank-2 reduction still answers the scalar.
    let matrix = dense(vec![3, 3], (0..9).map(f64::from).collect());
    assert_eq!(number(&call(&mut interp, "min_all", vec![matrix])), 0.0);
}

#[test]
fn empty_tensors_are_e0802_and_single_elements_keep_their_identity() {
    let mut interp = interpreter();
    for name in ["min_all", "max_all"] {
        let empty_dense = dense(vec![0], Vec::new());
        let error = interp
            .call_named(name, vec![empty_dense], Span::default())
            .expect_err("an empty reduction is E0802");
        assert_eq!(error.code, "E0802");
        let empty_dense = dense(vec![0], Vec::new());
        let empty_boxed = Value::Tensor(std::rc::Rc::new(TensorData {
            shape: vec![0],
            storage: Storage::Boxed(Vec::new()),
            dim: aevia_dim::Dim::one(),
            display: None,
        }));
        let error = interp
            .call_named(name, vec![empty_boxed], Span::default())
            .expect_err("an empty boxed reduction is E0802");
        assert_eq!(error.code, "E0802");
        let _ = empty_dense;
    }

    // A one-element reduction is that element, display unit included.
    let source = "let v = [2.5 m]\nlet lo = min_all(v)\n";
    let rendered = evaluate_both(source, &["lo"]);
    assert_eq!(rendered[0], "2.5 m");
}

#[test]
fn non_tensor_operands_keep_e0302() {
    let mut interp = interpreter();
    for name in ["min_all", "max_all"] {
        let error = interp
            .call_named(name, vec![Value::float(1.0)], Span::default())
            .expect_err("a scalar is not a tensor");
        assert_eq!(error.code, "E0302");
    }
}

#[test]
fn the_dense_scan_is_the_fast_path() {
    // Timing note (debug build): the dense fast path vs the element-wise path a
    // non-`f64` class takes on the same 512×512 data. The allocation gate lives
    // in `tests/allocations.rs`; this is the wall-clock observation only.
    let values = vec![0.5f64; 512 * 512];
    let mut interp = interpreter();
    let dense_tensor = dense(vec![512, 512], values.clone());
    let boxed_tensor = Value::Tensor(std::rc::Rc::new(TensorData {
        shape: vec![512, 512],
        storage: Storage::Boxed(values.into_iter().map(Value::float).collect()),
        dim: aevia_dim::Dim::one(),
        display: None,
    }));
    for (label, tensor) in [("dense", dense_tensor), ("boxed", boxed_tensor)] {
        let start = std::time::Instant::now();
        for _ in 0..5 {
            let _ = call(&mut interp, "min_all", vec![tensor.clone()]);
        }
        let elapsed = start.elapsed();
        eprintln!(
            "timing: {label} min_all 512×512 ≈ {:.3} ms per call (debug build)",
            elapsed.as_secs_f64() * 1e3 / 5.0
        );
    }
}
