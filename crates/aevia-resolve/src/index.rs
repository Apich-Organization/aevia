//! The data the resolver reads: an abstract package index.
//!
//! The resolver never touches a file or a socket. It asks an [`Index`] for the
//! versions of a package and what each one depends on, and the caller decides
//! where that comes from: a sparse-index text vendored on disk, a registry
//! response, or a [`MemoryIndex`] in a test.

use std::collections::BTreeMap;
use std::fmt;

use crate::req::VersionReq;
use crate::version::ParseError;
use crate::version::Version;

/// One dependency edge as the index records it.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct Dependency {
    /// The name the dependent imports it under (`UnitsExtra`). The resolver
    /// carries it through to [`crate::Edge`] and otherwise ignores it.
    pub label: String,
    /// The package's key in the index (`apich/units-extra`).
    pub package: String,
    /// Which versions are acceptable.
    pub req: VersionReq,
}

impl Dependency {
    /// A dependency whose label is its package key.
    #[must_use]
    pub fn new(
        package: &str,
        req: VersionReq,
    ) -> Self {
        Self {
            label: package.to_string(),
            package: package.to_string(),
            req,
        }
    }
}

/// One published version of a package, as far as resolution is concerned.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Summary {
    /// The version.
    pub version: Version,
    /// What it depends on.
    pub dependencies: Vec<Dependency>,
    /// Withdrawn: selectable only when a lock already names it.
    pub yanked: bool,
}

/// Why an index could not answer.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum IndexErrorKind {
    /// The index is complete and has no such package.
    Missing,
    /// The index could not look: not vendored yet, offline, unreadable. The
    /// resolver stops instead of guessing, so a caller can fetch and retry.
    Unavailable,
}

/// An index failure.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct IndexError {
    /// What kind of failure.
    pub kind: IndexErrorKind,
    /// The package that was asked for.
    pub package: String,
    /// A sentence for the user.
    pub message: String,
}

impl IndexError {
    /// "No such package."
    #[must_use]
    pub fn missing(package: &str) -> Self {
        Self {
            kind: IndexErrorKind::Missing,
            package: package.to_string(),
            message: format!("`{package}` is not in the index"),
        }
    }

    /// "Could not look."
    #[must_use]
    pub fn unavailable(
        package: &str,
        message: impl Into<String>,
    ) -> Self {
        Self {
            kind: IndexErrorKind::Unavailable,
            package: package.to_string(),
            message: message.into(),
        }
    }
}

impl fmt::Display for IndexError {
    fn fmt(
        &self,
        formatter: &mut fmt::Formatter<'_>,
    ) -> fmt::Result {
        formatter.write_str(&self.message)
    }
}

impl std::error::Error for IndexError {}

/// A source of package versions.
pub trait Index {
    /// Every version of `package`, in any order.
    ///
    /// # Errors
    ///
    /// [`IndexErrorKind::Missing`] for an unknown package (resolution treats it
    /// as "no candidates" and says so in the explanation) and
    /// [`IndexErrorKind::Unavailable`] when the data cannot be read at all
    /// (resolution stops with that error).
    fn summaries(
        &self,
        package: &str,
    ) -> Result<Vec<Summary>, IndexError>;
}

/// An index held in memory: tests, and the glue for callers that parse a
/// sparse-index text themselves.
#[derive(Clone, Debug, Default)]
pub struct MemoryIndex {
    packages: BTreeMap<String, Vec<Summary>>,
}

impl MemoryIndex {
    /// An empty index.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Adds a version with `(package, requirement)` dependencies.
    ///
    /// # Panics
    ///
    /// On a malformed version or requirement: this is a convenience for tests.
    #[allow(
        clippy::expect_used,
        reason = "fixture convenience retained for existing callers; use try_add for parsed input"
    )]
    pub fn add(
        &mut self,
        package: &str,
        version: &str,
        dependencies: &[(&str, &str)],
    ) -> &mut Self {
        self.try_add(package, version, dependencies, false)
            .expect("valid test fixture")
    }

    /// Adds a yanked version.
    ///
    /// # Panics
    ///
    /// As [`MemoryIndex::add`].
    #[allow(
        clippy::expect_used,
        reason = "fixture convenience retained for existing callers; use try_add for parsed input"
    )]
    pub fn add_yanked(
        &mut self,
        package: &str,
        version: &str,
        dependencies: &[(&str, &str)],
    ) -> &mut Self {
        self.try_add(package, version, dependencies, true)
            .expect("valid test fixture")
    }

    /// Parses and adds a version with `(package, requirement)` dependencies.
    ///
    /// # Errors
    ///
    /// A malformed version or dependency requirement. The index is updated
    /// only after the version and every requirement parse successfully.
    pub fn try_add(
        &mut self,
        package: &str,
        version: &str,
        dependencies: &[(&str, &str)],
        yanked: bool,
    ) -> Result<&mut Self, ParseError> {
        let summary = Summary {
            version: Version::parse(version)?,
            dependencies: dependencies
                .iter()
                .map(|(name, req)| Ok(Dependency::new(name, VersionReq::parse(req)?)))
                .collect::<Result<_, ParseError>>()?,
            yanked,
        };
        self.insert(package, summary);
        Ok(self)
    }

    /// Adds a prepared summary.
    pub fn insert(
        &mut self,
        package: &str,
        summary: Summary,
    ) {
        self.packages
            .entry(package.to_string())
            .or_default()
            .push(summary);
    }

    /// The package keys, sorted.
    pub fn packages(&self) -> impl Iterator<Item = &str> {
        self.packages.keys().map(String::as_str)
    }
}

impl Index for MemoryIndex {
    fn summaries(
        &self,
        package: &str,
    ) -> Result<Vec<Summary>, IndexError> {
        self.packages
            .get(package)
            .cloned()
            .ok_or_else(|| IndexError::missing(package))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_malformed_requirement_does_not_insert_the_package() {
        let mut index = MemoryIndex::new();
        let invalid = index.try_add("o/a", "1.0.0", &[("o/b", "^1"), ("o/c", "^x")], false);
        assert!(invalid.is_err());
        assert_eq!(index.packages().count(), 0);
    }
}
