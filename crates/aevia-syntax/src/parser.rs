//! Recursive-descent parser for Aevia.
//!
//! The grammar is appendix A. The parser is hand-written for two reasons the
//! spec makes explicit:
//!
//! * layout is part of the grammar (`INDENT`/`DEDENT` select blocks), and
//! * `f<T>(...)` versus `a < b` is decided by adjacency, not by a symbol table
//!   (§2.7.2).
//!
//! Errors are recovered per declaration: a bad declaration is dropped and
//! parsing continues at the next line, so one mistake does not hide the rest.

use crate::ast::*;
use crate::diagnostic::Diagnostic;
use crate::diagnostic::Diagnostics;
use crate::lexer::LexOutput;
use crate::lexer::lex_inline;
use crate::span::FileId;
use crate::span::Span;
use crate::token::DocComment;
use crate::token::Keyword;
use crate::token::Punct;
use crate::token::StringPart;
use crate::token::Token;
use crate::token::TokenKind;
use aevia_dim::Scale;
use aevia_dim::UnitExpr;
use num_bigint::BigInt;
use num_rational::BigRational;
use num_traits::ToPrimitive;
use num_traits::Zero;

/// A name or dotted name can receive explicit type arguments. This is a
/// syntactic check only; the checker resolves the path and callable identity.
fn is_named_type_callee(expr: &Expr) -> bool {
    match &expr.kind {
        | ExprKind::Var(_) | ExprKind::QName(_) => true,
        | ExprKind::Field { base, .. } => is_named_type_callee(base),
        | _ => false,
    }
}

/// Parses a lexed file into a module.
///
/// Always returns a tree; parse errors are reported in the second component.
#[must_use]
pub fn parse_module(
    file: FileId,
    source: &str,
    lexed: &LexOutput,
) -> (Module, Diagnostics) {
    let mut parser = Parser::new(&lexed.tokens, &lexed.docs, file, source);
    let module = parser.module();
    (module, parser.diagnostics)
}

/// Parses one expression from source text, used by the REPL and doctests.
///
/// # Errors
///
/// Returns the lexical and syntactic diagnostics when the input is malformed.
pub fn parse_expression(
    file: FileId,
    source: &str,
) -> Result<Expr, Diagnostics> {
    let lexed = crate::lexer::lex(file, source)?;
    let mut parser = Parser::new(&lexed.tokens, &lexed.docs, file, source);
    let expr = parser.expr();
    parser.skip_newlines();
    if !parser.at_eof() {
        parser.unexpected("end of input");
    }
    if parser.diagnostics.has_errors() {
        Err(parser.diagnostics)
    } else {
        Ok(expr)
    }
}

struct Parser<'a> {
    tokens: &'a [Token],
    docs: &'a [DocComment],
    doc_cursor: usize,
    file: FileId,
    source: &'a str,
    pos: usize,
    depth: usize,
    diagnostics: Diagnostics,
    /// Whether the pattern being parsed is in a binding position (`let`,
    /// parameters, `<-`), where a bare capitalised name binds a variable.
    binding_pattern: bool,
}

const MAX_DEPTH: usize = 96;

impl<'a> Parser<'a> {
    fn new(
        tokens: &'a [Token],
        docs: &'a [DocComment],
        file: FileId,
        source: &'a str,
    ) -> Self {
        Self {
            tokens,
            docs,
            doc_cursor: 0,
            file,
            source,
            pos: 0,
            depth: 0,
            diagnostics: Diagnostics::new(),
            binding_pattern: false,
        }
    }

    // ----- token helpers -------------------------------------------------

    fn token(&self) -> &Token {
        // The lexer always ends the stream with `Eof`, so the fallback is only
        // reachable if a caller slices tokens by hand.
        self.tokens
            .get(self.pos)
            .unwrap_or_else(|| &self.tokens[self.tokens.len() - 1])
    }

    fn kind(&self) -> &TokenKind {
        &self.token().kind
    }

    /// Whether the token `offset` places ahead can begin an operand.
    fn starts_expression_at(
        &self,
        offset: usize,
    ) -> bool {
        matches!(
            self.kind_at(offset),
            TokenKind::Ident(_)
                | TokenKind::UpperIdent(_)
                | TokenKind::Int(_)
                | TokenKind::Float(_)
                | TokenKind::Str(_)
                | TokenKind::Punct(Punct::LParen | Punct::LBracket | Punct::Minus)
        )
    }

    fn kind_at(
        &self,
        offset: usize,
    ) -> &TokenKind {
        self.tokens
            .get(self.pos + offset)
            .map_or(&TokenKind::Eof, |token| &token.kind)
    }

    fn span(&self) -> Span {
        self.token().span
    }

    fn text_of(
        &self,
        span: Span,
    ) -> &str {
        let start = usize::try_from(span.start).unwrap_or(0);
        let end = usize::try_from(span.end).unwrap_or(0);
        self.source.get(start..end).unwrap_or("")
    }

    fn at_eof(&self) -> bool {
        matches!(self.kind(), TokenKind::Eof)
    }

    fn at_newline(&self) -> bool {
        matches!(self.kind(), TokenKind::Newline)
    }

    fn at_indent(&self) -> bool {
        matches!(self.kind(), TokenKind::Indent)
    }

    fn at_dedent(&self) -> bool {
        matches!(self.kind(), TokenKind::Dedent)
    }

    fn at_punct(
        &self,
        punct: Punct,
    ) -> bool {
        matches!(self.kind(), TokenKind::Punct(p) if *p == punct)
    }

    fn punct_at(
        &self,
        offset: usize,
        punct: Punct,
    ) -> bool {
        matches!(self.kind_at(offset), TokenKind::Punct(p) if *p == punct)
    }

    fn at_kw(
        &self,
        keyword: Keyword,
    ) -> bool {
        matches!(self.kind(), TokenKind::Keyword(k) if *k == keyword)
    }

    fn at_ident(&self) -> bool {
        matches!(self.kind(), TokenKind::Ident(_))
    }

    fn ident_is(
        &self,
        name: &str,
    ) -> bool {
        matches!(self.kind(), TokenKind::Ident(text) if text == name)
    }

    fn bump(&mut self) -> Token {
        let token = self.token().clone();
        if !self.at_eof() {
            self.pos += 1;
        }
        token
    }

    fn eat_punct(
        &mut self,
        punct: Punct,
    ) -> bool {
        if self.at_punct(punct) {
            self.bump();
            true
        } else {
            false
        }
    }

    fn eat_kw(
        &mut self,
        keyword: Keyword,
    ) -> bool {
        if self.at_kw(keyword) {
            self.bump();
            true
        } else {
            false
        }
    }

    fn eat_newline(&mut self) -> bool {
        if self.at_newline() {
            self.bump();
            true
        } else {
            false
        }
    }

    fn skip_newlines(&mut self) {
        while self.at_newline() {
            self.bump();
        }
    }

    fn expect_punct(
        &mut self,
        punct: Punct,
    ) -> bool {
        if self.eat_punct(punct) {
            return true;
        }
        self.error_here(
            "E0202",
            format!("expected `{punct}`, found {}", self.describe()),
        );
        false
    }

    fn expect_kw(
        &mut self,
        keyword: Keyword,
    ) -> bool {
        if self.eat_kw(keyword) {
            return true;
        }
        self.error_here(
            "E0202",
            format!("expected `{keyword}`, found {}", self.describe()),
        );
        false
    }

    /// Requires the end of the logical line (§A.2: every declaration ends with
    /// `NEWLINE`, and the grammar's `NEWLINE?` only tolerates a line end that
    /// an inner block or layout group already consumed).
    fn expect_newline(&mut self) {
        if self.eat_newline() {
            return;
        }
        if self.at_dedent() || self.at_eof() {
            return;
        }
        if self.line_end_consumed() {
            return;
        }
        self.error_here(
            "E0202",
            format!("expected end of line, found {}", self.describe()),
        );
        // Skip the rest of the line: one mistake, one diagnostic, and the next
        // declaration starts from a known place.
        self.recover();
    }

    /// True when the construct just parsed ended with a block, which consumed
    /// the `NEWLINE` before its `DEDENT` — the grammar's `NEWLINE?`.
    fn line_end_consumed(&self) -> bool {
        self.pos > 0
            && matches!(
                self.tokens[self.pos - 1].kind,
                TokenKind::Newline | TokenKind::Dedent
            )
    }

    fn describe(&self) -> String {
        self.kind().describe()
    }

    fn error(
        &mut self,
        code: &str,
        message: impl Into<String>,
        span: Span,
    ) {
        self.diagnostics
            .push(Diagnostic::error(code, message, span));
    }

    fn error_here(
        &mut self,
        code: &str,
        message: impl Into<String>,
    ) {
        let span = self.span();
        self.error(code, message, span);
    }

    fn unexpected(
        &mut self,
        expected: &str,
    ) {
        let found = self.describe();
        self.error_here("E0202", format!("expected {expected}, found {found}"));
    }

    fn take_doc(
        &mut self,
        before: u32,
    ) -> Option<String> {
        let mut text: Option<String> = None;
        while let Some(doc) = self.docs.get(self.doc_cursor) {
            if doc.span.end <= before {
                text = Some(match text {
                    | Some(previous) => format!("{previous}\n{}", doc.text),
                    | None => doc.text.clone(),
                });
                self.doc_cursor += 1;
            } else {
                break;
            }
        }
        text
    }

    // ----- module and declarations --------------------------------------

    fn module(&mut self) -> Module {
        let file = self.file;
        let mut name = None;
        let mut unit_system = None;
        self.skip_newlines();
        if self.at_kw(Keyword::Module) {
            self.bump();
            name = Some(self.module_path());
            // §5.11: `module M with units natural` selects the unit system the
            // module's dimensions and unit literals are written in.
            if self.at_kw(Keyword::With) {
                self.bump();
                if self.ident_is("units") {
                    self.bump();
                    let span = self.span();
                    if let TokenKind::Ident(system) = self.kind().clone() {
                        self.bump();
                        unit_system = Some((system, span));
                    } else {
                        self.unexpected("a unit system name such as `natural`");
                    }
                } else {
                    self.unexpected("`units` (as in `module M with units natural`)");
                }
            }
            self.expect_newline();
            self.skip_newlines();
        }
        let mut decls = Vec::new();
        loop {
            self.skip_newlines();
            if self.at_eof() {
                break;
            }
            let before = self.pos;
            match self.top_decl() {
                | Some(decl) => decls.push(decl),
                | None => {
                    self.recover();
                },
            }
            if self.pos == before {
                // Guarantee progress even when a sub-parser bails out early.
                self.bump();
            }
        }
        Module {
            name,
            unit_system,
            decls,
            file,
        }
    }

    fn module_path(&mut self) -> Vec<String> {
        let mut path = Vec::new();
        match self.kind().clone() {
            | TokenKind::UpperIdent(name) => {
                self.bump();
                path.push(name);
            },
            | _ => {
                self.unexpected("a module name");
                return path;
            },
        }
        while self.at_punct(Punct::Dot) && matches!(self.kind_at(1), TokenKind::UpperIdent(_)) {
            self.bump();
            if let TokenKind::UpperIdent(name) = self.kind().clone() {
                self.bump();
                path.push(name);
            }
        }
        path
    }

    fn recover(&mut self) {
        // Skip to the end of the line, then let `module` skip the newline.
        while !self.at_eof() && !self.at_newline() && !self.at_dedent() {
            self.bump();
        }
        self.eat_newline();
    }

    fn top_decl(&mut self) -> Option<Decl> {
        let start_span = self.span();
        let doc = self.take_doc(start_span.start);
        let mut attrs = Vec::new();
        while self.at_punct(Punct::At) {
            attrs.push(self.attribute());
            self.skip_newlines();
        }
        for attr in &attrs {
            self.check_attribute(attr);
        }
        let private_span = self.span();
        let private = self.eat_kw(Keyword::Private);
        let kind = match self.kind().clone() {
            | TokenKind::Keyword(Keyword::Import) => self.import_decl(false)?,
            | TokenKind::Keyword(Keyword::Open) => self.import_decl(true)?,
            | TokenKind::Keyword(Keyword::Dimension) => self.dimension_decl()?,
            | TokenKind::Keyword(Keyword::Unit) => self.unit_decl()?,
            | TokenKind::Keyword(Keyword::Type) => self.type_decl()?,
            | TokenKind::Keyword(Keyword::Class) => self.class_decl()?,
            | TokenKind::Keyword(Keyword::Instance) => self.instance_decl()?,
            | TokenKind::Keyword(Keyword::Derive) => self.derive_decl()?,
            | TokenKind::Keyword(Keyword::Fn) => self.fn_decl()?,
            | TokenKind::Keyword(Keyword::Let) => self.let_decl()?,
            | TokenKind::Keyword(Keyword::Test) => self.test_decl()?,
            | _ => {
                self.unexpected("a declaration");
                return None;
            },
        };
        if private
            && !matches!(
                &kind,
                DeclKind::Type { .. }
                    | DeclKind::Dimension { .. }
                    | DeclKind::Unit { .. }
                    | DeclKind::Fn { .. }
                    | DeclKind::Let { .. }
                    | DeclKind::Class { .. }
            )
        {
            // §2.1: `private` marks a declaration as not exported, and only
            // types, dimensions, units, functions, values and classes are declarations in
            // that sense.
            self.error(
                "E0202",
                "`private` is only allowed on a `type`, `dimension`, `unit`, `fn`, `let` or `class` declaration",
                private_span,
            );
        }
        let span = start_span.join(self.previous_span());
        Some(Decl {
            attrs,
            private,
            doc,
            kind,
            span,
        })
    }

