//! Exercises the binary's device polling against a local HTTP fixture.

#![allow(clippy::expect_used, clippy::unwrap_used)] // Fixture failures fail the current test.

use std::io::BufRead as _;
use std::io::BufReader;
use std::io::Read as _;
use std::io::Write as _;
use std::net::TcpListener;
use std::net::TcpStream;
use std::process::Command;
use std::process::Output;
use std::process::Stdio;
use std::time::Duration;
use std::time::Instant;

use serde_json::Value;
use serde_json::json;

const PUBLIC_KEY: &str = "ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAIFsT4fD2Yrv4QkBrOKpubAd0OzRCinKR4yc9L4yUpuTD aevia-local-device-fixture";
const START: &str = "POST /api/v1/auth/device";
const POLL: &str = "GET /api/v1/auth/device/local-fixture-code";

struct Exchange {
    output: Output,
    requests: Vec<String>,
    first_poll_after: Option<Duration>,
    config: Option<String>,
}

fn request(stream: &mut TcpStream) -> (String, Vec<u8>) {
    let mut reader = BufReader::new(stream);
    let mut line = String::new();
    reader.read_line(&mut line).unwrap();
    let route = line
        .split_whitespace()
        .take(2)
        .collect::<Vec<_>>()
        .join(" ");
    let mut length = 0;
    loop {
        line.clear();
        assert_ne!(reader.read_line(&mut line).unwrap(), 0);
        if line.trim().is_empty() {
            break;
        }
        if let Some((name, value)) = line.split_once(':')
            && name.eq_ignore_ascii_case("content-length")
        {
            length = value.trim().parse().unwrap();
        }
    }
    let mut bytes = vec![0; length];
    reader.read_exact(&mut bytes).unwrap();
    (route, bytes)
}

fn reply(
    stream: &mut TcpStream,
    payload: &Value,
) {
    let body = payload.to_string();
    write!(
        stream,
        "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nContent-Type: application/json\r\nConnection: close\r\n\r\n{body}",
        body.len()
    )
    .unwrap();
}

fn login(
    lifetime_ms: Option<i64>,
    poll_reply: Option<Value>,
) -> Exchange {
    let folder = tempfile::tempdir().unwrap();
    let key = folder.path().join("device.pub");
    std::fs::write(&key, PUBLIC_KEY).unwrap();
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    listener.set_nonblocking(true).unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    let mut child = Command::new(env!("CARGO_BIN_EXE_aevia-pkg"))
        .args([
            "--registry",
            &base,
            "--quiet",
            "login",
            "--label",
            "local fixture",
            "--ssh",
        ])
        .arg(&key)
        .env("AEVIA_PKG_HOME", folder.path())
        .env("NO_PROXY", "127.0.0.1")
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let deadline = Instant::now().checked_add(Duration::from_secs(16)).unwrap();
    let mut requests = Vec::new();
    let mut start_sent = None;
    let mut first_poll_after = None;
    let mut hanging = Vec::new();
    while child.try_wait().unwrap().is_none() {
        if Instant::now() >= deadline {
            child.kill().unwrap();
            child.wait().unwrap();
            panic!("device login exceeded the fixture deadline");
        }
        match listener.accept() {
            | Ok((mut stream, _)) => {
                stream.set_nonblocking(false).unwrap();
                stream
                    .set_read_timeout(Some(Duration::from_secs(2)))
                    .unwrap();
                let (route, body) = request(&mut stream);
                match route.as_str() {
                    | START => {
                        let body: Value = serde_json::from_slice(&body).unwrap();
                        assert_eq!(body["kind"], "ssh");
                        assert_eq!(body["public_key"], PUBLIC_KEY);
                        assert_eq!(body["label"], "local fixture");
                        let expires = lifetime_ms.map_or_else(String::new, |milliseconds| {
                            chrono::Utc::now()
                                .checked_add_signed(chrono::Duration::milliseconds(milliseconds))
                                .unwrap()
                                .to_rfc3339()
                        });
                        reply(
                            &mut stream,
                            &json!({
                                "device_code": "local-fixture-code",
                                "user_code": "ABCD-EFGH",
                                "verification_uri": format!("{base}/approve"),
                                "expires_at": expires,
                                "interval": 1,
                            }),
                        );
                        start_sent = Some(Instant::now());
                    },
                    | POLL => {
                        first_poll_after.get_or_insert_with(|| start_sent.unwrap().elapsed());
                        if let Some(answer) = &poll_reply {
                            reply(&mut stream, answer);
                        } else {
                            hanging.push(stream);
                        }
                    },
                    | _ => panic!("unexpected HTTP route: {route}"),
                }
                requests.push(route);
            },
            | Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                std::thread::sleep(Duration::from_millis(5));
            },
            | Err(error) => panic!("local HTTP fixture failed: {error}"),
        }
    }
    Exchange {
        output: child.wait_with_output().unwrap(),
        requests,
        first_poll_after,
        config: std::fs::read_to_string(folder.path().join("config.toml")).ok(),
    }
}

#[test]
fn expiry_before_first_interval_sends_no_poll() {
    let exchange = login(Some(600), Some(json!({"status": "expired"})));
    assert!(!exchange.output.status.success());
    assert_eq!(exchange.requests, [START]);
    assert!(
        String::from_utf8_lossy(&exchange.output.stderr)
            .contains("the device code expired before it was approved at ")
    );
    assert!(exchange.config.is_none());
}

#[test]
fn expiry_cancels_a_poll_waiting_for_a_response() {
    let exchange = login(Some(7400), None);
    assert!(!exchange.output.status.success());
    assert_eq!(exchange.requests, [START, POLL]);
    assert!(
        String::from_utf8_lossy(&exchange.output.stderr)
            .contains("the device code expired before it was approved at ")
    );
    assert!(exchange.config.is_none());
}

#[test]
fn approval_retains_the_poll_interval_and_writes_configuration() {
    let exchange = login(
        Some(15000),
        Some(json!({
            "status": "approved",
            "publisher": {"handle": "fixture", "kind": "github"}
        })),
    );
    assert!(exchange.output.status.success());
    assert_eq!(exchange.requests, [START, POLL]);
    assert!(exchange.first_poll_after.unwrap() >= Duration::from_millis(4900));
    let stdout = String::from_utf8_lossy(&exchange.output.stdout);
    assert!(stdout.contains("ABCD-EFGH"));
    assert!(stdout.contains("bound "));
    let config = exchange.config.unwrap();
    assert!(config.contains("default_key = \"SHA256:"));
    assert!(config.contains("default_key_kind = \"ssh\""));
}

#[test]
fn a_missing_expiry_can_poll_and_report_denial() {
    let exchange = login(None, Some(json!({"status": "denied"})));
    assert!(!exchange.output.status.success());
    assert_eq!(exchange.requests, [START, POLL]);
    assert!(
        String::from_utf8_lossy(&exchange.output.stderr)
            .contains("the code was denied in the browser")
    );
    assert!(exchange.config.is_none());
}
