//! Runnable ODE, DAE and DDE examples must work through both evaluation engines.
use aevia_core::Interp;
use aevia_core::Mode;
use aevia_core::driver;
use aevia_core::value::Value;
use aevia_syntax::ast::DeclKind;
#[test]
fn adaptive_ode_dae_dde_examples_run_in_ast_and_ir() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    for name in [
        "ode_adaptive_geometric.aev",
        "ode_adaptive_dae.aev",
        "ode_adaptive_dde.aev",
    ] {
        let program = driver::load(&root.join("examples").join(name));
        assert!(
            !program.diagnostics.has_errors(),
            "{name}: {:?}",
            program.diagnostics.items
        );
        let entry = program.entry().expect("example entry");
        for mode in [Mode::Ast, Mode::Ir] {
            let mut interp = Interp::new(&entry.checked).with_mode(mode).with_buffer();
            let loaded = driver::run_modules(&mut interp, &program);
            assert!(!loaded.has_errors(), "{name}/{mode:?}: {:?}", loaded.items);
            let env = interp.root();
            let mut tested = 0;
            for decl in &entry.module.decls {
                if let DeclKind::Test { name: test, body } = &decl.kind {
                    let result = interp.eval_source_test(body, &env);
                    assert!(
                        matches!(result, Ok(Value::Bool(true))),
                        "{name}/{mode:?}/{test}: {result:?}"
                    );
                    tested += 1;
                }
            }
            assert!(tested > 0);
        }
    }
}

#[test]
fn physical_ode_atol_rejects_wrong_units_shape_and_nonpositive_components() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    for (atol, code) in [
        ("[1.0 s]", "E0401"),
        ("[0.0 m]", "E0802"),
        ("[1.0 m, 1.0 m]", "E0302"),
    ] {
        let source = format!(
            r#"
open Units.SI
import Ode {{ AdaptiveRk4, ode_solve }}
fn rate(_t: Float[Time], y: Tensor<[1], Float[Length]>): Tensor<[1], Float[Length/Time]> = -y / (1.0 s)
test "invalid atol" = ode_solve(rate, [1.0 m], (0.0 s, 1.0 s), {{ rtol = 0.000001, atol = {atol}, method = AdaptiveRk4, steps = None, events = None }}, []).nfev > 0
"#
        );
        let program =
            driver::load_with_source(&root.join("examples/ode_atol_invalid.aev"), &source);
        assert!(
            !program.diagnostics.has_errors(),
            "{atol}: {:?}",
            program.diagnostics.items
        );
        let entry = program.entry().expect("example entry");
        for mode in [Mode::Ast, Mode::Ir] {
            let mut interp = Interp::new(&entry.checked).with_mode(mode).with_buffer();
            assert!(!driver::run_modules(&mut interp, &program).has_errors());
            let env = interp.root();
            let body = entry
                .module
                .decls
                .iter()
                .find_map(|decl| {
                    match &decl.kind {
                        | DeclKind::Test { body, .. } => Some(body),
                        | _ => None,
                    }
                })
                .expect("invalid tolerance test");
            let error = interp
                .eval_source_test(body, &env)
                .expect_err("invalid physical tolerance must fail before a numerical solve");
            assert_eq!(error.code, code, "{atol}/{mode:?}: {error:?}");
        }
    }
}
