//! `Dyn` at run time (§3.11, §4.9).
//!
//! The checker has no signature for these names yet (recorded in
//! `.workbuddy-ai/audit/dicts.md`), so the tests drive the operations the way
//! a checked call would: through the dispatcher, with values a program can
//! build. What they assert is the contract the type promises — the payload,
//! the dimension, and the error code when a dimension does not match.

#![allow(clippy::result_large_err)]

use aevia_core::EvalError;
use aevia_core::builtins::call_simple;
use aevia_core::value::NumKind;
use aevia_core::value::NumVal;
use aevia_core::value::Storage;
use aevia_core::value::TensorData;
use aevia_core::value::Value;
use aevia_dim::Dim;
use aevia_dim::Scale;
use aevia_dim::UnitDef;
use num_bigint::BigInt;
use num_rational::BigRational;
use std::rc::Rc;

fn length() -> Dim {
    Dim::base("Length")
}

fn metre() -> Value {
    Value::UnitOf(Rc::new(UnitDef {
        name: "m".to_string(),
        aliases: Vec::new(),
        dim: length(),
        scale: Scale::one(),
        offset: None,
        prefixable: true,
        affine: false,
        display: "m".to_string(),
        category: "length".to_string(),
    }))
}

fn scalar(value: f64) -> Value {
    Value::Num(NumVal::float(value, length()))
}

fn call(
    name: &str,
    args: Vec<Value>,
) -> Value {
    call_simple(name, args)
        .unwrap_or_else(|error| panic!("{name}: {} {}", error.code, error.message))
}

fn failure(
    name: &str,
    args: Vec<Value>,
) -> EvalError {
    call_simple(name, args).expect_err("expected a failure")
}

fn dynamic(value: Value) -> Rc<aevia_core::value::DynValue> {
    match value {
        | Value::Dyn(dynamic) => dynamic,
        | other => panic!("expected a Dyn, found {}", other.type_name()),
    }
}

fn payload(dynamic: &aevia_core::value::DynValue) -> f64 {
    dynamic.value.to_f64().expect("a number")
}

fn dimension_text(value: Value) -> String {
    let Value::Record(record) = value else {
        panic!("a DimValue is a record");
    };
    match record.fields.get("dimension") {
        | Some(Value::Str(text)) => text.to_string(),
        | other => panic!("expected a dimension, found {other:?}"),
    }
}

#[test]
fn dyn_carries_the_dimension_and_the_payload() {
    let dynamic = dynamic(call("dyn", vec![scalar(3.0)]));
    assert_eq!(dynamic.dim, length());
    assert_eq!(payload(&dynamic), 3.0);
    // The payload is dimensionless; the dimension travels beside it.
    assert!(dynamic.value.dim.is_one());
}

#[test]
fn dim_of_and_value_of_are_total_on_a_dyn() {
    for value in [
        scalar(3.0),
        Value::Num(NumVal::exact(
            BigRational::new(BigInt::from(3), BigInt::from(2)),
            length(),
        )),
        Value::Num(NumVal {
            kind: NumKind::Int(BigInt::from(7)),
            dim: length(),
            display: None,
        }),
    ] {
        let dynamic = call("dyn", vec![value]);
        assert_eq!(
            dimension_text(call("dim_of", vec![dynamic.clone()])),
            "Length"
        );
        assert!(matches!(call("value_of", vec![dynamic]), Value::Num(_)));
    }
    // A dimensionless value carries `1`, not a missing dimension.
    let dimensionless = call("dim_of", vec![call("dyn", vec![Value::int(1)])]);
    assert_eq!(dimension_text(dimensionless), "1");
}

#[test]
fn check_restores_the_scalar() {
    let restored = call("check", vec![call("dyn", vec![scalar(3.0)])]);
    let Value::Variant(result) = restored else {
        panic!("`check` answers with a Result");
    };
    assert_eq!(result.index, 0, "Ok");
    let Value::Num(num) = &result.fields[0] else {
        panic!("Ok carries a scalar");
    };
    assert_eq!(num.dim, length());
    assert_eq!(num.to_f64(), Some(3.0));
}

