//! A degraded documentation scanner, used only when the compiler is not linked.
//!
//! This module is the `--no-default-features` path: it exists so the platform
//! still builds and still serves *something* on a machine where the language
//! repository is not checked out. It is not a parser and does not pretend to be
//! one. It looks for `##` blocks and the declaration lines that follow them at
//! column zero, and it reports what it finds.
//!
//! # What it cannot do
//!
//! * It cannot tell whether a file parses. A file with a syntax error is
//!   scanned line by line and produces declarations anyway, so `build` can only
//!   return `E_DOCS` here when `src/` holds no `.aev` file at all — the "one
//!   broken file is skipped, not fatal" rule has nothing to act on.
//! * It only recognises declarations at column zero, which is where a
//!   top-level declaration starts. A `let` nested in a function body is
//!   indented and is therefore ignored, which is what we want, but a top-level
//!   declaration the author indented by hand is missed.
//! * It cuts a signature at the first `=` outside brackets, so a body written
//!   on the same line is dropped (correct) and a multi-line `class … where`
//!   block loses its methods (a loss, and the compiler path does not have it).
//! * It recognises visibility markers on single-line attributes, including
//!   `@hidden`, `@doc(hidden)`, and `@doc(hidden = true)`. It does not parse
//!   multi-line attributes or track multi-line strings in declaration bodies.
//!
//! Because of all of that, pages built this way can differ from pages built
//! with the compiler feature on. The crate's documentation says so, and the
//! server's build log shows the feature state, so a difference is explainable
//! rather than mysterious.

use crate::Decl;
use crate::Module;

/// Scans one file for doc comments and declarations.
#[allow(
    clippy::single_call_fn,
    reason = "the fallback scanner implements the per-file documentation interface"
)]
pub fn scan(
    source: &str,
    file: &str,
    fallback_name: &str,
) -> Module {
    let mut path = None;
    let mut doc = None;
    let mut decls = Vec::new();
    let mut pending: Vec<String> = Vec::new();
    let mut hidden = false;
    for line in source.split(['\r', '\n']) {
        if let Some(text) = line.strip_prefix("##") {
            pending.push(text.trim().to_string());
            continue;
        }
        if line.trim().is_empty() {
            // A blank line does not detach a comment: the parser attaches every
            // doc comment that ends before the declaration starts.
            continue;
        }
        if let Some(declared) = module_decl(line) {
            if path.is_none() {
                path = Some(declared);
                if decls.is_empty() && !pending.is_empty() {
                    doc = Some(pending.join("\n"));
                }
            }
            pending.clear();
            hidden = false;
            continue;
        }
        let mut statement = line;
        while let Some((hide, rest)) = attribute(statement) {
            hidden |= hide;
            statement = rest;
        }
        if let Some(mut decl) = declaration(statement) {
            if !pending.is_empty() {
                decl.doc = Some(pending.join("\n"));
            }
            decl.doc_hidden = hidden;
            decls.push(decl);
            pending.clear();
            hidden = false;
        } else {
            // The compiler consumes these declarations and their metadata,
            // even though they do not contribute to the documentation.
            let mut words = statement.split_whitespace();
            let mut keyword = words.next();
            if matches!(keyword, Some("pub" | "export")) {
                keyword = words.next();
            }
            if !statement.starts_with(char::is_whitespace)
                && matches!(keyword, Some("import" | "open" | "test"))
            {
                pending.clear();
                hidden = false;
            }
        }
    }
    Module {
        path: path.unwrap_or_else(|| fallback_name.to_string()),
        file: file.to_string(),
        doc,
        decls,
    }
}

/// Consumes one single-line attribute and recognises a documentation marker.
#[allow(
    clippy::single_call_fn,
    reason = "the fallback can skip unrelated attribute arguments without reading their expressions"
)]
fn attribute(line: &str) -> Option<(bool, &str)> {
    let rest = line.strip_prefix('@')?;
    let end = rest
        .find(|character: char| !character.is_alphanumeric() && character != '_')
        .unwrap_or(rest.len());
    let (name, tail) = rest.split_at(end);
    let tail = tail.trim_start();
    let mut hidden = name == "hidden";
    let Some(args) = tail.strip_prefix('(') else {
        return Some((hidden, tail));
    };
    let mut depth = 0usize;
    let mut start = 0;
    let mut quoted = false;
    let mut escaped = false;
    for (index, character) in args.char_indices() {
        if quoted {
            if character == '"' && !escaped {
                quoted = false;
            }
            escaped = character == '\\' && !escaped;
            continue;
        }
        match character {
            | '"' => quoted = true,
            | '(' | '[' | '{' => depth = depth.saturating_add(1),
            | ',' | ')' if depth == 0 => {
                let (prefix, suffix) = args.split_at(index);
                if name == "doc" {
                    let argument = prefix.get(start..).unwrap_or_default().trim();
                    hidden |= argument == "hidden"
                        || argument.split_once('=').is_some_and(|(key, value)| {
                            key.trim() == "hidden" && value.trim() == "true"
                        });
                }
                if let Some(rest) = suffix.strip_prefix(')') {
                    return Some((hidden, rest.trim_start()));
                }
                start = index.saturating_add(1);
            },
            | ')' | ']' | '}' => depth = depth.saturating_sub(1),
            | _ => {},
        }
    }
    None
}

/// The module path of a `module A.B` line.
#[allow(
    clippy::single_call_fn,
    reason = "the fallback scanner recognizes module headers separately from declarations"
)]
fn module_decl(line: &str) -> Option<String> {
    let rest = line.strip_prefix("module ")?;
    let name = rest.split_whitespace().next()?;
    let path: Vec<&str> = name.split('.').filter(|part| !part.is_empty()).collect();
    if path.is_empty() {
        return None;
    }
    Some(path.join("."))
}

