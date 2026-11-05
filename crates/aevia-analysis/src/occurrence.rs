//! What is written at a byte offset.
//!
//! Hover, definition and completion all start from the same question: *which
//! name is the cursor on, and what role does it play here?* Answering it once,
//! by walking the parsed module, keeps the three queries from disagreeing
//! about a position — the classic failure of a language server is that
//! `definition` finds a symbol where `hover` finds nothing.
//!
//! The walk is over the AST the compiler built, not over tokens: a name inside
//! a comment or a string is not an occurrence, because the parser never put it
//! in the tree.
//!
//! It also carries the one piece of scoping this crate needs: a stack of the
//! bindings each enclosing function, lambda, block, `let … in` and `match` arm
//! introduced. That is enough to say where a *local* name is bound without
//! asking the checker, and it is deliberately not a second type checker — it
//! tracks names and spans, nothing else.

use aevia_syntax::ast::*;
use aevia_syntax::token::TokenKind;
use aevia_syntax::Span;
use unicode_ident::is_xid_continue;

/// The role a name plays at its span.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Role {
    /// A bare name: a reference or a binding site.
    Name(String),
    /// A dotted name such as `Units.SI.km`.
    Qualified(Vec<String>),
    /// A variant constructor, in expression or pattern position.
    Constructor(String),
    /// A method declared in an `instance`.
    Method { name: String, class: String },
    /// A name used as a type.
    TypeName(String),
    /// A unit name inside a unit literal: the `m` of `3 m`.
    ///
    /// The parser turns a unit literal into one `Quantity` node, so the units
    /// are not in the AST; they are recovered by lexing the literal's own
    /// source, which is the same text the compiler resolved.
    Unit(String),
}

/// One name written in the module.
#[derive(Clone, Debug)]
pub(crate) struct Occurrence {
    /// Where the name is.
    pub span: Span,
    /// What it is.
    pub role: Role,
    /// The span of the call this name is the callee of, when it is one.
    ///
    /// The checker records a resolved `Rssn.<op>` call and a resolved class
    /// method against the whole call expression, so finding the pinned
    /// overload from a name needs the call it belongs to.
    pub call: Option<Span>,
    /// Where the name is bound, when the binding is local.
    ///
    /// `None` for a module-level name — the module's own declaration table
    /// answers those — and for a name that is not bound anywhere the walk
    /// could see, such as a prelude builtin.
    pub binding: Option<Span>,
}

/// Every name in `module`.
///
/// The order is the walk's: source order, except that a `let`'s value is
/// visited before the name it binds, because the value is written in the
/// enclosing scope and `let x = x` must resolve the right-hand `x` to the
/// outer binding.
#[must_use]
pub(crate) fn collect(module: &Module, text: &str) -> Vec<Occurrence> {
    let mut collector = Collector {
        out: Vec::new(),
        text,
        scopes: vec![Vec::new()],
    };
    for (index, decl) in module.decls.iter().enumerate() {
        collector.decl(decl, index);
    }
    collector.out
}

/// The innermost occurrence containing `offset`.
#[must_use]
pub(crate) fn at(occurrences: &[Occurrence], offset: u32) -> Option<&Occurrence> {
    occurrences
        .iter()
        .filter(|occurrence| {
            occurrence.span.start <= offset && offset <= occurrence.span.end
        })
        .min_by_key(|occurrence| occurrence.span.end.saturating_sub(occurrence.span.start))
}

