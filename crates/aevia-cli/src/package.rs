//! `aevia init`, `add`, `update`, `fetch`, `tree` — the package commands.
//!
//! `add` takes a directory (a path dependency) or `owner/name[@requirement]`
//! (a registry dependency). Registry resolution reads the sparse index cached
//! under `.aevia/registry/index` and never the network by itself: when an
//! index file is missing the command asks `aevia-pkg` (the registry client,
//! see `registry.rs`) to download it, then resolves again. Verified package
//! sources are unpacked into `.aevia/registry/src` by `aevia-pkg vendor`; the
//! compiler reads only those files. `--offline` forbids the network and makes
//! a missing file an error that says which one.

use aevia_core::manifest::Dependency;
use aevia_core::manifest::DirRegistry;
use aevia_core::manifest::LockHints;
use aevia_core::manifest::Manifest;
use aevia_core::manifest::ManifestError;
use aevia_core::manifest::Package;
use aevia_core::manifest::RegistryEnv;
use aevia_core::manifest::canonical_package;
use aevia_core::manifest::import_root;
use aevia_core::manifest::valid_package_name;
use aevia_resolve::Index as _;
use aevia_resolve::Version;
use aevia_resolve::VersionReq;
use std::collections::HashMap;
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
#[allow(
    clippy::single_call_fn,
    reason = "the init command validates the package and writes its initial layout"
)]
pub fn init(args: &[String]) -> ExitCode {
    let Some(name) = args.first() else {
        eprintln!("aevia init: expected a package name, as in `aevia init kinematics`");
        return ExitCode::from(2);
    };
    if let Some(extra) = args.get(1) {
        eprintln!("aevia init: expected one package name, found `{extra}`");
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
        "## {name}\n##\n## `aevia run` runs `main`; `aevia fmt` formats this file.\nfn main(): IO Unit = do\n    println(\"hello from {name}\")\n"
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
#[allow(
    clippy::single_call_fn,
    clippy::too_many_lines,
    reason = "the add command distinguishes local and registry dependencies"
)]
pub fn add(
    args: &[String],
    locked: bool,
) -> ExitCode {
    let mut options = RegistryOptions::default();
    let mut positional: Vec<&String> = Vec::new();
    let mut iter = args.iter();
    while let Some(arg) = iter.next() {
        match arg.as_str() {
            | "--offline" => options.offline = true,
            | "--as" => {
                let Some(name) = iter.next().filter(|value| !value.starts_with('-')) else {
                    eprintln!("aevia add: `--as` expects the import root to use");
                    return ExitCode::from(2);
                };
                options.alias = Some(name.clone());
            },
            | _ => positional.push(arg),
        }
    }
    let Some(path) = positional.first().map(|arg| arg.as_str()) else {
        eprintln!(
            "aevia add: expected a package, as in `aevia add ../units-extra` (a path) or \
             `aevia add apich/units-extra@^1.2` (the registry)"
        );
        return ExitCode::from(2);
    };
    if path.starts_with('-') {
        eprintln!("aevia add: `{path}` is not an option of `aevia add` (`--as NAME`, `--offline`)");
        return ExitCode::from(2);
    }
    if let Some(extra) = positional.get(1) {
        eprintln!("aevia add: unexpected argument `{extra}` (add one package per command)");
        return ExitCode::from(2);
    }
    if let Some(spec) = registry_spec(path) {
        if locked {
            eprintln!(
                "aevia add: `--locked` forbids changing the manifest; re-run without it to add \
                 `{path}`"
            );
            return ExitCode::from(1);
        }
        return add_registry(&spec, &options);
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
    let key = options
        .alias
        .unwrap_or_else(|| import_root(&dependency_manifest.name));
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
            if change_manifest && let Err(restore) = std::fs::rename(&backup_stage, &manifest_path)
            {
                // Keep the backup available if restoration itself fails.
                owned_stages.retain(|path| path != &backup_stage);
                return Err(format!(
                    "cannot install `{}`: {error}; cannot restore manifest: {restore}; original saved at `{}`",
                    lock_path.display(),
                    backup_stage.display()
                ));
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

// ---------------------------------------------------------------- registry

/// Flags of the dependency commands.
#[derive(Default)]
struct RegistryOptions {
    /// `--offline`: never ask `aevia-pkg` to download anything.
    offline: bool,
    /// `--as NAME` (add): the import root.
    alias: Option<String>,
}

/// `owner/name[@requirement]`, as typed.
struct RegistrySpec {
    /// The canonical identity.
    id: String,
    /// The requirement, or why it did not parse.
    req: Result<Option<VersionReq>, String>,
}

/// Recognises a registry package among the arguments of `aevia add`.
///
/// A directory that holds an `aevia.toml` is always a path dependency; anything
/// else that spells `owner/name` (optionally `@requirement`) is a registry
/// package, and a bare `name` is a package of the official scope.
#[allow(
    clippy::single_call_fn,
    reason = "keeps registry coordinates separate from local path handling"
)]
fn registry_spec(arg: &str) -> Option<RegistrySpec> {
    if arg.starts_with(['.', '/', '\\']) || arg.contains('\\') || arg.contains(':') {
        return None;
    }
    let (name, requirement) = match arg.split_once('@') {
        | Some((name, requirement)) => (name, Some(requirement)),
        | None => (arg, None),
    };
    if Path::new(name).join("aevia.toml").is_file() {
        return None;
    }
    let id = if name.contains('/') {
        canonical_package(name)?
    } else {
        if Path::new(name).exists() || !valid_package_name(name) {
            return None;
        }
        canonical_package(&format!("{}/{name}", aevia_core::manifest::OFFICIAL_SCOPE))?
    };
    let req = match requirement {
        | None => Ok(None),
        | Some(text) => {
            VersionReq::parse(text)
                .map(Some)
                .map_err(|error| error.to_string())
        },
    };
    Some(RegistrySpec { id, req })
}

/// Asks `aevia-pkg` to download the index of `package` into `registry`.
#[allow(
    clippy::single_call_fn,
    reason = "maps registry process failures to package resolution diagnostics"
)]
fn fetch_index(
    registry: &DirRegistry,
    package: &str,
) -> Result<(), String> {
    crate::registry::invoke(&[
        "--cache".to_string(),
        registry.root().display().to_string(),
        "index".to_string(),
        package.to_string(),
    ])
}

