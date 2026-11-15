//! Terminal editing and submission boundaries for the REPL.
//!
//! Rustyline keeps bracketed pastes in one editable buffer. History is deliberately
//! in memory only: evaluated code can contain credentials and local file paths.

use std::borrow::Cow;
use std::collections::VecDeque;
use std::io::IsTerminal;
use std::io::Read;
use std::io::{
    self,
};

use aevia_syntax::Keyword;
use aevia_syntax::Punct;
use aevia_syntax::TokenKind;
use rustyline::CompletionType;
use rustyline::Config;
use rustyline::Context;
use rustyline::Editor;
use rustyline::Helper;
use rustyline::completion::Completer;
use rustyline::completion::FilenameCompleter;
use rustyline::completion::Pair;
use rustyline::config::ColorMode;
use rustyline::error::ReadlineError;
use rustyline::highlight::CmdKind;
use rustyline::highlight::Highlighter;
use rustyline::highlight::MatchingBracketHighlighter;
use rustyline::hint::Hinter;
use rustyline::hint::HistoryHinter;
use rustyline::history::DefaultHistory;
use rustyline::validate::ValidationContext;
use rustyline::validate::ValidationResult;
use rustyline::validate::Validator;

pub enum Event {
    Input(String),
    Interrupted,
    Eof,
}

pub struct Input {
    editor: Option<Editor<ReplHelper, DefaultHistory>>,
    pending: VecDeque<String>,
    pipe_read: bool,
}

impl Input {
    pub fn new(color: bool) -> Result<Self, String> {
        // TERM=dumb still supports ordinary line-at-a-time input, but not the
        // cursor-control sequences used by a terminal editor.
        let interactive = io::stdin().is_terminal() && io::stdout().is_terminal();
        let editor = if interactive {
            let config = Config::builder()
                .completion_type(CompletionType::List)
                .history_ignore_dups(true)
                .map_err(|error| error.to_string())?
                .history_ignore_space(true)
                .max_history_size(1000)
                .map_err(|error| error.to_string())?
                .color_mode(if color {
                    ColorMode::Forced
                } else {
                    ColorMode::Disabled
                })
                .indent_size(4)
                .bracketed_paste(true)
                .build();
            let mut editor = Editor::with_config(config).map_err(|error| error.to_string())?;
            editor.set_helper(Some(ReplHelper::new(color)));
            Some(editor)
        } else {
            None
        };
        Ok(Self {
            editor,
            pending: VecDeque::new(),
            pipe_read: false,
        })
    }

    pub fn interactive(&self) -> bool {
        self.editor.is_some()
    }

    pub fn set_bindings(
        &mut self,
        names: impl IntoIterator<Item = String>,
    ) {
        if let Some(helper) = self.editor.as_mut().and_then(Editor::helper_mut) {
            helper.bindings = names.into_iter().collect();
            helper.bindings.sort();
            helper.bindings.dedup();
        }
    }

    pub fn read(&mut self) -> Result<Event, String> {
        let Some(editor) = self.editor.as_mut() else {
            if !self.pipe_read {
                let mut source = String::new();
                io::stdin()
                    .read_to_string(&mut source)
                    .map_err(|error| error.to_string())?;
                self.pending = split_submissions(&source).into();
                self.pipe_read = true;
            }
            return Ok(self.pending.pop_front().map_or(Event::Eof, Event::Input));
        };
        let prompt = "aevia> ";
        let line = match editor.readline(prompt) {
            | Ok(line) => line,
            | Err(ReadlineError::Interrupted) => return Ok(Event::Interrupted),
            | Err(ReadlineError::Eof) => return Ok(Event::Eof),
            | Err(error) => return Err(error.to_string()),
        };
        if line.trim() == ":cancel" {
            return Ok(Event::Interrupted);
        }
        if line.trim() != ":paste" {
            if !line.trim().is_empty() {
                let _ = editor.add_history_entry(&line);
            }
            return Ok(Event::Input(line));
        }
        // This mode also works on terminals which cannot bracket a paste.
        // Validation stays off so no line is evaluated until :end is entered.
        if let Some(helper) = editor.helper_mut() {
            helper.paste = true;
        }
        let mut source = String::new();
        let event = loop {
            match editor.readline("paste> ") {
                | Ok(line) if line.trim() == ":end" && !inside_paste_literal(&source) => {
                    break Ok(Event::Input(source.clone()));
                },
                | Ok(line) if line.trim() == ":cancel" && !inside_paste_literal(&source) => {
                    break Ok(Event::Interrupted);
                },
                | Ok(line) => {
                    source.push_str(&line);
                    source.push('\n');
                },
                | Err(ReadlineError::Interrupted) => break Ok(Event::Interrupted),
                | Err(ReadlineError::Eof) => {
                    break Ok(Event::Input(":paste-unfinished".to_string()));
                },
                | Err(error) => break Err(error.to_string()),
            }
        };
        if let Some(helper) = editor.helper_mut() {
            helper.paste = false;
        }
        if matches!(&event, Ok(Event::Input(text)) if text != ":paste-unfinished")
            && !source.trim().is_empty()
        {
            let _ = editor.add_history_entry(source);
        }
        event
    }
}

