//! `aevia fmt` — a formatter over the AST.
//!
//! The two properties that matter are checked, not asserted:
//!
//! * **semantics-preserving** — the printed text is re-parsed and compared to
//!   the original tree by [`fingerprint`], which ignores spans and therefore
//!   ignores exactly the whitespace the formatter changes;
//! * **idempotent** — formatting the output again produces the same bytes.
//!
//! If either check fails, or a construct has no faithful source form, the
//! caller gets an error naming the construct and the file is left untouched. A
//! formatter that quietly rewrites a file it does not understand is the one
//! outcome that must not happen.
//!
//! Comments are kept, including comments inside string interpolations. The
//! formatter preserves the spelling of interpolations that depend on source
//! layout, nesting or comments. LF, CRLF and bare CR are supported.
//!
//! ```
//! use aevia_fmt::format_source;
//!
//! let source = "let  x=1+2\n";
//! let formatted = format_source(source).expect("formats");
//! assert_eq!(formatted, "let x = 1 + 2\n");
//! assert_eq!(format_source(&formatted).expect("idempotent"), formatted);
//! ```

#![forbid(unsafe_code)]
#![cfg_attr(
    not(test),
    deny(
        missing_docs,
        clippy::arithmetic_side_effects,
        clippy::expect_used,
        clippy::indexing_slicing,
        clippy::nursery,
        clippy::panic,
        clippy::pedantic,
        clippy::single_call_fn,
        clippy::unwrap_used
    )
)]

pub mod comments;
pub mod fingerprint;
pub mod print;

use aevia_syntax::Diagnostic;
use aevia_syntax::Module;
use aevia_syntax::lex;
use aevia_syntax::parse_module;

/// Why a file was not formatted.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Failure {
    /// The source does not lex.
    Lex,
    /// The source does not parse. A file with a syntax error is left alone:
    /// there is no tree to print, and guessing at the intent is how a
    /// formatter corrupts code.
    Syntax,
    /// A construct has no faithful source form.
    Unprintable,
    /// The printed text re-parses to a different tree.
    TreeChanged,
    /// Formatting twice differs from formatting once.
    NotIdempotent,
    /// A comment would be lost.
    CommentLost,
}

/// A formatting failure: what kind, and which construct.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FormatError {
    /// The check that failed.
    pub failure: Failure,
    /// A message naming the construct, with the line where there is one.
    pub message: String,
}

impl FormatError {
    fn new(
        failure: Failure,
        message: impl Into<String>,
    ) -> Self {
        Self {
            failure,
            message: message.into(),
        }
    }
}

impl std::fmt::Display for FormatError {
    fn fmt(
        &self,
        f: &mut std::fmt::Formatter<'_>,
    ) -> std::fmt::Result {
        f.write_str(&self.message)
    }
}

/// Formats one file.
///
/// # Errors
///
/// [`FormatError`] when the file cannot be formatted faithfully.
pub fn format_source(source: &str) -> Result<String, FormatError> {
    let line_ending = if source.contains("\r\n") {
        "\r\n"
    } else if source.contains('\r') {
        "\r"
    } else {
        "\n"
    };
    let original = parse(source)?;
    let once = original.print(line_ending)?;
    // Printing the same bytes also preserves the parsed tree and trivia.
    if once == source {
        return Ok(once);
    }
    let printed = parse(&once).map_err(|error| {
        FormatError::new(
            Failure::TreeChanged,
            format!("the printed file does not parse again ({})", error.message),
        )
    })?;
    verify(&original, &printed)?;

    // The idempotence check is part of the contract, so it is enforced here
    // using the same parsed result as the structural check. If the second
    // print is byte-identical, parsing it again cannot reveal a difference.
    let twice = printed.print(line_ending)?;
    if twice != once {
        return Err(FormatError::new(
            Failure::NotIdempotent,
            "formatting the formatted file changes it again",
        ));
    }
    Ok(once)
}

/// True when `source` is already in formatted form.
///
/// # Errors
///
/// [`FormatError`] when the file cannot be formatted faithfully.
pub fn is_formatted(source: &str) -> Result<bool, FormatError> {
    Ok(format_source(source)? == source)
}

struct ParsedSource<'a> {
    source: &'a str,
    module: Module,
    comments: Vec<comments::Comment>,
    preserved_strings: Vec<aevia_syntax::Span>,
}

