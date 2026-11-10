//! Device authorisation: how a signing key becomes bound to an account.
//!
//! This is the replacement for `cargo login`, and it is the one place where a
//! browser and a terminal meet.
//!
//! 1. `aevia-pkg login` generates or selects a key and asks the registry to
//!    bind it. The registry answers with a short code and a URL.
//! 2. The human opens the URL, signs in with GitHub or ORCID, and sees the key
//!    kind and fingerprint before approving it.
//! 3. The client polls with its secret device code and learns that the key is
//!    now bound.
//!
//! What is *not* handed out is a token. The binding is a row in `signing_keys`;
//! from then on the client signs statements, and the signature is the only
//! credential in the publish path. There is nothing to steal from a config file
//! and nothing to rotate.
//!
//! The short code is deliberately low-entropy (8 characters from a
//! 32-character alphabet, ~40 bits) because a human retypes it. That is
//! acceptable only because it cannot be guessed into an approval: it authorises
//! nothing by itself, it expires in fifteen minutes, and approving it requires
//! a signed-in session. The *device* code, which the client keeps, is 32 bytes.

use axum::Form;
use axum::Json;
use axum::extract::Path;
use axum::extract::Query;
use axum::extract::State;
use axum::http::HeaderMap;
use axum::response::Html;
use axum::response::IntoResponse;
use axum::response::Redirect;
use axum::response::Response;
use chrono::Utc;
use serde::Deserialize;
use serde::Serialize;

use aevia_registry_core::signature::SignerKey;
use aevia_registry_db::repo;

use crate::error::ApiError;
use crate::error::ApiResult;
use crate::error::PageError;
use crate::error::PageResult;
use crate::http;
use crate::state::AppState;

/// How long a device code lives.
const DEVICE_MINUTES: i64 = 15;

/// The polling interval the client should use, in seconds.
const POLL_SECONDS: i64 = 5;

#[derive(Deserialize)]
pub struct DeviceRequest {
    /// `openpgp` or `ssh`; the armour is sniffed and must agree.
    pub kind: String,
    /// The ASCII-armoured public key.
    pub public_key: String,
    /// A label the human will recognise.
    pub label: Option<String>,
}

#[derive(Serialize)]
pub struct DeviceResponse {
    /// The secret the client polls with.
    pub device_code: String,
    /// The code the human types.
    pub user_code: String,
    /// Where to send the human.
    pub verification_uri: String,
    /// When the code stops working, RFC 3339.
    pub expires_at: String,
    /// How often to poll, in seconds.
    pub interval: i64,
}

#[derive(Serialize)]
pub struct DeviceStatus {
    /// `pending`, `approved`, `denied` or `expired`.
    pub status: String,
    /// Who the key is bound to, once approved.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub publisher: Option<PublisherSummary>,
}

#[derive(Serialize)]
pub struct PublisherSummary {
    /// The handle.
    pub handle: String,
    /// `github` or `orcid`.
    pub kind: String,
    /// The provider's name.
    pub display_name: Option<String>,
}

/// `POST /api/v1/auth/device`
///
/// # Errors
///
/// `400` when the key does not parse, `409` when it is already bound.
pub async fn start(
    State(state): State<AppState>,
    Json(request): Json<DeviceRequest>,
) -> ApiResult<Json<DeviceResponse>> {
    let key = SignerKey::detect(&request.public_key).map_err(ApiError::from_core)?;
    if let Some(kind) = aevia_registry_core::signature::KeyKind::parse(&request.kind)
        && kind != key.kind
    {
        return Err(ApiError::bad_request(
            "E_KEY",
            format!(
                "the key is {}, but the request says {}",
                key.kind.as_str(),
                kind.as_str()
            ),
        ));
    }
    let fingerprint = key.fingerprint().map_err(ApiError::from_core)?;
    if let Some(existing) =
        repo::signing_key_by_fingerprint(&state.pool, key.kind.as_str(), &fingerprint).await?
    {
        let publisher = repo::publisher(&state.pool, existing.publisher_id).await?;
        let handle = publisher.map_or_else(|| "an account".to_string(), |row| row.handle);
        return Err(ApiError::conflict(format!(
            "the key {fingerprint} is already bound to {handle}; run `aevia-pkg publish` directly, \
             or generate a new key"
        )));
    }
    let device_code = http::random_hex(32);
    let user_code = http::user_code();
    let expires_at = Utc::now() + chrono::Duration::minutes(DEVICE_MINUTES);
    repo::create_device(
        &state.pool,
        &device_code,
        &user_code,
        key.kind.as_str(),
        &key.armored,
        &fingerprint,
        request.label.as_deref(),
        expires_at,
    )
    .await?;
    Ok(Json(DeviceResponse {
        device_code,
        user_code: user_code.clone(),
        verification_uri: state.config.url(&format!("/activate?code={user_code}")),
        expires_at: expires_at.to_rfc3339(),
        interval: POLL_SECONDS,
    }))
}