/// Runs `attempt`; each time it fails because a package's index is not cached
/// (and the network is allowed) the index is downloaded and it runs again.
fn retrying<T>(
    registry: &DirRegistry,
    offline: bool,
    mut attempt: impl FnMut() -> Result<T, ManifestError>,
) -> Result<T, ManifestError> {
    let mut fetched: std::collections::BTreeSet<String> = std::collections::BTreeSet::new();
    loop {
        match attempt() {
            | Err(mut error) if error.missing_index.is_some() => {
                let package = error.missing_index.clone().unwrap_or_default();
                if offline {
                    error
                        .message
                        .push_str(" (`--offline` forbids downloading it)");
                    return Err(error);
                }
                if fetched.len() > 512 || !fetched.insert(package.clone()) {
                    return Err(error);
                }
                if let Err(message) = fetch_index(registry, &package) {
                    error.message = format!("cannot download the index of `{package}`: {message}");
                    error.missing_index = None;
                    return Err(error);
                }
                registry.reload();
            },
            | other => return other,
        }
    }
}

fn lock_hints(package: &Package) -> LockHints {
    LockHints::File(package.dir.join(LOCK))
}

/// Downloads and verifies what the lock names into the registry cache.
fn vendor(
    registry: &DirRegistry,
    lock: &aevia_core::manifest::Lock,
    force: bool,
) -> Result<(), String> {
    if lock.registry_packages().is_empty() {
        return Ok(());
    }
    let mut args = vec![
        "--cache".to_string(),
        registry.root().display().to_string(),
        "vendor".to_string(),
    ];
    if force {
        args.push("--force".to_string());
    }
    crate::registry::invoke(&args)
}

