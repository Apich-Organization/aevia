//! Source-derived numerical codecs reach both interpreter engines through the
//! generated Aevia modules, including nested tensors, integer tuples and records.
use aevia_core::Interp;
use aevia_core::Mode;
use aevia_core::driver;
use aevia_core::value::Value;
use aevia_rssn::Val;
use aevia_rssn::bindings::Coverage;
use aevia_syntax::Span;

fn call(
    op: &str,
    module: &str,
    args: &[&str],
) -> (String, bool) {
    let binding = aevia_rssn::bindings::catalog(aevia_rssn::registry())
        .into_iter()
        .find(|row| row.op.name == op)
        .unwrap_or_else(|| panic!("source-derived export {op} is absent"));
    let dynamic = !matches!(binding.coverage, Coverage::Typed);
    let args = if dynamic {
        format!(
            "[{}]",
            args.iter()
                .map(|arg| format!("to_value({arg})"))
                .collect::<Vec<_>>()
                .join(", ")
        )
    } else {
        args.join(", ")
    };
    let expression = format!("{module}.{}({args})", binding.name);
    (
        if dynamic {
            expression
        } else {
            format!("to_value({expression})")
        },
        dynamic,
    )
}

fn payload(
    value: Value,
    dynamic: bool,
) -> Val {
    let value = if dynamic {
        let Value::Variant(result) = value else {
            panic!("expected dynamic Result")
        };
        assert_eq!(
            result.index, 0,
            "unexpected kernel error: {:?}",
            result.fields
        );
        result.fields[0].clone()
    } else {
        value
    };
    let Value::KernelValue(value) = value else {
        panic!("expected encoded kernel value, got {value:?}")
    };
    value.payload.clone()
}

fn scalar(value: &Val) -> f64 {
    match value {
        | Val::Num(x) => x.to_f64(),
        | other => panic!("expected numerical scalar, got {other:?}"),
    }
}

fn field<'a>(
    record: &'a [(String, Val)],
    name: &str,
) -> &'a Val {
    &record
        .iter()
        .find(|(key, _)| key == name)
        .unwrap_or_else(|| panic!("missing field {name}"))
        .1
}

