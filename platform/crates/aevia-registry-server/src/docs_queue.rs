//! Builds pending package documentation from the signed archive store.

use std::time::Duration;

use aevia_registry_core::Error;
use aevia_registry_core::PackageId;
use aevia_registry_core::tarball;
use aevia_registry_db::models::DocsJob;
use aevia_registry_db::models::DocsPage;
use aevia_registry_db::repo;

use crate::state::AppState;
use crate::storage::Storage;

/// Starts one documentation consumer in this server process.
#[must_use]
pub fn spawn(state: AppState) -> Option<tokio::task::JoinHandle<()>> {
    if !state.config.docs_enabled {
        return None;
    }
    Some(tokio::spawn(async move {
        loop {
            match next(&state).await {
                | Ok(true) => continue,
                | Ok(false) => {},
                | Err(error) => tracing::warn!(%error, "cannot process the documentation queue"),
            }
            tokio::select! {
                () = state.docs_wake.notified() => {},
                () = tokio::time::sleep(Duration::from_secs(5)) => {},
            }
        }
    }))
}

#[allow(
    clippy::single_call_fn,
    reason = "one queue item has an independent database and storage lifecycle"
)]
async fn next(state: &AppState) -> Result<bool, Error> {
    let mut transaction = state.pool.begin().await.map_err(|error| {
        aevia_registry_db::sql_error("cannot begin the documentation job", error)
    })?;
    let Some(job) = repo::next_docs_job(&mut transaction).await? else {
        transaction.rollback().await.map_err(|error| {
            aevia_registry_db::sql_error("cannot end the empty documentation transaction", error)
        })?;
        return Ok(false);
    };
    let version_id = job.version_id;
    let storage = state.storage.clone();
    let outcome = tokio::task::spawn_blocking(move || build(&storage, &job))
        .await
        .map_err(|error| Error::new("E_DOCS", format!("documentation task failed: {error}")))
        .and_then(std::convert::identity);
    match outcome {
        | Ok(pages) => {
            repo::replace_docs_in(&mut transaction, version_id, &pages).await?;
            repo::set_docs_status(&mut *transaction, version_id, "built", None).await?;
        },
        | Err(error) => {
            tracing::warn!(version_id, %error, "package documentation could not be built");
            repo::set_docs_status(
                &mut *transaction,
                version_id,
                "failed",
                Some(&error.message),
            )
            .await?;
        },
    }
    transaction.commit().await.map_err(|error| {
        aevia_registry_db::sql_error("cannot complete the documentation job", error)
    })?;
    Ok(true)
}

#[allow(
    clippy::single_call_fn,
    reason = "documentation generation runs at a blocking task boundary"
)]
fn build(
    storage: &Storage,
    job: &DocsJob,
) -> Result<Vec<DocsPage>, Error> {
    let name = PackageId::new(&job.owner, &job.name)?;
    let bytes = storage.read(&name, &job.version, &job.checksum)?;
    let files = tarball::text_files(&bytes)?;
    let docs = aevia_registry_docs::build(&files, &name.to_string())?;
    Ok(docs
        .pages
        .into_iter()
        .map(|page| {
            DocsPage {
                version_id: job.version_id,
                path: page.path,
                title: page.title,
                html: page.html,
                decls: i32::try_from(page.decls).unwrap_or(i32::MAX),
                undocumented: i32::try_from(page.undocumented).unwrap_or(i32::MAX),
                source_file: page.source_file,
            }
        })
        .collect())
}
