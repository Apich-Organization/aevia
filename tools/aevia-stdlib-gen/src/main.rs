//! Regenerate or check standard-library sources and the embedding table.
//! Usage: cargo run -p aevia-stdlib-gen -- --check|--write [--root DIR]
mod support;

use std::collections::BTreeMap;
use std::collections::BTreeSet;
use std::path::Path;
use std::path::PathBuf;

fn source_files(
    root: &Path,
    dir: &Path,
    files: &mut BTreeSet<PathBuf>,
) -> std::io::Result<()> {
    if !dir.exists() {
        return Ok(());
    }
    for entry in std::fs::read_dir(dir)? {
        let path = entry?.path();
        if path.is_dir() {
            source_files(root, &path, files)?;
        } else if path.extension().is_some_and(|ext| ext == "aev") {
            files.insert(path.strip_prefix(root).unwrap().to_path_buf());
        }
    }
    Ok(())
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = std::env::args().skip(1);
    let mode = args
        .next()
        .ok_or("usage: aevia-stdlib-gen --check|--write [--root DIR]")?;
    if mode != "--check" && mode != "--write" {
        return Err("expected --check or --write".into());
    }
    let default_root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../crates/aevia-std");
    let root = match args.next() {
        | None => default_root,
        | Some(flag) if flag == "--root" => {
            PathBuf::from(args.next().ok_or("--root needs a directory")?)
        },
        | Some(_) => return Err("usage: aevia-stdlib-gen --check|--write [--root DIR]".into()),
    };
    if args.next().is_some() {
        return Err("unexpected argument".into());
    }
    // Accept the workspace root as well as the std crate/output root. Writing
    // a second stdlib beside Cargo.toml is never the intended workspace layout.
    let nested = root.join("crates/aevia-std");
    let root = if root.join("Cargo.toml").is_file() && nested.join("Cargo.toml").is_file() {
        nested
    } else {
        root
    };
    let write = mode == "--write";
    let spec_path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../spec/08-stdlib.md");
    let spec = std::fs::read_to_string(spec_path)?;
    let mut expected: BTreeMap<PathBuf, String> =
        aevia_stdlib_model::stdlib::spec_module_sources(&spec)
            .into_iter()
            .map(|(path, source)| (PathBuf::from(format!("stdlib/{path}.aev")), source))
            .collect();
    expected.insert(
        PathBuf::from("stdlib/Units/Constants.aev"),
        support::constants_module_source(),
    );
    for (path, system) in aevia_stdlib_model::stdlib::SYSTEM_MODULES {
        expected.insert(
            PathBuf::from(format!("stdlib/{path}.aev")),
            support::system_module_source(path, system),
        );
    }
    expected.insert(
        PathBuf::from("src/stdlib_table.rs"),
        aevia_stdlib_model::stdlib::embedding_table_source(),
    );
    let mut actual = BTreeSet::new();
    source_files(&root, &root.join("stdlib"), &mut actual)?;
    let unexpected: Vec<_> = actual
        .iter()
        .filter(|path| !expected.contains_key(*path))
        .collect();
    if !unexpected.is_empty() {
        for path in unexpected {
            eprintln!("unexpected generated source: {}", path.display());
        }
        return Err("remove or account for unexpected source files".into());
    }
    let mut drift = false;
    for (path, source) in &expected {
        let target = root.join(path);
        if std::fs::read_to_string(&target).ok().as_deref() == Some(source) {
            continue;
        }
        if write {
            std::fs::create_dir_all(target.parent().unwrap())?;
            std::fs::write(&target, source)?;
            eprintln!("wrote {}", path.display());
        } else {
            drift = true;
            eprintln!("missing or changed: {}", path.display());
        }
    }
    if drift {
        return Err("generated sources differ; run with --write".into());
    }
    println!(
        "{} standard-library modules and embedding table verified",
        aevia_stdlib_model::stdlib::TREE.len()
    );
    Ok(())
}
