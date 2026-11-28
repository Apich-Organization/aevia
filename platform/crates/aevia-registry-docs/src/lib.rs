//! Documentation pages for a published package.
//!
//! This crate is the renderer behind `/docs/...`: it takes the text files of a
//! package tarball, asks `aevia-doc` what each module declares, and produces one
//! page per module plus an index. It never touches a database, a network or the
//! filesystem, so the server can render a version's documentation in memory and
//! store the result, and a test can render a package that was never published.
//!
//! # Why a broken file is skipped instead of failing the build
//!
//! A package is a tree of modules, and the parser is the same one `aevia check`
//! runs. A file that does not parse cannot have been published by a publisher
//! whose own check passed, but the registry must not assume that: a tarball can
//! be old, a language version can be newer than the registry's compiler, and a
//! single unparseable file in a twenty-module package is not a reason to serve
//! an empty site for all twenty. Every skip is logged with `tracing::warn!` and
//! names the file and the parser's message, so the gap is visible in the build
//! log. Only when *nothing* parses does the build fail, with [`E_DOCS`], because
//! a documentation site with no documentation is worse than a failed build the
//! publisher can see.
//!
//! # Why the pages are plain fragments
//!
//! A [`BuiltPage`] is the body of a page, not a document. The server owns the
//! layout, the version picker and the sidebar, and assembling those around a
//! body is the server's job; a crate that emitted `<html>` could not be
//! embedded, and one that emitted its own sidebar would drift from the one the
//! rest of the site draws.
//!
//! # The two paths
//!
//! With the `compiler` feature (the default) the pages come from `aevia-doc`,
//! which parses each file with the real lexer and parser. Without it — a
//! checkout that does not have the language repository next door — a degraded
//! line scanner in `fallback` produces the same structures. The scanner is
//! documented as degraded, and its pages may differ from the compiler's.

#![forbid(unsafe_code)]
#![cfg_attr(
    not(test),
    deny(
        clippy::arithmetic_side_effects,
        clippy::indexing_slicing,
        clippy::nursery,
        clippy::panic,
        clippy::pedantic,
        clippy::single_call_fn,
        clippy::unreachable
    )
)]

mod html;
mod markdown;

#[cfg(not(feature = "compiler"))]
mod fallback;

use std::collections::BTreeMap;
use std::collections::btree_map::Entry;

use aevia_registry_core::Error;
use aevia_registry_core::Result;

#[cfg(feature = "compiler")]
use aevia_doc::ModuleDoc as Module;

pub use markdown::render_manual_markdown;
pub use markdown::render_markdown;

#[cfg(feature = "compiler")]
pub use html::render_module_html;

/// `E_DOCS`: there is no source to document, so no page could be built.
pub const E_DOCS: &str = "E_DOCS";

/// The sentence the index page carries when the package has no README.
const NO_README: &str = "This package has no README.";

/// One rendered page.
///
/// `path` is the page's identity and its route: the module path, or the empty
/// string for the index. The fields beside `html` are the counts the site shows
/// in its sidebar and its "N of M documented" line, computed once here so a
/// page and the index cannot disagree about them.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BuiltPage {
    /// The module path, e.g. `"Kinematics"` or `"Thermo.IdealGas"`; `""` for
    /// the index page.
    pub path: String,
    /// The module path, or the package name on the index page.
    pub title: String,
    /// The page body, ready to be placed inside the site's layout.
    pub html: String,
    /// How many visible declarations the page lists.
    pub decls: usize,
    /// How many of them have no `##` doc comment.
    pub undocumented: usize,
    /// The tarball entry the page came from, e.g. `"src/Kinematics.aev"`.
    ///
    /// On the index page this is the README's entry name, so a "view source"
    /// link points at the file the page actually shows; it is empty when the
    /// page is the "no README" sentence.
    pub source_file: String,
}

