//! Type classes at run time (§3.8, PLAN D3).
//!
//! Every assertion here is about *which body ran* — an instance returns a
//! value no builtin and no other instance produces — or about the error code a
//! failed dispatch raises. Nothing asserts a rendered string except where the
//! builtin `show` is the operation under test.

#![allow(clippy::result_large_err)]

use aevia_core::Interp;
use aevia_core::compile;
use aevia_core::value::Value;

fn build(source: &str) -> (aevia_syntax::Module, aevia_types::CheckedModule) {
    let compilation = compile("classes.aev", source);
    assert!(
        !compilation.diagnostics.has_errors(),
        "compile errors: {:?}",
        compilation
            .diagnostics
            .items
            .iter()
            .map(|item| (&item.code, &item.message))
            .collect::<Vec<_>>()
    );
    let module = compilation.module.expect("module");
    (module, compilation.checked)
}

fn interp(source: &str) -> Interp {
    let (module, checked) = build(source);
    let mut interp = Interp::new(&checked).with_buffer();
    let diagnostics = interp.load(&module);
    assert!(
        !diagnostics.has_errors(),
        "load errors: {:?}",
        diagnostics
            .items
            .iter()
            .map(|item| (&item.code, &item.message))
            .collect::<Vec<_>>()
    );
    interp
}

fn value(
    source: &str,
    name: &str,
) -> Value {
    let mut interp = interp(source);
    interp.eval_toplevel(name).expect("the name is bound")
}

fn text(
    source: &str,
    name: &str,
) -> String {
    match value(source, name) {
        | Value::Str(text) => text.to_string(),
        | other => panic!("expected a String, found {}", other.type_name()),
    }
}

const PRETTY: &str = "\
class Pretty a where
    pretty(x: a): String
";

#[test]
fn an_instance_body_runs_at_its_call_site() {
    let source = format!(
        "{PRETTY}
type Shape = | Dot(Float)
instance Pretty for Shape where
    pretty(v) = \"instance-body\"
let s = pretty(Dot(1.0))
"
    );
    assert_eq!(text(&source, "s"), "instance-body");
}

#[test]
fn two_instances_each_run_their_own_body() {
    let source = format!(
        "{PRETTY}
type Circle = | Circle(Float)
type Square = | Square(Float)
instance Pretty for Circle where
    pretty(v) = \"circle\"
instance Pretty for Square where
    pretty(v) = \"square\"
let c = pretty(Circle(1.0))
let q = pretty(Square(1.0))
"
    );
    assert_eq!(text(&source, "c"), "circle");
    assert_eq!(text(&source, "q"), "square");
}

#[test]
fn a_polymorphic_caller_dispatches_on_the_argument() {
    // The call site inside `describe` is checked once, against a type
    // variable, so its dictionary record cannot name an instance. The
    // argument is the only thing that is concrete at run time.
    let source = format!(
        "{PRETTY}
type Circle = | Circle(Float)
type Square = | Square(Float)
instance Pretty for Circle where
    pretty(v) = \"circle\"
instance Pretty for Square where
    pretty(v) = \"square\"
let describe = fn x => pretty(x)
let c = describe(Circle(1.0))
let q = describe(Square(1.0))
"
    );
    assert_eq!(text(&source, "c"), "circle");
    assert_eq!(text(&source, "q"), "square");
}

#[test]
fn a_record_alias_instance_runs() {
    // An alias head is transparent: `Vec2` is a record type at run time, so
    // the instance is found through the recorded head rather than a name.
    let source = format!(
        "{PRETTY}
type Vec2 = {{ x: Float, y: Float }}
instance Pretty for Vec2 where
    pretty(v) = \"alias\"
let s = pretty({{ x = 1.0, y = 2.0 }})
"
    );
    assert_eq!(text(&source, "s"), "alias");
}

#[test]
fn a_user_show_instance_does_not_replace_the_builtin() {
    // §3.8.4: `show` of a `Real` is a prelude instance, and coherence keeps
    // the user's head disjoint from it. The user instance must not capture a
    // call whose head is a prelude one.
    let source = "\
type Vec2 = { x: Float, y: Float }
instance Show for Vec2 where
    show(v) = \"mine\"
let v = show({ x = 1.0, y = 2.0 })
let builtin = show(1.5 m)
";
    assert_eq!(text(source, "v"), "mine");
    assert_eq!(text(source, "builtin"), "1.5 m");
}

#[test]
fn a_class_method_is_a_value() {
    // §3.8.1: a method is a function, so it can be bound and passed. With no
    // call-site record the instance comes from the argument.
    let source = format!(
        "{PRETTY}
type Shape = | Dot(Float)
instance Pretty for Shape where
    pretty(v) = \"through-a-value\"
let f = pretty
let s = f(Dot(1.0))
"
    );
    assert_eq!(text(&source, "s"), "through-a-value");
}