struct ReplHelper {
    bindings: Vec<String>,
    files: FilenameCompleter,
    brackets: MatchingBracketHighlighter,
    history: HistoryHinter,
    color: bool,
    paste: bool,
}

impl ReplHelper {
    fn new(color: bool) -> Self {
        Self {
            bindings: Vec::new(),
            files: FilenameCompleter::new(),
            brackets: MatchingBracketHighlighter::new(),
            history: HistoryHinter::new(),
            color,
            paste: false,
        }
    }
}

impl Helper for ReplHelper {}

impl Completer for ReplHelper {
    type Candidate = Pair;

    fn complete(
        &self,
        line: &str,
        pos: usize,
        ctx: &Context<'_>,
    ) -> rustyline::Result<(usize, Vec<Pair>)> {
        let command = line.trim_start();
        if command
            .find(char::is_whitespace)
            .is_some_and(|end| matches!(&command[..end], ":load" | ":l" | ":save"))
        {
            return self.files.complete(line, pos, ctx);
        }
        let start = line[..pos]
            .char_indices()
            .rev()
            .find(|(_, ch)| !(ch.is_alphanumeric() || *ch == '_' || *ch == ':' || *ch == '.'))
            .map_or(0, |(index, ch)| index + ch.len_utf8());
        let word = &line[start..pos];
        let mut module_names = Vec::new();
        for name in &self.bindings {
            if name
                .split('.')
                .all(|part| part.chars().next().is_some_and(char::is_uppercase))
            {
                module_names.push(name.clone());
            }
            for (index, _) in name.match_indices('.') {
                let prefix = &name[..index];
                if prefix
                    .split('.')
                    .all(|part| part.chars().next().is_some_and(char::is_uppercase))
                {
                    module_names.push(prefix.to_string());
                }
            }
        }
        let prefix = line[..start].rsplit('\n').next().unwrap_or("").trim();
        if prefix == ":format" {
            return Ok((
                start,
                ["show", "repr", "pretty", "latex", "typst"]
                    .into_iter()
                    .filter(|name| name.starts_with(word))
                    .map(|name| {
                        Pair {
                            display: name.to_string(),
                            replacement: name.to_string(),
                        }
                    })
                    .collect(),
            ));
        }
        let module_context = matches!(prefix, "import" | "open");
        let mut names: Vec<&str> = module_names.iter().map(String::as_str).collect();
        if !module_context {
            names.extend(self.bindings.iter().map(String::as_str));
            names.extend(aevia_types::builtins::PRELUDE_NAMES.iter().copied());
            names.extend([
                ":help",
                ":h",
                ":?",
                ":bindings",
                ":imports",
                ":vars",
                ":t",
                ":l",
                ":quit",
                ":q",
                ":reset",
                ":type",
                ":dims",
                ":units",
                ":ast",
                ":core",
                ":time",
                ":latex",
                ":typst",
                ":pretty",
                ":pprint",
                ":repr",
                ":print",
                ":format",
                ":doc",
                ":apropos",
                ":load",
                ":paste",
                ":end",
                ":cancel",
                ":run",
                ":show",
                ":save",
                "let",
                "fn",
                "main",
                "type",
                "dimension",
                "unit",
                "import",
                "open",
                "class",
                "instance",
                "match",
                "with",
                "if",
                "then",
                "else",
                "do",
                "true",
                "false",
                "println",
                "print",
                "map",
                "filter",
                "fold",
                "sin",
                "cos",
                "sqrt",
                "sum",
            ]);
        }
        names.sort_unstable();
        names.dedup();
        Ok((
            start,
            names
                .into_iter()
                .filter(|name| name.starts_with(word))
                .map(|name| {
                    Pair {
                        display: name.to_string(),
                        replacement: name.to_string(),
                    }
                })
                .collect(),
        ))
    }
}

impl Hinter for ReplHelper {
    type Hint = String;

