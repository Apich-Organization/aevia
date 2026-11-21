//! The lexer: UTF-8 source to tokens with layout.
//!
//! Three things here are specific to Aevia and worth calling out:
//!
//! * **Layout.** `INDENT`/`DEDENT` are produced exactly as §1.4 describes.
//!   Inside brackets nothing is produced, so multi-line tensors and records
//!   need no continuation markers.
//! * **No automatic multiplication.** `3 m` lexes as a number followed by an
//!   identifier; the *parser* decides that this is a unit literal (§1.8.1).
//!   Keeping the decision out of the lexer is what lets units be declared after
//!   their first use.
//! * **Unicode operators.** `·`, `×`, `÷`, `−`, `≤`, `≥`, `≠` are normalised to
//!   their ASCII spelling while keeping their original span, so diagnostics and
//!   the formatter can still show what the user wrote.

use crate::diagnostic::Diagnostic;
use crate::diagnostic::Diagnostics;
use crate::span::FileId;
use crate::span::Span;
use crate::token::DocComment;
use crate::token::Keyword;
use crate::token::Punct;
use crate::token::StringPart;
use crate::token::Token;
use crate::token::TokenKind;
use num_bigint::BigInt;
use num_traits::Num;
use std::borrow::Cow;
use unicode_ident::is_xid_continue;
use unicode_ident::is_xid_start;
use unicode_normalization::UnicodeNormalization;

/// The result of lexing a file.
#[derive(Clone, Debug)]
pub struct LexOutput {
    /// The token stream, ending with [`TokenKind::Eof`].
    pub tokens: Vec<Token>,
    /// Documentation comments in source order.
    pub docs: Vec<DocComment>,
}

/// Lexes a whole file.
///
/// # Errors
///
/// Returns every lexical diagnostic that was recovered from; the caller
/// renders them and stops before parsing.
pub fn lex(
    file: FileId,
    text: &str,
) -> Result<LexOutput, Diagnostics> {
    Lexer::new(file, text, 0).run()
}

/// Lexes an inline expression, used for string interpolation.
///
/// `base` is the byte offset of `text` inside its file, so spans stay correct.
/// Inline expressions suppress layout. Invalid text produces a single EOF
/// token whose span covers the input.
#[must_use]
pub fn lex_inline(
    file: FileId,
    text: &str,
    base: usize,
) -> LexOutput {
    Lexer::new(file, text, base)
        .run_inline()
        .unwrap_or_else(|_diagnostics| {
            LexOutput {
                tokens: vec![Token::new(
                    TokenKind::Eof,
                    Span::new(file, base, base.saturating_add(text.len())),
                )],
                docs: Vec::new(),
            }
        })
}

/// Lexes an interpolation while retaining diagnostic kinds and original byte
/// offsets. Only backslashes protecting a quote or brace can disappear
/// between the original slice and the expression passed to the lexer.
#[allow(
    clippy::single_call_fn,
    reason = "keeps inline lexing and offset reconstruction out of the parser"
)]
pub(crate) fn lex_interpolation(
    span: Span,
    original: &str,
    source: &str,
) -> Result<LexOutput, Diagnostics> {
    let mut lexed = Lexer::new(span.file, source, span.start as usize).run_inline();
    if original.len() <= source.len() {
        return lexed;
    }
    let removed = removed_delimiter_escapes(original, source);
    if removed.is_empty() {
        return lexed;
    }
    let remap = |position: &mut Span| {
        let original_offset = |offset: u32| {
            let relative = offset.saturating_sub(span.start) as usize;
            let missing = removed.partition_point(|&removed| removed < relative);
            offset.saturating_add(u32::try_from(missing).unwrap_or(u32::MAX))
        };
        position.start = original_offset(position.start);
        position.end = original_offset(position.end);
    };
    match &mut lexed {
        | Ok(output) => {
            for token in &mut output.tokens {
                remap(&mut token.span);
                if let TokenKind::Str(parts) = &mut token.kind {
                    for part in parts {
                        if let StringPart::Interpolation { span, .. } = part {
                            remap(span);
                        }
                    }
                }
            }
            for doc in &mut output.docs {
                remap(&mut doc.span);
            }
        },
        | Err(diagnostics) => {
            for diagnostic in &mut diagnostics.items {
                remap(&mut diagnostic.span);
                for label in &mut diagnostic.labels {
                    remap(&mut label.span);
                }
            }
        },
    }
    lexed
}

/// Classifies an interpolation consisting of one ordinary ASCII identifier.
/// These words have no delimiter escapes, parser lookahead, or layout.
#[allow(
    clippy::single_call_fn,
    reason = "keeps the interpolation fast path on the shared identifier rules"
)]
pub(crate) fn interpolation_identifier(
    span: Span,
    source: &str,
) -> Option<(&str, Span)> {
    const LAYOUT: [char; 4] = [' ', '\t', '\r', '\n'];
    if span.len() != source.len() {
        return None;
    }
    let body = source.trim_start_matches(LAYOUT);
    let name = body.trim_end_matches(LAYOUT);
    if !name
        .as_bytes()
        .first()
        .is_some_and(|&byte| byte.is_ascii_alphabetic() || byte == b'_')
        || ascii_identifier_prefix_len(name) != name.len()
        || Keyword::from_word(name).is_some()
    {
        return None;
    }
    let start = (span.start as usize).saturating_add(source.len().saturating_sub(body.len()));
    Some((
        name,
        Span::new(span.file, start, start.saturating_add(name.len())),
    ))
}

fn ascii_identifier_prefix_len(text: &str) -> usize {
    text.bytes()
        .take_while(|&byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'\''))
        .count()
}

#[allow(
    clippy::single_call_fn,
    reason = "isolates source offset reconstruction"
)]
fn removed_delimiter_escapes(
    original: &str,
    source: &str,
) -> Vec<usize> {
    let mut removed = Vec::new();
    let mut original = original.bytes();
    for (offset, byte) in source.bytes().enumerate() {
        match original.next() {
            | Some(raw) if raw == byte => {},
            | Some(b'\\') if matches!(byte, b'"' | b'{' | b'}') && original.next() == Some(byte) =>
            {
                removed.push(offset);
            },
            // A host may supply its own tokens to parse_module. Without the
            // original spelling, use the offsets of the supplied expression.
            | _ => return Vec::new(),
        }
    }
    if original.next().is_some() {
        return Vec::new();
    }
    removed
}

struct Lexer<'a> {
    file: FileId,
    src: &'a str,
    base: usize,
    pos: usize,
    tokens: Vec<Token>,
    docs: Vec<DocComment>,
    indent_stack: Vec<usize>,
    bracket_depth: usize,
    inline: bool,
    at_line_start: bool,
    line_has_token: bool,
    diagnostics: Diagnostics,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum StringDelimiter {
    Ordinary,
    Raw,
    Multiline,
    Protected,
    ProtectedRaw,
}

