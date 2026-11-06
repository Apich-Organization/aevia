//! `@`, `det`, `inv` and `linsolve` after the move onto `aevia_std::linalg`.
//!
//! The `f64` branches of those four call sites now run on the `faer`-backed
//! dispatcher; the exact branches (`exact_matrix` / `invert_exact` / the
//! rational `linsolve`) do not. This file is the guard on both halves: the
//! numbers each operation answers are the ones the existing tests document, and
//! an exact operand still produces an exact result.

#![allow(clippy::result_large_err)]

use aevia_core::Interp;
use aevia_core::builtins::call_simple;
use aevia_core::compile;
use aevia_core::value::NumKind;
use aevia_core::value::NumVal;
use aevia_core::value::Storage;
use aevia_core::value::TensorData;
use aevia_core::value::Value;
use aevia_core::value::repr;

fn call(
    name: &str,
    args: Vec<Value>,
) -> Value {
    call_simple(name, args).unwrap_or_else(|error| panic!("`{name}` failed: {error:?}"))
}

fn session(source: &str) -> Interp {
    let compilation = compile("linalg.aev", source);
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

fn value(
    source: &str,
    name: &str,
) -> Value {
    let mut interp = session(source);
    interp.eval_toplevel(name).expect("value exists")
}

fn repr_of(
    source: &str,
    name: &str,
) -> String {
    repr(&value(source, name))
}

fn tensor(
    source: &str,
    name: &str,
) -> TensorData {
    match value(source, name) {
        | Value::Tensor(tensor) => (*tensor).clone(),
        | other => panic!("expected a tensor, found {}", other.type_name()),
    }
}

/// The tensor inside `Ok(...)`.
fn ok_tensor(value: Value) -> TensorData {
    let Value::Variant(result) = value else {
        panic!("expected a Result");
    };
    assert_eq!(result.name, "Ok", "expected Ok");
    let Some(Value::Tensor(tensor)) = result.fields.first() else {
        panic!("expected an Ok tensor");
    };
    (**tensor).clone()
}

/// The storage phase of a scalar: `Int`, `Exact`, `Float` or the others.
fn phase(num: &NumVal) -> &'static str {
    match num.kind {
        | NumKind::Int(_) => "Int",
        | NumKind::Exact(_) => "Exact",
        | NumKind::Float(_) => "Float",
        | NumKind::Sym(_) => "Sym",
        | NumKind::Complex(..) => "Complex",
    }
}

/// The element phases of a tensor, in flat order.
fn phases(tensor: &TensorData) -> Vec<&'static str> {
    (0..tensor.storage.len())
        .map(|index| {
            let element = tensor.get_flat(index).expect("element");
            let Value::Num(num) = element else {
                panic!("expected a scalar");
            };
            phase(&num)
        })
        .collect()
}

/// The floats of an `f64` tensor, in flat order.
fn floats(tensor: &TensorData) -> Vec<f64> {
    (0..tensor.storage.len())
        .map(|index| {
            let element = tensor.get_flat(index).expect("element");
            let Value::Num(num) = element else {
                panic!("expected a scalar");
            };
            match num.kind {
                | NumKind::Float(value) => value,
                | other => panic!("expected a float, found {}", other.type_name()),
            }
        })
        .collect()
}

/// The `f64` a scalar holds.
fn floats_of(num: &NumVal) -> f64 {
    match num.kind {
        | NumKind::Float(value) => value,
        | ref other => panic!("expected a float, found {}", other.type_name()),
    }
}

#[test]
fn the_documented_answers_still_hold() {
    // `@` — `crates/aevia-std/tests/exactness.rs` documents `19, 22, 43, 50`.
    let source = "let p = [[1, 2], [3, 4]] @ [[5, 6], [7, 8]]\n";
    assert_eq!(repr_of(source, "p"), "[[19, 22], [43, 50]]");

    // `det` — `tests/run.rs` documents `-2`, exactly `0` for a singular matrix
    // (not `-0`), and `1/3`.
    let source = "let a = det([[1, 2], [3, 4]])\n\
                  let b = det([[1, 2], [2, 4]])\n\
                  let c = det([[1/3, 0], [0, 1]])\n";
    assert_eq!(repr_of(source, "a"), "-2");
    assert_eq!(repr_of(source, "b"), "0");
    assert_eq!(repr_of(source, "c"), "1/3");

    // `inv` — `tests/run.rs` documents `Ok([[-2, 1], [3/2, -1/2]])`.
    let source = "let i = inv([[1, 2], [3, 4]])\n";
    assert_eq!(repr_of(source, "i"), "Ok([[-2, 1], [3/2, -1/2]])");

    // `linsolve` — `tests/run.rs` documents `Ok([4/5, 7/5])`.
    let source = "let s = linsolve([[2, 1], [1, 3]], [3, 5])\n";
    assert_eq!(repr_of(source, "s"), "Ok([4/5, 7/5])");
}

