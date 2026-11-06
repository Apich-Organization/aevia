//! The generator must build and run even when generated outputs do not exist.
use std::path::Path;
use std::process::Command;

fn run(
    mode: &str,
    root: &Path,
) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_aevia-stdlib-gen"))
        .arg(mode)
        .arg("--root")
        .arg(root)
        .output()
        .expect("run generator")
}

#[test]
fn empty_tree_can_be_generated_checked_and_repaired() {
    let root = std::env::temp_dir().join(format!(
        "aevia-stdlib-generator-bootstrap-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&root);
    let check = run("--check", &root);
    assert!(!check.status.success(), "missing outputs must fail --check");
    assert!(!root.exists(), "--check must not create the output tree");
    let write = run("--write", &root);
    assert!(
        write.status.success(),
        "{}",
        String::from_utf8_lossy(&write.stderr)
    );
    let tensor = root.join("stdlib/Tensor.aev");
    let table = root.join("src/stdlib_table.rs");
    assert!(tensor.exists() && table.exists());
    let original = std::fs::read(&tensor).unwrap();
    let original_table = std::fs::read(&table).unwrap();
    assert!(run("--check", &root).status.success());
    std::fs::remove_file(&tensor).unwrap();
    std::fs::remove_file(&table).unwrap();
    assert!(!run("--check", &root).status.success());
    assert!(
        !tensor.exists() && !table.exists(),
        "--check must not repair files"
    );
    let repair = run("--write", &root);
    assert!(
        repair.status.success(),
        "{}",
        String::from_utf8_lossy(&repair.stderr)
    );
    assert_eq!(std::fs::read(&tensor).unwrap(), original);
    assert_eq!(std::fs::read(&table).unwrap(), original_table);
    assert!(run("--check", &root).status.success());
    let unexpected = root.join("stdlib/Unexpected.aev");
    std::fs::copy(&tensor, &unexpected).unwrap();
    assert!(!run("--check", &root).status.success());
    assert!(!run("--write", &root).status.success());
    assert!(
        unexpected.exists(),
        "generator must not delete unknown files"
    );
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn workspace_root_resolves_to_the_std_crate_output_tree() {
    let root = std::env::temp_dir().join(format!(
        "aevia-stdlib-generator-workspace-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&root);
    let library = root.join("crates/aevia-std");
    std::fs::create_dir_all(&library).unwrap();
    std::fs::write(
        root.join("Cargo.toml"),
        "[workspace]\nmembers = [\"crates/aevia-std\"]\n",
    )
    .unwrap();
    std::fs::write(
        library.join("Cargo.toml"),
        "[package]\nname = \"aevia-std\"\nversion = \"0.1.0\"\n",
    )
    .unwrap();
    let generated = run("--write", &root);
    assert!(
        generated.status.success(),
        "{}",
        String::from_utf8_lossy(&generated.stderr)
    );
    assert!(library.join("stdlib/Prelude.aev").exists());
    assert!(library.join("src/stdlib_table.rs").exists());
    assert!(!root.join("stdlib").exists());
    assert!(!root.join("src").exists());
    assert!(run("--check", &root).status.success());
    std::fs::remove_dir_all(root).unwrap();
}
