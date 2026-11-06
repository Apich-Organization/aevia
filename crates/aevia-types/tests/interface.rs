//! Round-2 Phase A: dispatch and reuse correctness for the type checker.
//!
//! These tests exercise the module interface (§7.1.3/§7.4), the symbolic
//! class/instance identity and the witness table (§9.2.3), `derive` closure
//! (§3.8.5), the structural `Show` guarantee (§3.8.4) and the orphan/overlap
//! rules — the reviewed decisions 1a–1g in
//! `.workbuddy-ai/design/round2-design-register.md`.

use aevia_syntax::lex;
use aevia_syntax::parse_module;
use aevia_types::ClassKey;
use aevia_types::Export;
use aevia_types::InstanceKey;
use aevia_types::ModulePath;
use aevia_types::Type;
use aevia_types::check::CheckOptions;
use aevia_types::check::CheckedModule;
use aevia_types::check::DictTarget;
use aevia_types::check::InstanceExport;
use aevia_types::check::ModuleInterface;
use aevia_types::check::check_module;
use aevia_types::check::check_module_with_interfaces;
use aevia_types::head_key;
use indexmap::IndexMap;
use std::collections::HashMap;

fn parsed(source: &str) -> CheckedModule {
    let lexed = lex(0, source).expect("the source lexes");
    let (module, diagnostics) = parse_module(0, source, &lexed);
    assert!(
        !diagnostics.has_errors(),
        "parse errors: {:?}",
        diagnostics
            .items
            .iter()
            .map(|item| (&item.code, &item.message))
            .collect::<Vec<_>>()
    );
    let result = check_module_with_interfaces(0, &module, &[], &[], &CheckOptions::default());
    assert!(
        !result.diagnostics.has_errors(),
        "unexpected errors: {:?}",
        result
            .diagnostics
            .items
            .iter()
            .map(|item| (&item.code, &item.message))
            .collect::<Vec<_>>()
    );
    result.module
}

fn expect_error(
    source: &str,
    code: &str,
) {
    let lexed = lex(0, source).expect("lexes");
    let (module, diagnostics) = parse_module(0, source, &lexed);
    let mut codes: Vec<String> = diagnostics
        .items
        .iter()
        .map(|item| item.code.clone())
        .collect();
    if !diagnostics.has_errors() {
        let result = check_module(0, &module);
        codes.extend(
            result
                .diagnostics
                .items
                .iter()
                .map(|item| item.code.clone()),
        );
    }
    assert!(
        codes.iter().any(|found| found == code),
        "expected {code}, got {codes:?}"
    );
}

fn check_importing(
    source: &str,
    imports: Vec<(&str, CheckedModule)>,
    interfaces: Vec<CheckedModule>,
) -> aevia_types::check::CheckResult {
    let lexed = lex(0, source).expect("lexes");
    let (module, diagnostics) = parse_module(0, source, &lexed);
    assert!(!diagnostics.has_errors(), "parse: {diagnostics:?}");
    let import_table: Vec<(String, IndexMap<String, Export>)> = imports
        .into_iter()
        .map(|(prefix, checked)| (prefix.to_string(), checked.exports.clone()))
        .collect();
    let merged: Vec<ModuleInterface> = interfaces.into_iter().map(|m| m.interface()).collect();
    check_module_with_interfaces(0, &module, &import_table, &merged, &CheckOptions::default())
}

