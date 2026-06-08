//! Documentation generation from the AST. Phase 5.

use crate::ast::{DimExpr, Item, Param, SourceFile, Visibility};
use crate::diagnostics::AeviaResult;
use std::fmt::Write as _;
use std::path::Path;

/// Render a `SourceFile` as Markdown API reference.
pub fn render_markdown(path: &Path, file: &SourceFile) -> String {
    let mut out = String::new();
    let title = path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("module");
    let _ = writeln!(out, "# `{title}`\n");
    if let Some(doc) = &file.doc {
        let _ = writeln!(out, "{doc}\n");
    }
    for item in &file.items {
        render_item(&mut out, &item.node);
    }
    out
}

/// Generate docs for an entry file and write `index.md` under `output_dir`.
pub fn generate_for_entry(entry: &Path, output_dir: &Path) -> AeviaResult<()> {
    let program = crate::modules::load_program(entry)?;
    std::fs::create_dir_all(output_dir).map_err(|e| crate::diagnostics::AeviaError::io(output_dir, e))?;

    for module in crate::modules::modules(&program) {
        let md = render_markdown(&module.path, &module.ast);
        let name = module
            .path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("module");
        let out_path = output_dir.join(format!("{name}.md"));
        std::fs::write(&out_path, md).map_err(|e| crate::diagnostics::AeviaError::io(&out_path, e))?;
    }

    let index = output_dir.join("index.md");
    let entry_md = render_markdown(entry, crate::modules::entry_ast(&program));
    std::fs::write(&index, entry_md).map_err(|e| crate::diagnostics::AeviaError::io(&index, e))?;
    Ok(())
}

fn render_item(out: &mut String, item: &Item) {
    match item {
        Item::TypeAlias { name, dimension_expr, .. } => {
            let _ = writeln!(
                out,
                "## type `{name}`\n\n```ae\ntype {name} = {};\n```\n",
                format_dim(&dimension_expr.node)
            );
        }
        Item::Struct { name, fields, visibility, doc, .. } => {
            if !is_public(*visibility) {
                return;
            }
            let _ = writeln!(out, "## struct `{name}`\n");
            if let Some(d) = doc {
                let _ = writeln!(out, "{d}\n");
            }
            let _ = writeln!(out, "| Field | Dimension |");
            let _ = writeln!(out, "|-------|-----------|");
            for field in fields {
                let _ = writeln!(
                    out,
                    "| `{}` | `{}` |",
                    field.name,
                    format_dim(&field.dim.node)
                );
            }
            let _ = writeln!(out);
        }
        Item::Function {
            name,
            params,
            return_type,
            visibility,
            attributes,
            doc,
            ..
        } => {
            if !is_public(*visibility) && *name != "main" {
                return;
            }
            let _ = writeln!(out, "## fn `{name}`\n");
            if let Some(d) = doc {
                let _ = writeln!(out, "{d}\n");
            }
            if !attributes.is_empty() {
                let attrs: Vec<_> = attributes
                    .iter()
                    .map(|a| format!("#[{}]", a.name))
                    .collect();
                let _ = writeln!(out, "{}\n", attrs.join(" "));
            }
            let ret = return_type
                .as_ref()
                .map(|r| format_dim(&r.node))
                .unwrap_or_else(|| "dimensionless".to_string());
            let _ = writeln!(out, "**Returns:** `{ret}`\n");
            let _ = writeln!(out, "| Parameter | Dimension |");
            let _ = writeln!(out, "|-----------|-----------|");
            for Param { name, dim } in params {
                let _ = writeln!(out, "| `{name}` | `{}` |", format_dim(&dim.node));
            }
            let _ = writeln!(out);
        }
        Item::CustomOp {
            name,
            param_name,
            input_dim,
            output_dim,
            visibility,
            properties,
            doc,
            ..
        } => {
            if !is_public(*visibility) {
                return;
            }
            let _ = writeln!(
                out,
                "## op `{name}`\n\n```ae\nop {name}({param_name}: {} -> {})\n```\n",
                format_dim(&input_dim.node),
                format_dim(&output_dim.node)
            );
            if let Some(d) = doc {
                let _ = writeln!(out, "{d}\n");
            }
            let _ = writeln!(
                out,
                "- vectorizable: {}\n- cost: {}\n",
                properties.vectorizable,
                properties
                    .cost
                    .map(|c| c.to_string())
                    .unwrap_or_else(|| "default".to_string())
            );
        }
        Item::Module { name, body, visibility, doc, .. } => {
            if !is_public(*visibility) {
                return;
            }
            let _ = writeln!(out, "## mod `{name}`\n");
            if let Some(d) = doc {
                let _ = writeln!(out, "{d}\n");
            }
            if let Some(items) = body {
                for child in items {
                    render_item(out, &child.node);
                }
            }
        }
        Item::Use { .. } => {}
        Item::MacroDef { .. } => {} // macro definitions produce no doc output
    }
}

fn is_public(v: Visibility) -> bool {
    matches!(v, Visibility::Public | Visibility::Crate)
}

pub fn format_dim(expr: &DimExpr) -> String {
    match expr {
        DimExpr::Base(name) => name.clone(),
        DimExpr::Mul(lhs, rhs) => format!(
            "{} * {}",
            format_dim(&lhs.node),
            format_dim(&rhs.node)
        ),
        DimExpr::Div(lhs, rhs) => format!(
            "{} / {}",
            format_dim(&lhs.node),
            format_dim(&rhs.node)
        ),
        DimExpr::Power(base, exp) => format!("{}^{}", format_dim(&base.node), exp),
        DimExpr::Tensor { base, shape } => {
            let shape_strs: Vec<String> = shape.iter().map(|s| s.to_string()).collect();
            format!("{}[{}]", format_dim(&base.node), shape_strs.join(", "))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parser::items::parse_source;

    #[test]
    fn renders_public_fn() {
        let file = parse_source("pub fn f(x: m) -> m := x;").unwrap();
        let md = render_markdown(Path::new("f.ae"), &file);
        assert!(md.contains("## fn `f`"));
        assert!(md.contains("`x`"));
    }
}
