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
//! all and are checked separately (see [`crate::comments`]).

use aevia_syntax::ast::*;
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
    match &expr.kind {
        | ExprKind::UnitLit => out.push_str("()"),
        | ExprKind::Error => out.push_str("<error>"),
        | ExprKind::Bool(value) => {
            let _ = write!(out, "{value}");
        },
        | ExprKind::Int(value) => {
            let _ = write!(out, "int:{value}");
        },
        | ExprKind::Float(text) => {
            let _ = write!(out, "float:{text}");
        },
        | ExprKind::Str(pieces) => {
            out.push_str("str(");
            for piece in pieces {
                match piece {
                    | StrPiece::Text(text) => {
                        let _ = write!(out, "t{text:?}");
                    },
                    | StrPiece::Expr(inner) => {
                        out.push_str("e{");
                        expr_into(out, inner);
                        out.push('}');
                    },
                }
                out.push(',');
            }
            out.push(')');
        },
        | ExprKind::Quantity { value, unit } => {
            out.push_str("quantity:");
            match value {
                | NumberLit::Int(value) => {
                    let _ = write!(out, "{value}");
                },
                | NumberLit::Decimal(text) => {
                    let _ = write!(out, "{text}");
                },
            }
            out.push(' ');
            unit_expr_into(out, unit);
        },
        | ExprKind::Var(name) => {
            let _ = write!(out, "var:{name}");
        },
        | ExprKind::QName(path) => {
            let _ = write!(out, "qname:{}", path.join("."));
        },
        | ExprKind::Unary { op, operand } => {
            let _ = write!(out, "unary({op:?},");
            expr_into(out, operand);
            out.push(')');
        },
        | ExprKind::Binary { op, left, right } => {
            let _ = write!(out, "binary({op:?},");
            expr_into(out, left);
            out.push(',');
            expr_into(out, right);
            out.push(')');
        },
        | ExprKind::Call { callee, args } => {
            out.push_str("call(");
            expr_into(out, callee);
            out.push(';');
            for arg in args {
                expr_into(out, arg);
                out.push(',');
            }
            out.push(')');
        },
        | ExprKind::TypeApp { callee, args } => {
            out.push_str("typeapp(");
            expr_into(out, callee);
            out.push('<');
            for arg in args {
                ty_into(out, arg);
                out.push(',');
            }
            out.push_str(">)");
        },
        | ExprKind::Lambda { params, ret, body } => {
            out.push_str("lambda(");
            for param in params {
                param_into(out, param);
                out.push(',');
            }
            out.push(')');
            if let Some(ret) = ret {
                out.push_str(" : ");
                ty_into(out, ret);
            }
            out.push_str(" => ");
            expr_into(out, body);
        },
        | ExprKind::If {
            cond,
            then_branch,
            else_branch,
        } => {
            out.push_str("if(");
            expr_into(out, cond);
            out.push(',');
            expr_into(out, then_branch);
            out.push(',');
            expr_into(out, else_branch);
            out.push(')');
        },
        | ExprKind::Match { scrutinee, arms } => {
            out.push_str("match(");
            expr_into(out, scrutinee);
            out.push(';');
            for arm in arms {
                pat_into(out, &arm.pattern);
                if let Some(guard) = &arm.guard {
                    out.push_str(" if ");
                    expr_into(out, guard);
                }
                out.push_str(" -> ");
                expr_into(out, &arm.body);
                out.push('|');
            }
            out.push(')');
        },
        | ExprKind::LetIn {
            pattern,
            ty,
            value,
            body,
        } => {
            out.push_str("letin(");
            pat_into(out, pattern);
            if let Some(ty) = ty {
                out.push_str(" : ");
                ty_into(out, ty);
            }
            out.push_str(" = ");
            expr_into(out, value);
            out.push_str(" in ");
            expr_into(out, body);
            out.push(')');
        },
        | ExprKind::Block { stmts, tail } => {
            out.push_str("block{");
            for stmt in stmts {
                match stmt {
                    | BlockStmt::Let {
                        pattern,
                        ty,
                        value,
                        rec,
                        ..
                    } => {
                        out.push_str(if *rec { "let rec " } else { "let " });
                        pat_into(out, pattern);
                        if let Some(ty) = ty {
                            out.push_str(" : ");
                            ty_into(out, ty);
                        }
                        out.push_str(" = ");
                        expr_into(out, value);
                        out.push(';');
                    },
                    | BlockStmt::Fn(decl) => {
                        decl_into(out, decl);
                        out.push(';');
                    },
                }
            }
            out.push_str("tail ");
            match tail {
                | Some(tail) => expr_into(out, tail),
                | None => out.push_str("<none>"),
            }
            out.push('}');
        },
        | ExprKind::Do { stmts } => {
            out.push_str("do{");
            for stmt in stmts {
                match stmt {
                    | DoStmt::Let {
                        pattern, ty, value, ..
                    } => {
                        out.push_str("let ");
                        pat_into(out, pattern);
                        if let Some(ty) = ty {
                            out.push_str(" : ");
                            ty_into(out, ty);
                        }
                        out.push_str(" = ");
                        expr_into(out, value);
                    },
                    | DoStmt::Bind { pattern, value, .. } => {
                        pat_into(out, pattern);
                        out.push_str(" <- ");
                        expr_into(out, value);
                    },
                    | DoStmt::Expr(expr) => expr_into(out, expr),
                }
                out.push(';');
            }
            out.push('}');
        },
        | ExprKind::Record { base, fields } => {
            out.push_str("record(");
            if let Some(base) = base {
                expr_into(out, base);
            } else {
                out.push('-');
            }
            out.push(';');
            for (name, value) in fields {
                let _ = write!(out, "{name}=");
                expr_into(out, value);
                out.push(',');
            }
            out.push(')');
        },
        | ExprKind::Field { base, name } => {
            out.push_str("field(");
            expr_into(out, base);
            let _ = write!(out, ".{name})");
        },
        | ExprKind::Tuple(items) => {
            out.push_str("tuple(");
            for item in items {
                expr_into(out, item);
                out.push(',');
            }
            out.push(')');
        },
        | ExprKind::Tensor(items) => {
            out.push_str("tensor[");
            for item in items {
                expr_into(out, item);
                out.push(',');
            }
            out.push(']');
        },
        | ExprKind::Index { base, indices } => {
            out.push_str("index(");
            expr_into(out, base);
            out.push(';');
            for index in indices {
                match index {
                    | SliceAst::Index(expr) => expr_into(out, expr),
                    | SliceAst::Range {
                        start,
                        end,
                        inclusive,
                        step,
                    } => {
                        out.push_str("range(");
                        if let Some(start) = start {
                            expr_into(out, start);
                        }
                        out.push_str(if *inclusive { "..=" } else { ".." });
                        if let Some(end) = end {
                            expr_into(out, end);
                        }
                        if let Some(step) = step {
                            out.push_str(":");
                            expr_into(out, step);
                        }
                        out.push(')');
                    },
                }
                out.push(',');
            }
            out.push(')');
        },
        | ExprKind::Annot { expr, ty } => {
            out.push_str("annot(");
            expr_into(out, expr);
            out.push_str(" : ");
            ty_into(out, ty);
            out.push(')');
        },
        | ExprKind::Placeholder => out.push_str("_"),
        | ExprKind::Try(inner) => {
            out.push_str("try(");
            expr_into(out, inner);
            out.push(')');
        },
        | ExprKind::Factorial { expr, double } => {
            let _ = write!(out, "factorial({double},");
            expr_into(out, expr);
            out.push(')');
        },
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
