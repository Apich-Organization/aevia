//! The dependency commands: `index`, `add`, `update`, `vendor`/`fetch`, `tree`
//! and `lock`.
//!
//! # The cache they share with the toolchain
//!
//! ```text
//! <cache>/index/<owner>/<name>                sparse-index text, exactly as served
//! <cache>/src/<owner>/<name>/<version>/       unpacked, verified package
//! <cache>/src/<owner>/<name>/<version>/.aevia-pkg-checksum
//! ```
//!
//! `<cache>` is `--cache`, else `$AEVIA_REGISTRY_DIR`, else `.aevia/registry`
//! beside the `aevia.toml` found from the current directory. `aevia` (the
//! toolchain) reads these files and never the network: it resolves from the
//! index files and builds from the unpacked sources, refusing a source whose
//! marker does not carry the checksum the index records. This client is the
//! only thing that writes them.
//!
//! # Resolution
//!
//! Resolution is `aevia-resolve`, the crate the toolchain links, so a version
//! chosen here is the version `aevia update` chooses. The resolver is
//! synchronous; fetching is not. They are joined by a loop: resolve against
//! the cached index; when the resolver says a package's index is not cached,
//! download it (unless `--offline`) and resolve again.

use std::collections::BTreeMap;
use std::collections::BTreeSet;
use std::path::Path;
use std::path::PathBuf;

use aevia_registry_core::PackageId;
use aevia_registry_core::manifest::PackageManifest;
use aevia_registry_core::tarball;
use aevia_registry_core::vm::human_bytes;
use aevia_resolve::Dependency;
use aevia_resolve::Index;
use aevia_resolve::IndexError;
use aevia_resolve::IndexErrorKind;
use aevia_resolve::Node;
use aevia_resolve::Options;
use aevia_resolve::Resolution;
use aevia_resolve::ResolveError;
use aevia_resolve::Root;
use aevia_resolve::SparseIndex;
use aevia_resolve::Summary;
use aevia_resolve::Version;
use aevia_resolve::VersionReq;

use crate::App;
use crate::error::CliError;
use crate::fetch;
use crate::package;

/// The file that records which archive was unpacked into a source directory.
pub const MARKER: &str = ".aevia-pkg-checksum";

/// The most index files one command will download.
const MAX_FETCHES: usize = 1024;

/// The cache directory.
#[derive(Clone, Debug)]
pub struct Cache {
    root: PathBuf,
}

impl Cache {
    /// Opens the cache at `explicit`, else `$AEVIA_REGISTRY_DIR`, else
    /// `.aevia/registry` next to the package found from `start`.
    #[must_use]
    pub fn locate(
        explicit: Option<&Path>,
        start: &Path,
    ) -> Self {
        if let Some(root) = explicit {
            return Self {
                root: root.to_path_buf(),
            };
        }
        if let Some(shared) = std::env::var_os("AEVIA_REGISTRY_DIR")
            && !shared.is_empty()
        {
            return Self {
                root: PathBuf::from(shared),
            };
        }
        let base = package::find_root(start).unwrap_or_else(|| start.to_path_buf());
        Self {
            root: base.join(".aevia").join("registry"),
        }
    }

    /// A cache rooted exactly at `root`.
    #[must_use]
    pub fn at(root: PathBuf) -> Self {
        Self { root }
    }

    /// The cache root.
    #[must_use]
    pub fn root(&self) -> &Path {
        &self.root
    }

    fn parts(
        &self,
        base: &str,
        id: &PackageId,
    ) -> PathBuf {
        self.root
            .join(base)
            .join(id.owner())
            .join(id.name_key())
    }

    /// Where the index text of a package is kept.
    #[must_use]
    pub fn index_file(
        &self,
        id: &PackageId,
    ) -> PathBuf {
        self.parts("index", id)
    }

    /// Where a version is unpacked.
    #[must_use]
    pub fn source_dir(
        &self,
        id: &PackageId,
        version: &str,
    ) -> PathBuf {
        self.parts("src", id).join(version)
    }

