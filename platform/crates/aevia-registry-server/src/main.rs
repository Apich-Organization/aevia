//! `aevia-registry` — the server behind `aevia.apich.org`.
//!
//! It serves three things from one process: the public site, the JSON API the
//! client uses, and the documentation site. They share a database, an archive
//! store and one notion of identity, which is why they are one binary rather
//! than three services that have to be kept in step.
//!
//! Start-up is deliberately strict: no database URL and no session secret means
//! no start. A registry that boots in a half-configured state serves 500s that
//! look like bugs for hours.

mod api;
mod auth;
mod config;
mod error;
mod http;
mod pages;
mod state;
mod storage;

use std::sync::Arc;
use std::time::Duration;

use axum::Router;
use axum::extract::DefaultBodyLimit;
use axum::extract::State;
use axum::http::HeaderMap;
use axum::http::StatusCode;
use axum::http::Uri;
use axum::response::Html;
use axum::response::IntoResponse;
use axum::response::Response;
use axum::routing::get;
use axum::routing::post;
use tower_http::compression::CompressionLayer;
use tower_http::timeout::TimeoutLayer;
use tower_http::trace::TraceLayer;
use tracing_subscriber::EnvFilter;

use crate::config::Config;
use crate::state::AppState;
use crate::storage::Storage;

#[tokio::main]
async fn main() -> std::process::ExitCode {
    let filter = EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| EnvFilter::new("aevia_registry=info,tower_http=warn"));
    tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_target(true)
        .init();

    let config = match Config::from_env() {
        | Ok(config) => config,
        | Err(error) => {
            eprintln!("aevia-registry: {error}");
            return std::process::ExitCode::from(2);
        },
    };

    let pool = match aevia_registry_db::connect(&config.database_url, 16).await {
        | Ok(pool) => pool,
        | Err(error) => {
            eprintln!("aevia-registry: {}", error.render());
            return std::process::ExitCode::from(1);
        },
    };
    let storage = match Storage::open(&config.storage_dir) {
        | Ok(storage) => storage,
        | Err(error) => {
            eprintln!("aevia-registry: {}", error.render());
            return std::process::ExitCode::from(1);
        },
    };
    tracing::info!(
        storage = %storage.root().display(),
        docs = config.docs_enabled,
        github = config.github.is_some(),
        orcid = config.orcid.is_some(),
        "starting"
    );

    let bind = config.bind;
    let state = AppState::new(pool.clone(), Arc::new(config), storage);

    // Housekeeping: expired nonces and sessions are dead weight, and a table
    // that only grows is a slow leak. Hourly is often enough for a registry
    // that issues a nonce per publish.
    tokio::spawn({
        let pool = pool.clone();
        async move {
            loop {
                tokio::time::sleep(Duration::from_secs(3600)).await;
                if let Err(error) = aevia_registry_db::repo::purge_expired(&pool).await {
                    tracing::warn!(%error, "cannot purge expired rows");
                }
            }
        }
    });

    let app = router(state);
    let listener = match tokio::net::TcpListener::bind(bind).await {
        | Ok(listener) => listener,
        | Err(error) => {
            eprintln!("aevia-registry: cannot listen on {bind}: {error}");
            return std::process::ExitCode::from(1);
        },
    };
    tracing::info!(%bind, "listening");
    if let Err(error) = axum::serve(listener, app)
        .with_graceful_shutdown(shutdown())
        .await
    {
        eprintln!("aevia-registry: the server stopped: {error}");
        return std::process::ExitCode::from(1);
    }
    std::process::ExitCode::SUCCESS
}

/// The whole application.
pub fn router(state: AppState) -> Router {
    Router::new()
        .route("/", get(pages::home))
        .route("/crates", get(pages::crates))
        .route("/crates/{owner}/{name}", get(pages::crate_detail))
        .route(
            "/crates/{owner}/{name}/{version}",
            get(pages::crate_version),
        )
        .route("/crates/{owner}/{name}/yank", post(yank_from_browser))
        .route("/docs", get(pages::docs_landing))
        .route("/docs/{owner}/{name}", get(pages::docs_latest))
        .route("/docs/{owner}/{name}/{version}", get(pages::docs_index))
        .route(
            "/docs/{owner}/{name}/{version}/{module}",
            get(pages::docs_module),
        )
        .route("/account", get(pages::account))
        .route("/account/keys", post(pages::account_add_key))
        .route("/account/keys/{id}/revoke", post(pages::account_revoke_key))
        .route("/login", get(pages::login))
        .route("/logout", post(pages::logout))
        .route("/log", get(pages::log))
        .route(
            "/activate",
            get(auth::device::page).post(auth::device::submit),
        )
        .route("/auth/github/start", get(auth::github::start))
        .route("/auth/github/callback", get(auth::github::callback))
        .route("/auth/orcid/start", get(auth::orcid::start))
        .route("/auth/orcid/callback", get(auth::orcid::callback))
        .route("/static/site.css", get(pages::site_css))
        .route("/static/app.js", get(pages::app_js))
        .route("/healthz", get(pages::healthz))
        .nest("/api/v1", api::router())
        .fallback(not_found)
        .layer(DefaultBodyLimit::max(20 * 1024 * 1024))
        .layer(CompressionLayer::new())
        .layer(TimeoutLayer::with_status_code(
            StatusCode::REQUEST_TIMEOUT,
            Duration::from_secs(120),
        ))
        .layer(TraceLayer::new_for_http())
        .with_state(state)
}

