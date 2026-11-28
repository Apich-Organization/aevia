//! The `#` comments the AST does not carry.
//!
//! The lexer drops ordinary comments, so the formatter cannot see them in the
//! tree. Rather than silently deleting them — the one thing a formatter must
//! never do — they are recovered from the source by position and printed as
//! leading trivia of the next declaration or statement, which is where a reader
//! expects them and where a second pass will find them again.
//!
//! `##` doc comments are *not* trivia: they are [`aevia_syntax::ast::Decl::doc`]
//! and are printed from the tree, so this scanner skips them.

use aevia_syntax::Span;
use aevia_syntax::StringPart;
use aevia_syntax::Token;
use aevia_syntax::TokenKind;

/// One comment, located by byte span in the original source.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Comment {
    /// Byte offset of the `#`.
    pub start: usize,
    /// Byte offset just past the comment.
    pub end: usize,
    /// The comment text, verbatim, with trailing spaces removed.
    pub text: String,
    /// True for `#- … -#`, which may span lines.
    pub block: bool,
}

impl Comment {
    /// The text as it should be re-emitted.
    ///
    /// The first line carries no indentation: the caller is at the start of a
    /// line and indents it, and only the continuation lines of a block comment
    /// need it here.
    #[must_use]
    pub fn render(
        &self,
        indent: &str,
    ) -> String {
        self.render_with(indent, "\n")
    }

    pub(crate) fn render_with(
        &self,
        indent: &str,
        line_ending: &str,
    ) -> String {
        let mut text = String::with_capacity(self.text.len());
        for (index, line) in physical_lines(&self.text).enumerate() {
            if index > 0 {
                text.push_str(line_ending);
                text.push_str(indent);
            }
            text.push_str(line.trim_end());
        }
        text
    }
}

/// Ordinary comments outside string literals in `source`, in source order.
#[must_use]
pub fn scan(source: &str) -> Vec<Comment> {
    if !source.contains('#') {
        return Vec::new();
    }
    if let Ok(lexed) = aevia_syntax::lex(0, source) {
        return scan_lexed(source, &lexed.tokens);
    }
    let mut out = Vec::new();
    scan_text(source, 0, &mut out);
    out
}

/// Splits LF, CRLF and bare CR without dropping blank or trailing lines.
pub(crate) fn physical_lines(text: &str) -> impl DoubleEndedIterator<Item = &str> {
    text.split_inclusive('\n')
        .flat_map(|chunk| {
            chunk
                .strip_suffix('\n')
                .map_or(chunk, |line| line.strip_suffix('\r').unwrap_or(line))
                .split('\r')
        })
        .chain(text.ends_with('\n').then_some(""))
}

pub(crate) fn scan_lexed(
    source: &str,
    tokens: &[Token],
) -> Vec<Comment> {
    let mut out = Vec::new();
    if !source.contains('#') {
        return out;
    }
    for (start, gap) in token_gaps(source, tokens) {
        if gap.contains('#') {
            scan_text(gap, start, &mut out);
        }
    }
    out
}

/// Strings whose interpolation source must be copied intact.
#[allow(
    clippy::single_call_fn,
    reason = "keeps interpolation source preservation with the trivia scanner"
)]
pub(crate) fn protected_strings(
    source: &str,
    tokens: &[Token],
) -> Vec<Span> {
    tokens
        .iter()
        .filter(|token| requires_source(source, token, false))
        .map(|token| token.span)
        .collect()
}

fn requires_source(
    input: &str,
    token: &Token,
    nested: bool,
) -> bool {
    let TokenKind::Str(parts) = &token.kind else {
        return false;
    };
    let multiline = usize::try_from(token.span.start)
        .ok()
        .and_then(|start| input.get(start..))
        .is_some_and(|rest| rest.starts_with("\"\"\""));
    parts.iter().any(|part| {
        let StringPart::Interpolation { source, .. } = part else {
            // An extra interpolation decode can consume the escapes a
            // canonical nested string needs to represent these delimiters.
            return nested
                && matches!(part, StringPart::Text(text) if text.contains(['{', '}', '"']));
        };
        // Keep the original indentation and line breaks around multiline
        // interpolations intact as the surrounding code moves.
        if multiline {
            return true;
        }
        if !source.contains(['#', '"']) {
            return false;
        }
        let inline = aevia_syntax::lex_inline(0, source, 0);
        (source.contains('#')
            && token_gaps(source, &inline.tokens).any(|(_, gap)| gap.contains('#')))
            || inline
                .tokens
                .iter()
                .any(|token| requires_source(source, token, true))
    })
}

fn token_gaps<'source, 'tokens>(
    source: &'source str,
    tokens: &'tokens [Token],
) -> impl Iterator<Item = (usize, &'source str)> + use<'source, 'tokens> {
    let mut previous_end = 0usize;
    tokens.iter().filter_map(move |token| {
        let from = previous_end;
        let start = usize::try_from(token.span.start).unwrap_or(source.len());
        let end = usize::try_from(token.span.end).unwrap_or(source.len());
        previous_end = previous_end.max(end);
        (from < start)
            .then(|| source.get(from..start).map(|gap| (from, gap)))
            .flatten()
    })
}

