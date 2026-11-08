//! `aevia init`, `aevia add`, `aevia update` — the package commands.
//!
//! Only path dependencies exist: there is no registry, so `add` takes a
//! directory, and `update` re-resolves that directory's `aevia.lock`. Nothing
//! here pretends a version range was resolved.

use aevia_core::manifest::Dependency;
use aevia_core::manifest::Manifest;
use aevia_core::manifest::Package;
use aevia_core::manifest::import_root;
use aevia_core::manifest::valid_package_name;
use std::path::Path;
use std::path::PathBuf;
use std::process::ExitCode;

const LOCK: &str = "aevia.lock";

/// The package the current directory belongs to.
fn current() -> Result<Package, String> {
    let cwd = std::env::current_dir()
        .map_err(|error| format!("cannot read the current directory: {error}"))?;
    Package::find(&cwd)
        .map_err(|error| error.render())?
        .ok_or_else(|| {
            format!(
                "no `aevia.toml` in `{}` or any parent; run `aevia init <name>` first",
                cwd.display()
            )
        })
}

/// `aevia init <name>`
pub fn init(args: &[String]) -> ExitCode {
    let Some(name) = args.first() else {
        eprintln!("aevia init: expected a package name, as in `aevia init kinematics`");
        return ExitCode::from(2);
    };
    if args.len() > 1 {
        eprintln!("aevia init: expected one package name, found `{}`", args[1]);
        return ExitCode::from(2);
    }
    if !valid_package_name(name) {
        eprintln!(
            "aevia init: `{name}` is not a package name; use lower-case letters, digits and `_`"
        );
        return ExitCode::from(2);
    }
    let dir = PathBuf::from(name);
    if dir.exists() {
        // Overwriting an existing directory would destroy whatever is in it,
        // so this is a refusal rather than a prompt.
        eprintln!(
            "aevia init: `{}` already exists; refusing to overwrite it",
            dir.display()
        );
        return ExitCode::from(1);
    }
    let manifest = Manifest::new(name);
    if let Err(error) = std::fs::create_dir_all(dir.join("src")) {
        eprintln!(
            "aevia init: cannot create `{}`: {error}",
            dir.join("src").display()
        );
        return ExitCode::from(1);
    }
    let main = dir.join(&manifest.entry);
    let source = format!(
        "## {name}\n##\n## `aevia run` runs `main`; `aevia fmt` formats this file.\n\nfn main(): IO Unit = do\n    println(\"hello from {name}\")\n"
    );
    for (path, text) in [
        (dir.join("aevia.toml"), manifest.render()),
        (main.clone(), source),
    ] {
        if let Err(error) = std::fs::write(&path, text) {
            eprintln!("aevia init: cannot write `{}`: {error}", path.display());
            return ExitCode::from(1);
        }
    }
    let package = Package {
        dir: dir.clone(),
        manifest,
    };
    match package.resolve() {
        | Ok(lock) => {
            if let Err(error) = lock.save(&dir.join(LOCK)) {
                eprintln!("aevia init: {}", error.render());
                return ExitCode::from(1);
            }
        },
        | Err(error) => {
            eprintln!("aevia init: {}", error.render());
            return ExitCode::from(1);
        },
    }
    println!("created {}", dir.join("aevia.toml").display());
    println!("created {}", main.display());
    println!("created {}", dir.join(LOCK).display());
    ExitCode::SUCCESS
}

