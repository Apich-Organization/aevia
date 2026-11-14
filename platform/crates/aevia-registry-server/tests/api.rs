//! HTTP-level integration tests: real requests, the real router, a real
//! database.
//!
//! # What these cover that the unit tests cannot
//!
//! A unit test can check that `safe_next` refuses `//evil.example`; it cannot
//! check that `POST /api/v1/publish` spends the nonce *before* it verifies the
//! signature, that a replay is a `400` and not a `500`, or that the index, the
//! download and the package page agree about one archive. Those are properties
//! of the assembled application, so these tests assemble it — the same
//! [`router`] the binary serves — and push `Request`s through it with
//! `tower::ServiceExt::oneshot`, which is the whole stack minus the socket.
//!
//! # Running them
//!
//! ```text
//! AEVIA_TEST_DATABASE_URL=postgres://aevia:aevia@127.0.0.1:55433/aevia \
//!     cargo test -p aevia-registry-server
//! ```
//!
//! Without that variable every test returns immediately, so the suite passes on
//! a machine with no PostgreSQL. The signed-publish test additionally needs
//! `gpg` on `PATH` and a `GNUPGHOME` keyring holding a secret key; it skips with
//! a message when either is missing.
//!
//! # Why the tests do not run in parallel
//!
//! Each test starts from `DROP SCHEMA public CASCADE; CREATE SCHEMA public;` so
//! that a row left behind by one test cannot make another pass or fail. That
//! reset is global state: two tests doing it at once would delete each other's
//! tables. [`SCHEMA_LOCK`] serialises them, so the default thread count is
//! safe — `--test-threads=1` is simply uncontended.

use std::io::Write as _;
use std::process::Command;
use std::process::Stdio;
use std::sync::Arc;

use axum::Router;
use axum::body::Body;
use axum::body::to_bytes;
use axum::http::HeaderMap;
use axum::http::Request;
use axum::http::StatusCode;
use axum::http::header;
use sqlx::PgPool;
use sqlx::postgres::PgPoolOptions;
use tower::ServiceExt as _;

use aevia_registry::config::Config;
use aevia_registry::router;
use aevia_registry::spec::SpecStore;
use aevia_registry::state::AppState;
use aevia_registry::storage::Storage;
use aevia_registry_core::manifest::PackageManifest;
use aevia_registry_core::signature::SignerKey;
use aevia_registry_core::tarball;

/// Serialises the per-test schema reset; see the module comment.
static SCHEMA_LOCK: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

/// The application under test, plus what it needs to stay alive.
struct TestApp {
    /// The router the binary serves.
    app: Router,
    /// The pool, for the SQL a browser flow would have performed.
    pool: PgPool,
    /// The archive store; held so the directory outlives the test.
    _storage: tempfile::TempDir,
    /// The (empty) specification directory.
    _spec: tempfile::TempDir,
    /// Held for the test's lifetime so no other test resets the schema.
    _schema: tokio::sync::MutexGuard<'static, ()>,
}

