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
    pub fn render(&self, indent: &str) -> String {
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
    let bytes = source.as_bytes();
    let mut out = Vec::new();
    let mut index = 0;
    while index < bytes.len() {
        match bytes[index] {
            b'"' => {
                index = skip_string(bytes, index);
            }
            b'r' if bytes.get(index + 1) == Some(&b'"') => {
                index = skip_raw_string(bytes, index + 1);
            }
            b'#' => {
                if bytes[index..].starts_with(b"#-") {
                    let (end, closed) = skip_block(bytes, index);
                    let text = source[index..end].trim_end().to_string();
                    out.push(Comment {
                        start: index,
                        end,
                        text,
                        block: true,
                    });
                    index = if closed { end } else { bytes.len() };
                } else if bytes[index..].starts_with(b"##") {
                    index = skip_line(bytes, index);
                } else {
                    let end = skip_line(bytes, index);
                    out.push(Comment {
                        start: index,
                        end,
                        text: source[index..end].trim_end().to_string(),
                        block: false,
                    });
                    index = end;
                }
            }
            _ => index += 1,
        }
    }
    out
}

fn skip_line(bytes: &[u8], from: usize) -> usize {
    let mut index = from;
    while index < bytes.len() && bytes[index] != b'\n' {
        index += 1;
    }
    index
}

/// Past a `"…"` or `"""…"""` literal, including `{…}` interpolations.
fn skip_string(bytes: &[u8], from: usize) -> usize {
    if bytes[from..].starts_with(b"\"\"\"") {
        let mut index = from + 3;
        while index < bytes.len() {
            if bytes[index..].starts_with(b"\"\"\"") {
                return index + 3;
            }
            index += 1;
        }
        return bytes.len();
    }
    let mut index = from + 1;
    while index < bytes.len() {
        match bytes[index] {
            b'\\' => index += 2,
            b'"' => return index + 1,
            // An interpolation may contain a nested string with a quote in it.
            b'{' => {
                index = skip_interpolation(bytes, index);
            }
            _ => index += 1,
        }
    }
    bytes.len()
}

fn skip_interpolation(bytes: &[u8], from: usize) -> usize {
    let mut index = from + 1;
    let mut depth = 1usize;
    while index < bytes.len() {
        match bytes[index] {
            b'"' => {
                index = skip_string(bytes, index);
                continue;
            }
            b'{' => depth += 1,
            b'}' => {
                depth -= 1;
                if depth == 0 {
                    return index + 1;
                }
            }
            _ => {}
        }
        index += 1;
    }
    bytes.len()
}

fn skip_raw_string(bytes: &[u8], quote: usize) -> usize {
    let mut index = quote + 1;
    while index < bytes.len() {
        if bytes[index] == b'"' {
            return index + 1;
        }
        index += 1;
    }
    bytes.len()
}

/// Past a `#- … -#` comment. Returns the end and whether it was closed.
fn skip_block(bytes: &[u8], from: usize) -> (usize, bool) {
    let mut index = from + 2;
    let mut depth = 1usize;
    while index < bytes.len() {
        if bytes[index..].starts_with(b"#-") {
            depth += 1;
            index += 2;
        } else if bytes[index..].starts_with(b"-#") {
            depth -= 1;
            index += 2;
            if depth == 0 {
                return (index, true);
            }
        } else {
            index += 1;
        }
    }
    (bytes.len(), false)
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
}