impl ParsedSource<'_> {
    fn print(
        &self,
        line_ending: &'static str,
    ) -> Result<String, FormatError> {
        print::Printer::new(self.comments.clone())
            .with_source(self.source)
            .with_source_layout(line_ending, self.preserved_strings.clone())
            .module(&self.module)
            .map_err(|message| FormatError::new(Failure::Unprintable, message))
    }

    fn preserved_text(&self) -> impl Iterator<Item = Option<&str>> {
        self.preserved_strings.iter().map(|span| {
            self.source
                .get(usize::try_from(span.start).ok()?..usize::try_from(span.end).ok()?)
        })
    }
}

#[allow(
    clippy::single_call_fn,
    reason = "keeps verification of emitted source separate from rendering"
)]
fn verify(
    before: &ParsedSource<'_>,
    after: &ParsedSource<'_>,
) -> Result<(), FormatError> {
    let original = fingerprint::module(&before.module);
    let printed = fingerprint::module(&after.module);
    if original != printed {
        return Err(FormatError::new(
            Failure::TreeChanged,
            format!(
                "{}: the printed file means something else ({})",
                blame(&before.module, &after.module),
                first_difference(&original, &printed)
            ),
        ));
    }
    if !comments_match(&before.comments, &after.comments) {
        return Err(FormatError::new(
            Failure::CommentLost,
            "a comment would not survive formatting",
        ));
    }
    if !before.preserved_text().eq(after.preserved_text()) {
        return Err(FormatError::new(
            Failure::Unprintable,
            "the source spelling of a string interpolation would change",
        ));
    }
    Ok(())
}

/// Parses one file, reporting the first diagnostic with its line.
fn parse(source: &str) -> Result<ParsedSource<'_>, FormatError> {
    let lexed = lex(0, source).map_err(|diagnostics| {
        FormatError::new(
            Failure::Lex,
            format!(
                "{} (the file was not changed)",
                describe(source, &diagnostics)
            ),
        )
    })?;
    let (module, diagnostics) = parse_module(0, source, &lexed);
    if diagnostics.has_errors() {
        return Err(FormatError::new(
            Failure::Syntax,
            format!(
                "{} (the file was not changed)",
                describe(source, &diagnostics)
            ),
        ));
    }
    let mut comments = comments::scan_lexed(source, &lexed.tokens);
    // The parser consumes docs only before top-level declarations. Preserve
    // unattached docs as trivia, including a file that consists only of docs.
    let last_decl_start = module.decls.last().map(|decl| decl.span.start);
    let attached = lexed
        .docs
        .partition_point(|doc| last_decl_start.is_some_and(|start| doc.span.end <= start));
    let ordinary_count = comments.len();
    comments.extend(lexed.docs.iter().skip(attached).filter_map(|doc| {
        let start = usize::try_from(doc.span.start).ok()?;
        let end = usize::try_from(doc.span.end).ok()?;
        Some(comments::Comment {
            start,
            end,
            text: source.get(start..end)?.trim_end().to_string(),
            block: false,
        })
    }));
    if comments.len() > ordinary_count {
        comments.sort_unstable_by_key(|comment| comment.start);
    }
    Ok(ParsedSource {
        source,
        module,
        comments,
        preserved_strings: comments::protected_strings(source, &lexed.tokens),
    })
}

fn describe(
    source: &str,
    diagnostics: &aevia_syntax::Diagnostics,
) -> String {
    let Some(first) = diagnostics
        .items
        .iter()
        .find(|item| item.severity == aevia_syntax::Severity::Error)
    else {
        return "the file does not parse".to_string();
    };
    let offset = usize::try_from(first.span.start).unwrap_or(usize::MAX);
    let prefix = source.get(..offset).unwrap_or(source);
    let line = comments::physical_lines(prefix).count().max(1);
    format!("line {line}: {}: {}", first.code, first.message)
}

/// The declaration whose printed form differs, for the failure message.
#[allow(
    clippy::single_call_fn,
    reason = "isolates the declaration-level diagnostic from tree comparison"
)]
fn blame(
    original: &Module,
    printed: &Module,
) -> String {
    if original.decls.len() != printed.decls.len() {
        return format!(
            "the file has {} declarations and prints as {}",
            original.decls.len(),
            printed.decls.len()
        );
    }
    for (left, right) in original.decls.iter().zip(&printed.decls) {
        if fingerprint::declaration(left) != fingerprint::declaration(right) {
            return print::describe_decl(left);
        }
    }
    "the module header".to_string()
}

