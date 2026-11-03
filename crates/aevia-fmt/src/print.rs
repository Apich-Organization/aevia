//! The printer: AST in, source text out.
//!
//! Layout is part of the grammar, so this is not a "pretty printer" that can be
//! approximate: every construct is emitted in a shape the parser reads back to
//! the same tree. Precedence is recomputed from the tree (brackets are not in
//! the AST), and the forms that only exist where a line ends — `do`, a layout
//! block, `match` — are refused anywhere else rather than printed into
//! something that parses differently.
//!
//! [`crate::format_source`] re-parses the output and compares fingerprints, so
//! a gap here is a reported failure, never a silent rewrite.

use crate::comments::Comment;
use aevia_syntax::ast::*;
use std::fmt::Write as _;

/// The four spaces one level of layout is indented by.
const UNIT: &str = "    ";

// Binding powers, from the parser's expression ladder.
const TAIL: u8 = 0;
const PIPE: u8 = 1;
const OR: u8 = 2;
const AND: u8 = 3;
const CMP: u8 = 4;
const CONS: u8 = 5;
const ADD: u8 = 6;
const MUL: u8 = 7;
const UNARY: u8 = 8;
const POW: u8 = 9;
const POSTFIX: u8 = 10;
const ATOM: u8 = 11;

/// Where an expression is being printed.
#[derive(Clone, Copy)]
struct Ctx {
    /// The minimum binding power this position accepts without brackets.
    prec: u8,
    /// True when a layout block may start here (after `=`, `->`, `then`, …).
    tail: bool,
    /// True when a bracket delimits the expression, so a greedy form is safe.
    bracketed: bool,
}

impl Ctx {
    const fn tail() -> Self {
        Self {
            prec: TAIL,
            tail: true,
            bracketed: false,
        }
    }

    const fn operand(prec: u8) -> Self {
        Self {
            prec,
            tail: false,
            bracketed: false,
        }
    }

    const fn bracketed() -> Self {
        Self {
            prec: TAIL,
            tail: false,
            bracketed: true,
        }
    }
}

/// Prints a module, given the comments to interleave.
pub struct Printer {
    out: String,
    indent: usize,
    at_line_start: bool,
    comments: Vec<Comment>,
    next_comment: usize,
    error: Option<String>,
    /// The declaration being printed, for the failure message.
    subject: String,
}

impl Printer {
    /// Creates a printer for `comments`, which must be in source order.
    #[must_use]
    pub fn new(comments: Vec<Comment>) -> Self {
        Self {
            out: String::new(),
            indent: 0,
            at_line_start: true,
            comments,
            next_comment: 0,
            error: None,
            subject: "the file".to_string(),
        }
    }

    /// Prints `module`.
    ///
    /// # Errors
    ///
    /// The description of the construct that cannot be reproduced.
    pub fn module(mut self, module: &Module) -> Result<String, String> {
        if let Some(name) = &module.name {
            self.line(&format!("module {}", name.join(".")));
            self.blank();
        }
        for (index, decl) in module.decls.iter().enumerate() {
            if index > 0 {
                self.blank();
            }
            self.decl(decl);
        }
        // Whatever is left belongs to the end of the file.
        self.flush_comments(usize::MAX);
        if let Some(error) = self.error {
            return Err(error);
        }
        Ok(self.out)
    }

    /// Prints one expression, for a caller that has no module.
    ///
    /// # Errors
    ///
    /// The description of the construct that cannot be reproduced.
    pub fn expression(mut self, expr: &Expr) -> Result<String, String> {
        self.expr(expr, Ctx::tail());
        if let Some(error) = self.error {
            return Err(error);
        }
        Ok(self.out)
    }

    /// Prints one type, for a caller that has no module.
    ///
    /// # Errors
    ///
    /// The description of the construct that cannot be reproduced.
    pub fn type_(mut self, ty: &Ty) -> Result<String, String> {
        self.ty(ty, false);
        if let Some(error) = self.error {
            return Err(error);
        }
        Ok(self.out)
    }

    // ----- low-level output ----------------------------------------------

    fn push(&mut self, text: &str) {
        if text.is_empty() {
            return;
        }
        if self.at_line_start {
            for _ in 0..self.indent {
                self.out.push_str(UNIT);
            }
            self.at_line_start = false;
        }
        self.out.push_str(text);
    }

    fn newline(&mut self) {
        // Never leave trailing spaces on a line.
        while self.out.ends_with(' ') {
            self.out.pop();
        }
        self.out.push('\n');
        self.at_line_start = true;
    }

    /// Writes `" " + text`, or just `text` at the start of a line.
    ///
    /// A block ends at the start of a line, so `else` after one must not be
    /// written with a leading space — that would be an indentation level.
    fn push_after(&mut self, text: &str) {
        if !self.at_line_start {
            self.push(" ");
        }
        self.push(text);
    }

    fn end_line(&mut self) {
        if !self.at_line_start {
            self.newline();
        }
    }

    fn line(&mut self, text: &str) {
        self.push(text);
        self.newline();
    }

