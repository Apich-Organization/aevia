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
//! * It has no notion of attributes, string literals or `##` inside a string,
//!   so those appear as ordinary text.
//!
//! Because of all of that, pages built this way can differ from pages built
//! with the compiler feature on. The crate's documentation says so, and the
//! server's build log shows the feature state, so a difference is explainable
//! rather than mysterious.

use crate::Decl;
use crate::Module;

/// Scans one file for doc comments and declarations.
pub(crate) fn scan(
    source: &str,
    file: &str,
    fallback_name: &str,
) -> Module {
    let mut path = None;
    let mut decls = Vec::new();
    let mut pending: Vec<String> = Vec::new();
    for line in source.lines() {
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
            }
            pending.clear();
            continue;
        }
        if let Some(mut decl) = declaration(line) {
            if !pending.is_empty() {
                decl.doc = Some(pending.join("\n"));
            }
            decls.push(decl);
            pending.clear();
        }
    }
    Module {
        path: path.unwrap_or_else(|| fallback_name.to_string()),
        file: file.to_string(),
        decls,
    }
}

/// The module path of a `module A.B` line.
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
    })
}

/// Splits the keyword from the rest of a declaration line.
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
            | '(' | '[' | '{' => depth += 1,
            | ')' | ']' | '}' => depth -= 1,
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
    line[..cut].trim_end().to_string()
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
}
