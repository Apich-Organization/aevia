//! Shared server state.
//!
//! One `Arc`-cloned struct travels through every handler: the pool, the
//! configuration, the archive store and one HTTP client. The client is shared
//! deliberately — building a `reqwest::Client` per request would open a new
//! connection pool to GitHub or ORCID each time.

use std::sync::Arc;

use axum::http::HeaderMap;
use chrono::Utc;
use hmac::Hmac;
use hmac::Mac as _;
use sha2::Sha256;
use sqlx::PgPool;
use tokio::sync::Notify;
use tokio::sync::RwLock;
use tokio::sync::Semaphore;

use aevia_registry_core::vm::Viewer;
use aevia_registry_db::models::Publisher;
use aevia_registry_db::models::Session;
use aevia_registry_db::repo;

use crate::config::Config;
use crate::download_counts::DownloadCounts;
use crate::http;
use crate::limits::RateLimits;
use crate::manual::ManualStore;
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
    /// The bilingual electronic manuals.
    pub manuals: Arc<RwLock<Arc<ManualStore>>>,
    /// The outbound HTTP client.
    pub http: reqwest::Client,
    /// Limits simultaneous decompression and publication of large archives.
    pub(crate) publish_slots: Arc<Semaphore>,
    pub(crate) download_slots: Arc<Semaphore>,
    pub(crate) download_counts: Arc<DownloadCounts>,
    pub(crate) limits: Arc<RateLimits>,
    pub(crate) docs_wake: Arc<Notify>,
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
        manuals: Arc<ManualStore>,
    ) -> Self {
        let http = reqwest::Client::builder()
            // A provider that does not answer in ten seconds is not going to.
            .timeout(std::time::Duration::from_secs(10))
            .redirect(reqwest::redirect::Policy::none())
            .user_agent(concat!("aevia-registry/", env!("CARGO_PKG_VERSION")))
            .build()
            .unwrap_or_default();
        Self {
            pool,
            config,
            storage,
            spec: Arc::new(RwLock::new(spec)),
            stdlib_docs: Arc::new(RwLock::new(stdlib_docs)),
            manuals: Arc::new(RwLock::new(manuals)),
            http,
            publish_slots: Arc::new(Semaphore::new(2)),
            download_slots: Arc::new(Semaphore::new(8)),
            download_counts: Arc::new(DownloadCounts::default()),
            limits: Arc::new(RateLimits::new()),
            docs_wake: Arc::new(Notify::new()),
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
        let mut previous = self.spec.write().await;
        if loaded.is_empty() && !previous.is_empty() {
            return Err(aevia_registry_core::Error::new(
                crate::spec::E_SPEC,
                "the specification reload would remove all chapters",
            ));
        }
        *previous = Arc::new(loaded);
        drop(previous);
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
        let mut previous = self.stdlib_docs.write().await;
        if loaded.is_empty() && !previous.is_empty() {
            return Err(aevia_registry_core::Error::new(
                crate::stdlib::E_STDLIB,
                "the standard library documentation reload would remove all modules",
            ));
        }
        *previous = Arc::new(loaded);
        drop(previous);
        Ok(count)
    }

    pub(crate) async fn reload_manuals(&self) -> Result<(), aevia_registry_core::Error> {
        let config = self.config.clone();
        let loaded = tokio::task::spawn_blocking(move || {
            ManualStore::load(&config.manuals_dir, &config.base_url)
        })
        .await
        .map_err(|error| {
            aevia_registry_core::Error::new(
                "E_MANUAL",
                format!("manual reload task failed: {error}"),
            )
        })??;
        let mut previous = self.manuals.write().await;
        if loaded.loses_manual(&previous) {
            return Err(aevia_registry_core::Error::new(
                "E_MANUAL",
                "the reload would remove a published manual",
            ));
        }
        *previous = Arc::new(loaded);
        drop(previous);
        Ok(())
    }

    /// The live session behind a request's cookie, if there is one.
    pub async fn current(
        &self,
        headers: &HeaderMap,
    ) -> Option<(Session, Publisher)> {
        let id = self.session_key(headers)?;
        match repo::session_publisher(&self.pool, &id).await {
            | Ok(found) => found,
            | Err(error) => {
                tracing::warn!(%error, "cannot read the session");
                None
            },
        }
    }

    pub(crate) fn session_key(
        &self,
        headers: &HeaderMap,
    ) -> Option<String> {
        let secure = http::is_secure(&self.config.base_url);
        if secure && let Some(token) = http::cookie(headers, http::SECURE_SESSION_COOKIE) {
            return session_key(&self.config.session_secret, token.strip_prefix("v1.")?);
        }
        let token = http::cookie(headers, http::SESSION_COOKIE)?;
        if !secure && let Some(nonce) = token.strip_prefix("v1.") {
            return session_key(&self.config.session_secret, nonce);
        }
        // Existing session rows hold their original 64-character identifier.
        // New sessions cannot be looked up through this compatibility path.
        valid_nonce(&token).then_some(token)
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

    let nonce = http::random_hex(32);
    let id = session_key(&state.config.session_secret, &nonce).ok_or_else(|| {
        aevia_registry_core::Error::new("E_DB", "cannot derive a session identifier")
    })?;
    let expires_at = Utc::now()
        .checked_add_signed(chrono::Duration::days(SESSION_DAYS))
        .ok_or_else(|| {
            aevia_registry_core::Error::new("E_DB", "cannot calculate the session expiry")
        })?;
    repo::create_session(&state.pool, &id, publisher_id, expires_at, user_agent).await?;
    let cookie = http::session_cookie(
        &format!("v1.{nonce}"),
        SESSION_DAYS * 24 * 60 * 60,
        http::is_secure(&state.config.base_url),
    );
    let mut response = axum::response::Redirect::to(next).into_response();
    response
        .headers_mut()
        .insert(http::set_cookie_header(), cookie);
    Ok(response)
}

fn session_key(
    secret: &[u8],
    nonce: &str,
) -> Option<String> {
    if !valid_nonce(nonce) {
        return None;
    }
    let mut mac = Hmac::<Sha256>::new_from_slice(secret).ok()?;
    mac.update(b"aevia-browser-session-v1\0");
    mac.update(nonce.as_bytes());
    Some(format!("v1.{}", hex::encode(mac.finalize().into_bytes())))
}

fn valid_nonce(value: &str) -> bool {
    value.len() == 64 && value.bytes().all(|byte| byte.is_ascii_hexdigit())
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
                && !target.contains('\\')
                && target.parse::<axum::http::Uri>().is_ok() =>
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
    fn stored_session_keys_are_distinct_from_the_cookie_and_depend_on_the_secret() {
        let nonce = "a".repeat(64);
        let key = session_key(b"first deployment key", &nonce).unwrap();
        assert_ne!(key, format!("v1.{nonce}"));
        assert_ne!(Some(key), session_key(b"rotated deployment key", &nonce));
        assert_eq!(session_key(b"deployment key", "bad nonce"), None);
    }

    #[test]
    fn absolute_and_protocol_relative_targets_are_refused() {
        for target in [
            "https://evil.example",
            "//evil.example/x",
            "/\\evil.example",
            "/bad\nLocation: https://evil.example",
            "javascript:alert(1)",
        ] {
            assert_eq!(safe_next(Some(target)), "/account", "{target}");
        }
        assert_eq!(safe_next(None), "/account");
    }
}
