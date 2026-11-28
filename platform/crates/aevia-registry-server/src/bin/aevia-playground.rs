//! Private rootless Podman worker for the Aevia Book.

#![cfg_attr(
    not(test),
    deny(
        clippy::arithmetic_side_effects,
        clippy::indexing_slicing,
        clippy::nursery,
        clippy::panic,
        clippy::pedantic,
        clippy::single_call_fn,
        clippy::unreachable
    )
)]

use std::process::ExitCode;

use aevia_registry::sandbox;
use aevia_registry::sandbox::WorkerConfig;
use tracing_subscriber::EnvFilter;

#[tokio::main]
async fn main() -> ExitCode {
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")),
        )
        .init();
    if let Err(error) = serve().await {
        tracing::error!(%error, "playground worker stopped");
        return ExitCode::FAILURE;
    }
    ExitCode::SUCCESS
}

#[allow(
    clippy::single_call_fn,
    reason = "worker startup is separate from process exit status and error logging"
)]
async fn serve() -> Result<(), Box<dyn std::error::Error>> {
    let config = WorkerConfig::from_env()?;
    config.check_runtime().await?;
    let listener = tokio::net::TcpListener::bind(config.bind).await?;
    tracing::info!(bind = %listener.local_addr()?, "playground worker ready");
    axum::serve(listener, sandbox::router(config)).await?;
    Ok(())
}
