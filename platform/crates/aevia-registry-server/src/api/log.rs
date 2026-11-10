//! The transparency log, as JSON.
//!
//! The log is a hash chain: each entry hashes its own payload with the previous
//! entry's hash. A client that stores a head hash and later re-reads the chain
//! can prove that nothing before that head was rewritten — which is what turns
//! "the registry says so" into "the registry cannot quietly say otherwise".

use axum::Json;
use axum::extract::Query;
use axum::extract::State;
use serde::Deserialize;
use serde::Serialize;

use aevia_registry_core::vm::LogEntryView;
use aevia_registry_db::repo;

use crate::api::ApiResult;
use crate::state::AppState;

/// The query of `/api/v1/log`.
#[derive(Deserialize)]
pub struct LogQuery {
    /// The first sequence number to return.
    pub from: Option<i64>,
    /// How many entries, at most 500.
    pub limit: Option<i64>,
}

/// The head of the chain.
#[derive(Serialize)]
pub struct Head {
    /// The newest sequence number.
    pub seq: i64,
    /// Its hash.
    pub hash: String,
    /// When it was appended.
    pub at: String,
}

/// A page of the chain.
#[derive(Serialize)]
pub struct LogPage {
    /// The head, when the chain is not empty.
    pub head: Option<Head>,
    /// The entries, oldest first.
    pub entries: Vec<LogEntryView>,
}

/// `GET /api/v1/log`
///
/// # Errors
///
/// `E_DB` when the chain cannot be read.
pub async fn entries(
    State(state): State<AppState>,
    Query(query): Query<LogQuery>,
) -> ApiResult<Json<LogPage>> {
    let from = query.from.unwrap_or(0).max(0);
    let limit = query.limit.unwrap_or(100).clamp(1, 500);
    let rows = repo::log_entries(&state.pool, from, limit).await?;
    let head = repo::log_head(&state.pool).await?.map(|entry| {
        Head {
            seq: entry.seq,
            hash: entry.entry_hash,
            at: entry.created_at.to_rfc3339(),
        }
    });
    Ok(Json(LogPage {
        head,
        entries: rows
            .iter()
            .map(|entry| {
                LogEntryView {
                    seq: entry.seq,
                    kind: entry.kind.clone(),
                    summary: entry.summary.clone(),
                    hash: entry.entry_hash.clone(),
                    prev: entry.prev_hash.clone(),
                    at: entry.created_at.to_rfc3339(),
                }
            })
            .collect(),
    }))
}
