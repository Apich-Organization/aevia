//! Real PostgreSQL tests for startup and migration session cleanup.
//!
//! Set `AEVIA_TEST_DATABASE_URL` to a PostgreSQL 18 instance where the test
//! account can create schemas. `AEVIA_REQUIRE_TEST_FIXTURES=1` requires a URL.

#![allow(clippy::expect_used)] // Setup failure fails the current test.

use std::sync::atomic::AtomicUsize;
use std::sync::atomic::Ordering;
use std::time::Duration;
use std::time::SystemTime;
use std::time::UNIX_EPOCH;

use sqlx::Connection as _;
use sqlx::PgPool;
use sqlx::postgres::PgConnectOptions;
use sqlx::postgres::PgConnection;
use sqlx::postgres::PgPoolOptions;

use aevia_registry_db::E_DB;
use aevia_registry_db::migrate;

const WAIT: Duration = Duration::from_secs(5);
static NEXT_SCHEMA: AtomicUsize = AtomicUsize::new(0);

struct Fixture {
    schema: String,
    observer: PgConnection,
    first: PgPool,
    second: PgPool,
}

impl Fixture {
    async fn open() -> Option<Self> {
        let url = std::env::var("AEVIA_TEST_DATABASE_URL").unwrap_or_default();
        if url.trim().is_empty() {
            assert!(
                std::env::var_os("AEVIA_REQUIRE_TEST_FIXTURES").is_none(),
                "AEVIA_TEST_DATABASE_URL is required by AEVIA_REQUIRE_TEST_FIXTURES"
            );
            eprintln!("skipping: set AEVIA_TEST_DATABASE_URL to test migration session cleanup");
            return None;
        }
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock")
            .as_nanos();
        let serial = NEXT_SCHEMA.fetch_add(1, Ordering::Relaxed);
        let schema = format!("aevia_migration_{}_{nonce}_{serial}", std::process::id());
        let options: PgConnectOptions = url.trim().parse().expect("PostgreSQL test URL");
        let mut observer = PgConnection::connect_with(&options)
            .await
            .expect("observer session");
        sqlx::query(&format!("CREATE SCHEMA {schema}"))
            .execute(&mut observer)
            .await
            .expect("create isolated test schema");
        let options = options.options([("search_path", &schema)]);
        let make_pool = || {
            PgPoolOptions::new()
                .max_connections(1)
                .min_connections(1)
                .acquire_timeout(WAIT)
        };
        let first = make_pool()
            .connect_with(options.clone())
            .await
            .expect("connect first pool");
        let second = make_pool()
            .connect_with(options)
            .await
            .expect("connect second pool");
        Some(Self {
            schema,
            observer,
            first,
            second,
        })
    }

    async fn close(mut self) {
        self.first.close().await;
        self.second.close().await;
        sqlx::query(&format!("DROP SCHEMA {} CASCADE", self.schema))
            .execute(&mut self.observer)
            .await
            .expect("drop isolated test schema");
    }
}

async fn backend(pool: &PgPool) -> i32 {
    sqlx::query_scalar("SELECT pg_backend_pid()")
        .fetch_one(pool)
        .await
        .expect("read pool's backend PID")
}