#[test]
fn the_interface_round_trips_classes_instances_and_alias_definitions() {
    let library = parsed(
        r#"module A
class Pretty a where
    pretty(x: a): String

type Vec2 = { x: Float, y: Float }
instance Pretty for Vec2 where
    pretty(v) = "vec"
let rendered = pretty({ x = 1.0, y = 2.0 })
"#,
    );
    let interface = library.interface();
    assert_eq!(interface.name, ModulePath::new(["A"]));
    // §7.1.3/§7.4: the class and its instance are part of the interface ...
    assert_eq!(interface.classes.len(), 1);
    let (class_key, class) = interface.classes.iter().next().expect("class");
    assert_eq!(class_key.module, ModulePath::new(["A"]));
    assert_eq!(class_key.name, "Pretty");
    assert_eq!(class.methods.len(), 1);
    assert_eq!(interface.instances.len(), 1);
    let instance = &interface.instances[0];
    assert_eq!(instance.key.class, *class_key);
    assert_eq!(instance.key.module, ModulePath::new(["A"]));
    assert!(!instance.derived);
    // ... and the alias carries its definition, not just its arity (§7.4).
    let alias = interface.aliases.get("Vec2").expect("alias definition");
    assert_eq!(alias.params, 0);
    assert!(matches!(alias.body, Type::Record(_)));
    // The flat table still has the alias name.
    assert!(matches!(
        library.exports.get("Vec2"),
        Some(Export::Alias { .. })
    ));
}

#[test]
fn an_importer_can_expand_an_imported_alias_and_call_its_instance() {
    // Probe P5: an instance defined in an imported module used to be invisible
    // (`E0310`); the alias was `E0301` from an importer because only its arity
    // travelled. Both work through the interface.
    let library = parsed(
        r#"module A
class Pretty a where
    pretty(x: a): String

type Vec2 = { x: Float, y: Float }
instance Pretty for Vec2 where
    pretty(v) = "vec"
"#,
    );
    let result = check_importing(
        r#"module B
fn describe(v: A.Vec2): String = pretty(v)
"#,
        vec![("A", library.clone())],
        vec![library],
    );
    assert!(
        !result.diagnostics.has_errors(),
        "importer errors: {:?}",
        result
            .diagnostics
            .items
            .iter()
            .map(|item| (&item.code, &item.message))
            .collect::<Vec<_>>()
    );
    // The call site pins the imported instance by its symbolic key (§9.2.3).
    let dictionary = result
        .module
        .dictionaries
        .iter()
        .find(|use_site| use_site.method == "pretty")
        .expect("a dictionary use");
    let DictTarget::Instance(key) = &dictionary.target else {
        panic!("expected an exact instance, got {:?}", dictionary.target);
    };
    assert_eq!(key.module, ModulePath::new(["A"]));
    assert_eq!(key.class.name, "Pretty");
    assert_eq!(
        key.head,
        head_key(&Type::record(vec![
            ("x".to_string(), Type::float(aevia_dim::Dim::one())),
            ("y".to_string(), Type::float(aevia_dim::Dim::one())),
        ]))
    );
}

#[test]
fn two_modules_with_the_same_type_name_keep_distinct_instances() {
    // Probe P6: two modules each declare `type Shape` and an instance. Their
    // nominal keys differ by module, so both instances are legal and both calls
    // pin their own body.
    let source_a = r#"module A
type Shape = | Circle(Float) | Rect(Float, Float)
instance Show for Shape where
    show(s) = "A"
"#;
    let source_b = r#"module B
type Shape = | Circle(Float) | Rect(Float, Float)
instance Show for Shape where
    show(s) = "B"
"#;
    let a = parsed(source_a);
    let b = parsed(source_b);
    assert_ne!(
        a.instances[0].key.head, b.instances[0].key.head,
        "same name, different module must be different heads"
    );
    assert_ne!(a.instances[0].key.module, b.instances[0].key.module);
    let result = check_importing(
        r#"module C
fn show_a(s: A.Shape): String = show(s)
fn show_b(s: B.Shape): String = show(s)
"#,
        vec![("A", a.clone()), ("B", b.clone())],
        vec![a, b],
    );
    assert!(
        !result.diagnostics.has_errors(),
        "errors: {:?}",
        result
            .diagnostics
            .items
            .iter()
            .map(|item| (&item.code, &item.message))
            .collect::<Vec<_>>()
    );
    let mut modules: Vec<ModulePath> = result
        .module
        .dictionaries
        .iter()
        .filter(|use_site| use_site.method == "show")
        .filter_map(|use_site| {
            match &use_site.target {
                | DictTarget::Instance(key) => Some(key.module.clone()),
                | _ => None,
            }
        })
        .collect();
    modules.sort();
    modules.dedup();
    assert_eq!(
        modules.len(),
        2,
        "each call must pin its own module: {modules:?}"
    );
}