    /// Stores the index text of a package, replacing any older copy.
    ///
    /// # Errors
    ///
    /// A filesystem failure.
    pub fn write_index(
        &self,
        id: &PackageId,
        text: &str,
    ) -> Result<(), CliError> {
        let path = self.index_file(id);
        write_atomically(&path, text.as_bytes())
    }

    /// The cached index text, if any.
    #[must_use]
    pub fn read_index(
        &self,
        id: &PackageId,
    ) -> Option<String> {
        std::fs::read_to_string(self.index_file(id)).ok()
    }

    /// The packages whose index is cached, sorted.
    #[must_use]
    pub fn cached_packages(&self) -> Vec<PackageId> {
        let mut out = Vec::new();
        let Ok(owners) = std::fs::read_dir(self.root.join("index")) else {
            return out;
        };
        for owner in owners.flatten() {
            let Ok(names) = std::fs::read_dir(owner.path()) else {
                continue;
            };
            for name in names.flatten() {
                let key = format!(
                    "{}/{}",
                    owner.file_name().to_string_lossy(),
                    name.file_name().to_string_lossy()
                );
                if let Ok(id) = key.parse::<PackageId>() {
                    out.push(id);
                }
            }
        }
        out.sort();
        out
    }
}

fn write_atomically(
    path: &Path,
    bytes: &[u8],
) -> Result<(), CliError> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|error| {
            CliError::new(format!("cannot create `{}`: {error}", parent.display()))
        })?;
    }
    let temporary = path.with_extension("partial");
    std::fs::write(&temporary, bytes).map_err(|error| {
        CliError::new(format!("cannot write `{}`: {error}", temporary.display()))
    })?;
    std::fs::rename(&temporary, path)
        .map_err(|error| CliError::new(format!("cannot move `{}`: {error}", path.display())))
}

/// An [`Index`] over the cache directory.
pub struct CacheIndex<'a> {
    cache: &'a Cache,
    loaded: std::cell::RefCell<SparseIndex>,
}

impl<'a> CacheIndex<'a> {
    /// A fresh view of the cache.
    #[must_use]
    pub fn new(cache: &'a Cache) -> Self {
        Self {
            cache,
            loaded: std::cell::RefCell::new(SparseIndex::new()),
        }
    }

    /// The loaded records, for checksums after a resolution.
    #[must_use]
    pub fn record(
        &self,
        package: &str,
        version: &Version,
    ) -> Option<aevia_resolve::SparseRecord> {
        self.loaded.borrow().record(package, version).cloned()
    }
}

impl Index for CacheIndex<'_> {
    fn summaries(
        &self,
        package: &str,
    ) -> Result<Vec<Summary>, IndexError> {
        if self.loaded.borrow().records(package).is_none() {
            let id = package
                .parse::<PackageId>()
                .map_err(|_| IndexError::missing(package))?;
            let text = self.cache.read_index(&id).ok_or_else(|| {
                IndexError::unavailable(
                    package,
                    format!("the index of `{package}` is not cached"),
                )
            })?;
            self.loaded
                .borrow_mut()
                .insert_text(package, &text)
                .map_err(|error| {
                    IndexError::unavailable(
                        package,
                        format!("the cached index of `{package}` is damaged: {error}"),
                    )
                })?;
        }
        self.loaded.borrow().summaries(package)
    }
}

/// Downloads the index of `id` into the cache.
///
/// # Errors
///
/// The registry's error, or a filesystem failure.
pub async fn fetch_index(
    app: &App,
    cache: &Cache,
    id: &PackageId,
) -> Result<(), CliError> {
    let text = app.client.index_text(id.owner(), id.name()).await?;
    // Parse before storing, so a damaged response never replaces a good file.
    aevia_resolve::sparse::parse_text(&text).map_err(|error| {
        CliError::new(format!("the registry's index of `{id}` is damaged: {error}"))
    })?;
    cache.write_index(id, &text)
}

/// What a resolution needs besides the requirements.
pub struct Plan<'a> {
    /// The cache to read and fill.
    pub cache: &'a Cache,
    /// No network: a package that is not cached is an error.
    pub offline: bool,
    /// Download the index of every package the resolution touches, even when a
    /// cached copy exists (`update`).
    pub refresh: bool,
    /// Versions to prefer.
    pub locked: BTreeMap<String, BTreeSet<Version>>,
}

