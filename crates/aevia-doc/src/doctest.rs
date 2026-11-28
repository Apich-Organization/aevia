//! Extracting runnable `aevia` code blocks from `##` documentation comments.
//!
//! §7.5.5 attaches Markdown to the next declaration and promises that a fenced
//! block labelled `aevia` is compile-checked (doctests). This module does the
//! extraction half reads the lexer's
//! [`DocComment`][aevia_syntax::token::DocComment] spans to preserve the body
//! of each backtick or tilde fence labelled `aevia`. The returned line range
//! can map diagnostics from the snippet back to the documentation source.
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
use aevia_syntax::token::DocComment;
use std::ops::RangeInclusive;

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

/// A doctest paired with the source coordinates of its snippet.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LocatedDoctest {
    /// The extracted doctest and its declaring module context.
    pub doctest: Doctest,
    /// Inclusive, 1-based lines of the code in the original source file.
    pub snippet_lines: RangeInclusive<u32>,
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
    Ok(extract_with_locations(source, file)?
        .into_iter()
        .map(|located| located.doctest)
        .collect())
}

/// Extracts doctests and the original source lines covered by each snippet.
///
/// Each line of the snippet corresponds to one line of `snippet_lines`, in
/// order. The range excludes the fences, the expectation marker, and blank
/// lines trimmed from either end. Line endings in snippets are normalized to
/// LF.
///
/// # Errors
/// Returns the same extraction errors as [`extract`].
pub fn extract_with_locations(
    source: &str,
    file: &str,
) -> Result<Vec<LocatedDoctest>, String> {
    let mut lexed =
        lex(0, source).map_err(|diagnostics| super::first_error(source, &diagnostics))?;
    let comments = std::mem::take(&mut lexed.docs);
    let (module, diagnostics) = parse_module(0, source, &lexed);
    if diagnostics.has_errors() {
        return Err(super::first_error(source, &diagnostics));
    }
    let snippets = scan_comments(source, &comments)?;
    if snippets.is_empty() {
        return Ok(Vec::new());
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

    Ok(snippets
        .into_iter()
        .map(|snippet| {
            LocatedDoctest {
                doctest: Doctest {
                    file: file.to_owned(),
                    module: module_name.clone(),
                    line: snippet.opening_line,
                    snippet: snippet.text,
                    expected: snippet.expected,
                    imports: imports.clone(),
                },
                snippet_lines: snippet.lines,
            }
        })
        .collect())
}

struct Snippet {
    opening_line: u32,
    lines: RangeInclusive<u32>,
    expected: Expected,
    text: String,
}

#[cfg_attr(
    not(test),
    allow(
        clippy::single_call_fn,
        reason = "scans Markdown fences independently of the parsed module context"
    )
)]
fn scan_comments(
    source: &str,
    comments: &[DocComment],
) -> Result<Vec<Snippet>, String> {
    let mut snippets = Vec::new();
    let mut open: Option<Block<'_>> = None;
    let mut previous_end = 0;
    let mut number = 1u32;
    for comment in comments {
        let start = usize::try_from(comment.span.start).unwrap_or(usize::MAX);
        let end = usize::try_from(comment.span.end).unwrap_or(usize::MAX);
        let (Some(raw), Some(gap)) = (source.get(start..end), source.get(previous_end..start))
        else {
            continue;
        };
        let breaks = line_breaks(gap);
        number = number.saturating_add(breaks);
        previous_end = end;
        let text = super::comment_body(raw);
        let marker = fence(text);
        if let Some(block) = &mut open {
            if !gap.trim().is_empty() {
                return Err(block.unclosed());
            }
            if marker.is_some_and(|end| block.fence.closes(&end)) {
                if let Some(block) = open.take()
                    && let Some(snippet) = block.finish()?
                {
                    snippets.push(snippet);
                }
            } else if block.fence.info.eq_ignore_ascii_case("aevia") {
                block.push(text, number, breaks)?;
            }
        } else if let Some(fence) = marker {
            open = Some(Block {
                fence,
                opening_line: number,
                opening_text: text.trim_start(),
                expectation: None,
                first_line: None,
                last_content: (number, 0),
                text: String::new(),
            });
        }
    }
    if let Some(block) = open {
        return Err(block.unclosed());
    }
    Ok(snippets)
}

#[allow(
    clippy::single_call_fn,
    reason = "counts physical lines for source mapping"
)]
fn line_breaks(text: &str) -> u32 {
    let mut count = 0u32;
    let mut previous_cr = false;
    for byte in text.bytes() {
        if byte == b'\r' || (byte == b'\n' && !previous_cr) {
            count = count.saturating_add(1);
        }
        previous_cr = byte == b'\r';
    }
    count
}

struct Block<'a> {
    fence: Fence<'a>,
    opening_line: u32,
    opening_text: &'a str,
    expectation: Option<(Expected, u32)>,
    first_line: Option<u32>,
    last_content: (u32, usize),
    text: String,
}

