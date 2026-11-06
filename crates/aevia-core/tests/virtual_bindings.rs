//! End-to-end calls through manifest-generated, virtual RSSN namespaces.
use aevia_core::Interp;
use aevia_core::driver;
use aevia_core::value::Value;
use aevia_rssn::bindings::Coverage;
use aevia_rssn::bindings::catalog;

#[test]
fn dynamic_symbolic_binding_imports_and_runs() {
    let registry = aevia_rssn::registry();
    let row = catalog(registry)
        .into_iter()
        .find(|row| row.op.name == "sin" && matches!(row.coverage, Coverage::Dynamic(_)))
        .expect("sin dynamic binding");
    let module =
        aevia_rssn::codegen::namespace_names_for(registry.ops().map(|op| op.rule_set.as_str()))
            .into_iter()
            .find(|(_, group)| group == &row.op.rule_set)
            .unwrap()
            .0;
    let source = format!(
        "import {module} as D\nlet r = D.{}([to_value(symbol(\"x\"))])\n",
        row.name
    );
    let dir = std::env::temp_dir().join(format!("aevia-virtual-symbolic-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let entry = dir.join("main.aev");
    std::fs::write(&entry, source).unwrap();
    let program = driver::load(&entry);
    assert!(
        !program.diagnostics.has_errors(),
        "{:?}",
        program.diagnostics.items
    );
    for mode in [aevia_core::eval::Mode::Ast, aevia_core::eval::Mode::Ir] {
        let mut interp = Interp::new(&program.entry().unwrap().checked).with_mode(mode);
        let diagnostics = driver::run_modules(&mut interp, &program);
        assert!(
            !diagnostics.has_errors(),
            "{mode:?}: {:?}",
            diagnostics.items
        );
        let result = interp.eval_toplevel("r").expect("result");
        let Value::Variant(result) = result else {
            panic!("expected Result");
        };
        assert_eq!(
            result.index, 0,
            "{mode:?}: symbolic call failed: {:?}",
            result.fields
        );
        assert!(
            matches!(result.fields.first(), Some(Value::KernelValue(value)) if matches!(value.payload, aevia_rssn::Val::Term(_) | aevia_rssn::Val::Symbol(_))),
            "{mode:?}: symbolic result lost: {:?}",
            result.fields
        );
    }
    let _ = std::fs::remove_dir_all(dir);
}
