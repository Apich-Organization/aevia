//! Macro expansion pre-pass.
//!
//! Runs after parsing and before type-checking. Collects all `Item::MacroDef`
//! entries, then walks every function body and replaces `Expr::MacroCall` nodes
//! with the result of simple `$var` substitution followed by re-parsing.

use std::collections::HashMap;
use crate::ast::{Expr, FunctionBody, Item, MacroRule, SourceFile, Spanned, Stmt};
use crate::diagnostics::{AeviaError, AeviaResult};
use crate::parser::expr::parse_expr;

/// Expand all macro calls in `file` in-place.
///
/// Any `Item::MacroDef` in the file is used as a source of rules.
/// After expansion, unexpanded `Expr::MacroCall` and `Item::MacroCall` nodes should no longer exist.
pub fn expand(file: &mut SourceFile) -> AeviaResult<()> {
    let macros = collect_macros(&file.items);
    // First pass: expand item-level macro calls (produces new items).
    expand_items(file, &macros)?;
    // Second pass: expand expression-level macro calls within function bodies.
    for item in &mut file.items {
        expand_item(&mut item.node, &macros)?;
    }
    Ok(())
}

/// Expand top-level `Item::MacroCall` nodes by re-parsing the expansion result
/// as a list of items and splicing them into the file.
fn expand_items(file: &mut SourceFile, macros: &HashMap<String, Vec<MacroRule>>) -> AeviaResult<()> {
    let mut new_items: Vec<Spanned<Item>> = Vec::new();
    for item in std::mem::take(&mut file.items) {
        if let Item::MacroCall { name, args } = &item.node {
            let expanded = apply_macro_items(name, args, macros)?;
            new_items.extend(expanded);
        } else {
            new_items.push(item);
        }
    }
    file.items = new_items;
    Ok(())
}

/// Apply a macro and parse the result as a list of items (SourceFile).
fn apply_macro_items(
    name: &str,
    args: &str,
    macros: &HashMap<String, Vec<MacroRule>>,
) -> AeviaResult<Vec<Spanned<Item>>> {
    let rules = macros.get(name).ok_or_else(|| {
        AeviaError::message(format!("undefined macro `{name}!`"))
    })?;

    for rule in rules {
        if let Some(bindings) = match_pattern(&rule.pattern, args) {
            let expanded_src = substitute(&rule.replacement, &bindings);
            let expanded_file = crate::parser::items::parse_source(&expanded_src).map_err(|errs| {
                AeviaError::message(format!(
                    "macro `{name}!` expansion failed to parse `{expanded_src}`: {errs}"
                ))
            })?;
            return Ok(expanded_file.items);
        }
    }

    Err(AeviaError::message(format!(
        "no matching rule in macro `{name}!` for args `{args}`"
    )))
}

fn collect_macros(items: &[Spanned<Item>]) -> HashMap<String, Vec<MacroRule>> {
    items
        .iter()
        .filter_map(|s| {
            if let Item::MacroDef { name, rules } = &s.node {
                Some((name.clone(), rules.clone()))
            } else {
                None
            }
        })
        .collect()
}

fn expand_item(item: &mut Item, macros: &HashMap<String, Vec<MacroRule>>) -> AeviaResult<()> {
    match item {
        Item::Function { body, .. } => match body {
            FunctionBody::Expression(expr) => expand_expr(&mut expr.node, macros),
            FunctionBody::Block(stmts) => {
                for stmt in stmts {
                    expand_stmt(&mut stmt.node, macros)?;
                }
                Ok(())
            }
        },
        Item::Const { value, .. } => expand_expr(&mut value.node, macros),
        _ => Ok(()),
    }
}

fn expand_stmt(stmt: &mut Stmt, macros: &HashMap<String, Vec<MacroRule>>) -> AeviaResult<()> {
    match stmt {
        Stmt::Let { init, .. } => expand_expr(&mut init.node, macros),
        Stmt::Assign { value, .. } => expand_expr(&mut value.node, macros),
        Stmt::Expr(expr) => expand_expr(&mut expr.node, macros),
        Stmt::Break | Stmt::Continue => Ok(()),
    }
}

