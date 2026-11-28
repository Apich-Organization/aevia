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
//! a machine with no PostgreSQL. Set `AEVIA_REQUIRE_TEST_FIXTURES=1` when a
//! missing database or signing fixture must fail the run. The signed-publish test additionally needs
//! `gpg` on `PATH` and a `GNUPGHOME` keyring holding a secret key; it skips with
//! a message when either is missing.
//!
//! Each test creates an isolated schema. The supplied database user must be
//! able to create and drop its test schemas.

#![allow(clippy::expect_used, clippy::unwrap_used)] // Fixture setup fails the current test.

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
use sqlx::Connection as _;
use sqlx::PgPool;
use sqlx::postgres::PgConnection;
use sqlx::postgres::PgPoolOptions;
use tower::ServiceExt as _;

use aevia_registry::config::Config;
use aevia_registry::manual::ManualStore;
use aevia_registry::router;
use aevia_registry::spec::SpecStore;
use aevia_registry::state::AppState;
use aevia_registry::stdlib::StdlibDocStore;
use aevia_registry::storage::Storage;
use aevia_registry_core::manifest::PackageManifest;
use aevia_registry_core::signature::SignerKey;
use aevia_registry_core::tarball;
use aevia_registry_db::models::GithubOrganisation;
use aevia_registry_db::models::NewVersion;
use aevia_registry_db::repo;

/// The application under test, plus what it needs to stay alive.
struct TestApp {
    /// The router the binary serves.
    app: Router,
    state: AppState,
    /// The pool, for the SQL a browser flow would have performed.
    pool: PgPool,
    /// The archive store; held so the directory outlives the test.
    _storage: tempfile::TempDir,
    /// The (empty) specification directory.
    _spec: tempfile::TempDir,
    /// These directories outlive both the snapshot and reload tests.
    _stdlib_and_manuals: tempfile::TempDir,
    docs_worker: Option<tokio::task::JoinHandle<()>>,
    /// Removes only the test's own schema.
    _schema: TestSchema,
}

impl Drop for TestApp {
    fn drop(&mut self) {
        if let Some(worker) = &self.docs_worker {
            worker.abort();
        }
    }
}

struct TestSchema {
    url: String,
    name: String,
}

impl Drop for TestSchema {
    fn drop(&mut self) {
        // Tests use independent Tokio runtimes. Await the cleanup on a
        // dedicated thread so it survives shutdown of the test runtime.
        let url = self.url.clone();
        let name = self.name.clone();
        let result = std::thread::spawn(
            move || -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
                tokio::runtime::Builder::new_current_thread()
                    .enable_all()
                    .build()?
                    .block_on(async move {
                        let mut connection = PgConnection::connect(&url).await?;
                        sqlx::query(
                            "SELECT pg_terminate_backend(pid) FROM pg_stat_activity
                         WHERE application_name = $1 AND pid <> pg_backend_pid()",
                        )
                        .bind(&name)
                        .execute(&mut connection)
                        .await?;
                        sqlx::query(&format!("DROP SCHEMA {name} CASCADE"))
                            .execute(&mut connection)
                            .await?;
                        Ok(())
                    })
            },
        )
        .join();
        if !matches!(result, Ok(Ok(()))) {
            eprintln!("could not remove test schema {}", self.name);
        }
    }
}

/// Builds the application against a fresh schema and a fresh directory.
///
/// Returns `None` — after saying so — when `AEVIA_TEST_DATABASE_URL` is unset.
async fn start() -> Option<TestApp> {
    start_with_docs(false).await
}

