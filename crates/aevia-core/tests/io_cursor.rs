//! Deferred cursors preserve saved type bindings and lexical captures on replay.

use aevia_core::Interp;
use aevia_core::Mode;
use aevia_core::compile;
use aevia_core::value::Value;
use aevia_types::Type;

#[test]
fn nested_deferrals_replay_interleaved_with_independent_captures_and_types() {
    let compilation = compile(
        "io_cursor.aev",
        r#"fn replay<a>(item: a, label: String) = do
    let label = "{label} outer"
    forwarded <- do
        let label = "{label} inner"
        println(label)
        on("cpu", fn () => { value = item, witness = type_rep<a>() })
    println(label)
    on("cpu", fn () => forwarded)
let first = replay<Int>(7, "first")
let second = replay<String>("seven", "second")
"#,
    );
    assert!(
        !compilation.diagnostics.has_errors(),
        "{:?}",
        compilation.diagnostics.items
    );
    for mode in [Mode::Ast, Mode::Ir] {
        let mut interp = Interp::new(&compilation.checked)
            .with_mode(mode)
            .with_buffer();
        let loaded = interp.load(compilation.module.as_ref().unwrap());
        assert!(!loaded.has_errors(), "{mode:?}: {:?}", loaded.items);
        let Value::Io(first) = interp.eval_toplevel("first").unwrap() else {
            panic!("{mode:?}: expected first IO action");
        };
        let Value::Io(second) = interp.eval_toplevel("second").unwrap() else {
            panic!("{mode:?}: expected second IO action");
        };
        assert_eq!(interp.captured(), "");
        for (action, expected) in [
            (&first, Type::int()),
            (&second, Type::string()),
            (&first, Type::int()),
        ] {
            let Value::Record(output) = interp.run_io(action).unwrap() else {
                panic!("{mode:?}: expected result record");
            };
            let Some(Value::TypeRep(witness)) = output.fields.get("witness") else {
                panic!("{mode:?}: missing type evidence");
            };
            assert_eq!(*witness.as_ref(), expected, "{mode:?}");
            let value = output.fields.get("value").unwrap();
            if expected == Type::int() {
                assert_eq!(value.as_f64(), Some(7.0), "{mode:?}");
            } else {
                assert!(
                    matches!(value, Value::Str(text) if text.as_ref() == "seven"),
                    "{mode:?}"
                );
            }
        }
        assert_eq!(
            interp.captured(),
            concat!(
                "first outer inner\nfirst outer\n",
                "second outer inner\nsecond outer\n",
                "first outer inner\nfirst outer\n"
            ),
            "{mode:?}"
        );
    }
}
