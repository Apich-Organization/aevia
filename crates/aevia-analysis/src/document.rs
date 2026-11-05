//! Documents, their revisions, and the store a client keeps them in.
//!
//! A [`Document`] is a name (the client's URI, used only as an opaque key), a
//! revision, the text, and a [`LineIndex`] kept in step with that text. Every
//! edit goes through [`Document::apply`], which is the only way the index can
//! fall out of date.

use crate::position::LineIndex;
use crate::position::Position;
use crate::position::Range;

/// One edit to a document.
///
/// `range` is `None` for a whole-document replacement, which is what a client
/// that does not support incremental sync sends. The range is expressed in
/// positions rather than byte offsets because that is what the edit describes:
/// the bytes it means depend on the text the edit is applied to, and that text
/// is the document's.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Change {
    /// The text to replace, or `None` to replace the whole document.
    pub range: Option<Range>,
    /// The replacement text.
    pub text: String,
}

impl Change {
    /// A whole-document replacement.
    #[must_use]
    pub fn full(text: impl Into<String>) -> Self {
        Self {
            range: None,
            text: text.into(),
        }
    }

    /// A replacement of one range.
    #[must_use]
    pub fn ranged(
        range: Range,
        text: impl Into<String>,
    ) -> Self {
        Self {
            range: Some(range),
            text: text.into(),
        }
    }
}

/// Why an edit was refused.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum EditError {
    /// The range names a line the document does not have.
    UnknownLine(u32),
    /// The range ends before it starts.
    Reversed,
}

impl std::fmt::Display for EditError {
    fn fmt(
        &self,
        f: &mut std::fmt::Formatter<'_>,
    ) -> std::fmt::Result {
        match self {
            | Self::UnknownLine(line) => {
                write!(f, "the edit names line {}, which does not exist", line + 1)
            },
            | Self::Reversed => f.write_str("the edit ends before it starts"),
        }
    }
}

impl std::error::Error for EditError {}

/// One open document.
#[derive(Clone, Debug)]
pub struct Document {
    name: String,
    version: i64,
    text: String,
    index: LineIndex,
}

impl Document {
    /// Creates a document at revision `version`.
    #[must_use]
    pub fn new(
        name: impl Into<String>,
        version: i64,
        text: impl Into<String>,
    ) -> Self {
        let text = text.into();
        let index = LineIndex::new(&text);
        Self {
            name: name.into(),
            version,
            text,
            index,
        }
    }

    /// The name the client opened it under.
    #[must_use]
    pub fn name(&self) -> &str {
        &self.name
    }

    /// The revision this text belongs to.
    #[must_use]
    pub fn version(&self) -> i64 {
        self.version
    }

    /// The text.
    #[must_use]
    pub fn text(&self) -> &str {
        &self.text
    }

    /// The length in bytes.
    #[must_use]
    pub fn len(&self) -> usize {
        self.text.len()
    }

    /// True when the document is empty.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.text.is_empty()
    }

    /// The line index, for callers that need more than one conversion.
    #[must_use]
    pub fn index(&self) -> &LineIndex {
        &self.index
    }

    /// The position of a byte offset.
    #[must_use]
    pub fn position(
        &self,
        offset: u32,
    ) -> Position {
        self.index.position(&self.text, offset)
    }

    /// The byte offset of a position.
    #[must_use]
    pub fn offset(
        &self,
        position: Position,
    ) -> Option<u32> {
        self.index.offset(&self.text, position)
    }

    /// The range covering a byte range.
    #[must_use]
    pub fn range_of(
        &self,
        start: u32,
        end: u32,
    ) -> Range {
        Range::new(self.position(start), self.position(end))
    }

    /// The text of one line, without its terminator.
    #[must_use]
    pub fn line_text(
        &self,
        line: u32,
    ) -> Option<&str> {
        self.index.line_text(&self.text, line)
    }

    /// The range covering the whole document.
    #[must_use]
    pub fn full_range(&self) -> Range {
        Range::new(Position::origin(), self.position(u32::MAX))
    }

    /// Replaces the document's text and bumps the revision to `version`.
    pub fn replace(
        &mut self,
        text: impl Into<String>,
        version: i64,
    ) {
        self.text = text.into();
        self.index = LineIndex::new(&self.text);
        self.version = version;
    }

    /// Applies one edit at revision `version`.
    ///
    /// # Errors
    ///
    /// [`EditError`] when the range does not describe a place in the text. A
    /// refused edit leaves the document untouched — half-applying an edit
    /// would leave the client and the server disagreeing about every later
    /// offset.
    pub fn apply(
        &mut self,
        change: &Change,
        version: i64,
    ) -> Result<(), EditError> {
        match change.range {
            | None => {
                self.replace(change.text.clone(), version);
                Ok(())
            },
            | Some(range) => {
                if range.end < range.start {
                    return Err(EditError::Reversed);
                }
                let start = self
                    .index
                    .offset(&self.text, range.start)
                    .ok_or(EditError::UnknownLine(range.start.line))?;
                let end = self
                    .index
                    .offset(&self.text, range.end)
                    .ok_or(EditError::UnknownLine(range.end.line))?;
                let start = usize::try_from(start.min(end)).unwrap_or(0);
                let end = usize::try_from(end).unwrap_or(self.text.len()).max(start);
                let mut text = String::with_capacity(self.text.len() + change.text.len());
                text.push_str(&self.text[..start]);
                text.push_str(&change.text);
                text.push_str(&self.text[end..]);
                self.replace(text, version);
                Ok(())
            },
        }
    }
}

