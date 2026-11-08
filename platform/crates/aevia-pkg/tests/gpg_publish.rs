//! End-to-end tests that run the built binary against a real `gpg`.
//!
//! # Why these are integration tests
//!
//! The publish path's whole value is that it shells out to the publisher's own
//! key. Mocking that away would test the mock, not the tool, so these tests
//! drive the binary as a subprocess with a throwaway `GNUPGHOME` — a real key,
//! a real detached signature, a real verification.
//!
//! Every test **detects `gpg` and skips** when it is not installed: a machine
//! without GnuPG cannot exercise the OpenPGP path, and failing there would
//! report a missing tool as a broken client.
//!
//! The full publish test is `#[ignore]`d by default. It needs to generate a
//! key, run `gpg` several times and serve three HTTP requests, which is slower
//! and more environment-dependent than a unit test should be. Run it with
//! `cargo test -p aevia-pkg -- --ignored`.

use std::io::Read;
use std::io::Write;
use std::net::TcpListener;
use std::net::TcpStream;
use std::path::Path;
use std::path::PathBuf;
use std::process::Command;

/// A scratch directory that removes itself.
struct Scratch {
    root: PathBuf,
}

impl Scratch {
    fn new(name: &str) -> Option<Self> {
        let root = std::env::temp_dir().join(format!(
            "aevia-pkg-it-{name}-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).ok()?;
        Some(Self { root })
    }

    fn join(
        &self,
        path: &str,
    ) -> PathBuf {
        self.root.join(path)
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

/// Whether `gpg` can be run at all.
fn gpg_available() -> bool {
    Command::new("gpg")
        .arg("--version")
        .output()
        .is_ok_and(|output| output.status.success())
}

/// Creates a passphrase-less ed25519 key and returns the `GNUPGHOME` spelling
/// that this machine's `gpg` accepts.
///
/// Two `gpg` builds commonly sit on a Windows PATH: the native one, which wants
/// `C:\Users\…`, and the MSYS one, which wants `/c/Users/…`. An environment
/// variable is not translated between them, so the test tries both spellings
/// and uses whichever produces a key. `%no-protection` is what keeps the test
/// non-interactive: a key with a passphrase would need a pinentry dialog.
fn gpg_home(home: &Path) -> Option<String> {
    let mut candidates = vec![home.to_string_lossy().to_string()];
    if let Some(posix) = posix_path(home) {
        candidates.push(posix);
    }
    for candidate in candidates {
        if generate_key(&candidate) {
            return Some(candidate);
        }
    }
    eprintln!("skipping: gpg could not create a key in any path spelling");
    None
}

/// The fingerprint of the first secret key in `home`, read the way the client
/// reads it: the `fpr` record that follows a `sec` record.
fn fingerprint(home: &str) -> Option<String> {
    let output = Command::new("gpg")
        .env("GNUPGHOME", home)
        .args(["--batch", "--with-colons", "--list-secret-keys"])
        .output()
        .ok()?;
    let text = String::from_utf8_lossy(&output.stdout).to_string();
    let mut pending = false;
    for line in text.lines() {
        let fields: Vec<&str> = line.split(':').collect();
        match fields.first().copied().unwrap_or_default() {
            | "sec" => pending = true,
            | "fpr" if pending => return fields.get(9).map(|value| (*value).to_string()),
            | _ => {},
        }
    }
    None
}

/// `/c/Users/…` for `C:\Users\…`, the form MSYS builds of `gpg` expect.
fn posix_path(path: &Path) -> Option<String> {
    let text = path.to_string_lossy().replace('\\', "/");
    let bytes = text.as_bytes();
    if bytes.len() > 2 && bytes[1] == b':' {
        let drive = text.get(..1)?.to_ascii_lowercase();
        return Some(format!("/{drive}{}", text.get(2..)?));
    }
    None
}

/// Tries to create a key in the given `GNUPGHOME` spelling.
fn generate_key(home: &str) -> bool {
    let directory = PathBuf::from(home);
    if std::fs::create_dir_all(&directory).is_err() {
        return false;
    }
    let batch = "%no-protection\n\
                 Key-Type: eddsa\n\
                 Key-Curve: ed25519\n\
                 Key-Usage: sign\n\
                 Name-Real: Aevia Test\n\
                 Name-Email: test@example.test\n\
                 Expire-Date: 1d\n\
                 %commit\n";
    // The parameter block goes on stdin: a file *path* is the one thing the
    // native and MSYS builds of `gpg` disagree about, and stdin has no path.
    let Ok(mut child) = Command::new("gpg")
        .env("GNUPGHOME", home)
        .arg("--batch")
        .arg("--gen-key")
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::piped())
        .spawn()
    else {
        return false;
    };
    if let Some(mut stdin) = child.stdin.take() {
        let _ = stdin.write_all(batch.as_bytes());
    }
    match child.wait_with_output() {
        | Ok(output) => output.status.success(),
        | Err(_) => false,
    }
}

/// Writes a minimal package and returns its directory.
fn write_package(root: &Path) {
    let _ = std::fs::create_dir_all(root.join("src"));
    let _ = std::fs::write(
        root.join("aevia.toml"),
        "[package]\nname = \"units-extra\"\nversion = \"0.2.0\"\nentry = \"src/main.aev\"\n",
    );
    let _ = std::fs::write(
        root.join("src/main.aev"),
        "## Units.\nfn main(): Unit = ()\n",
    );
}

/// The built binary.
fn binary() -> &'static str {
    env!("CARGO_BIN_EXE_aevia-pkg")
}

#[test]
fn key_list_reads_a_real_gpg_home() {
    if !gpg_available() {
        eprintln!("skipping: gpg is not installed");
        return;
    }
    let Some(scratch) = Scratch::new("keylist") else {
        eprintln!("skipping: no scratch directory");
        return;
    };
    let home = scratch.join("gnupg");
    let Some(home) = gpg_home(&home) else {
        eprintln!("skipping: gpg could not create a key");
        return;
    };

    let output = Command::new(binary())
        .arg("key")
        .arg("list")
        .env("GNUPGHOME", &home)
        .env("AEVIA_PKG_HOME", scratch.join("cfg"))
        .output()
        .expect("the binary runs");
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(output.status.success(), "{stdout}");
    assert!(stdout.contains("openpgp"), "{stdout}");
    assert!(
        stdout.contains("Aevia Test"),
        "the user id is read: {stdout}"
    );
    // A fingerprint is 40 hexadecimal characters, printed without spaces.
    assert!(
        stdout
            .lines()
            .any(|line| line.starts_with("openpgp  ") && line.len() > 48),
        "{stdout}"
    );
}

#[test]
#[ignore = "needs gpg and a stub registry; run with --ignored"]
fn a_signed_publish_reaches_the_registry() {
    if !gpg_available() {
        eprintln!("skipping: gpg is not installed");
        return;
    }
    let Some(scratch) = Scratch::new("publish") else {
        eprintln!("skipping: no scratch directory");
        return;
    };
    let home = scratch.join("gnupg");
    let Some(home) = gpg_home(&home) else {
        eprintln!("skipping: gpg could not create a key");
        return;
    };
    let package = scratch.join("pkg");
    write_package(&package);

    // Two keys on one machine is the normal case (a gpg key and an ssh key),
    // and the client refuses to guess between them, so the test names one.
    let Some(fingerprint) = fingerprint(&home) else {
        eprintln!("skipping: gpg lists no secret key");
        return;
    };

    let stub = Stub::start();

    // A dry run shows the statement and the checksum, signs nothing and
    // uploads nothing — the point of the flag is that it is safe to run on a
    // release branch.
    let dry = Command::new(binary())
        .arg("--registry")
        .arg(&stub.base)
        .arg("publish")
        .arg("--key")
        .arg(&fingerprint)
        .arg("--allow-dirty")
        .arg("--dry-run")
        .current_dir(&package)
        .env("GNUPGHOME", &home)
        .env("AEVIA_PKG_HOME", scratch.join("cfg"))
        .output()
        .expect("the binary runs");
    let dry_stdout = String::from_utf8_lossy(&dry.stdout).to_string();
    assert!(dry.status.success(), "{dry_stdout}");
    assert!(dry_stdout.contains("aevia-publish-v1"), "{dry_stdout}");
    assert!(dry_stdout.contains("dry run"), "{dry_stdout}");
    assert!(
        !stub.saw("/api/v1/publish"),
        "a dry run uploaded the archive"
    );

    let output = Command::new(binary())
        .arg("--registry")
        .arg(&stub.base)
        .arg("publish")
        .arg("--key")
        .arg(&fingerprint)
        .arg("--allow-dirty")
        .arg("--description")
        .arg("Units for the test")
        .current_dir(&package)
        .env("GNUPGHOME", &home)
        .env("AEVIA_PKG_HOME", scratch.join("cfg"))
        .output()
        .expect("the binary runs");
    let stdout = String::from_utf8_lossy(&output.stdout).to_string();
    let stderr = String::from_utf8_lossy(&output.stderr).to_string();
    assert!(
        output.status.success(),
        "publish failed\nstdout: {stdout}\nstderr: {stderr}"
    );
    assert!(
        stdout.contains("published apich/units-extra 0.2.0"),
        "{stdout}"
    );
    assert!(
        stub.saw("/api/v1/publish/challenge"),
        "no challenge was requested"
    );
    assert!(stub.saw("/api/v1/publish"), "nothing was uploaded");

    // The signature the client sent must verify, under the key in the
    // throwaway home, over exactly the statement it was given.
    let upload = stub.body("/api/v1/publish").expect("the upload body");
    let statement = json_string(&upload, "statement").expect("the envelope holds a statement");
    let signature = json_string(&upload, "signature").expect("the envelope holds a signature");
    assert!(statement.starts_with("aevia-publish-v1\n"), "{statement}");
    assert!(
        statement.contains("name: apich/units-extra\n"),
        "{statement}"
    );
    assert!(statement.contains("version: 0.2.0\n"), "{statement}");

    let signed = scratch.join("statement.txt");
    let armour = scratch.join("signature.asc");
    std::fs::write(&signed, &statement).expect("write the statement");
    std::fs::write(&armour, &signature).expect("write the signature");
    let verify = Command::new("gpg")
        .env("GNUPGHOME", &home)
        .arg("--batch")
        .arg("--verify")
        .arg(&armour)
        .arg(&signed)
        .output()
        .expect("gpg runs");
    assert!(
        verify.status.success(),
        "gpg refused the uploaded signature: {}",
        String::from_utf8_lossy(&verify.stderr)
    );
}

/// A registry stub: three routes, canned answers, one request per connection.
struct Stub {
    base: String,
    log: std::sync::Arc<std::sync::Mutex<Vec<(String, String)>>>,
}

impl Stub {
    fn start() -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").expect("a port");
        let base = format!("http://{}", listener.local_addr().expect("an address"));
        let log = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
        let recorder = std::sync::Arc::clone(&log);
        std::thread::spawn(move || {
            for stream in listener.incoming() {
                let Ok(mut stream) = stream else {
                    break;
                };
                let Some(request) = read_request(&mut stream) else {
                    continue;
                };
                let (path, body) = request;
                let (status, answer) = respond(&path, &body);
                if let Ok(mut guard) = recorder.lock() {
                    guard.push((path, body));
                }
                let _ = write!(
                    stream,
                    "HTTP/1.1 {status}\r\nContent-Type: application/json\r\n\
                     Content-Length: {}\r\nConnection: close\r\n\r\n{answer}",
                    answer.len()
                );
                let _ = stream.flush();
            }
        });
        Self { base, log }
    }

    fn saw(
        &self,
        path: &str,
    ) -> bool {
        self.log
            .lock()
            .map(|guard| guard.iter().any(|(seen, _)| seen == path))
            .unwrap_or(false)
    }

    fn body(
        &self,
        path: &str,
    ) -> Option<String> {
        self.log.lock().ok().and_then(|guard| {
            guard
                .iter()
                .find(|(seen, _)| seen == path)
                .map(|(_, body)| body.clone())
        })
    }
}

/// The answers the client expects, keyed by path.
fn respond(
    path: &str,
    body: &str,
) -> (&'static str, String) {
    if path.ends_with("/publish/challenge") {
        let name = json_string(body, "name").unwrap_or_default();
        let version = json_string(body, "version").unwrap_or_default();
        let checksum = json_string(body, "checksum").unwrap_or_default();
        let expires = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|elapsed| elapsed.as_secs())
            .unwrap_or(0)
            + 300;
        let statement = format!(
            "aevia-publish-v1\nname: {name}\nversion: {version}\nchecksum: {checksum}\n\
             nonce: 0123456789abcdef0123456789abcdef\nexpires: {expires}\n"
        );
        let escaped = statement.replace('\n', "\\n");
        return (
            "200 OK",
            format!(
                "{{\"nonce\":\"0123456789abcdef0123456789abcdef\",\
                 \"statement\":\"{escaped}\",\"expires_at\":\"2030-01-01T00:00:00Z\"}}"
            ),
        );
    }
    if path.ends_with("/publish") {
        let checksum = json_string(body, "checksum").unwrap_or_default();
        return (
            "201 Created",
            format!(
                "{{\"name\":\"apich/units-extra\",\"version\":\"0.2.0\",\
                 \"checksum\":\"{checksum}\",\
                 \"docs_url\":\"/docs/apich/units-extra/0.2.0\",\
                 \"log\":{{\"seq\":1,\"hash\":\"ab\"}}}}"
            ),
        );
    }
    if path.starts_with("/api/v1/keys/") {
        return (
            "200 OK",
            "{\"registered\":true,\"publisher\":{\"handle\":\"apich\",\"kind\":\"github\"}}"
                .to_string(),
        );
    }
    (
        "404 Not Found",
        "{\"error\":{\"code\":\"E_NOT_FOUND\",\"message\":\"no such route\"}}".to_string(),
    )
}

