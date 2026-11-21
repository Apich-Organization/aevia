//! Every query the server runs.
//!
//! The functions are grouped by the table they own. Two of them deserve a note
//! before you read them:
//!
//! * [`append_log`] serialises itself with an advisory lock. A hash chain is
//!   only a chain if exactly one writer reads the head and appends the next
//!   link; two concurrent publishes that both read the same head would fork it,
//!   and a forked log proves nothing.
//! * [`consume_challenge`] is a single `UPDATE … RETURNING`, not a read
//!   followed by a write. The nonce is the only thing standing between a
//!   captured signature and a replay, so it is spent atomically or not at all.

use chrono::DateTime;
use chrono::Utc;
use sha2::Digest as _;
use sqlx::FromRow as _;
use sqlx::PgPool;
use sqlx::Row as _;

use aevia_registry_core::Error;
use aevia_registry_core::vm::LogEntryView;
use aevia_registry_core::vm::ModuleLink;
use aevia_registry_core::vm::SiteStats;

use crate::models::Challenge;
use crate::models::DeviceCode;
use crate::models::DocsJob;
use crate::models::DocsPage;
use crate::models::GithubOrganisation;
use crate::models::LogEntry;
use crate::models::LogHead;
use crate::models::NewVersion;
use crate::models::Package;
use crate::models::PackageSummary;
use crate::models::Publisher;
use crate::models::Session;
use crate::models::SigningKey;
use crate::models::Version;
use crate::sql_error;

// ---------------------------------------------------------------- publishers

/// Creates or refreshes a publisher identified by a verified ORCID iD.
///
/// Returns `None` if another provider already owns that registry namespace.
///
/// # Errors
///
/// `E_DB` when the row cannot be written.
pub async fn upsert_orcid_publisher(
    pool: &PgPool,
    handle: &str,
    display_name: Option<&str>,
) -> Result<Option<Publisher>, Error> {
    let mut transaction = pool
        .begin()
        .await
        .map_err(|error| sql_error("cannot begin the ORCID identity transaction", error))?;
    lock_namespace(&mut transaction, handle).await?;
    let publisher = sqlx::query_as::<_, Publisher>(
        "INSERT INTO publishers (kind, handle, display_name)
         SELECT 'orcid', $1, $2
         WHERE NOT EXISTS (
             SELECT 1 FROM publishers WHERE lower(handle) = lower($1) AND kind <> 'orcid'
         ) OR EXISTS (
             SELECT 1 FROM publishers WHERE handle = $1 AND kind = 'orcid'
         )
         ON CONFLICT (kind, handle) DO UPDATE
            SET display_name = COALESCE(EXCLUDED.display_name, publishers.display_name)
         RETURNING *",
    )
    .bind(handle)
    .bind(display_name)
    .fetch_optional(&mut *transaction)
    .await
    .map_err(|error| sql_error("cannot record the ORCID publisher", error))?;
    transaction
        .commit()
        .await
        .map_err(|error| sql_error("cannot finish the ORCID identity transaction", error))?;
    Ok(publisher)
}

/// Records a GitHub user by their durable account ID.
///
/// An existing binding keeps its namespace even when the GitHub login changes.
/// Returns `None` if the login cannot be claimed by a new registry identity.
///
/// # Errors
///
/// Returns the database error when the identity cannot be recorded.
pub async fn upsert_github_user(
    pool: &PgPool,
    github_id: i64,
    login: &str,
    display_name: Option<&str>,
    avatar_url: Option<&str>,
) -> Result<Option<Publisher>, Error> {
    let mut transaction = pool
        .begin()
        .await
        .map_err(|error| sql_error("cannot begin the GitHub identity transaction", error))?;
    let publisher = upsert_github_publisher(
        &mut transaction,
        "github",
        github_id,
        login,
        display_name,
        avatar_url,
    )
    .await?;
    transaction
        .commit()
        .await
        .map_err(|error| sql_error("cannot finish the GitHub identity transaction", error))?;
    Ok(publisher)
}

async fn upsert_github_publisher(
    transaction: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    kind: &str,
    github_id: i64,
    login: &str,
    display_name: Option<&str>,
    avatar_url: Option<&str>,
) -> Result<Option<Publisher>, Error> {
    let existing = sqlx::query_as::<_, Publisher>(
        "UPDATE publishers
         SET display_name = COALESCE($3, display_name), avatar_url = COALESCE($4, avatar_url)
         WHERE kind = $1 AND github_id = $2
         RETURNING *",
    )
    .bind(kind)
    .bind(github_id)
    .bind(display_name)
    .bind(avatar_url)
    .fetch_optional(&mut **transaction)
    .await
    .map_err(|error| sql_error("cannot refresh the GitHub publisher", error))?;
    if existing.is_some() {
        return Ok(existing);
    }
    let handle = login.to_ascii_lowercase();
    if github_id <= 0
        || !aevia_registry_core::name::valid_github_login(&handle)
        || aevia_registry_core::name::orcid_checksum_ok(&handle)
    {
        return Ok(None);
    }
    lock_namespace(transaction, &handle).await?;
    let inserted = sqlx::query_as::<_, Publisher>(
        "INSERT INTO publishers (kind, github_id, handle, display_name, avatar_url)
         SELECT $1, $2, $3, $4, $5
         WHERE NOT EXISTS (SELECT 1 FROM publishers WHERE lower(handle) = $3)
         ON CONFLICT (kind, github_id) DO UPDATE
            SET display_name = COALESCE(EXCLUDED.display_name, publishers.display_name),
                avatar_url = COALESCE(EXCLUDED.avatar_url, publishers.avatar_url)
         RETURNING *",
    )
    .bind(kind)
    .bind(github_id)
    .bind(handle)
    .bind(display_name)
    .bind(avatar_url)
    .fetch_optional(&mut **transaction)
    .await
    .map_err(|error| sql_error("cannot record the GitHub publisher", error))?;
    if inserted.is_some() {
        return Ok(inserted);
    }
    // A concurrent sign-in may have reserved this namespace for the same ID.
    sqlx::query_as::<_, Publisher>("SELECT * FROM publishers WHERE kind = $1 AND github_id = $2")
        .bind(kind)
        .bind(github_id)
        .fetch_optional(&mut **transaction)
        .await
        .map_err(|error| sql_error("cannot resolve the GitHub account binding", error))
}

async fn lock_namespace(
    transaction: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    handle: &str,
) -> Result<(), Error> {
    // String ownership is claimed once, separately from provider identity.
    sqlx::query("SELECT pg_advisory_xact_lock(71326, hashtext(lower($1)))")
        .bind(handle)
        .execute(&mut **transaction)
        .await
        .map_err(|error| sql_error("cannot lock the publisher namespace", error))?;
    Ok(())
}

/// One publisher by id.
///
/// # Errors
///
/// `E_DB` when the query fails.
pub async fn publisher(
    pool: &PgPool,
    id: i64,
) -> Result<Option<Publisher>, Error> {
    sqlx::query_as::<_, Publisher>("SELECT * FROM publishers WHERE id = $1")
        .bind(id)
        .fetch_optional(pool)
        .await
        .map_err(|error| sql_error("cannot read the publisher", error))
}

/// One publisher by handle and kind.
///
/// # Errors
///
/// `E_DB` when the query fails.
pub async fn publisher_by_handle(
    pool: &PgPool,
    kind: &str,
    handle: &str,
) -> Result<Option<Publisher>, Error> {
    sqlx::query_as::<_, Publisher>("SELECT * FROM publishers WHERE kind = $1 AND handle = $2")
        .bind(kind)
        .bind(handle)
        .fetch_optional(pool)
        .await
        .map_err(|error| sql_error("cannot read the publisher", error))
}