impl Block<'_> {
    #[allow(clippy::single_call_fn, reason = "assembles one runnable block")]
    fn push(
        &mut self,
        text: &str,
        number: u32,
        breaks: u32,
    ) -> Result<(), String> {
        let blank = text.trim().is_empty();
        if self.first_line.is_none() {
            if blank {
                return Ok(());
            }
            if self.expectation.is_none()
                && let Some(expected) =
                    parse_marker(text).map_err(|message| format!("line {number}: {message}"))?
            {
                self.expectation = Some((expected, number));
                return Ok(());
            }
            self.first_line = Some(number);
        } else if let Ok(breaks) = usize::try_from(breaks) {
            self.text.extend(std::iter::repeat_n('\n', breaks));
        }
        let indent = text
            .bytes()
            .take(self.fence.indent)
            .take_while(|byte| *byte == b' ')
            .count();
        self.text.push_str(text.get(indent..).unwrap_or(text));
        if !blank {
            self.last_content = (number, self.text.len());
        }
        Ok(())
    }

    #[allow(
        clippy::single_call_fn,
        reason = "validates and finishes one fenced block"
    )]
    fn finish(mut self) -> Result<Option<Snippet>, String> {
        let Some(first) = self.first_line else {
            return match self.expectation {
                | Some((_, line)) => {
                    Err(format!(
                        "line {line}: compile_fail marker requires a snippet"
                    ))
                },
                | None => Ok(None),
            };
        };
        let (last, end) = self.last_content;
        self.text.truncate(end);
        Ok(Some(Snippet {
            opening_line: self.opening_line,
            lines: first..=last,
            expected: self
                .expectation
                .map_or(Expected::Compiles, |(expected, _)| expected),
            text: self.text,
        }))
    }

    fn unclosed(&self) -> String {
        format!(
            "line {}: unclosed {} block; add the closing fence",
            self.opening_line, self.opening_text
        )
    }
}

struct Fence<'a> {
    marker: char,
    width: usize,
    info: &'a str,
    indent: usize,
}

impl Fence<'_> {
    #[allow(
        clippy::single_call_fn,
        reason = "keeps the closing delimiter rules with fence metadata"
    )]
    const fn closes(
        &self,
        end: &Fence<'_>,
    ) -> bool {
        self.marker == end.marker && self.width <= end.width && end.info.is_empty()
    }
}

#[allow(
    clippy::single_call_fn,
    reason = "recognizes Markdown delimiters in the source stream"
)]
fn fence(line: &str) -> Option<Fence<'_>> {
    let trimmed = line.trim_start();
    let marker = trimmed.chars().next()?;
    if !matches!(marker, '`' | '~') {
        return None;
    }
    let rest = trimmed.trim_start_matches(marker);
    let width = trimmed.len().checked_sub(rest.len())?;
    (width >= 3).then(|| {
        Fence {
            marker,
            width,
            info: rest.trim(),
            indent: line.bytes().take_while(|byte| *byte == b' ').count(),
        }
    })
}

/// Reads an optional `compile_fail` expectation from the first content line.
#[cfg_attr(
    not(test),
    expect(
        clippy::single_call_fn,
        reason = "compile-fail markers have a separate grammar from Markdown fences"
    )
)]
fn parse_marker(first: &str) -> Result<Option<Expected>, String> {
    let trimmed = first.trim();
    let bare = trimmed.strip_prefix('#').unwrap_or(trimmed).trim_start();
    let Some(rest) = bare.strip_prefix("compile_fail") else {
        return Ok(None);
    };
    // `compile_failure` is a comment, not a marker.
    if rest.starts_with(|character: char| character.is_alphanumeric() || character == '_') {
        return Ok(None);
    }
    let rest = rest.trim();
    if rest.is_empty() {
        return Ok(Some(Expected::Fails));
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
    Ok(Some(Expected::FailsWith(code.to_string())))
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
            "## ```aevia\r\n##\r## compile_fail NOPE\n## let x = 1\r\n## ```\nfn f(): Int = 1\n",
            "a.aev",
        )
        .expect_err("refuses");
        assert!(
            error.starts_with("line 3: invalid compile_fail code"),
            "{error}"
        );
    }

    #[test]
    fn a_compile_failure_requires_a_snippet() {
        let source = "## ```aevia\n##\n## # compile_fail E0401\n##\n## ```\nlet author = 1\n";
        let error = extract(source, "empty.aev").unwrap_err();
        assert_eq!(error, "line 3: compile_fail marker requires a snippet");
    }

    #[test]
    fn located_snippets_retain_every_physical_body_line_and_literal_spaces() {
        let source = concat!(
            "module Mixed\r\n",
            "## ```aevia\r",
            "##\r\n",
            "## # compile_fail E0401\n",
            "\r",
            "## let message = \"\"\"\r\n",
            "##   one  \r",
            "\n\n",
            "##   \n",
            "##   two\r\n",
            "## \"\"\"\n",
            "##\r",
            "## ```\r\n",
            "let author = 1\n",
        );
        let locations = extract_with_locations(source, "mixed.aev").unwrap();
        let [located] = locations.as_slice() else {
            panic!("one snippet")
        };
        assert_eq!(located.doctest.line, 2);
        assert_eq!(located.snippet_lines, 6..=11);
        assert_eq!(
            located.doctest.expected,
            Expected::FailsWith("E0401".into())
        );
        assert_eq!(
            located.doctest.snippet,
            "let message = \"\"\"\n  one  \n\n  \n  two\n\"\"\""
        );
    }

    #[test]
    fn nested_fences_remove_their_base_indentation() {
        let found = extract_ok(concat!(
            "## - Example:\n",
            "##   ```aevia\n",
            "##   fn twice(x: Int): Int =\n",
            "##       x + x\n",
            "##   ```\n",
            "let author = 1\n",
        ));
        assert_eq!(found[0].line, 2);
        assert_eq!(found[0].snippet, "fn twice(x: Int): Int =\n    x + x");
    }

    #[test]
    fn a_fence_cannot_continue_past_a_declaration() {
        let source = "## ```aevia\n## let x = 1\nlet a = 1\n## ```\nlet b = 2\n";
        let error = extract(source, "split.aev").unwrap_err();
        assert!(error.starts_with("line 1: unclosed"), "{error}");
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
