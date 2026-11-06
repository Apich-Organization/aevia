use aevia_types::{CheckedModule, Export, ModulePath, adt, check::check_module_with_identity};

fn check(source: &str) -> CheckedModule {
    let lexed = aevia_syntax::lex(0, source).unwrap();
    let (module, diagnostics) = aevia_syntax::parse_module(0, source, &lexed);
    assert!(!diagnostics.has_errors(), "{:?}", diagnostics.items);
    let checked = check_module_with_identity(
        0,
        &module,
        &[],
        &[],
        &Default::default(),
        Some(ModulePath::new(["SchemaTest"])),
        &[],
    );
    assert!(
        !checked.diagnostics.has_errors(),
        "{:?}",
        checked.diagnostics.items
    );
    checked.module
}
fn id(
    checked: &CheckedModule,
    name: &str,
) -> adt::AdtId {
    match checked.exports.get(name).unwrap() {
        | Export::Type { adt } | Export::TypeCtor { adt, .. } => *adt,
        | other => panic!("nominal type {name}: {other:?}"),
    }
}
#[test]
fn unchanged_declarations_survive_unrelated_edits_and_alpha_renaming() {
    let first = check(
        "type Box<a> = | Box(a)\ntype Measure<d: Dim> = | Measure(Float[d])\nlet value = 1\n",
    );
    let second = check(
        "type Measure<x: Dim> = | Measure(Float[x])\ntype Box<item> = | Box(item)\nlet value = 200\n",
    );
    for name in ["Box", "Measure"] {
        let (a, b) = (id(&first, name), id(&second, name));
        assert_ne!(a, b, "exact constructor witnesses remain immutable");
        assert!(adt::same_type(a, b));
        assert_eq!(adt::key_of(a), adt::key_of(b));
        assert!(adt::key_of(a).schema.is_some());
        let mut infer = aevia_types::Infer::new();
        infer
            .unify(
                &aevia_types::Type::Adt(a, vec![]),
                &aevia_types::Type::Adt(b, vec![]),
            )
            .unwrap();
    }
}
#[test]
fn constructor_fields_order_and_dimensions_are_schema_identity() {
    let first = check("type Item = | First(Int) | Last(Float[L])\n");
    for changed in [
        "type Item = | First(String) | Last(Float[L])\n",
        "type Item = | Last(Float[L]) | First(Int)\n",
        "type Item = | First(Int) | Last(Float[T])\n",
        "type Item = | Renamed(Int) | Last(Float[L])\n",
    ] {
        let changed = check(changed);
        let (a, b) = (id(&first, "Item"), id(&changed, "Item"));
        assert!(!adt::same_type(a, b));
        assert_ne!(adt::key_of(a), adt::key_of(b));
        assert!(
            aevia_types::Infer::new()
                .unify(
                    &aevia_types::Type::Adt(a, vec![]),
                    &aevia_types::Type::Adt(b, vec![])
                )
                .is_err()
        );
    }
}
#[test]
fn recursive_and_transitive_declaration_graphs_are_stable_and_sensitive() {
    let source =
        "type Inner = | Inner(Int)\ntype Tree<a> = | Leaf(a) | Node(Inner, Tree<a>, Tree<a>)\n";
    let first = check(source);
    let same = check(source);
    let changed = check(&source.replace("Inner(Int)", "Inner(String)"));
    assert!(adt::same_type(id(&first, "Tree"), id(&same, "Tree")));
    assert!(!adt::same_type(id(&first, "Tree"), id(&changed, "Tree")));
    assert_ne!(
        adt::key_of(id(&first, "Tree")),
        adt::key_of(id(&changed, "Tree"))
    );
}
#[test]
fn public_schema_uses_expanded_private_alias_semantics() {
    let first =
        check("private type Payload = { x: Int, y: String }\ntype Item = | Item(Payload)\n");
    let same = check("private type Data = { y: String, x: Int }\ntype Item = | Item(Data)\n");
    let changed = check("private type Data = { x: String, y: String }\ntype Item = | Item(Data)\n");
    assert!(adt::same_type(id(&first, "Item"), id(&same, "Item")));
    assert!(!adt::same_type(id(&first, "Item"), id(&changed, "Item")));
}
#[test]
fn incomplete_declarations_do_not_claim_equality_and_finalized_records_do_not_mutate() {
    let module = ModulePath::new(["SchemaIncomplete"]);
    let a = adt::allocate_in(module.clone(), "Item", vec![]);
    let b = adt::allocate_in(module, "Item", vec![]);
    assert!(!adt::same_type(a, b));
    for id in [a, b] {
        adt::set_ctors(
            id,
            vec![adt::Ctor {
                name: "Item".into(),
                fields: vec![aevia_types::Type::int()],
            }],
        );
    }
    assert!(adt::same_type(a, b));
    let original = adt::key_of(a);
    adt::set_ctors(
        a,
        vec![adt::Ctor {
            name: "Changed".into(),
            fields: vec![],
        }],
    );
    assert_eq!(adt::key_of(a), original);
    assert_eq!(adt::get(a).unwrap().ctors[0].name, "Item");
}

#[test]
fn equivalent_concrete_shapes_share_schema_without_expanding_unknown_ranks() {
    fn tensor(shape: aevia_types::Shape) -> adt::AdtId {
        let id = adt::allocate_in(ModulePath::new(["SchemaShape"]), "Item", vec![]);
        adt::set_ctors(
            id,
            vec![adt::Ctor {
                name: "Item".into(),
                fields: vec![aevia_types::Type::tensor(
                    shape,
                    aevia_types::Type::float(aevia_dim::Dim::one()),
                )],
            }],
        );
        id
    }
    let arithmetic = aevia_types::Nat::lit(1).add(aevia_types::Nat::lit(1));
    let a = tensor(aevia_types::Shape::Refined(vec![Some(arithmetic)]));
    let b = tensor(aevia_types::Shape::refined([Some(2)]));
    assert!(adt::same_type(a, b));
    let a = tensor(aevia_types::Shape::Rank(2));
    let b = tensor(aevia_types::Shape::Refined(vec![None, None]));
    assert!(adt::same_type(a, b));
    // A rank carries no per-axis schema and must not allocate rank-many entries.
    let huge = tensor(aevia_types::Shape::Rank(u32::MAX));
    assert!(adt::schema_of(huge).is_some());
}