impl StringDelimiter {
    const fn closing(self) -> &'static str {
        match self {
            | Self::Ordinary | Self::Raw => "\"",
            | Self::Multiline => "\"\"\"",
            | Self::Protected | Self::ProtectedRaw => "\\\"",
        }
    }
}

impl<'a> Lexer<'a> {
    fn new(
        file: FileId,
        src: &'a str,
        base: usize,
    ) -> Self {
        Self {
            file,
            src,
            base,
            pos: 0,
            tokens: Vec::new(),
            docs: Vec::new(),
            indent_stack: Vec::new(),
            bracket_depth: 0,
            inline: false,
            at_line_start: true,
            line_has_token: false,
            diagnostics: Diagnostics::new(),
        }
    }

    fn run(mut self) -> Result<LexOutput, Diagnostics> {
        // A UTF-8 BOM is allowed and ignored (§1.1).
        if self.src.starts_with('\u{feff}') {
            self.pos = self.pos.saturating_add('\u{feff}'.len_utf8());
        }
        // A shebang line is ignored.
        if self.remaining().starts_with("#!") {
            self.skip_to_line_end();
        }
        self.scan();
        self.finish();
        if self.diagnostics.has_errors() {
            Err(self.diagnostics)
        } else {
            Ok(LexOutput {
                tokens: self.tokens,
                docs: self.docs,
            })
        }
    }

    fn run_inline(mut self) -> Result<LexOutput, Diagnostics> {
        // The braces bound the expression independently of its own delimiters.
        self.inline = true;
        self.at_line_start = false;
        self.scan();
        self.finish();
        if self.diagnostics.has_errors() {
            Err(self.diagnostics)
        } else {
            Ok(LexOutput {
                tokens: self.tokens,
                docs: self.docs,
            })
        }
    }

    fn scan(&mut self) {
        loop {
            if self.at_line_start && self.layout_enabled() {
                if !self.handle_line_start() {
                    return;
                }
                continue;
            }
            self.skip_spaces();
            let Some(c) = self.peek() else {
                return;
            };
            match c {
                | '\n' | '\r' => self.newline(),
                | '#' => {
                    self.comment();
                },
                | '"' => {
                    self.string();
                },
                | 'r' if self.remaining().starts_with("r\"") => {
                    self.bump();
                    self.raw_string();
                },
                | c if c.is_ascii_digit() => self.number(),
                | c if is_xid_start(c) || c == '_' => self.identifier(),
                | '°' => self.degree_unit(),
                | c if is_superscript(c) => self.superscript(),
                | _ => self.punct(c),
            }
        }
    }

    fn finish(&mut self) {
        // A final line without a trailing newline still ends a logical line
        // (§1.4.1: every logical line ends with `NEWLINE`). The token is
        // zero-width at end of file, so positions are unaffected.
        if self.line_has_token && self.layout_enabled() {
            let at = self.mark();
            self.push(TokenKind::Newline, at, at);
            self.line_has_token = false;
        }
        while self.indent_stack.pop().is_some() {
            let at = self.mark();
            self.push(TokenKind::Dedent, at, at);
        }
        let at = self.mark();
        self.push(TokenKind::Eof, at, at);
    }

    const fn mark(&self) -> usize {
        self.pos
    }

    const fn layout_enabled(&self) -> bool {
        !self.inline && self.bracket_depth == 0
    }

    fn span(
        &self,
        start: usize,
        end: usize,
    ) -> Span {
        Span::new(
            self.file,
            self.base.saturating_add(start),
            self.base.saturating_add(end),
        )
    }

    fn remaining(&self) -> &str {
        self.src.get(self.pos..).unwrap_or_default()
    }

    fn peek(&self) -> Option<char> {
        self.remaining().chars().next()
    }

    fn peek_at(
        &self,
        offset: usize,
    ) -> Option<char> {
        self.remaining().chars().nth(offset)
    }

    fn bump(&mut self) -> Option<char> {
        let c = self.peek()?;
        self.pos = self.pos.saturating_add(c.len_utf8());
        Some(c)
    }

    fn skip_spaces(&mut self) {
        let length = self
            .remaining()
            .bytes()
            .take_while(|&byte| matches!(byte, b' ' | b'\t'))
            .count();
        self.pos = self.pos.saturating_add(length);
    }

    fn skip_to_line_end(&mut self) {
        let remaining = self.remaining();
        let offset = remaining.find(['\r', '\n']).unwrap_or(remaining.len());
        self.pos = self.pos.saturating_add(offset);
    }

    fn push(
        &mut self,
        kind: TokenKind,
        start: usize,
        end: usize,
    ) {
        if !matches!(kind, TokenKind::Newline | TokenKind::Dedent) {
            self.line_has_token = true;
        }
        self.tokens.push(Token::new(kind, self.span(start, end)));
    }

    fn emit(
        &mut self,
        kind: TokenKind,
        start: usize,
    ) {
        let end = self.mark();
        self.push(kind, start, end);
    }

    fn error(
        &mut self,
        code: &str,
        message: impl Into<String>,
        start: usize,
        end: usize,
    ) {
        let span = self.span(start, end);
        self.diagnostics
            .push(Diagnostic::error(code, message, span));
    }

    fn newline(&mut self) {
        let start = self.mark();
        if self.peek() == Some('\r') {
            self.bump();
            if self.peek() == Some('\n') {
                self.bump();
            }
        } else {
            self.bump();
        }
        if self.layout_enabled() {
            if self.line_has_token {
                self.push(TokenKind::Newline, start, self.mark());
            }
            self.at_line_start = true;
            self.line_has_token = false;
        }
    }

    /// Handles indentation at the start of a logical line.
    ///
    /// Returns false at end of file.
    fn handle_line_start(&mut self) -> bool {
        loop {
            let mut indent = 0usize;
            loop {
                match self.peek() {
                    | Some(' ') => {
                        self.bump();
                        indent = indent.saturating_add(1);
                    },
                    | Some('\t') => {
                        let start = self.mark();
                        self.bump();
                        indent = indent.saturating_add(1);
                        self.error("E0101", "tab character in indentation", start, self.mark());
                        // Keep measuring so the rest of the line still parses.
                        while let Some(' ' | '\t') = self.peek() {
                            if self.peek() == Some('\t') {
                                let at = self.mark();
                                self.bump();
                                self.error(
                                    "E0101",
                                    "tab character in indentation",
                                    at,
                                    self.mark(),
                                );
                            } else {
                                self.bump();
                            }
                            indent = indent.saturating_add(1);
                        }
                    },
                    | _ => break,
                }
            }
            match self.peek() {
                | None => {
                    self.at_line_start = false;
                    return false;
                },
                | Some('\n' | '\r') => {
                    self.bump();
                    continue;
                },
                | Some('#') => {
                    let spanned_newline = self.comment();
                    if spanned_newline {
                        // The comment swallowed one or more line breaks. Code
                        // that follows belongs to the current block level, not
                        // to an indent measured from the comment's tail.
                        self.skip_spaces();
                        if matches!(self.peek(), Some('\n' | '\r') | None) {
                            continue;
                        }
                        self.apply_indent(*self.indent_stack.last().unwrap_or(&0));
                        self.at_line_start = false;
                        return true;
                    }
                    self.skip_spaces();
                    match self.peek() {
                        | None => {
                            self.at_line_start = false;
                            return false;
                        },
                        | Some('\n' | '\r') => {
                            self.bump();
                            continue;
                        },
                        | _ => {},
                    }
                },
                | _ => {},
            }
            self.apply_indent(indent);
            self.at_line_start = false;
            return true;
        }
    }

