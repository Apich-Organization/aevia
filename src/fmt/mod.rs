//! Source formatter (AST pretty-printer). Phase 5.

use crate::ast::{
    Attribute, BinOp, DimExpr, Expr, Field, FunctionBody, Item, MatchArm, OpProperties, Param, Pattern, Spanned, Stmt,
    UnOp, Visibility,
};
use crate::diagnostics::{AeviaError, AeviaResult};
use crate::parser::items::parse_source;
use std::fmt::Write as _;
use std::path::Path;

/// Format a source string.
pub fn format_source(input: &str) -> Result<String, String> {
    let file = parse_source(input)?;
    Ok(format_file(&file))
}

/// Parse and write formatted source back to the same path if content changed.
pub fn format_file_in_place(path: &Path) -> AeviaResult<bool> {
    let input = std::fs::read_to_string(path).map_err(|e| AeviaError::io(path, e))?;
    let formatted = format_source(&input)
        .map_err(|e| AeviaError::message(format!("format error in {}: {e}", path.display())))?;
    if formatted != input {
        std::fs::write(path, &formatted).map_err(|e| AeviaError::io(path, e))?;
        Ok(true)
    } else {
        Ok(false)
    }
}

fn format_file(file: &crate::ast::SourceFile) -> String {
    let mut out = String::new();
    for (i, item) in file.items.iter().enumerate() {
        if i > 0 {
            out.push('\n');
        }
        format_item(&mut out, &item.node, 0);
        if item_needs_semicolon(&item.node) {
            out.push(';');
        }
    }
    if !out.ends_with('\n') {
        out.push('\n');
    }
    out
}

fn item_needs_semicolon(item: &Item) -> bool {
    !matches!(
        item,
        Item::Module { body: Some(_), .. }
            | Item::Function {
                body: FunctionBody::Block(_),
                ..
            }
            | Item::CustomOp { .. }
            | Item::MacroDef { .. }
    )
}

fn format_item(out: &mut String, item: &Item, indent: usize) {
    let pad = "    ".repeat(indent);
    match item {
        Item::Use { path, alias } => {
            let _ = write!(out, "{pad}use {}", path.join("::"));
            if let Some(a) = alias {
                let _ = write!(out, " as {a}");
            }
        }
        Item::TypeAlias { name, dimension_expr, .. } => {
            let _ = write!(
                out,
                "{pad}type {name} = {}",
                format_dim(&dimension_expr.node)
            );
        }
        Item::Struct { name, fields, visibility, .. } => {
            let _ = write!(out, "{}{}struct {name} {{ ", vis(*visibility), pad);
            for (i, field) in fields.iter().enumerate() {
                if i > 0 {
                    out.push_str(", ");
                }
                format_field(out, field);
            }
            out.push('}');
        }
        Item::Function {
            name,
            params,
            return_type,
            body,
            visibility,
            attributes,
            ..
        } => {
            for attr in attributes {
                format_attribute(out, attr);
                out.push('\n');
                let _ = write!(out, "{pad}");
            }
            let _ = write!(out, "{}{}fn {name}(", vis(*visibility), pad);
            format_params(out, params);
            if let Some(ret) = return_type {
                let _ = write!(out, ") -> {}", format_dim(&ret.node));
            } else {
                out.push(')');
            }
            match body {
                FunctionBody::Expression(expr) => {
                    let _ = write!(out, " := {}", format_expr(&expr.node));
                }
                FunctionBody::Block(stmts) => {
                    out.push(' ');
                    format_block(out, stmts, indent);
                }
            }
        }
        Item::CustomOp {
            name,
            param_name,
            input_dim,
            output_dim,
            visibility,
            properties,
            ..
        } => {
            let _ = write!(
                out,
                "{}{}op {name}({param_name}: {} -> {}) ",
                vis(*visibility),
                pad,
                format_dim(&input_dim.node),
                format_dim(&output_dim.node)
            );
            format_op_body(out, properties, indent);
        }
        Item::Module { name, body, visibility, .. } => {
            let _ = write!(out, "{}{}mod {name}", vis(*visibility), pad);
            match body {
                None => {}
                Some(items) => {
                    out.push(' ');
                    format_module_block(out, items, indent);
                }
            }
        }
        Item::MacroDef { name, rules } => {
            let _ = write!(out, "{pad}macro_rules! {name} {{");
            for rule in rules {
                let _ = write!(out, "\n{}    ({}) => {{ {} }}", pad, rule.pattern, rule.replacement);
            }
            let _ = write!(out, "\n{pad}}}");
        }
    }
}