/// Resolves `dependencies` (label, package, requirement), fetching indexes as
/// the resolver asks for them.
///
/// # Errors
///
/// An unsatisfiable resolution, or an index that cannot be had.
pub async fn resolve_closure(
    app: &App,
    plan: &Plan<'_>,
    dependencies: Vec<Dependency>,
) -> Result<(Resolution, BTreeMap<String, aevia_resolve::SparseRecord>), CliError> {
    let roots = [Root {
        id: "this package".to_string(),
        dependencies,
    }];
    let options = Options {
        locked: plan.locked.clone(),
        ..Options::default()
    };
    let mut refreshed: BTreeSet<String> = BTreeSet::new();
    let mut fetches = 0;
    loop {
        // A refresh re-downloads each touched package once, before the first
        // resolution that would read it.
        let index = CacheIndex::new(plan.cache);
        match aevia_resolve::resolve(&roots, &index, &options) {
            | Ok(resolution) => {
                let mut stale = Vec::new();
                if plan.refresh && !plan.offline {
                    for chosen in &resolution.packages {
                        if refreshed.insert(chosen.package.clone()) {
                            stale.push(chosen.package.clone());
                        }
                    }
                }
                if stale.is_empty() {
                    let mut records = BTreeMap::new();
                    for chosen in &resolution.packages {
                        if let Some(record) = index.record(&chosen.package, &chosen.version) {
                            records.insert(format!("{} {}", chosen.package, chosen.version), record);
                        }
                    }
                    return Ok((resolution, records));
                }
                for package in stale {
                    let id: PackageId = package.parse().map_err(CliError::from)?;
                    fetch_index(app, plan.cache, &id).await?;
                    fetches += 1;
                }
            },
            | Err(ResolveError::Index(error)) if error.kind == IndexErrorKind::Unavailable => {
                if plan.offline {
                    return Err(CliError::new(format!(
                        "{} (offline)",
                        error.message
                    ))
                    .with_next_step("run the command again without `--offline`"));
                }
                fetches += 1;
                if fetches > MAX_FETCHES {
                    return Err(CliError::new(format!(
                        "the requirements reach more than {MAX_FETCHES} packages"
                    )));
                }
                let id: PackageId = error.package.parse().map_err(CliError::from)?;
                fetch_index(app, plan.cache, &id).await?;
            },
            | Err(other) => {
                return Err(CliError::new(format!("E0606: {other}")));
            },
        }
    }
}

/// The dependencies of `./aevia.toml` that name registry packages.
fn manifest_dependencies(manifest: &PackageManifest) -> Result<Vec<Dependency>, CliError> {
    manifest
        .dependencies
        .iter()
        .filter_map(|dependency| {
            let package = dependency.package.clone()?;
            let req = dependency.req.clone()?;
            Some((dependency.import.clone(), package, req))
        })
        .map(|(label, package, req)| {
            let req = VersionReq::parse(&req)
                .map_err(|error| CliError::new(format!("dependency `{label}`: {error}")))?;
            Ok(Dependency {
                label,
                package,
                req,
            })
        })
        .collect()
}

/// The registry versions an `aevia.lock` names, as `(package, version,
/// checksum)`. A deliberately small reader: the lock is the toolchain's file,
/// and this client needs only the registry entries.
#[must_use]
pub fn lock_registry_entries(text: &str) -> BTreeSet<(String, String, String)> {
    let mut out = BTreeSet::new();
    let mut fields: BTreeMap<String, String> = BTreeMap::new();
    let mut flush = |fields: &mut BTreeMap<String, String>| {
        if fields
            .get("source")
            .is_some_and(|source| source.starts_with("registry+"))
            && let (Some(name), Some(version)) = (fields.get("name"), fields.get("version"))
        {
            out.insert((
                name.clone(),
                version.clone(),
                fields.get("checksum").cloned().unwrap_or_default(),
            ));
        }
        fields.clear();
    };
    for line in text.lines() {
        let line = line.trim();
        if line == "[[package]]" {
            flush(&mut fields);
        } else if let Some((key, value)) = line.split_once('=') {
            let value = value.trim().trim_matches('"');
            fields.insert(key.trim().to_string(), value.to_string());
        }
    }
    flush(&mut fields);
    out
}

