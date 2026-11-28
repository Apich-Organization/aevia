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
    pub const fn new(
        line: u32,
        column: u32,
    ) -> Self {
        Self { line, column }
    }
}

impl std::fmt::Display for Position {
    fn fmt(
        &self,
        f: &mut std::fmt::Formatter<'_>,
    ) -> std::fmt::Result {
        write!(
            f,
            "{}:{}",
            u64::from(self.line).saturating_add(1),
            u64::from(self.column).saturating_add(1)
        )
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
    pub const fn new(
        start: Position,
        end: Position,
    ) -> Self {
        Self { start, end }
    }

    /// True when `position` is inside the range, ends included.
    ///
    /// The end is included so that a cursor resting immediately after an
    /// identifier still hovers it, which is what every editor does.
    #[must_use]
    pub fn contains(
        self,
        position: Position,
    ) -> bool {
        self.start <= position && position <= self.end
    }
}

/// The byte and UTF-16 coordinates of a text revision.
///
/// LF, CRLF and bare CR each end one line. Pass the same text revision to
/// each conversion. ASCII columns are computed directly, and sparse UTF-16
/// checkpoints limit the part of a non-ASCII line that needs to be scanned.
#[derive(Clone, Debug)]
pub struct LineIndex {
    /// Byte offset of the first character of each line. Always starts with 0.
    starts: Vec<usize>,
    /// Each non-ASCII line and the start of its checkpoints in `utf16`.
    wide: Vec<(usize, usize)>,
    utf16: Vec<Checkpoint>,
}

#[derive(Clone, Copy, Debug)]
struct Checkpoint {
    byte: usize,
    column: usize,
}

// Save a coordinate at a character boundary once this many bytes elapse.
const CHECKPOINT_STRIDE: usize = 128;

impl Default for LineIndex {
    fn default() -> Self {
        Self::new("")
    }
}

impl LineIndex {
    /// Indexes `text`.
    #[must_use]
    pub fn new(text: &str) -> Self {
        let starts = line_starts(text);
        let mut result = Self {
            starts,
            wide: Vec::new(),
            utf16: Vec::new(),
        };
        if text.is_ascii() {
            return result;
        }
        let ends = result.starts.iter().copied().skip(1).chain([text.len()]);
        for (line, (start, end)) in result.starts.iter().copied().zip(ends).enumerate() {
            let Some(raw) = text.get(start..end) else {
                continue;
            };
            if raw.is_ascii() {
                continue;
            }
            result.wide.push((line, result.utf16.len()));
            collect_checkpoints(without_terminator(raw), &mut result.utf16);
        }
        result
    }

    fn checkpoints(
        &self,
        line: usize,
    ) -> &[Checkpoint] {
        let Ok(index) = self.wide.binary_search_by_key(&line, |(line, _)| *line) else {
            return &[];
        };
        let Some(&(_, start)) = self.wide.get(index) else {
            return &[];
        };
        let end = self
            .wide
            .get(index.saturating_add(1))
            .map_or(self.utf16.len(), |(_, start)| *start);
        self.utf16.get(start..end).unwrap_or_default()
    }

    /// The number of lines. An empty text has one (empty) line.
    #[must_use]
    pub fn line_count(&self) -> u32 {
        u32::try_from(self.starts.len()).unwrap_or(u32::MAX)
    }

    /// The byte range of a line's contents, excluding its terminator.
    #[must_use]
    pub fn line_span(
        &self,
        text: &str,
        line: u32,
    ) -> Option<std::ops::Range<usize>> {
        let line = usize::try_from(line).ok()?;
        let start = *self.starts.get(line)?;
        let end = self
            .starts
            .get(line.saturating_add(1))
            .copied()
            .unwrap_or(text.len());
        let raw = text.get(start..end)?;
        let contents = without_terminator(raw);
        Some(start..start.checked_add(contents.len())?)
    }

