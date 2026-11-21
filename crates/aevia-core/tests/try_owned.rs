//! Consuming a temporary at `?` preserves checked values and shared witnesses.

use aevia_core::Interp;
use aevia_core::Mode;
use aevia_core::compile;
use aevia_core::value::Value;
use aevia_dim::Dim;

#[test]
fn fresh_and_shared_successes_keep_dimension_and_payload_in_both_engines() {
    let compilation = compile(
        "try_success.aev",
        r#"fn fresh(): Result<Real[L], String> = Ok(2 m)
let stored: Result<Real[L], String> = Ok(3 m)
fn combined(): Result<Real[L], String> =
    let a = fresh()?
    let b = stored?
    let c = stored?
    Ok(a + b + c)
let outcome = combined()
"#,
    );
    assert!(
        !compilation.diagnostics.has_errors(),
        "{:?}",
        compilation.diagnostics.items
    );
    for mode in [Mode::Ast, Mode::Ir] {
        let mut interp = Interp::new(&compilation.checked).with_mode(mode);
        assert!(
            !interp
                .load(compilation.module.as_ref().unwrap())
                .has_errors(),
            "{mode:?}"
        );
        let Value::Variant(outcome) = interp.eval_toplevel("outcome").unwrap() else {
            panic!("{mode:?}: expected Result");
        };
        assert_eq!(outcome.name, "Ok");
        let [Value::Num(number)] = outcome.fields.as_slice() else {
            panic!("{mode:?}: expected one scalar");
        };
        assert_eq!(number.dim, Dim::base("Length"));
        assert_eq!(outcome.fields[0].as_f64(), Some(8.0));
    }
}

#[test]
fn propagated_failures_replace_phantom_types_without_mutating_shared_values() {
    let compilation = compile(
        "try_failure.aev",
        r#"fn fresh(): Result<Int, String> = Err("failure")
let stored: Result<Int, String> = fresh()
fn convert_fresh(): Result<String, String> =
    let value = fresh()?
    Ok(show(value))
fn convert_shared(): Result<String, String> =
    let value = stored?
    Ok(show(value))
let fresh_result = from_value<Result<String, String>>(to_value(convert_fresh()))
let shared_result = from_value<Result<String, String>>(to_value(convert_shared()))
let original = from_value<Result<Int, String>>(to_value(stored))
"#,
    );
    assert!(
        !compilation.diagnostics.has_errors(),
        "{:?}",
        compilation.diagnostics.items
    );
    for mode in [Mode::Ast, Mode::Ir] {
        let mut interp = Interp::new(&compilation.checked).with_mode(mode);
        assert!(
            !interp
                .load(compilation.module.as_ref().unwrap())
                .has_errors(),
            "{mode:?}"
        );
        for name in ["fresh_result", "shared_result", "original"] {
            let Value::Variant(decoded) = interp.eval_toplevel(name).unwrap() else {
                panic!("{mode:?} {name}: expected Result");
            };
            assert_eq!(decoded.name, "Ok", "{mode:?} {name}");
            let [Value::Variant(failure)] = decoded.fields.as_slice() else {
                panic!("{mode:?} {name}: missing failure");
            };
            assert_eq!(failure.name, "Err");
            let [Value::Str(message)] = failure.fields.as_slice() else {
                panic!("{mode:?} {name}: missing payload");
            };
            assert_eq!(message.as_ref(), "failure");
        }
    }
}
