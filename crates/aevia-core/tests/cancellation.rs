//! Interruption unwinds evaluator state and leaves a reusable interpreter.
use aevia_core::CancellationToken;
use aevia_core::EvalError;
use aevia_core::Interp;
use aevia_core::Mode;
use aevia_core::compile;
use aevia_core::value::IoAction;
use aevia_core::value::Value;
use aevia_core::value::caps;
use aevia_core::value::show;
use aevia_syntax::Span;
use std::rc::Rc;
use std::sync::Arc;
use std::sync::atomic::AtomicUsize;
use std::sync::atomic::Ordering;
use std::time::Duration;
use std::time::Instant;

fn interrupt_later(token: CancellationToken) -> std::thread::JoinHandle<()> {
    std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(25));
        token.cancel();
    })
}

#[test]
fn a_cancelled_ir_tail_loop_unwinds_and_the_same_interpreter_remains_usable() {
    let source = "let saved = 41\nfn loop(n: Int): Int = loop(n + 1)\nfn next(): Int = saved + 1\nfn main(): Int = loop(0)\n";
    let compilation = compile("interrupt.aev", source);
    assert!(
        !compilation.diagnostics.has_errors(),
        "{:?}",
        compilation.diagnostics.items
    );
    let token = CancellationToken::new();
    let mut interp = Interp::new(&compilation.checked).with_mode(Mode::Ir);
    interp.set_cancellation_token(token.clone());
    assert!(
        !interp
            .load(compilation.module.as_ref().unwrap())
            .has_errors()
    );
    let interrupt = interrupt_later(token.clone());
    let error = interp.run_main().unwrap_err();
    interrupt.join().unwrap();
    assert_eq!(error.code, "E0812");
    assert!(error.span.is_some());
    token.reset();
    let next = interp.eval_toplevel("next").unwrap();
    assert_eq!(
        show(&interp.apply(next, Vec::new(), Span::default()).unwrap()),
        "42"
    );
    assert!(
        !aevia_rssn::cancellation::is_cancelled(),
        "kernel TLS guard leaked after unwinding"
    );
}

#[test]
fn cancellation_during_ast_ir_and_release_initializers_rolls_back_the_batch() {
    let source = "let temporary = symbol(\"interrupted\")\nlet expensive = 10000000!\n";
    let compilation = compile("interrupt-batch.aev", source);
    assert!(
        !compilation.diagnostics.has_errors(),
        "{:?}",
        compilation.diagnostics.items
    );
    for (mode, optimized) in [(Mode::Ast, false), (Mode::Ir, false), (Mode::Ir, true)] {
        let token = CancellationToken::new();
        let mut interp = Interp::new(&compilation.checked).with_mode(mode);
        interp.optimize_ir = optimized;
        interp.define_global("saved", Value::int(42));
        interp.set_cancellation_token(token.clone());
        let counts = interp.symbol_counts.clone();
        let interrupt = interrupt_later(token.clone());
        let error = interp
            .load_incremental_declarations(
                &compilation.checked,
                &[],
                &compilation.module.as_ref().unwrap().decls,
            )
            .unwrap_err()
            .1;
        interrupt.join().unwrap();
        assert_eq!(error.code, "E0812", "{mode:?}: {error:?}");
        assert_eq!(interp.symbol_counts, counts);
        assert!(interp.eval_toplevel("temporary").is_err());
        assert!(interp.eval_toplevel("expensive").is_err());
        token.reset();
        assert_eq!(show(&interp.eval_toplevel("saved").unwrap()), "42");
        assert_eq!(
            show(
                &interp
                    .call_named("factorial", vec![Value::int(5)], Span::default())
                    .unwrap()
            ),
            "120"
        );
    }
}

#[test]
#[allow(
    clippy::result_large_err,
    reason = "exercises the interpreter's existing line reader interface"
)]
fn cancelling_program_input_restores_capabilities_and_does_not_print_later_actions() {
    let token = CancellationToken::new();
    let mut interp = Interp::new(&Default::default()).with_buffer();
    interp.set_cancellation_token(token.clone());
    let request = token.clone();
    interp.set_line_reader(Rc::new(move || {
        request.cancel();
        Err(EvalError::new("E0810", "console read interrupted"))
    }));
    let action = Rc::new(IoAction::WithCaps {
        allowed: 0,
        action: Rc::new(IoAction::Seq(vec![
            Rc::new(IoAction::ReadLine),
            Rc::new(IoAction::Print {
                text: "must not print".into(),
                newline: true,
                stderr: false,
            }),
        ])),
    });
    let capabilities = interp.capabilities;
    assert_eq!(interp.run_io(&action).unwrap_err().code, "E0812");
    assert_eq!(interp.capabilities, capabilities);
    assert_eq!(interp.capabilities, caps::ALL);
    assert_eq!(interp.captured(), "");
    token.reset();
    interp.set_line_reader(Rc::new(|| Ok("next input\n".into())));
    assert_eq!(
        show(&interp.run_io(&Rc::new(IoAction::ReadLine)).unwrap()),
        "next input"
    );
}

