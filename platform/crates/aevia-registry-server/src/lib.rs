//! `aevia-registry` as a library, plus the router that serves it.
//!
//! # Why the router lives in a library
//!
//! A binary cannot be imported, so anything defined in `main.rs` can only be
//! reached by starting a process and talking to it over a socket. The router is
//! the thing most worth testing — every route, extractor, layer and error
//! mapping in one value — and a test that has to bind a port, wait for a
//! listener and clean up afterwards is a test that will be flaky on CI and
//! skipped on a laptop. Moving [`router`] here lets an integration test call
//! `router(state).oneshot(request)` and drive real requests through the real
//! stack against a real database, with no socket and no port.
//!
//! `main.rs` keeps only what a process does: read the environment, connect,
//! build the state, bind, serve, and stop on a signal. Everything else is in
//! this crate, so the binary and the tests exercise the same code.

pub mod api;
pub mod auth;
pub mod config;
pub mod error;
pub mod http;
pub mod pages;
pub mod seo;
pub mod spec;
pub mod state;
pub mod storage;

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

use crate::state::AppState;

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
        .route("/cli", get(pages::cli))
        .route("/users/{handle}", get(pages::user))
        .route("/feed.xml", get(seo::feed))
        .route("/sitemap.xml", get(seo::sitemap))
        .route("/robots.txt", get(seo::robots))
        .route("/badge/{owner}/{name}", get(seo::badge))
        .route("/spec", get(pages::spec_index))
        .route("/spec/all", get(pages::spec_all))
        .route("/spec/{slug}", get(pages::spec_chapter))
        .route("/lang/{code}", get(pages::set_language))
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
        crate::http::language_from(&headers),
        StatusCode::NOT_FOUND.as_u16(),
        "E_NOT_FOUND",
        &format!("There is nothing at {}.", uri.path()),
        viewer.as_ref(),
    );
    (StatusCode::NOT_FOUND, Html(html)).into_response()
}