/// `aevia add <path>` (with `--locked`, adding a dependency is refused).
pub fn add(
    args: &[String],
    locked: bool,
) -> ExitCode {
    let Some(path) = args.first() else {
        eprintln!(
            "aevia add: expected the path of a package, as in `aevia add ../units-extra`\n\
             aevia add: there is no registry — only path dependencies are resolved"
        );
        return ExitCode::from(2);
    };
    if path.starts_with('-') {
        eprintln!(
            "aevia add: `{path}` looks like an option; aevia add takes a directory, and there \
             is no registry to add a version range from"
        );
        return ExitCode::from(2);
    }
    if locked {
        // `--locked` promises the manifest and lock are inputs. `add` would
        // change both.
        eprintln!(
            "aevia add: `--locked` forbids changing the manifest; re-run without it to add `{path}`"
        );
        return ExitCode::from(1);
    }
    let mut package = match current() {
        | Ok(package) => package,
        | Err(message) => {
            eprintln!("aevia add: {message}");
            return ExitCode::from(1);
        },
    };
    let dependency_dir = package.dir.join(path);
    let dependency_manifest = match Manifest::load(&dependency_dir.join("aevia.toml")) {
        | Ok(manifest) => manifest,
        | Err(error) => {
            eprintln!(
                "aevia add: `{}` is not a package ({})",
                dependency_dir.display(),
                error.render()
            );
            return ExitCode::from(1);
        },
    };
    let key = import_root(&dependency_manifest.name);
    let dependency = Dependency::Path {
        path: path.replace('\\', "/"),
    };
    let already_present = if let Some(existing) = package.manifest.dependencies.get(&key) {
        if *existing != dependency {
            eprintln!("aevia add: E0606: `{key}` is already a dependency with a different path");
            return ExitCode::from(1);
        }
        true
    } else {
        package
            .manifest
            .dependencies
            .insert(key.clone(), dependency);
        false
    };
    // Resolve the complete proposed graph before touching either input file.
    // Missing transitive packages, alias conflicts and unreadable source trees
    // must leave the original manifest and lock byte-for-byte intact.
    let lock = match package.resolve() {
        | Ok(lock) => lock,
        | Err(error) => {
            eprintln!("aevia add: {}", error.render());
            return ExitCode::from(1);
        },
    };
    if let Err(error) = commit_package(&package, &lock.render(), !already_present) {
        eprintln!("aevia add: {error}");
        return ExitCode::from(1);
    }
    if already_present {
        println!("`{key}` is already a dependency");
    } else {
        println!(
            "added `{key}` ({} {}) from `{path}`",
            dependency_manifest.name, dependency_manifest.version
        );
    }
    println!(
        "wrote {} ({} packages)",
        package.dir.join(LOCK).display(),
        lock.packages.len()
    );
    ExitCode::SUCCESS
}

/// Stage both outputs before replacing either. If installing the lock fails,
/// restore the original manifest. This is an ordinary I/O failure transaction;
/// it does not promise cross-file atomicity across process or machine crashes.
fn commit_package(
    package: &Package,
    lock: &str,
    change_manifest: bool,
) -> Result<(), String> {
    use std::io::Write as _;
    let manifest_path = package.dir.join("aevia.toml");
    let lock_path = package.dir.join(LOCK);
    let old_manifest = std::fs::read(&manifest_path).map_err(|error| error.to_string())?;
    let suffix = format!(".aevia-add-{}", std::process::id());
    let manifest_stage = package.dir.join(format!("{suffix}.toml"));
    let lock_stage = package.dir.join(format!("{suffix}.lock"));
    let backup_stage = package.dir.join(format!("{suffix}.backup"));
    let mut owned_stages = Vec::new();
    let result = (|| {
        let mut stage = |path: &Path, bytes: &[u8]| -> Result<(), String> {
            let mut file = std::fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(path)
                .map_err(|error| format!("cannot stage `{}`: {error}", path.display()))?;
            owned_stages.push(path.to_path_buf());
            file.write_all(bytes)
                .and_then(|()| file.sync_all())
                .map_err(|error| format!("cannot stage `{}`: {error}", path.display()))
        };
        if change_manifest {
            stage(&manifest_stage, package.manifest.render().as_bytes())?;
            stage(&backup_stage, &old_manifest)?;
            // Preserve permissions when a manifest is installed or restored.
            let permissions = std::fs::metadata(&manifest_path)
                .map_err(|error| error.to_string())?
                .permissions();
            std::fs::set_permissions(&manifest_stage, permissions.clone())
                .map_err(|error| error.to_string())?;
            std::fs::set_permissions(&backup_stage, permissions)
                .map_err(|error| error.to_string())?;
        }
        stage(&lock_stage, lock.as_bytes())?;
        if change_manifest {
            std::fs::rename(&manifest_stage, &manifest_path).map_err(|error| {
                format!("cannot install `{}`: {error}", manifest_path.display())
            })?;
        }
        if let Err(error) = std::fs::rename(&lock_stage, &lock_path) {
            if change_manifest {
                if let Err(restore) = std::fs::rename(&backup_stage, &manifest_path) {
                    // Keep the backup available if restoration itself fails.
                    owned_stages.retain(|path| path != &backup_stage);
                    return Err(format!(
                        "cannot install `{}`: {error}; cannot restore manifest: {restore}; original saved at `{}`",
                        lock_path.display(),
                        backup_stage.display()
                    ));
                }
            }
            return Err(format!("cannot install `{}`: {error}", lock_path.display()));
        }
        Ok(())
    })();
    for path in owned_stages {
        let _ = std::fs::remove_file(path);
    }
    result
}

