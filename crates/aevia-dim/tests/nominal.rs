//! Source-owned generators preserve identity through aliases and owned transport.
use aevia_dim::Atom;
use aevia_dim::Dim;
use aevia_dim::DimEnv;
use aevia_dim::DimUnifier;
use aevia_dim::ratio;
use std::collections::BTreeSet;
use std::collections::HashSet;

#[test]
fn display_aliases_do_not_change_equality_order_or_hashing() {
    let original = Dim::nominal_base("package:one/source:Api", "Credit");
    let first = original.with_display_name("A.Credit");
    let second = original.with_display_name("Again.Credit");
    let other = Dim::nominal_base("package:two/source:Api", "Credit");
    assert_eq!(first, second);
    assert_ne!(first.to_plain_string(), second.to_plain_string());
    assert_eq!(HashSet::from([first.clone(), second.clone()]).len(), 1);
    let atoms: BTreeSet<_> = first
        .factors()
        .chain(second.factors())
        .map(|(atom, _)| atom.clone())
        .collect();
    assert_eq!(atoms.len(), 1);
    assert!(first.div(&second).is_one());
    assert!(!first.div(&other).is_one());
    assert_ne!(first, Dim::base("Credit"));
    let mut unifier = DimUnifier::new();
    unifier.unify(&first, &second).unwrap();
    assert!(unifier.unify(&first, &other).is_err());
}

#[test]
fn owned_transport_roundtrips_products_roots_and_variables_without_aliases() {
    let base = Dim::nominal_base("pkg/货币:1\0module", "Credit_β");
    let value = base
        .with_display_name("A.Credit")
        .pow(&ratio(1, 2))
        .div(&Dim::base("Time"))
        .mul(&Dim::var(17));
    let text = value.to_transport_string();
    assert!(!text.contains("A.Credit"));
    assert_eq!(Dim::parse_transport(&text).unwrap(), value);
    assert_eq!(
        base.to_transport_string(),
        base.with_display_name("B.Credit").to_transport_string()
    );
    let atom = base.factors().next().unwrap().0;
    assert_eq!(
        Dim::from_transport_base(&atom.transport_name().unwrap()),
        base
    );
    assert!(
        Dim::parse_plain(&text).is_err(),
        "transport does not extend module source lookup"
    );
    for raw in [
        "Credit",
        "Length",
        "__aevia_dim_n_v1_61_62",
        "__aevia_dim_b_v1_61",
        "__aevia_dim_future",
        "a/b β",
    ] {
        let base = Dim::base(raw);
        let atom = base.factors().next().unwrap().0;
        assert_eq!(
            Dim::from_transport_base(&atom.transport_name().unwrap()),
            base
        );
        assert_eq!(
            Dim::parse_transport(&base.to_transport_string()).unwrap(),
            base
        );
    }
    assert_eq!(
        Dim::base("Length")
            .div(&Dim::base("Time"))
            .to_transport_string(),
        "Length/Time"
    );
    assert!(matches!(Atom::from_transport_base("Credit"), Atom::Base(_)));
}

#[test]
fn environment_keeps_owned_base_values_and_transparent_aliases() {
    let mut env = DimEnv::si();
    assert!(env.define_nominal_base("Credit", "owner"));
    let original = env.lookup("Credit").unwrap();
    env.define_alias("Balance", original.clone());
    assert_eq!(env.lookup("Balance"), Some(original.clone()));
    assert!(env.base_names().contains(&"Credit".to_string()));
    assert!(!env.define_nominal_base("Length", "owner"));
    assert!(!env.define_nominal_base("Balance", "owner"));
    assert_eq!(
        env.parse("Credit/Time").unwrap(),
        original.div(&Dim::base("Time"))
    );
}
