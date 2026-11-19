//! The separate playground worker. The registry process never launches containers.

use std::io;
use std::net::SocketAddr;
use std::process::Stdio;
use std::sync::Arc;
use std::time::Duration;

use axum::Json;
use axum::Router;
use axum::extract::DefaultBodyLimit;
use axum::extract::State;
use axum::http::HeaderMap;
use axum::http::StatusCode;
use axum::http::header;
use axum::routing::post;
use subtle::ConstantTimeEq as _;
use tokio::io::AsyncRead;
use tokio::io::AsyncReadExt as _;
use tokio::io::AsyncWriteExt as _;
use tokio::process::Command;
use tokio::sync::Semaphore;

use crate::config::ConfigError;
use crate::config::required;
use crate::config::var;
use crate::error::ApiError;
use crate::error::ApiResult;
use crate::playground::MAX_JSON;
use crate::playground::MAX_OUTPUT;
use crate::playground::Program;
use crate::playground::RunResult;
use crate::playground::RunStatus;
use crate::playground::busy;
use crate::playground::unavailable;
use crate::playground::validate_token;

const RUN_TIMEOUT: Duration = Duration::from_secs(7);
const CLEANUP_TIMEOUT: Duration = Duration::from_secs(2);
const CONTAINER_TIMEOUT: &str = "10";
const PROGRAM_ENTRY: &str = "/bin/cat > main.aev && exec /usr/local/bin/aevia --max-stack 16 --budget 100000 --jobs 1 --color never --caps= run main.aev </dev/null";

/// Configuration for the dedicated, rootless Podman worker.
pub struct WorkerConfig {
    /// Private HTTP socket reached by the registry or its private TLS proxy.
    pub bind: SocketAddr,
    image: String,
    token: String,
}

impl WorkerConfig {
    /// Reads the bind address, preinstalled image and bearer key.
    ///
    /// # Errors
    ///
    /// Returns a configuration error for a missing or malformed setting.
    pub fn from_env() -> Result<Self, ConfigError> {
        let bind = var("AEVIA_PLAYGROUND_BIND")
            .as_deref()
            .unwrap_or("127.0.0.1:9081")
            .parse::<SocketAddr>()
            .map_err(|_| {
                ConfigError {
                    message: "AEVIA_PLAYGROUND_BIND must be a socket address".to_string(),
                }
            })?;
        if !bind.ip().is_loopback() {
            return Err(ConfigError {
                message: "AEVIA_PLAYGROUND_BIND must be loopback; use a private TLS proxy for remote access".to_string(),
            });
        }
        let image = required("AEVIA_PLAYGROUND_IMAGE")?;
        if !image.starts_with(|character: char| character.is_ascii_alphanumeric())
            || image.contains(char::is_whitespace)
        {
            return Err(ConfigError {
                message: "AEVIA_PLAYGROUND_IMAGE must name the local runtime image".to_string(),
            });
        }
        let token = required("AEVIA_PLAYGROUND_TOKEN")?;
        validate_token(&token)?;
        Ok(Self { bind, image, token })
    }

    /// Checks the worker host before accepting requests.
    ///
    /// # Errors
    ///
    /// Returns an error when local rootless Podman with cgroup v2 or the
    /// preinstalled image is unavailable.
    pub async fn check_runtime(&self) -> io::Result<()> {
        let info = tokio::time::timeout(
            Duration::from_secs(20),
            podman()
                .args([
                    "info",
                    "--format",
                    "{{.Host.Security.Rootless}} {{.Host.CgroupsVersion}}",
                ])
                .output(),
        )
        .await??;
        let host = String::from_utf8_lossy(&info.stdout);
        if !info.status.success() || host.trim() != "true v2" {
            return Err(io::Error::other(
                "the Aevia worker requires local rootless Podman with cgroup v2",
            ));
        }
        let image = tokio::time::timeout(
            Duration::from_secs(10),
            podman().args(["image", "exists", &self.image]).status(),
        )
        .await??;
        if !image.success() {
            return Err(io::Error::other(
                "the configured Aevia runtime image is not installed",
            ));
        }
        Ok(())
    }
}

struct Worker {
    config: WorkerConfig,
    slots: Arc<Semaphore>,
}

/// Routes served by the worker on its private listener.
pub fn router(config: WorkerConfig) -> Router {
    Router::new()
        .route("/run", post(execute))
        .layer(DefaultBodyLimit::max(MAX_JSON))
        .with_state(Arc::new(Worker {
            config,
            slots: Arc::new(Semaphore::new(2)),
        }))
}

#[allow(
    clippy::single_call_fn,
    reason = "one worker request owns the permit, timeout and container lifecycle"
)]
async fn execute(
    State(worker): State<Arc<Worker>>,
    headers: HeaderMap,
    Json(program): Json<Program>,
) -> ApiResult<Json<RunResult>> {
    let authorised = headers
        .get(header::AUTHORIZATION)
        .and_then(|auth| auth.to_str().ok())
        .and_then(|auth| auth.strip_prefix("Bearer "))
        .is_some_and(|auth| bool::from(auth.as_bytes().ct_eq(worker.config.token.as_bytes())));
    if !authorised {
        return Err(ApiError::new(
            StatusCode::UNAUTHORIZED,
            "E_PLAYGROUND_AUTH",
            "Worker authentication failed",
        ));
    }
    program.validate()?;
    let slot = worker
        .slots
        .clone()
        .try_acquire_owned()
        .map_err(|_| busy())?;
    // Let cleanup finish even when the upstream HTTP request is dropped.
    let result = tokio::spawn(async move {
        let _slot = slot;
        run(&worker.config.image, program).await
    })
    .await
    .map_err(|error| {
        tracing::error!(%error, "playground task failed");
        unavailable()
    })?;
    Ok(Json(result?))
}

