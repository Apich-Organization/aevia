//! Owned dimension identity across real imports, closures and opaque metadata.
use aevia_core::Interp;
use aevia_core::Mode;
use aevia_core::builtins::call_simple;
use aevia_core::driver;
use aevia_core::manifest::Package;
use aevia_core::value::NumVal;
use aevia_core::value::Value;
use aevia_dim::Dim;
use std::path::Path;
use std::path::PathBuf;

struct Scratch(PathBuf);
impl Scratch {
    fn new(name: &str) -> Self {
        let path =
            std::env::temp_dir().join(format!("aevia-nominal-dim-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&path);
        std::fs::create_dir_all(&path).unwrap();
        Self(path)
    }

    fn write(
        &self,
        name: &str,
        source: &str,
    ) -> PathBuf {
        let path = self.0.join(name);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, source).unwrap();
        path
    }
}
impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}
fn run(
    program: &driver::Program,
    expected: &str,
) {
    assert!(
        !program.diagnostics.has_errors(),
        "{:?}",
        program.diagnostics.items
    );
    for mode in [Mode::Ast, Mode::Ir] {
        let mut interp = Interp::new(&program.entry().unwrap().checked)
            .with_buffer()
            .with_mode(mode);
        let diagnostics = driver::run_modules(&mut interp, program);
        assert!(
            !diagnostics.has_errors(),
            "{mode:?}: {:?}",
            diagnostics.items
        );
        interp
            .run_main()
            .unwrap_or_else(|error| panic!("{mode:?}: {error:?}"));
        assert_eq!(interp.captured(), expected, "{mode:?}");
    }
}
const API: &str = "module Api\ndimension Credit\nunit credit: Credit = 1\nfn amount(): Float[Credit] = 2.0 credit\nfn identity<d: Dim>(x: Float[d]): Float[d] = x\nfn retain<d: Dim>(x: Float[d]) = fn () => x\nfn reflect<d: Dim>(x: Float[d]) = type_rep<Float[d]>()\ntype Quantity<d: Dim> = | Quantity(Float[d])\n";

#[test]
fn alias_imports_keep_dimensions_through_generic_aliases_captured_closures_and_type_reps() {
    let scratch = Scratch::new("aliases");
    scratch.write("Api.aev", API);
    let entry = scratch.write("main.aev", "import Api as A\nimport Api as Again\nlet identity = A.identity\nlet x: Float[A.Credit] = identity<Again.Credit>(Again.amount())\nlet retained = A.retain<A.Credit>(x)\nlet reflected = A.reflect<Again.Credit>(x)\nlet item: A.Quantity<Again.Credit> = Again.Quantity(retained())\nlet boxed = to_value(retained())\nlet restored: Result<Float[Again.Credit], OpError> = from_value<Float[A.Credit]>(boxed)\nfn main(): IO Unit =\n    let matched = match restored with\n        | Ok(value) -> value == A.amount()\n        | Err(_) -> false\n    println(matched)\n");
    let program = driver::load(&entry);
    run(&program, "true\n");
    for mode in [Mode::Ast, Mode::Ir] {
        let checked = &program.entry().unwrap().checked;
        let mut interp = Interp::new(checked).with_mode(mode);
        assert!(!driver::run_modules(&mut interp, &program).has_errors());
        let Value::TypeRep(ty) = interp.eval_toplevel("reflected").unwrap() else {
            panic!("TypeRep")
        };
        assert_eq!(
            *ty,
            aevia_types::Type::float(checked.dims.lookup("A.Credit").unwrap()),
            "{mode:?}"
        );
    }
    assert_eq!(
        program.modules.len(),
        2,
        "two aliases share one checked source"
    );
}

fn package(
    scratch: &Scratch,
    directory: &str,
    dependencies: &str,
) {
    scratch.write(&format!("{directory}/aevia.toml"), &format!("[package]\nname = \"library\"\nversion = \"0.1.0\"\nentry = \"src/Api.aev\"\n[dependencies]\n{dependencies}"));
}
fn load(entry: &Path) -> driver::Program {
    let package = Package::owning(entry).unwrap().unwrap();
    driver::load_with_roots(entry, package.roots().unwrap())
}
#[test]
fn package_owners_separate_same_named_dimensions_while_dependency_aliases_merge() {
    let scratch = Scratch::new("owners");
    for directory in ["one", "two"] {
        package(&scratch, directory, "");
        scratch.write(&format!("{directory}/src/Api.aev"), API);
    }
    package(
        &scratch,
        "app",
        "A = { path = \"../one\" }\nAgain = { path = \"../one\" }\nB = { path = \"../two\" }\n",
    );
    let entry = scratch.write("app/src/Api.aev", "import A\nimport Again\nimport B\nlet same: Float[A.Credit] = Again.amount()\nlet wrong_owner: Result<Float[B.Credit], OpError> = from_value<Float[B.Credit]>(to_value(same))\nfn main(): IO Unit =\n    let rejected = match wrong_owner with\n        | Err(_) -> true\n        | Ok(_) -> false\n    println(rejected)\n");
    run(&load(&entry), "true\n");
    scratch.write(
        "app/src/Api.aev",
        "import A\nimport B\nlet invalid: Float[A.Credit] = B.amount()\n",
    );
    assert!(load(&entry).diagnostics.has_errors());
}

#[test]
fn dynamic_dimension_metadata_roundtrips_identity_and_hash_ignores_alias_hints() {
    fn call(
        name: &str,
        args: Vec<Value>,
    ) -> Value {
        call_simple(name, args).unwrap()
    }
    let original = Dim::nominal_base("one/source:Api", "Credit");
    let dynamic = call(
        "dyn",
        vec![Value::Num(NumVal::float(
            3.0,
            original.with_display_name("A.Credit"),
        ))],
    );
    let metadata = call("dim_of", vec![dynamic.clone()]);
    let restored = call("dyn_with", vec![Value::float(7.0), metadata]);
    let Value::Dyn(restored) = restored else {
        panic!("dynamic")
    };
    assert_eq!(restored.dim, original);
    let alias = call(
        "dyn",
        vec![Value::Num(NumVal::float(
            3.0,
            original.with_display_name("Again.Credit"),
        ))],
    );
    fn hash(value: Value) -> num_bigint::BigInt {
        let Value::Num(value) = call("hash", vec![value]) else {
            panic!("hash number")
        };
        let aevia_core::value::NumKind::Int(value) = value.kind else {
            panic!("integer hash")
        };
        value
    }
    assert_eq!(hash(dynamic), hash(alias));
    let wrong = call(
        "dyn",
        vec![Value::Num(NumVal::float(
            3.0,
            Dim::nominal_base("two/source:Api", "Credit"),
        ))],
    );
    let result = call(
        "check_with",
        vec![call("dim_of", vec![Value::Dyn(restored)]), wrong],
    );
    let Value::Variant(result) = result else {
        panic!("Result")
    };
    assert_eq!(
        result.index, 1,
        "same display name cannot pass a different owner's check"
    );
}
