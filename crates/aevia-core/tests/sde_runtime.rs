//! SDE runtime integration (§8.9.1, new in this round).
//!
//! The language-level schemes come from `aevia_std::sde`; this crate owns the
//! dispatch: flatten/rebuild the state with the same `StateShape` contract as
//! the ODE path, adapt the Aevia closures, map the option record, and answer
//! `Result<SdePath, SdeError>`.
//!
//! The checker schemes are frontend-owned (`DiffusionOf` is being added), so
//! these tests drive the builtin entry points exactly as a checked call would.

#![allow(clippy::result_large_err)]

use aevia_core::builtins::call_simple;
use aevia_core::value::NativeFn;
use aevia_core::value::RecordValue;
use aevia_core::value::Value;
use aevia_core::value::make_variant;
use aevia_syntax::Span;
use aevia_types::adt;
use indexmap::IndexMap;
use std::rc::Rc;

// ---------------------------------------------------------------------------
// harness
// ---------------------------------------------------------------------------

fn record(fields: Vec<(&str, Value)>) -> Value {
    let mut map = IndexMap::new();
    for (name, value) in fields {
        map.insert(name.to_string(), value);
    }
    Value::Record(Rc::new(RecordValue { fields: map }))
}

fn variant(
    type_name: &str,
    index: usize,
    name: &str,
    fields: Vec<Value>,
) -> Value {
    let id = adt::intern(adt::Adt {
        name: type_name.to_string(),
        params: Vec::new(),
        ctors: Vec::new(),
    });
    make_variant(id, index, name, fields)
}

fn method(
    index: usize,
    name: &str,
) -> Value {
    variant("SdeMethod", index, name, Vec::new())
}

fn structure(
    name: &str,
    noise: Option<i64>,
) -> Value {
    match noise {
        | Some(noise) => variant("SdeStructure", 1, name, vec![Value::int(noise)]),
        | None => variant("SdeStructure", 0, name, Vec::new()),
    }
}

fn fixed_options(
    method_value: Value,
    structure_value: Value,
    seed: i64,
    dt: f64,
    steps: i64,
) -> Value {
    fixed_options_with_terms(method_value, structure_value, seed, dt, steps, 0)
}

fn fixed_options_with_terms(
    method_value: Value,
    structure_value: Value,
    seed: i64,
    dt: f64,
    steps: i64,
    terms: i64,
) -> Value {
    let options = record(vec![
        ("structure", structure_value),
        ("method", method_value),
        ("seed", Value::int(seed)),
        ("t0", Value::float(0.0)),
        ("dt", Value::float(dt)),
        ("steps", Value::int(steps)),
        ("terms", Value::int(terms)),
    ]);
    variant("SdeOpts", 0, "Fixed", vec![options])
}

fn adaptive_options(
    method_value: Value,
    structure_value: Value,
    seed: i64,
    cells: i64,
    t1: f64,
    rtol: f64,
) -> Value {
    let options = record(vec![
        ("structure", structure_value),
        ("method", method_value),
        ("seed", Value::int(seed)),
        ("t0", Value::float(0.0)),
        ("t1", Value::float(t1)),
        ("rtol", Value::float(rtol)),
        ("atol", Value::float(1e-6)),
        ("max_steps", Value::int(100_000)),
        ("cells", Value::int(cells)),
        ("terms", Value::int(0)),
    ]);
    variant("SdeOpts", 1, "Adaptive", vec![options])
}

fn call(
    name: &str,
    args: Vec<Value>,
) -> Value {
    call_simple(name, args)
        .unwrap_or_else(|error| panic!("`{name}` failed: {} {}", error.code, error.message))
}

/// The `Ok` payload of a `Result`, or a panic carrying the `Err` payload.
fn ok_payload(
    value: Value,
    context: &str,
) -> Rc<RecordValue> {
    let Value::Variant(result) = value else {
        panic!("{context}: expected a Result");
    };
    if result.index != 0 {
        panic!("{context}: expected Ok, found Err({:?})", result.fields);
    }
    match result.fields.first() {
        | Some(Value::Record(record)) => record.clone(),
        | other => panic!("{context}: the Ok payload is not a record: {other:?}"),
    }
}