fn locked_hints(lock_path: &Path) -> BTreeMap<String, BTreeSet<Version>> {
    let mut out: BTreeMap<String, BTreeSet<Version>> = BTreeMap::new();
    if let Ok(text) = std::fs::read_to_string(lock_path) {
        for (name, version, _) in lock_registry_entries(&text) {
            if let Ok(version) = Version::parse(&version) {
                out.entry(name).or_default().insert(version);
            }
        }
    }
    out
}

fn here() -> Result<PathBuf, CliError> {
    std::env::current_dir()
        .map_err(|error| CliError::new(format!("cannot read the current directory: {error}")))
}

/// Unpacks a verified version into the cache unless it is already there.
///
/// # Errors
///
/// A verification failure, a checksum that disagrees with the index, or a
/// filesystem failure.
pub async fn vendor_version(
    app: &App,
    cache: &Cache,
    id: &PackageId,
    version: &str,
    expected: &str,
    force: bool,
) -> Result<bool, CliError> {
    let dest = cache.source_dir(id, version);
    if !force
        && let Ok(marker) = std::fs::read_to_string(dest.join(MARKER))
        && marker
            .lines()
            .next()
            .is_some_and(|first| first.trim().eq_ignore_ascii_case(expected))
        && dest.join("aevia.toml").is_file()
    {
        return Ok(false);
    }
    let verified = fetch::verify(app, id, version).await?;
    if !expected.is_empty() && !verified.checksum.eq_ignore_ascii_case(expected) {
        return Err(CliError::new(format!(
            "`{id}@{version}` downloads as {}, but the index (and the lock) say {expected}",
            verified.checksum
        ))
        .with_next_step("the registry changed a published version; do not use it"));
    }
    if dest.exists() {
        std::fs::remove_dir_all(&dest).map_err(|error| {
            CliError::new(format!("cannot replace `{}`: {error}", dest.display()))
        })?;
    }
    let written = tarball::unpack(&verified.bytes, &dest).map_err(CliError::from)?;
    std::fs::write(
        dest.join(MARKER),
        format!("{}\n{} {}\n", verified.checksum, verified.id, verified.version),
    )
    .map_err(|error| CliError::new(format!("cannot record the checksum: {error}")))?;
    if !app.quiet {
        println!(
            "vendored {} {} ({} files, {}), signed by {} {}",
            verified.id,
            verified.version,
            written.len(),
            human_bytes(verified.bytes.len() as u64),
            verified.key_kind,
            verified.fingerprint
        );
    }
    Ok(true)
}

async fn vendor_resolution(
    app: &App,
    cache: &Cache,
    resolution: &Resolution,
    records: &BTreeMap<String, aevia_resolve::SparseRecord>,
    force: bool,
) -> Result<usize, CliError> {
    let mut count = 0;
    for chosen in &resolution.packages {
        let id: PackageId = chosen.package.parse().map_err(CliError::from)?;
        let expected = records
            .get(&chosen.to_string())
            .map(|record| record.checksum.clone())
            .unwrap_or_default();
        if vendor_version(app, cache, &id, &chosen.version.to_string(), &expected, force).await? {
            count += 1;
        }
    }
    Ok(count)
}

fn print_resolution(
    resolution: &Resolution,
    records: &BTreeMap<String, aevia_resolve::SparseRecord>,
    json: bool,
) {
    if json {
        let packages: Vec<serde_json::Value> = resolution
            .packages
            .iter()
            .map(|chosen| {
                serde_json::json!({
                    "package": chosen.package,
                    "version": chosen.version.to_string(),
                    "checksum": records.get(&chosen.to_string()).map(|r| r.checksum.clone()),
                })
            })
            .collect();
        println!("{}", serde_json::json!({ "packages": packages }));
        return;
    }
    for chosen in &resolution.packages {
        let checksum = records
            .get(&chosen.to_string())
            .map(|record| record.checksum.as_str())
            .unwrap_or("?");
        println!("{} {}  {checksum}", chosen.package, chosen.version);
    }
}