/// Builds the application against a fresh schema and a fresh directory.
///
/// Returns `None` — after saying so — when `AEVIA_TEST_DATABASE_URL` is unset.
async fn start() -> Option<TestApp> {
    let url = match std::env::var("AEVIA_TEST_DATABASE_URL") {
        | Ok(value) if !value.trim().is_empty() => value.trim().to_string(),
        | _ => {
            eprintln!(
                "skipping: set AEVIA_TEST_DATABASE_URL to run the integration tests, e.g. \
                 `docker run -d --name aevia-tests-db -e POSTGRES_USER=aevia -e \
                 POSTGRES_PASSWORD=aevia -e POSTGRES_DB=aevia -p 55433:5432 \
                 postgres:18-trixie` and then \
                 AEVIA_TEST_DATABASE_URL=postgres://aevia:aevia@127.0.0.1:55433/aevia"
            );
            return None;
        },
    };
    // An async mutex: the guard has to live across the `.await` below, and a
    // `std::sync::MutexGuard` held across an await point can deadlock.
    let _schema = SCHEMA_LOCK.lock().await;

    let pool = PgPoolOptions::new()
        .max_connections(4)
        .connect(&url)
        .await
        .expect("connect to AEVIA_TEST_DATABASE_URL");

    // A fresh schema, not a truncated one: the migrations create the tables, so
    // dropping the schema also drops `_sqlx_migrations` and they run again. A
    // test that forgot to clean up cannot leak a row into the next one.
    sqlx::query("DROP SCHEMA IF EXISTS public CASCADE")
        .execute(&pool)
        .await
        .expect("drop the schema");
    sqlx::query("CREATE SCHEMA public")
        .execute(&pool)
        .await
        .expect("create the schema");
    aevia_registry_db::migrate(&pool)
        .await
        .expect("apply the migrations");

    let storage = tempfile::tempdir().expect("a temporary storage directory");
    let spec_dir = tempfile::tempdir().expect("a temporary specification directory");
    let config = Config {
        bind: "127.0.0.1:0".parse().expect("a socket address"),
        base_url: "http://127.0.0.1:8080".to_string(),
        database_url: url,
        storage_dir: storage.path().to_path_buf(),
        spec_dir: spec_dir.path().to_path_buf(),
        // A fixed secret: the tests do not restart, and a random one would make
        // a failure impossible to reproduce from the log.
        session_secret: vec![0x5a; 32],
        registry_name: "aevia".to_string(),
        docs_enabled: false,
        trust_proxy: false,
        github: None,
        orcid: None,
    };
    let store = Storage::open(storage.path()).expect("open the archive store");
    let spec = Arc::new(SpecStore::load(spec_dir.path()).expect("load the specification"));
    let state = AppState::new(pool.clone(), Arc::new(config), store, spec);
    Some(TestApp {
        app: router(state),
        pool,
        _storage: storage,
        _spec: spec_dir,
        _schema,
    })
}

// ------------------------------------------------------------------ requests

/// Sends one request through the router and reads the whole response.
async fn send(
    app: &Router,
    request: Request<Body>,
) -> (StatusCode, HeaderMap, Vec<u8>) {
    let response = app
        .clone()
        .oneshot(request)
        .await
        .expect("the router answers");
    let status = response.status();
    let headers = response.headers().clone();
    let body = to_bytes(response.into_body(), 16 * 1024 * 1024)
        .await
        .expect("the body is readable");
    (status, headers, body.to_vec())
}

/// `GET uri`.
async fn get(
    app: &Router,
    uri: &str,
) -> (StatusCode, HeaderMap, Vec<u8>) {
    let request = Request::builder()
        .uri(uri)
        .body(Body::empty())
        .expect("a request");
    send(app, request).await
}

/// `GET uri` with a `Cookie` header.
async fn get_with_cookie(
    app: &Router,
    uri: &str,
    cookie: &str,
) -> (StatusCode, HeaderMap, Vec<u8>) {
    let request = Request::builder()
        .uri(uri)
        .header(header::COOKIE, cookie)
        .body(Body::empty())
        .expect("a request");
    send(app, request).await
}

/// `POST uri` with a JSON body.
async fn post_json(
    app: &Router,
    uri: &str,
    body: &str,
) -> (StatusCode, HeaderMap, Vec<u8>) {
    let request = Request::builder()
        .method("POST")
        .uri(uri)
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(body.to_string()))
        .expect("a request");
    send(app, request).await
}

/// The `error.code` of an API error document.
fn error_code(body: &[u8]) -> String {
    let value: serde_json::Value = serde_json::from_slice(body).expect("the error body is JSON");
    value["error"]["code"]
        .as_str()
        .unwrap_or_default()
        .to_string()
}

/// `POST /api/v1/publish/challenge`.
async fn challenge(
    app: &Router,
    name: &str,
    version: &str,
    checksum: &str,
) -> (StatusCode, Vec<u8>) {
    let body = serde_json::json!({
        "name": name,
        "version": version,
        "checksum": checksum,
    })
    .to_string();
    let (status, _, body) = post_json(app, "/api/v1/publish/challenge", &body).await;
    (status, body)
}

// -------------------------------------------------------------- the signer

/// A secret key in `GNUPGHOME`, as `gpg` reports it.
struct Signer {
    /// The fingerprint `gpg --local-user` accepts.
    fingerprint: String,
    /// The public half, ASCII-armoured, as the envelope carries it.
    armored: String,
    /// The `GNUPGHOME` spelling this `gpg` accepted, if any.
    home: Option<String>,
}