fn err_payload(
    value: Value,
    context: &str,
) -> Rc<aevia_core::value::VariantValue> {
    let Value::Variant(result) = value else {
        panic!("{context}: expected a Result");
    };
    if result.index != 1 {
        panic!("{context}: expected Err, found Ok");
    }
    match result.fields.first() {
        | Some(Value::Variant(error)) => error.clone(),
        | other => panic!("{context}: the Err payload is not an SdeError: {other:?}"),
    }
}

fn tensor_of(value: &Value) -> &aevia_core::value::TensorData {
    match value {
        | Value::Tensor(tensor) => tensor,
        | other => panic!("expected a tensor, found {}", other.type_name()),
    }
}

fn float_of(value: &Value) -> f64 {
    value.as_f64().expect("a number")
}

/// A native drift that returns the state unchanged (`dX = X dW` has that shape
/// after flattening; the dimension of the rate is erased at run time).
fn identity_drift() -> Value {
    Value::Native(Rc::new(NativeFn(Box::new(
        |_interp: &mut aevia_core::Interp, args: Vec<Value>, _span: Span| {
            Ok(args.get(1).cloned().unwrap_or(Value::Unit))
        },
    ))))
}

/// A native diffusion returning `n` diagonal coefficients of 1.
fn unit_diffusion() -> Value {
    Value::Native(Rc::new(NativeFn(Box::new(
        |_interp: &mut aevia_core::Interp, args: Vec<Value>, _span: Span| {
            let state = args.get(1).cloned().unwrap_or(Value::float(0.0));
            let Value::Num(number) = &state else {
                panic!("the test state is a scalar");
            };
            let _ = number;
            Ok(Value::float(1.0))
        },
    ))))
}

// ---------------------------------------------------------------------------
// wiener / sde_order / sde_ensemble_seed
// ---------------------------------------------------------------------------

/// `wiener(seed, m, steps, dt)` is `steps × m` increments on the language
/// stream; the same seed reproduces them, and the values are the standard
/// library's own.
#[test]
fn wiener_is_the_language_stream() {
    let first = call(
        "wiener",
        vec![
            Value::int(7),
            Value::int(2),
            Value::int(3),
            Value::float(0.5),
        ],
    );
    let again = call(
        "wiener",
        vec![
            Value::int(7),
            Value::int(2),
            Value::int(3),
            Value::float(0.5),
        ],
    );
    let tensor = tensor_of(&first);
    assert_eq!(tensor.shape, vec![3, 2], "(seed, m, steps, dt) → steps × m");
    let expected: Vec<f64> = aevia_std::sde::wiener(7, 2, 3, 0.5)
        .into_iter()
        .flatten()
        .collect();
    let observed: Vec<f64> = tensor.elements().iter().map(float_of).collect();
    assert_eq!(
        observed, expected,
        "wiener must be the std library's stream"
    );
    let repeated: Vec<f64> = tensor_of(&again).elements().iter().map(float_of).collect();
    assert_eq!(observed, repeated, "same seed, same increments");
}

#[test]
fn sde_order_and_ensemble_seed_are_pure() {
    let order = call("sde_order", vec![method(0, "EulerMaruyama")]);
    let Value::Record(order) = order else {
        panic!("sde_order answers a record")
    };
    assert_eq!(float_of(&order.fields["strong"]), 0.5);
    assert_eq!(float_of(&order.fields["weak"]), 1.0);

    let seed = call("sde_ensemble_seed", vec![Value::int(11), Value::int(4)]);
    assert_eq!(float_of(&seed), aevia_std::sde::ensemble_seed(11, 4) as f64);
    assert_eq!(
        float_of(&call(
            "sde_ensemble_seed",
            vec![Value::int(11), Value::int(4)]
        )),
        float_of(&seed),
        "a function of (seed, index) only"
    );
}

