//! `aevia.toml`, `aevia.lock`, and the package roots an import resolves in.
//!
//! A package is a directory with a manifest, a `src/` tree and a lock file
//! (§7.5). A dependency is one of
//!
//! * a **path** dependency, `Lib = { path = "../lib" }`, found on disk;
//! * a **registry** dependency, `Units = "^1.2"` or
//!   `Plot = { package = "apich/plot", version = "^2.1" }`, chosen by the
//!   resolver (`aevia-resolve`) from the registry's sparse index and read from
//!   a vendored, verified copy under `.aevia/registry/`;
//! * both, `Dev = { path = "../dev", version = "^0.3" }`: the path while the
//!   package is a local checkout, the requirement once it is a registry package
//!   (a published manifest keeps its text, so a path that does not exist for
//!   the consumer must not be needed).
//!
//! The lock file records every package the build uses (name, version, source
//! and a digest) and is written by [`Lock::render`] in a fixed order with fixed
//! field order, so re-resolving an unchanged manifest produces a byte-identical
//! file and a diff always means something changed.
//!
//! The reader is a deliberately small subset of TOML: `[table]` headers,
//! `key = "string"` and inline tables `{ path = "...", version = "..." }`, with
//! `#` comments and blank lines. Anything outside that subset is `E0605` with
//! the line number — a manifest the tool only half-understands is worse than
//! one it rejects.

use std::cell::RefCell;
use std::collections::BTreeMap;
use std::collections::BTreeSet;
use std::fmt::Write as _;
use std::path::Path;
use std::path::PathBuf;

use aevia_resolve::SparseIndex;
use aevia_resolve::SparseRecord;
use aevia_resolve::Version;
use aevia_resolve::VersionReq;

pub use aevia_resolve::names::OFFICIAL_SCOPE;
pub use aevia_resolve::names::canonical_package;
pub use aevia_resolve::names::official_package;

/// The `source` of a registry package in `aevia.lock`: the registry's logical
/// name (`config.json`'s `registry`), not a URL, so a mirror does not change the
/// lock.
pub const REGISTRY_SOURCE: &str = "registry+aevia";

/// A manifest or resolution failure, with the diagnostic code it reports.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ManifestError {
    /// The §7.7 code: `E0605` malformed manifest, `E0606` conflict, `E0607`
    /// missing package.
    pub code: &'static str,
    /// What went wrong, with the file and line when there is one.
    pub message: String,
    /// When the failure is "this package's registry index is not available
    /// locally", the package, so a caller that may use the network can fetch it
    /// and try again.
    pub missing_index: Option<String>,
}

impl ManifestError {
    fn new(
        code: &'static str,
        message: impl Into<String>,
    ) -> Self {
        Self {
            code,
            message: message.into(),
            missing_index: None,
        }
    }

    /// The code and message as one line, for a command-line report.
    #[must_use]
    pub fn render(&self) -> String {
        format!("{}: {}", self.code, self.message)
    }
}

/// How a dependency is found.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Dependency {
    /// `{ path = "../units-extra" }` — relative to the manifest's directory.
    Path {
        /// The path as written, kept verbatim so the lock can quote it.
        path: String,
    },
    /// `"^1.2"` or `{ package = "apich/plot", version = "^2.1" }`.
    Registry {
        /// The registry identity, `owner/name`, canonical spelling.
        package: String,
        /// Which versions are acceptable.
        req: VersionReq,
    },
    /// `{ path = "../dev", package = "apich/dev", version = "^0.3" }`.
    Both {
        /// The local checkout, used while this package is built from source.
        path: String,
        /// The registry identity, used once this package is itself a registry
        /// package (and recorded in the index when it is published).
        package: String,
        /// Which versions are acceptable.
        req: VersionReq,
    },
}

impl Dependency {
    /// The local path, when the dependency has one.
    #[must_use]
    pub fn local_path(&self) -> Option<&str> {
        match self {
            | Self::Path { path } | Self::Both { path, .. } => Some(path),
            | Self::Registry { .. } => None,
        }
    }

    /// The registry identity and requirement, when the dependency has them.
    #[must_use]
    pub fn registry(&self) -> Option<(&str, &VersionReq)> {
        match self {
            | Self::Registry { package, req } | Self::Both { package, req, .. } => {
                Some((package, req))
            },
            | Self::Path { .. } => None,
        }
    }
}

/// The contents of `aevia.toml`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Manifest {
    /// `[package] name` — the package name, `snake_case`.
    pub name: String,
    /// `[package] version` — SemVer, `major.minor.patch[-pre]`.
    pub version: String,
    /// `[package] entry` — the executable entry, relative to the package root.
    pub entry: String,
    /// `[dependencies]` — import root to dependency, sorted by key.
    pub dependencies: BTreeMap<String, Dependency>,
}

impl Manifest {
    /// A manifest for a new package.
    #[must_use]
    pub fn new(name: &str) -> Self {
        Self {
            name: name.to_string(),
            version: "0.1.0".to_string(),
            entry: "src/main.aev".to_string(),
            dependencies: BTreeMap::new(),
        }
    }

    /// The package version, parsed (`0.0.0` for a manifest that was built by
    /// hand with an invalid string; [`Manifest::parse`] validates it).
    #[must_use]
    pub fn semver(&self) -> Version {
        Version::parse(&self.version).unwrap_or_else(|_| Version::new(0, 0, 0))
    }

    /// Renders the manifest in the canonical form `aevia init`/`add` write.
    #[must_use]
    pub fn render(&self) -> String {
        let mut out = String::new();
        out.push_str(
            "# aevia.toml — package manifest. `aevia update` rewrites aevia.lock, not this file.\n",
        );
        out.push_str("[package]\n");
        let _ = writeln!(out, "name = \"{}\"", self.name);
        let _ = writeln!(out, "version = \"{}\"", self.version);
        let _ = writeln!(out, "entry = \"{}\"", self.entry);
        out.push('\n');
        out.push_str("[dependencies]\n");
        for (key, dependency) in &self.dependencies {
            match dependency {
                | Dependency::Path { path } => {
                    let _ = writeln!(out, "{key} = {{ path = \"{path}\" }}");
                },
                | Dependency::Registry { package, req } => {
                    if *package == official_package(key) {
                        let _ = writeln!(out, "{key} = \"{req}\"");
                    } else {
                        let _ = writeln!(
                            out,
                            "{key} = {{ package = \"{package}\", version = \"{req}\" }}"
                        );
                    }
                },
                | Dependency::Both { path, package, req } => {
                    if *package == official_package(key) {
                        let _ =
                            writeln!(out, "{key} = {{ path = \"{path}\", version = \"{req}\" }}");
                    } else {
                        let _ = writeln!(
                            out,
                            "{key} = {{ path = \"{path}\", package = \"{package}\", version = \
                             \"{req}\" }}"
                        );
                    }
                },
            }
        }
        out
    }

    /// Parses a manifest.
    ///
    /// # Errors
    ///
    /// `E0605` for anything outside the supported subset, a missing required
    /// key, an invalid name, version or version requirement.
    pub fn parse(
        text: &str,
        origin: &str,
    ) -> Result<Self, ManifestError> {
        let mut table = String::new();
        let mut name: Option<String> = None;
        let mut version: Option<String> = None;
        let mut entry: Option<String> = None;
        let mut dependencies: BTreeMap<String, Dependency> = BTreeMap::new();
        for (index, raw) in text.lines().enumerate() {
            let line = strip_comment(raw).trim();
            let number = index + 1;
            if line.is_empty() {
                continue;
            }
            if let Some(header) = line.strip_prefix('[') {
                let Some(header) = header.strip_suffix(']') else {
                    return Err(ManifestError::new(
                        "E0605",
                        format!("{origin}:{number}: unterminated table header `{line}`"),
                    ));
                };
                if !matches!(header, "package" | "dependencies") {
                    return Err(ManifestError::new(
                        "E0605",
                        format!(
                            "{origin}:{number}: unknown table `[{header}]`; this tool reads \
                             `[package]` and `[dependencies]`"
                        ),
                    ));
                }
                table = header.to_string();
                continue;
            }
            let Some((key, value)) = line.split_once('=') else {
                return Err(ManifestError::new(
                    "E0605",
                    format!("{origin}:{number}: expected `key = value`, found `{line}`"),
                ));
            };
            let key = key.trim();
            let value = value.trim();
            match table.as_str() {
                | "package" => {
                    let text = parse_string(value).ok_or_else(|| {
                        ManifestError::new(
                            "E0605",
                            format!("{origin}:{number}: `{key}` expects a quoted string"),
                        )
                    })?;
                    match key {
                        | "name" => name = Some(text),
                        | "version" => version = Some(text),
                        | "entry" => entry = Some(text),
                        | other => {
                            return Err(ManifestError::new(
                                "E0605",
                                format!(
                                    "{origin}:{number}: unknown key `{other}` in `[package]`; \
                                     this tool reads `name`, `version` and `entry`"
                                ),
                            ));
                        },
                    }
                },
                | "dependencies" => {
                    if !is_module_segment(key) {
                        return Err(ManifestError::new(
                            "E0605",
                            format!(
                                "{origin}:{number}: `{key}` is not a module name; a dependency's \
                                 key is the root module it is imported under, so it must start \
                                 with an upper-case letter and contain no `-`"
                            ),
                        ));
                    }
                    let dependency = parse_dependency(key, value, origin, number)?;
                    if dependencies.insert(key.to_string(), dependency).is_some() {
                        return Err(ManifestError::new(
                            "E0605",
                            format!("{origin}:{number}: `{key}` is declared twice"),
                        ));
                    }
                },
                | _ => {
                    return Err(ManifestError::new(
                        "E0605",
                        format!("{origin}:{number}: `{line}` appears before any table header"),
                    ));
                },
            }
        }
        let name = name.ok_or_else(|| {
            ManifestError::new("E0605", format!("{origin}: `[package] name` is missing"))
        })?;
        if !is_package_name(&name) {
            return Err(ManifestError::new(
                "E0605",
                format!(
                    "{origin}: `{name}` is not a package name; use lower-case letters, digits \
                     and `_`"
                ),
            ));
        }
        let version = version.ok_or_else(|| {
            ManifestError::new("E0605", format!("{origin}: `[package] version` is missing"))
        })?;
        if let Err(error) = Version::parse(&version) {
            return Err(ManifestError::new(
                "E0605",
                format!(
                    "{origin}: `{version}` is not a SemVer version ({}); write \
                     `major.minor.patch`, optionally with a `-pre-release`",
                    error.reason
                ),
            ));
        }
        Ok(Self {
            name,
            version,
            entry: entry.unwrap_or_else(|| "src/main.aev".to_string()),
            dependencies,
        })
    }

    /// Reads a manifest from disk.
    ///
    /// # Errors
    ///
    /// `E0607` when the file is missing, `E0605` when it does not parse.
    pub fn load(path: &Path) -> Result<Self, ManifestError> {
        let text = std::fs::read_to_string(path).map_err(|error| {
            ManifestError::new(
                "E0607",
                format!("cannot read `{}`: {error}", path.display()),
            )
        })?;
        Self::parse(&text, &path.display().to_string())
    }

    /// Writes the manifest back in canonical form.
    ///
    /// # Errors
    ///
    /// The I/O error, with the path, when the file cannot be written.
    pub fn save(
        &self,
        path: &Path,
    ) -> Result<(), ManifestError> {
        std::fs::write(path, self.render()).map_err(|error| {
            ManifestError::new(
                "E0605",
                format!("cannot write `{}`: {error}", path.display()),
            )
        })
    }
}

/// A manifest together with the directory it was found in.
#[derive(Clone, Debug)]
pub struct Package {
    /// The directory holding `aevia.toml`.
    pub dir: PathBuf,
    /// The manifest itself.
    pub manifest: Manifest,
}

impl Package {
    /// Walks up from `start` looking for `aevia.toml`.
    ///
    /// `start` may be a file or a directory.
    ///
    /// # Errors
    ///
    /// `E0605` when a manifest exists but does not parse.
    pub fn find(start: &Path) -> Result<Option<Self>, ManifestError> {
        let mut dir = if start.is_dir() {
            start.to_path_buf()
        } else {
            start
                .parent()
                .map_or_else(|| PathBuf::from("."), Path::to_path_buf)
        };
        loop {
            let candidate = dir.join("aevia.toml");
            if candidate.is_file() {
                let manifest = Manifest::load(&candidate)?;
                return Ok(Some(Self { dir, manifest }));
            }
            if !dir.pop() {
                return Ok(None);
            }
        }
    }