#[test]
fn an_imported_instance_overlaps_with_e0311_naming_both_modules() {
    // A *corrupt* interface can carry an instance that is indistinguishable from
    // one already in scope; merging it must fail loudly, with both keys, rather
    // than pick one. Legal programs cannot reach this (the orphan rule keeps the
    // defining module unique), which is why the interface path is defensive.
    let module = ModulePath::new(["Evil"]);
    let class = ClassKey::prelude("Show");
    let head = Type::int();
    let evil = ModuleInterface {
        name: module.clone(),
        exports: IndexMap::new(),
        classes: IndexMap::new(),
        instances: vec![InstanceExport {
            key: InstanceKey::new(module, class, head_key(&head)),
            head,
            methods: HashMap::new(),
            derived: false,
        }],
        aliases: IndexMap::new(),
        function_headers: Default::default(),
    };
    let lexed = lex(0, "").expect("lexes");
    let (parsed_module, diagnostics) = parse_module(0, "", &lexed);
    assert!(!diagnostics.has_errors());
    let result =
        check_module_with_interfaces(0, &parsed_module, &[], &[evil], &CheckOptions::default());
    assert!(
        result
            .diagnostics
            .items
            .iter()
            .any(|item| item.code == "E0311" && item.message.contains("Evil")),
        "expected E0311 naming the imported module: {:?}",
        result
            .diagnostics
            .items
            .iter()
            .map(|item| (&item.code, &item.message))
            .collect::<Vec<_>>()
    );
}

#[test]
fn the_orphan_rule_still_rejects_a_foreign_class_and_type() {
    // §3.8.3: a user instance of a prelude class for a prelude type.
    expect_error(
        "instance Show for Int where\n    show(x) = \"n\"\n",
        "E0604",
    );
    // A local class may have an instance for a foreign prelude type.
    parsed(
        r#"class Pretty a where
    pretty(x: a): String
instance Pretty for Int where
    pretty(x) = "n"
"#,
    );
    // A `derive` for a foreign type is an orphan too (the old `head_is_local`
    // was hardcoded true, bypassing the rule).
    let library = parsed(
        r#"module A
type Foreign = | Mk(Float)
"#,
    );
    let result = check_importing(
        r#"module B
derive Eq for A.Foreign
"#,
        vec![("A", library.clone())],
        vec![library],
    );
    assert!(
        result
            .diagnostics
            .items
            .iter()
            .any(|item| item.code == "E0604"),
        "expected E0604 for a derive over a foreign type: {:?}",
        result
            .diagnostics
            .items
            .iter()
            .map(|item| (&item.code, &item.message))
            .collect::<Vec<_>>()
    );
}

#[test]
fn a_multi_parameter_class_is_rejected() {
    // §3.8.1 (round-2 decision 1f): the language has exactly one parameter, so
    // a two-parameter declaration is rejected instead of half-supported.
    expect_error("class Convert a b where\n    convert(x: a): b\n", "E0317");
    // An instance whose head is a bare variable cannot express its head either.
    expect_error(
        "class Pretty a where\n    pretty(x: a): String\ninstance Pretty for a where\n    pretty(x) = \"x\"\n",
        "E0317",
    );
    // `private instance` is not legal (§7.1.3); the parser reports the misuse.
    let lexed = lex(
        0,
        "private instance Show for Int where\n    show(x) = \"n\"\n",
    )
    .expect("lexes");
    let (_, diagnostics) = parse_module(
        0,
        "private instance Show for Int where\n    show(x) = \"n\"\n",
        &lexed,
    );
    assert!(diagnostics.has_errors());
}

