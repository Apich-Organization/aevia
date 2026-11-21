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
fn block_sparse_dense_product_keeps_shape_and_units_in_both_engines() {
    use aevia_dim::Dim;
    use aevia_rssn::witness::DimensionWitness;

    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../examples/native_linear_tensors.aev");
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
        let Value::Variant(result) = interp.eval_toplevel("packed_product").unwrap() else {
            panic!("expected a checked native result");
        };
        assert_eq!(result.index, 0, "kernel error: {:?}", result.fields);
        let [Value::KernelValue(packed)] = result.fields.as_slice() else {
            panic!("expected a kernel payload");
        };
        let Val::Record(product) = &packed.payload else {
            panic!("expected the packed block sparse result");
        };
        let shape = field(product, "shape").as_list().unwrap();
        assert_eq!(shape.iter().map(scalar).collect::<Vec<_>>(), [2.0, 3.0]);
        let data = field(product, "data").as_list().unwrap();
        assert_eq!(
            data.iter().map(scalar).collect::<Vec<_>>(),
            [14.0, 19.0, 24.0, 16.0, 20.0, 24.0]
        );
        assert_eq!(scalar(field(product, "reference")), 6.0);
        let DimensionWitness::Record(fields) = &packed.witness else {
            panic!("expected dimensional evidence for the output record");
        };
        let dimension = DimensionWitness::Scalar(Dim::base("Length").mul(&Dim::base("Time")));
        assert_eq!(
            fields
                .iter()
                .find(|(name, _)| name == "reference")
                .map(|(_, witness)| witness),
            Some(&dimension)
        );

        let Val::Record(solved) = payload(interp.eval_toplevel("solved").unwrap(), true) else {
            panic!("expected a sparse solve result");
        };
        assert_eq!(field(&solved, "algorithm"), &Val::Str("Cholesky".into()));
        assert!(matches!(field(&solved, "solution"), Val::Opaque { .. }));
    }
}