/// The newest release (not a pre-release, not yanked) in a cached index.
#[allow(
    clippy::single_call_fn,
    reason = "selects a compatible active registry release"
)]
fn newest_release(
    registry: &DirRegistry,
    package: &str,
) -> Option<Version> {
    registry
        .summaries(package)
        .ok()?
        .into_iter()
        .filter(|summary| !summary.yanked && !summary.version.is_prerelease())
        .map(|summary| summary.version)
        .max()
}

#[allow(
    clippy::single_call_fn,
    clippy::too_many_lines,
    reason = "keeps the registry validation and manifest update in one transaction"
)]
fn add_registry(
    spec: &RegistrySpec,
    options: &RegistryOptions,
) -> ExitCode {
    let requirement = match &spec.req {
        | Ok(requirement) => requirement.clone(),
        | Err(message) => {
            eprintln!("aevia add: {message}");
            return ExitCode::from(2);
        },
    };
    let mut package = match current() {
        | Ok(package) => package,
        | Err(message) => {
            eprintln!("aevia add: {message}");
            return ExitCode::from(1);
        },
    };
    let registry = package.default_registry();
    // The index of the package itself, so a missing requirement can default to
    // the newest release and a typo fails before anything is written.
    let fetched = retrying(&registry, options.offline, || {
        registry.summaries(&spec.id).map(|_| ()).map_err(|error| {
            let mut failure = ManifestError {
                code: "E0607",
                message: error.message.clone(),
                missing_index: None,
            };
            if error.kind == aevia_resolve::IndexErrorKind::Unavailable {
                failure.missing_index = Some(error.package);
            }
            failure
        })
    });
    if let Err(error) = fetched {
        eprintln!("aevia add: {}", error.render());
        return ExitCode::from(1);
    }
    let req = match requirement {
        | Some(req) => req,
        | None => {
            let Some(newest) = newest_release(&registry, &spec.id) else {
                eprintln!(
                    "aevia add: `{}` has no release that is neither yanked nor a pre-release; \
                     name a requirement such as `{}@>=1.0.0-0`",
                    spec.id, spec.id
                );
                return ExitCode::from(1);
            };
            VersionReq::caret(&newest)
        },
    };
    let name = spec.id.rsplit('/').next().unwrap_or(&spec.id);
    let key = options.alias.clone().unwrap_or_else(|| import_root(name));
    let dependency = Dependency::Registry {
        package: spec.id.clone(),
        req: req.clone(),
    };
    let already_present = match package.manifest.dependencies.get(&key) {
        | Some(existing) if *existing == dependency => true,
        | Some(_) => {
            eprintln!(
                "aevia add: E0606: `{key}` is already a dependency with a different source or \
                 requirement; edit aevia.toml, or pick another import root with `--as`"
            );
            return ExitCode::from(1);
        },
        | None => {
            package
                .manifest
                .dependencies
                .insert(key.clone(), dependency);
            false
        },
    };
    let lock = retrying(&registry, options.offline, || {
        package.resolve_with(&RegistryEnv {
            index: Some(&registry),
            locked: lock_hints(&package),
            require_sources: false,
        })
    });
    let lock = match lock {
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
        println!("added `{key}` = {} {req}", spec.id);
    }
    println!(
        "wrote {} ({} packages)",
        package.dir.join(LOCK).display(),
        lock.packages.len()
    );
    if options.offline {
        println!("not downloaded (--offline); run `aevia fetch` to vendor the sources");
    } else if let Err(message) = vendor(&registry, &lock, false) {
        eprintln!(
            "aevia add: the manifest and lock are updated, but the sources are not vendored: \
             {message}\naevia add: run `aevia fetch` to retry"
        );
        return ExitCode::from(1);
    }
    ExitCode::SUCCESS
}

