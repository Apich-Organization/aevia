//! Runnable dimensional examples exercise adaptive stochastic paths in both engines.
use aevia_core::Interp;
use aevia_core::Mode;
use aevia_core::driver;
use aevia_core::value::Value;
use aevia_syntax::DeclKind;
use aevia_syntax::Severity;
use aevia_syntax::Span;

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
            let invalid_arguments = "[1.0], drift, diffusion, jump)";
            let source = include_str!("../../../examples/sde_adaptive_jump.aev")
                .replace("[1.0, 1.0], drift, diffusion, jump)", invalid_arguments);
            let callback_start = source.find(invalid_arguments).expect("mutated jump call")
                + invalid_arguments.len()
                - "jump)".len();
            let invalid_path = std::env::temp_dir()
                .join(format!("aevia-sde-mark-extent-{}.aev", std::process::id()));
            std::fs::write(&invalid_path, &source).unwrap();
            let invalid = driver::load(&invalid_path);
            std::fs::remove_file(&invalid_path).unwrap();
            let mut errors = invalid
                .diagnostics
                .items
                .iter()
                .filter(|item| item.severity == Severity::Error);
            let diagnostic = errors.next().expect("mark extent mismatch");
            assert!(errors.next().is_none(), "{:?}", invalid.diagnostics.items);
            assert_eq!(diagnostic.code, "E0302");
            assert_eq!(
                diagnostic.message,
                "type-level equation has no solution: 2 = 1"
            );
            let diagnostic_source = invalid
                .sources
                .file(diagnostic.span.file)
                .expect("diagnostic source");
            assert_eq!(diagnostic_source.text, source);
            // The conflicting extent is detected while checking the `jump` callback.
            assert_eq!(
                diagnostic.span,
                Span::new(
                    diagnostic.span.file,
                    callback_start,
                    callback_start + "jump".len()
                )
            );
        })
        .unwrap()
        .join()
        .unwrap();
}
