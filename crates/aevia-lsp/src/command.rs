//! Background execution for the editor's run and test code lenses.

use serde_json::Value;
use serde_json::json;
use std::io;
use std::io::Read;
use std::path::Path;
use std::path::PathBuf;
use std::process::Child;
use std::process::Command;
use std::process::ExitStatus;
use std::process::Stdio;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use std::sync::atomic::Ordering;
use std::sync::mpsc::Sender;
use std::thread;
use std::thread::JoinHandle;
use std::time::Duration;
use std::time::Instant;

const TIMEOUT: Duration = Duration::from_secs(120);
const POLL_INTERVAL: Duration = Duration::from_millis(20);
const OUTPUT_LIMIT: usize = 256 * 1024;

pub const MAX_CONCURRENT: usize = 4;
const REQUEST_CANCELLED: i64 = -32800;
pub const REQUEST_FAILED: i64 = -32803;

pub struct Job {
    pub(crate) label: &'static str,
    pub(crate) process: Command,
}

pub struct Task {
    pub(crate) id: Value,
    cancelled: Arc<AtomicBool>,
    worker: JoinHandle<()>,
}

impl Task {
    #[cfg_attr(
        not(test),
        expect(
            clippy::single_call_fn,
            reason = "own the execution worker and its cancellation state"
        )
    )]
    pub(crate) fn spawn(
        out: Sender<Value>,
        id: Value,
        job: Job,
    ) -> io::Result<Self> {
        let cancelled = Arc::new(AtomicBool::new(false));
        let worker_id = id.clone();
        let flag = cancelled.clone();
        let worker = thread::Builder::new()
            .name("aevia-command".into())
            .spawn(move || {
                let result = run(job.process, &flag, TIMEOUT);
                let message = if flag.load(Ordering::Relaxed) {
                    failure(
                        &worker_id,
                        REQUEST_CANCELLED,
                        "the Aevia command was cancelled",
                    )
                } else {
                    match result {
                        | Ok(result) => {
                            publish_output(&out, job.label, &result);
                            json!({ "jsonrpc": "2.0", "id": worker_id, "result": result.to_json() })
                        },
                        | Err(error) => {
                            let detail =
                                format!("Aevia {} could not start or finish: {error}", job.label);
                            notify(&out, "window/showMessage", 1, &detail);
                            failure(&worker_id, REQUEST_FAILED, &detail)
                        },
                    }
                };
                let _ = out.send(message);
            })?;
        Ok(Self {
            id,
            cancelled,
            worker,
        })
    }

    pub(crate) fn finished(&self) -> bool {
        self.worker.is_finished()
    }

    pub(crate) fn cancel(&self) {
        self.cancelled.store(true, Ordering::Relaxed);
    }

    pub(crate) fn join(self) {
        let _ = self.worker.join();
    }
}

fn failure(
    id: &Value,
    code: i64,
    message: &str,
) -> Value {
    json!({ "jsonrpc": "2.0", "id": id, "error": { "code": code, "message": message } })
}

#[derive(Default)]
struct Captured {
    bytes: Vec<u8>,
    truncated: bool,
}

impl Captured {
    fn text(&self) -> std::borrow::Cow<'_, str> {
        String::from_utf8_lossy(&self.bytes)
    }
}

struct Outcome {
    status: ExitStatus,
    timed_out: bool,
    stdout: Captured,
    stderr: Captured,
}

impl Outcome {
    fn success(&self) -> bool {
        self.status.success() && !self.timed_out
    }

    fn to_json(&self) -> Value {
        json!({
            "success": self.success(),
            "exitCode": self.status.code(),
            "timedOut": self.timed_out,
            "stdout": self.stdout.text(),
            "stderr": self.stderr.text(),
            "stdoutTruncated": self.stdout.truncated,
            "stderrTruncated": self.stderr.truncated,
        })
    }
}

fn notify(
    out: &Sender<Value>,
    method: &str,
    kind: u8,
    message: &str,
) {
    let _ = out.send(json!({
        "jsonrpc": "2.0", "method": method, "params": { "type": kind, "message": message },
    }));
}

#[cfg_attr(
    not(test),
    expect(
        clippy::single_call_fn,
        reason = "keep editor notifications separate from process execution"
    )
)]
fn publish_output(
    out: &Sender<Value>,
    label: &str,
    result: &Outcome,
) {
    // Many clients discard the executeCommand result. Keep the output in their
    // language server log and surface the outcome in the editor.
    for (stream, output) in [("stdout", &result.stdout), ("stderr", &result.stderr)] {
        if !output.bytes.is_empty() || output.truncated {
            let suffix = if output.truncated {
                "\n[output truncated]"
            } else {
                ""
            };
            notify(
                out,
                "window/logMessage",
                4,
                &format!("Aevia {label} ({stream})\n{}{suffix}", output.text()),
            );
        }
    }
    let (kind, summary) = if result.timed_out {
        (2, "timed out")
    } else if result.success() {
        (3, "finished")
    } else {
        (1, "failed")
    };
    notify(
        out,
        "window/showMessage",
        kind,
        &format!("Aevia {label} {summary}. See the language server log for output."),
    );
}