async fn start_with_docs(docs_enabled: bool) -> Option<TestApp> {
    let url = match std::env::var("AEVIA_TEST_DATABASE_URL") {
        | Ok(value) if !value.trim().is_empty() => value.trim().to_string(),
        | _ => {
            assert!(
                std::env::var_os("AEVIA_REQUIRE_TEST_FIXTURES").is_none(),
                "AEVIA_TEST_DATABASE_URL is required by AEVIA_REQUIRE_TEST_FIXTURES"
            );
            eprintln!(
                "skipping: set AEVIA_TEST_DATABASE_URL to run the integration tests, e.g. \
                 `podman run -d --name aevia-tests-db -e POSTGRES_USER=aevia -e \
                 POSTGRES_PASSWORD=aevia -e POSTGRES_DB=aevia -p 55433:5432 \
                 docker.io/library/postgres:18-trixie` and then \
                 AEVIA_TEST_DATABASE_URL=postgres://aevia:aevia@127.0.0.1:55433/aevia"
            );
            return None;
        },
    };
    let name = format!("aevia_it_{}", aevia_registry::http::random_hex(12));
    let mut admin = PgConnection::connect(&url)
        .await
        .expect("connect to AEVIA_TEST_DATABASE_URL");
    sqlx::query(&format!("CREATE SCHEMA {name}"))
        .execute(&mut admin)
        .await
        .expect("create isolated test schema");
    admin.close().await.expect("close schema connection");
    let schema = TestSchema {
        url: url.clone(),
        name: name.clone(),
    };
    let pool = PgPoolOptions::new()
        .max_connections(4)
        .after_connect(move |connection, _| {
            let search_path = name.clone();
            Box::pin(async move {
                sqlx::query("SELECT set_config('search_path', $1, false), set_config('application_name', $1, false)")
                    .bind(search_path)
                    .execute(connection)
                    .await?;
                Ok(())
            })
        })
        .connect(&url)
        .await
        .expect("connect to AEVIA_TEST_DATABASE_URL");
    aevia_registry_db::migrate(&pool)
        .await
        .expect("apply the migrations");

    let storage = tempfile::tempdir().expect("a temporary storage directory");
    let spec_dir = tempfile::tempdir().expect("a temporary specification directory");
    let stdlib_dir = tempfile::tempdir().expect("a temporary stdlib docs directory");
    for (lang, text) in [
        (
            "en",
            "# Test chapter\n\n## Start\n\nWrite your first program.\n",
        ),
        ("zh", "# 测试章节\n\n## 开始\n\n编写第一个程序。\n"),
    ] {
        let directory = stdlib_dir.path().join("manuals/book").join(lang);
        std::fs::create_dir_all(&directory).unwrap();
        std::fs::write(directory.join("01-start.md"), text).unwrap();
    }
    let config = Config {
        bind: "127.0.0.1:0".parse().expect("a socket address"),
        base_url: "http://127.0.0.1:8080".to_string(),
        database_url: url,
        storage_dir: storage.path().to_path_buf(),
        spec_dir: spec_dir.path().to_path_buf(),
        stdlib_docs_dir: stdlib_dir.path().to_path_buf(),
        manuals_dir: stdlib_dir.path().join("manuals"),
        playground: None,
        install_dir: std::path::PathBuf::from("."),
        // A fixed secret: the tests do not restart, and a random one would make
        // a failure impossible to reproduce from the log.
        session_secret: vec![0x5a; 32],
        registry_name: "aevia".to_string(),
        docs_enabled,
        trust_proxy: false,
        github: None,
        orcid: None,
    };
    let store = Storage::open(storage.path()).expect("open the archive store");
    let spec = Arc::new(SpecStore::load(spec_dir.path()).expect("load the specification"));
    let stdlib_docs =
        Arc::new(StdlibDocStore::load(stdlib_dir.path()).expect("load the stdlib docs"));
    let manuals = ManualStore::load(&config.manuals_dir, &config.base_url).unwrap();
    let state = AppState::new(
        pool.clone(),
        Arc::new(config),
        store,
        spec,
        stdlib_docs,
        Arc::new(manuals),
    );
    let docs_worker = aevia_registry::docs_queue::spawn(state.clone());
    Some(TestApp {
        app: router(state.clone()),
        state,
        pool,
        _storage: storage,
        _spec: spec_dir,
        _stdlib_and_manuals: stdlib_dir,
        docs_worker,
        _schema: schema,
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
        assert!(
            std::env::var_os("GNUPGHOME").is_none()
                && std::env::var_os("AEVIA_REQUIRE_TEST_FIXTURES").is_none(),
            "GnuPG could not list and export a test signing key from GNUPGHOME"
        );
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

#[tokio::test]
async fn github_identity_rename_keeps_the_publisher_package_and_signing_key() {
    let Some(app) = start().await else {
        return;
    };
    let user = repo::upsert_github_user(&app.pool, 101, "catalogue-owner", Some("Before"), None)
        .await
        .unwrap()
        .unwrap();
    let other = repo::upsert_github_user(&app.pool, 102, "taken-namespace", None, None)
        .await
        .unwrap()
        .unwrap();
    let package = repo::create_package(&app.pool, user.id, "example")
        .await
        .unwrap();
    let key = repo::add_signing_key(
        &app.pool,
        user.id,
        "ssh",
        "SHA256:fixture",
        "public-key-fixture",
        None,
    )
    .await
    .unwrap();

    let renamed = repo::upsert_github_user(&app.pool, 101, "taken-namespace", Some("After"), None)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(renamed.id, user.id);
    assert_eq!(renamed.handle, "catalogue-owner");
    assert_eq!(renamed.github_id, Some(101));
    assert_eq!(renamed.display_name.as_deref(), Some("After"));
    assert_eq!(
        repo::package_by_handle(&app.pool, "catalogue-owner", "example")
            .await
            .unwrap()
            .unwrap()
            .0
            .id,
        package.id,
    );
    assert_eq!(
        repo::signing_key_by_fingerprint(&app.pool, "ssh", &key.fingerprint)
            .await
            .unwrap()
            .unwrap()
            .publisher_id,
        user.id,
    );
    assert_eq!(
        repo::resolve_publishing_owner(&app.pool, user.id, "catalogue-owner")
            .await
            .unwrap(),
        Some(user.id)
    );
    assert_eq!(
        repo::resolve_publishing_owner(&app.pool, user.id, "taken-namespace")
            .await
            .unwrap(),
        None
    );
    assert_eq!(
        repo::resolve_publishing_owner(&app.pool, other.id, "taken-namespace")
            .await
            .unwrap(),
        Some(other.id)
    );
    assert!(
        repo::upsert_github_user(&app.pool, 103, "catalogue-owner", None, None)
            .await
            .unwrap()
            .is_none()
    );
}

#[tokio::test]
async fn github_identity_legacy_rows_require_review_before_browser_binding() {
    let Some(app) = start().await else {
        return;
    };
    let legacy_user: i64 = sqlx::query_scalar(
        "INSERT INTO publishers (kind, handle) VALUES ('github', 'legacy-user') RETURNING id",
    )
    .fetch_one(&app.pool)
    .await
    .unwrap();
    let legacy_org: i64 = sqlx::query_scalar(
        "INSERT INTO publishers (kind, handle) VALUES ('github-org', 'legacy-lab') RETURNING id",
    )
    .fetch_one(&app.pool)
    .await
    .unwrap();
    assert!(
        repo::upsert_github_user(&app.pool, 201, "legacy-user", None, None)
            .await
            .unwrap()
            .is_none()
    );
    assert_eq!(
        repo::begin_membership_refresh(&app.pool, legacy_user)
            .await
            .unwrap_err()
            .code,
        "E_FORBIDDEN"
    );
    repo::create_session(
        &app.pool,
        "legacy-browser-session",
        legacy_user,
        chrono::Utc::now() + chrono::Duration::days(1),
        None,
    )
    .await
    .unwrap();
    assert!(
        repo::session_publisher(&app.pool, "legacy-browser-session")
            .await
            .unwrap()
            .is_none()
    );
    assert_eq!(
        repo::resolve_publishing_owner(&app.pool, legacy_user, "legacy-user")
            .await
            .unwrap(),
        Some(legacy_user)
    );

    let actor = repo::upsert_github_user(&app.pool, 202, "verified-member", None, None)
        .await
        .unwrap()
        .unwrap();
    let legacy_org_claim = GithubOrganisation {
        github_id: 220,
        login: "legacy-lab".into(),
        avatar_url: None,
    };
    let generation = repo::begin_membership_refresh(&app.pool, actor.id)
        .await
        .unwrap();
    repo::finish_membership_refresh(&app.pool, actor.id, generation, &[legacy_org_claim])
        .await
        .unwrap();
    assert_eq!(
        repo::resolve_publishing_owner(&app.pool, actor.id, "legacy-lab")
            .await
            .unwrap(),
        None
    );
    assert!(
        repo::publisher(&app.pool, legacy_org)
            .await
            .unwrap()
            .unwrap()
            .github_id
            .is_none()
    );

    // Reviewed bindings are written by row ID and checked provider ID.
    sqlx::query("UPDATE publishers SET github_id = $2 WHERE id = $1 AND github_id IS NULL")
        .bind(legacy_user)
        .bind(201_i64)
        .execute(&app.pool)
        .await
        .unwrap();
    let restored = repo::upsert_github_user(&app.pool, 201, "renamed-user", None, None)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(restored.id, legacy_user);
    assert_eq!(restored.handle, "legacy-user");
}

#[tokio::test]
async fn github_identity_membership_keeps_org_ownership_and_expires_stale_grants() {
    let Some(app) = start().await else {
        return;
    };
    let user = repo::upsert_github_user(&app.pool, 301, "active-member", None, None)
        .await
        .unwrap()
        .unwrap();
    let observation = |github_id, login: &str| {
        GithubOrganisation {
            github_id,
            login: login.to_string(),
            avatar_url: None,
        }
    };
    let first = repo::begin_membership_refresh(&app.pool, user.id)
        .await
        .unwrap();
    repo::finish_membership_refresh(
        &app.pool,
        user.id,
        first,
        &[observation(330, "original-lab")],
    )
    .await
    .unwrap();
    let org_id = repo::resolve_publishing_owner(&app.pool, user.id, "original-lab")
        .await
        .unwrap()
        .unwrap();
    let package = repo::create_package(&app.pool, org_id, "example")
        .await
        .unwrap();

    let obsolete = repo::begin_membership_refresh(&app.pool, user.id)
        .await
        .unwrap();
    assert_eq!(
        repo::resolve_publishing_owner(&app.pool, user.id, "original-lab")
            .await
            .unwrap(),
        None
    );
    let current = repo::begin_membership_refresh(&app.pool, user.id)
        .await
        .unwrap();
    repo::finish_membership_refresh(
        &app.pool,
        user.id,
        obsolete,
        &[observation(330, "original-lab")],
    )
    .await
    .unwrap();
    assert_eq!(
        repo::resolve_publishing_owner(&app.pool, user.id, "original-lab")
            .await
            .unwrap(),
        None
    );
    repo::finish_membership_refresh(
        &app.pool,
        user.id,
        current,
        &[observation(330, "renamed-lab")],
    )
    .await
    .unwrap();
    assert_eq!(
        repo::resolve_publishing_owner(&app.pool, user.id, "original-lab")
            .await
            .unwrap(),
        Some(org_id)
    );
    assert_eq!(
        repo::resolve_publishing_owner(&app.pool, user.id, "renamed-lab")
            .await
            .unwrap(),
        None
    );
    assert_eq!(
        repo::package_by_handle(&app.pool, "original-lab", "example")
            .await
            .unwrap()
            .unwrap()
            .0
            .id,
        package.id,
    );

    sqlx::query("UPDATE org_members SET checked_at = now() - interval '2 days'")
        .execute(&app.pool)
        .await
        .unwrap();
    assert_eq!(
        repo::resolve_publishing_owner(&app.pool, user.id, "original-lab")
            .await
            .unwrap(),
        None
    );
    let current = repo::begin_membership_refresh(&app.pool, user.id)
        .await
        .unwrap();
    repo::finish_membership_refresh(
        &app.pool,
        user.id,
        current,
        &[observation(331, "original-lab")],
    )
    .await
    .unwrap();
    assert_eq!(
        repo::resolve_publishing_owner(&app.pool, user.id, "original-lab")
            .await
            .unwrap(),
        None
    );
    assert_eq!(
        repo::publisher(&app.pool, org_id)
            .await
            .unwrap()
            .unwrap()
            .github_id,
        Some(330)
    );
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
fn fixture_with_manifest(
    name: &str,
    version: &str,
    extra_package_lines: &str,
) -> (tempfile::TempDir, Vec<u8>, String) {
    let directory = tempfile::tempdir().expect("a temporary package");
    std::fs::create_dir_all(directory.path().join("src")).expect("src/");
    std::fs::write(
        directory.path().join("aevia.toml"),
        format!("[package]\nname = \"{name}\"\nversion = \"{version}\"\n{extra_package_lines}"),
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

fn fixture(
    name: &str,
    version: &str,
) -> (tempfile::TempDir, Vec<u8>, String) {
    fixture_with_manifest(
        name,
        version,
        "edition = \"2026\"\nauthors = [\"Test Publisher\"]\ndescription = \"Integration test package\"\nlicense = \"Apache-2.0\"\nrepository = \"https://github.com/apich/demo\"\nhomepage = \"https://aevia.org/demo\"\ndocumentation = \"https://docs.aevia.org/demo\"\nkeywords = [\"demo\", \"test\"]\ncategories = [\"mathematics\"]\n",
    )
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

/// Seeds a catalogue entry without signing, for tests of downstream readers.
async fn catalogue_version(
    app: &TestApp,
    version: &str,
    checksum: &str,
    size: usize,
    build_docs: bool,
) -> aevia_registry_db::models::Version {
    let owner: i64 = sqlx::query_scalar(
        "INSERT INTO publishers (kind, handle, display_name)
         VALUES ('github', 'test-reader', 'Test Publisher')
         ON CONFLICT (kind, handle) DO UPDATE SET display_name = EXCLUDED.display_name
         RETURNING id",
    )
    .fetch_one(&app.pool)
    .await
    .expect("seed a catalogue owner");
    let package = match repo::package_by_identity(&app.pool, owner, "archive_fixture")
        .await
        .unwrap()
    {
        | Some(package) => package,
        | None => {
            repo::create_package(&app.pool, owner, "archive_fixture")
                .await
                .unwrap()
        },
    };
    repo::insert_version(
        &app.pool,
        &NewVersion {
            package_id: package.id,
            version,
            checksum,
            size_bytes: i64::try_from(size).expect("fixture fits in bigint"),
            manifest: serde_json::json!({"name": "archive_fixture", "version": version}),
            dependencies: serde_json::json!([]),
            readme: None,
            published_by: owner,
            key_kind: "openpgp",
            fingerprint: "fixture",
            signature: "fixture signature",
            statement: "fixture statement\n",
            build_docs,
        },
    )
    .await
    .expect("insert a version into the catalogue")
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

    // The package page and API record all Cargo-style metadata fields.
    let (status, _, body) = get(&app.app, "/api/v1/crates/tester/demo").await;
    assert_eq!(status, StatusCode::OK);
    let detail: serde_json::Value =
        serde_json::from_slice(&body).expect("the crate detail is JSON");
    assert_eq!(detail["edition"], "2026");
    assert_eq!(detail["authors"], serde_json::json!(["Test Publisher"]));
    assert_eq!(detail["description"], "Integration test package");
    assert_eq!(detail["license"], "Apache-2.0");
    assert_eq!(detail["repository"], "https://github.com/apich/demo");
    assert_eq!(detail["homepage"], "https://aevia.org/demo");
    assert_eq!(detail["documentation"], "https://docs.aevia.org/demo");
    assert_eq!(detail["keywords"], serde_json::json!(["demo", "test"]));
    assert_eq!(detail["categories"], serde_json::json!(["mathematics"]));

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
async fn index_validators_and_head_follow_the_live_catalogue() {
    use aevia_registry::api::index::record_of;
    use aevia_registry_core::PackageId;
    use aevia_registry_core::index::IndexRecord;
    use aevia_registry_core::index::etag;

    let Some(app) = start().await else {
        return;
    };
    let mut first = catalogue_version(
        &app,
        "2.0.0",
        &format!("sha256:{}", "a".repeat(64)),
        123,
        false,
    )
    .await;
    let mut second = catalogue_version(
        &app,
        "1.0.0",
        &format!("sha256:{}", "b".repeat(64)),
        456,
        false,
    )
    .await;
    let tied = chrono::DateTime::from_timestamp(1_700_000_000, 0).unwrap();
    for row in [&mut first, &mut second] {
        sqlx::query("UPDATE versions SET created_at = $1 WHERE id = $2")
            .bind(tied)
            .bind(row.id)
            .execute(&app.pool)
            .await
            .unwrap();
        row.created_at = tied;
    }
    first.signature = "signature\n\"雪\"\\".into();
    first.dependencies = serde_json::json!([
        {"package":"test-reader/dep", "req":"^1.2", "import":"Dep"},
        {"import":"Legacy", "path":"../legacy"}
    ]);
    sqlx::query("UPDATE versions SET signature = $1, dependencies = $2 WHERE id = $3")
        .bind(&first.signature)
        .bind(&first.dependencies)
        .bind(first.id)
        .execute(&app.pool)
        .await
        .unwrap();

    let uri = "/api/v1/index/test-reader/Archive_Fixture";
    let id = PackageId::new("test-reader", "archive_fixture").unwrap();
    let expected = format!(
        "{}\n{}\n",
        record_of(&id, &first).to_line().unwrap(),
        record_of(&id, &second).to_line().unwrap()
    );
    let tag = etag(&expected);
    let (status, headers, body) = get(&app.app, uri).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(headers[header::ETAG], tag);
    assert_eq!(headers[header::CACHE_CONTROL], "public, no-cache");
    assert_eq!(headers[header::CONTENT_TYPE], "text/plain; charset=utf-8");
    assert_eq!(body, expected.as_bytes());

    let head = Request::head(uri).body(Body::empty()).unwrap();
    let (status, headers, body) = send(&app.app, head).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(headers[header::ETAG], tag);
    assert_eq!(headers[header::CONTENT_LENGTH], expected.len().to_string());
    assert!(body.is_empty());

    for (method, validator) in [
        ("GET", tag.clone()),
        ("GET", format!("\"other,tag\", W/{tag}")),
        ("HEAD", "*".into()),
    ] {
        let request = Request::builder()
            .method(method)
            .uri(uri)
            .header(header::IF_NONE_MATCH, validator)
            .body(Body::empty())
            .unwrap();
        let (status, headers, body) = send(&app.app, request).await;
        assert_eq!(status, StatusCode::NOT_MODIFIED);
        assert_eq!(headers[header::ETAG], tag);
        assert_eq!(headers[header::CACHE_CONTROL], "public, no-cache");
        assert!(body.is_empty());
    }

    let multi = Request::get(uri)
        .header(header::IF_NONE_MATCH, "\"other\"")
        .header(header::IF_NONE_MATCH, &tag)
        .body(Body::empty())
        .unwrap();
    assert_eq!(send(&app.app, multi).await.0, StatusCode::NOT_MODIFIED);

    repo::set_yanked(&app.pool, first.package_id, &first.version, true)
        .await
        .unwrap()
        .unwrap();
    let stale = Request::get(uri)
        .header(header::IF_NONE_MATCH, &tag)
        .body(Body::empty())
        .unwrap();
    let (status, headers, body) = send(&app.app, stale).await;
    assert_eq!(status, StatusCode::OK);
    assert_ne!(headers[header::ETAG], tag);
    let body = String::from_utf8(body).unwrap();
    let mut records = body
        .lines()
        .map(|line| IndexRecord::from_line(line).unwrap());
    assert!(records.next().unwrap().yanked);
    assert!(!records.next().unwrap().yanked);
    assert!(records.next().is_none());

    let missing = Request::get("/api/v1/index/test-reader/missing")
        .header(header::IF_NONE_MATCH, "*")
        .body(Body::empty())
        .unwrap();
    assert_eq!(send(&app.app, missing).await.0, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn resolve_agrees_with_the_sparse_index_for_catalogue_rows() {
    use std::collections::BTreeMap;

    use aevia_registry::api::deps::RecordSource;
    use aevia_registry::api::deps::resolve_request;
    use aevia_resolve::SparseRecord;
    use aevia_resolve::sparse::parse_text;

    struct TextCatalogue(BTreeMap<String, Vec<SparseRecord>>);

    impl RecordSource for TextCatalogue {
        async fn records(
            &self,
            package: &str,
        ) -> aevia_registry_core::Result<Option<Vec<SparseRecord>>> {
            Ok(self.0.get(package).cloned())
        }
    }

    let Some(app) = start().await else {
        return;
    };
    let root = "test-reader/archive_fixture";
    let dependency = "test-reader/units_fixture";
    let checksum = |byte: &str| format!("sha256:{}", byte.repeat(64));
    let fallback = catalogue_version(&app, "1.0.0", &checksum("a"), 100, false).await;
    let superseded = catalogue_version(&app, "1.1.0+local", &checksum("b"), 100, false).await;
    // The DB sees a distinct spelling of the same parsed version. The newer
    // row wins even when the timestamps tie and that row is yanked.
    let preferred = catalogue_version(&app, " 1.1.0+local ", &checksum("c"), 100, false).await;
    let owner = preferred.published_by.unwrap();
    let units = repo::create_package(&app.pool, owner, "units_fixture")
        .await
        .unwrap();
    repo::insert_version(
        &app.pool,
        &NewVersion {
            package_id: units.id,
            version: "2.0.0",
            checksum: &checksum("d"),
            size_bytes: 200,
            manifest: serde_json::json!({}),
            dependencies: serde_json::json!([]),
            readme: None,
            published_by: owner,
            key_kind: "openpgp",
            fingerprint: "fixture",
            signature: "fixture signature",
            statement: "fixture statement\n",
            build_docs: false,
        },
    )
    .await
    .unwrap();
    let time = chrono::DateTime::from_timestamp(1_700_000_000, 0).unwrap();
    sqlx::query("UPDATE versions SET created_at = $1 WHERE package_id = $2")
        .bind(time)
        .bind(preferred.package_id)
        .execute(&app.pool)
        .await
        .unwrap();
    sqlx::query("UPDATE versions SET yanked = true, dependencies = $1 WHERE id = $2")
        .bind(serde_json::json!([
            {"import":"Legacy", "path":"../legacy"},
            {"package":"TEST-READER/Units_Fixture", "req":"^2", "import":"UnitsAlias"}
        ]))
        .bind(preferred.id)
        .execute(&app.pool)
        .await
        .unwrap();

    let mut texts = BTreeMap::new();
    for package in [root, dependency] {
        let (status, _, body) = get(&app.app, &format!("/api/v1/index/{package}")).await;
        assert_eq!(status, StatusCode::OK);
        texts.insert(
            package.to_string(),
            parse_text(std::str::from_utf8(&body).unwrap()).unwrap(),
        );
    }
    let source = TextCatalogue(texts);
    for locked in [
        serde_json::json!([]),
        serde_json::json!([{"package":root, "version":"1.1.0+local"}]),
    ] {
        let request = serde_json::json!({
            "dependencies": [{"package":"TEST-READER/Archive_Fixture", "req":"^1"}],
            "locked": locked,
        });
        let expected = resolve_request(&source, &serde_json::from_value(request.clone()).unwrap())
            .await
            .unwrap();
        let (status, _, body) = post_json(&app.app, "/api/v1/resolve", &request.to_string()).await;
        assert_eq!(status, StatusCode::OK, "{}", String::from_utf8_lossy(&body));
        let body: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(body, serde_json::to_value(expected).unwrap());
        if locked.as_array().unwrap().is_empty() {
            assert_eq!(body["packages"][0]["checksum"], fallback.checksum);
        } else {
            assert_eq!(body["packages"][0]["checksum"], preferred.checksum);
            assert_eq!(body["packages"][0]["yanked"], true);
            assert!(
                body["edges"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .any(|edge| edge["import"] == "UnitsAlias")
            );
        }
    }

    // A malformed stored requirement remains an error even for a row whose
    // parsed version is replaced by a later valid row.
    sqlx::query("UPDATE versions SET dependencies = $1 WHERE id = $2")
        .bind(serde_json::json!([{"package":"test-reader/units_fixture", "req":"invalid"}]))
        .bind(superseded.id)
        .execute(&app.pool)
        .await
        .unwrap();
    let (_, _, index) = get(&app.app, &format!("/api/v1/index/{root}")).await;
    let error = parse_text(std::str::from_utf8(&index).unwrap()).unwrap_err();
    assert_eq!(error.line, 2);
    let (status, _, body) = post_json(
        &app.app,
        "/api/v1/resolve",
        &serde_json::json!({"dependencies":[{"package":root, "req":"^1"}]}).to_string(),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    let body: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(body["error"]["code"], "E_JSON");
    assert_eq!(
        body["error"]["message"],
        format!("a stored record is damaged: {error}")
    );
}

#[tokio::test]
async fn archive_validators_and_head_do_not_inflate_download_counts() {
    let Some(app) = start().await else {
        return;
    };
    let version = "1.0.0";
    let (_dir, archive, checksum) = fixture("archive_fixture", version);
    app.state.storage.write(&checksum, &archive).unwrap();
    // A legacy row can have a stale catalogue size. Serve the verified file size.
    let entry = catalogue_version(&app, version, &checksum, 0, false).await;
    let uri = "/api/v1/download/test-reader/archive_fixture/1.0.0";
    let etag = format!("\"{checksum}\"");

    let head = Request::builder()
        .method("HEAD")
        .uri(uri)
        .body(Body::empty())
        .unwrap();
    let (status, headers, body) = send(&app.app, head).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(headers[header::ETAG], etag);
    assert_eq!(headers[header::CONTENT_LENGTH], archive.len().to_string());
    assert_eq!(headers["x-aevia-checksum"], checksum);
    assert_eq!(headers["x-aevia-signature-fingerprint"], entry.fingerprint);
    assert!(body.is_empty());

    for validator in [etag.clone(), format!("\"different\", W/{etag}"), "*".into()] {
        let request = Request::builder()
            .uri(uri)
            .header(header::IF_NONE_MATCH, validator)
            .body(Body::empty())
            .unwrap();
        let (status, headers, body) = send(&app.app, request).await;
        assert_eq!(status, StatusCode::NOT_MODIFIED);
        assert_eq!(headers[header::ETAG], etag);
        assert_eq!(
            headers[header::CACHE_CONTROL],
            "public, max-age=31536000, immutable"
        );
        assert!(body.is_empty());
    }

    let request = Request::builder()
        .uri(uri)
        .header(header::IF_NONE_MATCH, "\"different\"")
        .body(Body::empty())
        .unwrap();
    let (status, headers, body) = send(&app.app, request).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(headers[header::ETAG], etag);
    assert_eq!(headers[header::CONTENT_LENGTH], archive.len().to_string());
    assert_eq!(body, archive);
    let (status, _, signature) = get(&app.app, &format!("{uri}/signature")).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(signature, b"fixture statement\nfixture signature");

    // Start the worker after all requests so the complete batch is observed.
    let worker = aevia_registry::spawn_download_counts(app.state.clone());
    let downloads = tokio::time::timeout(std::time::Duration::from_secs(5), async {
        loop {
            let count = repo::package_downloads(&app.pool, entry.package_id)
                .await
                .unwrap();
            if count != 0 {
                break count;
            }
            tokio::time::sleep(std::time::Duration::from_millis(25)).await;
        }
    })
    .await;
    worker.abort();
    assert_eq!(downloads.expect("the download worker flushes its batch"), 1);
}

#[tokio::test]
async fn archive_get_and_head_require_present_verified_bytes() {
    let Some(app) = start().await else {
        return;
    };
    let good = tarball::checksum(b"signed archive");
    let missing = tarball::checksum(b"absent archive");
    catalogue_version(&app, "1.0.0", &missing, 0, false).await;
    catalogue_version(&app, "1.0.1", &good, 0, false).await;
    app.state.storage.write(&good, b"tampered archive").unwrap();

    for (version, code) in [("1.0.0", "E_IO"), ("1.0.1", "E_INTEGRITY")] {
        let uri = format!("/api/v1/download/test-reader/archive_fixture/{version}");
        let (status, headers, body) = get(&app.app, &uri).await;
        assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR);
        assert_eq!(error_code(&body), code);
        assert!(!headers.contains_key("x-aevia-checksum"));

        let head = Request::head(&uri).body(Body::empty()).unwrap();
        let (status, headers, body) = send(&app.app, head).await;
        assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR);
        assert!(!headers.contains_key("x-aevia-checksum"));
        assert!(body.is_empty());
    }
}

#[tokio::test]
async fn a_failed_documentation_job_does_not_block_the_next_version() {
    let Some(app) = start_with_docs(true).await else {
        return;
    };
    let missing = tarball::checksum(b"archive absent from storage");
    let failed = catalogue_version(&app, "1.0.0", &missing, 0, true).await;
    let version = "1.0.1";
    let (_dir, archive, checksum) = fixture("archive_fixture", version);
    app.state.storage.write(&checksum, &archive).unwrap();
    let valid = catalogue_version(&app, version, &checksum, archive.len(), true).await;
    assert_eq!(failed.package_id, valid.package_id);

    let failed_uri = "/docs/test-reader/archive_fixture/1.0.0?lang=zh";
    let valid_uri = "/docs/test-reader/archive_fixture/1.0.1?lang=en";
    tokio::time::timeout(std::time::Duration::from_secs(15), async {
        loop {
            let (status, _, body) = get(&app.app, valid_uri).await;
            if status == StatusCode::OK {
                assert!(String::from_utf8_lossy(&body).contains("archive_fixture"));
                break;
            }
            assert_eq!(
                status,
                StatusCode::ACCEPTED,
                "{}",
                String::from_utf8_lossy(&body)
            );
            tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        }
    })
    .await
    .expect("the documentation worker reaches the valid release");
    let (status, headers, body) = get(&app.app, failed_uri).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_eq!(headers[header::CONTENT_LANGUAGE], "zh");
    let body = String::from_utf8_lossy(&body);
    assert!(body.contains("此版本的文档生成失败"), "{body}");
    assert!(!body.contains("archive absent from storage"));
    let failed = repo::version(&app.pool, failed.package_id, "1.0.0")
        .await
        .unwrap()
        .unwrap();
    assert_eq!(failed.docs_status, "failed");
    assert!(failed.docs_error.is_some());
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

#[tokio::test]
async fn discovery_uses_the_public_origin_and_published_editions() {
    let Some(app) = start().await else {
        return;
    };
    let (status, headers, body) = send(
        &app.app,
        Request::builder()
            .uri("/book/01-start?lang=zh&tracking=example")
            .header(header::HOST, "outside.invalid")
            .body(Body::empty())
            .unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let links: Vec<_> = headers
        .get_all(header::LINK)
        .iter()
        .map(|value| value.to_str().unwrap())
        .collect();
    assert!(links.contains(&"<http://127.0.0.1:8080/book/01-start?lang=zh>; rel=canonical"));
    assert!(links.iter().any(|line| line.contains("hreflang=en")));
    assert!(
        links
            .iter()
            .any(|line| line.contains("01-start.md?lang=zh"))
    );
    assert!(String::from_utf8_lossy(&body).contains("name=\"description\""));

    let (status, headers, body) = get(&app.app, "/book/01-start.md?lang=zh").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        headers[header::CONTENT_TYPE],
        "text/markdown; charset=utf-8"
    );
    assert_eq!(headers[header::CONTENT_LANGUAGE], "zh");
    assert_eq!(headers["x-robots-tag"], "noindex");
    assert!(String::from_utf8_lossy(&body).starts_with("# 测试章节"));
    assert_eq!(
        get(&app.app, "/book/not-published.md").await.0,
        StatusCode::NOT_FOUND
    );

    let (_, _, body) = get(&app.app, "/llms.txt?lang=en").await;
    let body = String::from_utf8(body).unwrap();
    assert!(body.contains("/book/01-start.md?lang=en"));
    assert!(body.contains("/api/v1/search"));
    assert!(!body.contains("/architecture/"));
    let (_, _, sitemap) = get(&app.app, "/sitemap.xml").await;
    let sitemap = String::from_utf8(sitemap).unwrap();
    assert!(sitemap.contains("/book/01-start?lang=en"));
    assert!(sitemap.contains("/book/01-start?lang=zh"));
    assert!(!sitemap.contains("/architecture"));
    assert_eq!(get(&app.app, "/login").await.1["x-robots-tag"], "noindex");
    assert_eq!(
        get(&app.app, "/stdlib/catalog.json").await.0,
        StatusCode::NOT_FOUND
    );
}

#[tokio::test]
async fn reload_preserves_previous_complete_editions_and_recovers() {
    let Some(app) = start().await else {
        return;
    };
    let root = &app.state.config.spec_dir;
    let english = root.join("en");
    std::fs::create_dir_all(&english).unwrap();
    let title = root.join("00-overview.md");
    let translation = english.join("00-overview.md");
    std::fs::write(&title, "# 概述\n\n原始版。\n").unwrap();
    std::fs::write(&translation, "# Overview\n\nFirst edition.\n").unwrap();
    assert_eq!(app.state.reload_spec().await.unwrap(), 1);
    std::fs::remove_file(&translation).unwrap();
    assert!(app.state.reload_spec().await.is_err());
    let (_, _, old) = get(&app.app, "/spec/00-overview.md?lang=en").await;
    assert!(String::from_utf8_lossy(&old).contains("First edition."));
    std::fs::write(&translation, "# Overview\n\nRevised edition.\n").unwrap();
    assert_eq!(app.state.reload_spec().await.unwrap(), 1);
    let (_, _, revised) = get(&app.app, "/spec/00-overview.md?lang=en").await;
    assert!(String::from_utf8_lossy(&revised).contains("Revised edition."));

    let bundle = app.state.config.stdlib_docs_dir.join("stdlib-docs.json");
    let raw = br#"{"modules":[{"path":"Prelude","doc":"English","decls":[]}],"zh_modules":[{"path":"Prelude","doc":"Chinese","decls":[]}]}"#;
    std::fs::write(&bundle, raw).unwrap();
    assert_eq!(app.state.reload_stdlib_docs().await.unwrap(), 1);
    std::fs::write(&bundle, "{").unwrap();
    assert!(app.state.reload_stdlib_docs().await.is_err());
    let (status, headers, body) = get(&app.app, "/stdlib/catalog.json").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(headers[header::CONTENT_TYPE], "application/json");
    assert_eq!(&body[..], raw);
    let (_, headers, _) = get(&app.app, "/stdlib/prelude?lang=zh").await;
    assert_eq!(
        headers[header::LINK],
        "<http://127.0.0.1:8080/stdlib/Prelude?lang=zh>; rel=canonical"
    );
}

#[tokio::test]
async fn manuals_serve_localized_chapters_and_a_standalone_download() {
    let Some(app) = start().await else {
        return;
    };
    let (status, _, contents) = get(&app.app, "/book?lang=zh").await;
    assert_eq!(status, StatusCode::OK);
    assert!(String::from_utf8_lossy(&contents).contains("测试章节"));

    let (_, headers, chapter) = get(&app.app, "/book/01-start?lang=zh").await;
    assert_eq!(headers[header::CONTENT_LANGUAGE], "zh");
    assert!(
        !headers[header::CONTENT_SECURITY_POLICY]
            .to_str()
            .unwrap()
            .contains("unsafe-inline")
    );
    assert!(String::from_utf8_lossy(&chapter).contains("编写第一个程序"));
    assert!(String::from_utf8_lossy(&chapter).contains("/static/app.js"));

    let (status, headers, offline) = get(&app.app, "/book/download?lang=en").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        headers[header::CONTENT_DISPOSITION],
        "attachment; filename=\"aevia-book-en.html\""
    );
    let offline = String::from_utf8_lossy(&offline);
    assert!(offline.contains("section-01-start-start"));
    assert!(offline.contains("<style>"));
    assert!(!offline.contains("/static/app.js"));

    let (status, _, _) = post_json(
        &app.app,
        "/api/v1/playground/run",
        "{\"source\":\"let x = 1\"}",
    )
    .await;
    assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE);

    let (status, _, _) = send(
        &app.app,
        Request::builder()
            .method("POST")
            .uri("/logout")
            .header(header::ORIGIN, "https://other.example")
            .body(Body::empty())
            .unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn publish_enforces_required_metadata_and_accepts_license_file() {
    let Some(app) = start().await else {
        return;
    };
    let Some(signer) = Signer::detect() else {
        return;
    };
    let fingerprint = SignerKey::detect(&signer.armored)
        .expect("the exported key parses")
        .fingerprint()
        .expect("the exported key has a fingerprint");
    seed_publisher(&app.pool, "tester", &fingerprint, &signer.armored).await;

    // Missing `description` and `license` / `license-file` must fail cleanly.
    let (_dir, archive, checksum) = fixture_with_manifest(
        "incomplete",
        "0.1.0",
        "description = \"\"\nlicense = \"\"\n",
    );
    let (status, body) = challenge(&app.app, "tester/incomplete", "0.1.0", &checksum).await;
    assert_eq!(status, StatusCode::OK, "{}", String::from_utf8_lossy(&body));
    let issued: serde_json::Value = serde_json::from_slice(&body).expect("challenge");
    let statement = issued["statement"].as_str().expect("statement").to_string();
    let envelope = serde_json::json!({
        "statement": statement,
        "signature": signer.sign(&statement),
        "key": { "kind": "openpgp", "armored": signer.armored },
    })
    .to_string();
    let (content_type, body) = multipart(&envelope, &archive);
    let (status, _, answer) =
        post_multipart(&app.app, "/api/v1/publish", &content_type, body).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(error_code(&answer), "E_MANIFEST");

    // Providing `description` + `license-file` present in the archive succeeds.
    let dir = tempfile::tempdir().expect("tempdir");
    std::fs::create_dir_all(dir.path().join("src")).expect("src/");
    std::fs::write(
        dir.path().join("aevia.toml"),
        "[package]\nname = \"with_license_file\"\nversion = \"0.1.0\"\ndescription = \"Custom licensed crate\"\nlicense-file = \"LICENCE-CUSTOM\"\n",
    )
    .expect("aevia.toml");
    std::fs::write(
        dir.path().join("src/main.aev"),
        "fn main(): IO Unit = do\n    println(\"ok\")\n",
    )
    .expect("main.aev");
    std::fs::write(dir.path().join("LICENCE-CUSTOM"), "Custom Terms v1").expect("license file");
    let manifest = PackageManifest::load(dir.path()).expect("manifest parses");
    let archive = tarball::pack_dir(dir.path(), &manifest).expect("packs");
    let checksum = tarball::checksum(&archive);

    let (status, body) = challenge(&app.app, "tester/with_license_file", "0.1.0", &checksum).await;
    assert_eq!(status, StatusCode::OK, "{}", String::from_utf8_lossy(&body));
    let issued: serde_json::Value = serde_json::from_slice(&body).expect("challenge");
    let statement = issued["statement"].as_str().expect("statement").to_string();
    let envelope = serde_json::json!({
        "statement": statement,
        "signature": signer.sign(&statement),
        "key": { "kind": "openpgp", "armored": signer.armored },
    })
    .to_string();
    let (content_type, body) = multipart(&envelope, &archive);
    let (status, _, answer) =
        post_multipart(&app.app, "/api/v1/publish", &content_type, body).await;
    assert_eq!(
        status,
        StatusCode::CREATED,
        "{}",
        String::from_utf8_lossy(&answer)
    );
}

#[tokio::test]
async fn ssh_keys_register_publish_yank_and_emit_pgp_warnings() {
    let Some(app) = start_with_docs(true).await else {
        return;
    };

    // Refuse non-ed25519 SSH keys cleanly on device start.
    let (status, _, body) = post_json(
        &app.app,
        "/api/v1/auth/device",
        &serde_json::json!({
            "kind": "ssh",
            "public_key": "ssh-rsa AAAAB3NzaC1yc2EAAAADAQABAAABAQ== tester@host",
            "label": "rsa",
        })
        .to_string(),
    )
    .await;
    assert_eq!(
        status,
        StatusCode::FORBIDDEN,
        "{}",
        String::from_utf8_lossy(&body)
    );
    assert_eq!(error_code(&body), "E_KEY");

    // Generate a real temporary ssh-ed25519 keypair with `ssh-keygen`.
    let keydir = tempfile::tempdir().expect("tempdir");
    let key_path = keydir.path().join("id_ed25519");
    let keygen = Command::new("ssh-keygen")
        .args(["-q", "-t", "ed25519", "-N", "", "-C", "tester@aevia", "-f"])
        .arg(&key_path)
        .output();
    let Ok(output) = keygen else {
        assert!(
            std::env::var_os("AEVIA_REQUIRE_TEST_FIXTURES").is_none(),
            "ssh-keygen is required by AEVIA_REQUIRE_TEST_FIXTURES"
        );
        eprintln!("skipping ssh publish test: `ssh-keygen` not available");
        return;
    };
    assert!(
        output.status.success(),
        "`ssh-keygen` failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let pub_key = std::fs::read_to_string(keydir.path().join("id_ed25519.pub"))
        .expect("read ssh pubkey")
        .trim()
        .to_string();

    // Registering an ssh-ed25519 key succeeds and warns to prefer OpenPGP / hardware keys.
    let (status, _, body) = post_json(
        &app.app,
        "/api/v1/auth/device",
        &serde_json::json!({
            "kind": "ssh",
            "public_key": pub_key,
            "label": "ed25519 laptop",
        })
        .to_string(),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{}", String::from_utf8_lossy(&body));
    let device: serde_json::Value = serde_json::from_slice(&body).expect("device JSON");
    let device_warnings = device["warnings"].as_array().expect("device warnings");
    assert!(
        device_warnings
            .iter()
            .filter_map(|v| v.as_str())
            .any(|w| w.contains("OpenPGP") && w.contains("hardware")),
        "expected OpenPGP/hardware warning on SSH device start, got: {device}"
    );

    // Seed the publisher and SSH signing key directly (mirroring browser approval).
    let parsed_key = SignerKey::detect(&pub_key).expect("ssh pubkey parses");
    assert_eq!(
        parsed_key.kind,
        aevia_registry_core::signature::KeyKind::Ssh
    );
    let fingerprint = parsed_key.fingerprint().expect("ssh fingerprint");
    let publisher_id: i64 = sqlx::query_scalar(
        "INSERT INTO publishers (kind, handle, display_name)
         VALUES ('github', 'sshuser', 'SSH Publisher')
         RETURNING id",
    )
    .fetch_one(&app.pool)
    .await
    .expect("seed publisher");
    sqlx::query(
        "INSERT INTO signing_keys (publisher_id, kind, fingerprint, public_key, label)
         VALUES ($1, 'ssh', $2, $3, 'ed25519 laptop')",
    )
    .bind(publisher_id)
    .bind(&fingerprint)
    .bind(&pub_key)
    .execute(&app.pool)
    .await
    .expect("seed ssh key");

    let sign_with_ssh = |statement: &str, name: &str| -> String {
        let msg_path = keydir.path().join(name);
        std::fs::write(&msg_path, statement).expect("write statement");
        let sig_out = Command::new("ssh-keygen")
            .args(["-Y", "sign", "-f"])
            .arg(&key_path)
            .args(["-n", aevia_registry_core::signature::SSH_NAMESPACE])
            .arg(&msg_path)
            .output()
            .expect("run ssh-keygen -Y sign");
        assert!(
            sig_out.status.success(),
            "`ssh-keygen -Y sign` failed: {}",
            String::from_utf8_lossy(&sig_out.stderr)
        );
        std::fs::read_to_string(keydir.path().join(format!("{name}.sig"))).expect("read .sig")
    };

    // Publish with an SSH signature succeeds and emits the OpenPGP/hardware warning.
    let (_pkg_dir, archive, checksum) = fixture("ssh_demo", "0.1.0");
    let (status, body) = challenge(&app.app, "sshuser/ssh_demo", "0.1.0", &checksum).await;
    assert_eq!(status, StatusCode::OK, "{}", String::from_utf8_lossy(&body));
    let issued: serde_json::Value = serde_json::from_slice(&body).expect("challenge JSON");
    let statement = issued["statement"].as_str().expect("statement").to_string();
    let envelope = serde_json::json!({
        "statement": statement,
        "signature": sign_with_ssh(&statement, "publish.stmt"),
        "key": { "kind": "ssh", "armored": pub_key },
    })
    .to_string();
    let (content_type, body) = multipart(&envelope, &archive);
    let (status, _, answer) =
        post_multipart(&app.app, "/api/v1/publish", &content_type, body).await;
    assert_eq!(
        status,
        StatusCode::CREATED,
        "{}",
        String::from_utf8_lossy(&answer)
    );
    let published: serde_json::Value = serde_json::from_slice(&answer).expect("publish JSON");
    let docs_url = published["docs_url"]
        .as_str()
        .expect("the release has a docs URL");
    tokio::time::timeout(std::time::Duration::from_secs(10), async {
        loop {
            let (status, _, body) = get(&app.app, docs_url).await;
            if status == StatusCode::OK {
                assert!(String::from_utf8_lossy(&body).contains("ssh_demo"));
                break;
            }
            assert_eq!(
                status,
                StatusCode::ACCEPTED,
                "{}",
                String::from_utf8_lossy(&body)
            );
            tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        }
    })
    .await
    .expect("the documentation worker completes the pending job");
    let (status, headers, _) = get_with_cookie(
        &app.app,
        "/docs/sshuser/ssh_demo?lang=zh&unrelated=ignored",
        "aevia_lang=en",
    )
    .await;
    assert_eq!(status, StatusCode::SEE_OTHER);
    let localized = format!("{docs_url}?lang=zh");
    assert_eq!(
        headers
            .get(header::LOCATION)
            .and_then(|value| value.to_str().ok()),
        Some(localized.as_str())
    );
    let (status, headers, _) = get_with_cookie(&app.app, &localized, "aevia_lang=en").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        headers
            .get(header::CONTENT_LANGUAGE)
            .and_then(|value| value.to_str().ok()),
        Some("zh")
    );
    let publish_warnings = published["warnings"].as_array().expect("publish warnings");
    assert!(
        publish_warnings
            .iter()
            .filter_map(|v| v.as_str())
            .any(|w| w.contains("OpenPGP") && w.contains("hardware")),
        "expected OpenPGP/hardware warning on SSH publish, got: {published}"
    );

    // Download records the SSH fingerprint in its header.
    let (status, headers, _) = get(&app.app, "/api/v1/download/sshuser/ssh_demo/0.1.0").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        headers
            .get("x-aevia-signature-fingerprint")
            .and_then(|value| value.to_str().ok()),
        Some(fingerprint.as_str())
    );

    // Yank signed with the same SSH key succeeds.
    let (status, _, body) = post_json(
        &app.app,
        "/api/v1/yank/challenge",
        &serde_json::json!({
            "name": "sshuser/ssh_demo",
            "version": "0.1.0",
            "yanked": true,
        })
        .to_string(),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{}", String::from_utf8_lossy(&body));
    let issued: serde_json::Value = serde_json::from_slice(&body).expect("yank challenge");
    let statement = issued["statement"]
        .as_str()
        .expect("yank statement")
        .to_string();
    let (status, _, body) = post_json(
        &app.app,
        "/api/v1/yank",
        &serde_json::json!({
            "statement": statement,
            "signature": sign_with_ssh(&statement, "yank.stmt"),
            "key": { "kind": "ssh", "armored": pub_key },
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
}

async fn transparency_page(
    app: &Router,
    uri: &str,
) -> serde_json::Value {
    let (status, _, body) = get(app, uri).await;
    assert_eq!(status, StatusCode::OK, "{}", String::from_utf8_lossy(&body));
    serde_json::from_slice(&body).expect("the log page is JSON")
}

#[tokio::test]
async fn transparency_log_routes_keep_head_projection_and_page_bounds() {
    let Some(app) = start().await else {
        return;
    };
    assert_eq!(
        transparency_page(&app.app, "/api/v1/log").await,
        serde_json::json!({"head": null, "entries": []}),
    );
    let payload = serde_json::json!({"private_projection_marker": "only stored with the chain"});
    let first = repo::append_log(&app.pool, "publish", "example 1.0.0", payload.clone())
        .await
        .unwrap();
    let newest = repo::append_log(
        &app.pool,
        "yank",
        "example 1.0.0 withdrawn",
        payload.clone(),
    )
    .await
    .unwrap();
    let full = transparency_page(&app.app, "/api/v1/log").await;
    assert_eq!(full["entries"].as_array().unwrap().len(), 2);
    assert_eq!(full["entries"][1]["prev"], first.entry_hash);
    assert_eq!(full["entries"][1]["hash"], newest.entry_hash);

    let head = serde_json::json!({
        "seq": newest.seq,
        "hash": newest.entry_hash,
        "at": newest.created_at.to_rfc3339(),
    });
    let first_summary = serde_json::json!({
        "seq": first.seq,
        "kind": first.kind,
        "summary": first.summary,
        "hash": first.entry_hash,
        "prev": first.prev_hash,
        "at": first.created_at.to_rfc3339(),
    });
    let clamped = transparency_page(&app.app, "/api/v1/log?from=-5&limit=0").await;
    assert_eq!(
        clamped,
        serde_json::json!({"head": head, "entries": [first_summary]}),
    );
    let inclusive_uri = format!("/api/v1/log?from={}", newest.seq);
    let inclusive = transparency_page(&app.app, &inclusive_uri).await;
    assert_eq!(inclusive["entries"].as_array().unwrap().len(), 1);
    assert_eq!(inclusive["entries"][0]["seq"], newest.seq);
    let empty_uri = format!("/api/v1/log?from={}", newest.seq.saturating_add(1));
    let empty = transparency_page(&app.app, &empty_uri).await;
    assert_eq!(empty, serde_json::json!({"head": head, "entries": []}));

    let (status, _, html) = get(&app.app, "/log").await;
    assert_eq!(status, StatusCode::OK);
    let html = String::from_utf8(html).unwrap();
    assert!(html.contains("example 1.0.0 withdrawn"));
    assert!(html.contains(&newest.entry_hash[..32]));
    assert!(!html.contains("private_projection_marker"));
    let legacy = repo::log_entries(&app.pool, first.seq, 1).await.unwrap();
    assert_eq!(legacy.first().unwrap().payload, payload);
}

#[tokio::test]
async fn transparency_log_head_matches_complete_pages_while_writes_commit() {
    let Some(app) = start().await else {
        return;
    };
    repo::append_log(&app.pool, "publish", "first", serde_json::json!({}))
        .await
        .unwrap();
    let old_page = repo::log_entries(&app.pool, 0, 500).await.unwrap();
    let second = repo::append_log(&app.pool, "publish", "second", serde_json::json!({}))
        .await
        .unwrap();
    let old_head = repo::log_head(&app.pool).await.unwrap().unwrap();
    assert_eq!(old_head.seq, second.seq);
    assert!(old_page.last().unwrap().seq < old_head.seq);

    let pool = app.pool.clone();
    let writer = tokio::spawn(async move {
        for sequence in 0..32 {
            let summary = format!("concurrent entry {sequence}");
            repo::append_log(
                &pool,
                "publish",
                &summary,
                serde_json::json!({"n": sequence}),
            )
            .await
            .unwrap();
            tokio::task::yield_now().await;
        }
    });
    for _ in 0..64 {
        let page = transparency_page(&app.app, "/api/v1/log?limit=500").await;
        let entries = page["entries"].as_array().unwrap();
        let latest = entries.last().unwrap();
        assert_eq!(page["head"]["seq"], latest["seq"]);
        assert_eq!(page["head"]["hash"], latest["hash"]);
        assert_eq!(page["head"]["at"], latest["at"]);
    }
    writer.await.unwrap();
}
