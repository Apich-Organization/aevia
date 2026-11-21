//! The generator must build and run even when generated outputs do not exist.
use std::fs;
use std::io;
use std::path::Path;
use std::path::PathBuf;
use std::process::Command;

fn run(
    mode: &str,
    root: &Path,
) -> std::io::Result<std::process::Output> {
    Command::new(env!("CARGO_BIN_EXE_aevia-stdlib-gen"))
        .arg(mode)
        .arg("--root")
        .arg(root)
        .output()
}

struct Fixture(PathBuf);

impl Fixture {
    fn new(case: &str) -> io::Result<Self> {
        let path = std::env::temp_dir().join(format!(
            "aevia-stdlib-generator-{case}-{}",
            std::process::id()
        ));
        fs::create_dir(&path)?;
        Ok(Self(path))
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

#[test]
fn empty_tree_can_be_generated_checked_and_repaired() -> std::io::Result<()> {
    let fixture = Fixture::new("bootstrap")?;
    let root = fixture.0.join("output");
    let check = run("--check", &root)?;
    assert!(!check.status.success(), "missing outputs must fail --check");
    assert!(!root.exists(), "--check must not create the output tree");
    let write = run("--write", &root)?;
    assert!(
        write.status.success(),
        "{}",
        String::from_utf8_lossy(&write.stderr)
    );
    let tensor = root.join("stdlib/Tensor.aev");
    let table = root.join("src/stdlib_table.rs");
    assert!(tensor.exists() && table.exists());
    let original = std::fs::read(&tensor)?;
    let original_table = std::fs::read(&table)?;
    assert!(run("--check", &root)?.status.success());
    std::fs::remove_file(&tensor)?;
    std::fs::remove_file(&table)?;
    assert!(!run("--check", &root)?.status.success());
    assert!(
        !tensor.exists() && !table.exists(),
        "--check must not repair files"
    );
    let repair = run("--write", &root)?;
    assert!(
        repair.status.success(),
        "{}",
        String::from_utf8_lossy(&repair.stderr)
    );
    assert_eq!(std::fs::read(&tensor)?, original);
    assert_eq!(std::fs::read(&table)?, original_table);
    assert!(run("--check", &root)?.status.success());
    let unexpected = root.join("stdlib/Unexpected.aev");
    std::fs::copy(&tensor, &unexpected)?;
    assert!(!run("--check", &root)?.status.success());
    assert!(!run("--write", &root)?.status.success());
    assert!(
        unexpected.exists(),
        "generator must not delete unknown files"
    );
    Ok(())
}

#[test]
fn workspace_root_resolves_to_the_std_crate_output_tree() -> std::io::Result<()> {
    let fixture = Fixture::new("workspace")?;
    let root = &fixture.0;
    let library = root.join("crates/aevia-std");
    std::fs::create_dir_all(&library)?;
    std::fs::write(
        root.join("Cargo.toml"),
        "[workspace]\nmembers = [\"crates/aevia-std\"]\n",
    )?;
    std::fs::write(
        library.join("Cargo.toml"),
        "[package]\nname = \"aevia-std\"\nversion = \"0.1.0\"\n",
    )?;
    let generated = run("--write", root)?;
    assert!(
        generated.status.success(),
        "{}",
        String::from_utf8_lossy(&generated.stderr)
    );
    assert!(library.join("stdlib/Prelude.aev").exists());
    assert!(library.join("src/stdlib_table.rs").exists());
    assert!(!root.join("stdlib").exists());
    assert!(!root.join("src").exists());
    assert!(run("--check", root)?.status.success());
    Ok(())
}

#[test]
fn a_conflicting_destination_is_reported_before_any_repair() -> io::Result<()> {
    let fixture = Fixture::new("conflict")?;
    let root = &fixture.0;
    let table = root.join("src/stdlib_table.rs");
    let tensor = root.join("stdlib/Tensor.aev");
    fs::create_dir_all(&tensor)?;
    fs::create_dir_all(root.join("src"))?;
    fs::write(&table, "stale table")?;

    for mode in ["--check", "--write"] {
        let result = run(mode, root)?;
        assert!(!result.status.success());
        let diagnostic = String::from_utf8_lossy(&result.stderr);
        assert!(diagnostic.contains("Tensor.aev"), "{diagnostic}");
        assert!(
            diagnostic.contains("expected a regular output file"),
            "{diagnostic}"
        );
        assert_eq!(fs::read(&table)?, b"stale table");
        assert!(!root.join("stdlib/Prelude.aev").exists());
    }
    Ok(())
}

#[cfg(unix)]
#[test]
fn a_changed_file_is_replaced_and_matching_outputs_keep_their_inode() -> io::Result<()> {
    use std::os::unix::fs::MetadataExt as _;
    use std::os::unix::fs::PermissionsExt as _;

    let fixture = Fixture::new("replace")?;
    let root = &fixture.0;
    let first = run("--write", root)?;
    assert!(
        first.status.success(),
        "{}",
        String::from_utf8_lossy(&first.stderr)
    );
    let tensor = root.join("stdlib/Tensor.aev");
    let table = root.join("src/stdlib_table.rs");
    let table_inode = fs::metadata(&table)?.ino();
    let expected = fs::read(&tensor)?;
    let stale = vec![0xff; expected.len()];
    fs::write(&tensor, &stale)?;
    fs::set_permissions(&tensor, fs::Permissions::from_mode(0o640))?;
    let old_contents = root.join("retained-tensor");
    fs::hard_link(&tensor, &old_contents)?;

    let repair = run("--write", root)?;
    assert!(
        repair.status.success(),
        "{}",
        String::from_utf8_lossy(&repair.stderr)
    );
    assert_eq!(fs::read(&tensor)?, expected);
    assert_eq!(fs::read(old_contents)?, stale);
    assert_eq!(fs::metadata(&tensor)?.mode() & 0o777, 0o640);
    assert_eq!(fs::metadata(&table)?.ino(), table_inode);

    let tensor_inode = fs::metadata(&tensor)?.ino();
    let noop = run("--write", root)?;
    assert!(noop.status.success());
    assert!(noop.stderr.is_empty());
    assert_eq!(fs::metadata(&tensor)?.ino(), tensor_inode);
    assert_eq!(fs::metadata(&table)?.ino(), table_inode);
    Ok(())
}

#[cfg(unix)]
#[test]
fn an_output_directory_link_is_not_a_generated_directory() -> io::Result<()> {
    use std::os::unix::fs::symlink;

    let fixture = Fixture::new("linked-directory")?;
    let root = fixture.0.join("output");
    let units = root.join("stdlib/Units");
    let separate = fixture.0.join("separate");
    fs::create_dir_all(root.join("stdlib"))?;
    fs::create_dir(&separate)?;
    symlink(&separate, &units)?;
    let result = run("--write", &root)?;
    assert!(!result.status.success());
    let diagnostic = String::from_utf8_lossy(&result.stderr);
    assert!(
        diagnostic.contains("expected an output directory"),
        "{diagnostic}"
    );
    assert!(diagnostic.contains("Units"), "{diagnostic}");
    assert!(fs::read_dir(&separate)?.next().is_none());
    assert!(!root.join("src/stdlib_table.rs").exists());
    Ok(())
}

#[cfg(unix)]
#[test]
fn an_output_root_need_not_be_utf8() -> io::Result<()> {
    use std::ffi::OsString;
    use std::os::unix::ffi::OsStringExt as _;

    let fixture = Fixture::new("non-utf8")?;
    let root = fixture.0.join(OsString::from_vec(b"output-\xff".to_vec()));
    let write = run("--write", &root)?;
    assert!(
        write.status.success(),
        "{}",
        String::from_utf8_lossy(&write.stderr)
    );
    assert!(run("--check", &root)?.status.success());
    Ok(())
}

#[cfg(unix)]
#[test]
fn an_uninspectable_workspace_manifest_does_not_redirect_generation() -> io::Result<()> {
    use std::os::unix::fs::PermissionsExt as _;

    let fixture = Fixture::new("uninspectable-workspace")?;
    let root = &fixture.0;
    let library = root.join("crates/aevia-std");
    let nested_manifest = library.join("Cargo.toml");
    fs::create_dir_all(&library)?;
    fs::write(root.join("Cargo.toml"), "[workspace]\n")?;
    fs::write(&nested_manifest, "[package]\n")?;
    let original = fs::metadata(&library)?.permissions();
    fs::set_permissions(&library, fs::Permissions::from_mode(0o600))?;
    let denied = fs::metadata(&nested_manifest)
        .is_err_and(|error| error.kind() == io::ErrorKind::PermissionDenied);
    let output = denied.then(|| run("--write", root));
    fs::set_permissions(&library, original)?;

    // Elevated users ignore these permission bits.
    if let Some(output) = output {
        let output = output?;
        assert!(!output.status.success());
        let diagnostic = String::from_utf8_lossy(&output.stderr);
        assert!(
            diagnostic.contains("cannot inspect Cargo manifest"),
            "{diagnostic}"
        );
        assert!(!root.join("stdlib").exists());
        assert!(!root.join("src").exists());
    }
    Ok(())
}
