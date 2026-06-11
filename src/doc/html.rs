//! HTML documentation generation for Aevia.
//!
//! Produces styled, multi-page HTML documentation with a navigation sidebar.

use crate::ast::{Item, Param, SourceFile, Visibility};
use crate::fmt::format_expr;
use super::format_dim;
use std::fmt::Write as _;
use std::path::Path;

/// Embedded CSS stylesheet for generated HTML docs.
pub const STYLESHEET: &str = r#"
:root {
    --sidebar-width: 240px;
    --sidebar-bg: #1e293b;
    --sidebar-fg: #e2e8f0;
    --sidebar-link: #93c5fd;
    --sidebar-link-hover: #bfdbfe;
    --accent: #3b82f6;
    --accent-dark: #1d4ed8;
    --code-bg: #f1f5f9;
    --border: #e2e8f0;
    --text: #1e293b;
    --text-muted: #64748b;
    --bg: #ffffff;
}

* { margin: 0; padding: 0; box-sizing: border-box; }

body {
    font-family: -apple-system, BlinkMacSystemFont, "Segoe UI", Roboto, "Helvetica Neue", Arial, sans-serif;
    color: var(--text);
    background: var(--bg);
    line-height: 1.6;
}

nav {
    position: fixed;
    top: 0; left: 0;
    width: var(--sidebar-width);
    height: 100vh;
    background: var(--sidebar-bg);
    color: var(--sidebar-fg);
    overflow-y: auto;
    padding: 1.5rem 1rem;
}

nav h2 {
    font-size: 0.85rem;
    text-transform: uppercase;
    letter-spacing: 0.05em;
    color: var(--text-muted);
    margin-bottom: 0.75rem;
    padding-bottom: 0.5rem;
    border-bottom: 1px solid #334155;
}

nav ul { list-style: none; }

nav li { margin-bottom: 0.25rem; }

nav a {
    color: var(--sidebar-link);
    text-decoration: none;
    font-size: 0.9rem;
    display: block;
    padding: 0.25rem 0.5rem;
    border-radius: 4px;
}

nav a:hover {
    color: var(--sidebar-link-hover);
    background: rgba(255,255,255,0.05);
}

nav a.active {
    background: rgba(59,130,246,0.15);
    color: #fff;
    font-weight: 600;
}

main {
    margin-left: var(--sidebar-width);
    padding: 2rem 3rem;
    max-width: 960px;
}

h1 {
    font-size: 2rem;
    color: var(--accent-dark);
    margin-bottom: 0.5rem;
    padding-bottom: 0.5rem;
    border-bottom: 2px solid var(--accent);
}

.module-doc {
    font-size: 1.05rem;
    color: var(--text-muted);
    margin-bottom: 2rem;
    white-space: pre-line;
}

section {
    margin-bottom: 2.5rem;
    padding-top: 1rem;
}

section h2 {
    font-size: 1.4rem;
    margin-bottom: 0.75rem;
}

section h2 .kind {
    font-weight: 400;
    color: var(--text-muted);
    font-size: 0.9rem;
    margin-right: 0.35rem;
}

section h2 .name {
    color: var(--accent-dark);
    font-family: "SFMono-Regular", Consolas, "Liberation Mono", Menlo, monospace;
}

.doc-text {
    color: var(--text-muted);
    margin-bottom: 0.75rem;
    white-space: pre-line;
}

pre {
    background: var(--code-bg);
    border: 1px solid var(--border);
    border-radius: 6px;
    padding: 0.75rem 1rem;
    overflow-x: auto;
    font-family: "SFMono-Regular", Consolas, "Liberation Mono", Menlo, monospace;
    font-size: 0.875rem;
    line-height: 1.5;
    margin-bottom: 0.75rem;
}

code {
    font-family: "SFMono-Regular", Consolas, "Liberation Mono", Menlo, monospace;
    font-size: 0.875em;
    background: var(--code-bg);
    padding: 0.1em 0.3em;
    border-radius: 3px;
}

pre code {
    background: none;
    padding: 0;
}

table {
    width: 100%;
    border-collapse: collapse;
    margin-bottom: 0.75rem;
}