/// Every page of one package's documentation.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BuiltDocs {
    /// The index page first (`path` is `""`), then the modules sorted by path.
    ///
    /// The index is first because it is the page a reader arrives at, and the
    /// modules are sorted because a `BTreeMap`'s order does not depend on the
    /// order entries happen to appear in the tarball — two packs of the same
    /// tree must produce the same pages.
    pub pages: Vec<BuiltPage>,
    /// Visible declarations across every module.
    pub total_decls: usize,
    /// Declarations without a `##` comment, across every module.
    pub undocumented: usize,
}

/// Builds every page from a package's files.
///
/// `files` is the tarball as [`aevia_registry_core::tarball::text_files`]
/// returns it: `(entry name, contents)` pairs. Only `*.aev` files under `src/`
/// are documented. `examples/` is deliberately excluded: an example is a
/// program that demonstrates the API, not part of the API, and listing its
/// `main` beside the package's own declarations would misrepresent the surface
/// a consumer may rely on.
///
/// A file that fails to parse is skipped with a warning and the rest of the
/// package still builds. When parsed files claim the same module path, the
/// lexicographically first archive path wins and the others are logged as skipped.
/// The compiler's module description and documentation visibility attributes
/// control the module page. Visible private declarations carry a private badge.
///
/// # Errors
///
/// [`E_DOCS`] when no file under `src/` yields documentation: either there is
/// no `*.aev` file at all, or every one of them fails to parse. Without the
/// `compiler` feature the fallback cannot detect a parse failure, so there
/// `E_DOCS` means only that there was no source file.
///
/// # Examples
///
/// ```
/// let files = vec![
///     ("README.md".to_string(), "# Demo\n\nA demo.\n".to_string()),
///     (
///         "src/Main.aev".to_string(),
///         "## The entry point.\nfn main(): Int = 0\n".to_string(),
///     ),
/// ];
/// let docs = aevia_registry_docs::build(&files, "demo").expect("docs");
/// assert_eq!(docs.pages.len(), 2);
/// assert_eq!(docs.pages[0].path, "");
/// assert_eq!(docs.pages[1].path, "Main");
/// assert_eq!(docs.total_decls, 1);
/// ```
pub fn build(
    files: &[(String, String)],
    package: &str,
) -> Result<BuiltDocs> {
    let mut modules: BTreeMap<String, Module> = BTreeMap::new();
    let mut sources = 0usize;
    for (name, contents) in files {
        let Some(stem) = source_stem(name) else {
            continue;
        };
        sources = sources.saturating_add(1);
        match module_from_source(contents, name, stem) {
            | Ok(module) => {
                match modules.entry(module.path.clone()) {
                    | Entry::Vacant(slot) => {
                        slot.insert(module);
                    },
                    | Entry::Occupied(mut slot) => {
                        let skipped = if module.file < slot.get().file {
                            slot.insert(module).file
                        } else {
                            module.file
                        };
                        tracing::warn!(
                            file = skipped.as_str(),
                            module = slot.key().as_str(),
                            selected_file = slot.get().file.as_str(),
                            "skipping a duplicate documentation module path"
                        );
                    },
                }
            },
            | Err(message) => {
                tracing::warn!(
                    file = name.as_str(),
                    error = message.as_str(),
                    "skipping a source file that does not parse"
                );
            },
        }
    }
    if modules.is_empty() {
        let message = if sources == 0 {
            format!("`{package}` has no `.aev` files under `src/` to document")
        } else {
            format!(
                "none of the {sources} `.aev` files under `src/` parse, so `{package}` has nothing to document"
            )
        };
        return Err(Error::new(E_DOCS, message));
    }

    let mut pages = Vec::with_capacity(modules.len().saturating_add(1));
    let (source_file, index_html) = match find_readme(files) {
        | Some((name, text)) => (name.clone(), render_markdown(text)),
        | None => (String::new(), render_markdown(NO_README)),
    };
    pages.push(BuiltPage {
        path: String::new(),
        title: package.to_string(),
        html: index_html,
        decls: 0,
        undocumented: 0,
        source_file,
    });

    let mut total_decls = 0usize;
    let mut undocumented = 0usize;
    for module in modules.into_values() {
        let (decls, missing) =
            visible_declarations(&module).fold((0usize, 0usize), |(decls, missing), decl| {
                (
                    decls.saturating_add(1),
                    missing.saturating_add(usize::from(decl.doc.is_none())),
                )
            });
        total_decls = total_decls.saturating_add(decls);
        undocumented = undocumented.saturating_add(missing);
        pages.push(BuiltPage {
            path: module.path.clone(),
            title: module.path.clone(),
            html: html::render_module(&module),
            decls,
            undocumented: missing,
            source_file: module.file,
        });
    }
    Ok(BuiltDocs {
        pages,
        total_decls,
        undocumented,
    })
}