#[test]
fn batched_interpolation_and_frequency_covariance_run_in_both_engines() {
    let points = &["[0.0, 1.0, 2.0]", "[1.0, 3.0, 5.0]", "[1.5, 0.25, 2.0]"];
    let (pchip, pchip_dynamic) = call("num_interp_ext_pchip_many", "R", points);
    let (akima, akima_dynamic) = call("num_interp_ext_akima_many", "R", points);
    let covariance = r"
let cov = unwrap_or(R.num_native_online_covariance_new([to_value(2), to_value(1.0 m)]), to_value(0))
let weighted = R.num_native_online_covariance_push_frequency_batch([cov, to_value([[1.0 m, 3.0 m], [5.0 m, 11.0 m]]), to_value([3, 1])])
let observations = R.num_native_online_covariance_count([cov])
";
    let source = format!(
        "open Units.SI\nimport Rssn_numerical_vector as R\nlet pchip = {pchip}\nlet akima = {akima}\n{covariance}"
    );
    let dir = std::env::temp_dir().join(format!("aevia-batched-numerics-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("main.aev");
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
        for (name, dynamic) in [("pchip", pchip_dynamic), ("akima", akima_dynamic)] {
            let samples = payload(interp.eval_toplevel(name).unwrap(), dynamic);
            let samples = samples.as_list().unwrap();
            assert_eq!(samples.len(), 3);
            for (actual, expected) in samples.iter().map(scalar).zip([4.0, 1.5, 5.0]) {
                assert!((actual - expected).abs() < 1e-14);
            }
        }
        assert!(matches!(
            payload(interp.eval_toplevel("weighted").unwrap(), true),
            Val::Bool(true)
        ));
        assert_eq!(
            scalar(&payload(
                interp.eval_toplevel("observations").unwrap(),
                true
            )),
            4.0
        );
    }
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn sampled_implicit_sde_reaches_both_engines() {
    let source = r#"
import Rssn_numerical_vector as N

let no_names: Tensor<1, String> = []
let no_values: Tensor<1, Value> = []
let reference = to_value(1.0)

fn coefficient(expression: String): Value =
    unwrap_or(N.num_native_tape_lower([to_value([expression]), to_value(["t", "x"]), to_value(no_names), to_value([reference, reference]), to_value(no_values), to_value([reference])]), to_value(0))

let drift = unwrap_or(N.num_analysis_ode_rhs_new([coefficient("1"), to_value(no_values)]), to_value(0))
let model = unwrap_or(N.num_analysis_sde_model_new([drift, coefficient("0"), to_value(no_values), to_value(1)]), to_value(0))
let rng = unwrap_or(N.num_native_rng_new([to_value(42)]), to_value(0))
let noise = unwrap_or(N.num_analysis_sde_sample_noise([rng, to_value(1), to_value(5), to_value(0.25), to_value(2.0), to_value(2)]), to_value(0))
let states = N.num_analysis_sde_drift_implicit_sampled([model, noise, to_value([to_value(3.0)]), to_value(0.0), to_value("BackwardEuler"), to_value(1.0), to_value(2), to_value(200)])
"#;
    let directory = std::env::temp_dir().join(format!("aevia-sampled-sde-{}", std::process::id()));
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
        let states = payload(interp.eval_toplevel("states").unwrap(), true);
        let states = states.as_list().unwrap();
        assert_eq!(states.len(), 4);
        for (state, expected) in states.iter().zip([3.0, 3.5, 4.0, 4.25]) {
            let [coordinate] = state.as_list().unwrap() else {
                panic!("expected a scalar state")
            };
            assert_eq!(scalar(coordinate), expected);
        }
    }
    std::fs::remove_dir_all(directory).unwrap();
}

#[test]
fn signal_wrappers_agree_in_both_engines() {
    let samples = &["[1.0, 0.0, -1.0, 0.0]"];
    let (hilbert, hilbert_dynamic) = call("num_signal_hilbert", "S", samples);
    let (envelope, envelope_dynamic) = call("num_signal_analytic_envelope", "S", samples);
    let lag_arguments = &["[1.0 m, 0.0 m, -1.0 m, 0.0 m]", "2"];
    let (covariance, covariance_dynamic) = call("num_signal_autocovariance", "S", lag_arguments);
    let (correlation, correlation_dynamic) = call("num_signal_autocorrelation", "S", lag_arguments);
    let (partial, partial_dynamic) = call("num_signal_partial_autocorrelation", "S", lag_arguments);
    let source = format!(
        "open Units.SI\nimport Rssn_numerical_vector as S\nlet hilbert = {hilbert}\nlet envelope = {envelope}\nlet covariance = {covariance}\nlet correlation = {correlation}\nlet partial = {partial}\n"
    );
    let dir = std::env::temp_dir().join(format!("aevia-signal-codecs-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("main.aev");
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
        for (name, dynamic, expected) in [
            ("hilbert", hilbert_dynamic, &[0.0, 1.0, 0.0, -1.0][..]),
            ("envelope", envelope_dynamic, &[1.0; 4][..]),
            ("covariance", covariance_dynamic, &[0.5, 0.0, -0.25][..]),
            ("correlation", correlation_dynamic, &[1.0, 0.0, -0.5][..]),
            ("partial", partial_dynamic, &[1.0, 0.0, -0.5][..]),
        ] {
            let Val::Tensor { shape, data } = payload(interp.eval_toplevel(name).unwrap(), dynamic)
            else {
                panic!("expected a signal tensor");
            };
            assert_eq!(shape, [expected.len()]);
            for (value, expected) in data.iter().map(scalar).zip(expected) {
                assert!((value - expected).abs() < 1e-12, "{name}: {value}");
            }
        }
    }
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn graph_cuts_and_sparse_ordering_metrics_are_callable_in_both_engines() {
    let (cuts, dynamic) = call(
        "num_native_graph_undirected_cuts",
        "G",
        &["4", "[[0, 1], [1, 2], [2, 0], [2, 3]]"],
    );
    let (metrics, metrics_dynamic) = call(
        "num_native_ordering_metrics",
        "G",
        &["[0, 1, 3, 4, 4]", "[1, 0, 2, 1]", "[0, 2, 1, 3]"],
    );
    let directory = std::env::temp_dir().join(format!("aevia-graph-codecs-{}", std::process::id()));
    std::fs::create_dir_all(&directory).unwrap();
    let path = directory.join("main.aev");
    std::fs::write(
        &path,
        format!("import Rssn_numerical_vector as G\nlet cuts = {cuts}\nlet metrics = {metrics}\n"),
    )
    .unwrap();
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
        let Val::Record(record) = payload(interp.eval_toplevel("cuts").unwrap(), dynamic) else {
            panic!("expected a graph cuts record");
        };
        let Val::Tensor { shape, data } = field(&record, "bridges") else {
            panic!("expected the bridge endpoint matrix");
        };
        assert_eq!(shape, &[1, 2]);
        assert_eq!(data.iter().map(scalar).collect::<Vec<_>>(), [2.0, 3.0]);
        let Val::Tensor { shape, data } = field(&record, "articulation_points") else {
            panic!("expected an articulation vertex vector");
        };
        assert_eq!(shape, &[1]);
        assert_eq!(data.iter().map(scalar).collect::<Vec<_>>(), [2.0]);
        let Val::Record(record) =
            payload(interp.eval_toplevel("metrics").unwrap(), metrics_dynamic)
        else {
            panic!("expected an ordering metrics record");
        };
        assert_eq!(scalar(field(&record, "half_bandwidth")), 2.0);
        assert_eq!(scalar(field(&record, "profile")), 2.0);
    }
    std::fs::remove_dir_all(directory).unwrap();
}

#[test]
fn elliptic_integral_and_batched_jacobi_bindings_run_in_both_engines() {
    let (rg, rg_dynamic) = call("num_special_ext_carlson_rg", "S", &["4.0", "4.0", "4.0"]);
    let (amplitude, amplitude_dynamic) =
        call("num_special_ext_jacobi_amplitude", "S", &["-2.0", "0.0"]);
    let (batch, batch_dynamic) = call(
        "num_special_ext_jacobi_elliptic_batch",
        "S",
        &["[0.0, 1.0]", "0.0"],
    );
    let directory =
        std::env::temp_dir().join(format!("aevia-elliptic-codecs-{}", std::process::id()));
    std::fs::create_dir_all(&directory).unwrap();
    let path = directory.join("main.aev");
    std::fs::write(
        &path,
        format!("import Rssn_numerical_special as S\nlet rg = {rg}\nlet amplitude = {amplitude}\nlet batch = {batch}\n"),
    )
    .unwrap();
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
        let rg = payload(interp.eval_toplevel("rg").unwrap(), rg_dynamic);
        let amplitude = payload(
            interp.eval_toplevel("amplitude").unwrap(),
            amplitude_dynamic,
        );
        assert!((scalar(&rg) - 2.0).abs() < 1e-12);
        assert_eq!(scalar(&amplitude), -2.0);
        let Val::Tensor { shape, data } =
            payload(interp.eval_toplevel("batch").unwrap(), batch_dynamic)
        else {
            panic!("expected a matrix of Jacobi values");
        };
        assert_eq!(shape, [2, 3]);
        let (sin, cos) = 1.0_f64.sin_cos();
        for (actual, expected) in data.iter().map(scalar).zip([0.0, 1.0, 1.0, sin, cos, 1.0]) {
            assert!((actual - expected).abs() < 1e-12);
        }
    }
    std::fs::remove_dir_all(directory).unwrap();
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
    let (histogram, histogram_dynamic) = call(
        "num_descriptive_histogram_edges",
        "G",
        &["[0.0 m, 2.0 m, 5.0 m, 5.0 m]", "[-1.0 m, 2.0 m, 5.0 m]"],
    );
    let (power, power_dynamic) = call(
        "num_matfun_powm_psd",
        "M",
        &["[[0.0, 0.0], [0.0, 16.0]]", "0.5"],
    );
    let (levin, levin_dynamic) = call(
        "num_analysis_convergence_levin_condition_numbers",
        "G",
        &["[1.0 m, -0.5 m, 0.25 m, -0.125 m]", "\"U\"", "1.0"],
    );
    let (crt, crt_dynamic) = call("num_number_ext_crt", "S", &["[2, 3, 2]", "[3, 5, 7]"]);
    let (mobius, mobius_dynamic) = call("num_number_ext_mobius_sieve", "S", &["6"]);
    let (congruence, congruence_dynamic) = call(
        "num_number_ext_solve_linear_congruence",
        "S",
        &["14", "30", "100"],
    );
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
        "import Rssn_numerical_vector as G\nimport Rssn_numerical_stats as T\nimport Rssn_numerical_special as S\nimport Rssn_numerical_matrix as M\n{distributions}\nlet hull = {hull}\nlet stats = {stats}\nlet histogram = {histogram}\nlet power = {power}\nlet levin = {levin}\nlet crt = {crt}\nlet mobius = {mobius}\nlet congruence = {congruence}\nlet lambert = {lambert}\nlet mvn = {mvn}\nlet dirichlet = {dirichlet}\nlet dirichlet_draw = {dirichlet_sample}\nlet dirichlet_repeat = {dirichlet_sample}\nfn invalid() = {inverse}\nfn invalid_carlson() = {invalid_carlson}\n"
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
        let Val::Tensor { shape, data } = payload(
            interp.eval_toplevel("histogram").unwrap(),
            histogram_dynamic,
        ) else {
            panic!("expected an integer count tensor");
        };
        assert_eq!(shape, [2]);
        assert_eq!(data.iter().map(scalar).collect::<Vec<_>>(), [1.0, 3.0]);
        let Val::Tensor { shape, data } =
            payload(interp.eval_toplevel("power").unwrap(), power_dynamic)
        else {
            panic!("expected a matrix power");
        };
        assert_eq!(shape, [2, 2]);
        assert_eq!(
            data.iter().map(scalar).collect::<Vec<_>>(),
            [0.0, 0.0, 0.0, 4.0]
        );
        let Val::Tensor { shape, data } =
            payload(interp.eval_toplevel("levin").unwrap(), levin_dynamic)
        else {
            panic!("expected a Levin condition profile");
        };
        assert_eq!(shape, [4]);
        assert_eq!(scalar(&data[0]), 1.0);
        assert!(
            data.iter()
                .map(scalar)
                .all(|condition| condition.is_finite() && condition >= 1.0)
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
        let mobius = payload(interp.eval_toplevel("mobius").unwrap(), mobius_dynamic);
        assert_eq!(
            mobius
                .as_list()
                .unwrap()
                .iter()
                .map(scalar)
                .collect::<Vec<_>>(),
            [0., 1., -1., -1., 0., -1., 1.]
        );
        let Val::Variant {
            tag,
            payload: solution,
        } = payload(
            interp.eval_toplevel("congruence").unwrap(),
            congruence_dynamic,
        )
        else {
            panic!("expected an optional congruence solution");
        };
        assert_eq!(tag, "Some");
        let [Val::Record(solution)] = solution.as_slice() else {
            panic!("expected an exact pair")
        };
        assert_eq!(scalar(field(solution, "item0")), 45.0);
        assert_eq!(scalar(field(solution, "item1")), 50.0);
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

#[test]
fn generated_symbolic_results_run_in_both_engines() {
    let examples = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples");
    for (file, assertions) in [
        (
            "exact_congruences.aev",
            ["compatible_solution", "detects_incompatibility"],
        ),
        ("symbolic_hill.aev", ["finite_ground", "long_period_ground"]),
        (
            "symbolic_bootstrap.aev",
            ["reflected_bounds", "rejects_invalid_alpha"],
        ),
        ("symbolic_bessel_roots.aev", ["regular_zero", "robin_root"]),
    ] {
        let program = driver::load(&examples.join(file));
        assert!(
            !program.diagnostics.has_errors(),
            "{file}: {:?}",
            program.diagnostics.items
        );
        for mode in [Mode::Ast, Mode::Ir] {
            let mut interp = Interp::new(&program.entry().unwrap().checked).with_mode(mode);
            let diagnostics = driver::run_modules(&mut interp, &program);
            assert!(
                !diagnostics.has_errors(),
                "{file} {mode:?}: {:?}",
                diagnostics.items
            );
            for assertion in assertions {
                let value = interp.eval_toplevel(assertion).unwrap();
                assert!(
                    matches!(value, Value::Bool(true)),
                    "{file} {mode:?} {assertion}: {value:?}"
                );
            }
        }
    }
}