    /// Rejects an attribute the implementation does not know (§2.2.7).
    ///
    /// `@compile_fail` is deliberately *not* here: it cannot express "this
    /// source must fail to compile" without a source snippet, and the
    /// metaprogramming design replaces it with the `compile_fail("…")` test
    /// feature. Accepting it and doing nothing was worse than rejecting it, so
    /// it is an unknown attribute (`E0210`) until the standard says otherwise.
    fn check_attribute(
        &mut self,
        attr: &Attribute,
    ) {
        const KNOWN: &[&str] = &[
            "stable",
            "experimental",
            "deprecated",
            "jit",
            "op",
            "rule",
            // §9.12: a `bench` declaration is a function `aevia bench` times.
            "bench",
        ];
        if !KNOWN.contains(&attr.name.as_str()) {
            self.error(
                "E0210",
                format!("unknown attribute `@{}`", attr.name),
                attr.span,
            );
        }
    }

    fn previous_span(&self) -> Span {
        if self.pos == 0 {
            return self.span();
        }
        self.tokens[self.pos - 1].span
    }

    fn attribute(&mut self) -> Attribute {
        let start = self.span();
        self.bump(); // '@'
        let name = match self.kind().clone() {
            | TokenKind::Ident(name) | TokenKind::UpperIdent(name) => {
                self.bump();
                name
            },
            | _ => {
                self.unexpected("an attribute name");
                String::new()
            },
        };
        let mut args = Vec::new();
        if self.eat_punct(Punct::LParen) {
            while !self.at_punct(Punct::RParen) && !self.at_eof() {
                let arg = if self.at_ident() && self.punct_at(1, Punct::Equals) {
                    let name = if let TokenKind::Ident(name) = self.kind().clone() {
                        name
                    } else {
                        String::new()
                    };
                    self.bump();
                    self.bump();
                    AttrArg {
                        name: Some(name),
                        value: self.expr(),
                    }
                } else {
                    AttrArg {
                        name: None,
                        value: self.expr(),
                    }
                };
                args.push(arg);
                if !self.eat_punct(Punct::Comma) {
                    break;
                }
            }
            self.expect_punct(Punct::RParen);
        }
        // §A.2: `attribute ::= "@" IDENT [...] NEWLINE?` — the newline is
        // optional, so `@jit fn f() = 1` is one declaration.
        let span = start.join(self.previous_span());
        self.eat_newline();
        Attribute { name, args, span }
    }

    fn import_decl(
        &mut self,
        open: bool,
    ) -> Option<DeclKind> {
        self.bump(); // import / open
        let path = self.module_path();
        let mut alias = None;
        if self.eat_kw(Keyword::As) {
            if let TokenKind::UpperIdent(name) = self.kind().clone() {
                self.bump();
                alias = Some(name);
            } else {
                self.unexpected("a module alias");
            }
        }
        let mut names = None;
        if self.eat_punct(Punct::LBrace) {
            let mut list = Vec::new();
            while !self.at_punct(Punct::RBrace) && !self.at_eof() {
                match self.kind().clone() {
                    | TokenKind::Ident(name) | TokenKind::UpperIdent(name) => {
                        self.bump();
                        list.push(name);
                    },
                    | _ => {
                        self.unexpected("a name");
                        break;
                    },
                }
                if !self.eat_punct(Punct::Comma) {
                    break;
                }
            }
            self.expect_punct(Punct::RBrace);
            names = Some(list);
        }
        self.expect_newline();
        Some(DeclKind::Import {
            path,
            alias,
            names,
            open,
        })
    }

    fn dimension_decl(&mut self) -> Option<DeclKind> {
        self.bump();
        let name = match self.kind().clone() {
            | TokenKind::UpperIdent(name) => {
                self.bump();
                name
            },
            | _ => {
                self.unexpected("a dimension name");
                return None;
            },
        };
        let value = if self.eat_punct(Punct::Equals) {
            Some(self.dim())
        } else {
            None
        };
        self.expect_newline();
        Some(DeclKind::Dimension { name, value })
    }

    fn unit_decl(&mut self) -> Option<DeclKind> {
        self.bump();
        let name = self.unit_name()?;
        self.expect_punct(Punct::Colon);
        let dim = self.dim();
        self.expect_punct(Punct::Equals);
        let scale = self.unit_scale()?;
        let base = if self.starts_unit_atom()
            && !self.ident_is("offset")
            && !self.ident_is("prefixable")
        {
            self.unit_expr()
        } else {
            None
        };
        let mut offset = None;
        if self.ident_is("offset") {
            self.bump();
            let offset_scale = self.unit_scale()?;
            let offset_unit = if self.starts_unit_atom() {
                self.unit_expr()
            } else {
                None
            };
            offset = Some((offset_scale, offset_unit));
        }
        let prefixable = if self.ident_is("prefixable") {
            self.bump();
            true
        } else {
            false
        };
        self.expect_newline();
        Some(DeclKind::Unit {
            name,
            dim,
            scale,
            base,
            offset,
            prefixable,
        })
    }

    fn unit_name(&mut self) -> Option<String> {
        match self.kind().clone() {
            | TokenKind::Ident(name) | TokenKind::UpperIdent(name) => {
                self.bump();
                let mut path = name;
                while self.at_punct(Punct::Dot) {
                    self.bump();
                    match self.kind().clone() {
                        | TokenKind::Ident(part) | TokenKind::UpperIdent(part) => {
                            self.bump();
                            path.push('.');
                            path.push_str(&part);
                        },
                        | _ => {
                            self.unexpected("a unit name");
                            break;
                        },
                    }
                }
                Some(path)
            },
            | _ => {
                self.unexpected("a unit name");
                None
            },
        }
    }

    fn unit_scale(&mut self) -> Option<Scale> {
        let negative = self.eat_punct(Punct::Minus);
        if self.ident_is("pi") {
            self.bump();
            let mut scale = Scale {
                rat: BigRational::from_integer(BigInt::from(1)),
                pi: 1,
            };
            if self.eat_punct(Punct::Slash) {
                let denominator = self.number_text()?;
                let denominator = aevia_dim::parse_decimal(&denominator)?;
                scale.rat /= denominator;
            }
            if negative {
                scale.rat = -scale.rat;
            }
            return Some(scale);
        }
        let text = self.number_text()?;
        let mut scale = Scale::parse(&text)?;
        if self.eat_punct(Punct::Slash) {
            let denominator = self.number_text()?;
            let denominator = aevia_dim::parse_decimal(&denominator)?;
            if denominator.is_zero() {
                self.error_here("E0506", "unit scale divides by zero");
                return None;
            }
            scale.rat /= denominator;
        }
        if negative {
            scale.rat = -scale.rat;
        }
        Some(scale)
    }

    fn number_text(&mut self) -> Option<String> {
        match self.kind().clone() {
            | TokenKind::Int(value) => {
                self.bump();
                Some(value.to_string())
            },
            | TokenKind::Float(_) => {
                let span = self.span();
                self.bump();
                Some(self.text_of(span).replace('_', ""))
            },
            | _ => {
                self.unexpected("a number");
                None
            },
        }
    }

    fn type_decl(&mut self) -> Option<DeclKind> {
        self.bump();
        let name = match self.kind().clone() {
            | TokenKind::UpperIdent(name) => {
                self.bump();
                name
            },
            | _ => {
                self.unexpected("a type name");
                return None;
            },
        };
        let params = if self.at_punct(Punct::Lt) {
            self.type_params()
        } else {
            Vec::new()
        };
        let body = if self.eat_punct(Punct::Equals) {
            if self.at_punct(Punct::Pipe) {
                self.variants_inline()
            } else if self.at_newline() {
                if self.indent_follows() {
                    self.bump();
                    self.bump();
                    let variants = self.variants_block();
                    TypeBody::Variants(variants)
                } else {
                    // `type Foo =` with nothing after it (§A.2: `type_body`
                    // is a type, a variant list, or an indented variant list).
                    self.error_here("E0202", "expected a type or a variant block after `=`");
                    TypeBody::Abstract
                }
            } else {
                TypeBody::Alias(self.ty())
            }
        } else if self.at_newline() && self.indent_follows() {
            // §A.2: the indented variant list is a form of `type_body`, and
            // `type_body` is only reachable after `=`.
            self.error_here("E0202", "expected `=` before an indented variant block");
            self.bump();
            self.bump();
            TypeBody::Variants(self.variants_block())
        } else {
            TypeBody::Abstract
        };
        self.expect_newline();
        Some(DeclKind::Type { name, params, body })
    }

    fn eat_indent(&mut self) -> bool {
        if self.at_indent() {
            self.bump();
            true
        } else {
            false
        }
    }

    fn indent_follows(&self) -> bool {
        self.at_newline() && matches!(self.kind_at(1), TokenKind::Indent)
    }

    fn variants_inline(&mut self) -> TypeBody {
        let mut variants = Vec::new();
        while self.eat_punct(Punct::Pipe) {
            variants.push(self.constructor());
        }
        TypeBody::Variants(variants)
    }

    fn variants_block(&mut self) -> Vec<Constructor> {
        let mut variants = Vec::new();
        loop {
            self.skip_newlines();
            if self.at_dedent() || self.at_eof() {
                break;
            }
            if self.eat_punct(Punct::Pipe) {
                variants.push(self.constructor());
                self.expect_newline();
            } else {
                break;
            }
        }
        self.expect_dedent();
        variants
    }

    fn eat_dedent(&mut self) -> bool {
        if self.at_dedent() {
            self.bump();
            true
        } else {
            false
        }
    }

    /// Requires the `DEDENT` that closes an indented block.
    ///
    /// Failing here is a layout error, and — crucially — recovery must not
    /// consume a `DEDENT` that belongs to an enclosing block.
    fn expect_dedent(&mut self) {
        if self.eat_dedent() || self.at_eof() {
            return;
        }
        self.error_here(
            "E0202",
            format!("expected the end of the block, found {}", self.describe()),
        );
    }

    fn constructor(&mut self) -> Constructor {
        let start = self.span();
        let name = match self.kind().clone() {
            | TokenKind::UpperIdent(name) => {
                self.bump();
                name
            },
            | _ => {
                self.unexpected("a constructor name");
                String::new()
            },
        };
        let mut args = Vec::new();
        if self.eat_punct(Punct::LParen) {
            while !self.at_punct(Punct::RParen) && !self.at_eof() {
                args.push(self.ty());
                if !self.eat_punct(Punct::Comma) {
                    break;
                }
            }
            self.expect_punct(Punct::RParen);
        }
        Constructor {
            name,
            args,
            span: start.join(self.previous_span()),
        }
    }

    fn type_params(&mut self) -> Vec<TypeParam> {
        let mut params = Vec::new();
        if !self.expect_punct(Punct::Lt) {
            return params;
        }
        while !self.at_punct(Punct::Gt) && !self.at_eof() {
            let start = self.span();
            let name = match self.kind().clone() {
                | TokenKind::Ident(name) | TokenKind::UpperIdent(name) => {
                    self.bump();
                    name
                },
                | _ => {
                    self.unexpected("a type parameter");
                    break;
                },
            };
            let kind = if self.eat_punct(Punct::Colon) {
                match self.kind().clone() {
                    | TokenKind::UpperIdent(kind) => {
                        self.bump();
                        Kind::from_name(&kind)
                    },
                    | _ => {
                        self.unexpected("a kind");
                        None
                    },
                }
            } else {
                None
            };
            params.push(TypeParam {
                name,
                kind,
                span: start.join(self.previous_span()),
            });
            if !self.eat_punct(Punct::Comma) {
                break;
            }
        }
        self.expect_punct(Punct::Gt);
        params
    }

    fn class_decl(&mut self) -> Option<DeclKind> {
        self.bump();
        let name = match self.kind().clone() {
            | TokenKind::UpperIdent(name) => {
                self.bump();
                name
            },
            | _ => {
                self.unexpected("a class name");
                return None;
            },
        };
        let mut params = Vec::new();
        while let TokenKind::Ident(param) = self.kind().clone() {
            let start = self.span();
            self.bump();
            params.push(TypeParam {
                name: param,
                kind: None,
                span: start,
            });
        }
        self.expect_kw(Keyword::Where);
        let mut methods = Vec::new();
        if self.indent_follows() {
            self.bump();
            self.bump();
            loop {
                self.skip_newlines();
                if self.at_dedent() || self.at_eof() {
                    break;
                }
                let method_name = match self.kind().clone() {
                    | TokenKind::Ident(name) => {
                        self.bump();
                        name
                    },
                    | _ => break,
                };
                let ty = if self.at_punct(Punct::LParen) {
                    let args = self.param_list();
                    let ret = if self.eat_punct(Punct::Colon) {
                        Some(self.ty())
                    } else {
                        None
                    };
                    Ty::new(
                        TyKind::Fn(
                            args.into_iter().filter_map(|param| param.ty).collect(),
                            Box::new(ret.unwrap_or_else(|| Ty::new(TyKind::Unit, self.span()))),
                        ),
                        self.previous_span(),
                    )
                } else {
                    self.expect_punct(Punct::Colon);
                    self.ty()
                };
                methods.push((method_name, ty));
                self.expect_newline();
            }
            self.expect_dedent();
        } else {
            // §A.2: the method list is `NEWLINE INDENT ... DEDENT`.
            self.error_here("E0103", "expected an indented block after `where`");
        }
        self.expect_newline();
        Some(DeclKind::Class {
            name,
            params,
            methods,
        })
    }

