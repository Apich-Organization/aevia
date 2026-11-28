//! Nullary constants retain their kernel terms and raw overload keys.

use aevia_rssn::SigType;
use aevia_rssn::Source;
use aevia_types::Export;
use aevia_types::Rep;
use aevia_types::Type;
use aevia_types::check::CheckResult;
use aevia_types::check::check_module;
use aevia_types::rssn_resolve::Resolution;
use aevia_types::rssn_resolve::Score;
use aevia_types::rssn_resolve::resolve;

fn check(source: &str) -> CheckResult {
    let tokens = aevia_syntax::lex(0, source).unwrap();
    let (module, diagnostics) = aevia_syntax::parse_module(0, source, &tokens);
    assert!(!diagnostics.has_errors(), "{:?}", diagnostics.items);
    check_module(0, &module)
}

#[test]
fn source_constants_pin_the_term_contract_without_asserting_a_dimension() {
    let checked =
        check("let circle = Rssn.pi()\nlet infinity = Rssn.oo()\nlet gamma = Rssn.euler_gamma()\n");
    assert!(
        !checked.diagnostics.has_errors(),
        "{:?}",
        checked.diagnostics.items
    );
    for export in ["circle", "infinity", "gamma"] {
        let Export::Value { scheme, .. } = &checked.module.exports[export] else {
            panic!("expected a value binding of {export}");
        };
        assert_eq!(scheme.ty, Type::Dyn(Rep::Real));
    }
    assert_eq!(checked.module.rssn_calls.len(), 3);
    for call in &checked.module.rssn_calls {
        let entry = aevia_rssn::registry().op(&call.name).unwrap();
        assert_eq!(call.overload, 1);
        assert_eq!(Some(&call.key), entry.key(1));
    }

    let wrong_promise = check("let infinity: Real = Rssn.oo()\n");
    assert!(wrong_promise.diagnostics.has_errors());
    assert!(
        wrong_promise
            .diagnostics
            .items
            .iter()
            .all(|item| item.code != "E1002")
    );
}

#[test]
fn physical_and_nonmetric_constants_preserve_their_kernel_boundary() {
    let registry = aevia_rssn::registry();
    for name in ["c_0", "hbar", "true", "false"] {
        let entry = registry.op(name).unwrap();
        assert_eq!(
            resolve(entry, &[]),
            Resolution::Ambiguous {
                candidates: vec![0, 1]
            }
        );
        assert_eq!(registry.resolve(name, &[]).unwrap_err().code, "E1002");
    }
    let checked = check("let speed = Rssn.c_0()\n");
    let diagnostic = &checked.diagnostics.items[0];
    assert_eq!(diagnostic.code, "E1002");
    assert!(
        diagnostic
            .helps
            .iter()
            .any(|help| help.contains("Rssn.dynamic"))
    );
}

#[test]
fn bridge_lookup_and_checker_agree_for_derived_and_overlay_pairs() {
    let registry = aevia_rssn::registry();
    let mut names = Vec::new();
    for entry in registry.ops().filter(|entry| {
        matches!(entry.source, Source::Derived | Source::Overlay)
            && entry.arity == (0, Some(0))
            && matches!(entry.signatures.as_slice(), [scalar, symbolic]
                if matches!(scalar.ret, SigType::Real | SigType::Bool) && symbolic.ret == SigType::Dyn)
    }) {
        if entry.derived_nullary_term_index().is_none() {
            assert_eq!(resolve(entry, &[]), Resolution::Ambiguous { candidates: vec![0, 1] });
            continue;
        }
        names.push(entry.name.as_str());
        assert!(entry.witnesses.is_empty(), "{}", entry.name);
        if entry.source == Source::Overlay {
            assert_eq!(entry.witnessed, [0, 1], "{}", entry.name);
        }
        assert_eq!(
            resolve(entry, &[]),
            Resolution::Unique {
                index: 1,
                score: Score::Exact
            },
            "{}",
            entry.name
        );
        assert_eq!(registry.resolve(&entry.name, &[]).as_ref().ok(), entry.key(1));
        assert_eq!(resolve(entry, &[SigType::Real]), Resolution::None);
    }
    for name in ["oo", "pi", "E", "euler_gamma"] {
        assert!(names.contains(&name), "{names:?}");
    }
}

#[test]
fn nullary_term_preference_preserves_the_other_ambiguities() {
    let mut entry = aevia_rssn::registry().op("oo").unwrap().clone();
    let tie = Resolution::Ambiguous {
        candidates: vec![0, 1],
    };
    let assert_ambiguous = |entry: &aevia_rssn::OpEntry| {
        assert_eq!(entry.derived_nullary_term_index(), None);
        assert_eq!(resolve(entry, &[]), tie);
        assert_eq!(entry.resolve_by_type(&[]).unwrap_err().code, "E1002");
    };

    entry.source = Source::Declared;
    assert_ambiguous(&entry);

    let overlay_indices = std::mem::take(&mut entry.witnessed);
    entry.source = Source::Overlay;
    assert_ambiguous(&entry);

    for source in [Source::Derived, Source::Overlay] {
        entry.source = source;
        if source == Source::Overlay {
            entry.witnessed.clone_from(&overlay_indices);
        }
        assert_eq!(entry.derived_nullary_term_index(), Some(1));

        entry.arity = (0, None);
        assert_ambiguous(&entry);
        entry.arity = (0, Some(0));

        entry.witnesses.push(("oo()".into(), "Term".into()));
        assert_ambiguous(&entry);
        entry.witnesses.clear();

        entry.witnessed.push(0);
        assert_ambiguous(&entry);
        let _ = entry.witnessed.pop();
    }

    entry.raw.push(entry.raw[0].clone());
    entry.signatures.push(entry.signatures[0].clone());
    assert_eq!(entry.derived_nullary_term_index(), None);
    assert_eq!(
        resolve(&entry, &[]),
        Resolution::Ambiguous {
            candidates: vec![0, 1, 2],
        }
    );
    assert_eq!(entry.resolve_by_type(&[]).unwrap_err().code, "E1002");
}

#[test]
fn dynamic_resolution_and_both_explicit_keys_keep_their_behavior() {
    let kernel = aevia_rssn::kernel();
    let registry = aevia_rssn::registry();
    for name in ["oo", "pi", "E", "euler_gamma", "c_0", "true"] {
        let entry = registry.op(name).unwrap();
        assert_eq!(entry.keys.len(), 2);
        assert_eq!(kernel.resolve(name, &[]).unwrap_err().code, "E1002");
        let dynamic = kernel.call(name, &[]).unwrap();
        for key in &entry.keys {
            assert_eq!(kernel.call_key(key, &[]).unwrap(), dynamic);
        }
    }
}