/// The token gaps contain ordinary trivia. When lexing fails this also
/// recovers comments from an incomplete file for callers of [`scan`].
fn scan_text(
    source: &str,
    base: usize,
    out: &mut Vec<Comment>,
) {
    let mut bytes = source.as_bytes();
    while let Some((_, tail)) = bytes.split_first() {
        let (remaining, comment) = match bytes {
            | [b'r', b'"', content @ ..] => (after_byte(content, b'"'), None),
            | [b'"', b'"', b'"', content @ ..] => {
                let delimiter = b"\"\"\"";
                let remaining = content
                    .windows(delimiter.len())
                    .position(|window| window == delimiter)
                    .and_then(|end| content.get(end.saturating_add(delimiter.len())..))
                    .unwrap_or_default();
                (remaining, None)
            },
            | [b'"', ..] => (skip_string(tail), None),
            | [b'#', b'-', content @ ..] => (skip_block(content), Some(true)),
            | [b'#', ..] => {
                let remaining = tail
                    .iter()
                    .position(|&b| matches!(b, b'\n' | b'\r'))
                    .and_then(|end| tail.get(end..))
                    .unwrap_or_default();
                (remaining, (!tail.starts_with(b"#")).then_some(false))
            },
            | _ => (tail, None),
        };
        if let Some(block) = comment {
            let start = source.len().saturating_sub(bytes.len());
            let end = source.len().saturating_sub(remaining.len());
            if let Some(text) = source.get(start..end) {
                out.push(Comment {
                    start: base.saturating_add(start),
                    end: base.saturating_add(end),
                    text: text.trim_end().to_string(),
                    block,
                });
            }
        }
        bytes = remaining;
    }
}

fn after_byte(
    bytes: &[u8],
    delimiter: u8,
) -> &[u8] {
    bytes
        .iter()
        .position(|b| *b == delimiter)
        .and_then(|end| bytes.get(end.saturating_add(1)..))
        .unwrap_or_default()
}

/// Past a single-line string, including interpolations and escapes.
#[allow(
    clippy::single_call_fn,
    reason = "separates string state from top-level comment scanning"
)]
fn skip_string(mut bytes: &[u8]) -> &[u8] {
    let mut interpolation_depth = 0usize;
    while let Some((byte, tail)) = bytes.split_first() {
        bytes = match byte {
            | b'\\' if interpolation_depth == 0 && tail.starts_with(b"u{") => {
                after_byte(tail, b'}')
            },
            | b'\\' => tail.get(1..).unwrap_or_default(),
            | b'"' if interpolation_depth == 0 => return tail,
            | b'"' => {
                // Quoted text inside an interpolation can contain braces and escaped quotes.
                let mut quoted = tail;
                loop {
                    match quoted {
                        | [b'"', rest @ ..] => break rest,
                        | [b'\\', _, rest @ ..] | [_, rest @ ..] => quoted = rest,
                        | [] => break quoted,
                    }
                }
            },
            | b'{' => {
                interpolation_depth = interpolation_depth.saturating_add(1);
                tail
            },
            | b'}' => {
                interpolation_depth = interpolation_depth.saturating_sub(1);
                tail
            },
            | _ => tail,
        };
    }
    bytes
}

/// Past a possibly nested block comment.
#[allow(
    clippy::single_call_fn,
    reason = "keeps nested delimiter parsing separate from locating comments"
)]
const fn skip_block(mut bytes: &[u8]) -> &[u8] {
    let mut depth = 1usize;
    while let Some((_, tail)) = bytes.split_first() {
        bytes = match bytes {
            | [b'#', b'-', rest @ ..] => {
                depth = depth.saturating_add(1);
                rest
            },
            | [b'-', b'#', rest @ ..] => {
                depth = depth.saturating_sub(1);
                if depth == 0 {
                    return rest;
                }
                rest
            },
            | _ => tail,
        };
    }
    bytes
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn comments_are_found_and_doc_comments_are_not() {
        let source = "## doc\nlet x = 1 # trailing\n#- block -#\nlet y = 2\n";
        let comments = scan(source);
        let texts: Vec<&str> = comments.iter().map(|c| c.text.as_str()).collect();
        assert_eq!(texts, vec!["# trailing", "#- block -#"]);
        assert!(!comments[0].block);
        assert!(comments[1].block);
    }

    #[test]
    fn a_hash_inside_a_string_is_not_a_comment() {
        let source = "let s = \"a # b\"\nlet t = \"\"\"x\n# not a comment\n\"\"\"\n";
        assert!(scan(source).is_empty(), "{:?}", scan(source));
    }

    #[test]
    fn a_nested_block_comment_is_skipped_whole() {
        let source = "#- outer #- inner -# still outer -#\nlet x = 1\n";
        let comments = scan(source);
        assert_eq!(comments.len(), 1);
        assert!(comments[0].text.ends_with("-#"));
    }

    #[test]
    fn comments_follow_physical_boundaries_and_skip_literal_hashes() {
        let source = concat!(
            r##"let s = "{"# nested }"}" # first"##,
            "\r# second\r\n# 最后"
        );
        let comments: Vec<_> = scan(source).into_iter().map(|c| c.text).collect();
        assert_eq!(comments, ["# first", "# second", "# 最后"]);
    }
}