    fn blank(&mut self) {
        self.end_line();
        if !self.out.is_empty() && !self.out.ends_with("\n\n") {
            self.out.push('\n');
        }
        self.at_line_start = true;
    }

    fn fail(&mut self, reason: impl Into<String>) {
        if self.error.is_none() {
            let subject = self.subject.clone();
            self.error = Some(format!("{subject}: {}", reason.into()));
        }
    }

    /// Emits every comment that starts before `offset`.
    fn flush_comments(&mut self, offset: usize) {
        while let Some(comment) = self.comments.get(self.next_comment) {
            if comment.start >= offset {
                break;
            }
            let indent = UNIT.repeat(self.indent);
            let text = comment.render(&indent);
            self.push(&text);
            self.end_line();
            self.next_comment += 1;
        }
    }

    // ----- declarations ---------------------------------------------------

    fn decl(&mut self, decl: &Decl) {
        self.flush_comments(usize::try_from(decl.span.start).unwrap_or(0));
        self.subject = describe(decl);
        if let Some(doc) = &decl.doc {
            for line in doc.lines() {
                if line.trim().is_empty() {
                    self.line("##");
                } else {
                    self.line(&format!("## {}", line.trim_end()));
                }
            }
        }
        for attr in &decl.attrs {
            let mut text = format!("@{}", attr.name);
            if !attr.args.is_empty() {
                text.push('(');
                for (index, arg) in attr.args.iter().enumerate() {
                    if index > 0 {
                        text.push_str(", ");
                    }
                    if let Some(name) = &arg.name {
                        let _ = write!(text, "{name} = ");
                    }
                    match Printer::new(Vec::new()).expression(&arg.value) {
                        Ok(rendered) => text.push_str(&rendered),
                        Err(error) => {
                            self.fail(format!(
                                "an attribute argument cannot be printed: {error}"
                            ));
                            return;
                        }
                    }
                }
                text.push(')');
            }
            self.line(&text);
        }
        if decl.private {
            self.push("private ");
        }
        match &decl.kind {
            DeclKind::Import {
                path,
                alias,
                names,
                open,
            } => {
                let mut text = format!(
                    "{} {}",
                    if *open { "open" } else { "import" },
                    path.join(".")
                );
                if let Some(alias) = alias {
                    let _ = write!(text, " as {alias}");
                }
                if let Some(names) = names {
                    let _ = write!(text, " {{ {} }}", names.join(", "));
                }
                self.line(&text);
            }
            DeclKind::Dimension { name, value } => {
                let mut text = format!("dimension {name}");
                if let Some(value) = value {
                    let _ = write!(text, " = {}", value.to_source());
                }
                self.line(&text);
            }
            DeclKind::Unit {
                name,
                dim,
                scale,
                base,
                offset,
                prefixable,
            } => {
                let mut text = format!("unit {name} : {} = ", dim.to_source());
                if !scale_into(&mut text, scale) {
                    self.fail("this unit scale has no source form");
                    return;
                }
                if let Some(base) = base {
                    text.push(' ');
                    text.push_str(&base.to_source());
                }
                if let Some((scale, unit)) = offset {
                    text.push_str(" offset ");
                    if !scale_into(&mut text, scale) {
                        self.fail("this unit offset has no source form");
                        return;
                    }
                    if let Some(unit) = unit {
                        text.push(' ');
                        text.push_str(&unit.to_source());
                    }
                }
                if *prefixable {
                    text.push_str(" prefixable");
                }
                self.line(&text);
            }
            DeclKind::Type { name, params, body } => {
                let mut text = format!("type {name}{}", type_params(params));
                match body {
                    TypeBody::Alias(ty) => {
                        let rendered = self.ty_text(ty);
                        let _ = write!(text, " = {rendered}");
                    }
                    TypeBody::Variants(variants) => {
                        text.push_str(" =");
                        for variant in variants {
                            let _ = write!(text, " | {}", variant.name);
                            if !variant.args.is_empty() {
                                let args = variant
                                    .args
                                    .iter()
                                    .map(|arg| self.ty_text(arg))
                                    .collect::<Vec<_>>()
                                    .join(", ");
                                let _ = write!(text, "({args})");
                            }
                        }
                    }
                    TypeBody::Abstract => {}
                }
                self.line(&text);
            }
            DeclKind::Class {
                name,
                params,
                methods,
            } => {
                let names = params
                    .iter()
                    .map(|param| param.name.clone())
                    .collect::<Vec<_>>()
                    .join(" ");
                let mut text = format!("class {name}");
                if !names.is_empty() {
                    let _ = write!(text, " {names}");
                }
                text.push_str(" where");
                self.line(&text);
                self.indent += 1;
                for (method, ty) in methods {
                    let rendered = self.ty_text(ty);
                    self.line(&format!("{method}: {rendered}"));
                }
                self.indent -= 1;
            }
            DeclKind::Instance {
                class,
                ty,
                methods,
            } => {
                let head = self.ty_text(ty);
                let mut text = format!("instance {class} for {head}");
                if !methods.is_empty() {
                    text.push_str(" where");
                }
                self.line(&text);
                self.indent += 1;
                for method in methods {
                    let params = self.params(&method.params);
                    let mut text = format!("{}({params})", method.name);
                    if let Some(ret) = &method.ret {
                        let rendered = self.ty_text(ret);
                        let _ = write!(text, ": {rendered}");
                    }
                    text.push_str(" =");
                    self.push(&text);
                    self.body(&method.body);
                }
                self.indent -= 1;
            }
            DeclKind::Derive { classes, ty } => {
                let head = self.ty_text(ty);
                self.line(&format!("derive {} for {head}", classes.join(", ")));
            }
            DeclKind::Fn {
                name,
                params,
                args,
                ret,
                body,
            } => {
                let mut text = format!("fn {name}{}", type_params(params));
                let args = self.params(args);
                let _ = write!(text, "({args})");
                if let Some(ret) = ret {
                    let rendered = self.ty_text(ret);
                    let _ = write!(text, ": {rendered}");
                }
                text.push_str(" =");
                self.push(&text);
                self.body(body);
            }
            DeclKind::Let {
                pattern,
                ty,
                value,
                rec,
            } => {
                let mut text = if *rec {
                    "let rec ".to_string()
                } else {
                    "let ".to_string()
                };
                let bound = self.pattern_text(pattern);
                text.push_str(&bound);
                if let Some(ty) = ty {
                    let rendered = self.ty_text(ty);
                    let _ = write!(text, ": {rendered}");
                }
                text.push_str(" =");
                self.push(&text);
                self.body(value);
            }
            DeclKind::Test { name, body } => {
                self.push(&format!("test {} =", quote(name)));
                self.body(body);
            }
        }
    }