    fn instance_decl(&mut self) -> Option<DeclKind> {
        self.bump();
        let class = match self.kind().clone() {
            | TokenKind::UpperIdent(name) => {
                self.bump();
                name
            },
            | _ => {
                self.unexpected("a class name");
                return None;
            },
        };
        self.expect_kw(Keyword::For);
        let ty = self.ty();
        let mut methods = Vec::new();
        if self.eat_kw(Keyword::Where) {
            if !self.indent_follows() {
                // §A.2: `where` is followed by an indented method block. Saying
                // so here also stops the block below from swallowing a `DEDENT`
                // that belongs to the enclosing declaration.
                self.error_here("E0103", "expected an indented block after `where`");
                self.expect_newline();
                return Some(DeclKind::Instance { class, ty, methods });
            }
            self.bump();
            self.bump();
            loop {
                self.skip_newlines();
                if self.at_dedent() || self.at_eof() {
                    break;
                }
                let start = self.span();
                let name = match self.kind().clone() {
                    | TokenKind::Ident(name) => {
                        self.bump();
                        name
                    },
                    | _ => break,
                };
                let params = self.param_list();
                let ret = if self.eat_punct(Punct::Colon) {
                    Some(self.ty())
                } else {
                    None
                };
                self.expect_punct(Punct::Equals);
                let body = self.expr();
                methods.push(InstanceMethod {
                    name,
                    params,
                    ret,
                    body,
                    span: start.join(self.previous_span()),
                });
                self.expect_newline();
            }
            self.expect_dedent();
        }
        self.expect_newline();
        Some(DeclKind::Instance { class, ty, methods })
    }

    fn derive_decl(&mut self) -> Option<DeclKind> {
        self.bump();
        let mut classes = Vec::new();
        loop {
            // §A.2: `derive name_list for type`, and `name_list` is
            // `IDENT | UIDENT`.
            match self.kind().clone() {
                | TokenKind::UpperIdent(name) | TokenKind::Ident(name) => {
                    self.bump();
                    classes.push(name);
                },
                | _ => {
                    self.unexpected("a class name");
                    break;
                },
            }
            if !self.eat_punct(Punct::Comma) {
                break;
            }
        }
        self.expect_kw(Keyword::For);
        let ty = self.ty();
        self.expect_newline();
        Some(DeclKind::Derive { classes, ty })
    }

    fn fn_decl(&mut self) -> Option<DeclKind> {
        self.bump();
        let name = match self.kind().clone() {
            // §A.2: `fn_decl ::= "fn" IDENT ...`. A capitalised name is a
            // constructor, never a function.
            | TokenKind::Ident(name) => {
                self.bump();
                name
            },
            | _ => {
                self.unexpected("a function name");
                return None;
            },
        };
        let params = if self.at_punct(Punct::Lt) {
            self.type_params()
        } else {
            Vec::new()
        };
        let args = self.param_list();
        let ret = if self.eat_punct(Punct::Colon) {
            Some(self.ty())
        } else {
            None
        };
        self.expect_punct(Punct::Equals);
        let body = self.expr();
        self.expect_newline();
        Some(DeclKind::Fn {
            name,
            params,
            args,
            ret,
            body,
        })
    }

    fn param_list(&mut self) -> Vec<Param> {
        let mut params = Vec::new();
        if !self.expect_punct(Punct::LParen) {
            return params;
        }
        while !self.at_punct(Punct::RParen) && !self.at_eof() {
            let start = self.span();
            let pattern = self.binding_pattern();
            let ty = if self.eat_punct(Punct::Colon) {
                Some(self.ty())
            } else {
                None
            };
            params.push(Param {
                pattern,
                ty,
                span: start.join(self.previous_span()),
            });
            if !self.eat_punct(Punct::Comma) {
                break;
            }
        }
        self.expect_punct(Punct::RParen);
        params
    }

    fn let_decl(&mut self) -> Option<DeclKind> {
        self.bump();
        let rec = self.eat_kw(Keyword::Rec);
        let pattern = self.binding_pattern();
        let ty = if self.eat_punct(Punct::Colon) {
            Some(self.ty())
        } else {
            None
        };
        self.expect_punct(Punct::Equals);
        let value = self.expr();
        self.expect_newline();
        Some(DeclKind::Let {
            pattern,
            ty,
            value,
            rec,
        })
    }

    fn test_decl(&mut self) -> Option<DeclKind> {
        self.bump();
        let name = match self.kind().clone() {
            | TokenKind::Str(parts) => {
                self.bump();
                parts
                    .into_iter()
                    .filter_map(|part| {
                        match part {
                            | StringPart::Text(text) => Some(text),
                            | StringPart::Interpolation { .. } => None,
                        }
                    })
                    .collect::<Vec<_>>()
                    .join("")
            },
            | _ => {
                self.unexpected("a test name");
                String::new()
            },
        };
        self.expect_punct(Punct::Equals);
        let body = self.expr();
        self.expect_newline();
        Some(DeclKind::Test { name, body })
    }

    // ----- expressions ---------------------------------------------------

    fn expr(&mut self) -> Expr {
        self.depth += 1;
        if self.depth > MAX_DEPTH {
            self.depth -= 1;
            self.error_here("E0202", "expression nesting is too deep");
            return Expr::new(ExprKind::Error, self.span());
        }
        let result = self.expr_inner();
        self.depth -= 1;
        result
    }

    fn expr_inner(&mut self) -> Expr {
        if self.at_newline() && matches!(self.kind_at(1), TokenKind::Indent) {
            return self.layout_block();
        }
        match self.kind().clone() {
            | TokenKind::Keyword(Keyword::Let) => self.let_expr(),
            | TokenKind::Keyword(Keyword::If) => self.if_expr(),
            | TokenKind::Keyword(Keyword::Match) => self.match_expr(),
            | TokenKind::Keyword(Keyword::Fn) => self.lambda_expr(),
            | TokenKind::Keyword(Keyword::Do) => self.do_expr(),
            | _ => self.annot_expr(),
        }
    }

    fn layout_block(&mut self) -> Expr {
        let start = self.span();
        self.bump(); // newline
        if !self.eat_indent() {
            self.error_here("E0103", "expected an indented block");
            return Expr::new(ExprKind::Error, start);
        }
        // §A.1: `block` is chosen when the first token starts a `block_stmt`;
        // otherwise the group is a single expression (`layout_group`). A `let`
        // that has its own `in` is a `let_expr`, not a statement, so it must
        // not select a block.
        let block = self.is_fn_decl_start() || (self.at_kw(Keyword::Let) && !self.let_has_in());
        if block {
            let mut stmts = Vec::new();
            loop {
                self.skip_newlines();
                if self.at_dedent() || self.at_eof() {
                    break;
                }
                if self.at_kw(Keyword::Let) {
                    stmts.push(self.block_let());
                } else if self.is_fn_decl_start() {
                    if let Some(decl) = self.fn_decl() {
                        let span = decl_span(&decl);
                        stmts.push(BlockStmt::Fn(Box::new(Decl {
                            attrs: Vec::new(),
                            private: false,
                            doc: None,
                            kind: decl,
                            span,
                        })));
                    }
                } else {
                    break;
                }
            }
            self.skip_newlines();
            let tail = if self.at_dedent() || self.at_eof() {
                None
            } else {
                let tail = self.expr();
                self.expect_newline();
                Some(Box::new(tail))
            };
            if tail.is_none() {
                // §2.3.1: a block must end with an expression.
                self.error("E0201", "empty block", start);
            }
            self.expect_dedent();
            Expr::new(
                ExprKind::Block { stmts, tail },
                start.join(self.previous_span()),
            )
        } else {
            let inner = self.expr();
            self.expect_newline();
            self.expect_dedent();
            inner
        }
    }

    /// True when the `let` at the cursor is a `let ... in` expression: its
    /// matching `in` occurs on the same logical line.
    fn let_has_in(&self) -> bool {
        let mut pending = 0usize;
        let mut offset = 1; // past `let`
        loop {
            match self.kind_at(offset) {
                | TokenKind::Newline | TokenKind::Dedent | TokenKind::Eof => return false,
                | TokenKind::Keyword(Keyword::Let) => pending += 1,
                | TokenKind::Keyword(Keyword::In) => {
                    if pending == 0 {
                        return true;
                    }
                    pending -= 1;
                },
                | _ => {},
            }
            offset += 1;
        }
    }

    fn is_fn_decl_start(&self) -> bool {
        self.at_kw(Keyword::Fn)
            && matches!(self.kind_at(1), TokenKind::Ident(_))
            && (self.punct_at(2, Punct::LParen) || self.punct_at(2, Punct::Lt))
    }

    fn block_let(&mut self) -> BlockStmt {
        let start = self.span();
        self.bump(); // let
        let rec = self.eat_kw(Keyword::Rec);
        let pattern = self.binding_pattern();
        let ty = if self.eat_punct(Punct::Colon) {
            Some(self.ty())
        } else {
            None
        };
        self.expect_punct(Punct::Equals);
        let value = self.expr();
        self.expect_newline();
        BlockStmt::Let {
            pattern,
            ty,
            value,
            rec,
            span: start.join(self.previous_span()),
        }
    }

    fn let_expr(&mut self) -> Expr {
        let start = self.span();
        self.bump();
        let pattern = self.binding_pattern();
        let ty = if self.eat_punct(Punct::Colon) {
            Some(self.ty())
        } else {
            None
        };
        self.expect_punct(Punct::Equals);
        let value = self.expr();
        if !self.expect_kw(Keyword::In) {
            return Expr::new(ExprKind::Error, start);
        }
        let body = self.expr();
        Expr::new(
            ExprKind::LetIn {
                pattern,
                ty,
                value: Box::new(value),
                body: Box::new(body),
            },
            start.join(self.previous_span()),
        )
    }

    fn if_expr(&mut self) -> Expr {
        let start = self.span();
        self.bump();
        let cond = self.expr();
        self.expect_kw(Keyword::Then);
        let then_branch = self.expr();
        self.skip_newlines();
        self.expect_kw(Keyword::Else);
        let else_branch = self.expr();
        Expr::new(
            ExprKind::If {
                cond: Box::new(cond),
                then_branch: Box::new(then_branch),
                else_branch: Box::new(else_branch),
            },
            start.join(self.previous_span()),
        )
    }

    fn match_expr(&mut self) -> Expr {
        let start = self.span();
        self.bump();
        let scrutinee = self.expr();
        self.expect_kw(Keyword::With);
        // §A.3: `match_body` is `NEWLINE INDENT ... DEDENT`, `NEWLINE ...`, or
        // the inline form `with | p -> e | q -> f` whose arms share the `with`
        // line. The inline form is the only one that can be written inside
        // brackets, where layout produces no `NEWLINE` (§1.4): without it a
        // `match` could not be an argument, `println(match o with ...)`.
        if self.at_punct(Punct::Pipe) {
            let mut arms = Vec::new();
            while self.at_punct(Punct::Pipe) {
                arms.push(self.match_arm_inline());
            }
            return Expr::new(
                ExprKind::Match {
                    scrutinee: Box::new(scrutinee),
                    arms,
                },
                start.join(self.previous_span()),
            );
        }
        let indented = if self.indent_follows() {
            self.bump();
            self.bump();
            true
        } else if self.at_newline() {
            self.bump();
            false
        } else {
            self.error_here("E0202", "expected end of line or `|` after `with`");
            false
        };
        let mut arms = Vec::new();
        loop {
            self.skip_newlines();
            if self.at_dedent() || self.at_eof() {
                break;
            }
            if !self.at_punct(Punct::Pipe) {
                break;
            }
            arms.push(self.match_arm());
        }
        if indented {
            self.expect_dedent();
        }
        Expr::new(
            ExprKind::Match {
                scrutinee: Box::new(scrutinee),
                arms,
            },
            start.join(self.previous_span()),
        )
    }

    /// One arm of the inline form: the arm ends where its body does, and the
    /// next `|` (or whatever follows the `match`) is not consumed.
    fn match_arm_inline(&mut self) -> MatchArm {
        let start = self.span();
        self.bump(); // '|'
        let pattern = self.pattern();
        let guard = if self.eat_kw(Keyword::If) {
            Some(self.expr())
        } else {
            None
        };
        self.expect_punct(Punct::Arrow);
        let body = self.expr();
        MatchArm {
            pattern,
            guard,
            body,
            span: start.join(self.previous_span()),
        }
    }

    fn match_arm(&mut self) -> MatchArm {
        let start = self.span();
        self.bump(); // '|'
        let pattern = self.pattern();
        let guard = if self.eat_kw(Keyword::If) {
            Some(self.expr())
        } else {
            None
        };
        self.expect_punct(Punct::Arrow);
        let body = self.expr();
        self.expect_newline();
        MatchArm {
            pattern,
            guard,
            body,
            span: start.join(self.previous_span()),
        }
    }

    fn lambda_expr(&mut self) -> Expr {
        let start = self.span();
        self.bump(); // fn
        let params = if self.at_punct(Punct::LParen) {
            self.param_list()
        } else {
            let span = self.span();
            let pattern = self.binding_pattern();
            let mut params = vec![Param {
                pattern,
                ty: None,
                span,
            }];
            // `fn t, y => …` is the most common slip with a two-argument
            // lambda (the ODE right-hand side). Say how to write it and
            // recover with the parameters it reads as, instead of cascading
            // into unrelated errors at the `,`.
            if self.at_punct(Punct::Comma) {
                let comma = self.span();
                while self.eat_punct(Punct::Comma) {
                    let span = self.span();
                    let pattern = self.binding_pattern();
                    params.push(Param {
                        pattern,
                        ty: None,
                        span,
                    });
                }
                self.diagnostics.push(
                    Diagnostic::error(
                        "E0202",
                        "a lambda with several parameters needs parentheses",
                        comma,
                    )
                    .with_help("write `fn (a, b) => …`"),
                );
            }
            params
        };
        let ret = if self.eat_punct(Punct::Colon) {
            Some(self.ty())
        } else {
            None
        };
        self.expect_punct(Punct::FatArrow);
        let body = self.expr();
        Expr::new(
            ExprKind::Lambda {
                params,
                ret,
                body: Box::new(body),
            },
            start.join(self.previous_span()),
        )
    }