/// The documents a client has open.
///
/// Insertion order is preserved: a completion list, a symbol list or a log
/// line that reorders itself between runs is a list that cannot be tested.
#[derive(Clone, Debug, Default)]
pub struct DocumentStore {
    documents: Vec<Document>,
}

impl DocumentStore {
    /// An empty store.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Adds a document, replacing any document with the same name.
    pub fn open(
        &mut self,
        document: Document,
    ) {
        match self
            .documents
            .iter_mut()
            .find(|open| open.name == document.name)
        {
            | Some(existing) => *existing = document,
            | None => self.documents.push(document),
        }
    }

    /// Removes a document and returns it.
    pub fn close(
        &mut self,
        name: &str,
    ) -> Option<Document> {
        let index = self.documents.iter().position(|doc| doc.name == name)?;
        Some(self.documents.remove(index))
    }

    /// The document opened under `name`.
    #[must_use]
    pub fn get(
        &self,
        name: &str,
    ) -> Option<&Document> {
        self.documents.iter().find(|doc| doc.name == name)
    }

    /// The document opened under `name`, mutably.
    pub fn get_mut(
        &mut self,
        name: &str,
    ) -> Option<&mut Document> {
        self.documents.iter_mut().find(|doc| doc.name == name)
    }

    /// The open documents, in the order they were opened.
    pub fn iter(&self) -> impl Iterator<Item = &Document> {
        self.documents.iter()
    }

    /// How many documents are open.
    #[must_use]
    pub fn len(&self) -> usize {
        self.documents.len()
    }

    /// True when nothing is open.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.documents.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_incremental_edit_replaces_exactly_its_range() {
        let mut doc = Document::new("a.aev", 1, "let x = 1\nlet y = 2\n");
        // Replace `1` on line 0 with `41`.
        doc.apply(
            &Change::ranged(Range::new(Position::new(0, 8), Position::new(0, 9)), "41"),
            2,
        )
        .expect("applies");
        assert_eq!(doc.text(), "let x = 41\nlet y = 2\n");
        assert_eq!(doc.version(), 2);
    }

    #[test]
    fn an_edit_after_an_astral_character_lands_on_the_right_byte() {
        let mut doc = Document::new("a.aev", 1, "let s = \"😀\"\nlet y = 2\n");
        // The closing quote is column 11: nine units before the emoji, two for
        // it. Its byte is 13, not 10 — that is the whole point of the index.
        let start = doc.offset(Position::new(0, 11)).expect("offset");
        assert_eq!(start, 13);
        doc.apply(
            &Change::ranged(
                Range::new(Position::new(0, 11), Position::new(0, 12)),
                "!\"",
            ),
            2,
        )
        .expect("applies");
        assert_eq!(doc.text(), "let s = \"😀!\"\nlet y = 2\n");
    }

    #[test]
    fn a_full_replacement_ignores_the_previous_text() {
        let mut doc = Document::new("a.aev", 1, "let x = 1\n");
        doc.apply(&Change::full("let z = 3"), 2).expect("applies");
        assert_eq!(doc.text(), "let z = 3");
        assert_eq!(doc.full_range().end, Position::new(0, 9));
    }

    #[test]
    fn a_refused_edit_leaves_the_document_alone() {
        let mut doc = Document::new("a.aev", 1, "let x = 1\n");
        let error = doc
            .apply(
                &Change::ranged(Range::new(Position::new(5, 0), Position::new(5, 1)), "q"),
                2,
            )
            .expect_err("refuses");
        assert_eq!(error, EditError::UnknownLine(5));
        assert_eq!(doc.text(), "let x = 1\n");
        assert_eq!(doc.version(), 1);
    }

    #[test]
    fn the_store_keeps_opening_order() {
        let mut store = DocumentStore::new();
        store.open(Document::new("b.aev", 1, ""));
        store.open(Document::new("a.aev", 1, ""));
        let names: Vec<&str> = store.iter().map(Document::name).collect();
        assert_eq!(names, ["b.aev", "a.aev"]);
        assert!(store.close("b.aev").is_some());
        assert!(store.get("b.aev").is_none());
    }
}
