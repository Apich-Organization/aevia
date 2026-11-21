//! A nullary derived constant keeps its kernel term and raw overload keys.

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
fn bridge_lookup_and_checker_use_the_same_key_for_every_derived_pair() {
    let registry = aevia_rssn::registry();
    let mut names = Vec::new();
    for entry in registry.ops().filter(|entry| {
        entry.source == Source::Derived
            && entry.arity == (0, Some(0))
            && matches!(entry.signatures.as_slice(), [scalar, symbolic]
                if matches!(scalar.ret, SigType::Real | SigType::Bool) && symbolic.ret == SigType::Dyn)
    }) {
        if entry.derived_nullary_term_index().is_none() {
            assert_eq!(resolve(entry, &[]), Resolution::Ambiguous { candidates: vec![0, 1] });
            continue;
        }
        names.push(entry.name.as_str());
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
fn explicit_and_variadic_ties_are_still_reported() {
    let mut entry = aevia_rssn::registry().op("oo").unwrap().clone();
    entry.source = Source::Declared;
    let tie = Resolution::Ambiguous {
        candidates: vec![0, 1],
    };
    assert_eq!(resolve(&entry, &[]), tie);
    entry.source = Source::Derived;
    entry.arity = (0, None);
    assert_eq!(resolve(&entry, &[]), tie);
    entry.arity = (0, Some(0));
    entry.signatures.push(entry.signatures[0].clone());
    assert_eq!(
        resolve(&entry, &[]),
        Resolution::Ambiguous {
            candidates: vec![0, 1, 2],
        }
    );
}

#[test]
fn dynamic_resolution_and_both_explicit_keys_keep_their_behavior() {
    let kernel = aevia_rssn::kernel();
    let registry = aevia_rssn::registry();
    for name in ["oo", "pi", "c_0", "true"] {
        let entry = registry.op(name).unwrap();
        assert_eq!(entry.keys.len(), 2);
        assert_eq!(kernel.resolve(name, &[]).unwrap_err().code, "E1002");
        let dynamic = kernel.call(name, &[]).unwrap();
        for key in &entry.keys {
            assert_eq!(kernel.call_key(key, &[]).unwrap(), dynamic);
        }
    }
}
