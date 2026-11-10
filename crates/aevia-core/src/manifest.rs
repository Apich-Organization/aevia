//! `aevia.toml`, `aevia.lock`, and the package roots an import resolves in.
//!
//! A package is a directory with a manifest, a `src/` tree and a lock file
//! (§7.5). Only **path** dependencies are resolved: there is no registry, so a
//! bare version string is refused rather than silently treated as "whatever is
//! installed". The lock file is written by [`Lock::render`] in a fixed order
//! with fixed field order, so re-resolving an unchanged manifest produces a
//! byte-identical file and a diff always means something changed.
//!
//! The reader is a deliberately small subset of TOML: `[table]` headers,
//! `key = "string"` and inline tables `{ path = "..." }`, with `#` comments and
//! blank lines. Anything outside that subset is `E0605` with the line number —
//! a manifest the tool only half-understands is worse than one it rejects.

use std::collections::BTreeMap;
use std::collections::BTreeSet;
use std::fmt::Write as _;
use std::path::Path;
use std::path::PathBuf;

/// A manifest or resolution failure, with the diagnostic code it reports.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ManifestError {
    /// The §7.7 code: `E0605` malformed manifest, `E0606` conflict, `E0607`
    /// missing package.
    pub code: &'static str,
    /// What went wrong, with the file and line when there is one.
    pub message: String,
}

