//! Canonical owned dimension transport does not depend on a process side table.
use aevia_dim::Dim;
use aevia_dim::Scale;
use aevia_dim::ratio;
use aevia_rssn::Num;
use aevia_rssn::UnitVal;
use aevia_rssn::Val;
use aevia_rssn::registry::convert_dim;
use aevia_rssn::registry::to_dim_expr;
use rssn::kernel::codec::decode_deterministic;
use rssn::kernel::codec::encode_deterministic;
use rssn::kernel::manifest::KernelManifest;
use rssn::kernel::signature::DimExpr;
use rssn::kernel::signature::Signature;
use rssn::kernel::signature::TypeExpr;

#[test]
fn bridge_roundtrips_owned_dimensions_with_rational_exponents_and_raw_prefixes() {
    let nominal = Dim::nominal_base("package:货币/source:Api", "Credit");
    let dim = nominal.pow(&ratio(1, 2)).div(&Dim::base("Time"));
    let encoded = to_dim_expr(&dim.with_display_name("Alias.Credit"));
    let bytes = encode_deterministic(&encoded).unwrap();
    let decoded: DimExpr = decode_deterministic(&bytes).unwrap();
    assert_eq!(convert_dim(&decoded), dim);
    assert_ne!(convert_dim(&DimExpr::Base("Credit".into())), nominal);
    for raw in ["__aevia_dim_n_v1_61_62", "__aevia_dim_b_v1_61", "Credit"] {
        assert_eq!(convert_dim(&to_dim_expr(&Dim::base(raw))), Dim::base(raw));
    }
    let si = DimExpr::Base("Length".into());
    assert_eq!(to_dim_expr(&convert_dim(&si)), si);
    assert_eq!(
        encode_deterministic(&to_dim_expr(&Dim::base("Length"))).unwrap(),
        encode_deterministic(&si).unwrap()
    );
}

#[test]
fn signature_hash_and_binary_ignore_display_hints_but_include_owner() {
    fn signature(dim: &Dim) -> Signature {
        Signature::new(
            vec![TypeExpr::Float.with_dim(to_dim_expr(dim))],
            TypeExpr::Float.with_dim(to_dim_expr(dim)),
        )
    }
    let original = Dim::nominal_base("one", "Credit");
    let a = signature(&original.with_display_name("A.Credit"));
    let alias = signature(&original.with_display_name("Again.Credit"));
    let other = signature(&Dim::nominal_base("two", "Credit"));
    assert_eq!(a.hash(), alias.hash());
    assert_eq!(
        encode_deterministic(&a).unwrap(),
        encode_deterministic(&alias).unwrap()
    );
    assert_ne!(a.hash(), other.hash());
    let bytes = encode_deterministic(&a).unwrap();
    let decoded: Signature = decode_deterministic(&bytes).unwrap();
    assert_eq!(decoded, a);
}

#[test]
fn quantity_units_and_manifest_json_binary_keep_canonical_owners() {
    let dim = Dim::nominal_base("pkg/source:Api", "Credit");
    let unit = UnitVal::linear("credit", dim.with_display_name("A.Credit"), Scale::one());
    let value = Val::Quantity {
        value: Box::new(Val::Num(Num::Float(3.0))),
        unit: unit.clone(),
    };
    let session = aevia_rssn::kernel().id();
    assert_eq!(
        Val::from_rssn(&value.to_rssn(session).unwrap(), session).unwrap(),
        value
    );
    fn manifest(unit: &UnitVal) -> KernelManifest {
        let mut manifest = KernelManifest::default();
        manifest.units.push(unit.to_rssn());
        manifest.seal();
        manifest
    }
    let first = manifest(&unit);
    let mut alias = unit.clone();
    alias.dim = dim.with_display_name("Again.Credit");
    let second = manifest(&alias);
    assert_eq!(first.to_json(), second.to_json());
    assert_eq!(first.manifest_hash, second.manifest_hash);
    let bytes = first.to_bytes().unwrap();
    let decoded = KernelManifest::from_bytes(&bytes).unwrap();
    assert!(decoded.verify());
    assert_eq!(first.to_json(), decoded.to_json());
    assert_eq!(UnitVal::from_rssn(&decoded.units[0]).dim, dim);
}

#[test]
fn generated_facades_do_not_recreate_foreign_owners_from_display_names() {
    use rssn::kernel::value::ConstantValue;
    let dimension = Dim::nominal_base("foreign/source:Api", "Length");
    let mut manifest = KernelManifest::default();
    manifest
        .units
        .push(UnitVal::linear("source_credit", dimension.clone(), Scale::one()).to_rssn());
    manifest
        .units
        .push(UnitVal::linear("ordinary_metre", Dim::base("Length"), Scale::one()).to_rssn());
    for (symbol, dimension) in [
        ("foreign_constant", dimension),
        ("ordinary_constant", Dim::one()),
    ] {
        manifest.constants.push(ConstantValue {
            name: symbol.into(),
            symbol: symbol.into(),
            value: aevia_rssn::registry::to_scale_value(&Scale::one()),
            dimension: to_dim_expr(&dimension),
            unit: String::new(),
            exact: true,
            uncertainty: None,
            source: "test".into(),
        });
    }
    let registry = aevia_rssn::Registry::from_manifest(&manifest);
    for (source, missing, present) in [
        (
            aevia_rssn::codegen::units(&registry),
            "source_credit",
            "ordinary_metre",
        ),
        (
            aevia_rssn::codegen::constants(&registry),
            "foreign_constant",
            "ordinary_constant",
        ),
    ] {
        assert!(source.contains(
            "cannot be generated: source-owned dimension requires its defining source module"
        ));
        assert!(
            !source
                .lines()
                .any(|line| line.starts_with(&format!("let {missing}:"))),
            "{source}"
        );
        assert!(
            source
                .lines()
                .any(|line| line.starts_with(&format!("let {present}:"))),
            "{source}"
        );
    }
}
