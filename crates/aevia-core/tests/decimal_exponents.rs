//! Decimal unit exponents are exact notation, independent of evaluation mode.
use aevia_core::Interp;
use aevia_core::Mode;
use aevia_core::compile;
use aevia_core::value::Value;
use aevia_core::value::show;

#[test]
fn decimal_unit_notation_matches_fraction_notation_in_every_evaluator() {
    let source = r#"
dimension Fractal = L^1.5
unit root_m : L^0.5 = 1 m^(1/2)
let decimal: Float[L^0.75] = sqrt(17.0 m^1.5)
let fraction: Float[L^(3/4)] = sqrt(17.0 m^(3/2))
let equal = decimal == fraction
let exact: Real[L] = (1 m^0.1)^10
let scaled = 4 cm^0.5
let same_scaled = scaled == 4 cm^(1/2)
let custom: Real[L^(1/2)] = 3 root_m
"#;
    let compilation = compile("decimal.aev", source);
    assert!(
        !compilation.diagnostics.has_errors(),
        "{:?}",
        compilation.diagnostics.items
    );
    for (mode, optimized) in [(Mode::Ast, false), (Mode::Ir, false), (Mode::Ir, true)] {
        let mut interp = Interp::new(&compilation.checked).with_mode(mode);
        interp.optimize_ir = optimized;
        let diagnostics = interp.load(compilation.module.as_ref().unwrap());
        assert!(!diagnostics.has_errors(), "{:?}", diagnostics.items);
        for name in ["equal", "same_scaled"] {
            assert!(matches!(
                interp.eval_toplevel(name).unwrap(),
                Value::Bool(true)
            ));
        }
        assert_eq!(show(&interp.eval_toplevel("exact").unwrap()), "1 m");
        for (name, expected) in [
            ("scaled", aevia_dim::rat::ratio(2, 5)),
            ("custom", aevia_dim::rat::ratio(3, 1)),
        ] {
            let Value::Num(number) = interp.eval_toplevel(name).unwrap() else {
                panic!("expected scalar");
            };
            assert_eq!(number.to_rational(), Some(expected));
            assert_eq!(
                number.dim,
                aevia_dim::Dim::base("Length").pow(&aevia_dim::rat::ratio(1, 2))
            );
        }
        assert!(show(&interp.eval_toplevel("decimal").unwrap()).ends_with("m^(3/4)"));
    }
}