th {
    text-align: left;
    font-size: 0.8rem;
    text-transform: uppercase;
    letter-spacing: 0.04em;
    color: var(--text-muted);
    padding: 0.5rem 0.75rem;
    border-bottom: 2px solid var(--border);
}

td {
    padding: 0.5rem 0.75rem;
    border-bottom: 1px solid var(--border);
}

td code {
    font-size: 0.85rem;
}

.returns {
    font-size: 0.95rem;
    margin-bottom: 0.75rem;
}

.returns strong { color: var(--text); }

.properties {
    list-style: none;
    padding: 0;
}

.properties li {
    padding: 0.25rem 0;
    font-size: 0.9rem;
}

.properties li::before {
    content: "•";
    color: var(--accent);
    font-weight: bold;
    margin-right: 0.5rem;
}

.attributes {
    margin-bottom: 0.5rem;
}

.attributes span {
    display: inline-block;
    background: var(--code-bg);
    border: 1px solid var(--border);
    border-radius: 4px;
    padding: 0.1rem 0.4rem;
    font-size: 0.8rem;
    font-family: monospace;
    margin-right: 0.25rem;
}

.index-list {
    list-style: none;
    padding: 0;
}

.index-list li {
    padding: 0.5rem 0;
    border-bottom: 1px solid var(--border);
}

.index-list a {
    color: var(--accent);
    text-decoration: none;
    font-weight: 500;
    font-size: 1.05rem;
}

.index-list a:hover { text-decoration: underline; }

.index-list .desc {
    display: block;
    color: var(--text-muted);
    font-size: 0.9rem;
    margin-top: 0.15rem;
}

@media print {
    nav { display: none; }
    main { margin-left: 0; max-width: 100%; }
}

@media (max-width: 768px) {
    nav { position: static; width: 100%; height: auto; }
    main { margin-left: 0; padding: 1.5rem; }
}
"#;

/// Info about a module for cross-linking in the sidebar.
#[derive(Debug, Clone)]
pub struct ModuleInfo {
    pub name: String,
    pub html_filename: String,
}

/// Render a full HTML page for a single source file.
pub fn render_html_page(
    path: &Path,
    file: &SourceFile,
    all_modules: &[ModuleInfo],
) -> String {
    let title = path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("module");

    let mut out = String::new();
    let _ = writeln!(out, "<!DOCTYPE html>");
    let _ = writeln!(out, "<html lang=\"en\">");
    let _ = writeln!(out, "<head>");
    let _ = writeln!(out, "<meta charset=\"utf-8\">");
    let _ = writeln!(out, "<meta name=\"viewport\" content=\"width=device-width, initial-scale=1\">");
    let _ = writeln!(out, "<title>{title} - Aevia Docs</title>");
    let _ = writeln!(out, "<link rel=\"stylesheet\" href=\"style.css\">");
    let _ = writeln!(out, "</head>");
    let _ = writeln!(out, "<body>");

    // Sidebar navigation
    render_sidebar(&mut out, title, all_modules);

    // Main content
    let _ = writeln!(out, "<main>");
    let _ = writeln!(out, "<h1><code>{title}</code></h1>");
    if let Some(doc) = &file.doc {
        let _ = writeln!(out, "<p class=\"module-doc\">{}</p>", escape_html(doc));
    }
    for item in &file.items {
        render_html_item(&mut out, &item.node);
    }
    let _ = writeln!(out, "</main>");

    let _ = writeln!(out, "</body>");
    let _ = writeln!(out, "</html>");
    out
}

/// Render the index page listing all modules.
pub fn render_html_index(modules: &[ModuleInfo]) -> String {
    let mut out = String::new();
    let _ = writeln!(out, "<!DOCTYPE html>");
    let _ = writeln!(out, "<html lang=\"en\">");
    let _ = writeln!(out, "<head>");
    let _ = writeln!(out, "<meta charset=\"utf-8\">");
    let _ = writeln!(out, "<meta name=\"viewport\" content=\"width=device-width, initial-scale=1\">");
    let _ = writeln!(out, "<title>Aevia Documentation</title>");
    let _ = writeln!(out, "<link rel=\"stylesheet\" href=\"style.css\">");
    let _ = writeln!(out, "</head>");
    let _ = writeln!(out, "<body>");

    render_sidebar(&mut out, "index", modules);

    let _ = writeln!(out, "<main>");
    let _ = writeln!(out, "<h1>Aevia Documentation</h1>");
    let _ = writeln!(out, "<ul class=\"index-list\">");
    for m in modules {
        let _ = writeln!(
            out,
            "<li><a href=\"{}\">{}</a></li>",
            m.html_filename,
            escape_html(&m.name)
        );
    }
    let _ = writeln!(out, "</ul>");
    let _ = writeln!(out, "</main>");

    let _ = writeln!(out, "</body>");
    let _ = writeln!(out, "</html>");
    out
}

