//! The `Rssn` entry points of §8.19, from source to dispatch.
//!
//! Four names are *fixed members* rather than manifest operators: they are
//! declared by hand in the checker and dispatched by hand in the bridge. A name
//! that is typed but has no dispatch arm is a bug, and so is an arm that no
//! program can reach — so every test here goes from Aevia source through the
//! real checker and the real evaluator, not just through the bridge.
//!
//! Two of them cannot do their full job in this build, and the tests pin the
//! *exact* honest outcome instead of a silent block:
//!
//! * `Rssn.call_ho` needs `Tensor<1, Callback>`; the kernel's host-callback ABI
//!   is `Send + Sync` and this evaluator is single-threaded, so a non-empty
//!   callback list is `E1001("core.callback")`. The empty list is the kernel's
//!   ordinary dynamic call and runs.
//! * `Rssn.register`/`Rssn.register_plugin` need a `Plugin` value; in-process
//!   plugins are Rust host objects (§13.5.1) and source-level `@op` packaging
//!   (§13.5.2) is not implemented, so both are `E1001("core.plugin.source")`.

#![allow(clippy::result_large_err)]

use aevia_core::builtins::call_simple;
use aevia_core::value::{Storage, TensorData, Value};
use aevia_core::{Interp, compile, rssn_module};
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

/// Compiles `source`, asserting it has no errors, and returns the checked
/// module so the arguments below are evaluated by the real evaluator.
fn checked(source: &str) -> (aevia_syntax::Module, aevia_types::CheckedModule) {
    let compilation = compile("rssn.aev", source);
    let errors: Vec<String> = compilation
        .diagnostics
        .items
        .iter()
        .filter(|item| item.severity == aevia_syntax::Severity::Error)
        .map(|item| format!("{}: {}", item.code, item.message))
        .collect();
    assert!(errors.is_empty(), "compile errors: {errors:?}");
    (compilation.module.expect("a module"), compilation.checked)
}

fn toplevel(source: &str, name: &str) -> Value {
    let (module, checked) = checked(source);
    let mut interp = Interp::new(&checked);
    let load = interp.load(&module);
    assert!(
        !load.has_errors(),
        "load errors: {:?}",
        load.items
            .iter()
            .map(|item| (&item.code, &item.message))
            .collect::<Vec<_>>()
    );
    interp.eval_toplevel(name).expect("the binding exists")
}

// ---------------------------------------------------------------------------
// `Rssn.typed`
// ---------------------------------------------------------------------------

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
    let number = payload
        .as_f64()
        .unwrap_or_else(|| panic!("`sin` answers with a number, found {}", payload.type_name()));
    assert!((number - 0.5f64.sin()).abs() < 1e-12);
}

#[test]
fn typed_is_reachable_from_source_and_executes() {
    // §8.19 writes `Rssn.typed : (String, Tensor<1, Value>, TypeRep) ->
    // Result<a, OpError>`. This build types it `Result<Value, OpError>`: the
    // `TypeRep` `type_rep()` produces is opaque (it carries no type witness a
    // runtime could check the answer against), so a fresh `a` would be the
    // unsound `∀a.a` the bridge removed for `Rssn.invoke`. The caller converts
    // the payload with `from_value`. The declaration is reachable and the call
    // it types runs through the bridge.
    let value = toplevel(
        r#"let r = Rssn.typed("sin", [to_value(0.5)], type_rep())
"#,
        "r",
    );
    let (index, payload) = result(value);
    assert_eq!(index, 0, "the typed call succeeds");
    let number = payload
        .as_f64()
        .unwrap_or_else(|| panic!("expected a number, found {}", payload.type_name()));
    assert!((number - 0.5f64.sin()).abs() < 1e-12);
}

#[test]
fn the_entry_points_run_on_both_evaluators() {
    // The fixed members are dispatched by the bridge, not by the evaluator, so
    // the AST path and the IR path must agree about them. The AST evaluator is
    // a second implementation, not the reference, but an entry point reachable
    // on only one of the two is still a bug.
    use aevia_core::eval::Mode;

    let (module, checked) = checked(
        r#"let r = Rssn.typed("sin", [to_value(0.5)], type_rep())
"#,
    );
    let run = |mode: Mode| -> Value {
        let mut interp = Interp::new(&checked).with_mode(mode);
        let diagnostics = interp.load(&module);
        assert!(!diagnostics.has_errors(), "load errors: {diagnostics:?}");
        interp.eval_toplevel("r").expect("`r` evaluates")
    };
    let (ir_index, ir_payload) = result(run(Mode::Ir));
    let (ast_index, ast_payload) = result(run(Mode::Ast));
    assert_eq!(ir_index, 0, "the IR path");
    assert_eq!(ast_index, 0, "the AST path");
    assert_eq!(
        ir_payload.as_f64().map(f64::to_bits),
        ast_payload.as_f64().map(f64::to_bits),
        "the two evaluators must answer the same value"
    );
}

