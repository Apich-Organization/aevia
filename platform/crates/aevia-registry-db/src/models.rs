//! Row types, one per table.
//!
//! They are plain structs rather than a query DSL: the registry's queries are
//! hand-written SQL, because the interesting ones (the hash-chained log, the
//! "newest version that is not yanked", the aggregate download count) are
//! clearer as SQL than as a builder chain, and a builder would hide the index
//! the query depends on.

use chrono::DateTime;
use chrono::Utc;

/// A publisher: a GitHub account, an ORCID iD or a GitHub organisation.
#[derive(Clone, Debug, sqlx::FromRow)]
pub struct Publisher {
    /// Row id.
    pub id: i64,
    /// `github`, `orcid` or `github-org`.
    pub kind: String,
    /// The normalised handle.
    pub handle: String,
    /// The name the provider shows.
    pub display_name: Option<String>,
    /// The avatar the provider shows.
    pub avatar_url: Option<String>,
    /// When the account first signed in.
    pub created_at: DateTime<Utc>,
}

/// A public key bound to a publisher.
#[derive(Clone, Debug, sqlx::FromRow)]
pub struct SigningKey {
    /// Row id.
    pub id: i64,
    /// The publisher it authenticates.
    pub publisher_id: i64,
    /// `openpgp` or `ssh`.
    pub kind: String,
    /// The fingerprint, normalised.
    pub fingerprint: String,
    /// The ASCII-armoured public key.
    pub public_key: String,
    /// A label the publisher chose.
    pub label: Option<String>,
    /// When it was bound.
    pub created_at: DateTime<Utc>,
    /// When it was revoked, if it was.
    pub revoked_at: Option<DateTime<Utc>>,
}

/// A package, without its versions.
#[derive(Clone, Debug, sqlx::FromRow)]
pub struct Package {
    /// Row id.
    pub id: i64,
    /// The publishing identity.
    pub owner_id: i64,
    /// The name as the author spells it.
    pub name: String,
    /// `lower(name)`.
    pub name_key: String,
    /// One sentence.
    pub description: Option<String>,
    /// An SPDX identifier.
    pub license: Option<String>,
    /// Where the source lives.
    pub repository: Option<String>,
    /// Up to five keywords.
    pub keywords: Vec<String>,
    /// When the package first appeared.
    pub created_at: DateTime<Utc>,
    /// When it last changed.
    pub updated_at: DateTime<Utc>,
}

/// One published version.
#[derive(Clone, Debug, sqlx::FromRow)]
pub struct Version {
    /// Row id.
    pub id: i64,
    /// The package it belongs to.
    pub package_id: i64,
    /// `major.minor.patch`.
    pub version: String,
    /// `sha256:<hex>`.
    pub checksum: String,
    /// Archive size.
    pub size_bytes: i64,
    /// The parsed `aevia.toml`.
    pub manifest: serde_json::Value,
    /// The dependency list, as the index records it.
    pub dependencies: serde_json::Value,
    /// The README at publish time.
    pub readme: Option<String>,
    /// Who signed the publish.
    pub published_by: Option<i64>,
    /// `openpgp` or `ssh`.
    pub key_kind: String,
    /// The signing key's fingerprint.
    pub fingerprint: String,
    /// The armoured detached signature.
    pub signature: String,
    /// The exact bytes that were signed.
    pub statement: String,
    /// Whether the version is yanked.
    pub yanked: bool,
    /// When it was yanked.
    pub yanked_at: Option<DateTime<Utc>>,
    /// `pending`, `built`, `failed` or `skipped`.
    pub docs_status: String,
    /// Why documentation failed, when it did.
    pub docs_error: Option<String>,
    /// When documentation was built.
    pub docs_generated_at: Option<DateTime<Utc>>,
    /// Archive downloads.
    pub downloads: i64,
    /// When it was published.
    pub created_at: DateTime<Utc>,
}

/// One rendered documentation page.
#[derive(Clone, Debug, sqlx::FromRow)]
pub struct DocsPage {
    /// The version it documents.
    pub version_id: i64,
    /// The module path, or `""` for the landing page.
    pub path: String,
    /// The page title.
    pub title: String,
    /// The rendered HTML.
    pub html: String,
    /// Declarations on the page.
    pub decls: i32,
    /// Declarations without a `##` comment.
    pub undocumented: i32,
    /// The source file the module came from.
    pub source_file: String,
}