    fn do_expr(&mut self) -> Expr {
        let start = self.span();
        self.bump();
        if !(self.at_newline() && matches!(self.kind_at(1), TokenKind::Indent)) {
            self.error_here("E0103", "expected an indented `do` block");
            return Expr::new(ExprKind::Error, start);
        }
        self.bump();
        self.bump();
        let mut stmts = Vec::new();
        loop {
            self.skip_newlines();
            if self.at_dedent() || self.at_eof() {
                break;
            }
            let stmt_start = self.span();
            if self.at_kw(Keyword::Let) {
                self.bump();
                let pattern = self.binding_pattern();
                let ty = if self.eat_punct(Punct::Colon) {
                    Some(self.ty())
                } else {
                    None
                };
                if self.at_punct(Punct::LArrow) {
                    // `let x <- action` mixes the two statement forms. Say
                    // which one was meant and recover as the bind it reads as.
                    let span = self.span();
                    self.diagnostics.push(
                        Diagnostic::error("E0202", "`<-` does not follow `let`", span).with_help(
                            "run an action and bind its result with `x <- action`; \
                             `let x = e` binds a pure value",
                        ),
                    );
                    self.bump();
                    let value = self.expr();
                    self.expect_newline();
                    stmts.push(DoStmt::Bind {
                        pattern,
                        value,
                        span: stmt_start.join(self.previous_span()),
                    });
                    continue;
                }
                self.expect_punct(Punct::Equals);
                let value = self.expr();
                self.expect_newline();
                stmts.push(DoStmt::Let {
                    pattern,
                    ty,
                    value,
                    span: stmt_start.join(self.previous_span()),
                });
                continue;
            }
            if let Some(bind) = self.try_bind_statement() {
                stmts.push(bind);
                continue;
            }
            let value = self.expr();
            self.expect_newline();
            stmts.push(DoStmt::Expr(value));
        }
        self.expect_dedent();
        Expr::new(ExprKind::Do { stmts }, start.join(self.previous_span()))
    }

    fn try_bind_statement(&mut self) -> Option<DoStmt> {
        let saved_pos = self.pos;
        let saved_diagnostics = self.diagnostics.items.len();
        let start = self.span();
        let pattern = self.binding_pattern();
        if self.at_punct(Punct::LArrow) {
            self.bump();
            let value = self.expr();
            self.expect_newline();
            return Some(DoStmt::Bind {
                pattern,
                value,
                span: start.join(self.previous_span()),
            });
        }
        self.pos = saved_pos;
        self.diagnostics.items.truncate(saved_diagnostics);
        None
    }

    fn annot_expr(&mut self) -> Expr {
        let expr = self.pipe_expr();
        if self.eat_punct(Punct::Colon) {
            let ty = self.ty();
            let span = expr.span.join(ty.span);
            return Expr::new(
                ExprKind::Annot {
                    expr: Box::new(expr),
                    ty,
                },
                span,
            );
        }
        expr
    }

    fn pipe_expr(&mut self) -> Expr {
        let mut left = self.or_expr();
        loop {
            if self.at_punct(Punct::PipeGt) {
                self.bump();
                let right = self.or_expr();
                let span = left.span.join(right.span);
                left = Expr::new(
                    ExprKind::Call {
                        callee: Box::new(right),
                        args: vec![left],
                    },
                    span,
                );
            } else if self.at_punct(Punct::LtPipe) {
                self.bump();
                let right = self.or_expr();
                let span = left.span.join(right.span);
                left = Expr::new(
                    ExprKind::Call {
                        callee: Box::new(left),
                        args: vec![right],
                    },
                    span,
                );
            } else {
                return left;
            }
        }
    }

    fn or_expr(&mut self) -> Expr {
        let left = self.and_expr();
        if self.at_punct(Punct::OrOr) {
            self.bump();
            let right = self.or_expr();
            let span = left.span.join(right.span);
            return Expr::new(
                ExprKind::Binary {
                    op: BinOp::Or,
                    left: Box::new(left),
                    right: Box::new(right),
                },
                span,
            );
        }
        left
    }

    fn and_expr(&mut self) -> Expr {
        let left = self.cmp_expr();
        if self.at_punct(Punct::AndAnd) {
            self.bump();
            let right = self.and_expr();
            let span = left.span.join(right.span);
            return Expr::new(
                ExprKind::Binary {
                    op: BinOp::And,
                    left: Box::new(left),
                    right: Box::new(right),
                },
                span,
            );
        }
        left
    }

    fn cmp_expr(&mut self) -> Expr {
        let left = self.cons_expr();
        let op = match self.kind() {
            | TokenKind::Punct(Punct::EqEq) => Some(BinOp::Eq),
            | TokenKind::Punct(Punct::NotEq) => Some(BinOp::Ne),
            | TokenKind::Punct(Punct::Lt) => Some(BinOp::Lt),
            | TokenKind::Punct(Punct::Le) => Some(BinOp::Le),
            | TokenKind::Punct(Punct::Gt) => Some(BinOp::Gt),
            | TokenKind::Punct(Punct::Ge) => Some(BinOp::Ge),
            | _ => None,
        };
        let Some(op) = op else {
            return left;
        };
        self.bump();
        let right = self.cons_expr();
        let span = left.span.join(right.span);
        if matches!(
            self.kind(),
            TokenKind::Punct(
                Punct::EqEq | Punct::NotEq | Punct::Lt | Punct::Le | Punct::Gt | Punct::Ge
            )
        ) {
            self.error_here(
                "E0202",
                "comparison operators do not chain; use `&&` between comparisons",
            );
        }
        Expr::new(
            ExprKind::Binary {
                op,
                left: Box::new(left),
                right: Box::new(right),
            },
            span,
        )
    }

    fn cons_expr(&mut self) -> Expr {
        let left = self.add_expr();
        if self.at_punct(Punct::ColonColon) {
            self.bump();
            let right = self.cons_expr();
            let span = left.span.join(right.span);
            return Expr::new(
                ExprKind::Binary {
                    op: BinOp::Cons,
                    left: Box::new(left),
                    right: Box::new(right),
                },
                span,
            );
        }
        left
    }

    fn add_expr(&mut self) -> Expr {
        let mut left = self.mul_expr();
        loop {
            let op = match self.kind() {
                | TokenKind::Punct(Punct::Plus) => BinOp::Add,
                | TokenKind::Punct(Punct::Minus) => BinOp::Sub,
                | _ => return left,
            };
            self.bump();
            let right = self.mul_expr();
            let span = left.span.join(right.span);
            left = Expr::new(
                ExprKind::Binary {
                    op,
                    left: Box::new(left),
                    right: Box::new(right),
                },
                span,
            );
        }
    }

    fn mul_expr(&mut self) -> Expr {
        let mut left = self.unary_expr();
        loop {
            let op = match self.kind() {
                | TokenKind::Punct(Punct::Star) => BinOp::Mul,
                | TokenKind::Punct(Punct::Slash) => BinOp::Div,
                | TokenKind::Punct(Punct::At) => BinOp::MatMul,
                // §1.8.2: `%` is the percent unit; after an expression that
                // is not a number literal it can only be a remainder written
                // the C way. Say so once, and read it as `mod(a, b)` so the
                // rest of the line is checked normally.
                | TokenKind::Punct(Punct::Percent) if self.starts_expression_at(1) => {
                    let percent = self.span();
                    self.diagnostics.push(
                        Diagnostic::error(
                            "E0202",
                            "`%` is the percent unit, not a remainder operator",
                            percent,
                        )
                        .with_help("write `mod(a, b)` (or `rem(a, b)`) for the remainder"),
                    );
                    self.bump();
                    let right = self.unary_expr();
                    let span = left.span.join(right.span);
                    let callee = Expr::new(ExprKind::Var("mod".to_string()), percent);
                    left = Expr::new(
                        ExprKind::Call {
                            callee: Box::new(callee),
                            args: vec![left, right],
                        },
                        span,
                    );
                    continue;
                },
                | _ => return left,
            };
            self.bump();
            let right = self.unary_expr();
            let span = left.span.join(right.span);
            left = Expr::new(
                ExprKind::Binary {
                    op,
                    left: Box::new(left),
                    right: Box::new(right),
                },
                span,
            );
        }
    }

    fn unary_expr(&mut self) -> Expr {
        if !self.enter_depth() {
            return Expr::new(ExprKind::Error, self.span());
        }
        let result = self.unary_expr_inner();
        self.depth -= 1;
        result
    }

    fn enter_depth(&mut self) -> bool {
        self.depth += 1;
        if self.depth > MAX_DEPTH {
            self.depth -= 1;
            self.error_here("E0202", "expression nesting is too deep");
            return false;
        }
        true
    }

    fn unary_expr_inner(&mut self) -> Expr {
        let start = self.span();
        let op = match self.kind() {
            | TokenKind::Punct(Punct::Minus) => Some(UnOp::Neg),
            | TokenKind::Punct(Punct::Plus) => Some(UnOp::Pos),
            | TokenKind::Punct(Punct::Bang) => Some(UnOp::Not),
            | TokenKind::Punct(Punct::Sqrt) => Some(UnOp::Sqrt),
            | _ => None,
        };
        if let Some(op) = op {
            self.bump();
            let operand = self.unary_expr();
            let span = start.join(operand.span);
            return Expr::new(
                ExprKind::Unary {
                    op,
                    operand: Box::new(operand),
                },
                span,
            );
        }
        self.power_expr()
    }

    fn power_expr(&mut self) -> Expr {
        let left = self.postfix_expr();
        if self.at_punct(Punct::Caret) {
            self.bump();
            let right = self.unary_expr();
            let span = left.span.join(right.span);
            return Expr::new(
                ExprKind::Binary {
                    op: BinOp::Pow,
                    left: Box::new(left),
                    right: Box::new(right),
                },
                span,
            );
        }
        left
    }

    fn postfix_expr(&mut self) -> Expr {
        let mut expr = self.primary();
        loop {
            if self.at_punct(Punct::LParen) {
                self.bump();
                let mut args = Vec::new();
                while !self.at_punct(Punct::RParen) && !self.at_eof() {
                    args.push(self.expr());
                    if !self.eat_punct(Punct::Comma) {
                        break;
                    }
                }
                self.expect_punct(Punct::RParen);
                let span = expr.span.join(self.previous_span());
                expr = Expr::new(
                    ExprKind::Call {
                        callee: Box::new(expr),
                        args,
                    },
                    span,
                );
                continue;
            }
            if self.at_punct(Punct::LBracket) {
                self.bump();
                let mut indices = Vec::new();
                while !self.at_punct(Punct::RBracket) && !self.at_eof() {
                    indices.push(self.slice());
                    if !self.eat_punct(Punct::Comma) {
                        break;
                    }
                }
                self.expect_punct(Punct::RBracket);
                let span = expr.span.join(self.previous_span());
                expr = Expr::new(
                    ExprKind::Index {
                        base: Box::new(expr),
                        indices,
                    },
                    span,
                );
                continue;
            }
            if self.at_punct(Punct::Dot) && matches!(self.kind_at(1), TokenKind::Ident(_)) {
                self.bump();
                let name = if let TokenKind::Ident(name) = self.kind().clone() {
                    self.bump();
                    name
                } else {
                    String::new()
                };
                let span = expr.span.join(self.previous_span());
                expr = Expr::new(
                    ExprKind::Field {
                        base: Box::new(expr),
                        name,
                    },
                    span,
                );
                continue;
            }
            if self.at_punct(Punct::Question) {
                self.bump();
                let span = expr.span.join(self.previous_span());
                expr = Expr::new(ExprKind::Try(Box::new(expr)), span);
                continue;
            }
            if self.at_punct(Punct::Bang) {
                // Postfix `!` only binds to a literal or a parenthesised
                // expression (§2.7.5); otherwise it is the next expression's
                // prefix `!`.
                let previous_end = expr.span.end;
                let bang = self.span();
                if bang.start == previous_end {
                    self.bump();
                    let double = self.at_punct(Punct::Bang) && self.span().start == bang.end;
                    if double {
                        self.bump();
                    }
                    let span = expr.span.join(self.previous_span());
                    expr = Expr::new(
                        ExprKind::Factorial {
                            expr: Box::new(expr),
                            double,
                        },
                        span,
                    );
                    continue;
                }
            }
            if self.at_punct(Punct::Lt) && self.span().start == expr.span.end {
                // `f<T>(...)`: `<` is adjacent to the callee. Roll back when
                // the contents are not type arguments (§2.7.2). A type
                // argument may also be a shape or a natural number (§A.5
                // `type_arg`), which is how `reshape<[3, 2]>(a)` parses.
                let saved_pos = self.pos;
                let saved_diagnostics = self.diagnostics.items.len();
                self.bump();
                let mut args = Vec::new();
                let mut ok = true;
                if !self.at_punct(Punct::Gt) {
                    loop {
                        args.push(self.type_arg());
                        if !self.eat_punct(Punct::Comma) {
                            break;
                        }
                        if self.at_punct(Punct::Gt) {
                            break;
                        }
                    }
                }
                if !self.at_punct(Punct::Gt) {
                    ok = false;
                }
                if ok && self.eat_punct(Punct::Gt) {
                    let span = expr.span.join(self.previous_span());
                    // §2.7.2: a name or dotted name may take type arguments.
                    // Calls and computed expressions stay ambiguous (`E0207`).
                    if !is_named_type_callee(&expr) {
                        self.error(
                            "E0207",
                            "ambiguous type application: only a function name can take type arguments; add a space before `<` or parenthesise the callee",
                            span,
                        );
                    }
                    expr = Expr::new(
                        ExprKind::TypeApp {
                            callee: Box::new(expr),
                            args,
                        },
                        span,
                    );
                    continue;
                }
                self.pos = saved_pos;
                self.diagnostics.items.truncate(saved_diagnostics);
            }
            return expr;
        }
    }