/// Writes the lock when it changed; with `--locked` a change is an error.
#[allow(
    clippy::single_call_fn,
    reason = "regenerates a lock only after the manifest change succeeds"
)]
fn finish_lock(
    package: &Package,
    lock: &aevia_core::manifest::Lock,
    locked: bool,
) -> ExitCode {
    let path = package.dir.join(LOCK);
    let text = lock.render();
    let previous = std::fs::read_to_string(&path).ok();
    if previous.as_deref() == Some(text.as_str()) {
        println!(
            "{} is up to date ({} packages)",
            path.display(),
            lock.packages.len()
        );
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

/// Splits `--offline`/`--force` from the packages named on the command line.
fn split_flags(
    command: &str,
    args: &[String],
) -> Result<(RegistryOptions, bool, Vec<String>), ExitCode> {
    let mut options = RegistryOptions::default();
    let mut force = false;
    let mut names = Vec::new();
    for arg in args {
        match arg.as_str() {
            | "--offline" => options.offline = true,
            | "--force" => force = true,
            | flag if flag.starts_with('-') => {
                eprintln!("aevia {command}: unknown option `{flag}`");
                return Err(ExitCode::from(2));
            },
            | name => {
                match canonical_package(name) {
                    | Some(id) => names.push(id),
                    | None => {
                        eprintln!(
                            "aevia {command}: `{name}` is not a registry package; write \\
                             `owner/name`"
                        );
                        return Err(ExitCode::from(2));
                    },
                }
            },
        }
    }
    Ok((options, force, names))
}

/// `aevia update [owner/name…]` — with `--locked`, a change is an error rather
/// than a write. Without names every registry package moves to the newest
/// compatible version; with names only those do and the rest stay locked.
#[allow(
    clippy::single_call_fn,
    reason = "the update command refreshes package dependencies and lock state"
)]
pub fn update(
    args: &[String],
    locked: bool,
) -> ExitCode {
    let (options, _, names) = match split_flags("update", args) {
        | Ok(parsed) => parsed,
        | Err(code) => return code,
    };
    let package = match current() {
        | Ok(package) => package,
        | Err(message) => {
            eprintln!("aevia update: {message}");
            return ExitCode::from(1);
        },
    };
    let registry = package.default_registry();
    // Refresh what is cached first, so "newest" means newest today.
    if !options.offline && !locked && registry.root().join("index").is_dir() {
        let mut command = vec![
            "--cache".to_string(),
            registry.root().display().to_string(),
            "update".to_string(),
        ];
        command.extend(names.iter().cloned());
        if let Err(message) = crate::registry::invoke(&command) {
            eprintln!("aevia update: cannot refresh the registry index: {message}");
            return ExitCode::from(1);
        }
        registry.reload();
    }
    let hints = if names.is_empty() {
        LockHints::None
    } else {
        let mut held = package.locked_versions();
        for name in &names {
            held.remove(name);
        }
        LockHints::Map(held)
    };
    let lock = retrying(&registry, options.offline, || {
        package.resolve_with(&RegistryEnv {
            index: Some(&registry),
            locked: hints.clone(),
            require_sources: false,
        })
    });
    let lock = match lock {
        | Ok(lock) => lock,
        | Err(error) => {
            eprintln!("aevia update: {}", error.render());
            return ExitCode::from(1);
        },
    };
    let code = finish_lock(&package, &lock, locked);
    if code != ExitCode::SUCCESS {
        return code;
    }
    if !options.offline
        && !locked
        && let Err(message) = vendor(&registry, &lock, false)
    {
        eprintln!("aevia update: the lock is written, but the sources are not vendored: {message}");
        return ExitCode::from(1);
    }
    ExitCode::SUCCESS
}

