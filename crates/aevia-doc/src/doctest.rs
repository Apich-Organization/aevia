//! Extracting runnable `aevia` code blocks from `##` documentation comments.
//!
//! §7.5.5 attaches Markdown to the next declaration and promises that a fenced
//! block labelled `aevia` is compile-checked (doctests). This module does the
//! extraction half: it reads the lexer's
//! [`DocComment`][aevia_syntax::token::DocComment] spans (never the
//! declarations' `doc` strings, because those are per-line `trim`med and would
//! lose the indentation of a block body), reconstructs each comment run with
//! its indentation, and returns one [`Doctest`] per backtick or tilde fence
//! labelled `aevia`.
//!
//! A block may declare that it is *expected* to fail: a first content line of
//! `compile_fail` or `compile_fail E0401` (optionally written as a comment,
//! `# compile_fail E0401`). A bare `compile_fail` requires any error; a code
//! requires that code to be among the errors. The marker line is not part of
//! the snippet.
//!
//! Each snippet compiles as its own synthetic module with the declaring
//! module's `import` and `open` lines. Define any local helpers in the block.

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

/// Extracts every fenced `aevia` block from the `##` comments of one file.
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
    let lexed = lex(0, source).map_err(|diagnostics| super::first_error(source, &diagnostics))?;
    let (module, diagnostics) = parse_module(0, source, &lexed);
    if diagnostics.has_errors() {
        return Err(super::first_error(source, &diagnostics));
    }
    let module_name = module.name.as_ref().map_or_else(
        || {
            std::path::Path::new(file).file_stem().map_or_else(
                || file.to_string(),
                |stem| stem.to_string_lossy().into_owned(),
            )
        },
        |path| path.join("."),
    );
    let imports: Vec<String> = module
        .decls
        .iter()
        .filter(|declaration| matches!(declaration.kind, DeclKind::Import { .. }))
        .filter_map(|declaration| {
            let start = usize::try_from(declaration.span.start).ok()?;
            let end = usize::try_from(declaration.span.end).ok()?;
            source.get(start..end).map(str::trim)
        })
        .filter(|text| !text.is_empty())
        .map(str::to_owned)
        .collect();

    let mut doctests = Vec::new();
    for run in doc_runs(source, &lexed.docs) {
        scan_run(&run, &module_name, file, &imports, &mut doctests)?;
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
#[cfg_attr(
    not(test),
    expect(
        clippy::single_call_fn,
        reason = "source span reconstruction precedes Markdown scanning"
    )
)]
fn doc_runs(
    source: &str,
    docs: &[aevia_syntax::token::DocComment],
) -> Vec<DocRun> {
    let mut runs: Vec<DocRun> = Vec::new();
    let mut previous_end: Option<usize> = None;
    let mut line_breaks = source
        .match_indices(['\r', '\n'])
        .filter(|(offset, terminator)| {
            *terminator == "\n"
                || !source
                    .get(*offset..)
                    .is_some_and(|tail| tail.starts_with("\r\n"))
        })
        .map(|(offset, _)| offset)
        .peekable();
    let mut number = 1u32;
    for comment in docs {
        let start = usize::try_from(comment.span.start).unwrap_or(usize::MAX);
        let end = usize::try_from(comment.span.end).unwrap_or(usize::MAX);
        let Some(raw) = source.get(start..end) else {
            continue;
        };
        let trimmed = raw.trim_start();
        let body = trimmed.strip_prefix("##").unwrap_or(trimmed);
        let text = body.strip_prefix(' ').unwrap_or(body).trim_end().to_owned();
        while line_breaks.next_if(|offset| *offset < start).is_some() {
            number = number.saturating_add(1);
        }
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

/// Walks one doc run and appends the doctests it contains.
#[cfg_attr(
    not(test),
    expect(
        clippy::single_call_fn,
        reason = "Markdown scanning operates on the reconstructed comment lines"
    )
)]
fn scan_run(
    run: &[DocLine],
    module_name: &str,
    file: &str,
    imports: &[String],
    doctests: &mut Vec<Doctest>,
) -> Result<(), String> {
    let mut lines = run.iter();
    while let Some(open) = lines.next() {
        let Some((marker, width, info)) = fence(&open.text) else {
            continue;
        };
        let runnable = info.eq_ignore_ascii_case("aevia");
        let mut body = Vec::new();
        let mut closed = false;
        for line in lines.by_ref() {
            if fence(&line.text).is_some_and(|(end, length, info)| {
                marker == end && length >= width && info.is_empty()
            }) {
                closed = true;
                break;
            }
            if runnable {
                body.push(line.text.as_str());
            }
        }
        if !closed {
            return Err(format!(
                "line {}: unclosed {} block; add the closing fence",
                open.number,
                open.text.trim_start(),
            ));
        }
        if runnable {
            while body.last().is_some_and(|line| line.trim().is_empty()) {
                body.pop();
            }
            let mut body = body
                .into_iter()
                .skip_while(|line| line.trim().is_empty())
                .peekable();
            let (expected, marker) = parse_marker(body.peek().copied())
                .map_err(|message| format!("line {}: {message}", open.number))?;
            if marker {
                body.next();
            }
            let snippet = body
                .skip_while(|line| line.trim().is_empty())
                .collect::<Vec<_>>()
                .join("\n");
            if !snippet.is_empty() {
                doctests.push(Doctest {
                    file: file.to_string(),
                    module: module_name.to_string(),
                    line: open.number,
                    snippet,
                    expected,
                    imports: imports.to_vec(),
                });
            }
        }
    }
    Ok(())
}