    /// One index or slice (§A.4: `slice`).
    ///
    /// Three spellings reach this function:
    ///
    /// * `i` — an index;
    /// * `a..b` / `a..=b` — the half-open / inclusive range, untouched;
    /// * `a:b:s` — a slice with a step, where either bound and the step may be
    ///   omitted. `a:b` builds the same node `a..b` does, so the older form
    ///   keeps its exact meaning and the step is an additive third component.
    ///
    /// `::` lexes as one token ([`Punct::ColonColon`]) because it is also the
    /// cons operator and the module separator. At the *start* of a slice there
    /// is no left operand to cons onto, so `xs[::2]` is read as two slice
    /// colons; after an expression the cons reading wins and the slice needs
    /// its end written out.
    fn slice(&mut self) -> SliceAst {
        if self.at_punct(Punct::Colon) {
            self.bump();
            return self.slice_tail(None);
        }
        if self.at_punct(Punct::ColonColon) {
            self.bump();
            return self.slice_step(None, None);
        }
        if self.at_punct(Punct::DotDot) {
            self.bump();
            if self.at_punct(Punct::RBracket) || self.at_punct(Punct::Comma) {
                return SliceAst::Range {
                    start: None,
                    end: None,
                    inclusive: false,
                    step: None,
                };
            }
            let end = self.expr();
            return SliceAst::Range {
                start: None,
                end: Some(end),
                inclusive: false,
                step: None,
            };
        }
        // The bounds are parsed *below* `annot_expr`: inside `[...]` a `:` is
        // a slice separator, so `xs[a : b]` is a slice and not a type
        // ascription. An annotated index is written `xs[(a : T)]`.
        let start = self.pipe_expr();
        if self.eat_punct(Punct::Colon) {
            return self.slice_tail(Some(start));
        }
        if self.eat_punct(Punct::DotDot) {
            if self.at_punct(Punct::RBracket) || self.at_punct(Punct::Comma) {
                return SliceAst::Range {
                    start: Some(start),
                    end: None,
                    inclusive: false,
                    step: None,
                };
            }
            let end = self.expr();
            return SliceAst::Range {
                start: Some(start),
                end: Some(end),
                inclusive: false,
                step: None,
            };
        }
        if self.eat_punct(Punct::DotDotEq) {
            let end = self.expr();
            return SliceAst::Range {
                start: Some(start),
                end: Some(end),
                inclusive: true,
                step: None,
            };
        }
        SliceAst::Index(start)
    }

    /// The rest of a `:` slice, after its first `:` and its start.
    ///
    /// A second `:` is only an omitted end when there is no start either —
    /// `xs[::2]` — because after an expression `::` is the cons operator
    /// (§A.4), and `xs[1::2]` must keep meaning `xs[1 :: 2]`. A slice with a
    /// start and a step therefore names its end: `xs[1:5:2]`.
    fn slice_tail(
        &mut self,
        start: Option<Expr>,
    ) -> SliceAst {
        if self.at_punct(Punct::RBracket) || self.at_punct(Punct::Comma) {
            return SliceAst::Range {
                start,
                end: None,
                inclusive: false,
                step: None,
            };
        }
        if start.is_none() && self.eat_punct(Punct::Colon) {
            return self.slice_step(None, None);
        }
        let end = self.pipe_expr();
        if self.eat_punct(Punct::Colon) {
            return self.slice_step(start, Some(end));
        }
        SliceAst::Range {
            start,
            end: Some(end),
            inclusive: false,
            step: None,
        }
    }

    /// The step of a `:` slice; a trailing `:` with nothing after it means no
    /// step, so `xs[1:3:]` is `xs[1:3]`.
    fn slice_step(
        &mut self,
        start: Option<Expr>,
        end: Option<Expr>,
    ) -> SliceAst {
        let step = if self.at_punct(Punct::RBracket) || self.at_punct(Punct::Comma) {
            None
        } else {
            Some(self.pipe_expr())
        };
        SliceAst::Range {
            start,
            end,
            inclusive: false,
            step,
        }
    }

    fn primary(&mut self) -> Expr {
        let start = self.span();
        match self.kind().clone() {
            | TokenKind::Int(value) => {
                self.bump();
                self.quantity_or_number(NumberLit::Int(value), start)
            },
            | TokenKind::Float(_) => {
                let span = self.span();
                self.bump();
                let text = self.text_of(span).replace('_', "");
                self.quantity_or_number(NumberLit::Decimal(text), start)
            },
            | TokenKind::Str(parts) => {
                self.bump();
                let pieces = parts
                    .into_iter()
                    .map(|part| {
                        match part {
                            | StringPart::Text(text) => StrPiece::Text(text),
                            | StringPart::Interpolation { source, span } => {
                                StrPiece::Expr(Box::new(self.parse_interpolation(&source, span)))
                            },
                        }
                    })
                    .collect();
                Expr::new(ExprKind::Str(pieces), start.join(self.previous_span()))
            },
            | TokenKind::Keyword(Keyword::True) => {
                self.bump();
                Expr::new(ExprKind::Bool(true), start)
            },
            | TokenKind::Keyword(Keyword::False) => {
                self.bump();
                Expr::new(ExprKind::Bool(false), start)
            },
            | TokenKind::Keyword(Keyword::If) => self.if_expr(),
            | TokenKind::Keyword(Keyword::Match) => self.match_expr(),
            | TokenKind::Keyword(Keyword::Let) => self.let_expr(),
            | TokenKind::Keyword(Keyword::Fn) => self.lambda_expr(),
            | TokenKind::Keyword(Keyword::Do) => self.do_expr(),
            | TokenKind::Ident(name) => {
                self.bump();
                if name == "_" {
                    Expr::new(ExprKind::Placeholder, start)
                } else {
                    Expr::new(ExprKind::Var(name), start)
                }
            },
            | TokenKind::UpperIdent(name) => {
                self.bump();
                let mut path = vec![name];
                while self.at_punct(Punct::Dot)
                    && matches!(self.kind_at(1), TokenKind::UpperIdent(_))
                {
                    self.bump();
                    if let TokenKind::UpperIdent(next) = self.kind().clone() {
                        self.bump();
                        path.push(next);
                    }
                }
                if path.len() == 1 {
                    Expr::new(
                        ExprKind::Var(path.remove(0)),
                        start.join(self.previous_span()),
                    )
                } else {
                    Expr::new(ExprKind::QName(path), start.join(self.previous_span()))
                }
            },
            | TokenKind::Punct(Punct::LParen) => self.paren_expr(),
            | TokenKind::Punct(Punct::LBrace) => self.record_expr(),
            | TokenKind::Punct(Punct::LBracket) => {
                self.bump();
                let mut items = Vec::new();
                while !self.at_punct(Punct::RBracket) && !self.at_eof() {
                    items.push(self.expr());
                    if !self.eat_punct(Punct::Comma) {
                        break;
                    }
                }
                self.expect_punct(Punct::RBracket);
                Expr::new(ExprKind::Tensor(items), start.join(self.previous_span()))
            },
            | _ => {
                // A recovery node, so the checker does not invent a second
                // error for an expression the parser already rejected.
                self.unexpected("an expression");
                self.bump();
                Expr::new(ExprKind::Error, start)
            },
        }
    }

    fn parse_interpolation(
        &mut self,
        source: &str,
        span: Span,
    ) -> Expr {
        let file_source: &'a str = self.source;
        let lexed = lex_inline(self.file, source, usize::try_from(span.start).unwrap_or(0));
        let mut parser = Parser::new(&lexed.tokens, &lexed.docs, self.file, file_source);
        let expr = parser.expr();
        parser.skip_newlines();
        if parser.diagnostics.has_errors() {
            self.diagnostics.extend(parser.diagnostics);
        } else if !parser.at_eof() {
            let message = format!("unexpected {} in interpolation", parser.describe());
            let at = parser.span();
            self.diagnostics
                .push(Diagnostic::error("E0202", message, at));
        }
        expr
    }

    fn quantity_or_number(
        &mut self,
        value: NumberLit,
        start: Span,
    ) -> Expr {
        let unit = if self.starts_unit_atom() {
            self.unit_expr()
        } else if self.at_punct(Punct::LParen) {
            // `3 (m/s)` is a unit literal; `3 (2)` is not, and the spec makes
            // that an error rather than a call (§1.8.1).
            let saved_pos = self.pos;
            let saved_diagnostics = self.diagnostics.items.len();
            let parsed = self.unit_expr();
            if parsed.is_some() {
                parsed
            } else {
                self.pos = saved_pos;
                self.diagnostics.items.truncate(saved_diagnostics);
                // Two different mistakes reach this point. `3 (2)` is a
                // numeric literal followed by something that is not a unit at
                // all (`E0108`); `3 (m/)` is a malformed unit literal
                // (`E0209`, §1.8.1).
                match self.kind_at(1) {
                    | TokenKind::Int(_) | TokenKind::Float(_) => {
                        self.error(
                            "E0108",
                            "expected a unit after the numeric literal",
                            self.span(),
                        )
                    },
                    | _ => {
                        self.error(
                            "E0209",
                            "`(` after a numeric literal must contain a unit expression",
                            self.span(),
                        )
                    },
                }
                None
            }
        } else {
            None
        };
        match unit {
            | Some(unit) => {
                Expr::new(
                    ExprKind::Quantity { value, unit },
                    start.join(self.previous_span()),
                )
            },
            | None => {
                match value {
                    | NumberLit::Int(value) => Expr::new(ExprKind::Int(value), start),
                    | NumberLit::Decimal(text) => Expr::new(ExprKind::Float(text), start),
                }
            },
        }
    }

    fn paren_expr(&mut self) -> Expr {
        let start = self.span();
        self.bump(); // '('
        if self.at_punct(Punct::RParen) {
            self.bump();
            return Expr::new(ExprKind::UnitLit, start.join(self.previous_span()));
        }
        // Operator section `(+)`.
        if let TokenKind::Punct(punct) = self.kind().clone() {
            if self.punct_at(1, Punct::RParen) {
                if let Some(name) = operator_name(punct) {
                    self.bump();
                    self.bump();
                    return Expr::new(
                        ExprKind::Var(format!("({name})")),
                        start.join(self.previous_span()),
                    );
                }
            }
            // A *partial* section such as `(== 1)` or `(* 2)` is not in the
            // grammar (§1.8.5): only the bare operator or a lambda is a
            // function. Report it and keep parsing the operand so one mistake
            // does not cascade.
            if !self.punct_at(1, Punct::RParen)
                && operator_name(punct).is_some()
                && !matches!(punct, Punct::Plus | Punct::Minus)
            {
                self.error(
                    "E0205",
                    "invalid operator section: write `(+)` or a lambda",
                    self.span(),
                );
                self.bump();
            }
        }
        let first = self.expr();
        if self.at_punct(Punct::Comma) {
            let mut items = vec![first];
            while self.eat_punct(Punct::Comma) {
                if self.at_punct(Punct::RParen) {
                    break;
                }
                items.push(self.expr());
            }
            self.expect_punct(Punct::RParen);
            let span = start.join(self.previous_span());
            if items.len() == 1 {
                // §2.3.14: single-element tuples are not supported.
                self.error(
                    "E0202",
                    "single-element tuples are not supported; write the value itself",
                    span,
                );
            }
            return Expr::new(ExprKind::Tuple(items), span);
        }
        self.expect_punct(Punct::RParen);
        first
    }

    fn record_expr(&mut self) -> Expr {
        let start = self.span();
        self.bump(); // '{'
        if self.eat_punct(Punct::RBrace) {
            return Expr::new(
                ExprKind::Record {
                    base: None,
                    fields: Vec::new(),
                },
                start.join(self.previous_span()),
            );
        }
        let field_start = self.at_ident()
            && (self.punct_at(1, Punct::Equals)
                || self.punct_at(1, Punct::Comma)
                || self.punct_at(1, Punct::RBrace));
        if field_start {
            let fields = self.field_inits();
            self.expect_punct(Punct::RBrace);
            return Expr::new(
                ExprKind::Record { base: None, fields },
                start.join(self.previous_span()),
            );
        }
        let base = self.expr();
        self.expect_kw(Keyword::With);
        let fields = self.field_inits();
        self.expect_punct(Punct::RBrace);
        Expr::new(
            ExprKind::Record {
                base: Some(Box::new(base)),
                fields,
            },
            start.join(self.previous_span()),
        )
    }

    fn field_inits(&mut self) -> Vec<(String, Expr)> {
        let mut fields: Vec<(String, Expr)> = Vec::new();
        while !self.at_punct(Punct::RBrace) && !self.at_eof() {
            let name_span = self.span();
            let name = match self.kind().clone() {
                | TokenKind::Ident(name) => {
                    self.bump();
                    name
                },
                | _ => {
                    self.unexpected("a field name");
                    break;
                },
            };
            let value = if self.eat_punct(Punct::Equals) {
                self.expr()
            } else {
                let span = self.span();
                Expr::new(ExprKind::Var(name.clone()), span)
            };
            // §2.3.8: a record is a set of fields, so a repeated one has no
            // meaning — report it rather than silently keeping the last.
            if fields.iter().any(|(existing, _)| *existing == name) {
                self.error(
                    "E0208",
                    format!("duplicate field `{name}` in record"),
                    name_span,
                );
            }
            fields.push((name, value));
            if !self.eat_punct(Punct::Comma) {
                break;
            }
        }
        fields
    }

    // ----- patterns ------------------------------------------------------

    /// A pattern in a binding position.
    ///
    /// Scientific code names matrices and constants with capitals (`let A =
    /// …`, `fn step(M: Tensor<2, Float>)`), so in `let`, parameters and `<-`
    /// a bare `UIDENT` binds a variable. A constructor pattern there needs its
    /// argument list; a nullary constructor is matched with `match`, where a
    /// bare `UIDENT` keeps meaning the constructor (§2.4).
    fn binding_pattern(&mut self) -> Pat {
        let outer = std::mem::replace(&mut self.binding_pattern, true);
        let pattern = self.pattern();
        self.binding_pattern = outer;
        pattern
    }

    fn pattern(&mut self) -> Pat {
        let pat = self.pattern_atom();
        self.pattern_as(pat)
    }

