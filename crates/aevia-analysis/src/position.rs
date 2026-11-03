//! Byte offsets and line/column positions.
//!
//! Aevia source is UTF-8 and every span in the compiler is a **byte** range.
//! Every editor speaks **lines and UTF-16 code units**. The two do not agree
//! on any line that contains a character outside the Basic Multilingual Plane,
//! and they do not even agree on `∫`, `°C`, `Ω` or `²` once a column is
//! measured in bytes rather than in code units.
//!
//! `∫` is one UTF-8 character, one UTF-16 code unit, and three bytes. `😀` is
//! one UTF-8 character, *two* UTF-16 code units, and four bytes. A mapping that
//! uses `chars().count()` is right for `∫` and wrong for `😀`; one that uses
//! `byte_index` is wrong for both. [`LineIndex`] is the only place in the tree
//! that knows the difference.
//!
//! Positions produced here are zero-based, which is also what the protocol
//! uses; the compiler's own `line_col` is one-based, and is left alone.

/// A zero-based line and a column in UTF-16 code units.
///
/// The column counts code units, not characters: `∫` advances it by one and
/// `😀` by two. A position that lands inside a surrogate pair is not a
/// position in the text; [`LineIndex::offset`] snaps it down to the start of
/// the code point rather than failing, because a client can send one.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug, Default, Hash)]
pub struct Position {
    /// Zero-based line.
    pub line: u32,
    /// Zero-based column, in UTF-16 code units.
    pub column: u32,
}

impl Position {
    /// The start of a document.
    #[must_use]
    pub const fn origin() -> Self {
        Self { line: 0, column: 0 }
    }

    /// Creates a position.
    #[must_use]
    pub const fn new(line: u32, column: u32) -> Self {
        Self { line, column }
    }
}

impl std::fmt::Display for Position {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}:{}", self.line + 1, self.column + 1)
    }
}

/// A half-open range between two positions.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default, Hash)]
pub struct Range {
    /// Where the range starts.
    pub start: Position,
    /// Where it ends; exclusive.
    pub end: Position,
}

impl Range {
    /// Creates a range.
    #[must_use]
    pub const fn new(start: Position, end: Position) -> Self {
        Self { start, end }
    }

    /// True when `position` is inside the range, ends included.
    ///
    /// The end is included so that a cursor resting immediately after an
    /// identifier still hovers it, which is what every editor does.
    #[must_use]
    pub fn contains(self, position: Position) -> bool {
        self.start <= position && position <= self.end
    }
}

/// The byte offset at which each line of a text starts.
///
/// Built once per document revision; every conversion in the crate goes
/// through it.
#[derive(Clone, Debug, Default)]
pub struct LineIndex {
    /// Byte offset of the first character of each line. Always starts with 0.
    starts: Vec<usize>,
}

impl LineIndex {
    /// Indexes `text`.
    #[must_use]
    pub fn new(text: &str) -> Self {
        let mut starts = vec![0];
        for (offset, byte) in text.bytes().enumerate() {
            if byte == b'\n' {
                starts.push(offset + 1);
            }
        }
        Self { starts }
    }

    /// The number of lines. An empty text has one (empty) line.
    #[must_use]
    pub fn line_count(&self) -> u32 {
        u32::try_from(self.starts.len()).unwrap_or(u32::MAX)
    }

    /// The byte range of a line's contents, excluding its terminator.
    #[must_use]
    pub fn line_span(&self, text: &str, line: u32) -> Option<std::ops::Range<usize>> {
        let start = *self.starts.get(usize::try_from(line).ok()?)?;
        let mut end = self
            .starts
            .get(usize::try_from(line).ok()? + 1)
            .copied()
            .unwrap_or(text.len());
        // The terminator belongs to no line: a client counting the characters
        // of line 3 of a CRLF file counts neither the `\r` nor the `\n`.
        while end > start && (text.as_bytes()[end - 1] == b'\n' || text.as_bytes()[end - 1] == b'\r')
        {
            end -= 1;
        }
        Some(start..end)
    }