fn fence(line: &str) -> Option<(char, usize, &str)> {
    let line = line.trim_start();
    let marker = line.chars().next()?;
    if !matches!(marker, '`' | '~') {
        return None;
    }
    let rest = line.trim_start_matches(marker);
    let width = line.len().checked_sub(rest.len())?;
    (width >= 3).then_some((marker, width, rest.trim()))
}

/// Reads a `compile_fail` marker from the first content line.
///
/// Returns the expectation and whether the line was a marker (and therefore
/// must not be compiled).
#[cfg_attr(
    not(test),
    expect(
        clippy::single_call_fn,
        reason = "compile-fail markers have a separate grammar from Markdown fences"
    )
)]
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
    if !matches!(code.as_bytes(), [b'E', digits @ ..] if digits.len() == 4 && digits.iter().all(u8::is_ascii_digit))
    {
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
    fn fences_match_the_opening_width_and_markers_follow_empty_lines() {
        let source = concat!(
            "module Demo\r\n",
            "## ````markdown\r\n",
            "## ```aevia\r\n",
            "## let hidden = 1\r\n",
            "## ```\r\n",
            "## ````\r\n",
            "## ````aevia\r\n",
            "##\r\n",
            "## # compile_fail E0401\r\n",
            "## let wrong = 1 m + 2 s\r\n",
            "## ````\r\n",
            "let documented = 42\r\n",
        );
        let tests = extract_ok(source);
        assert_eq!(tests.len(), 1);
        assert_eq!(tests[0].line, 7);
        assert_eq!(tests[0].expected, Expected::FailsWith("E0401".into()));
        assert_eq!(tests[0].snippet, "let wrong = 1 m + 2 s");
    }

    #[test]
    fn tilde_fences_use_their_own_delimiter_and_minimum_width() {
        let source = concat!(
            "## ~~~~markdown\n",
            "## ```aevia\n",
            "## let hidden = 1\n",
            "## ```\n",
            "## ~~~~\n",
            "## ~~~~aevia\n",
            "## let example: Int = 2\n",
            "## ~~~~~\n",
            "let documented = 2\n",
        );
        let tests = extract_ok(source);
        assert_eq!(tests.len(), 1);
        assert_eq!(tests[0].line, 6);
        assert_eq!(tests[0].snippet, "let example: Int = 2");
        assert_eq!(tests[0].expected, Expected::Compiles);
        let unclosed = "## ~~~~aevia\n## let example = 2\n## ~~~\nlet documented = 2\n";
        assert!(extract(unclosed, "short.aev").is_err());
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
