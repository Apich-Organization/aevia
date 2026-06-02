//! Doc comment extraction (`//!`, `///`) before parsing. Phase 6.

/// Prepared source: code without doc/comment lines plus extracted docs.
#[derive(Debug, Clone)]
pub struct PreparedSource {
    pub code: String,
    pub module_doc: Option<String>,
    /// One entry per top-level item, in source order.
    pub item_docs: Vec<Option<String>>,
}

/// Strip `//` comments but preserve `///` and `//!` as structured documentation.
pub fn prepare_source(source: &str) -> PreparedSource {
    let mut module_lines: Vec<String> = Vec::new();
    let mut item_docs: Vec<Option<String>> = Vec::new();
    let mut pending_doc: Vec<String> = Vec::new();
    let mut code = String::new();

    for line in source.lines() {
        let trimmed = line.trim_start();
        if let Some(text) = trimmed.strip_prefix("//!") {
            module_lines.push(text.trim().to_string());
            continue;
        }
        if let Some(text) = trimmed.strip_prefix("///") {
            pending_doc.push(text.trim().to_string());
            continue;
        }
        if trimmed.starts_with("//") {
            continue;
        }
        if trimmed.is_empty() {
            code.push('\n');
            continue;
        }
        if is_item_start(trimmed) {
            item_docs.push(flush_doc(&mut pending_doc));
        }
        code.push_str(line);
        code.push('\n');
    }

    PreparedSource {
        code,
        module_doc: join_lines(module_lines),
        item_docs,
    }
}

fn flush_doc(pending: &mut Vec<String>) -> Option<String> {
    if pending.is_empty() {
        None
    } else {
        let doc = pending.join("\n");
        pending.clear();
        Some(doc)
    }
}

fn join_lines(lines: Vec<String>) -> Option<String> {
    if lines.is_empty() {
        None
    } else {
        Some(lines.join("\n"))
    }
}

fn is_item_start(line: &str) -> bool {
    const KW: &[&str] = &[
        "fn ", "type ", "struct ", "use ", "mod ", "op ", "#[", "pub ", "pub(",
    ];
    KW.iter().any(|kw| line.starts_with(kw))
}

/// Attach extracted docs to parsed items (by index).
pub fn attach_item_docs(
    file: &mut crate::ast::SourceFile,
    docs: Vec<Option<String>>,
) {
    for (item, doc) in file.items.iter_mut().zip(docs) {
        item.node.set_doc(doc);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extracts_module_and_item_docs() {
        let src = r#"//! Module docs

/// Energy function
pub fn energy(m: kg, v: m/s) -> J := m * v;
"#;
        let prep = prepare_source(src);
        assert_eq!(prep.module_doc.as_deref(), Some("Module docs"));
        assert_eq!(prep.item_docs.len(), 1);
        assert!(prep.item_docs[0].as_ref().unwrap().contains("Energy"));
        assert!(prep.code.contains("pub fn energy"));
        assert!(!prep.code.contains("///"));
    }
}