// ----------------------------------------------------------------- commands

/// `aevia-pkg index <owner/name>…`: download index files into the cache.
///
/// # Errors
///
/// The first download that fails.
pub async fn index_command(
    app: &App,
    cache: &Cache,
    targets: &[String],
) -> Result<(), CliError> {
    for target in targets {
        let (id, _) = package::parse_requirement_target(target)?;
        fetch_index(app, cache, &id).await?;
        if !app.quiet {
            println!("indexed {id} into `{}`", cache.index_file(&id).display());
        }
    }
    Ok(())
}

/// `aevia-pkg add owner/name[@requirement]`: resolve, download, verify and
/// vendor a package and its dependencies, then print the line for
/// `aevia.toml`.
///
/// # Errors
///
/// An unsatisfiable requirement, or a download that does not verify.
pub async fn add(
    app: &App,
    cache: &Cache,
    target: &str,
    alias: Option<&str>,
) -> Result<(), CliError> {
    let (id, requested) = package::parse_requirement_target(target)?;
    let label = alias
        .map(str::to_string)
        .unwrap_or_else(|| package::import_root(id.name()));
    let key = id.key();
    let plan = Plan {
        cache,
        offline: app.offline,
        refresh: false,
        locked: BTreeMap::new(),
    };
    // Without a requirement, ask for the newest release and write `^that`.
    let probe = requested.clone().unwrap_or_else(VersionReq::any);
    let (resolution, records) = resolve_closure(
        app,
        &plan,
        vec![Dependency {
            label: label.clone(),
            package: key.clone(),
            req: probe,
        }],
    )
    .await?;
    let chosen = resolution
        .packages
        .iter()
        .filter(|chosen| chosen.package == key)
        .map(|chosen| chosen.version.clone())
        .max()
        .ok_or_else(|| CliError::new(format!("nothing was chosen for `{id}`")))?;
    let written = requested.unwrap_or_else(|| VersionReq::caret(&chosen));
    let count = vendor_resolution(app, cache, &resolution, &records, false).await?;
    if !app.quiet {
        println!(
            "{} {} and {} other package(s) are in `{}` ({count} newly vendored)",
            id,
            chosen,
            resolution.packages.len().saturating_sub(1),
            cache.root().display()
        );
        println!();
        println!("add this to `aevia.toml`:");
        println!();
    }
    if aevia_resolve::official_package(&label) == key {
        println!("{label} = \"{written}\"");
    } else {
        println!("{label} = {{ package = \"{key}\", version = \"{written}\" }}");
    }
    Ok(())
}

/// `aevia-pkg vendor` / `fetch`: make the cache hold everything `aevia.lock`
/// names (or the listed `owner/name@version` targets), verified.
///
/// # Errors
///
/// A missing lock, or a download that does not verify.
pub async fn vendor(
    app: &App,
    cache: &Cache,
    targets: &[String],
    force: bool,
) -> Result<(), CliError> {
    let mut wanted: BTreeSet<(String, String, String)> = BTreeSet::new();
    if targets.is_empty() {
        let root = package::find_root(&here()?).ok_or_else(|| {
            CliError::new("no `aevia.toml` in the current directory or any parent")
                .with_next_step("run inside a package, or name `owner/name@version` targets")
        })?;
        let lock = root.join("aevia.lock");
        let text = std::fs::read_to_string(&lock).map_err(|error| {
            CliError::new(format!("cannot read `{}`: {error}", lock.display()))
                .with_next_step("run `aevia update` to write it")
        })?;
        wanted = lock_registry_entries(&text);
    } else {
        for target in targets {
            let (id, version) = package::parse_target(target)?;
            let version = version.ok_or_else(|| {
                CliError::usage(format!("`{target}` names no version; write `{id}@1.2.3`"))
            })?;
            wanted.insert((id.key(), version, String::new()));
        }
    }
    let mut vendored = 0;
    for (name, version, checksum) in &wanted {
        let id: PackageId = name.parse().map_err(CliError::from)?;
        if vendor_version(app, cache, &id, version, checksum, force).await? {
            vendored += 1;
        }
        // The index too, so the toolchain can resolve offline afterwards.
        if cache.read_index(&id).is_none() && !app.offline {
            fetch_index(app, cache, &id).await?;
        }
    }
    if !app.quiet {
        println!(
            "{} package(s) verified in `{}` ({vendored} downloaded)",
            wanted.len(),
            cache.root().display()
        );
    }
    Ok(())
}