/// A publisher by handle alone, whichever provider it belongs to.
///
/// The order preserves the canonical row if a legacy namespace collides.
///
/// # Errors
///
/// `E_DB` when the query fails.
pub async fn publisher_by_any_handle(
    pool: &PgPool,
    handle: &str,
) -> Result<Option<Publisher>, Error> {
    sqlx::query_as::<_, Publisher>(
        "SELECT * FROM publishers WHERE lower(handle) = lower($1)
         ORDER BY CASE kind WHEN 'github' THEN 0 WHEN 'orcid' THEN 1 ELSE 2 END, id
         LIMIT 1",
    )
    .bind(handle)
    .fetch_optional(pool)
    .await
    .map_err(|error| sql_error("cannot read the publisher", error))
}

/// The newest releases across the registry, for the feed.
///
/// # Errors
///
/// `E_DB` when the query fails.
pub async fn recent_releases(
    pool: &PgPool,
    limit: i64,
) -> Result<Vec<crate::models::Release>, Error> {
    sqlx::query_as::<_, crate::models::Release>(
        "SELECT o.handle AS owner_handle, p.name AS name, v.version AS version,
                p.description AS description, v.checksum AS checksum, v.created_at AS created_at
         FROM versions v
         JOIN packages p ON p.id = v.package_id
         JOIN publishers o ON o.id = p.owner_id
         ORDER BY v.created_at DESC
         LIMIT $1",
    )
    .bind(limit)
    .fetch_all(pool)
    .await
    .map_err(|error| sql_error("cannot read the recent releases", error))
}

// ---------------------------------------------------------------------- keys

/// Binds a public key to a publisher.
///
/// # Errors
///
/// `E_CONFLICT` when the key is already bound, `E_DB` otherwise.
pub async fn add_signing_key(
    pool: &PgPool,
    publisher_id: i64,
    kind: &str,
    fingerprint: &str,
    public_key: &str,
    label: Option<&str>,
) -> Result<SigningKey, Error> {
    sqlx::query_as::<_, SigningKey>(
        "INSERT INTO signing_keys (publisher_id, kind, fingerprint, public_key, label)
         VALUES ($1, $2, $3, $4, $5)
         RETURNING *",
    )
    .bind(publisher_id)
    .bind(kind)
    .bind(fingerprint)
    .bind(public_key)
    .bind(label)
    .fetch_one(pool)
    .await
    .map_err(|error| {
        if let sqlx::Error::Database(ref database) = error
            && database.is_unique_violation()
        {
            return Error::new(
                crate::E_CONFLICT,
                format!(
                    "the key {fingerprint} is already bound to a publisher; a key may sign \
                         for one account only"
                ),
            );
        }
        sql_error("cannot bind the key", error)
    })
}

/// The live key with this fingerprint, if any.
///
/// # Errors
///
/// `E_DB` when the query fails.
pub async fn signing_key_by_fingerprint(
    pool: &PgPool,
    kind: &str,
    fingerprint: &str,
) -> Result<Option<SigningKey>, Error> {
    sqlx::query_as::<_, SigningKey>(
        "SELECT * FROM signing_keys
         WHERE kind = $1 AND fingerprint = $2 AND revoked_at IS NULL",
    )
    .bind(kind)
    .bind(fingerprint)
    .fetch_optional(pool)
    .await
    .map_err(|error| sql_error("cannot read the key", error))
}

/// A published verification certificate, including any key retired for signing.
///
/// # Errors
///
/// Returns the database error when the certificate lookup fails.
pub async fn recorded_signing_key(
    pool: &PgPool,
    kind: &str,
    fingerprint: &str,
) -> Result<Option<SigningKey>, Error> {
    sqlx::query_as::<_, SigningKey>(
        "SELECT * FROM signing_keys WHERE kind = $1 AND fingerprint = $2",
    )
    .bind(kind)
    .bind(fingerprint)
    .fetch_optional(pool)
    .await
    .map_err(|error| sql_error("cannot read the verification certificate", error))
}

/// A publisher's keys, newest first.
///
/// # Errors
///
/// `E_DB` when the query fails.
pub async fn signing_keys(
    pool: &PgPool,
    publisher_id: i64,
) -> Result<Vec<SigningKey>, Error> {
    sqlx::query_as::<_, SigningKey>(
        "SELECT * FROM signing_keys WHERE publisher_id = $1 ORDER BY created_at DESC",
    )
    .bind(publisher_id)
    .fetch_all(pool)
    .await
    .map_err(|error| sql_error("cannot read the keys", error))
}

/// Revokes one of a publisher's keys.
///
/// # Errors
///
/// `E_DB` when the query fails.
pub async fn revoke_signing_key(
    pool: &PgPool,
    publisher_id: i64,
    id: i64,
) -> Result<bool, Error> {
    let result = sqlx::query(
        "UPDATE signing_keys SET revoked_at = now()
         WHERE id = $1 AND publisher_id = $2 AND revoked_at IS NULL",
    )
    .bind(id)
    .bind(publisher_id)
    .execute(pool)
    .await
    .map_err(|error| sql_error("cannot revoke the key", error))?;
    Ok(result.rows_affected() == 1)
}

// ------------------------------------------------------------------ packages

/// A package by id.
///
/// # Errors
///
/// `E_DB` when the query fails.
pub async fn package(
    pool: &PgPool,
    id: i64,
) -> Result<Option<Package>, Error> {
    sqlx::query_as::<_, Package>("SELECT * FROM packages WHERE id = $1")
        .bind(id)
        .fetch_optional(pool)
        .await
        .map_err(|error| sql_error("cannot read the package", error))
}

/// A package by owner and folded name.
///
/// # Errors
///
/// `E_DB` when the query fails.
pub async fn package_by_identity(
    pool: &PgPool,
    owner_id: i64,
    name_key: &str,
) -> Result<Option<Package>, Error> {
    sqlx::query_as::<_, Package>("SELECT * FROM packages WHERE owner_id = $1 AND name_key = $2")
        .bind(owner_id)
        .bind(name_key)
        .fetch_optional(pool)
        .await
        .map_err(|error| sql_error("cannot read the package", error))
}

/// A package and its owner, by the owner's handle and the folded name.
///
/// Two queries rather than one join: `FromRow` maps a whole row, and a joined
/// row would need a third struct that exists only here. Two indexed lookups are
/// cheap, and the order of the owner lookup is fixed so a page never flips
/// between two owners with the same handle.
///
/// # Errors
///
/// `E_DB` when either query fails.
pub async fn package_by_handle(
    pool: &PgPool,
    owner_handle: &str,
    name_key: &str,
) -> Result<Option<(Package, Publisher)>, Error> {
    let Some(owner) = publisher_by_any_handle(pool, owner_handle).await? else {
        return Ok(None);
    };
    let package = package_by_identity(pool, owner.id, name_key).await?;
    Ok(package.map(|package| (package, owner)))
}

/// Creates a package row.
///
/// `name_key` is populated automatically by PostgreSQL 18 as a virtual
/// generated column (`lower(name)`).
///
/// # Errors
///
/// `E_CONFLICT` when the owner already has a package with that name.
pub async fn create_package(
    pool: &PgPool,
    owner_id: i64,
    name: &str,
) -> Result<Package, Error> {
    sqlx::query_as::<_, Package>(
        "INSERT INTO packages (owner_id, name) VALUES ($1, $2) RETURNING NEW.*",
    )
    .bind(owner_id)
    .bind(name)
    .fetch_one(pool)
    .await
    .map_err(|error| sql_error("cannot create the package", error))
}