async fn replacement_backend(pool: &PgPool) -> i32 {
    tokio::time::timeout(WAIT, async {
        while pool.num_idle() == 0 {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("pool restores its minimum size without an acquire request");
    backend(pool).await
}

#[tokio::test]
async fn competing_migrations_reuse_healthy_sessions_and_release_a_failed_session() {
    let Some(fixture) = Fixture::open().await else {
        return;
    };
    let first_pid = backend(&fixture.first).await;
    let second_pid = backend(&fixture.second).await;
    tokio::time::timeout(WAIT, async {
        tokio::try_join!(migrate(&fixture.first), migrate(&fixture.second))
    })
    .await
    .expect("concurrent startup completes")
    .expect("both pools agree on the embedded migration history");
    assert_eq!(backend(&fixture.first).await, first_pid);
    assert_eq!(backend(&fixture.second).await, second_pid);

    let (version, checksum): (i64, Vec<u8>) =
        sqlx::query_as("SELECT version, checksum FROM _sqlx_migrations ORDER BY version LIMIT 1")
            .fetch_one(&fixture.second)
            .await
            .expect("read installed migration history");
    let replace_checksum = "UPDATE _sqlx_migrations SET checksum = $2 WHERE version = $1";
    sqlx::query(replace_checksum)
        .bind(version)
        .bind(Vec::<u8>::new())
        .execute(&fixture.second)
        .await
        .expect("damage the test migration checksum");
    let error = migrate(&fixture.first)
        .await
        .expect_err("migrations detect a modified checksum");
    sqlx::query(replace_checksum)
        .bind(version)
        .bind(checksum)
        .execute(&fixture.second)
        .await
        .expect("restore the original migration checksum");

    let retry = tokio::time::timeout(WAIT, migrate(&fixture.second)).await;
    let replacement_pid = replacement_backend(&fixture.first).await;
    fixture.close().await;
    assert_eq!(error.code, E_DB);
    assert!(
        error.message.contains("has been modified"),
        "{}",
        error.message
    );
    retry
        .expect("failure released the other pool's migration lock")
        .expect("a repaired history can migrate again");
    assert_ne!(first_pid, replacement_pid);
}

#[tokio::test]
async fn a_failed_script_rolls_back_before_another_pool_retries() {
    let Some(fixture) = Fixture::open().await else {
        return;
    };
    // The first embedded migration creates this table after `publishers`.
    sqlx::query("CREATE TABLE docs_pages (placeholder bigint)")
        .execute(&fixture.second)
        .await
        .expect("create a conflicting table in the test schema");
    let error = migrate(&fixture.first)
        .await
        .expect_err("the conflicting table makes migration fail");
    let rolled_back: bool = sqlx::query_scalar("SELECT to_regclass('publishers') IS NULL")
        .fetch_one(&fixture.second)
        .await
        .expect("check that earlier statements rolled back");
    sqlx::query("DROP TABLE docs_pages")
        .execute(&fixture.second)
        .await
        .expect("remove the test conflict");

    let retry = tokio::time::timeout(WAIT, migrate(&fixture.second)).await;
    fixture.close().await;
    assert_eq!(error.code, E_DB);
    assert!(error.message.contains("docs_pages"), "{}", error.message);
    assert!(rolled_back);
    retry
        .expect("script failure released the migration lock")
        .expect("a fresh session can apply the rolled-back schema");
}

#[tokio::test]
async fn cancellation_releases_the_migration_session_for_another_pool() {
    let Some(mut fixture) = Fixture::open().await else {
        return;
    };
    migrate(&fixture.first)
        .await
        .expect("install the embedded schema");
    let first_pid = backend(&fixture.first).await;
    let mut blocker = fixture
        .second
        .begin()
        .await
        .expect("start blocking transaction");
    sqlx::query("LOCK TABLE _sqlx_migrations IN ACCESS EXCLUSIVE MODE")
        .execute(&mut *blocker)
        .await
        .expect("pause migration after it takes its advisory lock");

    let mut attempt = Box::pin(migrate(&fixture.first));
    let wait_for_lock = async {
        loop {
            let held: bool = sqlx::query_scalar(
                "SELECT EXISTS (SELECT FROM pg_locks WHERE pid = $1 AND locktype = 'advisory' AND granted)",
            )
            .bind(first_pid)
            .fetch_one(&mut fixture.observer)
            .await
            .expect("observe the migration's advisory lock");
            if held {
                break;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    };
    tokio::select! {
        outcome = &mut attempt => panic!("migration completed before the table lock was released: {outcome:?}"),
        outcome = tokio::time::timeout(WAIT, wait_for_lock) => outcome.expect("migration reached its advisory lock"),
    }
    drop(attempt);
    blocker.rollback().await.expect("unblock migration query");

    let retry = tokio::time::timeout(WAIT, migrate(&fixture.second)).await;
    let replacement_pid = replacement_backend(&fixture.first).await;
    fixture.close().await;
    retry
        .expect("cancellation released the other pool's migration lock")
        .expect("an interrupted migration can be retried");
    assert_ne!(first_pid, replacement_pid);
}