/// Reads one HTTP request: method, path and body.
fn read_request(stream: &mut TcpStream) -> Option<(String, String)> {
    let mut buffer = Vec::new();
    let mut chunk = [0u8; 8192];
    let header_end = loop {
        let read = stream.read(&mut chunk).ok()?;
        if read == 0 {
            return None;
        }
        buffer.extend_from_slice(&chunk[..read]);
        if let Some(position) = find(&buffer, b"\r\n\r\n") {
            break position;
        }
        if buffer.len() > 1024 * 1024 {
            return None;
        }
    };
    let headers = String::from_utf8_lossy(&buffer[..header_end]).to_string();
    let path = headers
        .lines()
        .next()
        .and_then(|line| line.split_whitespace().nth(1))
        .unwrap_or("/")
        .to_string();
    let length = headers
        .lines()
        .find_map(|line| {
            let (name, value) = line.split_once(':')?;
            if name.trim().eq_ignore_ascii_case("content-length") {
                value.trim().parse::<usize>().ok()
            } else {
                None
            }
        })
        .unwrap_or(0);
    let mut body = buffer[header_end + 4..].to_vec();
    while body.len() < length {
        let read = stream.read(&mut chunk).ok()?;
        if read == 0 {
            break;
        }
        body.extend_from_slice(&chunk[..read]);
    }
    Some((path, String::from_utf8_lossy(&body).to_string()))
}

fn find(
    haystack: &[u8],
    needle: &[u8],
) -> Option<usize> {
    haystack
        .windows(needle.len())
        .position(|window| window == needle)
}

/// Reads one JSON string field without pulling in a JSON parser.
///
/// The stub only ever reads documents this client wrote, and the values it
/// needs are plain ASCII or escaped text, so a field scan is enough — and it
/// keeps the test crate free of dependencies it would otherwise only use here.
fn json_string(
    document: &str,
    key: &str,
) -> Option<String> {
    let needle = format!("\"{key}\":\"");
    let start = document.find(&needle)? + needle.len();
    let mut out = String::new();
    let mut characters = document[start..].chars();
    while let Some(character) = characters.next() {
        match character {
            | '"' => return Some(out),
            | '\\' => match characters.next()? {
                | 'n' => out.push('\n'),
                | 'r' => out.push('\r'),
                | 't' => out.push('\t'),
                | '"' => out.push('"'),
                | '\\' => out.push('\\'),
                | other => out.push(other),
            },
            | other => out.push(other),
        }
    }
    None
}
