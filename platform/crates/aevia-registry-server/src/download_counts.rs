//! Bounded, batched download counters.

use std::collections::HashMap;
use std::sync::Mutex;
use std::time::Duration;

use crate::state::AppState;

const MAX_PENDING_VERSIONS: usize = 4096;

#[derive(Default)]
pub struct DownloadCounts(Mutex<HashMap<i64, i64>>);

impl DownloadCounts {
    pub(crate) fn record(
        &self,
        version: i64,
    ) {
        let mut counts = self
            .0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if let Some(count) = counts.get_mut(&version) {
            *count = count.saturating_add(1);
        } else if counts.len() < MAX_PENDING_VERSIONS {
            counts.insert(version, 1);
        }
    }

    fn take(&self) -> (Vec<i64>, Vec<i64>) {
        let counts = std::mem::take(
            &mut *self
                .0
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner),
        );
        counts.into_iter().unzip()
    }
}

/// Flushes download summaries without a database write on each request.
#[must_use]
pub fn spawn(state: AppState) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(Duration::from_secs(2));
        interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        loop {
            interval.tick().await;
            let (versions, counts) = state.download_counts.take();
            if !versions.is_empty()
                && let Err(error) = aevia_registry_db::repo::increment_download_batch(
                    &state.pool,
                    &versions,
                    &counts,
                )
                .await
            {
                tracing::warn!(%error, "cannot record buffered download counts");
            }
        }
    })
}
