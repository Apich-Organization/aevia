//! Exercise complete REPL submissions through the real CLI, including failures.
use std::io::Write;
use std::process::Command;
use std::process::Output;
use std::process::Stdio;

fn repl(
    args: &[&str],
    source: &str,
) -> Output {
    let mut child = Command::new(env!("CARGO_BIN_EXE_aevia"))
        .args(args)
        .env("NO_COLOR", "1")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("start REPL");
    child
        .stdin
        .take()
        .unwrap()
        .write_all(source.as_bytes())
        .unwrap();
    child.wait_with_output().unwrap()
}
fn out(output: &Output) -> String {
    String::from_utf8_lossy(&output.stdout).replace("\r\n", "\n")
}
fn err(output: &Output) -> String {
    String::from_utf8_lossy(&output.stderr).replace("\r\n", "\n")
}

#[test]
fn no_command_enters_repl_without_pipe_prompts() {
    let result = repl(&[], "let distance = 20 m\ndistance / 4 s\n:type distance\n");
    assert!(result.status.success(), "{}", err(&result));
    assert!(out(&result).contains("5 m/s"), "{}", out(&result));
    assert!(!out(&result).contains("aevia>"));
    assert!(!out(&result).contains("interactive guide"));
    assert!(!out(&result).contains('\u{1b}'));
}

#[test]
fn pasted_program_runs_new_main_once_and_keeps_its_functions() {
    let source = "module Demo\nfn twice(x: Int): Int = x * 2\nfn main(): IO Unit = do\n    println(\"first\")\n    println(\"{twice(21)}\")\ntwice(3)\n:type twice\n:run\n";
    let result = repl(&["repl"], source);
    assert!(result.status.success(), "{}", err(&result));
    assert_eq!(
        out(&result).matches("first\n").count(),
        2,
        "{}",
        out(&result)
    );
    assert!(out(&result).contains("Int = 6"), "{}", out(&result));
    assert_eq!(out(&result).matches("42\n").count(), 2);
}

#[test]
fn declaration_group_allows_forward_function_references() {
    let result = repl(
        &[],
        "fn main(): IO Unit = println(\"{helper()}\")\nfn helper(): Int = 42\n",
    );
    assert!(result.status.success(), "{}", err(&result));
    assert_eq!(out(&result), "42\n");
}

#[test]
fn io_expressions_and_time_execute_their_actions() {
    let result = repl(&[], "println(\"hello\")\n:time println(\"timed\")\n");
    assert!(result.status.success(), "{}", err(&result));
    assert!(
        out(&result).starts_with("hello\ntimed\n"),
        "{}",
        out(&result)
    );
    assert!(out(&result).contains("time:"));
}

#[test]
fn failed_input_preserves_bindings_and_returns_failure_for_scripts() {
    let result = repl(&[], "let x = 7\n:type x\nmissing_name + 1\nx + 1\n");
    assert!(!result.status.success());
    assert!(out(&result).contains("Int = 8"), "{}", out(&result));
    assert!(err(&result).contains("missing_name"), "{}", err(&result));
}

#[test]
fn explicit_paste_and_cancel_never_execute_partial_code() {
    let result = repl(
        &[],
        ":paste\nfn main(): IO Unit = do\n    println(\"pasted\")\n:end\n:paste\nprintln(\"should not run\")\n:cancel\n",
    );
    assert!(result.status.success(), "{}", err(&result));
    assert_eq!(out(&result), "pasted\n");
    let result = repl(&[], ":paste\nprintln(\"should not run\")\n");
    assert!(!result.status.success());
    assert!(out(&result).is_empty());
    assert!(err(&result).contains("expected :end"));
}

#[test]
fn session_commands_explain_usage_and_unknown_dimensions() {
    let result = repl(
        &[],
        ":type\n:units nonsense\n:no_such_command\n:help main\n",
    );
    assert!(!result.status.success());
    assert!(err(&result).contains("usage: :type"));
    assert!(err(&result).contains("unknown dimension"));
    assert!(err(&result).contains("type :help"));
    assert!(out(&result).contains("fn main(): IO Unit"));
}

#[test]
fn runtime_failure_does_not_skip_the_next_declaration() {
    let result = repl(
        &[],
        "let good = 7\n:type good\nlet index = 9\nlet bad = [1][index]\n:type good\nlet next = 8\nnext + good\n",
    );
    assert!(!result.status.success());
    assert!(err(&result).contains("error[E0802]"), "{}", err(&result));
    assert!(err(&result).contains("--> <repl>"), "{}", err(&result));
    assert!(
        out(&result).contains("Int = 15"),
        "stdout: {}\nstderr: {}",
        out(&result),
        err(&result)
    );
}

#[test]
fn global_flags_after_separator_are_program_arguments() {
    let scratch = std::env::temp_dir().join(format!("aevia-repl-args-{}.aev", std::process::id()));
    std::fs::write(&scratch, "fn main(): IO Unit = println(\"{args()}\")\n").unwrap();
    let result = Command::new(env!("CARGO_BIN_EXE_aevia"))
        .arg("run")
        .arg(&scratch)
        .args(["--", "--release", "--color", "never"])
        .output()
        .unwrap();
    std::fs::remove_file(scratch).unwrap();
    assert!(result.status.success(), "{}", err(&result));
    assert!(out(&result).contains("--release"));
    assert!(out(&result).contains("--color"));
}

