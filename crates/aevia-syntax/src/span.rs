//! Source files and byte spans.
//!
//! Every token, expression and diagnostic carries a [`Span`]. Spans are byte
//! ranges into a file registered in a [`SourceMap`], which is what turns them
//! into `line:column` for diagnostics and the LSP.

use std::fmt;

/// Identifier of a file inside a [`SourceMap`].
pub type FileId = u32;

/// A byte range in one source file.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct Span {
    /// The file the range belongs to.
    pub file: FileId,
    /// Inclusive start byte offset.
    pub start: u32,
    /// Exclusive end byte offset.
    pub end: u32,
}

impl Span {
    /// Creates a span.
    #[must_use]
    pub fn new(file: FileId, start: usize, end: usize) -> Self {
        Self {
            file,
            start: u32::try_from(start).unwrap_or(u32::MAX),
            end: u32::try_from(end).unwrap_or(u32::MAX),
        }
    }

    /// A span that covers both inputs.
    #[must_use]
    pub fn join(self, other: Self) -> Self {
        if self.file != other.file {
            return self;
        }
        Self {
            file: self.file,
            start: self.start.min(other.start),
            end: self.end.max(other.end),
        }
    }

    /// The zero-width span at the start of this one.
    #[must_use]
    pub fn start_span(self) -> Self {
        Self {
            file: self.file,
            start: self.start,
            end: self.start,
        }
    }

    /// The length in bytes.
    #[must_use]
    pub fn len(self) -> usize {
        usize::try_from(self.end.saturating_sub(self.start)).unwrap_or(0)
    }

    /// True when the span covers no bytes.
    #[must_use]
    pub fn is_empty(self) -> bool {
        self.end <= self.start
    }
}

impl fmt::Debug for Span {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}..{}@{}", self.start, self.end, self.file)
    }
}

/// One source file: its display name and its text.
#[derive(Clone, Debug)]
pub struct SourceFile {
    /// Path or synthetic name shown in diagnostics.
    pub name: String,
    /// The full UTF-8 source text.
    pub text: String,
    /// Byte offsets at which each line starts.
    line_starts: Vec<usize>,
}

impl SourceFile {
    /// Registers a file and precomputes line starts.
    #[must_use]
    pub fn new(name: impl Into<String>, text: impl Into<String>) -> Self {
        let text = text.into();
        let mut line_starts = vec![0];
        for (index, byte) in text.bytes().enumerate() {
            if byte == b'\n' {
                line_starts.push(index + 1);
            }
        }
        Self {
            name: name.into(),
            text,
            line_starts,
        }
    }

    /// The 1-based line and column (in characters) of a byte offset.
    #[must_use]
    pub fn line_col(&self, offset: u32) -> (usize, usize) {
        let offset = usize::try_from(offset).unwrap_or(0).min(self.text.len());
        let line = match self.line_starts.binary_search(&offset) {
            Ok(index) => index,
            Err(index) => index.saturating_sub(1),
        };
        let start = self.line_starts.get(line).copied().unwrap_or(0);
        let column = self.text[start..offset].chars().count() + 1;
        (line + 1, column)
    }

    /// The text of a 1-based line, without its terminator.
    #[must_use]
    pub fn line_text(&self, line: usize) -> &str {
        let Some(&start) = self.line_starts.get(line.saturating_sub(1)) else {
            return "";
        };
        let end = self
            .line_starts
            .get(line)
            .copied()
            .unwrap_or(self.text.len());
        self.text[start..end].trim_end_matches(['\n', '\r'])
    }
}

/// All files of a compilation, indexed by [`FileId`].
#[derive(Clone, Debug, Default)]
pub struct SourceMap {
    files: Vec<SourceFile>,
}

impl SourceMap {
    /// An empty map.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Adds a file and returns its id.
    pub fn add(&mut self, name: impl Into<String>, text: impl Into<String>) -> FileId {
        let id = u32::try_from(self.files.len()).unwrap_or(u32::MAX);
        self.files.push(SourceFile::new(name, text));
        id
    }

    /// The file with the given id.
    #[must_use]
    pub fn file(&self, id: FileId) -> Option<&SourceFile> {
        self.files.get(usize::try_from(id).ok()?)
    }

    /// The text of a span, when it is in range.
    #[must_use]
    pub fn text(&self, span: Span) -> &str {
        let Some(file) = self.file(span.file) else {
            return "";
        };
        let start = usize::try_from(span.start).unwrap_or(0);
        let end = usize::try_from(span.end).unwrap_or(0);
        file.text.get(start..end).unwrap_or("")
    }

    /// The `file:line:column` prefix for a span.
    #[must_use]
    pub fn location(&self, span: Span) -> String {
        match self.file(span.file) {
            Some(file) => {
                let (line, column) = file.line_col(span.start);
                format!("{}:{line}:{column}", file.name)
            }
            None => "<unknown>".to_string(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn line_and_column_are_one_based() {
        let mut map = SourceMap::new();
        let id = map.add("test.aev", "let x = 1\nlet y = 2\n");
        let file = map.file(id).unwrap();
        assert_eq!(file.line_col(0), (1, 1));
        assert_eq!(file.line_col(4), (1, 5));
        assert_eq!(file.line_col(10), (2, 1));
        assert_eq!(file.line_text(1), "let x = 1");
    }

    #[test]
    fn spans_join_across_children() {
        let a = Span::new(0, 4, 5);
        let b = Span::new(0, 6, 9);
        assert_eq!(a.join(b), Span::new(0, 4, 9));
        assert_eq!(a.len(), 1);
    }
}