    /// Prints the right-hand side of `=`: inline, or as an indented block.
    ///
    /// A layout block and a `match` both read as blocks, so both start on the
    /// next line; everything else follows the `=` on the same line.
    fn body(&mut self, expr: &Expr) {
        match &expr.kind {
            ExprKind::Block { .. } => {
                self.expr(expr, Ctx::tail());
                self.end_line();
            }
            ExprKind::Match { .. } => {
                self.end_line();
                self.indent += 1;
                self.expr(expr, Ctx::tail());
                self.end_line();
                self.indent -= 1;
            }
            _ => {
                self.push(" ");
                self.expr(expr, Ctx::tail());
                self.end_line();
            }
        }
    }

    fn params(&mut self, params: &[Param]) -> String {
        let mut out = Vec::new();
        for param in params {
            let mut text = self.pattern_text(&param.pattern);
            if let Some(ty) = &param.ty {
                let rendered = self.ty_text(ty);
                let _ = write!(text, ": {rendered}");
            }
            out.push(text);
        }
        out.join(", ")
    }

    fn pattern_text(&mut self, pat: &Pat) -> String {
        let mut printer = Printer::new(Vec::new());
        printer.pattern(pat);
        if let Some(error) = printer.error {
            self.fail(error);
        }
        printer.out
    }

    /// A type as text, failing the whole print when it has no source form.
    fn ty_text(&mut self, ty: &Ty) -> String {
        let mut printer = Printer::new(Vec::new());
        printer.ty(ty, false);
        if let Some(error) = printer.error {
            self.fail(error);
        }
        printer.out
    }

    // ----- expressions ----------------------------------------------------