#[test]
fn an_instance_body_does_not_shadow_the_class_method() {
    // The body is bound under `Pretty@Shape::pretty`, which no program can
    // write, so the global `pretty` is still the class method: applied to a
    // head with no instance it is a missing-instance error, not the body.
    let source = format!(
        "{PRETTY}
type Shape = | Dot(Float)
instance Pretty for Shape where
    pretty(v) = \"instance-body\"
let f = pretty
"
    );
    let mut interp = interp(&source);
    let function = interp.eval_toplevel("f").expect("pretty is a value");
    assert!(
        matches!(function, Value::Native(_)),
        "the class method is not the instance body"
    );
    let failure = interp
        .apply(function, vec![Value::int(1)], aevia_syntax::Span::default())
        .expect_err("Int has no Pretty instance");
    assert_eq!(failure.code, "E0310");
}

#[test]
fn structural_equality_compares_payloads() {
    // §6.12.1 / §3.8.4: `Eq` is structural, so equality is about payloads and
    // never about the text a value prints as.
    let source = "\
type Shape = | Dot(Float)
let same_record = { x = 1.0, y = 2.0 } == { x = 1.0, y = 2.0 }
let different_record = { x = 1.0 } == { x = 2.0 }
let same_variant = Dot(1.0) == Dot(1.0)
let different_variant = Dot(1.0) == Dot(2.0)
";
    for (name, expected) in [
        ("same_record", true),
        ("different_record", false),
        ("same_variant", true),
        ("different_variant", false),
    ] {
        match value(source, name) {
            | Value::Bool(flag) => assert_eq!(flag, expected, "{name}"),
            | other => panic!("{name}: expected a Bool, found {}", other.type_name()),
        }
    }
}

#[test]
fn sort_uses_the_total_order() {
    // `sort` requires `TotalOrd` (§3.8.4). Sorting by `as_f64` left a tensor of
    // strings in source order because every element was `NaN`.
    let source = "\
let strings = sort([\"b\", \"a\", \"c\"])
let numbers = sort([3, 1, 2])
";
    let Value::Tensor(strings) = value(source, "strings") else {
        panic!("expected a tensor");
    };
    let order: Vec<String> = strings
        .elements()
        .iter()
        .map(|element| {
            match element {
                | Value::Str(text) => text.to_string(),
                | other => panic!("expected a String, found {}", other.type_name()),
            }
        })
        .collect();
    assert_eq!(order, vec!["a", "b", "c"]);

    let Value::Tensor(numbers) = value(source, "numbers") else {
        panic!("expected a tensor");
    };
    let values: Vec<String> = numbers
        .elements()
        .iter()
        .map(|element| {
            element
                .as_num()
                .and_then(|num| num.to_rational())
                .expect("a number")
                .to_string()
        })
        .collect();
    assert_eq!(values, vec!["1", "2", "3"]);
}

#[test]
fn sort_float_puts_nan_last() {
    // §3.8.4: `Float` has no `TotalOrd`, so `sort` rejects it and `sort_float`
    // is the NaN-aware order the language offers instead.
    let source = "let xs = sort_float([2.0, nan, 1.0], true)\n";
    let Value::Tensor(tensor) = value(source, "xs") else {
        panic!("expected a tensor");
    };
    let values: Vec<f64> = tensor
        .elements()
        .iter()
        .map(|element| element.as_f64().expect("a number"))
        .collect();
    assert_eq!(values.len(), 3);
    assert_eq!(values[0], 1.0);
    assert_eq!(values[1], 2.0);
    assert!(values[2].is_nan(), "NaN must sort last");
}

#[test]
fn total_compare_orders_by_value() {
    let source = "\
let less = total_compare(\"a\", \"b\")
let greater = total_compare(2, 1)
";
    for (name, expected) in [("less", 0usize), ("greater", 2)] {
        match value(source, name) {
            | Value::Variant(variant) => assert_eq!(variant.index, expected, "{name}"),
            | other => panic!("{name}: expected an Ordering, found {}", other.type_name()),
        }
    }
}

