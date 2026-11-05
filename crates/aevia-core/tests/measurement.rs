//! §8.14 Measurement arithmetic (runtime propagation rules).
//!
//! The checker's operator schemes are frontend-owned and land separately, so
//! these tests drive the runtime operator entry point (`Interp::apply_binary`)
//! with the measurement record the `measure` builtin builds. The formulas are
//! the spec's independent-error rule ("平方和开根").

#![allow(clippy::result_large_err)]

use aevia_core::Interp;
use aevia_core::compile;
use aevia_core::value::Value;
use aevia_syntax::Span;

fn interpreter() -> Interp {
    let compilation = compile(
        "measure.aev",
        "fn main(): IO Unit = do\n    println(\"\")\n",
    );
    assert!(!compilation.diagnostics.has_errors());
    let mut interp = Interp::new(&compilation.checked).with_buffer();
    let diagnostics = interp.load(&compilation.module.expect("a module"));
    assert!(!diagnostics.has_errors());
    interp
}

fn measurement(
    interp: &mut Interp,
    value: f64,
    uncertainty: f64,
) -> Value {
    interp
        .call_named(
            "measure",
            vec![Value::float(value), Value::float(uncertainty)],
            Span::default(),
        )
        .expect("measure builds a record")
}

fn apply(
    interp: &mut Interp,
    op: &str,
    left: Value,
    right: Value,
) -> Value {
    interp
        .apply_binary(op, left, right, Span::default())
        .expect("the operator is defined for the measurements")
}

fn parts(value: &Value) -> (f64, f64) {
    let Value::Record(record) = value else {
        panic!("expected a measurement record, found {}", value.type_name());
    };
    let value = record.fields["value"].as_f64().expect("value");
    let uncertainty = record.fields["uncertainty"].as_f64().expect("uncertainty");
    (value, uncertainty)
}

fn close(
    actual: f64,
    expected: f64,
) -> bool {
    (actual - expected).abs() < 1e-12
}

#[test]
fn addition_and_subtraction_use_quadrature() {
    let mut interp = interpreter();
    let a = measurement(&mut interp, 1.0, 0.1);
    let b = measurement(&mut interp, 2.0, 0.2);
    let (value, uncertainty) = parts(&apply(&mut interp, "+", a.clone(), b.clone()));
    assert!(close(value, 3.0), "sum value {value}");
    assert!(
        close(uncertainty, 0.05f64.sqrt()),
        "sum uncertainty {uncertainty}"
    );

    let (value, uncertainty) = parts(&apply(&mut interp, "-", b, a));
    assert!(close(value, 1.0));
    assert!(close(uncertainty, 0.05f64.sqrt()));
}

#[test]
fn multiplication_and_division_propagate_independent_errors() {
    let mut interp = interpreter();
    let a = measurement(&mut interp, 1.0, 0.1);
    let b = measurement(&mut interp, 2.0, 0.2);
    // (u1*v2)² + (u2*v1)² = (0.1*2)² + (0.2*1)² = 0.04 + 0.04.
    let (value, uncertainty) = parts(&apply(&mut interp, "*", a.clone(), b.clone()));
    assert!(close(value, 2.0), "product value {value}");
    assert!(
        close(uncertainty, 0.08f64.sqrt()),
        "product uncertainty {uncertainty}"
    );

    // (u1/v2)² + (u2*v1/v2²)² = (0.1/2)² + (0.2*1/4)² = 0.0025 + 0.0025.
    let (value, uncertainty) = parts(&apply(&mut interp, "/", a, b));
    assert!(close(value, 0.5), "quotient value {value}");
    assert!(
        close(uncertainty, 0.005f64.sqrt()),
        "quotient uncertainty {uncertainty}"
    );
}

#[test]
fn a_scalar_scales_value_and_uncertainty() {
    let mut interp = interpreter();
    let a = measurement(&mut interp, 2.0, 0.3);
    let (value, uncertainty) = parts(&apply(&mut interp, "*", a.clone(), Value::float(-2.0)));
    assert!(close(value, -4.0));
    assert!(close(uncertainty, 0.6), "|scalar| scales the magnitude");

    let (value, uncertainty) = parts(&apply(&mut interp, "*", Value::float(2.0), a.clone()));
    assert!(close(value, 4.0));
    assert!(close(uncertainty, 0.6));

    let (value, uncertainty) = parts(&apply(&mut interp, "/", a, Value::float(4.0)));
    assert!(close(value, 0.5));
    assert!(close(uncertainty, 0.075));
}

#[test]
fn mismatched_dimensions_are_e0401() {
    let mut interp = interpreter();
    let length = interp
        .call_named(
            "measure",
            vec![
                Value::Num(aevia_core::value::NumVal::float(
                    1.0,
                    aevia_dim::Dim::base("Length"),
                )),
                Value::Num(aevia_core::value::NumVal::float(
                    0.1,
                    aevia_dim::Dim::base("Length"),
                )),
            ],
            Span::default(),
        )
        .expect("measure");
    let time = interp
        .call_named(
            "measure",
            vec![
                Value::Num(aevia_core::value::NumVal::float(
                    1.0,
                    aevia_dim::Dim::base("Time"),
                )),
                Value::Num(aevia_core::value::NumVal::float(
                    0.1,
                    aevia_dim::Dim::base("Time"),
                )),
            ],
            Span::default(),
        )
        .expect("measure");
    let error = interp
        .apply_binary("+", length, time, Span::default())
        .expect_err("length + time is E0401");
    assert_eq!(error.code, "E0401", "{}: {}", error.code, error.message);
}

#[test]
fn value_and_uncertainty_dispatch() {
    let mut interp = interpreter();
    let a = measurement(&mut interp, 7.5, 0.25);
    let value = interp
        .call_named("value", vec![a.clone()], Span::default())
        .expect("value");
    assert!(close(value.as_f64().expect("a number"), 7.5));
    let uncertainty = interp
        .call_named("uncertainty", vec![a], Span::default())
        .expect("uncertainty");
    assert!(close(uncertainty.as_f64().expect("a number"), 0.25));
}
