//! Real NPY files, rank-selected source readers and scientific computation.
use aevia_core::Interp;
use aevia_core::Mode;
use aevia_core::value::Value;
use aevia_syntax::Span;

fn payload(
    value: Value,
    expected: usize,
) -> Value {
    let Value::Variant(result) = value else {
        panic!("expected Result")
    };
    assert_eq!(result.index, expected, "{:?}", result.fields);
    result.fields[0].clone()
}
fn action(
    interp: &mut Interp,
    value: Value,
) -> Value {
    let Value::Io(action) = value else {
        panic!("expected IO action")
    };
    interp.run_io(&action).unwrap()
}
#[test]
fn rank_selected_readers_and_aliases_preserve_evidence_in_both_engines() {
    let fixture = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/npy/rank3-le-f32.npy");
    let path = fixture
        .to_str()
        .unwrap()
        .replace('\\', "\\\\")
        .replace('"', "\\\"");
    let quote = |name: &str| {
        std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/npy")
            .join(name)
            .to_string_lossy()
            .replace('\\', "\\\\")
            .replace('"', "\\\"")
    };
    let scalar = quote("scalar-le-f64.npy");
    let vector = quote("rank1-be-f32.npy");
    let source = format!(
        r#"let scalar = read_npy<0>("{scalar}")
let vector = read_npy<1>("{vector}")
fn identity(f) = f
let base = read_npy
let selected = base<3>
let alias = identity(selected)
let good = alias("{path}")
let bad = base<2>("{path}")
"#
    );
    let compilation = aevia_core::compile("npy-rank.aev", &source);
    assert!(
        !compilation.diagnostics.has_errors(),
        "{:?}",
        compilation.diagnostics.items
    );
    for mode in [Mode::Ast, Mode::Ir] {
        let mut interp = Interp::new(&compilation.checked).with_mode(mode);
        let diagnostics = interp.load(compilation.module.as_ref().unwrap());
        assert!(
            !diagnostics.has_errors(),
            "{mode:?}: {:?}",
            diagnostics.items
        );
        for (name, expected) in [("scalar", Vec::<usize>::new()), ("vector", vec![3])] {
            let read = interp.eval_toplevel(name).unwrap();
            let Value::Tensor(tensor) = payload(action(&mut interp, read), 0) else {
                panic!("expected tensor")
            };
            assert_eq!(tensor.shape, expected, "{mode:?} {name}");
        }
        let read = interp.eval_toplevel("good").unwrap();
        let Value::Tensor(tensor) = payload(action(&mut interp, read), 0) else {
            panic!("expected tensor")
        };
        assert_eq!(tensor.shape, vec![2, 3, 4]);
        let bad = interp.eval_toplevel("bad").unwrap();
        let Value::Str(error) = payload(action(&mut interp, bad), 1) else {
            panic!("expected DataError")
        };
        assert!(error.starts_with("E0903:"), "{mode:?}: {error}");
    }
}
#[test]
fn rank_is_required_and_writer_checks_element_type_and_dimensions() {
    for (source, code) in [
        ("let bad = read_npy(\"x.npy\")\n", "E0314"),
        ("let bad = read_npy<Bool>(\"x.npy\")\n", "E0304"),
        ("fn bad<n: Nat>() = read_npy<n>(\"x.npy\")\n", "E0304"),
        ("let bad = write_npy(\"x.npy\", [true])\n", "E0302"),
        ("let bad = write_npy(\"x.npy\", [1.0 m])\n", "E0401"),
    ] {
        let compilation = aevia_core::compile("npy-bad.aev", source);
        assert!(
            compilation
                .diagnostics
                .items
                .iter()
                .any(|item| item.code == code),
            "{source}: {:?}",
            compilation.diagnostics.items
        );
    }
}
#[test]
fn explicit_function_type_boundary_is_diagnosed() {
    let source = "let reader: String -> IO (Result<Tensor<1, Float>, DataError>) = read_npy<1>\n";
    let compilation = aevia_core::compile("npy-function-boundary.aev", source);
    assert!(
        compilation
            .diagnostics
            .items
            .iter()
            .any(|item| item.code == "E0302" && item.message.contains("explicit function type")),
        "{:?}",
        compilation.diagnostics.items
    );
}
#[test]
fn source_shadowing_does_not_select_npy_intrinsics() {
    let source = "fn read_npy(x: Int): Int = x + 1\nfn write_npy(x: Int): Int = x + 2\nlet a = read_npy(1)\nlet b = write_npy(1)\n";
    let compilation = aevia_core::compile("npy-shadow.aev", source);
    assert!(
        !compilation.diagnostics.has_errors(),
        "{:?}",
        compilation.diagnostics.items
    );
    for mode in [Mode::Ast, Mode::Ir] {
        let mut interp = Interp::new(&compilation.checked).with_mode(mode);
        let diagnostics = interp.load(compilation.module.as_ref().unwrap());
        assert!(
            !diagnostics.has_errors(),
            "{mode:?}: {:?}",
            diagnostics.items
        );
        assert_eq!(interp.eval_toplevel("a").unwrap().as_f64(), Some(2.0));
        assert_eq!(interp.eval_toplevel("b").unwrap().as_f64(), Some(3.0));
    }
}
#[test]
fn read_numpy_solve_and_write_numpy_through_generated_scientific_namespace() {
    let fixture = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/npy/fortran-be-f64.npy");
    let output =
        std::env::temp_dir().join(format!("aevia-npy-workflow-{}.npy", std::process::id()));
    let quote = |path: &std::path::Path| {
        path.to_str()
            .unwrap()
            .replace('\\', "\\\\")
            .replace('"', "\\\"")
    };
    let source = format!(
        r#"import Data.Npy {{ read_npy, write_npy }}
import Rssn_numerical_matrix
let reader = read_npy<2>
let input = reader("{}")
fn transform(x: Tensor<2, Float>): Tensor<2, Float> = Rssn_numerical_matrix.num_matmul(x, [[1.0, 4.0], [2.0, 5.0], [3.0, 6.0]])
fn save(x: Tensor<2, Float>) = write_npy("{}", x)
"#,
        quote(&fixture),
        quote(&output)
    );
    let root =
        std::env::temp_dir().join(format!("aevia-npy-workflow-source-{}", std::process::id()));
    std::fs::create_dir_all(&root).unwrap();
    let path = root.join("main.aev");
    std::fs::write(&path, source).unwrap();
    let program = aevia_core::driver::load(&path);
    assert!(
        !program.diagnostics.has_errors(),
        "{:?}",
        program.diagnostics.items
    );
    for mode in [Mode::Ast, Mode::Ir] {
        let mut interp = Interp::new(&program.entry().unwrap().checked).with_mode(mode);
        let diagnostics = aevia_core::driver::run_modules(&mut interp, &program);
        assert!(
            !diagnostics.has_errors(),
            "{mode:?}: {:?}",
            diagnostics.items
        );
        let input = interp.eval_toplevel("input").unwrap();
        let input = payload(action(&mut interp, input), 0);
        let transform = interp.eval_toplevel("transform").unwrap();
        let result = interp
            .apply(transform, vec![input], Span::default())
            .unwrap();
        let save = interp.eval_toplevel("save").unwrap();
        let write = interp.apply(save, vec![result], Span::default()).unwrap();
        let _ = payload(action(&mut interp, write), 0);
        let bytes = std::fs::read(&output).unwrap();
        assert_eq!(
            bytes,
            include_bytes!("fixtures/npy/expected-matmul-le-f64.npy"),
            "{mode:?}: output must match NumPy's file bytes"
        );
        let tensor = aevia_core::builtins::npy_decode(&bytes).unwrap();
        assert_eq!(tensor.shape, vec![2, 2]);
        assert_eq!(
            tensor.dense_flat().unwrap().as_ref(),
            &[14.0, 32.0, 32.0, 77.0]
        );
    }
    let _ = std::fs::remove_file(output);
    std::fs::remove_dir_all(root).unwrap();
}
