//! Semantic versions, version requirements and dependency resolution for Aevia
//! packages.
//!
//! The crate is pure: it does no I/O and depends on nothing. `aevia-core` (the
//! toolchain) and the registry platform both link it, so "what does `^1.2`
//! mean" and "which versions does `aevia update` pick" have exactly one
//! answer.
//!
//! * [`Version`] — SemVer 2.0.0 with pre-release and build metadata and a total
//!   order.
//! * [`VersionReq`] — `^1.2`, `~1.2.3`, `>=1.2, <2`, `1.*`, `=1.2.3`, `*`.
//! * [`resolve`] — the resolver, over an abstract [`Index`].
//! * [`sparse`] — the registry's sparse-index text as an [`Index`].

#![forbid(unsafe_code)]

pub mod index;
pub mod names;
pub mod req;
pub mod resolve;
pub mod sparse;
pub mod version;

#[cfg(test)]
mod resolve_tests;
#[cfg(test)]
mod testing;

pub use index::Dependency;
pub use index::Index;
pub use index::IndexError;
pub use index::IndexErrorKind;
pub use index::MemoryIndex;
pub use index::Summary;
pub use names::OFFICIAL_SCOPE;
pub use names::canonical_package;
pub use names::default_import;
pub use names::official_package;
pub use req::Bound;
pub use req::Comparator;
pub use req::Interval;
pub use req::Op;
pub use req::VersionReq;
pub use resolve::Cause;
pub use resolve::Conflict;
pub use resolve::Edge;
pub use resolve::Entry;
pub use resolve::Failure;
pub use resolve::Node;
pub use resolve::Options;
pub use resolve::PackageVersion;
pub use resolve::Reason;
pub use resolve::Resolution;
pub use resolve::ResolveError;
pub use resolve::Root;
pub use resolve::Unify;
pub use resolve::resolve;
pub use sparse::SparseDependency;
pub use sparse::SparseError;
pub use sparse::SparseIndex;
pub use sparse::SparseRecord;
pub use sparse::SparseSignature;
pub use version::Identifier;
pub use version::ParseError;
pub use version::Series;
pub use version::Version;
