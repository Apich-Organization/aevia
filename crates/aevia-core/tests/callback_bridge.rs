//! Checked callbacks cross the kernel ABI without sending evaluator state to another thread.
use aevia_core::Interp;
use aevia_core::Mode;
use aevia_core::compile;
use aevia_core::value::NumKind;
use aevia_core::value::Value;
use aevia_syntax::Span;

fn payload(value: Value) -> (usize, Value) {
    let Value::Variant(value) = value else {
        panic!("expected Result")
    };
    (
        value.index,
        value.fields.first().cloned().unwrap_or(Value::Unit),
    )
}

#[test]
fn integration_uses_captured_callbacks_and_preserves_dimensions() {
    let source = r#"let scale: Float = 2.0
let callback = to_callback(fn (x: Float) => x * scale)
let result = Rssn.call_ho("num_integrate", [to_value(0.0), to_value(1.0), to_value(0.0), to_value(0.000001), to_value(100)], [callback])
fn decode(v: Value) = from_value<{ value: Float, error: Float, evaluations: Int, converged: Bool }>(v)
let dimensioned = Rssn.call_ho("num_integrate", [to_value(0.0 s), to_value(1.0 s), to_value(0.0), to_value(0.000001), to_value(100)], [to_callback(fn (x: Float[Time]) => x)])
fn decode_dimensioned(v: Value) = from_value<{ value: Float[Time^2], error: Float[Time^2], evaluations: Int, converged: Bool }>(v)
"#;
    let compiled = compile("callback_integration.aev", source);
    assert!(
        !compiled.diagnostics.has_errors(),
        "{:?}",
        compiled.diagnostics.items
    );
    let module = compiled.module.unwrap();
    for mode in [Mode::Ast, Mode::Ir] {
        let mut interp = Interp::new(&compiled.checked).with_mode(mode);
        let diagnostics = interp.load(&module);
        assert!(
            !diagnostics.has_errors(),
            "{mode:?}: {:?}",
            diagnostics.items
        );
        for (name, reader, expected) in [
            ("result", "decode", 1.0),
            ("dimensioned", "decode_dimensioned", 0.5),
        ] {
            let (index, opaque) = payload(interp.eval_toplevel(name).unwrap());
            assert_eq!(index, 0, "{mode:?}: {opaque:?}");
            let reader = interp.eval_toplevel(reader).unwrap();
            let (index, value) =
                payload(interp.apply(reader, vec![opaque], Span::default()).unwrap());
            assert_eq!(index, 0, "{mode:?}: {value:?}");
            let Value::Record(record) = value else {
                panic!("expected integral record")
            };
            let NumKind::Float(number) = record.fields["value"].as_num().unwrap().kind else {
                panic!("expected Float")
            };
            assert!((number - expected).abs() < 1e-5, "{mode:?}: {number}");
        }
    }
}

#[test]
fn callback_errors_are_returned_by_the_kernel_call() {
    let source = r#"fn pick(x: Float, index: Int) = [x][index]
let result = Rssn.call_ho("num_integrate", [to_value(0.0), to_value(1.0), to_value(0.0), to_value(0.000001), to_value(100)], [to_callback(fn (x: Float) => pick(x, 9))])
"#;
    let compiled = compile("callback_error.aev", source);
    assert!(
        !compiled.diagnostics.has_errors(),
        "{:?}",
        compiled.diagnostics.items
    );
    let module = compiled.module.unwrap();
    for mode in [Mode::Ast, Mode::Ir] {
        let mut interp = Interp::new(&compiled.checked).with_mode(mode);
        let diagnostics = interp.load(&module);
        assert!(
            !diagnostics.has_errors(),
            "{mode:?}: {:?}",
            diagnostics.items
        );
        let (index, error) = payload(interp.eval_toplevel("result").unwrap());
        assert_eq!(index, 1, "{mode:?}: callback error must reach the caller");
        let Value::KernelError(error) = error else {
            panic!("expected OpError")
        };
        assert_eq!(
            error.code, "E0802",
            "{mode:?}: callback error code was lost"
        );
    }
}