/// Locks or creates the canonical package row in the publication transaction.
///
/// # Errors
///
/// `E_DB` when the package cannot be recorded.
pub async fn publication_package(
    transaction: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    owner_id: i64,
    name: &str,
) -> Result<Package, Error> {
    sqlx::query_as::<_, Package>(
        "INSERT INTO packages (owner_id, name) VALUES ($1, $2)
         ON CONFLICT (owner_id, name_key) DO UPDATE SET name = packages.name
         RETURNING *",
    )
    .bind(owner_id)
    .bind(name)
    .fetch_one(&mut **transaction)
    .await
    .map_err(|error| sql_error("cannot reserve the publication package", error))
}

/// Cargo-style package metadata recorded during a publish.
#[derive(Clone, Copy, Debug, Default)]
pub struct PackageMetaUpdate<'a> {
    /// Language edition (`aevia.toml` `[package] edition`).
    pub edition: Option<&'a str>,
    /// Package authors (`aevia.toml` `[package] authors`).
    pub authors: &'a [String],
    /// Short description.
    pub description: Option<&'a str>,
    /// SPDX license expression.
    pub license: Option<&'a str>,
    /// Path to a custom license file inside the package.
    pub license_file: Option<&'a str>,
    /// Source repository URL.
    pub repository: Option<&'a str>,
    /// Project homepage URL.
    pub homepage: Option<&'a str>,
    /// Documentation URL.
    pub documentation: Option<&'a str>,
    /// Up to five search keywords.
    pub keywords: &'a [String],
    /// Up to five registry categories.
    pub categories: &'a [String],
}

/// Records the metadata a publish supplied.
///
/// # Errors
///
/// `E_DB` when the query fails.
pub async fn update_package_meta<'e, E: sqlx::PgExecutor<'e>>(
    pool: E,
    id: i64,
    meta: &PackageMetaUpdate<'_>,
) -> Result<(), Error> {
    sqlx::query(
        "UPDATE packages
         SET edition = COALESCE($2, edition),
             authors = CASE WHEN cardinality($3::text[]) > 0 THEN $3 ELSE authors END,
             description = COALESCE($4, description),
             license = COALESCE($5, license),
             license_file = COALESCE($6, license_file),
             repository = COALESCE($7, repository),
             homepage = COALESCE($8, homepage),
             documentation = COALESCE($9, documentation),
             keywords = CASE WHEN cardinality($10::text[]) > 0 THEN $10 ELSE keywords END,
             categories = CASE WHEN cardinality($11::text[]) > 0 THEN $11 ELSE categories END
         WHERE id = $1",
    )
    .bind(id)
    .bind(meta.edition)
    .bind(meta.authors)
    .bind(meta.description)
    .bind(meta.license)
    .bind(meta.license_file)
    .bind(meta.repository)
    .bind(meta.homepage)
    .bind(meta.documentation)
    .bind(meta.keywords)
    .bind(meta.categories)
    .execute(pool)
    .await
    .map_err(|error| sql_error("cannot update the package", error))?;
    Ok(())
}

/// Bumps a package's `updated_at`.
///
/// # Errors
///
/// `E_DB` when the query fails.
pub async fn touch_package<'e, E: sqlx::PgExecutor<'e>>(
    pool: E,
    id: i64,
) -> Result<(), Error> {
    sqlx::query("UPDATE packages SET updated_at = now() WHERE id = $1")
        .bind(id)
        .execute(pool)
        .await
        .map_err(|error| sql_error("cannot touch the package", error))?;
    Ok(())
}

const SUMMARY_COLUMNS: &str = "p.id, p.owner_id, o.handle AS owner_handle, o.kind AS owner_kind,
    p.name, p.description, p.license, p.updated_at,
    (SELECT v.version FROM versions v
      WHERE v.package_id = p.id AND NOT v.yanked
      ORDER BY v.created_at DESC LIMIT 1) AS latest_version,
    (SELECT COALESCE(SUM(v.downloads), 0)::bigint FROM versions v WHERE v.package_id = p.id) AS downloads,
    EXISTS (SELECT 1 FROM versions v
             WHERE v.package_id = p.id AND v.docs_status = 'built') AS has_docs";

const PACKAGE_SEARCH_WHERE: &str = "(NOT $1 OR p.name ILIKE $2 ESCAPE '\\'
    OR COALESCE(p.description, '') ILIKE $2 ESCAPE '\\'
    OR EXISTS (SELECT 1 FROM unnest(p.keywords) k WHERE k ILIKE $2 ESCAPE '\\')
    OR EXISTS (SELECT 1 FROM unnest(p.categories) c WHERE c ILIKE $2 ESCAPE '\\'))";

/// A page of packages, with the total count.
///
/// # Errors
///
/// `E_DB` when either query fails.
pub async fn list_packages(
    pool: &PgPool,
    query: &str,
    sort: &str,
    limit: i64,
    offset: i64,
) -> Result<(Vec<PackageSummary>, i64), Error> {
    let trimmed = query.trim();
    let has_query = !trimmed.is_empty();
    let pattern = format!("%{}%", escape_like(trimmed));
    let order = match sort {
        | "downloads" => "downloads DESC, p.name_key ASC",
        | "name" => "p.name_key ASC, o.handle ASC",
        | _ => "p.updated_at DESC, p.name_key ASC",
    };
    let sql = format!(
        "SELECT {SUMMARY_COLUMNS}
         FROM packages p JOIN publishers o ON o.id = p.owner_id
         WHERE {PACKAGE_SEARCH_WHERE}
         ORDER BY {order}
         LIMIT $3 OFFSET $4"
    );
    let items = sqlx::query_as::<_, PackageSummary>(&sql)
        .bind(has_query)
        .bind(&pattern)
        .bind(limit)
        .bind(offset)
        .fetch_all(pool)
        .await
        .map_err(|error| sql_error("cannot list packages", error))?;
    let count_sql = format!("SELECT count(*) FROM packages p WHERE {PACKAGE_SEARCH_WHERE}");
    let total: i64 = sqlx::query_scalar(&count_sql)
        .bind(has_query)
        .bind(&pattern)
        .fetch_one(pool)
        .await
        .map_err(|error| sql_error("cannot count packages", error))?;
    Ok((items, total))
}

/// Every package a publisher owns, for the account page.
///
/// # Errors
///
/// `E_DB` when the query fails.
pub async fn packages_for_publisher(
    pool: &PgPool,
    publisher_id: i64,
) -> Result<Vec<PackageSummary>, Error> {
    let sql = format!(
        "SELECT {SUMMARY_COLUMNS}
         FROM packages p JOIN publishers o ON o.id = p.owner_id
         WHERE p.owner_id = $1
         ORDER BY p.updated_at DESC"
    );
    sqlx::query_as::<_, PackageSummary>(&sql)
        .bind(publisher_id)
        .fetch_all(pool)
        .await
        .map_err(|error| sql_error("cannot list the publisher's packages", error))
}

// ------------------------------------------------------------------ versions

/// Writes a new version and its dependency rows in one transaction.
///
/// # Errors
///
/// `E_CONFLICT` when the version already exists.
pub async fn insert_version(
    pool: &PgPool,
    new: &NewVersion<'_>,
) -> Result<Version, Error> {
    let mut transaction = pool
        .begin()
        .await
        .map_err(|error| sql_error("cannot start the publish transaction", error))?;
    let version = insert_version_in(&mut transaction, new).await?;
    transaction
        .commit()
        .await
        .map_err(|error| sql_error("cannot commit the version", error))?;
    Ok(version)
}