/// `aevia-pkg update [owner/name…]`: refresh cached indexes and report which
/// versions are new.
///
/// # Errors
///
/// A download failure.
pub async fn update(
    app: &App,
    cache: &Cache,
    targets: &[String],
) -> Result<(), CliError> {
    let ids: Vec<PackageId> = if targets.is_empty() {
        cache.cached_packages()
    } else {
        targets
            .iter()
            .map(|target| package::parse_requirement_target(target).map(|(id, _)| id))
            .collect::<Result<_, _>>()?
    };
    if ids.is_empty() {
        println!("nothing is cached in `{}`", cache.root().display());
        return Ok(());
    }
    let versions_of = |text: Option<String>| -> BTreeSet<String> {
        text.and_then(|text| aevia_resolve::sparse::parse_text(&text).ok())
            .map(|records| {
                records
                    .into_iter()
                    .map(|record| {
                        format!("{}{}", record.version, if record.yanked { " (yanked)" } else { "" })
                    })
                    .collect()
            })
            .unwrap_or_default()
    };
    for id in ids {
        let before = versions_of(cache.read_index(&id));
        fetch_index(app, cache, &id).await?;
        let after = versions_of(cache.read_index(&id));
        let added: Vec<&String> = after.difference(&before).collect();
        if added.is_empty() {
            println!("{id}: up to date");
        } else {
            println!(
                "{id}: {}",
                added.iter().map(|v| format!("+{v}")).collect::<Vec<_>>().join(" ")
            );
        }
    }
    Ok(())
}

fn load_package(start: &Path) -> Result<(PathBuf, PackageManifest), CliError> {
    let package = package::load_from(start)?;
    Ok((package.root, package.manifest))
}

/// `aevia-pkg tree`: the registry dependency tree of `./aevia.toml`.
///
/// # Errors
///
/// An unsatisfiable manifest.
pub async fn tree(
    app: &App,
    cache: &Cache,
) -> Result<(), CliError> {
    let (root, manifest) = load_package(&here()?)?;
    let dependencies = manifest_dependencies(&manifest)?;
    let plan = Plan {
        cache,
        offline: app.offline,
        refresh: false,
        locked: locked_hints(&root.join("aevia.lock")),
    };
    let (resolution, _) = resolve_closure(app, &plan, dependencies).await?;
    print!("{}", render_tree(&manifest.name, &manifest.version, &resolution));
    Ok(())
}

/// Renders a resolution as a tree under the root package; a package reached
/// twice is expanded once and marked `(*)` afterwards.
#[must_use]
pub fn render_tree(
    name: &str,
    version: &str,
    resolution: &Resolution,
) -> String {
    fn walk(
        resolution: &Resolution,
        node: &Node,
        prefix: &str,
        seen: &mut BTreeSet<String>,
        out: &mut String,
    ) {
        let mut edges = resolution.edges_from(node);
        edges.sort_by(|a, b| a.label.cmp(&b.label).then_with(|| a.to.cmp(&b.to)));
        for (position, edge) in edges.iter().enumerate() {
            let last = position + 1 == edges.len();
            let target = Node::Package(edge.to.clone());
            let has_children = !resolution.edges_from(&target).is_empty();
            let repeated = !seen.insert(edge.to.to_string());
            out.push_str(&format!(
                "{prefix}{}{} {} {}{}\n",
                if last { "└── " } else { "├── " },
                edge.label,
                edge.to.package,
                edge.to.version,
                if repeated && has_children { " (*)" } else { "" }
            ));
            if !repeated {
                let next = format!("{prefix}{}", if last { "    " } else { "│   " });
                walk(resolution, &target, &next, seen, out);
            }
        }
    }
    let mut out = format!("{name} {version}\n");
    let mut seen = BTreeSet::new();
    walk(
        resolution,
        &Node::Root("this package".to_string()),
        "",
        &mut seen,
        &mut out,
    );
    out
}

