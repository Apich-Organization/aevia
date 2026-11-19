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
        self.text
            .lines()
            .enumerate()
            .map(|(index, line)| {
                if index == 0 {
                    line.trim_end().to_string()
                } else {
                    format!("{indent}{}", line.trim_end())
                }
            })
            .collect::<Vec<_>>()
            .join("\n")
    }
}

/// Every ordinary comment in `source`, in source order.
#[must_use]
pub fn scan(source: &str) -> Vec<Comment> {
    let mut bytes = source.as_bytes();
    let mut out = Vec::new();
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
                // Like the lexer, a line comment ends at LF. A preceding CR
                // is trimmed from its text along with other trailing space.
                let remaining = tail
                    .iter()
                    .position(|b| *b == b'\n')
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
                    start,
                    end,
                    text: text.trim_end().to_string(),
                    block,
                });
            }
        }
        bytes = remaining;
    }
    out
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
    fn comments_follow_lf_boundaries_and_skip_hashes_in_interpolations() {
        let source = concat!(
            r##"let s = "{"# nested }"}" # first"##,
            "\r# second\r\n# 最后"
        );
        let comments: Vec<_> = scan(source).into_iter().map(|c| c.text).collect();
        assert_eq!(comments, ["# first\r# second", "# 最后"]);
    }
}