#[test]
fn source_generated_geometry_statistics_numbers_and_specials_run_in_both_engines() {
    let (hull, hull_dynamic) = call(
        "num_geometry_convex_hull_2d",
        "G",
        &["[[0.0, 0.0], [2.0, 0.0], [2.0, 2.0], [0.0, 2.0], [1.0, 1.0]]"],
    );
    let (stats, stats_dynamic) = call(
        "num_stats_ext_t_test_one_sample",
        "T",
        &["[1.0, 2.0, 3.0, 4.0, 5.0]", "3.0"],
    );
    let (crt, crt_dynamic) = call("num_number_ext_crt", "S", &["[2, 3, 2]", "[3, 5, 7]"]);
    let (lambert, lambert_dynamic) = call("num_special_lambert_w", "S", &["1.0", "true"]);
    let (inverse, inverse_dynamic) = call("num_number_ext_mod_inv", "S", &["6", "9"]);
    let (mvn, mvn_dynamic) = call(
        "num_distributions_multivariate_normal_pdf",
        "T",
        &["[0.0, 0.0]", "[[1.0, 0.0], [0.0, 1.0]]", "[0.0, 0.0]"],
    );
    let (dirichlet, dirichlet_dynamic) = call(
        "num_distributions_dirichlet_pdf",
        "T",
        &["[1.0, 1.0, 1.0]", "[0.2, 0.3, 0.5]"],
    );
    let (dirichlet_sample, dirichlet_sample_dynamic) = call(
        "num_distributions_dirichlet_sample",
        "T",
        &["[0.000001, 0.000001, 0.000001]", "42"],
    );
    let (invalid_carlson, invalid_carlson_dynamic) =
        call("num_special_ext_carlson_rf", "S", &["-1.0", "1.0", "1.0"]);
    let distributions = r#"
open Units.SI
open Stats.Distributions
let lognormal = cdf(LogNormal({ mu = 0.0, sigma = 1.0 }), 1.0)
let weibull = cdf(Weibull({ shape = 1.0, scale = 2.0 m }), 2.0 m)
let cauchy = pdf(Cauchy({ loc = 2.0 m, scale = 3.0 m }), 2.0 m)
let fisher = cdf(F({ df1 = 2.0, df2 = 2.0 }), 1.0)
let negative_binomial = pdf(NegativeBinomial({ r = 1.0, p = 0.3 }), 2.0)
let geometric = pdf(Geometric({ p = 0.3 }), 2.0)
let weibull_samples = sample_n(Weibull({ shape = 2.0, scale = 3.0 m }), 3, Some(7))
"#;
    let source = format!(
        "import Rssn_numerical_vector as G\nimport Rssn_numerical_stats as T\nimport Rssn_numerical_special as S\n{distributions}\nlet hull = {hull}\nlet stats = {stats}\nlet crt = {crt}\nlet lambert = {lambert}\nlet mvn = {mvn}\nlet dirichlet = {dirichlet}\nlet dirichlet_draw = {dirichlet_sample}\nlet dirichlet_repeat = {dirichlet_sample}\nfn invalid() = {inverse}\nfn invalid_carlson() = {invalid_carlson}\n"
    );
    let directory =
        std::env::temp_dir().join(format!("aevia-source-codecs-{}", std::process::id()));
    std::fs::create_dir_all(&directory).unwrap();
    let path = directory.join("main.aev");
    std::fs::write(&path, source).unwrap();
    let program = driver::load(&path);
    assert!(
        !program.diagnostics.has_errors(),
        "{:?}",
        program.diagnostics.items
    );
    for mode in [Mode::Ast, Mode::Ir] {
        let mut interp = Interp::new(&program.entry().unwrap().checked).with_mode(mode);
        let diagnostics = driver::run_modules(&mut interp, &program);
        assert!(
            !diagnostics.has_errors(),
            "{mode:?}: {:?}",
            diagnostics.items
        );
        let Val::Tensor { shape, data } =
            payload(interp.eval_toplevel("hull").unwrap(), hull_dynamic)
        else {
            panic!("expected hull tensor")
        };
        assert_eq!(shape, vec![4, 2]);
        assert_eq!(
            data.iter().map(scalar).collect::<Vec<_>>(),
            vec![0.0, 0.0, 2.0, 0.0, 2.0, 2.0, 0.0, 2.0]
        );
        let Val::Record(stats) = payload(interp.eval_toplevel("stats").unwrap(), stats_dynamic)
        else {
            panic!("expected test result record")
        };
        assert!(scalar(field(&stats, "statistic")).abs() < 1e-12);
        assert!((scalar(field(&stats, "p_value")) - 1.0).abs() < 1e-12);
        let Val::Record(crt) = payload(interp.eval_toplevel("crt").unwrap(), crt_dynamic) else {
            panic!("expected CRT tuple record")
        };
        assert_eq!(scalar(field(&crt, "item0")), 23.0);
        assert_eq!(scalar(field(&crt, "item1")), 105.0);
        let lambert = payload(interp.eval_toplevel("lambert").unwrap(), lambert_dynamic);
        assert!((scalar(&lambert) - 0.567_143_290_409_783_8).abs() < 1e-12);
        let mvn = payload(interp.eval_toplevel("mvn").unwrap(), mvn_dynamic);
        assert!((scalar(&mvn) - 1.0 / (2.0 * std::f64::consts::PI)).abs() < 1e-12);
        let dirichlet = payload(
            interp.eval_toplevel("dirichlet").unwrap(),
            dirichlet_dynamic,
        );
        assert!((scalar(&dirichlet) - 2.0).abs() < 1e-12);
        let draw = payload(
            interp.eval_toplevel("dirichlet_draw").unwrap(),
            dirichlet_sample_dynamic,
        );
        let repeat = payload(
            interp.eval_toplevel("dirichlet_repeat").unwrap(),
            dirichlet_sample_dynamic,
        );
        assert_eq!(draw, repeat);
        let Val::Tensor { shape, data } = draw else {
            panic!("expected Dirichlet sample vector")
        };
        assert_eq!(shape, vec![3]);
        assert!((data.iter().map(scalar).sum::<f64>() - 1.0).abs() < 1e-12);
        assert!(
            data.iter().map(scalar).any(|x| x > 0.99),
            "tiny concentration sample was incorrectly replaced by the uniform mean"
        );
        for (name, expected) in [
            ("lognormal", 0.5),
            ("weibull", 1.0 - (-1.0f64).exp()),
            ("cauchy", 1.0 / (3.0 * std::f64::consts::PI)),
            ("fisher", 0.5),
            ("negative_binomial", 0.147),
            ("geometric", 0.147),
        ] {
            let value = interp.eval_toplevel(name).unwrap();
            assert!(
                (value.as_f64().unwrap() - expected).abs() < 1e-12,
                "{name}: {value:?}"
            );
        }
        let cauchy = interp.eval_toplevel("cauchy").unwrap();
        assert_eq!(
            aevia_core::value::value_dim(&cauchy),
            aevia_dim::Dim::base("Length").inverse()
        );
        let samples = interp.eval_toplevel("weibull_samples").unwrap();
        assert_eq!(
            aevia_core::value::value_dim(&samples),
            aevia_dim::Dim::base("Length")
        );
        for (name, dynamic) in [
            ("invalid", inverse_dynamic),
            ("invalid_carlson", invalid_carlson_dynamic),
        ] {
            let invalid = interp.eval_toplevel(name).unwrap();
            let result = interp.apply(invalid, vec![], Span::default());
            if dynamic {
                let Value::Variant(result) = result.unwrap() else {
                    panic!("expected rejected kernel Result")
                };
                assert_eq!(result.index, 1, "{name} unexpectedly succeeded");
            } else {
                assert!(result.is_err(), "{name} unexpectedly succeeded in {mode:?}");
            }
        }
    }
    std::fs::remove_dir_all(directory).unwrap();
}

