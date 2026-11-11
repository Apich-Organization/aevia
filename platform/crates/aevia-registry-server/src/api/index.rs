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
//!
//! The body is derived from the catalogue row on every request (a yank is a
//! single `UPDATE` and the next read already reflects it) and carries an
//! `ETag`, so a client that keeps the file revalidates with `If-None-Match`
//! instead of trusting a cache for a fixed time.

use axum::Json;
use axum::extract::Path;
use axum::extract::State;
use axum::http::HeaderMap;
use axum::http::StatusCode;
use axum::http::header;
use axum::response::IntoResponse;
use axum::response::Response;

use aevia_registry_core::PackageId;
use aevia_registry_core::index::IndexConfig;
use aevia_registry_core::index::IndexRecord;
use aevia_registry_core::index::IndexSignature;
use aevia_registry_core::index::deps_from_json;
use aevia_registry_db::models::Version;
use aevia_registry_db::repo;

use crate::api::ApiError;
use crate::api::ApiResult;
use crate::state::AppState;

/// `GET /api/v1/index/config.json`
pub async fn config(State(state): State<AppState>) -> Json<IndexConfig> {
    Json(IndexConfig::new(&state.config.base_url))
}

/// The record of one catalogue row.
///
/// The dependency list is read from the stored JSON tolerantly (rows from
/// before the registry understood requirements have none), and the package is
/// named in its canonical spelling, as the dependents' requirements are.
#[must_use]
pub fn record_of(
    id: &PackageId,
    version: &Version,
) -> IndexRecord {
    IndexRecord {
        name: id.to_string(),
        vers: version.version.clone(),
        deps: deps_from_json(&version.dependencies),
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
    }
}

/// The index file of a package: its versions, oldest first.
///
/// # Errors
///
/// `E_JSON` when a record cannot be serialised, which would be a bug.
pub fn render(
    id: &PackageId,
    newest_first: &[Version],
) -> aevia_registry_core::Result<String> {
    let mut body = String::new();
    for version in newest_first.iter().rev() {
        body.push_str(&record_of(id, version).to_line()?);
        body.push('\n');
    }
    Ok(body)
}

/// `GET /api/v1/index/{owner}/{name}`
///
/// # Errors
///
/// `404` when the package has no versions.
pub async fn package_index(
    State(state): State<AppState>,
    Path((owner, name)): Path<(String, String)>,
    headers: HeaderMap,
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
    let versions = repo::versions(&state.pool, package.id).await?;
    let body = render(&id, &versions).map_err(ApiError::from_core)?;
    let etag = aevia_registry_core::index::etag(&body);
    let revalidated = headers
        .get(header::IF_NONE_MATCH)
        .and_then(|value| value.to_str().ok())
        .is_some_and(|sent| sent.split(',').any(|candidate| candidate.trim() == etag));
    if revalidated {
        return Ok((StatusCode::NOT_MODIFIED, [(header::ETAG, etag)]).into_response());
    }
    Ok((
        StatusCode::OK,
        [
            (header::CONTENT_TYPE, "text/plain; charset=utf-8".to_string()),
            // Revalidate on every use: the body is cheap to build and a yank
            // must reach a client at its next resolution, not a minute later.
            (header::CACHE_CONTROL, "public, no-cache".to_string()),
            (header::ETAG, etag),
        ],
        body,
    )
        .into_response())
}
