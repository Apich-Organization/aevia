//! Item help describes accepted checked bindings without replaying the program.
use std::io::Write;
use std::path::Path;
use std::process::Command;
use std::process::Output;
use std::process::Stdio;

fn repl(
    directory: &Path,
    source: &str,
) -> Output {
    let mut child = Command::new(env!("CARGO_BIN_EXE_aevia"))
        .args(["repl", "--color=never"])
        .current_dir(directory)
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
fn stdout(output: &Output) -> String {
    String::from_utf8_lossy(&output.stdout).replace("\r\n", "\n")
}
fn stderr(output: &Output) -> String {
    String::from_utf8_lossy(&output.stderr).replace("\r\n", "\n")
}
fn cwd() -> std::path::PathBuf {
    std::env::current_dir().unwrap()
}

#[test]
fn builtin_output_helpers_types_units_and_operators_have_item_help() {
    let result = repl(
        &cwd(),
        ":help println\n:doc show\n:? repr\n:h to_pretty\n:doc Int\n:doc km\n:help +\n",
    );
    assert!(result.status.success(), "{}", stderr(&result));
    let text = stdout(&result);
    for expected in [
        "println — built-in",
        "standard output",
        "show — built-in",
        "repr — built-in",
        "to_pretty — built-in",
        "Int — built-in",
        "km — unit",
        "Exact coherent scale: 1000",
        "+ — operator",
    ] {
        assert!(text.contains(expected), "missing {expected}: {text}");
    }
}

#[test]
fn latest_binding_docs_replace_earlier_docs_and_rejected_drafts_never_replace_them() {
    let source = "## Original dimensions.\nfn measured(): Real[T] = 1 s\n:doc measured\n## Current dimensions.\nfn measured(): Real[L] = 1 m\n:doc measured\n## Rejected draft.\nfn measured(): String = 1 m\n:doc measured\n";
    let result = repl(&cwd(), source);
    assert!(!result.status.success());
    let text = stdout(&result);
    assert_eq!(text.matches("Original dimensions.").count(), 1, "{text}");
    assert_eq!(text.matches("Current dimensions.").count(), 2, "{text}");
    assert!(!text.contains("Rejected draft."), "{text}");
    assert!(text.contains("Real[L]"), "{text}");
}

#[test]
fn help_is_read_only_and_unknown_expressions_are_not_evaluated() {
    let source = "let first = symbol(\"q\")\n## Creates a fresh symbol when called.\nfn fresh() = symbol(\"q\")\n:doc fresh\n:doc fresh\nlet second = symbol(\"q\")\nrepr(second)\n:doc println(\"MUST_NOT_EXECUTE\")\n";
    let result = repl(&cwd(), source);
    assert!(!result.status.success());
    let text = stdout(&result);
    assert!(text.contains("q#2"), "{text}");
    assert_eq!(
        text.matches("Creates a fresh symbol when called.").count(),
        2,
        "{text}"
    );
    assert!(!text.contains("MUST_NOT_EXECUTE"), "{text}");
    assert!(
        stderr(&result).contains("expects a name"),
        "{}",
        stderr(&result)
    );
}

#[test]
fn item_lookup_wins_over_command_names_and_search_suggests_known_names() {
    let result = repl(
        &cwd(),
        "## My local run function.\nfn run(): Int = 42\n:help run\n:help :run\n:apropos print\n:doc prinln\n",
    );
    assert!(!result.status.success());
    let text = stdout(&result);
    assert!(text.contains("My local run function."), "{text}");
    assert!(text.contains("Item help matching `print`"), "{text}");
    assert!(stderr(&result).contains("println"), "{}", stderr(&result));
}

#[test]
fn imported_aliases_reexports_units_types_and_module_docs_keep_actual_origins() {
    let directory = std::env::temp_dir().join(format!("aevia-item-help-{}", std::process::id()));
    std::fs::create_dir_all(&directory).unwrap();
    std::fs::write(directory.join("Base.aev"), "## Base module documentation.\nmodule Base\n## Authored function documentation.\n## Example: convert(2)\nfn convert(n: Int): Int = n * 2\nprivate fn hidden(): Int = 0\n## A token.\ntype Token = | Wrap(Int)\n## Exact local unit.\nunit stride : L = 2 m\n").unwrap();
    std::fs::write(
        directory.join("Facade.aev"),
        "module Facade\nimport Base as Library\nlet exported = Library.convert\n",
    )
    .unwrap();
    let source = "import Base as Library\nimport Facade as F\n:doc Library.convert\n:doc F.exported\n:doc Library.Token\n:doc Library.stride\n:doc Library\n:doc Library.hidden\n";
    let result = repl(&directory, source);
    let text = stdout(&result);
    assert!(!result.status.success());
    assert_eq!(
        text.matches("Authored function documentation.").count(),
        2,
        "{text}\n{}",
        stderr(&result)
    );
    assert!(text.contains("Base.aev"), "{text}");
    assert!(text.contains("Facade.aev"), "{text}");
    assert!(text.contains("Library.Token — type"), "{text}");
    assert!(text.contains("Exact coherent scale: 2"), "{text}");
    assert!(text.contains("Base module documentation."), "{text}");
    assert!(!text.contains("hidden —"), "{text}");
    assert!(
        stderr(&result).contains("no accepted item named `Library.hidden`"),
        "{}",
        stderr(&result)
    );
    std::fs::remove_dir_all(directory).unwrap();
}

#[test]
fn user_bindings_named_like_builtins_keep_their_own_docs_and_reset_clears_them() {
    let result = repl(
        &cwd(),
        "## My replacement function.\nfn sqrt(): String = \"custom\"\n:doc sqrt\n:reset\n:doc sqrt\n",
    );
    assert!(result.status.success(), "{}", stderr(&result));
    let text = stdout(&result);
    assert_eq!(
        text.matches("My replacement function.").count(),
        1,
        "{text}"
    );
    assert_eq!(text.matches("Compute a square root.").count(), 1, "{text}");
}
