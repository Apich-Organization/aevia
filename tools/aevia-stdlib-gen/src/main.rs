//! Regenerate or check standard-library sources and the embedding table.
//! Usage: cargo run -p aevia-stdlib-gen -- --check|--write [--root DIR]
//! `--check` reports drift. `--write` preflights output paths and replaces changed files.
#![deny(
    missing_docs,
    clippy::arithmetic_side_effects,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::nursery,
    clippy::pedantic,
    clippy::single_call_fn,
    clippy::unwrap_used
)]

mod support;

use std::collections::BTreeMap;
use std::collections::BTreeSet;
use std::fs;
use std::fs::Permissions;
use std::io;
use std::io::Read as _;
use std::io::Write as _;
use std::path::Path;
use std::path::PathBuf;
use std::sync::atomic::AtomicUsize;
use std::sync::atomic::Ordering;

use support::file_error;
use support::regular_files;

fn has_manifest(dir: &Path) -> io::Result<bool> {
    let manifest = dir.join("Cargo.toml");
    match fs::metadata(&manifest) {
        | Ok(metadata) => Ok(metadata.is_file()),
        | Err(error)
            if matches!(
                error.kind(),
                io::ErrorKind::NotFound | io::ErrorKind::NotADirectory
            ) =>
        {
            Ok(false)
        },
        | Err(error) => {
            Err(file_error(
                "cannot inspect Cargo manifest",
                &manifest,
                &error,
            ))
        },
    }
}

fn require_directory(path: &Path) -> io::Result<bool> {
    match fs::symlink_metadata(path) {
        | Ok(metadata) if metadata.is_dir() => Ok(true),
        | Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(false),
        | Err(error) => Err(file_error("cannot inspect output directory", path, &error)),
        | Ok(_) => {
            Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                format!("expected an output directory at `{}`", path.display()),
            ))
        },
    }
}

struct Change<'a> {
    relative: &'a Path,
    source: &'a str,
    permissions: Option<Permissions>,
}

#[expect(
    clippy::single_call_fn,
    reason = "preflight all targets before changing any output"
)]
fn pending_changes<'a>(
    root: &Path,
    expected: &'a BTreeMap<PathBuf, String>,
) -> io::Result<Vec<Change<'a>>> {
    let mut directories = BTreeSet::new();
    let mut parents = Vec::new();
    let mut bytes = Vec::new();
    let mut changes = Vec::new();
    for (relative, source) in expected {
        // Inspect ancestors in order so an intermediate link is never followed.
        parents.extend(
            relative
                .ancestors()
                .skip(1)
                .take_while(|dir| !directories.contains(dir)),
        );
        while let Some(dir) = parents.pop() {
            if !dir.as_os_str().is_empty() && directories.insert(dir) {
                require_directory(&root.join(dir))?;
            }
        }
        let target = root.join(relative);
        let permissions = match fs::symlink_metadata(&target) {
            | Ok(metadata) if metadata.is_file() => {
                let file = fs::File::open(&target)
                    .map_err(|error| file_error("cannot open output", &target, &error))?;
                let length = u64::try_from(source.len()).map_err(io::Error::other)?;
                if metadata.len() == length {
                    bytes.clear();
                    file.take(length.saturating_add(1))
                        .read_to_end(&mut bytes)
                        .map_err(|error| file_error("cannot read output", &target, &error))?;
                    if bytes == source.as_bytes() {
                        continue;
                    }
                }
                Some(metadata.permissions())
            },
            | Err(error) if error.kind() == io::ErrorKind::NotFound => None,
            | Err(error) => return Err(file_error("cannot inspect output", &target, &error)),
            | Ok(_) => {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidInput,
                    format!("expected a regular output file at `{}`", target.display()),
                ));
            },
        };
        changes.push(Change {
            relative,
            source,
            permissions,
        });
    }
    Ok(changes)
}

#[expect(
    clippy::single_call_fn,
    reason = "isolates staging, replacement and cleanup"
)]
fn replace_output(
    target: &Path,
    change: Change<'_>,
) -> io::Result<()> {
    static NEXT_STAGE: AtomicUsize = AtomicUsize::new(0);
    let parent = target
        .parent()
        .ok_or_else(|| io::Error::other("generated target has no parent directory"))?;
    fs::create_dir_all(parent)
        .map_err(|error| file_error("cannot create output directory", parent, &error))?;
    let (path, mut stage) = loop {
        let id = NEXT_STAGE.fetch_add(1, Ordering::Relaxed);
        let path = parent.join(format!(".aevia-stdlib-gen-{}-{id}.tmp", std::process::id()));
        match fs::File::create_new(&path) {
            | Ok(stage) => break (path, stage),
            | Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {},
            | Err(error) => return Err(file_error("cannot stage output", &path, &error)),
        }
    };
    let prepared = stage.write_all(change.source.as_bytes()).and_then(|()| {
        if let Some(permissions) = change.permissions {
            stage.set_permissions(permissions)?;
        }
        Ok(())
    });
    drop(stage);
    let replaced = prepared
        .map_err(|error| file_error("cannot prepare output", target, &error))
        .and_then(|()| {
            fs::rename(&path, target)
                .map_err(|error| file_error("cannot replace output", target, &error))
        });
    if replaced.is_err() {
        let _ = fs::remove_file(path);
    }
    replaced
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = std::env::args_os().skip(1);
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
    let root = if has_manifest(&root)? && has_manifest(&nested)? {
        require_directory(&root.join("crates"))?;
        require_directory(&nested)?;
        nested
    } else {
        root
    };
    let write = mode == "--write";
    let spec_path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../spec/08-stdlib.md");
    let spec = fs::read_to_string(&spec_path)
        .map_err(|error| file_error("cannot read chapter 8", &spec_path, &error))?;
    let crates_dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../crates");
    let mut expected: BTreeMap<PathBuf, String> =
        support::docs::module_sources(&spec, &crates_dir)?
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
    let stdlib_dir = root.join("stdlib");
    let mut unexpected = false;
    if require_directory(&stdlib_dir)? {
        for entry in regular_files(&stdlib_dir, "aev")? {
            let path = entry.strip_prefix(&root)?;
            if !expected.contains_key(path) {
                unexpected = true;
                eprintln!("unexpected generated source: {}", path.display());
            }
        }
    }
    if unexpected {
        return Err("remove or account for unexpected source files".into());
    }
    let changes = pending_changes(&root, &expected)?;
    let drift = !changes.is_empty();
    for change in changes {
        let path = change.relative;
        if write {
            replace_output(&root.join(path), change)?;
            eprintln!("wrote {}", path.display());
        } else {
            eprintln!("missing or changed: {}", path.display());
        }
    }
    if !write && drift {
        return Err("generated sources differ; run with --write".into());
    }
    println!(
        "{} standard-library modules and embedding table verified",
        aevia_stdlib_model::stdlib::TREE.len()
    );
    Ok(())
}
