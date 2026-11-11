//! The JSON API under `/api/v1`.
//!
//! # The one credential
//!
//! Every mutating endpoint takes a signed statement and nothing else — no
//! cookie, no bearer token, no API key. [`authorise`] is the single place that
//! turns "here is a statement and a signature" into "this is who is asking",
//! so the rules cannot drift between publish and yank.

pub mod deps;
pub mod download;
pub mod index;
pub mod keys;
pub mod log;
pub mod publish;
pub mod search;
pub mod yank;

use axum::Json;
use axum::Router;
use axum::extract::State;
use axum::routing::get;
use axum::routing::post;
use chrono::Utc;

use aevia_registry_core::signature::SignerKey;
use aevia_registry_core::signature::verify_detached;
use aevia_registry_core::statement::Statement;
use aevia_registry_db::models::Publisher;
use aevia_registry_db::repo;

use crate::error::ApiError;
use crate::error::ApiResult;
use crate::state::AppState;

/// What a verified statement proves.
pub(crate) struct Authorised {
    /// The parsed statement.
    pub statement: Statement,
    /// The publisher whose key signed it.
    pub publisher: Publisher,
    /// The fingerprint that signed.
    pub fingerprint: String,
    /// `openpgp` or `ssh`.
    pub key_kind: String,
}

/// Verifies a signed statement end to end.
///
/// The order matters: parse, check the kind, check the clock, spend the nonce,
/// compare the bytes with the ones we issued, find the key, verify the
/// signature, load the publisher. Anything that fails before the nonce is spent
/// can be retried; anything after it needs a new challenge, which is the point
/// of a single-use nonce.
pub(crate) async fn authorise(
    state: &AppState,
    kind: &str,
    statement_text: &str,
    signature: &str,
    key: &SignerKey,
) -> ApiResult<Authorised> {
    let statement = Statement::parse(statement_text).map_err(ApiError::from_core)?;
    if statement.kind() != kind {
        return Err(ApiError::bad_request(
            "E_STATEMENT",
            format!(
                "the statement is a `{}` statement, not a `{kind}` one",
                statement.kind()
            ),
        ));
    }
    if statement.is_expired(Utc::now().timestamp()) {
        return Err(ApiError::bad_request(
            "E_STATEMENT",
            "the statement has expired; request a new challenge",
        ));
    }
    let Some(challenge) = repo::consume_challenge(&state.pool, statement.nonce(), kind).await?
    else {
        return Err(ApiError::bad_request(
            "E_STATEMENT",
            "the nonce is unknown, expired or already used; request a new challenge",
        ));
    };
    if challenge.statement != statement_text {
        return Err(ApiError::bad_request(
            "E_STATEMENT",
            "the statement differs from the one the challenge issued",
        ));
    }
    let fingerprint = key.fingerprint().map_err(ApiError::from_core)?;
    let Some(bound) =
        repo::signing_key_by_fingerprint(&state.pool, key.kind.as_str(), &fingerprint).await?
    else {
        return Err(ApiError::forbidden(
            "E_KEY",
            format!(
                "the key {fingerprint} is not bound to any account; run `aevia-pkg login` and \
                 approve the code it prints"
            ),
        ));
    };
    verify_detached(key, signature, statement_text.as_bytes()).map_err(ApiError::from_core)?;
    let Some(publisher) = repo::publisher(&state.pool, bound.publisher_id).await? else {
        return Err(ApiError::forbidden(
            "E_KEY",
            "the key's publisher no longer exists",
        ));
    };
    Ok(Authorised {
        statement,
        publisher,
        fingerprint,
        key_kind: key.kind.as_str().to_string(),
    })
}

/// `GET /api/v1/stats`
///
/// # Errors
///
/// `E_DB` when the counts cannot be read.
pub(crate) async fn stats(
    State(state): State<AppState>
) -> ApiResult<Json<aevia_registry_core::vm::SiteStats>> {
    Ok(Json(repo::stats(&state.pool).await?))
}

/// The `/api/v1` router.
pub fn router() -> Router<AppState> {
    Router::new()
        .route("/stats", get(stats))
        .route("/search", get(search::search))
        .route("/crates/{owner}/{name}", get(search::detail))
        .route("/crates/{owner}/{name}/dependents", get(deps::dependents))
        .route("/resolve", post(deps::resolve))
        .route("/index/config.json", get(index::config))
        .route("/index/{owner}/{name}", get(index::package_index))
        .route("/download/{owner}/{name}/{version}", get(download::archive))
        .route(
            "/download/{owner}/{name}/{version}/signature",
            get(download::signature),
        )
        .route("/publish/challenge", post(publish::challenge))
        .route("/publish", post(publish::publish))
        .route("/yank/challenge", post(yank::challenge))
        .route("/yank", post(yank::yank))
        .route("/auth/device", post(crate::auth::device::start))
        .route(
            "/auth/device/{device_code}",
            get(crate::auth::device::status),
        )
        .route("/keys", get(keys::status))
        .route("/keys/public", get(keys::public_key))
        .route("/log", get(log::entries))
}