    fn pattern_as(
        &mut self,
        pat: Pat,
    ) -> Pat {
        if self.eat_kw(Keyword::As) {
            let name = match self.kind().clone() {
                | TokenKind::Ident(name) => {
                    self.bump();
                    name
                },
                | _ => {
                    self.unexpected("a binding name");
                    String::new()
                },
            };
            let span = pat.span.join(self.previous_span());
            return Pat::new(
                PatKind::As {
                    pattern: Box::new(pat),
                    name,
                },
                span,
            );
        }
        pat
    }

    fn pattern_atom(&mut self) -> Pat {
        let start = self.span();
        match self.kind().clone() {
            | TokenKind::Ident(name) => {
                self.bump();
                if name == "_" {
                    Pat::new(PatKind::Wildcard, start)
                } else {
                    Pat::new(PatKind::Var(name), start)
                }
            },
            | TokenKind::UpperIdent(name)
                if self.binding_pattern && !self.punct_at(1, Punct::LParen) =>
            {
                self.bump();
                Pat::new(PatKind::Var(name), start)
            },
            | TokenKind::UpperIdent(name) => {
                self.bump();
                let mut args = Vec::new();
                if self.eat_punct(Punct::LParen) {
                    while !self.at_punct(Punct::RParen) && !self.at_eof() {
                        args.push(self.pattern());
                        if !self.eat_punct(Punct::Comma) {
                            break;
                        }
                    }
                    self.expect_punct(Punct::RParen);
                }
                Pat::new(
                    PatKind::Ctor { name, args },
                    start.join(self.previous_span()),
                )
            },
            | TokenKind::Int(value) => {
                self.bump();
                Pat::new(PatKind::Lit(PatLit::Int(value)), start)
            },
            | TokenKind::Str(parts) => {
                self.bump();
                let text = parts
                    .into_iter()
                    .filter_map(|part| {
                        match part {
                            | StringPart::Text(text) => Some(text),
                            | StringPart::Interpolation { .. } => None,
                        }
                    })
                    .collect::<Vec<_>>()
                    .join("");
                Pat::new(PatKind::Lit(PatLit::Str(text)), start)
            },
            | TokenKind::Keyword(Keyword::True) => {
                self.bump();
                Pat::new(PatKind::Lit(PatLit::Bool(true)), start)
            },
            | TokenKind::Keyword(Keyword::False) => {
                self.bump();
                Pat::new(PatKind::Lit(PatLit::Bool(false)), start)
            },
            | TokenKind::Punct(Punct::LParen) => {
                self.bump();
                if self.at_punct(Punct::RParen) {
                    self.bump();
                    return Pat::new(PatKind::Lit(PatLit::Unit), start.join(self.previous_span()));
                }
                let first = self.pattern();
                if self.eat_punct(Punct::Colon) {
                    let ty = self.ty();
                    self.expect_punct(Punct::RParen);
                    return Pat::new(
                        PatKind::Annot {
                            pattern: Box::new(first),
                            ty,
                        },
                        start.join(self.previous_span()),
                    );
                }
                if self.at_punct(Punct::Comma) {
                    let mut items = vec![first];
                    while self.eat_punct(Punct::Comma) {
                        if self.at_punct(Punct::RParen) {
                            break;
                        }
                        items.push(self.pattern());
                    }
                    self.expect_punct(Punct::RParen);
                    return Pat::new(PatKind::Tuple(items), start.join(self.previous_span()));
                }
                self.expect_punct(Punct::RParen);
                first
            },
            | TokenKind::Punct(Punct::LBrace) => {
                self.bump();
                let mut fields = Vec::new();
                while !self.at_punct(Punct::RBrace) && !self.at_eof() {
                    let name = match self.kind().clone() {
                        | TokenKind::Ident(name) => {
                            self.bump();
                            name
                        },
                        | _ => {
                            self.unexpected("a field name");
                            break;
                        },
                    };
                    let sub = if self.eat_punct(Punct::Equals) {
                        Some(self.pattern())
                    } else {
                        None
                    };
                    fields.push((name, sub));
                    if !self.eat_punct(Punct::Comma) {
                        break;
                    }
                }
                self.expect_punct(Punct::RBrace);
                Pat::new(PatKind::Record(fields), start.join(self.previous_span()))
            },
            | TokenKind::Punct(Punct::LBracket) => {
                self.bump();
                let mut items = Vec::new();
                let mut rest = None;
                while !self.at_punct(Punct::RBracket) && !self.at_eof() {
                    if self.eat_punct(Punct::DotDot) {
                        if let TokenKind::Ident(name) = self.kind().clone() {
                            self.bump();
                            rest = Some(name);
                        }
                        break;
                    }
                    items.push(self.pattern());
                    if !self.eat_punct(Punct::Comma) {
                        break;
                    }
                }
                self.expect_punct(Punct::RBracket);
                Pat::new(
                    PatKind::Tensor { items, rest },
                    start.join(self.previous_span()),
                )
            },
            | _ => {
                // §B.3/B.4: a keyword where a binder belongs is E0206 (reserved
                // word as identifier), anything else that cannot start a
                // pattern is E0204 (invalid pattern).
                let found = self.describe();
                if matches!(self.kind(), TokenKind::Keyword(_)) {
                    self.error_here(
                        "E0206",
                        format!("{found} is a reserved word and cannot be a binding"),
                    );
                } else {
                    self.error_here("E0204", format!("invalid pattern: {found}"));
                }
                let span = self.span();
                self.bump();
                Pat::new(PatKind::Error, span)
            },
        }
    }

    // ----- types ---------------------------------------------------------

    fn ty(&mut self) -> Ty {
        self.depth += 1;
        if self.depth > MAX_DEPTH {
            self.depth -= 1;
            self.error_here("E0202", "type nesting is too deep");
            return Ty::new(TyKind::Unit, self.span());
        }
        let result = self.ty_inner();
        self.depth -= 1;
        result
    }

    fn ty_inner(&mut self) -> Ty {
        if self.ident_is("forall") && self.looks_like_forall() {
            let start = self.span();
            self.bump();
            let mut params = Vec::new();
            loop {
                let span = self.span();
                let name = match self.kind().clone() {
                    | TokenKind::Ident(name) | TokenKind::UpperIdent(name) => {
                        self.bump();
                        name
                    },
                    | _ => {
                        self.unexpected("a type parameter");
                        break;
                    },
                };
                let kind = if self.eat_punct(Punct::Colon) {
                    match self.kind().clone() {
                        | TokenKind::UpperIdent(kind) => {
                            self.bump();
                            Kind::from_name(&kind)
                        },
                        | _ => None,
                    }
                } else {
                    None
                };
                params.push(TypeParam { name, kind, span });
                if !self.eat_punct(Punct::Comma) {
                    break;
                }
            }
            self.expect_punct(Punct::Dot);
            let body = self.ty_inner();
            let span = start.join(body.span);
            return Ty::new(TyKind::Forall(params, Box::new(body)), span);
        }
        let mut first = self.ty_atom();
        // §A.5: `type_postfix ::= type_atom { "[" IDENT "]" }`. The bracketed
        // arguments are dimensions of a type constructor, which is how a user
        // type with a `Dim` parameter is applied (`Vec[L]`).
        while self.at_punct(Punct::LBracket) {
            self.bump();
            let dim = self.dim();
            self.expect_punct(Punct::RBracket);
            let span = first.span.join(self.previous_span());
            match &mut first.kind {
                | TyKind::Named { args, .. } => {
                    args.push(Ty::new(TyKind::DimArg(dim), span));
                },
                | _ => {
                    self.error(
                        "E0202",
                        "only a named type takes `[dimension]` arguments",
                        span,
                    );
                },
            }
            first.span = span;
        }
        if self.at_punct(Punct::Arrow) {
            self.bump();
            let ret = self.ty_inner();
            let span = first.span.join(ret.span);
            let args = match first.kind {
                | TyKind::Tuple(items) => items,
                | _ => vec![first],
            };
            return Ty::new(TyKind::Fn(args, Box::new(ret)), span);
        }
        first
    }

    fn looks_like_forall(&self) -> bool {
        // `forall d. ...` versus the standard-library function `forall(...)`.
        matches!(
            self.kind_at(1),
            TokenKind::Ident(_) | TokenKind::UpperIdent(_)
        ) && (self.punct_at(2, Punct::Dot) || self.punct_at(2, Punct::Comma))
    }

    fn ty_atom(&mut self) -> Ty {
        let start = self.span();
        match self.kind().clone() {
            | TokenKind::UpperIdent(name) => {
                self.bump();
                self.named_type(name, start)
            },
            | TokenKind::Ident(name) => {
                self.bump();
                Ty::new(TyKind::Var(name), start)
            },
            | TokenKind::Punct(Punct::LParen) => {
                self.bump();
                if self.at_punct(Punct::RParen) {
                    self.bump();
                    return Ty::new(TyKind::Unit, start.join(self.previous_span()));
                }
                let first = self.ty();
                if self.at_punct(Punct::Comma) {
                    let mut items = vec![first];
                    while self.eat_punct(Punct::Comma) {
                        if self.at_punct(Punct::RParen) {
                            break;
                        }
                        items.push(self.ty());
                    }
                    self.expect_punct(Punct::RParen);
                    return Ty::new(TyKind::Tuple(items), start.join(self.previous_span()));
                }
                self.expect_punct(Punct::RParen);
                first
            },
            | TokenKind::Punct(Punct::LBrace) => {
                self.bump();
                let mut fields: Vec<(String, Ty)> = Vec::new();
                let mut rest = None;
                while !self.at_punct(Punct::RBrace) && !self.at_eof() {
                    if self.eat_punct(Punct::Ellipsis) {
                        if let TokenKind::Ident(name) = self.kind().clone() {
                            self.bump();
                            rest = Some(name);
                        }
                        break;
                    }
                    let name_span = self.span();
                    let name = match self.kind().clone() {
                        | TokenKind::Ident(name) => {
                            self.bump();
                            name
                        },
                        | _ => {
                            self.unexpected("a field name");
                            break;
                        },
                    };
                    self.expect_punct(Punct::Colon);
                    let ty = self.ty();
                    // §2.3.8: a record type is a set of fields.
                    if fields.iter().any(|(existing, _)| *existing == name) {
                        self.error(
                            "E0208",
                            format!("duplicate field `{name}` in record type"),
                            name_span,
                        );
                    }
                    fields.push((name, ty));
                    if !self.eat_punct(Punct::Comma) {
                        break;
                    }
                }
                self.expect_punct(Punct::RBrace);
                Ty::new(
                    TyKind::Record { fields, rest },
                    start.join(self.previous_span()),
                )
            },
            | _ => {
                self.unexpected("a type");
                self.bump();
                Ty::new(TyKind::Unit, start)
            },
        }
    }

