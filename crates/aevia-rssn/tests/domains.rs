//! The coverage contract between the Kernel Manifest and `domains.overlay`.
//!
//! * every manifest operator is in a domain, and an operator of a `typed`
//!   domain is typed, accepted-dynamic or excluded (never silently untyped);
//! * every overlay entry still names a live operator with a compatible arity;
//! * the committed report `tools/rssn-api-audit/RSSN-COVERAGE.md` is exactly
//!   what the current manifest and overlay produce (`AEVIA_BLESS=1` rewrites it).

use aevia_rssn::domains;
use aevia_rssn::registry;
use std::collections::BTreeSet;
use std::path::Path;

#[test]
fn the_overlay_parses() {
    let overlay = domains::overlay();
    assert!(overlay.errors.is_empty(), "{:#?}", overlay.errors);
    assert!(overlay.domains.len() >= 20);
}

#[test]
fn every_operator_is_bound_or_explicitly_excluded() {
    let problems = domains::problems(registry(), domains::overlay());
    assert!(
        problems.is_empty(),
        "{} coverage problem(s); fix domains.overlay:\n  {}",
        problems.len(),
        problems.join("\n  ")
    );
}

#[test]
fn a_wrapper_exists_for_every_placed_operator() {
    // Rendering is total: each non-excluded operator has a `fn` in its module.
    let registry = registry();
    let overlay = domains::overlay();
    let mut missing = Vec::new();
    let mut sources = std::collections::BTreeMap::new();
    for placement in domains::classify(registry, overlay) {
        let Some(domain) = placement.domain else {
            continue;
        };
        if matches!(placement.status, domains::Status::Excluded(_)) {
            continue;
        }
        let source = sources.entry(domain.name.clone()).or_insert_with(|| {
            domains::domain_source(registry, &domains::module_name(&domain.name))
                .expect("a module for the domain")
        });
        if !source.contains(&format!("\"{}\"", placement.op.name)) {
            missing.push(format!("{}:{}", placement.op.rule_set, placement.op.name));
        }
    }
    assert!(missing.is_empty(), "no wrapper for: {missing:?}");
}

fn chapter8_names() -> BTreeSet<String> {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../spec/08-stdlib.md");
    let text = std::fs::read_to_string(path).expect("spec/08-stdlib.md");
    aevia_stdlib_model::spec::names(&text).into_iter().collect()
}

#[test]
fn the_committed_coverage_report_is_current() {
    let report = domains::coverage_report(registry(), domains::overlay(), &chapter8_names());
    let path =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tools/rssn-api-audit/RSSN-COVERAGE.md");
    if std::env::var_os("AEVIA_BLESS").is_some() {
        std::fs::write(&path, &report).expect("write the report");
        return;
    }
    let committed = std::fs::read_to_string(&path)
        .unwrap_or_default()
        .replace("\r\n", "\n");
    assert!(
        committed == report,
        "tools/rssn-api-audit/RSSN-COVERAGE.md is out of date: run \
         `AEVIA_BLESS=1 cargo test -p aevia-rssn --test domains` and review the diff"
    );
}

#[test]
fn a_domain_module_is_found_by_the_driver_hook() {
    let registry = registry();
    let source = aevia_rssn::codegen::namespace_source(registry, "Rssn_NumberTheory")
        .expect("Rssn_NumberTheory resolves");
    assert!(source.contains("module Rssn_NumberTheory"));
    assert!(source.contains("fn invmod("));
    // The old rule-set namespaces still resolve.
    assert!(aevia_rssn::codegen::namespace_source(registry, "Rssn_number_theory").is_some());
}
