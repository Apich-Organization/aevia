//! GitHub sign-in, through a GitHub App.
//!
//! The authenticated profile establishes the user's stable GitHub ID. An
//! organisation grant also requires active membership and installation access
//! for the same numeric organisation ID.

use std::collections::BTreeMap;
use std::collections::HashSet;
use std::fmt::Write as _;
use std::time::Duration;

use axum::extract::Query;
use axum::extract::State;
use axum::http::HeaderMap;
use axum::http::header;
use axum::response::IntoResponse;
use axum::response::Redirect;
use axum::response::Response;
use serde::Deserialize;
use serde::de::DeserializeOwned;

use aevia_registry_db::models::GithubOrganisation;
use aevia_registry_db::repo;

use crate::auth::Provider;
use crate::error::PageError;
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
    id: i64,
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
    id: i64,
    login: String,
    avatar_url: Option<String>,
    #[serde(rename = "type")]
    kind: String,
}

#[derive(Deserialize)]
struct Membership {
    state: String,
    organization: Organisation,
}

#[derive(Deserialize)]
struct Organisation {
    id: i64,
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
    let state_token = super::issue_state(&state, Provider::Github, &next).await?;
    let redirect_uri = state.config.url("/auth/github/callback");
    let target = format!(
        "https://github.com/login/oauth/authorize?client_id={}&state={}&redirect_uri={}&allow_signup=false",
        urlencode(&github.client_id),
        state_token,
        urlencode(&redirect_uri),
    );
    let mut response = Redirect::temporary(&target).into_response();
    super::set_state_cookie(&mut response, &state, Provider::Github, Some(&state_token));
    Ok(response)
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
    let next = super::take_state(&state, Provider::Github, &state_token, &headers).await?;

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

    let publisher = repo::upsert_github_user(
        &state.pool,
        user.id,
        &user.login,
        user.name.as_deref(),
        user.avatar_url.as_deref(),
    )
    .await?
    .ok_or_else(|| {
        PageError::forbidden(
            "This GitHub account cannot claim the Aevia namespace. For an existing account, \
             contact the registry operator for a reviewed identity binding.",
        )
    })?;

    let generation = repo::begin_membership_refresh(&state.pool, publisher.id).await?;
    match tokio::time::timeout(
        Duration::from_secs(15),
        organisation_scopes(&state, &access_token),
    )
    .await
    {
        | Ok(Ok(orgs)) => {
            if let Err(error) =
                repo::finish_membership_refresh(&state.pool, publisher.id, generation, &orgs).await
            {
                tracing::warn!(%error, "cannot persist GitHub organisation scopes");
            }
        },
        | Ok(Err(error)) => tracing::warn!(%error, "cannot refresh GitHub organisation scopes"),
        | Err(_) => tracing::warn!("GitHub organisation scope lookup timed out"),
    }

    let mut response = crate::state::sign_in(
        &state,
        publisher.id,
        &next,
        headers
            .get(axum::http::header::USER_AGENT)
            .and_then(|value| value.to_str().ok()),
    )
    .await?;
    super::set_state_cookie(&mut response, &state, Provider::Github, None);
    Ok(response)
}

#[allow(
    clippy::single_call_fn,
    reason = "GitHub organisation membership is a separate authorization request"
)]
async fn organisation_scopes(
    state: &AppState,
    access_token: &str,
) -> Result<Vec<GithubOrganisation>, aevia_registry_core::Error> {
    let (installations, memberships) = tokio::try_join!(
        collect_pages(
            state,
            access_token,
            "https://api.github.com/user/installations",
            |value: InstallationList| value.installations
        ),
        collect_pages(
            state,
            access_token,
            "https://api.github.com/user/memberships/orgs",
            |value: Vec<Membership>| value
        ),
    )?;
    Ok(verified_organisations(installations, memberships))
}

#[allow(
    clippy::single_call_fn,
    reason = "membership claims have their own pagination and scope policy"
)]
fn verified_organisations(
    installations: Vec<Installation>,
    memberships: Vec<Membership>,
) -> Vec<GithubOrganisation> {
    let active: HashSet<_> = memberships
        .into_iter()
        .filter(|membership| membership.state == "active" && membership.organization.id > 0)
        .map(|membership| membership.organization.id)
        .collect();
    let granted: BTreeMap<_, _> = installations
        .into_iter()
        .map(|installation| installation.account)
        .filter(|account| account.kind == "Organization" && active.contains(&account.id))
        .map(|account| {
            (
                account.id,
                GithubOrganisation {
                    github_id: account.id,
                    login: account.login,
                    avatar_url: account.avatar_url,
                },
            )
        })
        .collect();
    granted.into_values().collect()
}

async fn collect_pages<T: DeserializeOwned, Item>(
    state: &AppState,
    token: &str,
    endpoint: &str,
    into_items: impl Fn(T) -> Vec<Item>,
) -> Result<Vec<Item>, aevia_registry_core::Error> {
    let mut items = Vec::new();
    for page in 1..=20 {
        let response = state
            .http
            .get(endpoint)
            .query(&[("per_page", 100), ("page", page)])
            .header(header::ACCEPT, "application/vnd.github+json")
            .bearer_auth(token)
            .send()
            .await
            .and_then(reqwest::Response::error_for_status)
            .map_err(|error| github_error(&error))?;
        let has_next = response
            .headers()
            .get(header::LINK)
            .and_then(|links| links.to_str().ok())
            .is_some_and(|links| links.contains("rel=\"next\""));
        let document = response
            .json()
            .await
            .map_err(|error| github_error(&error))?;
        items.extend(into_items(document));
        if !has_next {
            return Ok(items);
        }
    }
    Err(aevia_registry_core::Error::new(
        "E_HTTP",
        "GitHub membership pagination exceeded the request budget",
    ))
}

fn github_error(error: &reqwest::Error) -> aevia_registry_core::Error {
    aevia_registry_core::Error::new(
        "E_HTTP",
        format!("GitHub organisation data request failed: {error}"),
    )
}

fn urlencode(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    for byte in value.bytes() {
        match byte {
            | b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(char::from(byte));
            },
            | _ => {
                let _ = write!(out, "%{byte:02X}");
            },
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn github_identity_grants_require_matching_active_organisation_ids() {
        let installations = serde_json::from_value(serde_json::json!([
            {"account": {"id": 11, "login": "renamed-lab", "type": "Organization"}},
            {"account": {"id": 22, "login": "original-lab", "type": "Organization"}},
            {"account": {"id": 33, "login": "pending-lab", "type": "Organization"}},
            {"account": {"id": 44, "login": "personal-account", "type": "User"}}
        ]))
        .unwrap();
        let memberships = serde_json::from_value(serde_json::json!([
            {"state": "active", "organization": {"id": 11, "login": "original-lab"}},
            {"state": "pending", "organization": {"id": 33, "login": "pending-lab"}},
            {"state": "active", "organization": {"id": 44, "login": "personal-account"}}
        ]))
        .unwrap();

        let scopes = verified_organisations(installations, memberships);
        assert_eq!(scopes.len(), 1);
        assert_eq!(scopes[0].github_id, 11);
        assert_eq!(scopes[0].login, "renamed-lab");
    }
}