/// A server-issued nonce, with the statement it authorises.
#[derive(Clone, Debug, sqlx::FromRow)]
pub struct Challenge {
    /// The nonce.
    pub nonce: String,
    /// `publish` or `yank`.
    pub kind: String,
    /// The canonical statement the nonce is baked into.
    pub statement: String,
    /// When it stops being usable.
    pub expires_at: DateTime<Utc>,
    /// When it was used, if it was.
    pub consumed_at: Option<DateTime<Utc>>,
    /// When it was issued.
    pub created_at: DateTime<Utc>,
}

/// A pending device authorisation.
#[derive(Clone, Debug, sqlx::FromRow)]
pub struct DeviceCode {
    /// The secret the client polls with.
    pub device_code: String,
    /// The short code the human types.
    pub user_code: String,
    /// `openpgp` or `ssh`.
    pub key_kind: String,
    /// The key being bound.
    pub public_key: String,
    /// Its fingerprint.
    pub fingerprint: String,
    /// A label the client suggested.
    pub label: Option<String>,
    /// `pending`, `approved`, `denied` or `expired`.
    pub status: String,
    /// Who approved it.
    pub publisher_id: Option<i64>,
    /// When the code stops working.
    pub expires_at: DateTime<Utc>,
    /// When it was created.
    pub created_at: DateTime<Utc>,
}

/// A browser session.
#[derive(Clone, Debug, sqlx::FromRow)]
pub struct Session {
    /// The session id, which is also the cookie value.
    pub id: String,
    /// Who is signed in.
    pub publisher_id: i64,
    /// When it started.
    pub created_at: DateTime<Utc>,
    /// When it ends.
    pub expires_at: DateTime<Utc>,
    /// The browser that started it.
    pub user_agent: Option<String>,
}

/// One entry in the transparency log.
#[derive(Clone, Debug, sqlx::FromRow)]
pub struct LogEntry {
    /// Position in the chain, from zero.
    pub seq: i64,
    /// `publish`, `yank`, `key` or `publisher`.
    pub kind: String,
    /// What happened, in one line.
    pub summary: String,
    /// The structured payload that was hashed.
    pub payload: serde_json::Value,
    /// The previous entry's hash.
    pub prev_hash: String,
    /// This entry's hash.
    pub entry_hash: String,
    /// When it was appended.
    pub created_at: DateTime<Utc>,
}

/// A package as a list shows it: with its owner and its newest version.
#[derive(Clone, Debug, sqlx::FromRow)]
pub struct PackageSummary {
    /// Row id.
    pub id: i64,
    /// The publishing identity.
    pub owner_id: i64,
    /// The owner's handle.
    pub owner_handle: String,
    /// The owner's kind.
    pub owner_kind: String,
    /// The name as the author spells it.
    pub name: String,
    /// One sentence.
    pub description: Option<String>,
    /// An SPDX identifier.
    pub license: Option<String>,
    /// The newest non-yanked version, when there is one.
    pub latest_version: Option<String>,
    /// When the package last changed.
    pub updated_at: DateTime<Utc>,
    /// Downloads across all versions.
    pub downloads: i64,
    /// Whether any version has documentation.
    pub has_docs: bool,
}

/// A published package that depends on another: one row per dependent.
#[derive(Clone, Debug, sqlx::FromRow)]
pub struct Dependent {
    /// The dependent's owner handle.
    pub owner_handle: String,
    /// The dependent's name, as its author spells it.
    pub name: String,
    /// The newest non-yanked version of it that depends on the package.
    pub version: String,
    /// The requirement that version states.
    pub req: String,
}

/// One published version, as the feed lists it.
#[derive(Clone, Debug, sqlx::FromRow)]
pub struct Release {
    /// The owner's handle.
    pub owner_handle: String,
    /// The package name, as its author spells it.
    pub name: String,
    /// The version.
    pub version: String,
    /// The package's sentence.
    pub description: Option<String>,
    /// `sha256:<hex>`.
    pub checksum: String,
    /// When it was published.
    pub created_at: DateTime<Utc>,
}

/// The columns a new version writes.
pub struct NewVersion<'a> {
    /// The package.
    pub package_id: i64,
    /// The version.
    pub version: &'a str,
    /// `sha256:<hex>`.
    pub checksum: &'a str,
    /// Archive size.
    pub size_bytes: i64,
    /// The parsed manifest.
    pub manifest: serde_json::Value,
    /// The dependency list.
    pub dependencies: serde_json::Value,
    /// The README, when the package has one.
    pub readme: Option<&'a str>,
    /// Who signed it.
    pub published_by: i64,
    /// `openpgp` or `ssh`.
    pub key_kind: &'a str,
    /// The fingerprint.
    pub fingerprint: &'a str,
    /// The signature.
    pub signature: &'a str,
    /// The statement.
    pub statement: &'a str,
}