    fn hint(
        &self,
        line: &str,
        pos: usize,
        ctx: &Context<'_>,
    ) -> Option<String> {
        // Pasted programs remain available through history navigation, but an
        // inline suggestion should not redraw an entire program while typing.
        self.history
            .hint(line, pos, ctx)
            .filter(|hint| !hint.contains(['\n', '\r']))
    }
}

impl Highlighter for ReplHelper {
    fn highlight<'l>(
        &self,
        line: &'l str,
        pos: usize,
    ) -> Cow<'l, str> {
        if !self.color {
            return Cow::Borrowed(line);
        }
        let bracketed = self.brackets.highlight(line, pos);
        // Rustyline marks one matching ASCII bracket. Its insertion offset is
        // also the original byte offset, before any syntax styles are added.
        let matching = bracketed.find("\x1b[");
        syntax_highlight(line, matching).map_or(bracketed, Cow::Owned)
    }

    fn highlight_prompt<'b, 's: 'b, 'p: 'b>(
        &'s self,
        prompt: &'p str,
        _default: bool,
    ) -> Cow<'b, str> {
        if self.color {
            Cow::Owned(format!("\x1b[1;36m{prompt}\x1b[0m"))
        } else {
            Cow::Borrowed(prompt)
        }
    }

    fn highlight_hint<'h>(
        &self,
        hint: &'h str,
    ) -> Cow<'h, str> {
        if self.color {
            Cow::Owned(format!("\x1b[2m{hint}\x1b[0m"))
        } else {
            Cow::Borrowed(hint)
        }
    }

    fn highlight_char(
        &self,
        line: &str,
        pos: usize,
        kind: CmdKind,
    ) -> bool {
        if self.color {
            self.brackets.highlight_char(line, pos, kind);
            // Any inserted/deleted character can change a token's class.
            true
        } else {
            false
        }
    }
}

/// Color token spans without changing a byte of the user's input. Gaps can
/// contain comments (the lexer intentionally omits them) or plain whitespace.
fn syntax_highlight(
    line: &str,
    matching: Option<usize>,
) -> Option<String> {
    let lexed = aevia_syntax::lex(0, line).ok()?;
    let mut output = String::with_capacity(line.len() + lexed.tokens.len() * 10);
    let mut cursor = 0;
    for token in &lexed.tokens {
        if matches!(
            token.kind,
            TokenKind::Newline | TokenKind::Indent | TokenKind::Dedent | TokenKind::Eof
        ) {
            continue;
        }
        let start = token.span.start as usize;
        let end = token.span.end as usize;
        if start < cursor || end > line.len() {
            continue;
        }
        highlight_gap(&mut output, &line[cursor..start]);
        let code = match token.kind {
            | TokenKind::Keyword(_) => "1;35",
            | TokenKind::UpperIdent(_) => "36",
            | TokenKind::Int(_) | TokenKind::Float(_) | TokenKind::Superscript(_) => "33",
            | TokenKind::Str(_) => "32",
            | _ => "",
        };
        highlight_span(
            &mut output,
            &line[start..end],
            code,
            matching
                .and_then(|index| index.checked_sub(start))
                .filter(|index| *index < end - start),
        );
        cursor = end;
    }
    highlight_gap(&mut output, &line[cursor..]);
    Some(output)
}

fn highlight_gap(
    output: &mut String,
    gap: &str,
) {
    if let Some(comment) = gap.find('#') {
        output.push_str(&gap[..comment]);
        highlight_span(output, &gap[comment..], "2;90", None);
    } else {
        output.push_str(gap);
    }
}

fn highlight_span(
    output: &mut String,
    text: &str,
    code: &str,
    matching: Option<usize>,
) {
    if !code.is_empty() {
        output.push_str(&format!("\x1b[{code}m"));
    }
    if let Some(index) = matching {
        output.push_str(&text[..index]);
        output.push_str("\x1b[1;34m");
        output.push_str(&text[index..index + 1]);
        output.push_str("\x1b[0m");
        if !code.is_empty() {
            output.push_str(&format!("\x1b[{code}m"));
        }
        output.push_str(&text[index + 1..]);
    } else {
        output.push_str(text);
    }
    if !code.is_empty() {
        output.push_str("\x1b[0m");
    }
}

impl Validator for ReplHelper {
    fn validate(
        &self,
        ctx: &mut ValidationContext<'_>,
    ) -> rustyline::Result<ValidationResult> {
        Ok(if !self.paste && incomplete(ctx.input()) {
            ValidationResult::Incomplete
        } else {
            ValidationResult::Valid(None)
        })
    }
}

