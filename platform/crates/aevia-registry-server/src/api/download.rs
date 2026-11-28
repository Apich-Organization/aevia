//! Archive downloads.
//!
//! The bytes are served with the checksum and the signing fingerprint in the
//! headers, and the detached signature is available next to them. A client that
//! wants to verify a download needs nothing else: no token, no second request
//! to a different host.

use axum::body::Bytes;
use axum::extract::Path;
use axum::extract::State;
use axum::http::HeaderMap;
use axum::http::Method;
use axum::http::StatusCode;
use axum::http::header;
use axum::response::IntoResponse;
use axum::response::Response;
use sqlx::postgres::PgRow;
use tokio::sync::OwnedSemaphorePermit;

use aevia_registry_core::PackageId;
use aevia_registry_db::repo;

use crate::api::ApiError;
use crate::api::ApiResult;
use crate::state::AppState;

#[derive(sqlx::FromRow)]
struct ArchiveVersion {
    id: i64,
    version: String,
    checksum: String,
    fingerprint: String,
}

/// `GET /api/v1/download/{owner}/{name}/{version}`
///
/// # Errors
///
/// `404` when the package or version is unknown, `500` when the archive is
/// missing from storage (which means storage and catalogue disagree).
pub async fn archive(
    State(state): State<AppState>,
    Path((owner, name, version)): Path<(String, String, String)>,
    method: Method,
    headers: HeaderMap,
) -> ApiResult<Response> {
    const CACHE: &str = "public, max-age=31536000, immutable";

    let (id, row): (_, ArchiveVersion) = resolve(
        &state,
        &owner,
        &name,
        &version,
        "SELECT id, version, checksum, fingerprint FROM versions WHERE package_id = $1 AND version = $2",
    )
    .await?;
    let ArchiveVersion {
        id: version_id,
        version,
        checksum,
        fingerprint,
    } = row;
    let etag = format!("\"{checksum}\"");
    if headers.get_all(header::IF_NONE_MATCH).iter().any(|header| {
        header.to_str().is_ok_and(|value| {
            value.split(',').map(str::trim).any(|candidate| {
                candidate == "*" || candidate.strip_prefix("W/").unwrap_or(candidate) == etag
            })
        })
    }) {
        return Ok((
            StatusCode::NOT_MODIFIED,
            [
                (header::ETAG, etag),
                (header::CACHE_CONTROL, CACHE.to_string()),
            ],
        )
            .into_response());
    }
    let slot = state
        .download_slots
        .clone()
        .try_acquire_owned()
        .map_err(|_| {
            ApiError::new(
                StatusCode::TOO_MANY_REQUESTS,
                "E_DOWNLOAD_BUSY",
                "The registry is serving other archives. Try again shortly.",
            )
        })?;
    let storage = state.storage.clone();
    let disposition = format!("attachment; filename=\"{}-{version}.tar.gz\"", id.name());
    let download = method == Method::GET;
    let (bytes, size, checksum) =
        tokio::task::spawn_blocking(move || -> aevia_registry_core::Result<_> {
            let (body, size) = if download {
                let bytes = storage.read(&id, &version, &checksum)?;
                let size = bytes.len() as u64;
                (Bytes::from_owner(ArchiveBody { bytes, _slot: slot }), size)
            } else {
                (
                    Bytes::new(),
                    storage.verified_size(&id, &version, &checksum)?,
                )
            };
            Ok((body, size, checksum))
        })
        .await
        .map_err(|error| ApiError::internal(format!("the archive read task failed: {error}")))??;
    if download {
        state.download_counts.record(version_id);
    }
    Ok((
        StatusCode::OK,
        [
            (header::CONTENT_TYPE, "application/gzip".to_string()),
            (header::CONTENT_LENGTH, size.to_string()),
            (header::CONTENT_DISPOSITION, disposition),
            (header::ETAG, etag),
            (header::CACHE_CONTROL, CACHE.to_string()),
            (
                header::HeaderName::from_static("x-aevia-checksum"),
                checksum,
            ),
            (
                header::HeaderName::from_static("x-aevia-signature-fingerprint"),
                fingerprint,
            ),
        ],
        bytes,
    )
        .into_response())
}

struct ArchiveBody {
    bytes: Vec<u8>,
    // Also limits memory held by an archive still being sent to a slow client.
    _slot: OwnedSemaphorePermit,
}

impl AsRef<[u8]> for ArchiveBody {
    fn as_ref(&self) -> &[u8] {
        &self.bytes
    }
}

/// `GET /api/v1/download/{owner}/{name}/{version}/signature`
///
/// The body is the statement followed by the detached signature over it — a
/// self-contained receipt. A verifier needs no second request and no server
/// metadata: the statement names the package, the version and the archive's
/// digest, and the signature proves who asked for that exact text. The two
/// halves are separated by the armour header, so a client splits on that.
///
/// # Errors
///
/// `404` when the package or version is unknown.
pub async fn signature(
    State(state): State<AppState>,
    Path((owner, name, version)): Path<(String, String, String)>,
) -> ApiResult<Response> {
    let (_, (mut statement, signature)): (_, (String, String)) = resolve(
        &state,
        &owner,
        &name,
        &version,
        "SELECT statement, signature FROM versions WHERE package_id = $1 AND version = $2",
    )
    .await?;
    // The statement's existing newline is part of what the signature covers.
    statement.push_str(&signature);
    Ok((
        StatusCode::OK,
        [(header::CONTENT_TYPE, "text/plain; charset=utf-8")],
        statement,
    )
        .into_response())
}

async fn resolve<T: for<'row> sqlx::FromRow<'row, PgRow> + Send + Unpin>(
    state: &AppState,
    owner: &str,
    name: &str,
    version: &str,
    query: &str,
) -> ApiResult<(PackageId, T)> {
    let requested = PackageId::new(owner, name).map_err(ApiError::from_core)?;
    let Some((package, owner_row)) =
        repo::package_by_handle(&state.pool, requested.owner(), &requested.name_key()).await?
    else {
        return Err(ApiError::not_found(format!(
            "There is no package called {requested}."
        )));
    };
    let id = PackageId::new(&owner_row.handle, &package.name).map_err(ApiError::from_core)?;
    let Some(row) = sqlx::query_as(query)
        .bind(package.id)
        .bind(version)
        .fetch_optional(&state.pool)
        .await
        .map_err(|error| aevia_registry_db::sql_error("cannot read the version", error))?
    else {
        return Err(ApiError::not_found(format!(
            "{requested} has no version {version}."
        )));
    };
    Ok((id, row))
}
