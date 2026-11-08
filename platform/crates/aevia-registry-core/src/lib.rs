//! Domain types shared by the Aevia registry, its server and its client.
//!
//! Everything in this crate is pure: it parses, validates, hashes, packs and
//! verifies. It never talks to a database, a network or a browser, so both the
//! server and the `aevia-pkg` client can agree on a wire format by depending on
//! the same code rather than on the same prose.
//!
//! The pieces, in the order a publish travels through them:
//!
//! * [`name`] — `owner/name`, the identity of a package. The owner is a
//!   verified identity (a GitHub login or an ORCID iD), never a name the
//!   registry invented.
//! * [`manifest`] — `aevia.toml`, read with exactly the subset of TOML the
//!   toolchain's own reader accepts, so the registry cannot accept a package
//!   that `aevia check` would reject.
//! * [`tarball`] — the deterministic `.tar.gz` that is the artifact, and its
//!   SHA-256.
//! * [`statement`] — the canonical text a publisher signs. There is no API
//!   token anywhere: the signature *is* the credential.
//! * [`signature`] — OpenPGP (recommended) and SSH signature verification.
//! * [`index`] — the sparse-index record clients resolve versions from.
//! * [`vm`] — the view models the server hands to the Leptos pages.

#![forbid(unsafe_code)]

pub mod error;
pub mod index;
pub mod manifest;
pub mod name;
pub mod signature;
pub mod statement;
pub mod tarball;
pub mod vm;

pub use error::Error;
pub use error::Result;
pub use name::PackageId;