#[test]
fn release_repl_preserves_exact_values_io_and_scientific_types() {
    let source = "fn exact(x: Real): Real = if true then x + 0.1 else x / 0\nfn main(): IO Unit = println(\"{exact(0.2)}\")\nexact(0.2)\nif true then 3 m else 4 m\n:time println(\"once\")\n";
    let reference = repl(&[], source);
    let optimized = repl(&["--release"], source);
    assert!(reference.status.success(), "{}", err(&reference));
    assert!(optimized.status.success(), "{}", err(&optimized));
    assert_eq!(
        out(&reference).split("time:").next(),
        out(&optimized).split("time:").next()
    );
    assert_eq!(out(&optimized).matches("once\n").count(), 1);
}

#[test]
fn save_and_load_support_quoted_paths_and_protect_existing_files() {
    let path = std::env::temp_dir().join(format!("aevia session {}.aev", std::process::id()));
    let _ = std::fs::remove_file(&path);
    let source = format!(
        "let answer = 42\n:save \"{}\"\n:save \"{}\"\n:reset\n:load \"{}\"\nanswer\n",
        path.display(),
        path.display(),
        path.display()
    );
    let result = repl(&[], &source);
    let saved = std::fs::read_to_string(&path).unwrap();
    std::fs::remove_file(&path).unwrap();
    assert!(!result.status.success(), "second save refuses to overwrite");
    assert_eq!(saved.trim(), "let answer = 42");
    assert!(out(&result).contains("Int = 42"), "{}", out(&result));
    assert!(err(&result).contains("cannot save"));
}

#[test]
fn pasted_initializers_can_call_later_functions_and_capture_the_batch() {
    for args in [vec![], vec!["--release"]] {
        let result = repl(
            &args,
            "let result = helper()\nfn helper(): Int = 42\nfn original(): Int = result\nresult\nlet result = 9\noriginal()\n",
        );
        assert!(result.status.success(), "{}", err(&result));
        assert_eq!(
            out(&result).matches("Int = 42").count(),
            2,
            "{}",
            out(&result)
        );
    }
}

#[test]
fn failed_initializer_rolls_back_the_whole_batch_and_its_forward_definitions() {
    let source = "let previous = 7\n:type previous\nlet partial = helper()\nlet index = 9\nlet bad = [1][index]\nfn helper(): Int = 42\n:bindings\nprevious + 1\n";
    let result = repl(&[], source);
    assert!(!result.status.success());
    assert!(out(&result).contains("Int = 8"), "{}", out(&result));
    assert!(!out(&result).contains("partial :"));
    assert!(!out(&result).contains("helper :"));
}

#[test]
fn named_programs_can_be_appended_after_existing_bindings() {
    let result = repl(
        &[],
        "let value = 21\n:type value\nmodule Appended\nfn main(): IO Unit = println(\"{value * 2}\")\nvalue\n",
    );
    assert!(result.status.success(), "{}", err(&result));
    assert!(out(&result).contains("42\n"), "{}", out(&result));
    assert!(out(&result).contains("Int = 21"));
}

#[test]
fn type_inspection_respects_user_bindings_that_shadow_builtins() {
    let result = repl(&[], "let sqrt = 42\n:type sqrt\n");
    assert!(result.status.success(), "{}", err(&result));
    assert_eq!(out(&result).trim(), "Int");
}

#[test]
fn repl_rebinding_changes_dimension_after_an_invalid_submission() {
    let source = "let a = 1 km\n:type a\nlet b = 1 s\n:type b\nlet c = a + b\n:type b\nlet b = 1 km\nb / 1 km\nlet c = a + b\nc / 1 km\n";
    for args in [vec![], vec!["--release"]] {
        let result = repl(&args, source);
        assert!(
            !result.status.success(),
            "the initial mismatched addition fails"
        );
        assert_eq!(
            err(&result).matches("error[E0401]").count(),
            1,
            "{}",
            err(&result)
        );
        assert!(out(&result).contains("= 1"), "{}", out(&result));
        assert!(out(&result).contains("= 2"), "{}", out(&result));
    }
}

#[test]
fn scripts_overwrite_dimensions_types_and_self_referencing_initializers() {
    let path = std::env::temp_dir().join(format!("aevia-overwrite-{}.aev", std::process::id()));
    for (source, expected) in [
        (
            "let a = 1 km\nlet b = 1 s\nlet b = 1 km\nlet c = a + b\nfn main(): IO Unit = println(\"{c}\")\n",
            "2 km\n",
        ),
        (
            "let x = 1\nlet x = \"overwritten\"\nfn main(): IO Unit = println(x)\n",
            "overwritten\n",
        ),
        (
            "let x = 1\nlet x = x + 1\nfn main(): IO Unit = println(\"{x}\")\n",
            "2\n",
        ),
    ] {
        std::fs::write(&path, source).unwrap();
        for optimized in [false, true] {
            let mut command = Command::new(env!("CARGO_BIN_EXE_aevia"));
            command.arg("run").arg(&path).arg("--no-cache");
            if optimized {
                command.arg("--release");
            }
            let result = command.output().unwrap();
            assert!(result.status.success(), "{}", err(&result));
            assert_eq!(out(&result), expected);
        }
    }
    std::fs::remove_file(path).unwrap();
}
