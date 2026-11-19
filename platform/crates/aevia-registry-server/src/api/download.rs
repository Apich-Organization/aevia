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
use axum::http::StatusCode;
use axum::http::header;
use axum::response::IntoResponse;
use axum::response::Response;
use tokio::sync::OwnedSemaphorePermit;

use aevia_registry_core::PackageId;
use aevia_registry_db::repo;

use crate::api::ApiError;
use crate::api::ApiResult;
use crate::state::AppState;

/// `GET /api/v1/download/{owner}/{name}/{version}`
///
/// # Errors
///
/// `404` when the package or version is unknown, `500` when the archive is
/// missing from storage (which means storage and catalogue disagree).
pub async fn archive(
    State(state): State<AppState>,
    Path((owner, name, version)): Path<(String, String, String)>,
    headers: HeaderMap,
) -> ApiResult<Response> {
    const CACHE: &str = "public, max-age=31536000, immutable";

    let (id, row) = resolve(&state, &owner, &name, &version).await?;
    let etag = format!("\"{}\"", row.checksum);
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
    let version = row.version.clone();
    let checksum = row.checksum.clone();
    let name = id.clone();
    let bytes = tokio::task::spawn_blocking(move || {
        storage
            .read(&name, &version, &checksum)
            .map(|bytes| Bytes::from_owner(ArchiveBody { bytes, _slot: slot }))
    })
    .await
    .map_err(|error| ApiError::internal(format!("the archive read task failed: {error}")))??;
    state.download_counts.record(row.id);
    Ok((
        StatusCode::OK,
        [
            (header::CONTENT_TYPE, "application/gzip".to_string()),
            (
                header::CONTENT_DISPOSITION,
                format!(
                    "attachment; filename=\"{}-{}.tar.gz\"",
                    id.name(),
                    row.version
                ),
            ),
            (header::ETAG, etag),
            (header::CACHE_CONTROL, CACHE.to_string()),
            (
                header::HeaderName::from_static("x-aevia-checksum"),
                row.checksum.clone(),
            ),
            (
                header::HeaderName::from_static("x-aevia-signature-fingerprint"),
                row.fingerprint.clone(),
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
    let (_, row) = resolve(&state, &owner, &name, &version).await?;
    Ok((
        StatusCode::OK,
        [(header::CONTENT_TYPE, "text/plain; charset=utf-8")],
        // No separator: the statement already ends with a newline, and those
        // exact bytes are what the signature covers.
        format!("{}{}", row.statement, row.signature),
    )
        .into_response())
}

async fn resolve(
    state: &AppState,
    owner: &str,
    name: &str,
    version: &str,
) -> ApiResult<(PackageId, aevia_registry_db::models::Version)> {
    let requested = PackageId::new(owner, name).map_err(ApiError::from_core)?;
    let Some((package, owner_row)) =
        repo::package_by_handle(&state.pool, requested.owner(), &requested.name_key()).await?
    else {
        return Err(ApiError::not_found(format!(
            "There is no package called {requested}."
        )));
    };
    let id = PackageId::new(&owner_row.handle, &package.name).map_err(ApiError::from_core)?;
    let Some(row) = repo::version(&state.pool, package.id, version).await? else {
        return Err(ApiError::not_found(format!(
            "{requested} has no version {version}."
        )));
    };
    Ok((id, row))
}