fn format_module_block(out: &mut String, items: &[Spanned<Item>], indent: usize) {
    out.push('{');
    out.push('\n');
    for (i, item) in items.iter().enumerate() {
        let _ = write!(out, "{}", "    ".repeat(indent + 1));
        format_item(out, &item.node, indent + 1);
        out.push(';');
        out.push('\n');
        if i + 1 < items.len() {
            out.push('\n');
        }
    }
    let _ = write!(out, "{}", "    ".repeat(indent));
    out.push('}');
}

fn format_block(out: &mut String, stmts: &[Spanned<Stmt>], indent: usize) {
    out.push('{');
    out.push('\n');
    for stmt in stmts {
        let _ = write!(out, "{}", "    ".repeat(indent + 1));
        format_stmt(out, &stmt.node, indent + 1);
        out.push('\n');
    }
    let _ = write!(out, "{}", "    ".repeat(indent));
    out.push('}');
}

fn format_stmt(out: &mut String, stmt: &Stmt, _indent: usize) {
    match stmt {
        Stmt::Let { name, declared_type, init, is_mut } => {
            let _ = write!(out, "let ");
            if *is_mut {
                out.push_str("mut ");
            }
            let _ = write!(out, "{name}");
            if let Some(ty) = declared_type {
                let _ = write!(out, ": {}", format_dim(&ty.node));
            }
            let _ = write!(out, " = {}", format_expr(&init.node));
            out.push(';');
        }
        Stmt::Assign { target, value } => {
            let _ = write!(out, "{target} = {}", format_expr(&value.node));
            out.push(';');
        }
        Stmt::Expr(expr) => {
            let _ = write!(out, "{}", format_expr(&expr.node));
            out.push(';');
        }
        Stmt::Break => out.push_str("break;"),
        Stmt::Continue => out.push_str("continue;"),
    }
}

fn format_op_body(out: &mut String, props: &OpProperties, indent: usize) {
    out.push('{');
    out.push('\n');
    let pad = "    ".repeat(indent + 1);
    let _ = writeln!(out, "{pad}properties {{");
    if props.vectorizable {
        let _ = writeln!(out, "{pad}    vectorizable: true,");
    }
    if props.commutative {
        let _ = writeln!(out, "{pad}    commutative: true,");
    }
    if props.associative {
        let _ = writeln!(out, "{pad}    associative: true,");
    }
    if let Some(cost) = props.cost {
        let _ = writeln!(out, "{pad}    cost: {cost},");
    }
    let _ = writeln!(out, "{pad}}}");
    let _ = write!(out, "{}", "    ".repeat(indent));
    out.push('}');
}

fn format_params(out: &mut String, params: &[Param]) {
    for (i, param) in params.iter().enumerate() {
        if i > 0 {
            out.push_str(", ");
        }
        let _ = write!(out, "{}: {}", param.name, format_dim(&param.dim.node));
    }
}

fn format_field(out: &mut String, field: &Field) {
    let _ = write!(
        out,
        "{}{}: {}",
        vis(field.visibility),
        field.name,
        format_dim(&field.dim.node)
    );
}

fn format_attribute(out: &mut String, attr: &Attribute) {
    let _ = write!(out, "#[{}", attr.name);
    if !attr.args.is_empty() {
        out.push('(');
        for (i, arg) in attr.args.iter().enumerate() {
            if i > 0 {
                out.push_str(", ");
            }
            let _ = write!(out, "{arg}");
        }
        out.push(')');
    }
    out.push(']');
}

fn vis(v: Visibility) -> &'static str {
    match v {
        Visibility::Private => "",
        Visibility::Crate => "pub(crate) ",
        Visibility::Public => "pub ",
    }
}

fn format_dim(expr: &DimExpr) -> String {
    crate::doc::format_dim(expr)
}