/// `aevia fetch [--offline] [--force]` — make `.aevia/registry` hold verified
/// sources for everything `aevia.lock` names (re-resolving first if the lock is
/// stale, without writing it).
#[allow(
    clippy::single_call_fn,
    reason = "the fetch command materializes the verified dependency graph"
)]
pub fn fetch(args: &[String]) -> ExitCode {
    let (options, force, names) = match split_flags("fetch", args) {
        | Ok(parsed) => parsed,
        | Err(code) => return code,
    };
    if !names.is_empty() {
        eprintln!("aevia fetch: expected no packages; it fetches what aevia.lock names");
        return ExitCode::from(2);
    }
    let package = match current() {
        | Ok(package) => package,
        | Err(message) => {
            eprintln!("aevia fetch: {message}");
            return ExitCode::from(1);
        },
    };
    let registry = package.default_registry();
    let lock = retrying(&registry, options.offline, || {
        package.resolve_with(&RegistryEnv {
            index: Some(&registry),
            locked: lock_hints(&package),
            require_sources: false,
        })
    });
    let lock = match lock {
        | Ok(lock) => lock,
        | Err(error) => {
            eprintln!("aevia fetch: {}", error.render());
            return ExitCode::from(1);
        },
    };
    if std::fs::read_to_string(package.dir.join(LOCK))
        .ok()
        .as_deref()
        != Some(lock.render().as_str())
    {
        eprintln!("aevia fetch: note: aevia.lock is out of date; run `aevia update`");
    }
    if lock.registry_packages().is_empty() {
        println!("no registry dependencies");
        return ExitCode::SUCCESS;
    }
    if options.offline {
        // Offline there is nothing to download: say whether the build works.
        return match package.roots_with(&RegistryEnv {
            index: Some(&registry),
            locked: lock_hints(&package),
            require_sources: true,
        }) {
            | Ok(_) => {
                println!(
                    "{} registry package(s) are vendored and verified",
                    lock.registry_packages().len()
                );
                ExitCode::SUCCESS
            },
            | Err(error) => {
                eprintln!("aevia fetch: {}", error.render());
                ExitCode::from(1)
            },
        };
    }
    match vendor(&registry, &lock, force) {
        | Ok(()) => ExitCode::SUCCESS,
        | Err(message) => {
            eprintln!("aevia fetch: {message}");
            ExitCode::from(1)
        },
    }
}

/// `aevia tree [--offline]` — the dependency tree: path and registry packages.
#[allow(
    clippy::single_call_fn,
    reason = "the tree command displays resolved dependency relationships"
)]
pub fn tree(args: &[String]) -> ExitCode {
    let (options, _, names) = match split_flags("tree", args) {
        | Ok(parsed) => parsed,
        | Err(code) => return code,
    };
    if !names.is_empty() {
        eprintln!("aevia tree: expected no packages");
        return ExitCode::from(2);
    }
    let package = match current() {
        | Ok(package) => package,
        | Err(message) => {
            eprintln!("aevia tree: {message}");
            return ExitCode::from(1);
        },
    };
    let registry = package.default_registry();
    let tree = retrying(&registry, options.offline, || {
        package.tree(&RegistryEnv {
            index: Some(&registry),
            locked: lock_hints(&package),
            require_sources: false,
        })
    });
    match tree {
        | Ok(text) => {
            print!("{text}");
            ExitCode::SUCCESS
        },
        | Err(error) => {
            eprintln!("aevia tree: {}", error.render());
            ExitCode::from(1)
        },
    }
}

