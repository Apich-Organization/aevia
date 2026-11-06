//! The declaration owner survives interface lookup names and generic binders.
use aevia_dim::Dim;
use aevia_types::CheckResult;
use aevia_types::CheckedModule;
use aevia_types::Export;
use aevia_types::ModulePath;
use aevia_types::Type;
use aevia_types::check::InterfaceBinding;
use aevia_types::check::check_module_with_identity;
use aevia_types::keys::ModuleOrigin;

fn identity(owner: &str) -> ModulePath {
    ModulePath::new(["Api"]).with_origin(ModuleOrigin {
        package: owner.to_string(),
        source: "src/Api.aev".to_string(),
    })
}
fn check(
    source: &str,
    owner: ModulePath,
    imports: &[(&str, &CheckedModule)],
) -> CheckResult {
    let tokens = aevia_syntax::lex(0, source).unwrap();
    let (module, diagnostics) = aevia_syntax::parse_module(0, source, &tokens);
    assert!(!diagnostics.has_errors(), "{:?}", diagnostics.items);
    let interfaces: Vec<_> = imports
        .iter()
        .map(|(_, checked)| checked.interface())
        .collect();
    let names: Vec<_> = imports
        .iter()
        .map(|(prefix, checked)| (prefix.to_string(), checked.exports.clone()))
        .collect();
    let bindings: Vec<_> = imports
        .iter()
        .map(|(prefix, checked)| {
            InterfaceBinding {
                module: checked.module_path(),
                prefix: prefix.to_string(),
                names: None,
            }
        })
        .collect();
    check_module_with_identity(
        0,
        &module,
        &names,
        &interfaces,
        &Default::default(),
        Some(owner),
        &bindings,
    )
}
fn good(result: CheckResult) -> CheckedModule {
    assert!(
        !result.diagnostics.has_errors(),
        "{:?}",
        result.diagnostics.items
    );
    result.module
}
fn dimension(
    checked: &CheckedModule,
    name: &str,
) -> Dim {
    let Export::Dimension { dim, .. } = checked.exports.get(name).unwrap() else {
        panic!("dimension")
    };
    dim.clone()
}
const API: &str = "module Api\ndimension Credit\nunit credit: Credit = 1\nfn amount(): Float[Credit] = 2.0 credit\nfn identity<d: Dim>(x: Float[d]): Float[d] = x\ntype Quantity<d: Dim> = | Quantity(Float[d])\n";

#[test]
fn aliases_preserve_owner_through_generic_functions_and_dimension_kinded_adts() {
    let library = good(check(API, identity("one"), &[]));
    let result = good(check(
        "let x: Float[A.Credit] = Again.amount()\nlet y: Float[Again.Credit] = A.identity<A.Credit>(x)\nlet z: A.Quantity<Again.Credit> = A.Quantity(y)\n",
        identity("app"),
        &[("A", &library), ("Again", &library)],
    ));
    assert_eq!(
        result.dims.lookup("A.Credit"),
        result.dims.lookup("Again.Credit")
    );
    assert_eq!(
        result.dims.lookup("A.Credit").unwrap().to_plain_string(),
        "A.Credit"
    );
    assert_eq!(
        result
            .dims
            .lookup("Again.Credit")
            .unwrap()
            .to_plain_string(),
        "Again.Credit"
    );
    assert_eq!(
        dimension(&library, "Credit"),
        dimension(&good(check(API, identity("one"), &[])), "Credit")
    );
}

#[test]
fn same_named_declarations_in_different_package_owners_are_distinct() {
    let first = good(check(API, identity("one"), &[]));
    let second = good(check(API, identity("two"), &[]));
    assert_ne!(dimension(&first, "Credit"), dimension(&second, "Credit"));
    assert_ne!(dimension(&first, "Credit"), Dim::base("Credit"));
    let result = check(
        "let invalid: Float[A.Credit] = B.amount()\n",
        identity("app"),
        &[("A", &first), ("B", &second)],
    );
    assert!(result.diagnostics.has_errors());
    assert!(
        aevia_types::Infer::new()
            .unify(
                &Type::float(dimension(&first, "Credit")),
                &Type::float(dimension(&second, "Credit"))
            )
            .is_err()
    );
}

#[test]
fn private_dimension_helpers_are_resolved_in_public_aliases_without_exporting_names() {
    let api = good(check(
        "private dimension Secret\ndimension Credit = Secret\nprivate dimension Hidden = Secret^2\ntype Amount = Float[Credit]\ntype Area = Float[Hidden]\nunit credit: Credit = 1\nfn amount(): Amount = 2.0 credit\n",
        identity("private"),
        &[],
    ));
    assert!(!api.exports.contains_key("Secret"));
    assert!(!api.exports.contains_key("Hidden"));
    good(check(
        "let x: A.Amount = A.amount()\nfn square(x: A.Amount): A.Area = x * x\nlet y: Float[A.Credit] = A.amount()\n",
        identity("app"),
        &[("A", &api)],
    ));
    for name in ["Secret", "Hidden"] {
        let invalid = check(
            &format!("fn invalid(x: Float[A.{name}]): Int = 0\n"),
            identity("app"),
            &[("A", &api)],
        );
        assert!(invalid.diagnostics.has_errors(), "private name {name}");
    }
}

#[test]
fn module_owner_framing_and_declaration_schema_ignore_display_hints() {
    assert_ne!(
        ModulePath::new(["a", "b"]).identity_key(),
        ModulePath::new(["a.b"]).identity_key()
    );
    let api = good(check(API, identity("schema"), &[]));
    let dimension = dimension(&api, "Credit");
    fn record(dim: Dim) -> aevia_types::AdtKey {
        let adt = aevia_types::adt::allocate_in(ModulePath::new(["Record"]), "Amount", vec![]);
        aevia_types::adt::set_ctors(
            adt,
            vec![aevia_types::adt::Ctor {
                name: "Amount".into(),
                fields: vec![Type::float(dim)],
            }],
        );
        aevia_types::adt::key_of(adt)
    }
    assert_eq!(
        record(dimension.with_display_name("A.Credit")),
        record(dimension.with_display_name("Again.Credit"))
    );
    assert_ne!(
        record(dimension),
        record(Dim::nominal_base("different-owner", "Credit"))
    );
}
