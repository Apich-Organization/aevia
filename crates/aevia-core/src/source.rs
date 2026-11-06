//! Stable identities for physical source files and host-owned virtual buffers.
//!
//! A source identity does not include a revision, FileId or import alias.
//! Hosts should retain a virtual identity while editing one buffer and assign
//! distinct identities to independent sessions.
use aevia_types::keys::ModuleOrigin;
use aevia_types::keys::ModulePath;
use std::path::Component;
use std::path::Path;
use std::path::PathBuf;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SourceIdentity {
    origin: ModuleOrigin,
    fallback_name: String,
}

impl SourceIdentity {
    /// Resolves a physical source, including a not-yet-created editor buffer.
    /// Existing ancestors are canonicalized so saving a new file, or opening
    /// the same file through a directory symlink, preserves its identity.
    #[must_use]
    pub fn for_path(path: &Path) -> Self {
        let canonical = canonical_path(path);
        let package = crate::manifest::Package::owning(path).ok().flatten();
        if let Some(package) = package {
            return Self::for_package(&package, &canonical);
        }
        let origin = ModuleOrigin {
            package: "<standalone>".into(),
            source: canonical.to_string_lossy().into_owned(),
        };
        Self {
            origin,
            fallback_name: stem(&canonical.to_string_lossy()),
        }
    }

    /// Compiles a source under an explicitly selected package, including shared
    /// physical sources reached through that package's symlinked src tree.
    #[must_use]
    pub fn for_package(
        package: &crate::manifest::Package,
        file: &Path,
    ) -> Self {
        let directory = canonical_path(&package.dir);
        Self::for_owner(
            format!(
                "{}@{}:{}",
                package.manifest.name,
                package.manifest.version,
                directory.display()
            ),
            &directory,
            file,
        )
    }

    /// An explicitly selected package owner, even when the physical source is
    /// outside its directory (for example a symlinked source tree).
    pub(crate) fn for_owner(
        package: String,
        directory: &Path,
        file: &Path,
    ) -> Self {
        let physical = canonical_path(file);
        Self {
            origin: ModuleOrigin {
                package,
                source: crate::manifest::relative_to(directory, &physical),
            },
            fallback_name: stem(&physical.to_string_lossy()),
        }
    }

    /// A host-owned virtual source. Reuse the same URI for revisions of one
    /// document; use a distinct URI for each independent REPL/session.
    #[must_use]
    pub fn virtual_source(uri: impl Into<String>) -> Self {
        let uri = uri.into();
        Self {
            fallback_name: stem(&uri),
            origin: ModuleOrigin {
                package: "<virtual>".into(),
                source: uri,
            },
        }
    }

    /// Interprets file names, file URIs and virtual document names consistently.
    #[must_use]
    pub fn from_name(name: &str) -> Self {
        if let Some(path) = file_uri_path(name) {
            return Self::for_path(&path);
        }
        if has_uri_scheme(name) || (name.starts_with('<') && name.ends_with('>')) {
            Self::virtual_source(name)
        } else {
            Self::for_path(Path::new(name))
        }
    }

    /// The readable declared module name with its independent source origin.
    #[must_use]
    pub fn module_path(
        &self,
        declared: Option<&[String]>,
    ) -> ModulePath {
        let visible = declared.map_or_else(|| vec![self.fallback_name.clone()], <[String]>::to_vec);
        ModulePath::new(visible).with_origin(self.origin.clone())
    }
}

fn stem(name: &str) -> String {
    Path::new(name)
        .file_stem()
        .map(|stem| stem.to_string_lossy().into_owned())
        .filter(|name| !name.is_empty())
        .unwrap_or_else(|| "<entry>".into())
}

/// Canonicalize the longest existing ancestor before folding a missing suffix.
/// This gives a new buffer the identity that an ordinary file creation will
/// have, without requiring that the buffer already exists on disk.
pub fn canonical_path(path: &Path) -> PathBuf {
    let absolute = if path.is_absolute() {
        path.to_path_buf()
    } else {
        std::env::current_dir()
            .map(|dir| dir.join(path))
            .unwrap_or_else(|_| path.to_path_buf())
    };
    let mut prefix = absolute.clone();
    let mut suffix = Vec::new();
    let mut resolved = loop {
        if let Ok(path) = std::fs::canonicalize(&prefix) {
            break path;
        }
        let Some(component) = prefix.components().next_back() else {
            suffix.clear();
            break absolute;
        };
        suffix.push(component.as_os_str().to_owned());
        if !prefix.pop() {
            suffix.clear();
            break absolute;
        }
    };
    for component in suffix.into_iter().rev() {
        resolved.push(component);
    }
    let mut normalized = PathBuf::new();
    for component in resolved.components() {
        match component {
            | Component::CurDir => {},
            | Component::ParentDir if normalized.file_name().is_some_and(|name| name != "..") => {
                normalized.pop();
            },
            | Component::ParentDir if normalized.has_root() => {},
            | other => normalized.push(other.as_os_str()),
        }
    }
    normalized
}

/// Whether a source name has a URI scheme, preserving nonhierarchical editor
/// buffers while keeping native Windows drive spellings as physical paths.
#[must_use]
pub fn has_uri_scheme(name: &str) -> bool {
    let Some((scheme, _)) = name.split_once(':') else {
        return false;
    };
    // A native Windows drive spelling remains a physical path.
    if scheme.len() == 1 && scheme.as_bytes()[0].is_ascii_alphabetic() {
        return false;
    }
    let mut chars = scheme.chars();
    chars
        .next()
        .is_some_and(|first| first.is_ascii_alphabetic())
        && chars.all(|character| {
            character.is_ascii_alphanumeric() || matches!(character, '+' | '-' | '.')
        })
}

