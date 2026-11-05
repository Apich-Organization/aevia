//! Code actions: the four fixes §9.10 names.
//!
//! Every action is a **candidate edit validated by the compiler**: the text
//! the action would produce is compiled, and the action is offered only when
//! that text compiles at least as well as what is there now. That is what
//! stops a "quick fix" from being a guess — an action that would introduce an
//! error is never shown, and the caller does not have to trust the heuristic
//! that produced it.
//!
//! The four:
//!
//! * **add a type annotation** — the binder under the cursor has no written
//!   type, but the checker recorded one;
//! * **convert a Celsius literal to kelvin** — the value is evaluated by the
//!   interpreter (`to(K, …)`) and printed, so the replacement is the exact
//!   text the runtime's own conversion produces;
//! * **extract a `let`** — pull the innermost expression at the cursor out of
//!   a top-level `let` value into its own binding;
//! * **fix a dimension mismatch** — take the unit from the operand that has
//!   one and add it to the dimensionless operand.

use crate::document::Document;
use crate::position::Range;
use crate::types::CodeAction;
use crate::types::CodeActionKind;
use crate::types::TextEdit;
use crate::walk;
use aevia_syntax::Diagnostics;
use aevia_syntax::Severity;
use aevia_syntax::Span;
use aevia_syntax::ast::*;
use aevia_types::check::CheckedModule;

/// Builds the actions that apply to `request`.
#[must_use]
pub(crate) fn actions(
    document: &Document,
    module: &Module,
    checked: &CheckedModule,
    compiler_diagnostics: &Diagnostics,
    request: Range,
) -> Vec<CodeAction> {
    let (Some(start), Some(end)) = (document.offset(request.start), document.offset(request.end))
    else {
        return Vec::new();
    };
    let (start, end) = if start <= end {
        (start, end)
    } else {
        (end, start)
    };
    let old_errors = error_count(compiler_diagnostics);
    let mut actions = Vec::new();
    actions.extend(annotation_actions(
        document, module, checked, start, end, old_errors,
    ));
    actions.extend(celsius_actions(document, module, start, end, old_errors));
    actions.extend(extract_actions(document, module, start, end, old_errors));
    actions.extend(dimension_actions(
        document,
        module,
        compiler_diagnostics,
        start,
        end,
        old_errors,
    ));
    actions
}

// --- add a type annotation -------------------------------------------------

/// A binder that has no written type.
struct Binder<'a> {
    pattern: &'a Pat,
    ty: Option<&'a Ty>,
    /// Whether `: T` can be written after this pattern as it stands.
    annotatable: bool,
}

/// Every binder in the module whose type may be written.
fn binders<'a>(module: &'a Module) -> Vec<Binder<'a>> {
    let mut binders = Vec::new();
    let push_params = |params: &'a [Param], binders: &mut Vec<Binder<'a>>| {
        for param in params {
            binders.push(Binder {
                pattern: &param.pattern,
                ty: param.ty.as_ref(),
                annotatable: true,
            });
        }
    };
    for declaration in &module.decls {
        match &declaration.kind {
            | DeclKind::Fn { args, .. } => push_params(args, &mut binders),
            | DeclKind::Let { pattern, ty, .. } => {
                binders.push(Binder {
                    pattern,
                    ty: ty.as_ref(),
                    annotatable: true,
                })
            },
            | DeclKind::Instance { methods, .. } => {
                for method in methods {
                    push_params(&method.params, &mut binders);
                }
            },
            | _ => {},
        }
    }
    walk::module_exprs(module, |expr| {
        match &expr.kind {
            | ExprKind::Lambda { params, .. } => {
                for param in params {
                    binders.push(Binder {
                        pattern: &param.pattern,
                        ty: param.ty.as_ref(),
                        annotatable: true,
                    });
                }
            },
            | ExprKind::LetIn { pattern, ty, .. } => {
                binders.push(Binder {
                    pattern,
                    ty: ty.as_ref(),
                    annotatable: true,
                })
            },
            | ExprKind::Block { stmts, .. } => {
                for statement in stmts {
                    if let BlockStmt::Let { pattern, ty, .. } = statement {
                        binders.push(Binder {
                            pattern,
                            ty: ty.as_ref(),
                            annotatable: true,
                        });
                    }
                }
            },
            | ExprKind::Do { stmts } => {
                for statement in stmts {
                    if let DoStmt::Let { pattern, ty, .. } = statement {
                        binders.push(Binder {
                            pattern,
                            ty: ty.as_ref(),
                            annotatable: true,
                        });
                    }
                }
            },
            | _ => {},
        }
    });
    binders
}