#[test]
fn cancellation_reaches_parallel_workers_without_replaying_the_work_sequentially() {
    let source = "fn loop(n: Int): Int = loop(n + 1)\nfn main() = par_tabulate(64, loop)\n";
    let compilation = compile("interrupt-parallel.aev", source);
    assert!(
        !compilation.diagnostics.has_errors(),
        "{:?}",
        compilation.diagnostics.items
    );
    let mut interp = Interp::new(&compilation.checked).with_mode(Mode::Ir);
    interp.set_jobs(2);
    assert!(
        !interp
            .load(compilation.module.as_ref().unwrap())
            .has_errors()
    );
    let token = CancellationToken::new();
    interp.set_cancellation_token(token.clone());
    let started = Arc::new(AtomicUsize::new(0));
    let marker = started.clone();
    interp.set_replica(Arc::new(move || {
        let compilation = compile("interrupt-parallel.aev", source);
        let mut worker = Interp::new(&compilation.checked).with_mode(Mode::Ir);
        if worker
            .load(compilation.module.as_ref().ok_or("no module")?)
            .has_errors()
        {
            return Err("load failed".into());
        }
        marker.fetch_add(1, Ordering::Release);
        Ok(worker)
    }));
    let request = token.clone();
    let interrupt = std::thread::spawn(move || {
        let deadline = Instant::now() + Duration::from_secs(5);
        while started.load(Ordering::Acquire) == 0 && Instant::now() < deadline {
            std::thread::yield_now();
        }
        request.cancel();
    });
    let error = interp.run_main().unwrap_err();
    interrupt.join().unwrap();
    assert_eq!(error.code, "E0812");
    token.reset();
    assert_eq!(
        show(
            &interp
                .call_named("factorial", vec![Value::int(5)], Span::default())
                .unwrap()
        ),
        "120"
    );
}

#[test]
fn interruptible_background_tasks_cannot_steal_foreground_input() {
    let source = "fn ask(): IO String = read_line()\nfn main(): IO String = do\n    task <- spawn(ask)\n    await(task)\n";
    let compilation = compile("interrupt-input-worker.aev", source);
    assert!(
        !compilation.diagnostics.has_errors(),
        "{:?}",
        compilation.diagnostics.items
    );
    let mut interp = Interp::new(&compilation.checked).with_mode(Mode::Ir);
    interp.set_jobs(2);
    interp.set_cancellation_token(CancellationToken::new());
    interp.set_replica(Arc::new(move || {
        let compilation = compile("interrupt-input-worker.aev", source);
        let mut worker = Interp::new(&compilation.checked).with_mode(Mode::Ir);
        if worker
            .load(compilation.module.as_ref().ok_or("no module")?)
            .has_errors()
        {
            return Err("load failed".into());
        }
        Ok(worker)
    }));
    assert!(
        !interp
            .load(compilation.module.as_ref().unwrap())
            .has_errors()
    );
    let error = interp.run_main().unwrap_err();
    assert_eq!(error.code, "E0811");
    assert!(error.message.contains("foreground REPL"), "{error:?}");
}

#[test]
#[allow(
    clippy::result_large_err,
    reason = "exercises the interpreter's existing native callback interface"
)]
fn interruption_takes_precedence_over_a_native_domain_failure() {
    let token = CancellationToken::new();
    let mut interp = Interp::new(&Default::default());
    interp.set_cancellation_token(token.clone());
    let request = token.clone();
    let native = Value::Native(Rc::new(aevia_core::value::NativeFn(Box::new(
        move |_, _, _| {
            request.cancel();
            Err(EvalError::new(
                "E0806",
                "native routine returned a domain failure",
            ))
        },
    ))));
    let error = interp
        .apply(native, Vec::new(), Span::default())
        .unwrap_err();
    assert_eq!(error.code, "E0812");
    token.reset();
    assert!(interp.check_cancelled().is_ok());
}
