//! Deferred source programs, lexical early returns, replay and bounded IO work.
use aevia_core::Compilation;
use aevia_core::Interp;
use aevia_core::Mode;
use aevia_core::compile;
use aevia_core::value::IoAction;
use aevia_core::value::Value;
use aevia_syntax::Span;
use std::rc::Rc;

fn checked(source: &str) -> Compilation {
    let compilation = compile("io_control.aev", source);
    assert!(
        !compilation.diagnostics.has_errors(),
        "{source}\n{:?}",
        compilation.diagnostics.items
    );
    compilation
}
fn installed(
    compilation: &Compilation,
    mode: Mode,
) -> Interp {
    let mut interp = Interp::new(&compilation.checked)
        .with_mode(mode)
        .with_buffer();
    let loaded = interp.load(compilation.module.as_ref().unwrap());
    assert!(!loaded.has_errors(), "{mode:?}: {:?}", loaded.items);
    interp
}
fn action(
    interp: &mut Interp,
    name: &str,
) -> Rc<IoAction> {
    let function = interp.eval_toplevel(name).unwrap();
    let Value::Io(action) = interp.apply(function, vec![], Span::default()).unwrap() else {
        panic!("expected IO")
    };
    action
}
fn variant(
    value: &Value,
    name: &str,
) {
    let Value::Variant(value) = value else {
        panic!("expected {name}, got {value:?}")
    };
    assert_eq!(value.name, name);
}

#[test]
fn construction_is_lazy_and_effects_precede_later_pure_failures() {
    for middle in ["", "    let unused = 0\n"] {
        let source = format!(
            "fn explode(n: Int): Real = 10 / n\nfn main(): IO Unit = do\n    println(\"before\")\n{middle}    println(\"{{explode(0)}}\")\n"
        );
        let compilation = checked(&source);
        for mode in [Mode::Ast, Mode::Ir] {
            let mut interp = installed(&compilation, mode);
            let program = action(&mut interp, "main");
            assert_eq!(
                interp.captured(),
                "",
                "construction executes no statements: {mode:?}"
            );
            let error = interp.run_io(&program).unwrap_err();
            assert_eq!(error.code, "E0801");
            assert_eq!(interp.captured(), "before\n", "{mode:?}");
            assert!(
                !error.stack.is_empty(),
                "deferred failures retain source frames"
            );
        }
    }
}

#[test]
fn result_and_option_return_at_the_do_boundary_and_outer_io_can_continue() {
    let compilation = checked(
        r#"fn result(): IO (Result<Int, String>) = do
    println("start")
    let n: Int = Err("stop")?
    println("unreachable")
    on("cpu", fn() => Ok(n))
fn optional(): IO (Option<Int>) = do
    let n: Int = None?
    println("unreachable")
    on("cpu", fn() => Some(n))
fn main(): IO Unit = do
    failed <- result()
    absent <- optional()
    println("{failed} {absent}")
"#,
    );
    for mode in [Mode::Ast, Mode::Ir] {
        let mut interp = installed(&compilation, mode);
        let result = action(&mut interp, "result");
        variant(&interp.run_io(&result).unwrap(), "Err");
        let optional = action(&mut interp, "optional");
        variant(&interp.run_io(&optional).unwrap(), "None");
        interp.run_main().unwrap();
        let output = interp.captured();
        assert!(!output.contains("unreachable"), "{mode:?}: {output}");
        assert!(
            output.contains("stop") && output.contains("None"),
            "{mode:?}: {output}"
        );
    }
}

#[test]
fn try_checks_the_actual_return_container_and_error_type() {
    for source in [
        "fn bad(): Result<Int, Int> =\n    let n: Int = Err(\"wrong\")?\n    Ok(n)\n",
        "fn bad() =\n    let n: Int = None?\n    n + 1\n",
        "let n: Int = None?\n",
        "fn bad(): IO Unit = do\n    let n: Int = None?\n    println(n)\n",
    ] {
        let compilation = compile("rejected_try.aev", source);
        assert!(
            compilation.diagnostics.has_errors(),
            "must reject: {source}"
        );
    }
}