    /// The text of a line, without its terminator.
    #[must_use]
    pub fn line_text<'a>(
        &self,
        text: &'a str,
        line: u32,
    ) -> Option<&'a str> {
        let span = self.line_span(text, line)?;
        text.get(span)
    }

    /// The position of a byte offset.
    ///
    /// An offset past the end is clamped to the end, and an offset in the
    /// middle of a multi-byte character or CRLF sequence snaps down to that
    /// character or sequence's start.
    #[must_use]
    pub fn position(
        &self,
        text: &str,
        offset: u32,
    ) -> Position {
        let mut offset = usize::try_from(offset)
            .unwrap_or(usize::MAX)
            .min(text.len());
        while offset > 0 && !text.is_char_boundary(offset) {
            offset = offset.saturating_sub(1);
        }
        let line = match self.starts.binary_search(&offset) {
            | Ok(index) => index,
            | Err(index) => index.saturating_sub(1),
        };
        let start = self.starts.get(line).copied().unwrap_or(0);
        if offset > start
            && text.as_bytes().get(offset) == Some(&b'\n')
            && text.as_bytes().get(offset.saturating_sub(1)) == Some(&b'\r')
        {
            offset = offset.saturating_sub(1);
        }
        let byte = offset.saturating_sub(start);
        let checkpoints = self.checkpoints(line);
        let slot = checkpoints.partition_point(|mark| mark.byte <= byte);
        let column = slot
            .checked_sub(1)
            .and_then(|i| checkpoints.get(i))
            .map_or(byte, |mark| {
                let prefix = text
                    .get(start.saturating_add(mark.byte)..offset)
                    .unwrap_or_default();
                mark.column.saturating_add(utf16_len(prefix))
            });
        Position {
            line: u32::try_from(line).unwrap_or(u32::MAX),
            column: u32::try_from(column).unwrap_or(u32::MAX),
        }
    }

    /// The byte offset of a position, or `None` when the line does not exist.
    ///
    /// A column past the end of its line is clamped to the line's end, and a
    /// column inside a surrogate pair snaps down to the code point's start.
    #[must_use]
    pub fn offset(
        &self,
        text: &str,
        position: Position,
    ) -> Option<u32> {
        self.column_offset(text, position, false)
    }

    /// The byte offset of an exact UTF-16 position.
    ///
    /// Returns `None` for nonexistent lines, columns beyond the line end,
    /// or positions inside a surrogate pair.
    #[must_use]
    pub fn exact_offset(
        &self,
        text: &str,
        position: Position,
    ) -> Option<u32> {
        self.column_offset(text, position, true)
    }

    fn column_offset(
        &self,
        text: &str,
        position: Position,
        exact: bool,
    ) -> Option<u32> {
        let span = self.line_span(text, position.line)?;
        let column = usize::try_from(position.column).ok()?;
        let checkpoints = self.checkpoints(usize::try_from(position.line).ok()?);
        let slot = checkpoints.partition_point(|mark| mark.column <= column);
        let byte = match slot.checked_sub(1).and_then(|i| checkpoints.get(i)) {
            | Some(mark) => {
                let start = span.start.checked_add(mark.byte)?;
                let suffix = text.get(start..span.end)?;
                start.checked_add(byte_in_suffix(
                    suffix,
                    column.checked_sub(mark.column)?,
                    exact,
                )?)?
            },
            | None if !exact || column <= span.len() => {
                span.start.checked_add(column.min(span.len()))?
            },
            | None => return None,
        };
        if exact {
            u32::try_from(byte).ok()
        } else {
            Some(u32::try_from(byte).unwrap_or(u32::MAX))
        }
    }
}

fn without_terminator(line: &str) -> &str {
    line.strip_suffix("\r\n")
        .or_else(|| line.strip_suffix(['\r', '\n']))
        .unwrap_or(line)
}

pub(crate) fn split_last_line(text: &str) -> (&str, Option<&str>) {
    let Some(offset) = text.rfind(['\r', '\n']) else {
        return (text, None);
    };
    let (before, line) = text.split_at(offset.saturating_add(1));
    (line, Some(without_terminator(before)))
}

#[cfg_attr(
    not(test),
    expect(
        clippy::single_call_fn,
        reason = "build physical line starts independently of UTF-16 checkpoints"
    )
)]
fn line_starts(text: &str) -> Vec<usize> {
    let mut starts = vec![0];
    let mut feeds = text
        .match_indices('\n')
        .map(|(offset, _)| offset)
        .peekable();
    // Each search targets one ASCII byte. Match a CR followed by LF as one
    // separator and preserve the relative order of the two separator streams.
    for (carriage, _) in text.match_indices('\r') {
        while let Some(feed) = feeds.next_if(|feed| *feed < carriage) {
            starts.push(feed.saturating_add(1));
        }
        let after_cr = carriage.saturating_add(1);
        starts.push(
            feeds
                .next_if_eq(&after_cr)
                .map_or(after_cr, |feed| feed.saturating_add(1)),
        );
    }
    starts.extend(feeds.map(|feed| feed.saturating_add(1)));
    starts
}

#[cfg_attr(
    not(test),
    expect(
        clippy::single_call_fn,
        reason = "keep the per-line checkpoint walk independent of file indexing"
    )
)]
fn collect_checkpoints(
    line: &str,
    result: &mut Vec<Checkpoint>,
) {
    let Some(first) = line.bytes().position(|byte| !byte.is_ascii()) else {
        return;
    };
    let mut column = first;
    let mut byte = first;
    result.push(Checkpoint { byte: first, column });
    while byte.saturating_add(CHECKPOINT_STRIDE) < line.len() {
        let next = line.ceil_char_boundary(byte.saturating_add(CHECKPOINT_STRIDE));
        if next == line.len() {
            break;
        }
        let Some(chunk) = line.get(byte..next) else {
            break;
        };
        column = column.saturating_add(utf16_len(chunk));
        byte = next;
        result.push(Checkpoint { byte, column });
    }
}

fn utf16_len(text: &str) -> usize {
    // Each UTF-8 lead byte starts one UTF-16 code unit. A four-byte lead
    // needs a second unit, while UTF-8 continuation bytes count as zero.
    text.bytes()
        .map(|byte| {
            match byte {
                | 0x80..=0xbf => 0,
                | 0xf0..=0xff => 2,
                | _ => 1,
            }
        })
        .sum()
}