    fn expr(&mut self, expr: &Expr, cx: Ctx) {
        if self.error.is_some() {
            return;
        }
        let (prec, open) = precedence(&expr.kind);
        let needs_brackets =
            prec < cx.prec || (open && !cx.tail && !cx.bracketed);
        if needs_brackets {
            if matches!(
                expr.kind,
                ExprKind::Block { .. } | ExprKind::Do { .. } | ExprKind::Match { .. }
            ) {
                let what = match expr.kind {
                    ExprKind::Block { .. } => "a layout block",
                    ExprKind::Do { .. } => "a `do` block",
                    _ => "a `match`",
                };
                self.fail(format!(
                    "{what} can only be printed where a line ends, not inside an expression"
                ));
                return;
            }
            self.push("(");
            self.expr(expr, Ctx::tail());
            self.push(")");
            return;
        }
        match &expr.kind {
            ExprKind::UnitLit => self.push("()"),
            ExprKind::Error => self.fail("the parser reported an error here"),
            ExprKind::Bool(value) => self.push(if *value { "true" } else { "false" }),
            ExprKind::Int(value) => self.push(&value.to_string()),
            ExprKind::Float(text) => self.push(text),
            ExprKind::Str(pieces) => self.string(pieces),
            ExprKind::Quantity { value, unit } => {
                match value {
                    NumberLit::Int(value) => self.push(&value.to_string()),
                    NumberLit::Decimal(text) => self.push(text),
                }
                self.push(" ");
                self.push(&unit.to_source());
            }
            ExprKind::Var(name) => self.push(name),
            ExprKind::QName(path) => self.push(&path.join(".")),
            ExprKind::Unary { op, operand } => {
                self.push(match op {
                    UnOp::Neg => "-",
                    UnOp::Pos => "+",
                    UnOp::Not => "!",
                    UnOp::Sqrt => "\u{221a}",
                });
                self.expr(operand, Ctx::operand(UNARY));
            }
            ExprKind::Binary { op, left, right } => {
                let prec = binary_prec(*op);
                let (left_prec, right_prec) = if *op == BinOp::Pow {
                    (POSTFIX, UNARY)
                } else if is_right_associative(*op) {
                    (prec + 1, prec)
                } else {
                    (prec, prec + 1)
                };
                self.expr(left, Ctx::operand(left_prec));
                let separator = format!(" {} ", op.as_str());
                self.push(&separator);
                self.expr(right, Ctx::operand(right_prec));
            }
            ExprKind::Call { callee, args } => {
                self.expr(callee, Ctx::operand(POSTFIX));
                self.push("(");
                for (index, arg) in args.iter().enumerate() {
                    if index > 0 {
                        self.push(", ");
                    }
                    self.expr(arg, Ctx::bracketed());
                }
                self.push(")");
            }
            ExprKind::TypeApp { callee, args } => {
                self.expr(callee, Ctx::operand(POSTFIX));
                self.push("<");
                for (index, arg) in args.iter().enumerate() {
                    if index > 0 {
                        self.push(", ");
                    }
                    let text = self.ty_text(arg);
                    self.push(&text);
                }
                self.push(">");
            }
            ExprKind::Lambda { params, ret, body } => {
                let bare = params.len() == 1
                    && params[0].ty.is_none()
                    && !matches!(params[0].pattern.kind, PatKind::Tuple(_) | PatKind::Annot { .. });
                if bare {
                    // `fn t => …` is the form the corpus writes; `fn (t) => …`
                    // is the same tree, but a bare parameter list is not.
                    let pattern = self.pattern_text(&params[0].pattern);
                    self.push("fn ");
                    self.push(&pattern);
                } else {
                    let params = self.params(params);
                    self.push("fn (");
                    self.push(&params);
                    self.push(")");
                }
                if let Some(ret) = ret {
                    let rendered = self.ty_text(ret);
                    let text = format!(": {rendered}");
                    self.push(&text);
                }
                self.push(" => ");
                self.expr(body, Ctx::tail());
            }
            ExprKind::If {
                cond,
                then_branch,
                else_branch,
            } => {
                self.push("if ");
                self.expr(cond, Ctx::operand(TAIL));
                self.push(" then");
                self.branch(then_branch);
                self.push_after("else");
                self.branch(else_branch);
            }
            ExprKind::Match { scrutinee, arms } => {
                self.push("match ");
                self.expr(scrutinee, Ctx::operand(TAIL));
                self.push(" with");
                self.end_line();
                for arm in arms {
                    self.flush_comments(usize::try_from(arm.span.start).unwrap_or(0));
                    let pattern = self.pattern_text(&arm.pattern);
                    self.push("| ");
                    self.push(&pattern);
                    if let Some(guard) = &arm.guard {
                        self.push(" if ");
                        self.expr(guard, Ctx::operand(TAIL));
                    }
                    self.push(" ->");
                    self.body(&arm.body);
                }
                self.flush_comments(usize::try_from(expr.span.end).unwrap_or(usize::MAX));
            }
            ExprKind::LetIn {
                pattern,
                ty,
                value,
                body,
            } => {
                let bound = self.pattern_text(pattern);
                self.push("let ");
                self.push(&bound);
                if let Some(ty) = ty {
                    let rendered = self.ty_text(ty);
                    let text = format!(": {rendered}");
                    self.push(&text);
                }
                self.push(" = ");
                self.expr(value, Ctx::operand(TAIL));
                self.push_after("in");
                self.branch(body);
            }
            ExprKind::Block { stmts, tail } => {
                self.end_line();
                self.indent += 1;
                for stmt in stmts {
                    self.flush_comments(block_stmt_start(stmt));
                    match stmt {
                        BlockStmt::Let {
                            pattern,
                            ty,
                            value,
                            rec,
                            ..
                        } => {
                            let mut text = if *rec {
                                "let rec ".to_string()
                            } else {
                                "let ".to_string()
                            };
                            let bound = self.pattern_text(pattern);
                            text.push_str(&bound);
                            if let Some(ty) = ty {
                                let rendered = self.ty_text(ty);
                                let _ = write!(text, ": {rendered}");
                            }
                            text.push_str(" =");
                            self.push(&text);
                            self.body(value);
                        }
                        BlockStmt::Fn(decl) => self.decl(decl),
                    }
                }
                if let Some(tail) = tail {
                    self.expr(tail, Ctx::tail());
                    self.end_line();
                }
                self.flush_comments(usize::try_from(expr.span.end).unwrap_or(usize::MAX));
                self.indent -= 1;
            }
            ExprKind::Do { stmts } => {
                self.push("do");
                self.end_line();
                self.indent += 1;
                for stmt in stmts {
                    self.flush_comments(do_stmt_start(stmt));
                    match stmt {
                        DoStmt::Let {
                            pattern,
                            ty,
                            value,
                            ..
                        } => {
                            let bound = self.pattern_text(pattern);
                            let mut text = format!("let {bound}");
                            if let Some(ty) = ty {
                                let rendered = self.ty_text(ty);
                                let _ = write!(text, ": {rendered}");
                            }
                            text.push_str(" =");
                            self.push(&text);
                            self.body(value);
                        }
                        DoStmt::Bind { pattern, value, .. } => {
                            // The source form is `p <- action`; the `let` is
                            // only for a pure binding.
                            let bound = self.pattern_text(pattern);
                            let text = format!("{bound} <- ");
                            self.push(&text);
                            self.expr(value, Ctx::operand(TAIL));
                            self.end_line();
                        }
                        DoStmt::Expr(expr) => {
                            self.expr(expr, Ctx::tail());
                            self.end_line();
                        }
                    }
                }
                self.flush_comments(usize::try_from(expr.span.end).unwrap_or(usize::MAX));
                self.indent -= 1;
            }
            ExprKind::Record { base, fields } => {
                self.push("{ ");
                if let Some(base) = base {
                    self.expr(base, Ctx::bracketed());
                    self.push(" with ");
                }
                for (index, (name, value)) in fields.iter().enumerate() {
                    if index > 0 {
                        self.push(", ");
                    }
                    self.push(name);
                    self.push(" = ");
                    self.expr(value, Ctx::bracketed());
                }
                self.push(" }");
            }
            ExprKind::Field { base, name } => {
                self.expr(base, Ctx::operand(POSTFIX));
                self.push(".");
                self.push(name);
            }
            ExprKind::Tuple(items) => {
                self.push("(");
                for (index, item) in items.iter().enumerate() {
                    if index > 0 {
                        self.push(", ");
                    }
                    self.expr(item, Ctx::bracketed());
                }
                self.push(")");
            }
            ExprKind::Tensor(items) => {
                self.push("[");
                for (index, item) in items.iter().enumerate() {
                    if index > 0 {
                        self.push(", ");
                    }
                    self.expr(item, Ctx::bracketed());
                }
                self.push("]");
            }
            ExprKind::Index { base, indices } => {
                self.expr(base, Ctx::operand(POSTFIX));
                self.push("[");
                for (index, slice) in indices.iter().enumerate() {
                    if index > 0 {
                        self.push(", ");
                    }
                    match slice {
                        SliceAst::Index(expr) => self.expr(expr, Ctx::bracketed()),
                        SliceAst::Range {
                            start,
                            end,
                            inclusive,
                        } => {
                            if let Some(start) = start {
                                self.expr(start, Ctx::bracketed());
                            }
                            self.push(if *inclusive { "..=" } else { ".." });
                            if let Some(end) = end {
                                self.expr(end, Ctx::bracketed());
                            }
                        }
                    }
                }
                self.push("]");
            }
            ExprKind::Annot { expr: inner, ty } => {
                self.expr(inner, Ctx::operand(TAIL));
                let rendered = self.ty_text(ty);
                let text = format!(" : {rendered}");
                self.push(&text);
            }
            ExprKind::Placeholder => self.push("_"),
            ExprKind::Try(inner) => {
                self.expr(inner, Ctx::operand(POSTFIX));
                self.push("?");
            }
            ExprKind::Factorial { expr: inner, double } => {
                self.expr(inner, Ctx::operand(POSTFIX));
                self.push(if *double { "!!" } else { "!" });
            }
        }
    }

