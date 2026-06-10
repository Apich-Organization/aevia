//! Static analysis: parse, dimensional check, and advisory lints. Phase 5.

use crate::ast::{Expr, Item, SourceFile, Stmt, Visibility};
use crate::diagnostics::AeviaResult;
use crate::modules::{self, ImportBindings};
use crate::types::checker;
use std::collections::HashSet;
use std::path::{Path, PathBuf};

/// Severity of a lint finding.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LintLevel {
    Error,
    Warning,
}

/// A single lint diagnostic.
#[derive(Debug, Clone)]
pub struct Lint {
    pub level: LintLevel,
    pub message: String,
    pub file: PathBuf,
}

/// Aggregated lint results for one or more files.
#[derive(Debug, Default)]
pub struct LintReport {
    pub lints: Vec<Lint>,
}

impl LintReport {
    #[must_use]
    pub fn ok(&self) -> bool {
        !self.lints.iter().any(|l| l.level == LintLevel::Error)
    }

    pub fn push(&mut self, level: LintLevel, file: &Path, message: impl Into<String>) {
        self.lints.push(Lint {
            level,
            message: message.into(),
            file: file.to_path_buf(),
        });
    }
}

/// Lint a single entry `.ae` file (including its module graph).
pub fn lint_file(entry: &Path) -> AeviaResult<LintReport> {
    let program = modules::load_program(entry)?;
    let mut report = LintReport::default();

    for module in modules::modules(&program) {
        lint_source(&module.path, &module.ast, &module.imports, &mut report);
    }

    lint_entry_conventions(&program.entry, modules::entry_ast(&program), &mut report);
    Ok(report)
}

fn lint_source(path: &Path, file: &SourceFile, imports: &ImportBindings, report: &mut LintReport) {
    for err in checker::check_with_imports(file, imports).errors {
        report.push(
            LintLevel::Error,
            path,
            format!("{err}"),
        );
    }

    let used_names = collect_used_names(file);
    for name in imports.functions.keys() {
        if !used_names.contains(name) {
            report.push(
                LintLevel::Warning,
                path,
                format!("unused import `{name}`"),
            );
        }
    }

    for item in &file.items {
        if let Item::Function {
            name,
            visibility,
            return_type,
            ..
        } = &item.node
        {
            if *visibility == Visibility::Public && return_type.is_none() && name != "main" {
                report.push(
                    LintLevel::Warning,
                    path,
                    format!("public function `{name}` has no return type annotation"),
                );
            }
        }
        lint_item_unsafe(path, &item.node, report);
    }
}

fn lint_entry_conventions(entry: &Path, file: &SourceFile, report: &mut LintReport) {
    let has_main = file.items.iter().any(|i| {
        matches!(
            &i.node,
            Item::Function { name, .. } if name == "main"
        )
    });
    if !has_main {
        report.push(
            LintLevel::Warning,
            entry,
            "entry file has no `main` function",
        );
    }
}

fn lint_item_unsafe(path: &Path, item: &Item, report: &mut LintReport) {
    match item {
        Item::Module { body: Some(items), .. } => {
            for child in items {
                lint_item_unsafe(path, &child.node, report);
            }
        }
        Item::Function { body, .. } => {
            walk_expr_unsafe(path, body, report);
        }
        Item::Const { value, .. } => walk_expr(path, &value.node, report),
        _ => {}
    }
}

fn walk_expr_unsafe(path: &Path, body: &crate::ast::FunctionBody, report: &mut LintReport) {
    match body {
        crate::ast::FunctionBody::Expression(expr) => walk_expr(path, &expr.node, report),
        crate::ast::FunctionBody::Block(stmts) => {
            for stmt in stmts {
                walk_stmt_unsafe(path, &stmt.node, report);
            }
        }
    }
}

fn walk_stmt_unsafe(path: &Path, stmt: &Stmt, report: &mut LintReport) {
    match stmt {
        Stmt::Let { init, .. } => walk_expr(path, &init.node, report),
        Stmt::Assign { value, .. } => walk_expr(path, &value.node, report),
        Stmt::Expr(expr) => walk_expr(path, &expr.node, report),
        Stmt::Break | Stmt::Continue => {}
    }
}