/// `aevia clean` — remove the `.aevia/` build directory of §9.11.
///
/// `aevia check` and `aevia run` write the whole-program replay records and
/// the per-module `.aevi` files (§9.11) under `.aevia/cache/`, next to the
/// entry file: in the package directory for an entry at its root and in
/// `src/` for the conventional `src/main.aev`. The command removes `.aevia/`
/// from both places and otherwise says there is nothing to clean. It never
/// claims to have removed a cache that does not exist.
#[allow(
    clippy::single_call_fn,
    reason = "the clean command applies registry preservation to each cache directory"
)]
pub fn clean(args: &[String]) -> ExitCode {
    let mut with_registry = false;
    for arg in args {
        if arg == "--registry" {
            with_registry = true;
        } else {
            eprintln!("aevia clean: expected no arguments (or `--registry`), found `{arg}`");
            return ExitCode::from(2);
        }
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
    let mut existing = Vec::new();
    for cache in caches {
        match std::fs::symlink_metadata(&cache) {
            | Ok(meta) if meta.is_dir() => existing.push(cache),
            | Ok(_) => {
                eprintln!(
                    "aevia clean: `{}` must be a cache directory",
                    cache.display()
                );
                return ExitCode::from(1);
            },
            | Err(error) if error.kind() == std::io::ErrorKind::NotFound => {},
            | Err(error) => {
                eprintln!("aevia clean: cannot inspect `{}`: {error}", cache.display());
                return ExitCode::from(1);
            },
        }
    }
    if existing.is_empty() {
        println!(
            "nothing to clean: `{}` does not exist (`aevia check` and `aevia run` write the \
             per-module `.aevi` cache there)",
            package.dir.join(".aevia").display()
        );
        return ExitCode::SUCCESS;
    }
    for cache in existing {
        let files = count_files(&cache);
        // `.aevia/registry` holds downloaded, verified packages and their
        // indexes: not a build product, and not recoverable offline. It stays
        // unless `--registry` asks for it.
        let keep = if with_registry {
            None
        } else {
            Some(cache.join("registry")).filter(|dir| dir.exists())
        };
        let result = remove_except(&cache, keep.as_deref());
        match result {
            | Ok(removed) => {
                println!(
                    "removed {} ({removed} of {files} file{}{})",
                    cache.display(),
                    if files == 1 { "" } else { "s" },
                    if keep.as_ref().is_some_and(|dir| dir.exists()) {
                        "; kept registry/, use `aevia clean --registry` to remove it"
                    } else {
                        ""
                    }
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

/// Removes everything under `dir` except `keep` (and the directories on the
/// way to it); returns how many files went.
#[allow(
    clippy::single_call_fn,
    reason = "cache removal and its reported count use the same operation"
)]
fn remove_except(
    dir: &Path,
    keep: Option<&Path>,
) -> std::io::Result<usize> {
    let Some(keep) = keep else {
        let files = count_files(dir);
        std::fs::remove_dir_all(dir)?;
        return Ok(files);
    };
    let mut removed = 0usize;
    for entry in std::fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();
        if path == keep {
            continue;
        }
        let kind = entry.file_type()?;
        if kind.is_dir() {
            removed = removed.saturating_add(count_files(&path));
            std::fs::remove_dir_all(&path)?;
        } else {
            // A Windows junction or directory symlink is removed as a directory.
            // No recursive file count should traverse the link.
            #[cfg(windows)]
            {
                use std::os::windows::fs::FileTypeExt as _;
                if kind.is_symlink_dir() {
                    std::fs::remove_dir(&path)?;
                    removed = removed.saturating_add(1);
                    continue;
                }
            }
            std::fs::remove_file(&path)?;
            removed = removed.saturating_add(1);
        }
    }
    Ok(removed)
}

/// How many files a directory holds, for the `clean` report.
fn count_files(dir: &Path) -> usize {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return 0;
    };
    entries
        .flatten()
        .map(|entry| {
            if entry.file_type().is_ok_and(|kind| kind.is_dir()) {
                count_files(&entry.path())
            } else {
                1
            }
        })
        .sum()
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
/// Patterns sharing a directory use one scan and emit sorted matches in
/// argument order, including repeated matches.
#[must_use]
pub fn expand_globs(names: &[String]) -> Vec<PathBuf> {
    struct Glob<'a> {
        parent: &'a str,
        pattern: &'a str,
        matches: Vec<PathBuf>,
    }

    let has_pattern = |name: &str| name.contains('*') || name.contains('?');
    if names.iter().all(|name| !has_pattern(name)) {
        return names.iter().map(PathBuf::from).collect();
    }
    // Keep one output slot per argument so duplicates and argument order survive grouping.
    let mut arguments: Vec<_> = names
        .iter()
        .map(|name| {
            has_pattern(name).then(|| {
                let (parent, pattern) = name.rsplit_once(['/', '\\']).map_or(
                    (".", name.as_str()),
                    |(parent, pattern)| {
                        let directory =
                            if parent.is_empty() || (cfg!(windows) && parent.ends_with(':')) {
                                name.get(..parent.len().saturating_add(1)).unwrap_or(parent)
                            } else {
                                parent
                            };
                        (directory, pattern)
                    },
                );
                Glob {
                    parent,
                    pattern,
                    matches: Vec::new(),
                }
            })
        })
        .collect();
    let mut directories: HashMap<&Path, Vec<&mut Glob<'_>>> = HashMap::new();
    for glob in arguments.iter_mut().flatten() {
        directories
            .entry(Path::new(glob.parent))
            .or_default()
            .push(glob);
    }
    for (directory, mut globs) in directories {
        let Ok(entries) = std::fs::read_dir(directory) else {
            continue;
        };
        for entry in entries.flatten() {
            let filename = entry.file_name();
            let spelling = filename.to_string_lossy();
            for glob in &mut globs {
                if wildcard_matches(glob.pattern, &spelling) {
                    glob.matches.push(Path::new(glob.parent).join(&filename));
                }
            }
        }
    }
    let mut paths = Vec::new();
    for (name, glob) in names.iter().zip(arguments) {
        if let Some(mut glob) = glob
            && !glob.matches.is_empty()
        {
            glob.matches.sort();
            paths.extend(glob.matches);
        } else {
            paths.push(PathBuf::from(name));
        }
    }
    paths
}

/// Shell-style matching of one file name against `*`/`?`.
#[allow(
    clippy::single_call_fn,
    reason = "keeps filename matching separate from filesystem traversal"
)]
fn wildcard_matches(
    pattern: &str,
    name: &str,
) -> bool {
    let mut pattern = pattern.chars();
    let mut name = name.chars();
    let mut current = name.next();
    let mut star = None;
    while let Some(character) = current {
        let mut next_pattern = pattern.clone();
        match next_pattern.next() {
            | Some('*') => {
                pattern = next_pattern;
                // `name` is already past the scalar currently being compared.
                star = Some((pattern.clone(), name.clone()));
            },
            | Some(next) if next == character || next == '?' => {
                pattern = next_pattern;
                current = name.next();
            },
            | _ => {
                let Some((suffix, consumed)) = &mut star else {
                    return false;
                };
                pattern = suffix.clone();
                current = consumed.next();
                name = consumed.clone();
            },
        }
    }
    pattern.all(|character| character == '*')
}

