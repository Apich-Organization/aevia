//! Diagnostics: codes, labels, notes and the renderer of appendix B.
//!
//! Every error the compiler emits carries a stable code (`E0401`), a primary
//! span, and — where the spec requires it — the expected/actual pair plus one
//! actionable suggestion. The renderer is deliberately simple: it prints the
//! offending source line with a caret underline, the way the appendix shows.

use crate::span::SourceMap;
use crate::span::Span;
use std::fmt::Write as _;

/// How serious a diagnostic is.
#[derive(Clone, Copy, PartialEq, Eq, Debug, serde::Serialize, serde::Deserialize)]
pub enum Severity {
    /// The program cannot be compiled or run.
    Error,
    /// The program runs but is suspicious.
    Warning,
    /// Explanatory context, never printed on its own.
    Note,
}

impl Severity {
    fn as_str(self) -> &'static str {
        match self {
            | Self::Error => "error",
            | Self::Warning => "warning",
            | Self::Note => "note",
        }
    }
}

/// A secondary span with its own message.
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct Label {
    /// Where the label points.
    pub span: Span,
    /// What to say about it.
    pub message: String,
}

/// A compiler diagnostic.
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct Diagnostic {
    /// Error or warning.
    pub severity: Severity,
    /// Stable code such as `E0401`; empty for notes.
    pub code: String,
    /// One-line summary.
    pub message: String,
    /// The primary location.
    pub span: Span,
    /// The message printed next to the primary caret.
    pub label: String,
    /// Additional locations.
    pub labels: Vec<Label>,
    /// `= ...` lines: canonical forms, differences, related declarations.
    pub notes: Vec<String>,
    /// `help: ...` lines, most confident first.
    pub helps: Vec<String>,
}

impl Diagnostic {
    /// Creates an error.
    #[must_use]
    pub fn error(
        code: &str,
        message: impl Into<String>,
        span: Span,
    ) -> Self {
        Self {
            severity: Severity::Error,
            code: code.to_string(),
            message: message.into(),
            span,
            label: String::new(),
            labels: Vec::new(),
            notes: Vec::new(),
            helps: Vec::new(),
        }
    }

    /// Creates a warning.
    #[must_use]
    pub fn warning(
        code: &str,
        message: impl Into<String>,
        span: Span,
    ) -> Self {
        Self {
            severity: Severity::Warning,
            code: code.to_string(),
            message: message.into(),
            span,
            label: String::new(),
            labels: Vec::new(),
            notes: Vec::new(),
            helps: Vec::new(),
        }
    }

    /// Sets the text printed beside the primary caret.
    #[must_use]
    pub fn with_primary_label(
        mut self,
        label: impl Into<String>,
    ) -> Self {
        self.label = label.into();
        self
    }

    /// Attaches a label to a secondary span.
    #[must_use]
    pub fn with_label(
        mut self,
        span: Span,
        message: impl Into<String>,
    ) -> Self {
        self.labels.push(Label {
            span,
            message: message.into(),
        });
        self
    }

    /// Adds a `= ...` note.
    #[must_use]
    pub fn with_note(
        mut self,
        note: impl Into<String>,
    ) -> Self {
        self.notes.push(note.into());
        self
    }

    /// Adds a `help:` line.
    #[must_use]
    pub fn with_help(
        mut self,
        help: impl Into<String>,
    ) -> Self {
        self.helps.push(help.into());
        self
    }

    /// The stable code, or `""` when there is none.
    #[must_use]
    pub fn code(&self) -> &str {
        &self.code
    }

    /// Renders the diagnostic against a source map.
    #[must_use]
    pub fn render(
        &self,
        sources: &SourceMap,
    ) -> String {
        self.render_with(sources, Style::Plain)
    }