    fn apply_indent(
        &mut self,
        indent: usize,
    ) {
        let current = *self.indent_stack.last().unwrap_or(&0);
        let at = self.mark();
        if indent > current {
            self.indent_stack.push(indent);
            self.push(TokenKind::Indent, at, at);
        } else if indent < current {
            while self
                .indent_stack
                .last()
                .is_some_and(|&level| level > indent)
            {
                self.indent_stack.pop();
                self.push(TokenKind::Dedent, at, at);
            }
            if *self.indent_stack.last().unwrap_or(&0) != indent {
                // §1.4.1: the level must match one on the stack. Recovery must
                // not adopt the bogus level — pushing it would make every later
                // line at that indent look well-formed and hide the error.
                self.error(
                    "E0102",
                    "inconsistent indentation",
                    at,
                    at.saturating_add(
                        indent.saturating_sub(*self.indent_stack.last().unwrap_or(&0)),
                    ),
                );
            }
        }
    }

    /// Skips a comment. Returns true when it consumed a newline, which means
    /// the following text starts a new physical line.
    fn comment(&mut self) -> bool {
        let start = self.mark();
        if self.remaining().starts_with("#-") {
            let (closed, newline) = self.skip_block_comment();
            if !closed {
                self.error("E0104", "unclosed block comment", start, self.mark());
            }
            return newline;
        }
        if self.remaining().starts_with("##") {
            self.pos = self.pos.saturating_add(2);
            let text_start = self.pos;
            self.skip_to_line_end();
            let text = self
                .src
                .get(text_start..self.pos)
                .unwrap_or_default()
                .trim()
                .to_string();
            self.docs.push(DocComment {
                span: self.span(start, self.mark()),
                text,
            });
            return false;
        }
        self.skip_to_line_end();
        false
    }

    fn skip_block_comment(&mut self) -> (bool, bool) {
        self.pos = self.pos.saturating_add(2);
        // Comment delimiters and line breaks are ASCII. Scanning their bytes
        // lets us skip UTF-8 text without decoding it, and stops only at an
        // ASCII delimiter or EOF, both of which are character boundaries.
        let content = self.remaining().as_bytes();
        let mut bytes = content.iter();
        let mut depth = 1usize;
        let mut newline = false;
        while let Some(&byte) = bytes.next() {
            match byte {
                | b'#' if bytes.as_slice().first() == Some(&b'-') => {
                    bytes.next();
                    depth = depth.saturating_add(1);
                },
                | b'-' if bytes.as_slice().first() == Some(&b'#') => {
                    bytes.next();
                    depth = depth.saturating_sub(1);
                    if depth == 0 {
                        break;
                    }
                },
                | b'\r' | b'\n' => newline = true,
                | _ => {},
            }
        }
        self.pos = self
            .pos
            .saturating_add(content.len().saturating_sub(bytes.len()));
        (depth == 0, newline)
    }

    fn skip_identifier(&mut self) -> bool {
        let remaining = self.remaining();
        let ascii = ascii_identifier_prefix_len(remaining);
        let tail = remaining.get(ascii..).unwrap_or_default();
        let unicode = if tail.as_bytes().first().is_some_and(|byte| !byte.is_ascii()) {
            tail.char_indices()
                .find_map(|(offset, ch)| {
                    (!is_xid_continue(ch) && ch != '\'' && !('₀'..='₉').contains(&ch))
                        .then_some(offset)
                })
                .unwrap_or(tail.len())
        } else {
            0
        };
        self.pos = self.pos.saturating_add(ascii).saturating_add(unicode);
        unicode == 0
    }

    fn identifier(&mut self) {
        let start = self.mark();
        let ascii = self.skip_identifier();
        let raw = self.src.get(start..self.pos).unwrap_or_default();
        // §1.5.1: identifiers are compared in NFC, so `é` written as `e` plus a
        // combining accent is the same name as the precomposed one.
        let text = if ascii {
            Cow::Borrowed(raw)
        } else {
            Cow::Owned(raw.nfc().collect())
        };
        let kind = match Keyword::from_word(&text) {
            | Some(keyword) => TokenKind::Keyword(keyword),
            | None => {
                // Only ASCII capitals introduce a `UIDENT`: the spec's own
                // example `Δt` is a value binding, not a constructor (§1.5.1).
                let first = text.chars().next().unwrap_or('_');
                if first.is_ascii_uppercase() {
                    TokenKind::UpperIdent(text.into_owned())
                } else {
                    TokenKind::Ident(text.into_owned())
                }
            },
        };
        self.emit(kind, start);
    }

    fn degree_unit(&mut self) {
        let start = self.mark();
        self.bump();
        if let Some(next) = self.peek()
            && (next == 'C' || next == 'F')
        {
            self.bump();
        }
        let text = self
            .src
            .get(start..self.pos)
            .unwrap_or_default()
            .to_string();
        self.emit(TokenKind::Ident(text), start);
    }

    fn superscript(&mut self) {
        let start = self.mark();
        while let Some(c) = self.peek() {
            if is_superscript(c) {
                self.bump();
            } else {
                break;
            }
        }
        let text = self
            .src
            .get(start..self.pos)
            .unwrap_or_default()
            .to_string();
        self.emit(TokenKind::Superscript(text), start);
    }