    /// A branch of `if` or `let … in`: inline, or an indented block.
    fn branch(&mut self, expr: &Expr) {
        if matches!(expr.kind, ExprKind::Block { .. }) {
            self.expr(expr, Ctx::tail());
        } else {
            self.push(" ");
            self.expr(expr, Ctx::tail());
        }
    }

    fn string(&mut self, pieces: &[StrPiece]) {
        self.push("\"");
        for piece in pieces {
            match piece {
                StrPiece::Text(text) => {
                    let escaped = escape(text);
                    self.push(&escaped);
                }
                StrPiece::Expr(inner) => {
                    self.push("{");
                    self.expr(inner, Ctx::bracketed());
                    self.push("}");
                }
            }
        }
        self.push("\"");
    }

    // ----- patterns -------------------------------------------------------

    fn pattern(&mut self, pat: &Pat) {
        match &pat.kind {
            PatKind::Wildcard => self.push("_"),
            PatKind::Error => self.fail("the parser reported an error here"),
            PatKind::Var(name) => self.push(name),
            PatKind::Ctor { name, args } => {
                self.push(name);
                if !args.is_empty() {
                    self.push("(");
                    for (index, arg) in args.iter().enumerate() {
                        if index > 0 {
                            self.push(", ");
                        }
                        self.pattern(arg);
                    }
                    self.push(")");
                }
            }
            PatKind::Record(fields) => {
                self.push("{ ");
                for (index, (name, sub)) in fields.iter().enumerate() {
                    if index > 0 {
                        self.push(", ");
                    }
                    self.push(name);
                    if let Some(sub) = sub {
                        self.push(" = ");
                        self.pattern(sub);
                    }
                }
                self.push(" }");
            }
            PatKind::Tuple(items) => {
                self.push("(");
                for (index, item) in items.iter().enumerate() {
                    if index > 0 {
                        self.push(", ");
                    }
                    self.pattern(item);
                }
                self.push(")");
            }
            PatKind::Tensor { items, rest } => {
                self.push("[");
                for (index, item) in items.iter().enumerate() {
                    if index > 0 {
                        self.push(", ");
                    }
                    self.pattern(item);
                }
                if let Some(rest) = rest {
                    if !items.is_empty() {
                        self.push(", ");
                    }
                    let text = format!("..{rest}");
                    self.push(&text);
                }
                self.push("]");
            }
            PatKind::Lit(lit) => match lit {
                PatLit::Int(value) => self.push(&value.to_string()),
                PatLit::Str(text) => {
                    let quoted = quote(text);
                    self.push(&quoted);
                }
                PatLit::Bool(value) => self.push(if *value { "true" } else { "false" }),
                PatLit::Unit => self.push("()"),
            },
            PatKind::As { pattern, name } => {
                if matches!(pattern.kind, PatKind::As { .. }) {
                    self.push("(");
                    self.pattern(pattern);
                    self.push(")");
                } else {
                    self.pattern(pattern);
                }
                let text = format!(" as {name}");
                self.push(&text);
            }
            PatKind::Annot { pattern, ty } => {
                self.push("(");
                self.pattern(pattern);
                let rendered = self.ty_text(ty);
                let text = format!(": {rendered})");
                self.push(&text);
            }
        }
    }