/// Offers `: T` on a binder the checker knows the type of.
fn annotation_actions(
    document: &Document,
    module: &Module,
    checked: &CheckedModule,
    start: u32,
    end: u32,
    old_errors: usize,
) -> Vec<CodeAction> {
    let text = document.text();
    let mut actions = Vec::new();
    for binder in binders(module) {
        if binder.ty.is_some() || !binder.annotatable {
            continue;
        }
        let PatKind::Var(name) = &binder.pattern.kind else {
            continue;
        };
        if !walk::touches(binder.pattern.span, start, end) {
            continue;
        }
        let Some(ty) = checked.expr_types.get(&binder.pattern.span) else {
            continue;
        };
        if ty.is_error() || aevia_types::class::has_variables(ty) {
            continue;
        }
        let name_span = crate::occurrence::name_span(text, binder.pattern.span, name);
        let edit = (
            Span {
                file: name_span.file,
                start: name_span.end,
                end: name_span.end,
            },
            format!(": {ty}"),
        );
        if !compiles_at_least_as_well(document, &edit, old_errors) {
            continue;
        }
        actions.push(CodeAction {
            title: format!("Add type annotation `{name}: {ty}`"),
            kind: CodeActionKind::QuickFix,
            edits: vec![to_edit(document, edit)],
        });
    }
    actions
}

// --- Celsius to kelvin -----------------------------------------------------

fn celsius_actions(
    document: &Document,
    module: &Module,
    start: u32,
    end: u32,
    old_errors: usize,
) -> Vec<CodeAction> {
    let text = document.text();
    let mut actions = Vec::new();
    for expr in {
        let mut found = Vec::new();
        walk::module_exprs(module, |expr| found.push(expr));
        found
    } {
        if !matches!(expr.kind, ExprKind::Quantity { .. }) || !walk::touches(expr.span, start, end)
        {
            continue;
        }
        let Some(source) = slice(text, expr.span) else {
            continue;
        };
        let trimmed = source.trim_end();
        if !(trimmed.ends_with("°C") || trimmed.ends_with("degC")) {
            continue;
        }
        let Some(kelvin) = convert_to_kelvin(source) else {
            continue;
        };
        let edit = (expr.span, kelvin.clone());
        if !compiles_at_least_as_well(document, &edit, old_errors) {
            continue;
        }
        actions.push(CodeAction {
            title: format!("Convert `{source}` to `{kelvin}`"),
            kind: CodeActionKind::QuickFix,
            edits: vec![to_edit(document, edit)],
        });
    }
    actions
}

/// Evaluates `to(K, value)` and returns the printed value, unit included.
///
/// The interpreter is the conversion the language defines, so the replacement
/// is the value the user would get from `to(K, …)` in their own code. A value
/// the runtime cannot express in kelvin produces no action.
fn convert_to_kelvin(source: &str) -> Option<String> {
    let program = format!("let __converted = to(K, ({source}))\n");
    let compilation = aevia_core::compile("<code-action>.aev", &program);
    if compilation.diagnostics.has_errors() {
        return None;
    }
    let module = compilation.module.as_ref()?;
    let mut interp = aevia_core::Interp::new(&compilation.checked);
    if interp.load(module).has_errors() {
        return None;
    }
    let value = interp.eval_toplevel("__converted").ok()?;
    Some(aevia_core::value::show(&value))
}

// --- extract a let ---------------------------------------------------------