/// `aevia-pkg lock [--check]`: print the registry versions `aevia.toml`
/// resolves to; with `--check`, fail when `aevia.lock` names something else.
///
/// # Errors
///
/// An unsatisfiable manifest, or (with `--check`) a lock that has drifted.
pub async fn lock(
    app: &App,
    cache: &Cache,
    check: bool,
) -> Result<(), CliError> {
    let (root, manifest) = load_package(&here()?)?;
    let dependencies = manifest_dependencies(&manifest)?;
    let lock_path = root.join("aevia.lock");
    let plan = Plan {
        cache,
        offline: app.offline,
        refresh: false,
        // Checking against the lock means resolving the way the toolchain
        // does when it has a lock: preferring what it names.
        locked: locked_hints(&lock_path),
    };
    let (resolution, records) = resolve_closure(app, &plan, dependencies).await?;
    if !check {
        print_resolution(&resolution, &records, app.json);
        return Ok(());
    }
    let expected: BTreeSet<(String, String, String)> = resolution
        .packages
        .iter()
        .map(|chosen| {
            (
                chosen.package.clone(),
                chosen.version.to_string(),
                records
                    .get(&chosen.to_string())
                    .map(|record| record.checksum.clone())
                    .unwrap_or_default(),
            )
        })
        .collect();
    let text = std::fs::read_to_string(&lock_path).map_err(|error| {
        CliError::new(format!("cannot read `{}`: {error}", lock_path.display()))
            .with_next_step("run `aevia update`")
    })?;
    let held = lock_registry_entries(&text);
    let problems = lock_drift(&expected, &held);
    if problems.is_empty() {
        println!("aevia.lock matches the registry ({} package(s))", expected.len());
        Ok(())
    } else {
        Err(CliError::new(format!(
            "aevia.lock does not match the registry index:\n  {}",
            problems.join("\n  ")
        ))
        .with_next_step("run `aevia update`; a changed checksum for a locked version means the registry rewrote it"))
    }
}

/// Differences between the packages a resolution wants and the lock's.
#[must_use]
pub fn lock_drift(
    expected: &BTreeSet<(String, String, String)>,
    held: &BTreeSet<(String, String, String)>,
) -> Vec<String> {
    let mut problems = Vec::new();
    for (name, version, checksum) in expected {
        match held
            .iter()
            .find(|(held_name, held_version, _)| held_name == name && held_version == version)
        {
            | None => problems.push(format!("missing from the lock: {name} {version}")),
            | Some((_, _, held_checksum)) if held_checksum != checksum => {
                problems.push(format!(
                    "{name} {version}: the lock says {held_checksum}, the index says {checksum}"
                ));
            },
            | Some(_) => {},
        }
    }
    for (name, version, _) in held {
        if !expected
            .iter()
            .any(|(n, v, _)| n == name && v == version)
        {
            problems.push(format!("in the lock but not required: {name} {version}"));
        }
    }
    problems
}

#[cfg(test)]
mod tests {
    use super::*;

    const LOCK: &str = "version = 1\n\n[[package]]\nname = \"app\"\nversion = \"0.1.0\"\nimport = \"App\"\nsource = \"root\"\n\n[[package]]\nname = \"apich/plot\"\nversion = \"2.1.0\"\nimport = \"Plot\"\nsource = \"registry+aevia\"\nchecksum = \"sha256:aa\"\n\n[[package]]\nname = \"dep\"\nversion = \"0.2.0\"\nimport = \"Dep\"\nsource = \"path+../dep\"\nhash = \"fnv1a64:00\"\n\n[[package]]\nname = \"apich/units\"\nversion = \"1.2.0\"\nimport = \"Units\"\nsource = \"registry+aevia\"\nchecksum = \"sha256:bb\"\n";

    #[test]
    fn only_registry_entries_are_read_from_a_lock() {
        let entries = lock_registry_entries(LOCK);
        let names: Vec<&str> = entries.iter().map(|(n, _, _)| n.as_str()).collect();
        assert_eq!(names, ["apich/plot", "apich/units"]);
        assert!(entries.contains(&(
            "apich/units".to_string(),
            "1.2.0".to_string(),
            "sha256:bb".to_string()
        )));
    }

