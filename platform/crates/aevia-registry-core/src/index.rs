//! The sparse index: one newline-delimited JSON record per version.
//!
//! Cargo's sparse index is a good idea — one small file per crate, so a client
//! fetches only what it resolves, and no git clone of the whole registry. The
//! Aevia registry keeps that shape and changes two things:
//!
//! * **The path is the identity.** `owner/name` is already namespaced, so there
//!   is no prefix-sharding scheme to memorise: the file is at
//!   `/api/v1/index/{owner}/{name}`.
//! * **Every record carries the publisher's signature.** Cargo's index carries
//!   a checksum and nothing that says who chose it. Here the record includes
//!   the fingerprint and the detached signature over the statement that
//!   authorised the version, so a client can verify provenance from the index
//!   alone, without trusting the download endpoint. The checksum proves the
//!   bytes; the signature proves who asked for them.
//!
//! Records are append-only, oldest first, and a yank rewrites only the
//! `yanked` flag of the record it applies to.

use crate::error::Error;
use crate::error::Result;

/// Where a client finds the index and the archives.
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct IndexConfig {
    /// The archive endpoint prefix.
    pub dl: String,
    /// The API endpoint prefix.
    pub api: String,
    /// The registry's name, for `aevia-pkg` to print.
    pub registry: String,
}

impl IndexConfig {
    /// The default configuration for a registry served at `base`.
    #[must_use]
    pub fn new(base: &str) -> Self {
        let base = base.trim_end_matches('/');
        Self {
            dl: format!("{base}/api/v1/download"),
            api: format!("{base}/api/v1"),
            registry: "aevia".to_string(),
        }
    }
}

/// One dependency, as the index records it.
///
/// This is what a resolver needs and nothing else: *which package* and *which
/// versions*. `import` is the root the dependent imports it under; it is
/// written only when it differs from the package's default
/// ([`aevia_resolve::default_import`]), so the common case is two short fields.
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct IndexDependency {
    /// The package, `owner/name` in its canonical (lower-case name) spelling.
    pub package: String,
    /// The version requirement, in its canonical rendering (`^1.2`).
    pub req: String,
    /// The import root, when it is not the package's default.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub import: Option<String>,
}

impl IndexDependency {
    /// The import root the dependent uses.
    #[must_use]
    pub fn label(&self) -> String {
        self.import
            .clone()
            .unwrap_or_else(|| aevia_resolve::default_import(&self.package))
    }
}

/// Reads the dependency list stored with a version.
///
/// Tolerant by design: rows written before the registry understood version
/// requirements hold `{"import", "path"}` entries, which name no package and
/// can never be resolved. They are skipped instead of failing the whole read,
/// so an old version still shows up in the index (with no dependencies) rather
/// than breaking the page.
#[must_use]
pub fn deps_from_json(value: &serde_json::Value) -> Vec<IndexDependency> {
    value
        .as_array()
        .map(|items| {
            items
                .iter()
                .filter_map(|item| serde_json::from_value::<IndexDependency>(item.clone()).ok())
                .collect()
        })
        .unwrap_or_default()
}

/// A strong validator for an index body: clients may send it back as
/// `If-None-Match` and get `304` while nothing was published or yanked.
#[must_use]
pub fn etag(body: &str) -> String {
    use sha2::Digest as _;
    let digest = sha2::Sha256::digest(body.as_bytes());
    format!("\"{}\"", hex::encode(&digest[..12]))
}

/// The signature that authorised a version.
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct IndexSignature {
    /// The key kind, `openpgp` or `ssh`.
    pub kind: String,
    /// The key's fingerprint.
    pub fingerprint: String,
    /// The ASCII-armoured detached signature over the publish statement.
    pub signature: String,
}

/// One version's index record.
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct IndexRecord {
    /// `owner/name`.
    pub name: String,
    /// The version.
    pub vers: String,
    /// Its dependencies.
    #[serde(default)]
    pub deps: Vec<IndexDependency>,
    /// `sha256:<hex>` of the archive.
    pub cksum: String,
    /// Whether the version is yanked.
    #[serde(default)]
    pub yanked: bool,
    /// When it was published, RFC 3339.
    pub published_at: String,
    /// Whether documentation was built for it.
    #[serde(default)]
    pub docs: bool,
    /// The archive's size in bytes.
    #[serde(default)]
    pub size: u64,
    /// The signature that authorised it.
    pub signature: IndexSignature,
}

impl IndexRecord {
    /// The record as one index line.
    ///
    /// # Errors
    ///
    /// `E_JSON` when the record cannot be serialised, which would be a bug.
    pub fn to_line(&self) -> Result<String> {
        serde_json::to_string(self)
            .map_err(|error| Error::json("cannot serialise an index record", &error))
    }

    /// Parses one index line.
    ///
    /// # Errors
    ///
    /// `E_JSON` when the line is not a record.
    pub fn from_line(line: &str) -> Result<Self> {
        serde_json::from_str(line)
            .map_err(|error| Error::json("cannot parse an index record", &error))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn record() -> IndexRecord {
        IndexRecord {
            name: "apich/units-extra".to_string(),
            vers: "0.2.0".to_string(),
            deps: vec![
                IndexDependency {
                    package: "apich/units".to_string(),
                    req: "^1.2".to_string(),
                    import: None,
                },
                IndexDependency {
                    package: "apich/plot".to_string(),
                    req: ">=0.3, <0.5".to_string(),
                    import: Some("Charts".to_string()),
                },
            ],
            cksum: format!("sha256:{}", "ab".repeat(32)),
            yanked: false,
            published_at: "2026-11-08T09:00:00Z".to_string(),
            docs: true,
            size: 1024,
            signature: IndexSignature {
                kind: "openpgp".to_string(),
                fingerprint: "0123456789ABCDEF".to_string(),
                signature: "-----BEGIN PGP SIGNATURE-----\n…".to_string(),
            },
        }
    }

    #[test]
    fn a_record_survives_a_line_round_trip() {
        let record = record();
        let line = record.to_line().expect("serialises");
        assert!(!line.contains('\n'), "an index record is one line");
        assert_eq!(IndexRecord::from_line(&line).expect("parses"), record);
    }

    #[test]
    fn the_default_import_is_not_written() {
        let line = record().to_line().expect("serialises");
        assert!(line.contains("\"import\":\"Charts\""), "{line}");
        assert_eq!(line.matches("\"import\"").count(), 1, "{line}");
        let first = &record().deps[0];
        assert_eq!(first.label(), "Units");
    }

    #[test]
    fn stored_dependencies_are_read_tolerantly() {
        let stored = serde_json::json!([
            {"import": "Old", "path": "../old"},
            {"package": "apich/units", "req": "^1"},
            "nonsense",
        ]);
        let deps = deps_from_json(&stored);
        assert_eq!(deps.len(), 1);
        assert_eq!(deps[0].package, "apich/units");
        assert!(deps_from_json(&serde_json::json!(null)).is_empty());
    }

    #[test]
    fn the_etag_changes_with_the_body() {
        assert_eq!(etag("a"), etag("a"));
        assert_ne!(etag("a"), etag("b"));
        assert!(etag("a").starts_with('"') && etag("a").ends_with('"'));
    }

    #[test]
    fn the_config_points_at_the_api() {
        let config = IndexConfig::new("https://aevia.apich.org/");
        assert_eq!(config.dl, "https://aevia.apich.org/api/v1/download");
        assert_eq!(config.api, "https://aevia.apich.org/api/v1");
    }
}