    /// The package that owns `file`, when the file is inside its `src/` tree.
    ///
    /// A manifest above a file that is *not* under `src/` is deliberately
    /// ignored: `aevia run examples/modules/main.aev` from inside a package
    /// must keep resolving imports next to the example, exactly as it does
    /// without a manifest. That is the behaviour a file run without a manifest
    /// has today, and generalising package roots must not break it.
    ///
    /// # Errors
    ///
    /// `E0605` when a manifest exists but does not parse.
    /// The package that owns `file`, when the file is one of its modules.
    ///
    /// A package's module trees are `src/`, `tests/` and `examples/` (§7.5.3),
    /// so a test imports the package's modules by their `src/` path. A manifest
    /// above a file that is in none of them is deliberately ignored:
    /// `aevia run examples/modules/main.aev` from inside a package must keep
    /// resolving imports next to the example, exactly as it does without a
    /// manifest. That is the behaviour a file run without a manifest has today,
    /// and generalising package roots must not break it.
    ///
    /// # Errors
    ///
    /// `E0605` when a manifest exists but does not parse.
    pub fn owning(file: &Path) -> Result<Option<Self>, ManifestError> {
        let Some(package) = Self::find(file)? else {
            return Ok(None);
        };
        let file = normalise(file);
        let owns = ["src", "tests", "examples"]
            .iter()
            .any(|tree| file.starts_with(normalise(&package.dir.join(tree))))
            || file == normalise(&package.entry_path());
        if owns {
            Ok(Some(package))
        } else {
            Ok(None)
        }
    }

    /// The `src/` directory, where module paths start.
    #[must_use]
    pub fn source_dir(&self) -> PathBuf {
        self.dir.join("src")
    }

    /// The entry file, `[package] entry` resolved against the package root.
    #[must_use]
    pub fn entry_path(&self) -> PathBuf {
        self.dir.join(&self.manifest.entry)
    }

    /// Resolves the manifest's dependencies, transitively, into import roots.
    ///
    /// A dependency's own `[dependencies]` are followed too, because a module
    /// in a dependency may import a module of *its* dependency. The first
    /// package to claim an import root wins; a second one that claims the same
    /// root from a different directory is `E0606`, since which module an
    /// import names would otherwise depend on traversal order.
    ///
    /// Registry dependencies are read from the vendored copies under
    /// `.aevia/registry/src` (or `$AEVIA_REGISTRY_DIR`), at the versions
    /// `aevia.lock` names; a package whose manifest changed since the lock was
    /// written is re-resolved against the *local* index, never the network.
    ///
    /// # Errors
    ///
    /// `E0606` for a conflicting import root or unsatisfiable requirements,
    /// `E0607` for a missing dependency, a dependency without a manifest, or a
    /// registry package that is not vendored (`aevia fetch`), `E0605` for a
    /// dependency whose manifest does not parse.
    pub fn roots(&self) -> Result<Roots, ManifestError> {
        let registry = self.default_registry();
        self.roots_with(&RegistryEnv {
            index: Some(&registry),
            locked: LockHints::File(self.dir.join("aevia.lock")),
            require_sources: true,
        })
    }

    /// [`Package::roots`] with an explicit registry.
    ///
    /// # Errors
    ///
    /// As [`Package::roots`].
    pub fn roots_with(
        &self,
        env: &RegistryEnv<'_>,
    ) -> Result<Roots, ManifestError> {
        let mut env = env.clone();
        env.require_sources = true;
        let graph = PackageGraph::resolve(self, &env)?;
        let mut roots = Roots {
            source: self.source_dir(),
            local: None,
            packages: BTreeMap::new(),
            entries: BTreeMap::new(),
            scopes: BTreeMap::new(),
            root_owner: Some(graph.root.clone()),
            package_owners: graph.aliases.clone(),
        };
        for (alias, identity) in &graph.aliases {
            let package = &graph.packages[identity];
            roots.packages.insert(alias.clone(), package.source_dir());
            roots.entries.insert(alias.clone(), package.entry_path());
        }
        for (identity, package) in &graph.packages {
            roots.scopes.insert(
                identity.clone(),
                PackageScope {
                    source: package.source_dir(),
                    logical_entry: package.entry_path(),
                    entry: std::fs::canonicalize(package.entry_path())
                        .unwrap_or_else(|_| package.entry_path()),
                    name: import_root(&package.manifest.name),
                    directory: identity.clone(),
                    package: format!(
                        "{}@{}:{}",
                        package.manifest.name,
                        package.manifest.version,
                        identity.display()
                    ),
                },
            );
        }
        Ok(roots)
    }

    /// Resolves the same validated package graph used by the module loader.
    ///
    /// Every import alias is recorded, including multiple aliases of one
    /// package. A path dependency's digest covers its manifest, source tree and
    /// entry file, so changing module resolution invalidates `--locked` too; a
    /// registry package is identified by its archive checksum from the index.
    /// Versions already in `aevia.lock` are kept while they still satisfy the
    /// manifests.
    ///
    /// # Errors
    ///
    /// The same failures as [`Package::roots`] (except a registry package that
    /// is not vendored, which the lock does not need), plus unreadable source
    /// trees.
    pub fn resolve(&self) -> Result<Lock, ManifestError> {
        let registry = self.default_registry();
        self.resolve_with(&RegistryEnv {
            index: Some(&registry),
            locked: LockHints::File(self.dir.join("aevia.lock")),
            require_sources: false,
        })
    }

    /// [`Package::resolve`] with an explicit registry and lock hints (an
    /// `aevia update` passes [`LockHints::None`] to take the newest versions).
    ///
    /// # Errors
    ///
    /// As [`Package::resolve`].
    pub fn resolve_with(
        &self,
        env: &RegistryEnv<'_>,
    ) -> Result<Lock, ManifestError> {
        let mut env = env.clone();
        env.require_sources = false;
        let graph = PackageGraph::resolve(self, &env)?;
        let root = LockedPackage {
            name: self.manifest.name.clone(),
            version: self.manifest.version.clone(),
            import: import_root(&self.manifest.name),
            source: "root".to_string(),
            hash: None,
            checksum: None,
        };
        let mut hashes: BTreeMap<PathBuf, String> = BTreeMap::new();
        let mut packages = Vec::new();
        for (alias, identity) in &graph.aliases {
            if let Some(node) = graph.registry.get(identity) {
                packages.push(LockedPackage {
                    name: node.package.clone(),
                    version: node.version.to_string(),
                    import: alias.clone(),
                    source: REGISTRY_SOURCE.to_string(),
                    hash: None,
                    checksum: Some(node.checksum.clone()),
                });
                continue;
            }
            let package = &graph.packages[identity];
            let hash = match hashes.get(identity) {
                | Some(hash) => hash.clone(),
                | None => {
                    let hash = hash_package(package)?;
                    hashes.insert(identity.clone(), hash.clone());
                    hash
                },
            };
            packages.push(LockedPackage {
                name: package.manifest.name.clone(),
                version: package.manifest.version.clone(),
                import: alias.clone(),
                source: format!("path+{}", relative_to(&graph.root, identity)),
                hash: Some(hash),
                checksum: None,
            });
        }
        packages.sort_by(|left, right| {
            left.source
                .cmp(&right.source)
                .then_with(|| left.import.cmp(&right.import))
        });
        packages.insert(0, root);
        Ok(Lock { packages })
    }

    /// The dependency tree, one package per line, repeated subtrees marked
    /// `(*)`. Path packages show their path, registry packages their identity.
    ///
    /// # Errors
    ///
    /// As [`Package::resolve`].
    pub fn tree(
        &self,
        env: &RegistryEnv<'_>,
    ) -> Result<String, ManifestError> {
        let mut env = env.clone();
        env.require_sources = false;
        let graph = PackageGraph::resolve(self, &env)?;
        let mut out = String::new();
        let mut seen = BTreeSet::new();
        let _ = writeln!(out, "{} {}", self.manifest.name, self.manifest.version);
        graph.render_children(&graph.root, "", &mut seen, &mut out);
        Ok(out)
    }

    /// Where this package's vendored registry sources and index cache live:
    /// `$AEVIA_REGISTRY_DIR`, else `<package>/.aevia/registry`.
    #[must_use]
    pub fn default_registry(&self) -> DirRegistry {
        DirRegistry::for_package(&self.dir)
    }

    /// The registry versions `aevia.lock` currently names (empty when there is
    /// no readable lock).
    #[must_use]
    pub fn locked_versions(&self) -> BTreeMap<String, BTreeSet<Version>> {
        Lock::load(&self.dir.join("aevia.lock"))
            .map(|lock| lock.registry_versions())
            .unwrap_or_default()
    }

    /// Whether this manifest itself (not its dependencies) names a registry
    /// package.
    #[must_use]
    pub fn has_registry_dependencies(&self) -> bool {
        self.manifest
            .dependencies
            .values()
            .any(|dependency| dependency.local_path().is_none())
    }
}

/// Where a resolution gets the lock's versions from.
#[derive(Clone, Debug)]
pub enum LockHints {
    /// No hints: take the newest compatible versions (`aevia update`).
    None,
    /// These versions.
    Map(BTreeMap<String, BTreeSet<Version>>),
    /// Read this lock file if the resolution turns out to need registry data.
    File(PathBuf),
}

/// What a resolution may consult besides the manifests.
#[derive(Clone)]
pub struct RegistryEnv<'a> {
    /// The registry index and vendored sources; `None` makes a registry
    /// dependency an error.
    pub index: Option<&'a dyn RegistryIndex>,
    /// The versions to prefer (and keep even if yanked).
    pub locked: LockHints,
    /// Whether every registry package must be vendored and verified (building)
    /// or only known (writing the lock).
    pub require_sources: bool,
}

impl LockHints {
    fn versions(&self) -> BTreeMap<String, BTreeSet<Version>> {
        match self {
            | Self::None => BTreeMap::new(),
            | Self::Map(map) => map.clone(),
            | Self::File(path) => {
                Lock::load(path)
                    .map(|lock| lock.registry_versions())
                    .unwrap_or_default()
            },
        }
    }
}

/// A registry the resolver can read: the sparse index, and where the verified
/// source of a chosen version lives.
pub trait RegistryIndex: aevia_resolve::Index {
    /// The index record of one version (checksum, dependencies, signature).
    fn record(
        &self,
        package: &str,
        version: &Version,
    ) -> Option<SparseRecord>;

    /// The directory holding the unpacked, verified source of a version.
    fn source_dir(
        &self,
        package: &str,
        version: &Version,
    ) -> PathBuf;
}

/// The file that records which archive was unpacked into a vendored directory.
/// Its first line is the archive's `sha256:` checksum. Written by
/// `aevia-pkg vendor`; the toolchain refuses a vendored directory without it.
pub const VENDOR_MARKER: &str = ".aevia-pkg-checksum";

/// A registry cache on disk:
///
/// ```text
/// <root>/index/<owner>/<name>                 sparse-index text, as served
/// <root>/src/<owner>/<name>/<version>/        unpacked, verified package
/// <root>/src/<owner>/<name>/<version>/.aevia-pkg-checksum
/// ```
///
/// `<root>` is `.aevia/registry` beside `aevia.toml`, or `$AEVIA_REGISTRY_DIR`
/// to share one cache between projects. Index files are read lazily and parsed
/// once.
pub struct DirRegistry {
    root: PathBuf,
    loaded: RefCell<SparseIndex>,
    tried: RefCell<BTreeSet<String>>,
}

impl DirRegistry {
    /// A cache rooted at `root`.
    #[must_use]
    pub fn open(root: PathBuf) -> Self {
        Self {
            root,
            loaded: RefCell::new(SparseIndex::new()),
            tried: RefCell::new(BTreeSet::new()),
        }
    }

    /// The cache for a package directory.
    #[must_use]
    pub fn for_package(dir: &Path) -> Self {
        match std::env::var_os("AEVIA_REGISTRY_DIR") {
            | Some(shared) if !shared.is_empty() => Self::open(PathBuf::from(shared)),
            | _ => Self::open(dir.join(".aevia").join("registry")),
        }
    }

