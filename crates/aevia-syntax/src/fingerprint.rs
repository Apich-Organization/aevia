//! A span-free structural rendering of the AST.
//!
//! `fmt` has to prove that it changed nothing but whitespace. The AST carries
//! a span on every node, so `==` would fail on any reformatting; what has to be
//! equal is the *tree*. This module writes the tree to a canonical string with
//! every span left out, so two trees are the same program exactly when their
//! fingerprints are equal, and a mismatch can be diffed.
//!
//! Comments and doc comments: `##` text is part of [`Decl::doc`] and is
//! therefore in the fingerprint; ordinary `#` comments are not in the AST at
//! all and are checked separately (the formatter checks them separately).

use crate::ast::*;
use std::fmt::Write as _;

/// The fingerprint of a module.
#[must_use]
pub fn module(module: &Module) -> String {
    let mut out = String::new();
    out.push_str("module");
    match &module.name {
        | Some(path) => {
            out.push('<');
            out.push_str(&path.join("."));
            out.push('>');
        },
        | None => out.push_str("<none>"),
    }
    out.push('{');
    for decl in &module.decls {
        decl_into(&mut out, decl);
        out.push(';');
    }
    out.push('}');
    out
}

/// The fingerprint of one declaration, for attributing a difference.
#[must_use]
pub fn declaration(decl: &Decl) -> String {
    let mut out = String::new();
    decl_into(&mut out, decl);
    out
}

/// A method's complete signature and body, without source offsets or FileId.
#[must_use]
pub fn method(method: &InstanceMethod) -> String {
    let mut out = String::new();
    let _ = write!(out, "method:{}(", method.name);
    for param in &method.params {
        param_into(&mut out, param);
        out.push(',');
    }
    out.push(')');
    if let Some(ret) = &method.ret {
        ty_into(&mut out, ret);
    }
    expr_into(&mut out, &method.body);
    out
}