#[test]
fn propagated_failures_reconstruct_the_destination_phantom_success_type() {
    let compilation = checked(
        r#"fn result(): Result<String, String> =
    let n: Int = Err("bad")?
    Ok(show(n))
fn optional(): Option<String> =
    let n: Int = None?
    Some(show(n))
let result_roundtrip = from_value<Result<String, String>>(to_value(result()))
let option_roundtrip = from_value<Option<String>>(to_value(optional()))
"#,
    );
    for mode in [Mode::Ast, Mode::Ir] {
        let mut interp = installed(&compilation, mode);
        for (name, payload_name) in [("result_roundtrip", "Err"), ("option_roundtrip", "None")] {
            let outer = interp.eval_toplevel(name).unwrap();
            variant(&outer, "Ok");
            let Value::Variant(outer) = outer else {
                unreachable!()
            };
            variant(&outer.fields[0], payload_name);
        }
    }
}

#[test]
fn replay_uses_fresh_locals_and_preserves_lexical_shadowing() {
    let compilation = checked(
        r#"fn main(): IO Unit = do
    let x = 1
    let captured = fn() => x
    let x = 2
    n <- on("cpu", fn() => captured())
    println("{n} {x}")
"#,
    );
    for mode in [Mode::Ast, Mode::Ir] {
        let mut interp = installed(&compilation, mode);
        let program = action(&mut interp, "main");
        interp.run_io(&program).unwrap();
        interp.run_io(&program).unwrap();
        assert_eq!(interp.captured(), "1 2\n1 2\n", "{mode:?}");
    }
}

#[test]
fn io_step_and_continuation_limits_are_independent_and_recoverable() {
    let compilation = checked(
        r#"fn tail(): IO Unit = do
    tail()
fn nested(): IO Unit = do
    nested()
    println("unreachable")
fn fine(): IO Unit = do
    println("recovered")
"#,
    );
    for mode in [Mode::Ast, Mode::Ir] {
        let mut interp = installed(&compilation, mode);
        interp.max_io_steps = 100;
        interp.max_io_depth = 4;
        let tail = action(&mut interp, "tail");
        assert_eq!(interp.run_io(&tail).unwrap_err().code, "E0805");
        interp.max_io_steps = 1000;
        let nested = action(&mut interp, "nested");
        let error = interp.run_io(&nested).unwrap_err();
        assert_eq!(error.code, "E0805");
        assert!(error.message.contains("depth"), "{mode:?}: {error:?}");
        let fine = action(&mut interp, "fine");
        interp.run_io(&fine).unwrap();
        assert_eq!(interp.captured(), "recovered\n", "{mode:?}");
        let flat = Rc::new(IoAction::Seq(
            (0..1000)
                .map(|_| Rc::new(IoAction::Pure(Value::Unit)))
                .collect(),
        ));
        interp.max_io_steps = 3000;
        interp.max_io_depth = 1;
        interp.run_io(&flat).unwrap();
    }
}

#[test]
fn saved_do_program_rejects_a_foreign_interpreter_session() {
    let compilation = checked("fn main(): IO Unit = do\n    println(\"secret\")\n");
    for mode in [Mode::Ast, Mode::Ir] {
        let mut owner = installed(&compilation, mode);
        let program = action(&mut owner, "main");
        let mut other = installed(&compilation, mode);
        assert_eq!(other.run_io(&program).unwrap_err().code, "E0809");
        assert_eq!(other.captured(), "");
        owner.run_io(&program).unwrap();
    }
}

#[test]
fn ordinary_errors_with_payloads_remain_errors_at_function_boundaries() {
    let compilation = checked("fn invoke(f: () -> Int): Int = f()\n");
    for mode in [Mode::Ast, Mode::Ir] {
        let mut interp = installed(&compilation, mode);
        let native = Value::Native(Rc::new(aevia_core::value::NativeFn(Box::new(|_, _, _| {
            let mut error = aevia_core::EvalError::new("E0899", "host failure");
            error.payload = Some(Value::int(99));
            Err(error)
        }))));
        let invoke = interp.eval_toplevel("invoke").unwrap();
        let error = interp
            .apply(invoke, vec![native], Span::default())
            .unwrap_err();
        assert_eq!(error.code, "E0899", "{mode:?}");
        assert_eq!(error.payload.unwrap().as_f64(), Some(99.0));
    }
}