/// Explicit paste markers are ordinary source inside multiline strings or
/// nested block comments. Delimiter incompleteness alone does not hide :end.
fn inside_paste_literal(source: &str) -> bool {
    aevia_syntax::lex(0, source).is_err_and(|diagnostics| {
        diagnostics.items.iter().any(|d| {
            d.message.contains("unclosed block comment")
                || d.message.contains("unclosed multi-line string")
        })
    })
}

/// Uses the language lexer so comments, escaped strings, raw strings and
/// interpolations have exactly the same delimiter rules as ordinary programs.
fn lexical_continuation(source: &str) -> bool {
    let lexed = match aevia_syntax::lex(0, source) {
        | Ok(lexed) => lexed,
        | Err(diagnostics) => {
            return diagnostics.items.iter().any(|d| {
                // Ordinary strings cannot cross newlines; submit those errors
                // immediately. Triple strings and nested block comments can.
                d.message.contains("unclosed block comment") || d.message.contains("unclosed multi")
            });
        },
    };
    let mut stack = Vec::new();
    let mut last = None;
    for token in &lexed.tokens {
        match token.kind {
            | TokenKind::Punct(Punct::LParen) => stack.push(Punct::RParen),
            | TokenKind::Punct(Punct::LBracket) => stack.push(Punct::RBracket),
            | TokenKind::Punct(Punct::LBrace) => stack.push(Punct::RBrace),
            | TokenKind::Punct(close @ (Punct::RParen | Punct::RBracket | Punct::RBrace)) => {
                if stack.pop() != Some(close) {
                    // A mismatch is an error, not an invitation to keep typing.
                    return false;
                }
            },
            | _ => {},
        }
        if !matches!(
            token.kind,
            TokenKind::Newline | TokenKind::Indent | TokenKind::Dedent | TokenKind::Eof
        ) {
            last = Some(&token.kind);
        }
    }
    !stack.is_empty()
        || matches!(
            last,
            Some(TokenKind::Keyword(
                Keyword::Do | Keyword::Then | Keyword::Else | Keyword::Where | Keyword::With
            )) | Some(TokenKind::Punct(
                Punct::Equals
                    | Punct::Arrow
                    | Punct::FatArrow
                    | Punct::LArrow
                    | Punct::Pipe
                    | Punct::Plus
                    | Punct::Minus
                    | Punct::Star
                    | Punct::Slash
                    | Punct::Caret
                    | Punct::AndAnd
                    | Punct::OrOr
                    | Punct::PipeGt
                    | Punct::LtPipe
                    | Punct::Comma
            ))
        )
}

/// Continue a conditional only when the parser reports missing text at EOF.
/// Earlier syntax errors must be submitted rather than hidden by continuation.
fn missing_else(source: &str) -> bool {
    let Ok(lexed) = aevia_syntax::lex(0, source) else {
        return false;
    };
    if !lexed
        .tokens
        .iter()
        .any(|token| matches!(token.kind, TokenKind::Keyword(Keyword::If)))
    {
        return false;
    }
    let source = if declaration(source) {
        format!("{source}\n")
    } else {
        format!("let __repl_input = {source}\n")
    };
    let Ok(lexed) = aevia_syntax::lex(0, &source) else {
        return false;
    };
    let (_, diagnostics) = aevia_syntax::parse_module(0, &source, &lexed);
    diagnostics
        .items
        .iter()
        .any(|d| d.message == "expected `else`, found end of file")
        && diagnostics
            .items
            .iter()
            .all(|d| d.message.ends_with("found end of file"))
}

fn incomplete(source: &str) -> bool {
    if source.trim_start().starts_with(':') {
        return false;
    }
    if lexical_continuation(source) {
        return true;
    }
    if !source.ends_with('\n') && missing_else(source) {
        return true;
    }
    // Indentation has no closing delimiter. A blank line ends manual entry;
    // complete pasted text ending with a newline is already ready to submit.
    if source.ends_with('\n') {
        return false;
    }
    // Match arms may align with `match` rather than opening an indented
    // scope. Keep manual arm entry open until the same blank-line terminator.
    if source.lines().count() > 1
        && source
            .lines()
            .last()
            .is_some_and(|line| match_arm(source, line))
    {
        return true;
    }
    let Ok(lexed) = aevia_syntax::lex(0, source) else {
        return false;
    };
    let mut indentation = 0usize;
    for token in lexed.tokens {
        match token.kind {
            | TokenKind::Indent => indentation += 1,
            // EOF closes layout synthetically. Only physical dedents matter.
            | TokenKind::Dedent if (token.span.start as usize) < source.len() => {
                indentation = indentation.saturating_sub(1);
            },
            | _ => {},
        }
    }
    indentation > 0
}

