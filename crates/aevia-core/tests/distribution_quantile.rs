//! Distribution quantile semantics at the Aevia AST and IR entry points.

use aevia_core::Interp;
use aevia_core::Mode;
use aevia_core::compile;
use aevia_core::driver;
use aevia_core::value::NumVal;
use aevia_core::value::Value;
use aevia_dim::Dim;
use aevia_syntax::Span;
use std::path::Path;

#[test]
fn distribution_quantiles_keep_units_support_and_extreme_scales() {
    let source = r#"import Stats.Distributions { Normal, LogNormal, Binomial, dist_quantile }
let normal = Normal({ mu = 4.4942328371557893e307 m, sigma = 1.7976931348623157e308 m })
let lognormal = LogNormal({ mu = 0.6931471805599453, sigma = 1.7976931348623157e308 })
let middle = dist_quantile(normal, 0.5)
let near_middle = dist_quantile(normal, 0.51)
let lower = dist_quantile(normal, 0.0)
let upper = dist_quantile(normal, 1.0)
let logmiddle = dist_quantile(lognormal, 0.5)
let loglower = dist_quantile(lognormal, 0.0)
let logupper = dist_quantile(lognormal, 1.0)
let discrete = dist_quantile(Binomial({ n = 6, p = 0.25 }), 0.5)
"#;
    let program = driver::load_with_source(Path::new("distribution_quantile.aev"), source);
    assert!(
        !program.diagnostics.has_errors(),
        "{:?}",
        program.diagnostics
    );
    for mode in [Mode::Ast, Mode::Ir] {
        let mut interp = Interp::new(&program.entry().unwrap().checked).with_mode(mode);
        let loads = driver::run_modules(&mut interp, &program);
        assert!(!loads.has_errors(), "{mode:?} {loads:?}");
        let length = Dim::base("Length");
        let one = Dim::one();
        for (name, expected, dimension) in [
            ("middle", f64::MAX / 4.0, &length),
            (
                "near_middle",
                f64::from_bits(0x7fd1_9aba_9f47_8691),
                &length,
            ),
            ("lower", f64::NEG_INFINITY, &length),
            ("upper", f64::INFINITY, &length),
            ("logmiddle", 2.0, &one),
            ("loglower", 0.0, &one),
            ("logupper", f64::INFINITY, &one),
            ("discrete", 1.0, &one),
        ] {
            let Value::Num(value) = interp.eval_toplevel(name).unwrap() else {
                panic!("{mode:?} {name} must be numeric");
            };
            assert_eq!(&value.dim, dimension, "{mode:?} {name}");
            assert!(
                value
                    .to_f64()
                    .unwrap()
                    .to_bits()
                    .abs_diff(expected.to_bits())
                    <= 3,
                "{mode:?} {name}: {value:?}, expected {expected}"
            );
        }
    }
}

#[test]
fn distribution_quantile_rejects_invalid_probabilities_and_parameters() {
    let source = r#"fn query(p: Real[1], sigma: Real[1]) = dist_quantile({ mu = 0.0, sigma = sigma }, p)
let normal = { mu = 0.0, sigma = 1.0 }
"#;
    let program = compile("distribution_probability.aev", source);
    assert!(
        !program.diagnostics.has_errors(),
        "{:?}",
        program.diagnostics
    );
    for mode in [Mode::Ast, Mode::Ir] {
        let mut interp = Interp::new(&program.checked).with_mode(mode);
        let loads = interp.load(program.module.as_ref().unwrap());
        assert!(!loads.has_errors(), "{mode:?} {loads:?}");
        let query = interp.eval_toplevel("query").unwrap();
        for (probability, sigma) in [(-0.01, 1.0), (1.01, 1.0), (f64::NAN, 1.0), (0.5, 0.0)] {
            let values = [probability, sigma]
                .map(|value| Value::Num(NumVal::float(value, Dim::one())))
                .to_vec();
            let error = interp
                .apply(query.clone(), values, Span::default())
                .unwrap_err();
            assert_eq!(error.code, "E0806", "{mode:?} {probability}, {sigma}");
        }
        let normal = interp.eval_toplevel("normal").unwrap();
        let dimensioned_probability = Value::Num(NumVal::float(0.5, Dim::base("Length")));
        let error = interp
            .call_named(
                "dist_quantile",
                vec![normal, dimensioned_probability],
                Span::default(),
            )
            .unwrap_err();
        assert_eq!(error.code, "E0401", "{mode:?}");
    }
    let wrong_unit = compile(
        "distribution_probability_dimension.aev",
        "let wrong = dist_quantile({ mu = 0.0, sigma = 1.0 }, 0.5 m)\n",
    );
    assert!(wrong_unit.diagnostics.has_errors());
}
