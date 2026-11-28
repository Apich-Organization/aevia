//! Dense worker inputs, units, views, and fallback for `par_map` and `par_reduce`.

use std::sync::Arc;
use std::sync::atomic::AtomicUsize;
use std::sync::atomic::Ordering::Relaxed;

use aevia_core::CancellationToken;
use aevia_core::Interp;
use aevia_core::Mode;

fn session(
    program: &'static str,
    mode: Mode,
    jobs: usize,
    cancel_on_replica: Option<CancellationToken>,
) -> (Interp, Arc<AtomicUsize>) {
    let compilation = aevia_core::compile("par-map-dense.aev", program);
    assert!(
        !compilation.diagnostics.has_errors(),
        "{:?}",
        compilation.diagnostics.items
    );
    let mut interp = Interp::new(&compilation.checked)
        .with_mode(mode)
        .with_buffer();
    interp.set_jobs(jobs);
    let builds = Arc::new(AtomicUsize::new(0));
    let worker_builds = builds.clone();
    interp.set_replica(Arc::new(move || {
        worker_builds.fetch_add(1, Relaxed);
        let compilation = aevia_core::compile("par-map-dense.aev", program);
        let module = compilation.module.as_ref().ok_or("worker did not parse")?;
        let mut worker = Interp::new(&compilation.checked)
            .with_mode(mode)
            .with_buffer();
        if worker.load(module).has_errors() {
            return Err("worker did not load".to_string());
        }
        if let Some(token) = &cancel_on_replica {
            token.cancel();
        }
        Ok(worker)
    }));
    assert!(
        !interp
            .load(compilation.module.as_ref().unwrap())
            .has_errors()
    );
    (interp, builds)
}

const VIEWS: &str = r#"fn main(): IO Unit = do
    let xs = linspace(-128.0 m, 128.0 m, 257)
    let ys = par_map(abs, xs[11:211])
    let zs = par_map(abs, xs[11:211:2])
    let rs = par_map(abs, xs[::-2])
    let ds = par_map(abs, fill(-2.0 cm, 160))
    println("{ys[0]} {ys[-1]} {length(ys)}")
    println("{zs[0]} {zs[-1]} {length(zs)}")
    println("{rs[1]} {rs[-2]} {length(rs)}")
    println("{ds[17]} {length(ds)}")
    println(ys == map(abs, xs[11:211]))
    println(zs == map(abs, xs[11:211:2]))
    println(rs == map(abs, xs[::-2]))
"#;

#[test]
fn contiguous_offset_strides_and_display_units_preserve_language_outputs() {
    for mode in [Mode::Ast, Mode::Ir] {
        let (mut serial, _) = session(VIEWS, mode, 1, None);
        serial.run_main().unwrap();
        let (mut parallel, builds) = session(VIEWS, mode, 4, None);
        parallel.run_main().unwrap();
        let output = parallel.captured();
        assert_eq!(output, serial.captured());
        assert!(output.contains("2.0 cm 160"), "{output}");
        assert!(output.ends_with("true\ntrue\ntrue\n"), "{output}");
        assert!(builds.load(Relaxed) > 0, "no worker evaluated the map");
    }
}

#[test]
fn worker_errors_report_the_first_source_error() {
    const PROGRAM: &str = r#"fn fail(x: Float): Float =
    if x == 8.0 then at([1.0], 5)
    else if x == 48.0 then at([1.0], 6)
    else x
fn main(): IO Unit = do
    let xs = linspace(0.0, 128.0, 129)
    println(par_map(fail, xs))
"#;
    let (mut serial, _) = session(PROGRAM, Mode::Ir, 1, None);
    let first = serial.run_main().unwrap_err();
    let (mut parallel, builds) = session(PROGRAM, Mode::Ir, 4, None);
    let error = parallel.run_main().unwrap_err();
    assert_eq!(first.code, "E0802");
    assert_eq!(
        (error.code, error.message, error.span),
        (first.code, first.message, first.span)
    );
    assert!(
        builds.load(Relaxed) > 0,
        "no worker attempted the failing map"
    );
    assert!(parallel.captured().is_empty());
}

#[test]
fn cancellation_after_dispatch_interrupts_instead_of_rerunning() {
    // Floor uses the evaluator replicas exercised by the cancellation hook.
    const PROGRAM: &str = r#"fn main(): IO Unit = do
    let xs = linspace(-128.0, 128.0, 257)
    let mapped = par_map(floor, xs)
    println(mapped[3])
"#;
    for mode in [Mode::Ast, Mode::Ir] {
        let token = CancellationToken::new();
        let (mut parallel, builds) = session(PROGRAM, mode, 4, Some(token.clone()));
        parallel.set_cancellation_token(token);
        let error = parallel.run_main().unwrap_err();
        assert_eq!(error.code, "E0812");
        assert!(
            builds.load(Relaxed) > 0,
            "the worker did not request cancellation"
        );
        assert!(parallel.captured().is_empty());

        let token = CancellationToken::new();
        let (mut serial, builds) = session(PROGRAM, mode, 1, None);
        serial.set_cancellation_token(token.clone());
        token.cancel();
        assert_eq!(serial.run_main().unwrap_err().code, "E0812");
        assert_eq!(builds.load(Relaxed), 0);
        assert!(serial.captured().is_empty());
    }
}