fn render_sidebar(out: &mut String, active: &str, modules: &[ModuleInfo]) {
    let _ = writeln!(out, "<nav>");
    let _ = writeln!(out, "<h2>Modules</h2>");
    let _ = writeln!(out, "<ul>");
    // Index link
    let idx_class = if active == "index" { " class=\"active\"" } else { "" };
    let _ = writeln!(out, "<li><a href=\"index.html\"{idx_class}>Index</a></li>");
    for m in modules {
        let cls = if m.name == active { " class=\"active\"" } else { "" };
        let _ = writeln!(
            out,
            "<li><a href=\"{}\"{}>{}</a></li>",
            m.html_filename,
            cls,
            escape_html(&m.name)
        );
    }
    let _ = writeln!(out, "</ul>");
    let _ = writeln!(out, "</nav>");
}

fn render_html_item(out: &mut String, item: &Item) {
    match item {
        Item::TypeAlias { name, dimension_expr, doc } => {
            let _ = writeln!(out, "<section id=\"type-{name}\">");
            let _ = writeln!(
                out,
                "<h2><span class=\"kind\">type</span> <span class=\"name\">{name}</span></h2>"
            );
            if let Some(d) = doc {
                let _ = writeln!(out, "<p class=\"doc-text\">{}</p>", escape_html(d));
            }
            let _ = writeln!(
                out,
                "<pre><code>type {name} = {};</code></pre>",
                format_dim(&dimension_expr.node)
            );
            let _ = writeln!(out, "</section>");
        }
        Item::Struct { name, fields, visibility, doc, .. } => {
            if !is_public(*visibility) { return; }
            let _ = writeln!(out, "<section id=\"struct-{name}\">");
            let _ = writeln!(
                out,
                "<h2><span class=\"kind\">struct</span> <span class=\"name\">{name}</span></h2>"
            );
            if let Some(d) = doc {
                let _ = writeln!(out, "<p class=\"doc-text\">{}</p>", escape_html(d));
            }
            let _ = writeln!(out, "<table>");
            let _ = writeln!(out, "<thead><tr><th>Field</th><th>Dimension</th></tr></thead>");
            let _ = writeln!(out, "<tbody>");
            for field in fields {
                let _ = writeln!(
                    out,
                    "<tr><td><code>{}</code></td><td><code>{}</code></td></tr>",
                    escape_html(&field.name),
                    format_dim(&field.dim.node)
                );
            }
            let _ = writeln!(out, "</tbody></table>");
            let _ = writeln!(out, "</section>");
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
            if !is_public(*visibility) && *name != "main" { return; }
            let _ = writeln!(out, "<section id=\"fn-{name}\">");
            let _ = writeln!(
                out,
                "<h2><span class=\"kind\">fn</span> <span class=\"name\">{name}</span></h2>"
            );
            if let Some(d) = doc {
                let _ = writeln!(out, "<p class=\"doc-text\">{}</p>", escape_html(d));
            }
            if !attributes.is_empty() {
                let _ = write!(out, "<div class=\"attributes\">");
                for attr in attributes {
                    let _ = write!(out, "<span>#[{}]</span>", escape_html(&attr.name));
                }
                let _ = writeln!(out, "</div>");
            }

            // Signature code block
            let params_str: Vec<String> = params
                .iter()
                .map(|p| {
                    let mut s = String::new();
                    if p.is_mut { s.push_str("mut "); }
                    let _ = write!(s, "{}: {}", p.name, format_dim(&p.dim.node));
                    s
                })
                .collect();
            let ret = return_type
                .as_ref()
                .map(|r| format!(" -&gt; {}", format_dim(&r.node)))
                .unwrap_or_default();
            let _ = writeln!(
                out,
                "<pre><code>fn {}({}){} </code></pre>",
                escape_html(name),
                params_str.join(", "),
                ret
            );

            // Return type
            let ret_str = return_type
                .as_ref()
                .map(|r| format_dim(&r.node))
                .unwrap_or_else(|| "dimensionless".to_string());
            let _ = writeln!(
                out,
                "<p class=\"returns\"><strong>Returns:</strong> <code>{ret_str}</code></p>"
            );

            // Parameters table
            if !params.is_empty() {
                let _ = writeln!(out, "<table>");
                let _ = writeln!(out, "<thead><tr><th>Parameter</th><th>Dimension</th></tr></thead>");
                let _ = writeln!(out, "<tbody>");
                for Param { name, dim, .. } in params {
                    let _ = writeln!(
                        out,
                        "<tr><td><code>{}</code></td><td><code>{}</code></td></tr>",
                        escape_html(name),
                        format_dim(&dim.node)
                    );
                }
                let _ = writeln!(out, "</tbody></table>");
            }
            let _ = writeln!(out, "</section>");
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
            if !is_public(*visibility) { return; }
            let _ = writeln!(out, "<section id=\"op-{name}\">");
            let _ = writeln!(
                out,
                "<h2><span class=\"kind\">op</span> <span class=\"name\">{name}</span></h2>"
            );
            if let Some(d) = doc {
                let _ = writeln!(out, "<p class=\"doc-text\">{}</p>", escape_html(d));
            }
            let _ = writeln!(
                out,
                "<pre><code>op {}({}: {} -&gt; {})</code></pre>",
                escape_html(name),
                escape_html(param_name),
                format_dim(&input_dim.node),
                format_dim(&output_dim.node)
            );
            let _ = writeln!(out, "<ul class=\"properties\">");
            let _ = writeln!(out, "<li>vectorizable: <code>{}</code></li>", properties.vectorizable);
            let _ = writeln!(out, "<li>commutative: <code>{}</code></li>", properties.commutative);
            let _ = writeln!(out, "<li>associative: <code>{}</code></li>", properties.associative);
            let cost_str = properties
                .cost
                .as_ref()
                .map(|c| c.to_string())
                .unwrap_or_else(|| "default".to_string());
            let _ = writeln!(out, "<li>cost: <code>{cost_str}</code></li>");
            let _ = writeln!(out, "</ul>");
            let _ = writeln!(out, "</section>");
        }
        Item::Module { name, body, visibility, doc, .. } => {
            if !is_public(*visibility) { return; }
            let _ = writeln!(out, "<section id=\"mod-{name}\">");
            let _ = writeln!(
                out,
                "<h2><span class=\"kind\">mod</span> <span class=\"name\">{name}</span></h2>"
            );
            if let Some(d) = doc {
                let _ = writeln!(out, "<p class=\"doc-text\">{}</p>", escape_html(d));
            }
            if let Some(items) = body {
                for child in items {
                    render_html_item(out, &child.node);
                }
            }
            let _ = writeln!(out, "</section>");
        }
        Item::Const { name, dim, value, visibility, doc } => {
            if !is_public(*visibility) { return; }
            let _ = writeln!(out, "<section id=\"const-{name}\">");
            let _ = writeln!(
                out,
                "<h2><span class=\"kind\">const</span> <span class=\"name\">{name}</span></h2>"
            );
            if let Some(d) = doc {
                let _ = writeln!(out, "<p class=\"doc-text\">{}</p>", escape_html(d));
            }
            let dim_str = dim
                .as_ref()
                .map(|d| format_dim(&d.node))
                .unwrap_or_else(|| "inferred".to_string());
            let _ = writeln!(
                out,
                "<pre><code>const {}: {} = {};</code></pre>",
                escape_html(name),
                dim_str,
                format_expr(&value.node)
            );
            let _ = writeln!(out, "</section>");
        }
        Item::Use { .. } | Item::MacroDef { .. } | Item::MacroCall { .. } => {}
    }
}

fn is_public(v: Visibility) -> bool {
    matches!(v, Visibility::Public | Visibility::Crate)
}

fn escape_html(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}
