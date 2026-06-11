//! Documentation generation from the AST. Phase 5.

pub mod html;

use crate::ast::{DimExpr, Item, Param, SourceFile, Visibility};
use crate::diagnostics::AeviaResult;
use crate::fmt::format_expr;
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

/// Generate HTML documentation for an entry file and all its modules.
///
/// Writes `{module}.html` for each module, plus `index.html` and `style.css`.
pub fn generate_html_for_entry(entry: &std::path::Path, output_dir: &std::path::Path) -> AeviaResult<()> {
    let program = crate::modules::load_program(entry)?;
    std::fs::create_dir_all(output_dir).map_err(|e| crate::diagnostics::AeviaError::io(output_dir, e))?;

    // Collect module info for cross-linking in the sidebar.
    let module_list: Vec<_> = crate::modules::modules(&program).collect();
    let modules: Vec<html::ModuleInfo> = module_list
        .iter()
        .map(|m| {
            let name = m
                .path
                .file_stem()
                .and_then(|s: &std::ffi::OsStr| s.to_str())
                .unwrap_or("module")
                .to_string();
            html::ModuleInfo {
                html_filename: format!("{name}.html"),
                name,
            }
        })
        .collect();

    // Also add the entry module itself if not already present.
    let entry_name = entry
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("main")
        .to_string();
    let mut all_modules = modules.clone();
    if !all_modules.iter().any(|m| m.name == entry_name) {
        all_modules.push(html::ModuleInfo {
            html_filename: format!("{entry_name}.html"),
            name: entry_name.clone(),
        });
    }

    // Write stylesheet.
    let css_path = output_dir.join("style.css");
    std::fs::write(&css_path, html::STYLESHEET)
        .map_err(|e| crate::diagnostics::AeviaError::io(&css_path, e))?;

    // Write module pages.
    for module in &module_list {
        let page = html::render_html_page(&module.path, &module.ast, &all_modules);
        let name = module
            .path
            .file_stem()
            .and_then(|s: &std::ffi::OsStr| s.to_str())
            .unwrap_or("module");
        let out_path = output_dir.join(format!("{name}.html"));
        std::fs::write(&out_path, page).map_err(|e| crate::diagnostics::AeviaError::io(&out_path, e))?;
    }

    // Write entry page.
    let entry_page = html::render_html_page(entry, crate::modules::entry_ast(&program), &all_modules);
    let entry_path = output_dir.join(format!("{entry_name}.html"));
    std::fs::write(&entry_path, entry_page)
        .map_err(|e| crate::diagnostics::AeviaError::io(&entry_path, e))?;

    // Write index page.
    let index_page = html::render_html_index(&all_modules);
    let index_path = output_dir.join("index.html");
    std::fs::write(&index_path, index_page)
        .map_err(|e| crate::diagnostics::AeviaError::io(&index_path, e))?;

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
            for Param { name, dim, .. } in params {
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
        Item::Use { .. } | Item::MacroDef { .. } | Item::MacroCall { .. } => {}
        Item::Const { name, dim, value, visibility, doc } => {
            if !is_public(*visibility) {
                return;
            }
            let _ = writeln!(out, "## const `{name}`\n");
            if let Some(d) = doc {
                let _ = writeln!(out, "{d}\n");
            }
            let dim_str = dim
                .as_ref()
                .map(|d| format_dim(&d.node))
                .unwrap_or_else(|| "inferred".to_string());
            let _ = writeln!(
                out,
                "```ae\nconst {name}: {dim_str} = {};\n```\n",
                format_expr(&value.node)
            );
        }
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

    #[test]
    fn renders_html_page() {
        let src = r#"
pub struct Particle {
    pub mass: kg,
    pub velocity: m / s,
}

pub fn kinetic_energy(m: kg, v: m / s) -> kg * m^2 / s^2 := 0.5 * m * v^2;

pub const G: m / s^2 = 9.8;
"#;
        let file = parse_source(src).unwrap();
        let modules = vec![html::ModuleInfo {
            name: "physics".to_string(),
            html_filename: "physics.html".to_string(),
        }];
        let page = html::render_html_page(Path::new("physics.ae"), &file, &modules);

        // Check HTML structure
        assert!(page.contains("<!DOCTYPE html>"));
        assert!(page.contains("<title>physics - Aevia Docs</title>"));
        assert!(page.contains("style.css"));

        // Check sidebar
        assert!(page.contains("<nav>"));
        assert!(page.contains("physics.html"));

        // Check struct rendering
        assert!(page.contains("struct"));
        assert!(page.contains("Particle"));
        assert!(page.contains("mass"));
        assert!(page.contains("kg"));

        // Check function rendering
        assert!(page.contains("kinetic_energy"));
        assert!(page.contains("fn"));

        // Check const rendering
        assert!(page.contains("const"));
        assert!(page.contains("G"));
    }

    #[test]
    fn renders_html_index() {
        let modules = vec![
            html::ModuleInfo {
                name: "main".to_string(),
                html_filename: "main.html".to_string(),
            },
            html::ModuleInfo {
                name: "physics".to_string(),
                html_filename: "physics.html".to_string(),
            },
        ];
        let index = html::render_html_index(&modules);
        assert!(index.contains("<!DOCTYPE html>"));
        assert!(index.contains("Aevia Documentation"));
        assert!(index.contains("main.html"));
        assert!(index.contains("physics.html"));
    }

    #[test]
    fn stylesheet_is_non_empty() {
        assert!(!html::STYLESHEET.is_empty());
        assert!(html::STYLESHEET.contains("nav"));
        assert!(html::STYLESHEET.contains("main"));
        assert!(html::STYLESHEET.contains("@media print"));
    }
}
