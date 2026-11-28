//! Diagnostics show submitted source, while internal compilation keeps stable spans.
use std::io::Write;
use std::path::Path;
use std::path::PathBuf;
use std::process::Command;
use std::process::Output;
use std::process::Stdio;

struct Scratch(PathBuf);
impl Scratch {
    fn new(name: &str) -> Self {
        let path = std::env::temp_dir().join(format!(
            "aevia-repl-diagnostics-{name}-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&path);
        std::fs::create_dir_all(&path).unwrap();
        Self(path)
    }

    fn write(
        &self,
        name: &str,
        source: &str,
    ) -> PathBuf {
        let path = self.0.join(name);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, source).unwrap();
        path
    }
}
impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn run(
    directory: &Path,
    source: &str,
    optimized: bool,
) -> Output {
    let mut command = Command::new(env!("CARGO_BIN_EXE_aevia"));
    command
        .args(["--color", "never"])
        .env("NO_COLOR", "1")
        .current_dir(directory)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    if optimized {
        command.arg("--release");
    }
    let mut child = command.spawn().unwrap();
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
fn assert_no_internal_source(output: &Output) {
    let diagnostics = err(output);
    for name in [
        "__repl_value",
        "__repl_type",
        "__repl_core",
        "__repl_time",
        "__repl_render",
        "__repl_latex",
        "__repl_ast",
    ] {
        assert!(
            !diagnostics.contains(name),
            "internal source {name} leaked:\n{diagnostics}"
        );
    }
}

#[test]
fn current_expression_shows_entered_code_on_line_one_after_history() {
    let directory = Scratch::new("expression");
    for optimized in [false, true] {
        let output = run(
            &directory.0,
            "let keep = 40\n:type keep\n1 + \"bad\"\nkeep + 2\n",
            optimized,
        );
        assert!(!output.status.success());
        let diagnostics = err(&output);
        assert!(diagnostics.contains("error[E0302]"), "{diagnostics}");
        assert!(diagnostics.contains("<repl>:1:"), "{diagnostics}");
        assert!(diagnostics.contains("1 | 1 + \"bad\""), "{diagnostics}");
        assert!(
            !diagnostics.contains("let keep"),
            "unrelated history displayed:\n{diagnostics}"
        );
        assert!(out(&output).contains("Int = 42"), "{}", out(&output));
        assert_no_internal_source(&output);
    }
}

#[test]
fn multiline_submission_lines_are_relative_to_the_submitted_fragment() {
    let directory = Scratch::new("multiline");
    let output = run(
        &directory.0,
        "let keep = 40\n:type keep\n:paste\nfn broken(): Int =\n    let x = 1\n    \"bad\"\n:end\nkeep + 2\n",
        false,
    );
    assert!(!output.status.success());
    let diagnostics = err(&output);
    assert!(diagnostics.contains("<repl>:3:"), "{diagnostics}");
    assert!(diagnostics.contains("3 |     \"bad\""), "{diagnostics}");
    assert!(out(&output).contains("Int = 42"), "{}", out(&output));
    assert_no_internal_source(&output);
}

#[test]
fn readonly_expression_commands_report_user_source_and_failure_status() {
    let directory = Scratch::new("readonly");
    for command in [
        "type", "core", "time", "latex", "typst", "pretty", "repr", "ast",
    ] {
        let source = format!("let keep = 40\n:type keep\n:{command} 1 +\nkeep + 2\n");
        let output = run(&directory.0, &source, false);
        assert!(!output.status.success(), "{command}: {}", out(&output));
        let diagnostics = err(&output);
        assert!(
            diagnostics.contains("error[E0202]"),
            "{command}: {diagnostics}"
        );
        assert!(
            diagnostics.contains("<repl>:1:"),
            "{command}: {diagnostics}"
        );
        assert!(
            diagnostics
                .lines()
                .any(|line| line.contains("1 |") && line.contains("1 +")),
            "{command}: {diagnostics}"
        );
        assert!(
            out(&output).contains("Int = 42"),
            "{command}: {}",
            out(&output)
        );
        assert_no_internal_source(&output);
    }
}

#[test]
fn loaded_file_compile_errors_keep_original_filename_and_line() {
    let directory = Scratch::new("load-compile");
    let file = directory.write(
        "loaded program.aev",
        "module Loaded\n\nfn broken(): Int = \"bad\"\n",
    );
    let source = format!(
        "let keep = 40\n:type keep\n:load \"{}\"\nkeep + 2\n",
        file.display()
    );
    let output = run(&directory.0, &source, false);
    assert!(!output.status.success());
    let diagnostics = err(&output);
    assert!(
        diagnostics.contains("loaded program.aev:3:"),
        "{diagnostics}"
    );
    assert!(
        diagnostics.contains("3 | fn broken(): Int = \"bad\""),
        "{diagnostics}"
    );
    assert!(out(&output).contains("Int = 42"), "{}", out(&output));
    assert_no_internal_source(&output);
}

#[test]
fn runtime_error_in_an_earlier_definition_shows_definition_and_current_call() {
    let directory = Scratch::new("archived-runtime");
    for optimized in [false, true] {
        let output = run(
            &directory.0,
            "fn fail(n: Int): Real = 1 / n\n:type fail\nfail(0)\n2 + 2\n",
            optimized,
        );
        assert!(!output.status.success());
        let diagnostics = err(&output);
        assert_eq!(
            diagnostics.matches("error[E0801]").count(),
            1,
            "{diagnostics}"
        );
        assert!(
            diagnostics.contains("<repl #"),
            "archived definition is unidentified:\n{diagnostics}"
        );
        assert!(
            diagnostics.contains("fn fail(n: Int): Real = 1 / n"),
            "{diagnostics}"
        );
        assert!(
            diagnostics.contains("<repl>:1:"),
            "current callsite is missing:\n{diagnostics}"
        );
        assert!(diagnostics.contains("fail(0)"), "{diagnostics}");
        assert!(
            diagnostics.contains("`fail`"),
            "call stack is missing:\n{diagnostics}"
        );
        assert!(out(&output).contains("Int = 4"), "{}", out(&output));
        assert_no_internal_source(&output);
    }
}

#[test]
fn imported_runtime_errors_keep_the_real_file_and_current_callsite() {
    let directory = Scratch::new("import-runtime");
    directory.write(
        "Foreign.aev",
        "module Foreign\n\nfn fail(n: Int): Real = 1 / n\n",
    );
    let output = run(
        &directory.0,
        "import Foreign as F\nF.fail(0)\n2 + 2\n",
        false,
    );
    assert!(!output.status.success());
    let diagnostics = err(&output);
    assert!(diagnostics.contains("Foreign.aev:3:"), "{diagnostics}");
    assert!(
        diagnostics.contains("fn fail(n: Int): Real = 1 / n"),
        "{diagnostics}"
    );
    assert!(diagnostics.contains("<repl>:1:"), "{diagnostics}");
    assert!(diagnostics.contains("F.fail(0)"), "{diagnostics}");
    assert!(out(&output).contains("Int = 4"), "{}", out(&output));
    assert_no_internal_source(&output);
}

#[test]
fn imported_compile_errors_keep_the_real_file_and_do_not_poison_the_next_input() {
    let directory = Scratch::new("import-compile");
    directory.write(
        "Foreign.aev",
        "module Foreign\n\nfn broken(n: Int): Int = n + \"bad\"\n",
    );
    let output = run(
        &directory.0,
        "let keep = 40\n:type keep\nimport Foreign\nkeep + 2\n",
        false,
    );
    assert!(!output.status.success());
    let diagnostics = err(&output);
    assert!(diagnostics.contains("Foreign.aev:3:"), "{diagnostics}");
    assert!(
        diagnostics.contains("fn broken(n: Int): Int = n + \"bad\""),
        "{diagnostics}"
    );
    assert!(out(&output).contains("Int = 42"), "{}", out(&output));
    assert_no_internal_source(&output);
}

#[test]
fn unicode_columns_and_indentation_match_the_submitted_source() {
    let directory = Scratch::new("unicode");
    let output = run(
        &directory.0,
        "let keep = 40\n:type keep\n:paste\nfn broken(): Int =\n    let 中文 = @\n    0\n:end\nkeep + 2\n",
        false,
    );
    assert!(!output.status.success());
    let diagnostics = err(&output);
    assert!(diagnostics.contains("<repl>:2:14"), "{diagnostics}");
    assert!(
        diagnostics.contains("2 |     let 中文 = @"),
        "{diagnostics}"
    );
    let caret = diagnostics
        .lines()
        .find(|line| line.contains('|') && line.contains('^'))
        .expect("caret line");
    let padding = caret.split_once('|').unwrap().1.find('^').unwrap();
    assert_eq!(
        padding, 16,
        "CJK display widths misalign the caret:\n{diagnostics}"
    );
    assert!(out(&output).contains("Int = 42"), "{}", out(&output));
    assert_no_internal_source(&output);
}

#[test]
fn independent_parser_errors_survive_without_delimiter_or_checker_cascades() {
    let directory = Scratch::new("parser-cascades");
    let output = run(
        &directory.0,
        ":paste\nlet first = f(1 + )\nlet second = g(, 2)\n:end\n3 + 4\n",
        false,
    );
    assert!(!output.status.success());
    let diagnostics = err(&output);
    assert_eq!(
        diagnostics.matches("error[E0202]").count(),
        2,
        "{diagnostics}"
    );
    assert!(
        !diagnostics.contains("error[E030"),
        "checker ran on malformed source:\n{diagnostics}"
    );
    assert!(diagnostics.contains("let first = f(1 + )"), "{diagnostics}");
    assert!(diagnostics.contains("let second = g(, 2)"), "{diagnostics}");
    assert!(out(&output).contains("Int = 7"), "{}", out(&output));
    assert_no_internal_source(&output);
}

#[test]
fn independent_type_errors_in_one_batch_are_both_reported() {
    let directory = Scratch::new("type-errors");
    let output = run(
        &directory.0,
        ":paste\nlet first: Int = \"bad\"\nlet second: Bool = 42\n:end\n3 + 4\n",
        false,
    );
    assert!(!output.status.success());
    let diagnostics = err(&output);
    assert_eq!(
        diagnostics.matches("error[E0302]").count(),
        2,
        "{diagnostics}"
    );
    assert!(
        diagnostics.contains("let first: Int = \"bad\""),
        "{diagnostics}"
    );
    assert!(
        diagnostics.contains("let second: Bool = 42"),
        "{diagnostics}"
    );
    assert!(out(&output).contains("Int = 7"), "{}", out(&output));
    assert_no_internal_source(&output);
}

#[test]
fn warnings_from_accepted_history_are_not_replayed_on_every_expression() {
    let directory = Scratch::new("warning-history");
    let output = run(
        &directory.0,
        "private fn never_used(): Int = 1\n1 + 1\n2 + 2\n",
        false,
    );
    assert!(output.status.success(), "{}", err(&output));
    let diagnostics = err(&output);
    assert_eq!(
        diagnostics.matches("warning[W0601]").count(),
        1,
        "{diagnostics}"
    );
    assert!(out(&output).contains("Int = 2"));
    assert!(out(&output).contains("Int = 4"));
    assert_no_internal_source(&output);
}

#[test]
fn runtime_errors_from_loaded_functions_keep_the_original_file() {
    let directory = Scratch::new("load-runtime");
    let file = directory.write(
        "loaded runtime.aev",
        "module Loaded\n\nfn fail(n: Int): Real = 1 / n\n",
    );
    let source = format!(
        "let keep = 40\n:type keep\n:load \"{}\"\nfail(0)\nkeep + 2\n",
        file.display()
    );
    let output = run(&directory.0, &source, false);
    assert!(!output.status.success());
    let diagnostics = err(&output);
    assert!(
        diagnostics.contains("loaded runtime.aev:3:"),
        "{diagnostics}"
    );
    assert!(
        diagnostics.contains("fn fail(n: Int): Real = 1 / n"),
        "{diagnostics}"
    );
    assert!(diagnostics.contains("<repl>:1:"), "{diagnostics}");
    assert!(diagnostics.contains("fail(0)"), "{diagnostics}");
    assert!(out(&output).contains("Int = 42"), "{}", out(&output));
    assert_no_internal_source(&output);
}

#[test]
fn dimension_secondary_labels_use_the_current_expression_coordinates() {
    let directory = Scratch::new("dimension-labels");
    let output = run(
        &directory.0,
        "let distance = 1 m\nlet duration = 1 s\n:type duration\ndistance + duration\n2 + 2\n",
        false,
    );
    assert!(!output.status.success());
    let diagnostics = err(&output);
    assert!(diagnostics.contains("error[E0401]"), "{diagnostics}");
    assert!(diagnostics.contains("<repl>:1:"), "{diagnostics}");
    assert!(
        diagnostics.contains("1 | distance + duration"),
        "{diagnostics}"
    );
    assert!(diagnostics.contains("this is L"), "{diagnostics}");
    assert!(diagnostics.contains("this is T"), "{diagnostics}");
    assert!(out(&output).contains("Int = 4"), "{}", out(&output));
    assert_no_internal_source(&output);
}

#[test]
fn identical_warnings_from_distinct_imports_are_each_reported_once() {
    let directory = Scratch::new("import-warning-history");
    for module in ["First", "Second"] {
        directory.write(
            &format!("{module}.aev"),
            &format!("module {module}\nprivate fn unused(): Int = 0\nfn answer(): Int = 21\n"),
        );
    }
    let output = run(
        &directory.0,
        "import First as F\nimport Second as S\nF.answer() + S.answer()\nF.answer() + S.answer()\n:type F.answer\n",
        false,
    );
    assert!(output.status.success(), "{}", err(&output));
    let diagnostics = err(&output);
    assert_eq!(
        diagnostics.matches("warning[W0601]").count(),
        2,
        "distinct source warnings were lost or replayed:\n{diagnostics}"
    );
    assert!(diagnostics.contains("First.aev:2:"), "{diagnostics}");
    assert!(diagnostics.contains("Second.aev:2:"), "{diagnostics}");
    assert_eq!(
        out(&output).matches("Int = 42").count(),
        2,
        "{}",
        out(&output)
    );
    assert_no_internal_source(&output);
}

#[test]
fn symbolic_result_warnings_describe_the_expression_without_internal_names() {
    let directory = Scratch::new("symbol-warning");
    let output = run(&directory.0, "symbol(\"x\")\n2 + 2\n", false);
    assert!(output.status.success(), "{}", err(&output));
    let diagnostics = err(&output);
    assert!(diagnostics.contains("warning[W0301]"), "{diagnostics}");
    assert!(
        !diagnostics.contains("warning[W0303]"),
        "displayed result was reported unused:\n{diagnostics}"
    );
    assert!(diagnostics.contains("symbol(\"x\")"), "{diagnostics}");
    assert!(out(&output).contains("Int = 4"), "{}", out(&output));
    assert_no_internal_source(&output);
}

#[test]
fn source_inspector_shows_accepted_original_input_without_rerunning_it_and_reset_clears_it() {
    let directory = Scratch::new("source-inspector");
    let program = "## Original input documentation.\nmodule Original\nfn main(): IO Unit = println(\"RUN_ONCE\")\n";
    let source = format!(":paste\n{program}:end\n:source\n:source 1\n:reset\n:source\n");
    let output = run(&directory.0, &source, false);
    assert!(output.status.success(), "{}", err(&output));
    let text = out(&output);
    assert!(text.contains("Accepted source"), "{text}");
    assert!(text.contains("<repl #1>"), "{text}");
    assert!(
        text.contains(program),
        "original input text was altered:\n{text}"
    );
    assert_eq!(
        text.lines().filter(|line| *line == "RUN_ONCE").count(),
        1,
        "inspection executed main:\n{text}"
    );
    assert!(
        text.contains("no accepted source"),
        "reset retained source history:\n{text}"
    );
    assert_no_internal_source(&output);
}

#[test]
fn source_inspector_keeps_a_loaded_files_original_header_path_and_text() {
    let directory = Scratch::new("loaded-source-inspector");
    let program = "module Loaded\n\n## Preserve the original module header.\nfn main(): IO Unit = println(\"LOADED_ONCE\")\n";
    let file = directory.write("inspected program.aev", program);
    let prefix = format!("let keep = 7\n:load \"{}\"\n", file.display());
    let listed = run(&directory.0, &format!("{prefix}:source\n"), false);
    assert!(listed.status.success(), "{}", err(&listed));
    let listing = out(&listed);
    let id = listing
        .lines()
        .filter(|line| line.contains("inspected program.aev"))
        .find_map(|line| line.split_whitespace().next()?.parse::<u64>().ok())
        .expect("loaded file has a listed source number");
    let output = run(
        &directory.0,
        &format!("{prefix}:source {id}\nkeep + 1\n"),
        false,
    );
    assert!(output.status.success(), "{}", err(&output));
    let text = out(&output);
    assert!(
        text.contains(&file.display().to_string()),
        "loaded source path is absent:\n{text}"
    );
    assert!(
        text.contains(program),
        "inspector showed normalized compiler source:\n{text}"
    );
    assert_eq!(
        text.lines().filter(|line| *line == "LOADED_ONCE").count(),
        1,
        "inspection reran the file:\n{text}"
    );
    assert!(
        text.contains("Int = 8"),
        "inspection changed bindings:\n{text}"
    );
    assert_no_internal_source(&output);
}

#[test]
fn initializer_failures_show_the_archived_definition_and_current_declaration() {
    let directory = Scratch::new("initializer-callsite");
    for optimized in [false, true] {
        let output = run(
            &directory.0,
            "fn fail(n: Int): Real = 1 / n\n:type fail\nlet bad = fail(0)\n:bindings\n2 + 2\n",
            optimized,
        );
        assert!(!output.status.success());
        let diagnostics = err(&output);
        assert_eq!(
            diagnostics.matches("error[E0801]").count(),
            1,
            "{diagnostics}"
        );
        assert!(diagnostics.contains("<repl #"), "{diagnostics}");
        assert!(
            diagnostics.contains("fn fail(n: Int): Real = 1 / n"),
            "{diagnostics}"
        );
        assert!(
            diagnostics.contains("<repl>:1:"),
            "current initializer callsite is missing:\n{diagnostics}"
        );
        assert!(diagnostics.contains("let bad = fail(0)"), "{diagnostics}");
        assert!(
            !out(&output).lines().any(|line| line.starts_with("bad :")),
            "failed initializer was committed:\n{}",
            out(&output)
        );
        assert!(out(&output).contains("Int = 4"), "{}", out(&output));
        assert_no_internal_source(&output);
    }
}

#[test]
fn loaded_main_failures_keep_the_real_file_callsite_and_prior_definition() {
    let directory = Scratch::new("loaded-main-callsite");
    let file = directory.write(
        "failing main.aev",
        "module Loaded\nfn main(): IO Unit = println(\"{fail(0)}\")\n",
    );
    let source = format!(
        "fn fail(n: Int): Real = 1 / n\n:type fail\n:load \"{}\"\n2 + 2\n",
        file.display()
    );
    let output = run(&directory.0, &source, false);
    assert!(!output.status.success());
    let diagnostics = err(&output);
    assert!(diagnostics.contains("error[E0801]"), "{diagnostics}");
    assert!(diagnostics.contains("<repl #"), "{diagnostics}");
    assert!(
        diagnostics.contains("fn fail(n: Int): Real = 1 / n"),
        "{diagnostics}"
    );
    assert!(
        diagnostics.contains("failing main.aev:2:"),
        "loaded main callsite is missing:\n{diagnostics}"
    );
    assert!(
        diagnostics.contains("fn main(): IO Unit = println(\"{fail(0)}\")"),
        "{diagnostics}"
    );
    assert!(out(&output).contains("Int = 4"), "{}", out(&output));
    assert_no_internal_source(&output);
}
