//! Native-only source families execute through generated, dimensioned adapters.
use aevia_core::Interp;
use aevia_core::Mode;
use aevia_core::driver;
use aevia_core::value::Value;
use aevia_syntax::DeclKind;

#[test]
fn native_source_families_execute_in_ast_and_ir() {
    std::thread::Builder::new()
        .stack_size(256 * 1024 * 1024)
        .spawn(|| {
            let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("../../examples/native_kernel_families.aev");
            let program = driver::load(&path);
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
                let mut count = 0;
                for declaration in &entry.module.decls {
                    if let DeclKind::Test { name, body } = &declaration.kind {
                        let outcome = interp.eval(body, &root);
                        assert!(
                            matches!(outcome, Ok(Value::Bool(true))),
                            "{mode:?}: {name}: {outcome:?}"
                        );
                        count += 1;
                    }
                }
                assert_eq!(count, 17);
            }
        })
        .unwrap()
        .join()
        .unwrap();
}

#[test]
fn native_discrete_objects_execute_in_ast_and_ir() {
    std::thread::Builder::new()
        .stack_size(256 * 1024 * 1024)
        .spawn(|| {
            let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("../../examples/native_discrete_objects.aev");
            let program = driver::load(&path);
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
                let mut count = 0;
                for declaration in &entry.module.decls {
                    if let DeclKind::Test { name, body } = &declaration.kind {
                        let outcome = interp.eval(body, &root);
                        assert!(
                            matches!(outcome, Ok(Value::Bool(true))),
                            "{mode:?}: {name}: {outcome:?}"
                        );
                        count += 1;
                    }
                }
                assert_eq!(count, 5);
            }
        })
        .unwrap()
        .join()
        .unwrap();
}

#[test]
fn native_particle_engines_execute_in_ast_and_ir() {
    std::thread::Builder::new()
        .stack_size(256 * 1024 * 1024)
        .spawn(|| {
            let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("../../examples/native_particle_engines.aev");
            let program = driver::load(&path);
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
                let mut count = 0;
                for declaration in &entry.module.decls {
                    if let DeclKind::Test { name, body } = &declaration.kind {
                        let outcome = interp.eval(body, &root);
                        assert!(
                            matches!(outcome, Ok(Value::Bool(true))),
                            "{mode:?}: {name}: {outcome:?}"
                        );
                        count += 1;
                    }
                }
                assert_eq!(count, 5);
            }
        })
        .unwrap()
        .join()
        .unwrap();
}

#[test]
fn native_continuum_models_execute_in_ast_and_ir() {
    std::thread::Builder::new()
        .stack_size(256 * 1024 * 1024)
        .spawn(|| {
            let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("../../examples/native_continuum_models.aev");
            let program = driver::load(&path);
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
                let mut count = 0;
                for declaration in &entry.module.decls {
                    if let DeclKind::Test { name, body } = &declaration.kind {
                        let outcome = interp.eval(body, &root);
                        assert!(
                            matches!(outcome, Ok(Value::Bool(true))),
                            "{mode:?}: {name}: {outcome:?}"
                        );
                        count += 1;
                    }
                }
                assert_eq!(count, 5);
            }
        })
        .unwrap()
        .join()
        .unwrap();
}
