//! `W0102` (`unused-import`): imported module or selective names never referenced in the module.

use std::collections::HashSet;

use aevia_dim::UnitExpr;
use aevia_syntax::SourceMap;
use aevia_syntax::Span;
use aevia_syntax::ast::Decl;
use aevia_syntax::ast::DeclKind;
use aevia_syntax::ast::Expr;
use aevia_syntax::ast::ExprKind;
use aevia_syntax::ast::Module;
use aevia_syntax::ast::Pat;
use aevia_syntax::ast::PatKind;
use aevia_syntax::ast::Ty;
use aevia_syntax::ast::TyKind;
use aevia_syntax::visit;

use crate::lint::Lint;
use crate::lint::W0102_UNUSED_IMPORT;
use crate::pass::Applicability;
use crate::pass::Fix;
use crate::pass::LintCx;
use crate::pass::LintPassInfo;
use crate::pass::SyntaxPass;
use crate::pass::TextEdit;

/// Detects unused `import` declarations (`import Foo`, `import Foo as Bar`, `import Foo { a, b }`).
pub struct UnusedImportPass;

impl LintPassInfo for UnusedImportPass {
    fn lints(&self) -> &'static [&'static Lint] {
        static LINTS: &[&Lint] = &[&W0102_UNUSED_IMPORT];
        LINTS
    }
}

impl SyntaxPass for UnusedImportPass {
    fn check_module(
        &self,
        cx: &mut LintCx<'_>,
        module: &Module,
    ) {
        let mut imports = module
            .decls
            .iter()
            .filter_map(|decl| {
                match &decl.kind {
                    | DeclKind::Import {
                        path,
                        alias,
                        names,
                        open: false,
                        export: false,
                    } => Some((decl.span, path, alias, names)),
                    | _ => None,
                }
            })
            .peekable();
        if imports.peek().is_none() {
            return;
        }
        let mut refs = RefCollector::default();
        visit::walk_module(&mut refs, module);

        for (span, path, alias, names) in imports {
            if let Some(selective) = names {
                let unused: Vec<&String> = selective
                    .iter()
                    .filter(|name| !refs.used.contains(name.as_str()))
                    .collect();
                if unused.len() == selective.len() {
                    let fix = delete_decl_fix(cx.sources, span, "remove unused import");
                    cx.emit(
                        &W0102_UNUSED_IMPORT,
                        span,
                        format!("unused import `{}`", path.join(".")),
                    )
                    .fix(fix);
                } else {
                    for name in unused {
                        cx.emit(
                            &W0102_UNUSED_IMPORT,
                            span,
                            format!("unused import `{name}` from `{}`", path.join(".")),
                        )
                        .help(format!("remove `{name}` from the import list"));
                    }
                }
            } else if let Some(binding) = alias.as_ref().or_else(|| path.last())
                && !refs.used.contains(binding.as_str())
            {
                let fix = delete_decl_fix(cx.sources, span, "remove unused import");
                cx.emit(
                    &W0102_UNUSED_IMPORT,
                    span,
                    format!("unused import `{}`", path.join(".")),
                )
                .fix(fix);
            }
        }
    }
}

fn delete_decl_fix(
    sources: &SourceMap,
    span: Span,
    title: &str,
) -> Fix {
    // Expand end to consume a trailing newline if present.
    let mut end = span.end;
    if let Some(file) = sources.file(span.file) {
        let bytes = file.text.as_bytes();
        let idx = usize::try_from(end).unwrap_or(bytes.len());
        match bytes.get(idx..) {
            | Some([b'\r', b'\n', ..]) => end = end.checked_add(2).unwrap_or(end),
            | Some([b'\n' | b'\r', ..]) => end = end.checked_add(1).unwrap_or(end),
            | _ => {},
        }
    }
    Fix {
        title: title.to_string(),
        applicability: Applicability::MachineApplicable,
        edits: vec![TextEdit {
            span: Span {
                file: span.file,
                start: span.start,
                end,
            },
            replacement: String::new(),
        }],
    }
}

#[derive(Default)]
struct RefCollector<'a> {
    used: HashSet<&'a str>,
}

impl<'a> RefCollector<'a> {
    fn record_unit_expr(
        &mut self,
        unit: &'a UnitExpr,
    ) {
        match unit {
            | UnitExpr::Name(name) => {
                self.used.extend(name.split('.'));
            },
            | UnitExpr::Percent => {},
            | UnitExpr::Mul(a, b) | UnitExpr::Div(a, b) => {
                self.record_unit_expr(a);
                self.record_unit_expr(b);
            },
            | UnitExpr::Pow(inner, _) | UnitExpr::Paren(inner) => self.record_unit_expr(inner),
        }
    }
}

impl<'ast> visit::Visitor<'ast> for RefCollector<'ast> {
    fn visit_decl(
        &mut self,
        decl: &'ast Decl,
    ) {
        match &decl.kind {
            | DeclKind::Import { .. } => return,
            | DeclKind::Instance { class, .. } => {
                self.used.extend(class.split('.'));
            },
            | DeclKind::Derive { classes, .. } => {
                for class in classes {
                    self.used.extend(class.split('.'));
                }
            },
            | DeclKind::Unit { base, offset, .. } => {
                if let Some(u) = base {
                    self.record_unit_expr(u);
                }
                if let Some((_, Some(u))) = offset {
                    self.record_unit_expr(u);
                }
            },
            | _ => {},
        }
        visit::walk_decl(self, decl);
    }

    fn visit_expr(
        &mut self,
        expr: &'ast Expr,
    ) {
        match &expr.kind {
            | ExprKind::Var(name) => {
                self.used.insert(name.as_str());
            },
            | ExprKind::QName(path) => {
                self.used.extend(path.iter().map(String::as_str));
            },
            | ExprKind::Quantity { unit, .. } => {
                self.record_unit_expr(unit);
            },
            | _ => {},
        }
        visit::walk_expr(self, expr);
    }

    fn visit_pat(
        &mut self,
        pat: &'ast Pat,
    ) {
        if let PatKind::Ctor { name, .. } = &pat.kind {
            self.used.extend(name.split('.'));
        }
        visit::walk_pat(self, pat);
    }

    fn visit_ty(
        &mut self,
        ty: &'ast Ty,
    ) {
        if let TyKind::Named { path, .. } = &ty.kind {
            self.used.extend(path.iter().map(String::as_str));
        }
        visit::walk_ty(self, ty);
    }
}
