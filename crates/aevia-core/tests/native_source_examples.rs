//! Native-only source families execute through generated, dimensioned adapters.
use aevia_core::Interp;
use aevia_core::Mode;
use aevia_core::driver;
use aevia_core::value::Value;
use aevia_syntax::DeclKind;

fn assert_native_example(
    name: &'static str,
    expected_cases: usize,
) {
    std::thread::Builder::new()
        .stack_size(256 * 1024 * 1024)
        .spawn(move || {
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
                    "{name}, {mode:?}: {:?}",
                    diagnostics.items
                );
                let root = interp.root();
                let mut count = 0;
                for declaration in &entry.module.decls {
                    if let DeclKind::Test { name: case, body } = &declaration.kind {
                        let outcome = interp.eval_source_test(body, &root);
                        assert!(
                            matches!(outcome, Ok(Value::Bool(true))),
                            "{name}, {mode:?}, {case}: {outcome:?}"
                        );
                        count += 1;
                    }
                }
                assert_eq!(count, expected_cases, "{name}, {mode:?}");
            }
        })
        .unwrap()
        .join()
        .unwrap();
}

#[test]
fn native_source_families_execute_in_ast_and_ir() {
    assert_native_example("native_kernel_families.aev", 37);
}

#[test]
fn native_discrete_objects_execute_in_ast_and_ir() {
    assert_native_example("native_discrete_objects.aev", 19);
}

#[test]
fn native_particle_engines_execute_in_ast_and_ir() {
    assert_native_example("native_particle_engines.aev", 20);
}

#[test]
fn native_continuum_models_execute_in_ast_and_ir() {
    assert_native_example("native_continuum_models.aev", 23);
}

#[test]
fn native_fvm_advection3_executes_in_ast_and_ir() {
    assert_native_example("native_fvm_advection3.aev", 2);
}

#[test]
fn native_epd_goursat_executes_in_ast_and_ir() {
    assert_native_example("native_epd_goursat.aev", 3);
}
