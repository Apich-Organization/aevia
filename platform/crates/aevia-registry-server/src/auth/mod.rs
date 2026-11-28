//! Sign-in: GitHub App user authorisation and ORCID.
//!
//! # Why there are no passwords
//!
//! The registry never stores a credential. Signing in delegates the proof of
//! identity to GitHub or ORCID, and the registry keeps only what those
//! providers assert: a handle, a display name, an avatar. There is nothing here
//! to leak, nothing to reset, and no password policy to get wrong.
//!
//! # Why the GitHub side is an App and not an OAuth App
//!
//! An OAuth App can read a user's public profile and nothing else. A GitHub App
//! additionally knows which installations the user can reach, which is what
//! lets the registry confirm organisation membership: publishing as
//! `apich-organization/units` requires the App to be installed on that
//! organisation *and* the signed-in user to have access to the installation.
//! That check is what stops a package from claiming an organisation's scope.
//!
//! Both flows end the same way: a `publishers` row and a session cookie. The
//! device-authorisation flow in [`device`] then binds a signing key to that row,
//! and from that point the browser is not needed to publish.

pub mod device;
pub mod github;
pub mod orcid;

use axum::http::HeaderMap;
use axum::http::HeaderValue;
use axum::http::header;
use axum::response::Response;
use chrono::Utc;
use subtle::ConstantTimeEq as _;

use aevia_registry_db::repo;

use crate::error::PageError;
use crate::http;
use crate::state::AppState;

/// The provider a publisher signed in with.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Provider {
    /// GitHub, through a GitHub App.
    Github,
    /// ORCID, through its OAuth server.
    Orcid,
}

impl Provider {
    /// The path segment, `github` or `orcid`.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            | Self::Github => "github",
            | Self::Orcid => "orcid",
        }
    }
}

const STATE_MINUTES: i64 = 10;

pub(crate) async fn issue_state(
    state: &AppState,
    provider: Provider,
    next: &str,
) -> Result<String, PageError> {
    let token = http::random_hex(32);
    let expires_at = Utc::now()
        .checked_add_signed(chrono::Duration::minutes(STATE_MINUTES))
        .ok_or_else(|| PageError::internal("cannot calculate the OAuth state expiry"))?;
    repo::create_oauth_state(&state.pool, &token, provider.as_str(), next, expires_at).await?;
    Ok(token)
}

pub(crate) async fn take_state(
    state: &AppState,
    provider: Provider,
    token: &str,
    headers: &HeaderMap,
) -> Result<String, PageError> {
    let secure = http::is_secure(&state.config.base_url);
    if token.len() == 64
        && state_cookie_matches(headers, provider, secure, token)
        && let Some(next) = repo::take_oauth_state(&state.pool, token, provider.as_str()).await?
    {
        return Ok(next);
    }
    Err(PageError::forbidden(
        "This sign-in attempt is unavailable in this browser. Start again from /login.",
    ))
}

#[allow(
    clippy::single_call_fn,
    reason = "the OAuth CSRF cookie is checked in constant time"
)]
fn state_cookie_matches(
    headers: &HeaderMap,
    provider: Provider,
    secure: bool,
    token: &str,
) -> bool {
    http::cookie(headers, &state_cookie_name(provider, secure))
        .is_some_and(|cookie| bool::from(cookie.as_bytes().ct_eq(token.as_bytes())))
}

fn state_cookie_name(
    provider: Provider,
    secure: bool,
) -> String {
    let host_only = if secure { "__Host-" } else { "" };
    format!("{host_only}aevia_oauth_{}", provider.as_str())
}

pub(crate) fn set_state_cookie(
    response: &mut Response,
    state: &AppState,
    provider: Provider,
    token: Option<&str>,
) {
    let secure = http::is_secure(&state.config.base_url);
    let name = state_cookie_name(provider, secure);
    let max_age = if token.is_some() {
        STATE_MINUTES * 60
    } else {
        0
    };
    let value = token.unwrap_or_default();
    let suffix = if secure { "; Secure" } else { "" };
    let raw = format!("{name}={value}; Path=/; HttpOnly; SameSite=Lax; Max-Age={max_age}{suffix}");
    if let Ok(value) = HeaderValue::from_str(&raw) {
        response.headers_mut().append(header::SET_COOKIE, value);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn oauth_state_requires_the_cookie_for_the_provider() {
        let mut headers = HeaderMap::new();
        headers.insert(
            header::COOKIE,
            HeaderValue::from_static("__Host-aevia_oauth_github=abc"),
        );
        assert!(state_cookie_matches(
            &headers,
            Provider::Github,
            true,
            "abc"
        ));
        assert!(!state_cookie_matches(
            &headers,
            Provider::Github,
            true,
            "def"
        ));
        assert!(!state_cookie_matches(
            &headers,
            Provider::Orcid,
            true,
            "abc"
        ));
        assert!(!state_cookie_matches(
            &HeaderMap::new(),
            Provider::Github,
            true,
            "abc"
        ));
    }
}