    #[allow(
        clippy::too_many_lines,
        reason = "keeps numeric literal validation and error recovery together"
    )]
    fn number(&mut self) {
        let start = self.mark();
        if self.peek() == Some('0')
            && let Some(radix_char) = self.peek_at(1)
        {
            let radix = match radix_char {
                | 'x' | 'X' => Some(16),
                | 'o' | 'O' => Some(8),
                | 'b' | 'B' => Some(2),
                | _ => None,
            };
            if let Some(radix) = radix {
                self.bump();
                self.bump();
                let digits_start = self.pos;
                let mut cleaned = String::new();
                let mut last_underscore = false;
                while let Some(c) = self.peek() {
                    if c == '_' {
                        if cleaned.is_empty() || last_underscore {
                            let at = self.mark();
                            self.bump();
                            self.error(
                                "E0106",
                                "misplaced `_` in numeric literal",
                                at,
                                self.mark(),
                            );
                            continue;
                        }
                        last_underscore = true;
                        self.bump();
                    } else if c.is_digit(radix) {
                        cleaned.push(c);
                        last_underscore = false;
                        self.bump();
                    } else {
                        break;
                    }
                }
                if last_underscore {
                    let at = self.mark();
                    self.error("E0106", "numeric literal ends with `_`", at, at);
                }
                if cleaned.is_empty() {
                    self.error("E0106", "numeric literal has no digits", start, self.mark());
                    return;
                }
                if self.pos == digits_start {
                    self.error("E0106", "numeric literal has no digits", start, self.mark());
                    return;
                }
                if let Some(next) = self.peek()
                    && next.is_alphanumeric()
                {
                    let at = self.mark();
                    self.bump();
                    self.error(
                        "E0106",
                        format!("`{next}` is not a digit of this base"),
                        at,
                        self.mark(),
                    );
                }
                match BigInt::from_str_radix(&cleaned, radix) {
                    | Ok(value) => self.emit(TokenKind::Int(value), start),
                    | Err(_) => self.error("E0106", "invalid numeric literal", start, self.mark()),
                }
                return;
            }
        }

        let mut cleaned = String::new();
        let mut last_underscore = false;
        let mut is_float = false;
        while let Some(c) = self.peek() {
            if c.is_ascii_digit() {
                cleaned.push(c);
                last_underscore = false;
                self.bump();
            } else if c == '_' {
                if cleaned.is_empty() || last_underscore {
                    let at = self.mark();
                    self.bump();
                    self.error("E0106", "misplaced `_` in numeric literal", at, self.mark());
                    continue;
                }
                last_underscore = true;
                self.bump();
            } else {
                break;
            }
        }
        if last_underscore {
            let at = self.mark();
            self.error("E0106", "numeric literal ends with `_`", at, at);
        }
        if self.peek() == Some('.') && self.peek_at(1).is_some_and(|c| c.is_ascii_digit()) {
            is_float = true;
            cleaned.push('.');
            self.bump();
            let mut last_underscore = false;
            while let Some(c) = self.peek() {
                if c.is_ascii_digit() {
                    cleaned.push(c);
                    last_underscore = false;
                    self.bump();
                } else if c == '_' {
                    if last_underscore || cleaned.ends_with('.') {
                        let at = self.mark();
                        self.bump();
                        self.error("E0106", "misplaced `_` in numeric literal", at, self.mark());
                        continue;
                    }
                    last_underscore = true;
                    self.bump();
                } else {
                    break;
                }
            }
            if last_underscore {
                let at = self.mark();
                self.error("E0106", "numeric literal ends with `_`", at, at);
            }
        }
        if matches!(self.peek(), Some('e' | 'E')) {
            let mut lookahead = 1usize;
            if matches!(self.peek_at(lookahead), Some('+' | '-')) {
                lookahead = lookahead.saturating_add(1);
            }
            if self.peek_at(lookahead).is_some_and(|c| c.is_ascii_digit()) {
                is_float = true;
                cleaned.push('e');
                self.bump();
                if let Some(sign @ ('+' | '-')) = self.peek() {
                    cleaned.push(sign);
                    self.bump();
                }
                while let Some(c) = self.peek() {
                    if c.is_ascii_digit() {
                        cleaned.push(c);
                        self.bump();
                    } else {
                        break;
                    }
                }
            }
        }
        if is_float {
            match cleaned.parse::<f64>() {
                | Ok(value) => self.emit(TokenKind::Float(value), start),
                | Err(_) => {
                    self.error(
                        "E0106",
                        "invalid floating-point literal",
                        start,
                        self.mark(),
                    );
                },
            }
        } else {
            match BigInt::from_str_radix(&cleaned, 10) {
                | Ok(value) => self.emit(TokenKind::Int(value), start),
                | Err(_) => self.error("E0106", "invalid integer literal", start, self.mark()),
            }
        }
    }

    #[allow(
        clippy::too_many_lines,
        reason = "keeps literal escapes and string recovery in one scanner"
    )]
    fn string(&mut self) {
        let start = self.mark();
        if self.remaining().starts_with("\"\"\"") {
            self.multiline_string(start);
            return;
        }
        self.pos = self.pos.saturating_add(1);
        let mut parts: Vec<StringPart> = Vec::new();
        let mut text = String::new();
        loop {
            match self.peek() {
                | None | Some('\r' | '\n') => {
                    self.error("E0104", "unclosed string literal", start, self.mark());
                    break;
                },
                | Some('"') => {
                    self.bump();
                    break;
                },
                | Some('\\') => {
                    self.bump();
                    match self.peek() {
                        | Some('n') => {
                            text.push('\n');
                            self.bump();
                        },
                        | Some('t') => {
                            text.push('\t');
                            self.bump();
                        },
                        | Some('r') => {
                            text.push('\r');
                            self.bump();
                        },
                        | Some('0') => {
                            text.push('\0');
                            self.bump();
                        },
                        | Some('\\') => {
                            text.push('\\');
                            self.bump();
                        },
                        | Some('"') => {
                            text.push('"');
                            self.bump();
                        },
                        | Some('{') => {
                            text.push('{');
                            self.bump();
                        },
                        | Some('}') => {
                            text.push('}');
                            self.bump();
                        },
                        | Some('u') => {
                            self.bump();
                            if self.peek() != Some('{') {
                                self.error("E0104", "expected `{` after `\\u`", start, self.mark());
                                continue;
                            }
                            self.bump();
                            let hex_start = self.pos;
                            while let Some(c) = self.peek() {
                                if c == '}' {
                                    break;
                                }
                                self.bump();
                            }
                            let hex = self
                                .src
                                .get(hex_start..self.pos)
                                .unwrap_or_default()
                                .to_string();
                            if self.peek() != Some('}') {
                                self.error(
                                    "E0104",
                                    "unclosed `\\u{...}` escape",
                                    start,
                                    self.mark(),
                                );
                                continue;
                            }
                            self.bump();
                            match u32::from_str_radix(&hex, 16).ok().and_then(char::from_u32) {
                                | Some(c) => text.push(c),
                                | None => {
                                    self.error(
                                        "E0105",
                                        format!("`\\u{{{hex}}}` is not a Unicode scalar value"),
                                        hex_start,
                                        self.pos,
                                    );
                                },
                            }
                        },
                        | Some(other) => {
                            let at = self.mark();
                            self.bump();
                            self.error(
                                "E0105",
                                format!("unknown escape `\\{other}`"),
                                at,
                                self.mark(),
                            );
                        },
                        | None => {},
                    }
                },
                | Some('{') => {
                    if !text.is_empty() {
                        parts.push(StringPart::Text(std::mem::take(&mut text)));
                    }
                    match self.interpolation() {
                        | Some(part) => parts.push(part),
                        | None => {
                            self.skip_to_line_end();
                            break;
                        },
                    }
                },
                | Some(c) => {
                    text.push(c);
                    self.bump();
                },
            }
        }
        if !text.is_empty() {
            parts.push(StringPart::Text(text));
        }
        self.emit(TokenKind::Str(parts), start);
    }

    fn interpolation(&mut self) -> Option<StringPart> {
        let open = self.mark();
        self.bump(); // '{'
        let content_start = self.pos;
        let mut protected_delimiters = Vec::new();
        let before = self.diagnostics.items.len();
        if !self.skip_interpolation(0, &mut protected_delimiters) {
            if self.diagnostics.items.len() == before {
                self.error(
                    "E0104",
                    "unclosed interpolation `{...}` in string",
                    open,
                    self.mark(),
                );
            }
            return None;
        }
        let content_end = self.mark();
        let raw = self.src.get(content_start..content_end).unwrap_or_default();
        let span = self.span(content_start, content_end);
        self.bump(); // '}'
        // `"{}"` is a placeholder in other languages; here it is an
        // interpolation with nothing in it.
        if raw.trim().is_empty() {
            let span = self.span(open, self.mark());
            self.diagnostics.push(
                Diagnostic::error("E0104", "empty interpolation `{}` in a string", span).with_help(
                    "write an expression inside the braces, `\\{` for a literal brace, \\
                     or a raw string r\"…\" (no interpolation) for a `format` template",
                ),
            );
            return Some(StringPart::Text(String::new()));
        }
        let source = if protected_delimiters.is_empty() {
            raw.to_owned()
        } else {
            let mut source =
                String::with_capacity(raw.len().saturating_sub(protected_delimiters.len()));
            let mut previous = content_start;
            for slash in protected_delimiters {
                source.push_str(self.src.get(previous..slash).unwrap_or_default());
                previous = slash.saturating_add(1);
            }
            source.push_str(self.src.get(previous..content_end).unwrap_or_default());
            source
        };
        Some(StringPart::Interpolation { source, span })
    }

    // Follow the lexical delimiters in the embedded expression without
    // allocating its tokens; the parser lexes it on demand.
    fn skip_interpolation(
        &mut self,
        nesting: usize,
        protected_delimiters: &mut Vec<usize>,
    ) -> bool {
        let mut braces = 1usize;
        while let Some(c) = self.peek() {
            let delimiter = match c {
                | '"' if self.remaining().starts_with("\"\"\"") => Some(StringDelimiter::Multiline),
                | '"' => Some(StringDelimiter::Ordinary),
                | '\\' if self.remaining().starts_with("\\\"") => Some(StringDelimiter::Protected),
                | 'r' if self.remaining().starts_with("r\"") => Some(StringDelimiter::Raw),
                | 'r' if self.remaining().starts_with("r\\\"") => {
                    Some(StringDelimiter::ProtectedRaw)
                },
                | _ => None,
            };
            if let Some(delimiter) = delimiter {
                let start = self.mark();
                if !self.skip_nested_string(delimiter, nesting, protected_delimiters) {
                    self.pos = start;
                    return false;
                }
                continue;
            }
            match c {
                | '\\' if matches!(self.peek_at(1), Some('{' | '}')) => {
                    protected_delimiters.push(self.pos);
                    self.bump();
                    if self.peek() == Some('{') {
                        braces = braces.saturating_add(1);
                    } else if braces > 1 {
                        braces = braces.saturating_sub(1);
                    }
                },
                | '{' => braces = braces.saturating_add(1),
                | '}' => {
                    braces = braces.saturating_sub(1);
                    if braces == 0 {
                        return true;
                    }
                },
                | '#' => {
                    if self.remaining().starts_with("#-") {
                        if !self.skip_block_comment().0 {
                            return false;
                        }
                    } else {
                        self.skip_to_line_end();
                    }
                    continue;
                },
                | c if is_xid_start(c) || c == '_' => {
                    self.skip_identifier();
                    continue;
                },
                | _ => {},
            }
            self.bump();
        }
        false
    }

    fn skip_nested_string(
        &mut self,
        delimiter: StringDelimiter,
        nesting: usize,
        protected_delimiters: &mut Vec<usize>,
    ) -> bool {
        const MAX_STRING_NESTING: usize = 96;
        if nesting == MAX_STRING_NESTING {
            let at = self.mark();
            self.error("E0104", "string interpolation nesting is too deep", at, at);
            return false;
        }
        let raw = matches!(
            delimiter,
            StringDelimiter::Raw | StringDelimiter::ProtectedRaw
        );
        let protected = matches!(
            delimiter,
            StringDelimiter::Protected | StringDelimiter::ProtectedRaw
        );
        if raw {
            self.bump(); // 'r'
        }
        if protected {
            protected_delimiters.push(self.pos);
        }
        let closing = delimiter.closing();
        self.pos = self.pos.saturating_add(closing.len());
        while let Some(c) = self.peek() {
            if self.remaining().starts_with(closing) {
                if protected {
                    protected_delimiters.push(self.pos);
                }
                self.pos = self.pos.saturating_add(closing.len());
                return true;
            }
            match c {
                | '\r' | '\n' if delimiter != StringDelimiter::Multiline => return false,
                | '\\' if !raw => self.skip_literal_escape(),
                | '{' if !raw => {
                    self.bump();
                    if !self.skip_interpolation(nesting.saturating_add(1), protected_delimiters) {
                        return false;
                    }
                    self.bump(); // '}'
                },
                | _ => {
                    self.bump();
                },
            }
        }
        false
    }

    fn skip_literal_escape(&mut self) {
        self.bump(); // '\\'
        if self.bump() == Some('u') && self.peek() == Some('{') {
            self.bump();
            while let Some(c) = self.peek() {
                if matches!(c, '"' | '\r' | '\n') {
                    break;
                }
                self.bump();
                if c == '}' {
                    break;
                }
            }
        }
    }

    fn raw_string(&mut self) {
        let start = self.mark();
        self.pos = self.pos.saturating_add(1); // opening quote
        let content_start = self.pos;
        loop {
            match self.peek() {
                | None | Some('\r' | '\n') => {
                    self.error("E0104", "unclosed raw string literal", start, self.mark());
                    break;
                },
                | Some('"') => break,
                | Some(_) => {
                    self.bump();
                },
            }
        }
        let text = self
            .src
            .get(content_start..self.pos)
            .unwrap_or_default()
            .to_string();
        if self.peek() == Some('"') {
            self.bump();
        }
        self.emit(TokenKind::Str(vec![StringPart::Text(text)]), start);
    }

    fn multiline_string(
        &mut self,
        start: usize,
    ) {
        let delimiter = StringDelimiter::Multiline.closing();
        self.pos = self.pos.saturating_add(delimiter.len());
        let content_start = self.pos;
        let mut interpolations = Vec::new();
        while let Some(c) = self.peek() {
            if self.remaining().starts_with(delimiter) {
                let end = self.mark();
                self.pos = self.pos.saturating_add(delimiter.len());
                let raw = self.src.get(content_start..end).unwrap_or_default();
                let span = self.span(start, end);
                let parts = multiline_parts(raw, span, interpolations, &mut self.diagnostics);
                self.emit(TokenKind::Str(parts), start);
                return;
            }
            match c {
                | '\\' => self.skip_literal_escape(),
                | '{' => {
                    let open = self.pos.saturating_sub(content_start);
                    let Some(part) = self.interpolation() else {
                        let remaining = self.remaining();
                        let skip = remaining
                            .find(delimiter)
                            .map_or(remaining.len(), |at| at.saturating_add(delimiter.len()));
                        self.pos = self.pos.saturating_add(skip);
                        return;
                    };
                    let after = self.pos.saturating_sub(content_start);
                    interpolations.push((open..after, part));
                },
                | _ => {
                    self.bump();
                },
            }
        }
        self.error(
            "E0104",
            "unclosed multi-line string literal",
            start,
            self.mark(),
        );
    }

    fn punct(
        &mut self,
        first: char,
    ) {
        let bytes = self.remaining().as_bytes();
        let next = bytes.get(1).copied();
        let punct = match (first, next) {
            | ('.', Some(b'.')) => {
                match bytes.get(2) {
                    | Some(b'.') => Punct::Ellipsis,
                    | Some(b'=') => Punct::DotDotEq,
                    | _ => Punct::DotDot,
                }
            },
            | ('-', Some(b'>')) => Punct::Arrow,
            | ('=', Some(b'>')) => Punct::FatArrow,
            | ('<', Some(b'-')) => Punct::LArrow,
            | ('|', Some(b'>')) => Punct::PipeGt,
            | ('<', Some(b'|')) => Punct::LtPipe,
            | (':', Some(b':')) => Punct::ColonColon,
            | ('=', Some(b'=')) => Punct::EqEq,
            | ('!', Some(b'=')) | ('≠', _) => Punct::NotEq,
            | ('<', Some(b'=')) | ('≤', _) => Punct::Le,
            | ('>', Some(b'=')) | ('≥', _) => Punct::Ge,
            | ('&', Some(b'&')) => Punct::AndAnd,
            | ('|', Some(b'|')) => Punct::OrOr,
            | ('(', _) => Punct::LParen,
            | (')', _) => Punct::RParen,
            | ('[', _) => Punct::LBracket,
            | (']', _) => Punct::RBracket,
            | ('{', _) => Punct::LBrace,
            | ('}', _) => Punct::RBrace,
            | (',', _) => Punct::Comma,
            | (';', _) => Punct::Semicolon,
            | (':', _) => Punct::Colon,
            | ('.', _) => Punct::Dot,
            | ('|', _) => Punct::Pipe,
            | ('=', _) => Punct::Equals,
            | ('+', _) => Punct::Plus,
            | ('-' | '−', _) => Punct::Minus,
            | ('*' | '·' | '×', _) => Punct::Star,
            | ('/' | '÷', _) => Punct::Slash,
            | ('^', _) => Punct::Caret,
            | ('@', _) => Punct::At,
            | ('%', _) => Punct::Percent,
            | ('<', _) => Punct::Lt,
            | ('>', _) => Punct::Gt,
            | ('!', _) => Punct::Bang,
            | ('?', _) => Punct::Question,
            | ('√', _) => Punct::Sqrt,
            | _ => {
                let start = self.mark();
                self.pos = self.pos.saturating_add(first.len_utf8());
                self.error(
                    "E0105",
                    format!("invalid character `{first}`"),
                    start,
                    self.mark(),
                );
                return;
            },
        };
        let start = self.mark();
        let width = if first.is_ascii() {
            punct.as_str().len()
        } else {
            first.len_utf8()
        };
        self.pos = self.pos.saturating_add(width);
        match punct {
            | Punct::LParen | Punct::LBracket | Punct::LBrace => {
                self.bracket_depth = self.bracket_depth.saturating_add(1);
            },
            | Punct::RParen | Punct::RBracket | Punct::RBrace => {
                self.bracket_depth = self.bracket_depth.saturating_sub(1);
            },
            | _ => {},
        }
        self.emit(TokenKind::Punct(punct), start);
    }
}