// ---------------------------------------------------------------------------
// sdeint_diag / sdeint_general
// ---------------------------------------------------------------------------

/// A scalar state integrates through the diagonal path and answers the
/// `SdePath` record: `t` and `y` parallel, metadata filled.
#[test]
fn sdeint_diag_answers_a_path() {
    let options = fixed_options(
        method(0, "EulerMaruyama"),
        structure("Diagonal", None),
        7,
        0.1,
        4,
    );
    let first = call(
        "sdeint_diag",
        vec![
            identity_drift(),
            unit_diffusion(),
            Value::float(1.0),
            options.clone(),
        ],
    );
    let path = ok_payload(first, "sdeint_diag");
    let times = tensor_of(&path.fields["t"]);
    let states = tensor_of(&path.fields["y"]);
    assert_eq!(times.shape.len(), 1, "t is rank 1");
    assert_eq!(times.shape, states.shape, "t and y are parallel");
    assert!(times.shape[0] >= 5, "initial point plus the accepted steps");
    assert_eq!(float_of(&path.fields["steps"]), 4.0);
    assert_eq!(float_of(&path.fields["noise"]), 1.0);
    assert_eq!(float_of(&path.fields["seed"]), 7.0);
    for element in states.elements() {
        assert!(float_of(&element).is_finite(), "states stay finite");
    }

    // Reproducibility: a fixed-step path on the language stream is deterministic.
    let second = call(
        "sdeint_diag",
        vec![
            identity_drift(),
            unit_diffusion(),
            Value::float(1.0),
            options,
        ],
    );
    let again = ok_payload(second, "sdeint_diag again");
    let first_states: Vec<f64> = states.elements().iter().map(float_of).collect();
    let second_states: Vec<f64> = tensor_of(&again.fields["y"])
        .elements()
        .iter()
        .map(float_of)
        .collect();
    assert_eq!(first_states, second_states, "same options, same path");
}

/// The wrong structure for the entry point is the recoverable `Err(SdeError)`,
/// not a panic: `sdeint_diag` requires `Diagonal`, `sdeint_general` a general
/// structure.
#[test]
fn the_wrong_structure_is_a_recoverable_error() {
    let options = fixed_options(
        method(0, "EulerMaruyama"),
        structure("General", Some(1)),
        7,
        0.1,
        2,
    );
    let error = err_payload(
        call(
            "sdeint_diag",
            vec![
                identity_drift(),
                unit_diffusion(),
                Value::float(1.0),
                options,
            ],
        ),
        "sdeint_diag with a general structure",
    );
    assert_eq!(error.name, "Invalid");
    assert!(
        aevia_core::value::show(&Value::Variant(error)).contains("sdeint_general"),
        "the message points at the right entry point"
    );
}

/// General noise: the diffusion returns the `n × m` matrix; a scalar state with
/// two channels integrates.
#[test]
fn sdeint_general_takes_a_matrix_diffusion() {
    let matrix_diffusion: Value = Value::Native(Rc::new(NativeFn(Box::new(
        |_interp: &mut aevia_core::Interp, _args: Vec<Value>, _span: Span| {
            // n = 1 state, m = 2 noise channels → two coefficients.
            Ok(aevia_core::eval::build_tensor(
                vec![Value::float(1.0), Value::float(0.5)],
                vec![1, 2],
                aevia_dim::Dim::one(),
            ))
        },
    ))));
    let options = fixed_options(
        method(0, "EulerMaruyama"),
        structure("General", Some(2)),
        3,
        0.1,
        2,
    );
    let path = ok_payload(
        call(
            "sdeint_general",
            vec![
                identity_drift(),
                matrix_diffusion,
                Value::float(0.0),
                options,
            ],
        ),
        "sdeint_general",
    );
    assert_eq!(float_of(&path.fields["noise"]), 2.0);
    for element in tensor_of(&path.fields["y"]).elements() {
        assert!(float_of(&element).is_finite());
    }
}

