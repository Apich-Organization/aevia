//! GitHub sign-in, through a GitHub App.
//!
//! The App is used for two things: the user-authorisation flow (which proves
//! the handle) and the installation listing (which proves organisation
//! membership). Nothing is written until both calls succeed.

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

/// The query of `/auth/github/start`.
#[derive(Deserialize)]
pub struct StartParams {
    /// Where to go after signing in.
    pub next: Option<String>,
}

/// The query of `/auth/github/callback`.
#[derive(Deserialize)]
pub struct CallbackParams {
    /// The authorisation code.
    pub code: Option<String>,
    /// The state we issued.
    pub state: Option<String>,
    /// The provider's error, when the user declined.
    pub error: Option<String>,
}

#[derive(Deserialize)]
struct TokenResponse {
    access_token: Option<String>,
    error_description: Option<String>,
}

#[derive(Deserialize)]
struct GithubUser {
    login: String,
    name: Option<String>,
    avatar_url: Option<String>,
}

#[derive(Deserialize)]
struct InstallationList {
    installations: Vec<Installation>,
}

#[derive(Deserialize)]
struct Installation {
    account: Account,
}

#[derive(Deserialize)]
struct Account {
    login: String,
    avatar_url: Option<String>,
    #[serde(rename = "type")]
    kind: String,
}

/// `GET /auth/github/start`
///
/// # Errors
///
/// `503` when no GitHub App is configured.
pub async fn start(
    State(state): State<AppState>,
    Query(params): Query<StartParams>,
) -> Result<Response, PageError> {
    let Some(github) = state.config.github.as_ref() else {
        return Err(PageError::forbidden(
            "GitHub sign-in is not configured on this registry. Set AEVIA_GITHUB_CLIENT_ID and \
             AEVIA_GITHUB_CLIENT_SECRET.",
        ));
    };
    let next = safe_next(params.next.as_deref());
    let state_token = http::random_hex(16);
    repo::create_oauth_state(
        &state.pool,
        &state_token,
        Provider::Github.as_str(),
        &next,
        Utc::now() + chrono::Duration::minutes(10),
    )
    .await?;
    let redirect_uri = state.config.url("/auth/github/callback");
    let target = format!(
        "https://github.com/login/oauth/authorize?client_id={}&state={}&redirect_uri={}&allow_signup=false",
        urlencode(&github.client_id),
        state_token,
        urlencode(&redirect_uri),
    );
    Ok(Redirect::temporary(&target).into_response())
}

/// `GET /auth/github/callback`
///
/// # Errors
///
/// `400` when the state is unknown or the provider refused, `502` when GitHub
/// cannot be reached, `E_DB` when the publisher cannot be recorded.
pub async fn callback(
    State(state): State<AppState>,
    Query(params): Query<CallbackParams>,
    headers: HeaderMap,
) -> Result<Response, PageError> {
    let Some(github) = state.config.github.as_ref() else {
        return Err(PageError::forbidden("GitHub sign-in is not configured"));
    };
    if let Some(error) = params.error {
        return Err(PageError::forbidden(format!(
            "GitHub refused the sign-in: {error}"
        )));
    }
    let (Some(code), Some(state_token)) = (params.code, params.state) else {
        return Err(PageError::forbidden(
            "the sign-in callback is missing its code or state",
        ));
    };
    let Some(next) =
        repo::take_oauth_state(&state.pool, &state_token, Provider::Github.as_str()).await?
    else {
        return Err(PageError::forbidden(
            "this sign-in link has expired or was already used; start again from /login",
        ));
    };

    let redirect_uri = state.config.url("/auth/github/callback");
    let token: TokenResponse = state
        .http
        .post("https://github.com/login/oauth/access_token")
        .header(axum::http::header::ACCEPT, "application/json")
        .form(&[
            ("client_id", github.client_id.as_str()),
            ("client_secret", github.client_secret.as_str()),
            ("code", code.as_str()),
            ("redirect_uri", redirect_uri.as_str()),
        ])
        .send()
        .await
        .map_err(|error| PageError::internal(format!("cannot reach GitHub: {error}")))?
        .json()
        .await
        .map_err(|error| {
            PageError::internal(format!("GitHub's token reply is not JSON: {error}"))
        })?;
    let Some(access_token) = token.access_token else {
        return Err(PageError::forbidden(format!(
            "GitHub refused the token exchange: {}",
            token
                .error_description
                .unwrap_or_else(|| "no reason given".to_string())
        )));
    };

    let user: GithubUser = state
        .http
        .get("https://api.github.com/user")
        .header(axum::http::header::ACCEPT, "application/vnd.github+json")
        .header(
            axum::http::header::AUTHORIZATION,
            format!("Bearer {access_token}"),
        )
        .send()
        .await
        .map_err(|error| PageError::internal(format!("cannot read the GitHub profile: {error}")))?
        .error_for_status()
        .map_err(|error| {
            PageError::internal(format!("GitHub refused the profile request: {error}"))
        })?
        .json()
        .await
        .map_err(|error| PageError::internal(format!("GitHub's profile is not JSON: {error}")))?;

    let handle = aevia_registry_core::name::normalize_owner(&user.login);
    let identity = Identity {
        kind: "github",
        handle: handle.clone(),
        display_name: user.name,
        avatar_url: user.avatar_url,
    };
    let publisher = crate::auth::upsert(&state, &identity).await?;

    // Organisation membership: only installations the user can actually reach.
    // A failure here must not block sign-in — it only limits which scopes the
    // user may publish under.
    match installations(&state, &access_token).await {
        | Ok(orgs) => {
            for (org, avatar) in orgs {
                if let Ok(org_publisher) =
                    repo::upsert_org(&state.pool, &org, None, avatar.as_deref()).await
                    && let Err(error) = repo::record_membership(
                        &state.pool,
                        &org_publisher.handle,
                        publisher.id,
                        "member",
                    )
                    .await
                    {
                        tracing::warn!(%error, org = %org, "cannot record organisation membership");
                    }
            }
        },
        | Err(error) => tracing::warn!(%error, "cannot list GitHub App installations"),
    }

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

async fn installations(
    state: &AppState,
    access_token: &str,
) -> Result<Vec<(String, Option<String>)>, aevia_registry_core::Error> {
    let response = state
        .http
        .get("https://api.github.com/user/installations")
        .header(axum::http::header::ACCEPT, "application/vnd.github+json")
        .header(
            axum::http::header::AUTHORIZATION,
            format!("Bearer {access_token}"),
        )
        .send()
        .await
        .map_err(|error| {
            aevia_registry_core::Error::new("E_HTTP", format!("cannot list installations: {error}"))
        })?;
    if !response.status().is_success() {
        return Err(aevia_registry_core::Error::new(
            "E_HTTP",
            format!(
                "GitHub answered {} for the installation list",
                response.status()
            ),
        ));
    }
    let list: InstallationList = response.json().await.map_err(|error| {
        aevia_registry_core::Error::new(
            "E_HTTP",
            format!("the installation list is not JSON: {error}"),
        )
    })?;
    Ok(list
        .installations
        .into_iter()
        .filter(|installation| installation.account.kind == "Organization")
        .map(|installation| {
            (
                aevia_registry_core::name::normalize_owner(&installation.account.login),
                installation.account.avatar_url,
            )
        })
        .collect())
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