/// `aevia update` — with `--locked`, a change is an error rather than a write.
pub fn update(
    args: &[String],
    locked: bool,
) -> ExitCode {
    if let Some(extra) = args.first() {
        eprintln!("aevia update: expected no arguments, found `{extra}`");
        return ExitCode::from(2);
    }
    let package = match current() {
        | Ok(package) => package,
        | Err(message) => {
            eprintln!("aevia update: {message}");
            return ExitCode::from(1);
        },
    };
    write_lock(&package, true, locked)
}

/// `aevia clean` — remove the `.aevia/` build directory of §9.11.
///
/// `aevia check` and `aevia run` write the whole-program replay records and
/// the per-module `.aevi` files (§9.11) under `.aevia/cache/`, next to the
/// entry file: in the package directory for an entry at its root and in
/// `src/` for the conventional `src/main.aev`. The command removes `.aevia/`
/// from both places and otherwise says there is nothing to clean. It never
/// claims to have removed a cache that does not exist.
pub fn clean(args: &[String]) -> ExitCode {
    if let Some(extra) = args.first() {
        eprintln!("aevia clean: expected no arguments, found `{extra}`");
        return ExitCode::from(2);
    }
    let package = match current() {
        | Ok(package) => package,
        | Err(message) => {
            eprintln!("aevia clean: {message}");
            return ExitCode::from(1);
        },
    };
    // The cache lives next to the entry file: in the package directory, and in
    // the entry's own directory (`src/` for the conventional `src/main.aev`).
    let mut caches = vec![package.dir.join(".aevia")];
    if let Some(entry_dir) = package.entry_path().parent() {
        let beside_entry = entry_dir.join(".aevia");
        if !caches.contains(&beside_entry) {
            caches.push(beside_entry);
        }
    }
    caches.retain(|cache| cache.exists());
    if caches.is_empty() {
        println!(
            "nothing to clean: `{}` does not exist (`aevia check` and `aevia run` write the \
             per-module `.aevi` cache there)",
            package.dir.join(".aevia").display()
        );
        return ExitCode::SUCCESS;
    }
    for cache in caches {
        let files = count_files(&cache);
        match std::fs::remove_dir_all(&cache) {
            | Ok(()) => {
                println!(
                    "removed {} ({files} file{})",
                    cache.display(),
                    if files == 1 { "" } else { "s" }
                );
            },
            | Err(error) => {
                eprintln!("aevia clean: cannot remove `{}`: {error}", cache.display());
                return ExitCode::from(1);
            },
        }
    }
    ExitCode::SUCCESS
}

/// How many files a directory holds, for the `clean` report.
fn count_files(dir: &Path) -> usize {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return 0;
    };
    entries
        .flatten()
        .map(|entry| {
            let path = entry.path();
            if path.is_dir() {
                count_files(&path)
            } else {
                1
            }
        })
        .sum()
}