#[test]
fn derive_is_closed_to_the_structural_set_and_names_the_failing_field() {
    // §3.8.5/round-2 decision 1e: only Eq/Ord/TotalOrd/Show/Hash are derivable.
    expect_error(
        "class Pretty a where\n    pretty(x: a): String\ntype V = { x: Float }\nderive Pretty for V\n",
        "E0317",
    );
    // A function field cannot support any structural instance, and the message
    // names the field/instance (the old code said only "cannot derive").
    let lexed = lex(0, "type F = | F(Int -> Int)\nderive Show for F\n").expect("lexes");
    let (module, _) = parse_module(0, "type F = | F(Int -> Int)\nderive Show for F\n", &lexed);
    let result = check_module(0, &module);
    let diagnostic = result
        .diagnostics
        .items
        .iter()
        .find(|item| item.code == "E0317")
        .expect("E0317");
    assert!(
        diagnostic.message.contains("Show") && diagnostic.message.contains("function"),
        "the diagnostic must name the failing instance: {}",
        diagnostic.message
    );
    // The derivable set still works.
    parsed("type V = { x: Int, y: Int }\nderive Eq, Show, Hash, Ord, TotalOrd for V\n");
}

#[test]
fn show_is_a_structural_guarantee_for_the_spec_heads() {
    // §3.8.4: `Show (Record)` is part of the standard, so `show(record)` is not
    // `E0310` (probe P9). The runtime's builtin printer is the fallback, and the
    // call site records that it is the builtin (round-2 decision 1e).
    let module = parsed("let s = show({ x = 1, y = 2.0 })\n");
    assert_eq!(
        module
            .dictionaries
            .iter()
            .find(|use_site| use_site.method == "show")
            .map(|use_site| use_site.target.clone()),
        Some(DictTarget::Builtin(
            aevia_types::check::BuiltinDispatch::ShowStructural
        ))
    );
    parsed("let s = show(dyn(3.0 m))\n");
    // A function has no `Show` instance and no structural fallback.
    expect_error("let s = show(fn (x) => x)\n", "E0310");
}

#[test]
fn an_exact_call_site_pins_the_instance_key() {
    // §9.2.3: a concrete head records the exact instance, not a shape guess.
    let module = parsed("let s = show(1)\n");
    let dictionary = module
        .dictionaries
        .iter()
        .find(|use_site| use_site.method == "show")
        .expect("a dictionary use");
    let DictTarget::Instance(key) = &dictionary.target else {
        panic!("expected an exact instance, got {:?}", dictionary.target);
    };
    assert_eq!(key.class.name, "Show");
    assert!(key.module.is_prelude());
    assert_eq!(key.head, head_key(&Type::int()));
}

#[test]
fn exact_tensor_instances_can_differ_by_element_representation() {
    // Exact tensors with known Int/Real provenance are distinguishable.
    parsed(
        r#"class Pretty a where
    pretty(x: a): String
instance Pretty for Tensor<1, Int> where
    pretty(t) = "int"
instance Pretty for Tensor<1, Real> where
    pretty(t) = "real"
"#,
    );
    // Different ranks are distinguishable, so both are legal.
    parsed(
        r#"class Pretty a where
    pretty(x: a): String
instance Pretty for Tensor<1, Int> where
    pretty(t) = "vector"
instance Pretty for Tensor<2, Int> where
    pretty(t) = "matrix"
"#,
    );
}

#[test]
fn a_private_class_is_local_but_not_exported() {
    let library = parsed(
        r#"module A
private class Hidden a where
    reveal(x: a): String
type V = { x: Float }
instance Hidden for V where
    reveal(v) = "v"
let local = reveal({ x = 1.0 })
"#,
    );
    // §7.1.3: `private class` keeps the class out of the interface.
    assert!(
        library.classes.is_empty(),
        "a private class must not be exported"
    );
    assert!(
        library.instances.is_empty(),
        "an instance of a private class is not exported either"
    );
    // The private class is still usable inside its module.
    assert!(library.exports.contains_key("local"));
}