#[test]
fn reductions_preserve_uneven_blocks_views_exact_values_and_display_units() {
    const PROGRAM: &str = r#"fn main(): IO Unit = do
    let xs = linspace(0.0, 4098.0, 4099)
    let harmonic = map(fn (i) => 1.0 / (1.0 + i), xs)
    println(par_reduce(fn (a, b) => a + b, 0.0, harmonic[1:4098]))
    println(par_reduce(fn (a, b) => a + b, 0.0, harmonic[::-2]))
    println(par_reduce(fn (a, b) => if a > b then a else b, 0.0 cm, fill(2.0 cm, 2051)))
    println(par_reduce(fn (a, b) => if a > b then a else b, 0 cm, fill(2 cm, 2051)))
"#;
    for mode in [Mode::Ast, Mode::Ir] {
        let (mut serial, _) = session(PROGRAM, mode, 1, None);
        serial.run_main().unwrap();
        let output = serial.captured();
        assert!(output.ends_with("2.0 cm\n2 cm\n"), "{output}");
        for jobs in [2, 4] {
            let (mut parallel, builds) = session(PROGRAM, mode, jobs, None);
            parallel.run_main().unwrap();
            assert_eq!(parallel.captured(), output);
            if mode == Mode::Ir {
                assert!(
                    builds.load(Relaxed) > 0,
                    "no worker evaluated the reduction"
                );
            }
        }
    }
}

#[test]
fn reduction_failures_replay_the_first_source_error_and_capture_cancellation() {
    const FAIL: &str = r#"fn fold(a: Float, b: Float): Float =
    if b == 8.0 then at([1.0], 5)
    else if b == 2048.0 then at([1.0], 6)
    else a + b
fn main(): IO Unit = do
    let xs = linspace(0.0, 4096.0, 4097)
    println(par_reduce(fold, 0.0, xs))
"#;
    let (mut serial, _) = session(FAIL, Mode::Ir, 1, None);
    let first = serial.run_main().unwrap_err();
    let (mut parallel, builds) = session(FAIL, Mode::Ir, 4, None);
    let failure = parallel.run_main().unwrap_err();
    assert_eq!(first.code, "E0802");
    assert_eq!(
        (failure.code, failure.message, failure.span),
        (first.code, first.message, first.span),
    );
    assert!(
        builds.load(Relaxed) > 0,
        "no worker attempted the failing reduction"
    );
    assert!(parallel.captured().is_empty());

    let token = CancellationToken::new();
    let (mut cancelled, builds) = session(FAIL, Mode::Ir, 4, Some(token.clone()));
    cancelled.set_cancellation_token(token);
    assert_eq!(cancelled.run_main().unwrap_err().code, "E0812");
    assert!(builds.load(Relaxed) > 0, "no worker requested cancellation");
    assert!(cancelled.captured().is_empty());
}

#[test]
fn float_tabulate_preserves_bits_order_and_display() {
    const PROGRAM: &str =
        "fn sample(index: Int) = (index + 0.5) * 2.0 cm\nfn main() = par_tabulate(257, sample)\n";
    for mode in [Mode::Ast, Mode::Ir] {
        let (mut serial, _) = session(PROGRAM, mode, 1, None);
        let Some(aevia_core::value::Value::Tensor(expected)) = serial.run_main().unwrap() else {
            panic!("expected a sequential Float tensor")
        };
        let expected = expected.dense().unwrap();
        for jobs in [2, 4] {
            let (mut interp, builds) = session(PROGRAM, mode, jobs, None);
            let Some(aevia_core::value::Value::Tensor(tensor)) = interp.run_main().unwrap() else {
                panic!("expected a Float tensor")
            };
            assert_eq!(tensor.shape, [257]);
            assert_eq!(tensor.display.as_ref().unwrap().name, "cm");
            let values = tensor.dense().unwrap();
            assert_eq!(values.len(), 257);
            assert!(
                values
                    .iter()
                    .zip(expected)
                    .all(|(a, b)| a.to_bits() == b.to_bits())
            );
            assert_eq!(aevia_core::value::show(&tensor.elements()[256]), "513.0 cm");
            if mode == Mode::Ir {
                assert!(builds.load(Relaxed) > 0, "no worker tabulated the tensor");
            }
        }
    }
}

#[test]
fn boxed_map_keeps_structured_values_in_order() {
    const PROGRAM: &str = "fn boxed(x: Float) = { sample = x }\nfn main() = par_map(boxed, linspace(0.0, 128.0, 129))\n";
    for mode in [Mode::Ast, Mode::Ir] {
        let (mut serial, _) = session(PROGRAM, mode, 1, None);
        let expected = serial.run_main().unwrap().unwrap();
        let (mut parallel, builds) = session(PROGRAM, mode, 4, None);
        let actual = parallel.run_main().unwrap().unwrap();
        let aevia_core::value::Value::Tensor(tensor) = &actual else {
            panic!("expected a boxed tensor")
        };
        assert!(matches!(
            tensor.storage,
            aevia_core::value::Storage::Boxed(_)
        ));
        assert_eq!(tensor.shape, [129]);
        assert_eq!(
            aevia_core::value::show(&actual),
            aevia_core::value::show(&expected)
        );
        if mode == Mode::Ir {
            assert!(builds.load(Relaxed) > 0, "no worker returned a record");
        }
    }
}
