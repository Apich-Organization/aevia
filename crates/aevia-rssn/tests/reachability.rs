//! Every operator of the live Kernel Manifest is reachable from Aevia source.
//!
//! "Reachable" is checked against the generated modules a program imports, not
//! against a table: each operator must have a wrapper in
//!
//! * the namespace of its kernel rule set (`Rssn_<rule_set>`), and
//! * a domain module (`Rssn_<Domain>`, `domains.overlay`),
//!
//! and an operator that is deliberately left out of the domain modules must say
//! why in the overlay's `exclude` table, which is itself bounded below so that
//! "excluded" cannot become the place unbridged operators are parked.
//!
//! The manifest has no notion of an internal or test-only operator (every
//! registered operator is public API, `Stability` is stable, experimental or
//! deprecated), so there is nothing to skip: a new operator that is not
//! reachable fails this test until it is bridged.
use aevia_rssn::codegen;
use aevia_rssn::domains;
use aevia_rssn::registry;
use std::collections::BTreeSet;

/// The call sites an operator's wrapper contains: every generated wrapper
/// (dynamic, higher-order, key-pinned or flat-typed) reaches the kernel through
/// one of these entry points, naming the operator as its first argument.
fn call_sites(name: &str) -> [String; 4] {
    // Operator names are identifiers or symbols; none contains a quote.
    let literal = format!("\"{name}\"");
    [
        format!("Rssn.dynamic({literal}"),
        format!("Rssn.call_ho({literal}"),
        format!("Rssn.call_key({literal}"),
        format!("Rssn.invoke({literal}"),
    ]
}

fn mentions(
    source: &str,
    name: &str,
) -> bool {
    call_sites(name).iter().any(|site| source.contains(site))
}

#[test]
fn every_manifest_operator_has_a_wrapper_in_its_rule_set_namespace() {
    let registry = registry();
    let namespaces = codegen::namespace_files(registry);
    let missing: Vec<String> = registry
        .ops()
        .filter(|op| {
            let header = format!("## Kernel rule set: {}.", op.rule_set);
            !namespaces.iter().any(|(_, source)| {
                source.starts_with(&header) && mentions(source, &op.name)
            })
        })
        .map(|op| format!("{}:{}", op.rule_set, op.name))
        .collect();
    assert!(
        missing.is_empty(),
        "{} operator(s) are not reachable from their rule-set namespace: {missing:?}",
        missing.len()
    );
}

#[test]
fn every_manifest_operator_has_a_wrapper_in_a_domain_module() {
    let registry = registry();
    let overlay = domains::overlay();
    let files = domains::domain_files(registry);
    let reachable: BTreeSet<&str> = registry
        .ops()
        .filter(|op| {
            files.iter().any(|(_, source)| mentions(source, &op.name))
        })
        .map(|op| op.name.as_str())
        .collect();
    let unreachable: Vec<String> = registry
        .ops()
        .filter(|op| !reachable.contains(op.name.as_str()))
        .filter(|op| !overlay.excluded.contains_key(&op.name))
        .map(|op| format!("{}:{}", op.rule_set, op.name))
        .collect();
    assert!(
        unreachable.is_empty(),
        "{} operator(s) are in no domain module and not excluded in domains.overlay: {unreachable:?}",
        unreachable.len()
    );
    // An exclusion that is not needed any more is a stale reason.
    let stale: Vec<&String> = overlay
        .excluded
        .keys()
        .filter(|name| reachable.contains(name.as_str()))
        .collect();
    assert!(stale.is_empty(), "excluded operators that are bound anyway: {stale:?}");
    assert!(
        overlay.excluded.len() <= 3,
        "{} operators are excluded from the domain modules; bridge them (dynamic wrappers need no signature)",
        overlay.excluded.len()
    );
}
