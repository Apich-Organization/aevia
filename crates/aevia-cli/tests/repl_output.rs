//! Output formats use the live session and execute each expression once.
use std::io::Write;
use std::process::Command;
use std::process::Output;
use std::process::Stdio;

fn repl(
    source: &str,
    optimized: bool,
) -> Output {
    let mut command = Command::new(env!("CARGO_BIN_EXE_aevia"));
    command.args(["--color", "never"]);
    if optimized {
        command.arg("--release");
    }
    let mut child = command
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(source.as_bytes())
        .unwrap();
    child.wait_with_output().unwrap()
}
fn out(result: &Output) -> String {
    String::from_utf8_lossy(&result.stdout).replace("\r\n", "\n")
}
fn err(result: &Output) -> String {
    String::from_utf8_lossy(&result.stderr).replace("\r\n", "\n")
}

#[test]
fn direct_renderers_and_language_utilities_cover_all_formats() {
    for optimized in [false, true] {
        let result = repl(
            "let q = 1/2\n:latex q\n:typst q\n:pretty q\n:pprint q\n:repr \"hello\"\n:print q\n:show q\nto_pretty(q)\nto_typst(q)\nto_latex(q)\n",
            optimized,
        );
        assert!(result.status.success(), "{}", err(&result));
        let text = out(&result);
        assert!(text.contains("\\frac{1}{2}"), "{text}");
        assert!(text.contains("frac(1, 2)"), "{text}");
        assert!(text.matches('─').count() >= 3, "{text}");
        assert!(text.contains("\"hello\""), "{text}");
        assert!(text.contains("0.5\n0.5"), "{text}");
        assert!(!text.contains('\u{1b}'), "{text}");
    }
}

#[test]
fn selected_format_applies_to_results_and_time_and_survives_reset() {
    for optimized in [false, true] {
        let result = repl(
            ":format repr\n\"quoted\"\n:format typst\n1/2\n:time 1/2\n:reset\n1/3\n:format pretty\n1/2\n:format latex\n1/2\n:format show\n1/2\n:format\n",
            optimized,
        );
        assert!(result.status.success(), "{}", err(&result));
        let text = out(&result);
        assert!(text.contains("String = \"quoted\""), "{text}");
        assert!(text.matches("frac(1, 2)").count() >= 2, "{text}");
        assert!(text.contains("frac(1, 3)"), "{text}");
        assert!(text.contains("time:"), "{text}");
        assert!(text.contains("=\n") && text.contains('─'), "{text}");
        assert!(text.contains("\\frac{1}{2}"), "{text}");
        assert!(text.contains("= 0.5"), "{text}");
        assert!(text.contains("output format: show"), "{text}");
    }
}

#[test]
fn output_commands_execute_io_once_without_replaying_declarations() {
    let result = repl(
        "let marker = symbol(\"x\")\n:pretty println(\"pretty-once\")\n:typst println(\"typst-once\")\n:latex println(\"latex-once\")\n:format pretty\nprintln(\"program-unchanged\")\n:repr marker\n",
        false,
    );
    assert!(result.status.success(), "{}", err(&result));
    let text = out(&result);
    for marker in [
        "pretty-once",
        "typst-once",
        "latex-once",
        "program-unchanged",
    ] {
        assert_eq!(text.matches(marker).count(), 1, "{text}");
    }
}

#[test]
fn rendering_failures_leave_the_session_and_selected_format_usable() {
    let result = repl(
        "fn square(x: Int): Int = x*x\n:format typst\n:typst square\n:format nonsense\n:pretty missing\n:typst\n1/2\n:format show\nsquare(7)\n",
        false,
    );
    assert!(!result.status.success());
    let errors = err(&result);
    assert!(errors.contains("function"), "{errors}");
    assert!(errors.contains("unknown output format"), "{errors}");
    assert!(errors.contains("E0301"), "{errors}");
    assert!(errors.contains("usage: :typst"), "{errors}");
    assert!(out(&result).contains("frac(1, 2)"), "{}", out(&result));
    assert!(out(&result).contains("Int = 49"), "{}", out(&result));
}

#[test]
fn typst_quantities_and_matrices_use_typst_syntax() {
    let result = repl(
        ":typst 3 m/s\n:typst [[1,2],[3,4]]\n:pretty [[1,2],[3,4]]\n",
        false,
    );
    assert!(result.status.success(), "{}", err(&result));
    let text = out(&result);
    assert!(text.contains("mat("), "{text}");
    assert!(
        !text.contains("\\mathrm") && !text.contains("\\begin"),
        "{text}"
    );
    assert!(text.contains('m') && text.contains('s'), "{text}");
}