/// Whether a submission starts a declaration rather than an expression.
/// Use lexer tokens, so nested comments, documentation, tabs between tokens,
/// and line breaks do not change how a program is classified.
pub fn declaration(source: &str) -> bool {
    match aevia_syntax::lex(0, source) {
        | Ok(lexed) => declaration_tokens(source, &lexed.tokens),
        | Err(diagnostics) => {
            // A lexical error in a declaration's body must not make the REPL
            // wrap that declaration as an expression. Its valid prefix normally
            // already contains enough tokens to identify the submission.
            let limit = diagnostics
                .items
                .iter()
                .map(|d| d.span.start as usize)
                .min()
                .unwrap_or(0);
            if let Ok(lexed) = aevia_syntax::lex(0, &source[..limit]) {
                if lexed.tokens.iter().any(meaningful) {
                    return declaration_tokens(source, &lexed.tokens);
                }
            }
            // Invalid indentation can precede the first token. Trim only for
            // classification; the real compiler still diagnoses the source.
            let trimmed = source.trim_start();
            if trimmed.len() != source.len() {
                return declaration(trimmed);
            }
            let end = trimmed
                .find(|ch: char| !ch.is_alphanumeric() && ch != '_')
                .unwrap_or(trimmed.len());
            let word = &trimmed[..end];
            if word == "fn" {
                return !trimmed[end..].trim_start().starts_with('(')
                    && !trimmed[end..]
                        .find("=>")
                        .is_some_and(|arrow| !trimmed[end..][..arrow].contains('='));
            }
            matches!(
                Keyword::from_word(word),
                Some(
                    Keyword::Let
                        | Keyword::Type
                        | Keyword::Unit
                        | Keyword::Dimension
                        | Keyword::Import
                        | Keyword::Open
                        | Keyword::Test
                        | Keyword::Class
                        | Keyword::Instance
                        | Keyword::Derive
                        | Keyword::Module
                        | Keyword::Private
                )
            ) || trimmed.starts_with('@')
                || trimmed.starts_with('#')
                || trimmed.is_empty()
        },
    }
}

fn meaningful(token: &aevia_syntax::Token) -> bool {
    !matches!(
        token.kind,
        TokenKind::Newline | TokenKind::Indent | TokenKind::Dedent | TokenKind::Eof
    )
}

fn declaration_tokens(
    source: &str,
    tokens: &[aevia_syntax::Token],
) -> bool {
    let mut tokens = tokens.iter().filter(|token| meaningful(token));
    let Some(first) = tokens.next() else {
        return true;
    };
    match first.kind {
        | TokenKind::Keyword(Keyword::Fn) => {
            // Inspect the source after the actual fn token, not its physical
            // line: comments and arbitrary whitespace can precede the lambda's
            // parameter list. Named declarations encounter '=' before any
            // lambda arrow in their body.
            let tail = &source[first.span.end as usize..];
            if tail.trim_start().starts_with('(') {
                return false;
            }
            let mut first_parameter = true;
            for token in tokens {
                if first_parameter && matches!(token.kind, TokenKind::Punct(Punct::LParen)) {
                    return false;
                }
                first_parameter = false;
                match token.kind {
                    | TokenKind::Punct(Punct::Equals) => return true,
                    | TokenKind::Punct(Punct::FatArrow) => return false,
                    | _ => {},
                }
            }
            true
        },
        | TokenKind::Keyword(
            Keyword::Let
            | Keyword::Type
            | Keyword::Unit
            | Keyword::Dimension
            | Keyword::Import
            | Keyword::Open
            | Keyword::Test
            | Keyword::Class
            | Keyword::Instance
            | Keyword::Derive
            | Keyword::Module
            | Keyword::Private,
        )
        | TokenKind::Punct(Punct::At) => true,
        | _ => false,
    }
}

/// An unindented match arm belongs to the match immediately before it.
/// Token checks avoid confusing a pipe operator or a quoted `match` with an arm.
fn match_arm(
    source: &str,
    line: &str,
) -> bool {
    let Ok(arm) = aevia_syntax::lex(0, line) else {
        return false;
    };
    if !arm
        .tokens
        .iter()
        .find(|token| {
            !matches!(
                token.kind,
                TokenKind::Newline | TokenKind::Indent | TokenKind::Dedent | TokenKind::Eof
            )
        })
        .is_some_and(|token| matches!(token.kind, TokenKind::Punct(Punct::Pipe)))
    {
        return false;
    }
    aevia_syntax::lex(0, source).is_ok_and(|lexed| {
        lexed
            .tokens
            .iter()
            .any(|token| matches!(token.kind, TokenKind::Keyword(Keyword::Match)))
    })
}