/// True for the characters that may appear in a superscript exponent.
const fn is_superscript(c: char) -> bool {
    matches!(
        c,
        '\u{2070}'
            ..='\u{2079}'
                | '\u{00B9}'
                | '\u{00B2}'
                | '\u{00B3}'
                | '\u{207B}'
                | '\u{207A}'
                | '\u{207D}'
                | '\u{207E}'
    )
}

/// Dedents the literal text while using the already-scanned source ranges
/// to splice in expression parts. Layout within an expression is ignored by
/// the inline lexer, so embedded source can retain its original whitespace.
#[allow(
    clippy::single_call_fn,
    reason = "keeps multiline text normalization separate from delimiter scanning"
)]
fn multiline_parts(
    raw: &str,
    span: Span,
    interpolations: Vec<(std::ops::Range<usize>, StringPart)>,
    diagnostics: &mut Diagnostics,
) -> Vec<StringPart> {
    let (text, lines) = dedent_multiline(raw, !interpolations.is_empty());
    let dedented_offset = |offset: usize| {
        let line = lines.partition_point(|&(raw_start, _)| raw_start <= offset);
        lines
            .get(line.saturating_sub(1))
            .map_or(0, |&(raw_start, text_start)| {
                text_start.saturating_add(offset.saturating_sub(raw_start))
            })
    };
    let mut parts = Vec::new();
    let mut append_text = |literal: &str, parts: &mut Vec<StringPart>| {
        if !literal.is_empty() {
            let decoded = decode_escapes(literal, |escape| {
                diagnostics.push(Diagnostic::error(
                    "E0105",
                    format!("unknown escape `{escape}`"),
                    span,
                ));
            });
            parts.push(StringPart::Text(decoded));
        }
    };
    let mut previous = 0;
    for (range, part) in interpolations {
        let open = dedented_offset(range.start);
        let after = dedented_offset(range.end);
        append_text(text.get(previous..open).unwrap_or_default(), &mut parts);
        parts.push(part);
        previous = after;
    }
    append_text(text.get(previous..).unwrap_or_default(), &mut parts);
    parts
}

