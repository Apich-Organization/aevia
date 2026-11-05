//! Extracting runnable `aevia` code blocks from `##` documentation comments.
//!
//! §7.5.5 attaches Markdown to the next declaration and promises that a fenced
//! block labelled `aevia` is compile-checked (doctests). This module does the
//! extraction half: it reads the lexer's
//! [`DocComment`][aevia_syntax::token::DocComment] spans (never the
//! declarations' `doc` strings, because those are per-line `trim`med and would
//! lose the indentation of a block body), reconstructs each comment run with
//! its indentation, and returns one [`Doctest`] per `aevia` fenced block.
//!
//! A block may declare that it is *expected* to fail: a first content line of
//! `compile_fail` or `compile_fail E0401` (optionally written as a comment,
//! `# compile_fail E0401`). A bare `compile_fail` requires any error; a code
//! requires that code to be among the errors. The marker line is not part of
//! the snippet.
//!
//! The declaring module's `import`/`open` lines are carried with every
//! snippet. The module's own declarations are **not** in scope: a snippet is
//! compiled as its own synthetic module, which is what the compiler's
//! single-file [`aevia_core::compile`] can check without a package graph. A
//! snippet that needs a module function has to define what it uses; this
//! limitation is documented in the command's help and in its output.

use aevia_syntax::ast::Decl;
use aevia_syntax::ast::DeclKind;
use aevia_syntax::lex;
use aevia_syntax::parse_module;

/// What a doctest promises about its snippet.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Expected {
    /// The snippet must compile without errors.
    Compiles,
    /// `compile_fail`: the snippet must fail to compile, with any error code.
    Fails,
    /// `compile_fail EXXXX`: the snippet must fail with that code.
    FailsWith(String),
}

/// One `aevia` fenced block found in a `##` comment.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Doctest {
    /// The file the block was written in, as the caller named it.
    pub file: String,
    /// The module the block documents, when the file declares one.
    pub module: String,
    /// The 1-based line of the opening fence.
    pub line: u32,
    /// The snippet, without the fence and without the marker line.
    pub snippet: String,
    /// What the snippet is expected to do.
    pub expected: Expected,
    /// The declaring module's `import`/`open` lines, verbatim.
    pub imports: Vec<String>,
}

/// Extracts every ```aevia block from the `##` comments of one file.
///
/// # Errors
///
/// A message with the line number when the file does not lex or parse, when a
/// fence is never closed, or when a `compile_fail` marker is malformed:
/// documentation the compiler cannot read must not silently run nothing.
pub fn extract(
    source: &str,
    file: &str,
) -> Result<Vec<Doctest>, String> {
    let lexed = lex(0, source).map_err(|diagnostics| first_error(source, &diagnostics))?;
    let (module, diagnostics) = parse_module(0, source, &lexed);
    if diagnostics.has_errors() {
        return Err(first_error(source, &diagnostics));
    }
    let module_name = module
        .name
        .as_ref()
        .map(|path| path.join("."))
        .unwrap_or_else(|| file_stem(file));
    let imports: Vec<String> = module
        .decls
        .iter()
        .filter(|declaration| matches!(declaration.kind, DeclKind::Import { .. }))
        .filter_map(|declaration| slice(source, declaration).map(str::to_string))
        .filter(|text| !text.is_empty())
        .collect();

    let mut doctests = Vec::new();
    for run in doc_runs(source, &lexed.docs) {
        scan_run(run, &module_name, file, &imports, &mut doctests)?;
    }
    Ok(doctests)
}

/// One reconstructed line of a documentation comment.
struct DocLine {
    /// 1-based line in the file.
    number: u32,
    /// The comment body, with `##` and at most one following space removed.
    text: String,
}

/// A contiguous group of `##` comments that reads as one Markdown block.
type DocRun = Vec<DocLine>;

