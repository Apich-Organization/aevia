//! References and rename over the loaded program.
//!
//! Every answer is checked against the text it claims to describe: a location
//! must land on the name in the file it names, and a rename must produce edits
//! that leave a program which still checks.

use aevia_analysis::Analysis;
use aevia_analysis::Document;
use aevia_analysis::FileEdit;

/// A temporary package that removes itself.
struct Package {
    root: std::path::PathBuf,
}

impl Package {
    fn new(name: &str) -> Self {
        let mut root = std::env::temp_dir();
        root.push(format!("aevia-rename-{name}-{}", std::process::id()));
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

/// A package whose `B` imports and uses `A.value` twice.
struct Imported {
    package: Package,
    a_source: String,
    b_source: String,
}

fn imported(name: &str) -> Imported {
    let package = Package::new(name);
    let a_source = "module A\n\nfn value(): Int = 1\n".to_string();
    let b_source =
        "module B\n\nimport A\n\nlet got = A.value()\nlet again = A.value()\n".to_string();
    package.write("src/A.aev", &a_source);
    package.write("src/B.aev", &b_source);
    Imported {
        package,
        a_source,
        b_source,
    }
}

impl Imported {
    fn open_b(&self) -> (Document, std::path::PathBuf) {
        let path = self.package.path("src/B.aev");
        let document = Document::new(path.display().to_string(), 1, &self.b_source);
        (document, path)
    }

    fn text_of(
        &self,
        document: &str,
    ) -> &str {
        if document.ends_with("A.aev") {
            &self.a_source
        } else {
            &self.b_source
        }
    }
}

fn offset_of(
    source: &str,
    needle: &str,
) -> u32 {
    u32::try_from(source.find(needle).expect("the needle is in the source")).expect("offset")
}

/// The source text a location covers.
fn text_at(
    source: &str,
    range: aevia_analysis::Range,
) -> String {
    let document = Document::new("x.aev", 1, source);
    let start = document.offset(range.start).expect("start offset");
    let end = document.offset(range.end).expect("end offset");
    source[start as usize..end as usize].to_string()
}

/// Applies a rename's edits to one file's text.
fn apply(
    text: &str,
    edits: &[FileEdit],
    document: &str,
) -> String {
    let mut replacements: Vec<(usize, usize, String)> = edits
        .iter()
        .filter(|edit| edit.document == document)
        .map(|edit| {
            let source = Document::new("x.aev", 1, text);
            let start = source.offset(edit.edit.range.start).expect("start");
            let end = source.offset(edit.edit.range.end).expect("end");
            (
                usize::try_from(start).expect("offset"),
                usize::try_from(end).expect("offset"),
                edit.edit.new_text.clone(),
            )
        })
        .collect();
    replacements.sort_by(|left, right| right.0.cmp(&left.0));
    let mut out = text.to_string();
    for (start, end, new_text) in replacements {
        out.replace_range(start..end, &new_text);
    }
    out
}

fn renames_in<'a>(
    edits: &'a [FileEdit],
    suffix: &str,
) -> Vec<&'a FileEdit> {
    edits
        .iter()
        .filter(|edit| edit.document.ends_with(suffix))
        .collect()
}

#[test]
fn references_find_uses_across_imported_modules() {
    let package = imported("references");
    let (document, _path) = package.open_b();
    let analysis = Analysis::from_path(&document, &package.package.path("src/B.aev"));
    assert!(analysis.is_multi_module());

    let references = analysis.references(offset_of(&package.b_source, "value"), true);
    assert_eq!(references.len(), 3, "{references:#?}");
    // One declaration in A and two uses in B, each range covering `value` in
    // the file it names.
    for location in &references {
        assert_eq!(
            text_at(package.text_of(&location.document), location.range),
            "value",
            "{location:#?}"
        );
    }
    assert_eq!(
        references
            .iter()
            .filter(|location| location.document.ends_with("A.aev"))
            .count(),
        1
    );
    assert_eq!(
        references
            .iter()
            .filter(|location| location.document.ends_with("B.aev"))
            .count(),
        2
    );

    // `includeDeclaration: false` drops the declaration.
    let uses = analysis.references(offset_of(&package.b_source, "value"), false);
    assert_eq!(uses.len(), 2, "{uses:#?}");
    assert!(
        uses.iter()
            .all(|location| location.document.ends_with("B.aev"))
    );
}