impl Signer {
    /// Finds a secret key, or explains why the test is skipping.
    fn detect() -> Option<Self> {
        for home in keyrings() {
            let Some(listing) = gpg(
                &["--batch", "--with-colons", "--list-secret-keys"],
                home.as_deref(),
            ) else {
                continue;
            };
            let listing = String::from_utf8_lossy(&listing).to_string();
            let fingerprint = listing.lines().find_map(|line| {
                let rest = line.strip_prefix("fpr:")?;
                rest.split(':')
                    .find(|field| !field.is_empty())
                    .map(str::to_string)
            });
            let Some(fingerprint) = fingerprint else {
                continue;
            };
            let Some(exported) = gpg(
                &["--batch", "--armor", "--export", &fingerprint],
                home.as_deref(),
            ) else {
                continue;
            };
            let armored = String::from_utf8_lossy(&exported).to_string();
            if !armored.contains("BEGIN PGP PUBLIC KEY BLOCK") {
                continue;
            }
            return Some(Self {
                fingerprint,
                armored,
                home,
            });
        }
        eprintln!(
            "skipping the signed publish: set GNUPGHOME to a keyring holding a secret key; \
             create one with `export GNUPGHOME=/tmp/aevia-tests-gpg; mkdir -p $GNUPGHOME; \
             chmod 700 $GNUPGHOME; gpg --batch --passphrase '' --quick-generate-key \"Test \
             <t@example.invalid>\" ed25519 sign 1y`"
        );
        None
    }