    // ----- types ----------------------------------------------------------

    /// Prints a type. `atom` asks for a form that `ty_atom` accepts.
    fn ty(&mut self, ty: &Ty, atom: bool) {
        if self.error.is_some() {
            return;
        }
        match &ty.kind {
            TyKind::Int => self.push("Int"),
            TyKind::Bool => self.push("Bool"),
            TyKind::Prop => self.push("Prop"),
            TyKind::Str => self.push("String"),
            // `Unit` and `()` are the same type; the tree cannot tell them
            // apart, and `Unit` is how the specification writes it in a
            // signature (`fn main(): IO Unit`).
            TyKind::Unit => self.push("Unit"),
            TyKind::Real(dim) => self.numeric("Real", dim),
            TyKind::Float(dim) => self.numeric("Float", dim),
            TyKind::Complex(dim) => self.numeric("Complex", dim),
            TyKind::Tensor { shape, elem } => {
                self.push("Tensor<");
                self.shape(shape);
                self.push(", ");
                self.ty(elem, false);
                self.push(">");
            }
            TyKind::Matrix { rows, cols, elem } => {
                self.push("Matrix<");
                self.nat(rows);
                self.push(", ");
                self.nat(cols);
                self.push(", ");
                self.ty(elem, false);
                self.push(">");
            }
            TyKind::Sparse {
                format,
                shape,
                elem,
            } => {
                let head = format!("Sparse<{format}, ");
                self.push(&head);
                self.shape(shape);
                self.push(", ");
                self.ty(elem, false);
                self.push(">");
            }
            TyKind::Row(items) => {
                // A row sum is only reachable inside `Table<…>`; there is no
                // parenthesised row production to fall back on.
                if atom {
                    self.fail("a row type cannot be printed in this position");
                    return;
                }
                for (index, item) in items.iter().enumerate() {
                    if index > 0 {
                        self.push(" + ");
                    }
                    self.ty(item, false);
                }
            }
            TyKind::DimArg(dim) => {
                let text = format!("[{}]", dim.to_source());
                self.push(&text);
            }
            TyKind::NatArg(nat) => self.nat(nat),
            TyKind::ShapeArg(shape) => self.shape(shape),
            TyKind::Point(dim) => {
                let text = format!("Point[{}]", dim.to_source());
                self.push(&text);
            }
            TyKind::UnitOf(dim) => {
                let text = format!("Unit[{}]", dim.to_source());
                self.push(&text);
            }
            TyKind::Option(inner) => {
                self.push("Option<");
                self.ty(inner, false);
                self.push(">");
            }
            TyKind::Set(inner) => {
                self.push("Set<");
                self.ty(inner, false);
                self.push(">");
            }
            TyKind::Table(row) => {
                self.push("Table<");
                self.ty(row, false);
                self.push(">");
            }
            TyKind::Result(ok, err) => {
                self.push("Result<");
                self.ty(ok, false);
                self.push(", ");
                self.ty(err, false);
                self.push(">");
            }
            TyKind::Dyn(rep) => {
                let text = format!("Dyn<{rep}>");
                self.push(&text);
            }
            TyKind::RateOf(inner) => {
                self.push("RateOf<");
                self.ty(inner, false);
                self.push(">");
            }
            TyKind::SymFun(domain, codomain) => {
                let text = format!(
                    "SymFun<{}, {}>",
                    domain.to_source(),
                    codomain.to_source()
                );
                self.push(&text);
            }
            TyKind::Io(inner) => {
                self.push("IO ");
                self.ty(inner, true);
            }
            TyKind::Fn(args, ret) => {
                if args.is_empty() {
                    // `() -> t` re-parses as one unit argument, so a zero-argument
                    // function type has no source form to print.
                    self.fail("a function type with no arguments has no source form");
                    return;
                }
                if atom {
                    self.push("(");
                }
                if args.len() == 1 {
                    // `a -> b` is how a one-argument function type is written;
                    // `(a) -> b` is the same tree, and a tuple argument keeps
                    // its own brackets.
                    self.ty(&args[0], true);
                } else {
                    self.push("(");
                    for (index, arg) in args.iter().enumerate() {
                        if index > 0 {
                            self.push(", ");
                        }
                        self.ty(arg, true);
                    }
                    self.push(")");
                }
                self.push(" -> ");
                self.ty(ret, false);
                if atom {
                    self.push(")");
                }
            }
            TyKind::Record { fields, rest } => {
                self.push("{ ");
                for (index, (name, field)) in fields.iter().enumerate() {
                    if index > 0 {
                        self.push(", ");
                    }
                    let head = format!("{name}: ");
                    self.push(&head);
                    self.ty(field, false);
                }
                if let Some(rest) = rest {
                    if !fields.is_empty() {
                        self.push(", ");
                    }
                    let text = format!("...{rest}");
                    self.push(&text);
                }
                self.push(" }");
            }
            TyKind::Tuple(items) => {
                self.push("(");
                for (index, item) in items.iter().enumerate() {
                    if index > 0 {
                        self.push(", ");
                    }
                    self.ty(item, false);
                }
                self.push(")");
            }
            TyKind::Named { path, args } => {
                let name = path.join(".");
                self.push(&name);
                // `T[a]` is how the parser records a dimension argument and
                // `T<…>` everything else; the postfix form can only carry the
                // dimension arguments that come last.
                let split = args
                    .iter()
                    .position(|arg| matches!(arg.kind, TyKind::DimArg(_)))
                    .unwrap_or(args.len());
                if split > 0 {
                    self.push("<");
                    for (index, arg) in args[..split].iter().enumerate() {
                        if index > 0 {
                            self.push(", ");
                        }
                        self.ty(arg, false);
                    }
                    self.push(">");
                }
                for arg in &args[split..] {
                    match &arg.kind {
                        TyKind::DimArg(dim) => {
                            let text = format!("[{}]", dim.to_source());
                            self.push(&text);
                        }
                        _ => self.fail(
                            "a type argument after a dimension argument has no source form",
                        ),
                    }
                }
            }
            TyKind::Var(name) => self.push(name),
            TyKind::Forall(params, body) => {
                let names = params
                    .iter()
                    .map(|param| match param.kind {
                        Some(kind) => format!("{}: {}", param.name, kind.as_str()),
                        None => param.name.clone(),
                    })
                    .collect::<Vec<_>>()
                    .join(", ");
                let head = format!("forall {names}. ");
                self.push(&head);
                self.ty(body, false);
            }
        }
    }