/// Move standalone trailing comments to the next submission. The lexer tells
/// us where the last real token ends, so nested block comments work too; inline
/// comments stay with the line they describe.
fn trailing_comments(source: &mut String) -> String {
    let Ok(lexed) = aevia_syntax::lex(0, source) else {
        return String::new();
    };
    let Some(last) = lexed.tokens.iter().rev().find(|token| {
        !matches!(
            token.kind,
            TokenKind::Newline | TokenKind::Indent | TokenKind::Dedent | TokenKind::Eof
        )
    }) else {
        return String::new();
    };
    let end = last.span.end as usize;
    let Some(newline) = source[end..].find('\n') else {
        return String::new();
    };
    source.split_off(end + newline + 1)
}

/// Splits an unbracketed input stream into submissions. Adjacent declarations
/// stay together, allowing forward references and complete programs with main;
/// expressions remain separate so `let x = 2\nx + 1\nx + 2` is useful too.
/// Colon commands always form a boundary. Explicit paste blocks are atomic.
pub fn split_submissions(source: &str) -> Vec<String> {
    let mut units = Vec::new();
    let mut current = String::new();
    let mut paste = false;
    let mut pasted = String::new();
    for line in source.lines() {
        let trimmed = line.trim();
        if paste {
            if trimmed == ":end" && !inside_paste_literal(&pasted) {
                units.push(std::mem::take(&mut pasted));
                paste = false;
            } else if trimmed == ":cancel" && !inside_paste_literal(&pasted) {
                pasted.clear();
                paste = false;
            } else {
                pasted.push_str(line);
                pasted.push('\n');
            }
            continue;
        }
        let top = !line.starts_with(char::is_whitespace) && !trimmed.is_empty();
        if trimmed.starts_with(':') && !lexical_continuation(&current) {
            if !current.trim().is_empty() {
                units.push(std::mem::take(&mut current));
            }
            if trimmed == ":paste" {
                paste = true;
            } else {
                units.push(line.to_string());
            }
            continue;
        }
        let comment = trimmed.starts_with('#');
        let content = current
            .lines()
            .any(|line| !line.trim().is_empty() && !line.trim_start().starts_with('#'));
        if top
            && !comment
            && content
            && !lexical_continuation(&current)
            && !missing_else(&current)
            && !match_arm(&current, line)
        {
            // The next top-level expression or declaration starts a new unit.
            // Consecutive declarations are coalesced below after parsing units.
            let comments = trailing_comments(&mut current);
            units.push(std::mem::take(&mut current));
            current = comments;
        }
        current.push_str(line);
        current.push('\n');
    }
    if paste {
        // The caller receives a command with an actionable error instead of
        // accidentally evaluating an unfinished paste at end of input.
        units.push(":paste-unfinished".to_string());
    } else if !current.trim().is_empty() {
        units.push(current);
    }
    let mut grouped: Vec<String> = Vec::new();
    for unit in units {
        if !unit.trim_start().starts_with(':')
            && declaration(&unit)
            && let Some(previous) = grouped.last_mut()
            && !previous.trim_start().starts_with(':')
            && declaration(previous)
        {
            previous.push_str(&unit);
        } else {
            grouped.push(unit);
        }
    }
    grouped
}

#[cfg(test)]
mod tests {
    use super::*;

    fn without_ansi(styled: &str) -> String {
        let mut output = String::new();
        let mut bytes = styled.chars().peekable();
        while let Some(ch) = bytes.next() {
            if ch == '\x1b' && bytes.peek() == Some(&'[') {
                bytes.next();
                for next in bytes.by_ref() {
                    if next == 'm' {
                        break;
                    }
                }
            } else {
                output.push(ch);
            }
        }
        output
    }

    #[test]
    fn disabled_highlighting_preserves_plain_input_prompt_and_hints() {
        // Input receives the resolved --color/NO_COLOR policy from the CLI.
        let helper = ReplHelper::new(false);
        let source = "let π: Real = 3.14 # circle";
        assert_eq!(helper.highlight(source, source.len()), source);
        assert_eq!(helper.highlight_prompt("aevia> ", true), "aevia> ");
        assert_eq!(helper.highlight_hint("next input"), "next input");
        assert!(!helper.highlight_char(source, source.len(), CmdKind::Other));
    }

