//! Signature help: the call under the cursor, its overload and its argument.
//!
//! Each case asserts the part a user reads: the rendered label, the parameter
//! labels, which overload is active, and which argument the cursor is in.

use aevia_analysis::Analysis;
use aevia_analysis::Document;

#[test]
fn npy_signatures_follow_intrinsic_identity_including_aliases() {
    for (source, needle, expected) in [
        (
            "let action = read_npy<3>(\"cube.npy\")\n",
            "cube.npy",
            "Tensor<3, Float>",
        ),
        (
            "let reader = read_npy<0>\nlet action = reader(\"scalar.npy\")\n",
            "scalar.npy",
            "Tensor<0, Float>",
        ),
        (
            "let writer = write_npy\nfn save(cube: Tensor<3, Float>) = writer(\"cube.npy\", cube)\n",
            "cube.npy",
            "Tensor<n, Float>",
        ),
    ] {
        let document = Document::new("npy.aev", 1, source);
        let analysis = Analysis::new(&document);
        assert!(
            analysis.diagnostics().is_empty(),
            "{:?}",
            analysis.diagnostics()
        );
        let help = analysis
            .signature_help(source.find(needle).unwrap() as u32)
            .unwrap();
        let label = &help.signatures[0].label;
        assert!(label.contains(expected), "{label}");
        assert!(label.contains("IO<Result<"), "{label}");
        assert!(label.contains("DataError"), "{label}");
    }
    let source = "fn read_npy(x: Int): Int = x\nlet result = read_npy(3)\n";
    let document = Document::new("shadow.aev", 1, source);
    let analysis = Analysis::new(&document);
    let help = analysis
        .signature_help(source.rfind('3').unwrap() as u32)
        .unwrap();
    assert!(help.signatures[0].label.contains("Int"));
    assert!(!help.signatures[0].label.contains("DataError"));
}

/// A temporary package that removes itself.
struct Package {
    root: std::path::PathBuf,
}