/// Decodes the escapes of §1.7 (`\n`, `\t`, `\r`, `\0`, `\\`, `\"`, `\{`,
/// `\}`, `\u{...}`). An unknown escape is reported through `on_error` and its
/// character is kept, so no source text is lost.
#[allow(
    clippy::single_call_fn,
    reason = "keeps escape decoding separate from interpolation splitting"
)]
fn decode_escapes(
    text: &str,
    mut on_error: impl FnMut(&str),
) -> String {
    let mut out = String::with_capacity(text.len());
    let mut chars = text.chars().peekable();
    while let Some(ch) = chars.next() {
        if ch != '\\' {
            out.push(ch);
            continue;
        }
        let Some(next) = chars.next() else {
            out.push('\\');
            break;
        };
        match next {
            | 'n' => out.push('\n'),
            | 't' => out.push('\t'),
            | 'r' => out.push('\r'),
            | '0' => out.push('\0'),
            | '\\' => out.push('\\'),
            | '"' => out.push('"'),
            | '{' => out.push('{'),
            | '}' => out.push('}'),
            | 'u' => {
                let mut hex = String::new();
                if chars.peek() == Some(&'{') {
                    chars.next();
                    for inner in chars.by_ref() {
                        if inner == '}' {
                            break;
                        }
                        hex.push(inner);
                    }
                }
                match u32::from_str_radix(&hex, 16).ok().and_then(char::from_u32) {
                    | Some(decoded) => out.push(decoded),
                    | None => on_error(&format!("\\u{{{hex}}}")),
                }
            },
            | other => {
                on_error(&format!("\\{other}"));
                out.push(other);
            },
        }
    }
    out
}

