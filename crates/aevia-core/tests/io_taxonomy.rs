//! IO error taxonomy (§9.12, appendix B: E0901–E0905) and program arguments.
//!
//! The taxonomy codes are observable in the `Err` payload of the file actions
//! and in the fatal diagnostics of the plot actions; `args()` is the host-set
//! argument vector.

#![allow(clippy::result_large_err)]

use aevia_core::Interp;
use aevia_core::compile;
use aevia_core::value::IoAction;
use aevia_core::value::Value;
use aevia_syntax::Span;
use std::path::PathBuf;

fn interpreter() -> Interp {
    let source = "fn main(): IO Unit = do\n    println(\"\")\n";
    let compilation = compile("io.aev", source);
    assert!(!compilation.diagnostics.has_errors());
    let mut interp = Interp::new(&compilation.checked).with_buffer();
    let diagnostics = interp.load(&compilation.module.expect("a module"));
    assert!(!diagnostics.has_errors());
    interp
}

/// The `Err` text of a `Result` value.
fn err_text(value: &Value) -> String {
    let Value::Variant(variant) = value else {
        panic!("expected a Result, found {}", value.type_name());
    };
    assert_eq!(variant.name, "Err", "expected Err, found Ok");
    match variant.fields.first() {
        | Some(Value::Str(text)) => text.to_string(),
        | other => panic!("the Err payload is not a String: {other:?}"),
    }
}

fn run_action(
    interp: &mut Interp,
    action: Value,
) -> Value {
    let Value::Io(action) = action else {
        panic!("expected an IO action, found {}", action.type_name());
    };
    interp.run_io(&action).expect("the action runs")
}

#[test]
fn a_missing_file_is_e0901() {
    let mut interp = interpreter();
    let missing = std::env::temp_dir()
        .join(format!("aevia-does-not-exist-{}.txt", std::process::id()))
        .to_string_lossy()
        .to_string();
    let action = interp
        .call_named("read_file", vec![Value::string(missing)], Span::default())
        .expect("read_file builds an action");
    let result = run_action(&mut interp, action);
    let text = err_text(&result);
    assert!(
        text.starts_with("E0901:"),
        "a missing file is E0901, found {text}"
    );
}

/// A failed read propagates its taxonomy through `read_csv`/`read_json`.
#[test]
fn a_missing_file_propagates_through_readers() {
    let mut interp = interpreter();
    let missing = std::env::temp_dir()
        .join(format!("aevia-does-not-exist-{}.csv", std::process::id()))
        .to_string_lossy()
        .to_string();
    let action = interp
        .call_named("read_csv", vec![Value::string(missing)], Span::default())
        .expect("read_csv builds an action");
    let text = err_text(&run_action(&mut interp, action));
    assert!(text.starts_with("E0901:"), "found {text}");
}

#[test]
fn a_malformed_data_file_is_e0903() {
    let path: PathBuf =
        std::env::temp_dir().join(format!("aevia-bad-json-{}.json", std::process::id()));
    std::fs::write(&path, "{not json").expect("write the malformed file");
    let mut interp = interpreter();
    let action = interp
        .call_named(
            "read_json",
            vec![Value::string(path.to_string_lossy().to_string())],
            Span::default(),
        )
        .expect("read_json builds an action");
    let text = err_text(&run_action(&mut interp, action));
    assert!(
        text.starts_with("E0903:"),
        "a malformed data file is E0903, found {text}"
    );
    let _ = std::fs::remove_file(&path);
}

/// A file whose extension the reader cannot decode is `E0904` (unsupported
/// format), before any IO happens.
#[test]
fn an_unsupported_extension_is_e0904() {
    let mut interp = interpreter();
    let action = interp
        .call_named("read_csv", vec![Value::string("data.npy")], Span::default())
        .expect("read_csv builds a Result value");
    let text = err_text(&action);
    assert!(
        text.starts_with("E0904:"),
        "an unsupported extension is E0904, found {text}"
    );

    let action = interp
        .call_named(
            "read_json",
            vec![Value::string("data.csv")],
            Span::default(),
        )
        .expect("read_json builds a Result value");
    let text = err_text(&action);
    assert!(text.starts_with("E0904:"), "found {text}");
}

/// A failed plot write is `E0905` (`plot rendering failed`).
#[test]
fn a_failed_plot_write_is_e0905() {
    let mut interp = interpreter();
    let action = interp
        .call_named(
            "save",
            vec![
                Value::string("this/directory/does/not/exist/plot.html"),
                Value::Unit,
            ],
            Span::default(),
        )
        .expect("save builds an action");
    let Value::Io(save) = action else {
        panic!("save answers an IO action")
    };
    let error = interp.run_io(&save).expect_err("the save must fail");
    assert_eq!(
        error.code, "E0905",
        "found {}: {}",
        error.code, error.message
    );
}

/// `args()` answers the host-set arguments as a rank-1 string tensor, and an
/// empty vector when unchanged.
#[test]
fn args_are_the_host_set_vector() {
    let mut interp = interpreter();
    let none = interp
        .call_named("args", Vec::new(), Span::default())
        .expect("args");
    let Value::Tensor(tensor) = &none else {
        panic!("args answers a tensor, found {}", none.type_name());
    };
    assert_eq!(tensor.storage.len(), 0);

    interp.set_args(["one".to_string(), "two".to_string()]);
    let some = interp
        .call_named("args", Vec::new(), Span::default())
        .expect("args");
    let Value::Tensor(tensor) = &some else {
        panic!("args answers a tensor");
    };
    assert_eq!(tensor.shape, vec![2]);
    let rendered: Vec<String> = tensor
        .elements()
        .iter()
        .map(|element| {
            match element {
                | Value::Str(text) => text.to_string(),
                | other => panic!("args holds strings, found {}", other.type_name()),
            }
        })
        .collect();
    assert_eq!(rendered, vec!["one", "two"]);
}

/// The plot action type is still reachable from the builtin layer (guards the
/// `Plot`/`Save` renames in the taxonomy change).
#[test]
fn plot_actions_are_still_built() {
    let mut interp = interpreter();
    let action = interp
        .call_named("plot", vec![Value::Unit], Span::default())
        .expect("plot");
    match action {
        | Value::Io(action) => {
            assert!(matches!(*action, IoAction::Plot(_)));
        },
        | other => panic!("plot answers an IO action, found {}", other.type_name()),
    }
}
