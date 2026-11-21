//! Every manifest operator is represented by generated Aevia source.
//!
//! Each operator has a wrapper in its kernel rule-set namespace. The domain
//! modules also include the operator or record a reason in the overlay's
//! bounded `exclude` table. The binding catalog reports invocation availability.
use aevia_rssn::codegen;
use aevia_rssn::domains;
use aevia_rssn::registry;
use std::collections::BTreeSet;

/// Every operator name written in one of the generated wrapper call forms.
fn mentioned_operators(source: &str) -> impl Iterator<Item = &str> {
    const PREFIXES: [&str; 4] = [
        "Rssn.dynamic(\"",
        "Rssn.call_ho(\"",
        "Rssn.call_key(\"",
        "Rssn.invoke(\"",
    ];
    PREFIXES.into_iter().flat_map(move |prefix| {
        source.match_indices(prefix).filter_map(move |(start, _)| {
            let rest = source.get(start.checked_add(prefix.len())?..)?;
            rest.split_once('"').map(|(name, _)| name)
        })
    })
}

#[test]
fn every_manifest_operator_has_a_wrapper_in_its_rule_set_namespace() {
    let registry = registry();
    let namespaces = codegen::namespace_files(registry);
    let mentions = namespaces
        .iter()
        .map(|(_, source)| (source, mentioned_operators(source).collect::<BTreeSet<_>>()))
        .collect::<Vec<_>>();
    let missing: Vec<String> = registry
        .ops()
        .filter(|op| {
            let header = format!("## Kernel rule set: {}.", op.rule_set);
            !mentions.iter().any(|(source, bindings)| {
                source.starts_with(&header) && bindings.contains(op.name.as_str())
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
    let mentions = files
        .iter()
        .flat_map(|(_, source)| mentioned_operators(source))
        .collect::<BTreeSet<_>>();
    let reachable: BTreeSet<&str> = registry
        .ops()
        .filter(|op| mentions.contains(op.name.as_str()))
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
    assert!(
        stale.is_empty(),
        "excluded operators that are bound anyway: {stale:?}"
    );
    assert!(
        overlay.excluded.len() <= 3,
        "{} operators are excluded from the domain modules; bridge them (dynamic wrappers need no signature)",
        overlay.excluded.len()
    );
}