impl Package {
    fn new(name: &str) -> Self {
        let mut root = std::env::temp_dir();
        root.push(format!("aevia-signature-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("src")).expect("create package");
        std::fs::write(
            root.join("aevia.toml"),
            "[package]\nname = \"demo\"\nversion = \"0.1.0\"\nentry = \"src/B.aev\"\n",
        )
        .expect("write manifest");
        Self { root }
    }

    fn write(
        &self,
        name: &str,
        source: &str,
    ) -> std::path::PathBuf {
        let path = self.root.join(name);
        std::fs::write(&path, source).expect("write source");
        path
    }

    fn path(
        &self,
        name: &str,
    ) -> std::path::PathBuf {
        self.root.join(name)
    }
}

impl Drop for Package {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

fn offset_of(
    source: &str,
    needle: &str,
) -> u32 {
    u32::try_from(source.find(needle).expect("the needle is in the source")).expect("offset")
}

#[test]
fn signature_help_names_the_call_and_the_active_parameter() {
    let source = "## The answer.\nfn f(x: Int, y: Float): Int = x\n\nlet a = f(1, 2.0)\n";
    let document = Document::new("p.aev", 1, source);
    let analysis = Analysis::new(&document);
    let help = analysis
        .signature_help(offset_of(source, "(1") + 1)
        .expect("inside the first argument");
    assert_eq!(help.active_parameter, 0, "{help:#?}");
    assert_eq!(help.active_signature, 0);
    assert_eq!(help.signatures.len(), 1);
    assert_eq!(help.signatures[0].label, "f: (Int, Float) -> Int");
    assert_eq!(help.signatures[0].parameters, ["Int", "Float"]);
    assert_eq!(
        help.signatures[0].documentation.as_deref(),
        Some("The answer.")
    );

    let help = analysis
        .signature_help(offset_of(source, "2.0"))
        .expect("inside the second argument");
    assert_eq!(help.active_parameter, 1, "{help:#?}");
}

#[test]
fn signature_help_is_none_outside_the_parentheses() {
    let source = "fn f(x: Int): Int = x\nlet a = f(1)\n";
    let document = Document::new("p.aev", 1, source);
    let analysis = Analysis::new(&document);
    // On the callee name, before its `(`.
    assert!(analysis.signature_help(offset_of(source, "f(1)")).is_none());
    // After the closing `)`.
    assert!(
        analysis
            .signature_help(offset_of(source, "f(1)") + 4)
            .is_none()
    );
    // Nowhere near a call.
    assert!(analysis.signature_help(0).is_none());
}

#[test]
fn signature_help_follows_the_innermost_call() {
    let source = "fn f(x: Int): Int = x\nfn g(x: Int, y: Int): Int = x\n\nlet a = g(f(1), 2)\n";
    let document = Document::new("p.aev", 1, source);
    let analysis = Analysis::new(&document);
    let inner = analysis
        .signature_help(offset_of(source, "(1)") + 1)
        .expect("inside the nested call");
    assert_eq!(inner.signatures[0].label, "f: (Int) -> Int", "{inner:#?}");
    assert_eq!(inner.active_parameter, 0);

    let outer = analysis
        .signature_help(offset_of(source, "), 2)") + 3)
        .expect("inside the outer call's second argument");
    assert_eq!(
        outer.signatures[0].label, "g: (Int, Int) -> Int",
        "{outer:#?}"
    );
    assert_eq!(outer.active_parameter, 1, "{outer:#?}");
}

#[test]
fn a_tuple_argument_is_one_argument() {
    let source = "fn p(t: (Int, Int)): Int = 0\nlet a = p((1, 2))\n";
    let document = Document::new("p.aev", 1, source);
    let analysis = Analysis::new(&document);
    let help = analysis
        .signature_help(offset_of(source, "(1, 2)") + 4)
        .expect("inside the tuple argument");
    assert_eq!(help.active_parameter, 0, "{help:#?}");
    assert_eq!(help.signatures[0].parameters.len(), 1, "{help:#?}");
}

#[test]
fn a_trailing_comma_asks_for_the_next_parameter() {
    let source = "fn f(x: Int, y: Float): Int = x\nlet a = f(1,)\n";
    let document = Document::new("p.aev", 1, source);
    let analysis = Analysis::new(&document);
    let comma = offset_of(source, "(1,") + 3;
    let help = analysis
        .signature_help(comma)
        .expect("after the trailing comma");
    assert_eq!(help.active_parameter, 1, "{help:#?}");
}

#[test]
fn signature_help_uses_an_imported_declarations_scheme_and_docs() {
    let package = Package::new("imported");
    package.write(
        "src/A.aev",
        "module A\n\n## Magnitude of x.\nfn magnitude(x: Real[L]): Real[L] = x\n",
    );
    package.write(
        "src/B.aev",
        "module B\n\nimport A\n\nlet m = A.magnitude(2.0 m)\n",
    );
    let path = package.path("src/B.aev");
    let source = std::fs::read_to_string(&path).expect("reads");
    let document = Document::new(path.display().to_string(), 1, &source);
    let analysis = Analysis::from_path(&document, &path);
    assert!(analysis.is_multi_module());
    let help = analysis
        .signature_help(offset_of(&source, "2.0 m"))
        .expect("inside the imported call");
    assert_eq!(
        help.signatures[0].label, "A.magnitude: (Real[L]) -> Real[L]",
        "{help:#?}"
    );
    assert_eq!(
        help.signatures[0].documentation.as_deref(),
        Some("Magnitude of x.")
    );
}

#[test]
fn a_builtin_reports_all_its_overloads_and_selects_by_arity() {
    let source = "let t = [1.0, 2.0]\nlet x = sum(t, 1)\n";
    let document = Document::new("p.aev", 1, source);
    let analysis = Analysis::new(&document);
    let help = analysis
        .signature_help(offset_of(source, "(t, 1)") + 1)
        .expect("inside sum");
    // Four dense overloads plus the `Sparse` ones (§8.4.1).
    assert!(help.signatures.len() >= 4, "{help:#?}");
    assert!(
        help.signatures
            .iter()
            .any(|signature| signature.label.contains("Sparse<")),
        "{help:#?}"
    );
    assert!(
        help.signatures[help.active_signature]
            .label
            .starts_with("sum: "),
        "{help:#?}"
    );
    assert_eq!(
        help.signatures[help.active_signature].parameters.len(),
        2,
        "the two-argument overload is active: {help:#?}"
    );
}

#[test]
fn a_constructor_reports_its_type_and_docs() {
    let source = "## A circle.\ntype Shape = | Circle(Real[L])\n\nlet s = Circle(2.0 m)\n";
    let document = Document::new("p.aev", 1, source);
    let analysis = Analysis::new(&document);
    let help = analysis
        .signature_help(offset_of(source, "2.0 m"))
        .expect("inside the constructor call");
    assert_eq!(
        help.signatures[0].label, "Circle: (Real[L]) -> Shape",
        "{help:#?}"
    );
    assert_eq!(
        help.signatures[0].documentation.as_deref(),
        Some("A circle.")
    );
}

#[test]
fn an_unknown_callee_has_no_help() {
    let source = "let x = unknown(1)\n";
    let document = Document::new("p.aev", 1, source);
    let analysis = Analysis::new(&document);
    assert!(analysis.signature_help(offset_of(source, "1")).is_none());
}

#[test]
fn a_rssn_operator_reports_its_manifest_overloads() {
    let source = "let r = Rssn.sqrt(4.0)\n";
    let document = Document::new("p.aev", 1, source);
    let analysis = Analysis::new(&document);
    let help = analysis
        .signature_help(offset_of(source, "4.0"))
        .expect("inside the kernel call");
    assert!(!help.signatures.is_empty(), "{help:#?}");
    assert!(
        help.signatures[help.active_signature]
            .label
            .starts_with("Rssn.sqrt: "),
        "{help:#?}"
    );
    assert!(
        !help.signatures[help.active_signature].parameters.is_empty(),
        "{help:#?}"
    );
}

#[test]
fn a_class_method_names_its_class_and_proved_instance() {
    let source = "class Pretty a where\n    pretty: a -> String\n\ninstance Pretty for Int where\n    pretty(x: Int): String = \"i\"\n\nfn f(): String = pretty(1)\n";
    let document = Document::new("p.aev", 1, source);
    let analysis = Analysis::new(&document);
    let help = analysis
        .signature_help(offset_of(source, "pretty(1)") + 7)
        .expect("inside the method call");
    assert!(help.signatures[0].label.contains("Pretty"), "{help:#?}");
    assert_eq!(help.active_parameter, 0);
}