    #[test]
    fn syntax_colors_preserve_unicode_and_spacing_and_style_comments() {
        let helper = ReplHelper::new(true);
        let source = "let π: Real = 3.14\nlet 中文 = \"λ # text\" # 注释";
        let styled = helper.highlight(source, source.len());
        assert_eq!(without_ansi(&styled), source);
        assert!(styled.contains("\x1b[1;35mlet\x1b[0m"));
        assert!(styled.contains("\x1b[36mReal\x1b[0m"));
        assert!(styled.contains("\x1b[33m3.14\x1b[0m"));
        assert!(styled.contains("\x1b[32m\"λ # text\"\x1b[0m"));
        assert!(styled.contains("\x1b[2;90m# 注释\x1b[0m"));
        let source = "## docs\n#- outer #- inner -# comment -#\nlet x = 1";
        assert_eq!(
            without_ansi(&helper.highlight(source, source.len())),
            source
        );
    }

    #[test]
    fn matching_brackets_keep_priority_over_syntax_styles() {
        let helper = ReplHelper::new(true);
        let source = "[π, 42]";
        assert!(helper.highlight_char(source, source.len(), CmdKind::MoveCursor));
        let styled = helper.highlight(source, source.len());
        assert_eq!(without_ansi(&styled), source);
        assert!(styled.contains("\x1b[1;34m[\x1b[0m"));
        assert!(styled.contains("\x1b[33m42\x1b[0m"));
    }

    #[test]
    fn conditional_continuation_requires_only_eof_errors() {
        assert!(incomplete("if true then 1"));
        assert!(incomplete("let x = if true then 1"));
        assert!(incomplete("1 + if true then 2"));
        assert!(!incomplete("if true then 1\n"));
        assert!(!incomplete("if true then @"));
        assert!(!incomplete("if then 1"));
        assert!(!incomplete("if true 1"));
        assert!(!incomplete("if true then 1 else 2"));
        let units = split_submissions("let x = if true then 1\nelse 2\nx + 3\n");
        assert_eq!(units.len(), 2, "{units:?}");
        assert!(units[0].contains("else 2"));
    }

    #[test]
    fn delimiters_and_operators_continue_without_confusing_comments_or_strings() {
        assert!(incomplete("let xs = [1,\n2,"));
        assert!(incomplete("1 +"));
        assert!(incomplete("fn f() = # an explanation"));
        assert!(incomplete("#- nested #- comment -#"));
        assert!(!incomplete("\"a [ bracket\" # ( comment"));
        assert!(!incomplete("r\"a [ bracket\""));
        assert!(!incomplete("[1, 2)"));
        assert!(!incomplete("let undone = \"oops"));
        assert!(!incomplete(":type map"));
    }

    #[test]
    fn manual_indentation_waits_for_blank_line() {
        assert!(incomplete("fn f() ="));
        assert!(incomplete("fn f() =\n    1"));
        assert!(!incomplete("fn f() =\n    1\n"));
        assert!(!incomplete("[1,\n 2]"));
    }

    #[test]
    fn piped_declarations_stay_together_and_expressions_run_separately() {
        let units = split_submissions(
            "fn main(): Int = helper()\nfn helper(): Int = 42\n:run\nlet x = 2\nx + 1\nx + 2\n",
        );
        assert_eq!(units.len(), 5, "{units:?}");
        assert!(units[0].contains("helper():"));
        assert_eq!(units[1], ":run");
        assert_eq!(units[3].trim(), "x + 1");
    }

    #[test]
    fn piped_indented_blocks_and_bracketed_expressions_are_whole() {
        let units = split_submissions(
            "fn main(): IO Unit = do\n    println(\"a\")\n    println(\"b\")\n[1,\n2]\n:type main\n",
        );
        assert_eq!(units.len(), 3, "{units:?}");
        assert!(units[0].contains("println(\"b\")"));
        assert!(units[1].contains("2]"));
    }

    #[test]
    fn unindented_match_arms_are_one_submission() {
        let source = "match true with\n| true -> 1\n| false -> 2\n3 + 4\n";
        let units = split_submissions(source);
        assert_eq!(units.len(), 2, "{units:?}");
        assert!(units[0].contains("| false -> 2"));
        assert_eq!(units[1].trim(), "3 + 4");
        assert!(incomplete("match true with\n| true -> 1"));
        assert!(!incomplete("match true with\n| true -> 1\n"));
        let units = split_submissions(
            "fn f(x: Bool): Int = match x with\n| true -> 1\n| false -> 2\nfn main(): Int = f(true)\n",
        );
        assert_eq!(units.len(), 1, "{units:?}");
        assert!(units[0].contains("| false -> 2\nfn main"));
    }

