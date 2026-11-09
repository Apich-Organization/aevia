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
//!
//! The routes themselves live in the crate's library, so an integration test
//! can drive them without a socket; this file is only the process.

use std::sync::Arc;
use std::time::Duration;

use tracing_subscriber::EnvFilter;

use aevia_registry::config::Config;
use aevia_registry::router;
use aevia_registry::spec::SpecStore;
use aevia_registry::state::AppState;
use aevia_registry::storage::Storage;

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

    let spec = match SpecStore::load(&config.spec_dir) {
        | Ok(spec) => Arc::new(spec),
        | Err(error) => {
            eprintln!("aevia-registry: {}", error.render());
            return std::process::ExitCode::from(1);
        },
    };
    if spec.is_empty() {
        tracing::warn!(
            directory = %spec.directory().display(),
            "/spec has no chapters; set AEVIA_SPEC_DIR to the language repository's spec/ directory"
        );
    }

    let bind = config.bind;
    let state = AppState::new(pool.clone(), Arc::new(config), storage, spec);

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