#[cfg_attr(
    not(test),
    expect(
        clippy::single_call_fn,
        reason = "locate a UTF-16 column inside one checkpoint interval"
    )
)]
fn byte_in_suffix(
    suffix: &str,
    mut column: usize,
    exact: bool,
) -> Option<usize> {
    for (index, character) in suffix.char_indices() {
        if column == 0 {
            return Some(index);
        }
        let Some(remaining) = column.checked_sub(character.len_utf16()) else {
            return (!exact).then_some(index);
        };
        column = remaining;
    }
    (!exact || column == 0).then_some(suffix.len())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn round_trip(
        text: &str,
        offset: usize,
    ) {
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
    fn mismatched_text_cannot_index_past_its_end() {
        let index = LineIndex::new("first\nsecond");
        assert_eq!(index.line_span("x", 0), None);
        assert_eq!(index.offset("x", Position::new(0, 0)), None);
        assert_eq!(
            Position::new(u32::MAX, u32::MAX).to_string(),
            "4294967296:4294967296"
        );
    }

    #[test]
    fn a_crlf_pair_has_no_interior_position() {
        let text = "😀\r\nlet x = 1";
        let index = LineIndex::new(text);
        assert_eq!(index.position(text, 4), Position::new(0, 2));
        assert_eq!(index.position(text, 5), Position::new(0, 2));
        assert_eq!(index.position(text, 6), Position::new(1, 0));
        assert_eq!(index.offset(text, Position::new(0, 3)), Some(4));
        assert_eq!(index.line_text(text, 0), Some("😀"));
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

    #[test]
    fn mixed_terminators_share_one_line_model_in_both_directions() {
        let lines = ["Ω", "😀", "X", "", "終", ""];
        let text = "Ω\r😀\r\nX\n\r終\r";
        let index = LineIndex::new(text);
        assert_eq!(index.line_count(), 6);
        let starts = [0, 3, 9, 11, 12, 16];
        for (line, (&start, contents)) in starts.iter().zip(lines).enumerate() {
            let line = u32::try_from(line).unwrap();
            assert_eq!(index.position(text, start), Position::new(line, 0));
            assert_eq!(index.offset(text, Position::new(line, 0)), Some(start));
            assert_eq!(index.line_text(text, line), Some(contents));
        }
        assert_eq!(index.position(text, 7), Position::new(1, 2));
        assert_eq!(index.position(text, 8), Position::new(1, 2));
        assert_eq!(index.position(text, u32::MAX), Position::new(5, 0));
        assert_eq!(split_last_line(text), ("", Some("Ω\r😀\r\nX\n\r終")));
        assert_eq!(split_last_line("Ω\r\nX"), ("X", Some("Ω")));
        assert_eq!(split_last_line("Ω"), ("Ω", None));
    }

    #[test]
    fn long_unicode_line_checkpoints_preserve_exact_and_clamped_boundaries() {
        let mut line = format!("{}Ω{}", "x".repeat(513), "a😀∫β".repeat(97));
        // Select a long line whose content ends just before a stride. When
        // followed by CRLF, no checkpoint may fall inside the terminator.
        while (line.len() - 513) % CHECKPOINT_STRIDE != CHECKPOINT_STRIDE - 1 {
            line.push('x');
        }
        let text = format!("p\r{line}\r\n");
        let index = LineIndex::new(&text);
        let mut column = 0u32;
        for (offset, character) in line.char_indices() {
            let byte = u32::try_from(offset + 2).unwrap();
            let position = Position::new(1, column);
            assert_eq!(index.position(&text, byte), position);
            assert_eq!(index.offset(&text, position), Some(byte));
            assert_eq!(index.exact_offset(&text, position), Some(byte));
            if character.len_utf8() > 1 {
                assert_eq!(index.position(&text, byte + 1), position);
            }
            if character.len_utf16() == 2 {
                let middle = Position::new(1, column + 1);
                assert_eq!(index.offset(&text, middle), Some(byte));
                assert_eq!(index.exact_offset(&text, middle), None);
            }
            column += u32::try_from(character.len_utf16()).unwrap();
        }
        let end = u32::try_from(line.len() + 2).unwrap();
        assert_eq!(
            index.exact_offset(&text, Position::new(1, column)),
            Some(end)
        );
        assert_eq!(index.position(&text, end + 1), Position::new(1, column));
        assert_eq!(index.offset(&text, Position::new(1, u32::MAX)), Some(end));
        assert_eq!(
            index.exact_offset(&text, Position::new(1, column + 1)),
            None
        );
        assert_eq!(index.exact_offset(&text, Position::new(1, u32::MAX)), None);
        assert_eq!(
            index.exact_offset(&text, Position::new(2, 0)),
            Some(end + 2)
        );
    }

    #[test]
    fn default_coordinates_match_an_empty_document() {
        let index = LineIndex::default();
        assert_eq!(index.line_count(), 1);
        assert_eq!(index.exact_offset("", Position::origin()), Some(0));
        assert_eq!(index.exact_offset("", Position::new(0, 1)), None);
    }
}
