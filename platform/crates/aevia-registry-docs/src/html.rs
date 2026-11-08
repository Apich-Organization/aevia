//! Module pages as HTML.
//!
//! A module page is the body of the page the site serves at
//! `/docs/<package>/<version>/<module>`: a heading, the source file it came
//! from, and one block per declaration. It is deliberately not a full document
//! — the layout, the sidebar and the version picker belong to the server, and
//! a page that carried its own `<html>` element could not be embedded in them.
//!
//! # Why anchors are derived from the declaration and then disambiguated
//!
//! `Kinematics.md#fn-circle` is a link an author may already have written by
//! hand, and the Markdown renderer emits the same slug, so the anchor has to be
//! a pure function of the kind and the name: `{kind}-{name}`, lower-cased. Two
//! declarations may still share a kind and a name (an overloaded `instance`, a
//! `let` shadowing a `fn`), and an id that repeats makes every link to it land
//! on the first one, so the second and third occurrences take `-2`, `-3` in
//! source order. The order is the file's, which does not change between builds
//! of the same source, so a link keeps working.
//!
//! # Why the signature is escaped rather than highlighted
//!
//! The signature is the author's text, not a parsed tree; escaping it keeps a
//! type such as `Map<K, V>` from being read as a tag. Highlighting happens in
//! the browser from the `language-aevia` class, so the server never needs a
//! tokeniser to render a page.

use std::collections::HashMap;

use crate::Module;
use crate::markdown::render_markdown;

/// Renders one module's declarations to HTML (the body of a module page).
///
/// This is the `compiler` path: the declarations come from `aevia-doc`, so the
/// page shows exactly what `aevia doc` would write, and a file the parser
/// rejected never reaches this function. With the feature off, [`build`] uses
/// the degraded scanner instead and there is no `ModuleDoc` to render.
///
/// [`build`]: crate::build
#[cfg(feature = "compiler")]
#[must_use]
pub fn render_module_html(doc: &aevia_doc::ModuleDoc) -> String {
    render_module(&Module::from_doc(doc))
}

/// Escapes text for element content and for a double-quoted attribute value.
///
/// One function for both because a value that is safe in one context is safe
/// in the other once quotes and apostrophes are escaped too, and having two
/// near-identical escapers is how one of them ends up missing a case.
pub(crate) fn escape(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    for character in value.chars() {
        match character {
            | '&' => out.push_str("&amp;"),
            | '<' => out.push_str("&lt;"),
            | '>' => out.push_str("&gt;"),
            | '"' => out.push_str("&quot;"),
            | '\'' => out.push_str("&#39;"),
            | other => out.push(other),
        }
    }
    out
}

/// Renders a module body.
pub(crate) fn render_module(module: &Module) -> String {
    // The page chrome already states the module path and its source file, and
    // it does so with the breadcrumb and the declaration counts beside it.
    // Repeating them here would put two `h1`s on one page.
    let mut out = String::new();
    if module.decls.is_empty() {
        out.push_str("<p class=\"empty\">This module declares nothing.</p>\n");
        return out;
    }
    let mut anchors = Anchors::default();
    for decl in &module.decls {
        let id = anchors.claim(&decl.kind, &decl.name);
        out.push_str("<div class=\"decl\" id=\"");
        out.push_str(&escape(&id));
        out.push_str("\">\n<h2><span class=\"kind\">");
        out.push_str(&escape(&decl.kind));
        out.push_str("</span> <span class=\"name\">");
        out.push_str(&escape(&decl.name));
        out.push_str("</span>");
        if decl.private {
            out.push_str(" <span class=\"badge private\">private</span>");
        }
        out.push_str("</h2>\n<pre class=\"sig\"><code class=\"language-aevia\">");
        out.push_str(&escape(&decl.signature));
        out.push_str("</code></pre>\n");
        if let Some(text) = &decl.doc {
            let body = render_markdown(text);
            // A `##` comment that holds only whitespace documents nothing; an
            // empty `<div>` would suggest it did.
            if !body.trim().is_empty() {
                out.push_str("<div class=\"doc\">\n");
                out.push_str(&body);
                out.push_str("</div>\n");
            }
        }
        out.push_str("</div>\n");
    }
    out
}

