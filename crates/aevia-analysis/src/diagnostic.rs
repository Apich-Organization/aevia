//! Compiler diagnostics, in a form a client can place in a document.
//!
//! The compiler's [`aevia_syntax::Diagnostic`] is already almost this type: a
//! span, a severity, a code, a headline, one primary label, secondary labels,
//! notes and helps. The conversion is therefore mechanical, and the only
//! decisions are the two that matter:
//!
//! * **spans are byte ranges, ranges are positions.** [`crate::LineIndex`] does
//!   the conversion, once, for every diagnostic;
//! * **a span in another file is not this document's.** A diagnostic whose
//!   primary span belongs to a different file is dropped rather than placed at
//!   the nearest offset of the open document, which would underline innocent
//!   code.

use crate::document::Document;
use crate::position::Range;
use aevia_syntax::{FileId, Severity as SyntaxSeverity};

/// How serious a diagnostic is.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Severity {
    /// The program cannot be compiled.
    Error,
    /// It compiles but is suspicious.
    Warning,
    /// Explanatory context.
    Info,
    /// A suggestion the editor may render faintly.
    Hint,
}

/// A secondary location with its own message.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Related {
    /// Where it points.
    pub range: Range,
    /// What it says.
    pub message: String,
}

/// One diagnostic, placed in one document.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Diagnostic {
    /// Where to underline.
    pub range: Range,
    /// How serious it is.
    pub severity: Severity,
    /// The stable code, when there is one.
    pub code: Option<String>,
    /// The whole text: headline, primary label, notes and helps, one per line.
    pub message: String,
    /// Secondary locations in the same document.
    pub related: Vec<Related>,
}

/// Converts one compiler diagnostic, or drops it.
///
/// Returns `None` when the diagnostic's primary span belongs to a file other
/// than `file`, or when its secondary labels do — a label in another file
/// cannot be placed in this document either.
#[must_use]
pub fn convert(diagnostic: &aevia_syntax::Diagnostic, file: FileId, doc: &Document) -> Option<Diagnostic> {
    if diagnostic.span.file != file {
        return None;
    }
    let mut message = diagnostic.message.clone();
    if !diagnostic.label.is_empty() {
        message.push('\n');
        message.push_str(&diagnostic.label);
    }
    for note in &diagnostic.notes {
        message.push_str("\n= ");
        message.push_str(note);
    }
    for help in &diagnostic.helps {
        message.push_str("\nhelp: ");
        message.push_str(help);
    }
    let related = diagnostic
        .labels
        .iter()
        .filter(|label| label.span.file == file)
        .map(|label| Related {
            range: doc.range_of(label.span.start, label.span.end),
            message: label.message.clone(),
        })
        .collect();
    Some(Diagnostic {
        range: doc.range_of(diagnostic.span.start, diagnostic.span.end),
        severity: severity(diagnostic.severity),
        code: (!diagnostic.code.is_empty()).then(|| diagnostic.code.clone()),
        message,
        related,
    })
}

fn severity(severity: SyntaxSeverity) -> Severity {
    match severity {
        SyntaxSeverity::Error => Severity::Error,
        SyntaxSeverity::Warning => Severity::Warning,
        SyntaxSeverity::Note => Severity::Info,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use aevia_syntax::{Diagnostic as CompilerDiagnostic, Span};

    fn document() -> Document {
        Document::new("a.aev", 1, "let x = 1 m\nlet y = x + 1\n")
    }

    #[test]
    fn a_span_becomes_a_range() {
        let doc = document();
        let compiler = CompilerDiagnostic::error("E0401", "dimension mismatch", Span::new(0, 16, 21))
            .with_primary_label("expected Real[L], found Int")
            .with_note("left:  L")
            .with_help("divide by a unit");
        let converted = convert(&compiler, 0, &doc).expect("same file");
        assert_eq!(converted.range, doc.range_of(16, 21));
        assert_eq!(converted.range.start, crate::Position::new(1, 4));
        assert_eq!(converted.severity, Severity::Error);
        assert_eq!(converted.code.as_deref(), Some("E0401"));
        assert_eq!(
            converted.message,
            "dimension mismatch\nexpected Real[L], found Int\n= left:  L\nhelp: divide by a unit"
        );
    }

    #[test]
    fn a_span_in_another_file_is_dropped() {
        let doc = document();
        let compiler =
            CompilerDiagnostic::error("E0401", "from elsewhere", Span::new(7, 0, 3));
        assert!(convert(&compiler, 0, &doc).is_none());
    }

    #[test]
    fn a_label_in_another_file_is_dropped_but_the_diagnostic_survives() {
        let doc = document();
        let compiler = CompilerDiagnostic::error("E0401", "mismatch", Span::new(0, 0, 3))
            .with_label(Span::new(0, 4, 5), "here")
            .with_label(Span::new(3, 4, 5), "elsewhere");
        let converted = convert(&compiler, 0, &doc).expect("same file");
        assert_eq!(converted.related.len(), 1);
        assert_eq!(converted.related[0].message, "here");
    }

    #[test]
    fn warnings_and_notes_keep_their_rank() {
        let doc = document();
        let warning = CompilerDiagnostic::warning("W0301", "unresolved", Span::new(0, 0, 3));
        let note = CompilerDiagnostic {
            severity: SyntaxSeverity::Note,
            code: String::new(),
            message: "for context".to_string(),
            span: Span::new(0, 0, 3),
            label: String::new(),
            labels: Vec::new(),
            notes: Vec::new(),
            helps: Vec::new(),
        };
        assert_eq!(convert(&warning, 0, &doc).expect("w").severity, Severity::Warning);
        let converted = convert(&note, 0, &doc).expect("n");
        assert_eq!(converted.severity, Severity::Info);
        assert_eq!(converted.code, None);
    }
}