fn extract_actions(
    document: &Document,
    module: &Module,
    start: u32,
    end: u32,
    old_errors: usize,
) -> Vec<CodeAction> {
    let text = document.text();
    let mut actions = Vec::new();
    for declaration in &module.decls {
        let DeclKind::Let { value, rec, .. } = &declaration.kind else {
            continue;
        };
        if *rec || !walk::touches(value.span, start, end) {
            continue;
        }
        let Some(extracted) = innermost_inside(value, start, end) else {
            continue;
        };
        if extracted.span == value.span {
            // Extracting the whole right-hand side only renames it; the
            // refactor a user wants is the subexpression under the cursor.
            continue;
        }
        let Some(extracted_source) = slice(text, extracted.span) else {
            continue;
        };
        let binding = fresh_name(text, module, "__extracted");
        let Some(declaration_source) = slice(text, declaration.span) else {
            continue;
        };
        let offset = usize::try_from(extracted.span.start - declaration.span.start).ok();
        let end_offset = usize::try_from(extracted.span.end - declaration.span.start).ok();
        let (Some(offset), Some(end_offset)) = (offset, end_offset) else {
            continue;
        };
        if offset > declaration_source.len() || end_offset > declaration_source.len() {
            continue;
        }
        let mut replaced = String::with_capacity(declaration_source.len() + 32);
        replaced.push_str(&declaration_source[..offset]);
        replaced.push_str(&binding);
        replaced.push_str(&declaration_source[end_offset..]);
        let replacement = format!("let {binding} = {extracted_source}\n{replaced}");
        let edit = (declaration.span, replacement.clone());
        if !compiles_at_least_as_well(document, &edit, old_errors) {
            continue;
        }
        actions.push(CodeAction {
            title: format!("Extract `{extracted_source}` into `{binding}`"),
            kind: CodeActionKind::RefactorExtract,
            edits: vec![to_edit(document, edit)],
        });
    }
    actions
}

/// The innermost expression inside `outer` that the request touches.
fn innermost_inside<'a>(
    outer: &'a Expr,
    start: u32,
    end: u32,
) -> Option<&'a Expr> {
    let mut best: Option<&Expr> = None;
    walk::expression(outer, &mut |expr| {
        if !walk::touches(expr.span, start, end) {
            return;
        }
        let shorter = best.is_none_or(|current: &Expr| {
            expr.span.end.saturating_sub(expr.span.start)
                < current.span.end.saturating_sub(current.span.start)
        });
        if shorter {
            best = Some(expr);
        }
    });
    best
}

/// A name derived from `base` that nothing in the module already uses.
fn fresh_name(
    text: &str,
    module: &Module,
    base: &str,
) -> String {
    let mut used = std::collections::HashSet::new();
    walk::module_exprs(module, |expr| {
        if let ExprKind::Var(name) = &expr.kind {
            used.insert(name.clone());
        }
    });
    // Declarations are not `Var` occurrences, so scan the source words as
    // well; a false collision only costs a suffix.
    for word in text.split(|character: char| !character.is_alphanumeric() && character != '_') {
        if !word.is_empty() {
            used.insert(word.to_string());
        }
    }
    if !used.contains(base) {
        return base.to_string();
    }
    let mut index = 2;
    loop {
        let candidate = format!("{base}{index}");
        if !used.contains(&candidate) {
            return candidate;
        }
        index += 1;
    }
}

// --- fix a dimension mismatch ---------------------------------------------

fn dimension_actions(
    document: &Document,
    module: &Module,
    compiler_diagnostics: &Diagnostics,
    start: u32,
    end: u32,
    old_errors: usize,
) -> Vec<CodeAction> {
    let text = document.text();
    let mut actions = Vec::new();
    for diagnostic in &compiler_diagnostics.items {
        if diagnostic.code != "E0401"
            || diagnostic.severity != Severity::Error
            || !walk::touches(diagnostic.span, start, end)
        {
            continue;
        }
        let Some(Expr {
            kind: ExprKind::Binary { left, right, .. },
            ..
        }) = walk::exactly(module, diagnostic.span)
        else {
            continue;
        };
        let operands = [
            (left.as_ref(), right.as_ref()),
            (right.as_ref(), left.as_ref()),
        ];
        for (with_unit, without_unit) in operands {
            let (Some(with_unit_text), Some(without_unit_text)) =
                (slice(text, with_unit.span), slice(text, without_unit.span))
            else {
                continue;
            };
            let Some(unit) = unit_suffix(with_unit_text) else {
                continue;
            };
            if !is_number_literal(without_unit_text) {
                continue;
            }
            let edit = (
                Span {
                    file: without_unit.span.file,
                    start: without_unit.span.end,
                    end: without_unit.span.end,
                },
                format!(" {unit}"),
            );
            let candidate = candidate_text(document, &[edit.clone()]);
            let new_errors = compile_errors(document, &candidate);
            // A dimension fix must *remove* the error it fixes; an action that
            // merely moves it elsewhere is not offered.
            if new_errors >= old_errors {
                continue;
            }
            actions.push(CodeAction {
                title: format!("Add `{unit}` to fix the dimension mismatch"),
                kind: CodeActionKind::QuickFix,
                edits: vec![to_edit(document, edit)],
            });
        }
    }
    actions
}

