//! The book's execution endpoint. The registry sends code to a separately deployed worker.

use std::sync::Arc;

use axum::Json;
use axum::Router;
use axum::extract::DefaultBodyLimit;
use axum::extract::State;
use axum::http::StatusCode;
use axum::routing::post;
use serde::Deserialize;
use serde::Serialize;
use tokio::sync::Semaphore;

use crate::config::ConfigError;
use crate::config::var;
use crate::error::ApiError;
use crate::error::ApiResult;
use crate::state::AppState;

const MAX_SOURCE: usize = 16 * 1024;
pub(crate) const MAX_JSON: usize = MAX_SOURCE * 6 + 128;
pub(crate) const MAX_OUTPUT: usize = 16 * 1024;
const MAX_RESPONSE: usize = MAX_OUTPUT * 2 * 6 + 128;

/// Where the registry forwards playground requests.
#[derive(Clone)]
pub struct PlaygroundConfig {
    url: url::Url,
    token: String,
    permits: Arc<Semaphore>,
}

impl PlaygroundConfig {
    #[allow(
        clippy::single_call_fn,
        reason = "the playground worker configuration is an independent startup boundary"
    )]
    pub(crate) fn from_env() -> Result<Option<Self>, ConfigError> {
        let endpoint = var("AEVIA_PLAYGROUND_URL");
        let token = var("AEVIA_PLAYGROUND_TOKEN");
        match (endpoint, token) {
            | (None, None) => Ok(None),
            | (Some(endpoint), Some(token)) => Self::new(&endpoint, token).map(Some),
            | _ => Err(ConfigError {
                message:
                    "set both AEVIA_PLAYGROUND_URL and AEVIA_PLAYGROUND_TOKEN to enable execution"
                        .to_string(),
            }),
        }
    }

    #[allow(
        clippy::single_call_fn,
        reason = "configuration constructors centralize their invariants"
    )]
    fn new(
        endpoint: &str,
        token: String,
    ) -> Result<Self, ConfigError> {
        validate_token(&token)?;
        let invalid = || {
            ConfigError {
            message: "AEVIA_PLAYGROUND_URL must name /run over HTTPS (or loopback HTTP), without credentials, query, or fragment".to_string(),
        }
        };
        let url = url::Url::parse(endpoint).map_err(|_| invalid())?;
        let loopback = match url.host() {
            | Some(url::Host::Domain(host)) => host == "localhost",
            | Some(url::Host::Ipv4(ip)) => ip.is_loopback(),
            | Some(url::Host::Ipv6(ip)) => ip.is_loopback(),
            | None => false,
        };
        if !(url.scheme() == "https" || url.scheme() == "http" && loopback)
            || url.path() != "/run"
            || !url.username().is_empty()
            || url.password().is_some()
            || url.query().is_some()
            || url.fragment().is_some()
        {
            return Err(invalid());
        }
        Ok(Self {
            url,
            token,
            permits: Arc::new(Semaphore::new(2)),
        })
    }
}

pub(crate) fn validate_token(token: &str) -> Result<(), ConfigError> {
    if token.len() == 64 && token.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        Ok(())
    } else {
        Err(ConfigError {
            message:
                "AEVIA_PLAYGROUND_TOKEN must be a 32-byte key generated with openssl rand -hex 32"
                    .to_string(),
        })
    }
}

#[derive(Deserialize, Serialize)]
pub(crate) struct Program {
    pub source: String,
}

impl Program {
    pub(crate) fn validate(&self) -> ApiResult<()> {
        if self.source.len() > MAX_SOURCE {
            return Err(ApiError::new(
                StatusCode::PAYLOAD_TOO_LARGE,
                "E_PLAYGROUND_SOURCE",
                "Aevia source must be at most 16 KiB",
            ));
        }
        if self.source.trim().is_empty() || self.source.contains('\0') {
            return Err(ApiError::bad_request(
                "E_PLAYGROUND_SOURCE",
                "The program must contain Aevia source text",
            ));
        }
        Ok(())
    }
}

#[derive(Clone, Copy, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum RunStatus {
    Success,
    Failed,
    Timeout,
    OutputLimit,
}

#[derive(Deserialize, Serialize)]
pub(crate) struct RunResult {
    pub status: RunStatus,
    pub stdout: String,
    pub stderr: String,
}

#[allow(
    clippy::single_call_fn,
    reason = "the playground router applies its own request-size limit"
)]
pub(crate) fn router() -> Router<AppState> {
    Router::new()
        .route("/api/v1/playground/run", post(run))
        .layer(DefaultBodyLimit::max(MAX_JSON))
}

#[allow(
    clippy::single_call_fn,
    reason = "the HTTP run endpoint owns request limits and response mapping"
)]
async fn run(
    State(state): State<AppState>,
    Json(program): Json<Program>,
) -> ApiResult<Json<RunResult>> {
    let config = state.config.playground.as_ref().ok_or_else(unavailable)?;
    program.validate()?;
    let _permit = config.permits.try_acquire().map_err(|_| busy())?;
    let mut response = state
        .http
        .post(config.url.clone())
        .bearer_auth(&config.token)
        .json(&program)
        .send()
        .await
        .map_err(|error| worker_error(&error))?;
    if response.status() == StatusCode::TOO_MANY_REQUESTS {
        return Err(busy());
    }
    if response.status() != StatusCode::OK {
        tracing::warn!(status = %response.status(), "playground worker failed");
        return Err(unavailable());
    }
    let mut body = Vec::new();
    while let Some(chunk) = response
        .chunk()
        .await
        .map_err(|error| worker_error(&error))?
    {
        if chunk.len() > MAX_RESPONSE.saturating_sub(body.len()) {
            return Err(unavailable());
        }
        body.extend_from_slice(&chunk);
    }
    let result = serde_json::from_slice(&body).map_err(|error| worker_error(&error))?;
    Ok(Json(result))
}

pub(crate) fn busy() -> ApiError {
    ApiError::new(
        StatusCode::TOO_MANY_REQUESTS,
        "E_PLAYGROUND_BUSY",
        "The Aevia runner is busy",
    )
}

pub(crate) fn unavailable() -> ApiError {
    ApiError::new(
        StatusCode::SERVICE_UNAVAILABLE,
        "E_PLAYGROUND_UNAVAILABLE",
        "The Aevia runner is unavailable",
    )
}

fn worker_error(error: &dyn std::fmt::Display) -> ApiError {
    tracing::warn!(%error, "playground worker request failed");
    unavailable()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plaintext_worker_connections_are_local_only() {
        let token = "f".repeat(64);
        for endpoint in [
            "http://127.0.0.1:9081/run",
            "https://worker.example.com/run",
        ] {
            assert!(PlaygroundConfig::new(endpoint, token.clone()).is_ok());
        }
        for endpoint in [
            "http://worker.example.com/run",
            "https://worker.example.com/run?secret",
        ] {
            assert!(PlaygroundConfig::new(endpoint, token.clone()).is_err());
        }
    }
}
