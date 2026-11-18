//! `aevia doc` — Markdown API documentation from `##` doc comments.
//!
//! One page per module plus an index. A declaration with no doc comment is
//! listed with its signature and nothing else: the generator never invents
//! prose, because a page that reads as documentation but was not written by
//! anyone is worse than a blank one.
//!
//! [`doctest`] extracts the `aevia` fenced blocks for `aevia test --doc`: a
//! page's examples are checked by compiling them, not only by rendering them.

#![forbid(unsafe_code)]

pub mod doctest;

use aevia_fmt::print::Printer;
use aevia_syntax::ast::*;
use aevia_syntax::lex;
use aevia_syntax::parse_module;
use std::fmt::Write as _;

/// One documented declaration.
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize)]
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
    /// Whether the declaration is hidden from documentation (`@hidden` or `@doc(hidden)`).
    pub doc_hidden: bool,
}

/// One module's documentation.
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize)]
pub struct ModuleDoc {
    /// The module path, `Kinematics` or `Thermo.IdealGas`.
    pub path: String,
    /// The file it came from, as the caller named it.
    pub file: String,
    /// The module's own description: the `##` block above the `module` line.
    ///
    /// The parser attaches that block to the first declaration; it is lifted
    /// out here so it describes the module and not one function.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub doc: Option<String>,
    /// The documented declarations, in source order.
    pub decls: Vec<DeclDoc>,
}

impl ModuleDoc {
    /// The file name this module's page is written to.
    #[must_use]
    pub fn page(&self) -> String {
        format!("{}.md", self.path)
    }

    /// The declarations whose `##` comment is missing.
    ///
    /// A page lists them, but a generator that reports the gap is how a
    /// package can make "every public declaration is documented" a check
    /// rather than a hope.
    #[must_use]
    pub fn undocumented(&self) -> Vec<&DeclDoc> {
        self.decls
            .iter()
            .filter(|decl| decl.doc.is_none())
            .collect()
    }

    /// How many declarations have no `##` comment.
    #[must_use]
    pub fn undocumented_count(&self) -> usize {
        self.decls.iter().filter(|decl| decl.doc.is_none()).count()
    }

    /// The declarations [`render_module`] shows: neither `private` nor hidden.
    pub fn public_decls(&self) -> impl Iterator<Item = &DeclDoc> {
        self.decls
            .iter()
            .filter(|decl| !decl.private && !decl.doc_hidden)
    }
}

/// Collects one module's documentation.
///
/// # Errors
///
/// A message with the line number when the file does not parse: documentation
/// generated from a tree the parser guessed at would be wrong.
pub fn module_doc(
    source: &str,
    file: &str,
    fallback_name: &str,
) -> Result<ModuleDoc, String> {
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
    let header = module_header(source);
    let mut decls = Vec::new();
    for decl in &module.decls {
        // An `import` has no documentation to write, and a `test` is not part
        // of the API surface — `aevia test` is where tests are listed.
        if matches!(decl.kind, DeclKind::Import { .. } | DeclKind::Test { .. }) {
            continue;
        }
        let (kind, name, signature) = signature(decl);
        let mut doc = decl.doc.clone();
        // The first declaration inherits the module header; take it back.
        if decls.is_empty()
            && let (Some(header), Some(text)) = (&header, &doc)
            && let Some(rest) = text.strip_prefix(header.as_str())
        {
            let rest = rest.trim();
            doc = (!rest.is_empty()).then(|| rest.to_owned());
        }
        decls.push(DeclDoc {
            kind,
            name,
            signature,
            doc,
            private: decl.private,
            doc_hidden: decl.is_doc_hidden(),
        });
    }
    Ok(ModuleDoc {
        path,
        file: file.to_string(),
        doc: header,
        decls,
    })
}

