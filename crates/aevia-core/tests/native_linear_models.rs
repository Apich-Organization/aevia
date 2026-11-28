//! Native generic and opaque algebraic objects run through real generated bindings.
use aevia_core::Interp;
use aevia_core::Mode;
use aevia_core::driver;
use aevia_core::value::Value;
use aevia_syntax::DeclKind;

#[test]
fn native_linear_and_mechanics_models_preserve_physical_reference_domains() {
    std::thread::Builder::new()
        .stack_size(256 * 1024 * 1024)
        .spawn(|| {
            for name in [
                "native_linear_tensors.aev",
                "native_mechanics.aev",
                "native_execution.aev",
                "native_flow_conservation.aev",
            ] {
                let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                    .join("../../examples")
                    .join(name);
                let program = driver::load(&path);
                assert!(
                    !program.diagnostics.has_errors(),
                    "{name}: {:?}",
                    program.diagnostics.items
                );
                let entry = program.entry().unwrap();
                for mode in [Mode::Ast, Mode::Ir] {
                    // Each fixture and evaluator has its own file-scope objects.
                    // Keep them alive across assertions but release them before the next run.
                    aevia_rssn::kernel()
                        .with_native_object_scope(|| {
                            let mut interp = Interp::new(&entry.checked).with_mode(mode);
                            let diagnostics = driver::run_modules(&mut interp, &program);
                            assert!(
                                !diagnostics.has_errors(),
                                "{name} {mode:?}: {:?}",
                                diagnostics.items
                            );
                            let root = interp.root();
                            let mut assertions = 0;
                            for declaration in &entry.module.decls {
                                if let DeclKind::Test {
                                    name: assertion,
                                    body,
                                } = &declaration.kind
                                {
                                    let outcome = interp.eval_source_test(body, &root);
                                    assert!(
                                        matches!(outcome, Ok(Value::Bool(true))),
                                        "{name} {mode:?}: {assertion}: {outcome:?}"
                                    );
                                    assertions += 1;
                                }
                            }
                            assert!(assertions > 0, "{name} has no assertions");
                        })
                        .expect("open native-object scope for the program");
                }
            }
        })
        .unwrap()
        .join()
        .unwrap();
}