impl ManifestError {
    fn new(
        code: &'static str,
        message: impl Into<String>,
    ) -> Self {
        Self {
            code,
            message: message.into(),
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
}

/// The contents of `aevia.toml`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Manifest {
    /// `[package] name` — the package name, `snake_case`.
    pub name: String,
    /// `[package] version` — `major.minor.patch`.
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
            }
        }
        out
    }

    /// Parses a manifest.
    ///
    /// # Errors
    ///
    /// `E0605` for anything outside the supported subset, a missing required
    /// key, an invalid name/version, or a registry dependency.
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
            let line = raw.trim();
            let number = index + 1;
            if line.is_empty() || line.starts_with('#') {
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
                    let dependency = parse_dependency(value, origin, number)?;
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
                    dependencies.insert(key.to_string(), dependency);
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
        if !is_version(&version) {
            return Err(ManifestError::new(
                "E0605",
                format!("{origin}: `{version}` is not a `major.minor.patch` version"),
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
    /// # Errors
    ///
    /// `E0606` for a conflicting import root, `E0607` for a missing dependency
    /// or a dependency without a manifest, `E0605` for a dependency whose
    /// manifest does not parse.
    pub fn roots(&self) -> Result<Roots, ManifestError> {
        let graph = PackageGraph::resolve(self)?;
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
    /// package. A dependency digest covers its manifest, source tree and entry
    /// file, so changing module resolution invalidates `--locked` too.
    ///
    /// # Errors
    ///
    /// The same failures as [`Package::roots`], plus unreadable source trees.
    pub fn resolve(&self) -> Result<Lock, ManifestError> {
        let graph = PackageGraph::resolve(self)?;
        let root = LockedPackage {
            name: self.manifest.name.clone(),
            version: self.manifest.version.clone(),
            import: import_root(&self.manifest.name),
            source: "root".to_string(),
            hash: None,
        };
        let mut hashes: BTreeMap<PathBuf, String> = BTreeMap::new();
        let mut packages = Vec::new();
        for (alias, identity) in &graph.aliases {
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
}

/// Package identity is a canonical directory; import names belong to edges,
/// not nodes. Keep this graph shared by lock generation and source resolution.
struct PackageGraph {
    root: PathBuf,
    packages: BTreeMap<PathBuf, Package>,
    aliases: BTreeMap<String, PathBuf>,
}

impl PackageGraph {
    fn resolve(root: &Package) -> Result<Self, ManifestError> {
        let identity = package_identity(&root.dir)?;
        let mut graph = Self {
            root: identity.clone(),
            packages: BTreeMap::new(),
            aliases: BTreeMap::new(),
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
        while let Some(identity) = queue.pop() {
            let package = &graph.packages[&identity];
            let dependencies = package.manifest.dependencies.clone();
            for (alias, dependency) in dependencies {
                let Dependency::Path { path } = dependency;
                let dependency_dir = package_identity(&identity.join(path))?;
                if let Some(existing) = graph.aliases.get(&alias)
                    && existing != &dependency_dir
                {
                    return Err(ManifestError::new(
                        "E0606",
                        format!(
                            "`{alias}` is claimed by both `{}` and `{}`",
                            existing.display(),
                            dependency_dir.display()
                        ),
                    ));
                }
                graph.aliases.insert(alias, dependency_dir.clone());
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
        Ok(graph)
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
    /// The package's own `[package] name`.
    pub name: String,
    /// Its exact version.
    pub version: String,
    /// The root module it is imported under.
    pub import: String,
    /// `root` for the package itself, `path+<as written>` for a path
    /// dependency.
    pub source: String,
    /// `fnv1a64:<hex>` over the dependency's canonical manifest, source tree
    /// and entry file; absent for the root package.
    pub hash: Option<String>,
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
        }
        out
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

fn is_version(version: &str) -> bool {
    let mut parts = version.split('.');
    let mut count = 0;
    for part in &mut parts {
        if part.is_empty() || !part.chars().all(|c| c.is_ascii_digit()) {
            return false;
        }
        count += 1;
    }
    count == 3
}

fn parse_string(value: &str) -> Option<String> {
    let value = value.trim();
    let inner = value.strip_prefix('"')?.strip_suffix('"')?;
    if inner.contains('"') {
        return None;
    }
    Some(inner.to_string())
}

fn parse_dependency(
    value: &str,
    origin: &str,
    number: usize,
) -> Result<Dependency, ManifestError> {
    let value = value.trim();
    if value.starts_with('"') {
        return Err(ManifestError::new(
            "E0605",
            format!(
                "{origin}:{number}: registry dependencies are not supported; write \
                 `{{ path = \"../package\" }}` (this tool resolves path dependencies only)"
            ),
        ));
    }
    let Some(inner) = value
        .strip_prefix('{')
        .and_then(|rest| rest.strip_suffix('}'))
    else {
        return Err(ManifestError::new(
            "E0605",
            format!("{origin}:{number}: expected an inline table `{{ path = \"...\" }}`"),
        ));
    };
    let mut path = None;
    for field in inner.split(',') {
        let field = field.trim();
        if field.is_empty() {
            continue;
        }
        let Some((key, field_value)) = field.split_once('=') else {
            return Err(ManifestError::new(
                "E0605",
                format!("{origin}:{number}: expected `key = value` in `{field}`"),
            ));
        };
        match key.trim() {
            | "path" => {
                path = parse_string(field_value).map(Some).ok_or_else(|| {
                    ManifestError::new(
                        "E0605",
                        format!("{origin}:{number}: `path` expects a quoted string"),
                    )
                })?;
            },
            | other => {
                return Err(ManifestError::new(
                    "E0605",
                    format!(
                        "{origin}:{number}: `{other}` is not supported; a dependency is \
                         `{{ path = \"...\" }}` (no registry, no git, no version ranges)"
                    ),
                ));
            },
        }
    }
    match path {
        | Some(path) => Ok(Dependency::Path { path }),
        | None => {
            Err(ManifestError::new(
                "E0605",
                format!("{origin}:{number}: a dependency needs a `path`"),
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
    fn a_registry_dependency_is_refused_rather_than_ignored() {
        // A bare version string is the one thing a resolver could pretend to
        // handle. Refusing it is the only honest answer without a registry.
        let error = Manifest::parse(
            "[package]\nname = \"a\"\nversion = \"1.0.0\"\n\n[dependencies]\naevia-std = \"0.1\"\n",
            "aevia.toml",
        )
        .expect_err("registry dependency");
        assert_eq!(error.code, "E0605");
        assert!(error.message.contains("path"), "{}", error.message);

        let error = Manifest::parse(
            "[package]\nname = \"a\"\nversion = \"1.0.0\"\n\n[dependencies]\nA = { git = \"x\" }\n",
            "aevia.toml",
        )
        .expect_err("git dependency");
        assert_eq!(error.code, "E0605");
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
        let error = Manifest::parse(
            "[package]\nname = \"a\"\nversion = \"1.0.0\"\n\n[dependencies]\nunits-extra = { path = \"../x\" }\n",
            "aevia.toml",
        )
        .expect_err("bad key");
        assert_eq!(error.code, "E0605");
    }

    #[test]
    fn a_lock_file_round_trips_byte_for_byte() {
        let lock = Lock {
            packages: vec![
                LockedPackage {
                    name: "kinematics".to_string(),
                    version: "0.1.0".to_string(),
                    import: "Kinematics".to_string(),
                    source: "root".to_string(),
                    hash: None,
                },
                LockedPackage {
                    name: "units-extra".to_string(),
                    version: "0.2.0".to_string(),
                    import: "UnitsExtra".to_string(),
                    source: "path+../units-extra".to_string(),
                    hash: Some("fnv1a64:0123456789abcdef".to_string()),
                },
            ],
        };
        let text = lock.render();
        let parsed = Lock::parse(&text, "aevia.lock").expect("parses");
        assert_eq!(lock, parsed);
        assert_eq!(text, parsed.render());
        // The rendering is the identity on itself: a second write is a no-op,
        // which is what makes a diff mean a real change.
        assert_eq!(text, Lock::parse(&text, "aevia.lock").unwrap().render());
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
}
