//! `aevia test --doc` — the doctest runner of §7.5.5 / §9.12.
//!
//! `aevia doc` renders a declaration's `aevia`-labelled code blocks; this
//! module compiles them. Each block becomes its own **synthetic module**: the
//! declaring module's `import`/`open` lines are copied in verbatim, but the
//! declaring module's declarations are not implicitly in scope. Imports resolve
//! in that source file's package and adjacent directory, including an explicit
//! import of the documented module. Snippets exist only as in-memory overlays.
//!
//! A block may declare an expected failure on its first line, as `compile_fail`
//! alone (any error) or `compile_fail E0401` (that code). A block that says it
//! is expected to fail and compiles is a failure, and so is a code mismatch:
//! the point of a doctest is that the documentation is checked, not that
//! something happened.

use crate::Options;
use crate::tooling;
use aevia_core::driver::CheckCache;
use aevia_core::driver::Program;
use aevia_core::manifest::Package;
use aevia_core::manifest::Roots;
use aevia_core::source::SourceOverlays;
use aevia_doc::doctest::Doctest;
use aevia_doc::doctest::Expected;
use aevia_doc::doctest::extract;
use aevia_syntax::Severity;
use std::cell::RefCell;
use std::process::ExitCode;
use std::rc::Rc;

/// The outcome of compiling one doctest.
#[derive(Clone, Debug, PartialEq, Eq)]
enum Outcome {
    /// The snippet behaved as its marker promised.
    Pass,
    /// It did not; the string says what was expected and what happened.
    Fail(String),
}

/// `aevia test --doc [--filter PAT] [files…]`
#[allow(
    clippy::single_call_fn,
    reason = "the test command routes documentation snippets through a separate runner"
)]
pub fn run(
    options: Options,
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
    // Share one incremental check cache across files so stdlib and package
    // dependencies are checked once rather than once per file.
    let shared_cache = Rc::new(RefCell::new(CheckCache::new()));
    for path in &paths {
        let shown = path.display().to_string();
        if let Some(pattern) = filter
            && !shown.contains(pattern)
        {
            continue;
        }
        let source = match std::fs::read_to_string(path) {
            | Ok(source) => source,
            | Err(error) => {
                eprintln!("aevia test --doc: cannot read `{shown}`: {error}");
                failed = failed.saturating_add(1);
                continue;
            },
        };
        let doctests = match extract(&source, &shown) {
            | Ok(doctests) => doctests,
            | Err(message) => {
                // A file whose documentation cannot be read is a failure, not
                // an empty success: nothing was checked.
                eprintln!("aevia test --doc: `{shown}`: {message}");
                failed = failed.saturating_add(1);
                continue;
            },
        };
        if doctests.is_empty() && !options.locked {
            continue;
        }
        let package = Package::owning(path);
        if options.locked {
            match &package {
                | Ok(Some(package)) => {
                    if let Err(code) = crate::verify_package_lock(package) {
                        return code;
                    }
                },
                | Ok(None) => {
                    eprintln!("aevia test --doc: --locked needs a package for `{shown}`");
                    return ExitCode::from(2);
                },
                | Err(error) => {
                    eprintln!("aevia test --doc: {}", error.render());
                    return ExitCode::from(1);
                },
            }
        }
        if doctests.is_empty() {
            continue;
        }
        let roots = package
            .and_then(|package| package.map_or_else(|| Ok(Roots::adjacent(path)), |p| p.roots()))
            .map_err(|error| error.render());
        for doctest in &doctests {
            checked = checked.saturating_add(1);
            let (outcome, compilation) = match &roots {
                | Ok(roots) => {
                    let (outcome, compilation) = check(doctest, roots.clone(), &shared_cache);
                    (outcome, Some(compilation))
                },
                | Err(message) => (Outcome::Fail(message.clone()), None),
            };
            match outcome {
                | Outcome::Pass => {
                    println!("doctest {}:{} ... ok", doctest.file, doctest.line);
                    passed = passed.saturating_add(1);
                },
                | Outcome::Fail(reason) => {
                    println!(
                        "doctest {}:{} ... FAILED ({reason})",
                        doctest.file, doctest.line
                    );
                    if let Some(compilation) = compilation {
                        eprint!("{}", compilation.diagnostics.render(&compilation.sources));
                    }
                    failed = failed.saturating_add(1);
                },
            }
        }
    }
    if checked == 0 && failed == 0 {
        if let Some(pattern) = filter {
            eprintln!("aevia test --doc: the filter `{pattern}` matched no doctest");
        } else {
            eprintln!("aevia test --doc: no fenced `aevia` code blocks found");
        }
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
#[allow(
    clippy::single_call_fn,
    reason = "isolates compilation and failure accounting for a single snippet"
)]
fn check(
    doctest: &Doctest,
    roots: Roots,
    cache: &Rc<RefCell<CheckCache>>,
) -> (Outcome, Program) {
    let source = candidate(doctest);
    let declaring = std::path::Path::new(&doctest.file);
    let parent = declaring
        .parent()
        .unwrap_or_else(|| std::path::Path::new("."));
    let mut serial = 0usize;
    let entry = loop {
        let entry = parent.join(format!("AeviaDoctest{}_{}.aev", doctest.line, serial));
        if !entry.exists() && entry != declaring {
            break entry;
        }
        serial = serial.saturating_add(1);
    };
    let mut overlays = SourceOverlays::new();
    overlays.insert(&entry, &source);
    let (compilation, _) = aevia_core::driver::load_incremental(
        &entry,
        roots,
        &overlays,
        &aevia_types::check::CheckOptions::default(),
        cache,
    );
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
    let prefix_len = source
        .len()
        .saturating_sub(doctest.snippet.trim_end().len().saturating_add(1));
    let entry_file = compilation.entry().map(|entry| entry.file);
    if entry_file.is_none()
        || errors.iter().any(|error| {
            Some(error.span.file) != entry_file || (error.span.start as usize) < prefix_len
        })
    {
        return (
            Outcome::Fail(format!(
                "dependency or inherited import failed ({})",
                codes_or(&codes)
            )),
            compilation,
        );
    }
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
    (outcome, compilation)
}