fn podman() -> Command {
    let mut command = Command::new("podman");
    command.arg("--remote=false").kill_on_drop(true);
    command
}

#[allow(
    clippy::single_call_fn,
    reason = "the Podman invocation is independently testable as a sandbox policy"
)]
fn launch(
    image: &str,
    name: &str,
) -> Command {
    let mut command = podman();
    command
        .args([
            "run",
            "--interactive",
            "--rm",
            "--name",
            name,
            "--timeout",
            CONTAINER_TIMEOUT,
            "--pull=never",
            "--network=none",
            "--ipc=private",
            "--shm-size=8m",
            "--userns=auto:size=4096",
            "--user=1000:1000",
            "--cap-drop=ALL",
            "--security-opt=no-new-privileges",
            "--read-only",
            "--read-only-tmpfs=false",
            "--tmpfs=/work:rw,nodev,nosuid,noexec,size=4m,mode=1777",
            "--workdir=/work",
            "--memory=256m",
            "--memory-swap=256m",
            "--cpus=1",
            "--pids-limit=32",
            "--ulimit=nofile=128:128",
            "--ulimit=core=0:0",
            "--log-driver=none",
            "--unsetenv-all",
            "--env=HOME=/work",
            "--entrypoint=/bin/sh",
            "--",
            image,
            "-c",
            PROGRAM_ENTRY,
        ])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    command
}

#[allow(
    clippy::single_call_fn,
    reason = "process supervision keeps the child and all output readers under the same deadline"
)]
async fn run(
    image: &str,
    program: Program,
) -> ApiResult<RunResult> {
    let name = format!("aevia-playground-{}", crate::http::random_hex(16));
    let mut child = launch(image, &name)
        .spawn()
        .map_err(|error| worker_error(&error))?;
    let mut input = child.stdin.take().ok_or_else(unavailable)?;
    let mut output = child.stdout.take().ok_or_else(unavailable)?;
    let mut diagnostics = child.stderr.take().ok_or_else(unavailable)?;
    let mut stdout = Vec::new();
    let mut stderr = Vec::new();
    let result = tokio::time::timeout(RUN_TIMEOUT, async {
        tokio::try_join!(
            async move {
                if let Err(error) = input.write_all(program.source.as_bytes()).await
                    && error.kind() != io::ErrorKind::BrokenPipe
                {
                    return Err(ProcessError::Io(error));
                }
                Ok(())
            },
            read_capped(&mut output, &mut stdout),
            read_capped(&mut diagnostics, &mut stderr),
            async { child.wait().await.map_err(ProcessError::Io) },
        )
        .map(|((), (), (), status)| status)
    })
    .await;
    if matches!(&result, Ok(Err(_)) | Err(_)) {
        drop(child);
        remove_container(&name).await;
    }
    let status = match result {
        | Ok(Ok(status)) if status.success() => RunStatus::Success,
        | Ok(Ok(status)) if matches!(status.code(), Some(125..=127)) => {
            remove_container(&name).await;
            tracing::warn!(?status, "Podman could not start the Aevia runtime");
            return Err(unavailable());
        },
        | Ok(Ok(_)) => RunStatus::Failed,
        | Ok(Err(ProcessError::Limit)) => RunStatus::OutputLimit,
        | Ok(Err(ProcessError::Io(error))) => return Err(worker_error(&error)),
        | Err(_) => RunStatus::Timeout,
    };
    Ok(RunResult {
        status,
        stdout: String::from_utf8_lossy(&stdout).into_owned(),
        stderr: String::from_utf8_lossy(&stderr).into_owned(),
    })
}

fn worker_error(error: &io::Error) -> ApiError {
    tracing::warn!(%error, "could not execute the Aevia runtime");
    unavailable()
}

async fn remove_container(name: &str) {
    let result = tokio::time::timeout(
        CLEANUP_TIMEOUT,
        podman()
            .args(["rm", "--ignore", "--force", "--time", "0", name])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status(),
    )
    .await;
    if !matches!(result, Ok(Ok(status)) if status.success()) {
        tracing::warn!(
            container = name,
            "could not remove the playground container"
        );
    }
}

enum ProcessError {
    Io(io::Error),
    Limit,
}

async fn read_capped(
    pipe: &mut (impl AsyncRead + Unpin),
    output: &mut Vec<u8>,
) -> Result<(), ProcessError> {
    let mut buffer = [0; 4096];
    loop {
        let count = pipe.read(&mut buffer).await.map_err(ProcessError::Io)?;
        if count == 0 {
            return Ok(());
        }
        let remaining = MAX_OUTPUT.saturating_sub(output.len());
        let chunk = buffer.get(..count.min(remaining)).ok_or_else(|| {
            ProcessError::Io(io::Error::new(
                io::ErrorKind::InvalidData,
                "invalid pipe read length",
            ))
        })?;
        output.extend_from_slice(chunk);
        if count > remaining {
            return Err(ProcessError::Limit);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn output_limit_does_not_require_waiting_for_eof() {
        let data = vec![b'x'; MAX_OUTPUT + 1];
        let mut output = Vec::new();
        assert!(matches!(
            read_capped(&mut data.as_slice(), &mut output).await,
            Err(ProcessError::Limit)
        ));
        assert_eq!(output.len(), MAX_OUTPUT);
    }
}
