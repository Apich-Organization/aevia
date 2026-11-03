//! `aevia doc` — Markdown API documentation from `##` doc comments.
//!
//! One page per module plus an index. A declaration with no doc comment is
//! listed with its signature and nothing else: the generator never invents
//! prose, because a page that reads as documentation but was not written by
//! anyone is worse than a blank one.

#![forbid(unsafe_code)]

use aevia_fmt::print::Printer;
use aevia_syntax::ast::*;
use aevia_syntax::{lex, parse_module};
use std::fmt::Write as _;

/// One documented declaration.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DeclDoc {
    /// `fn`, `let`, `type`, `unit`, `dimension`, `class`, `instance`, `derive`.
    pub kind: String,
    /// The declared name, or a short description for an instance.
    pub name: String,
    /// The signature, as the author wrote it.
    pub signature: String,
    /// The `##` text, verbatim.
    pub doc: Option<String>,
    /// Whether the declaration is not exported.
    pub private: bool,
}

/// One module's documentation.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ModuleDoc {
    /// The module path, `Kinematics` or `Thermo.IdealGas`.
    pub path: String,
    /// The file it came from, as the caller named it.
    pub file: String,
    /// The documented declarations, in source order.
    pub decls: Vec<DeclDoc>,
}

impl ModuleDoc {
    /// The file name this module's page is written to.
    #[must_use]
    pub fn page(&self) -> String {
        format!("{}.md", self.path)
    }
}

/// Collects one module's documentation.
///
/// # Errors
///
/// A message with the line number when the file does not parse: documentation
/// generated from a tree the parser guessed at would be wrong.
pub fn module_doc(source: &str, file: &str, fallback_name: &str) -> Result<ModuleDoc, String> {
    let lexed = lex(0, source).map_err(|diagnostics| first_error(source, &diagnostics))?;
    let (module, diagnostics) = parse_module(0, source, &lexed);
    if diagnostics.has_errors() {
        return Err(first_error(source, &diagnostics));
    }
    let path = module
        .name
        .as_ref()
        .map(|name| name.join("."))
        .unwrap_or_else(|| fallback_name.to_string());
    let mut decls = Vec::new();
    for decl in &module.decls {
        // An `import` has no documentation to write, and a `test` is not part
        // of the API surface — `aevia test` is where tests are listed.
        if matches!(decl.kind, DeclKind::Import { .. } | DeclKind::Test { .. }) {
            continue;
        }
        let (kind, name, signature) = signature(decl);
        decls.push(DeclDoc {
            kind,
            name,
            signature,
            doc: decl.doc.clone(),
            private: decl.private,
        });
    }
    Ok(ModuleDoc {
        path,
        file: file.to_string(),
        decls,
    })
}

fn first_error(source: &str, diagnostics: &aevia_syntax::Diagnostics) -> String {
    let Some(first) = diagnostics.items.first() else {
        return "the file does not parse".to_string();
    };
    let line = 1 + source[..usize::try_from(first.span.start).unwrap_or(0).min(source.len())]
        .matches('\n')
        .count();
    format!("line {line}: {}: {}", first.code, first.message)
}

/// The kind, name and signature of a declaration.
fn signature(decl: &Decl) -> (String, String, String) {
    let kind = match &decl.kind {
        DeclKind::Import { .. } => "import",
        DeclKind::Dimension { .. } => "dimension",
        DeclKind::Unit { .. } => "unit",
        DeclKind::Type { .. } => "type",
        DeclKind::Class { .. } => "class",
        DeclKind::Instance { .. } => "instance",
        DeclKind::Derive { .. } => "derive",
        DeclKind::Fn { .. } => "fn",
        DeclKind::Let { .. } => "let",
        DeclKind::Test { .. } => "test",
    }
    .to_string();
    let (name, signature) = match &decl.kind {
        DeclKind::Import { path, .. } => (path.join("."), String::new()),
        DeclKind::Dimension { name, value } => {
            let mut text = format!("dimension {name}");
            if let Some(value) = value {
                let _ = write!(text, " = {}", value.to_source());
            }
            (name.clone(), text)
        }
        DeclKind::Unit { name, dim, .. } => (
            name.clone(),
            format!("unit {name} : {}", dim.to_source()),
        ),
        DeclKind::Type { name, params, .. } => (
            name.clone(),
            format!("type {name}{}", type_params(params)),
        ),
        DeclKind::Class {
            name,
            params,
            methods,
        } => {
            let names = params
                .iter()
                .map(|param| param.name.clone())
                .collect::<Vec<_>>()
                .join(" ");
            let mut text = format!("class {name}");
            if !names.is_empty() {
                let _ = write!(text, " {names}");
            }
            text.push_str(" where");
            for (method, ty) in methods {
                let _ = write!(text, "\n    {method}: {}", type_text(ty));
            }
            (name.clone(), text)
        }
        DeclKind::Instance { class, ty, .. } => (
            class.clone(),
            format!("instance {class} for {}", type_text(ty)),
        ),
        DeclKind::Derive { classes, ty } => (
            classes.join(", "),
            format!("derive {} for {}", classes.join(", "), type_text(ty)),
        ),
        DeclKind::Fn {
            name,
            params,
            args,
            ret,
            ..
        } => {
            let mut text = format!("fn {name}{}", type_params(params));
            let rendered = args
                .iter()
                .map(|param| {
                    let mut item = pattern_text(&param.pattern);
                    if let Some(ty) = &param.ty {
                        let _ = write!(item, ": {}", type_text(ty));
                    }
                    item
                })
                .collect::<Vec<_>>()
                .join(", ");
            let _ = write!(text, "({rendered})");
            if let Some(ret) = ret {
                let _ = write!(text, ": {}", type_text(ret));
            }
            (name.clone(), text)
        }
        DeclKind::Let {
            pattern,
            ty,
            value,
            rec,
        } => {
            let name = pattern_text(pattern);
            let mut text = if *rec {
                format!("let rec {name}")
            } else {
                format!("let {name}")
            };
            if let Some(ty) = ty {
                let _ = write!(text, ": {}", type_text(ty));
            } else {
                let rendered = Printer::new(Vec::new())
                    .expression(value)
                    .unwrap_or_else(|_| "_".to_string());
                let _ = write!(text, " = {rendered}");
            }
            (name, text)
        }
        DeclKind::Test { name, .. } => (name.clone(), format!("test {name:?}")),
    };
    (kind, name, signature)
}

