//! A slice step: `xs[a:b:s]` (§6.6.4).
//!
//! A step turns an axis into a *walk* rather than a window: the stride is
//! multiplied by the step and a negative step flips the axis's direction. Two
//! things have to be true for that to be a representation choice rather than a
//! second semantics:
//!
//! 1. **A negative step is a view, not a copy.** `Rc::ptr_eq` on the buffers
//!    must hold and the buffer must still be the source's length, so a copying
//!    implementation cannot pass.
//! 2. **The length is the walk's count.** A step that passes its stop
//!    immediately visits nothing — `xs[0:0:1]` and `xs[5:2:1]` are empty, not
//!    one element — and a zero step is refused rather than divided by.
//!
//! Assertions are on values, shapes, storage classes and error codes.

#![allow(clippy::result_large_err)]

use aevia_core::value::{equal, repr, show, NumKind, TensorData, Value};
use aevia_core::{compile, Interp};
use std::rc::Rc;

// --- harness ---------------------------------------------------------------

fn session(source: &str) -> Interp {
    let compilation = compile("steps.aev", source);
    assert!(
        !compilation.diagnostics.has_errors(),
        "compile errors: {:?}",
        compilation
            .diagnostics
            .items
            .iter()
            .map(|item| (&item.code, &item.message, item.span.start))
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

/// A tensor's elements as `f64`, in the tensor's own row-major order.
fn values(value: &Value) -> Vec<f64> {
    tensor_of(value)
        .elements()
        .iter()
        .map(|element| element.as_f64().expect("an f64 element"))
        .collect()
}

/// Strict equality: kind, shape, dimension and element *bits*, plus the two
/// renderings compared to each other rather than to a literal.
fn assert_same(left: &Value, right: &Value, context: &str) {
    match (left, right) {
        (Value::Tensor(a), Value::Tensor(b)) => {
            assert_eq!(a.shape, b.shape, "{context}: shape");
            assert_eq!(a.dim, b.dim, "{context}: element dimension");
            assert_eq!(a.storage.len(), b.storage.len(), "{context}: count");
            for index in 0..a.storage.len() {
                let x = a.get_flat(index).expect("left element");
                let y = b.get_flat(index).expect("right element");
                assert_same(&x, &y, &format!("{context}[{index}]"));
            }
            assert_eq!(show(left), show(right), "{context}: show");
            assert_eq!(repr(left), repr(right), "{context}: repr");
        }
        (Value::Num(a), Value::Num(b)) => {
            assert_eq!(a.kind.type_name(), b.kind.type_name(), "{context}: phase");
            if let (NumKind::Float(x), NumKind::Float(y)) = (&a.kind, &b.kind) {
                assert_eq!(x.to_bits(), y.to_bits(), "{context}: float bits");
            }
            assert_eq!(a.dim, b.dim, "{context}: dimension");
        }
        _ => {
            assert_eq!(
                equal(left, right),
                Ok(true),
                "{context}: values differ ({left:?} vs {right:?})"
            );
        }
    }
}

const VECTOR: &str = "let v = [1.5, -2.25, 3.0, 0.5, 8.0]\n";
const MATRIX: &str = "let m = [[1.0, 2.0, 3.0], [4.0, 5.0, 6.0], [7.0, 8.0, 9.0]]\n";

// --- 1. the values the walk visits -----------------------------------------

#[test]
fn a_stepped_slice_takes_every_nth_element() {
    let mut interp = session(&format!(
        "{VECTOR}let a = v[1:5:2]\nlet b = v[::2]\nlet c = v[4:0:-2]\n"
    ));
    let a = interp.eval_toplevel("a").expect("a");
    let b = interp.eval_toplevel("b").expect("b");
    let c = interp.eval_toplevel("c").expect("c");

    // v = [1.5, -2.25, 3.0, 0.5, 8.0]; indices 1 and 3.
    assert_eq!(values(&a), vec![-2.25, 0.5], "v[1:5:2]");
    assert_eq!(
        tensor_of(&a).shape,
        vec![2],
        "the length is the walk's count"
    );
    assert_eq!(values(&b), vec![1.5, 3.0, 8.0], "v[::2]");
    assert_eq!(values(&c), vec![8.0, 3.0], "v[4:0:-2] walks down by two");

    // A forward step is still a view of the same buffer, with the stride
    // multiplied by the step.
    assert!(tensor_of(&b).is_view(), "a stepped slice is a view");
    assert_eq!(tensor_of(&b).offset(), 0, "v[::2] starts at slot 0");
    assert_eq!(tensor_of(&b).strides(), Some(&[2usize][..]), "stride x 2");
    assert_eq!(
        tensor_of(&b).reversed(),
        Some(&[false][..]),
        "a forward step keeps the axis forward"
    );
}

#[test]
fn a_negative_step_is_the_reversal_reverse_returns() {
    let mut interp = session(&format!("{VECTOR}let r = v[::-1]\nlet b = reverse(v)\n"));
    let r = interp.eval_toplevel("r").expect("r");
    let b = interp.eval_toplevel("b").expect("b");

    // The same value, bit for bit: `xs[::-1]` is `reverse(xs)` written as a
    // slice.
    assert_same(&r, &b, "v[::-1] == reverse(v)");
    assert_eq!(
        tensor_of(&r).shape,
        tensor_of(&b).shape,
        "the shape is unchanged"
    );
    assert_eq!(
        tensor_of(&r).get(&[0]).and_then(|value| value.as_f64()),
        Some(8.0),
        "r[0] is the last element of v"
    );
}

// --- 2. a negative step is a view ------------------------------------------

#[test]
fn a_reversed_slice_is_a_view_of_the_source_buffer() {
    let mut interp = session(&format!("{VECTOR}let r = v[::-1]\n"));
    let v = interp.eval_toplevel("v").expect("v");
    let r = interp.eval_toplevel("r").expect("r");

    assert!(tensor_of(&r).is_view(), "a negative step is a view");
    let source = tensor_of(&v).buffer().expect("the source buffer");
    let window = tensor_of(&r).buffer().expect("the reversed buffer");
    assert!(
        Rc::ptr_eq(source, window),
        "a reversed slice must share the source's allocation, not copy it"
    );
    // A copying reversal would own a five-element buffer; the shared one is
    // still the whole source.
    assert_eq!(source.len(), 5, "the buffer is still the source's");

    // The description the class claims: the last slot, a stride of one, and
    // the axis running back.
    assert_eq!(
        tensor_of(&r).offset(),
        4,
        "the first element is the last slot"
    );
    assert_eq!(tensor_of(&r).strides(), Some(&[1usize][..]), "stride");
    assert_eq!(
        tensor_of(&r).reversed(),
        Some(&[true][..]),
        "the axis runs back"
    );
    assert!(
        !tensor_of(&r).is_contiguous(),
        "a reversed view is not contiguous"
    );
}

#[test]
fn a_negative_step_on_a_matrix_reverses_only_its_axis() {
    let mut interp = session(&format!("{MATRIX}let r = m[::, ::-1]\n"));
    let m = interp.eval_toplevel("m").expect("m");
    let r = interp.eval_toplevel("r").expect("r");

    assert!(tensor_of(&r).is_view(), "the step is a view");
    assert!(
        Rc::ptr_eq(
            tensor_of(&m).buffer().expect("source"),
            tensor_of(&r).buffer().expect("window")
        ),
        "the reversed axis shares the matrix's buffer"
    );
    assert_eq!(tensor_of(&r).shape, vec![3, 3], "the shape is unchanged");
    // Row-major order of `m` with each row read backwards: only axis 1 moved.
    assert_eq!(
        values(&r),
        vec![3.0, 2.0, 1.0, 6.0, 5.0, 4.0, 9.0, 8.0, 7.0],
        "the second axis runs back, the first does not"
    );
    assert_eq!(tensor_of(&r).offset(), 2, "the first element is column 2");
    assert_eq!(tensor_of(&r).strides(), Some(&[3usize, 1][..]), "strides");
    assert_eq!(
        tensor_of(&r).reversed(),
        Some(&[false, true][..]),
        "only the second axis is reversed"
    );
    assert_eq!(
        tensor_of(&r).get(&[1, 0]).and_then(|value| value.as_f64()),
        Some(6.0),
        "r[1, 0] is m[1, 2]"
    );
}

// --- 3. the length is the walk's count -------------------------------------

#[test]
fn a_slice_that_walks_past_its_end_is_empty() {
    // `xs[0:0:1]` visits nothing and `xs[5:2:1]` starts past its stop; neither
    // is one element, and neither panics. `xs[0:3:-1]` walks away from its
    // stop for the same reason.
    let mut interp = session(&format!(
        "{VECTOR}let a = v[0:0:1]\nlet b = v[5:2:1]\nlet c = v[2:0:1]\nlet d = v[0:3:-1]\n"
    ));
    for name in ["a", "b", "c", "d"] {
        let value = interp.eval_toplevel(name).expect(name);
        assert_eq!(
            tensor_of(&value).shape,
            vec![0],
            "`{name}` has no elements"
        );
        assert_eq!(tensor_of(&value).storage.len(), 0, "`{name}` is empty");
    }
}

#[test]
fn a_slice_without_a_step_keeps_the_behaviour_it_had() {
    // `xs[5:2]` is the same node as `xs[5..2]` — the step is absent, so the
    // two-component slice answers exactly as it always has, including the
    // `E0802` on an inverted range. The empty answer belongs to the walk, and
    // a walk is a slice that has a step.
    let compilation = compile("steps.aev", &format!("{VECTOR}let a = v[5:2]
"));
    assert!(
        !compilation.diagnostics.has_errors(),
        "the program type-checks"
    );
    let module = compilation.module.expect("module");
    let mut interp = Interp::new(&compilation.checked).with_buffer();
    let diagnostics = interp.load(&module);
    assert_eq!(
        diagnostics
            .items
            .iter()
            .map(|item| item.code.as_str())
            .collect::<Vec<_>>(),
        vec!["E0802"],
        "an inverted slice with no step is still the error it was"
    );
}

#[test]
fn the_length_is_the_exact_number_of_indices() {
    // ceil((b - a) / s) forwards and the mirror backwards, checked on a
    // ten-element axis where an off-by-one would show.
    let mut interp = session(
        "let x = [0.0, 1.0, 2.0, 3.0, 4.0, 5.0, 6.0, 7.0, 8.0, 9.0]\n\
         let a = x[1:9:3]\nlet b = x[1:10:3]\nlet c = x[9:0:-3]\nlet d = x[0:9:4]\n",
    );
    // 1, 4, 7 — 9 is the stop, not an index.
    assert_eq!(values(&interp.eval_toplevel("a").expect("a")), vec![1.0, 4.0, 7.0]);
    // 1, 4, 7 — the stop is past the end, so the count is still three.
    assert_eq!(values(&interp.eval_toplevel("b").expect("b")), vec![1.0, 4.0, 7.0]);
    // 9, 6, 3 — 0 is the stop, so it is not visited.
    assert_eq!(values(&interp.eval_toplevel("c").expect("c")), vec![9.0, 6.0, 3.0]);
    // 0, 4, 8 — the last step lands exactly on 8.
    assert_eq!(values(&interp.eval_toplevel("d").expect("d")), vec![0.0, 4.0, 8.0]);
}

// --- 4. a zero step is refused ---------------------------------------------

#[test]
fn a_zero_step_is_rejected_before_it_can_run() {
    let compilation = compile("steps.aev", &format!("{VECTOR}let a = v[::0]\n"));
    assert!(
        compilation
            .diagnostics
            .items
            .iter()
            .any(|item| item.code == "E0802" && item.message.contains("step of zero")),
        "a literal zero step is a static error: {:?}",
        compilation
            .diagnostics
            .items
            .iter()
            .map(|item| (&item.code, &item.message))
            .collect::<Vec<_>>()
    );
}

#[test]
fn a_computed_zero_step_is_a_runtime_error() {
    // `2 - 2` is not a literal, so the checker cannot see it; the walk refuses
    // it instead of dividing by zero or looping.
    let compilation = compile(
        "steps.aev",
        &format!("{VECTOR}let n = 2 - 2\nlet a = v[::n]\n"),
    );
    assert!(
        !compilation.diagnostics.has_errors(),
        "the program type-checks: {:?}",
        compilation
            .diagnostics
            .items
            .iter()
            .map(|item| (&item.code, &item.message))
            .collect::<Vec<_>>()
    );
    let module = compilation.module.expect("module");
    let mut interp = Interp::new(&compilation.checked).with_buffer();
    let diagnostics = interp.load(&module);
    assert_eq!(
        diagnostics
            .items
            .iter()
            .map(|item| item.code.as_str())
            .collect::<Vec<_>>(),
        vec!["E0802"],
        "a computed zero step is refused at run time"
    );
}

// --- 5. typing -------------------------------------------------------------

#[test]
fn a_non_integer_step_is_a_type_error() {
    let compilation = compile("steps.aev", &format!("{VECTOR}let a = v[::1.5]\n"));
    assert!(
        compilation
            .diagnostics
            .items
            .iter()
            .any(|item| item.code == "E0302" && item.message.contains("slice step")),
        "a float step has its own message: {:?}",
        compilation
            .diagnostics
            .items
            .iter()
            .map(|item| (&item.code, &item.message))
            .collect::<Vec<_>>()
    );
}

#[test]
fn a_step_keeps_the_rank_and_the_element_type() {
    // A slice keeps the operand's rank and element type; a step changes
    // neither, so these annotations must unify.
    let source = "fn every_other(v: Tensor<1, Float>): Tensor<1, Float> = v[::2]\n\
                  fn flip_columns(m: Tensor<2, Float>): Tensor<2, Float> = m[::, ::-1]\n";
    let compilation = compile("steps.aev", source);
    assert!(
        !compilation.diagnostics.has_errors(),
        "rank and element type survive the step: {:?}",
        compilation
            .diagnostics
            .items
            .iter()
            .map(|item| (&item.code, &item.message, item.span.start))
            .collect::<Vec<_>>()
    );
}