#[test]
fn primitive_head_instances_get_distinct_bodies() {
    // The dominant silent bug (round-2 decision 1a): two primitive-head
    // instances used to be registered under one `Class@?` global, so every call
    // ran the last-declared body. The symbolic key makes them distinct, and the
    // call site pins the right one.
    let module = parsed(
        r#"class Pretty a where
    pretty(x: a): String
instance Pretty for Int where
    pretty(x) = "int"
instance Pretty for String where
    pretty(x) = "string"
let a = pretty(1)
let b = pretty("s")
"#,
    );
    assert_eq!(module.instances.len(), 2);
    assert_ne!(module.instances[0].key, module.instances[1].key);
    assert_ne!(
        module.instances[0].key.mangle(),
        module.instances[1].key.mangle()
    );
    // Each call site pins its own instance key.
    let mut heads: Vec<_> = module
        .dictionaries
        .iter()
        .filter(|use_site| use_site.method == "pretty")
        .filter_map(|use_site| {
            match &use_site.target {
                | DictTarget::Instance(key) => Some(key.head.clone()),
                | _ => None,
            }
        })
        .collect();
    heads.sort();
    heads.dedup();
    assert_eq!(heads.len(), 2, "distinct primitive heads: {heads:?}");
}

// --- §7.2.1/§7.2.2: imported types and constructors (task-22) --------------

#[test]
fn a_selective_import_brings_the_type_and_its_constructors() {
    // `import A { Pair, Color, Red }`: `Pair` is an alias (its body comes from
    // the interface), `Color` an ADT and `Red` one of its constructors.
    let library = parsed("module A\ntype Pair = { x: Int, y: Int }\ntype Color = | Red | Blue\n");
    let result = check_importing(
        "module B\nimport A { Pair, Color, Red }\nlet p: Pair = { x = 1, y = 2 }\nlet c: Color = Red\nfn pick(c: Color): Int = match c with\n    | Red -> 1\n    | Blue -> 2\n",
        vec![("", library.clone())],
        vec![library],
    );
    assert!(
        !result.diagnostics.has_errors(),
        "{:?}",
        result
            .diagnostics
            .items
            .iter()
            .map(|item| (&item.code, &item.message))
            .collect::<Vec<_>>()
    );
}

#[test]
fn an_open_import_brings_every_export_unqualified() {
    let library = parsed("module A\ntype Pair = { x: Int, y: Int }\ntype Color = | Red | Blue\n");
    let result = check_importing(
        "module B\nopen A\nlet p: Pair = { x = 1, y = 2 }\nlet c: Color = Blue\n",
        vec![("", library.clone())],
        vec![library],
    );
    assert!(!result.diagnostics.has_errors(), "{:?}", result.diagnostics);
}

#[test]
fn a_plain_import_qualifies_types_and_constructors() {
    let library = parsed("module A\ntype Pair = { x: Int, y: Int }\ntype Color = | Red | Blue\n");
    let result = check_importing(
        "module B\nimport A\nlet p: A.Pair = { x = 1, y = 2 }\nlet c: A.Color = A.Red\n",
        vec![("A", library.clone())],
        vec![library],
    );
    assert!(
        !result.diagnostics.has_errors(),
        "{:?}",
        result
            .diagnostics
            .items
            .iter()
            .map(|item| (&item.code, &item.message))
            .collect::<Vec<_>>()
    );
}

#[test]
fn same_named_types_from_two_modules_stay_distinct() {
    let first = parsed("module A\ntype T = { a: Int }\n");
    let second = parsed("module B\ntype T = { b: String }\n");
    let result = check_importing(
        "module C\nimport A\nimport B\nlet x: A.T = { a = 1 }\nlet y: B.T = { b = \"s\" }\n",
        vec![("A", first.clone()), ("B", second.clone())],
        vec![first, second],
    );
    assert!(
        !result.diagnostics.has_errors(),
        "{:?}",
        result
            .diagnostics
            .items
            .iter()
            .map(|item| (&item.code, &item.message))
            .collect::<Vec<_>>()
    );
}