#[test]
fn hash_agrees_with_equality() {
    // A hash is only usable if `x == y` implies `hash(x) == hash(y)`.
    // `1` and `1.0` compare equal numerically, so they must hash alike.
    let source = "\
let a = hash(1)
let b = hash(1.0)
let c = hash(2)
";
    let hash_of = |name: &str| {
        match value(source, name) {
            | Value::Num(num) => num.to_rational(),
            | other => panic!("{name}: expected a number, found {}", other.type_name()),
        }
    };
    assert_eq!(hash_of("a"), hash_of("b"), "1 == 1.0, so the hashes agree");
    assert_ne!(hash_of("a"), hash_of("c"));

    // A record has no `Hash` instance in §3.8.4, so the order-insensitivity of
    // the structural hash is asserted at the operation, not through a program.
    let record = |fields: &[(&str, i64)]| {
        let mut map = indexmap::IndexMap::new();
        for (name, value) in fields {
            map.insert((*name).to_string(), Value::int(*value));
        }
        Value::Record(std::rc::Rc::new(aevia_core::value::RecordValue {
            fields: map,
        }))
    };
    let forward = record(&[("x", 1), ("y", 2)]);
    let reversed = record(&[("y", 2), ("x", 1)]);
    let other = record(&[("x", 1), ("y", 3)]);
    let hash = |value: Value| {
        match aevia_core::builtins::call_simple("hash", vec![value]) {
            | Ok(Value::Num(num)) => num.to_rational(),
            | other => panic!("hash failed: {other:?}"),
        }
    };
    assert_eq!(hash(forward.clone()), hash(reversed));
    assert_ne!(hash(forward), hash(other));
}

#[test]
fn a_function_has_no_hash_instance() {
    let source = "let f = fn x => x\n";
    let mut interp = interp(source);
    let function = interp.eval_toplevel("f").expect("f is bound");
    let failure = aevia_core::builtins::call_simple("hash", vec![function])
        .expect_err("a function has no Hash instance");
    assert_eq!(failure.code, "E0310");
}

#[test]
fn an_alias_head_is_found_among_several_instances() {
    // A single instance for a class is found by the single-instance rule, so
    // the alias case only proves anything when the class has more than one
    // instance: the head has to be *selected*, not guessed.
    let source = format!(
        "{PRETTY}
type Vec2 = {{ x: Float, y: Float }}
instance Pretty for Vec2 where
    pretty(v) = \"vec2\"
instance Pretty for Int where
    pretty(n) = \"int\"
let a = pretty({{ x = 1.0, y = 2.0 }})
let b = pretty(3)
"
    );
    assert_eq!(text(&source, "a"), "vec2");
    assert_eq!(text(&source, "b"), "int");
}

#[test]
fn an_instance_body_sees_the_head_type() {
    // The method's parameter must be the instance head, not a fresh variable:
    // `area(s)` used to be `t0`, so `match s with | Circle(r) -> …` was E0308.
    let source = "\
type Shape = | Circle(Float) | Square(Float)
class Area a where
    area(x: a): Float
instance Area for Shape where
    area(s) = match s with
        | Circle(r) -> 3.0 * r * r
        | Square(w) -> w * w
let c = area(Circle(2.0))
let q = area(Square(3.0))
";
    let circle = match value(source, "c") {
        | Value::Num(number) => number.to_f64().expect("a number"),
        | other => panic!("expected a scalar, found {}", other.type_name()),
    };
    assert!((circle - 12.0).abs() < 1e-12, "got {circle}");
    let square = match value(source, "q") {
        | Value::Num(number) => number.to_f64().expect("a number"),
        | other => panic!("expected a scalar, found {}", other.type_name()),
    };
    assert!((square - 9.0).abs() < 1e-12, "got {square}");
}

#[test]
fn runtime_dispatch_uses_record_field_types_and_tensor_extents() {
    let source = r#"class Pretty a where
    pretty(x: a): String
instance Pretty for { x: Int } where
    pretty(v) = "int"
instance Pretty for { x: String } where
    pretty(v) = "string"
instance Pretty for Tensor<[2], Float> where
    pretty(v) = "two"
instance Pretty for Tensor<[3], Float> where
    pretty(v) = "three"
let describe = fn x => pretty(x)
let a = describe({ x = 1 })
let b = describe({ x = "hello" })
let c = describe([1.0, 2.0])
let d = describe([1.0, 2.0, 3.0])
"#;
    let (module, checked) = build(source);
    for mode in [aevia_core::Mode::Ast, aevia_core::Mode::Ir] {
        let mut interp = Interp::new(&checked).with_buffer().with_mode(mode);
        let diagnostics = interp.load(&module);
        assert!(!diagnostics.has_errors(), "{mode:?}: {:?}", diagnostics.items);
        for (name, expected) in [("a", "int"), ("b", "string"), ("c", "two"), ("d", "three")] {
            match interp.eval_toplevel(name).expect("value") {
                | Value::Str(value) => assert_eq!(value.as_ref(), expected, "{mode:?}: {name}"),
                | other => panic!("{mode:?}: unexpected value {other:?}"),
            }
        }
    }
}