/// The span of `name` written inside `within`, or `within` when it is not
/// found.
///
/// The AST records a declaration's whole extent, not the extent of its name,
/// so the name is located in the source the declaration came from. Searching
/// for a whole identifier (not a substring) is enough: `fn fall_time` cannot
/// contain an earlier standalone `fall_time`.
#[must_use]
pub(crate) fn name_span(text: &str, within: Span, name: &str) -> Span {
    let start = usize::try_from(within.start).unwrap_or(usize::MAX);
    let end = usize::try_from(within.end).unwrap_or(usize::MAX).min(text.len());
    let Some(slice) = text.get(start..end) else {
        return within;
    };
    let mut from = 0;
    while let Some(found) = slice[from..].find(name) {
        let at = from + found;
        let before_ok = at == 0
            || !slice[..at]
                .chars()
                .next_back()
                .is_some_and(is_identifier_continue);
        let after = at + name.len();
        let after_ok = after >= slice.len()
            || !slice[after..]
                .chars()
                .next()
                .is_some_and(is_identifier_continue);
        if before_ok && after_ok {
            return Span::new(within.file, start + at, start + after);
        }
        from = after.max(at + 1);
    }
    within
}

/// True for a character that may continue an identifier, as the lexer defines
/// it.
fn is_identifier_continue(character: char) -> bool {
    is_xid_continue(character) || character == '\'' || ('₀'..='₉').contains(&character)
}

/// The path a dotted name starts with, when `expr` is one.
///
/// Aevia's module paths are upper-case (`Units.SI.km`), and the parser only
/// joins upper-case segments into a [`ExprKind::QName`]; the trailing member of
/// `Rssn.eig` is a lower-case field of an upper-case root.
fn qualified_root(expr: &Expr) -> Option<Vec<String>> {
    match &expr.kind {
        ExprKind::QName(path) => Some(path.clone()),
        ExprKind::Var(name)
            if name
                .chars()
                .next()
                .is_some_and(|first| first.is_uppercase()) =>
        {
            Some(vec![name.clone()])
        }
        _ => None,
    }
}

struct Collector<'a> {
    out: Vec<Occurrence>,
    text: &'a str,
    /// The bindings in scope, innermost last.
    scopes: Vec<Vec<(String, Span)>>,
}