/// One declaration, if the line starts with a declaration keyword.
#[allow(
    clippy::single_call_fn,
    reason = "fallback declaration recognition is separate from comment and module tracking"
)]
fn declaration(line: &str) -> Option<Decl> {
    // An indented line is inside a body, not at the module's surface.
    if line.starts_with(char::is_whitespace) {
        return None;
    }
    let (private, rest) = match line.strip_prefix("private ") {
        | Some(rest) => (true, rest),
        | None => (false, line),
    };
    let (kind, body) = split_keyword(rest)?;
    let name = declaration_name(kind, body);
    if name.is_empty() {
        return None;
    }
    Some(Decl {
        kind: kind.to_string(),
        name,
        signature: signature(rest),
        doc: None,
        private,
        doc_hidden: false,
    })
}

/// Splits the keyword from the rest of a declaration line.
#[allow(
    clippy::single_call_fn,
    reason = "the fallback declaration keywords have a distinct lexical table"
)]
fn split_keyword(line: &str) -> Option<(&'static str, &str)> {
    const KEYWORDS: [(&str, &str); 9] = [
        ("let rec ", "let"),
        ("fn ", "fn"),
        ("let ", "let"),
        ("type ", "type"),
        ("unit ", "unit"),
        ("dimension ", "dimension"),
        ("class ", "class"),
        ("instance ", "instance"),
        ("derive ", "derive"),
    ];
    for (prefix, kind) in KEYWORDS {
        if let Some(body) = line.strip_prefix(prefix) {
            return Some((kind, body));
        }
    }
    None
}

/// The declared name, as far as a line scanner can tell.
#[allow(
    clippy::single_call_fn,
    reason = "binding patterns and derive clauses have different name extraction rules"
)]
fn declaration_name(
    kind: &str,
    body: &str,
) -> String {
    if kind == "derive" {
        return body
            .split(" for ")
            .next()
            .unwrap_or(body)
            .split_whitespace()
            .collect::<Vec<_>>()
            .join(" ");
    }
    let name: String = body
        .chars()
        .take_while(|character| character.is_alphanumeric() || matches!(character, '_' | '\''))
        .collect();
    if name.is_empty() {
        // A destructuring `let (a, b)` has no single name; the first token at
        // least identifies the declaration for a reader.
        return body
            .split_whitespace()
            .next()
            .unwrap_or_default()
            .to_string();
    }
    name
}

/// The signature, cut where the body begins.
#[allow(
    clippy::single_call_fn,
    reason = "signature extraction tracks nesting and quoted strings independently"
)]
fn signature(line: &str) -> String {
    let mut depth = 0i32;
    let mut in_string = false;
    let mut previous = '\0';
    let mut cut = line.len();
    for (index, character) in line.char_indices() {
        if in_string {
            if character == '"' && previous != '\\' {
                in_string = false;
            }
            previous = character;
            continue;
        }
        match character {
            | '"' => in_string = true,
            | '(' | '[' | '{' => depth = depth.saturating_add(1),
            | ')' | ']' | '}' => depth = depth.saturating_sub(1),
            | '=' if depth == 0
                && !matches!(
                    previous,
                    '=' | '<' | '>' | '!' | '+' | '-' | '*' | '/' | '%' | '&' | '|'
                ) =>
            {
                cut = index;
                break;
            },
            | _ => {},
        }
        previous = character;
    }
    line.get(..cut).unwrap_or(line).trim_end().to_string()
}

#[cfg(all(test, not(feature = "compiler")))]
mod tests {
    use super::*;

    #[test]
    fn a_doc_block_and_a_signature_are_found() {
        let source = "module A\n\n## The answer.\n##\n## Half of `tau`.\nlet answer: Int = 42\n\nfn f(x: Int): Int = x\n";
        let module = scan(source, "src/A.aev", "A");
        assert_eq!(module.path, "A");
        assert_eq!(module.decls.len(), 2);
        assert_eq!(module.decls[0].name, "answer");
        assert_eq!(module.decls[0].signature, "let answer: Int");
        assert_eq!(
            module.decls[0].doc.as_deref(),
            Some("The answer.\n\nHalf of `tau`.")
        );
        assert_eq!(module.decls[1].signature, "fn f(x: Int): Int");
        assert_eq!(module.decls[1].doc, None);
    }

    #[test]
    fn an_indented_let_is_not_a_declaration() {
        let module = scan("fn f(): Int =\n    let x = 1\n    x\n", "a.aev", "A");
        assert_eq!(module.decls.len(), 1);
        assert_eq!(module.decls[0].name, "f");
    }

    #[test]
    fn a_private_declaration_is_marked() {
        let module = scan("private fn hidden(): Int = 1\n", "a.aev", "A");
        assert!(module.decls[0].private);
    }

    #[test]
    fn single_line_attributes_respect_quoted_text_and_reset_after_tests() {
        let module = scan(
            concat!(
                "module A\r",
                "@doc(section = \"text, hidden)\", hidden = true) fn first(): Int = 1\r",
                "## This comment belongs to the test.\r",
                "@hidden test \"example\" = true\r",
                "@doc(section = \"text, hidden)\", hidden = false) fn second(): Int = 2\r",
            ),
            "src/A.aev",
            "A",
        );
        assert_eq!(module.decls.len(), 2);
        assert!(module.decls[0].doc_hidden);
        assert_eq!(module.decls[1].name, "second");
        assert!(!module.decls[1].doc_hidden);
        assert_eq!(module.decls[1].doc, None);
    }
}