    /// The cache root.
    #[must_use]
    pub fn root(&self) -> &Path {
        &self.root
    }

    /// Where the index text of `package` (`owner/name`) is kept.
    #[must_use]
    pub fn index_file(
        &self,
        package: &str,
    ) -> PathBuf {
        let mut path = self.root.join("index");
        for part in package.split('/') {
            path.push(part.to_ascii_lowercase());
        }
        path
    }

    /// Forgets what was read, so the next lookup reads the files again.
    pub fn reload(&self) {
        *self.loaded.borrow_mut() = SparseIndex::new();
        self.tried.borrow_mut().clear();
    }

    fn ensure(
        &self,
        package: &str,
    ) -> Result<(), aevia_resolve::IndexError> {
        let key = package.to_ascii_lowercase();
        if self.tried.borrow().contains(&key) {
            return if self.loaded.borrow().records(&key).is_some() {
                Ok(())
            } else {
                Err(self.unavailable(&key))
            };
        }
        let path = self.index_file(&key);
        let text = std::fs::read_to_string(&path).map_err(|_| self.unavailable(&key))?;
        match self.loaded.borrow_mut().insert_text(&key, &text) {
            | Ok(()) => {
                self.tried.borrow_mut().insert(key);
                Ok(())
            },
            | Err(error) => {
                Err(aevia_resolve::IndexError::unavailable(
                    &key,
                    format!("the cached index `{}` is damaged ({error})", path.display()),
                ))
            },
        }
    }

    fn unavailable(
        &self,
        package: &str,
    ) -> aevia_resolve::IndexError {
        aevia_resolve::IndexError::unavailable(
            package,
            format!(
                "the registry index of `{package}` is not in `{}`; run `aevia fetch` (or \
                 `aevia update`) with network access",
                self.root.display()
            ),
        )
    }
}

impl aevia_resolve::Index for DirRegistry {
    fn summaries(
        &self,
        package: &str,
    ) -> Result<Vec<aevia_resolve::Summary>, aevia_resolve::IndexError> {
        self.ensure(package)?;
        self.loaded.borrow().summaries(package)
    }
}

impl RegistryIndex for DirRegistry {
    fn record(
        &self,
        package: &str,
        version: &Version,
    ) -> Option<SparseRecord> {
        self.ensure(package).ok()?;
        self.loaded.borrow().record(package, version).cloned()
    }

    fn source_dir(
        &self,
        package: &str,
        version: &Version,
    ) -> PathBuf {
        let mut path = self.root.join("src");
        for part in package.split('/') {
            path.push(part.to_ascii_lowercase());
        }
        path.push(version.to_string());
        path
    }
}

/// A registry package the resolver chose.
#[derive(Clone, Debug)]
struct RegistryNode {
    package: String,
    version: Version,
    checksum: String,
}

/// Package identity is a canonical directory; import names belong to edges,
/// not nodes. Keep this graph shared by lock generation and source resolution.
/// A registry package's identity is its vendored directory, which is also where
/// its source is read from, so everything downstream treats it like a path
/// package.
struct PackageGraph {
    root: PathBuf,
    packages: BTreeMap<PathBuf, Package>,
    aliases: BTreeMap<String, PathBuf>,
    registry: BTreeMap<PathBuf, RegistryNode>,
    edges: BTreeMap<PathBuf, Vec<(String, PathBuf)>>,
}

impl PackageGraph {
    fn resolve(
        root: &Package,
        env: &RegistryEnv<'_>,
    ) -> Result<Self, ManifestError> {
        let identity = package_identity(&root.dir)?;
        let mut graph = Self {
            root: identity.clone(),
            packages: BTreeMap::new(),
            aliases: BTreeMap::new(),
            registry: BTreeMap::new(),
            edges: BTreeMap::new(),
        };
        // Use the supplied manifest, which may be an in-memory proposal from
        // `aevia add`. Never silently reload the old manifest from disk.
        graph.packages.insert(
            identity.clone(),
            Package {
                dir: identity.clone(),
                manifest: root.manifest.clone(),
            },
        );
        let mut queue = vec![identity];
        let mut requests: Vec<RegistryRequest> = Vec::new();
        while let Some(identity) = queue.pop() {
            let package = &graph.packages[&identity];
            let dependencies = package.manifest.dependencies.clone();
            for (alias, dependency) in dependencies {
                let Some(path) = dependency.local_path() else {
                    if let Some((package, req)) = dependency.registry() {
                        requests.push(RegistryRequest {
                            owner: identity.clone(),
                            alias,
                            package: package.to_string(),
                            req: req.clone(),
                        });
                    }
                    continue;
                };
                let dependency_dir = package_identity(&identity.join(path))?;
                graph.claim(&alias, &dependency_dir)?;
                graph
                    .edges
                    .entry(identity.clone())
                    .or_default()
                    .push((alias, dependency_dir.clone()));
                if !graph.packages.contains_key(&dependency_dir) {
                    let manifest = Manifest::load(&dependency_dir.join("aevia.toml"))?;
                    graph.packages.insert(
                        dependency_dir.clone(),
                        Package {
                            dir: dependency_dir.clone(),
                            manifest,
                        },
                    );
                    queue.push(dependency_dir);
                }
            }
        }
        if !requests.is_empty() {
            graph.attach_registry(&requests, env)?;
        }
        Ok(graph)
    }

    /// Records that `alias` names `dir`; a second, different claim is `E0606`.
    fn claim(
        &mut self,
        alias: &str,
        dir: &Path,
    ) -> Result<(), ManifestError> {
        if let Some(existing) = self.aliases.get(alias)
            && existing != dir
        {
            return Err(ManifestError::new(
                "E0606",
                format!(
                    "`{alias}` is claimed by both {} and {}; an import root names one package \
                     in the whole program (rename one dependency in the manifest that imports \
                     it)",
                    self.describe(existing),
                    self.describe(dir)
                ),
            ));
        }
        self.aliases.insert(alias.to_string(), dir.to_path_buf());
        Ok(())
    }

    fn describe(
        &self,
        dir: &Path,
    ) -> String {
        match self.registry.get(dir) {
            | Some(node) => format!("`{} {}`", node.package, node.version),
            | None => format!("`{}`", dir.display()),
        }
    }

    fn attach_registry(
        &mut self,
        requests: &[RegistryRequest],
        env: &RegistryEnv<'_>,
    ) -> Result<(), ManifestError> {
        let Some(index) = env.index else {
            return Err(ManifestError::new(
                "E0607",
                format!(
                    "`{}` is a registry dependency, but no registry is configured",
                    requests[0].package
                ),
            ));
        };
        // One resolver root per package that has registry requirements.
        let mut roots: BTreeMap<PathBuf, aevia_resolve::Root> = BTreeMap::new();
        for request in requests {
            let manifest = &self.packages[&request.owner].manifest;
            let id = if request.owner == self.root {
                manifest.name.clone()
            } else {
                format!(
                    "{} [{}]",
                    manifest.name,
                    relative_to(&self.root, &request.owner)
                )
            };
            roots
                .entry(request.owner.clone())
                .or_insert_with(|| {
                    aevia_resolve::Root {
                        id,
                        dependencies: Vec::new(),
                    }
                })
                .dependencies
                .push(aevia_resolve::Dependency {
                    label: request.alias.clone(),
                    package: request.package.clone(),
                    req: request.req.clone(),
                });
        }
        let owner_of: BTreeMap<String, PathBuf> = roots
            .iter()
            .map(|(dir, root)| (root.id.clone(), dir.clone()))
            .collect();
        let resolver_roots: Vec<aevia_resolve::Root> = roots.values().cloned().collect();
        let options = aevia_resolve::Options {
            locked: env.locked.versions(),
            ..aevia_resolve::Options::default()
        };
        let resolution =
            aevia_resolve::resolve(&resolver_roots, index, &options).map_err(registry_failure)?;

        let mut directories: BTreeMap<aevia_resolve::PackageVersion, PathBuf> = BTreeMap::new();
        for chosen in &resolution.packages {
            let record = index
                .record(&chosen.package, &chosen.version)
                .ok_or_else(|| {
                    ManifestError::new(
                        "E0607",
                        format!(
                            "the index has no record of `{chosen}` (the cache changed while \
                         resolving)"
                        ),
                    )
                })?;
            let source = index.source_dir(&chosen.package, &chosen.version);
            let identity =
                std::fs::canonicalize(&source).unwrap_or_else(|_| normalise(&absolute(&source)));
            if env.require_sources {
                self.load_vendored(chosen, &record, &identity)?;
            }
            self.registry.insert(
                identity.clone(),
                RegistryNode {
                    package: chosen.package.clone(),
                    version: chosen.version.clone(),
                    checksum: record.checksum.clone(),
                },
            );
            directories.insert(chosen.clone(), identity);
        }
        for edge in &resolution.edges {
            let from = match &edge.from {
                | aevia_resolve::Node::Root(id) => owner_of[id].clone(),
                | aevia_resolve::Node::Package(chosen) => directories[chosen].clone(),
            };
            let to = directories[&edge.to].clone();
            self.claim(&edge.label, &to)?;
            self.edges
                .entry(from)
                .or_default()
                .push((edge.label.clone(), to));
        }
        Ok(())
    }

    /// Reads a vendored registry package and checks it is the one the index
    /// describes.
    fn load_vendored(
        &mut self,
        chosen: &aevia_resolve::PackageVersion,
        record: &SparseRecord,
        dir: &Path,
    ) -> Result<(), ManifestError> {
        let hint = "run `aevia fetch` to download and verify it";
        if !dir.join("aevia.toml").is_file() {
            return Err(ManifestError::new(
                "E0607",
                format!(
                    "`{chosen}` is not vendored (`{}` has no aevia.toml); {hint}",
                    dir.display()
                ),
            ));
        }
        let marker = std::fs::read_to_string(dir.join(VENDOR_MARKER)).unwrap_or_default();
        let recorded = marker.lines().next().unwrap_or_default().trim();
        if !recorded.eq_ignore_ascii_case(&record.checksum) {
            return Err(ManifestError::new(
                "E0607",
                format!(
                    "`{}` was not unpacked from the verified archive {} ({}); {hint}",
                    dir.display(),
                    record.checksum,
                    if recorded.is_empty() {
                        "it has no checksum marker".to_string()
                    } else {
                        format!("its marker says {recorded}")
                    }
                ),
            ));
        }
        let manifest = Manifest::load(&dir.join("aevia.toml"))?;
        let expected_name = chosen.package.rsplit('/').next().unwrap_or_default();
        if !manifest.name.eq_ignore_ascii_case(expected_name) || manifest.semver() != chosen.version
        {
            return Err(ManifestError::new(
                "E0605",
                format!(
                    "the vendored copy of `{chosen}` declares `{} {}`; the index and the \
                     archive disagree",
                    manifest.name, manifest.version
                ),
            ));
        }
        // The index is what was resolved; the manifest is what will be built.
        // They must describe the same dependencies.
        let mut declared: Vec<(String, String, String)> = manifest
            .dependencies
            .iter()
            .filter_map(|(alias, dependency)| {
                dependency
                    .registry()
                    .map(|(package, req)| (alias.clone(), package.to_string(), req.to_string()))
            })
            .collect();
        let mut indexed: Vec<(String, String, String)> = record
            .deps
            .iter()
            .map(|dep| (dep.label(), dep.package.clone(), dep.req.to_string()))
            .collect();
        declared.sort();
        indexed.sort();
        if declared != indexed {
            return Err(ManifestError::new(
                "E0605",
                format!(
                    "the vendored manifest of `{chosen}` and the registry index disagree about \
                     its dependencies; {hint} (`--force` if the files were edited)"
                ),
            ));
        }
        self.packages.insert(
            dir.to_path_buf(),
            Package {
                dir: dir.to_path_buf(),
                manifest,
            },
        );
        Ok(())
    }

