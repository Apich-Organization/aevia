//! The `Rssn` entry points (§8.19, item 5 of the round).
//!
//! `Rssn.typed` is the one of the four that the bridge can already answer: it
//! is the dynamic call plus a declared result type. `call_ho`, `register` and
//! `register_plugin` need bridge APIs that do not exist (a `Callback` and a
//! plugin registry), and the checker has no signature for any of the four, so
//! the last test pins the state that remains.

#![allow(clippy::result_large_err)]

use aevia_core::builtins::call_simple;
use aevia_core::value::{Storage, TensorData, Value};
use aevia_core::{compile, rssn_module};
use aevia_dim::Dim;
use aevia_syntax::Span;
use std::rc::Rc;

fn list(items: Vec<f64>) -> Value {
    let len = items.len();
    Value::Tensor(Rc::new(TensorData {
        shape: vec![len],
        storage: Storage::Dense(Rc::new(items)),
        dim: Dim::one(),
        display: None,
    }))
}

fn type_rep() -> Value {
    call_simple("type_rep", Vec::new()).expect("`type_rep` is implemented")
}

fn result(value: Value) -> (usize, Value) {
    match value {
        Value::Variant(variant) => (
            variant.index,
            variant.fields.first().cloned().unwrap_or(Value::Unit),
        ),
        other => panic!("expected a Result, found {}", other.type_name()),
    }
}

#[test]
fn typed_runs_the_dynamic_call() {
    let value = rssn_module::call(
        "typed",
        vec![Value::string("sin"), list(vec![0.5]), type_rep()],
        Span::default(),
    )
    .expect("`Rssn.typed` is an entry point");
    let (index, payload) = result(value);
    assert_eq!(index, 0, "the call succeeds");
    assert!(
        payload.as_f64().is_some(),
        "`sin` answers with a number, found {}",
        payload.type_name()
    );
}

#[test]
fn typed_requires_a_type_rep() {
    let error = rssn_module::call(
        "typed",
        vec![Value::string("sin"), list(vec![0.5]), Value::int(1)],
        Span::default(),
    )
    .expect_err("a scalar is not a TypeRep");
    assert_eq!(error.code, "E0302");

    let error = rssn_module::call(
        "typed",
        vec![Value::string("sin"), list(vec![0.5])],
        Span::default(),
    )
    .expect_err("the TypeRep argument is required");
    assert_eq!(error.code, "E0315");
}

#[test]
fn typed_is_an_entry_point_not_an_operator() {
    // The evaluator must route it to `call`, not look for a pinned overload:
    // it is a fixed member, not an operator of the manifest.
    assert!(rssn_module::is_entry_point("typed"));
    assert!(rssn_module::is_entry_point("call"));
    assert!(!rssn_module::is_entry_point("sin"));
}

#[test]
fn the_checker_still_has_no_signature_for_the_entry_points() {
    // §8.19 declares all four; `check.rs::rssn_member` types none of them, so
    // a program cannot name them yet. `typed` runs at the bridge level, which
    // is what makes it a checker gap rather than a runtime one.
    for member in ["typed", "call_ho", "register", "register_plugin"] {
        let source = format!("let x = Rssn.{member}\n");
        let compilation = compile("rssn.aev", &source);
        assert!(
            compilation.diagnostics.has_errors(),
            "`Rssn.{member}` now checks; the entry must be removed from the report"
        );
    }
}