/// The unit an expression such as `1 m/s^2` carries, when the leading token is
/// a bare number.
fn unit_suffix(text: &str) -> Option<&str> {
    let text = text.trim();
    let bytes = text.as_bytes();
    let mut index = 0;
    while index < bytes.len()
        && (bytes[index].is_ascii_digit() || bytes[index] == b'.' || bytes[index] == b'_')
    {
        index += 1;
    }
    if index == 0 {
        return None;
    }
    // An exponent belongs to the number: `1e-3 m`.
    if index < bytes.len() && (bytes[index] == b'e' || bytes[index] == b'E') {
        index += 1;
        if index < bytes.len() && (bytes[index] == b'+' || bytes[index] == b'-') {
            index += 1;
        }
        while index < bytes.len() && bytes[index].is_ascii_digit() {
            index += 1;
        }
    }
    let rest = text.get(index..)?.trim();
    if rest.is_empty()
        || rest.starts_with(&['*', '+', '-', '/', '^', ')', ','][..])
        || rest.contains(char::is_whitespace)
    {
        return None;
    }
    Some(rest)
}

/// True when `text` is a numeric literal and nothing else.
fn is_number_literal(text: &str) -> bool {
    let text = text.trim();
    text.starts_with(|character: char| character.is_ascii_digit())
        && unit_suffix(text).is_none()
        && text.chars().all(|character| {
            character.is_ascii_digit() || matches!(character, '.' | '_' | 'e' | 'E' | '+' | '-')
        })
}

// --- validation ------------------------------------------------------------

/// The number of errors a set of diagnostics contains.
fn error_count(diagnostics: &Diagnostics) -> usize {
    diagnostics
        .items
        .iter()
        .filter(|diagnostic| diagnostic.severity == Severity::Error)
        .count()
}

/// Compiles the document with `edit` applied and counts its errors.
fn compile_errors(
    document: &Document,
    text: &str,
) -> usize {
    let compilation = aevia_core::compile(document.name(), text);
    error_count(&compilation.diagnostics)
}

/// True when applying `edit` does not make the program worse.
fn compiles_at_least_as_well(
    document: &Document,
    edit: &(Span, String),
    old_errors: usize,
) -> bool {
    let text = candidate_text(document, std::slice::from_ref(edit));
    compile_errors(document, &text) <= old_errors
}

/// The document's text with the edits applied.
fn candidate_text(
    document: &Document,
    edits: &[(Span, String)],
) -> String {
    let original = document.text();
    let mut ordered: Vec<&(Span, String)> = edits.iter().collect();
    ordered.sort_by_key(|(span, _)| span.start);
    let mut out = String::with_capacity(original.len() + 64);
    let mut cursor = 0usize;
    for (span, text) in ordered {
        let start = usize::try_from(span.start)
            .unwrap_or(usize::MAX)
            .min(original.len());
        let end = usize::try_from(span.end)
            .unwrap_or(usize::MAX)
            .min(original.len());
        if start < cursor || end < start {
            continue;
        }
        out.push_str(&original[cursor..start]);
        out.push_str(text);
        cursor = end;
    }
    out.push_str(&original[cursor..]);
    out
}

fn to_edit(
    document: &Document,
    (span, text): (Span, String),
) -> TextEdit {
    TextEdit {
        range: document.range_of(span.start, span.end),
        new_text: text,
    }
}

fn slice(
    text: &str,
    span: Span,
) -> Option<&str> {
    let start = usize::try_from(span.start).ok()?;
    let end = usize::try_from(span.end).ok()?;
    text.get(start..end)
}