#[test]
fn a_host_cannot_forge_a_callback_signature() {
    let source = r#"fn wrap(f: Float -> Float) = to_callback(f)
fn wrong(x: Int) = x
"#;
    let compiled = compile("callback_spoof.aev", source);
    assert!(
        !compiled.diagnostics.has_errors(),
        "{:?}",
        compiled.diagnostics.items
    );
    let module = compiled.module.unwrap();
    for mode in [Mode::Ast, Mode::Ir] {
        let mut interp = Interp::new(&compiled.checked).with_mode(mode);
        let diagnostics = interp.load(&module);
        assert!(
            !diagnostics.has_errors(),
            "{mode:?}: {:?}",
            diagnostics.items
        );
        let wrapper = interp.eval_toplevel("wrap").unwrap();
        let wrong = interp.eval_toplevel("wrong").unwrap();
        assert!(
            interp.apply(wrapper, vec![wrong], Span::default()).is_err(),
            "{mode:?}: accepted a host-forged callable signature"
        );
    }
}

#[test]
fn generic_callback_constructor_uses_the_actual_call_specialization() {
    let source = r#"fn make<a>(f: a -> a) = to_callback(f)
let callback = make<Float>(fn (x: Float) => x)
"#;
    let compiled = compile("callback_generic.aev", source);
    assert!(
        !compiled.diagnostics.has_errors(),
        "{:?}",
        compiled.diagnostics.items
    );
    let module = compiled.module.unwrap();
    for mode in [Mode::Ast, Mode::Ir] {
        let mut interp = Interp::new(&compiled.checked).with_mode(mode);
        let diagnostics = interp.load(&module);
        assert!(
            !diagnostics.has_errors(),
            "{mode:?}: {:?}",
            diagnostics.items
        );
        assert!(
            matches!(
                interp.eval_toplevel("callback").unwrap(),
                Value::Callback(_)
            ),
            "{mode:?}"
        );
    }
}

#[test]
fn first_class_converter_accepts_only_callable_bound_evidence() {
    let source = r#"let converter = to_callback
fn concrete(x: Float) = x
fn generic<a>(x: a) = x
"#;
    let compiled = compile("callback_host_apply.aev", source);
    assert!(
        !compiled.diagnostics.has_errors(),
        "{:?}",
        compiled.diagnostics.items
    );
    let module = compiled.module.unwrap();
    for mode in [Mode::Ast, Mode::Ir] {
        let mut interp = Interp::new(&compiled.checked).with_mode(mode);
        let diagnostics = interp.load(&module);
        assert!(
            !diagnostics.has_errors(),
            "{mode:?}: {:?}",
            diagnostics.items
        );
        let converter = interp.eval_toplevel("converter").unwrap();
        let concrete = interp.eval_toplevel("concrete").unwrap();
        let value = interp
            .apply(converter.clone(), vec![concrete], Span::default())
            .unwrap();
        assert!(matches!(value, Value::Callback(_)), "{mode:?}");
        let generic = interp.eval_toplevel("generic").unwrap();
        assert!(
            interp
                .apply(converter, vec![generic], Span::default())
                .is_err(),
            "{mode:?}: unbound generic callback accepted"
        );
    }
}

#[test]
fn generated_namespace_callback_facade_executes_in_both_engines() {
    let registry = aevia_rssn::registry();
    let binding = aevia_rssn::bindings::catalog(registry)
        .into_iter()
        .find(|row| row.op.name == "num_integrate")
        .expect("integration binding");
    let (path, _) = aevia_rssn::codegen::namespace_files(registry)
        .into_iter()
        .find(|(_, source)| source.contains(&format!("fn {}(", binding.name)))
        .expect("integration namespace");
    let namespace = path.strip_suffix(".aev").unwrap();
    let source = format!(
        "import {namespace}\nlet result = {namespace}.{}([to_value(0.0), to_value(1.0), to_value(0.0), to_value(0.000001), to_value(100)], [to_callback(fn (x: Float) => x)])\n",
        binding.name
    );
    let directory =
        std::env::temp_dir().join(format!("aevia-callback-facade-{}", std::process::id()));
    std::fs::create_dir_all(&directory).unwrap();
    let path = directory.join("main.aev");
    std::fs::write(&path, source).unwrap();
    let program = aevia_core::driver::load(&path);
    assert!(
        !program.diagnostics.has_errors(),
        "{:?}",
        program.diagnostics.items
    );
    for mode in [Mode::Ast, Mode::Ir] {
        let mut interp = Interp::new(&program.entry().unwrap().checked).with_mode(mode);
        let diagnostics = aevia_core::driver::run_modules(&mut interp, &program);
        assert!(
            !diagnostics.has_errors(),
            "{mode:?}: {:?}",
            diagnostics.items
        );
        let (index, value) = payload(interp.eval_toplevel("result").unwrap());
        assert_eq!(index, 0, "{mode:?}: {value:?}");
    }
    std::fs::remove_dir_all(directory).unwrap();
}