    /// Renders the diagnostic, optionally colouring the parts a terminal shows
    /// in colour: the severity header and the carets under the span.
    ///
    /// Colour is a rendering concern, so it is a parameter rather than a
    /// global: a test that compares a rendering must be able to ask for the
    /// plain one.
    #[must_use]
    pub fn render_with(
        &self,
        sources: &SourceMap,
        style: Style,
    ) -> String {
        let mut out = String::new();
        let code = if self.code.is_empty() {
            String::new()
        } else {
            format!("[{}]", self.code)
        };
        let paint = match self.severity {
            | Severity::Error => "1;31",
            | Severity::Warning => "1;33",
            | Severity::Note => "1;36",
        };
        let header = format!("{}{}: {}", self.severity.as_str(), code, self.message);
        let _ = writeln!(out, "{}", style.paint(&header, paint));
        let location = sources.location(self.span);
        let _ = writeln!(out, "  --> {location}");

        let file = sources.file(self.span.file);
        let (line, column) = file.map_or((1, 1), |file| file.line_col(self.span.start));
        let width = line.to_string().len();
        let _ = writeln!(out, "{} |", " ".repeat(width + 1));

        if let Some(file) = file {
            let text = file.line_text(line);
            let _ = writeln!(out, "{line:>width$} | {text}");
            let start_column = column.saturating_sub(1);
            let length = if self.span.end > self.span.start {
                let (_, end_column) = file.line_col(self.span.end);
                if end_column > column {
                    end_column - column
                } else {
                    text.chars().count().saturating_sub(start_column).max(1)
                }
            } else {
                1
            };
            let pad = " ".repeat(width + 1);
            let caret_pad = " ".repeat(start_column);
            let carets = style.paint(&"^".repeat(length.max(1)), paint);
            let label = if self.label.is_empty() {
                String::new()
            } else {
                format!(" {}", self.label)
            };
            let _ = writeln!(out, "{pad} | {caret_pad}{carets}{label}");
            for label in &self.labels {
                let (label_line, label_column) = file.line_col(label.span.start);
                if label_line != line {
                    let _ = writeln!(
                        out,
                        "{pad} = note: {} at {}:{label_line}:{label_column}",
                        label.message,
                        crate::span::display_path(&file.name)
                    );
                    continue;
                }
                let label_pad = " ".repeat(label_column.saturating_sub(1));
                let label_len = usize::try_from(label.span.end.saturating_sub(label.span.start))
                    .unwrap_or(1)
                    .max(1);
                let _ = writeln!(
                    out,
                    "{pad} | {label_pad}{} {}",
                    "^".repeat(label_len),
                    label.message
                );
            }
            let _ = writeln!(out, "{pad} |");
        }

        for note in &self.notes {
            let _ = writeln!(out, "{:width$} = {note}", "", width = width + 1);
        }
        for help in &self.helps {
            let _ = writeln!(out, "help: {help}");
        }
        out
    }
}

/// A list of diagnostics produced by one phase.
#[derive(Clone, Debug, Default, serde::Serialize, serde::Deserialize)]
pub struct Diagnostics {
    /// The diagnostics, in source order.
    pub items: Vec<Diagnostic>,
}

impl Diagnostics {
    /// An empty list.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Appends a diagnostic.
    pub fn push(
        &mut self,
        diagnostic: Diagnostic,
    ) {
        self.items.push(diagnostic);
    }

    /// True when at least one error was recorded.
    #[must_use]
    pub fn has_errors(&self) -> bool {
        self.items
            .iter()
            .any(|item| item.severity == Severity::Error)
    }

    /// Number of errors.
    #[must_use]
    pub fn error_count(&self) -> usize {
        self.items
            .iter()
            .filter(|item| item.severity == Severity::Error)
            .count()
    }

    /// True when nothing was recorded.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    /// Renders every diagnostic, separated by blank lines.
    #[must_use]
    pub fn render(
        &self,
        sources: &SourceMap,
    ) -> String {
        self.render_with(sources, Style::Plain)
    }

    /// Renders every diagnostic with the given style.
    #[must_use]
    pub fn render_with(
        &self,
        sources: &SourceMap,
        style: Style,
    ) -> String {
        self.items
            .iter()
            .map(|item| item.render_with(sources, style))
            .collect::<Vec<_>>()
            .join("\n")
    }

    /// Merges another list into this one.
    pub fn extend(
        &mut self,
        other: Self,
    ) {
        self.items.extend(other.items);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::span::SourceMap;

    #[test]
    fn renders_the_appendix_shape() {
        let mut sources = SourceMap::new();
        let id = sources.add("kinematics.aev", "fn f(h) =\n    sqrt(2 * h * g)\n");
        let start = sources.file(id).unwrap().text.find("2 * h * g").unwrap();
        let span = Span::new(id, start, start + 9);
        let diagnostic = Diagnostic::error("E0401", "dimension mismatch", span)
            .with_note("left:  L^2/T^2")
            .with_note("right: T^2")
            .with_help("did you mean `2 * h / g`?");
        let rendered = diagnostic.render(&sources);
        assert!(rendered.starts_with("error[E0401]: dimension mismatch"));
        assert!(rendered.contains("--> kinematics.aev:2:10"));
        assert!(rendered.contains("^^^^^^^^^"));
        assert!(rendered.contains("= left:  L^2/T^2"));
        assert!(rendered.contains("help: did you mean"));
    }
}

/// How a diagnostic is rendered.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum Style {
    /// No escape sequences: what a file or a test sees.
    #[default]
    Plain,
    /// ANSI colour for a terminal.
    Ansi,
}

impl Style {
    /// Wraps `text` in the ANSI sequence `code`, or returns it unchanged.
    fn paint(
        self,
        text: &str,
        code: &str,
    ) -> String {
        match self {
            | Self::Plain => text.to_string(),
            | Self::Ansi => format!("\u{1b}[{code}m{text}\u{1b}[0m"),
        }
    }
}