/// [`insert_version`] inside a transaction the caller owns, so the publish can
/// keep the catalogue, the archive file and the log entry in step.
///
/// # Errors
///
/// `E_CONFLICT` when the version already exists.
pub async fn insert_version_in(
    transaction: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    new: &NewVersion<'_>,
) -> Result<Version, Error> {
    let version = sqlx::query_as::<_, Version>(
        "INSERT INTO versions (
            package_id, version, checksum, size_bytes, manifest, dependencies, readme,
            published_by, key_kind, fingerprint, signature, statement, docs_status
         ) VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12,
                   CASE WHEN $13 THEN 'pending' ELSE 'skipped' END)
         RETURNING *",
    )
    .bind(new.package_id)
    .bind(new.version)
    .bind(new.checksum)
    .bind(new.size_bytes)
    .bind(&new.manifest)
    .bind(&new.dependencies)
    .bind(new.readme)
    .bind(new.published_by)
    .bind(new.key_kind)
    .bind(new.fingerprint)
    .bind(new.signature)
    .bind(new.statement)
    .bind(new.build_docs)
    .fetch_one(&mut **transaction)
    .await
    .map_err(|error| sql_error("cannot record the version", error))?;
    for dependency in aevia_registry_core::index::deps_from_json(&new.dependencies) {
        sqlx::query(
            "INSERT INTO version_dependencies (version_id, dep_key, req, import)
             VALUES ($1, $2, $3, $4)
             ON CONFLICT DO NOTHING",
        )
        .bind(version.id)
        .bind(dependency.package.to_ascii_lowercase())
        .bind(&dependency.req)
        .bind(&dependency.import)
        .execute(&mut **transaction)
        .await
        .map_err(|error| sql_error("cannot record a dependency", error))?;
    }
    Ok(version)
}

/// The newest non-yanked version of every package that depends on `dep_key`
/// (`owner/name`, name folded), at most `limit` packages.
///
/// # Errors
///
/// `E_DB` when the query fails.
pub async fn reverse_dependencies(
    pool: &PgPool,
    dep_key: &str,
    limit: i64,
) -> Result<Vec<crate::models::Dependent>, Error> {
    sqlx::query_as::<_, crate::models::Dependent>(
        "SELECT * FROM (
             SELECT DISTINCT ON (p.id)
                    o.handle AS owner_handle, p.name AS name, v.version AS version, d.req AS req
             FROM version_dependencies d
             JOIN versions v ON v.id = d.version_id
             JOIN packages p ON p.id = v.package_id
             JOIN publishers o ON o.id = p.owner_id
             WHERE d.dep_key = $1 AND NOT v.yanked
             ORDER BY p.id, v.created_at DESC
         ) AS newest
         ORDER BY owner_handle, name
         LIMIT $2",
    )
    .bind(dep_key)
    .bind(limit)
    .fetch_all(pool)
    .await
    .map_err(|error| sql_error("cannot read the reverse dependencies", error))
}

/// How many distinct packages depend on `dep_key` through a non-yanked version.
///
/// # Errors
///
/// `E_DB` when the query fails.
pub async fn dependents_count(
    pool: &PgPool,
    dep_key: &str,
) -> Result<i64, Error> {
    let (count,): (i64,) = sqlx::query_as(
        "SELECT COUNT(DISTINCT v.package_id)
         FROM version_dependencies d JOIN versions v ON v.id = d.version_id
         WHERE d.dep_key = $1 AND NOT v.yanked",
    )
    .bind(dep_key)
    .fetch_one(pool)
    .await
    .map_err(|error| sql_error("cannot count the dependents", error))?;
    Ok(count)
}

/// Every version of a package, newest first.
///
/// # Errors
///
/// `E_DB` when the query fails.
pub async fn versions(
    pool: &PgPool,
    package_id: i64,
) -> Result<Vec<Version>, Error> {
    sqlx::query_as::<_, Version>(
        "SELECT * FROM versions WHERE package_id = $1 ORDER BY created_at DESC",
    )
    .bind(package_id)
    .fetch_all(pool)
    .await
    .map_err(|error| sql_error("cannot read the versions", error))
}

/// One version by number.
///
/// # Errors
///
/// `E_DB` when the query fails.
pub async fn version(
    pool: &PgPool,
    package_id: i64,
    version: &str,
) -> Result<Option<Version>, Error> {
    sqlx::query_as::<_, Version>("SELECT * FROM versions WHERE package_id = $1 AND version = $2")
        .bind(package_id)
        .bind(version)
        .fetch_optional(pool)
        .await
        .map_err(|error| sql_error("cannot read the version", error))
}

/// The newest version that is not yanked.
///
/// # Errors
///
/// `E_DB` when the query fails.
pub async fn latest_version(
    pool: &PgPool,
    package_id: i64,
) -> Result<Option<Version>, Error> {
    sqlx::query_as::<_, Version>(
        "SELECT * FROM versions WHERE package_id = $1 AND NOT yanked
         ORDER BY created_at DESC LIMIT 1",
    )
    .bind(package_id)
    .fetch_optional(pool)
    .await
    .map_err(|error| sql_error("cannot read the latest version", error))
}

/// Sets or clears the yank flag, returning the updated version and whether the
/// state actually changed (via PostgreSQL 18 `RETURNING OLD` / `NEW`).
///
/// # Errors
///
/// `E_DB` when the query fails.
pub async fn set_yanked_with_changed<'e, E: sqlx::PgExecutor<'e>>(
    pool: E,
    package_id: i64,
    version: &str,
    yanked: bool,
) -> Result<Option<(Version, bool)>, Error> {
    let row = sqlx::query(
        "UPDATE versions
         SET yanked = $3, yanked_at = CASE WHEN $3 THEN now() ELSE NULL END
         WHERE package_id = $1 AND version = $2
         RETURNING (OLD.yanked IS DISTINCT FROM NEW.yanked) AS changed, NEW.*",
    )
    .bind(package_id)
    .bind(version)
    .bind(yanked)
    .fetch_optional(pool)
    .await
    .map_err(|error| sql_error("cannot change the yank flag", error))?;

    row.map(|row| {
        let changed: bool = row
            .try_get("changed")
            .map_err(|error| sql_error("cannot read yank transition flag", error))?;
        let version = Version::from_row(&row)
            .map_err(|error| sql_error("cannot decode yanked version", error))?;
        Ok((version, changed))
    })
    .transpose()
}

/// Locks a package before altering one of its versions.
///
/// # Errors
///
/// `E_DB` when the row cannot be locked.
pub async fn lock_package(
    transaction: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    package_id: i64,
) -> Result<(), Error> {
    sqlx::query("SELECT id FROM packages WHERE id = $1 FOR UPDATE")
        .bind(package_id)
        .execute(&mut **transaction)
        .await
        .map_err(|error| sql_error("cannot lock the package", error))?;
    Ok(())
}

/// Sets or clears the yank flag.
///
/// # Errors
///
/// `E_DB` when the query fails.
pub async fn set_yanked<'e, E: sqlx::PgExecutor<'e>>(
    pool: E,
    package_id: i64,
    version: &str,
    yanked: bool,
) -> Result<Option<Version>, Error> {
    Ok(set_yanked_with_changed(pool, package_id, version, yanked)
        .await?
        .map(|(version, _)| version))
}