fn expand_expr(expr: &mut Expr, macros: &HashMap<String, Vec<MacroRule>>) -> AeviaResult<()> {
    match expr {
        Expr::MacroCall { name, args } => {
            let expanded = apply_macro(name, args, macros)?;
            *expr = expanded;
            Ok(())
        }
        Expr::BinaryOp { lhs, rhs, .. } => {
            expand_expr(&mut lhs.node, macros)?;
            expand_expr(&mut rhs.node, macros)
        }
        Expr::UnaryOp { expr: inner, .. } => expand_expr(&mut inner.node, macros),
        Expr::Call { args, .. } => {
            for arg in args {
                expand_expr(&mut arg.node, macros)?;
            }
            Ok(())
        }
        Expr::Block(stmts) => {
            for stmt in stmts {
                expand_stmt(&mut stmt.node, macros)?;
            }
            Ok(())
        }
        Expr::If { cond, then_branch, else_branch } => {
            expand_expr(&mut cond.node, macros)?;
            expand_expr(&mut then_branch.node, macros)?;
            if let Some(e) = else_branch {
                expand_expr(&mut e.node, macros)?;
            }
            Ok(())
        }
        Expr::Loop { body } => {
            for stmt in body {
                expand_stmt(&mut stmt.node, macros)?;
            }
            Ok(())
        }
        Expr::While { cond, body } => {
            expand_expr(&mut cond.node, macros)?;
            for stmt in body {
                expand_stmt(&mut stmt.node, macros)?;
            }
            Ok(())
        }
        Expr::For { start, end, body, .. } => {
            expand_expr(&mut start.node, macros)?;
            expand_expr(&mut end.node, macros)?;
            for stmt in body {
                expand_stmt(&mut stmt.node, macros)?;
            }
            Ok(())
        }
        Expr::Match { scrutinee, arms } => {
            expand_expr(&mut scrutinee.node, macros)?;
            for arm in arms {
                expand_expr(&mut arm.body.node, macros)?;
            }
            Ok(())
        }
        Expr::UnsafeTransmute { expr: inner, .. } => expand_expr(&mut inner.node, macros),
        Expr::Print { expr: inner } => expand_expr(&mut inner.node, macros),
        Expr::StructLit { fields, .. } => {
            for (_, fexpr) in fields {
                expand_expr(&mut fexpr.node, macros)?;
            }
            Ok(())
        }
        Expr::FieldAccess { expr: inner, .. } => expand_expr(&mut inner.node, macros),
        Expr::Literal { .. } | Expr::Variable(_) | Expr::Log { .. } => Ok(()),
    }
}

// ── Macro application ──────────────────────────────────────────────────────────

fn apply_macro(
    name: &str,
    args: &str,
    macros: &HashMap<String, Vec<MacroRule>>,
) -> AeviaResult<Expr> {
    let rules = macros.get(name).ok_or_else(|| {
        AeviaError::message(format!("undefined macro `{name}!`"))
    })?;

    for rule in rules {
        if let Some(bindings) = match_pattern(&rule.pattern, args) {
            let expanded_src = substitute(&rule.replacement, &bindings);
            let spanned = parse_expr(&expanded_src).map_err(|errs| {
                AeviaError::message(format!(
                    "macro `{name}!` expansion failed to parse `{expanded_src}`: {}",
                    errs.iter()
                        .map(|e| format!("{e:?}"))
                        .collect::<Vec<_>>()
                        .join("; ")
                ))
            })?;
            return Ok(spanned.node);
        }
    }

    Err(AeviaError::message(format!(
        "no matching rule in macro `{name}!` for args `{args}`"
    )))
}

// ── Pattern matching ───────────────────────────────────────────────────────────

