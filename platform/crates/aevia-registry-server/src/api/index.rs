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
    let mut body = Vec::new();
    let mut tied = Vec::new();
    for versions in newest_first
        .chunk_by(|a, b| a.created_at == b.created_at)
        .rev()
    {
        // Row IDs break timestamp ties so repeated catalogue reads of the
        // same rows emit identical bytes.
        if versions.iter().rev().is_sorted_by_key(|version| version.id) {
            for version in versions.iter().rev() {
                record_of(id, version).append_line(&mut body)?;
            }
        } else {
            tied.extend(versions);
            tied.sort_unstable_by_key(|version| version.id);
            for &version in &tied {
                record_of(id, version).append_line(&mut body)?;
            }
            tied.clear();
        }
    }
    String::from_utf8(body).map_err(|error| {
        aevia_registry_core::Error::new(
            "E_JSON",
            format!("cannot encode the index as UTF-8: {error}"),
        )
    })
}

#[allow(
    clippy::single_call_fn,
    reason = "HTTP validator parsing has focused unit tests"
)]
fn matches_validator(
    headers: &HeaderMap,
    etag: &str,
) -> bool {
    let values = headers.get_all(header::IF_NONE_MATCH);
    let mut fields = values.iter().peekable();
    let Some(first) = fields.next() else {
        return false;
    };
    if first.as_bytes().trim_ascii() == b"*" {
        return fields.peek().is_none();
    }
    std::iter::once(first).chain(fields).any(|field| {
        let mut quoted = false;
        field
            .as_bytes()
            .split(|byte| {
                if *byte == b'"' {
                    quoted = !quoted;
                }
                *byte == b',' && !quoted
            })
            .any(|candidate| {
                // RFC 9110 compares opaque tags weakly for If-None-Match.
                let candidate = candidate.trim_ascii();
                candidate.strip_prefix(b"W/").unwrap_or(candidate) == etag.as_bytes()
            })
    })
}

/// `GET /api/v1/index/{owner}/{name}`
///
/// # Errors
///
/// `404` when the package is unknown.
pub async fn package_index(
    State(state): State<AppState>,
    Path((owner, name)): Path<(String, String)>,
    headers: HeaderMap,
) -> ApiResult<Response> {
    const CACHE: &str = "public, no-cache";

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
    if matches_validator(&headers, &etag) {
        return Ok((
            StatusCode::NOT_MODIFIED,
            [
                (header::ETAG, etag),
                (header::CACHE_CONTROL, CACHE.to_string()),
            ],
        )
            .into_response());
    }
    Ok((
        StatusCode::OK,
        [
            (
                header::CONTENT_TYPE,
                "text/plain; charset=utf-8".to_string(),
            ),
            // Revalidate on every use: the body is cheap to build and a yank
            // must reach a client at its next resolution, not a minute later.
            (header::CACHE_CONTROL, CACHE.to_string()),
            (header::ETAG, etag),
        ],
        body,
    )
        .into_response())
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::http::HeaderValue;

    #[test]
    fn validators_allow_opaque_bytes_and_commas_in_other_tags() {
        let mut headers = HeaderMap::new();
        assert!(!matches_validator(&headers, "\"current\""));
        let value = HeaderValue::from_bytes(b"\"other,\xff\", W/\"current\"")
            .unwrap_or_else(|error| panic!("valid opaque entity-tag bytes: {error}"));
        headers.insert(header::IF_NONE_MATCH, value);
        assert!(matches_validator(&headers, "\"current\""));
        assert!(!matches_validator(&headers, "\"different\""));
    }

    #[test]
    fn wildcard_is_a_complete_field_value() {
        let mut headers = HeaderMap::new();
        headers.insert(
            header::IF_NONE_MATCH,
            HeaderValue::from_static("*, \"other\""),
        );
        assert!(!matches_validator(&headers, "\"current\""));
        headers.insert(header::IF_NONE_MATCH, HeaderValue::from_static("*"));
        assert!(matches_validator(&headers, "\"current\""));
        headers.append(header::IF_NONE_MATCH, HeaderValue::from_static("\"other\""));
        assert!(!matches_validator(&headers, "\"current\""));
    }
}