/// Compares comment texts after allowing for line endings and trailing space.
#[allow(
    clippy::single_call_fn,
    reason = "compares comment trivia independently of the AST"
)]
fn comments_match(
    before: &[comments::Comment],
    after: &[comments::Comment],
) -> bool {
    before.len() == after.len()
        && before.iter().zip(after).all(|(left, right)| {
            left.block == right.block
                && comments::physical_lines(&left.text)
                    .map(str::trim_end)
                    .eq(comments::physical_lines(&right.text).map(str::trim_end))
        })
}

/// The first place two fingerprints differ, as a short excerpt.
#[allow(
    clippy::single_call_fn,
    reason = "keeps the bounded fingerprint excerpt out of the formatting pass"
)]
fn first_difference(
    left: &str,
    right: &str,
) -> String {
    let at = left
        .chars()
        .zip(right.chars())
        .take_while(|(a, b)| a == b)
        .count();
    let from = at.saturating_sub(20);
    let left: String = left.chars().skip(from).take(60).collect();
    let right: String = right.chars().skip(from).take(60).collect();
    format!("`{left}` became `{right}`")
}

/// A diagnostic for a caller that wants to print one.
#[must_use]
pub fn diagnostic(error: &FormatError) -> Diagnostic {
    Diagnostic::error(
        "E0202",
        error.message.clone(),
        aevia_syntax::Span::default(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn formatting_normalises_spacing_without_changing_the_program() {
        let source = "let  x=1+2*3\nfn f(a:Int):Int=a  +1\n";
        let formatted = format_source(source).expect("formats");
        assert_eq!(
            formatted,
            "let x = 1 + 2 * 3\n\nfn f(a: Int): Int = a + 1\n"
        );
        assert_eq!(format_source(&formatted).expect("again"), formatted);
    }

    #[test]
    fn a_syntax_error_leaves_the_file_alone() {
        let source = "fn f( = 1\n";
        let error = format_source(source).expect_err("refuses");
        assert_eq!(error.failure, Failure::Syntax);
        assert!(error.message.contains("line 1"), "{}", error.message);
    }

    #[test]
    fn a_construct_that_cannot_be_printed_is_named_and_refused() {
        // An untyped class method parses as `() -> ()`, which has no source
        // form: `() -> ()` re-parses as one unit argument. The formatter must
        // refuse and name the declaration, not print something else.
        let source = "class Pretty a where\n    show_it()\n";
        let error = format_source(source).expect_err("refuses");
        assert!(error.message.contains("class Pretty"), "{}", error.message);
        assert!(
            matches!(error.failure, Failure::Unprintable | Failure::TreeChanged),
            "{:?}",
            error.failure
        );
    }

    #[test]
    fn comments_survive() {
        let source = "# leading\nlet x = 1 # trailing\n\n# last\n";
        let formatted = format_source(source).expect("formats");
        assert!(formatted.contains("# leading"));
        assert!(formatted.contains("# trailing"));
        assert!(formatted.contains("# last"));
    }

    #[test]
    fn shebang_precedes_module_header() {
        let source = "#!/usr/bin/env -S aevia run\nmodule Sample\nlet  x=1\n";
        let expected = "#!/usr/bin/env -S aevia run\nmodule Sample\n\nlet x = 1\n";
        assert_eq!(format_source(source).expect("formats"), expected);
        assert_eq!(
            format_source(&format!("\u{feff}{source}")).expect("formats with BOM"),
            expected
        );
    }

    #[test]
    fn precedence_is_recovered_from_the_tree() {
        // Brackets are not in the AST, so `(1 + 2) * 3` has to be printed with
        // its brackets back; `1 + 2 * 3` must not gain any.
        for source in ["let a = (1 + 2) * 3\n", "let b = 1 + 2 * 3\n"] {
            let formatted = format_source(source).expect("formats");
            assert_eq!(formatted, source);
        }
        let source = "let c = 1 - (2 - 3)\n";
        assert_eq!(format_source(source).expect("formats"), source);
    }

    #[test]
    fn a_crlf_file_stays_crlf() {
        // Rewriting every line ending would bury the change in a whole-file
        // diff, which is exactly what a formatter must not do on Windows.
        let source = "let  x=1\r\n";
        let formatted = format_source(source).expect("formats");
        assert_eq!(formatted, "let x = 1\r\n");
        assert_eq!(format_source(&formatted).expect("again"), formatted);
    }
}