    fn render_children(
        &self,
        identity: &Path,
        prefix: &str,
        seen: &mut BTreeSet<PathBuf>,
        out: &mut String,
    ) {
        let Some(children) = self.edges.get(identity) else {
            return;
        };
        let mut children = children.clone();
        children.sort();
        for (position, (alias, child)) in children.iter().enumerate() {
            let last = position + 1 == children.len();
            let (branch, next_prefix) = if last {
                ("└── ", format!("{prefix}    "))
            } else {
                ("├── ", format!("{prefix}│   "))
            };
            let label = match self.registry.get(child) {
                | Some(node) => format!("{alias} {} {} [registry]", node.package, node.version),
                | None => {
                    match self.packages.get(child) {
                        | Some(package) => {
                            format!(
                                "{alias} {} {} (path {})",
                                package.manifest.name,
                                package.manifest.version,
                                relative_to(&self.root, child)
                            )
                        },
                        | None => format!("{alias} (missing)"),
                    }
                },
            };
            let repeated = !seen.insert(child.clone());
            let _ = writeln!(
                out,
                "{prefix}{branch}{label}{}",
                if repeated && self.edges.contains_key(child) {
                    " (*)"
                } else {
                    ""
                }
            );
            if !repeated {
                self.render_children(child, &next_prefix, seen, out);
            }
        }
    }
}

struct RegistryRequest {
    owner: PathBuf,
    alias: String,
    package: String,
    req: VersionReq,
}

/// Maps a resolver failure to the diagnostic codes of §7.7.
fn registry_failure(error: aevia_resolve::ResolveError) -> ManifestError {
    match &error {
        | aevia_resolve::ResolveError::Index(index) => {
            let mut failure = ManifestError::new("E0607", index.message.clone());
            if index.kind == aevia_resolve::IndexErrorKind::Unavailable {
                failure.missing_index = Some(index.package.clone());
            }
            failure
        },
        | other => ManifestError::new("E0606", other.to_string()),
    }
}

fn package_identity(dir: &Path) -> Result<PathBuf, ManifestError> {
    std::fs::canonicalize(dir).map_err(|error| {
        ManifestError::new(
            "E0607",
            format!("cannot resolve package `{}`: {error}", dir.display()),
        )
    })
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct PackageScope {
    source: PathBuf,
    entry: PathBuf,
    logical_entry: PathBuf,
    name: String,
    directory: PathBuf,
    package: String,
}

/// A source path together with the package context that selected it.
/// The path retains its logical spelling for declaration checks and neighbour
/// resolution. Canonical physical identity alone cannot recover this owner.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ResolvedModule {
    /// Source file (or an embedded module's synthetic key).
    pub path: PathBuf,
    /// Canonical package directory; absent for standalone/embedded sources.
    pub owner: Option<PathBuf>,
}

/// Where module paths resolve: the package's `src/`, plus every dependency's.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Roots {
    /// The directory `import A.B` starts from: the package's `src/`.
    pub source: PathBuf,
    /// The entry file's own directory, when it is not the package's `src/`.
    ///
    /// A file in `tests/` or `examples/` imports the package's modules by
    /// their `src/` path *and* its own neighbours, so both roots are tried.
    pub local: Option<PathBuf>,
    /// Import root (`import <Key>.…`) to that package's `src/`.
    pub packages: BTreeMap<String, PathBuf>,
    /// Import root to that package's entry file, for `import <Key>` with no
    /// sub-path: the package itself is its entry, not `src.aev`.
    pub entries: BTreeMap<String, PathBuf>,
    scopes: BTreeMap<PathBuf, PackageScope>,
    root_owner: Option<PathBuf>,
    package_owners: BTreeMap<String, PathBuf>,
}

impl Roots {
    /// The roots a file run without a manifest uses: its own directory.
    ///
    /// This is the behaviour of every program that predates packages, and it
    /// must not change: `import Geometry.Shapes` in `examples/modules/main.aev`
    /// still reads `examples/modules/Geometry/Shapes.aev`.
    #[must_use]
    pub fn adjacent(file: &Path) -> Self {
        Self {
            source: file
                .parent()
                .map_or_else(|| PathBuf::from("."), Path::to_path_buf),
            local: None,
            packages: BTreeMap::new(),
            entries: BTreeMap::new(),
            scopes: BTreeMap::new(),
            root_owner: None,
            package_owners: BTreeMap::new(),
        }
    }

    /// The entry source under the root package context explicitly supplied by
    /// the caller. This remains authoritative through symlinked source trees.
    #[must_use]
    pub fn entry_module(
        &self,
        path: &Path,
    ) -> ResolvedModule {
        ResolvedModule {
            path: path.to_path_buf(),
            owner: self.root_owner.clone(),
        }
    }

    /// Resolves an import under its explicit owning package. Dependencies
    /// switch owner at the import edge; local imports retain the current owner.
    #[must_use]
    pub fn resolve_module(
        &self,
        importer: &ResolvedModule,
        path: &[String],
    ) -> Option<ResolvedModule> {
        self.module_candidates(importer, path)
            .into_iter()
            .find(|module| module.path.is_file())
    }

    /// All candidates with their original owner, for loading and diagnostics.
    #[must_use]
    pub fn module_candidates(
        &self,
        importer: &ResolvedModule,
        path: &[String],
    ) -> Vec<ResolvedModule> {
        if path.is_empty() {
            return Vec::new();
        }
        let scope = importer
            .owner
            .as_ref()
            .and_then(|owner| self.scopes.get(owner));
        let source = scope.map_or(self.source.as_path(), |scope| scope.source.as_path());
        let local = if let Some(scope) = scope {
            // Use logical paths first: a single src file may link outside the
            // package while still importing the package's own source modules.
            let file = crate::source::canonical_path(&importer.path);
            let in_source = normalise(&absolute(&importer.path))
                .starts_with(normalise(&absolute(&scope.source)))
                || file.starts_with(crate::source::canonical_path(&scope.source))
                || (file == scope.entry
                    && normalise(&scope.logical_entry).starts_with(normalise(&scope.source)));
            if in_source {
                None
            } else if file == scope.entry {
                scope.logical_entry.parent().map(Path::to_path_buf)
            } else {
                file.parent().map(Path::to_path_buf)
            }
        } else {
            self.local.clone()
        };
        let mut out = Vec::new();
        for base in std::iter::once(source).chain(local.as_deref()) {
            let mut file = base.to_path_buf();
            for segment in path {
                file.push(segment);
            }
            file.set_extension("aev");
            out.push(ResolvedModule {
                path: file,
                owner: importer.owner.clone(),
            });
        }
        if let Some((first, rest)) = path.split_first()
            && let Some(base) = self.packages.get(first)
        {
            let owner = self.package_owners.get(first).cloned();
            if rest.is_empty() {
                if let Some(entry) = self.entries.get(first) {
                    out.push(ResolvedModule {
                        path: entry.clone(),
                        owner: owner.clone(),
                    });
                }
                out.push(ResolvedModule {
                    path: base.join(format!("{first}.aev")),
                    owner,
                });
            } else {
                let mut file = base.clone();
                for segment in rest {
                    file.push(segment);
                }
                file.set_extension("aev");
                out.push(ResolvedModule { path: file, owner });
            }
        }
        out
    }

    /// Nominal identity in the owner that selected this module, including a
    /// source that lives physically outside that package's canonical prefix.
    #[must_use]
    pub fn resolved_module_identity(
        &self,
        module: &ResolvedModule,
        visible: &[String],
    ) -> aevia_types::keys::ModulePath {
        if let Some(scope) = module
            .owner
            .as_ref()
            .and_then(|owner| self.scopes.get(owner))
        {
            crate::source::SourceIdentity::for_owner(
                scope.package.clone(),
                &scope.directory,
                &module.path,
            )
            .module_path(Some(visible))
        } else {
            crate::source::SourceIdentity::for_path(&module.path).module_path(Some(visible))
        }
    }

    pub(crate) fn resolved_name_matches(
        &self,
        module: &ResolvedModule,
        declared: &[String],
        expected: &[String],
    ) -> bool {
        if declared == expected {
            return true;
        }
        let Some(scope) = module
            .owner
            .as_ref()
            .and_then(|owner| self.scopes.get(owner))
        else {
            return self.declared_name_matches(&module.path, declared, expected);
        };
        let physical = crate::source::canonical_path(&module.path);
        if physical == scope.entry && expected.len() == 1 && declared == [scope.name.clone()] {
            return true;
        }
        let source = normalise(&absolute(&scope.source));
        let supplied = normalise(&absolute(&module.path));
        let logical = if physical == scope.entry && !supplied.starts_with(&source) {
            normalise(&scope.logical_entry)
        } else {
            supplied
        };
        let physical_source = crate::source::canonical_path(&scope.source);
        let relative = logical
            .strip_prefix(&source)
            .or_else(|_| physical.strip_prefix(&physical_source));
        let Ok(relative) = relative else {
            return false;
        };
        let relative = relative.with_extension("");
        let parts: Vec<String> = relative
            .components()
            .map(|part| part.as_os_str().to_string_lossy().into_owned())
            .collect();
        declared == parts
            || declared
                == std::iter::once(scope.name.clone())
                    .chain(parts)
                    .collect::<Vec<_>>()
    }

    /// Also tries `dir` after the package's `src/`.
    #[must_use]
    pub fn with_local(
        mut self,
        dir: &Path,
    ) -> Self {
        if normalise(dir) != normalise(&self.source) {
            self.local = Some(dir.to_path_buf());
        }
        self
    }

    /// The file an import path names: the package's `src/`, the entry's own
    /// directory, then the dependencies.
    #[must_use]
    pub fn resolve(
        &self,
        path: &[String],
    ) -> Option<PathBuf> {
        if path.is_empty() {
            return None;
        }
        self.candidates(path)
            .into_iter()
            .find(|candidate| candidate.is_file())
    }

    /// Every file an import path could have meant, for a `E0607` message.
    #[must_use]
    pub fn candidates(
        &self,
        path: &[String],
    ) -> Vec<PathBuf> {
        self.candidates_in(&self.source, self.local.as_deref(), path)
    }

    /// Import candidates in the package that owns the importing file. This
    /// prevents a dependency's `import Helper` from capturing the application's
    /// `Helper.aev` and permits libraries to use their own source modules.
    #[must_use]
    pub fn candidates_from(
        &self,
        importer: &Path,
        path: &[String],
    ) -> Vec<PathBuf> {
        match self.scope_for(importer) {
            | Some(scope) if normalise(&absolute(&self.source)) != scope.source => {
                self.candidates_in(&scope.source, None, path)
            },
            | Some(_) => self.candidates(path),
            | None => self.candidates(path),
        }
    }

    /// Resolves an import in its importing package's source scope.
    #[must_use]
    pub fn resolve_from(
        &self,
        importer: &Path,
        path: &[String],
    ) -> Option<PathBuf> {
        self.candidates_from(importer, path)
            .into_iter()
            .find(|path| path.is_file())
    }

    fn scope_for(
        &self,
        file: &Path,
    ) -> Option<&PackageScope> {
        let file = std::fs::canonicalize(file).unwrap_or_else(|_| normalise(&absolute(file)));
        self.scopes
            .iter()
            .filter(|(dir, _)| file.starts_with(dir))
            .max_by_key(|(dir, _)| dir.components().count())
            .map(|(_, scope)| scope)
    }

    /// Visible module names and package source origins travel separately through
    /// checking and runtime dispatch. Canonical package directories distinguish
    /// two local packages with the same name/version; file paths are relative
    /// to their owning package and independent of import aliases.
    #[must_use]
    pub fn module_identity(
        &self,
        file: &Path,
        visible: &[String],
    ) -> Option<aevia_types::keys::ModulePath> {
        let Some(scope) = self.scope_for(file) else {
            return Some(crate::source::SourceIdentity::for_path(file).module_path(Some(visible)));
        };
        let file = std::fs::canonicalize(file).unwrap_or_else(|_| normalise(&absolute(file)));
        Some(
            aevia_types::keys::ModulePath::new(visible.iter().cloned()).with_origin(
                aevia_types::keys::ModuleOrigin {
                    package: scope.package.clone(),
                    source: relative_to(&scope.directory, &file),
                },
            ),
        )
    }

    /// Package aliases affect lookup, not the declared identity of a library.
    /// Accept names relative to its source tree or rooted at its own package
    /// name as well as the exact import path.
    pub(crate) fn declared_name_matches(
        &self,
        file: &Path,
        declared: &[String],
        expected: &[String],
    ) -> bool {
        if declared == expected {
            return true;
        }
        let Some(scope) = self.scope_for(file) else {
            return false;
        };
        let file = std::fs::canonicalize(file).unwrap_or_else(|_| file.to_path_buf());
        if file == scope.entry && declared == [scope.name.clone()] {
            return true;
        }
        let Ok(relative) = file.strip_prefix(&scope.source) else {
            return false;
        };
        let relative = relative.with_extension("");
        let parts: Vec<String> = relative
            .components()
            .map(|part| part.as_os_str().to_string_lossy().into_owned())
            .collect();
        declared == parts
            || declared
                == std::iter::once(scope.name.clone())
                    .chain(parts)
                    .collect::<Vec<_>>()
    }

