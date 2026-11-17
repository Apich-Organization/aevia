//! Shared server state.
//!
//! One `Arc`-cloned struct travels through every handler: the pool, the
//! configuration, the archive store and one HTTP client. The client is shared
//! deliberately — building a `reqwest::Client` per request would open a new
//! connection pool to GitHub or ORCID each time.

use std::sync::Arc;

use axum::http::HeaderMap;
use chrono::Utc;
use sqlx::PgPool;
use tokio::sync::RwLock;

use aevia_registry_core::vm::Viewer;
use aevia_registry_db::models::Publisher;
use aevia_registry_db::models::Session;
use aevia_registry_db::repo;

use crate::config::Config;
use crate::http;
use crate::spec::SpecStore;
use crate::stdlib::StdlibDocStore;
use crate::storage::Storage;

/// Everything a handler needs.
#[derive(Clone)]
pub struct AppState {
    /// The connection pool.
    pub pool: PgPool,
    /// The configuration.
    pub config: Arc<Config>,
    /// The archive store.
    pub storage: Storage,
    /// The language specification, hot-reloaded when `AEVIA_SPEC_DIR` changes.
    pub spec: Arc<RwLock<Arc<SpecStore>>>,
    /// The standard library documentation, hot-reloaded when `AEVIA_STDLIB_DOCS_DIR` changes.
    pub stdlib_docs: Arc<RwLock<Arc<StdlibDocStore>>>,
    /// The outbound HTTP client.
    pub http: reqwest::Client,
}

impl AppState {
    /// Builds the state.
    #[must_use]
    pub fn new(
        pool: PgPool,
        config: Arc<Config>,
        storage: Storage,
        spec: Arc<SpecStore>,
        stdlib_docs: Arc<StdlibDocStore>,
    ) -> Self {
        let http = reqwest::Client::builder()
            // A provider that does not answer in ten seconds is not going to.
            .timeout(std::time::Duration::from_secs(10))
            .user_agent(concat!("aevia-registry/", env!("CARGO_PKG_VERSION")))
            .build()
            .unwrap_or_default();
        Self {
            pool,
            config,
            storage,
            spec: Arc::new(RwLock::new(spec)),
            stdlib_docs: Arc::new(RwLock::new(stdlib_docs)),
            http,
        }
    }

    /// Reloads the specification from `config.spec_dir`.
    ///
    /// # Errors
    ///
    /// Returns an error if the directory cannot be read or the blocking task panics.
    pub async fn reload_spec(&self) -> Result<usize, aevia_registry_core::Error> {
        let dir = self.config.spec_dir.clone();
        let loaded = tokio::task::spawn_blocking(move || SpecStore::load(&dir))
            .await
            .map_err(|error| {
                aevia_registry_core::Error::new(
                    crate::spec::E_SPEC,
                    format!("spec reload task failed: {error}"),
                )
            })??;
        let count = loaded.book().chapters.len();
        *self.spec.write().await = Arc::new(loaded);
        Ok(count)
    }

    /// Reloads the standard library documentation from `config.stdlib_docs_dir`.
    ///
    /// # Errors
    ///
    /// Returns an error if the directory cannot be read/parsed or the blocking task panics.
    pub async fn reload_stdlib_docs(&self) -> Result<usize, aevia_registry_core::Error> {
        let dir = self.config.stdlib_docs_dir.clone();
        let loaded = tokio::task::spawn_blocking(move || StdlibDocStore::load(&dir))
            .await
            .map_err(|error| {
                aevia_registry_core::Error::new(
                    crate::stdlib::E_STDLIB,
                    format!("stdlib reload task failed: {error}"),
                )
            })??;
        let count = loaded.book().modules.len();
        *self.stdlib_docs.write().await = Arc::new(loaded);
        Ok(count)
    }

    /// The live session behind a request's cookie, if there is one.
    pub async fn current(
        &self,
        headers: &HeaderMap,
    ) -> Option<(Session, Publisher)> {
        let id = http::cookie(headers, http::SESSION_COOKIE)?;
        match repo::session_publisher(&self.pool, &id).await {
            | Ok(found) => found,
            | Err(error) => {
                tracing::warn!(%error, "cannot read the session");
                None
            },
        }
    }

    /// The viewer for the page chrome, if someone is signed in.
    pub async fn viewer(
        &self,
        headers: &HeaderMap,
    ) -> Option<Viewer> {
        self.current(headers)
            .await
            .map(|(_, publisher)| viewer_of(&publisher))
    }

    /// Whether a publisher may manage a package owned by `owner_id`.
    #[must_use]
    pub fn can_manage(
        &self,
        publisher: Option<&Publisher>,
        owner_id: i64,
    ) -> bool {
        publisher.is_some_and(|publisher| publisher.id == owner_id)
    }
}

/// The public shape of a publisher.
#[must_use]
pub fn viewer_of(publisher: &Publisher) -> Viewer {
    Viewer {
        handle: publisher.handle.clone(),
        kind: publisher.kind.clone(),
        display_name: publisher.display_name.clone(),
        avatar_url: publisher.avatar_url.clone(),
    }
}

/// How long a browser session lasts.
pub const SESSION_DAYS: i64 = 30;

/// Starts a session and returns the redirect that carries its cookie.
///
/// # Errors
///
/// `E_DB` when the session cannot be recorded.
pub async fn sign_in(
    state: &AppState,
    publisher_id: i64,
    next: &str,
    user_agent: Option<&str>,
) -> Result<axum::response::Response, aevia_registry_core::Error> {
    use axum::response::IntoResponse as _;

    let id = http::random_hex(32);
    let expires_at = Utc::now() + chrono::Duration::days(SESSION_DAYS);
    repo::create_session(&state.pool, &id, publisher_id, expires_at, user_agent).await?;
    let cookie = http::session_cookie(
        &id,
        SESSION_DAYS * 24 * 60 * 60,
        http::is_secure(&state.config.base_url),
    );
    let mut response = axum::response::Redirect::to(next).into_response();
    response
        .headers_mut()
        .insert(http::set_cookie_header(), cookie);
    Ok(response)
}

/// Rejects a `next` target that could leave the site.
///
/// An open redirect is a phishing primitive: `?next=https://evil.example` must
/// not turn the registry's own sign-in link into a bounce to someone else.
#[must_use]
pub fn safe_next(next: Option<&str>) -> String {
    match next {
        | Some(target)
            if target.starts_with('/')
                && !target.starts_with("//")
                && !target.contains("://")
                && !target.contains('\\') =>
        {
            target.to_string()
        },
        | _ => "/account".to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn local_paths_survive() {
        assert_eq!(
            safe_next(Some("/activate?code=ABCD-EFGH")),
            "/activate?code=ABCD-EFGH"
        );
        assert_eq!(safe_next(Some("/crates/apich/x")), "/crates/apich/x");
    }

    #[test]
    fn absolute_and_protocol_relative_targets_are_refused() {
        for target in [
            "https://evil.example",
            "//evil.example/x",
            "/\\evil.example",
            "javascript:alert(1)",
        ] {
            assert_eq!(safe_next(Some(target)), "/account", "{target}");
        }
        assert_eq!(safe_next(None), "/account");
    }
}