    /// `gpg --detach-sign --armor` over the exact statement bytes.
    fn sign(
        &self,
        statement: &str,
    ) -> String {
        let mut command = Command::new("gpg");
        command
            .args([
                "--batch",
                "--yes",
                "--armor",
                "--detach-sign",
                "--local-user",
                &self.fingerprint,
                "--output",
                "-",
            ])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        if let Some(home) = &self.home {
            command.env("GNUPGHOME", home);
        }
        let mut child = command.spawn().expect("start gpg");
        child
            .stdin
            .as_mut()
            .expect("gpg's stdin is piped")
            .write_all(statement.as_bytes())
            .expect("write the statement to gpg");
        let output = child.wait_with_output().expect("gpg finishes");
        assert!(
            output.status.success(),
            "gpg refused to sign: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        String::from_utf8(output.stdout).expect("gpg prints an armoured signature")
    }
}

/// The `GNUPGHOME` spellings worth trying, in order.
///
/// An unset `GNUPGHOME` means "skip", not "use the developer's own keyring":
/// that keyring may hold a hardware key, and a test has no business asking for
/// its PIN.
///
/// On Windows a native process inherits the variable in Windows form
/// (`C:/Users/…`), while the `gpg` on this machine's `PATH` is an MSYS build
/// that resolves only POSIX paths — so the shell's `GNUPGHOME=/tmp/…` arrives
/// here as something `gpg` cannot open. Offering both spellings means the test
/// uses whatever keyring the developer actually set up, on any platform.
fn keyrings() -> Vec<Option<String>> {
    let Some(inherited) = std::env::var("GNUPGHOME")
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
    else {
        return Vec::new();
    };
    let mut homes = vec![Some(inherited.clone())];
    if let Some(posix) = posix_form(&inherited) {
        homes.push(Some(posix));
    }
    homes
}

/// `C:/Users/x` → `/c/Users/x`, the form an MSYS `gpg` resolves.
fn posix_form(home: &str) -> Option<String> {
    let bytes = home.as_bytes();
    if bytes.len() < 3 || bytes[1] != b':' || !bytes[0].is_ascii_alphabetic() {
        return None;
    }
    let rest = home[2..].replace('\\', "/");
    Some(format!(
        "/{}{rest}",
        char::from(bytes[0]).to_ascii_lowercase()
    ))
}

/// Runs `gpg`, returning its stdout when it succeeds.
fn gpg(
    args: &[&str],
    home: Option<&str>,
) -> Option<Vec<u8>> {
    let mut command = Command::new("gpg");
    command.args(args);
    if let Some(home) = home {
        command.env("GNUPGHOME", home);
    }
    match command.output() {
        | Ok(output) if output.status.success() => Some(output.stdout),
        | Ok(output) => {
            eprintln!(
                "`gpg {}` failed: {}",
                args.join(" "),
                String::from_utf8_lossy(&output.stderr).trim()
            );
            None
        },
        | Err(error) => {
            eprintln!("cannot run `gpg`: {error}");
            None
        },
    }
}

// ------------------------------------------------------------- the fixtures

/// A small package, packed by the same code the client uses.
///
/// Using [`tarball::pack_dir`] rather than hand-rolling a tar means the test
/// exercises the real archive format: if the packer and the server's unpacker
/// ever disagree, this fails.
fn fixture(
    name: &str,
    version: &str,
) -> (tempfile::TempDir, Vec<u8>, String) {
    let directory = tempfile::tempdir().expect("a temporary package");
    std::fs::create_dir_all(directory.path().join("src")).expect("src/");
    std::fs::write(
        directory.path().join("aevia.toml"),
        format!("[package]\nname = \"{name}\"\nversion = \"{version}\"\n"),
    )
    .expect("aevia.toml");
    std::fs::write(
        directory.path().join("src/main.aev"),
        "## The demo entry.\nfn main(): IO Unit = do\n    println(\"hi\")\n",
    )
    .expect("src/main.aev");
    std::fs::write(
        directory.path().join("README.md"),
        "# demo\n\nA package that exists to be published by a test.\n",
    )
    .expect("README.md");
    let manifest = PackageManifest::load(directory.path()).expect("the manifest parses");
    let archive = tarball::pack_dir(directory.path(), &manifest).expect("the fixture packs");
    let checksum = tarball::checksum(&archive);
    (directory, archive, checksum)
}

/// Writes the publisher and key rows a successful browser sign-in would have
/// written, so the publish needs no OAuth round trip.
async fn seed_publisher(
    pool: &PgPool,
    handle: &str,
    fingerprint: &str,
    armored: &str,
) {
    let publisher_id: i64 = sqlx::query_scalar(
        "INSERT INTO publishers (kind, handle, display_name)
         VALUES ('github', $1, 'Test Publisher')
         RETURNING id",
    )
    .bind(handle)
    .fetch_one(pool)
    .await
    .expect("seed a publisher");
    sqlx::query(
        "INSERT INTO signing_keys (publisher_id, kind, fingerprint, public_key, label)
         VALUES ($1, 'openpgp', $2, $3, 'integration test')",
    )
    .bind(publisher_id)
    .bind(fingerprint)
    .bind(armored)
    .execute(pool)
    .await
    .expect("seed a signing key");
}

/// Assembles the two-part `multipart/form-data` body by hand.
///
/// Byte for byte what `aevia-pkg` sends, which is the point: the test should
/// fail if the server stops accepting the body the client produces.
fn multipart(
    envelope: &str,
    archive: &[u8],
) -> (String, Vec<u8>) {
    let boundary = "----aevia-integration-test";
    let mut body = Vec::with_capacity(archive.len() + envelope.len() + 512);
    body.extend_from_slice(
        format!(
            "--{boundary}\r\n\
             Content-Disposition: form-data; name=\"envelope\"\r\n\
             Content-Type: application/json\r\n\r\n"
        )
        .as_bytes(),
    );
    body.extend_from_slice(envelope.as_bytes());
    body.extend_from_slice(
        format!(
            "\r\n--{boundary}\r\n\
             Content-Disposition: form-data; name=\"package\"; filename=\"package.tar.gz\"\r\n\
             Content-Type: application/gzip\r\n\r\n"
        )
        .as_bytes(),
    );
    body.extend_from_slice(archive);
    body.extend_from_slice(format!("\r\n--{boundary}--\r\n").as_bytes());
    (format!("multipart/form-data; boundary={boundary}"), body)
}

/// `POST uri` with a hand-built multipart body.
async fn post_multipart(
    app: &Router,
    uri: &str,
    content_type: &str,
    body: Vec<u8>,
) -> (StatusCode, HeaderMap, Vec<u8>) {
    let request = Request::builder()
        .method("POST")
        .uri(uri)
        .header(header::CONTENT_TYPE, content_type)
        .body(Body::from(body))
        .expect("a request");
    send(app, request).await
}

// ------------------------------------------------------------------- tests

#[tokio::test]
async fn health_and_stats_are_green_on_an_empty_registry() {
    let Some(app) = start().await else {
        return;
    };

    let (status, _, body) = get(&app.app, "/healthz").await;
    assert_eq!(status, StatusCode::OK);
    let health: serde_json::Value =
        serde_json::from_slice(&body).expect("health is a JSON document");
    assert_eq!(health["status"], "ok", "{}", String::from_utf8_lossy(&body));

    let (status, _, body) = get(&app.app, "/api/v1/stats").await;
    assert_eq!(status, StatusCode::OK);
    let stats: serde_json::Value = serde_json::from_slice(&body).expect("stats is a JSON document");
    for counter in ["packages", "versions", "downloads", "publishers"] {
        assert_eq!(stats[counter], 0, "`{counter}` starts at zero: {stats}");
    }
}

#[tokio::test]
async fn a_malformed_challenge_names_the_field_that_is_wrong() {
    let Some(app) = start().await else {
        return;
    };
    let checksum = format!("sha256:{}", "ab".repeat(32));

    let (status, body) = challenge(&app.app, "not a package", "0.1.0", &checksum).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(error_code(&body), "E_NAME");

    let (status, body) = challenge(&app.app, "tester/demo", "1.0", &checksum).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(error_code(&body), "E_VERSION");

    let (status, body) = challenge(&app.app, "tester/demo", "0.1.0", "not-a-digest").await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(error_code(&body), "E_TARBALL");
}

#[tokio::test]
async fn a_signed_publish_round_trips_and_cannot_be_replayed() {
    let Some(app) = start().await else {
        return;
    };
    let Some(signer) = Signer::detect() else {
        return;
    };

    // The fingerprint the server will compute, so the seeded row matches.
    let fingerprint = SignerKey::detect(&signer.armored)
        .expect("the exported key parses")
        .fingerprint()
        .expect("the exported key has a fingerprint");
    seed_publisher(&app.pool, "tester", &fingerprint, &signer.armored).await;

    let (_package_dir, archive, checksum) = fixture("demo", "0.1.0");

    // The challenge is the only thing a client cannot fabricate.
    let (status, body) = challenge(&app.app, "tester/demo", "0.1.0", &checksum).await;
    assert_eq!(status, StatusCode::OK, "{}", String::from_utf8_lossy(&body));
    let issued: serde_json::Value = serde_json::from_slice(&body).expect("the challenge is JSON");
    let statement = issued["statement"]
        .as_str()
        .expect("the challenge carries a statement")
        .to_string();
    assert!(statement.contains(&checksum), "{statement}");

    let envelope = serde_json::json!({
        "statement": statement,
        "signature": signer.sign(&statement),
        "key": { "kind": "openpgp", "armored": signer.armored },
    })
    .to_string();
    let (content_type, body) = multipart(&envelope, &archive);

    let (status, _, answer) =
        post_multipart(&app.app, "/api/v1/publish", &content_type, body.clone()).await;
    assert_eq!(
        status,
        StatusCode::CREATED,
        "{}",
        String::from_utf8_lossy(&answer)
    );
    let accepted: serde_json::Value =
        serde_json::from_slice(&answer).expect("the acceptance is JSON");
    assert_eq!(accepted["name"], "tester/demo");
    assert_eq!(accepted["version"], "0.1.0");
    assert_eq!(accepted["checksum"], checksum);

    // The same bytes, again: the nonce was spent, so this must not publish.
    let (status, _, answer) =
        post_multipart(&app.app, "/api/v1/publish", &content_type, body).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(error_code(&answer), "E_STATEMENT");

    // The index holds exactly one record, and it names the archive's digest.
    let (status, _, body) = get(&app.app, "/api/v1/index/tester/demo").await;
    assert_eq!(status, StatusCode::OK);
    let index = String::from_utf8(body).expect("the index is UTF-8");
    let lines: Vec<&str> = index.lines().collect();
    assert_eq!(lines.len(), 1, "{index}");
    let record: serde_json::Value =
        serde_json::from_str(lines[0]).expect("an index record is JSON");
    assert_eq!(record["cksum"], checksum);
    assert_eq!(record["yanked"], false);

    // The download is the archive that was signed, and says so in a header.
    let (status, headers, body) = get(&app.app, "/api/v1/download/tester/demo/0.1.0").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body, archive, "the stored archive is the one uploaded");
    assert_eq!(
        headers
            .get("x-aevia-checksum")
            .and_then(|value| value.to_str().ok()),
        Some(checksum.as_str())
    );

