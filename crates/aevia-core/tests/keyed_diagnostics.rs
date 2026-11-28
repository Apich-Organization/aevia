//! Source calls distinguish live signature mismatches from stale keys.

use aevia_core::Interp;
use aevia_core::Mode;
use aevia_core::value::Value;

#[test]
fn explicit_pinned_calls_report_the_failing_signature_in_both_backends() {
    let source = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/keyed_diagnostics.aev");
    let program = aevia_core::driver::load(&source);
    assert!(
        !program.diagnostics.has_errors(),
        "{:?}",
        program.diagnostics.items
    );
    for mode in [Mode::Ast, Mode::Ir] {
        let mut interp = Interp::new(&program.entry().unwrap().checked).with_mode(mode);
        let diagnostics = aevia_core::driver::run_modules(&mut interp, &program);
        assert!(
            !diagnostics.has_errors(),
            "{mode:?}: {:?}",
            diagnostics.items
        );
        for (name, code, fragments) in [
            ("bad_type", "E0302", &["argument 1", "Bool", "Int"][..]),
            ("bad_arity", "E1003", &["takes", "2", "got 1"][..]),
            ("stale_key", "E1005", &["no overload"][..]),
            ("missing_name", "E1005", &["no overload"][..]),
            (
                "bad_result_dimension",
                "E0401",
                &["argument 7", "result", "Time"][..],
            ),
            ("bad_parameter_rank", "E0302", &["argument 7"][..]),
            ("bad_result_kind", "E0302", &["argument 7"][..]),
            ("stale_callback_key", "E1005", &["no overload"][..]),
        ] {
            let Value::Variant(result) = interp.eval_toplevel(name).unwrap() else {
                panic!("{mode:?} {name}: expected Result");
            };
            assert_eq!(result.index, 1, "{mode:?} {name}: {:?}", result.fields);
            let [Value::KernelError(error)] = result.fields.as_slice() else {
                panic!("{mode:?} {name}: expected OpError");
            };
            assert_eq!(error.code, code, "{mode:?} {name}: {}", error.message);
            assert!(
                fragments.iter().all(|word| error.message.contains(word)),
                "{mode:?} {name}: {}",
                error.message,
            );
        }
    }
}