/// Removes the first line ending and common indentation, returning the
/// original and dedented start of each line. Only the literal's text uses
/// normalized newlines; the expression ranges remain tied to the input.
#[allow(clippy::single_call_fn, reason = "isolates indentation normalization")]
fn dedent_multiline(
    raw: &str,
    track_offsets: bool,
) -> (String, Vec<(usize, usize)>) {
    let without_leading = raw
        .strip_prefix("\r\n")
        .or_else(|| raw.strip_prefix(['\r', '\n']));
    let body = without_leading.unwrap_or(raw);
    let leading = raw.len().saturating_sub(body.len());
    let common = physical_lines(body)
        .map(|(_, line)| line)
        .filter(|line| !line.trim().is_empty())
        .map(|line| {
            line.len()
                .saturating_sub(line.trim_start_matches([' ', '\t']).len())
        })
        .min()
        .unwrap_or(0);
    let mut text = String::new();
    let mut starts = Vec::new();
    for (index, (start, line)) in physical_lines(body).enumerate() {
        if index > 0 {
            text.push('\n');
        }
        if !line.trim().is_empty() {
            if track_offsets {
                starts.push((
                    leading.saturating_add(start).saturating_add(common),
                    text.len(),
                ));
            }
            text.push_str(line.get(common..).unwrap_or_default());
        }
    }
    (text, starts)
}

