//! Editor edits must preserve tokens and lexical binding identity.
use aevia_analysis::Analysis;
use aevia_analysis::Document;
use aevia_analysis::FileEdit;
use aevia_analysis::RenameError;
use aevia_analysis::Severity;

fn rewritten(
    source: &str,
    edits: &[FileEdit],
) -> String {
    let document = Document::new("editor.aev", 1, source);
    let mut replacements: Vec<_> = edits
        .iter()
        .map(|edit| {
            (
                document.offset(edit.edit.range.start).unwrap() as usize,
                document.offset(edit.edit.range.end).unwrap() as usize,
                &edit.edit.new_text,
            )
        })
        .collect();
    replacements.sort_by_key(|(start, _, _)| std::cmp::Reverse(*start));
    let mut result = source.to_owned();
    for (start, end, text) in replacements {
        result.replace_range(start..end, text);
    }
    result
}

#[test]
fn attributes_and_comments_are_not_declaration_tokens() {
    for name in ["fall_time", "deprecated", "μ"] {
        let source = format!(
            "@deprecated(message=\"🙂 {name}\")\n# {name}\nfn {name}(x: Float): Float = x\nlet result = {name}(1.0)\n"
        );
        let document = Document::new("editor.aev", 1, &source);
        let analysis = Analysis::new(&document);
        assert!(
            !analysis
                .diagnostics()
                .iter()
                .any(|d| d.severity == Severity::Error)
        );
        let call = source.rfind(name).unwrap() as u32;
        let definition = analysis.definition(call).unwrap();
        let expected = source.find(&format!("fn {name}")).unwrap() + 3;
        assert_eq!(
            definition.range,
            document.range_of(expected as u32, (expected + name.len()) as u32)
        );
        let symbol = analysis
            .symbols()
            .into_iter()
            .find(|s| s.name == name)
            .unwrap();
        assert_eq!(symbol.selection, definition.range);
        let edits = analysis.rename(call, "duration").unwrap();
        assert_eq!(edits.len(), 2);
        let changed = rewritten(&source, &edits);
        assert!(changed.contains(&format!("message=\"🙂 {name}\"")));
        assert!(changed.contains(&format!("# {name}\n")));
        assert!(changed.contains("fn duration("));
        assert!(changed.contains("= duration(1.0)"));
        let changed_document = Document::new("editor.aev", 2, changed);
        assert!(
            !Analysis::new(&changed_document)
                .diagnostics()
                .iter()
                .any(|d| d.severity == Severity::Error)
        );
        assert!(
            analysis
                .prepare_rename(source.find("🙂").unwrap() as u32)
                .is_err()
        );
    }
}

#[test]
fn global_rename_cannot_be_captured_by_a_parameter_or_nested_let() {
    for body in [
        "fn f(amount: Int): Int = total + amount\n",
        "fn f(): Int = let amount = 4 in total + amount\n",
    ] {
        let source = format!("let total = 3\n{body}");
        let document = Document::new("editor.aev", 1, &source);
        let analysis = Analysis::new(&document);
        assert!(
            analysis.diagnostics().is_empty(),
            "{:?}",
            analysis.diagnostics()
        );
        assert!(matches!(
            analysis.rename(source.find("total").unwrap() as u32, "amount"),
            Err(RenameError::Collision(_))
        ));
    }
}

#[test]
fn unrelated_lexical_scopes_do_not_block_global_rename() {
    let source = "let total = 3\nfn unrelated(amount: Int): Int = amount\nfn f(): Int = total\n";
    let document = Document::new("editor.aev", 1, source);
    let analysis = Analysis::new(&document);
    let edits = analysis
        .rename(source.find("total").unwrap() as u32, "amount")
        .unwrap();
    assert_eq!(edits.len(), 2);
    let changed = Document::new("editor.aev", 2, rewritten(source, &edits));
    assert!(Analysis::new(&changed).diagnostics().is_empty());
}

#[test]
fn replacement_names_follow_the_lexer_and_preserve_identifier_kind() {
    let source = "let _value = 3\nlet result = _value\n";
    let document = Document::new("editor.aev", 1, source);
    let analysis = Analysis::new(&document);
    for name in ["_", "Value", "value ignored", "value#comment"] {
        assert!(
            matches!(analysis.rename(4, name), Err(RenameError::InvalidName(_))),
            "{name}"
        );
    }
    for name in ["value'", "value₁", "_Value"] {
        let edits = analysis.rename(4, name).unwrap();
        let changed = Document::new("editor.aev", 2, rewritten(source, &edits));
        assert!(Analysis::new(&changed).diagnostics().is_empty(), "{name}");
    }
    let source = "type Token = | End\nlet value = End\n";
    let document = Document::new("editor.aev", 1, source);
    let analysis = Analysis::new(&document);
    assert!(matches!(
        analysis.rename(source.find("End").unwrap() as u32, "_End"),
        Err(RenameError::InvalidName(_))
    ));
}