fn format_expr(expr: &Expr) -> String {
    match expr {
        Expr::Literal { value, suffix } => {
            let mut s = value.to_string();
            if let Some(suf) = suffix {
                s.push('_');
                s.push_str(&format_dim(&suf.node));
            }
            s
        }
        Expr::Variable(name) => name.clone(),
        Expr::BinaryOp { op, lhs, rhs } => format!(
            "{} {} {}",
            format_expr(&lhs.node),
            bin_op(*op),
            format_expr(&rhs.node)
        ),
        Expr::UnaryOp { op: UnOp::Neg, expr } => format!("-{}", format_expr(&expr.node)),
        Expr::Call { func, args } => {
            let args: Vec<_> = args.iter().map(|a| format_expr(&a.node)).collect();
            format!("{}({})", func, args.join(", "))
        }
        Expr::Block(stmts) => {
            let mut s = String::from("{ ");
            for stmt in stmts {
                format_stmt(&mut s, &stmt.node, 0);
                s.push(' ');
            }
            s.push('}');
            s
        }
        Expr::If { cond, then_branch, else_branch } => {
            let mut s = format!(
                "if {} {{ {} }}",
                format_expr(&cond.node),
                format_expr(&then_branch.node)
            );
            if let Some(e) = else_branch {
                // If the else branch is an if, format as elseif.
                if let Expr::If { cond: next_cond, then_branch: next_then, else_branch: next_else } = &e.node {
                    s.push_str(&format!(" elseif {} {{ {} }}", format_expr(&next_cond.node), format_expr(&next_then.node)));
                    let mut current_else = next_else;
                    while let Some(next_node) = current_else {
                        if let Expr::If { cond: next_cond, then_branch: next_then, else_branch: next_else } = &next_node.node {
                            s.push_str(&format!(" elseif {} {{ {} }}", format_expr(&next_cond.node), format_expr(&next_then.node)));
                            current_else = next_else;
                        } else {
                            s.push_str(&format!(" else {{ {} }}", format_expr(&next_node.node)));
                            break;
                        }
                    }
                } else {
                    s.push_str(&format!(" else {{ {} }}", format_expr(&e.node)));
                }
            }
            s
        }
        Expr::Loop { body } => {
            let mut s = String::from("loop { ");
            for stmt in body {
                format_stmt(&mut s, &stmt.node, 0);
                s.push(' ');
            }
            s.push_str("}");
            s
        }
        Expr::While { cond, body } => {
            let mut s = format!("while {} {{ ", format_expr(&cond.node));
            for stmt in body {
                format_stmt(&mut s, &stmt.node, 0);
                s.push(' ');
            }
            s.push_str("}");
            s
        }
        Expr::For { var, start, end, body } => {
            let mut s = format!("for {var} in {}..{} {{ ", format_expr(&start.node), format_expr(&end.node));
            for stmt in body {
                format_stmt(&mut s, &stmt.node, 0);
                s.push(' ');
            }
            s.push_str("}");
            s
        }
        Expr::Match { scrutinee, arms } => {
            let mut s = format!("match {} {{ ", format_expr(&scrutinee.node));
            for (i, arm) in arms.iter().enumerate() {
                if i > 0 {
                    s.push_str(", ");
                }
                let pat_str = match &arm.pattern {
                    Pattern::Wildcard => "_".to_string(),
                    Pattern::Literal(val) => val.to_string(),
                    Pattern::Binding { name, type_guard: None } => name.clone(),
                    Pattern::Binding { name, type_guard: Some(tg) } => {
                        format!("{name}: {}", format_dim(&tg.node))
                    }
                };
                s.push_str(&format!("{pat_str} => {}", format_expr(&arm.body.node)));
            }
            s.push_str(" }");
            s
        }
        Expr::UnsafeTransmute { expr, assume_unit, assume_layout } => {
            let mut s = format!("unsafe transmute({})", format_expr(&expr.node));
            if assume_unit.is_some() || assume_layout.is_some() {
                s.push(' ');
                s.push('{');
                if let Some(u) = assume_unit {
                    let _ = write!(s, " assume_unit: {}", format_dim(&u.node),);
                }
                if let Some(layout) = assume_layout {
                    if assume_unit.is_some() {
                        s.push(',');
                    }
                    let _ = write!(s, " assume_layout: {layout}");
                }
                s.push('}');
            }
            s
        }
        Expr::MacroCall { name, args } => format!("{name}!({args})"),
        Expr::Print { expr } => format!("print({})", format_expr(&expr.node)),
        Expr::Log { message } => format!("log(\"{message}\")"),
    }
}

fn bin_op(op: BinOp) -> &'static str {
    match op {
        BinOp::Add => "+",
        BinOp::Sub => "-",
        BinOp::Mul => "*",
        BinOp::Div => "/",
        BinOp::Pow => "^",
        BinOp::Mod => "%",
        BinOp::Lt => "<",
        BinOp::Gt => ">",
        BinOp::Eq => "==",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn formats_type_alias() {
        let out = format_source("type V = m / s;").unwrap();
        assert!(out.contains("type V = m / s"));
    }

    #[test]
    fn formats_control_flow() {
        let src = "fn test(x: m) {\n    while x < 10.0_m {\n        let x = x + 1.0_m;\n        continue;\n    };\n}";
        let out = format_source(src).unwrap();
        assert!(out.contains("while x < 10.0_m { let x = x + 1.0_m; continue; };"));
    }

    #[test]
    fn formats_for_and_match() {
        let src = "fn test() {\n    for i in 0..10 {\n        match i {\n            1 => 1.0,\n            _ => 0.0,\n        };\n    };\n}";
        let out = format_source(src).unwrap();
        assert!(out.contains("for i in 0..10 { match i { 1 => 1.0, _ => 0.0 }; };"));
    }
}