    #[test]
    fn comments_move_to_the_following_submission_and_keep_original_order() {
        let source = "1 + 1 # inline\n## The next declaration.\n#- a nested #- comment -# here -#\nlet next = 3\n# Another declaration.\nlet after = next + 1\nnext + after\n";
        let units = split_submissions(source);
        assert_eq!(units.len(), 3, "{units:?}");
        assert_eq!(units[0], "1 + 1 # inline\n");
        assert!(units[1].starts_with("## The next declaration."));
        assert!(units[1].contains("# Another declaration.\nlet after"));
        assert_eq!(units.concat(), source);
    }

    #[test]
    fn explicit_paste_of_multiple_declarations_is_one_submission() {
        let units = split_submissions(
            ":paste\n## A program.\nfn main(): Int = helper()\n\n# Forward declaration.\nfn helper(): Int = 42\n:end\n",
        );
        assert_eq!(units.len(), 1, "{units:?}");
        assert!(units[0].contains("\n\n# Forward declaration."));
        assert_eq!(split_submissions(&units[0]), units);
    }

    #[test]
    fn qualified_and_module_completions_preserve_the_whole_path() {
        let mut helper = ReplHelper::new(false);
        helper.bindings = vec![
            "Math.Stats.mean".into(),
            "Math.sin".into(),
            "中文.工具.计算".into(),
            "local".into(),
        ];
        let history = DefaultHistory::new();
        let context = Context::new(&history);
        let (start, names) = helper.complete("Math.St", 7, &context).unwrap();
        assert_eq!(start, 0);
        assert!(names.iter().any(|name| name.replacement == "Math.Stats"));
        let (start, names) = helper.complete("import Math.St", 14, &context).unwrap();
        assert_eq!(start, 7);
        assert_eq!(
            names
                .iter()
                .map(|name| name.replacement.as_str())
                .collect::<Vec<_>>(),
            ["Math.Stats"]
        );
        let (_, names) = helper.complete("open Math.", 10, &context).unwrap();
        assert!(names.iter().all(|name| name.replacement != "Math.sin"));
        let (start, names) = helper
            .complete("中文.工具.计", "中文.工具.计".len(), &context)
            .unwrap();
        assert_eq!(start, 0);
        assert!(
            names
                .iter()
                .any(|name| name.replacement == "中文.工具.计算")
        );
    }

    #[test]
    fn explicit_paste_markers_inside_multiline_literals_are_source() {
        let source = ":paste\nlet text = \"\"\"\n:end\n:cancel\n\"\"\"\n#- block\n:end\n:cancel\n-#\nlet other = 1\n:end\n:quit\n";
        let units = split_submissions(source);
        assert_eq!(units.len(), 2, "{units:?}");
        assert!(units[0].contains("\n:end\n:cancel\n"));
        assert!(units[0].contains("let other = 1"));
        assert_eq!(units[1], ":quit");
        assert_eq!(
            split_submissions(":paste\nlet text = \"\"\"\n:end"),
            [":paste-unfinished"]
        );
    }

    #[test]
    fn declaration_classification_uses_tokens_after_comments_and_tabs() {
        let comment = "#- outer\nnot source code\n#- nested -#\n-#\n";
        assert!(declaration(&format!("{comment}let value = 1")));
        assert!(declaration(&format!(
            "{comment}fn\tidentity(x: Int): Int = x"
        )));
        assert!(declaration(&format!("{comment}let value = \"unfinished")));
        assert!(declaration("\tfn\tidentity(x: Int): Int = x"));
        assert!(declaration("## docs\n@inline\nfn f(): Int = 1"));
        assert!(!declaration(&format!("{comment}1 + 2")));
        assert!(!declaration(&format!("{comment}fn\t(x: Int) => x")));
        assert!(!declaration("fn #- an = in a comment -# x => x"));
        assert!(!declaration("fn #- comment -# (x: Int) => x"));
        let source = format!("{comment}fn\tmain(): Int = helper()\nfn\thelper(): Int = 42\n");
        assert_eq!(split_submissions(&source), vec![source]);
    }

    #[test]
    fn anonymous_functions_remain_separate_expressions() {
        let units = split_submissions("let x = 2\nfn (y: Int) => y + x\nfn z => z\n");
        assert_eq!(units.len(), 3, "{units:?}");
        assert!(!declaration(&units[1]));
        assert!(!declaration(&units[2]));
    }

    #[test]
    fn explicit_paste_preserves_empty_lines_and_commands_until_end() {
        let units = split_submissions(":paste\nfn f() =\n    2\n\nfn g() = f()\n:end\n:quit\n");
        assert_eq!(units.len(), 2, "{units:?}");
        assert!(units[0].contains("\n\nfn g"));
        assert_eq!(units[1], ":quit");
        assert_eq!(
            split_submissions(":paste\nlet x = 1\n:cancel\n:quit"),
            vec![":quit"]
        );
    }
}
