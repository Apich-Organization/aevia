//! Search and package detail, as JSON.
//!
//! These are the same view models the HTML pages use, which is the point: a
//! tool that wants to know what `apich/units-extra` contains gets exactly what
//! the browser sees, and there is no second serialisation to keep in step.

use axum::Json;
use axum::extract::Path;
use axum::extract::Query;
use axum::extract::State;
use axum::http::HeaderMap;
use serde::Deserialize;

use aevia_registry_core::PackageId;
use aevia_registry_core::vm::CrateCard;
use aevia_registry_core::vm::CrateList;
use aevia_registry_core::vm::CratePage;
use aevia_registry_db::repo;

use crate::api::ApiError;
use crate::api::ApiResult;
use crate::pages::build_crate_page;
use crate::state::AppState;

/// The query of `/api/v1/search`.
#[derive(Deserialize)]
pub struct SearchQuery {
    /// The search text.
    pub q: Option<String>,
    /// `relevance`, `downloads`, `recent` or `name`.
    pub sort: Option<String>,
    /// The page, one-based.
    pub page: Option<i64>,
}

/// `GET /api/v1/search`
///
/// # Errors
///
/// `E_DB` when the query fails.
pub async fn search(
    State(state): State<AppState>,
    Query(query): Query<SearchQuery>,
) -> ApiResult<Json<CrateList>> {
    let search = query.q.unwrap_or_default();
    let sort = match query.sort.as_deref() {
        | Some("downloads" | "recent" | "name" | "relevance") => {
            query.sort.unwrap_or_else(|| "recent".to_string())
        },
        | _ => "recent".to_string(),
    };
    let page = query.page.unwrap_or(1).max(1);
    let per_page = 20;
    let (items, total) =
        repo::list_packages(&state.pool, &search, &sort, per_page, (page - 1) * per_page).await?;
    let items = items
        .iter()
        .filter_map(|item| {
            let id = PackageId::new(&item.owner_handle, &item.name).ok()?;
            Some(CrateCard {
                id,
                description: item.description.clone(),
                version: item
                    .latest_version
                    .clone()
                    .unwrap_or_else(|| "—".to_string()),
                updated_at: item.updated_at.to_rfc3339(),
                downloads: item.downloads,
                has_docs: item.has_docs,
            })
        })
        .collect();
    Ok(Json(CrateList {
        total,
        page,
        per_page,
        query: search,
        sort,
        items,
    }))
}

/// `GET /api/v1/crates/{owner}/{name}`
///
/// # Errors
///
/// `404` when the package does not exist.
pub async fn detail(
    State(state): State<AppState>,
    Path((owner, name)): Path<(String, String)>,
    headers: HeaderMap,
) -> ApiResult<Json<CratePage>> {
    let page = build_crate_page(&state, &headers, &owner, &name, None)
        .await
        .map_err(|error| ApiError::new(error.status, "E_NOT_FOUND", error.message))?;
    Ok(Json(page))
}
