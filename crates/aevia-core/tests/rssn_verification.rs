//! Boolean symbolic verification through both Aevia evaluators.

use aevia_core::Interp;
use aevia_core::Mode;
use aevia_core::compile;
use aevia_core::value::Value;
use aevia_syntax::DeclKind;

#[test]
fn rssn_limit_claims_and_exact_derivative_residuals_cross_the_bridge() {
    let source = include_str!("../../../examples/rssn_verification.aev");
    let compilation = compile("rssn_verification.aev", source);
    assert!(
        !compilation.diagnostics.has_errors(),
        "{:?}",
        compilation.diagnostics.items
    );
    let module = compilation.module.unwrap();
    for mode in [Mode::Ast, Mode::Ir] {
        aevia_rssn::reset();
        let mut interp = Interp::new(&compilation.checked).with_mode(mode);
        let diagnostics = interp.load(&module);
        assert!(
            !diagnostics.has_errors(),
            "{mode:?}: {:?}",
            diagnostics.items
        );
        let root = interp.root();
        let mut assertions = 0;
        for declaration in &module.decls {
            if let DeclKind::Test { name, body } = &declaration.kind {
                let result = interp.eval_source_test(body, &root);
                assert!(
                    matches!(result, Ok(Value::Bool(true))),
                    "{mode:?} {name}: {result:?}"
                );
                assertions += 1;
            }
        }
        assert_eq!(assertions, 3);
    }
}