    fn numeric(&mut self, name: &str, dim: &DimAst) {
        self.push(name);
        if !matches!(dim, DimAst::One) {
            let text = format!("[{}]", dim.to_source());
            self.push(&text);
        }
    }

    fn shape(&mut self, shape: &ShapeAst) {
        match shape {
            ShapeAst::Rank(nat) => self.nat(nat),
            ShapeAst::Refined(items) => {
                self.push("[");
                for (index, item) in items.iter().enumerate() {
                    if index > 0 {
                        self.push(", ");
                    }
                    match item {
                        ShapeItem::Known(nat) => self.nat(nat),
                        ShapeItem::Unknown => self.push("?"),
                    }
                }
                self.push("]");
            }
        }
    }

    fn nat(&mut self, nat: &NatAst) {
        match nat {
            NatAst::Lit(value) => self.push(&value.to_string()),
            NatAst::Var(name) => self.push(name),
            NatAst::Add(left, right) => {
                if matches!(**right, NatAst::Add(..)) {
                    self.fail("a right-nested `+` in a shape has no source form");
                    return;
                }
                self.nat(left);
                self.push(" + ");
                self.nat(right);
            }
        }
    }
}

/// The binding power of an expression, and whether it is a form that greedily
/// consumes what follows it.
fn precedence(kind: &ExprKind) -> (u8, bool) {
    match kind {
        ExprKind::If { .. }
        | ExprKind::Match { .. }
        | ExprKind::LetIn { .. }
        | ExprKind::Lambda { .. }
        | ExprKind::Do { .. }
        | ExprKind::Block { .. }
        | ExprKind::Annot { .. } => (TAIL, true),
        ExprKind::Binary { op, .. } => (binary_prec(*op), false),
        ExprKind::Unary { .. } => (UNARY, false),
        ExprKind::Call { .. }
        | ExprKind::TypeApp { .. }
        | ExprKind::Field { .. }
        | ExprKind::Index { .. }
        | ExprKind::Try(_)
        | ExprKind::Factorial { .. } => (POSTFIX, false),
        _ => (ATOM, false),
    }
}