/// The `##` lines at the top of a file, before the `module` line.
fn module_header(source: &str) -> Option<String> {
    let mut lines = Vec::new();
    for raw in source.lines() {
        let line = raw.trim();
        if let Some(rest) = line.strip_prefix("##") {
            lines.push(rest.strip_prefix(' ').unwrap_or(rest).trim_end().to_owned());
        } else if line.starts_with('#') || line.is_empty() {
            // A plain comment or a blank line does not end the header.
        } else if line.starts_with("module ") {
            break;
        } else {
            return None;
        }
    }
    let text = lines.join("
");
    let text = text.trim();
    (!text.is_empty()).then(|| text.to_owned())
}

fn first_error(
    source: &str,
    diagnostics: &aevia_syntax::Diagnostics,
) -> String {
    let Some(first) = diagnostics.items.first() else {
        return "the file does not parse".to_string();
    };
    let line = 1 + source[..usize::try_from(first.span.start)
        .unwrap_or(0)
        .min(source.len())]
        .matches('\n')
        .count();
    format!("line {line}: {}: {}", first.code, first.message)
}

/// The kind, name and signature of a declaration.
fn signature(decl: &Decl) -> (String, String, String) {
    let kind = match &decl.kind {
        | DeclKind::Import { .. } => "import",
        | DeclKind::Dimension { .. } => "dimension",
        | DeclKind::Unit { .. } => "unit",
        | DeclKind::Type { .. } => "type",
        | DeclKind::Class { .. } => "class",
        | DeclKind::Instance { .. } => "instance",
        | DeclKind::Derive { .. } => "derive",
        | DeclKind::Fn { .. } => "fn",
        | DeclKind::Let { .. } => "let",
        | DeclKind::Test { .. } => "test",
    }
    .to_string();
    let (name, signature) = match &decl.kind {
        | DeclKind::Import { path, .. } => (path.join("."), String::new()),
        | DeclKind::Dimension { name, value } => {
            let mut text = format!("dimension {name}");
            if let Some(value) = value {
                let _ = write!(text, " = {}", value.to_source());
            }
            (name.clone(), text)
        },
        | DeclKind::Unit { name, dim, .. } => {
            (name.clone(), format!("unit {name} : {}", dim.to_source()))
        },
        | DeclKind::Type { name, params, .. } => {
            (name.clone(), format!("type {name}{}", type_params(params)))
        },
        | DeclKind::Class {
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
            for method in methods {
                let prefix = if method.private {
                    "private "
                } else {
                    ""
                };
                let _ = write!(
                    text,
                    "\n    {prefix}{}: {}",
                    method.name,
                    type_text(&method.ty)
                );
            }
            (name.clone(), text)
        },
        | DeclKind::Instance { class, ty, .. } => {
            (
                class.clone(),
                format!("instance {class} for {}", type_text(ty)),
            )
        },
        | DeclKind::Derive { classes, ty } => {
            (
                classes.join(", "),
                format!("derive {} for {}", classes.join(", "), type_text(ty)),
            )
        },
        | DeclKind::Fn {
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
        },
        | DeclKind::Let {
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
        },
        | DeclKind::Test { name, .. } => (name.clone(), format!("test {name:?}")),
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
        | PatKind::Var(name) => name.clone(),
        | PatKind::Wildcard => "_".to_string(),
        | PatKind::Tuple(items) => {
            format!(
                "({})",
                items
                    .iter()
                    .map(pattern_text)
                    .collect::<Vec<_>>()
                    .join(", ")
            )
        },
        | _ => "_".to_string(),
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
            .map(|param| {
                match param.kind {
                    | Some(kind) => format!("{}: {}", param.name, kind.as_str()),
                    | None => param.name.clone(),
                }
            })
            .collect::<Vec<_>>()
            .join(", ")
    )
}

fn plural(
    count: usize,
    word: &str,
) -> String {
    if count == 1 {
        word.to_string()
    } else {
        format!("{word}s")
    }
}

/// Renders one module's public API page, omitting `private` and `@hidden` / `@doc(hidden)` items.
#[must_use]
pub fn render_module(doc: &ModuleDoc) -> String {
    render_module_with_options(doc, false, false)
}

/// Renders one module's page with explicit control over hidden and private items.
#[must_use]
pub fn render_module_with_options(
    doc: &ModuleDoc,
    include_hidden: bool,
    include_private: bool,
) -> String {
    let mut out = String::new();
    let _ = writeln!(out, "# `{}`\n", doc.path);
    let _ = writeln!(out, "Source: `{}`\n", doc.file);
    if let Some(text) = &doc.doc {
        let _ = writeln!(out, "{}\n", text.trim_end());
    }
    let visible: Vec<&DeclDoc> = doc
        .decls
        .iter()
        .filter(|decl| (include_hidden || !decl.doc_hidden) && (include_private || !decl.private))
        .collect();
    if visible.is_empty() {
        out.push_str("This module declares nothing.\n");
        return out;
    }
    for decl in visible {
        let private = if decl.private {
            " (private)"
        } else {
            ""
        };
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

/// Documentation for one RSSN kernel operator, with full untruncated prose and examples.
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize)]
pub struct OperatorDoc {
    /// The operator's name.
    pub name: String,
    /// The rule set that registered it.
    pub rule_set: String,
    /// Rendered overload signatures.
    pub signatures: Vec<String>,
    /// Stability level.
    pub stability: String,
    /// Full, untruncated operator documentation.
    pub doc: String,
    /// Recorded call examples `(call, result_sort)`.
    pub examples: Vec<(String, String)>,
}

/// Combined documentation bundle for the Aevia standard library and RSSN kernel operators.
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize)]
pub struct StdlibDocBundle {
    /// Parsed `aevia-std/stdlib/*.aev` module documentation.
    pub modules: Vec<ModuleDoc>,
    /// Full, untruncated `aevia_rssn::registry()` operator documentation.
    pub operators: Vec<OperatorDoc>,
}