#[test]
fn brent_and_all_vector_ode_methods_run_checked_dimensioned_callbacks() {
    let methods = [
        "num_ode_dopri5",
        "num_ode_rosenbrock23",
        "num_ode_radau5",
        "num_ode_bdf",
    ];
    let mut source = String::from(
        r#"let period: Float[Time] = 1.0 s
let target: Float[Time] = 0.5 s
let root = Rssn.call_ho("num_brent", [to_value(0.0 s), to_value(1.0 s), to_value(0.000001 s), to_value(100)], [to_callback(fn (t: Float[Time]) => t - target)])
fn root_value(v: Value) = from_value<{ root: Float[Time], evaluations: Int }>(v)
fn ode_value(v: Value) = from_value<{ times: Tensor<1, Float[Time]>, states: Tensor<2, Float[Length]>, evaluations: Int, rejected: Int }>(v)
"#,
    );
    for (i, method) in methods.iter().enumerate() {
        source.push_str(&format!("let ode{i} = Rssn.call_ho(\"{method}\", [to_value(0.0 s), to_value(0.1 s), to_value([1.0 m]), to_value(0.000001 m), to_value(0.00001), to_value(0.01 s), to_value(1000)], [to_callback(fn (t: Float[Time], y: Tensor<1, Float[Length]>) => y / period)])\n"));
    }
    let compiled = compile("dimensioned_callback_ode.aev", &source);
    assert!(
        !compiled.diagnostics.has_errors(),
        "{:?}",
        compiled.diagnostics.items
    );
    let module = compiled.module.unwrap();
    for mode in [Mode::Ast, Mode::Ir] {
        let mut interp = Interp::new(&compiled.checked).with_mode(mode);
        let diagnostics = interp.load(&module);
        assert!(
            !diagnostics.has_errors(),
            "{mode:?}: {:?}",
            diagnostics.items
        );
        for (name, reader, expected) in std::iter::once(("root".to_string(), "root_value", 0.5))
            .chain(
                methods
                    .iter()
                    .enumerate()
                    .map(|(i, _)| (format!("ode{i}"), "ode_value", 0.1_f64.exp())),
            )
        {
            let (index, opaque) = payload(interp.eval_toplevel(&name).unwrap());
            assert_eq!(index, 0, "{mode:?} {name}: {opaque:?}");
            let decoder = interp.eval_toplevel(reader).unwrap();
            let (index, value) = payload(
                interp
                    .apply(decoder, vec![opaque], Span::default())
                    .unwrap(),
            );
            assert_eq!(index, 0, "{mode:?} {name}: {value:?}");
            let Value::Record(record) = value else {
                panic!("expected numerical result record")
            };
            let result = if name == "root" {
                record.fields["root"].as_f64().unwrap()
            } else {
                let Value::Tensor(times) = &record.fields["times"] else {
                    panic!("expected times tensor")
                };
                let Value::Tensor(states) = &record.fields["states"] else {
                    panic!("expected states tensor")
                };
                assert_eq!(times.shape.len(), 1, "{mode:?} {name}");
                assert_eq!(states.shape, vec![times.shape[0], 1], "{mode:?} {name}");
                states
                    .get_flat(states.storage.len() - 1)
                    .unwrap()
                    .as_f64()
                    .unwrap()
            };
            assert!(
                (result - expected).abs() < 0.002,
                "{mode:?} {name}: {result} vs {expected}"
            );
        }
    }
}

