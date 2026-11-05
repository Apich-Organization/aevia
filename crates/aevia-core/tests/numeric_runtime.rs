//! Runtime semantics for the options a numeric call takes: the `sample_n` seed
//! (§8.7.1), `OdeOpts.events` (§8.9) and the numeric-failure codes (§6.9.2,
//! appendix B §B.10).
//!
//! These tests drive the builtin entry points directly where the checker does
//! not yet carry the option, and assert values/codes — never text, except the
//! capability name a named gap must carry.

#![allow(clippy::result_large_err)]

use aevia_core::builtins::call_simple;
use aevia_core::value::{make_variant, NativeFn, RecordValue, Value};
use aevia_core::Interp;
use aevia_syntax::Span;
use aevia_types::adt;
use indexmap::IndexMap;
use std::rc::Rc;

fn int(value: i64) -> Value {
    Value::int(value)
}

fn float(value: f64) -> Value {
    Value::float(value)
}

fn record(fields: Vec<(&str, Value)>) -> Value {
    let mut map = IndexMap::new();
    for (name, value) in fields {
        map.insert(name.to_string(), value);
    }
    Value::Record(Rc::new(RecordValue { fields: map }))
}

fn uniform() -> Value {
    record(vec![("lo", float(0.0)), ("hi", float(4.0))])
}

fn some(seed: i64) -> Value {
    make_variant(adt::OPTION_ID, 1, "Some", vec![int(seed)])
}

fn none() -> Value {
    make_variant(adt::OPTION_ID, 0, "None", Vec::new())
}

fn samples(name: &str, args: Vec<Value>) -> Vec<f64> {
    let value = call_simple(name, args)
        .unwrap_or_else(|error| panic!("`{name}` failed: {} {}", error.code, error.message));
    tensor_values(value)
}

/// The same, on one session, so an unseeded call advances that session's RNG.
fn samples_via(interp: &mut Interp, args: Vec<Value>) -> Vec<f64> {
    let value = interp
        .call_named("sample_n", args, Span::default())
        .unwrap_or_else(|error| panic!("`sample_n` failed: {} {}", error.code, error.message));
    tensor_values(value)
}

fn tensor_values(value: Value) -> Vec<f64> {
    let Value::Tensor(tensor) = value else {
        panic!("`sample_n` answers a tensor")
    };
    tensor
        .elements()
        .iter()
        .map(|element| element.as_f64().expect("a float"))
        .collect()
}

// ---------------------------------------------------------------------------
// §8.7.1 — `sample_n`'s seed
// ---------------------------------------------------------------------------

/// `sample_n(dist, n, Some(seed))` is reproducible: the seed decides the
/// sequence, independently of the session's RNG state and of what ran before.
#[test]
fn a_seeded_sample_is_reproducible() {
    let argument = vec![uniform(), int(8), some(7)];
    let first = samples("sample_n", argument.clone());
    let second = samples("sample_n", argument);
    assert_eq!(first, second, "same seed, same sample");

    let other = samples("sample_n", vec![uniform(), int(8), some(8)]);
    assert_ne!(first, other, "a different seed gives a different sample");

    // The sample is still inside the distribution's support.
    for value in &first {
        assert!((0.0..=4.0).contains(value), "sample {value} outside support");
    }
}

/// A seeded sample must not advance the session RNG: it is a pure function of
/// its arguments (§6.10), so its result cannot depend on how many `random`
/// actions ran earlier.
#[test]
fn a_seeded_sample_does_not_perturb_the_session_rng() {
    let mut expected = Interp::new(&aevia_types::CheckedModule::default());
    let first = expected.next_random();
    let second = expected.next_random();

    let mut interpreter = Interp::new(&aevia_types::CheckedModule::default());
    let observed_first = interpreter.next_random();
    interpreter
        .call_named(
            "sample_n",
            vec![uniform(), int(4), some(99)],
            Span::default(),
        )
        .expect("a seeded sample");
    let observed_second = interpreter.next_random();
    assert_eq!(observed_first, first);
    assert_eq!(
        observed_second, second,
        "the seeded sample consumed a value from the session sequence"
    );
}