#[test]
fn references_of_a_local_parameter_stay_in_the_file() {
    let source = "fn double(n: Int): Int = n + n\n";
    let document = Document::new("p.aev", 1, source);
    let analysis = Analysis::new(&document);
    // `n:` is the parameter; a bare `n` would find the one in `fn`.
    let parameter = offset_of(source, "n:");
    let references = analysis.references(parameter, true);
    assert_eq!(references.len(), 3, "{references:#?}");
    assert!(
        references
            .iter()
            .all(|location| location.document == "p.aev")
    );
    assert_eq!(analysis.references(parameter, false).len(), 2);
}

#[test]
fn references_find_constructor_and_type_uses() {
    let package = Package::new("constructors");
    let a_source = "module A\n\ntype Shape = | Circle(Real[L]) | Square(Real[L])\n\nfn area(s: Shape): Real[L] =\n    match s with\n    | Circle(r) -> r ^ 2\n    | Square(w) -> w * w\n";
    let b_source = "module B\n\nimport A\n\nlet shape = A.Circle(2.0 m)\n";
    package.write("src/A.aev", a_source);
    package.write("src/B.aev", b_source);
    let path = package.path("src/B.aev");
    let document = Document::new(path.display().to_string(), 1, b_source);
    let analysis = Analysis::from_path(&document, &path);

    let references = analysis.references(offset_of(b_source, "Circle"), true);
    assert_eq!(references.len(), 3, "{references:#?}");
    let documents: Vec<&str> = references
        .iter()
        .map(|location| location.document.as_str())
        .collect();
    assert!(documents.iter().any(|document| document.ends_with("A.aev")));
    assert!(documents.iter().any(|document| document.ends_with("B.aev")));

    let edits = analysis
        .rename(offset_of(b_source, "Circle"), "Round")
        .expect("a constructor is renamable");
    let a_document = references
        .iter()
        .find(|location| location.document.ends_with("A.aev"))
        .expect("a reference in A")
        .document
        .clone();
    let renamed = apply(a_source, &edits, &a_document);
    assert!(renamed.contains("| Round(Real[L])"), "{renamed}");
    assert!(renamed.contains("| Round(r) ->"), "{renamed}");
}

#[test]
fn rename_of_an_imported_function_updates_both_files() {
    let package = imported("rename");
    let (document, path) = package.open_b();
    let analysis = Analysis::from_path(&document, &path);
    let edits = analysis
        .rename(offset_of(&package.b_source, "value"), "amount")
        .expect("a top-level function is renamable");
    assert_eq!(renames_in(&edits, "A.aev").len(), 1, "{edits:#?}");
    assert_eq!(renames_in(&edits, "B.aev").len(), 2, "{edits:#?}");

    let a_document = renames_in(&edits, "A.aev")[0].document.clone();
    let b_document = renames_in(&edits, "B.aev")[0].document.clone();
    let renamed_a = apply(&package.a_source, &edits, &a_document);
    let renamed_b = apply(&package.b_source, &edits, &b_document);
    assert!(renamed_a.contains("fn amount(): Int = 1"), "{renamed_a}");
    assert!(!renamed_a.contains("value"), "{renamed_a}");
    assert_eq!(renamed_b.matches("A.amount()").count(), 2, "{renamed_b}");
    assert!(!renamed_b.contains("value"), "{renamed_b}");

    // The renamed files must still check, and the new name must be what the
    // importer sees.
    package.package.write("src/A.aev", &renamed_a);
    package.package.write("src/B.aev", &renamed_b);
    let renamed_document = Document::new(b_document, 2, &renamed_b);
    let renamed_analysis =
        Analysis::from_path(&renamed_document, &package.package.path("src/B.aev"));
    assert!(renamed_analysis.is_multi_module());
    assert!(
        renamed_analysis.diagnostics().is_empty(),
        "{:#?}",
        renamed_analysis.diagnostics()
    );
    let hover = renamed_analysis
        .hover(offset_of(&renamed_b, "amount"))
        .expect("a hover on the renamed name");
    assert!(hover.contents.contains("Int"), "{}", hover.contents);
}

