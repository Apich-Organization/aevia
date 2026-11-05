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
            .any(|tree| file.starts_with(normalise(&package.dir.join(tree))));
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
        let mut roots = Roots {
            source: self.source_dir(),
            local: None,
            packages: BTreeMap::new(),
            entries: BTreeMap::new(),
        };
        let mut seen: BTreeSet<PathBuf> = BTreeSet::new();
        seen.insert(normalise(&self.dir));
        let mut queue: Vec<PathBuf> = vec![self.dir.clone()];
        while let Some(dir) = queue.pop() {
            let manifest = Manifest::load(&dir.join("aevia.toml"))?;
            for (key, dependency) in &manifest.dependencies {
                let Dependency::Path { path } = dependency;
                let dependency_dir = normalise(&dir.join(path));
                let source = dependency_dir.join("src");
                if let Some(existing) = roots.packages.get(key) {
                    if normalise(existing) != normalise(&source) {
                        return Err(ManifestError::new(
                            "E0606",
                            format!(
                                "`{key}` is claimed by both `{}` and `{}`",
                                existing.display(),
                                source.display()
                            ),
                        ));
                    }
                    continue;
                }
                if !dependency_dir.join("aevia.toml").is_file() {
                    return Err(ManifestError::new(
                        "E0607",
                        format!(
                            "`{key}`: `{}` has no `aevia.toml`",
                            dependency_dir.display()
                        ),
                    ));
                }
                roots
                    .entries
                    .insert(key.clone(), dependency_dir.join(&manifest.entry));
                roots.packages.insert(key.clone(), source);
                if seen.insert(dependency_dir.clone()) {
                    queue.push(dependency_dir);
                }
            }
        }
        Ok(roots)
    }

    /// Resolves the lock file for this package.
    ///
    /// # Errors
    ///
    /// The same failures as [`Package::roots`], plus `E0607` for an unreadable
    /// dependency source tree.
    pub fn resolve(&self) -> Result<Lock, ManifestError> {
        let root = LockedPackage {
            name: self.manifest.name.clone(),
            version: self.manifest.version.clone(),
            import: import_root(&self.manifest.name),
            source: "root".to_string(),
            hash: None,
        };
        let mut packages: Vec<LockedPackage> = Vec::new();
        let mut seen: BTreeSet<PathBuf> = BTreeSet::new();
        seen.insert(normalise(&self.dir));
        let mut queue: Vec<PathBuf> = vec![self.dir.clone()];
        while let Some(dir) = queue.pop() {
            let manifest = Manifest::load(&dir.join("aevia.toml"))?;
            for (key, dependency) in &manifest.dependencies {
                let Dependency::Path { path } = dependency;
                let dependency_dir = normalise(&dir.join(path));
                if !seen.insert(dependency_dir.clone()) {
                    continue;
                }
                let dependency_manifest = Manifest::load(&dependency_dir.join("aevia.toml"))?;
                let hash = hash_sources(&dependency_dir.join("src"))?;
                packages.push(LockedPackage {
                    name: dependency_manifest.name,
                    version: dependency_manifest.version,
                    import: key.clone(),
                    // The path is written relative to the *root* package, so
                    // the lock is the same on another checkout of the same
                    // tree rather than naming an absolute directory.
                    source: format!("path+{}", relative_to(&self.dir, &dir.join(path))),
                    hash: Some(hash),
                });
                queue.push(dependency_dir);
            }
        }
        packages.sort_by(|left, right| {
            left.source
                .cmp(&right.source)
                .then_with(|| left.import.cmp(&right.import))
        });
        // The root package is first and the dependencies follow in a fixed
        // order, so the file reads top-down and re-resolving cannot reorder it.
        packages.insert(0, root);
        Ok(Lock { packages })
    }
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
        }
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
        for candidate in self.candidates(path) {
            if candidate.is_file() {
                return Some(candidate);
            }
        }
        None
    }

    /// Every file an import path could have meant, for a `E0607` message.
    #[must_use]
    pub fn candidates(
        &self,
        path: &[String],
    ) -> Vec<PathBuf> {
        let mut out = Vec::new();
        for base in std::iter::once(&self.source).chain(self.local.iter()) {
            let mut candidate = base.clone();
            for segment in path {
                candidate.push(segment);
            }
            candidate.set_extension("aev");
            out.push(candidate);
        }
        if let Some((first, rest)) = path.split_first() {
            if let Some(base) = self.packages.get(first) {
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
    /// `fnv1a64:<hex>` over the dependency's `src/**/*.aev`, absent for the
    /// root package (its own sources are the thing being built).
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
                if !out.pop() {
                    out.push("..");
                }
            },
            | other => out.push(other.as_os_str()),
        }
    }
    out
}

/// `target` as a path relative to `root`, with `/` separators.
fn relative_to(
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
            collect(&path, out)?;
        } else if path.extension().is_some_and(|extension| extension == "aev") {
            out.push(path);
        }
    }
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
