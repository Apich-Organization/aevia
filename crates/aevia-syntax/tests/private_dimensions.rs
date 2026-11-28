//! Source dimensions follow the same visibility modifier as types and units.
use aevia_syntax::ast::DeclKind;

#[test]
fn private_base_and_alias_declarations_keep_visibility_on_the_ast() {
    let source = "private dimension Credit\nprivate dimension Balance = Credit\ndimension Public = Balance\n";
    let tokens = aevia_syntax::lex(0, source).unwrap();
    let (module, diagnostics) = aevia_syntax::parse_module(0, source, &tokens);
    assert!(!diagnostics.has_errors(), "{:?}", diagnostics.items);
    assert_eq!(
        module
            .decls
            .iter()
            .map(|decl| decl.private)
            .collect::<Vec<_>>(),
        vec![true, true, false]
    );
    assert!(matches!(
        &module.decls[0].kind,
        DeclKind::Dimension { value: None, .. }
    ));
    assert!(matches!(
        &module.decls[1].kind,
        DeclKind::Dimension { value: Some(_), .. }
    ));
}

#[test]
fn private_still_cannot_hide_imports_or_instances() {
    for source in [
        "private import Api\n",
        "private instance Show for Int where\n    show(x) = \"n\"\n",
    ] {
        let tokens = aevia_syntax::lex(0, source).unwrap();
        let (_, diagnostics) = aevia_syntax::parse_module(0, source, &tokens);
        assert!(
            diagnostics.items.iter().any(|diagnostic| {
                diagnostic.code == "E0202"
                    && diagnostic.message.contains("`private` is only allowed")
            }),
            "{:?}",
            diagnostics.items
        );
    }
}
