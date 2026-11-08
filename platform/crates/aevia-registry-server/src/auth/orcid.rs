//! ORCID sign-in.
//!
//! ORCID is the identifier a researcher already has, so a registry for a
//! scientific language should accept it as a first-class identity rather than
//! forcing everyone through a code host. The flow is plain OAuth 2.0 with the
//! `/authenticate` scope, which yields an iD and a name and nothing else.

use axum::extract::Query;
use axum::extract::State;
use axum::http::HeaderMap;
use axum::response::IntoResponse;
use axum::response::Redirect;
use axum::response::Response;
use chrono::Utc;
use serde::Deserialize;

use aevia_registry_db::repo;

use crate::auth::Identity;
use crate::auth::Provider;
use crate::error::PageError;
use crate::http;
use crate::state::AppState;
use crate::state::safe_next;

/// The query of `/auth/orcid/start`.
#[derive(Deserialize)]
pub struct StartParams {
    /// Where to go after signing in.
    pub next: Option<String>,
}

/// The query of `/auth/orcid/callback`.
#[derive(Deserialize)]
pub struct CallbackParams {
    /// The authorisation code.
    pub code: Option<String>,
    /// The state we issued.
    pub state: Option<String>,
    /// The provider's error, when the user declined.
    pub error: Option<String>,
    /// The provider's error description.
    pub error_description: Option<String>,
}

#[derive(Deserialize)]
struct TokenResponse {
    access_token: Option<String>,
    orcid: Option<String>,
    error_description: Option<String>,
}

#[derive(Deserialize)]
struct PersonRecord {
    name: Option<PersonName>,
}

#[derive(Deserialize)]
struct PersonName {
    #[serde(rename = "given-names")]
    given: Option<Value>,
    #[serde(rename = "family-name")]
    family: Option<Value>,
}

#[derive(Deserialize)]
struct Value {
    value: Option<String>,
}

/// `GET /auth/orcid/start`
///
/// # Errors
///
/// `403` when no ORCID client is configured.
pub async fn start(
    State(state): State<AppState>,
    Query(params): Query<StartParams>,
) -> Result<Response, PageError> {
    let Some(orcid) = state.config.orcid.as_ref() else {
        return Err(PageError::forbidden(
            "ORCID sign-in is not configured on this registry. Set AEVIA_ORCID_CLIENT_ID and \
             AEVIA_ORCID_CLIENT_SECRET.",
        ));
    };
    let next = safe_next(params.next.as_deref());
    let state_token = http::random_hex(16);
    repo::create_oauth_state(
        &state.pool,
        &state_token,
        Provider::Orcid.as_str(),
        &next,
        Utc::now() + chrono::Duration::minutes(10),
    )
    .await?;
    let redirect_uri = state.config.url("/auth/orcid/callback");
    let target = format!(
        "{}/oauth/authorize?client_id={}&response_type=code&scope=/authenticate&redirect_uri={}&state={}",
        orcid.base,
        urlencode(&orcid.client_id),
        urlencode(&redirect_uri),
        state_token,
    );
    Ok(Redirect::temporary(&target).into_response())
}

/// `GET /auth/orcid/callback`
///
/// # Errors
///
/// `400` when the state is unknown or the provider refused, `502` when ORCID
/// cannot be reached, `E_DB` when the publisher cannot be recorded.
pub async fn callback(
    State(state): State<AppState>,
    Query(params): Query<CallbackParams>,
    headers: HeaderMap,
) -> Result<Response, PageError> {
    let Some(orcid) = state.config.orcid.as_ref() else {
        return Err(PageError::forbidden("ORCID sign-in is not configured"));
    };
    if let Some(error) = params.error {
        return Err(PageError::forbidden(format!(
            "ORCID refused the sign-in: {}",
            params.error_description.unwrap_or(error)
        )));
    }
    let (Some(code), Some(state_token)) = (params.code, params.state) else {
        return Err(PageError::forbidden(
            "the sign-in callback is missing its code or state",
        ));
    };
    let Some(next) =
        repo::take_oauth_state(&state.pool, &state_token, Provider::Orcid.as_str()).await?
    else {
        return Err(PageError::forbidden(
            "this sign-in link has expired or was already used; start again from /login",
        ));
    };

    let redirect_uri = state.config.url("/auth/orcid/callback");
    let token: TokenResponse = state
        .http
        .post(format!("{}/oauth/token", orcid.base))
        .header(axum::http::header::ACCEPT, "application/json")
        .form(&[
            ("client_id", orcid.client_id.as_str()),
            ("client_secret", orcid.client_secret.as_str()),
            ("grant_type", "authorization_code"),
            ("code", code.as_str()),
            ("redirect_uri", redirect_uri.as_str()),
        ])
        .send()
        .await
        .map_err(|error| PageError::internal(format!("cannot reach ORCID: {error}")))?
        .json()
        .await
        .map_err(|error| {
            PageError::internal(format!("ORCID's token reply is not JSON: {error}"))
        })?;
    let Some(access_token) = token.access_token else {
        return Err(PageError::forbidden(format!(
            "ORCID refused the token exchange: {}",
            token
                .error_description
                .unwrap_or_else(|| "no reason given".to_string())
        )));
    };
    let Some(orcid_id) = token.orcid else {
        return Err(PageError::forbidden(
            "ORCID's token reply carries no iD, which means the /authenticate scope was not granted",
        ));
    };
    let handle = aevia_registry_core::name::normalize_owner(&orcid_id);
    let display_name = person_name(&state, orcid.base.as_str(), &handle, &access_token).await;
    let identity = Identity {
        kind: "orcid",
        handle,
        display_name,
        // ORCID publishes no avatar; the page falls back to the initials.
        avatar_url: None,
    };
    let publisher = crate::auth::upsert(&state, &identity).await?;
    let response = crate::state::sign_in(
        &state,
        publisher.id,
        &next,
        headers
            .get(axum::http::header::USER_AGENT)
            .and_then(|value| value.to_str().ok()),
    )
    .await?;
    Ok(response)
}

/// Reads the name from the public record, ignoring any failure: a missing name
/// is a cosmetic loss, not a reason to refuse the sign-in.
async fn person_name(
    state: &AppState,
    base: &str,
    orcid_id: &str,
    access_token: &str,
) -> Option<String> {
    let response = state
        .http
        .get(format!("{base}/v3.0/{orcid_id}/person"))
        .header(axum::http::header::ACCEPT, "application/json")
        .header(
            axum::http::header::AUTHORIZATION,
            format!("Bearer {access_token}"),
        )
        .send()
        .await
        .ok()?;
    if !response.status().is_success() {
        return None;
    }
    let record: PersonRecord = response.json().await.ok()?;
    let name = record.name?;
    let given = name.given.and_then(|value| value.value).unwrap_or_default();
    let family = name
        .family
        .and_then(|value| value.value)
        .unwrap_or_default();
    let joined = format!("{} {}", given.trim(), family.trim());
    let joined = joined.trim();
    if joined.is_empty() {
        None
    } else {
        Some(joined.to_string())
    }
}

fn urlencode(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    for byte in value.bytes() {
        match byte {
            | b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(char::from(byte));
            },
            | _ => out.push_str(&format!("%{byte:02X}")),
        }
    }
    out
}