#[test]
fn check_with_matches_and_rejects() {
    let dynamic = call("dyn", vec![scalar(3.0)]);
    let dim = call("dim_of", vec![dynamic.clone()]);
    let matched = call("check_with", vec![dim.clone(), dynamic.clone()]);
    let Value::Variant(result) = matched else {
        panic!("`check_with` answers with a Result");
    };
    assert_eq!(result.index, 0, "a matching dimension is Ok");
    // The payload of `check_with` is the dimensionless scalar (§4.9.2).
    let Value::Num(num) = &result.fields[0] else {
        panic!("Ok carries a scalar");
    };
    assert!(num.dim.is_one());

    let other = dimension_text(call(
        "dim_of",
        vec![call(
            "dyn",
            vec![Value::Num(NumVal::float(1.0, Dim::base("Time")))],
        )],
    ));
    let mismatched = call("check_with", vec![record_dim(&other), dynamic]);
    let Value::Variant(result) = mismatched else {
        panic!("`check_with` answers with a Result");
    };
    assert_eq!(result.index, 1, "a different dimension is Err");
    let Value::Variant(error) = &result.fields[0] else {
        panic!("the payload is a DimError");
    };
    assert_eq!(error.name, "DimensionMismatch");
}

fn record_dim(text: &str) -> Value {
    let mut fields = indexmap::IndexMap::new();
    fields.insert("dimension".to_string(), Value::string(text));
    Value::Record(Rc::new(aevia_core::value::RecordValue { fields }))
}

#[test]
fn dyn_with_takes_the_dimension_it_is_given() {
    let dynamic = dynamic(call(
        "dyn_with",
        vec![
            Value::Num(NumVal::float(2.0, Dim::one())),
            record_dim("Length/Time"),
        ],
    ));
    assert_eq!(dynamic.dim.to_plain_string(), "Length/Time");
    assert_eq!(payload(&dynamic), 2.0);
}

#[test]
fn pow_dyn_multiplies_the_exponent() {
    let powered = dynamic(call(
        "pow_dyn",
        vec![call("dyn", vec![scalar(2.0)]), Value::int(2)],
    ));
    assert_eq!(
        powered.dim,
        length().pow(&BigRational::from_integer(BigInt::from(2)))
    );
    assert_eq!(payload(&powered), 4.0);
}

#[test]
fn det_unit_answers_with_a_dyn() {
    // A 2×2 matrix of lengths: its determinant is an area, and the order is
    // only known at run time, which is why the result is a `Dyn`.
    let matrix = Value::Tensor(Rc::new(TensorData {
        shape: vec![2, 2],
        storage: Storage::Dense(Rc::new(vec![2.0, 0.0, 0.0, 3.0])),
        dim: length(),
        display: None,
    }));
    let determinant = dynamic(call("det_unit", vec![matrix, metre()]));
    assert_eq!(payload(&determinant), 6.0);
    assert_eq!(
        determinant.dim,
        length().pow(&BigRational::from_integer(BigInt::from(2)))
    );
}

#[test]
fn show_names_the_quantity() {
    let rendered = call("show", vec![call("dyn", vec![scalar(3.0)])]);
    assert_eq!(
        match rendered {
            | Value::Str(text) => text.to_string(),
            | other => panic!("show answers with a String, found {}", other.type_name()),
        },
        "3 m"
    );
}

#[test]
fn a_non_dyn_argument_is_a_type_error() {
    assert_eq!(failure("dim_of", vec![Value::int(1)]).code, "E0302");
    assert_eq!(failure("value_of", vec![Value::int(1)]).code, "E0302");
    assert_eq!(
        failure("pow_dyn", vec![Value::int(1), Value::int(2)]).code,
        "E0302"
    );
    assert_eq!(failure("dyn", vec![Value::string("x")]).code, "E0302");
    assert_eq!(
        failure("dyn_with", vec![scalar(1.0), Value::int(1)]).code,
        "E0302"
    );
}

#[test]
fn a_symbolic_payload_keeps_its_symbol() {
    let symbol = Value::Num(NumVal::symbolic(
        aevia_core::sym::Term::Sym("x".to_string()),
        length(),
    ));
    let dynamic = dynamic(call("dyn", vec![symbol]));
    let Value::Num(num) = call("value_of", vec![Value::Dyn(dynamic)]) else {
        panic!("value_of answers with a scalar");
    };
    assert!(matches!(num.kind, NumKind::Sym(_)));
}