    fn candidates_in(
        &self,
        source: &Path,
        local: Option<&Path>,
        path: &[String],
    ) -> Vec<PathBuf> {
        if path.is_empty() {
            return Vec::new();
        }
        let mut out = Vec::new();
        for base in std::iter::once(source).chain(local) {
            let mut candidate = base.to_path_buf();
            for segment in path {
                candidate.push(segment);
            }
            candidate.set_extension("aev");
            out.push(candidate);
        }
        if let Some((first, rest)) = path.split_first()
            && let Some(base) = self.packages.get(first)
        {
            if rest.is_empty() {
                // `import Lib` names the package itself: its entry file.
                // Joining `src` with no segment produced `src.aev`, which
                // is why a dependency could not be imported at all.
                if let Some(entry) = self.entries.get(first) {
                    out.push(entry.clone());
                }
                out.push(base.join(format!("{first}.aev")));
            } else {
                let mut target = base.clone();
                for segment in rest {
                    target.push(segment);
                }
                target.set_extension("aev");
                out.push(target);
            }
        }
        out
    }
}

/// The contents of `aevia.lock`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Lock {
    /// The resolved packages: the root first, then dependencies sorted by
    /// source, then by import root.
    pub packages: Vec<LockedPackage>,
}

/// One resolved package in the lock file.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LockedPackage {
    /// The package's own `[package] name`; for a registry package, its
    /// registry identity `owner/name`.
    pub name: String,
    /// Its exact version.
    pub version: String,
    /// The root module it is imported under.
    pub import: String,
    /// `root` for the package itself, `path+<as written>` for a path
    /// dependency, `registry+aevia` for a registry package.
    pub source: String,
    /// `fnv1a64:<hex>` over a path dependency's canonical manifest, source
    /// tree and entry file; absent for the root package and registry packages.
    pub hash: Option<String>,
    /// `sha256:<hex>` of a registry package's archive, as the index records
    /// it; absent for the root and path packages.
    pub checksum: Option<String>,
}

/// The lock file's format version.
pub const LOCK_VERSION: u32 = 1;

impl Lock {
    /// Renders the lock file deterministically.
    ///
    /// Every field has a fixed position and packages are already sorted, so
    /// two runs over the same tree produce the same bytes.
    #[must_use]
    pub fn render(&self) -> String {
        let mut out = String::new();
        out.push_str(
            "# aevia.lock — generated by `aevia update`. Do not edit; run `aevia update`.\n",
        );
        let _ = writeln!(out, "version = {LOCK_VERSION}");
        for package in &self.packages {
            out.push('\n');
            out.push_str("[[package]]\n");
            let _ = writeln!(out, "name = \"{}\"", package.name);
            let _ = writeln!(out, "version = \"{}\"", package.version);
            let _ = writeln!(out, "import = \"{}\"", package.import);
            let _ = writeln!(out, "source = \"{}\"", package.source);
            if let Some(hash) = &package.hash {
                let _ = writeln!(out, "hash = \"{hash}\"");
            }
            if let Some(checksum) = &package.checksum {
                let _ = writeln!(out, "checksum = \"{checksum}\"");
            }
        }
        out
    }

    /// The registry packages the lock names, by identity.
    #[must_use]
    pub fn registry_versions(&self) -> BTreeMap<String, BTreeSet<Version>> {
        let mut out: BTreeMap<String, BTreeSet<Version>> = BTreeMap::new();
        for package in &self.packages {
            if package.source.starts_with("registry+")
                && let Ok(version) = Version::parse(&package.version)
            {
                out.entry(package.name.clone()).or_default().insert(version);
            }
        }
        out
    }

    /// The distinct registry packages (name, version, checksum), sorted.
    #[must_use]
    pub fn registry_packages(&self) -> Vec<(String, String, String)> {
        let mut out: BTreeSet<(String, String, String)> = BTreeSet::new();
        for package in &self.packages {
            if package.source.starts_with("registry+") {
                out.insert((
                    package.name.clone(),
                    package.version.clone(),
                    package.checksum.clone().unwrap_or_default(),
                ));
            }
        }
        out.into_iter().collect()
    }

    /// Parses a lock file.
    ///
    /// # Errors
    ///
    /// `E0605` for a malformed or unrecognised lock file.
    pub fn parse(
        text: &str,
        origin: &str,
    ) -> Result<Self, ManifestError> {
        let mut packages: Vec<LockedPackage> = Vec::new();
        let mut current: Option<LockedPackage> = None;
        let mut version = None;
        for (index, raw) in text.lines().enumerate() {
            let line = raw.trim();
            let number = index + 1;
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            if line == "[[package]]" {
                if let Some(package) = current.take() {
                    packages.push(package);
                }
                current = Some(LockedPackage {
                    name: String::new(),
                    version: String::new(),
                    import: String::new(),
                    source: String::new(),
                    hash: None,
                    checksum: None,
                });
                continue;
            }
            let Some((key, value)) = line.split_once('=') else {
                return Err(ManifestError::new(
                    "E0605",
                    format!("{origin}:{number}: expected `key = value`, found `{line}`"),
                ));
            };
            let key = key.trim();
            let value = value.trim();
            if let Some(package) = current.as_mut() {
                let text = parse_string(value).ok_or_else(|| {
                    ManifestError::new(
                        "E0605",
                        format!("{origin}:{number}: `{key}` expects a quoted string"),
                    )
                })?;
                match key {
                    | "name" => package.name = text,
                    | "version" => package.version = text,
                    | "import" => package.import = text,
                    | "source" => package.source = text,
                    | "hash" => package.hash = Some(text),
                    | "checksum" => package.checksum = Some(text),
                    | other => {
                        return Err(ManifestError::new(
                            "E0605",
                            format!("{origin}:{number}: unknown lock key `{other}`"),
                        ));
                    },
                }
            } else if key == "version" {
                version = Some(value.to_string());
            } else {
                return Err(ManifestError::new(
                    "E0605",
                    format!("{origin}:{number}: `{line}` appears before `[[package]]`"),
                ));
            }
        }
        if let Some(package) = current.take() {
            packages.push(package);
        }
        match version.as_deref() {
            | Some("1") => {},
            | Some(other) => {
                return Err(ManifestError::new(
                    "E0605",
                    format!("{origin}: lock version `{other}` is not supported; expected `1`"),
                ));
            },
            | None => {
                return Err(ManifestError::new(
                    "E0605",
                    format!("{origin}: lock file has no `version`"),
                ));
            },
        }
        for package in &packages {
            if package.name.is_empty() || package.source.is_empty() || package.import.is_empty() {
                return Err(ManifestError::new(
                    "E0605",
                    format!("{origin}: a `[[package]]` entry needs `name`, `import` and `source`"),
                ));
            }
            if package.source.starts_with("registry+") && package.checksum.is_none() {
                return Err(ManifestError::new(
                    "E0605",
                    format!(
                        "{origin}: the registry package `{}` has no `checksum`",
                        package.name
                    ),
                ));
            }
        }
        Ok(Self { packages })
    }

    /// Reads a lock file.
    ///
    /// # Errors
    ///
    /// `E0607` when it cannot be read, `E0605` when it does not parse.
    pub fn load(path: &Path) -> Result<Self, ManifestError> {
        let text = std::fs::read_to_string(path).map_err(|error| {
            ManifestError::new(
                "E0607",
                format!("cannot read `{}`: {error}", path.display()),
            )
        })?;
        Self::parse(&text, &path.display().to_string())
    }

    /// Writes the lock file.
    ///
    /// # Errors
    ///
    /// The I/O error, with the path, when it cannot be written.
    pub fn save(
        &self,
        path: &Path,
    ) -> Result<(), ManifestError> {
        std::fs::write(path, self.render()).map_err(|error| {
            ManifestError::new(
                "E0605",
                format!("cannot write `{}`: {error}", path.display()),
            )
        })
    }
}

/// The import root a package name maps to: `units_extra` is `UnitsExtra`.
///
/// The parser's module paths are `UpperCamelCase` segments, so a `snake_case`
/// package name cannot be used verbatim in `import`. The mapping is total and
/// reversible enough to be predictable, and `aevia add` writes the result as
/// the dependency key.
#[must_use]
pub fn import_root(name: &str) -> String {
    let mut out = String::new();
    for part in name.split(['_', '-']).filter(|part| !part.is_empty()) {
        let mut chars = part.chars();
        if let Some(first) = chars.next() {
            out.extend(first.to_uppercase());
            out.push_str(chars.as_str());
        }
    }
    if out.is_empty() {
        name.to_string()
    } else {
        out
    }
}

fn is_module_segment(key: &str) -> bool {
    let mut chars = key.chars();
    match chars.next() {
        | Some(first) if first.is_uppercase() => {},
        | _ => return false,
    }
    chars.all(|c| c.is_alphanumeric() || c == '_')
}

/// True when `name` is a valid `[package] name`: lower-case, digits, `_`, `-`.
#[must_use]
pub fn valid_package_name(name: &str) -> bool {
    is_package_name(name)
}

fn is_package_name(name: &str) -> bool {
    let mut chars = name.chars();
    match chars.next() {
        | Some(first) if first.is_ascii_alphabetic() => {},
        | _ => return false,
    }
    chars.all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
}

fn parse_string(value: &str) -> Option<String> {
    let value = value.trim();
    let inner = value.strip_prefix('"')?.strip_suffix('"')?;
    if inner.contains('"') {
        return None;
    }
    Some(inner.to_string())
}

/// Cuts a trailing `# comment` that is not inside a quoted string.
fn strip_comment(line: &str) -> &str {
    let mut quoted = false;
    for (position, character) in line.char_indices() {
        match character {
            | '"' => quoted = !quoted,
            | '#' if !quoted => return &line[..position],
            | _ => {},
        }
    }
    line
}

/// Splits the inside of an inline table on the commas that are not inside a
/// quoted string (`version = ">=1.2, <2"` has one that is).
fn split_fields(inner: &str) -> Result<Vec<&str>, String> {
    let mut fields = Vec::new();
    let mut quoted = false;
    let mut start = 0;
    for (position, character) in inner.char_indices() {
        match character {
            | '"' => quoted = !quoted,
            | ',' if !quoted => {
                fields.push(&inner[start..position]);
                start = position + 1;
            },
            | _ => {},
        }
    }
    if quoted {
        return Err("an unterminated string in the inline table".to_string());
    }
    fields.push(&inner[start..]);
    Ok(fields)
}

fn parse_requirement(text: &str) -> Result<VersionReq, String> {
    VersionReq::parse(text).map_err(|error| {
        format!("{error}; a requirement is `^1.2`, `~1.2.3`, `>=1.2, <2`, `1.*`, `=1.2.3` or `*`")
    })
}