/// Every fixed-step scheme this build exposes integrates a scalar state.
#[test]
fn every_fixed_step_method_integrates() {
    let diagonal_methods = [
        (0, "EulerMaruyama"),
        (1, "Milstein"),
        (2, "MilsteinStratonovich"),
        (3, "HeunStratonovich"),
        (4, "SrkPlaten"),
    ];
    for (index, name) in diagonal_methods {
        let options = fixed_options(method(index, name), structure("Diagonal", None), 5, 0.05, 3);
        let path = ok_payload(
            call(
                "sdeint_diag",
                vec![
                    identity_drift(),
                    unit_diffusion(),
                    Value::float(1.0),
                    options,
                ],
            ),
            name,
        );
        assert!(
            float_of(&path.fields["steps"]) >= 3.0,
            "{name}: the fixed steps ran"
        );
    }

    // The non-commutative scheme needs a non-commutative structure and a
    // positive number of Lévy-area modes.
    let matrix: Value = Value::Native(Rc::new(NativeFn(Box::new(
        |_interp: &mut aevia_core::Interp, _args: Vec<Value>, _span: Span| {
            Ok(aevia_core::eval::build_tensor(
                vec![Value::float(1.0)],
                vec![1, 1],
                aevia_dim::Dim::one(),
            ))
        },
    ))));
    let options = fixed_options_with_terms(
        method(5, "MilsteinNoncommutative"),
        structure("GeneralNoncommutative", Some(1)),
        5,
        0.05,
        3,
        2,
    );
    let path = ok_payload(
        call(
            "sdeint_general",
            vec![identity_drift(), matrix, Value::float(1.0), options],
        ),
        "MilsteinNoncommutative",
    );
    assert_eq!(float_of(&path.fields["noise"]), 1.0);
}

/// The adaptive controller runs for the Itô commutative schemes.
#[test]
fn adaptive_integration_runs() {
    let zero_drift: Value = Value::Native(Rc::new(NativeFn(Box::new(
        |_interp: &mut aevia_core::Interp, _args: Vec<Value>, _span: Span| Ok(Value::float(0.0)),
    ))));
    let options = variant(
        "SdeOpts",
        1,
        "Adaptive",
        vec![record(vec![
            ("structure", structure("Diagonal", None)),
            ("method", method(1, "Milstein")),
            ("seed", Value::int(9)),
            ("t0", Value::float(0.0)),
            ("t1", Value::float(0.05)),
            ("rtol", Value::float(1e-3)),
            ("atol", Value::float(1e-6)),
            ("max_steps", Value::int(100_000)),
            ("cells", Value::int(0)),
            ("terms", Value::int(0)),
        ])],
    );
    let path = ok_payload(
        call(
            "sdeint_diag",
            vec![zero_drift, unit_diffusion(), Value::float(1.0), options],
        ),
        "adaptive",
    );
    assert!(float_of(&path.fields["steps"]) >= 1.0);
    let times = tensor_of(&path.fields["t"]);
    assert!(
        times
            .elements()
            .iter()
            .all(|value| float_of(value) <= 0.05 + 1e-9),
        "the path stays inside [t0, t1]"
    );
}

