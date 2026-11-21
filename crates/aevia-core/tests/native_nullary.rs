//! Importing a numerical namespace must not execute its native operations.
use aevia_core::Interp;
use aevia_core::Mode;
use aevia_core::driver;
use aevia_core::value::Value;
use aevia_syntax::DeclKind;

#[test]
fn native_nullary_namespace_import_is_lazy_and_errors_use_result_in_both_engines() {
    std::thread::Builder::new()
        .stack_size(256 * 1024 * 1024)
        .spawn(|| {
            let path = std::env::temp_dir()
                .join(format!("aevia-native-nullary-{}.aev", std::process::id()));
            let source = r#"import Rssn_numerical_vector as Native
let probe = Native.num_native_jit_target_key()
test "native zero-argument call has a checked result channel" =
    match probe with
    | Ok(key) -> key >= 0
    | Err(_) -> true
"#;
            let program = driver::load_with_source(&path, source);
            assert!(
                !program.diagnostics.has_errors(),
                "{:?}",
                program.diagnostics.items
            );
            let entry = program.entry().unwrap();
            for mode in [Mode::Ast, Mode::Ir] {
                let mut interp = Interp::new(&entry.checked).with_mode(mode);
                let diagnostics = driver::run_modules(&mut interp, &program);
                assert!(
                    !diagnostics.has_errors(),
                    "{mode:?}: {:?}",
                    diagnostics.items
                );
                let root = interp.root();
                for declaration in &entry.module.decls {
                    if let DeclKind::Test { body, .. } = &declaration.kind {
                        assert!(
                            matches!(interp.eval_source_test(body, &root), Ok(Value::Bool(true))),
                            "{mode:?}"
                        );
                    }
                }
            }
        })
        .unwrap()
        .join()
        .unwrap();
}
