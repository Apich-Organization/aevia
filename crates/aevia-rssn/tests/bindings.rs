//! The binding catalog is an audit of the live manifest, not a curated list.
use aevia_rssn::Source;
use aevia_rssn::bindings::Coverage;
use aevia_rssn::bindings::catalog;
use aevia_rssn::codegen;
use aevia_rssn::registry;
use std::collections::BTreeSet;

#[test]
fn catalog_covers_every_overload_and_unsigned_operator_once() {
    let registry = registry();
    let rows = catalog(registry);
    let expected: usize = registry.ops().map(|op| op.raw.len().max(1)).sum();
    assert_eq!(rows.len(), expected);
    let mut names = BTreeSet::new();
    for row in &rows {
        assert!(
            names.insert(&row.name),
            "duplicate source binding {}",
            row.name
        );
        if row.op.source == Source::Derived || row.op.source == Source::None {
            assert!(
                !matches!(row.coverage, Coverage::Typed),
                "derived signature presented as exact: {}",
                row.op.name
            );
        }
        match &row.coverage {
            | Coverage::Typed => {
                assert!(row.op.source.is_exact());
                assert!(row.overload.and_then(|i| row.op.key(i)).is_some());
            },
            | Coverage::Dynamic(reasons) | Coverage::Unavailable(reasons) => {
                assert!(!reasons.is_empty())
            },
        }
    }
    let mut typed_callbacks = 0;
    for row in &rows {
        if let Some(name) = &row.typed_callback_name {
            typed_callbacks += 1;
            assert!(names.insert(name), "typed alias collides: {name}");
            assert!(row.op.higher_order && row.op.source.is_exact());
            assert_eq!(row.op.signatures.len(), 1);
            assert!(row.overload.and_then(|index| row.op.key(index)).is_some());
        }
    }
    assert!(
        typed_callbacks > 0,
        "live callback contracts need typed aliases"
    );
    let again = catalog(registry);
    assert_eq!(
        rows.iter()
            .map(|r| {
                (
                    &r.op.name,
                    r.overload,
                    &r.name,
                    &r.coverage,
                    &r.typed_callback_name,
                )
            })
            .collect::<Vec<_>>(),
        again
            .iter()
            .map(|r| {
                (
                    &r.op.name,
                    r.overload,
                    &r.name,
                    &r.coverage,
                    &r.typed_callback_name,
                )
            })
            .collect::<Vec<_>>()
    );
}

#[test]
fn namespaces_cover_the_same_manifest_and_parse() {
    let registry = registry();
    let files = codegen::namespace_files(registry);
    let groups: BTreeSet<_> = registry.ops().map(|op| &op.rule_set).collect();
    assert_eq!(files.len(), groups.len());
    let mut names = BTreeSet::new();
    let mut invokes = 0;
    let mut dynamic_calls = 0;
    for (name, source) in &files {
        assert!(names.insert(name), "duplicate generated file {name}");
        let module_name = name.strip_suffix(".aev").expect("module extension");
        assert_eq!(
            codegen::namespace_source(registry, module_name).as_deref(),
            Some(source.as_str())
        );
        let lexed = aevia_syntax::lex(0, source)
            .unwrap_or_else(|e| panic!("{name} does not lex: {:?}", e.items));
        let (_, diagnostics) = aevia_syntax::parse_module(0, source, &lexed);
        assert!(
            !diagnostics.has_errors(),
            "{name} does not parse: {}",
            diagnostics.render(&aevia_syntax::SourceMap::new())
        );
        invokes += source
            .lines()
            .filter(|line| {
                (line.starts_with("fn ") || line.starts_with("let "))
                    && line.contains(" = Rssn.invoke(")
            })
            .count();
        dynamic_calls += source
            .lines()
            .filter(|line| {
                line.starts_with("fn ")
                    && (line.contains(" = Rssn.dynamic(") || line.contains(" = Rssn.call_ho("))
            })
            .count();
    }
    let rows = catalog(registry);
    let typed = rows
        .iter()
        .filter(|row| matches!(row.coverage, Coverage::Typed))
        .count();
    assert_eq!(invokes, typed);
    assert_eq!(dynamic_calls, rows.len() - typed);
}

#[test]
fn callback_signatures_use_an_explicit_callback_facade() {
    let source = codegen::operators(registry());
    for row in catalog(registry()).iter().filter(|row| row.op.higher_order) {
        assert!(!matches!(row.coverage, Coverage::Typed));
        if !registry().capabilities.has("core.callback") {
            assert!(
                matches!(&row.coverage, Coverage::Unavailable(reasons) if reasons.iter().any(|reason| reason.contains("core.callback")))
            );
        }
        let line = source
            .lines()
            .find(|line| line.starts_with(&format!("fn {}(", row.name)))
            .expect("binding");
        assert!(
            line.contains("callbacks: Tensor<1, Callback>") && line.contains("Rssn.call_ho("),
            "{line}"
        );
    }
}