#[cfg(feature = "compiler")]
use aevia_doc::DeclDoc as Decl;

/// One documented declaration in the fallback scanner.
#[cfg(not(feature = "compiler"))]
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Decl {
    /// `fn`, `let`, `type`, `unit`, …
    pub kind: String,
    /// The declared name.
    pub name: String,
    /// The signature, as the author wrote it.
    pub signature: String,
    /// The `##` text, verbatim.
    pub doc: Option<String>,
    /// Whether the declaration is not exported.
    pub private: bool,
    /// Whether a single-line visibility annotation hides this declaration.
    pub doc_hidden: bool,
}

/// One documented module in the fallback scanner.
#[cfg(not(feature = "compiler"))]
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Module {
    /// The module path, `Kinematics` or `Thermo.IdealGas`.
    pub path: String,
    /// The tarball entry the module came from.
    pub file: String,
    /// The documentation comment above the module header.
    pub doc: Option<String>,
    /// The declarations, in source order.
    pub decls: Vec<Decl>,
}

/// The declarations that contribute to both the page and its counts.
fn visible_declarations(module: &Module) -> impl Iterator<Item = &Decl> {
    module.decls.iter().filter(|decl| !decl.doc_hidden)
}

/// Documents one file with the compiler.
#[cfg(feature = "compiler")]
#[allow(
    clippy::single_call_fn,
    reason = "the compiler and fallback share this per-file documentation interface"
)]
fn module_from_source(
    source: &str,
    file: &str,
    fallback_name: &str,
) -> std::result::Result<Module, String> {
    aevia_doc::module_doc(source, file, fallback_name)
}

/// Documents one file with the degraded scanner.
#[cfg(not(feature = "compiler"))]
#[allow(
    clippy::single_call_fn,
    clippy::unnecessary_wraps,
    reason = "the fallback shares the compiler's per-file documentation interface"
)]
fn module_from_source(
    source: &str,
    file: &str,
    fallback_name: &str,
) -> std::result::Result<Module, String> {
    Ok(fallback::scan(source, file, fallback_name))
}

/// The file stem of a `src/*.aev` entry, or `None` for anything else.
///
/// The stem is only a fallback name: a module that declares `module X.Y` is
/// documented under that path, and a file that declares nothing takes its own
/// name. That is the same rule `aevia doc` applies, so the Markdown and the HTML
/// of one package name their pages identically.
#[allow(
    clippy::single_call_fn,
    reason = "the documented source path policy is independent of rendering"
)]
fn source_stem(name: &str) -> Option<&str> {
    let name = name.strip_prefix("src/")?;
    let (stem, extension) = name.rsplit('/').next()?.rsplit_once('.')?;
    (!stem.is_empty() && extension.eq_ignore_ascii_case("aev")).then_some(stem)
}

