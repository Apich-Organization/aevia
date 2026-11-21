//! Seeded resampling preserves an independently dimensioned callback statistic.
use aevia_core::Interp;
use aevia_core::Mode;
use aevia_core::driver;
use aevia_core::value::Value;
use aevia_syntax::DeclKind;

#[test]
fn resampling_example_checks_seeded_realization_statistic_units_and_budgets() {
    std::thread::Builder::new()
        .stack_size(256 * 1024 * 1024)
        .spawn(|| {
            let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("../../examples/resampling.aev");
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
                        let outcome = interp.eval_source_test(body, &root);
                        assert!(
                            matches!(outcome, Ok(Value::Bool(true))),
                            "{mode:?}: {name}: {outcome:?}"
                        );
                        count += 1;
                    }
                }
                assert_eq!(count, 3);
            }
        })
        .unwrap()
        .join()
        .unwrap();
}