/// Records the outcome of building documentation.
///
/// # Errors
///
/// `E_DB` when the query fails.
pub async fn set_docs_status<'e, E: sqlx::PgExecutor<'e>>(
    executor: E,
    version_id: i64,
    status: &str,
    error: Option<&str>,
) -> Result<(), Error> {
    sqlx::query(
        "UPDATE versions
         SET docs_status = $2,
             docs_error = $3,
             docs_generated_at = CASE WHEN $2 = 'built' THEN now() ELSE docs_generated_at END
         WHERE id = $1",
    )
    .bind(version_id)
    .bind(status)
    .bind(error)
    .execute(executor)
    .await
    .map_err(|error| sql_error("cannot record the documentation state", error))?;
    Ok(())
}

/// Counts one download.
///
/// # Errors
///
/// `E_DB` when the query fails.
pub async fn increment_downloads(
    pool: &PgPool,
    version_id: i64,
) -> Result<(), Error> {
    sqlx::query("UPDATE versions SET downloads = downloads + 1 WHERE id = $1")
        .bind(version_id)
        .execute(pool)
        .await
        .map_err(|error| sql_error("cannot count the download", error))?;
    Ok(())
}

/// Adds a batch of download counts grouped by version.
///
/// # Errors
///
/// Returns the database error when the counts cannot be added.
pub async fn increment_download_batch(
    pool: &PgPool,
    versions: &[i64],
    counts: &[i64],
) -> Result<(), Error> {
    sqlx::query(
        "UPDATE versions v SET downloads = v.downloads + batch.downloads
         FROM unnest($1::bigint[], $2::bigint[]) AS batch(id, downloads)
         WHERE v.id = batch.id",
    )
    .bind(versions)
    .bind(counts)
    .execute(pool)
    .await
    .map_err(|error| sql_error("cannot count the downloads", error))?;
    Ok(())
}

/// Downloads across every version of a package.
///
/// # Errors
///
/// `E_DB` when the query fails.
pub async fn package_downloads(
    pool: &PgPool,
    package_id: i64,
) -> Result<i64, Error> {
    sqlx::query_scalar(
        "SELECT COALESCE(SUM(downloads), 0)::bigint FROM versions WHERE package_id = $1",
    )
    .bind(package_id)
    .fetch_one(pool)
    .await
    .map_err(|error| sql_error("cannot sum the downloads", error))
}

/// The versions of a package that have documentation, newest first.
///
/// # Errors
///
/// `E_DB` when the query fails.
pub async fn versions_with_docs(
    pool: &PgPool,
    package_id: i64,
) -> Result<Vec<String>, Error> {
    sqlx::query_scalar(
        "SELECT version FROM versions
         WHERE package_id = $1 AND docs_status = 'built'
         ORDER BY created_at DESC",
    )
    .bind(package_id)
    .fetch_all(pool)
    .await
    .map_err(|error| sql_error("cannot list the documented versions", error))
}

// ---------------------------------------------------------------------- docs

/// Claims the earliest pending documentation job until the transaction ends.
///
/// # Errors
///
/// `E_DB` when the queue cannot be read.
pub async fn next_docs_job(
    transaction: &mut sqlx::Transaction<'_, sqlx::Postgres>
) -> Result<Option<DocsJob>, Error> {
    sqlx::query_as(
        "SELECT v.id AS version_id, v.version, v.checksum, o.handle AS owner, p.name
         FROM versions v
         JOIN packages p ON p.id = v.package_id
         JOIN publishers o ON o.id = p.owner_id
         WHERE v.docs_status = 'pending'
         ORDER BY v.id
         LIMIT 1
         FOR UPDATE OF v SKIP LOCKED",
    )
    .fetch_optional(&mut **transaction)
    .await
    .map_err(|error| sql_error("cannot claim a documentation job", error))
}

/// Replaces every page of a version's documentation in one transaction.
///
/// A rebuild writes the whole set: a page that no longer exists must disappear,
/// and a half-applied rebuild would serve a sidebar that disagrees with the
/// page next to it.
///
/// # Errors
///
/// `E_DB` when the transaction fails.
pub async fn replace_docs(
    pool: &PgPool,
    version_id: i64,
    pages: &[DocsPage],
) -> Result<(), Error> {
    let mut transaction = pool
        .begin()
        .await
        .map_err(|error| sql_error("cannot start the documentation transaction", error))?;
    replace_docs_in(&mut transaction, version_id, pages).await?;
    transaction
        .commit()
        .await
        .map_err(|error| sql_error("cannot commit the documentation", error))?;
    Ok(())
}

/// Replaces a version's documentation inside an existing transaction.
///
/// # Errors
///
/// `E_DB` when the existing pages cannot be replaced.
pub async fn replace_docs_in(
    transaction: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    version_id: i64,
    pages: &[DocsPage],
) -> Result<(), Error> {
    sqlx::query("DELETE FROM docs_pages WHERE version_id = $1")
        .bind(version_id)
        .execute(&mut **transaction)
        .await
        .map_err(|error| sql_error("cannot clear the documentation", error))?;

    // PostgreSQL accepts at most 65535 parameters in one statement.
    for batch in pages.chunks(4096) {
        let mut insert = sqlx::QueryBuilder::new(
            "INSERT INTO docs_pages (version_id, path, title, html, decls, undocumented, source_file) ",
        );
        insert.push_values(batch, |mut row, page| {
            row.push_bind(version_id)
                .push_bind(&page.path)
                .push_bind(&page.title)
                .push_bind(&page.html)
                .push_bind(page.decls)
                .push_bind(page.undocumented)
                .push_bind(&page.source_file);
        });
        insert
            .build()
            .execute(&mut **transaction)
            .await
            .map_err(|error| sql_error("cannot write documentation pages", error))?;
    }
    Ok(())
}

/// Every page of a version's documentation, the landing page first.
///
/// # Errors
///
/// `E_DB` when the query fails.
pub async fn docs_pages(
    pool: &PgPool,
    version_id: i64,
) -> Result<Vec<DocsPage>, Error> {
    sqlx::query_as::<_, DocsPage>(
        "SELECT * FROM docs_pages WHERE version_id = $1 ORDER BY (path = '') DESC, path ASC",
    )
    .bind(version_id)
    .fetch_all(pool)
    .await
    .map_err(|error| sql_error("cannot read the documentation", error))
}

/// The module sidebar without reading any rendered HTML.
///
/// # Errors
///
/// `E_DB` when the module outline cannot be read.
pub async fn docs_outline(
    pool: &PgPool,
    version_id: i64,
) -> Result<Vec<ModuleLink>, Error> {
    let rows = sqlx::query_as::<_, (String, i32, i32)>(
        "SELECT path, decls, undocumented FROM docs_pages
         WHERE version_id = $1 AND path <> '' ORDER BY path",
    )
    .bind(version_id)
    .fetch_all(pool)
    .await
    .map_err(|error| sql_error("cannot read the documentation outline", error))?;
    Ok(rows
        .into_iter()
        .map(|(path, decls, undocumented)| {
            ModuleLink {
                path,
                decls: usize::try_from(decls).unwrap_or(0),
                undocumented: usize::try_from(undocumented).unwrap_or(0),
            }
        })
        .collect())
}

/// One page of a version's documentation.
///
/// # Errors
///
/// `E_DB` when the query fails.
pub async fn docs_page(
    pool: &PgPool,
    version_id: i64,
    path: &str,
) -> Result<Option<DocsPage>, Error> {
    sqlx::query_as::<_, DocsPage>("SELECT * FROM docs_pages WHERE version_id = $1 AND path = $2")
        .bind(version_id)
        .bind(path)
        .fetch_optional(pool)
        .await
        .map_err(|error| sql_error("cannot read the documentation page", error))
}