/// The README of a package, preferring the shallowest one. At equal depths,
/// use the lexicographically first archive path.
///
/// `README.md` is matched case-insensitively because a package packed on
/// Windows may spell it `Readme.MD`, and the file is the author's README
/// whichever way it is cased.
#[allow(
    clippy::single_call_fn,
    reason = "README discovery applies its own case-insensitive basename and nesting policy"
)]
fn find_readme(files: &[(String, String)]) -> Option<&(String, String)> {
    files
        .iter()
        .filter(|(name, _)| {
            name.rsplit('/')
                .next()
                .is_some_and(|base| base.eq_ignore_ascii_case("README.md"))
        })
        .min_by(|(left, _), (right, _)| {
            left.matches('/')
                .count()
                .cmp(&right.matches('/').count())
                .then_with(|| left.cmp(right))
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A package with two modules, a README and a manifest.
    fn package() -> Vec<(String, String)> {
        vec![
            (
                "aevia.toml".to_string(),
                "[package]\nname = \"demo\"\nversion = \"0.1.0\"\n".to_string(),
            ),
            (
                "README.md".to_string(),
                "# Demo\n\nA demo package.\n\n| Module | Decls |\n|---|---|\n| `Kinematics` | 3 |\n"
                    .to_string(),
            ),
            (
                "src/Kinematics.aev".to_string(),
                "module Kinematics\n\n## A circle's area.\n##\n## Uses `tau`.\nfn circle(r: Real[L]): Real[L^2] = r^2\n\nfn undocumented(x: Int): Int = x\n\nprivate fn hidden(): Int = 1\n"
                    .to_string(),
            ),
            (
                "src/Thermo.aev".to_string(),
                "module Thermo\n\n## The gas constant.\nlet k = 1\n".to_string(),
            ),
        ]
    }

    fn page<'a>(
        docs: &'a BuiltDocs,
        path: &str,
    ) -> &'a BuiltPage {
        docs.pages
            .iter()
            .find(|page| page.path == path)
            .expect("a page for the path")
    }

    #[test]
    fn a_two_module_package_builds_an_index_and_two_pages() {
        let docs = build(&package(), "demo").expect("docs");
        assert_eq!(docs.pages.len(), 3);
        assert_eq!(docs.pages[0].path, "");
        assert_eq!(docs.pages[1].path, "Kinematics");
        assert_eq!(docs.pages[2].path, "Thermo");
        assert_eq!(docs.total_decls, 4);
        assert_eq!(docs.undocumented, 2);
    }

    #[test]
    fn the_index_is_the_rendered_readme() {
        let docs = build(&package(), "demo").expect("docs");
        let index = &docs.pages[0];
        assert_eq!(index.title, "demo");
        assert_eq!(index.source_file, "README.md");
        assert!(index.html.contains("<h1>Demo</h1>"), "{}", index.html);
        assert!(index.html.contains("A demo package."), "{}", index.html);
        assert!(index.html.contains("<table>"), "{}", index.html);
    }

    #[test]
    fn a_package_without_a_readme_says_so() {
        let files: Vec<(String, String)> = vec![(
            "src/Main.aev".to_string(),
            "fn main(): Int = 0\n".to_string(),
        )];
        let docs = build(&files, "bare").expect("docs");
        assert_eq!(docs.pages[0].source_file, "");
        assert_eq!(
            docs.pages[0].html.trim(),
            "<p>This package has no README.</p>"
        );
    }

    #[test]
    fn a_module_page_holds_the_signature_the_doc_and_the_counts() {
        let docs = build(&package(), "demo").expect("docs");
        let module = page(&docs, "Kinematics");
        assert_eq!(module.title, "Kinematics");
        assert_eq!(module.source_file, "src/Kinematics.aev");
        assert_eq!(module.decls, 3);
        assert_eq!(module.undocumented, 2);
        assert!(!module.html.contains("<h1>"), "{}", module.html);
        assert!(
            module.html.contains("fn circle(r: Real[L]): Real[L^2]"),
            "{}",
            module.html
        );
        assert!(module.html.contains("A circle's area."), "{}", module.html);
        assert!(
            module.html.contains("Uses <code>tau</code>"),
            "{}",
            module.html
        );
        assert!(
            module
                .html
                .contains("<span class=\"badge private\">private</span>"),
            "{}",
            module.html
        );
    }

    #[test]
    fn anchors_are_stable_and_the_undocumented_declaration_has_no_prose() {
        let docs = build(&package(), "demo").expect("docs");
        let module = page(&docs, "Kinematics");
        assert!(
            module
                .html
                .contains("<div class=\"decl\" id=\"fn-circle\">"),
            "{}",
            module.html
        );
        assert!(
            module
                .html
                .contains("<div class=\"decl\" id=\"fn-undocumented\">"),
            "{}",
            module.html
        );
        // Rendering twice produces the same ids: the slug is a pure function.
        let again = build(&package(), "demo").expect("docs");
        assert_eq!(
            page(&again, "Kinematics").html,
            module.html,
            "a rebuild must be byte-identical"
        );
        // The declaration with no `##` comment carries no invented prose.
        let tail = module
            .html
            .split("id=\"fn-undocumented\"")
            .nth(1)
            .expect("block");
        let block = tail.split("</div>").next().expect("block");
        assert!(!block.contains("class=\"doc\""), "{block}");
    }

    #[test]
    fn a_documented_module_reports_no_gap() {
        let docs = build(&package(), "demo").expect("docs");
        let thermo = page(&docs, "Thermo");
        assert_eq!(thermo.decls, 1);
        assert_eq!(thermo.undocumented, 0);
        assert!(thermo.html.contains("The gas constant."), "{}", thermo.html);
    }

    #[test]
    fn the_module_summary_has_its_own_prose_block() {
        let files = vec![(
            "src/Main.aev".to_owned(),
            "## Module overview for **Main**.\nmodule Main\n\n## Entry point.\nfn main(): Int = 0\n".to_owned(),
        )];
        let built = build(&files, "demo").expect("docs");
        let page = page(&built, "Main");
        let overview = page.html.find("overview for <strong>Main</strong>");
        let entry = page.html.find("id=\"fn-main\"");
        assert!(overview.is_some() && overview < entry, "{}", page.html);
        assert!(page.html.contains("<p>Entry point.</p>"), "{}", page.html);
        assert_eq!(page.html.matches("Module overview").count(), 1);
        assert_eq!((page.decls, page.undocumented), (1, 0));
    }

    #[test]
    fn hidden_declarations_do_not_render_or_contribute_to_counts() {
        let source = concat!(
            "module Main\n\n@doc(hidden)\nfn hidden_impl(): Int = 1\n",
            "@doc(hidden = true) fn hidden_argument(): Int = 1\n",
            "## Hidden with a single marker.\n@hidden fn hidden_marker(): Int = 1\n",
            "\n## Visible private helper.\n@doc(hidden = false) private fn helper(): Int = 2\n",
        );
        let files = vec![("src/Main.aev".to_owned(), source.to_owned())];
        let built = build(&files, "demo").expect("docs");
        let page = page(&built, "Main");
        assert_eq!((page.decls, page.undocumented), (1, 0));
        assert_eq!((built.total_decls, built.undocumented), (1, 0));
        assert!(!page.html.contains("hidden_impl"), "{}", page.html);
        assert!(!page.html.contains("hidden_marker"), "{}", page.html);
        assert!(!page.html.contains("hidden_argument"), "{}", page.html);
        assert!(page.html.contains("badge private"), "{}", page.html);
        #[cfg(feature = "compiler")]
        {
            let doc = aevia_doc::module_doc(source, "src/Main.aev", "Main").expect("parsed");
            assert_eq!(render_module_html(&doc), page.html);
        }
    }

    // Skipping a broken file is the compiler path's behaviour: the degraded
    // scanner cannot tell a broken file from a good one, so these two tests
    // describe a guarantee that only exists with `aevia-doc` linked.
    #[cfg(feature = "compiler")]
    #[test]
    fn one_unparseable_file_does_not_blank_the_site() {
        let mut files = package();
        files.push(("src/Broken.aev".to_string(), "fn broken( = 1\n".to_string()));
        let docs = build(&files, "demo").expect("docs still builds");
        assert_eq!(docs.pages.len(), 3);
        assert!(docs.pages.iter().all(|page| page.path != "Broken"));
        assert_eq!(docs.total_decls, 4);
    }

    #[cfg(feature = "compiler")]
    #[test]
    fn a_package_whose_every_file_is_broken_is_an_error() {
        let files: Vec<(String, String)> = vec![
            ("README.md".to_string(), "# x\n".to_string()),
            ("src/A.aev".to_string(), "fn a( = 1\n".to_string()),
        ];
        let error = build(&files, "broken").expect_err("refused");
        assert_eq!(error.code, E_DOCS);
        assert!(
            error.message.contains("A.aev") || error.message.contains("parse"),
            "{error}"
        );
    }

    #[test]
    fn a_package_with_no_sources_is_an_error() {
        let files: Vec<(String, String)> = vec![("README.md".to_string(), "# x\n".to_string())];
        let error = build(&files, "empty").expect_err("refused");
        assert_eq!(error.code, E_DOCS);
        assert!(error.message.contains("src/"), "{}", error.message);
    }

    #[test]
    fn examples_are_not_documented() {
        let mut files = package();
        files.push((
            "examples/demo.aev".to_string(),
            "fn demo(): Int = 1\n".to_string(),
        ));
        let docs = build(&files, "demo").expect("docs");
        assert_eq!(docs.pages.len(), 3);
        assert!(docs.pages.iter().all(|page| page.path != "demo"));
    }

    #[test]
    fn the_fallback_name_is_the_file_stem() {
        let files: Vec<(String, String)> = vec![(
            "src/Thermo/IdealGas.aev".to_string(),
            "fn pressure(): Real = 1\n".to_string(),
        )];
        let docs = build(&files, "demo").expect("docs");
        assert_eq!(docs.pages[1].path, "IdealGas");
        assert_eq!(docs.pages[1].source_file, "src/Thermo/IdealGas.aev");
    }

    #[test]
    fn a_readme_in_any_case_is_found() {
        let files: Vec<(String, String)> = vec![
            ("Readme.MD".to_string(), "# Demo\n".to_string()),
            (
                "src/Main.aev".to_string(),
                "fn main(): Int = 0\n".to_string(),
            ),
        ];
        let docs = build(&files, "demo").expect("docs");
        assert_eq!(docs.pages[0].source_file, "Readme.MD");
        assert!(docs.pages[0].html.contains("<h1>Demo</h1>"));
    }

    #[test]
    fn equal_depth_readmes_and_duplicate_modules_use_archive_path_order() {
        let mut files = vec![
            ("z/README.md".to_owned(), "# Other README\n".to_owned()),
            (
                "src/Z.aev".to_owned(),
                "module Same\nlet z = 0\n".to_owned(),
            ),
            ("a/readme.MD".to_owned(), "# Selected README\n".to_owned()),
            (
                "src/A.aev".to_owned(),
                "module Same\nlet a = 0\n".to_owned(),
            ),
            ("a/0/readme.md".to_owned(), "# Deeper README\n".to_owned()),
        ];
        let docs = build(&files, "demo").expect("docs");
        files.reverse();
        assert_eq!(build(&files, "demo").expect("docs"), docs);
        assert_eq!(page(&docs, "").source_file, "a/readme.MD");
        assert_eq!(page(&docs, "Same").source_file, "src/A.aev");
        assert!(page(&docs, "Same").html.contains("id=\"let-a\""));
        assert_eq!(docs.total_decls, 1);
    }

    #[cfg(feature = "compiler")]
    #[test]
    fn the_public_module_renderer_matches_the_built_page() {
        let doc = aevia_doc::module_doc(
            "## A circle's area.\nfn circle(r: Real[L]): Real[L^2] = r^2\n",
            "src/Kinematics.aev",
            "Kinematics",
        )
        .expect("parses");
        let html = render_module_html(&doc);
        assert!(
            !html.contains("<h1>"),
            "the page chrome owns the heading: {html}"
        );
        assert!(html.contains("id=\"fn-circle\""), "{html}");
        assert!(html.contains("A circle&#39;s area.") || html.contains("A circle's area."));
    }
}
