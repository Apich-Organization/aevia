//! Diagnostics: codes, labels, notes and the renderer of appendix B.
//!
//! Every error the compiler emits carries a stable code (`E0401`), a primary
//! span, and — where the spec requires it — the expected/actual pair plus one
//! actionable suggestion. The renderer is deliberately simple: it prints the
//! offending source excerpts with bounded caret underlines, including related
//! declarations in other files.

use crate::span::SourceMap;
use crate::span::Span;
use std::fmt::Write as _;
use unicode_segmentation::UnicodeSegmentation;
use unicode_width::UnicodeWidthStr;

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
        let shared_line = (!self.labels.is_empty())
            .then(|| inline_line(sources, self.span))
            .flatten();
        let (shared, separate): (Vec<_>, Vec<_>) = self.labels.iter().partition(|label| {
            shared_line.is_some_and(|line| {
                label.span.file == self.span.file
                    && sources.file(label.span.file).is_some_and(|file| {
                        let (_, _, first, last) = span_range(file, label.span);
                        first == line && last == line
                    })
            })
        });
        let width = render_span(
            &mut out,
            sources,
            self.span,
            (&self.label, &shared),
            style,
            paint,
            "-->",
        );
        for label in separate {
            render_span(
                &mut out,
                sources,
                label.span,
                (&label.message, &[]),
                style,
                "1;36",
                ":::",
            );
        }

        for note in &self.notes {
            let _ = writeln!(out, "{:width$} = {note}", "", width = width);
        }
        for help in &self.helps {
            let _ = writeln!(out, "help: {help}");
        }
        out
    }
}

// Keep excerpts small even when a recovery span covers an entire pasted
// program or a generated source line contains thousands of characters.
const EXCERPT_WIDTH: usize = 120;
const TAB_WIDTH: usize = 4;
const MAX_GRAPHEME_CHARS: usize = 16;

fn render_span(
    out: &mut String,
    sources: &SourceMap,
    span: Span,
    annotation: (&str, &[&Label]),
    style: Style,
    paint: &str,
    arrow: &str,
) -> usize {
    let (label, shared_labels) = annotation;
    let _ = writeln!(out, "  {arrow} {}", sources.location(span));
    let Some(file) = sources.file(span.file) else {
        if !label.is_empty() {
            let _ = writeln!(out, "  = {label}");
        }
        return 1;
    };
    let (start, end, first_line, last_line) = span_range(file, span);
    let width = last_line.to_string().len();
    let _ = writeln!(out, "{:width$} |", "");
    let lines: Vec<usize> = if last_line.saturating_sub(first_line) <= 2 {
        (first_line..=last_line).collect()
    } else {
        vec![first_line, first_line + 1, last_line]
    };
    let mut previous = first_line;
    for (index, line) in lines.into_iter().enumerate() {
        if line > previous + 1 {
            let _ = writeln!(out, "{:width$} | ...", "");
        }
        let text = file.line_text(line);
        let line_start = file.line_start(line);
        let local_start = start.saturating_sub(line_start).min(text.len());
        let local_end = end.saturating_sub(line_start).min(text.len());
        let view = line_view(text, local_start, local_end, end == start);
        let _ = writeln!(out, "{line:>width$} | {}", view.text);
        let message = if index == 0 { label } else { "" };
        render_caret(out, &view, message, width, style, paint);
        if index == 0 {
            for label in shared_labels {
                let (start, end, _, _) = span_range(file, label.span);
                let view = line_view(
                    text,
                    start.saturating_sub(line_start).min(text.len()),
                    end.saturating_sub(line_start).min(text.len()),
                    start == end,
                );
                render_caret(out, &view, &label.message, width, style, "1;36");
            }
        }
        previous = line;
    }
    let _ = writeln!(out, "{:width$} |", "");
    width
}

fn render_caret(
    out: &mut String,
    view: &LineView,
    label: &str,
    width: usize,
    style: Style,
    paint: &str,
) {
    let carets = style.paint(&"^".repeat(view.length), paint);
    let message = if label.is_empty() {
        String::new()
    } else {
        format!(" {label}")
    };
    let _ = writeln!(
        out,
        "{:width$} | {}{carets}{message}",
        "",
        " ".repeat(view.column)
    );
}