fn decl_into(
    out: &mut String,
    decl: &Decl,
) {
    out.push_str("decl[");
    for attr in &decl.attrs {
        attribute_into(out, attr);
        out.push(',');
    }
    out.push(']');
    if decl.private {
        out.push_str("private ");
    }
    match &decl.doc {
        | Some(doc) => {
            let _ = write!(out, "doc({doc:?}) ");
        },
        | None => out.push_str("doc(none) "),
    }
    match &decl.kind {
        | DeclKind::Import {
            path,
            alias,
            names,
            open,
        } => {
            let _ = write!(
                out,
                "{}({} as {} names {:?})",
                if *open { "open" } else { "import" },
                path.join("."),
                alias.as_deref().unwrap_or("-"),
                names
            );
        },
        | DeclKind::Dimension { name, value } => {
            out.push_str("dimension ");
            out.push_str(name);
            if let Some(value) = value {
                out.push_str(" = ");
                dim_into(out, value);
            }
        },
        | DeclKind::Unit {
            name,
            dim,
            scale,
            base,
            offset,
            prefixable,
        } => {
            let _ = write!(out, "unit {name} : ");
            dim_into(out, dim);
            let _ = write!(out, " = {}/{}", scale.rat.numer(), scale.rat.denom());
            let _ = write!(out, " pi^{}", scale.pi);
            if let Some(base) = base {
                out.push_str(" base ");
                unit_expr_into(out, base);
            }
            if let Some((scale, unit)) = offset {
                let _ = write!(out, " offset {}/{}", scale.rat.numer(), scale.rat.denom());
                let _ = write!(out, " pi^{}", scale.pi);
                if let Some(unit) = unit {
                    out.push_str(" of ");
                    unit_expr_into(out, unit);
                }
            }
            if *prefixable {
                out.push_str(" prefixable");
            }
        },
        | DeclKind::Type { name, params, body } => {
            out.push_str("type ");
            out.push_str(name);
            type_params_into(out, params);
            out.push_str(" = ");
            match body {
                | TypeBody::Alias(ty) => {
                    out.push_str("alias ");
                    ty_into(out, ty);
                },
                | TypeBody::Variants(variants) => {
                    out.push_str("variants ");
                    for variant in variants {
                        let _ = write!(out, "{}(", variant.name);
                        for arg in &variant.args {
                            ty_into(out, arg);
                            out.push(',');
                        }
                        out.push_str(")|");
                    }
                },
                | TypeBody::Abstract => out.push_str("abstract"),
            }
        },
        | DeclKind::Class {
            name,
            params,
            methods,
        } => {
            out.push_str("class ");
            out.push_str(name);
            type_params_into(out, params);
            out.push_str(" where ");
            for (method, ty) in methods {
                let _ = write!(out, "{method}: ");
                ty_into(out, ty);
                out.push(';');
            }
        },
        | DeclKind::Instance { class, ty, methods } => {
            let _ = write!(out, "instance {class} for ");
            ty_into(out, ty);
            out.push_str(" where ");
            for method in methods {
                let _ = write!(out, "{}(", method.name);
                for param in &method.params {
                    param_into(out, param);
                    out.push(',');
                }
                out.push_str(")");
                if let Some(ret) = &method.ret {
                    out.push_str(" : ");
                    ty_into(out, ret);
                }
                out.push_str(" = ");
                expr_into(out, &method.body);
                out.push(';');
            }
        },
        | DeclKind::Derive { classes, ty } => {
            let _ = write!(out, "derive {} for ", classes.join(","));
            ty_into(out, ty);
        },
        | DeclKind::Fn {
            name,
            params,
            args,
            ret,
            body,
        } => {
            out.push_str("fn ");
            out.push_str(name);
            type_params_into(out, params);
            out.push('(');
            for arg in args {
                param_into(out, arg);
                out.push(',');
            }
            out.push(')');
            if let Some(ret) = ret {
                out.push_str(" : ");
                ty_into(out, ret);
            }
            out.push_str(" = ");
            expr_into(out, body);
        },
        | DeclKind::Let {
            pattern,
            ty,
            value,
            rec,
        } => {
            out.push_str(if *rec { "let rec " } else { "let " });
            pat_into(out, pattern);
            if let Some(ty) = ty {
                out.push_str(" : ");
                ty_into(out, ty);
            }
            out.push_str(" = ");
            expr_into(out, value);
        },
        | DeclKind::Test { name, body } => {
            let _ = write!(out, "test {name:?} = ");
            expr_into(out, body);
        },
    }
}

fn attribute_into(
    out: &mut String,
    attr: &Attribute,
) {
    let _ = write!(out, "@{}(", attr.name);
    for arg in &attr.args {
        if let Some(name) = &arg.name {
            let _ = write!(out, "{name}=");
        }
        expr_into(out, &arg.value);
        out.push(',');
    }
    out.push(')');
}

fn type_params_into(
    out: &mut String,
    params: &[TypeParam],
) {
    if params.is_empty() {
        return;
    }
    out.push('<');
    for param in params {
        out.push_str(&param.name);
        if let Some(kind) = param.kind {
            let _ = write!(out, ":{}", kind.as_str());
        }
        out.push(',');
    }
    out.push('>');
}

fn param_into(
    out: &mut String,
    param: &Param,
) {
    pat_into(out, &param.pattern);
    if let Some(ty) = &param.ty {
        out.push_str(" : ");
        ty_into(out, ty);
    }
}

/// The fingerprint of one expression.
#[must_use]
pub fn expression(expr: &Expr) -> String {
    let mut out = String::new();
    expr_into(&mut out, expr);
    out
}