#[test]
fn the_entry_point_signatures_state_their_arguments() {
    // "Reachable from source" must not mean "accepts anything": the checker
    // signature is a contract about the argument list, and a wrong one is a
    // compile error there, not a runtime surprise at the dynamic boundary.
    for (source, expected) in [
        // The third argument is a `TypeRep`, not a scalar.
        (r#"let bad = Rssn.typed("sin", [], 1)"#, "E0302"),
        // All three arguments are required.
        (r#"let bad = Rssn.typed("sin")"#, "E0315"),
        // The third argument is `Tensor<1, Callback>`, not a scalar.
        (r#"let bad = Rssn.call_ho("sin", [], 1)"#, "E0302"),
        // A `Plugin` is an opaque host handle, not an `Int` or a `String`.
        (r#"let bad = Rssn.register(1)"#, "E0302"),
        (r#"let bad = Rssn.register_plugin("x")"#, "E0302"),
    ] {
        let compilation = compile("rssn.aev", source);
        let codes: Vec<&str> = compilation
            .diagnostics
            .items
            .iter()
            .map(|item| item.code.as_str())
            .collect();
        assert!(
            compilation.diagnostics.has_errors(),
            "`{source}` must not type-check; codes were {codes:?}"
        );
        assert!(
            codes.contains(&expected),
            "`{source}` must report {expected}, found {codes:?}"
        );
    }
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

// ---------------------------------------------------------------------------
// `Rssn.call_ho`
// ---------------------------------------------------------------------------

#[test]
fn a_callback_free_call_ho_is_the_dynamic_call() {
    // The kernel's `call_ho` with no callbacks *is* `call_value`; forwarding
    // there is the one faithful half of §8.19 this build can support.
    let via_ho = rssn_module::call(
        "call_ho",
        vec![Value::string("sin"), list(vec![0.5]), list(Vec::new())],
        Span::default(),
    )
    .expect("a callback-free `Rssn.call_ho` is an entry point");
    let via_dynamic = rssn_module::call(
        "dynamic",
        vec![Value::string("sin"), list(vec![0.5])],
        Span::default(),
    )
    .expect("`Rssn.dynamic`");
    let (ho_index, ho_payload) = result(via_ho);
    let (dynamic_index, dynamic_payload) = result(via_dynamic);
    assert_eq!(ho_index, 0);
    assert_eq!(dynamic_index, 0);
    assert_eq!(
        ho_payload.as_f64().map(f64::to_bits),
        dynamic_payload.as_f64().map(f64::to_bits),
        "the callback-free `call_ho` must answer exactly what `dynamic` answers"
    );
}

#[test]
fn call_ho_is_reachable_from_source() {
    // `[]` is `Tensor<1, Callback>` by the parameter's type; the checker
    // supplies the element type, so an empty callback list is expressible.
    let value = toplevel(
        r#"let r = Rssn.call_ho("sin", [to_value(0.5)], [])
"#,
        "r",
    );
    let (index, payload) = result(value);
    assert_eq!(index, 0);
    let number = payload
        .as_f64()
        .unwrap_or_else(|| panic!("expected a number, found {}", payload.type_name()));
    assert!((number - 0.5f64.sin()).abs() < 1e-12);
}

#[test]
fn a_non_empty_callback_list_names_the_callback_capability() {
    let error = rssn_module::call(
        "call_ho",
        vec![
            Value::string("sin"),
            list(vec![0.5]),
            list(vec![1.0]),
        ],
        Span::default(),
    )
    .expect_err("an Aevia closure cannot cross as a kernel callback");
    assert_eq!(error.code, "E1001");
    assert!(
        error.message.contains("core.callback"),
        "the diagnostic must name the capability: {}",
        error.message
    );
    assert!(
        error
            .notes
            .iter()
            .any(|note| note.contains("Send + Sync")),
        "the note must say what the ABI requires: {:?}",
        error.notes
    );
    assert!(
        !aevia_rssn::registry()
            .capabilities
            .has("core.callback"),
        "the capability row exists but must be disabled"
    );
}

// ---------------------------------------------------------------------------
// `Rssn.register` / `Rssn.register_plugin`
// ---------------------------------------------------------------------------

#[test]
fn register_names_the_missing_source_plugin_capability() {
    for name in ["register", "register_plugin"] {
        let error = rssn_module::call(name, vec![Value::Unit], Span::default())
            .expect_err("no Aevia expression can build a `Plugin`");
        assert_eq!(error.code, "E1001", "`{name}` must report a capability gap");
        assert!(
            error.message.contains("core.plugin.source"),
            "`{name}` must name `core.plugin.source`, not `core.plugin` (which \
             this build does provide): {}",
            error.message
        );
        assert!(
            error.notes.iter().any(|note| note.contains("@op")),
            "the note must name the missing source-level plugin packaging: {:?}",
            error.notes
        );
    }
    let capabilities = &aevia_rssn::registry().capabilities;
    assert!(!capabilities.has("core.plugin.source"));
    assert!(
        capabilities.has("core.plugin"),
        "in-process host plugins exist in this build; only the source-level \
         packaging is missing"
    );
}

#[test]
fn register_is_reachable_from_source_as_a_function() {
    // A `Plugin` is an opaque host handle: a program can name the type and
    // pass one around, even though this build cannot construct a value of it.
    let compilation = compile(
        "rssn.aev",
        "fn reg(p: Plugin) = Rssn.register(p)\nlet alias = Rssn.register_plugin\n",
    );
    let errors: Vec<String> = compilation
        .diagnostics
        .items
        .iter()
        .filter(|item| item.severity == aevia_syntax::Severity::Error)
        .map(|item| format!("{}: {}", item.code, item.message))
        .collect();
    assert!(errors.is_empty(), "not reachable from source: {errors:?}");
}

#[test]
fn the_entry_points_are_entry_points_not_operators() {
    // The evaluator must route them to `call`, not look for a pinned overload:
    // they are fixed members, not operators of the manifest.
    for member in [
        "typed",
        "call_ho",
        "register",
        "register_plugin",
        "call",
        "dynamic",
        "call_key",
    ] {
        assert!(
            rssn_module::is_entry_point(member),
            "`Rssn.{member}` is an entry point"
        );
        assert!(
            aevia_rssn::registry().op(member).is_none(),
            "`{member}` is not a manifest operator"
        );
    }
    assert!(!rssn_module::is_entry_point("sin"));
}
