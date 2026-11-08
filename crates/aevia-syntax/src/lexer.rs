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
                    Span::new(file, base, base + text.len()),
                )],
                docs: Vec::new(),
            }
        })
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
    at_line_start: bool,
    line_has_token: bool,
    diagnostics: Diagnostics,
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
            indent_stack: vec![0],
            bracket_depth: 0,
            at_line_start: true,
            line_has_token: false,
            diagnostics: Diagnostics::new(),
        }
    }

    fn run(mut self) -> Result<LexOutput, Diagnostics> {
        // A UTF-8 BOM is allowed and ignored (§1.1).
        if self.src.starts_with('\u{feff}') {
            self.pos += '\u{feff}'.len_utf8();
        }
        // A shebang line is ignored.
        if self.src[self.pos..].starts_with("#!") {
            while let Some(c) = self.peek() {
                if c == '\n' {
                    break;
                }
                self.bump();
            }
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
        // Layout inside an interpolation is not meaningful: the expression is
        // bounded by braces, so treat it as already inside a bracket.
        self.bracket_depth = 1;
        self.at_line_start = false;
        self.scan();
        // Drop layout tokens the scanner may still have produced.
        self.tokens
            .retain(|token| !matches!(token.kind, TokenKind::Indent | TokenKind::Dedent));
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
            if self.at_line_start && self.bracket_depth == 0 {
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
                | 'r' if self.peek_at(1) == Some('"') => {
                    self.bump();
                    self.raw_string();
                },
                | c if c.is_ascii_digit() => self.number(),
                | c if is_xid_start(c) || c == '_' => self.identifier(),
                | '°' => self.degree_unit(),
                | c if is_superscript(c) => self.superscript(),
                | _ => self.punct(),
            }
        }
    }

    fn finish(&mut self) {
        // A final line without a trailing newline still ends a logical line
        // (§1.4.1: every logical line ends with `NEWLINE`). The token is
        // zero-width at end of file, so positions are unaffected.
        if self.line_has_token && self.bracket_depth == 0 {
            let at = self.mark();
            self.push(TokenKind::Newline, at, at);
            self.line_has_token = false;
        }
        while self.indent_stack.len() > 1 {
            self.indent_stack.pop();
            let at = self.mark();
            self.push(TokenKind::Dedent, at, at);
        }
        let at = self.mark();
        self.push(TokenKind::Eof, at, at);
    }

    fn mark(&self) -> usize {
        self.base + self.pos
    }

    fn peek(&self) -> Option<char> {
        self.src[self.pos..].chars().next()
    }

    fn peek_at(
        &self,
        offset: usize,
    ) -> Option<char> {
        self.src[self.pos..].chars().nth(offset)
    }

    fn bump(&mut self) -> Option<char> {
        let c = self.peek()?;
        self.pos += c.len_utf8();
        Some(c)
    }

    fn skip_spaces(&mut self) {
        while let Some(c) = self.peek() {
            if c == ' ' || c == '\t' {
                self.pos += c.len_utf8();
            } else {
                break;
            }
        }
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
        self.tokens
            .push(Token::new(kind, Span::new(self.file, start, end)));
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
        let span = Span::new(self.file, start, end);
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
        if self.bracket_depth == 0 {
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
                        indent += 1;
                    },
                    | Some('\t') => {
                        let start = self.mark();
                        self.bump();
                        indent += 1;
                        self.error("E0101", "tab character in indentation", start, self.mark());
                        // Keep measuring so the rest of the line still parses.
                        while let Some(' ') | Some('\t') = self.peek() {
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
                            indent += 1;
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
                | Some('\n') => {
                    self.bump();
                    continue;
                },
                | Some('\r') => {
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
                        if matches!(self.peek(), Some('\n') | Some('\r') | None) {
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
                        | Some('\n') => {
                            self.bump();
                            continue;
                        },
                        | Some('\r') => {
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
            while self.indent_stack.len() > 1 && *self.indent_stack.last().unwrap_or(&0) > indent {
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
                    at + indent.saturating_sub(*self.indent_stack.last().unwrap_or(&0)),
                );
            }
        }
    }

    /// Skips a comment. Returns true when it consumed a newline, which means
    /// the following text starts a new physical line.
    fn comment(&mut self) -> bool {
        let start = self.mark();
        if self.src[self.pos..].starts_with("#-") {
            self.pos += 2;
            let mut depth = 1usize;
            let mut newline = false;
            while self.pos < self.src.len() {
                if self.src[self.pos..].starts_with("#-") {
                    self.pos += 2;
                    depth += 1;
                } else if self.src[self.pos..].starts_with("-#") {
                    self.pos += 2;
                    depth -= 1;
                    if depth == 0 {
                        break;
                    }
                } else {
                    if self.peek() == Some('\n') {
                        newline = true;
                    }
                    self.bump();
                }
            }
            if depth != 0 {
                self.error("E0104", "unclosed block comment", start, self.mark());
            }
            return newline;
        }
        if self.src[self.pos..].starts_with("##") {
            self.pos += 2;
            let text_start = self.pos;
            while let Some(c) = self.peek() {
                if c == '\n' {
                    break;
                }
                self.bump();
            }
            let text = self.src[text_start..self.pos].trim().to_string();
            self.docs.push(DocComment {
                span: Span::new(self.file, start, self.mark()),
                text,
            });
            return false;
        }
        while let Some(c) = self.peek() {
            if c == '\n' {
                break;
            }
            self.bump();
        }
        false
    }

    fn identifier(&mut self) {
        let start = self.mark();
        self.bump();
        while let Some(c) = self.peek() {
            if is_xid_continue(c) || c == '\'' || ('₀'..='₉').contains(&c) {
                self.bump();
            } else {
                break;
            }
        }
        let raw = &self.src[start - self.base..self.pos];
        // §1.5.1: identifiers are compared in NFC, so `é` written as `e` plus a
        // combining accent is the same name as the precomposed one.
        let normalised: String = raw.nfc().collect();
        let text: &str = if normalised == raw {
            raw
        } else {
            &normalised
        };
        let kind = match Keyword::from_word(text) {
            | Some(keyword) => TokenKind::Keyword(keyword),
            | None => {
                // Only ASCII capitals introduce a `UIDENT`: the spec's own
                // example `Δt` is a value binding, not a constructor (§1.5.1).
                let first = text.chars().next().unwrap_or('_');
                if first.is_ascii_uppercase() {
                    TokenKind::UpperIdent(text.to_string())
                } else {
                    TokenKind::Ident(text.to_string())
                }
            },
        };
        self.emit(kind, start);
    }

    fn degree_unit(&mut self) {
        let start = self.mark();
        self.bump();
        if let Some(next) = self.peek() {
            if next == 'C' || next == 'F' {
                self.bump();
            }
        }
        let text = self.src[start - self.base..self.pos].to_string();
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
        let text = self.src[start - self.base..self.pos].to_string();
        self.emit(TokenKind::Superscript(text), start);
    }

    fn number(&mut self) {
        let start = self.mark();
        if self.peek() == Some('0') {
            if let Some(radix_char) = self.peek_at(1) {
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
                    if let Some(next) = self.peek() {
                        if next.is_alphanumeric() {
                            let at = self.mark();
                            self.bump();
                            self.error(
                                "E0106",
                                format!("`{next}` is not a digit of this base"),
                                at,
                                self.mark(),
                            );
                        }
                    }
                    match BigInt::from_str_radix(&cleaned, radix) {
                        | Ok(value) => self.emit(TokenKind::Int(value), start),
                        | Err(_) => {
                            self.error("E0106", "invalid numeric literal", start, self.mark())
                        },
                    }
                    return;
                }
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
        if matches!(self.peek(), Some('e') | Some('E')) {
            let mut lookahead = 1;
            if matches!(self.peek_at(lookahead), Some('+') | Some('-')) {
                lookahead += 1;
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
                    )
                },
            }
        } else {
            match BigInt::from_str_radix(&cleaned, 10) {
                | Ok(value) => self.emit(TokenKind::Int(value), start),
                | Err(_) => self.error("E0106", "invalid integer literal", start, self.mark()),
            }
        }
    }

    fn string(&mut self) {
        let start = self.mark();
        if self.src[self.pos..].starts_with("\"\"\"") {
            self.multiline_string(start);
            return;
        }
        self.pos += 1;
        let mut parts: Vec<StringPart> = Vec::new();
        let mut text = String::new();
        loop {
            match self.peek() {
                | None => {
                    self.error("E0104", "unclosed string literal", start, self.mark());
                    break;
                },
                | Some('"') => {
                    self.bump();
                    break;
                },
                | Some('\n') => {
                    self.error("E0104", "unclosed string literal", start, self.mark());
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
                            let hex = self.src[hex_start..self.pos].to_string();
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
                                    )
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
                        | None => break,
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
        let mut depth = 1usize;
        while let Some(c) = self.peek() {
            match c {
                | '{' => {
                    depth += 1;
                    self.bump();
                },
                | '}' => {
                    depth -= 1;
                    if depth == 0 {
                        break;
                    }
                    self.bump();
                },
                | '\\' => {
                    // An escape belongs to a nested string literal: consume the
                    // backslash and its character together, so `\"` does not
                    // look like the start of a string.
                    self.bump();
                    if self.peek().is_some() {
                        self.bump();
                    }
                },
                | '"' => {
                    // Skip a nested string so braces inside it do not count.
                    self.bump();
                    while let Some(inner) = self.peek() {
                        if inner == '\\' {
                            self.bump();
                            self.bump();
                        } else if inner == '"' {
                            self.bump();
                            break;
                        } else {
                            self.bump();
                        }
                    }
                },
                | _ => {
                    self.bump();
                },
            }
        }
        if self.peek() != Some('}') {
            self.error(
                "E0104",
                "unclosed interpolation `{...}` in string",
                open,
                self.mark(),
            );
            return None;
        }
        let source = unescape_source(&self.src[content_start..self.pos]);
        let span = Span::new(self.file, self.base + content_start, self.base + self.pos);
        self.bump(); // '}'
        // `"{}"` is a placeholder in other languages; here it is an
        // interpolation with nothing in it. Name the two ways out instead of
        // the parser's "expected an expression".
        if source.trim().is_empty() {
            let span = Span::new(self.file, open, self.mark());
            self.diagnostics.push(
                Diagnostic::error("E0104", "empty interpolation `{}` in a string", span).with_help(
                    "write an expression inside the braces, `\\{` for a literal brace, \
                     or a raw string r\"…\" (no interpolation) for a `format` template",
                ),
            );
            return Some(StringPart::Text(String::new()));
        }
        Some(StringPart::Interpolation { source, span })
    }

    fn raw_string(&mut self) {
        let start = self.mark();
        self.pos += 1; // opening quote
        let content_start = self.pos;
        loop {
            match self.peek() {
                | None | Some('\n') => {
                    self.error("E0104", "unclosed raw string literal", start, self.mark());
                    break;
                },
                | Some('"') => break,
                | Some(_) => {
                    self.bump();
                },
            }
        }
        let text = self.src[content_start..self.pos].to_string();
        if self.peek() == Some('"') {
            self.bump();
        }
        self.emit(TokenKind::Str(vec![StringPart::Text(text)]), start);
    }

    fn multiline_string(
        &mut self,
        start: usize,
    ) {
        self.pos += 3;
        let content_start = self.pos;
        let mut end = None;
        while self.pos < self.src.len() {
            if self.src[self.pos..].starts_with("\"\"\"") {
                end = Some(self.pos);
                self.pos += 3;
                break;
            }
            self.bump();
        }
        let Some(end) = end else {
            self.error(
                "E0104",
                "unclosed multi-line string literal",
                start,
                self.mark(),
            );
            return;
        };
        let raw = &self.src[content_start..end];
        let text = dedent_multiline(raw);
        let span = Span::new(self.file, start, end);
        let parts = split_interpolations(&text, span, &mut self.diagnostics);
        self.emit(TokenKind::Str(parts), start);
    }

    fn punct(&mut self) {
        let start = self.mark();
        const CANDIDATES: &[(&str, Punct)] = &[
            ("...", Punct::Ellipsis),
            ("..=", Punct::DotDotEq),
            ("..", Punct::DotDot),
            ("->", Punct::Arrow),
            ("=>", Punct::FatArrow),
            ("<-", Punct::LArrow),
            ("|>", Punct::PipeGt),
            ("<|", Punct::LtPipe),
            ("::", Punct::ColonColon),
            ("==", Punct::EqEq),
            ("!=", Punct::NotEq),
            ("<=", Punct::Le),
            (">=", Punct::Ge),
            ("&&", Punct::AndAnd),
            ("||", Punct::OrOr),
            ("(", Punct::LParen),
            (")", Punct::RParen),
            ("[", Punct::LBracket),
            ("]", Punct::RBracket),
            ("{", Punct::LBrace),
            ("}", Punct::RBrace),
            (",", Punct::Comma),
            (";", Punct::Semicolon),
            (":", Punct::Colon),
            (".", Punct::Dot),
            ("|", Punct::Pipe),
            ("=", Punct::Equals),
            ("+", Punct::Plus),
            ("-", Punct::Minus),
            ("*", Punct::Star),
            ("/", Punct::Slash),
            ("^", Punct::Caret),
            ("@", Punct::At),
            ("%", Punct::Percent),
            ("<", Punct::Lt),
            (">", Punct::Gt),
            ("!", Punct::Bang),
            ("?", Punct::Question),
            ("√", Punct::Sqrt),
            // Unicode spellings normalised to their ASCII meaning (§1.9).
            ("·", Punct::Star),
            ("×", Punct::Star),
            ("÷", Punct::Slash),
            ("−", Punct::Minus),
            ("≤", Punct::Le),
            ("≥", Punct::Ge),
            ("≠", Punct::NotEq),
        ];
        for (text, punct) in CANDIDATES {
            if self.src[self.pos..].starts_with(text) {
                self.pos += text.len();
                match punct {
                    | Punct::LParen | Punct::LBracket | Punct::LBrace => {
                        self.bracket_depth += 1;
                    },
                    | Punct::RParen | Punct::RBracket | Punct::RBrace => {
                        self.bracket_depth = self.bracket_depth.saturating_sub(1);
                    },
                    | _ => {},
                }
                self.emit(TokenKind::Punct(*punct), start);
                return;
            }
        }
        let c = self.bump().unwrap_or('\0');
        self.error(
            "E0105",
            format!("invalid character `{c}`"),
            start,
            self.mark(),
        );
    }
}

/// True for the characters that may appear in a superscript exponent.
fn is_superscript(c: char) -> bool {
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

/// Removes the first newline and the common indentation of a `"""` literal.
/// Removes the escapes that only exist to protect the enclosing literal.
///
/// Inside `{...}` a nested string is written with plain quotes; a writer who
/// escapes them (`\"`) means the same thing, and the parser should not see the
/// backslash.
fn unescape_source(source: &str) -> String {
    let mut out = String::with_capacity(source.len());
    let mut chars = source.chars().peekable();
    while let Some(ch) = chars.next() {
        if ch == '\\' {
            match chars.peek() {
                | Some('"') | Some('{') | Some('}') => {
                    if let Some(escaped) = chars.next() {
                        out.push(escaped);
                    }
                },
                | _ => out.push('\\'),
            }
            continue;
        }
        out.push(ch);
    }
    out
}

/// Splits text into literal runs and interpolations.
///
/// `\{` is a literal brace, as in a single-line string. Literal runs keep the
/// escapes of §1.7 — they are decoded here exactly as in a `"..."` literal, so
/// `"""x\ty"""` contains a tab and not a `t`. The span is the literal's,
/// because the dedented text no longer lines up with the source.
fn split_interpolations(
    text: &str,
    span: Span,
    diagnostics: &mut Diagnostics,
) -> Vec<StringPart> {
    let mut parts = Vec::new();
    let mut literal = String::new();
    let mut chars = text.char_indices().peekable();
    let mut flush = |literal: &mut String, parts: &mut Vec<StringPart>| {
        if literal.is_empty() {
            return;
        }
        let raw = std::mem::take(literal);
        let mut bad: Vec<String> = Vec::new();
        let decoded = decode_escapes(&raw, |escape| bad.push(escape.to_string()));
        for escape in bad {
            diagnostics.push(Diagnostic::error(
                "E0105",
                format!("unknown escape `{escape}`"),
                span,
            ));
        }
        parts.push(StringPart::Text(decoded));
    };
    while let Some((_, ch)) = chars.next() {
        match ch {
            | '\\' => {
                // Keep the escape verbatim; the run is decoded when it is
                // flushed, so `\{` never starts an interpolation.
                literal.push('\\');
                match chars.next() {
                    | Some((_, 'u')) => {
                        literal.push('u');
                        if matches!(chars.peek(), Some((_, '{'))) {
                            for (_, inner) in chars.by_ref() {
                                literal.push(inner);
                                if inner == '}' {
                                    break;
                                }
                            }
                        }
                    },
                    | Some((_, escaped)) => literal.push(escaped),
                    | None => {},
                }
            },
            | '{' => {
                let start = chars.peek().map_or(text.len(), |(index, _)| *index);
                let mut depth = 1usize;
                let mut end = text.len();
                while let Some((index, inner)) = chars.next() {
                    match inner {
                        | '{' => depth += 1,
                        | '}' => {
                            depth -= 1;
                            if depth == 0 {
                                end = index;
                                break;
                            }
                        },
                        | '\\' => {
                            chars.next();
                        },
                        | '"' => {
                            // A nested string may contain braces.
                            for (_, quoted) in chars.by_ref() {
                                if quoted == '"' {
                                    break;
                                }
                            }
                        },
                        | _ => {},
                    }
                }
                flush(&mut literal, &mut parts);
                parts.push(StringPart::Interpolation {
                    source: unescape_source(&text[start..end]),
                    span,
                });
            },
            | other => literal.push(other),
        }
    }
    flush(&mut literal, &mut parts);
    parts
}

/// Decodes the escapes of §1.7 (`\n`, `\t`, `\r`, `\0`, `\\`, `\"`, `\{`,
/// `\}`, `\u{...}`). An unknown escape is reported through `on_error` and its
/// character is kept, so no source text is lost.
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

fn dedent_multiline(raw: &str) -> String {
    let raw = raw.strip_prefix('\n').unwrap_or(raw);
    let raw = raw.strip_prefix("\r\n").unwrap_or(raw);
    let lines: Vec<&str> = raw.lines().collect();
    let mut common: Option<usize> = None;
    for line in &lines {
        if line.trim().is_empty() {
            continue;
        }
        let indent = line.len() - line.trim_start_matches([' ', '\t']).len();
        common = Some(common.map_or(indent, |value: usize| value.min(indent)));
    }
    let common = common.unwrap_or(0);
    let mut out = String::new();
    for (index, line) in lines.iter().enumerate() {
        if index > 0 {
            out.push('\n');
        }
        if line.trim().is_empty() {
            continue;
        }
        out.push_str(&line[common.min(line.len())..]);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::token::TokenKind;

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
