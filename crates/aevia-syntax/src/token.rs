//! Tokens produced by the lexer.

use crate::span::Span;
use num_bigint::BigInt;
use std::fmt;

/// A reserved word (spec §1.5.2).
///
/// `forall` and `return` are deliberately absent: the first is a contextual
/// keyword in types, the second is an ordinary Prelude function.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Keyword {
    /// `as`
    As,
    /// `class`
    Class,
    /// `derive`
    Derive,
    /// `dimension`
    Dimension,
    /// `do`
    Do,
    /// `else`
    Else,
    /// `false`
    False,
    /// `fn`
    Fn,
    /// `for`
    For,
    /// `if`
    If,
    /// `import`
    Import,
    /// `in`
    In,
    /// `instance`
    Instance,
    /// `let`
    Let,
    /// `match`
    Match,
    /// `module`
    Module,
    /// `open`
    Open,
    /// `private`
    Private,
    /// `rec`
    Rec,
    /// `test`
    Test,
    /// `then`
    Then,
    /// `true`
    True,
    /// `type`
    Type,
    /// `unit`
    Unit,
    /// `where`
    Where,
    /// `with`
    With,
}

impl Keyword {
    /// Looks a word up as a keyword.
    #[must_use]
    pub fn from_word(word: &str) -> Option<Self> {
        Some(match word {
            | "as" => Self::As,
            | "class" => Self::Class,
            | "derive" => Self::Derive,
            | "dimension" => Self::Dimension,
            | "do" => Self::Do,
            | "else" => Self::Else,
            | "false" => Self::False,
            | "fn" => Self::Fn,
            | "for" => Self::For,
            | "if" => Self::If,
            | "import" => Self::Import,
            | "in" => Self::In,
            | "instance" => Self::Instance,
            | "let" => Self::Let,
            | "match" => Self::Match,
            | "module" => Self::Module,
            | "open" => Self::Open,
            | "private" => Self::Private,
            | "rec" => Self::Rec,
            | "test" => Self::Test,
            | "then" => Self::Then,
            | "true" => Self::True,
            | "type" => Self::Type,
            | "unit" => Self::Unit,
            | "where" => Self::Where,
            | "with" => Self::With,
            | _ => return None,
        })
    }

    /// The source spelling.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            | Self::As => "as",
            | Self::Class => "class",
            | Self::Derive => "derive",
            | Self::Dimension => "dimension",
            | Self::Do => "do",
            | Self::Else => "else",
            | Self::False => "false",
            | Self::Fn => "fn",
            | Self::For => "for",
            | Self::If => "if",
            | Self::Import => "import",
            | Self::In => "in",
            | Self::Instance => "instance",
            | Self::Let => "let",
            | Self::Match => "match",
            | Self::Module => "module",
            | Self::Open => "open",
            | Self::Private => "private",
            | Self::Rec => "rec",
            | Self::Test => "test",
            | Self::Then => "then",
            | Self::True => "true",
            | Self::Type => "type",
            | Self::Unit => "unit",
            | Self::Where => "where",
            | Self::With => "with",
        }
    }
}