#[test]
fn the_float_matmul_answers_are_unchanged() {
    // `@` on `Float` operands is the one call site the dispatcher reaches at
    // every size: the dense path is chosen by storage class, not by shape.
    // Every partial sum below is exactly representable, so the answer is the
    // one the loop gave, bit for bit.
    let source = "let p = [[1.0, 2.0], [3.0, 4.0]] @ [[5.0, 6.0], [7.0, 8.0]]\n";
    assert_eq!(floats(&tensor(source, "p")), vec![19.0, 22.0, 43.0, 50.0]);
}

/// A diagonal matrix as a dense `Value`, `size × size`.
fn diagonal(
    size: usize,
    values: &[(usize, f64)],
) -> Value {
    let mut data = vec![2.0; size * size];
    for (index, value) in values {
        data[index * size + index] = *value;
    }
    for row in 0..size {
        for column in 0..size {
            if row != column {
                data[row * size + column] = 0.0;
            }
        }
    }
    Value::Tensor(std::rc::Rc::new(TensorData::new(
        vec![size, size],
        Storage::Dense(std::rc::Rc::new(data)),
        aevia_dim::Dim::one(),
        None,
    )))
}

/// `det`, `inv` and `linsolve` on the `f64` branch.
///
/// That branch is only reached above 64×64: `exact_matrix` converts a `Float`
/// to a `BigRational` (`NumVal::to_rational`) and takes the exact path for
/// every matrix up to its 64-element bound. So the numbers below are the ones
/// the hand-written elimination answered on the same input, with every value a
/// power of two and therefore exact.
#[test]
fn the_float_factorisations_agree_on_a_large_matrix() {
    let size = 65;
    // 2^64 on the diagonal, halved once: the determinant is exactly 2^63.
    let a = diagonal(size, &[(size - 1, 0.5)]);
    let b = Value::Tensor(std::rc::Rc::new(TensorData::new(
        vec![size],
        Storage::Dense(std::rc::Rc::new(vec![1.0; size])),
        aevia_dim::Dim::one(),
        None,
    )));

    let determinant = call("det", vec![a.clone()]);
    let Value::Num(determinant) = determinant else {
        panic!("expected a scalar");
    };
    assert_eq!(phase(&determinant), "Float");
    assert_eq!(floats_of(&determinant), 2f64.powi(63));

    let inverse = ok_tensor(call("inv", vec![a.clone()]));
    let values = floats(&inverse);
    assert_eq!(values[size * size - 1], 2.0);
    assert_eq!(values[0], 0.5);

    let solution = ok_tensor(call("linsolve", vec![a, b]));
    let values = floats(&solution);
    assert_eq!(values[size - 1], 2.0);
    assert_eq!(values[0], 0.5);
}

/// A singular matrix on the `f64` branch still reports `Singular`.
#[test]
fn a_large_singular_matrix_still_reports_singular() {
    let a = diagonal(65, &[(0, 0.0)]);
    let b = Value::Tensor(std::rc::Rc::new(TensorData::new(
        vec![65],
        Storage::Dense(std::rc::Rc::new(vec![1.0; 65])),
        aevia_dim::Dim::one(),
        None,
    )));
    for (name, args) in [("inv", vec![a.clone()]), ("linsolve", vec![a, b])] {
        let Value::Variant(result) = call(name, args) else {
            panic!("{name}: expected a Result");
        };
        assert_eq!(result.name, "Err", "{name}");
        assert!(
            matches!(&result.fields[0], Value::Variant(error) if error.name == "Singular"),
            "{name}: expected Singular"
        );
    }
}

#[test]
fn an_exact_operand_still_produces_an_exact_result() {
    // Not one element of these is a `Float`: the exact branch is still in
    // front of the dispatcher, and the dispatcher is `f64`-only so it could
    // not answer them if it were not.
    let source = "let p = [[1, 2], [3, 4]] @ [[5, 6], [7, 8]]\n\
                  let d = det([[1, 2], [3, 4]])\n\
                  let i = inv([[1, 2], [3, 4]])\n";
    let product = tensor(source, "p");
    // Integer matrix multiplication stays exact and retains the Int element
    // representation promised by its checked type.
    assert_eq!(phases(&product), vec!["Int"; 4]);
    assert_eq!(
        (0..4)
            .map(|index| repr(&product.get_flat(index).expect("element")))
            .collect::<Vec<_>>(),
        vec!["19", "22", "43", "50"]
    );

    let determinant = value(source, "d");
    let Value::Num(determinant) = determinant else {
        panic!("expected a scalar");
    };
    assert_eq!(phase(&determinant), "Int");
    assert_eq!(repr(&Value::Num(determinant)), "-2");

    // A rational inverse: `-2, 1, 3/2, -1/2`. `build_tensor` stores the four
    // rationals as one exact tensor, so every element reads back `Exact`.
    let inverse = ok_tensor(value(source, "i"));
    assert_eq!(phases(&inverse), vec!["Exact"; 4]);
    assert_eq!(
        (0..4)
            .map(|index| repr(&inverse.get_flat(index).expect("element")))
            .collect::<Vec<_>>(),
        vec!["-2", "1", "3/2", "-1/2"]
    );
}