impl StdlibDocBundle {
    /// Serializes the documentation bundle to pretty-printed JSON.
    ///
    /// # Errors
    ///
    /// Returns a `serde_json::Error` if serialization fails.
    pub fn to_json(&self) -> Result<String, serde_json::Error> {
        serde_json::to_string_pretty(self)
    }

    /// Renders the documentation bundle as a combined Markdown document.
    #[must_use]
    pub fn to_markdown(&self) -> String {
        let mut out = render_index("Aevia Standard Library", &self.modules);
        for module in &self.modules {
            out.push('\n');
            out.push_str(&render_module(module));
        }
        out.push_str("\n# RSSN Kernel Operators\n\n");
        for op in &self.operators {
            let _ = writeln!(out, "## `{}.{}` ({})\n", op.rule_set, op.name, op.stability);
            if !op.signatures.is_empty() {
                out.push_str("```aevia\n");
                for sig in &op.signatures {
                    let _ = writeln!(out, "{sig}");
                }
                out.push_str("```\n\n");
            }
            if !op.doc.trim().is_empty() {
                let _ = writeln!(out, "{}\n", op.doc.trim_end());
            }
            if !op.examples.is_empty() {
                out.push_str("### Examples\n\n");
                for (call, sort) in &op.examples {
                    let _ = writeln!(out, "- `{call}` → `{sort}`");
                }
                out.push('\n');
            }
        }
        out
    }
}

/// Generates the full standard-library documentation bundle, combining
/// `aevia-std/stdlib/*.aev` module docs with untruncated `aevia_rssn::registry()`
/// operator docs and examples.
///
/// # Errors
///
/// Returns an error if any embedded stdlib `.aev` module fails to parse.
pub fn generate_stdlib_docs() -> Result<StdlibDocBundle, String> {
    let mut modules = Vec::new();
    for entry in aevia_std::stdlib::modules() {
        let file = format!("stdlib/{}.aev", entry.path);
        let fallback = entry.path.replace('/', ".");
        modules.push(module_doc(entry.source, &file, &fallback)?);
    }
    let operators = aevia_rssn::registry::registry()
        .ops()
        .map(|op| {
            OperatorDoc {
                name: op.name.clone(),
                rule_set: op.rule_set.clone(),
                signatures: op.signatures.iter().map(aevia_rssn::Sig::render).collect(),
                stability: format!("{:?}", op.stability),
                doc: op.doc.clone(),
                examples: op.witnesses.clone(),
            }
        })
        .collect();
    Ok(StdlibDocBundle { modules, operators })
}