#[test]
fn prepare_rename_names_the_occurrence_and_its_range() {
    let package = imported("prepare");
    let (document, path) = package.open_b();
    let analysis = Analysis::from_path(&document, &path);
    let placeholder = analysis
        .prepare_rename(offset_of(&package.b_source, "value"))
        .expect("a renamable name");
    assert_eq!(placeholder.placeholder, "value");
    assert_eq!(
        text_at(&package.b_source, placeholder.range),
        "value",
        "{placeholder:#?}"
    );
}

#[test]
fn rename_refuses_a_name_already_exported() {
    let package = Package::new("collision");
    package.write(
        "src/A.aev",
        "module A\n\nfn value(): Int = 1\nfn amount(): Int = 2\n",
    );
    package.write("src/B.aev", "module B\n\nimport A\n\nlet got = A.value()\n");
    let path = package.path("src/B.aev");
    let source = std::fs::read_to_string(&path).expect("reads");
    let document = Document::new(path.display().to_string(), 1, &source);
    let analysis = Analysis::from_path(&document, &path);
    let error = analysis
        .rename(offset_of(&source, "value"), "amount")
        .expect_err("the name is taken");
    match error {
        | aevia_analysis::RenameError::Collision(message) => {
            assert!(message.contains("already declared"), "{message}");
        },
        | other => panic!("expected a collision, got {other:?}"),
    }
}

#[test]
fn rename_rejects_capture_of_opened_names_in_both_directions() {
    for (case, body, old_name, new_name) in [
        (
            "export",
            "fn f(amount: Int): Int = value() + amount\n",
            "value",
            "amount",
        ),
        ("local", "fn f(x: Int): Int = x + value()\n", "x:", "value"),
    ] {
        let package = Package::new(&format!("capture-{case}"));
        package.write("src/A.aev", "module A\nfn value(): Int = 1\n");
        let source = format!("module B\nopen A\n{body}");
        let path = package.write("src/B.aev", &source);
        let document = Document::new(path.display().to_string(), 1, &source);
        let analysis = Analysis::from_path(&document, &path);
        assert!(
            analysis.diagnostics().is_empty(),
            "{:?}",
            analysis.diagnostics()
        );
        assert!(matches!(
            analysis.rename(offset_of(&source, old_name), new_name),
            Err(aevia_analysis::RenameError::Collision(_))
        ));
    }
}

#[test]
fn qualified_rename_does_not_capture_a_same_named_local() {
    let package = Package::new("qualified-safe");
    package.write("src/A.aev", "module A\nfn value(): Int = 1\n");
    let source = "module B\nimport A\nfn f(amount: Int): Int = A.value() + amount\n";
    let path = package.write("src/B.aev", source);
    let document = Document::new(path.display().to_string(), 1, source);
    let analysis = Analysis::from_path(&document, &path);
    let edits = analysis
        .rename(offset_of(source, "value"), "amount")
        .unwrap();
    let changed = apply(source, &edits, &path.display().to_string());
    assert!(changed.contains("A.amount() + amount"));
    assert_eq!(renames_in(&edits, "A.aev").len(), 1);
}

#[test]
fn rename_refuses_invalid_names_and_the_wrong_case() {
    let package = imported("invalid");
    let (document, path) = package.open_b();
    let analysis = Analysis::from_path(&document, &path);
    let offset = offset_of(&package.b_source, "value");
    for new_name in ["fn", "two words", "Value", ""] {
        let error = analysis
            .rename(offset, new_name)
            .expect_err("the new name is invalid");
        assert!(
            matches!(error, aevia_analysis::RenameError::InvalidName(_)),
            "{new_name:?} produced {error:?}"
        );
    }
}

#[test]
fn rename_of_a_local_binding_rewrites_its_uses() {
    let source = "fn add(n: Int): Int = n + 1\n";
    let document = Document::new("p.aev", 1, source);
    let analysis = Analysis::new(&document);
    let edits = analysis
        .rename(offset_of(source, "n:"), "total")
        .expect("a parameter is renamable");
    assert_eq!(edits.len(), 2, "{edits:#?}");
    let renamed = apply(source, &edits, "p.aev");
    assert_eq!(renamed, "fn add(total: Int): Int = total + 1\n");

    // A name already bound in the same file is refused.
    let shadowed = "fn add(n: Int, x: Int): Int = n + x\n";
    let shadowed_document = Document::new("p.aev", 1, shadowed);
    let shadowed_analysis = Analysis::new(&shadowed_document);
    let error = shadowed_analysis
        .rename(offset_of(shadowed, "n:"), "x")
        .expect_err("`x` is already bound");
    assert!(
        matches!(error, aevia_analysis::RenameError::Collision(_)),
        "{error:?}"
    );
}