fn type_text(ty: &Ty) -> String {
    Printer::new(Vec::new())
        .type_(ty)
        .unwrap_or_else(|_| "<unprintable>".to_string())
}

fn pattern_text(pat: &Pat) -> String {
    match &pat.kind {
        PatKind::Var(name) => name.clone(),
        PatKind::Wildcard => "_".to_string(),
        PatKind::Tuple(items) => format!(
            "({})",
            items.iter().map(pattern_text).collect::<Vec<_>>().join(", ")
        ),
        _ => "_".to_string(),
    }
}

fn type_params(params: &[TypeParam]) -> String {
    if params.is_empty() {
        return String::new();
    }
    format!(
        "<{}>",
        params
            .iter()
            .map(|param| match param.kind {
                Some(kind) => format!("{}: {}", param.name, kind.as_str()),
                None => param.name.clone(),
            })
            .collect::<Vec<_>>()
            .join(", ")
    )
}

fn plural(count: usize, word: &str) -> String {
    if count == 1 {
        word.to_string()
    } else {
        format!("{word}s")
    }
}

/// Renders one module's page.
#[must_use]
pub fn render_module(doc: &ModuleDoc) -> String {
    let mut out = String::new();
    let _ = writeln!(out, "# `{}`\n", doc.path);
    let _ = writeln!(out, "Source: `{}`\n", doc.file);
    if doc.decls.is_empty() {
        out.push_str("This module declares nothing.\n");
        return out;
    }
    for decl in &doc.decls {
        let private = if decl.private { " (private)" } else { "" };
        let _ = writeln!(out, "## `{} {}`{}\n", decl.kind, decl.name, private);
        out.push_str("```aevia\n");
        let _ = writeln!(out, "{}", decl.signature);
        out.push_str("```\n");
        if let Some(text) = &decl.doc {
            out.push('\n');
            let _ = writeln!(out, "{}", text.trim_end());
        }
    }
    out
}

/// Renders the index page.
#[must_use]
pub fn render_index(name: &str, docs: &[ModuleDoc]) -> String {
    let mut out = String::new();
    let _ = writeln!(out, "# {name} — API\n");
    let count: usize = docs.iter().map(|doc| doc.decls.len()).sum();
    let _ = writeln!(
        out,
        "Generated by `aevia doc` from the `##` comments. {count} {} in {} {}.\n",
        plural(count, "declaration"),
        docs.len(),
        plural(docs.len(), "module"),
    );
    out.push_str("| Module | Declarations |\n|---|---|\n");
    for doc in docs {
        let _ = writeln!(
            out,
            "| [`{}`]({}) | {} |",
            doc.path,
            doc.page(),
            doc.decls.len()
        );
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    const SOURCE: &str = "## A circle's area.
##
## Uses `tau`.
fn circle(r: Real[L]): Real[L^2] = r^2

fn undocumented(x: Int): Int = x

private fn hidden(): Int = 1
";

    #[test]
    fn doc_comments_are_attached_and_missing_ones_are_not_invented() {
        let doc = module_doc(SOURCE, "src/geometry.aev", "Geometry").expect("parses");
        assert_eq!(doc.decls.len(), 3);
        assert_eq!(doc.decls[0].kind, "fn");
        assert_eq!(doc.decls[0].name, "circle");
        assert_eq!(
            doc.decls[0].signature,
            "fn circle(r: Real[L]): Real[L^2]"
        );
        assert_eq!(
            doc.decls[0].doc.as_deref(),
            Some("A circle's area.\n\nUses `tau`.")
        );
        assert_eq!(doc.decls[1].doc, None);
        assert!(doc.decls[2].private);
        let page = render_module(&doc);
        assert!(page.contains("fn circle(r: Real[L]): Real[L^2]"));
        assert!(page.contains("A circle's area."));
        // The undocumented function is listed with no prose of its own.
        let tail = page.split("## `fn undocumented`").nth(1).expect("listed");
        assert!(!tail.contains("A circle"), "{tail}");
    }

    #[test]
    fn the_index_links_every_module() {
        let one = module_doc(SOURCE, "a.aev", "One").expect("parses");
        let two = module_doc("let x = 1\n", "b.aev", "Two").expect("parses");
        let index = render_index("demo", &[one, two]);
        assert!(index.contains("| [`One`](One.md) | 3 |"));
        assert!(index.contains("| [`Two`](Two.md) | 1 |"));
    }

    #[test]
    fn a_file_that_does_not_parse_is_refused() {
        let error = module_doc("fn f( = 1\n", "a.aev", "A").expect_err("refuses");
        assert!(error.contains("line 1"), "{error}");
    }
}