/// Resolves the lock and writes it when it changed.
fn write_lock(
    package: &Package,
    report_unchanged: bool,
    locked: bool,
) -> ExitCode {
    let lock = match package.resolve() {
        | Ok(lock) => lock,
        | Err(error) => {
            eprintln!("aevia update: {}", error.render());
            return ExitCode::from(1);
        },
    };
    let path = package.dir.join(LOCK);
    let text = lock.render();
    let previous = std::fs::read_to_string(&path).ok();
    if previous.as_deref() == Some(text.as_str()) {
        if report_unchanged {
            println!(
                "{} is up to date ({} packages)",
                path.display(),
                lock.packages.len()
            );
        }
        return ExitCode::SUCCESS;
    }
    if locked {
        eprintln!(
            "aevia update: `--locked` refuses to rewrite `{}`; re-run without `--locked`",
            path.display()
        );
        return ExitCode::from(1);
    }
    if let Err(error) = lock.save(&path) {
        eprintln!("aevia update: {}", error.render());
        return ExitCode::from(1);
    }
    println!(
        "wrote {} ({} packages)",
        path.display(),
        lock.packages.len()
    );
    ExitCode::SUCCESS
}

/// Every `.aev` file under a directory, sorted, for `fmt` and `doc`.
#[must_use]
pub fn sources_under(dir: &Path) -> Vec<PathBuf> {
    let mut files = Vec::new();
    collect(dir, &mut files);
    files.sort();
    files
}

/// Expands a `*`/`?` pattern in the last component of a file argument.
///
/// A POSIX shell expands `examples/*.aev` before the program starts; Windows
/// PowerShell passes the pattern to a native executable verbatim, so the tool
/// expands it itself. Only the last component is a pattern: the directory part
/// is a directory, which keeps the rule simple and the errors honest. A
/// pattern that matches nothing is left as it was, so the file-not-found
/// diagnostic names exactly what the user wrote.
#[must_use]
pub fn expand_globs(names: &[String]) -> Vec<PathBuf> {
    let mut paths = Vec::new();
    for name in names {
        if !name.contains('*') && !name.contains('?') {
            paths.push(PathBuf::from(name));
            continue;
        }
        let (parent, pattern) = match name.rfind(['/', '\\']) {
            | Some(index) => (&name[..index], &name[index + 1..]),
            | None => (".", name.as_str()),
        };
        let Ok(entries) = std::fs::read_dir(if parent.is_empty() {
            "."
        } else {
            parent
        }) else {
            paths.push(PathBuf::from(name));
            continue;
        };
        let mut matches: Vec<PathBuf> = entries
            .flatten()
            .filter(|entry| {
                wildcard_matches(
                    &pattern.chars().collect::<Vec<_>>(),
                    &entry
                        .file_name()
                        .to_string_lossy()
                        .chars()
                        .collect::<Vec<_>>(),
                )
            })
            .map(|entry| PathBuf::from(parent).join(entry.file_name()))
            .collect();
        if matches.is_empty() {
            paths.push(PathBuf::from(name));
        } else {
            matches.sort();
            paths.extend(matches);
        }
    }
    paths
}

/// Shell-style matching of one file name against `*`/`?`.
fn wildcard_matches(
    pattern: &[char],
    name: &[char],
) -> bool {
    match pattern.split_first() {
        | None => name.is_empty(),
        | Some(('*', rest)) => (0..=name.len()).any(|start| wildcard_matches(rest, &name[start..])),
        | Some(('?', rest)) => !name.is_empty() && wildcard_matches(rest, &name[1..]),
        | Some((character, rest)) => {
            name.first() == Some(character) && wildcard_matches(rest, &name[1..])
        },
    }
}

fn collect(
    dir: &Path,
    out: &mut Vec<PathBuf>,
) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect(&path, out);
        } else if path.extension().is_some_and(|extension| extension == "aev") {
            out.push(path);
        }
    }
}
