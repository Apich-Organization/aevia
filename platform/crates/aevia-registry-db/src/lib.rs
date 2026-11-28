//! The registry's PostgreSQL layer.
//!
//! Everything here is one of three things: a pool, a migration, or a query.
//! There is no ORM and no query builder — the interesting statements (the hash
//! chain, the "newest version that is not yanked", the aggregate download
//! count) read better as SQL, and the server needs to know exactly which index
//! answers each one.
//!
//! # Why the schema is applied by the binary
//!
//! `sqlx::migrate!` embeds the `.sql` files at compile time. At startup the
//! registry validates the applied history and runs pending migrations under a
//! PostgreSQL advisory lock. Each successful session returns to the pool.
//! An unsuccessful or cancelled migration retires its session so another
//! startup can acquire the lock after PostgreSQL closes the session.

#![forbid(unsafe_code)]
#![cfg_attr(
    not(test),
    deny(
        clippy::arithmetic_side_effects,
        clippy::indexing_slicing,
        clippy::nursery,
        clippy::panic,
        clippy::pedantic,
        clippy::single_call_fn,
        clippy::unreachable
    )
)]

pub mod models;
pub mod repo;

use std::time::Duration;

use sqlx::PgPool;
use sqlx::Postgres;
use sqlx::migrate::MigrateError;
use sqlx::pool::PoolConnection;
use sqlx::postgres::PgPoolOptions;

use aevia_registry_core::Error;

/// `E_DB`: the database refused.
pub const E_DB: &str = "E_DB";

/// `E_CONFLICT`: the row already exists, or the version was published already.
pub const E_CONFLICT: &str = "E_CONFLICT";

/// `E_NOT_FOUND`: the row does not exist.
pub const E_NOT_FOUND: &str = "E_NOT_FOUND";

static MIGRATOR: sqlx::migrate::Migrator = sqlx::migrate!("./migrations");

struct MigrationSession {
    connection: PoolConnection<Postgres>,
    succeeded: bool,
}

impl Drop for MigrationSession {
    fn drop(&mut self) {
        if !self.succeeded {
            // SQLx releases the session advisory lock on its success path.
            // Close a failed or cancelled session before the pool reuses it.
            self.connection.close_on_drop();
        }
    }
}

/// Opens the pool and applies the migrations.
///
/// # Errors
///
/// `E_DB` when the database is unreachable or a migration fails.
pub async fn connect(
    url: &str,
    max_connections: u32,
) -> Result<PgPool, Error> {
    let pool = PgPoolOptions::new()
        .max_connections(max_connections)
        // A request that waits longer than this for a connection is better off
        // failing fast than holding the client for the whole timeout of the
        // proxy in front of it.
        .acquire_timeout(Duration::from_secs(5))
        .connect(url)
        .await
        .map_err(|error| Error::new(E_DB, format!("cannot reach PostgreSQL: {error}")))?;
    migrate(&pool).await?;
    Ok(pool)
}

/// Applies the embedded migrations.
///
/// # Errors
///
/// `E_DB` when a migration fails.
pub async fn migrate(pool: &PgPool) -> Result<(), Error> {
    let schema_error =
        |error: MigrateError| Error::new(E_DB, format!("cannot apply the schema: {error}"));
    let connection = pool
        .acquire()
        .await
        .map_err(MigrateError::from)
        .map_err(schema_error)?;
    let mut session = MigrationSession {
        connection,
        succeeded: false,
    };
    let result = MIGRATOR.run(&mut *session.connection).await;
    session.succeeded = result.is_ok();
    result.map_err(schema_error)
}

/// Maps a `sqlx` failure to the registry's error type.
///
/// A unique violation is a conflict the caller can act on (that version is
/// already published, that key is already bound), so it gets its own code
/// instead of being flattened into "database error".
#[must_use]
#[allow(
    clippy::needless_pass_by_value,
    reason = "the public error adapter accepts ownership directly from Result::map_err"
)]
pub fn sql_error(
    context: &str,
    error: sqlx::Error,
) -> Error {
    if let sqlx::Error::Database(ref database) = error {
        if database.is_unique_violation() {
            return Error::new(E_CONFLICT, format!("{context}: it already exists"));
        }
        if database.is_foreign_key_violation() {
            return Error::new(
                E_NOT_FOUND,
                format!("{context}: a referenced row is missing"),
            );
        }
    }
    Error::new(E_DB, format!("{context}: {error}"))
}

/// A `sqlx` failure that means "no such row".
#[must_use]
pub fn not_found(what: impl std::fmt::Display) -> Error {
    Error::new(E_NOT_FOUND, format!("{what} does not exist"))
}