#[cfg(test)]
mod glob_tests {
    use super::expand_globs;
    use super::wildcard_matches;
    use std::path::Path;
    use std::path::PathBuf;

    struct Scratch(PathBuf);

    impl Scratch {
        fn new(label: &str) -> Self {
            let path = std::env::temp_dir().join(format!(
                "aevia-package-globs-{label}-{}",
                std::process::id()
            ));
            let _ = std::fs::remove_dir_all(&path);
            std::fs::create_dir_all(&path).unwrap();
            Self(path)
        }
    }

    impl Drop for Scratch {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    fn spelling(path: &Path) -> String {
        path.to_string_lossy().into_owned()
    }

    #[test]
    fn directory_groups_keep_argument_order_and_the_spelling_of_each_argument() {
        let scratch = Scratch::new("order");
        let shared = scratch.0.join("shared");
        let other = scratch.0.join("other");
        std::fs::create_dir_all(shared.join("folder.aev")).unwrap();
        std::fs::create_dir(&other).unwrap();
        for entry in ["z.aev", "猫.aev", "x\u{301}.aev", "plain.txt"] {
            std::fs::write(shared.join(entry), "").unwrap();
        }
        std::fs::write(other.join("first.aev"), "").unwrap();

        let literal = scratch.0.join("missing.aev");
        let alias = shared.join(".");
        let glob = spelling(&shared.join("*.aev"));
        let missing = spelling(&shared.join("absent*.aev"));
        let actual = expand_globs(&[
            glob.clone(),
            spelling(&literal),
            spelling(&other.join("*.aev")),
            spelling(&alias.join("?.aev")),
            missing.clone(),
            glob,
        ]);
        let mut shared_matches =
            ["folder.aev", "z.aev", "猫.aev", "x\u{301}.aev"].map(|basename| shared.join(basename));
        shared_matches.sort();
        let mut alias_matches = ["z.aev", "猫.aev"].map(|basename| alias.join(basename));
        alias_matches.sort();
        let expected = shared_matches
            .iter()
            .cloned()
            .chain([literal, other.join("first.aev")])
            .chain(alias_matches)
            .chain([PathBuf::from(missing)])
            .chain(shared_matches.iter().cloned())
            .collect::<Vec<_>>();
        assert_eq!(
            actual
                .iter()
                .map(|path| path.as_os_str())
                .collect::<Vec<_>>(),
            expected
                .iter()
                .map(|path| path.as_os_str())
                .collect::<Vec<_>>()
        );
    }