/// Rebuilds the source of every documentation-comment run.
///
/// The lexer trims each `##` line, so the spans are the only way back to the
/// original indentation — which a fenced code block needs.
fn doc_runs(
    source: &str,
    docs: &[aevia_syntax::token::DocComment],
) -> Vec<DocRun> {
    let mut runs: Vec<DocRun> = Vec::new();
    let mut previous_end: Option<usize> = None;
    for comment in docs {
        let start = usize::try_from(comment.span.start).unwrap_or(usize::MAX);
        let end = usize::try_from(comment.span.end).unwrap_or(usize::MAX);
        let Some(raw) = source.get(start..end) else {
            continue;
        };
        let text = comment_body(raw);
        let number = 1 + source[..start.min(source.len())].matches('\n').count() as u32;
        let contiguous = previous_end.is_some_and(|previous| {
            source
                .get(previous..start)
                .is_some_and(|between| between.trim().is_empty())
        });
        if contiguous {
            if let Some(run) = runs.last_mut() {
                run.push(DocLine { number, text });
            }
        } else {
            runs.push(vec![DocLine { number, text }]);
        }
        previous_end = Some(end);
    }
    runs
}

/// `## text` → `text`, preserving indentation beyond one separator space.
fn comment_body(raw: &str) -> String {
    let trimmed = raw.trim_start();
    let body = trimmed.strip_prefix("##").unwrap_or(trimmed);
    let body = body.strip_prefix(' ').unwrap_or(body);
    body.trim_end().to_string()
}

/// Walks one doc run and appends the doctests it contains.
fn scan_run(
    run: DocRun,
    module_name: &str,
    file: &str,
    imports: &[String],
    doctests: &mut Vec<Doctest>,
) -> Result<(), String> {
    let mut index = 0;
    while index < run.len() {
        let trimmed = run[index].text.trim_start();
        let Some(info) = trimmed.strip_prefix("```") else {
            index += 1;
            continue;
        };
        // Any fenced block is skipped whole; only `aevia` runs.
        let close = (index + 1..run.len())
            .find(|candidate| run[*candidate].text.trim_start().starts_with("```"));
        let Some(close) = close else {
            return Err(format!(
                "line {}: unclosed ```{info} block; add the closing fence",
                run[index].number
            ));
        };
        let language = info.trim();
        if language.eq_ignore_ascii_case("aevia") {
            let mut body: Vec<&str> = run[index + 1..close]
                .iter()
                .map(|line| line.text.as_str())
                .collect();
            // Trailing blank lines are formatting, not part of the snippet.
            while body.last().is_some_and(|line| line.trim().is_empty()) {
                body.pop();
            }
            let (expected, marker) = parse_marker(body.first().copied())
                .map_err(|message| format!("line {}: {message}", run[index].number))?;
            if marker {
                body.remove(0);
            }
            // A blank line between the fence and the code is not code.
            while body.first().is_some_and(|line| line.trim().is_empty()) {
                body.remove(0);
            }
            let snippet = body.join("\n");
            if !snippet.trim().is_empty() {
                doctests.push(Doctest {
                    file: file.to_string(),
                    module: module_name.to_string(),
                    line: run[index].number,
                    snippet,
                    expected,
                    imports: imports.to_vec(),
                });
            }
        }
        index = close + 1;
    }
    Ok(())
}

