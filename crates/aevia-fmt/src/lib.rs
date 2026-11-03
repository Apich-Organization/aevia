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
//! ```
//! use aevia_fmt::format_source;
//!
//! let source = "let  x=1+2\n";
//! let formatted = format_source(source).expect("formats");
//! assert_eq!(formatted, "let x = 1 + 2\n");
//! assert_eq!(format_source(&formatted).expect("idempotent"), formatted);
//! ```

#![forbid(unsafe_code)]

pub mod comments;
pub mod fingerprint;
pub mod print;

use aevia_syntax::{lex, parse_module, Diagnostic, Module};

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
    fn new(failure: Failure, message: impl Into<String>) -> Self {
        Self {
            failure,
            message: message.into(),
        }
    }
}

impl std::fmt::Display for FormatError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.message)
    }
}

/// Formats one file.
///
/// # Errors
///
/// [`FormatError`] when the file cannot be formatted faithfully.
pub fn format_source(source: &str) -> Result<String, FormatError> {
    let once = print_once(source)?;
    // The idempotence check is part of the contract, so it is enforced here
    // rather than trusted: a printer that oscillates is a printer that will
    // fight every editor save.
    let twice = print_once(&once)?;
    if twice != once {
        return Err(FormatError::new(
            Failure::NotIdempotent,
            "formatting the formatted file changes it again",
        ));
    }
    // A file written with CRLF stays CRLF: rewriting every line ending would
    // bury the formatting change in a whole-file diff.
    if source.contains("\r\n") {
        return Ok(once.replace('\n', "\r\n"));
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

/// Parses, prints and verifies one pass.
fn print_once(source: &str) -> Result<String, FormatError> {
    let module = parse(source)?;
    let before = comments::scan(source);
    let text = print::Printer::new(before.clone())
        .module(&module)
        .map_err(|message| FormatError::new(Failure::Unprintable, message))?;
    let after_module = parse(&text).map_err(|error| {
        FormatError::new(
            Failure::TreeChanged,
            format!("the printed file does not parse again ({})", error.message),
        )
    })?;
    let original = fingerprint::module(&module);
    let printed = fingerprint::module(&after_module);
    if original != printed {
        return Err(FormatError::new(
            Failure::TreeChanged,
            format!(
                "{}: the printed file means something else ({})",
                blame(&module, &after_module),
                first_difference(&original, &printed)
            ),
        ));
    }
    let after = comments::scan(&text);
    if normalise(&before) != normalise(&after) {
        return Err(FormatError::new(
            Failure::CommentLost,
            "a comment would not survive formatting",
        ));
    }
    Ok(text)
}

/// Parses one file, reporting the first diagnostic with its line.
fn parse(source: &str) -> Result<Module, FormatError> {
    let lexed = lex(0, source).map_err(|diagnostics| {
        FormatError::new(
            Failure::Lex,
            format!("{} (the file was not changed)", describe(source, &diagnostics)),
        )
    })?;
    let (module, diagnostics) = parse_module(0, source, &lexed);
    if diagnostics.has_errors() {
        return Err(FormatError::new(
            Failure::Syntax,
            format!("{} (the file was not changed)", describe(source, &diagnostics)),
        ));
    }
    Ok(module)
}

fn describe(source: &str, diagnostics: &aevia_syntax::Diagnostics) -> String {
    let Some(first) = diagnostics
        .items
        .iter()
        .find(|item| item.severity == aevia_syntax::Severity::Error)
    else {
        return "the file does not parse".to_string();
    };
    format!(
        "line {}: {}: {}",
        line_of(source, first.span.start),
        first.code,
        first.message
    )
}

fn line_of(source: &str, offset: u32) -> usize {
    let offset = usize::try_from(offset).unwrap_or(usize::MAX).min(source.len());
    1 + source[..offset].matches('\n').count()
}

/// The declaration whose printed form differs, for the failure message.
fn blame(original: &Module, printed: &Module) -> String {
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

/// The comment texts, with trailing spaces normalised away per line.
fn normalise(comments: &[comments::Comment]) -> Vec<String> {
    comments
        .iter()
        .map(|comment| {
            comment
                .text
                .lines()
                .map(str::trim_end)
                .collect::<Vec<_>>()
                .join("\n")
        })
        .collect()
}

/// The first place two fingerprints differ, as a short excerpt.
fn first_difference(left: &str, right: &str) -> String {
    let left: Vec<char> = left.chars().collect();
    let right: Vec<char> = right.chars().collect();
    let at = left
        .iter()
        .zip(right.iter())
        .position(|(a, b)| a != b)
        .unwrap_or_else(|| left.len().min(right.len()));
    let from = at.saturating_sub(20);
    let left: String = left[from..].iter().take(60).collect();
    let right: String = right[from..].iter().take(60).collect();
    format!("`{left}` became `{right}`")
}

/// A diagnostic for a caller that wants to print one.
#[must_use]
pub fn diagnostic(error: &FormatError) -> Diagnostic {
    Diagnostic::error("E0202", error.message.clone(), aevia_syntax::Span::default())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn formatting_normalises_spacing_without_changing_the_program() {
        let source = "let  x=1+2*3\nfn f(a:Int):Int=a  +1\n";
        let formatted = format_source(source).expect("formats");
        assert_eq!(formatted, "let x = 1 + 2 * 3\n\nfn f(a: Int): Int = a + 1\n");
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
        assert!(
            error.message.contains("class Pretty"),
            "{}",
            error.message
        );
        assert!(
            matches!(
                error.failure,
                Failure::Unprintable | Failure::TreeChanged
            ),
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
