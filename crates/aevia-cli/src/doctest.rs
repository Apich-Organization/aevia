//! `aevia test --doc` — the doctest runner of §7.5.5 / §9.12.
//!
//! `aevia doc` renders a declaration's `aevia`-labelled code blocks; this
//! module compiles them. Each block becomes its own **synthetic module**: the
//! declaring module's `import`/`open` lines are copied in verbatim, but the
//! declaring module's declarations are not in scope (the compiler is run in
//! single-file mode, which cannot resolve a package graph). A snippet that
//! needs a function from the module it documents must therefore define what it
//! uses — the limitation is stated in the command's output, and it is the same
//! isolation `compile_fail("…")` has (§8.13).
//!
//! A block may declare an expected failure on its first line, as `compile_fail`
//! alone (any error) or `compile_fail E0401` (that code). A block that says it
//! is expected to fail and compiles is a failure, and so is a code mismatch:
//! the point of a doctest is that the documentation is checked, not that
//! something happened.

use crate::Options;
use crate::tooling;
use aevia_doc::doctest::Doctest;
use aevia_doc::doctest::Expected;
use aevia_doc::doctest::extract;
use aevia_syntax::Severity;
use std::process::ExitCode;

/// The outcome of compiling one doctest.
#[derive(Clone, Debug, PartialEq, Eq)]
enum Outcome {
    /// The snippet behaved as its marker promised.
    Pass,
    /// It did not; the string says what was expected and what happened.
    Fail(String),
}

/// `aevia test --doc [--filter PAT] [files…]`
pub fn run(
    _options: Options,
    files: &[String],
    filter: Option<&str>,
) -> ExitCode {
    let paths = match tooling::inputs(files) {
        | Ok(paths) => paths,
        | Err(code) => return code,
    };
    let mut passed = 0usize;
    let mut failed = 0usize;
    let mut checked = 0usize;
    for path in &paths {
        let shown = path.display().to_string();
        if let Some(pattern) = filter {
            if !shown.contains(pattern) {
                continue;
            }
        }
        let source = match std::fs::read_to_string(path) {
            | Ok(source) => source,
            | Err(error) => {
                eprintln!("aevia test --doc: cannot read `{shown}`: {error}");
                failed += 1;
                continue;
            },
        };
        let doctests = match extract(&source, &shown) {
            | Ok(doctests) => doctests,
            | Err(message) => {
                // A file whose documentation cannot be read is a failure, not
                // an empty success: nothing was checked.
                eprintln!("aevia test --doc: `{shown}`: {message}");
                failed += 1;
                continue;
            },
        };
        for doctest in &doctests {
            checked += 1;
            let (outcome, compilation) = check(doctest);
            match outcome {
                | Outcome::Pass => {
                    println!("doctest {}:{} ... ok", doctest.file, doctest.line);
                    passed += 1;
                },
                | Outcome::Fail(reason) => {
                    println!(
                        "doctest {}:{} ... FAILED ({reason})",
                        doctest.file, doctest.line
                    );
                    if let Some(compilation) = compilation {
                        eprint!("{}", compilation.diagnostics.render(&compilation.sources));
                    }
                    failed += 1;
                },
            }
        }
    }
    if let Some(pattern) = filter {
        if checked == 0 && failed == 0 {
            eprintln!("aevia test --doc: the filter `{pattern}` matched no doctest");
            return ExitCode::from(1);
        }
    }
    if checked == 0 {
        eprintln!("aevia test --doc: no ```aevia code blocks found; nothing was checked");
        return ExitCode::from(1);
    }
    println!("\n{passed} passed, {failed} failed");
    println!(
        "note: each snippet compiles as its own module — the declaring module's imports are \
         copied in, its declarations are not in scope"
    );
    if failed == 0 {
        ExitCode::SUCCESS
    } else {
        ExitCode::from(1)
    }
}

