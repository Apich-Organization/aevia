//! Stable callable keys distinguish hidden creation contracts across revisions.
use aevia_types::CallableId;
use aevia_types::ModulePath;
use aevia_types::check::check_module_with_identity;
fn identity(
    file: u32,
    source: &str,
) -> CallableId {
    let tokens = aevia_syntax::lex(file, source).unwrap();
    let (module, diagnostics) = aevia_syntax::parse_module(file, source, &tokens);
    assert!(!diagnostics.has_errors());
    let checked = check_module_with_identity(
        file,
        &module,
        &[],
        &[],
        &Default::default(),
        Some(ModulePath::new(["StableCallable"])),
        &[],
    );
    assert!(
        !checked.diagnostics.has_errors(),
        "{:?}",
        checked.diagnostics.items
    );
    let declaration = module.decls.iter().find(|decl| matches!(&decl.kind, aevia_syntax::DeclKind::Fn { name, .. } if name == "choose")).unwrap();
    checked.module.functions[&declaration.span].identity.clone()
}
#[test]
fn file_ids_and_unrelated_generic_allocations_do_not_change_a_contract() {
    let source = "fn choose<a, b>(x: a) = type_rep<b>()\n";
    let first = identity(0, source);
    assert_eq!(first, identity(91, source));
    assert_eq!(
        first,
        identity(
            91,
            &format!("{source}fn after<p, q, r>(x: p, y: q) = type_rep<r>()\n")
        )
    );
}
#[test]
fn an_expanded_phantom_alias_is_part_of_the_checked_contract() {
    let source = "type Helper<a, b> = a\nfn choose<a, b>(x: Int) = type_rep<Helper<a, b>>()\n";
    assert_ne!(
        identity(0, source),
        identity(0, &source.replace("= a\n", "= b\n"))
    );
}