/// The set of anchor ids handed out while rendering one page.
#[derive(Default)]
struct Anchors {
    /// How many times each base id has been claimed.
    claimed: HashMap<String, usize>,
}

impl Anchors {
    /// Claims the next free id for a declaration.
    fn claim(
        &mut self,
        kind: &str,
        name: &str,
    ) -> String {
        let mut base = format!("{}-{}", slug(kind), slug(name));
        if base.trim_matches('-').is_empty() {
            base = "decl".to_string();
        }
        let count = self.claimed.entry(base.clone()).or_insert(0);
        *count += 1;
        if *count == 1 {
            base
        } else {
            format!("{base}-{count}")
        }
    }
}

/// Reduces a value to the characters an HTML id may contain.
fn slug(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    let mut after_dash = false;
    for character in value.chars() {
        let character = character.to_ascii_lowercase();
        let keep = character.is_ascii_alphanumeric() || matches!(character, '-' | '_' | '.');
        if keep {
            if character != '-' || !after_dash {
                out.push(character);
            }
            after_dash = character == '-';
        } else if !after_dash {
            out.push('-');
            after_dash = true;
        }
    }
    out.trim_matches('-').to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Decl;

    fn decl(
        kind: &str,
        name: &str,
        signature: &str,
        doc: Option<&str>,
        private: bool,
    ) -> Decl {
        Decl {
            kind: kind.to_string(),
            name: name.to_string(),
            signature: signature.to_string(),
            doc: doc.map(str::to_string),
            private,
        }
    }

    #[test]
    fn anchors_are_lower_cased_and_unique() {
        let mut anchors = Anchors::default();
        assert_eq!(anchors.claim("fn", "Circle"), "fn-circle");
        assert_eq!(anchors.claim("fn", "Circle"), "fn-circle-2");
        assert_eq!(anchors.claim("fn", "Circle"), "fn-circle-3");
        // A different kind is a different id even with the same name.
        assert_eq!(anchors.claim("let", "Circle"), "let-circle");
    }

    #[test]
    fn a_name_that_is_not_an_identifier_still_has_a_stable_id() {
        let mut anchors = Anchors::default();
        assert_eq!(anchors.claim("derive", "Eq, Ord"), "derive-eq-ord");
        assert_eq!(
            anchors.claim("instance", "Show (Vec a)"),
            "instance-show-vec-a"
        );
    }

    #[test]
    fn a_page_holds_every_part_of_a_declaration() {
        let module = Module {
            path: "Kinematics".to_string(),
            file: "src/Kinematics.aev".to_string(),
            decls: vec![
                decl(
                    "fn",
                    "circle",
                    "fn circle(r: Real[L]): Real[L^2]",
                    Some("A circle's area."),
                    false,
                ),
                decl("fn", "hidden", "fn hidden(): Int", None, true),
            ],
        };
        let html = render_module(&module);
        assert!(
            !html.contains("<h1>"),
            "the page chrome owns the heading: {html}"
        );
        assert!(
            !html.contains("Source:"),
            "the page chrome names the file: {html}"
        );
        assert!(
            html.contains("<div class=\"decl\" id=\"fn-circle\">"),
            "{html}"
        );
        assert!(html.contains("fn circle(r: Real[L]): Real[L^2]"), "{html}");
        assert!(html.contains("<p>A circle's area.</p>"), "{html}");
        // The private declaration is marked, and the undocumented one carries
        // no prose at all.
        assert!(html.contains("badge private"), "{html}");
        let hidden = html.split("id=\"fn-hidden\"").nth(1).expect("hidden block");
        assert!(!hidden.contains("class=\"doc\""), "{hidden}");
    }

    #[test]
    fn a_signature_with_angle_brackets_is_escaped() {
        let module = Module {
            path: "A".to_string(),
            file: "src/A.aev".to_string(),
            decls: vec![decl("fn", "f", "fn f(x: Map<K, V>)", None, false)],
        };
        let html = render_module(&module);
        assert!(html.contains("Map&lt;K, V&gt;"), "{html}");
    }
}