fn expr_into(
    out: &mut String,
    expr: &Expr,
) {
    use std::borrow::Cow;
    enum Task<'a> {
        Expr(&'a Expr),
        Pat(&'a Pat),
        Ty(&'a Ty),
        Param(&'a Param),
        Decl(&'a Decl),
        Unit(&'a aevia_dim::UnitExpr),
        Text(Cow<'a, str>),
        Char(char),
    }
    let mut pending = vec![Task::Expr(expr)];
    while let Some(task) = pending.pop() {
        let expr = match task {
            | Task::Expr(expr) => expr,
            | Task::Pat(value) => {
                pat_into(out, value);
                continue;
            },
            | Task::Ty(value) => {
                ty_into(out, value);
                continue;
            },
            | Task::Param(value) => {
                param_into(out, value);
                continue;
            },
            | Task::Decl(value) => {
                decl_into(out, value);
                continue;
            },
            | Task::Unit(value) => {
                unit_expr_into(out, value);
                continue;
            },
            | Task::Text(value) => {
                out.push_str(&value);
                continue;
            },
            | Task::Char(value) => {
                out.push(value);
                continue;
            },
        };
        let mut actions = Vec::new();
        match &expr.kind {
            | ExprKind::UnitLit => actions.push(Task::Text(("()").into())),
            | ExprKind::Error => actions.push(Task::Text(("<error>").into())),
            | ExprKind::Bool(value) => {
                actions.push(Task::Text(format!("{value}").into()));
            },
            | ExprKind::Int(value) => {
                actions.push(Task::Text(format!("int:{value}").into()));
            },
            | ExprKind::Float(text) => {
                actions.push(Task::Text(format!("float:{text}").into()));
            },
            | ExprKind::Str(pieces) => {
                actions.push(Task::Text(("str(").into()));
                for piece in pieces {
                    match piece {
                        | StrPiece::Text(text) => {
                            actions.push(Task::Text(format!("t{text:?}").into()));
                        },
                        | StrPiece::Expr(inner) => {
                            actions.push(Task::Text(("e{").into()));
                            actions.push(Task::Expr(inner));
                            actions.push(Task::Char('}'));
                        },
                    }
                    actions.push(Task::Char(','));
                }
                actions.push(Task::Char(')'));
            },
            | ExprKind::Quantity { value, unit } => {
                actions.push(Task::Text(("quantity:").into()));
                match value {
                    | NumberLit::Int(value) => {
                        actions.push(Task::Text(format!("{value}").into()));
                    },
                    | NumberLit::Decimal(text) => {
                        actions.push(Task::Text(format!("{text}").into()));
                    },
                }
                actions.push(Task::Char(' '));
                actions.push(Task::Unit(unit));
            },
            | ExprKind::Var(name) => {
                actions.push(Task::Text(format!("var:{name}").into()));
            },
            | ExprKind::QName(path) => {
                actions.push(Task::Text(format!("qname:{}", path.join(".")).into()));
            },
            | ExprKind::Unary { op, operand } => {
                actions.push(Task::Text(format!("unary({op:?},").into()));
                actions.push(Task::Expr(operand));
                actions.push(Task::Char(')'));
            },
            | ExprKind::Binary { op, left, right } => {
                actions.push(Task::Text(format!("binary({op:?},").into()));
                actions.push(Task::Expr(left));
                actions.push(Task::Char(','));
                actions.push(Task::Expr(right));
                actions.push(Task::Char(')'));
            },
            | ExprKind::Call { callee, args } => {
                actions.push(Task::Text(("call(").into()));
                actions.push(Task::Expr(callee));
                actions.push(Task::Char(';'));
                for arg in args {
                    actions.push(Task::Expr(arg));
                    actions.push(Task::Char(','));
                }
                actions.push(Task::Char(')'));
            },
            | ExprKind::TypeApp { callee, args } => {
                actions.push(Task::Text(("typeapp(").into()));
                actions.push(Task::Expr(callee));
                actions.push(Task::Char('<'));
                for arg in args {
                    actions.push(Task::Ty(arg));
                    actions.push(Task::Char(','));
                }
                actions.push(Task::Text((">)").into()));
            },
            | ExprKind::Lambda { params, ret, body } => {
                actions.push(Task::Text(("lambda(").into()));
                for param in params {
                    actions.push(Task::Param(param));
                    actions.push(Task::Char(','));
                }
                actions.push(Task::Char(')'));
                if let Some(ret) = ret {
                    actions.push(Task::Text((" : ").into()));
                    actions.push(Task::Ty(ret));
                }
                actions.push(Task::Text((" => ").into()));
                actions.push(Task::Expr(body));
            },
            | ExprKind::If {
                cond,
                then_branch,
                else_branch,
            } => {
                actions.push(Task::Text(("if(").into()));
                actions.push(Task::Expr(cond));
                actions.push(Task::Char(','));
                actions.push(Task::Expr(then_branch));
                actions.push(Task::Char(','));
                actions.push(Task::Expr(else_branch));
                actions.push(Task::Char(')'));
            },
            | ExprKind::Match { scrutinee, arms } => {
                actions.push(Task::Text(("match(").into()));
                actions.push(Task::Expr(scrutinee));
                actions.push(Task::Char(';'));
                for arm in arms {
                    actions.push(Task::Pat(&arm.pattern));
                    if let Some(guard) = &arm.guard {
                        actions.push(Task::Text((" if ").into()));
                        actions.push(Task::Expr(guard));
                    }
                    actions.push(Task::Text((" -> ").into()));
                    actions.push(Task::Expr(&arm.body));
                    actions.push(Task::Char('|'));
                }
                actions.push(Task::Char(')'));
            },
            | ExprKind::LetIn {
                pattern,
                ty,
                value,
                body,
            } => {
                actions.push(Task::Text(("letin(").into()));
                actions.push(Task::Pat(pattern));
                if let Some(ty) = ty {
                    actions.push(Task::Text((" : ").into()));
                    actions.push(Task::Ty(ty));
                }
                actions.push(Task::Text((" = ").into()));
                actions.push(Task::Expr(value));
                actions.push(Task::Text((" in ").into()));
                actions.push(Task::Expr(body));
                actions.push(Task::Char(')'));
            },
            | ExprKind::Block { stmts, tail } => {
                actions.push(Task::Text(("block{").into()));
                for stmt in stmts {
                    match stmt {
                        | BlockStmt::Let {
                            pattern,
                            ty,
                            value,
                            rec,
                            ..
                        } => {
                            actions
                                .push(Task::Text((if *rec { "let rec " } else { "let " }).into()));
                            actions.push(Task::Pat(pattern));
                            if let Some(ty) = ty {
                                actions.push(Task::Text((" : ").into()));
                                actions.push(Task::Ty(ty));
                            }
                            actions.push(Task::Text((" = ").into()));
                            actions.push(Task::Expr(value));
                            actions.push(Task::Char(';'));
                        },
                        | BlockStmt::Fn(decl) => {
                            actions.push(Task::Decl(decl));
                            actions.push(Task::Char(';'));
                        },
                    }
                }
                actions.push(Task::Text(("tail ").into()));
                match tail {
                    | Some(tail) => actions.push(Task::Expr(tail)),
                    | None => actions.push(Task::Text(("<none>").into())),
                }
                actions.push(Task::Char('}'));
            },
            | ExprKind::Do { stmts } => {
                actions.push(Task::Text(("do{").into()));
                for stmt in stmts {
                    match stmt {
                        | DoStmt::Let {
                            pattern, ty, value, ..
                        } => {
                            actions.push(Task::Text(("let ").into()));
                            actions.push(Task::Pat(pattern));
                            if let Some(ty) = ty {
                                actions.push(Task::Text((" : ").into()));
                                actions.push(Task::Ty(ty));
                            }
                            actions.push(Task::Text((" = ").into()));
                            actions.push(Task::Expr(value));
                        },
                        | DoStmt::Bind { pattern, value, .. } => {
                            actions.push(Task::Pat(pattern));
                            actions.push(Task::Text((" <- ").into()));
                            actions.push(Task::Expr(value));
                        },
                        | DoStmt::Expr(expr) => actions.push(Task::Expr(expr)),
                    }
                    actions.push(Task::Char(';'));
                }
                actions.push(Task::Char('}'));
            },
            | ExprKind::Record { base, fields } => {
                actions.push(Task::Text(("record(").into()));
                if let Some(base) = base {
                    actions.push(Task::Expr(base));
                } else {
                    actions.push(Task::Char('-'));
                }
                actions.push(Task::Char(';'));
                for (name, value) in fields {
                    actions.push(Task::Text(format!("{name}=").into()));
                    actions.push(Task::Expr(value));
                    actions.push(Task::Char(','));
                }
                actions.push(Task::Char(')'));
            },
            | ExprKind::Field { base, name } => {
                actions.push(Task::Text(("field(").into()));
                actions.push(Task::Expr(base));
                actions.push(Task::Text(format!(".{name})").into()));
            },
            | ExprKind::Tuple(items) => {
                actions.push(Task::Text(("tuple(").into()));
                for item in items {
                    actions.push(Task::Expr(item));
                    actions.push(Task::Char(','));
                }
                actions.push(Task::Char(')'));
            },
            | ExprKind::Tensor(items) => {
                actions.push(Task::Text(("tensor[").into()));
                for item in items {
                    actions.push(Task::Expr(item));
                    actions.push(Task::Char(','));
                }
                actions.push(Task::Char(']'));
            },
            | ExprKind::Index { base, indices } => {
                actions.push(Task::Text(("index(").into()));
                actions.push(Task::Expr(base));
                actions.push(Task::Char(';'));
                for index in indices {
                    match index {
                        | SliceAst::Index(expr) => actions.push(Task::Expr(expr)),
                        | SliceAst::Range {
                            start,
                            end,
                            inclusive,
                            step,
                        } => {
                            actions.push(Task::Text(("range(").into()));
                            if let Some(start) = start {
                                actions.push(Task::Expr(start));
                            }
                            actions
                                .push(Task::Text((if *inclusive { "..=" } else { ".." }).into()));
                            if let Some(end) = end {
                                actions.push(Task::Expr(end));
                            }
                            if let Some(step) = step {
                                actions.push(Task::Text((":").into()));
                                actions.push(Task::Expr(step));
                            }
                            actions.push(Task::Char(')'));
                        },
                    }
                    actions.push(Task::Char(','));
                }
                actions.push(Task::Char(')'));
            },
            | ExprKind::Annot { expr, ty } => {
                actions.push(Task::Text(("annot(").into()));
                actions.push(Task::Expr(expr));
                actions.push(Task::Text((" : ").into()));
                actions.push(Task::Ty(ty));
                actions.push(Task::Char(')'));
            },
            | ExprKind::Placeholder => actions.push(Task::Text(("_").into())),
            | ExprKind::Try(inner) => {
                actions.push(Task::Text(("try(").into()));
                actions.push(Task::Expr(inner));
                actions.push(Task::Char(')'));
            },
            | ExprKind::Factorial { expr, double } => {
                actions.push(Task::Text(format!("factorial({double},").into()));
                actions.push(Task::Expr(expr));
                actions.push(Task::Char(')'));
            },
        }
        pending.extend(actions.into_iter().rev());
    }
}

fn pat_into(
    out: &mut String,
    pat: &Pat,
) {
    match &pat.kind {
        | PatKind::Wildcard => out.push('_'),
        | PatKind::Error => out.push_str("<error>"),
        | PatKind::Var(name) => {
            let _ = write!(out, "pvar:{name}");
        },
        | PatKind::Ctor { name, args } => {
            let _ = write!(out, "ctor:{name}(");
            for arg in args {
                pat_into(out, arg);
                out.push(',');
            }
            out.push(')');
        },
        | PatKind::Record(fields) => {
            out.push_str("precord(");
            for (name, sub) in fields {
                let _ = write!(out, "{name}=");
                if let Some(sub) = sub {
                    pat_into(out, sub);
                } else {
                    out.push('-');
                }
                out.push(',');
            }
            out.push(')');
        },
        | PatKind::Tuple(items) => {
            out.push_str("ptuple(");
            for item in items {
                pat_into(out, item);
                out.push(',');
            }
            out.push(')');
        },
        | PatKind::Tensor { items, rest } => {
            out.push_str("ptensor[");
            for item in items {
                pat_into(out, item);
                out.push(',');
            }
            let _ = write!(out, "..{}]", rest.as_deref().unwrap_or("-"));
        },
        | PatKind::Lit(lit) => {
            match lit {
                | PatLit::Int(value) => {
                    let _ = write!(out, "plit:int:{value}");
                },
                | PatLit::Str(text) => {
                    let _ = write!(out, "plit:str:{text:?}");
                },
                | PatLit::Bool(value) => {
                    let _ = write!(out, "plit:bool:{value}");
                },
                | PatLit::Unit => out.push_str("plit:()"),
            }
        },
        | PatKind::As { pattern, name } => {
            out.push_str("pas(");
            pat_into(out, pattern);
            let _ = write!(out, " as {name})");
        },
        | PatKind::Annot { pattern, ty } => {
            out.push_str("pannot(");
            pat_into(out, pattern);
            out.push_str(" : ");
            ty_into(out, ty);
            out.push(')');
        },
    }
}

fn ty_into(
    out: &mut String,
    ty: &Ty,
) {
    match &ty.kind {
        | TyKind::Int => out.push_str("Int"),
        | TyKind::Real(dim) => {
            out.push_str("Real[");
            dim_into(out, dim);
            out.push(']');
        },
        | TyKind::Float(dim) => {
            out.push_str("Float[");
            dim_into(out, dim);
            out.push(']');
        },
        | TyKind::Complex(dim) => {
            out.push_str("Complex[");
            dim_into(out, dim);
            out.push(']');
        },
        | TyKind::Bool => out.push_str("Bool"),
        | TyKind::Prop => out.push_str("Prop"),
        | TyKind::Str => out.push_str("String"),
        | TyKind::Unit => out.push_str("()"),
        | TyKind::Var(name) => out.push_str(name),
        | TyKind::Tensor { shape, elem } => {
            out.push_str("Tensor<");
            shape_into(out, shape);
            out.push(',');
            ty_into(out, elem);
            out.push('>');
        },
        | TyKind::Matrix { rows, cols, elem } => {
            out.push_str("Matrix<");
            nat_into(out, rows);
            out.push(',');
            nat_into(out, cols);
            out.push(',');
            ty_into(out, elem);
            out.push('>');
        },
        | TyKind::Sparse { format, shape, elem } => {
            let _ = write!(out, "Sparse<{format},");
            shape_into(out, shape);
            out.push(',');
            ty_into(out, elem);
            out.push('>');
        },
        | TyKind::Row(items) => {
            out.push_str("row(");
            for item in items {
                ty_into(out, item);
                out.push('+');
            }
            out.push(')');
        },
        | TyKind::DimArg(dim) => {
            out.push_str("dimarg(");
            dim_into(out, dim);
            out.push(')');
        },
        | TyKind::NatArg(nat) => {
            out.push_str("natarg(");
            nat_into(out, nat);
            out.push(')');
        },
        | TyKind::ShapeArg(shape) => {
            out.push_str("shapearg(");
            shape_into(out, shape);
            out.push(')');
        },
        | TyKind::Point(dim) => {
            out.push_str("Point[");
            dim_into(out, dim);
            out.push(']');
        },
        | TyKind::UnitOf(dim) => {
            out.push_str("Unit[");
            dim_into(out, dim);
            out.push(']');
        },
        | TyKind::Option(inner) => {
            out.push_str("Option<");
            ty_into(out, inner);
            out.push('>');
        },
        | TyKind::Result(ok, err) => {
            out.push_str("Result<");
            ty_into(out, ok);
            out.push(',');
            ty_into(out, err);
            out.push('>');
        },
        | TyKind::Set(inner) => {
            out.push_str("Set<");
            ty_into(out, inner);
            out.push('>');
        },
        | TyKind::Table(row) => {
            out.push_str("Table<");
            ty_into(out, row);
            out.push('>');
        },
        | TyKind::Dyn(rep) => {
            let _ = write!(out, "Dyn<{rep}>");
        },
        | TyKind::RateOf(inner) => {
            out.push_str("RateOf<");
            ty_into(out, inner);
            out.push('>');
        },
        | TyKind::DiffusionOf(inner) => {
            out.push_str("DiffusionOf<");
            ty_into(out, inner);
            out.push('>');
        },
        | TyKind::SymFun(domain, codomain) => {
            out.push_str("SymFun<");
            dim_into(out, domain);
            out.push(',');
            dim_into(out, codomain);
            out.push('>');
        },
        | TyKind::Io(inner) => {
            out.push_str("IO(");
            ty_into(out, inner);
            out.push(')');
        },
        | TyKind::Fn(args, ret) => {
            out.push_str("fn(");
            for arg in args {
                ty_into(out, arg);
                out.push(',');
            }
            out.push_str(")->");
            ty_into(out, ret);
        },
        | TyKind::Record { fields, rest } => {
            out.push_str("rec{");
            for (name, ty) in fields {
                let _ = write!(out, "{name}:");
                ty_into(out, ty);
                out.push(',');
            }
            let _ = write!(out, "..{}}}", rest.as_deref().unwrap_or("-"));
        },
        | TyKind::Tuple(items) => {
            out.push_str("tup(");
            for item in items {
                ty_into(out, item);
                out.push(',');
            }
            out.push(')');
        },
        | TyKind::Named { path, args } => {
            let _ = write!(out, "named:{}", path.join("."));
            if !args.is_empty() {
                out.push('<');
                for arg in args {
                    ty_into(out, arg);
                    out.push(',');
                }
                out.push('>');
            }
        },
        | TyKind::Forall(params, body) => {
            out.push_str("forall ");
            type_params_into(out, params);
            out.push('.');
            ty_into(out, body);
        },
    }
}

fn shape_into(
    out: &mut String,
    shape: &ShapeAst,
) {
    match shape {
        | ShapeAst::Rank(nat) => {
            out.push_str("rank(");
            nat_into(out, nat);
            out.push(')');
        },
        | ShapeAst::Refined(items) => {
            out.push_str("refined[");
            for item in items {
                match item {
                    | ShapeItem::Known(nat) => nat_into(out, nat),
                    | ShapeItem::Unknown => out.push('?'),
                }
                out.push(',');
            }
            out.push(']');
        },
    }
}

fn nat_into(
    out: &mut String,
    nat: &NatAst,
) {
    match nat {
        | NatAst::Lit(value) => {
            let _ = write!(out, "{value}");
        },
        | NatAst::Var(name) => out.push_str(name),
        | NatAst::Add(left, right) => {
            out.push('(');
            nat_into(out, left);
            out.push('+');
            nat_into(out, right);
            out.push(')');
        },
        | NatAst::Mul(left, right) => {
            out.push('(');
            nat_into(out, left);
            out.push('*');
            nat_into(out, right);
            out.push(')');
        },
    }
}

fn dim_into(
    out: &mut String,
    dim: &DimAst,
) {
    match dim {
        | DimAst::One => out.push('1'),
        | DimAst::Name(name) => out.push_str(name),
        | DimAst::Mul(left, right) => {
            out.push('(');
            dim_into(out, left);
            out.push('*');
            dim_into(out, right);
            out.push(')');
        },
        | DimAst::Div(left, right) => {
            out.push('(');
            dim_into(out, left);
            out.push('/');
            dim_into(out, right);
            out.push(')');
        },
        | DimAst::Pow(base, exponent) => {
            out.push('(');
            dim_into(out, base);
            let _ = write!(out, "^{}/{}", exponent.numer(), exponent.denom());
            out.push(')');
        },
        | DimAst::Paren(inner) => {
            out.push_str("paren(");
            dim_into(out, inner);
            out.push(')');
        },
    }
}

fn unit_expr_into(
    out: &mut String,
    unit: &aevia_dim::UnitExpr,
) {
    use aevia_dim::UnitExpr;
    match unit {
        | UnitExpr::Name(name) => out.push_str(name),
        | UnitExpr::Percent => out.push('%'),
        | UnitExpr::Mul(left, right) => {
            out.push('(');
            unit_expr_into(out, left);
            out.push('*');
            unit_expr_into(out, right);
            out.push(')');
        },
        | UnitExpr::Div(left, right) => {
            out.push('(');
            unit_expr_into(out, left);
            out.push('/');
            unit_expr_into(out, right);
            out.push(')');
        },
        | UnitExpr::Pow(base, exponent) => {
            out.push('(');
            unit_expr_into(out, base);
            let _ = write!(out, "^{}/{}", exponent.numer(), exponent.denom());
            out.push(')');
        },
        | UnitExpr::Paren(inner) => {
            out.push_str("paren(");
            unit_expr_into(out, inner);
            out.push(')');
        },
    }
}