    #[test]
    fn drift_is_reported_in_both_directions() {
        let held = lock_registry_entries(LOCK);
        assert!(lock_drift(&held, &held).is_empty());
        let mut expected = held.clone();
        expected.remove(&(
            "apich/units".to_string(),
            "1.2.0".to_string(),
            "sha256:bb".to_string(),
        ));
        expected.insert((
            "apich/units".to_string(),
            "1.3.0".to_string(),
            "sha256:cc".to_string(),
        ));
        expected.insert((
            "apich/plot".to_string(),
            "2.1.0".to_string(),
            "sha256:zz".to_string(),
        ));
        expected.remove(&(
            "apich/plot".to_string(),
            "2.1.0".to_string(),
            "sha256:aa".to_string(),
        ));
        let problems = lock_drift(&expected, &held);
        assert!(problems.iter().any(|p| p.contains("missing from the lock: apich/units 1.3.0")));
        assert!(problems.iter().any(|p| p.contains("apich/plot 2.1.0") && p.contains("sha256:zz")));
        assert!(problems.iter().any(|p| p.contains("not required: apich/units 1.2.0")));
    }

    fn line(
        name: &str,
        version: &str,
        deps: &str,
    ) -> String {
        format!(
            "{{\"name\":\"{name}\",\"vers\":\"{version}\",\"deps\":[{deps}],\"cksum\":\"sha256:{}\"}}\n",
            "0".repeat(64)
        )
    }

    #[test]
    fn a_cache_index_resolves_offline_and_reports_what_it_lacks() {
        let dir = std::env::temp_dir().join(format!("aevia-pkg-cache-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let cache = Cache::at(dir.clone());
        let plot: PackageId = "apich/plot".parse().unwrap();
        let units: PackageId = "apich/units".parse().unwrap();
        cache
            .write_index(
                &plot,
                &(line("apich/plot", "1.0.0", "{\"package\":\"apich/units\",\"req\":\"^1\"}")
                    + &line("apich/plot", "1.1.0", "{\"package\":\"apich/units\",\"req\":\"^1.2\"}")),
            )
            .unwrap();
        let roots = [Root {
            id: "this package".to_string(),
            dependencies: vec![Dependency::new("apich/plot", VersionReq::parse("^1").unwrap())],
        }];
        let index = CacheIndex::new(&cache);
        let error = aevia_resolve::resolve(&roots, &index, &Options::default()).expect_err("units missing");
        assert_eq!(error.unavailable_package(), Some("apich/units"));
        cache
            .write_index(
                &units,
                &(line("apich/units", "1.0.0", "") + &line("apich/units", "1.2.0", "")),
            )
            .unwrap();
        let index = CacheIndex::new(&cache);
        let resolution = aevia_resolve::resolve(&roots, &index, &Options::default()).expect("resolves");
        let tree = render_tree("app", "0.1.0", &resolution);
        assert!(tree.contains("apich/plot apich/plot 1.1.0"), "{tree}");
        assert!(tree.contains("Units apich/units 1.2.0"), "{tree}");
        assert_eq!(cache.cached_packages().len(), 2);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_tree_marks_a_repeated_subtree() {
        let mut index = aevia_resolve::MemoryIndex::new();
        index
            .add("o/a", "1.0.0", &[("o/c", "^1")])
            .add("o/b", "1.0.0", &[("o/c", "^1")])
            .add("o/c", "1.0.0", &[("o/d", "^1")])
            .add("o/d", "1.0.0", &[]);
        let roots = [Root {
            id: "this package".to_string(),
            dependencies: vec![
                Dependency::new("o/a", VersionReq::parse("^1").unwrap()),
                Dependency::new("o/b", VersionReq::parse("^1").unwrap()),
            ],
        }];
        let resolution = aevia_resolve::resolve(&roots, &index, &Options::default()).unwrap();
        let tree = render_tree("app", "0.1.0", &resolution);
        assert_eq!(tree.matches("o/d 1.0.0").count(), 1, "{tree}");
        assert!(tree.contains("(*)"), "{tree}");
    }
}