/// Decodes an absolute local file URI. Remote authorities, malformed escapes,
/// fragments and invalid UTF-8 do not name a local source file.
#[must_use]
pub fn file_uri_path(name: &str) -> Option<PathBuf> {
    let (scheme, rest) = name.split_once(':')?;
    if !scheme.eq_ignore_ascii_case("file") || rest.contains(['#', '?']) {
        return None;
    }
    let path = if let Some(rest) = rest.strip_prefix("//") {
        let slash = rest.find('/')?;
        let (authority, path) = rest.split_at(slash);
        if !authority.is_empty() && !authority.eq_ignore_ascii_case("localhost") {
            return None;
        }
        path
    } else {
        rest
    };
    if !path.starts_with('/') {
        return None;
    }
    let bytes = path.as_bytes();
    let mut decoded = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'%' {
            let hex = |byte: u8| (byte as char).to_digit(16).map(|value| value as u8);
            let high = hex(*bytes.get(index + 1)?)?;
            let low = hex(*bytes.get(index + 2)?)?;
            let value = (high << 4) | low;
            if value == 0 {
                return None;
            }
            decoded.push(value);
            index += 3;
        } else {
            if bytes[index] == 0 {
                return None;
            }
            decoded.push(bytes[index]);
            index += 1;
        }
    }
    let decoded = String::from_utf8(decoded).ok()?;
    // Keep native Windows drive spellings consistent even in cross-platform
    // editor tests; a POSIX absolute path otherwise retains its leading slash.
    let path = match decoded.strip_prefix('/') {
        | Some(rest)
            if rest.as_bytes().first().is_some_and(u8::is_ascii_alphabetic)
                && rest.as_bytes().get(1) == Some(&b':') =>
        {
            rest
        },
        | _ => &decoded,
    };
    Some(PathBuf::from(path))
}

/// An immutable-at-load snapshot of unsaved physical source buffers. Keys use
/// canonical physical identity, independently of the importing package owner.
/// Inserting another spelling of one physical file replaces its buffer text.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct SourceOverlays {
    sources: std::collections::BTreeMap<PathBuf, std::sync::Arc<str>>,
}

impl SourceOverlays {
    /// A snapshot with no unsaved buffers.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Adds or replaces a physical buffer, including a not-yet-created file.
    pub fn insert(
        &mut self,
        path: &Path,
        source: impl Into<String>,
    ) -> Option<std::sync::Arc<str>> {
        self.sources
            .insert(canonical_path(path), std::sync::Arc::from(source.into()))
    }

    /// The current buffer text, resolving aliases by physical identity.
    #[must_use]
    pub fn get(
        &self,
        path: &Path,
    ) -> Option<&str> {
        self.sources.get(&canonical_path(path)).map(AsRef::as_ref)
    }

    /// Removes a buffer, so subsequent loads read that source from disk.
    pub fn remove(
        &mut self,
        path: &Path,
    ) -> Option<std::sync::Arc<str>> {
        self.sources.remove(&canonical_path(path))
    }

    /// Number of distinct physical buffers in the snapshot.
    #[must_use]
    pub fn len(&self) -> usize {
        self.sources.len()
    }

    /// Whether the snapshot has no physical buffers.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.sources.is_empty()
    }
}

/// An observed input to module resolution, including a candidate that did not
/// exist. A cache watches these exact paths instead of scanning package trees.
#[derive(Clone, Debug)]
pub struct SourceDependency {
    path: PathBuf,
    canonical: PathBuf,
    available: bool,
    read: SourceRead,
}

#[derive(Clone, Debug)]
enum SourceRead {
    NotRead,
    Loaded(String),
    Failed(std::io::ErrorKind),
}

impl SourceDependency {
    pub(crate) fn observe(
        path: &Path,
        overlays: &SourceOverlays,
    ) -> Self {
        Self {
            path: path.to_path_buf(),
            canonical: canonical_path(path),
            available: overlays.get(path).is_some() || path.is_file(),
            read: SourceRead::NotRead,
        }
    }

    pub(crate) const fn available(&self) -> bool {
        self.available
    }

    pub(crate) fn loaded(
        &mut self,
        text: &str,
    ) {
        self.read = SourceRead::Loaded(text.to_owned());
    }

    pub(crate) fn failed(
        &mut self,
        error: &std::io::Error,
    ) {
        self.read = SourceRead::Failed(error.kind());
    }

    /// The logical candidate path selected by its owning package's resolver.
    #[must_use]
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Physical target observed at load time, retained separately from the
    /// logical candidate so hosts can refresh after a symlink is retargeted.
    #[must_use]
    pub fn physical_path(&self) -> &Path {
        &self.canonical
    }

    /// Whether this observed input still describes the effective source.
    /// Contents are compared exactly for read files, even if length and mtime
    /// have not changed. Unselected candidates need only existence/identity.
    #[must_use]
    pub fn is_current(
        &self,
        overlays: &SourceOverlays,
    ) -> bool {
        if canonical_path(&self.path) != self.canonical
            || (overlays.get(&self.path).is_some() || self.path.is_file()) != self.available
        {
            return false;
        }
        match &self.read {
            | SourceRead::NotRead => true,
            | SourceRead::Loaded(expected) => {
                match overlays.get(&self.path) {
                    | Some(text) => text == expected,
                    | None => {
                        std::fs::read_to_string(&self.path).is_ok_and(|text| text == *expected)
                    },
                }
            },
            | SourceRead::Failed(kind) => {
                overlays.get(&self.path).is_none()
                    && std::fs::read_to_string(&self.path).is_err_and(|error| error.kind() == *kind)
            },
        }
    }
}