/// Reads a `compile_fail` marker from the first content line.
///
/// Returns the expectation and whether the line was a marker (and therefore
/// must not be compiled).
fn parse_marker(first: Option<&str>) -> Result<(Expected, bool), String> {
    let Some(first) = first else {
        return Ok((Expected::Compiles, false));
    };
    let trimmed = first.trim();
    let bare = trimmed.strip_prefix('#').unwrap_or(trimmed).trim_start();
    let Some(rest) = bare.strip_prefix("compile_fail") else {
        return Ok((Expected::Compiles, false));
    };
    // `compile_failure` is a comment, not a marker.
    if rest.starts_with(|character: char| character.is_alphanumeric() || character == '_') {
        return Ok((Expected::Compiles, false));
    }
    let rest = rest.trim();
    if rest.is_empty() {
        return Ok((Expected::Fails, true));
    }
    let mut parts = rest.splitn(2, char::is_whitespace);
    let code = parts.next().unwrap_or_default();
    if !is_error_code(code) {
        return Err(format!(
            "invalid compile_fail code `{code}`; expected a code such as `E0401`"
        ));
    }
    let tail = parts.next().unwrap_or_default().trim();
    if !tail.is_empty() && !tail.starts_with('#') {
        return Err(format!(
            "unexpected text after the compile_fail code: `{tail}`"
        ));
    }
    Ok((Expected::FailsWith(code.to_string()), true))
}

/// True for an appendix-B error code: `E` and four decimal digits.
fn is_error_code(code: &str) -> bool {
    let bytes = code.as_bytes();
    bytes.len() == 5 && bytes[0] == b'E' && bytes[1..].iter().all(u8::is_ascii_digit)
}

/// The file stem, for a file that declares no module.
fn file_stem(file: &str) -> String {
    std::path::Path::new(file)
        .file_stem()
        .map(|stem| stem.to_string_lossy().to_string())
        .unwrap_or_else(|| file.to_string())
}

/// The first diagnostic as `line N: code: message`.
fn first_error(
    source: &str,
    diagnostics: &aevia_syntax::Diagnostics,
) -> String {
    let Some(first) = diagnostics.items.first() else {
        return "the file does not parse".to_string();
    };
    let line = 1 + source[..usize::try_from(first.span.start)
        .unwrap_or(0)
        .min(source.len())]
        .matches('\n')
        .count();
    format!("line {line}: {}: {}", first.code, first.message)
}

