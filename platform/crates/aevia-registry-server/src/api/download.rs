//! Archive downloads.
//!
//! The bytes are served with the checksum and the signing fingerprint in the
//! headers, and the detached signature is available next to them. A client that
//! wants to verify a download needs nothing else: no token, no second request
//! to a different host.

use axum::extract::Path;
use axum::extract::State;
use axum::http::StatusCode;
use axum::http::header;
use axum::response::IntoResponse;
use axum::response::Response;

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
) -> ApiResult<Response> {
    let (id, row) = resolve(&state, &owner, &name, &version).await?;
    let bytes = state.storage.read(&id, &row.version)?;
    // The catalogue's checksum is what the index, the signature and the lock
    // files of every consumer name. Serving bytes that hash to something else
    // would hand a client an archive its own verification must reject; saying
    // so here turns silent storage corruption into an operator-visible error.
    let actual = aevia_registry_core::tarball::checksum(&bytes);
    if !actual.eq_ignore_ascii_case(&row.checksum) {
        return Err(ApiError::new(
            StatusCode::INTERNAL_SERVER_ERROR,
            "E_INTEGRITY",
            format!(
                "the stored archive of {id} {} hashes to {actual}, but the catalogue (and the \
                 signature) say {}; storage and catalogue have diverged",
                row.version, row.checksum
            ),
        ));
    }
    // Counting is best-effort: a download that succeeded must not fail because
    // the counter could not be bumped.
    let pool = state.pool.clone();
    let version_id = row.id;
    tokio::spawn(async move {
        if let Err(error) = repo::increment_downloads(&pool, version_id).await {
            tracing::warn!(%error, "cannot count a download");
        }
    });
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
            (header::ETAG, format!("\"{}\"", row.checksum)),
            (
                header::CACHE_CONTROL,
                "public, max-age=31536000, immutable".to_string(),
            ),
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