// ---------------------------------------------------------------- challenges

/// Stores a challenge for a statement the server just issued.
///
/// # Errors
///
/// `E_DB` when the query fails.
pub async fn create_challenge(
    pool: &PgPool,
    nonce: &str,
    kind: &str,
    statement: &str,
    expires_at: DateTime<Utc>,
) -> Result<(), Error> {
    sqlx::query(
        "INSERT INTO challenges (nonce, kind, statement, expires_at) VALUES ($1, $2, $3, $4)",
    )
    .bind(nonce)
    .bind(kind)
    .bind(statement)
    .bind(expires_at)
    .execute(pool)
    .await
    .map_err(|error| sql_error("cannot record the challenge", error))?;
    Ok(())
}

/// Spends a nonce, returning the challenge it belonged to.
///
/// `None` means the nonce is unknown, expired, of the wrong kind, or already
/// spent — the caller must not distinguish those cases to the client.
///
/// # Errors
///
/// `E_DB` when the query fails.
pub async fn consume_challenge(
    pool: &PgPool,
    nonce: &str,
    kind: &str,
) -> Result<Option<Challenge>, Error> {
    sqlx::query_as::<_, Challenge>(
        "UPDATE challenges SET consumed_at = now()
         WHERE nonce = $1 AND kind = $2 AND consumed_at IS NULL AND expires_at > now()
         RETURNING *",
    )
    .bind(nonce)
    .bind(kind)
    .fetch_optional(pool)
    .await
    .map_err(|error| sql_error("cannot spend the challenge", error))
}

/// Deletes challenges that expired more than an hour ago.
///
/// # Errors
///
/// `E_DB` when the query fails.
pub async fn purge_expired(pool: &PgPool) -> Result<(), Error> {
    sqlx::query("DELETE FROM challenges WHERE expires_at < now() - interval '1 hour'")
        .execute(pool)
        .await
        .map_err(|error| sql_error("cannot purge challenges", error))?;
    sqlx::query("DELETE FROM sessions WHERE expires_at < now()")
        .execute(pool)
        .await
        .map_err(|error| sql_error("cannot purge sessions", error))?;
    sqlx::query("DELETE FROM device_codes WHERE expires_at < now() - interval '1 day'")
        .execute(pool)
        .await
        .map_err(|error| sql_error("cannot purge device codes", error))?;
    Ok(())
}

// -------------------------------------------------------------------- device

/// Records a device authorisation request.
///
/// # Errors
///
/// `E_DB` when the query fails.
#[allow(clippy::too_many_arguments)]
pub async fn create_device(
    pool: &PgPool,
    device_code: &str,
    user_code: &str,
    key_kind: &str,
    public_key: &str,
    fingerprint: &str,
    label: Option<&str>,
    expires_at: DateTime<Utc>,
) -> Result<(), Error> {
    sqlx::query(
        "INSERT INTO device_codes
            (device_code, user_code, key_kind, public_key, fingerprint, label, expires_at)
         VALUES ($1, $2, $3, $4, $5, $6, $7)",
    )
    .bind(device_code)
    .bind(user_code)
    .bind(key_kind)
    .bind(public_key)
    .bind(fingerprint)
    .bind(label)
    .bind(expires_at)
    .execute(pool)
    .await
    .map_err(|error| sql_error("cannot record the device code", error))?;
    Ok(())
}

/// A device request by its secret code.
///
/// # Errors
///
/// `E_DB` when the query fails.
pub async fn device_by_code(
    pool: &PgPool,
    device_code: &str,
) -> Result<Option<DeviceCode>, Error> {
    sqlx::query_as::<_, DeviceCode>("SELECT * FROM device_codes WHERE device_code = $1")
        .bind(device_code)
        .fetch_optional(pool)
        .await
        .map_err(|error| sql_error("cannot read the device code", error))
}

/// A device request by the code a human types.
///
/// # Errors
///
/// `E_DB` when the query fails.
pub async fn device_by_user_code(
    pool: &PgPool,
    user_code: &str,
) -> Result<Option<DeviceCode>, Error> {
    sqlx::query_as::<_, DeviceCode>("SELECT * FROM device_codes WHERE user_code = $1")
        .bind(user_code)
        .fetch_optional(pool)
        .await
        .map_err(|error| sql_error("cannot read the device code", error))
}

/// Marks a device request approved and binds its key.
///
/// # Errors
///
/// `E_CONFLICT` when the key is already bound elsewhere, `E_DB` otherwise.
pub async fn approve_device(
    pool: &PgPool,
    device_code: &str,
    publisher_id: i64,
) -> Result<bool, Error> {
    let mut transaction = pool
        .begin()
        .await
        .map_err(|error| sql_error("cannot start the approval transaction", error))?;
    // Claim the pending code and read its key fields atomically in one query
    // using PostgreSQL 18 `RETURNING NEW.*` rather than a separate pre-read.
    let approved: Option<(String, String, String, Option<String>)> = sqlx::query_as(
        "UPDATE device_codes SET status = 'approved', publisher_id = $2
         WHERE device_code = $1 AND status = 'pending' AND expires_at > now()
         RETURNING NEW.key_kind, NEW.fingerprint, NEW.public_key, NEW.label",
    )
    .bind(device_code)
    .bind(publisher_id)
    .fetch_optional(&mut *transaction)
    .await
    .map_err(|error| sql_error("cannot approve the device code", error))?;
    let Some((key_kind, fingerprint, public_key, label)) = approved else {
        return Ok(false);
    };
    sqlx::query(
        "INSERT INTO signing_keys (publisher_id, kind, fingerprint, public_key, label)
         VALUES ($1, $2, $3, $4, $5)
         ON CONFLICT (kind, fingerprint) DO UPDATE
            SET revoked_at = NULL, label = COALESCE(EXCLUDED.label, signing_keys.label)
            WHERE signing_keys.publisher_id = EXCLUDED.publisher_id",
    )
    .bind(publisher_id)
    .bind(&key_kind)
    .bind(&fingerprint)
    .bind(&public_key)
    .bind(label.as_deref())
    .execute(&mut *transaction)
    .await
    .map_err(|error| sql_error("cannot bind the key", error))?;
    transaction
        .commit()
        .await
        .map_err(|error| sql_error("cannot commit the approval", error))?;
    Ok(true)
}

/// Marks a device request denied.
///
/// # Errors
///
/// `E_DB` when the query fails.
pub async fn deny_device(
    pool: &PgPool,
    device_code: &str,
) -> Result<bool, Error> {
    let result = sqlx::query(
        "UPDATE device_codes SET status = 'denied'
         WHERE device_code = $1 AND status = 'pending'",
    )
    .bind(device_code)
    .execute(pool)
    .await
    .map_err(|error| sql_error("cannot deny the device code", error))?;
    Ok(result.rows_affected() == 1)
}

// ------------------------------------------------------------------ sessions

/// Records a browser session.
///
/// # Errors
///
/// `E_DB` when the query fails.
pub async fn create_session(
    pool: &PgPool,
    id: &str,
    publisher_id: i64,
    expires_at: DateTime<Utc>,
    user_agent: Option<&str>,
) -> Result<(), Error> {
    sqlx::query(
        "INSERT INTO sessions (id, publisher_id, expires_at, user_agent)
         VALUES ($1, $2, $3, $4)",
    )
    .bind(id)
    .bind(publisher_id)
    .bind(expires_at)
    .bind(user_agent)
    .execute(pool)
    .await
    .map_err(|error| sql_error("cannot record the session", error))?;
    Ok(())
}