/// Retains only a prefix while reading to EOF. The child can continue writing
/// after the cutoff without blocking on a full pipe.
fn capture(mut pipe: impl Read + Send + 'static) -> io::Result<JoinHandle<Captured>> {
    thread::Builder::new().spawn(move || {
        let mut output = Captured::default();
        let mut chunk = [0; 8192];
        loop {
            match pipe.read(&mut chunk) {
                | Ok(0) => break,
                | Ok(read) => {
                    let retained = read.min(OUTPUT_LIMIT.saturating_sub(output.bytes.len()));
                    let Some(prefix) = chunk.get(..retained) else {
                        output.truncated = true;
                        break;
                    };
                    output.bytes.extend_from_slice(prefix);
                    output.truncated |= retained < read;
                },
                | Err(error) if error.kind() == io::ErrorKind::Interrupted => {},
                | Err(_) => {
                    output.truncated = true;
                    break;
                },
            }
        }
        output
    })
}

struct RunningChild(Child);

impl Drop for RunningChild {
    fn drop(&mut self) {
        if self.0.try_wait().ok().flatten().is_none() {
            let _ = self.0.kill();
            let _ = self.0.wait();
        }
    }
}

#[cfg_attr(
    not(test),
    expect(
        clippy::single_call_fn,
        reason = "separate process lifetime from request handling"
    )
)]
fn run(
    mut process: Command,
    cancelled: &AtomicBool,
    timeout: Duration,
) -> io::Result<Outcome> {
    if cancelled.load(Ordering::Relaxed) {
        return Err(io::ErrorKind::Interrupted.into());
    }
    let mut child = RunningChild(
        process
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()?,
    );
    let stdout = capture(
        child
            .0
            .stdout
            .take()
            .ok_or_else(|| io::Error::other("no stdout pipe"))?,
    )?;
    let stderr = capture(
        child
            .0
            .stderr
            .take()
            .ok_or_else(|| io::Error::other("no stderr pipe"))?,
    )?;
    let start = Instant::now();
    let (status, timed_out) = loop {
        if cancelled.load(Ordering::Relaxed) {
            return Err(io::ErrorKind::Interrupted.into());
        }
        let status = child.0.try_wait()?;
        if let Some(status) = status
            && stdout.is_finished()
            && stderr.is_finished()
        {
            break (status, false);
        }
        if start.elapsed() >= timeout {
            let status = if let Some(status) = status {
                status
            } else {
                let _ = child.0.kill();
                child.0.wait()?
            };
            break (status, true);
        }
        thread::sleep(POLL_INTERVAL);
    };
    let collected = |pipe: JoinHandle<Captured>| {
        // A redirected pipe can still be held open by another process. Only
        // collect completed readers after a timeout so shutdown stays bounded.
        if pipe.is_finished() {
            pipe.join().unwrap_or_default()
        } else {
            Captured {
                bytes: Vec::new(),
                truncated: true,
            }
        }
    };
    Ok(Outcome {
        status,
        timed_out,
        stdout: collected(stdout),
        stderr: collected(stderr),
    })
}

/// Uses the same directory as an interactive invocation at a package root.
#[cfg_attr(
    not(test),
    expect(
        clippy::single_call_fn,
        reason = "isolate package-root discovery from request validation"
    )
)]
pub fn working_directory(path: &Path) -> &Path {
    let directory = path.parent().unwrap_or(path);
    directory
        .ancestors()
        .find(|ancestor| ancestor.join("aevia.toml").is_file())
        .unwrap_or(directory)
}

/// Locates the CLI when launched as `aevia lsp` or beside that executable.
#[cfg_attr(
    not(test),
    expect(
        clippy::single_call_fn,
        reason = "isolate platform executable discovery"
    )
)]
pub fn executable() -> PathBuf {
    if let Ok(exe) = std::env::current_exe() {
        let stem = exe.file_stem().and_then(|s| s.to_str()).unwrap_or_default();
        if stem.eq_ignore_ascii_case("aevia") {
            return exe;
        }
        let sibling = exe.with_file_name(if cfg!(windows) {
            "aevia.exe"
        } else {
            "aevia"
        });
        if sibling.is_file() {
            return sibling;
        }
    }
    PathBuf::from("aevia")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[ignore = "spawned by the captured program tests"]
    fn sample_program() {
        if std::env::var_os("AEVIA_COMMAND_TEST_WAIT").is_some() {
            thread::sleep(Duration::from_secs(30));
        } else {
            println!("{}", "a".repeat(OUTPUT_LIMIT + 4096));
            eprintln!("stderr stays separate");
        }
    }

    fn sample() -> Command {
        let mut command = Command::new(std::env::current_exe().expect("current test executable"));
        command.args([
            "--ignored",
            "--exact",
            "command::tests::sample_program",
            "--nocapture",
        ]);
        command
    }

    #[test]
    fn captured_output_is_bounded_and_still_drained() {
        let result = run(sample(), &AtomicBool::new(false), Duration::from_secs(15))
            .expect("sample program");
        assert!(result.success());
        assert!(result.stdout.truncated);
        assert_eq!(result.stdout.bytes.len(), OUTPUT_LIMIT);
        assert!(result.stderr.text().contains("stderr stays separate"));
    }

    #[test]
    fn cancellation_releases_a_running_task() {
        let (out, responses) = std::sync::mpsc::channel();
        let mut process = sample();
        process.env("AEVIA_COMMAND_TEST_WAIT", "1");
        let task = Task::spawn(
            out,
            json!(42),
            Job {
                label: "test",
                process,
            },
        )
        .expect("start");
        thread::sleep(Duration::from_millis(100));
        task.cancel();
        let reply = responses
            .recv_timeout(Duration::from_secs(5))
            .expect("cancelled reply");
        assert_eq!(reply["id"], 42);
        assert_eq!(reply["error"]["code"], REQUEST_CANCELLED);
        task.join();
    }
}