/// The synthetic module: copied imports, a blank line, then the snippet.
#[allow(
    clippy::single_call_fn,
    reason = "keeps generated file layout with the corresponding diagnostic remapping"
)]
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
        let doctest = doctest(snippet, expected);
        let roots = Roots::adjacent(std::path::Path::new(&doctest.file));
        let cache = Rc::new(RefCell::new(CheckCache::new()));
        check(&doctest, roots, &cache).0
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
        let roots = Roots::adjacent(std::path::Path::new(&with_import.file));
        let cache = Rc::new(RefCell::new(CheckCache::new()));
        let (outcome, _) = check(&with_import, roots, &cache);
        assert_eq!(outcome, Outcome::Pass);
        assert!(candidate(&with_import).starts_with("open Units.SI\n\n"));
    }

    #[test]
    fn snippets_reuse_imports_and_keep_separate_declarations() {
        let mut first = doctest("let first_only = 2.0 m", Expected::Compiles);
        first.imports = vec!["open Units.SI".to_string()];
        let mut second = first.clone();
        second.line += 10;
        second.snippet = "let value = first_only".to_string();
        second.expected = Expected::FailsWith("E0301".to_string());

        let roots = Roots::adjacent(std::path::Path::new(&first.file));
        let cache = Rc::new(RefCell::new(CheckCache::new()));
        assert_eq!(check(&first, roots.clone(), &cache).0, Outcome::Pass);
        assert_eq!(cache.borrow().reused, 0);
        assert_eq!(check(&second, roots, &cache).0, Outcome::Pass);
        assert!(
            cache.borrow().reused > 0,
            "the inherited modules should be reused"
        );
        assert_eq!(
            cache.borrow().checked,
            1,
            "only the second snippet needs checking"
        );
    }

    #[test]
    fn the_declaring_modules_declarations_are_not_in_scope() {
        // Isolation is deliberate: declarations require an explicit module import.
        let Outcome::Fail(reason) = outcome("let x = area(2.0 m)", Expected::Compiles) else {
            panic!("expected a failure");
        };
        assert!(reason.contains("E0301"), "{reason}");
    }
}
