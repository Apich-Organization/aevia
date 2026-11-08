//! The sparse index.
//!
//! One request per package, newline-delimited JSON, oldest record first. A
//! client resolves a version by reading the file and taking the newest record
//! that satisfies its constraint; there is no git clone and no global index
//! document to keep in step.
//!
//! Each record carries the signature that authorised the version, so a client
//! can check provenance from the index alone — before it downloads anything,
//! and without trusting the download endpoint.

use axum::Json;
use axum::extract::Path;
use axum::extract::State;
use axum::http::StatusCode;
use axum::http::header;
use axum::response::IntoResponse;
use axum::response::Response;

use aevia_registry_core::PackageId;
use aevia_registry_core::index::IndexConfig;
use aevia_registry_core::index::IndexDependency;
use aevia_registry_core::index::IndexRecord;
use aevia_registry_core::index::IndexSignature;
use aevia_registry_db::repo;

use crate::api::ApiError;
use crate::api::ApiResult;
use crate::state::AppState;

/// `GET /api/v1/index/config.json`
pub async fn config(State(state): State<AppState>) -> Json<IndexConfig> {
    Json(IndexConfig::new(&state.config.base_url))
}

/// `GET /api/v1/index/{owner}/{name}`
///
/// # Errors
///
/// `404` when the package has no versions.
pub async fn package_index(
    State(state): State<AppState>,
    Path((owner, name)): Path<(String, String)>,
) -> ApiResult<Response> {
    let id = PackageId::new(&owner, &name).map_err(ApiError::from_core)?;
    let Some((package, owner_row)) =
        repo::package_by_handle(&state.pool, id.owner(), &id.name_key()).await?
    else {
        return Err(ApiError::not_found(format!(
            "There is no package called {id}."
        )));
    };
    let id = PackageId::new(&owner_row.handle, &package.name).map_err(ApiError::from_core)?;
    let mut versions = repo::versions(&state.pool, package.id).await?;
    // Oldest first: an index is a history, and appending is what clients expect.
    versions.reverse();
    let mut body = String::new();
    for version in &versions {
        let deps: Vec<IndexDependency> =
            serde_json::from_value(version.dependencies.clone()).unwrap_or_default();
        let record = IndexRecord {
            name: id.to_string(),
            vers: version.version.clone(),
            deps,
            cksum: version.checksum.clone(),
            yanked: version.yanked,
            published_at: version.created_at.to_rfc3339(),
            docs: version.docs_status == "built",
            size: u64::try_from(version.size_bytes).unwrap_or(0),
            signature: IndexSignature {
                kind: version.key_kind.clone(),
                fingerprint: version.fingerprint.clone(),
                signature: version.signature.clone(),
            },
        };
        body.push_str(&record.to_line().map_err(ApiError::from_core)?);
        body.push('\n');
    }
    Ok((
        StatusCode::OK,
        [
            (header::CONTENT_TYPE, "text/plain; charset=utf-8"),
            // An index is append-only, so a client may keep it for a while; the
            // registry is not a CDN, but a minute of caching spares it a burst
            // of resolutions from one build.
            (header::CACHE_CONTROL, "public, max-age=60"),
        ],
        body,
    )
        .into_response())
}