fn physical_lines(raw: &str) -> impl Iterator<Item = (usize, &str)> {
    let mut remaining = raw;
    std::iter::from_fn(move || {
        if remaining.is_empty() {
            return None;
        }
        let start = raw.len().saturating_sub(remaining.len());
        let end = remaining.find(['\r', '\n']).unwrap_or(remaining.len());
        let (line, rest) = remaining.split_at(end);
        remaining = rest
            .strip_prefix("\r\n")
            .or_else(|| rest.strip_prefix(['\r', '\n']))
            .unwrap_or(rest);
        Some((start, line))
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::token::TokenKind;

    #[test]
    fn punctuation_keeps_longest_matches_and_original_widths() {
        let source =
            "... ..= .. . => == = -> - <- <= <| < |> || | >= > :: : && != ! · × ÷ − ≤ ≥ ≠ √";
        let lexed = lex_inline(1, source, 0);
        let punct: Vec<_> = lexed
            .tokens
            .iter()
            .filter_map(|token| {
                match token.kind {
                    | TokenKind::Punct(punct) => {
                        let written = &source[token.span.start as usize..token.span.end as usize];
                        Some((written, punct.as_str()))
                    },
                    | _ => None,
                }
            })
            .collect();
        let expected: Vec<_> = source
            .split_whitespace()
            .map(|written| {
                let canonical = match written {
                    | "·" | "×" => "*",
                    | "÷" => "/",
                    | "−" => "-",
                    | "≤" => "<=",
                    | "≥" => ">=",
                    | "≠" => "!=",
                    | _ => written,
                };
                (written, canonical)
            })
            .collect();
        assert_eq!(punct, expected);
    }

    #[test]
    fn unmatched_inline_closers_do_not_turn_layout_back_on() {
        let lexed = lex_inline(7, ")\r\n\t42", 10);
        assert!(matches!(
            &lexed.tokens[..],
            [
                Token {
                    kind: TokenKind::Punct(Punct::RParen),
                    ..
                },
                Token {
                    kind: TokenKind::Int(_),
                    ..
                },
                Token {
                    kind: TokenKind::Eof,
                    ..
                },
            ]
        ));
    }

    #[test]
    fn inline_lexing_keeps_local_text_at_saturating_offsets() {
        let output = lex_inline(7, "x + 1", usize::MAX);
        assert!(
            matches!(output.tokens.first().map(|token| &token.kind), Some(TokenKind::Ident(name)) if name == "x")
        );
        assert!(output.tokens.iter().all(|token| {
            token.span
                == Span {
                    file: 7,
                    start: u32::MAX,
                    end: u32::MAX,
                }
        }));
        let output = lex_inline(7, "\"", usize::MAX);
        assert!(matches!(
            output.tokens.as_slice(),
            [Token {
                kind: TokenKind::Eof,
                ..
            }]
        ));
    }

    fn kinds(source: &str) -> Vec<TokenKind> {
        let output = lex(0, source).expect("lexes");
        output
            .tokens
            .into_iter()
            .map(|token| token.kind)
            .filter(|kind| !matches!(kind, TokenKind::Eof))
            .collect()
    }

    fn layout(source: &str) -> Vec<String> {
        kinds(source)
            .into_iter()
            .map(|kind| {
                match kind {
                    | TokenKind::Newline => "NL".to_string(),
                    | TokenKind::Indent => "INDENT".to_string(),
                    | TokenKind::Dedent => "DEDENT".to_string(),
                    | other => other.describe(),
                }
            })
            .collect()
    }

    #[test]
    fn layout_tracks_blocks() {
        let tokens = layout("fn f(x) =\n    let y = 1\n    y\n");
        assert_eq!(
            tokens,
            vec![
                "`fn`",
                "identifier `f`",
                "`(`",
                "identifier `x`",
                "`)`",
                "`=`",
                "NL",
                "INDENT",
                "`let`",
                "identifier `y`",
                "`=`",
                "`1`",
                "NL",
                "identifier `y`",
                "NL",
                "DEDENT",
            ]
        );
    }

    #[test]
    fn brackets_suppress_layout() {
        let tokens = layout("let a = [\n  1,\n  2,\n]\n");
        assert!(!tokens.contains(&"INDENT".to_string()));
        assert!(!tokens.contains(&"DEDENT".to_string()));
    }

    #[test]
    fn blank_and_comment_lines_do_not_change_layout() {
        let a = layout("fn f() =\n    1\n");
        let b = layout("fn f() =\n\n    # comment\n    1\n");
        assert_eq!(a, b);
    }

    #[test]
    fn tabs_in_indentation_are_reported() {
        let error = lex(0, "fn f() =\n\t1\n").expect_err("tab");
        assert_eq!(error.items[0].code(), "E0101");
    }

    #[test]
    fn numbers_are_exact() {
        let tokens = kinds("1_000 0xff 0b1010 0o17 1.5 2e3 1e-3");
        assert!(matches!(&tokens[0], TokenKind::Int(value) if value.to_string() == "1000"));
        assert!(matches!(&tokens[1], TokenKind::Int(value) if value.to_string() == "255"));
        assert!(matches!(&tokens[2], TokenKind::Int(value) if value.to_string() == "10"));
        assert!(matches!(&tokens[3], TokenKind::Int(value) if value.to_string() == "15"));
        assert!(matches!(&tokens[4], TokenKind::Float(value) if (*value - 1.5).abs() < 1e-12));
        assert!(matches!(&tokens[5], TokenKind::Float(value) if (*value - 2000.0).abs() < 1e-9));
        assert!(matches!(&tokens[6], TokenKind::Float(value) if (*value - 0.001).abs() < 1e-12));
    }

    #[test]
    fn strings_interpolate_and_escape() {
        let tokens = kinds("\"a{x + 1}b\\{c\\}\"");
        let TokenKind::Str(parts) = &tokens[0] else {
            panic!("expected string");
        };
        assert_eq!(parts[0], StringPart::Text("a".to_string()));
        assert!(
            matches!(&parts[1], StringPart::Interpolation { source, .. } if source.trim() == "x + 1")
        );
        assert_eq!(parts[2], StringPart::Text("b{c}".to_string()));
    }

    #[test]
    fn unicode_operators_are_normalised() {
        let tokens = kinds("a · b ÷ c ≤ d ≠ e − f");
        assert!(tokens.contains(&TokenKind::Punct(Punct::Star)));
        assert!(tokens.contains(&TokenKind::Punct(Punct::Slash)));
        assert!(tokens.contains(&TokenKind::Punct(Punct::Le)));
        assert!(tokens.contains(&TokenKind::Punct(Punct::NotEq)));
        assert!(tokens.contains(&TokenKind::Punct(Punct::Minus)));
    }

    #[test]
    fn degree_units_lex_as_identifiers() {
        let tokens = kinds("25 °C");
        assert_eq!(tokens[1], TokenKind::Ident("°C".to_string()));
    }

    #[test]
    fn identifiers_allow_unicode_and_primes() {
        let tokens = kinds("Δt f' x₁ λ");
        assert_eq!(tokens[0], TokenKind::Ident("Δt".to_string()));
        assert_eq!(tokens[1], TokenKind::Ident("f'".to_string()));
        assert_eq!(tokens[2], TokenKind::Ident("x₁".to_string()));
        assert_eq!(tokens[3], TokenKind::Ident("λ".to_string()));
    }

    #[test]
    fn doc_comments_are_collected() {
        let output = lex(0, "## The answer\nlet x = 42\n").unwrap();
        assert_eq!(output.docs.len(), 1);
        assert_eq!(output.docs[0].text, "The answer");
    }

    #[test]
    fn nested_block_comments_close() {
        let tokens = kinds("#- a #- b -# c -#\n1");
        assert!(matches!(tokens[0], TokenKind::Int(_)));
    }
}

#[cfg(test)]
mod regression_tests {
    use super::*;

    #[test]
    fn typographic_superscripts_are_recognised() {
        // U+00B2 is the `²` most keyboards produce.
        let output = lex(0, "3 m\u{00B2}").expect("lexes");
        assert!(output.tokens.iter().any(
            |token| matches!(&token.kind, TokenKind::Superscript(text) if text == "\u{00B2}")
        ));
        let output = lex(0, "2 s\u{207B}\u{00B9}").expect("lexes");
        assert!(output.tokens.iter().any(
            |token| matches!(&token.kind, TokenKind::Superscript(text) if text == "\u{207B}\u{00B9}")
        ));
        let output = lex(0, "4 m\u{207D}\u{207B}\u{00B2}\u{207E}").expect("lexes");
        assert!(output.tokens.iter().any(
            |token| matches!(&token.kind, TokenKind::Superscript(text) if text == "\u{207D}\u{207B}\u{00B2}\u{207E}")
        ));
    }

    #[test]
    fn carriage_return_is_a_line_terminator() {
        let tokens = lex(0, "fn f() =\r    let x = 1\r    x\r").expect("lexes");
        assert!(
            tokens
                .tokens
                .iter()
                .any(|token| matches!(token.kind, TokenKind::Indent))
        );
        assert!(
            tokens
                .tokens
                .iter()
                .any(|token| matches!(token.kind, TokenKind::Dedent))
        );
    }

    #[test]
    fn a_multiline_comment_does_not_indent_the_next_line() {
        let tokens = lex(0, "#- c\n-# let b = 2\n").expect("lexes");
        assert!(
            !tokens
                .tokens
                .iter()
                .any(|token| matches!(token.kind, TokenKind::Indent))
        );
    }

    #[test]
    fn an_unmatched_indent_is_not_pushed() {
        // §1.4.1: the level must match one on the stack. Recovery must report
        // `E0102` and leave the stack alone, so the following line at the bogus
        // level is still seen as a fresh indent rather than as well-formed.
        let source = "fn f() =\n      let x = 1\n    let y = 2\n    let z = 3\n";
        let mut lexer = Lexer::new(0, source, 0);
        lexer.scan();
        lexer.finish();
        assert!(lexer.diagnostics.has_errors());
        assert_eq!(lexer.diagnostics.items[0].code(), "E0102");
        let indents = lexer
            .tokens
            .iter()
            .filter(|token| matches!(token.kind, TokenKind::Indent))
            .count();
        assert_eq!(indents, 2, "the unmatched level was adopted");
    }

    #[test]
    fn the_last_line_gets_a_newline_without_a_trailing_newline() {
        let mut lexer = Lexer::new(0, "let x = 1", 0);
        lexer.scan();
        lexer.finish();
        let kinds: Vec<&TokenKind> = lexer.tokens.iter().map(|token| &token.kind).collect();
        assert!(
            matches!(kinds[kinds.len() - 2], TokenKind::Newline),
            "{kinds:?}"
        );
    }
}