/// Renders the index page.
#[must_use]
pub fn render_index(
    name: &str,
    docs: &[ModuleDoc],
) -> String {
    let mut out = String::new();
    let _ = writeln!(out, "# {name} — API\n");
    // The counts describe the pages the index links to, which show the
    // public API only (see `render_module`).
    let public = |doc: &ModuleDoc| doc.public_decls().count();
    let public_undocumented =
        |doc: &ModuleDoc| doc.public_decls().filter(|decl| decl.doc.is_none()).count();
    let count: usize = docs.iter().map(public).sum();
    let undocumented: usize = docs.iter().map(public_undocumented).sum();
    let _ = writeln!(
        out,
        "Generated by `aevia doc` from the `##` comments. {count} {} in {} {}. \
         {undocumented} without a `##` doc comment.\n",
        plural(count, "declaration"),
        docs.len(),
        plural(docs.len(), "module"),
    );
    out.push_str("| Module | Declarations | Undocumented |\n|---|---|---|\n");
    for doc in docs {
        let _ = writeln!(
            out,
            "| [`{}`]({}) | {} | {} |",
            doc.path,
            doc.page(),
            public(doc),
            public_undocumented(doc)
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
        assert_eq!(doc.decls[0].signature, "fn circle(r: Real[L]): Real[L^2]");
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
    fn the_module_header_describes_the_module_not_its_first_declaration() {
        let source = "## The `Geo` module.\n# a plain comment\nmodule Geo\n\n## Square of `x`.\nfn sq(x: Int): Int = x * x\n\nfn bare(x: Int): Int = x\n";
        let doc = module_doc(source, "geo.aev", "Geo").expect("parses");
        assert_eq!(doc.doc.as_deref(), Some("The `Geo` module."));
        assert_eq!(doc.decls[0].doc.as_deref(), Some("Square of `x`."));
        assert_eq!(doc.decls[1].doc, None);
        let page = render_module(&doc);
        assert_eq!(page.matches("The `Geo` module.").count(), 1, "{page}");
        let head = page.split("## `fn sq`").next().unwrap();
        assert!(head.contains("The `Geo` module."), "{page}");
    }

    #[test]
    fn the_index_links_every_module() {
        let one = module_doc(SOURCE, "a.aev", "One").expect("parses");
        let two = module_doc("let x = 1\n", "b.aev", "Two").expect("parses");
        let index = render_index("demo", &[one, two]);
        // Counts describe the linked pages: the private `hidden` is not on
        // `One`'s page, so it is not counted either.
        assert!(index.contains("| [`One`](One.md) | 2 | 1 |"), "{index}");
        assert!(index.contains("| [`Two`](Two.md) | 1 | 1 |"));
        // The index names the gap instead of hiding it.
        assert!(index.contains("2 without a `##` doc comment"), "{index}");
    }

    #[test]
    fn undocumented_declarations_are_reported() {
        let doc = module_doc(SOURCE, "a.aev", "A").expect("parses");
        assert_eq!(doc.undocumented_count(), 2);
        let names: Vec<&str> = doc
            .undocumented()
            .iter()
            .map(|decl| decl.name.as_str())
            .collect();
        assert_eq!(names, ["undocumented", "hidden"]);
    }

    #[test]
    fn a_file_that_does_not_parse_is_refused() {
        let error = module_doc("fn f( = 1\n", "a.aev", "A").expect_err("refuses");
        assert!(error.contains("line 1"), "{error}");
    }

    #[test]
    fn doc_hidden_and_private_items_are_filtered_unless_requested() {
        let source = "\
## Public item.
fn visible(): Int = 1

@hidden
fn attr_hidden(): Int = 2

@doc(hidden)
fn doc_attr_hidden(): Int = 3

private fn secret(): Int = 4
";
        let doc = module_doc(source, "src/vis.aev", "Vis").expect("parses");
        assert!(!doc.decls[0].doc_hidden);
        assert!(doc.decls[1].doc_hidden);
        assert!(doc.decls[2].doc_hidden);
        assert!(doc.decls[3].private);

        let public_page = render_module(&doc);
        assert!(public_page.contains("visible"));
        assert!(!public_page.contains("attr_hidden"));
        assert!(!public_page.contains("doc_attr_hidden"));
        assert!(!public_page.contains("secret"));

        let full_page = render_module_with_options(&doc, true, true);
        assert!(full_page.contains("visible"));
        assert!(full_page.contains("attr_hidden"));
        assert!(full_page.contains("doc_attr_hidden"));
        assert!(full_page.contains("secret"));
    }

    #[test]
    fn stdlib_doc_bundle_combines_modules_and_untruncated_operators() {
        let bundle = generate_stdlib_docs().expect("generates stdlib docs");
        assert!(!bundle.modules.is_empty());
        assert!(!bundle.operators.is_empty());
        let json = bundle.to_json().expect("serializes to json");
        assert!(json.contains("\"modules\""));
        assert!(json.contains("\"operators\""));
        let md = bundle.to_markdown();
        assert!(md.contains("# Aevia Standard Library"));
        assert!(md.contains("# RSSN Kernel Operators"));
    }
}