/// The session and the publisher behind it, if it is still live.
///
/// # Errors
///
/// `E_DB` when the query fails.
pub async fn session_publisher(
    pool: &PgPool,
    id: &str,
) -> Result<Option<(Session, Publisher)>, Error> {
    let session =
        sqlx::query_as::<_, Session>("SELECT * FROM sessions WHERE id = $1 AND expires_at > now()")
            .bind(id)
            .fetch_optional(pool)
            .await
            .map_err(|error| sql_error("cannot read the session", error))?;
    let Some(session) = session else {
        return Ok(None);
    };
    let Some(publisher) = publisher(pool, session.publisher_id).await? else {
        return Ok(None);
    };
    if publisher.kind == "github" && publisher.github_id.is_none() {
        return Ok(None);
    }
    Ok(Some((session, publisher)))
}

/// Ends a session.
///
/// # Errors
///
/// `E_DB` when the query fails.
pub async fn delete_session(
    pool: &PgPool,
    id: &str,
) -> Result<(), Error> {
    sqlx::query("DELETE FROM sessions WHERE id = $1")
        .bind(id)
        .execute(pool)
        .await
        .map_err(|error| sql_error("cannot end the session", error))?;
    Ok(())
}

// --------------------------------------------------------------- oauth state

/// Records an OAuth `state` value.
///
/// # Errors
///
/// `E_DB` when the query fails.
pub async fn create_oauth_state(
    pool: &PgPool,
    state: &str,
    provider: &str,
    redirect: &str,
    expires_at: DateTime<Utc>,
) -> Result<(), Error> {
    sqlx::query(
        "INSERT INTO oauth_states (state, provider, redirect, expires_at)
         VALUES ($1, $2, $3, $4)",
    )
    .bind(state)
    .bind(provider)
    .bind(redirect)
    .bind(expires_at)
    .execute(pool)
    .await
    .map_err(|error| sql_error("cannot record the OAuth state", error))?;
    Ok(())
}

/// Spends an OAuth `state` value, returning where to send the browser next.
///
/// # Errors
///
/// `E_DB` when the query fails.
pub async fn take_oauth_state(
    pool: &PgPool,
    state: &str,
    provider: &str,
) -> Result<Option<String>, Error> {
    let redirect: Option<String> = sqlx::query_scalar(
        "DELETE FROM oauth_states
         WHERE state = $1 AND provider = $2 AND expires_at > now()
         RETURNING redirect",
    )
    .bind(state)
    .bind(provider)
    .fetch_optional(pool)
    .await
    .map_err(|error| sql_error("cannot spend the OAuth state", error))?;
    Ok(redirect)
}

// ---------------------------------------------------------------- membership

/// Starts a fresh GitHub membership observation and invalidates its previous grants.
///
/// # Errors
///
/// Returns an error for an unbound publisher or when the change cannot be saved.
pub async fn begin_membership_refresh(
    pool: &PgPool,
    publisher_id: i64,
) -> Result<i64, Error> {
    let mut transaction = pool
        .begin()
        .await
        .map_err(|error| sql_error("cannot begin the GitHub membership check", error))?;
    let generation = sqlx::query_scalar::<_, i64>(
        "UPDATE publishers SET github_membership_generation = github_membership_generation + 1
         WHERE id = $1 AND kind = 'github' AND github_id IS NOT NULL
         RETURNING github_membership_generation",
    )
    .bind(publisher_id)
    .fetch_optional(&mut *transaction)
    .await
    .map_err(|error| sql_error("cannot start the GitHub membership check", error))?
    .ok_or_else(|| {
        Error::new(
            "E_FORBIDDEN",
            "the publisher has no verified GitHub account",
        )
    })?;
    sqlx::query("DELETE FROM org_members WHERE publisher_id = $1")
        .bind(publisher_id)
        .execute(&mut *transaction)
        .await
        .map_err(|error| {
            sql_error(
                "cannot invalidate previous GitHub organisation grants",
                error,
            )
        })?;
    transaction
        .commit()
        .await
        .map_err(|error| sql_error("cannot commit the GitHub membership check", error))?;
    Ok(generation)
}

/// Records the current observation's active, verified GitHub organisation grants.
///
/// A newer observation supersedes any older result. Organisations with reserved
/// namespaces and no matching provider ID do not receive an automatic binding.
///
/// # Errors
///
/// Returns the database error when the refreshed grants cannot be stored.
pub async fn finish_membership_refresh(
    pool: &PgPool,
    publisher_id: i64,
    generation: i64,
    organisations: &[GithubOrganisation],
) -> Result<(), Error> {
    let mut transaction = pool
        .begin()
        .await
        .map_err(|error| sql_error("cannot begin the membership update", error))?;
    let current = sqlx::query_scalar::<_, i64>(
        "UPDATE publishers SET github_membership_generation = github_membership_generation + 1
         WHERE id = $1 AND kind = 'github' AND github_id IS NOT NULL
           AND github_membership_generation = $2
         RETURNING id",
    )
    .bind(publisher_id)
    .bind(generation)
    .fetch_optional(&mut *transaction)
    .await
    .map_err(|error| sql_error("cannot lock the current GitHub membership check", error))?;
    if current.is_none() {
        return Ok(());
    }
    let mut organisations: Vec<_> = organisations.iter().collect();
    organisations.sort_unstable_by_key(|organisation| organisation.github_id);
    for organisation in organisations {
        let Some(owner) = upsert_github_publisher(
            &mut transaction,
            "github-org",
            organisation.github_id,
            &organisation.login,
            None,
            organisation.avatar_url.as_deref(),
        )
        .await?
        else {
            continue;
        };
        sqlx::query(
            "INSERT INTO org_members (org_id, publisher_id) VALUES ($1, $2)
             ON CONFLICT (org_id, publisher_id) DO NOTHING",
        )
        .bind(owner.id)
        .bind(publisher_id)
        .execute(&mut *transaction)
        .await
        .map_err(|error| sql_error("cannot store the verified GitHub organisation grant", error))?;
    }
    transaction
        .commit()
        .await
        .map_err(|error| sql_error("cannot commit verified GitHub organisation grants", error))?;
    Ok(())
}

/// The publisher id a bound signing key may publish under for the requested namespace.
///
/// A personal scope follows the signing key's publisher row. An organisation
/// scope also requires a current grant between its bound GitHub identities.
///
/// # Errors
///
/// Returns the database error when the namespace or its grants cannot be read.
pub async fn resolve_publishing_owner(
    pool: &PgPool,
    publisher_id: i64,
    owner_handle: &str,
) -> Result<Option<i64>, Error> {
    sqlx::query_scalar(
        "WITH owner AS (
            SELECT id, kind, github_id FROM publishers WHERE lower(handle) = lower($2)
            ORDER BY CASE kind WHEN 'github' THEN 0 WHEN 'orcid' THEN 1 ELSE 2 END, id
            LIMIT 1
         )
         SELECT owner.id FROM owner
         WHERE (owner.id = $1 AND owner.kind IN ('github', 'orcid'))
            OR (owner.kind = 'github-org' AND owner.github_id IS NOT NULL AND EXISTS (
                SELECT 1 FROM org_members m JOIN publishers member ON member.id = m.publisher_id
                WHERE m.org_id = owner.id AND m.publisher_id = $1
                  AND member.kind = 'github' AND member.github_id IS NOT NULL
                  AND m.checked_at > now() - interval '1 day'
            ))",
    )
    .bind(publisher_id)
    .bind(owner_handle)
    .fetch_optional(pool)
    .await
    .map_err(|error| sql_error("cannot resolve the publishing identity", error))
}