/// Try to match `args` against a macro pattern like `$name:ident, $unit:expr`.
/// Extracts metavariable bindings in order (split on top-level commas).
/// Returns `None` if the arity doesn't match.
fn match_pattern(pattern: &str, args: &str) -> Option<HashMap<String, String>> {
    let metavars = extract_metavars(pattern);
    if metavars.is_empty() {
        return None;
    }
    let parts = split_args(args);
    if parts.len() != metavars.len() {
        return None;
    }
    Some(
        metavars
            .into_iter()
            .zip(parts)
            .map(|(var, val)| (var, val.trim().to_string()))
            .collect(),
    )
}

/// Extract ordered metavariable names from a pattern string.
/// Recognises `$name` and `$name:kind` forms.
fn extract_metavars(pattern: &str) -> Vec<String> {
    let mut vars = Vec::new();
    let bytes = pattern.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'$' {
            i += 1;
            let start = i;
            while i < bytes.len()
                && (bytes[i].is_ascii_alphanumeric() || bytes[i] == b'_')
            {
                i += 1;
            }
            let var_name = pattern[start..i].to_string();
            if !var_name.is_empty() {
                // Skip optional `:kind` suffix.
                if i < bytes.len() && bytes[i] == b':' {
                    i += 1;
                    while i < bytes.len()
                        && (bytes[i].is_ascii_alphanumeric() || bytes[i] == b'_')
                    {
                        i += 1;
                    }
                }
                vars.push(var_name);
            }
        } else {
            i += 1;
        }
    }
    vars
}

/// Split `args` on top-level commas, respecting balanced `(`, `[`, `{`.
fn split_args(args: &str) -> Vec<&str> {
    let mut parts = Vec::new();
    let mut depth = 0usize;
    let mut start = 0;
    for (i, c) in args.char_indices() {
        match c {
            '(' | '[' | '{' => depth += 1,
            ')' | ']' | '}' => {
                if depth > 0 {
                    depth -= 1;
                }
            }
            ',' if depth == 0 => {
                parts.push(&args[start..i]);
                start = i + 1;
            }
            _ => {}
        }
    }
    parts.push(&args[start..]);
    parts
}

/// Substitute `$var` occurrences in the replacement template.
fn substitute(template: &str, bindings: &HashMap<String, String>) -> String {
    let mut result = template.to_string();
    // Sort by length descending so longer var names match before shorter prefixes.
    let mut pairs: Vec<_> = bindings.iter().collect();
    pairs.sort_by(|a, b| b.0.len().cmp(&a.0.len()));
    for (var, val) in pairs {
        result = result.replace(&format!("${var}"), val);
    }
    result
}

// ── Tests ──────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_metavars() {
        let vars = extract_metavars("$x:expr, $y:ident");
        assert_eq!(vars, vec!["x", "y"]);
    }

    #[test]
    fn test_split_args_simple() {
        let parts = split_args("a, b, c");
        assert_eq!(parts, vec!["a", " b", " c"]);
    }

    #[test]
    fn test_split_args_nested() {
        let parts = split_args("foo(a, b), c");
        assert_eq!(parts, vec!["foo(a, b)", " c"]);
    }

    #[test]
    fn test_substitute() {
        let mut bindings = HashMap::new();
        bindings.insert("x".to_string(), "side".to_string());
        let result = substitute("$x * $x", &bindings);
        assert_eq!(result, "side * side");
    }

    #[test]
    fn test_expand_sq_macro() {
        use crate::parser::items::parse_source;

        let src = r#"
macro_rules! sq {
    ($x:expr) => { $x * $x }
}
fn area(side: m) -> m^2 := sq!(side);
"#;
        let mut file = parse_source(src).expect("parse failed");
        expand(&mut file).expect("expansion failed");

        // After expansion the function body must not contain MacroCall.
        if let Item::Function { body: FunctionBody::Expression(expr), .. } = &file.items[1].node {
            assert!(
                !matches!(expr.node, Expr::MacroCall { .. }),
                "MacroCall should have been expanded, got: {:?}",
                expr.node
            );
            assert!(
                matches!(expr.node, Expr::BinaryOp { .. }),
                "expected BinaryOp after sq! expansion, got: {:?}",
                expr.node
            );
        } else {
            panic!("expected function item");
        }
    }
}
