//! Native generic and opaque algebraic objects run through real generated bindings.
use aevia_core::Interp;
use aevia_core::Mode;
use aevia_core::driver;
use aevia_core::value::Value;
use aevia_syntax::DeclKind;

#[test]
fn native_backend_handles_preserve_compiled_programs_and_mixed_units() {
    std::thread::Builder::new()
        .stack_size(256 * 1024 * 1024)
        .spawn(|| {
            for name in ["native_backend.aev"] {
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
                            let outcome = interp.eval(body, &root);
                            assert!(
                                matches!(outcome, Ok(Value::Bool(true))),
                                "{name} {mode:?}: {assertion}: {outcome:?}"
                            );
                            assertions += 1;
                        }
                    }
                    assert!(assertions > 0, "{name} has no assertions");
                }
            }
        })
        .unwrap()
        .join()
        .unwrap();
}