/// The form the package page posts when the owner yanks a version.
#[derive(serde::Deserialize)]
pub struct YankForm {
    /// The version to yank or restore.
    pub version: String,
    /// `true` to yank; absent means restore.
    pub yanked: Option<String>,
}

/// `POST /crates/{owner}/{name}/yank`
///
/// Yanking from the browser is authenticated by the session, not by a
/// signature. That is a deliberate exception to "the signature is the only
/// credential", and it is narrow: a yank is reversible, it cannot introduce new
/// bytes, and it still requires the session of the account that owns the
/// package. Publishing — the action that adds code — never takes this path.
async fn yank_from_browser(
    State(state): State<AppState>,
    axum::extract::Path((owner, name)): axum::extract::Path<(String, String)>,
    headers: HeaderMap,
    axum::Form(form): axum::Form<YankForm>,
) -> Response {
    let Some((_, publisher)) = state.current(&headers).await else {
        return error::PageError::forbidden("Sign in before yanking a version.").into_response();
    };
    let requested = match aevia_registry_core::PackageId::new(&owner, &name) {
        | Ok(id) => id,
        | Err(error) => return error::PageError::from(error).into_response(),
    };
    let found = aevia_registry_db::repo::package_by_handle(
        &state.pool,
        requested.owner(),
        &requested.name_key(),
    )
    .await;
    let (package, _) = match found {
        | Ok(Some(found)) => found,
        | Ok(None) => {
            return error::PageError::not_found(format!("There is no package called {requested}."))
                .into_response();
        },
        | Err(error) => return error::PageError::from(error).into_response(),
    };
    if package.owner_id != publisher.id {
        return error::PageError::forbidden("Only the package's owner can yank a version.")
            .into_response();
    }
    let yanked = form.yanked.is_some();
    match aevia_registry_db::repo::set_yanked(&state.pool, package.id, &form.version, yanked).await
    {
        | Ok(Some(_)) => {
            let _ = aevia_registry_db::repo::touch_package(&state.pool, package.id).await;
            let verb = if yanked {
                "yanked"
            } else {
                "restored"
            };
            let _ = aevia_registry_db::repo::append_log(
                &state.pool,
                "yank",
                &format!("{} {verb} {requested} {}", publisher.handle, form.version),
                serde_json::json!({
                    "name": requested.to_string(),
                    "version": form.version,
                    "yanked": yanked,
                    "publisher": publisher.handle,
                    "via": "browser",
                }),
            )
            .await;
            axum::response::Redirect::to(&format!("/crates/{requested}")).into_response()
        },
        | Ok(None) => {
            error::PageError::not_found(format!("{requested} has no version {}.", form.version))
                .into_response()
        },
        | Err(error) => error::PageError::from(error).into_response(),
    }
}

/// The 404 page.
async fn not_found(
    uri: Uri,
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Response {
    let viewer = state.viewer(&headers).await;
    let html = aevia_registry_web::error_page(
        StatusCode::NOT_FOUND.as_u16(),
        "E_NOT_FOUND",
        &format!("There is nothing at {}.", uri.path()),
        viewer.as_ref(),
    );
    (StatusCode::NOT_FOUND, Html(html)).into_response()
}

/// Waits for `Ctrl-C` or `SIGTERM`, so a container restart drains requests.
async fn shutdown() {
    let ctrl_c = async {
        let _ = tokio::signal::ctrl_c().await;
    };
    #[cfg(unix)]
    let terminate = async {
        if let Ok(mut signal) =
            tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
        {
            signal.recv().await;
        }
    };
    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();
    tokio::select! {
        () = ctrl_c => {},
        () = terminate => {},
    }
    tracing::info!("shutting down");
}