/// The option record is validated at run time (§8.9.1): a malformed one is the
/// recoverable `SdeError::Invalid` (`E0802`), not a silent default.
#[test]
fn malformed_options_are_e0802() {
    // Missing `dt`.
    let options = variant(
        "SdeOpts",
        0,
        "Fixed",
        vec![record(vec![
            ("structure", structure("Diagonal", None)),
            ("method", method(0, "EulerMaruyama")),
            ("seed", Value::int(1)),
            ("t0", Value::float(0.0)),
            ("steps", Value::int(2)),
            ("terms", Value::int(0)),
        ])],
    );
    let error = err_payload(
        call(
            "sdeint_diag",
            vec![
                identity_drift(),
                unit_diffusion(),
                Value::float(1.0),
                options,
            ],
        ),
        "missing dt",
    );
    assert_eq!(error.name, "Invalid");
    assert!(
        aevia_core::value::show(&Value::Variant(error)).contains("dt"),
        "the message names the missing field"
    );

    // The non-commutative method without any Lévy-area modes.
    let options = fixed_options_with_terms(
        method(5, "MilsteinNoncommutative"),
        structure("GeneralNoncommutative", Some(1)),
        1,
        0.1,
        1,
        0,
    );
    let matrix: Value = Value::Native(Rc::new(NativeFn(Box::new(
        |_interp: &mut aevia_core::Interp, _args: Vec<Value>, _span: Span| {
            Ok(aevia_core::eval::build_tensor(
                vec![Value::float(1.0)],
                vec![1, 1],
                aevia_dim::Dim::one(),
            ))
        },
    ))));
    let error = err_payload(
        call(
            "sdeint_general",
            vec![identity_drift(), matrix, Value::float(1.0), options],
        ),
        "terms missing",
    );
    assert_eq!(error.name, "Invalid");
    assert!(
        aevia_core::value::show(&Value::Variant(error)).contains("terms"),
        "the message asks for the Lévy-area modes"
    );
}

/// Capability gaps surface as `Unsupported` with the capability name, not as a
/// silently different scheme.
#[test]
fn unsupported_combinations_name_their_capability() {
    let capability_of = |value: Value, context: &str| -> String {
        let error = err_payload(value, context);
        assert_eq!(error.name, "Unsupported", "{context}: {error:?}");
        match error.fields.first() {
            | Some(Value::Str(text)) => text.to_string(),
            | other => panic!("{context}: the capability is a string, found {other:?}"),
        }
    };

    // A Stratonovich scheme has no adaptive controller in this build.
    let options = adaptive_options(
        method(3, "HeunStratonovich"),
        structure("Diagonal", None),
        9,
        0,
        0.05,
        1e-3,
    );
    let capability = capability_of(
        call(
            "sdeint_diag",
            vec![
                identity_drift(),
                unit_diffusion(),
                Value::float(1.0),
                options,
            ],
        ),
        "adaptive Stratonovich",
    );
    assert_eq!(capability, "sde.interpretation");

    // The non-commutative method requires non-commutative noise. The Lévy-area
    // mode count is valid, so the *combination* is what is reported.
    let options = fixed_options_with_terms(
        method(5, "MilsteinNoncommutative"),
        structure("Diagonal", None),
        1,
        0.1,
        1,
        2,
    );
    let capability = capability_of(
        call(
            "sdeint_diag",
            vec![
                identity_drift(),
                unit_diffusion(),
                Value::float(1.0),
                options,
            ],
        ),
        "non-commutative on diagonal",
    );
    assert_eq!(capability, "sde.noncommutative");
}
#[test]
fn general_noise_with_a_record_state_is_named_gap() {
    let drift: Value = Value::Native(Rc::new(NativeFn(Box::new(
        |_interp: &mut aevia_core::Interp, _args: Vec<Value>, _span: Span| {
            Ok(record(vec![("x", Value::float(0.0))]))
        },
    ))));
    let diffusion: Value = Value::Native(Rc::new(NativeFn(Box::new(
        |_interp: &mut aevia_core::Interp, _args: Vec<Value>, _span: Span| {
            Ok(aevia_core::eval::build_tensor(
                vec![Value::float(1.0)],
                vec![1, 1],
                aevia_dim::Dim::one(),
            ))
        },
    ))));
    let initial = record(vec![("x", Value::float(1.0))]);
    let options = fixed_options(
        method(0, "EulerMaruyama"),
        structure("General", Some(1)),
        1,
        0.1,
        1,
    );
    let error = err_payload(
        call("sdeint_general", vec![drift, diffusion, initial, options]),
        "sdeint_general with a record state",
    );
    assert_eq!(error.name, "Unsupported");
    assert!(
        error
            .fields
            .first()
            .and_then(|value| {
                match value {
                    | Value::Str(text) => Some(text.to_string()),
                    | _ => None,
                }
            })
            .is_some_and(|capability| capability == "sde.general.record"),
        "the Err payload names the capability: {:?}",
        error.fields
    );
}