#[test]
fn prepare_rename_refuses_prelude_names_and_units() {
    let source = "fn f(x: Real[L]): Real[L] = do\n    let shown = show(x)\n    shown\n";
    let document = Document::new("p.aev", 1, source);
    let analysis = Analysis::new(&document);
    let error = analysis
        .prepare_rename(offset_of(source, "show("))
        .expect_err("a prelude builtin has no declaration to rename");
    assert!(
        matches!(error, aevia_analysis::RenameError::NotRenamable(_)),
        "{error:?}"
    );

    let units = "let d = 3.0 m\n";
    let units_document = Document::new("p.aev", 1, units);
    let units_analysis = Analysis::new(&units_document);
    let error = units_analysis
        .prepare_rename(offset_of(units, "m"))
        .expect_err("a unit name is not renamable");
    match error {
        | aevia_analysis::RenameError::NotRenamable(message) => {
            assert!(message.contains("unit"), "{message}");
        },
        | other => panic!("expected a refusal, got {other:?}"),
    }
}

#[test]
fn references_are_empty_off_a_name() {
    let package = imported("empty");
    let (document, path) = package.open_b();
    let analysis = Analysis::from_path(&document, &path);
    let blank = offset_of(&package.b_source, "module") + 3;
    assert!(analysis.references(blank, true).is_empty());
    assert!(analysis.prepare_rename(blank).is_err());
}

#[test]
fn record_field_references_and_rename_stay_in_the_document() {
    let source = "type Point = { x: Real[L], y: Real[L] }\n\nfn shift(p: Point): Point = { x = p.x + 1.0 m, y = p.y }\n";
    let document = Document::new("p.aev", 1, source);
    let analysis = Analysis::new(&document);
    let offset = offset_of(source, "x");
    let placeholder = analysis
        .prepare_rename(offset)
        .expect("a field declared in this document is renamable");
    assert_eq!(placeholder.placeholder, "x");
    assert_eq!(text_at(source, placeholder.range), "x");

    let references = analysis.references(offset, true);
    assert_eq!(references.len(), 3, "{references:#?}");
    for location in &references {
        assert_eq!(text_at(source, location.range), "x", "{location:#?}");
    }

    let edits = analysis
        .rename(offset, "width")
        .expect("the field is renamable");
    let renamed = apply(source, &edits, "p.aev");
    assert!(
        renamed.contains("type Point = { width: Real[L], y: Real[L] }"),
        "{renamed}"
    );
    assert!(
        renamed.contains("{ width = p.width + 1.0 m, y = p.y }"),
        "{renamed}"
    );

    // A field of a type the document only imports is refused.
    let field_package = Package::new("field-owned");
    field_package.write(
        "src/A.aev",
        "module A\n\ntype Point = { x: Real[L], y: Real[L] }\n",
    );
    field_package.write(
        "src/B.aev",
        "module B\n\nimport A\n\nfn shift(p: A.Point): A.Point = { x = p.x, y = p.y }\n",
    );
    let path = field_package.path("src/B.aev");
    let source = std::fs::read_to_string(&path).expect("reads");
    let document = Document::new(path.display().to_string(), 1, &source);
    let analysis = Analysis::from_path(&document, &path);
    let error = analysis
        .prepare_rename(offset_of(&source, "x"))
        .expect_err("the field belongs to an imported type");
    match error {
        | aevia_analysis::RenameError::NotRenamable(message) => {
            assert!(message.contains("does not declare"), "{message}");
        },
        | other => panic!("expected a refusal, got {other:?}"),
    }
}

#[test]
fn a_field_declared_by_two_types_in_the_document_is_ambiguous() {
    let source = "type A = { x: Real[L] }\ntype B = { x: Real[L] }\n";
    let document = Document::new("p.aev", 1, source);
    let analysis = Analysis::new(&document);
    let error = analysis
        .prepare_rename(offset_of(source, "x"))
        .expect_err("two records declare x");
    match error {
        | aevia_analysis::RenameError::NotRenamable(message) => {
            assert!(message.contains("several record types"), "{message}");
        },
        | other => panic!("expected a refusal, got {other:?}"),
    }
}