fn span_range(
    file: &crate::span::SourceFile,
    span: Span,
) -> (usize, usize, usize, usize) {
    let start = file.floor_boundary(span.start as usize);
    let end = if span.end > span.start {
        file.ceil_boundary((span.end as usize).max(start))
    } else {
        start
    };
    let first_line = file.line_col(start as u32).0;
    // Spans are half-open: a next-line start does not cover that next line.
    let last_line = if end > start {
        file.line_col(end.saturating_sub(1) as u32).0
    } else {
        first_line
    };
    (start, end, first_line, last_line)
}

/// Nearby labels can share a full source line. Cropped windows stay separate
/// so a marker can never point to text absent from the displayed excerpt.
fn inline_line(
    sources: &SourceMap,
    span: Span,
) -> Option<usize> {
    let file = sources.file(span.file)?;
    let (start, end, first, last) = span_range(file, span);
    if first != last {
        return None;
    }
    let text = file.line_text(first);
    let offset = file.line_start(first);
    let view = line_view(
        text,
        start.saturating_sub(offset).min(text.len()),
        end.saturating_sub(offset).min(text.len()),
        start == end,
    );
    (!view.cropped).then_some(first)
}

struct LineView {
    text: String,
    column: usize,
    length: usize,
    cropped: bool,
}

/// Translate source byte ranges into display cells before cropping. A span
/// inside a combining/emoji cluster highlights the whole visible cluster.
fn line_view(
    text: &str,
    start: usize,
    end: usize,
    point: bool,
) -> LineView {
    let mut column = 0usize;
    let mut marker_start = None;
    let mut marker_end = 0usize;
    for (offset, grapheme) in text.grapheme_indices(true) {
        let width = grapheme_width(grapheme, column);
        let limit = offset + grapheme.len();
        let covered = if point {
            offset <= start && start < limit
        } else {
            start < limit && offset < end
        };
        if covered {
            marker_start.get_or_insert(column);
            marker_end = if point {
                column + 1
            } else {
                column + width
            };
        }
        column += width;
    }
    let marker_start = marker_start.unwrap_or(column);
    let marker_end = marker_end.max(marker_start + 1);
    let desired_start = if column > EXCERPT_WIDTH {
        marker_start.saturating_sub(EXCERPT_WIDTH / 3)
    } else {
        0
    };
    let mut full_column = 0usize;
    let mut visible_start = 0usize;
    let mut visible_end = 0usize;
    let mut rendered = String::new();
    let mut leading = false;
    let mut trailing = false;
    for (_, grapheme) in text.grapheme_indices(true) {
        let width = grapheme_width(grapheme, full_column);
        if full_column < desired_start {
            full_column += width;
            continue;
        }
        if rendered.is_empty() {
            visible_start = full_column;
            leading = visible_start > 0;
            if leading {
                rendered.push_str("...");
            }
        }
        // Reserve room for a suffix even if this particular glyph turns out
        // to be the last one. Short lines retain their original full text.
        let prefix = usize::from(leading) * 3;
        let budget = if column > EXCERPT_WIDTH {
            EXCERPT_WIDTH - 3
        } else {
            EXCERPT_WIDTH
        };
        if full_column - visible_start + width + prefix > budget {
            trailing = true;
            break;
        }
        append_grapheme(&mut rendered, grapheme, width);
        full_column += width;
        visible_end = full_column;
    }
    if trailing {
        rendered.push_str("...");
    }
    let prefix = usize::from(leading) * 3;
    let caret_column = marker_start.saturating_sub(visible_start) + prefix;
    let available = visible_end.saturating_sub(marker_start).max(1);
    LineView {
        text: rendered,
        column: caret_column.min(EXCERPT_WIDTH),
        length: (marker_end - marker_start).min(available).max(1),
        cropped: leading || trailing,
    }
}