impl Collector<'_> {
    fn push(&mut self, span: Span, role: Role, call: Option<Span>) {
        self.out.push(Occurrence {
            span,
            role,
            call,
            binding: None,
        });
    }

    /// Records a declaration's name and makes it visible to what follows.
    fn declare(&mut self, within: Span, name: &str, role: Role) {
        let span = name_span(self.text, within, name);
        self.bind(name, span);
        self.record(span, role);
    }

    /// Records a declaration's name without making it visible to what follows.
    ///
    /// An `instance` method is declared, but it is not a name in scope: a call
    /// to it is dispatched by the checker against the instance table, and two
    /// instances may declare the same method. Binding it here would make the
    /// first instance win every later lookup.
    fn declare_only(&mut self, within: Span, name: &str, role: Role) {
        let span = name_span(self.text, within, name);
        self.record(span, role);
    }

    /// Records a name whose definition is itself.
    fn record(&mut self, span: Span, role: Role) {
        self.out.push(Occurrence {
            span,
            role,
            call: None,
            binding: Some(span),
        });
    }

    /// Records a use of `name`, resolving it against the scopes in view.
    fn use_name(&mut self, span: Span, name: &str, call: Option<Span>) {
        let binding = self.lookup(name);
        self.out.push(Occurrence {
            span,
            role: Role::Name(name.to_string()),
            call,
            binding,
        });
    }

    fn bind(&mut self, name: &str, span: Span) {
        if let Some(scope) = self.scopes.last_mut() {
            scope.push((name.to_string(), span));
        }
    }

    /// Records the units of a unit literal by lexing the literal's own source.
    ///
    /// The AST stores the resolved unit expression, not its tokens; the tokens
    /// are still in the document, so lexing the literal recovers their spans.
    /// Every identifier after the numeric coefficient is part of the unit
    /// expression, including the `°C` of `25 °C`.
    fn quantity_units(&mut self, span: Span) {
        let start = usize::try_from(span.start).unwrap_or(usize::MAX);
        let end = usize::try_from(span.end).unwrap_or(usize::MAX);
        let Some(text) = self.text.get(start..end) else {
            return;
        };
        let lexed = aevia_syntax::lex_inline(span.file, text, start);
        let mut after_number = false;
        for token in lexed.tokens {
            match &token.kind {
                TokenKind::Int(_) | TokenKind::Float(_) => after_number = true,
                TokenKind::Ident(name) | TokenKind::UpperIdent(name) if after_number => {
                    self.push(token.span, Role::Unit(name.clone()), None);
                }
                _ => {}
            }
        }
    }

    fn lookup(&self, name: &str) -> Option<Span> {
        self.scopes
            .iter()
            .rev()
            .flat_map(|scope| scope.iter().rev())
            .find(|(bound, _)| bound == name)
            .map(|(_, span)| *span)
    }

    fn enter(&mut self) {
        self.scopes.push(Vec::new());
    }

    fn leave(&mut self) {
        self.scopes.pop();
    }

    fn decl(&mut self, decl: &Decl, index: usize) {
        let _ = index;
        match &decl.kind {
            DeclKind::Import { .. } => {}
            DeclKind::Dimension { name, value } => {
                self.declare(decl.span, name, Role::Name(name.clone()));
                let _ = value;
            }
            DeclKind::Unit { name, dim, .. } => {
                self.declare(decl.span, name, Role::Name(name.clone()));
                let _ = dim;
            }
            DeclKind::Type { name, params, body } => {
                self.declare(decl.span, name, Role::TypeName(name.clone()));
                for param in params {
                    let _ = param;
                }
                match body {
                    TypeBody::Alias(ty) => self.ty(ty),
                    TypeBody::Variants(ctors) => {
                        for ctor in ctors {
                            self.push(
                                ctor.span,
                                Role::Constructor(ctor.name.clone()),
                                None,
                            );
                            for arg in &ctor.args {
                                self.ty(arg);
                            }
                        }
                    }
                    TypeBody::Abstract => {}
                }
            }
            DeclKind::Class {
                name,
                params,
                methods,
            } => {
                self.declare(decl.span, name, Role::TypeName(name.clone()));
                let _ = params;
                for (_, ty) in methods {
                    self.ty(ty);
                }
            }
            DeclKind::Instance {
                class,
                ty,
                methods,
            } => {
                self.ty(ty);
                for method in methods {
                    self.declare_only(
                        method.span,
                        &method.name,
                        Role::Method {
                            name: method.name.clone(),
                            class: class.clone(),
                        },
                    );
                    self.enter();
                    for param in &method.params {
                        self.pat(&param.pattern);
                        if let Some(ty) = &param.ty {
                            self.ty(ty);
                        }
                    }
                    if let Some(ret) = &method.ret {
                        self.ty(ret);
                    }
                    self.expr(&method.body, None);
                    self.leave();
                }
            }
            DeclKind::Derive { ty, .. } => self.ty(ty),
            DeclKind::Fn {
                name,
                params,
                args,
                ret,
                body,
            } => {
                self.declare(decl.span, name, Role::Name(name.clone()));
                let _ = params;
                self.enter();
                for arg in args {
                    self.pat(&arg.pattern);
                    if let Some(ty) = &arg.ty {
                        self.ty(ty);
                    }
                }
                if let Some(ret) = ret {
                    self.ty(ret);
                }
                self.expr(body, None);
                self.leave();
            }
            DeclKind::Let { pattern, ty, value, .. } => {
                if let Some(ty) = ty {
                    self.ty(ty);
                }
                self.expr(value, None);
                self.pat(pattern);
            }
            DeclKind::Test { body, .. } => self.expr(body, None),
        }
    }

    fn expr(&mut self, expr: &Expr, call: Option<Span>) {
        match &expr.kind {
            ExprKind::UnitLit | ExprKind::Error | ExprKind::Bool(_) => {}
            ExprKind::Int(_) | ExprKind::Float(_) | ExprKind::Placeholder => {}
            ExprKind::Quantity { .. } => self.quantity_units(expr.span),
            ExprKind::Var(name) => self.use_name(expr.span, name, call),
            ExprKind::QName(path) => self.push(expr.span, Role::Qualified(path.clone()), call),
            ExprKind::Str(pieces) => {
                for piece in pieces {
                    if let StrPiece::Expr(inner) = piece {
                        self.expr(inner, None);
                    }
                }
            }
            ExprKind::Unary { operand, .. } => self.expr(operand, None),
            ExprKind::Binary { left, right, .. } => {
                self.expr(left, None);
                self.expr(right, None);
            }
            ExprKind::Call { callee, args } => {
                self.expr(callee, Some(expr.span));
                for arg in args {
                    self.expr(arg, None);
                }
            }
            ExprKind::TypeApp { callee, args } => {
                self.expr(callee, Some(expr.span));
                for arg in args {
                    self.ty(arg);
                }
            }
            ExprKind::Lambda { params, ret, body } => {
                self.enter();
                for param in params {
                    self.pat(&param.pattern);
                    if let Some(ty) = &param.ty {
                        self.ty(ty);
                    }
                }
                if let Some(ret) = ret {
                    self.ty(ret);
                }
                self.expr(body, None);
                self.leave();
            }
            ExprKind::If {
                cond,
                then_branch,
                else_branch,
            } => {
                self.expr(cond, None);
                self.expr(then_branch, None);
                self.expr(else_branch, None);
            }
            ExprKind::Match { scrutinee, arms } => {
                self.expr(scrutinee, None);
                for arm in arms {
                    self.enter();
                    self.pat(&arm.pattern);
                    if let Some(guard) = &arm.guard {
                        self.expr(guard, None);
                    }
                    self.expr(&arm.body, None);
                    self.leave();
                }
            }
            ExprKind::LetIn {
                pattern,
                ty,
                value,
                body,
            } => {
                if let Some(ty) = ty {
                    self.ty(ty);
                }
                self.expr(value, None);
                self.enter();
                self.pat(pattern);
                self.expr(body, None);
                self.leave();
            }
            ExprKind::Block { stmts, tail } => {
                self.enter();
                for stmt in stmts {
                    match stmt {
                        BlockStmt::Let {
                            pattern,
                            ty,
                            value,
                            ..
                        } => {
                            if let Some(ty) = ty {
                                self.ty(ty);
                            }
                            self.expr(value, None);
                            self.pat(pattern);
                        }
                        BlockStmt::Fn(decl) => self.decl(decl, 0),
                    }
                }
                if let Some(tail) = tail {
                    self.expr(tail, None);
                }
                self.leave();
            }
            ExprKind::Do { stmts } => {
                self.enter();
                for stmt in stmts {
                    match stmt {
                        DoStmt::Let {
                            pattern,
                            ty,
                            value,
                            ..
                        } => {
                            if let Some(ty) = ty {
                                self.ty(ty);
                            }
                            self.expr(value, None);
                            self.pat(pattern);
                        }
                        DoStmt::Bind { pattern, value, .. } => {
                            self.expr(value, None);
                            self.pat(pattern);
                        }
                        DoStmt::Expr(expr) => self.expr(expr, None),
                    }
                }
                self.leave();
            }
            ExprKind::Record { base, fields } => {
                if let Some(base) = base {
                    self.expr(base, None);
                }
                for (_, value) in fields {
                    self.expr(value, None);
                }
            }
            ExprKind::Field { base, name } => {
                // `Rssn.eig` and `Units.SI.km` are dotted *names*, not field
                // accesses: the parser leaves `Rssn.eig` as a field of an
                // upper-case root, and the checker reads it back the same way.
                if let Some(mut path) = qualified_root(base) {
                    path.push(name.clone());
                    self.push(expr.span, Role::Qualified(path), call);
                } else {
                    self.expr(base, None);
                }
            }
            ExprKind::Tuple(items) | ExprKind::Tensor(items) => {
                for item in items {
                    self.expr(item, None);
                }
            }
            ExprKind::Index { base, indices } => {
                self.expr(base, None);
                for index in indices {
                    match index {
                        SliceAst::Index(expr) => self.expr(expr, None),
                        SliceAst::Range { start, end, .. } => {
                            if let Some(start) = start {
                                self.expr(start, None);
                            }
                            if let Some(end) = end {
                                self.expr(end, None);
                            }
                        }
                    }
                }
            }
            ExprKind::Annot { expr, ty } => {
                self.expr(expr, None);
                self.ty(ty);
            }
            ExprKind::Try(inner) | ExprKind::Factorial { expr: inner, .. } => {
                self.expr(inner, None)
            }
        }
    }

    fn pat(&mut self, pat: &Pat) {
        match &pat.kind {
            PatKind::Wildcard | PatKind::Error => {}
            PatKind::Var(name) => {
                self.bind(name, pat.span);
                self.record(pat.span, Role::Name(name.clone()));
            }
            PatKind::Ctor { name, args } => {
                self.push(pat.span, Role::Constructor(name.clone()), None);
                for arg in args {
                    self.pat(arg);
                }
            }
            PatKind::Record(fields) => {
                for (_, pattern) in fields {
                    if let Some(pattern) = pattern {
                        self.pat(pattern);
                    }
                }
            }
            PatKind::Tuple(items) => {
                for item in items {
                    self.pat(item);
                }
            }
            PatKind::Tensor { items, .. } => {
                for item in items {
                    self.pat(item);
                }
            }
            PatKind::Lit(_) => {}
            PatKind::As { pattern, name } => {
                self.pat(pattern);
                self.bind(name, pat.span);
                self.record(pat.span, Role::Name(name.clone()));
            }
            PatKind::Annot { pattern, ty } => {
                self.pat(pattern);
                self.ty(ty);
            }
        }
    }

    fn ty(&mut self, ty: &Ty) {
        match &ty.kind {
            TyKind::Int
            | TyKind::Bool
            | TyKind::Prop
            | TyKind::Str
            | TyKind::Unit
            | TyKind::Var(_) => {}
            TyKind::Real(_)
            | TyKind::Float(_)
            | TyKind::Complex(_)
            | TyKind::Point(_)
            | TyKind::UnitOf(_)
            | TyKind::SymFun(_, _)
            | TyKind::DimArg(_)
            | TyKind::NatArg(_)
            | TyKind::ShapeArg(_)
            | TyKind::Dyn(_) => {}
            TyKind::Tensor { elem, .. }
            | TyKind::Sparse { elem, .. }
            | TyKind::Option(elem)
            | TyKind::Set(elem)
            | TyKind::Table(elem)
            | TyKind::RateOf(elem)
            | TyKind::DiffusionOf(elem)
            | TyKind::Io(elem) => self.ty(elem),
            TyKind::Matrix { rows, cols, elem } => {
                let _ = (rows, cols);
                self.ty(elem);
            }
            TyKind::Result(ok, err) => {
                self.ty(ok);
                self.ty(err);
            }
            TyKind::Row(parts) => {
                for part in parts {
                    self.ty(part);
                }
            }
            TyKind::Fn(args, ret) => {
                for arg in args {
                    self.ty(arg);
                }
                self.ty(ret);
            }
            TyKind::Record { fields, .. } => {
                for (_, field) in fields {
                    self.ty(field);
                }
            }
            TyKind::Tuple(items) => {
                for item in items {
                    self.ty(item);
                }
            }
            TyKind::Named { path, args } => {
                match path.as_slice() {
                    [name] => self.push(ty.span, Role::TypeName(name.clone()), None),
                    _ => self.push(ty.span, Role::Qualified(path.clone()), None),
                }
                for arg in args {
                    self.ty(arg);
                }
            }
            TyKind::Forall(params, body) => {
                let _ = params;
                self.ty(body);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use aevia_syntax::{lex, parse_module};

    fn occurrences(source: &str) -> Vec<Occurrence> {
        let lexed = lex(0, source).expect("lexes");
        let (module, diagnostics) = parse_module(0, source, &lexed);
        assert!(!diagnostics.has_errors(), "{:?}", diagnostics.items);
        collect(&module, source)
    }

    #[test]
    fn a_declaration_name_and_its_uses_are_both_recorded() {
        let source = "fn double(x: Int): Int = x + x\nlet y = double(2)\n";
        let found = occurrences(source);
        let names: Vec<&str> = found
            .iter()
            .filter_map(|occurrence| match &occurrence.role {
                Role::Name(name) => Some(name.as_str()),
                _ => None,
            })
            .collect();
        // `double(2)` is walked before the `y` it is bound to.
        assert_eq!(names, ["double", "x", "x", "x", "double", "y"]);
        // The declaration's name is its own three characters, not the body.
        assert_eq!(
            &source[found[0].span.start as usize..found[0].span.end as usize],
            "double"
        );
    }

    #[test]
    fn a_callee_carries_the_call_it_belongs_to() {
        let source = "let a = show(x)\n";
        let found = occurrences(source);
        let callee = found
            .iter()
            .find(|occurrence| matches!(&occurrence.role, Role::Name(name) if name == "show"))
            .expect("show");
        let call = callee.call.expect("callee of a call");
        assert_eq!(&source[call.start as usize..call.end as usize], "show(x)");
    }

    #[test]
    fn a_qualified_name_keeps_its_segments() {
        let source = "let a = Rssn.eig(m)\n";
        let found = occurrences(source);
        let qualified = found
            .iter()
            .find(|occurrence| matches!(occurrence.role, Role::Qualified(_)))
            .expect("qualified");
        assert_eq!(
            qualified.role,
            Role::Qualified(vec!["Rssn".to_string(), "eig".to_string()])
        );
        assert!(qualified.call.is_some());
    }

    #[test]
    fn a_constructor_pattern_is_a_constructor_not_a_name() {
        let source = "type Shape = | Circle(Real[L]) | Dot\n\nfn area(s: Shape): Real[L] =\n    match s with\n    | Dot -> 0.0 m^2\n    | Circle(r) -> r\n";
        let found = occurrences(source);
        let ctors: Vec<&str> = found
            .iter()
            .filter_map(|occurrence| match &occurrence.role {
                Role::Constructor(name) => Some(name.as_str()),
                _ => None,
            })
            .collect();
        assert_eq!(ctors, ["Circle", "Dot", "Dot", "Circle"]);
    }

    #[test]
    fn a_name_in_a_comment_is_not_an_occurrence() {
        let source = "# show is not called here\nlet x = 1\n";
        let found = occurrences(source);
        assert!(
            found
                .iter()
                .all(|occurrence| !matches!(&occurrence.role, Role::Name(name) if name == "show"))
        );
    }

    #[test]
    fn the_innermost_name_wins() {
        let source = "let f = fn (x: Int) => x + 1\n";
        let found = occurrences(source);
        let offset = u32::try_from(source.rfind('x').expect("x")).expect("offset");
        let at = at(&found, offset).expect("an occurrence");
        assert_eq!(at.role, Role::Name("x".to_string()));
        assert_eq!(at.span.start, offset);
    }

    #[test]
    fn a_method_records_its_class() {
        let source = "instance Show for Float where\n    show(x: Float): String = \"f\"\n";
        let found = occurrences(source);
        let method = found
            .iter()
            .find(|occurrence| matches!(occurrence.role, Role::Method { .. }))
            .expect("method");
        assert_eq!(
            method.role,
            Role::Method {
                name: "show".to_string(),
                class: "Show".to_string()
            }
        );
    }
}