    fn named_type(
        &mut self,
        name: String,
        start: Span,
    ) -> Ty {
        match name.as_str() {
            | "Int" => return Ty::new(TyKind::Int, start),
            | "Bool" => return Ty::new(TyKind::Bool, start),
            | "Prop" => return Ty::new(TyKind::Prop, start),
            | "String" => return Ty::new(TyKind::Str, start),
            | "Real" | "Float" | "Complex" => {
                let dim = if self.at_punct(Punct::LBracket) {
                    self.bump();
                    let dim = self.dim();
                    self.expect_punct(Punct::RBracket);
                    dim
                } else {
                    DimAst::One
                };
                let kind = match name.as_str() {
                    | "Real" => TyKind::Real(dim),
                    | "Float" => TyKind::Float(dim),
                    | _ => TyKind::Complex(dim),
                };
                return Ty::new(kind, start.join(self.previous_span()));
            },
            | "Tensor" => {
                self.expect_punct(Punct::Lt);
                let shape = self.shape();
                self.expect_punct(Punct::Comma);
                let elem = self.ty();
                self.expect_punct(Punct::Gt);
                return Ty::new(
                    TyKind::Tensor {
                        shape,
                        elem: Box::new(elem),
                    },
                    start.join(self.previous_span()),
                );
            },
            | "Matrix" => {
                // §A.5: `Matrix<nat, nat, type>`.
                self.expect_punct(Punct::Lt);
                let rows = self.nat();
                self.expect_punct(Punct::Comma);
                let cols = self.nat();
                self.expect_punct(Punct::Comma);
                let elem = self.ty();
                self.expect_punct(Punct::Gt);
                return Ty::new(
                    TyKind::Matrix {
                        rows,
                        cols,
                        elem: Box::new(elem),
                    },
                    start.join(self.previous_span()),
                );
            },
            | "Sparse" => {
                // §A.5: `Sparse<format_name, shape, type>`.
                self.expect_punct(Punct::Lt);
                let format = match self.kind().clone() {
                    | TokenKind::UpperIdent(format) | TokenKind::Ident(format) => {
                        self.bump();
                        format
                    },
                    | _ => {
                        self.unexpected("a storage format");
                        "CSR".to_string()
                    },
                };
                self.expect_punct(Punct::Comma);
                let shape = self.shape();
                self.expect_punct(Punct::Comma);
                let elem = self.ty();
                self.expect_punct(Punct::Gt);
                return Ty::new(
                    TyKind::Sparse {
                        format,
                        shape,
                        elem: Box::new(elem),
                    },
                    start.join(self.previous_span()),
                );
            },
            | "Point" => {
                self.expect_punct(Punct::LBracket);
                let dim = self.dim();
                self.expect_punct(Punct::RBracket);
                return Ty::new(TyKind::Point(dim), start.join(self.previous_span()));
            },
            | "Unit" => {
                if self.at_punct(Punct::LBracket) {
                    self.bump();
                    let dim = self.dim();
                    self.expect_punct(Punct::RBracket);
                    return Ty::new(TyKind::UnitOf(dim), start.join(self.previous_span()));
                }
                return Ty::new(TyKind::Unit, start);
            },
            | "IO" => {
                let inner = self.ty_atom();
                let span = start.join(inner.span);
                return Ty::new(TyKind::Io(Box::new(inner)), span);
            },
            | "SymFun" => {
                self.expect_punct(Punct::Lt);
                let domain = self.dim();
                self.expect_punct(Punct::Comma);
                let codomain = self.dim();
                self.expect_punct(Punct::Gt);
                return Ty::new(
                    TyKind::SymFun(domain, codomain),
                    start.join(self.previous_span()),
                );
            },
            | "Dyn" => {
                self.expect_punct(Punct::Lt);
                let rep = match self.kind().clone() {
                    | TokenKind::UpperIdent(rep) | TokenKind::Ident(rep) => {
                        self.bump();
                        rep
                    },
                    | _ => {
                        self.unexpected("a representation");
                        "Real".to_string()
                    },
                };
                self.expect_punct(Punct::Gt);
                return Ty::new(TyKind::Dyn(rep), start.join(self.previous_span()));
            },
            | "RateOf" => {
                self.expect_punct(Punct::Lt);
                let inner = self.ty();
                self.expect_punct(Punct::Gt);
                return Ty::new(
                    TyKind::RateOf(Box::new(inner)),
                    start.join(self.previous_span()),
                );
            },
            | "DiffusionOf" => {
                self.expect_punct(Punct::Lt);
                let inner = self.ty();
                self.expect_punct(Punct::Gt);
                return Ty::new(
                    TyKind::DiffusionOf(Box::new(inner)),
                    start.join(self.previous_span()),
                );
            },
            | "Option" | "Set" => {
                self.expect_punct(Punct::Lt);
                let inner = self.ty();
                self.expect_punct(Punct::Gt);
                let kind = match name.as_str() {
                    | "Option" => TyKind::Option(Box::new(inner)),
                    | _ => TyKind::Set(Box::new(inner)),
                };
                return Ty::new(kind, start.join(self.previous_span()));
            },
            | "Table" => {
                // §A.5: `Table<row_type>`, and a row type may be a sum.
                self.expect_punct(Punct::Lt);
                let row = self.row_type();
                self.expect_punct(Punct::Gt);
                return Ty::new(
                    TyKind::Table(Box::new(row)),
                    start.join(self.previous_span()),
                );
            },
            | "Result" => {
                self.expect_punct(Punct::Lt);
                let ok = self.ty();
                self.expect_punct(Punct::Comma);
                let err = self.ty();
                self.expect_punct(Punct::Gt);
                return Ty::new(
                    TyKind::Result(Box::new(ok), Box::new(err)),
                    start.join(self.previous_span()),
                );
            },
            | "Solutions" => {
                self.expect_punct(Punct::Lt);
                let inner = self.ty();
                self.expect_punct(Punct::Comma);
                let dim = self.dim();
                self.expect_punct(Punct::Gt);
                return Ty::new(
                    TyKind::Named {
                        path: vec!["Solutions".to_string()],
                        args: vec![inner, Ty::new(TyKind::Real(dim), start)],
                    },
                    start.join(self.previous_span()),
                );
            },
            | _ => {},
        }
        let mut path = vec![name];
        while self.at_punct(Punct::Dot) && matches!(self.kind_at(1), TokenKind::UpperIdent(_)) {
            self.bump();
            if let TokenKind::UpperIdent(next) = self.kind().clone() {
                self.bump();
                path.push(next);
            }
        }
        let mut args = Vec::new();
        if self.at_punct(Punct::Lt) {
            self.bump();
            while !self.at_punct(Punct::Gt) && !self.at_eof() {
                args.push(self.type_arg());
                if !self.eat_punct(Punct::Comma) {
                    break;
                }
            }
            self.expect_punct(Punct::Gt);
        }
        Ty::new(
            TyKind::Named { path, args },
            start.join(self.previous_span()),
        )
    }

    /// One type argument (§A.5: `type_arg ::= type | nat | dim | shape`).
    ///
    /// A natural number and a refined shape cannot start a type, so they are
    /// recognised by their first token; everything else is a type. A bare name
    /// stays a type (or type variable) because only the applied constructor's
    /// declared parameter kinds can tell `Vec<a>` from `Vec<d>`; an explicit
    /// dimension is written `T[d]` (`type_postfix`).
    fn type_arg(&mut self) -> Ty {
        let start = self.span();
        if self.at_punct(Punct::LBracket) {
            let shape = self.shape();
            return Ty::new(TyKind::ShapeArg(shape), start.join(self.previous_span()));
        }
        if matches!(self.kind(), TokenKind::Int(_)) {
            let nat = self.nat();
            return Ty::new(TyKind::NatArg(nat), start.join(self.previous_span()));
        }
        self.ty()
    }

    /// A row type (§A.5: `row_type ::= type { "+" type }`).
    ///
    /// One summand is just that type; a sum is kept as written so a formatter
    /// can reproduce it and the checker can merge the rows.
    fn row_type(&mut self) -> Ty {
        let first = self.ty();
        if !self.at_punct(Punct::Plus) {
            return first;
        }
        let start = first.span;
        let mut items = vec![first];
        while self.eat_punct(Punct::Plus) {
            items.push(self.ty());
        }
        Ty::new(TyKind::Row(items), start.join(self.previous_span()))
    }

    fn shape(&mut self) -> ShapeAst {
        if self.at_punct(Punct::LBracket) {
            self.bump();
            let mut items = Vec::new();
            while !self.at_punct(Punct::RBracket) && !self.at_eof() {
                if self.at_punct(Punct::Question) {
                    self.bump();
                    items.push(ShapeItem::Unknown);
                } else {
                    items.push(ShapeItem::Known(self.nat()));
                }
                if !self.eat_punct(Punct::Comma) {
                    break;
                }
            }
            self.expect_punct(Punct::RBracket);
            ShapeAst::Refined(items)
        } else {
            ShapeAst::Rank(self.nat())
        }
    }

    /// A type-level natural number (§3.5.1: literal, variable, `+`, `*`).
    ///
    /// `*` binds tighter than `+`, so `n + 1 * 2` is `n + 2`; parentheses
    /// group, so `2 * (n + 1)` is expressible.
    fn nat(&mut self) -> NatAst {
        let mut left = self.nat_term();
        while self.eat_punct(Punct::Plus) {
            let right = self.nat_term();
            left = NatAst::Add(Box::new(left), Box::new(right));
        }
        left
    }

    fn nat_term(&mut self) -> NatAst {
        let mut left = self.nat_atom();
        while self.eat_punct(Punct::Star) {
            let right = self.nat_atom();
            left = NatAst::Mul(Box::new(left), Box::new(right));
        }
        left
    }

    fn nat_atom(&mut self) -> NatAst {
        match self.kind().clone() {
            | TokenKind::Int(value) => {
                self.bump();
                NatAst::Lit(value.to_usize().unwrap_or(0))
            },
            | TokenKind::Ident(name) | TokenKind::UpperIdent(name) => {
                self.bump();
                NatAst::Var(name)
            },
            | TokenKind::Punct(Punct::LParen) => {
                self.bump();
                let inner = self.nat();
                self.expect_punct(Punct::RParen);
                inner
            },
            | _ => {
                self.unexpected("a natural number");
                NatAst::Lit(0)
            },
        }
    }

    // ----- dimensions and unit expressions --------------------------------

    fn dim(&mut self) -> DimAst {
        let mut left = self.dim_factor();
        loop {
            if self.at_punct(Punct::Star) {
                self.bump();
                let right = self.dim_factor();
                left = DimAst::Mul(Box::new(left), Box::new(right));
            } else if self.at_punct(Punct::Slash) {
                self.bump();
                let right = self.dim_factor();
                left = DimAst::Div(Box::new(left), Box::new(right));
            } else {
                return left;
            }
        }
    }

    fn dim_factor(&mut self) -> DimAst {
        let mut atom = self.dim_atom();
        if self.eat_punct(Punct::Caret) {
            let exponent = self.exponent();
            atom = DimAst::Pow(Box::new(atom), exponent);
        }
        atom
    }

    fn dim_atom(&mut self) -> DimAst {
        let start = self.span();
        match self.kind().clone() {
            | TokenKind::Int(value) if value == BigInt::from(1) => {
                self.bump();
                DimAst::One
            },
            | TokenKind::UpperIdent(name) | TokenKind::Ident(name) => {
                self.bump();
                let mut path = name;
                while self.at_punct(Punct::Dot) {
                    self.bump();
                    match self.kind().clone() {
                        | TokenKind::UpperIdent(part) | TokenKind::Ident(part) => {
                            self.bump();
                            path.push('.');
                            path.push_str(&part);
                        },
                        | _ => break,
                    }
                }
                let _ = start;
                DimAst::Name(path)
            },
            | TokenKind::Punct(Punct::LParen) => {
                self.bump();
                let inner = self.dim();
                self.expect_punct(Punct::RParen);
                DimAst::Paren(Box::new(inner))
            },
            | _ => {
                self.unexpected("a dimension");
                self.bump();
                DimAst::One
            },
        }
    }

    fn exponent(&mut self) -> BigRational {
        let negative = self.eat_punct(Punct::Minus);
        if self.eat_punct(Punct::LParen) {
            let numerator = self.signed_int();
            let mut value = BigRational::from_integer(numerator);
            if self.eat_punct(Punct::Slash) {
                let denominator = self.signed_int();
                if denominator == BigInt::from(0) {
                    self.error_here("E0202", "zero denominator in exponent");
                } else {
                    value /= BigRational::from_integer(denominator);
                }
            }
            self.expect_punct(Punct::RParen);
            return if negative { -value } else { value };
        }
        let value = BigRational::from_integer(self.signed_int());
        if negative { -value } else { value }
    }

    fn signed_int(&mut self) -> BigInt {
        let negative = self.eat_punct(Punct::Minus);
        match self.kind().clone() {
            | TokenKind::Int(value) => {
                self.bump();
                if negative { -value } else { value }
            },
            | _ => {
                self.unexpected("an integer");
                BigInt::from(1)
            },
        }
    }

    fn starts_unit_atom(&self) -> bool {
        matches!(
            self.kind(),
            TokenKind::Ident(_) | TokenKind::UpperIdent(_) | TokenKind::Punct(Punct::Percent)
        )
    }

    fn unit_expr(&mut self) -> Option<UnitExpr> {
        let mut left = self.unit_factor()?;
        loop {
            // §1.8.1: `*` and `/` continue a unit expression only when they
            // are written tight, with no space on either side (`m/s^2`,
            // `kg*m^2`). A spaced operator ends the literal and is ordinary
            // arithmetic, so `9.81 m/s^2 * mass` is `(9.81 m/s^2) * mass`.
            if !self.tight_unit_operator() {
                return Some(left);
            }
            if self.at_punct(Punct::Star) && self.starts_unit_atom_at(1) {
                self.bump();
                let right = self.unit_factor()?;
                left = UnitExpr::Mul(Box::new(left), Box::new(right));
            } else if self.at_punct(Punct::Slash) && self.starts_unit_atom_at(1) {
                self.bump();
                let right = self.unit_factor()?;
                left = UnitExpr::Div(Box::new(left), Box::new(right));
            } else {
                return Some(left);
            }
        }
    }

    /// Whether the current token touches both its neighbours.
    fn tight_unit_operator(&self) -> bool {
        let current = self.span();
        let Some(next) = self.tokens.get(self.pos + 1) else {
            return false;
        };
        self.pos > 0 && self.previous_span().end == current.start && current.end == next.span.start
    }

    fn starts_unit_atom_at(
        &self,
        offset: usize,
    ) -> bool {
        matches!(
            self.kind_at(offset),
            TokenKind::Ident(_) | TokenKind::UpperIdent(_) | TokenKind::Punct(Punct::Percent)
        )
    }

    fn unit_factor(&mut self) -> Option<UnitExpr> {
        let mut atom = self.unit_atom()?;
        if self.eat_punct(Punct::Caret) {
            let exponent = self.exponent();
            atom = UnitExpr::Pow(Box::new(atom), exponent);
        } else if let TokenKind::Superscript(text) = self.kind().clone() {
            self.bump();
            match parse_superscript(&text) {
                | Some(exponent) => {
                    atom = UnitExpr::Pow(
                        Box::new(atom),
                        BigRational::from_integer(BigInt::from(exponent)),
                    );
                },
                | None => self.error_here("E0107", "invalid superscript sequence"),
            }
        }
        Some(atom)
    }

    fn unit_atom(&mut self) -> Option<UnitExpr> {
        match self.kind().clone() {
            | TokenKind::Punct(Punct::Percent) => {
                self.bump();
                Some(UnitExpr::Percent)
            },
            | TokenKind::Punct(Punct::LParen) => {
                self.bump();
                let inner = self.unit_expr()?;
                self.expect_punct(Punct::RParen);
                Some(UnitExpr::Paren(Box::new(inner)))
            },
            | TokenKind::Ident(name) | TokenKind::UpperIdent(name) => {
                self.bump();
                let mut path = name;
                while self.at_punct(Punct::Dot) {
                    match self.kind_at(1).clone() {
                        | TokenKind::Ident(part) | TokenKind::UpperIdent(part) => {
                            self.bump();
                            self.bump();
                            path.push('.');
                            path.push_str(&part);
                        },
                        | _ => break,
                    }
                }
                Some(UnitExpr::Name(path))
            },
            | _ => {
                self.unexpected("a unit");
                None
            },
        }
    }
}

fn decl_span(kind: &DeclKind) -> Span {
    // Used only for nested function declarations, where the exact span is not
    // observable; the body's span is the best available approximation.
    match kind {
        | DeclKind::Fn { body, .. } => body.span,
        | DeclKind::Let { value, .. } => value.span,
        | _ => Span::default(),
    }
}