#[test]
fn the_sde_module_shape_resolves_types_and_constructors() {
    // The §8.9.1 surface: a method ADT, a structure ADT and a
    // dimension-kinded option ADT with constructors.
    let library = parsed(
        r#"module Sde
type SdeMethod = | EulerMaruyama | Milstein
type SdeStructure = | Diagonal | General(Int)
type SdeFixedOpts<T> = { structure: SdeStructure, method: SdeMethod, seed: Int, t0: Float[T], dt: Float[T], steps: Int, terms: Int }
type SdeOpts<T> = | Fixed(SdeFixedOpts<T>) | Adaptive(Int)
let sdeint_diag = sdeint_diag
"#,
    );
    let selected = check_importing(
        "module Use\nimport Sde { SdeMethod, SdeStructure, SdeOpts, Fixed, EulerMaruyama, Diagonal }\nlet opts = Fixed({ structure = Diagonal, method = EulerMaruyama, seed = 7, t0 = 0.0 s, dt = 1.0 s, steps = 4, terms = 0 })\nlet m: SdeMethod = EulerMaruyama\nfn which(o: SdeOpts[T]): Int = match o with\n    | Fixed(_) -> 1\n    | Adaptive(n) -> n\n",
        vec![("", library.clone())],
        vec![library.clone()],
    );
    assert!(
        !selected.diagnostics.has_errors(),
        "{:?}",
        selected
            .diagnostics
            .items
            .iter()
            .map(|item| (&item.code, &item.message))
            .collect::<Vec<_>>()
    );
    // The qualified spelling is equivalent (`Sde.Fixed(...)`, `Sde.Milstein`).
    let qualified = check_importing(
        "module Use2\nimport Sde\nlet opts = Sde.Fixed({ structure = Sde.Diagonal, method = Sde.Milstein, seed = 7, t0 = 0.0 s, dt = 1.0 s, steps = 4, terms = 0 })\nlet m: Sde.SdeMethod = Sde.Milstein\n",
        vec![("Sde", library.clone())],
        vec![library],
    );
    assert!(
        !qualified.diagnostics.has_errors(),
        "{:?}",
        qualified
            .diagnostics
            .items
            .iter()
            .map(|item| (&item.code, &item.message))
            .collect::<Vec<_>>()
    );
}

#[test]
fn repeated_instance_head_parameters_refer_to_one_type() {
    let module = parsed(
        r#"class Same a where
    same(x: a): String
instance Same for { first: t, second: t } where
    same(x) = "same"
"#,
    );
    let head = &module.instances[0].head;
    let pair =
        |first, second| Type::record(vec![("first".into(), first), ("second".into(), second)]);
    assert!(aevia_types::class::matches_head(
        head,
        &pair(Type::int(), Type::int())
    ));
    assert!(!aevia_types::class::matches_head(
        head,
        &pair(Type::int(), Type::Str)
    ));
}

#[test]
fn repeated_instance_head_parameters_reject_different_field_types() {
    expect_error(
        r#"class Same a where
    same(x: a): String
instance Same for { first: t, second: t } where
    same(x) = "same"
let value = same({ first = 1, second = "text" })
"#,
        "E0310",
    );
}

#[test]
fn disjoint_record_fields_and_tensor_extents_have_distinct_instance_keys() {
    let module = parsed(
        r#"class Classify a where
    classify(x: a): String
instance Classify for { x: Int } where
    classify(x) = "int"
instance Classify for { x: String } where
    classify(x) = "string"
instance Classify for Tensor<[2], Float> where
    classify(x) = "two"
instance Classify for Tensor<[3], Float> where
    classify(x) = "three"
"#,
    );
    let keys: std::collections::HashSet<_> = module.instances.iter().map(|i| &i.key).collect();
    assert_eq!(keys.len(), 4);
    let fixed = &module.instances[2].head;
    let erased = Type::tensor(
        aevia_types::ty::Shape::Rank(1),
        Type::float(aevia_dim::Dim::one()),
    );
    assert!(!aevia_types::class::matches_head(fixed, &erased));
}

#[test]
fn generic_adt_arguments_make_distinct_instance_keys() {
    let module = parsed(
        r#"class Describe a where
    describe(x: a): String
instance Describe for Option<Int> where
    describe(x) = "int"
instance Describe for Option<String> where
    describe(x) = "string"
"#,
    );
    assert_eq!(module.instances.len(), 2);
    assert_ne!(module.instances[0].key, module.instances[1].key);
}