impl fmt::Display for Keyword {
    fn fmt(
        &self,
        f: &mut fmt::Formatter<'_>,
    ) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Punctuation and operators.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Punct {
    /// `(`
    LParen,
    /// `)`
    RParen,
    /// `[`
    LBracket,
    /// `]`
    RBracket,
    /// `{`
    LBrace,
    /// `}`
    RBrace,
    /// `,`
    Comma,
    /// `;`
    Semicolon,
    /// `:`
    Colon,
    /// `.`
    Dot,
    /// `..`
    DotDot,
    /// `..=`
    DotDotEq,
    /// `...`
    Ellipsis,
    /// `->`
    Arrow,
    /// `=>`
    FatArrow,
    /// `<-`
    LArrow,
    /// `|`
    Pipe,
    /// `=`
    Equals,
    /// `+`
    Plus,
    /// `-`
    Minus,
    /// `*`
    Star,
    /// `/`
    Slash,
    /// `^`
    Caret,
    /// `@`
    At,
    /// `%`
    Percent,
    /// `==`
    EqEq,
    /// `!=`
    NotEq,
    /// `<`
    Lt,
    /// `<=`
    Le,
    /// `>`
    Gt,
    /// `>=`
    Ge,
    /// `&&`
    AndAnd,
    /// `||`
    OrOr,
    /// `!`
    Bang,
    /// `|>`
    PipeGt,
    /// `<|`
    LtPipe,
    /// `::`
    ColonColon,
    /// `?`
    Question,
    /// `√`
    Sqrt,
}

impl Punct {
    /// The source spelling.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            | Self::LParen => "(",
            | Self::RParen => ")",
            | Self::LBracket => "[",
            | Self::RBracket => "]",
            | Self::LBrace => "{",
            | Self::RBrace => "}",
            | Self::Comma => ",",
            | Self::Semicolon => ";",
            | Self::Colon => ":",
            | Self::Dot => ".",
            | Self::DotDot => "..",
            | Self::DotDotEq => "..=",
            | Self::Ellipsis => "...",
            | Self::Arrow => "->",
            | Self::FatArrow => "=>",
            | Self::LArrow => "<-",
            | Self::Pipe => "|",
            | Self::Equals => "=",
            | Self::Plus => "+",
            | Self::Minus => "-",
            | Self::Star => "*",
            | Self::Slash => "/",
            | Self::Caret => "^",
            | Self::At => "@",
            | Self::Percent => "%",
            | Self::EqEq => "==",
            | Self::NotEq => "!=",
            | Self::Lt => "<",
            | Self::Le => "<=",
            | Self::Gt => ">",
            | Self::Ge => ">=",
            | Self::AndAnd => "&&",
            | Self::OrOr => "||",
            | Self::Bang => "!",
            | Self::PipeGt => "|>",
            | Self::LtPipe => "<|",
            | Self::ColonColon => "::",
            | Self::Question => "?",
            | Self::Sqrt => "√",
        }
    }
}

impl fmt::Display for Punct {
    fn fmt(
        &self,
        f: &mut fmt::Formatter<'_>,
    ) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// One piece of a string literal.
#[derive(Clone, Debug, PartialEq)]
pub enum StringPart {
    /// Literal text, with escapes already decoded.
    Text(String),
    /// `{expr}`: the raw source of the embedded expression.
    Interpolation {
        /// The expression source, without the braces.
        source: String,
        /// The span of the source, so sub-lexing reports correct positions.
        span: Span,
    },
}

/// What a token is.
#[derive(Clone, Debug, PartialEq)]
pub enum TokenKind {
    /// A lowercase identifier.
    Ident(String),
    /// An uppercase identifier (constructors, modules, types).
    UpperIdent(String),
    /// A reserved word.
    Keyword(Keyword),
    /// An integer literal.
    Int(BigInt),
    /// A floating-point literal.
    Float(f64),
    /// A string literal, split into text and interpolations.
    Str(Vec<StringPart>),
    /// A superscript run such as `²` or `⁻¹`, in a unit expression.
    Superscript(String),
    /// Punctuation or an operator.
    Punct(Punct),
    /// End of a logical line.
    Newline,
    /// Start of an indented block.
    Indent,
    /// End of an indented block.
    Dedent,
    /// End of file.
    Eof,
}

impl TokenKind {
    /// A short description used in "expected X, found Y" messages.
    #[must_use]
    pub fn describe(&self) -> String {
        match self {
            | Self::Ident(name) => format!("identifier `{name}`"),
            | Self::UpperIdent(name) => format!("`{name}`"),
            | Self::Keyword(keyword) => format!("`{keyword}`"),
            | Self::Int(value) => format!("`{value}`"),
            | Self::Float(value) => format!("`{value}`"),
            | Self::Str(_) => "string literal".to_string(),
            | Self::Superscript(text) => format!("`{text}`"),
            | Self::Punct(punct) => format!("`{punct}`"),
            | Self::Newline => "end of line".to_string(),
            | Self::Indent => "indentation".to_string(),
            | Self::Dedent => "end of block".to_string(),
            | Self::Eof => "end of file".to_string(),
        }
    }
}

/// A token with its source location.
#[derive(Clone, Debug, PartialEq)]
pub struct Token {
    /// What the token is.
    pub kind: TokenKind,
    /// Where it came from.
    pub span: Span,
}

impl Token {
    /// Creates a token.
    #[must_use]
    pub fn new(
        kind: TokenKind,
        span: Span,
    ) -> Self {
        Self { kind, span }
    }
}

/// A `##` documentation comment, attached to the next declaration.
#[derive(Clone, Debug)]
pub struct DocComment {
    /// Where the comment starts.
    pub span: Span,
    /// The comment body, with the `##` markers removed.
    pub text: String,
}