fn parse_dependency(
    key: &str,
    value: &str,
    origin: &str,
    number: usize,
) -> Result<Dependency, ManifestError> {
    let fail =
        |message: String| ManifestError::new("E0605", format!("{origin}:{number}: {message}"));
    let value = value.trim();
    if value.starts_with('"') {
        let text = parse_string(value)
            .ok_or_else(|| fail("a version requirement is one quoted string".to_string()))?;
        let req = parse_requirement(&text).map_err(fail)?;
        return Ok(Dependency::Registry {
            package: official_package(key),
            req,
        });
    }
    let Some(inner) = value
        .strip_prefix('{')
        .and_then(|rest| rest.strip_suffix('}'))
    else {
        return Err(fail(format!(
            "expected a version requirement (`{key} = \"^1.2\"`) or an inline table such as \
             `{{ path = \"...\" }}` or `{{ package = \"owner/name\", version = \"^1.2\" }}`"
        )));
    };
    let mut path: Option<String> = None;
    let mut package: Option<String> = None;
    let mut version: Option<String> = None;
    for field in split_fields(inner).map_err(fail)? {
        let field = field.trim();
        if field.is_empty() {
            continue;
        }
        let Some((name, field_value)) = field.split_once('=') else {
            return Err(fail(format!("expected `key = value` in `{field}`")));
        };
        let name = name.trim();
        let text = parse_string(field_value)
            .ok_or_else(|| fail(format!("`{name}` expects a quoted string")))?;
        let slot = match name {
            | "path" => &mut path,
            | "package" => &mut package,
            | "version" => &mut version,
            | other => {
                return Err(fail(format!(
                    "`{other}` is not supported; a dependency is a version requirement, \
                     `{{ package = \"owner/name\", version = \"...\" }}`, `{{ path = \"...\" }}`, \
                     or a path with a version (there is no git dependency)"
                )));
            },
        };
        if slot.replace(text).is_some() {
            return Err(fail(format!("`{name}` is given twice")));
        }
    }
    let package = match package {
        | Some(text) => {
            Some(canonical_package(&text).ok_or_else(|| {
                fail(format!(
                    "`{text}` is not a package identity; write `owner/name` (a GitHub login or \
                     ORCID iD, then the package name)"
                ))
            })?)
        },
        | None => None,
    };
    let req = match version {
        | Some(text) => {
            Some(
                parse_requirement(&text)
                    .map_err(|message| fail(format!("`version`: {message}")))?,
            )
        },
        | None => None,
    };
    match (path, package, req) {
        | (Some(path), None, None) => Ok(Dependency::Path { path }),
        | (None, package, Some(req)) => {
            Ok(Dependency::Registry {
                package: package.unwrap_or_else(|| official_package(key)),
                req,
            })
        },
        | (Some(path), package, Some(req)) => {
            Ok(Dependency::Both {
                path,
                package: package.unwrap_or_else(|| official_package(key)),
                req,
            })
        },
        | (None, Some(package), None) => {
            Err(fail(format!(
                "`package = \"{package}\"` needs a `version` (write `version = \"*\"` to accept \
                 any)"
            )))
        },
        | (Some(_), Some(_), None) => {
            Err(fail(
                "`package` needs a `version` as well; without one this is a path dependency"
                    .to_string(),
            ))
        },
        | (None, None, None) => {
            Err(fail(
                "a dependency needs a `path` or a `version`".to_string(),
            ))
        },
    }
}

/// A path with `.` and `..` folded away, without touching the filesystem.
///
/// `std::fs::canonicalize` would need the file to exist and would return a
/// Windows `\\?\` prefix; comparison only needs the components resolved.
fn normalise(path: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for component in path.components() {
        match component {
            | std::path::Component::CurDir => {},
            | std::path::Component::ParentDir => {
                if out.file_name().is_some_and(|name| name != "..") {
                    out.pop();
                } else if !out.has_root() {
                    out.push("..");
                }
            },
            | other => out.push(other.as_os_str()),
        }
    }
    out
}

/// `target` as a path relative to `root`, with `/` separators.
pub(crate) fn relative_to(
    root: &Path,
    target: &Path,
) -> String {
    let root = normalise(&absolute(root));
    let target = normalise(&absolute(target));
    if let Ok(rest) = target.strip_prefix(&root) {
        let text = rest.to_string_lossy().replace('\\', "/");
        return if text.is_empty() {
            ".".to_string()
        } else {
            text
        };
    }
    let root_parts: Vec<_> = root.components().collect();
    let target_parts: Vec<_> = target.components().collect();
    let common = root_parts
        .iter()
        .zip(target_parts.iter())
        .take_while(|(left, right)| left == right)
        .count();
    if common == 0 {
        return target.to_string_lossy().replace('\\', "/");
    }
    let mut out = String::new();
    for _ in common..root_parts.len() {
        out.push_str("../");
    }
    for component in &target_parts[common..] {
        out.push_str(&component.as_os_str().to_string_lossy());
        out.push('/');
    }
    out.trim_end_matches('/').to_string()
}

fn absolute(path: &Path) -> PathBuf {
    if path.is_absolute() {
        return path.to_path_buf();
    }
    std::env::current_dir()
        .map(|cwd| cwd.join(path))
        .unwrap_or_else(|_| path.to_path_buf())
}

/// The package's resolution contract is part of its content: source-only
/// hashes miss entry changes and dependency-edge changes in the manifest.
fn hash_package(package: &Package) -> Result<String, ManifestError> {
    let mut files = Vec::new();
    collect(&package.source_dir(), &mut files)?;
    let entry = package.entry_path();
    if entry.is_file() && !files.contains(&entry) {
        files.push(entry);
    }
    files.sort();
    let mut hash = 0xcbf2_9ce4_8422_2325u64;
    hash_frame(
        &mut hash,
        "aevia.toml",
        package.manifest.render().as_bytes(),
    );
    for file in files {
        let bytes = std::fs::read(&file).map_err(|error| {
            ManifestError::new(
                "E0607",
                format!("cannot read `{}`: {error}", file.display()),
            )
        })?;
        hash_frame(&mut hash, &relative_to(&package.dir, &file), &bytes);
    }
    Ok(format!("fnv1a64:{hash:016x}"))
}