/// Compiles one doctest and judges it against its marker.
///
/// The compilation is returned alongside the outcome so a caller can print the
/// compiler's own diagnostics when the snippet did not do what it promised.
fn check(doctest: &Doctest) -> (Outcome, Option<aevia_core::Compilation>) {
    let source = candidate(doctest);
    let name = format!("{}:{}", doctest.file, doctest.line);
    let compilation = aevia_core::compile(&name, &source);
    let errors: Vec<&aevia_syntax::Diagnostic> = compilation
        .diagnostics
        .items
        .iter()
        .filter(|diagnostic| diagnostic.severity == Severity::Error)
        .collect();
    let codes: Vec<String> = errors
        .iter()
        .filter(|diagnostic| !diagnostic.code.is_empty())
        .map(|diagnostic| diagnostic.code.clone())
        .collect();
    let outcome = match &doctest.expected {
        | Expected::Compiles => {
            if errors.is_empty() {
                Outcome::Pass
            } else {
                Outcome::Fail(format!(
                    "the snippet did not compile ({})",
                    codes_or(&codes)
                ))
            }
        },
        | Expected::Fails => {
            if errors.is_empty() {
                Outcome::Fail(
                    "expected the snippet to fail to compile, but it compiled".to_string(),
                )
            } else {
                Outcome::Pass
            }
        },
        | Expected::FailsWith(expected) => {
            if errors.is_empty() {
                Outcome::Fail(format!(
                    "expected `{expected}` but the snippet compiled; add the error it documents"
                ))
            } else if codes.iter().any(|code| code == expected) {
                Outcome::Pass
            } else {
                Outcome::Fail(format!("expected `{expected}`, got {}", codes_or(&codes)))
            }
        },
    };
    (outcome, Some(compilation))
}

/// The synthetic module: copied imports, a blank line, then the snippet.
fn candidate(doctest: &Doctest) -> String {
    let mut source = String::new();
    for import in &doctest.imports {
        source.push_str(import);
        source.push('\n');
    }
    if !doctest.imports.is_empty() {
        source.push('\n');
    }
    source.push_str(doctest.snippet.trim_end());
    source.push('\n');
    source
}

/// The error codes as one list, never empty-looking.
fn codes_or(codes: &[String]) -> String {
    if codes.is_empty() {
        "an error with no code".to_string()
    } else {
        codes.join(", ")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn doctest(
        snippet: &str,
        expected: Expected,
    ) -> Doctest {
        Doctest {
            file: "tests/example.aev".to_string(),
            module: "Example".to_string(),
            line: 10,
            snippet: snippet.to_string(),
            expected,
            imports: Vec::new(),
        }
    }

    fn outcome(
        snippet: &str,
        expected: Expected,
    ) -> Outcome {
        check(&doctest(snippet, expected)).0
    }

    #[test]
    fn a_compiling_snippet_passes() {
        assert_eq!(outcome("let x = 1", Expected::Compiles), Outcome::Pass);
    }

    #[test]
    fn a_snippet_that_does_not_compile_fails() {
        let Outcome::Fail(reason) = outcome("let broken = 1 m + 1", Expected::Compiles) else {
            panic!("expected a failure");
        };
        assert!(reason.contains("did not compile"), "{reason}");
        assert!(reason.contains("E0401"), "{reason}");
    }

    #[test]
    fn a_matching_compile_fail_passes() {
        assert_eq!(
            outcome(
                "let broken = 1 m + 1",
                Expected::FailsWith("E0401".to_string())
            ),
            Outcome::Pass
        );
    }

    #[test]
    fn a_mismatched_compile_fail_fails_and_names_both_codes() {
        let Outcome::Fail(reason) = outcome(
            "let broken = 1 m + 1",
            Expected::FailsWith("E0302".to_string()),
        ) else {
            panic!("expected a failure");
        };
        assert!(reason.contains("expected `E0302`"), "{reason}");
        assert!(reason.contains("E0401"), "{reason}");
    }

    #[test]
    fn compile_fail_alone_accepts_any_error() {
        assert_eq!(
            outcome("let broken = 1 m + 1", Expected::Fails),
            Outcome::Pass
        );
        let Outcome::Fail(reason) = outcome("let fine = 1", Expected::Fails) else {
            panic!("expected a failure");
        };
        assert!(reason.contains("but it compiled"), "{reason}");
    }

    #[test]
    fn a_compile_fail_block_that_compiles_is_not_a_pass() {
        let Outcome::Fail(reason) =
            outcome("let fine = 1", Expected::FailsWith("E0401".to_string()))
        else {
            panic!("expected a failure");
        };
        assert!(reason.contains("expected `E0401`"), "{reason}");
    }

    #[test]
    fn the_copied_imports_reach_the_synthetic_module() {
        let mut with_import = doctest("let d = 2.0 m", Expected::Compiles);
        with_import.imports = vec!["open Units.SI".to_string()];
        let (outcome, _) = check(&with_import);
        assert_eq!(outcome, Outcome::Pass);
        assert!(candidate(&with_import).starts_with("open Units.SI\n\n"));
    }

    #[test]
    fn the_declaring_modules_declarations_are_not_in_scope() {
        // Documented limitation: a single-file compile cannot see the module
        // the docs are attached to, so a snippet must be self-contained.
        let Outcome::Fail(reason) = outcome("let x = area(2.0 m)", Expected::Compiles) else {
            panic!("expected a failure");
        };
        assert!(reason.contains("E0301"), "{reason}");
    }
}