fn binary_prec(op: BinOp) -> u8 {
    match op {
        BinOp::Pipe | BinOp::PipeBack => PIPE,
        BinOp::Or => OR,
        BinOp::And => AND,
        BinOp::Eq | BinOp::Ne | BinOp::Lt | BinOp::Le | BinOp::Gt | BinOp::Ge => CMP,
        BinOp::Cons => CONS,
        BinOp::Add | BinOp::Sub => ADD,
        BinOp::Mul | BinOp::Div | BinOp::MatMul => MUL,
        BinOp::Pow => POW,
    }
}

fn is_right_associative(op: BinOp) -> bool {
    matches!(op, BinOp::Or | BinOp::And | BinOp::Cons)
}

fn block_stmt_start(stmt: &BlockStmt) -> usize {
    match stmt {
        BlockStmt::Let { span, .. } => usize::try_from(span.start).unwrap_or(0),
        BlockStmt::Fn(decl) => usize::try_from(decl.span.start).unwrap_or(0),
    }
}

fn do_stmt_start(stmt: &DoStmt) -> usize {
    match stmt {
        DoStmt::Let { span, .. } | DoStmt::Bind { span, .. } => {
            usize::try_from(span.start).unwrap_or(0)
        }
        DoStmt::Expr(expr) => usize::try_from(expr.span.start).unwrap_or(0),
    }
}

/// A short description of a declaration, for a failure message.
#[must_use]
pub fn describe_decl(decl: &Decl) -> String {
    describe(decl)
}

fn describe(decl: &Decl) -> String {
    match &decl.kind {
        DeclKind::Import { path, .. } => format!("`import {}`", path.join(".")),
        DeclKind::Dimension { name, .. } => format!("`dimension {name}`"),
        DeclKind::Unit { name, .. } => format!("`unit {name}`"),
        DeclKind::Type { name, .. } => format!("`type {name}`"),
        DeclKind::Class { name, .. } => format!("`class {name}`"),
        DeclKind::Instance { class, .. } => format!("`instance {class}`"),
        DeclKind::Derive { classes, .. } => format!("`derive {}`", classes.join(", ")),
        DeclKind::Fn { name, .. } => format!("`fn {name}`"),
        DeclKind::Let { pattern, .. } => match &pattern.kind {
            PatKind::Var(name) => format!("`let {name}`"),
            PatKind::Wildcard => "`let _`".to_string(),
            _ => "a `let`".to_string(),
        },
        DeclKind::Test { name, .. } => format!("`test {name}`"),
    }
}

fn type_params(params: &[TypeParam]) -> String {
    if params.is_empty() {
        return String::new();
    }
    let inner = params
        .iter()
        .map(|param| match param.kind {
            Some(kind) => format!("{}: {}", param.name, kind.as_str()),
            None => param.name.clone(),
        })
        .collect::<Vec<_>>()
        .join(", ");
    format!("<{inner}>")
}

/// Renders a unit scale as source: `1000`, `1/1000`, `pi/180`.
fn scale_into(out: &mut String, scale: &aevia_dim::Scale) -> bool {
    use num_traits::One;
    let numer = scale.rat.numer();
    let denom = scale.rat.denom();
    let negative_one = -numer.clone();
    match scale.pi {
        0 => {
            if denom.is_one() {
                let _ = write!(out, "{numer}");
            } else {
                let _ = write!(out, "{numer}/{denom}");
            }
            true
        }
        1 if numer.is_one() || negative_one.is_one() => {
            out.push_str(if numer.is_one() { "pi" } else { "-pi" });
            if !denom.is_one() {
                let _ = write!(out, "/{denom}");
            }
            true
        }
        _ => false,
    }
}

/// Escapes a string's contents for a single-line `"…"` literal.
#[must_use]
pub fn escape(text: &str) -> String {
    let mut out = String::new();
    for c in text.chars() {
        match c {
            '\\' => out.push_str("\\\\"),
            '"' => out.push_str("\\\""),
            '{' => out.push_str("\\{"),
            '}' => out.push_str("\\}"),
            '\n' => out.push_str("\\n"),
            '\t' => out.push_str("\\t"),
            '\r' => out.push_str("\\r"),
            '\0' => out.push_str("\\0"),
            c if (c as u32) < 0x20 || c == '\u{7f}' => {
                let _ = write!(out, "\\u{{{:x}}}", c as u32);
            }
            c => out.push(c),
        }
    }
    out
}

fn quote(text: &str) -> String {
    format!("\"{}\"", escape(text))
}