    #[test]
    fn unavailable_directories_are_retried_by_the_next_expansion() {
        let scratch = Scratch::new("refresh");
        let directory = scratch.0.join("sources");
        let patterns = [
            spelling(&directory.join("*.aev")),
            spelling(&directory.join("?.aev")),
        ];
        let fallback = patterns.each_ref().map(PathBuf::from);
        assert_eq!(expand_globs(&patterns), fallback);

        std::fs::create_dir(&directory).unwrap();
        std::fs::write(directory.join("a.aev"), "").unwrap();
        assert_eq!(
            expand_globs(&patterns),
            [directory.join("a.aev"), directory.join("a.aev")]
        );

        std::fs::rename(&directory, scratch.0.join("previous")).unwrap();
        std::fs::write(&directory, "").unwrap();
        assert_eq!(expand_globs(&patterns), fallback);
        std::fs::remove_file(&directory).unwrap();
        std::fs::create_dir(&directory).unwrap();
        std::fs::write(directory.join("b.aev"), "").unwrap();
        assert_eq!(
            expand_globs(&patterns),
            [directory.join("b.aev"), directory.join("b.aev")]
        );
    }

    #[cfg(unix)]
    #[test]
    fn lossy_matching_keeps_the_native_filename() {
        use std::ffi::OsString;
        use std::os::unix::ffi::OsStringExt as _;

        let scratch = Scratch::new("native");
        let directory = scratch.0.join("literal?component");
        std::fs::create_dir(&directory).unwrap();
        let filename = OsString::from_vec(b"\xff.aev".to_vec());
        let original = directory.join(filename);
        std::fs::write(&original, "").unwrap();
        assert_eq!(
            expand_globs(&[spelling(&directory.join("?.aev"))]),
            [original]
        );
    }

    #[test]
    fn wildcard_backtracking_matches_a_unicode_dynamic_program() {
        fn words(alphabet: &[char]) -> Vec<String> {
            let mut values = vec![String::new()];
            let mut layer = values.clone();
            for _ in 0..4 {
                layer = layer
                    .iter()
                    .flat_map(|prefix| {
                        alphabet
                            .iter()
                            .map(move |character| format!("{prefix}{character}"))
                    })
                    .collect();
                values.extend(layer.iter().cloned());
            }
            values
        }
        fn reference(
            pattern: &str,
            name: &str,
        ) -> bool {
            let name: Vec<_> = name.chars().collect();
            let mut row = vec![false; name.len() + 1];
            row[0] = true;
            for token in pattern.chars() {
                let mut next = vec![false; name.len() + 1];
                next[0] = token == '*' && row[0];
                for (index, &character) in name.iter().enumerate() {
                    next[index + 1] = match token {
                        | '*' => row[index + 1] || next[index],
                        | '?' => row[index],
                        | literal => literal == character && row[index],
                    };
                }
                row = next;
            }
            row[name.len()]
        }
        let filenames = words(&['a', 'é', '\u{301}']);
        for pattern in words(&['a', 'é', '*', '?']) {
            for name in &filenames {
                assert_eq!(
                    wildcard_matches(&pattern, name),
                    reference(&pattern, name),
                    "{pattern:?} against {name:?}"
                );
            }
        }
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
