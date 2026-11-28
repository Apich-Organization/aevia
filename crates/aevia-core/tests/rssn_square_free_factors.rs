//! Structured symbolic square-free factors through both Aevia evaluators.

use aevia_core::Interp;
use aevia_core::Mode;
use aevia_core::compile;
use aevia_core::value::Value;
use aevia_rssn::Val;

#[test]
fn square_free_factor_records_cross_the_bridge() {
    let source = include_str!("../../../examples/rssn_square_free_factors.aev");
    let compiled = compile("rssn_square_free_factors.aev", source);
    assert!(
        !compiled.diagnostics.has_errors(),
        "{:?}",
        compiled.diagnostics.items
    );
    let module = compiled.module.unwrap();
    for mode in [Mode::Ast, Mode::Ir] {
        aevia_rssn::reset();
        let mut interp = Interp::new(&compiled.checked).with_mode(mode);
        let diagnostics = interp.load(&module);
        assert!(
            !diagnostics.has_errors(),
            "{mode:?}: {:?}",
            diagnostics.items
        );
        for (binding, expected) in [
            (
                "decomposition",
                "list(-6, list(list(x, 2), list(x^2 + 1, 3)))",
            ),
            ("constant", "list(7/3, list())"),
        ] {
            let Value::Variant(result) = interp.eval_toplevel(binding).unwrap() else {
                panic!("{mode:?} {binding}: expected Result");
            };
            assert_eq!(result.index, 0, "{mode:?} {binding}: {:?}", result.fields);
            let Some(Value::KernelValue(value)) = result.fields.first() else {
                panic!("{mode:?} {binding}: expected kernel value");
            };
            let Val::Term(handle) = &value.payload else {
                panic!("{mode:?} {binding}: {:?}", value.payload);
            };
            assert_eq!(
                aevia_rssn::kernel().text(*handle),
                expected,
                "{mode:?} {binding}"
            );
        }
        for binding in ["composite_variable", "measured_polynomial"] {
            let Value::Variant(result) = interp.eval_toplevel(binding).unwrap() else {
                panic!("{mode:?} {binding}: expected Result");
            };
            assert_eq!(result.index, 1, "{mode:?} {binding}: {:?}", result.fields);
        }
    }
}