/// `GET /api/v1/auth/device/{device_code}`
///
/// # Errors
///
/// `404` when the code is unknown.
pub async fn status(
    State(state): State<AppState>,
    Path(device_code): Path<String>,
) -> ApiResult<Json<DeviceStatus>> {
    let Some(device) = repo::device_by_code(&state.pool, &device_code).await? else {
        return Err(ApiError::not_found(
            "no device authorisation has that code; it may have expired",
        ));
    };
    let status = if device.status == "pending" && device.expires_at < Utc::now() {
        "expired".to_string()
    } else {
        device.status.clone()
    };
    let publisher = match device.publisher_id {
        | Some(id) => {
            repo::publisher(&state.pool, id).await?.map(|row| {
                PublisherSummary {
                    handle: row.handle,
                    kind: row.kind,
                    display_name: row.display_name,
                }
            })
        },
        | None => None,
    };
    Ok(Json(DeviceStatus { status, publisher }))
}

/// The query of `/activate`.
#[derive(Deserialize)]
pub struct ActivateQuery {
    /// The code the terminal printed.
    pub code: Option<String>,
}

/// The form of `/activate`.
#[derive(Deserialize)]
pub struct ActivateForm {
    /// The code, retyped or carried in the hidden field.
    pub user_code: String,
}

/// `GET /activate`
///
/// # Errors
///
/// `400` when the code is missing, `404` when it is unknown or expired.
pub async fn page(
    State(state): State<AppState>,
    crate::http::Language(lang): crate::http::Language,
    Query(query): Query<ActivateQuery>,
    headers: HeaderMap,
) -> PageResult<Response> {
    let Some(raw) = query.code else {
        return Err(PageError::not_found(
            "Open the link your terminal printed, which looks like \
             https://aevia.apich.org/activate?code=ABCD-EFGH",
        ));
    };
    let user_code = http::normalize_user_code(&raw);
    let Some(device) = repo::device_by_user_code(&state.pool, &user_code).await? else {
        return Err(PageError::not_found(
            "That code is not waiting for approval. Codes expire after fifteen minutes; run \
             `aevia-pkg login` again to get a new one.",
        ));
    };
    if device.expires_at < Utc::now() {
        return Err(PageError::not_found(
            "That code has expired. Run `aevia-pkg login` again to get a new one.",
        ));
    }
    let viewer = state.viewer(&headers).await;
    if viewer.is_none() {
        // Send them through sign-in and back here, rather than showing a button
        // that cannot work.
        let next = format!("/activate?code={user_code}");
        let target = format!("/login?next={}", crate::pages::urlencode(&next));
        return Ok(Redirect::to(&target).into_response());
    }
    Ok(Html(aevia_registry_web::activate(
        lang,
        &user_code,
        viewer.as_ref(),
        &device.key_kind,
        &device.fingerprint,
    ))
    .into_response())
}

/// `POST /activate`
///
/// # Errors
///
/// `403` when nobody is signed in, `404` when the code is unknown.
pub async fn submit(
    State(state): State<AppState>,
    headers: HeaderMap,
    Form(form): Form<ActivateForm>,
) -> PageResult<Response> {
    let Some((_, publisher)) = state.current(&headers).await else {
        return Err(PageError::forbidden(
            "Sign in before approving a key, so the registry knows whose key it is.",
        ));
    };
    let user_code = http::normalize_user_code(&form.user_code);
    let Some(device) = repo::device_by_user_code(&state.pool, &user_code).await? else {
        return Err(PageError::not_found(
            "That code is no longer waiting for approval.",
        ));
    };
    if !repo::approve_device(&state.pool, &device.device_code, publisher.id).await? {
        return Err(PageError::not_found(
            "That code has expired or was already used. Run `aevia-pkg login` again.",
        ));
    }
    repo::append_log(
        &state.pool,
        "key",
        &format!("{} bound a {} key", publisher.handle, device.key_kind),
        serde_json::json!({
            "publisher": publisher.handle,
            "fingerprint": device.fingerprint,
            "kind": device.key_kind,
        }),
    )
    .await?;
    Ok(Redirect::to("/account?key=bound").into_response())
}