fn hash_frame(
    hash: &mut u64,
    name: &str,
    bytes: &[u8],
) {
    // Length framing prevents one file's contents from impersonating the next
    // file's name and contents when files are added or removed.
    for byte in (name.len() as u64)
        .to_le_bytes()
        .iter()
        .chain(name.as_bytes())
        .chain((bytes.len() as u64).to_le_bytes().iter())
        .chain(bytes)
    {
        *hash ^= u64::from(*byte);
        *hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
}

/// `fnv1a64:<hex>` over every `.aev` file under `src`, sorted by path.
///
/// A stable, dependency-free digest: `std`'s hasher is explicitly not stable
/// across releases, and a lock file that changes when the compiler is upgraded
/// would defeat the point of a lock file.
///
/// # Errors
///
/// `E0607` when the tree cannot be read.
pub fn hash_sources(source: &Path) -> Result<String, ManifestError> {
    let mut files: Vec<PathBuf> = Vec::new();
    collect(source, &mut files)?;
    files.sort();
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for file in &files {
        let relative = file.strip_prefix(source).unwrap_or(file);
        let relative = relative.to_string_lossy().replace('\\', "/");
        let bytes = std::fs::read(file).map_err(|error| {
            ManifestError::new(
                "E0607",
                format!("cannot read `{}`: {error}", file.display()),
            )
        })?;
        for byte in relative.as_bytes().iter().chain(&[0u8]).chain(&bytes) {
            hash ^= u64::from(*byte);
            hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
        }
    }
    Ok(format!("fnv1a64:{hash:016x}"))
}

fn collect(
    dir: &Path,
    out: &mut Vec<PathBuf>,
) -> Result<(), ManifestError> {
    collect_acyclic(dir, out, &mut BTreeSet::new())
}

fn collect_acyclic(
    dir: &Path,
    out: &mut Vec<PathBuf>,
    active: &mut BTreeSet<PathBuf>,
) -> Result<(), ManifestError> {
    if !dir.exists() {
        return Ok(());
    }
    let identity = std::fs::canonicalize(dir).map_err(|error| {
        ManifestError::new(
            "E0607",
            format!(
                "cannot resolve source directory `{}`: {error}",
                dir.display()
            ),
        )
    })?;
    if !active.insert(identity.clone()) {
        return Err(ManifestError::new(
            "E0607",
            format!("source directory cycle at `{}`", dir.display()),
        ));
    }
    let entries = match std::fs::read_dir(dir) {
        | Ok(entries) => entries,
        // A dependency with no `src/` is empty, not broken: the lock still
        // records it, so removing a directory shows up as a hash change.
        | Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        | Err(error) => {
            return Err(ManifestError::new(
                "E0607",
                format!("cannot read `{}`: {error}", dir.display()),
            ));
        },
    };
    for entry in entries {
        let entry = entry.map_err(|error| {
            ManifestError::new("E0607", format!("cannot read `{}`: {error}", dir.display()))
        })?;
        let path = entry.path();
        if path.is_dir() {
            collect_acyclic(&path, out, active)?;
        } else if path.extension().is_some_and(|extension| extension == "aev") {
            out.push(path);
        }
    }
    active.remove(&identity);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = r#"# a manifest
[package]
name = "kinematics"
version = "0.1.0"
entry = "src/main.aev"

[dependencies]
UnitsExtra = { path = "../units-extra" }
"#;

    fn req(text: &str) -> VersionReq {
        VersionReq::parse(text).expect("requirement")
    }

    fn with_dependencies(lines: &str) -> String {
        format!("[package]\nname = \"a\"\nversion = \"1.0.0\"\n\n[dependencies]\n{lines}")
    }

    #[test]
    fn a_manifest_round_trips_through_its_rendering() {
        // A formatter that rewrites the manifest must not lose or reorder
        // anything: parse -> render -> parse is the identity.
        let manifest = Manifest::parse(SAMPLE, "aevia.toml").expect("parses");
        assert_eq!(manifest.name, "kinematics");
        assert_eq!(manifest.version, "0.1.0");
        assert_eq!(manifest.entry, "src/main.aev");
        assert_eq!(
            manifest.dependencies.get("UnitsExtra"),
            Some(&Dependency::Path {
                path: "../units-extra".to_string()
            })
        );
        let rendered = manifest.render();
        let again = Manifest::parse(&rendered, "aevia.toml").expect("re-parses");
        assert_eq!(manifest, again);
        assert_eq!(rendered, again.render());
    }

    #[test]
    fn every_dependency_form_parses_and_round_trips() {
        let text = with_dependencies(
            "Local = { path = \"../local\" }\n\
             Units = \"^1.2\"\n\
             UnitsExtra = \">=1.2, <2\"  # a comma inside the string\n\
             Plot = { package = \"Apich/Plot\", version = \"~2.1.0\" }\n\
             Wide = { version = \"1.*\" }\n\
             Dev = { path = \"../dev\", package = \"apich/dev\", version = \"^0.3\" }\n\
             Same = { path = \"../same\", version = \"1\" }\n",
        );
        let manifest = Manifest::parse(&text, "aevia.toml").expect("parses");
        let d = &manifest.dependencies;
        assert_eq!(
            d["Units"],
            Dependency::Registry {
                package: "aevia/units".to_string(),
                req: req("^1.2")
            }
        );
        assert_eq!(
            d["UnitsExtra"],
            Dependency::Registry {
                package: "aevia/units_extra".to_string(),
                req: req(">=1.2, <2")
            }
        );
        assert_eq!(
            d["Plot"],
            Dependency::Registry {
                package: "apich/plot".to_string(),
                req: req("~2.1.0")
            },
            "the identity is canonicalised"
        );
        assert_eq!(
            d["Wide"],
            Dependency::Registry {
                package: "aevia/wide".to_string(),
                req: req("1.*")
            }
        );
        assert_eq!(
            d["Dev"],
            Dependency::Both {
                path: "../dev".to_string(),
                package: "apich/dev".to_string(),
                req: req("^0.3")
            }
        );
        assert_eq!(d["Same"].local_path(), Some("../same"));
        assert_eq!(d["Same"].registry().map(|(p, _)| p), Some("aevia/same"));
        let rendered = manifest.render();
        assert!(rendered.contains("Units = \"^1.2\"\n"), "{rendered}");
        assert!(
            rendered.contains("UnitsExtra = \">=1.2, <2\"\n"),
            "{rendered}"
        );
        assert!(
            rendered.contains("Plot = { package = \"apich/plot\", version = \"~2.1.0\" }"),
            "{rendered}"
        );
        let again = Manifest::parse(&rendered, "aevia.toml").expect("re-parses");
        assert_eq!(manifest, again);
        assert_eq!(rendered, again.render());
    }

    #[test]
    fn a_trailing_comment_is_ignored_but_a_hash_inside_a_string_is_not() {
        let manifest = Manifest::parse(
            "[package] # the package\nname = \"a\" # its name\nversion = \"1.0.0\"\n",
            "aevia.toml",
        )
        .expect("parses");
        assert_eq!(manifest.name, "a");
        assert_eq!(strip_comment("x = \"a#b\" # c"), "x = \"a#b\" ");
    }

    #[test]
    fn the_package_version_may_carry_a_pre_release() {
        let manifest = Manifest::parse(
            "[package]\nname = \"a\"\nversion = \"1.0.0-rc.1\"\n",
            "aevia.toml",
        )
        .expect("parses");
        assert!(manifest.semver().is_prerelease());
        for bad in ["1.0", "v1.0.0", "1.0.0.0", "01.0.0"] {
            let error = Manifest::parse(
                &format!("[package]\nname = \"a\"\nversion = \"{bad}\"\n"),
                "aevia.toml",
            )
            .expect_err(bad);
            assert_eq!(error.code, "E0605", "{bad}");
        }
    }

    #[test]
    fn the_entry_defaults_to_src_main_aev() {
        let manifest = Manifest::parse(
            "[package]\nname = \"a\"\nversion = \"1.0.0\"\n",
            "aevia.toml",
        )
        .expect("parses");
        assert_eq!(manifest.entry, "src/main.aev");
        assert!(manifest.dependencies.is_empty());
    }

    #[test]
    fn unsupported_or_malformed_dependencies_stay_e0605_and_name_the_line() {
        for (lines, needle) in [
            ("A = { git = \"x\" }\n", "not supported"),
            ("A = \"not a version\"\n", "is not a number"),
            ("A = \"^1.2.3.4\"\n", "more than three"),
            ("A = { package = \"apich/a\" }\n", "needs a `version`"),
            (
                "A = { package = \"nonsense\", version = \"1\" }\n",
                "owner/name",
            ),
            ("A = { path = \"x\", path = \"y\" }\n", "given twice"),
            ("A = { }\n", "needs a `path` or a `version`"),
            ("A = { version = 1 }\n", "quoted string"),
            ("A = 7\n", "expected a version requirement"),
            ("A = { version = \"^1\"\n", "expected a version requirement"),
            ("A = \"^1\"\nA = \"^2\"\n", "declared twice"),
            (
                "A = { path = \"x\", package = \"o/a\" }\n",
                "needs a `version`",
            ),
        ] {
            let error = Manifest::parse(&with_dependencies(lines), "aevia.toml").expect_err(lines);
            assert_eq!(error.code, "E0605", "{lines}");
            assert!(error.message.contains(needle), "{lines}: {}", error.message);
            assert!(error.message.contains("aevia.toml:"), "{}", error.message);
        }
    }

    #[test]
    fn a_malformed_manifest_names_the_line() {
        let error = Manifest::parse(
            "[package]\nname = \"a\"\nversion = \"1.0.0\"\nnonsense\n",
            "aevia.toml",
        )
        .expect_err("malformed");
        assert_eq!(error.code, "E0605");
        assert!(error.message.contains(":4:"), "{}", error.message);
    }

    #[test]
    fn a_dependency_key_must_be_importable() {
        // The key is a module path segment, so `units-extra` cannot be one.
        for line in ["units-extra = { path = \"../x\" }\n", "units = \"^1\"\n"] {
            let error =
                Manifest::parse(&with_dependencies(line), "aevia.toml").expect_err("bad key");
            assert_eq!(error.code, "E0605");
        }
    }

    #[test]
    fn official_and_canonical_names() {
        assert_eq!(official_package("UnitsExtra"), "aevia/units_extra");
        assert_eq!(official_package("Linalg"), "aevia/linalg");
        assert_eq!(
            canonical_package("ApIch/Units-Extra").as_deref(),
            Some("apich/units-extra")
        );
        assert_eq!(
            canonical_package("0000-0002-1825-009x/lin").as_deref(),
            Some("0000-0002-1825-009X/lin")
        );
        for bad in ["a", "/a", "a/", "a/b/c", "a/9b", "-a/b", "a_/b", "a/b c"] {
            assert_eq!(canonical_package(bad), None, "{bad}");
        }
    }

    fn locked(
        name: &str,
        version: &str,
        import: &str,
        source: &str,
        hash: Option<&str>,
        checksum: Option<&str>,
    ) -> LockedPackage {
        LockedPackage {
            name: name.to_string(),
            version: version.to_string(),
            import: import.to_string(),
            source: source.to_string(),
            hash: hash.map(str::to_string),
            checksum: checksum.map(str::to_string),
        }
    }

    #[test]
    fn a_lock_file_round_trips_byte_for_byte() {
        let lock = Lock {
            packages: vec![
                locked("kinematics", "0.1.0", "Kinematics", "root", None, None),
                locked(
                    "units-extra",
                    "0.2.0",
                    "UnitsExtra",
                    "path+../units-extra",
                    Some("fnv1a64:0123456789abcdef"),
                    None,
                ),
                locked(
                    "apich/plot",
                    "2.1.3-rc.1",
                    "Plot",
                    "registry+aevia",
                    None,
                    Some("sha256:00ff"),
                ),
            ],
        };
        let text = lock.render();
        let parsed = Lock::parse(&text, "aevia.lock").expect("parses");
        assert_eq!(lock, parsed);
        assert_eq!(text, parsed.render());
        // The rendering is the identity on itself: a second write is a no-op,
        // which is what makes a diff mean a real change.
        assert_eq!(text, Lock::parse(&text, "aevia.lock").unwrap().render());
        assert!(text.contains("checksum = \"sha256:00ff\"\n"));
        let versions = parsed.registry_versions();
        assert_eq!(versions["apich/plot"].len(), 1);
        assert_eq!(parsed.registry_packages().len(), 1);
    }

    #[test]
    fn a_path_only_lock_is_unchanged_by_registry_support() {
        // The exact bytes a toolchain without a registry wrote.
        let text = "# aevia.lock — generated by `aevia update`. Do not edit; run `aevia update`.\n\
                    version = 1\n\
                    \n\
                    [[package]]\n\
                    name = \"app\"\n\
                    version = \"0.1.0\"\n\
                    import = \"App\"\n\
                    source = \"root\"\n\
                    \n\
                    [[package]]\n\
                    name = \"dep\"\n\
                    version = \"0.2.0\"\n\
                    import = \"Dep\"\n\
                    source = \"path+../dep\"\n\
                    hash = \"fnv1a64:0123456789abcdef\"\n";
        assert_eq!(Lock::parse(text, "aevia.lock").unwrap().render(), text);
    }

    #[test]
    fn a_registry_lock_entry_needs_its_checksum() {
        let error = Lock::parse(
            "version = 1\n\n[[package]]\nname = \"o/a\"\nversion = \"1.0.0\"\nimport = \"A\"\nsource = \"registry+aevia\"\n",
            "aevia.lock",
        )
        .expect_err("no checksum");
        assert_eq!(error.code, "E0605");
        assert!(error.message.contains("checksum"), "{}", error.message);
    }

    #[test]
    fn a_lock_file_without_a_version_is_rejected() {
        let error = Lock::parse(
            "[[package]]\nname = \"a\"\nimport = \"A\"\nsource = \"root\"\n",
            "aevia.lock",
        )
        .expect_err("no version");
        assert_eq!(error.code, "E0605");
    }

    #[test]
    fn package_names_map_to_import_roots() {
        assert_eq!(import_root("units_extra"), "UnitsExtra");
        assert_eq!(import_root("units-extra"), "UnitsExtra");
        assert_eq!(import_root("kinematics"), "Kinematics");
        assert_eq!(import_root("a"), "A");
    }

    // -----------------------------------------------------------------
    // Registry resolution over a cache directory.

    /// A scratch directory holding a package, a path dependency and a
    /// registry cache.
    struct Scratch(PathBuf);

    impl Scratch {
        fn new(name: &str) -> Self {
            let mut path = std::env::temp_dir();
            path.push(format!("aevia-manifest-{name}-{}", std::process::id()));
            let _ = std::fs::remove_dir_all(&path);
            std::fs::create_dir_all(&path).expect("scratch");
            Self(path)
        }

        fn write(
            &self,
            name: &str,
            text: &str,
        ) -> PathBuf {
            let path = self.0.join(name);
            std::fs::create_dir_all(path.parent().unwrap()).expect("parent");
            std::fs::write(&path, text).expect("write");
            path
        }

        fn registry(&self) -> DirRegistry {
            DirRegistry::open(self.0.join("reg"))
        }

        /// Publishes `name`/`version` into the cache: the index line and,
        /// when `vendored`, an unpacked source with its checksum marker.
        fn publish(
            &self,
            name: &str,
            index_lines: &[(&str, bool)],
        ) {
            let text: String = index_lines
                .iter()
                .map(|(line, _)| format!("{line}\n"))
                .collect();
            self.write(&format!("reg/index/{name}"), &text);
        }

        fn vendor(
            &self,
            name: &str,
            version: &str,
            checksum: &str,
            dependencies: &str,
        ) {
            let package = name.rsplit('/').next().unwrap();
            let dir = format!("reg/src/{name}/{version}");
            self.write(
                &format!("{dir}/aevia.toml"),
                &format!(
                    "[package]\nname = \"{package}\"\nversion = \"{version}\"\n\n[dependencies]\n{dependencies}"
                ),
            );
            self.write(
                &format!("{dir}/src/{}.aev", import_root(package)),
                "let x = 1\n",
            );
            self.write(&format!("{dir}/{VENDOR_MARKER}"), &format!("{checksum}\n"));
        }
    }

    impl Drop for Scratch {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    fn line(
        name: &str,
        version: &str,
        deps: &[(&str, &str)],
        yanked: bool,
    ) -> (String, bool) {
        let deps: Vec<String> = deps
            .iter()
            .map(|(package, req)| format!("{{\"package\":\"{package}\",\"req\":\"{req}\"}}"))
            .collect();
        (
            format!(
                "{{\"name\":\"{name}\",\"vers\":\"{version}\",\"deps\":[{}],\"cksum\":\"sha256:{}\",\"yanked\":{yanked}}}",
                deps.join(","),
                checksum_of(name, version)
            ),
            yanked,
        )
    }

    fn checksum_of(
        name: &str,
        version: &str,
    ) -> String {
        let mut hash = 0xcbf2_9ce4_8422_2325u64;
        for byte in format!("{name}@{version}").bytes() {
            hash ^= u64::from(byte);
            hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
        }
        format!("{hash:016x}").repeat(4)
    }

    fn publish(
        scratch: &Scratch,
        name: &str,
        lines: &[(String, bool)],
    ) {
        let borrowed: Vec<(&str, bool)> = lines.iter().map(|(l, y)| (l.as_str(), *y)).collect();
        scratch.publish(name, &borrowed);
    }

    fn app(
        scratch: &Scratch,
        dependencies: &str,
    ) -> Package {
        scratch.write(
            "app/aevia.toml",
            &format!(
                "[package]\nname = \"app\"\nversion = \"0.1.0\"\n\n[dependencies]\n{dependencies}"
            ),
        );
        scratch.write("app/src/main.aev", "let x = 1\n");
        Package::find(&scratch.0.join("app")).unwrap().unwrap()
    }

    fn env<'a>(
        registry: &'a DirRegistry,
        locked: LockHints,
        sources: bool,
    ) -> RegistryEnv<'a> {
        RegistryEnv {
            index: Some(registry),
            locked,
            require_sources: sources,
        }
    }

    #[test]
    fn registry_dependencies_resolve_to_the_newest_compatible_versions_and_lock_checksums() {
        let scratch = Scratch::new("resolve");
        publish(
            &scratch,
            "apich/plot",
            &[
                line("apich/plot", "2.0.0", &[("apich/units", "^1")], false),
                line("apich/plot", "2.1.0", &[("apich/units", "^1.2")], false),
                line("apich/plot", "3.0.0", &[], false),
            ],
        );
        publish(
            &scratch,
            "apich/units",
            &[
                line("apich/units", "1.0.0", &[], false),
                line("apich/units", "1.2.0", &[], false),
                line("apich/units", "1.9.0", &[], true),
            ],
        );
        let package = app(
            &scratch,
            "Plot = { package = \"apich/plot\", version = \"^2\" }\n",
        );
        let registry = scratch.registry();
        let lock = package
            .resolve_with(&env(&registry, LockHints::None, false))
            .expect("resolves");
        let by_import: BTreeMap<&str, &LockedPackage> = lock
            .packages
            .iter()
            .map(|p| (p.import.as_str(), p))
            .collect();
        assert_eq!(by_import["Plot"].name, "apich/plot");
        assert_eq!(by_import["Plot"].version, "2.1.0");
        assert_eq!(by_import["Plot"].source, REGISTRY_SOURCE);
        assert_eq!(
            by_import["Plot"].checksum.as_deref(),
            Some(format!("sha256:{}", checksum_of("apich/plot", "2.1.0")).as_str())
        );
        // The dependency's alias is the default import root of its name, and
        // the yanked 1.9.0 is not chosen.
        assert_eq!(by_import["Units"].version, "1.2.0");
        // Deterministic: a second resolution renders the same bytes.
        let again = package
            .resolve_with(&env(&registry, LockHints::None, false))
            .expect("resolves");
        assert_eq!(lock.render(), again.render());
        // The lock names exactly the registry packages it should.
        assert_eq!(
            lock.registry_packages()
                .iter()
                .map(|(n, v, _)| format!("{n} {v}"))
                .collect::<Vec<_>>(),
            ["apich/plot 2.1.0", "apich/units 1.2.0"]
        );
    }

    #[test]
    fn the_lock_keeps_versions_and_a_yanked_one_survives_in_it() {
        let scratch = Scratch::new("locked");
        publish(
            &scratch,
            "apich/units",
            &[
                line("apich/units", "1.0.0", &[], false),
                line("apich/units", "1.1.0", &[], true),
                line("apich/units", "1.2.0", &[], false),
            ],
        );
        let package = app(
            &scratch,
            "Units = { package = \"apich/units\", version = \"^1\" }\n",
        );
        let registry = scratch.registry();
        let mut hints = BTreeMap::new();
        hints.insert(
            "apich/units".to_string(),
            BTreeSet::from([Version::new(1, 1, 0)]),
        );
        let lock = package
            .resolve_with(&env(&registry, LockHints::Map(hints), false))
            .expect("resolves");
        assert_eq!(lock.packages[1].version, "1.1.0", "yanked but locked");
        let fresh = package
            .resolve_with(&env(&registry, LockHints::None, false))
            .expect("resolves");
        assert_eq!(fresh.packages[1].version, "1.2.0");
    }

    #[test]
    fn a_missing_index_is_e0607_and_names_the_package_to_fetch() {
        let scratch = Scratch::new("no-index");
        let package = app(
            &scratch,
            "Plot = { package = \"apich/plot\", version = \"^2\" }\n",
        );
        let registry = scratch.registry();
        let error = package
            .resolve_with(&env(&registry, LockHints::None, false))
            .expect_err("no index");
        assert_eq!(error.code, "E0607");
        assert_eq!(error.missing_index.as_deref(), Some("apich/plot"));
        assert!(error.message.contains("aevia fetch"), "{}", error.message);
    }

    #[test]
    fn building_needs_a_vendored_and_verified_copy_but_locking_does_not() {
        let scratch = Scratch::new("vendor");
        publish(
            &scratch,
            "apich/units",
            &[line("apich/units", "1.0.0", &[], false)],
        );
        let package = app(
            &scratch,
            "Units = { package = \"apich/units\", version = \"^1\" }\n",
        );
        let registry = scratch.registry();
        package
            .resolve_with(&env(&registry, LockHints::None, false))
            .expect("the lock needs only the index");
        let error = package
            .roots_with(&env(&registry, LockHints::None, true))
            .expect_err("not vendored");
        assert_eq!(error.code, "E0607");
        assert!(error.message.contains("not vendored"), "{}", error.message);

        // Vendored with the wrong checksum: refused.
        scratch.vendor("apich/units", "1.0.0", "sha256:bad", "");
        let error = package
            .roots_with(&env(&registry, LockHints::None, true))
            .expect_err("wrong marker");
        assert_eq!(error.code, "E0607");
        assert!(
            error.message.contains("verified archive"),
            "{}",
            error.message
        );

        // Vendored properly: the module root exists and resolves.
        scratch.vendor(
            "apich/units",
            "1.0.0",
            &format!("sha256:{}", checksum_of("apich/units", "1.0.0")),
            "",
        );
        let roots = package
            .roots_with(&env(&registry, LockHints::None, true))
            .expect("roots");
        let file = roots
            .resolve(&["Units".to_string(), "Units".to_string()])
            .expect("the vendored module resolves under its alias");
        assert!(file.ends_with("Units.aev"), "{}", file.display());
    }

    #[test]
    fn a_vendored_manifest_that_disagrees_with_the_index_is_refused() {
        let scratch = Scratch::new("disagree");
        publish(
            &scratch,
            "apich/plot",
            &[line("apich/plot", "1.0.0", &[("apich/units", "^1")], false)],
        );
        publish(
            &scratch,
            "apich/units",
            &[line("apich/units", "1.0.0", &[], false)],
        );
        let package = app(
            &scratch,
            "Plot = { package = \"apich/plot\", version = \"^1\" }\n",
        );
        let registry = scratch.registry();
        let sum = |n: &str| format!("sha256:{}", checksum_of(n, "1.0.0"));
        scratch.vendor("apich/units", "1.0.0", &sum("apich/units"), "");
        // The vendored plot forgot its dependency.
        scratch.vendor("apich/plot", "1.0.0", &sum("apich/plot"), "");
        let error = package
            .roots_with(&env(&registry, LockHints::None, true))
            .expect_err("disagree");
        assert_eq!(error.code, "E0605");
        assert!(error.message.contains("disagree"), "{}", error.message);
        scratch.vendor(
            "apich/plot",
            "1.0.0",
            &sum("apich/plot"),
            "Units = { package = \"apich/units\", version = \"^1\" }\n",
        );
        package
            .roots_with(&env(&registry, LockHints::None, true))
            .expect("now consistent");
    }

    #[test]
    fn unsatisfiable_requirements_are_e0606_with_an_explanation() {
        let scratch = Scratch::new("conflict");
        publish(
            &scratch,
            "apich/units",
            &[line("apich/units", "1.0.0", &[], false)],
        );
        let package = app(
            &scratch,
            "Units = { package = \"apich/units\", version = \"^2\" }\n",
        );
        let registry = scratch.registry();
        let error = package
            .resolve_with(&env(&registry, LockHints::None, false))
            .expect_err("no ^2");
        assert_eq!(error.code, "E0606");
        assert!(
            error.message.contains("apich/units ^2"),
            "{}",
            error.message
        );
        assert!(
            error.message.contains("available: 1.0.0"),
            "{}",
            error.message
        );
    }

    #[test]
    fn two_majors_under_one_import_root_are_e0606_but_distinct_roots_coexist() {
        let scratch = Scratch::new("majors");
        publish(
            &scratch,
            "apich/a",
            &[line("apich/a", "1.0.0", &[("apich/units", "^1")], false)],
        );
        publish(
            &scratch,
            "apich/b",
            &[line("apich/b", "1.0.0", &[("apich/units", "^2")], false)],
        );
        publish(
            &scratch,
            "apich/units",
            &[
                line("apich/units", "1.4.0", &[], false),
                line("apich/units", "2.0.0", &[], false),
            ],
        );
        let package = app(
            &scratch,
            "A = { package = \"apich/a\", version = \"1\" }\nB = { package = \"apich/b\", version = \"1\" }\n",
        );
        let registry = scratch.registry();
        // Both dependents import `Units`, a program-wide name: a conflict, and
        // the message says which two versions fight over it.
        let error = package
            .resolve_with(&env(&registry, LockHints::None, false))
            .expect_err("alias conflict");
        assert_eq!(error.code, "E0606");
        assert!(error.message.contains("`Units`"), "{}", error.message);
        assert!(
            error.message.contains("apich/units 1.4.0")
                && error.message.contains("apich/units 2.0.0"),
            "{}",
            error.message
        );
        // Giving the second dependent a different import root for it lets both
        // majors coexist.
        let renamed = "{\"package\":\"apich/units\",\"req\":\"^2\",\"import\":\"UnitsTwo\"}";
        let line_b = format!(
            "{{\"name\":\"apich/b\",\"vers\":\"1.0.0\",\"deps\":[{renamed}],\"cksum\":\"sha256:{}\"}}",
            checksum_of("apich/b", "1.0.0")
        );
        scratch.publish("apich/b", &[(line_b.as_str(), false)]);
        let registry = scratch.registry();
        let lock = package
            .resolve_with(&env(&registry, LockHints::None, false))
            .expect("coexist");
        let imports: Vec<&str> = lock.packages.iter().map(|p| p.import.as_str()).collect();
        assert!(
            imports.contains(&"Units") && imports.contains(&"UnitsTwo"),
            "{imports:?}"
        );
    }

    #[test]
    fn a_path_package_shares_the_registry_solution_with_the_application() {
        let scratch = Scratch::new("mixed");
        publish(
            &scratch,
            "apich/units",
            &[
                line("apich/units", "1.0.0", &[], false),
                line("apich/units", "1.3.0", &[], false),
            ],
        );
        scratch.write(
            "lib/aevia.toml",
            "[package]\nname = \"lib\"\nversion = \"0.2.0\"\n\n[dependencies]\nUnits = { package = \"apich/units\", version = \"^1.0\" }\n",
        );
        scratch.write("lib/src/main.aev", "let y = 2\n");
        let package = app(
            &scratch,
            "Lib = { path = \"../lib\" }\nUnits = { package = \"apich/units\", version = \"^1.2\" }\n",
        );
        let registry = scratch.registry();
        let lock = package
            .resolve_with(&env(&registry, LockHints::None, false))
            .expect("resolves");
        let units: Vec<&LockedPackage> = lock
            .packages
            .iter()
            .filter(|p| p.name == "apich/units")
            .collect();
        assert_eq!(units.len(), 1, "one alias, one node");
        assert_eq!(units[0].version, "1.3.0");
        assert!(lock.packages.iter().any(|p| p.source == "path+../lib"));
        // The path package alone could be satisfied by 1.0.0, but the shared
        // solution picks one version for both.
        let tree = package
            .tree(&env(&registry, LockHints::None, false))
            .expect("tree");
        assert!(tree.starts_with("app 0.1.0\n"), "{tree}");
        assert!(tree.contains("Lib lib 0.2.0 (path ../lib)"), "{tree}");
        assert!(
            tree.contains("Units apich/units 1.3.0 [registry]"),
            "{tree}"
        );
        assert_eq!(
            tree.matches("apich/units 1.3.0").count(),
            2,
            "both dependents point at the one node: {tree}"
        );
    }

    #[test]
    fn a_dependency_with_both_a_path_and_a_version_uses_the_path_locally() {
        let scratch = Scratch::new("both");
        scratch.write(
            "dev/aevia.toml",
            "[package]\nname = \"dev\"\nversion = \"0.3.1\"\n",
        );
        scratch.write("dev/src/main.aev", "let z = 3\n");
        let package = app(
            &scratch,
            "Dev = { path = \"../dev\", package = \"apich/dev\", version = \"^0.3\" }\n",
        );
        // No registry data is needed: the path wins.
        let lock = package
            .resolve_with(&RegistryEnv {
                index: None,
                locked: LockHints::None,
                require_sources: false,
            })
            .expect("resolves from the path");
        assert_eq!(lock.packages[1].source, "path+../dev");
        assert!(lock.packages[1].hash.is_some() && lock.packages[1].checksum.is_none());
    }

    #[test]
    fn a_registry_dependency_without_a_registry_is_an_error_not_a_guess() {
        let scratch = Scratch::new("none");
        let package = app(&scratch, "Units = \"^1\"\n");
        let error = package
            .resolve_with(&RegistryEnv {
                index: None,
                locked: LockHints::None,
                require_sources: false,
            })
            .expect_err("no registry");
        assert_eq!(error.code, "E0607");
    }

    #[test]
    fn shuffling_the_index_does_not_change_the_lock() {
        let scratch = Scratch::new("shuffle");
        let versions = ["1.0.0", "1.1.0", "1.2.0", "1.3.0"];
        let mut orders: Vec<Vec<usize>> =
            vec![vec![0, 1, 2, 3], vec![3, 2, 1, 0], vec![2, 0, 3, 1]];
        let package = app(
            &scratch,
            "Units = { package = \"apich/units\", version = \"^1.1\" }\n",
        );
        let mut rendered = Vec::new();
        for order in orders.drain(..) {
            let lines: Vec<(String, bool)> = order
                .iter()
                .map(|i| line("apich/units", versions[*i], &[], false))
                .collect();
            publish(&scratch, "apich/units", &lines);
            let registry = scratch.registry();
            rendered.push(
                package
                    .resolve_with(&env(&registry, LockHints::None, false))
                    .expect("resolves")
                    .render(),
            );
        }
        assert!(rendered.windows(2).all(|pair| pair[0] == pair[1]));
        assert!(rendered[0].contains("version = \"1.3.0\""));
    }
}