#[test]
fn every_reviewed_codec_export_is_declared_and_has_a_language_binding() {
    let policy = include_str!("../../../../rssn/tools/rssn-api-gen/codec-policy.tsv");
    let registry = aevia_rssn::registry();
    let bindings = aevia_rssn::bindings::catalog(registry);
    let mut count = 0;
    for row in policy
        .lines()
        .filter(|line| !line.is_empty() && !line.starts_with('#'))
    {
        let fields: Vec<_> = row.split('\t').collect();
        let op = registry
            .op(fields[1])
            .unwrap_or_else(|| panic!("missing {}", fields[0]));
        assert!(op.source.is_exact(), "{} is not declared", fields[1]);
        assert_eq!(op.rule_set, fields[2]);
        assert!(bindings.iter().any(|binding| {
            binding.op.name == fields[1] && !matches!(binding.coverage, Coverage::Unavailable(_))
        }));
        count += 1;
    }
    assert!(count >= 220, "expected the expanded kernel policy");
}

#[test]
fn tape_automatic_derivatives_run_in_both_engines() {
    let expression = "\"x^2*y + sin(y)\"";
    let names = "[\"x\", \"y\"]";
    let point = "[2.0, 0.5]";
    let expressions = "[\"x*y\", \"x^2+y\"]";
    let specs = [
        (
            "forward",
            "num_ad_forward_gradient",
            vec![expression, names, point],
            vec![2.0, 4.0 + 0.5_f64.cos()],
        ),
        (
            "reverse",
            "num_ad_reverse_gradient",
            vec![expression, names, point],
            vec![2.0, 4.0 + 0.5_f64.cos()],
        ),
        (
            "hessian",
            "num_ad_hessian",
            vec![expression, names, point],
            vec![1.0, 4.0, 4.0, -0.5_f64.sin()],
        ),
        (
            "jacobian",
            "num_ad_jacobian",
            vec![expressions, names, point],
            vec![0.5, 2.0, 4.0, 1.0],
        ),
        (
            "vjp",
            "num_ad_vjp",
            vec![expressions, names, point, "[2.0, 3.0]"],
            vec![13.0, 7.0],
        ),
    ];
    let mut source = String::from("import Rssn_numerical_vector as N\n");
    let mut assertions = Vec::new();
    for (name, op, args, expected) in specs {
        let (expression, dynamic) = call(op, "N", &args);
        source.push_str(&format!("let {name} = {expression}\n"));
        assertions.push((name, dynamic, expected));
    }
    let directory = std::env::temp_dir().join(format!("aevia-tape-ad-{}", std::process::id()));
    std::fs::create_dir_all(&directory).unwrap();
    let path = directory.join("main.aev");
    std::fs::write(&path, source).unwrap();
    let program = driver::load(&path);
    assert!(
        !program.diagnostics.has_errors(),
        "{:?}",
        program.diagnostics.items
    );
    for mode in [Mode::Ast, Mode::Ir] {
        let mut interp = Interp::new(&program.entry().unwrap().checked).with_mode(mode);
        let diagnostics = driver::run_modules(&mut interp, &program);
        assert!(!diagnostics.has_errors(), "{:?}", diagnostics.items);
        for (name, dynamic, expected) in &assertions {
            let Val::Tensor { data, .. } = payload(interp.eval_toplevel(name).unwrap(), *dynamic)
            else {
                panic!("expected derivative tensor")
            };
            assert_eq!(data.len(), expected.len());
            for (actual, expected) in data.iter().zip(expected) {
                assert!(
                    (scalar(actual) - expected).abs() < 1e-12,
                    "{name} in {mode:?}: {actual:?} != {expected}"
                );
            }
        }
    }
    std::fs::remove_dir_all(directory).unwrap();
}
