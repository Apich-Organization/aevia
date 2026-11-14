//! Runnable dimensional examples exercise adaptive PDE evolution and sparse spectral solves in both engines.
use aevia_core::Interp;
use aevia_core::Mode;
use aevia_core::driver;
use aevia_core::value::Value;
use aevia_syntax::DeclKind;

#[test]
fn adaptive_pde_examples_keep_dimensions_methods_rejection_and_sparse_contracts() {
    std::thread::Builder::new()
        .stack_size(256 * 1024 * 1024)
        .spawn(|| {
            for name in [
                "pde_adaptive_heat.aev",
                "pde_adaptive_wave_event.aev",
                "pde_adaptive_advanced.aev",
                "sparse_spectral_dimensions.aev",
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