/// `sample_n(dist, n, None)` draws from the session RNG, so two calls on the
/// same interpreter differ; the historical two-argument/`()` dispatch keeps
/// that behaviour. (A fresh interpreter starts from the same fixed seed, so the
/// test must reuse one.)
#[test]
fn an_unseeded_sample_advances_the_session_rng() {
    let mut interpreter = Interp::new(&aevia_types::CheckedModule::default());
    let first = samples_via(&mut interpreter, vec![uniform(), int(6), none()]);
    let second = samples_via(&mut interpreter, vec![uniform(), int(6), none()]);
    assert_ne!(first, second, "an unseeded sample advances the session RNG");
    let unit = samples_via(&mut interpreter, vec![uniform(), int(6), Value::Unit]);
    assert_eq!(unit.len(), 6, "the direct-dispatch shape still answers");
}

// ---------------------------------------------------------------------------
// §8.9 — `OdeOpts.events` is a named capability gap
// ---------------------------------------------------------------------------

fn constant_rhs() -> Value {
    Value::Native(Rc::new(NativeFn(Box::new(
        |_interp: &mut Interp, _args: Vec<Value>, _span: Span| Ok(float(0.0)),
    ))))
}

fn times() -> Value {
    aevia_core::eval::build_tensor(
        vec![float(0.0), float(0.25), float(0.5), float(1.0)],
        vec![4],
        aevia_dim::Dim::base("Time"),
    )
}

fn ode_options(events: Option<Value>) -> Value {
    let mut fields = vec![("rtol", float(1e-8)), ("atol", float(1e-10))];
    if let Some(events) = events {
        fields.push(("events", events));
    }
    record(fields)
}

/// §8.9: `events` terminates and interpolates at a sign change. This build's
/// kernel has no event support, so asking for it is `E1001` naming the
/// capability — never a silently ignored option.
#[test]
fn an_ode_event_option_is_a_named_capability_gap() {
    let mut interpreter = Interp::new(&aevia_types::CheckedModule::default());
    let error = interpreter
        .call_named(
            "odeint_with",
            vec![
                constant_rhs(),
                float(0.0),
                times(),
                ode_options(Some(some(1))),
            ],
            Span::default(),
        )
        .expect_err("events are not supported in this build");
    assert_eq!(error.code, "E1001", "a capability gap names the capability");
    assert!(
        error.message.contains("core.ode.events"),
        "the message names `core.ode.events`: {}",
        error.message
    );
    assert!(
        error.message.contains("events"),
        "the message names the option: {}",
        error.message
    );
}

/// `events = None` asks for no events, so it is not a gap: the integration runs
/// and answers the `odeint_with` record.
#[test]
fn an_absent_event_option_is_not_a_gap() {
    let mut interpreter = Interp::new(&aevia_types::CheckedModule::default());
    let outcome = interpreter
        .call_named(
            "odeint_with",
            vec![
                constant_rhs(),
                float(0.0),
                times(),
                ode_options(Some(none())),
            ],
            Span::default(),
        )
        .expect("no events requested");
    let Value::Record(trajectory) = outcome else {
        panic!("odeint_with answers a record");
    };
    assert!(trajectory.fields.contains_key("t"));
    assert!(trajectory.fields.contains_key("y"));
}

// ---------------------------------------------------------------------------
// §8.17 — the Json boundary (`to_json` / `from_json`)
// ---------------------------------------------------------------------------

/// §8.17: `Json` is the *decoded* value channel. `read_json`/`simulate` decode
/// on the way in and encode on the way out, so `to_json`/`from_json` are the
/// identity over that representation; a wrapper can pass a record to
/// `simulate` and decode the answer without a second value model.
#[test]
fn the_json_boundary_is_the_decoded_value_channel() {
    let original = record(vec![
        ("x", float(1.5)),
        ("label", Value::string("ok")),
        ("flag", Value::Bool(true)),
        ("seed", some(7)),
        ("missing", none()),
    ]);
    let json = call_simple("to_json", vec![original.clone()]).expect("to_json is total");
    assert!(
        aevia_core::value::equal(&json, &original).expect("comparable"),
        "to_json must hand `simulate` the decoded value, not JSON text"
    );
    let decoded = call_simple("from_json", vec![json]).expect("from_json is total");
    let Value::Variant(result) = decoded else {
        panic!("from_json answers a Result")
    };
    assert_eq!(result.index, 0, "a decoded value decodes");
    assert!(
        aevia_core::value::equal(&result.fields[0], &original).expect("comparable"),
        "from_json is the identity over the decoded channel"
    );
}
