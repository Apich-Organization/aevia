//! Runnable dimensional examples exercise adaptive stochastic paths in both engines.
use aevia_core::Interp;
use aevia_core::Mode;
use aevia_core::driver;
use aevia_core::value::Value;
use aevia_syntax::DeclKind;

#[test]
fn adaptive_sde_examples_keep_dimensions_rejection_and_reproducible_noise() {
    std::thread::Builder::new()
        .stack_size(256 * 1024 * 1024)
        .spawn(|| {
            for name in [
                "sde_adaptive_implicit.aev",
                "sde_adaptive_jump.aev",
                "sde_adaptive_levy.aev",
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
                            let outcome = interp.eval_source_test(body, &root);
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
            // The generated jump facade binds mark extent independently from
            // the state, so a mismatched exact mark callback fails in checking.
            let source = include_str!("../../../examples/sde_adaptive_jump.aev").replace(
                "[1.0, 1.0], drift, diffusion, jump)",
                "[1.0], drift, diffusion, jump)",
            );
            let invalid_path = std::env::temp_dir()
                .join(format!("aevia-sde-mark-extent-{}.aev", std::process::id()));
            std::fs::write(&invalid_path, source).unwrap();
            let invalid = driver::load(&invalid_path);
            std::fs::remove_file(&invalid_path).unwrap();
            assert!(
                invalid.diagnostics.has_errors(),
                "a one-element mark must reject a two-element callback domain"
            );
            assert!(
                invalid
                    .diagnostics
                    .items
                    .iter()
                    .any(|item| item.message.contains("shape")),
                "expected a mark shape diagnostic: {:?}",
                invalid.diagnostics.items
            );
        })
        .unwrap()
        .join()
        .unwrap();
}