// ----------------------------------------------------------------------- log

/// Appends one link to the transparency log.
///
/// The lock is a transaction-scoped advisory lock on a fixed key: it serialises
/// appenders without blocking readers, and it is released even if the process
/// dies mid-transaction.
///
/// # Errors
///
/// `E_DB` when the transaction fails.
pub async fn append_log(
    pool: &PgPool,
    kind: &str,
    summary: &str,
    payload: serde_json::Value,
) -> Result<LogEntry, Error> {
    let mut transaction = pool
        .begin()
        .await
        .map_err(|error| sql_error("cannot start the log transaction", error))?;
    let entry = append_log_in(&mut transaction, kind, summary, payload).await?;
    transaction
        .commit()
        .await
        .map_err(|error| sql_error("cannot commit the log entry", error))?;
    Ok(entry)
}

/// [`append_log`] inside a transaction the caller owns. The advisory lock is
/// held until that transaction ends, so the catalogue change and its log link
/// become visible together or not at all.
///
/// # Errors
///
/// `E_DB` when a statement fails.
pub async fn append_log_in(
    transaction: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    kind: &str,
    summary: &str,
    payload: serde_json::Value,
) -> Result<LogEntry, Error> {
    // 0x6165766961 = "aevia", so the lock id is recognisable in pg_locks.
    sqlx::query("SELECT pg_advisory_xact_lock(1634759273)")
        .execute(&mut **transaction)
        .await
        .map_err(|error| sql_error("cannot lock the log", error))?;
    let head: Option<(i64, String)> =
        sqlx::query_as("SELECT seq, entry_hash FROM publish_log ORDER BY seq DESC LIMIT 1")
            .fetch_optional(&mut **transaction)
            .await
            .map_err(|error| sql_error("cannot read the log head", error))?;
    let prev_hash = head
        .as_ref()
        .map_or_else(|| "0".repeat(64), |(_, hash)| hash.clone());
    let payload_text = serde_json::to_string(&payload)
        .map_err(|error| Error::json("cannot serialise a log payload", &error))?;
    let mut hasher = sha2::Sha256::new();
    hasher.update(prev_hash.as_bytes());
    hasher.update(b"\n");
    hasher.update(kind.as_bytes());
    hasher.update(b"\n");
    hasher.update(summary.as_bytes());
    hasher.update(b"\n");
    hasher.update(payload_text.as_bytes());
    let entry_hash = hex::encode(hasher.finalize());
    sqlx::query_as::<_, LogEntry>(
        "INSERT INTO publish_log (kind, summary, payload, prev_hash, entry_hash)
         VALUES ($1, $2, $3, $4, $5) RETURNING *",
    )
    .bind(kind)
    .bind(summary)
    .bind(&payload)
    .bind(&prev_hash)
    .bind(&entry_hash)
    .fetch_one(&mut **transaction)
    .await
    .map_err(|error| sql_error("cannot append to the log", error))
}

/// Log entries from `from_seq`, oldest first.
///
/// # Errors
///
/// `E_DB` when the query fails.
pub async fn log_entries(
    pool: &PgPool,
    from_seq: i64,
    limit: i64,
) -> Result<Vec<LogEntry>, Error> {
    sqlx::query_as::<_, LogEntry>(
        "SELECT * FROM publish_log WHERE seq >= $1 ORDER BY seq ASC LIMIT $2",
    )
    .bind(from_seq)
    .bind(limit)
    .fetch_all(pool)
    .await
    .map_err(|error| sql_error("cannot read the log", error))
}

/// The newest log entry.
///
/// # Errors
///
/// `E_DB` when the query fails.
pub async fn log_head(pool: &PgPool) -> Result<Option<LogEntry>, Error> {
    sqlx::query_as::<_, LogEntry>("SELECT * FROM publish_log ORDER BY seq DESC LIMIT 1")
        .fetch_optional(pool)
        .await
        .map_err(|error| sql_error("cannot read the log head", error))
}

#[derive(sqlx::FromRow)]
struct LogPageRow {
    is_head: bool,
    seq: i64,
    kind: String,
    summary: String,
    prev_hash: String,
    entry_hash: String,
    created_at: DateTime<Utc>,
}

/// Reads a page of log summaries and the chain head from one database snapshot.
///
/// Entries start at `from_seq` inclusive and continue in ascending order up
/// to `limit`. The head is available even if the requested page is empty.
///
/// # Errors
///
/// `E_DB` when the query fails.
pub async fn log_page(
    pool: &PgPool,
    from_seq: i64,
    limit: i64,
) -> Result<(Vec<LogEntryView>, Option<LogHead>), Error> {
    // The head marker is independent of ordering. PostgreSQL can merge the
    // bounded index scans when only the sequence controls the output order.
    let rows = sqlx::query_as::<_, LogPageRow>(
        "(SELECT true AS is_head, seq, kind, summary, prev_hash, entry_hash, created_at
          FROM publish_log ORDER BY seq DESC LIMIT 1)
         UNION ALL
         (SELECT false AS is_head, seq, kind, summary, prev_hash, entry_hash, created_at
          FROM publish_log WHERE seq >= $1 ORDER BY seq ASC LIMIT $2)
         ORDER BY seq ASC",
    )
    .bind(from_seq)
    .bind(limit)
    .fetch_all(pool)
    .await
    .map_err(|error| sql_error("cannot read the log", error))?;

    let mut head = None;
    let mut entries = Vec::with_capacity(rows.len().saturating_sub(1));
    for row in rows {
        if row.is_head {
            head = Some(LogHead {
                seq: row.seq,
                entry_hash: row.entry_hash,
                created_at: row.created_at,
            });
        } else {
            entries.push(LogEntryView {
                seq: row.seq,
                kind: row.kind,
                summary: row.summary,
                hash: row.entry_hash,
                prev: row.prev_hash,
                at: row.created_at.to_rfc3339(),
            });
        }
    }
    Ok((entries, head))
}

// --------------------------------------------------------------------- stats

/// The counts the front page shows.
///
/// # Errors
///
/// `E_DB` when the query fails.
pub async fn stats(pool: &PgPool) -> Result<SiteStats, Error> {
    let row: (i64, i64, i64, i64) = sqlx::query_as(
        "SELECT
            (SELECT count(*) FROM packages),
            (SELECT count(*) FROM versions),
            (SELECT COALESCE(SUM(downloads), 0)::bigint FROM versions),
            (SELECT count(*) FROM publishers WHERE kind <> 'github-org')",
    )
    .fetch_one(pool)
    .await
    .map_err(|error| sql_error("cannot read the statistics", error))?;
    Ok(SiteStats {
        packages: row.0,
        versions: row.1,
        downloads: row.2,
        publishers: row.3,
    })
}

/// Escapes the wildcards a user may type into a search box.
#[allow(
    clippy::single_call_fn,
    reason = "SQL LIKE wildcard escaping is tested independently of database search"
)]
fn escape_like(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for character in text.chars() {
        if matches!(character, '%' | '_' | '\\') {
            out.push('\\');
        }
        out.push(character);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn like_wildcards_are_escaped() {
        assert_eq!(escape_like("100%_a\\b"), "100\\%\\_a\\\\b");
        assert_eq!(escape_like("plain"), "plain");
    }
}
