//! Key lookup, so a client can check a binding before it signs.
//!
//! The fingerprint travels as a query parameter rather than a path segment.
//! An OpenPGP fingerprint is hexadecimal and would be safe in a path, but an SSH
//! fingerprint is `SHA256:<base64>`, and base64 contains `/` — which a proxy may
//! decode, merge with the path, and route to something else entirely. A query
//! parameter has no such ambiguity.

use axum::Json;
use axum::extract::Query;
use axum::extract::State;
use axum::http::StatusCode;
use axum::http::header;
use axum::response::IntoResponse;
use axum::response::Response;
use serde::Deserialize;
use serde::Serialize;

use aevia_registry_db::repo;

use crate::api::ApiError;
use crate::api::ApiResult;
use crate::state::AppState;

/// The query of `/api/v1/keys`.
#[derive(Deserialize)]
pub struct KeyQuery {
    /// The fingerprint, as the signing tool prints it.
    pub fingerprint: String,
    /// `openpgp` or `ssh`; when absent, both are searched.
    pub kind: Option<String>,
}

/// The reply to a lookup.
#[derive(Serialize)]
pub struct KeyStatus {
    /// Whether the key is bound to an account.
    pub registered: bool,
    /// The account, when it is.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub publisher: Option<PublisherSummary>,
}

/// The account behind a key.
#[derive(Serialize)]
pub struct PublisherSummary {
    /// The handle.
    pub handle: String,
    /// `github` or `orcid`.
    pub kind: String,
    /// The provider's name.
    pub display_name: Option<String>,
}

/// `GET /api/v1/keys?fingerprint=…`
///
/// # Errors
///
/// `E_DB` when the lookup fails.
pub async fn status(
    State(state): State<AppState>,
    Query(query): Query<KeyQuery>,
) -> ApiResult<Json<KeyStatus>> {
    let kinds: Vec<&str> = match query.kind.as_deref() {
        | Some("openpgp") => vec!["openpgp"],
        | Some("ssh") => vec!["ssh"],
        | _ => vec!["openpgp", "ssh"],
    };
    let mut found = None;
    for kind in kinds {
        if let Some(key) =
            repo::signing_key_by_fingerprint(&state.pool, kind, &query.fingerprint).await?
        {
            found = Some(key);
            break;
        }
    }
    let Some(key) = found else {
        return Ok(Json(KeyStatus {
            registered: false,
            publisher: None,
        }));
    };
    let publisher = repo::publisher(&state.pool, key.publisher_id)
        .await?
        .map(|row| PublisherSummary {
            handle: row.handle,
            kind: row.kind,
            display_name: row.display_name,
        });
    Ok(Json(KeyStatus {
        registered: true,
        publisher,
    }))
}

/// `GET /api/v1/keys/public?fingerprint=…`
///
/// # Errors
///
/// `404` when the key is not bound.
pub async fn public_key(
    State(state): State<AppState>,
    Query(query): Query<KeyQuery>,
) -> ApiResult<Response> {
    let kinds: Vec<&str> = match query.kind.as_deref() {
        | Some("openpgp") => vec!["openpgp"],
        | Some("ssh") => vec!["ssh"],
        | _ => vec!["openpgp", "ssh"],
    };
    for kind in kinds {
        if let Some(key) =
            repo::signing_key_by_fingerprint(&state.pool, kind, &query.fingerprint).await?
        {
            return Ok((
                StatusCode::OK,
                [
                    (header::CONTENT_TYPE, "text/plain; charset=utf-8"),
                    (header::CACHE_CONTROL, "public, max-age=3600"),
                ],
                key.public_key,
            )
                .into_response());
        }
    }
    Err(ApiError::not_found(format!(
        "no bound key has the fingerprint {}",
        query.fingerprint
    )))
}