#[test]
fn nested_source_functions_own_their_try_return_boundary() {
    let compilation = checked(
        r#"fn main(): IO Unit = do
    let nested = fn(): Option<Int> =>
        let value: Int = None?
        Some(value)
    let result = nested()
    println("{result}")
"#,
    );
    for mode in [Mode::Ast, Mode::Ir] {
        let mut interp = installed(&compilation, mode);
        interp.run_main().unwrap();
        assert_eq!(interp.captured(), "None\n", "{mode:?}");
    }
}

#[test]
fn saved_actions_keep_their_checked_literal_revision() {
    let identity = aevia_core::source::SourceIdentity::virtual_source("aevia-test://saved-do");
    let first = aevia_core::compile_with_source_identity(
        "saved.aev",
        "fn main(): IO Unit = do\n    let n: Float = 1\n    println(repr(n))\n",
        &identity,
    );
    let second = aevia_core::compile_with_source_identity(
        "saved.aev",
        "fn main(): IO Unit = do\n    let n: Int   = 2\n    println(repr(n))\n",
        &identity,
    );
    assert!(!first.diagnostics.has_errors());
    assert!(!second.diagnostics.has_errors());
    for mode in [Mode::Ast, Mode::Ir] {
        let mut interp = installed(&first, mode);
        let old = action(&mut interp, "main");
        interp.prepare_module(&second.checked);
        assert!(!interp.load(second.module.as_ref().unwrap()).has_errors());
        let new = action(&mut interp, "main");
        interp.run_io(&old).unwrap();
        interp.run_io(&new).unwrap();
        assert_eq!(interp.captured(), "1.0\n2\n", "{mode:?}");
    }
}

#[test]
fn long_flat_do_binding_programs_execute_and_release_on_a_small_stack() {
    std::thread::Builder::new()
        .stack_size(2 * 1024 * 1024)
        .spawn(|| {
            let mut source = String::from("fn main(): IO Unit = do\n");
            for index in 0..8192 {
                source.push_str(&format!("    let x{index} = {index}\n"));
            }
            source.push_str("    println(x8191)\n");
            let compilation = checked(&source);
            for mode in [Mode::Ast, Mode::Ir] {
                let mut interp = installed(&compilation, mode);
                interp.max_io_depth = 2;
                interp.max_io_steps = 20_000;
                interp.run_main().unwrap();
                assert_eq!(interp.captured(), "8191\n", "{mode:?}");
                drop(interp);
            }
        })
        .unwrap()
        .join()
        .unwrap();
}

#[test]
fn legacy_builtin_error_text_propagates_without_nominal_roundtrip_proof() {
    let compilation = checked(
        r#"fn parse(text: String): Result<Int, ParseError> =
    let n = parse_int(text)?
    Ok(n + 1)
let failed = parse("bad")
let packed = to_value(failed)
let unpacked = from_value<Result<Int, ParseError>>(packed)
fn missing(): IO (Result<String, IOError>) = do
    answer <- read_file("/aevia-io-control-no-such-file")
    let text = answer?
    read_file(text)
"#,
    );
    for mode in [Mode::Ast, Mode::Ir] {
        let mut interp = installed(&compilation, mode);
        variant(&interp.eval_toplevel("failed").unwrap(), "Err");
        // The ABI text is a usable language error, not proof of ParseError.
        variant(&interp.eval_toplevel("unpacked").unwrap(), "Err");
        let missing = action(&mut interp, "missing");
        variant(&interp.run_io(&missing).unwrap(), "Err");
    }
}

#[test]
fn legacy_exception_never_authenticates_wrong_concrete_or_source_nominal_errors() {
    for source in [
        "fn pass(value: Result<Int, Int>): Result<String, Int> =\n    let n = value?\n    Ok(show(n))\n",
        "type ParseError = | ParseError(Int)\nfn pass(value: Result<Int, ParseError>): Result<String, ParseError> =\n    let n = value?\n    Ok(show(n))\n",
    ] {
        let compilation = checked(source);
        for mode in [Mode::Ast, Mode::Ir] {
            let mut interp = installed(&compilation, mode);
            let pass = interp.eval_toplevel("pass").unwrap();
            let forged = aevia_core::value::make_variant(
                aevia_types::adt::RESULT_ID,
                1,
                "Err",
                vec![Value::string("forged")],
            );
            assert_eq!(
                interp
                    .apply(pass, vec![forged], Span::default())
                    .unwrap_err()
                    .code,
                "E0302",
                "{mode:?}"
            );
        }
    }
}