fn escaped_grapheme(grapheme: &str) -> bool {
    grapheme
        .chars()
        .any(|character| character.is_control() || matches!(character, '\u{2028}' | '\u{2029}'))
        || UnicodeWidthStr::width(grapheme) == 0
}

fn grapheme_width(
    grapheme: &str,
    column: usize,
) -> usize {
    if grapheme == "\t" {
        return TAB_WIDTH - column % TAB_WIDTH;
    }
    if grapheme.chars().take(MAX_GRAPHEME_CHARS + 1).count() > MAX_GRAPHEME_CHARS {
        return 3;
    }
    if escaped_grapheme(grapheme) {
        return grapheme
            .chars()
            .map(|character| character.escape_default().count())
            .sum();
    }
    UnicodeWidthStr::width(grapheme).max(1)
}

fn append_grapheme(
    out: &mut String,
    grapheme: &str,
    width: usize,
) {
    if grapheme == "\t" {
        out.push_str(&" ".repeat(width));
    } else if grapheme.chars().take(MAX_GRAPHEME_CHARS + 1).count() > MAX_GRAPHEME_CHARS {
        out.push_str("...");
    } else if escaped_grapheme(grapheme) {
        for character in grapheme.chars() {
            out.extend(character.escape_default());
        }
    } else {
        out.push_str(grapheme);
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

    fn pointer(rendered: &str) -> &str {
        rendered
            .lines()
            .find_map(|line| line.strip_prefix("  | ").filter(|line| line.contains('^')))
            .unwrap()
    }

    #[test]
    fn same_line_labels_share_one_excerpt_with_independent_caret_rows() {
        let mut sources = SourceMap::new();
        let code = "let answer = left + right";
        let file = sources.add("one.aev", code);
        let left = code.find("left").unwrap();
        let right = code.find("right").unwrap();
        let error = Diagnostic::error(
            "E0401",
            "incompatible dimensions",
            Span::new(file, left, code.len()),
        )
        .with_primary_label("cannot add these")
        .with_label(Span::new(file, left, left + 4), "length operand")
        .with_label(Span::new(file, right, right + 5), "time operand");
        let rendered = error.render(&sources);
        assert_eq!(rendered.matches(code).count(), 1, "{rendered}");
        assert_eq!(
            rendered.lines().filter(|line| line.contains('^')).count(),
            3,
            "{rendered}"
        );
        assert!(!rendered.contains(":::"), "{rendered}");
        assert!(rendered.contains("^^^^ length operand"), "{rendered}");
        assert!(rendered.contains("^^^^^ time operand"), "{rendered}");
        assert_eq!(rendered.matches("--> one.aev:").count(), 1);
    }

    #[test]
    fn labels_on_cropped_windows_keep_their_own_source_location_and_excerpt() {
        let mut sources = SourceMap::new();
        let code = format!("left {} right", "x".repeat(1000));
        let file = sources.add("long.aev", &code);
        let right = code.find("right").unwrap();
        let rendered = Diagnostic::error(
            "E0401",
            "incompatible dimensions",
            Span::new(file, right, right + 5),
        )
        .with_label(Span::new(file, 0, 4), "other operand")
        .render(&sources);
        assert_eq!(
            rendered
                .lines()
                .filter(|line| line.starts_with("1 | "))
                .count(),
            2,
            "{rendered}"
        );
        assert!(rendered.contains("::: long.aev:1:1"), "{rendered}");
        assert!(rendered.contains("1 | left"), "{rendered}");
    }

    #[test]
    fn secondary_labels_use_their_own_files_and_source_excerpts() {
        let mut sources = SourceMap::new();
        let left = sources.add("distance.aev", "let distance = 3 m\n");
        let right = sources.add("duration.aev", "let duration = 2 s\n");
        let error = Diagnostic::error("E0401", "incompatible dimensions", Span::new(left, 15, 18))
            .with_label(Span::new(right, 15, 18), "time was declared here");
        let rendered = error.render(&sources);
        assert!(rendered.contains("--> distance.aev:1:16"), "{rendered}");
        assert!(rendered.contains("::: duration.aev:1:16"), "{rendered}");
        assert!(rendered.contains("let duration = 2 s"), "{rendered}");
        assert!(rendered.contains("time was declared here"), "{rendered}");
    }

    #[test]
    fn utf8_secondary_widths_are_display_cells_not_bytes() {
        let mut sources = SourceMap::new();
        let file = sources.add("unicode.aev", "let 变量 = 2\n");
        let rendered = Diagnostic::error("E0001", "example", Span::new(file, 13, 14))
            .with_label(Span::new(file, 4, 10), "the variable")
            .render(&sources);
        assert!(rendered.contains("  |     ^^^^ the variable"), "{rendered}");
        assert!(!rendered.contains("^^^^^^ the variable"), "{rendered}");
    }

    #[test]
    fn tabs_cjk_and_combining_marks_align_with_the_displayed_source() {
        let mut sources = SourceMap::new();
        let text = "\t界e\u{301} = x\r\n";
        let file = sources.add("aligned.aev", text);
        let start = text.find('x').unwrap();
        let rendered = Diagnostic::error("E0001", "example", Span::new(file, start, start + 1))
            .render(&sources);
        let source = rendered
            .lines()
            .find_map(|line| line.strip_prefix("1 | "))
            .unwrap();
        assert_eq!(source, "    界e\u{301} = x");
        assert_eq!(pointer(&rendered).find('^'), Some(10), "{rendered}");
        assert!(!rendered.contains('\r'));
        let mark = text.find('e').unwrap() + 1;
        let combined =
            Diagnostic::error("E0001", "example", Span::new(file, mark, mark + 2)).render(&sources);
        assert_eq!(pointer(&combined), "      ^", "{combined}");
    }

    #[test]
    fn joined_emoji_are_one_display_cluster_for_both_location_and_underline() {
        let mut sources = SourceMap::new();
        let text = "let s = \"👩‍🔬\"; x";
        let file = sources.add("emoji.aev", text);
        let start = text.find('x').unwrap();
        let rendered = Diagnostic::error("E0001", "example", Span::new(file, start, start + 1))
            .render(&sources);
        assert_eq!(pointer(&rendered).find('^'), Some(14), "{rendered}");
        let inside = text.find('🔬').unwrap();
        let rendered = Diagnostic::error(
            "E0001",
            "example",
            Span::new(file, inside, inside + '🔬'.len_utf8()),
        )
        .render(&sources);
        assert_eq!(pointer(&rendered), "         ^^", "{rendered}");
    }

    #[test]
    fn multiline_spans_show_bounded_context_and_respect_exclusive_endpoints() {
        let mut sources = SourceMap::new();
        let text = (1..=12)
            .map(|line| format!("row{line} content\n"))
            .collect::<String>();
        let file = sources.add("many.aev", &text);
        let end = text.find("row10").unwrap();
        let rendered = Diagnostic::error("E0001", "large construct", Span::new(file, 2, end))
            .with_primary_label("starts here")
            .render(&sources);
        assert!(rendered.contains("row1 content"), "{rendered}");
        assert!(rendered.contains("row2 content"), "{rendered}");
        assert!(rendered.contains("row9 content"), "{rendered}");
        assert!(rendered.contains("| ..."), "{rendered}");
        assert!(!rendered.contains("row5 content"), "{rendered}");
        assert!(!rendered.contains("row10 content"), "{rendered}");
        assert_eq!(rendered.matches("starts here").count(), 1);
        for line in rendered.lines().filter(|line| line.contains('^')) {
            assert!(line.len() < 40, "{rendered}");
        }
    }

    #[test]
    fn long_lines_and_highlights_are_cropped_around_the_actual_location() {
        let mut sources = SourceMap::new();
        let text = "a".repeat(4000);
        let file = sources.add("long.aev", &text);
        let rendered =
            Diagnostic::error("E0001", "example", Span::new(file, 1500, 3500)).render(&sources);
        let source = rendered
            .lines()
            .find_map(|line| line.strip_prefix("1 | "))
            .unwrap();
        assert!(
            source.starts_with("...") && source.ends_with("..."),
            "{rendered}"
        );
        assert!(UnicodeWidthStr::width(source) <= EXCERPT_WIDTH);
        assert!(
            pointer(&rendered).find('^').unwrap() <= EXCERPT_WIDTH / 2,
            "{rendered}"
        );
        assert!(
            pointer(&rendered).len() <= UnicodeWidthStr::width(source),
            "{rendered}"
        );
        let end = Diagnostic::error("E0001", "example", Span::new(file, text.len(), text.len()))
            .render(&sources);
        assert!(pointer(&end).len() <= EXCERPT_WIDTH + 1, "{end}");
    }

    #[test]
    fn zero_width_eof_empty_sources_and_invalid_offsets_are_safe() {
        let mut sources = SourceMap::new();
        let empty = sources.add("empty.aev", "");
        let empty_error =
            Diagnostic::error("E0001", "expected a token", Span::new(empty, 0, 0)).render(&sources);
        assert!(empty_error.contains("1 | \n  | ^"), "{empty_error}");
        let file = sources.add("eof.aev", "界\r\n");
        let eof =
            Diagnostic::error("E0001", "expected a token", Span::new(file, 5, 5)).render(&sources);
        assert!(eof.contains("--> eof.aev:2:1"), "{eof}");
        assert!(eof.contains("2 | \n  | ^"), "{eof}");
        let invalid = Diagnostic::error(
            "E0001",
            "external byte span",
            Span {
                file,
                start: 1,
                end: 2,
            },
        )
        .render(&sources);
        assert!(invalid.contains("  | ^^"), "{invalid}");
        let beyond = Diagnostic::error(
            "E0001",
            "external byte span",
            Span {
                file,
                start: u32::MAX,
                end: u32::MAX,
            },
        )
        .render(&sources);
        assert!(beyond.contains("eof.aev:2:1"), "{beyond}");
    }

    #[test]
    fn missing_secondary_files_still_preserve_the_label_message() {
        let mut sources = SourceMap::new();
        let file = sources.add("main.aev", "x");
        let rendered = Diagnostic::error("E0001", "example", Span::new(file, 0, 1))
            .with_label(Span::new(99, 0, 1), "source unavailable")
            .render(&sources);
        assert!(rendered.contains("::: <unknown>"), "{rendered}");
        assert!(rendered.contains("= source unavailable"), "{rendered}");
    }

    #[test]
    fn oversized_clusters_and_source_controls_cannot_flood_or_restyle_the_terminal() {
        let mut sources = SourceMap::new();
        let cluster = format!("a{}", "\u{301}".repeat(4000));
        let file = sources.add("cluster.aev", &cluster);
        let rendered = Diagnostic::error("E0001", "example", Span::new(file, 0, cluster.len()))
            .render(&sources);
        assert!(rendered.len() < 200, "{} bytes", rendered.len());
        let controls = sources.add("controls.aev", "\u{1b}[31mX");
        let plain =
            Diagnostic::error("E0001", "example", Span::new(controls, 0, 1)).render(&sources);
        assert!(plain.contains("\\u{1b}[31mX"), "{plain}");
        assert!(!plain.contains('\u{1b}'));
    }

    #[test]
    fn ansi_and_plain_render_the_same_positions_and_text() {
        let mut sources = SourceMap::new();
        let file = sources.add("unicode.aev", "\t界e\u{301}\n");
        let diagnostic = Diagnostic::warning("W0001", "example", Span::new(file, 1, 4))
            .with_label(Span::new(file, 4, 7), "another part");
        let plain = diagnostic.render(&sources);
        let ansi = diagnostic.render_with(&sources, Style::Ansi);
        let mut stripped = String::new();
        let mut escape = false;
        for character in ansi.chars() {
            if character == '\u{1b}' {
                escape = true;
            } else if escape {
                if character == 'm' {
                    escape = false;
                }
            } else {
                stripped.push(character);
            }
        }
        assert_eq!(plain, stripped);
        assert!(ansi.contains("\u{1b}[1;33m"));
        assert!(ansi.contains("\u{1b}[1;36m"));
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