fn operator_name(punct: Punct) -> Option<&'static str> {
    Some(match punct {
        | Punct::Plus => "+",
        | Punct::Minus => "-",
        | Punct::Star => "*",
        | Punct::Slash => "/",
        | Punct::At => "@",
        | Punct::Caret => "^",
        | Punct::EqEq => "==",
        | Punct::NotEq => "!=",
        | Punct::Lt => "<",
        | Punct::Le => "<=",
        | Punct::Gt => ">",
        | Punct::Ge => ">=",
        | Punct::AndAnd => "&&",
        | Punct::OrOr => "||",
        | Punct::ColonColon => "::",
        | _ => return None,
    })
}

/// Converts a superscript run such as `⁻¹` to an integer exponent.
fn parse_superscript(text: &str) -> Option<i32> {
    let mut value: i32 = 0;
    let mut negative = false;
    let mut saw_digit = false;
    for c in text.chars() {
        match c {
            | '\u{2070}'..='\u{2079}' => {
                let digit = (c as u32) - 0x2070;
                value = value
                    .checked_mul(10)?
                    .checked_add(i32::try_from(digit).ok()?)?;
                saw_digit = true;
            },
            // U+00B9/B2/B3: the superscripts on most keyboards.
            | '\u{00B9}' => {
                value = value.checked_mul(10)?.checked_add(1)?;
                saw_digit = true;
            },
            | '\u{00B2}' => {
                value = value.checked_mul(10)?.checked_add(2)?;
                saw_digit = true;
            },
            | '\u{00B3}' => {
                value = value.checked_mul(10)?.checked_add(3)?;
                saw_digit = true;
            },
            | '\u{207B}' => negative = true,
            | '\u{207A}' => {},
            | _ => return None,
        }
    }
    if !saw_digit {
        return None;
    }
    Some(if negative { -value } else { value })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lexer::lex;

    fn parse(source: &str) -> Module {
        let lexed = lex(0, source).expect("lexes");
        let (module, diagnostics) = parse_module(0, source, &lexed);
        assert!(
            !diagnostics.has_errors(),
            "unexpected diagnostics:\n{}",
            diagnostics
                .items
                .iter()
                .map(|item| format!("{}: {}", item.code, item.message))
                .collect::<Vec<_>>()
                .join("\n")
        );
        module
    }

    fn parse_expr_source(source: &str) -> Expr {
        let lexed = lex(0, source).expect("lexes");
        let mut parser = Parser::new(&lexed.tokens, &lexed.docs, 0, source);
        let expr = parser.expr();
        assert!(
            !parser.diagnostics.has_errors(),
            "unexpected diagnostics: {:?}",
            parser
                .diagnostics
                .items
                .iter()
                .map(|item| (&item.code, &item.message))
                .collect::<Vec<_>>()
        );
        expr
    }

    #[test]
    fn parses_a_function_with_a_block_body() {
        let module =
            parse("fn fall_time(h: Float[L]): Float[T] =\n    let n = 2\n    sqrt(n * h)\n");
        assert_eq!(module.decls.len(), 1);
        let DeclKind::Fn { name, args, body, .. } = &module.decls[0].kind else {
            panic!("expected fn");
        };
        assert_eq!(name, "fall_time");
        assert_eq!(args.len(), 1);
        assert!(matches!(body.kind, ExprKind::Block { .. }));
    }

    #[test]
    fn parses_unit_literals() {
        let expr = parse_expr_source("9.80665 m/s^2");
        let ExprKind::Quantity { value, unit } = &expr.kind else {
            panic!("expected quantity, got {:?}", expr.kind);
        };
        assert!(value.is_decimal());
        assert_eq!(unit.to_source(), "m/s^2");
    }

    #[test]
    fn division_after_a_unit_stops_at_a_number() {
        let expr = parse_expr_source("3 m/2 s");
        let ExprKind::Binary { op, left, right } = &expr.kind else {
            panic!("expected division, got {:?}", expr.kind);
        };
        assert_eq!(*op, BinOp::Div);
        assert!(matches!(left.kind, ExprKind::Quantity { .. }));
        assert!(matches!(right.kind, ExprKind::Quantity { .. }));
    }

    #[test]
    fn multiplication_of_a_quantity_and_a_variable() {
        // A spaced `*` ends the unit literal (§1.8.1): this is `(3 m) * x`.
        let expr = parse_expr_source("3 m * x");
        let ExprKind::Binary { op, left, right } = &expr.kind else {
            panic!("expected multiplication, got {:?}", expr.kind);
        };
        assert_eq!(*op, BinOp::Mul);
        let ExprKind::Quantity { unit, .. } = &left.kind else {
            panic!("expected quantity, got {:?}", left.kind);
        };
        assert_eq!(unit.to_source(), "m");
        assert!(matches!(&right.kind, ExprKind::Var(name) if name == "x"));
    }

    #[test]
    fn a_comma_separated_lambda_head_is_one_diagnostic_with_a_fix() {
        let lexed = lex(
            0,
            "let f = fn t, y => t + y
",
        )
        .expect("lexes");
        let (module, diagnostics) = parse_module(
            0,
            "let f = fn t, y => t + y
",
            &lexed,
        );
        let errors: Vec<_> = diagnostics.items.iter().collect();
        assert_eq!(errors.len(), 1, "{errors:?}");
        assert_eq!(errors[0].code, "E0202");
        assert!(
            errors[0]
                .helps
                .iter()
                .any(|help| help.contains("fn (a, b)"))
        );
        let DeclKind::Let { value, .. } = &module.decls[0].kind else {
            panic!("expected a let");
        };
        let ExprKind::Lambda { params, .. } = &value.kind else {
            panic!("expected a lambda, got {:?}", value.kind);
        };
        assert_eq!(params.len(), 2);
    }

    #[test]
    fn tight_unit_operators_continue_the_literal() {
        for (source, unit) in [
            ("3 m*x", "m*x"),
            ("9.81 m/s^2", "m/s^2"),
            ("1.5 kg*m^2/s^3", "kg*m^2/s^3"),
            ("2 kg·m", "kg·m"),
        ] {
            let expr = parse_expr_source(source);
            let ExprKind::Quantity { unit: parsed, .. } = &expr.kind else {
                panic!("{source}: expected quantity, got {:?}", expr.kind);
            };
            assert_eq!(parsed.to_source(), unit, "{source}");
        }
    }

    #[test]
    fn a_spaced_operator_after_a_unit_is_arithmetic() {
        // The specification's own tour writes `70 kg * v^2 / 2` (§0.5).
        let expr = parse_expr_source("70 kg * v^2 / 2");
        let ExprKind::Binary {
            op: BinOp::Div, left, ..
        } = &expr.kind
        else {
            panic!("expected division, got {:?}", expr.kind);
        };
        let ExprKind::Binary {
            op: BinOp::Mul, left, ..
        } = &left.kind
        else {
            panic!("expected multiplication, got {:?}", left.kind);
        };
        assert!(matches!(left.kind, ExprKind::Quantity { .. }));
        // Half-tight is not tight: `m /s` and `m/ s` end the literal too.
        for source in ["3 m /s", "3 m/ s"] {
            let expr = parse_expr_source(source);
            assert!(
                matches!(expr.kind, ExprKind::Binary { op: BinOp::Div, .. }),
                "{source}: {:?}",
                expr.kind
            );
        }
    }

    #[test]
    fn parses_precedence() {
        let expr = parse_expr_source("1 + 2 * 3 ^ 2");
        let ExprKind::Binary { op, right, .. } = &expr.kind else {
            panic!("expected add");
        };
        assert_eq!(*op, BinOp::Add);
        let ExprKind::Binary { op, right, .. } = &right.kind else {
            panic!("expected mul");
        };
        assert_eq!(*op, BinOp::Mul);
        assert!(matches!(
            right.kind,
            ExprKind::Binary { op: BinOp::Pow, .. }
        ));
    }

    #[test]
    fn parses_match_with_guards() {
        let module = parse(
            "fn sign(x: Real[1]): Int =\n    match x with\n    | 0 -> 0\n    | n if n > 0 -> 1\n    | _ -> -1\n",
        );
        let DeclKind::Fn { body, .. } = &module.decls[0].kind else {
            panic!("expected fn");
        };
        let ExprKind::Match { arms, .. } = &body.kind else {
            panic!("expected match");
        };
        assert_eq!(arms.len(), 3);
        assert!(arms[1].guard.is_some());
    }

    #[test]
    fn parses_records_and_updates() {
        let expr = parse_expr_source("{ p with x = 3.0 m }");
        let ExprKind::Record { base, fields } = &expr.kind else {
            panic!("expected record update");
        };
        assert!(base.is_some());
        assert_eq!(fields.len(), 1);
    }

    #[test]
    fn parses_type_application_versus_comparison() {
        let expr = parse_expr_source("zeros<Float>(3, 4)");
        assert!(matches!(expr.kind, ExprKind::Call { .. }));
        let expr = parse_expr_source("a < b");
        assert!(matches!(expr.kind, ExprKind::Binary { op: BinOp::Lt, .. }));
    }

    #[test]
    fn parses_do_blocks() {
        let module = parse(
            "fn main(): IO Unit = do\n    text <- read_file(\"x\")\n    let n = 1\n    println(text)\n",
        );
        let DeclKind::Fn { body, .. } = &module.decls[0].kind else {
            panic!("expected fn");
        };
        let ExprKind::Do { stmts } = &body.kind else {
            panic!("expected do");
        };
        assert_eq!(stmts.len(), 3);
        assert!(matches!(stmts[0], DoStmt::Bind { .. }));
    }

    #[test]
    fn parses_unit_and_dimension_declarations() {
        let module = parse(
            "dimension Currency\nunit USD : Currency = 1\nunit EUR : Currency = 1.08 USD\nunit m : L = 1 prefixable\nunit degC : Θ = 1 K offset 273.15 K\n",
        );
        assert_eq!(module.decls.len(), 5);
        let DeclKind::Unit {
            name,
            scale,
            prefixable,
            ..
        } = &module.decls[3].kind
        else {
            panic!("expected unit");
        };
        assert_eq!(name, "m");
        assert!(scale.is_one());
        assert!(prefixable);
        let DeclKind::Unit { offset, .. } = &module.decls[4].kind else {
            panic!("expected unit");
        };
        assert!(offset.is_some());
    }

    #[test]
    fn parses_variants_and_types() {
        let module = parse(
            "type Shape =\n    | Circle(Real[L])\n    | Rect(Real[L], Real[L])\n\ntype Vec2 = { x: Real[L], y: Real[L] }\n",
        );
        let DeclKind::Type { body, .. } = &module.decls[0].kind else {
            panic!("expected type");
        };
        let TypeBody::Variants(variants) = body else {
            panic!("expected variants");
        };
        assert_eq!(variants.len(), 2);
        let DeclKind::Type { body, .. } = &module.decls[1].kind else {
            panic!("expected type");
        };
        assert!(matches!(body, TypeBody::Alias(_)));
    }

    #[test]
    fn parses_string_interpolation() {
        let expr = parse_expr_source("\"value = {x + 1}\"");
        let ExprKind::Str(pieces) = &expr.kind else {
            panic!("expected string");
        };
        assert_eq!(pieces.len(), 2);
        assert!(matches!(pieces[1], StrPiece::Expr(_)));
    }

    #[test]
    fn parses_index_and_slices() {
        let expr = parse_expr_source("m[0..2, 1..=3]");
        let ExprKind::Index { indices, .. } = &expr.kind else {
            panic!("expected index");
        };
        assert_eq!(indices.len(), 2);
    }

    #[test]
    fn parses_slice_steps() {
        // The step is the third component and only the `:` spelling carries
        // it; `a..b` keeps its two-component node with no step.
        let expr = parse_expr_source("m[1:5:2, ::-1, :, 3]");
        let ExprKind::Index { indices, .. } = &expr.kind else {
            panic!("expected index");
        };
        assert_eq!(indices.len(), 4);
        let steps: Vec<bool> = indices
            .iter()
            .map(|index| {
                match index {
                    | SliceAst::Range { step, .. } => step.is_some(),
                    | SliceAst::Index(_) => false,
                }
            })
            .collect();
        assert_eq!(steps, vec![true, true, false, false]);
        let SliceAst::Range { start, end, step, .. } = &indices[0] else {
            panic!("expected a range");
        };
        assert!(start.is_some() && end.is_some() && step.is_some());
        let SliceAst::Range { start, end, step, .. } = &indices[1] else {
            panic!("expected a range");
        };
        assert!(start.is_none() && end.is_none() && step.is_some());
    }

    #[test]
    fn a_slice_step_does_not_steal_the_cons_operator() {
        // `1::2` is a cons expression, and inside an index it stays one.
        let expr = parse_expr_source("xs[1::rest]");
        let ExprKind::Index { indices, .. } = &expr.kind else {
            panic!("expected index");
        };
        assert_eq!(indices.len(), 1);
        assert!(matches!(indices[0], SliceAst::Index(_)));
    }

    #[test]
    fn parses_lambdas() {
        let expr = parse_expr_source("fn (x: Real[L], y) => x + y");
        let ExprKind::Lambda { params, .. } = &expr.kind else {
            panic!("expected lambda");
        };
        assert_eq!(params.len(), 2);
    }

    #[test]
    fn parses_nested_blocks_after_equals() {
        let module = parse("let y =\n    let a = 1\n    let b = 2\n    a + b\n");
        let DeclKind::Let { value, .. } = &module.decls[0].kind else {
            panic!("expected let");
        };
        let ExprKind::Block { stmts, tail } = &value.kind else {
            panic!("expected block");
        };
        assert_eq!(stmts.len(), 2);
        assert!(tail.is_some());
    }

    #[test]
    fn reports_missing_else() {
        let source = "let x = if a then 1\n";
        let lexed = lex(0, source).expect("lexes");
        let (_, diagnostics) = parse_module(0, source, &lexed);
        assert!(diagnostics.has_errors());
    }
}