#[test]
fn vector_ode_rejects_wrong_callback_units_and_preserves_runtime_errors() {
    let source = r#"fn pick(y: Tensor<1, Float[Length]>, index: Int) = y[index]
let args = [to_value(0.0 s), to_value(0.1 s), to_value([1.0 m]), to_value(0.000001 m), to_value(0.00001), to_value(0.01 s), to_value(1000)]
let bad_units = Rssn.call_ho("num_ode_dopri5", args, [to_callback(fn (t: Float[Time], y: Tensor<1, Float[Length]>) => y)])
let bad_extent = Rssn.call_ho("num_ode_dopri5", args, [to_callback(fn (t: Float[Time], y: Tensor<[2], Float[Length]>) => y / (1.0 s))])
let bad_index = Rssn.call_ho("num_ode_dopri5", args, [to_callback(fn (t: Float[Time], y: Tensor<1, Float[Length]>) => [pick(y, 9) / (1.0 s)])])
"#;
    let compiled = compile("callback_ode_errors.aev", source);
    assert!(
        !compiled.diagnostics.has_errors(),
        "{:?}",
        compiled.diagnostics.items
    );
    let module = compiled.module.unwrap();
    for mode in [Mode::Ast, Mode::Ir] {
        let mut interp = Interp::new(&compiled.checked).with_mode(mode);
        let diagnostics = interp.load(&module);
        assert!(
            !diagnostics.has_errors(),
            "{mode:?}: {:?}",
            diagnostics.items
        );
        let (index, _) = payload(interp.eval_toplevel("bad_units").unwrap());
        assert_eq!(index, 1, "{mode:?}: wrong derivative units were accepted");
        let (index, error) = payload(interp.eval_toplevel("bad_extent").unwrap());
        assert_eq!(index, 1, "{mode:?}: wrong callback extent was accepted");
        let Value::KernelError(error) = error else {
            panic!("expected OpError")
        };
        assert!(
            error.message.contains("extent"),
            "{mode:?}: expected extent preflight error, got {error:?}"
        );
        let (index, error) = payload(interp.eval_toplevel("bad_index").unwrap());
        assert_eq!(index, 1, "{mode:?}: callback error was swallowed");
        let Value::KernelError(error) = error else {
            panic!("expected OpError")
        };
        assert_eq!(error.code, "E0802", "{mode:?}");
    }
}

#[test]
fn generated_vector_ode_facade_executes_with_a_dimensioned_callback() {
    let registry = aevia_rssn::registry();
    let binding = aevia_rssn::bindings::catalog(registry)
        .into_iter()
        .find(|row| row.op.name == "num_ode_dopri5")
        .expect("ODE binding");
    let (path, _) = aevia_rssn::codegen::namespace_files(registry)
        .into_iter()
        .find(|(_, source)| source.contains(&format!("fn {}(", binding.name)))
        .expect("ODE namespace");
    let namespace = path.strip_suffix(".aev").unwrap();
    let source = format!(
        "import {namespace}\nlet period: Float[Time] = 1.0 s\nlet result = {namespace}.{}([to_value(0.0 s), to_value(0.1 s), to_value([1.0 m]), to_value(0.000001 m), to_value(0.00001), to_value(0.01 s), to_value(1000)], [to_callback(fn (t: Float[Time], y: Tensor<1, Float[Length]>) => y / period)])\n",
        binding.name
    );
    let directory = std::env::temp_dir().join(format!("aevia-ode-facade-{}", std::process::id()));
    std::fs::create_dir_all(&directory).unwrap();
    let path = directory.join("main.aev");
    std::fs::write(&path, source).unwrap();
    let program = aevia_core::driver::load(&path);
    assert!(
        !program.diagnostics.has_errors(),
        "{:?}",
        program.diagnostics.items
    );
    for mode in [Mode::Ast, Mode::Ir] {
        let mut interp = Interp::new(&program.entry().unwrap().checked).with_mode(mode);
        let diagnostics = aevia_core::driver::run_modules(&mut interp, &program);
        assert!(
            !diagnostics.has_errors(),
            "{mode:?}: {:?}",
            diagnostics.items
        );
        let (index, value) = payload(interp.eval_toplevel("result").unwrap());
        assert_eq!(index, 0, "{mode:?}: {value:?}");
    }
    std::fs::remove_dir_all(directory).unwrap();
}