    // The package page renders.
    let (status, _, body) = get(&app.app, "/crates/tester/demo").await;
    assert_eq!(status, StatusCode::OK);
    let page = String::from_utf8_lossy(&body);
    assert!(page.contains("demo"), "the page names the package");
    assert!(page.contains("0.1.0"), "the page names the version");

    // A signed yank flips the flag in the index.
    let (status, _, body) = post_json(
        &app.app,
        "/api/v1/yank/challenge",
        &serde_json::json!({
            "name": "tester/demo",
            "version": "0.1.0",
            "yanked": true,
        })
        .to_string(),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{}", String::from_utf8_lossy(&body));
    let issued: serde_json::Value =
        serde_json::from_slice(&body).expect("the yank challenge is JSON");
    let statement = issued["statement"]
        .as_str()
        .expect("the yank challenge carries a statement")
        .to_string();
    let (status, _, body) = post_json(
        &app.app,
        "/api/v1/yank",
        &serde_json::json!({
            "statement": statement,
            "signature": signer.sign(&statement),
            "key": { "kind": "openpgp", "armored": signer.armored },
        })
        .to_string(),
    )
    .await;
    assert_eq!(
        status,
        StatusCode::NO_CONTENT,
        "{}",
        String::from_utf8_lossy(&body)
    );

    let (_, _, body) = get(&app.app, "/api/v1/index/tester/demo").await;
    let index = String::from_utf8(body).expect("the index is UTF-8");
    let record: serde_json::Value =
        serde_json::from_str(index.lines().next().expect("one record")).expect("JSON");
    assert_eq!(record["yanked"], true, "the yank reached the index");
}

#[tokio::test]
async fn an_unknown_package_and_an_empty_specification_are_not_found() {
    let Some(app) = start().await else {
        return;
    };

    let (status, _, body) = get(&app.app, "/api/v1/download/nobody/nothing/1.0.0").await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_eq!(error_code(&body), "E_NOT_FOUND");

    // The spec store is empty here, so any chapter is missing but the index
    // still explains how to mount one.
    let (status, _, _) = get(&app.app, "/spec/no-such-chapter").await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    let (status, _, _) = get(&app.app, "/spec").await;
    assert_eq!(status, StatusCode::OK);
}

#[tokio::test]
async fn the_language_switch_sets_a_cookie_and_renders_chinese() {
    let Some(app) = start().await else {
        return;
    };

    let (status, headers, _) = get(&app.app, "/lang/zh").await;
    assert_eq!(status, StatusCode::SEE_OTHER);
    let cookie = headers
        .get(header::SET_COOKIE)
        .and_then(|value| value.to_str().ok())
        .expect("the language switch sets a cookie");
    assert!(cookie.starts_with("aevia_lang=zh"), "{cookie}");

    let (status, _, body) = get_with_cookie(&app.app, "/", "aevia_lang=zh").await;
    assert_eq!(status, StatusCode::OK);
    let page = String::from_utf8_lossy(&body);
    assert!(
        page.contains("为 Aevia 语言提供带签名的包"),
        "the home page honours the cookie: {}",
        &page[..page.len().min(400)]
    );
}

#[tokio::test]
async fn the_machine_pages_and_the_cli_guide_are_served() {
    let Some(app) = start().await else {
        return;
    };

    let (status, headers, body) = get(&app.app, "/robots.txt").await;
    assert_eq!(status, StatusCode::OK);
    assert!(String::from_utf8_lossy(&body).contains("Sitemap: "));
    assert!(
        headers
            .get(header::CONTENT_TYPE)
            .is_some_and(|value| value.to_str().is_ok_and(|v| v.starts_with("text/plain")))
    );

    let (status, _, body) = get(&app.app, "/sitemap.xml").await;
    assert_eq!(status, StatusCode::OK);
    assert!(String::from_utf8_lossy(&body).contains("<urlset"));

    let (status, _, body) = get(&app.app, "/feed.xml").await;
    assert_eq!(status, StatusCode::OK);
    assert!(String::from_utf8_lossy(&body).contains("<feed"));

    let (status, _, body) = get(&app.app, "/cli").await;
    assert_eq!(status, StatusCode::OK);
    assert!(String::from_utf8_lossy(&body).contains("aevia publish"));

    let (status, _, _) = get(&app.app, "/users/nobody-here").await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    let (status, _, _) = get(&app.app, "/badge/nobody/nothing.svg").await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}