fn walk_expr(path: &Path, expr: &Expr, report: &mut LintReport) {
    if matches!(expr, Expr::UnsafeTransmute { .. }) {
        report.push(
            LintLevel::Warning,
            path,
            "unsafe `transmute` bypasses dimensional safety guarantees",
        );
    }
    match expr {
        Expr::BinaryOp { lhs, rhs, .. } => {
            walk_expr(path, &lhs.node, report);
            walk_expr(path, &rhs.node, report);
        }
        Expr::UnaryOp { expr, .. } => walk_expr(path, &expr.node, report),
        Expr::Call { args, .. } => {
            for arg in args {
                walk_expr(path, &arg.node, report);
            }
        }
        Expr::Block(stmts) => {
            for stmt in stmts {
                walk_stmt_unsafe(path, &stmt.node, report);
            }
        }
        Expr::If { cond, then_branch, else_branch } => {
            walk_expr(path, &cond.node, report);
            walk_expr(path, &then_branch.node, report);
            if let Some(e) = else_branch {
                walk_expr(path, &e.node, report);
            }
        }
        Expr::Loop { body } => {
            for stmt in body {
                walk_stmt_unsafe(path, &stmt.node, report);
            }
        }
        Expr::While { cond, body } => {
            walk_expr(path, &cond.node, report);
            for stmt in body {
                walk_stmt_unsafe(path, &stmt.node, report);
            }
        }
        Expr::For { start, end, body, .. } => {
            walk_expr(path, &start.node, report);
            walk_expr(path, &end.node, report);
            for stmt in body {
                walk_stmt_unsafe(path, &stmt.node, report);
            }
        }
        Expr::Match { scrutinee, arms } => {
            walk_expr(path, &scrutinee.node, report);
            for arm in arms {
                walk_expr(path, &arm.body.node, report);
            }
        }
        Expr::UnsafeTransmute { expr, .. } => walk_expr(path, &expr.node, report),
        Expr::Print { expr } => walk_expr(path, &expr.node, report),
        Expr::StructLit { fields, .. } => {
            for (_, fexpr) in fields {
                walk_expr(path, &fexpr.node, report);
            }
        }
        Expr::FieldAccess { expr: inner, .. } => walk_expr(path, &inner.node, report),
        Expr::Literal { .. } | Expr::Variable(_) | Expr::MacroCall { .. } | Expr::Log { .. } => {}
    }
}

fn collect_used_names(file: &SourceFile) -> HashSet<String> {
    let mut names = HashSet::new();
    for item in &file.items {
        collect_item_uses(&item.node, &mut names);
    }
    names
}

fn collect_item_uses(item: &Item, names: &mut HashSet<String>) {
    match item {
        Item::Module { body: Some(items), .. } => {
            for child in items {
                collect_item_uses(&child.node, names);
            }
        }
        Item::Function { body, .. } => collect_body_uses(body, names),
        Item::Const { value, .. } => collect_expr_uses(&value.node, names),
        _ => {}
    }
}

fn collect_body_uses(body: &crate::ast::FunctionBody, names: &mut HashSet<String>) {
    match body {
        crate::ast::FunctionBody::Expression(expr) => collect_expr_uses(&expr.node, names),
        crate::ast::FunctionBody::Block(stmts) => {
            for stmt in stmts {
                collect_stmt_uses(&stmt.node, names);
            }
        }
    }
}

fn collect_stmt_uses(stmt: &Stmt, names: &mut HashSet<String>) {
    match stmt {
        Stmt::Let { init, .. } => collect_expr_uses(&init.node, names),
        Stmt::Assign { value, .. } => collect_expr_uses(&value.node, names),
        Stmt::Expr(expr) => collect_expr_uses(&expr.node, names),
        Stmt::Break | Stmt::Continue => {}
    }
}

fn collect_expr_uses(expr: &Expr, names: &mut HashSet<String>) {
    match expr {
        Expr::Variable(name) => {
            names.insert(name.clone());
        }
        Expr::Call { func, args } => {
            names.insert(func.clone());
            for arg in args {
                collect_expr_uses(&arg.node, names);
            }
        }
        Expr::BinaryOp { lhs, rhs, .. } => {
            collect_expr_uses(&lhs.node, names);
            collect_expr_uses(&rhs.node, names);
        }
        Expr::UnaryOp { expr, .. } => collect_expr_uses(&expr.node, names),
        Expr::Block(stmts) => {
            for stmt in stmts {
                collect_stmt_uses(&stmt.node, names);
            }
        }
        Expr::If { cond, then_branch, else_branch } => {
            collect_expr_uses(&cond.node, names);
            collect_expr_uses(&then_branch.node, names);
            if let Some(e) = else_branch {
                collect_expr_uses(&e.node, names);
            }
        }
        Expr::Loop { body } => {
            for stmt in body {
                collect_stmt_uses(&stmt.node, names);
            }
        }
        Expr::While { cond, body } => {
            collect_expr_uses(&cond.node, names);
            for stmt in body {
                collect_stmt_uses(&stmt.node, names);
            }
        }
        Expr::For { start, end, body, .. } => {
            collect_expr_uses(&start.node, names);
            collect_expr_uses(&end.node, names);
            for stmt in body {
                collect_stmt_uses(&stmt.node, names);
            }
        }
        Expr::Match { scrutinee, arms } => {
            collect_expr_uses(&scrutinee.node, names);
            for arm in arms {
                collect_expr_uses(&arm.body.node, names);
            }
        }
        Expr::UnsafeTransmute { expr, .. } => collect_expr_uses(&expr.node, names),
        Expr::MacroCall { name, .. } => { names.insert(name.clone()); }
        Expr::Print { expr } => collect_expr_uses(&expr.node, names),
        Expr::StructLit { fields, .. } => {
            for (_, fexpr) in fields {
                collect_expr_uses(&fexpr.node, names);
            }
        }
        Expr::FieldAccess { expr: inner, field } => {
            collect_expr_uses(&inner.node, names);
            names.insert(field.clone());
        }
        Expr::Literal { .. } | Expr::Log { .. } => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lint_catches_dimension_error() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("bad.ae");
        std::fs::write(&file, "fn bad(x: m) -> s := x;").unwrap();
        let report = lint_file(&file).unwrap();
        assert!(!report.ok());
    }
}