/// The source text of one declaration, trimmed.
fn slice<'a>(
    source: &'a str,
    declaration: &Decl,
) -> Option<&'a str> {
    let start = usize::try_from(declaration.span.start).ok()?;
    let end = usize::try_from(declaration.span.end).ok()?;
    source.get(start..end).map(str::trim)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn extract_ok(source: &str) -> Vec<Doctest> {
        extract(source, "src/Example.aev").expect("extracts")
    }

    #[test]
    fn a_fenced_block_becomes_one_doctest_with_its_line() {
        let source = "## A square's area.\n##\n## ```aevia\n## let side = 2.0 m\n## let area = side * side\n## ```\nfn area(side: Real[L]): Real[L^2] = side * side\n";
        let found = extract_ok(source);
        assert_eq!(found.len(), 1, "{found:#?}");
        assert_eq!(found[0].line, 3);
        assert_eq!(found[0].snippet, "let side = 2.0 m\nlet area = side * side");
        assert_eq!(found[0].expected, Expected::Compiles);
        assert_eq!(found[0].module, "Example");
        assert_eq!(found[0].file, "src/Example.aev");
    }

    #[test]
    fn several_blocks_are_all_found_in_source_order() {
        let source = "## First.\n##\n## ```aevia\n## let a = 1\n## ```\nfn f(): Int = 1\n\n## Second.\n##\n## ```aevia\n## let b = 2\n## ```\nfn g(): Int = 2\n";
        let found = extract_ok(source);
        assert_eq!(found.len(), 2, "{found:#?}");
        assert_eq!(found[0].snippet, "let a = 1");
        assert_eq!(found[1].snippet, "let b = 2");
        assert!(found[0].line < found[1].line);
    }

    #[test]
    fn only_aevia_blocks_run() {
        let source = "## ```rust\n## let not_aevia = true;\n## ```\n##\n## ```aevia\n## let a = 1\n## ```\n##\n## ```\n## plain fence\n## ```\nfn f(): Int = 1\n";
        let found = extract_ok(source);
        assert_eq!(found.len(), 1, "{found:#?}");
        assert_eq!(found[0].snippet, "let a = 1");
    }

    #[test]
    fn a_compile_fail_marker_names_the_expected_code() {
        let source = "## ```aevia\n## compile_fail E0401\n## let broken = 1 m + 1\n## ```\nfn broken(): Int = 1\n";
        let found = extract_ok(source);
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].expected, Expected::FailsWith("E0401".to_string()));
        assert_eq!(found[0].snippet, "let broken = 1 m + 1");
    }

    #[test]
    fn a_compile_fail_marker_without_a_code_only_requires_failure() {
        let source = "## ```aevia\n## compile_fail\n## let broken: Int = \"s\"\n## ```\nfn broken(): Int = 1\n";
        let found = extract_ok(source);
        assert_eq!(found[0].expected, Expected::Fails);
        assert_eq!(found[0].snippet, "let broken: Int = \"s\"");
    }

    #[test]
    fn the_marker_may_be_written_as_a_hash_comment() {
        let source = "## ```aevia\n## # compile_fail E0302\n## let broken = 1 m + 1\n## ```\nfn broken(): Int = 1\n";
        let found = extract_ok(source);
        assert_eq!(found[0].expected, Expected::FailsWith("E0302".to_string()));
        assert_eq!(found[0].snippet, "let broken = 1 m + 1");
    }

    #[test]
    fn a_bad_marker_is_refused_with_its_line() {
        let error = extract(
            "## ```aevia\n## compile_fail NOPE\n## let x = 1\n## ```\nfn f(): Int = 1\n",
            "a.aev",
        )
        .expect_err("refuses");
        assert!(error.contains("invalid compile_fail code"), "{error}");
        assert!(error.contains("line"), "{error}");
    }

    #[test]
    fn an_unclosed_fence_is_refused() {
        let error =
            extract("## ```aevia\n## let x = 1\nfn f(): Int = 1\n", "a.aev").expect_err("refuses");
        assert!(error.contains("unclosed"), "{error}");
    }

    #[test]
    fn a_block_keeps_its_indentation() {
        let source = "## ```aevia\n## fn double(x: Int): Int =\n##     x + x\n## ```\nfn double(x: Int): Int = x + x\n";
        let found = extract_ok(source);
        assert_eq!(
            found[0].snippet, "fn double(x: Int): Int =\n    x + x",
            "{}",
            found[0].snippet
        );
    }

    #[test]
    fn the_declaring_modules_imports_are_carried() {
        let source = "import Geometry.Shapes as S\nopen Units.SI\n\n## ```aevia\n## let d = 2.0 m\n## ```\nfn f(): Int = 1\n";
        let found = extract_ok(source);
        assert_eq!(
            found[0].imports,
            ["import Geometry.Shapes as S", "open Units.SI"]
        );
    }

    #[test]
    fn a_declared_module_name_is_reported() {
        let source = "module Geo.Shapes\n\n## ```aevia\n## let x = 1\n## ```\nfn f(): Int = 1\n";
        let found = extract_ok(source);
        assert_eq!(found[0].module, "Geo.Shapes");
    }

    #[test]
    fn a_file_that_does_not_parse_is_refused() {
        let error = extract("fn f( = 1\n", "a.aev").expect_err("refuses");
        assert!(error.contains("line 1"), "{error}");
    }

    #[test]
    fn a_file_without_blocks_has_no_doctests() {
        assert!(extract_ok("## Just prose.\nfn f(): Int = 1\n").is_empty());
        assert!(extract_ok("fn f(): Int = 1\n").is_empty());
    }

    #[test]
    fn a_comment_that_merely_mentions_compile_fail_is_not_a_marker() {
        let source = "## ```aevia\n## let compile_failure = 1\n## ```\nfn f(): Int = 1\n";
        let found = extract_ok(source);
        assert_eq!(found[0].expected, Expected::Compiles);
        assert_eq!(found[0].snippet, "let compile_failure = 1");
    }
}