    /// The text of a line, without its terminator.
    #[must_use]
    pub fn line_text<'a>(&self, text: &'a str, line: u32) -> Option<&'a str> {
        let span = self.line_span(text, line)?;
        text.get(span)
    }

    /// The position of a byte offset.
    ///
    /// An offset past the end is clamped to the end, and an offset in the
    /// middle of a multi-byte character snaps down to that character's start:
    /// neither can be expressed as a position, and returning the nearest one
    /// is what keeps a diagnostic from vanishing.
    #[must_use]
    pub fn position(&self, text: &str, offset: u32) -> Position {
        let mut offset = usize::try_from(offset).unwrap_or(usize::MAX).min(text.len());
        while offset > 0 && !text.is_char_boundary(offset) {
            offset -= 1;
        }
        let line = match self.starts.binary_search(&offset) {
            Ok(index) => index,
            Err(index) => index.saturating_sub(1),
        };
        let start = self.starts.get(line).copied().unwrap_or(0);
        let column = text
            .get(start..offset)
            .map_or(0, |slice| slice.chars().map(char::len_utf16).sum());
        Position {
            line: u32::try_from(line).unwrap_or(0),
            column: u32::try_from(column).unwrap_or(u32::MAX),
        }
    }

    /// The byte offset of a position, or `None` when the line does not exist.
    ///
    /// A column past the end of its line is clamped to the line's end, and a
    /// column inside a surrogate pair snaps down to the code point's start.
    #[must_use]
    pub fn offset(&self, text: &str, position: Position) -> Option<u32> {
        let span = self.line_span(text, position.line)?;
        let line = text.get(span.clone())?;
        let mut units = 0_u32;
        for (index, character) in line.char_indices() {
            // The column is inside this code point when it is at least the
            // units before it and fewer than the units it occupies. Inside a
            // surrogate pair there is no such position; the start of the code
            // point is the nearest one.
            let next = units + u32::try_from(character.len_utf16()).unwrap_or(1);
            if position.column < next {
                return Some(u32::try_from(span.start + index).unwrap_or(u32::MAX));
            }
            units = next;
        }
        // Past the last character: the end of the line is the only position
        // that can mean.
        Some(u32::try_from(span.end).unwrap_or(u32::MAX))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn round_trip(text: &str, offset: usize) {
        let index = LineIndex::new(text);
        let offset = u32::try_from(offset).expect("offset");
        let position = index.position(text, offset);
        assert_eq!(
            index.offset(text, position),
            Some(offset),
            "round trip at byte {offset} of {text:?} went through {position:?}"
        );
    }

    #[test]
    fn ascii_offsets_are_columns() {
        let text = "let x = 1\nlet y = 2\n";
        let index = LineIndex::new(text);
        assert_eq!(index.position(text, 0), Position::new(0, 0));
        assert_eq!(index.position(text, 4), Position::new(0, 4));
        assert_eq!(index.position(text, 10), Position::new(1, 0));
        assert_eq!(index.line_text(text, 1), Some("let y = 2"));
    }

    #[test]
    fn a_bmp_symbol_is_one_utf16_unit_and_three_bytes() {
        // `∫` is 3 bytes, 1 UTF-16 unit. A byte-based column would say 3.
        let text = "let ∫ = 1\n";
        let index = LineIndex::new(text);
        let offset = text.find('∫').expect("found");
        assert_eq!(offset, 4);
        let position = index.position(text, u32::try_from(offset).unwrap());
        assert_eq!(position, Position::new(0, 4));
        assert_eq!(index.offset(text, position), Some(4));
        // One byte further is inside the character; it snaps down, not up.
        assert_eq!(index.position(text, 5), Position::new(0, 4));
        // After it the column is 5, not 7: one code unit, not three bytes.
        assert_eq!(index.position(text, 7), Position::new(0, 5));
        round_trip(text, 7);
    }

    #[test]
    fn an_astral_character_is_two_utf16_units_and_four_bytes() {
        // `😀` is 4 bytes, 2 UTF-16 units. `chars().count()` says 1, bytes say
        // 4, and only UTF-16 says 2 — which is what a client counts.
        let text = "let e = \"😀\" + x\n";
        let index = LineIndex::new(text);
        let emoji = text.find('😀').expect("found");
        assert_eq!(emoji, 9);
        assert_eq!(index.position(text, 9), Position::new(0, 9));
        // The closing quote sits after the emoji: 2 units, so column 11.
        let after = text[emoji..].find('"').expect("found") + emoji;
        assert_eq!(after, 13);
        assert_eq!(
            index.position(text, u32::try_from(after).unwrap()),
            Position::new(0, 11)
        );
        assert_eq!(index.offset(text, Position::new(0, 11)), Some(13));
        // Column 10 is inside the surrogate pair: it snaps to the emoji.
        assert_eq!(index.offset(text, Position::new(0, 10)), Some(9));
        round_trip(text, after);
    }

    #[test]
    fn every_boundary_of_a_mixed_line_round_trips() {
        let text = "fn f(x: Real[L]) = x  # °C Ω ² ∫ 😀\nlet y = x²\n";
        for (offset, _) in text.char_indices() {
            round_trip(text, offset);
        }
        // The end of the text is a position too.
        round_trip(text, text.len());
    }

    #[test]
    fn a_column_past_the_end_clamps_to_the_line() {
        let text = "abc\ndef";
        let index = LineIndex::new(text);
        assert_eq!(index.offset(text, Position::new(0, 99)), Some(3));
        assert_eq!(index.offset(text, Position::new(1, 99)), Some(7));
        assert_eq!(index.offset(text, Position::new(9, 0)), None);
    }

    #[test]
    fn crlf_does_not_count_its_terminator() {
        let text = "let a = 1\r\nlet b = 2\r\n";
        let index = LineIndex::new(text);
        assert_eq!(index.line_text(text, 0), Some("let a = 1"));
        assert_eq!(index.position(text, 11), Position::new(1, 0));
        round_trip(text, 11);
        // The end of line 0 is column 9, before `\r`.
        assert_eq!(index.position(text, 9), Position::new(0, 9));
    }

    #[test]
    fn an_empty_document_has_one_line() {
        let text = "";
        let index = LineIndex::new(text);
        assert_eq!(index.line_count(), 1);
        assert_eq!(index.position(text, 0), Position::origin());
        assert_eq!(index.offset(text, Position::origin()), Some(0));
    }
}
