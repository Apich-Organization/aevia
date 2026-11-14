//! The HTML pages, and the view models they render.
//!
//! Every handler here does the same three things: read rows, turn them into the
//! view models from `aevia_registry_core::vm`, and hand them to a page function
//! in `aevia_registry_web`. No markup lives in this file, and no SQL lives in
//! the page functions — the boundary is the view model, which is why a page can
//! be unit-tested without a database and a query can be changed without
//! touching a template.

use std::collections::BTreeMap;

use axum::Form;
use axum::extract::Path;
use axum::extract::Query;
use axum::extract::State;
use axum::http::HeaderMap;
use axum::http::StatusCode;
use axum::http::header;
use axum::response::Html;
use axum::response::IntoResponse;
use axum::response::Redirect;
use axum::response::Response;
use chrono::Utc;
use serde::Deserialize;

use aevia_registry_core::PackageId;
use aevia_registry_core::vm::AccountPage;
use aevia_registry_core::vm::CrateCard;
use aevia_registry_core::vm::CrateList;
use aevia_registry_core::vm::CratePage;
use aevia_registry_core::vm::DependencyView;
use aevia_registry_core::vm::DependentView;
use aevia_registry_core::vm::DocModulePage;
use aevia_registry_core::vm::DocsIndexPage;
use aevia_registry_core::vm::KeyView;
use aevia_registry_core::vm::LogEntryView;
use aevia_registry_core::vm::ModuleLink;
use aevia_registry_core::vm::OwnerView;
use aevia_registry_core::vm::UserPage;
use aevia_registry_core::vm::VersionView;
use aevia_registry_db::models::PackageSummary;
use aevia_registry_db::models::Publisher;
use aevia_registry_db::models::Version;
use aevia_registry_db::repo;
use aevia_registry_web as web;

use crate::error::PageError;
use crate::error::PageResult;
use crate::http::Language;
use crate::state::AppState;

/// How many packages a list page shows.
const PER_PAGE: i64 = 20;

/// How many entries the log page shows.
const LOG_PAGE: i64 = 100;

/// Percent-encodes a value for a query string.
#[must_use]
pub fn urlencode(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    for byte in value.bytes() {
        match byte {
            | b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(char::from(byte));
            },
            | _ => out.push_str(&format!("%{byte:02X}")),
        }
    }
    out
}

/// `GET /`
///
/// # Errors
///
/// `E_DB` when the counts or the recent list cannot be read.
pub async fn home(
    State(state): State<AppState>,
    Language(lang): Language,
    headers: HeaderMap,
) -> PageResult<Html<String>> {
    let stats = repo::stats(&state.pool).await?;
    let (recent, _) = repo::list_packages(&state.pool, "", "recent", 6, 0).await?;
    let cards = cards_of(&recent);
    let viewer = state.viewer(&headers).await;
    Ok(Html(web::home(lang, &stats, &cards, viewer.as_ref())))
}

/// The query of `/crates`.
#[derive(Deserialize)]
pub struct ListQuery {
    /// The search text.
    pub q: Option<String>,
    /// `relevance`, `downloads`, `recent` or `name`.
    pub sort: Option<String>,
    /// The page, one-based.
    pub page: Option<i64>,
}

/// `GET /crates`
///
/// # Errors
///
/// `E_DB` when the list cannot be read.
pub async fn crates(
    State(state): State<AppState>,
    Language(lang): Language,
    Query(query): Query<ListQuery>,
    headers: HeaderMap,
) -> PageResult<Html<String>> {
    let search = query.q.unwrap_or_default();
    let sort = query.sort.unwrap_or_else(|| "recent".to_string());
    let sort = match sort.as_str() {
        | "downloads" | "recent" | "name" | "relevance" => sort,
        | _ => "recent".to_string(),
    };
    let page = query.page.unwrap_or(1).max(1);
    let (items, total) =
        repo::list_packages(&state.pool, &search, &sort, PER_PAGE, (page - 1) * PER_PAGE).await?;
    let list = CrateList {
        total,
        page,
        per_page: PER_PAGE,
        query: search,
        sort,
        items: cards_of(&items),
    };
    let viewer = state.viewer(&headers).await;
    Ok(Html(web::crate_list(lang, &list, viewer.as_ref())))
}

/// `GET /crates/{owner}/{name}`
///
/// # Errors
///
/// `404` when the package does not exist, `E_DB` when a read fails.
pub async fn crate_detail(
    State(state): State<AppState>,
    Language(lang): Language,
    Path((owner, name)): Path<(String, String)>,
    headers: HeaderMap,
) -> PageResult<Html<String>> {
    let page = build_crate_page(&state, &headers, &owner, &name, None).await?;
    Ok(Html(web::crate_page(
        lang,
        &page,
        state.viewer(&headers).await.as_ref(),
    )))
}

/// `GET /crates/{owner}/{name}/{version}`
///
/// # Errors
///
/// `404` when the package or the version does not exist.
pub async fn crate_version(
    State(state): State<AppState>,
    Language(lang): Language,
    Path((owner, name, version)): Path<(String, String, String)>,
    headers: HeaderMap,
) -> PageResult<Html<String>> {
    let page = build_crate_page(&state, &headers, &owner, &name, Some(version)).await?;
    Ok(Html(web::crate_page(
        lang,
        &page,
        state.viewer(&headers).await.as_ref(),
    )))
}

pub(crate) async fn build_crate_page(
    state: &AppState,
    headers: &HeaderMap,
    owner: &str,
    name: &str,
    version: Option<String>,
) -> PageResult<CratePage> {
    let id = PackageId::new(owner, name)?;
    let Some((package, owner_row)) =
        repo::package_by_handle(&state.pool, id.owner(), &id.name_key()).await?
    else {
        return Err(PageError::not_found(format!(
            "There is no package called {id}."
        )));
    };
    let versions = repo::versions(&state.pool, package.id).await?;
    let selected = match &version {
        | Some(version) => {
            versions
                .iter()
                .find(|row| &row.version == version)
                .cloned()
                .ok_or_else(|| PageError::not_found(format!("{id} has no version {version}.")))?
        },
        | None => {
            repo::latest_version(&state.pool, package.id)
                .await?
                .or_else(|| versions.first().cloned())
                .ok_or_else(|| PageError::not_found(format!("{id} has no published versions.")))?
        },
    };
    let downloads = repo::package_downloads(&state.pool, package.id).await?;
    let viewer = state.current(headers).await;
    let can_manage = viewer
        .as_ref()
        .is_some_and(|(_, publisher)| publisher.id == package.owner_id);
    let dependencies: Vec<DependencyView> =
        aevia_registry_core::index::deps_from_json(&selected.dependencies)
            .into_iter()
            .map(|dependency| {
                DependencyView {
                    import: dependency.label(),
                    package: dependency.package,
                    req: dependency.req,
                }
            })
            .collect();
    let package_id = PackageId::new(&owner_row.handle, &package.name)?;
    let dependents: Vec<DependentView> =
        repo::reverse_dependencies(&state.pool, &package_id.key(), 20)
            .await?
            .into_iter()
            .map(|row| {
                DependentView {
                    package: format!("{}/{}", row.owner_handle, row.name),
                    version: row.version,
                    req: row.req,
                }
            })
            .collect();
    Ok(CratePage {
        id: package_id,
        description: package.description.clone(),
        license: package.license.clone(),
        repository: package.repository.clone(),
        keywords: package.keywords.clone(),
        readme_html: selected
            .readme
            .as_deref()
            .map(aevia_registry_docs::render_markdown),
        versions: versions.iter().map(version_view).collect(),
        selected: version_view(&selected),
        owners: vec![owner_view(&owner_row)],
        dependencies,
        dependents,
        created_at: package.created_at.to_rfc3339(),
        updated_at: package.updated_at.to_rfc3339(),
        downloads,
        can_manage,
    })
}

/// `GET /cli`
pub async fn cli(
    State(state): State<AppState>,
    Language(lang): Language,
    headers: HeaderMap,
) -> Html<String> {
    Html(web::cli_page(lang, state.viewer(&headers).await.as_ref()))
}

/// `GET /users/{handle}`
///
/// # Errors
///
/// `404` when nobody has that handle, `E_DB` when a read fails.
pub async fn user(
    State(state): State<AppState>,
    Language(lang): Language,
    Path(handle): Path<String>,
    headers: HeaderMap,
) -> PageResult<Html<String>> {
    let Some(publisher) = repo::publisher_by_any_handle(&state.pool, handle.trim()).await? else {
        return Err(PageError::not_found(format!(
            "There is no publisher called {handle}."
        )));
    };
    let packages = repo::packages_for_publisher(&state.pool, publisher.id).await?;
    let page = UserPage {
        owner: owner_view(&publisher),
        joined_at: publisher.created_at.to_rfc3339(),
        downloads: packages.iter().map(|package| package.downloads).sum(),
        packages: cards_of(&packages),
    };
    Ok(Html(web::user_page(
        lang,
        &page,
        state.viewer(&headers).await.as_ref(),
    )))
}

/// `GET /docs`
///
/// # Errors
///
/// `E_DB` when the list cannot be read.
pub async fn docs_landing(
    State(state): State<AppState>,
    Language(lang): Language,
    headers: HeaderMap,
) -> PageResult<Html<String>> {
    let (items, _) = repo::list_packages(&state.pool, "", "recent", 60, 0).await?;
    let documented: Vec<PackageSummary> = items.into_iter().filter(|item| item.has_docs).collect();
    let cards = cards_of(&documented);
    Ok(Html(web::docs_landing(
        lang,
        &cards,
        state.viewer(&headers).await.as_ref(),
    )))
}

/// `GET /docs/{owner}/{name}` — the newest documented version.
///
/// # Errors
///
/// `404` when the package has no documentation.
pub async fn docs_latest(
    State(state): State<AppState>,
    Path((owner, name)): Path<(String, String)>,
) -> PageResult<Response> {
    let id = PackageId::new(&owner, &name)?;
    let Some((package, _)) =
        repo::package_by_handle(&state.pool, id.owner(), &id.name_key()).await?
    else {
        return Err(PageError::not_found(format!(
            "There is no package called {id}."
        )));
    };
    let documented = repo::versions_with_docs(&state.pool, package.id).await?;
    let Some(version) = documented.first() else {
        return Err(PageError::not_found(format!(
            "{id} has no built documentation yet."
        )));
    };
    Ok(Redirect::to(&format!("/docs/{id}/{version}")).into_response())
}

/// `GET /docs/{owner}/{name}/{version}`
///
/// # Errors
///
/// `404` when the version has no documentation.
pub async fn docs_index(
    State(state): State<AppState>,
    Language(lang): Language,
    Path((owner, name, version)): Path<(String, String, String)>,
    headers: HeaderMap,
) -> PageResult<Html<String>> {
    let (id, package, row) = resolve_version(&state, &owner, &name, &version).await?;
    let pages = repo::docs_pages(&state.pool, row.id).await?;
    let index = pages.iter().find(|page| page.path.is_empty());
    let modules: Vec<ModuleLink> = pages
        .iter()
        .filter(|page| !page.path.is_empty())
        .map(|page| {
            ModuleLink {
                path: page.path.clone(),
                decls: usize::try_from(page.decls).unwrap_or(0),
                undocumented: usize::try_from(page.undocumented).unwrap_or(0),
            }
        })
        .collect();
    let total_decls: usize = modules.iter().map(|module| module.decls).sum();
    let undocumented: usize = modules.iter().map(|module| module.undocumented).sum();
    let other_versions = repo::versions_with_docs(&state.pool, package.id).await?;
    let page = DocsIndexPage {
        id,
        version: row.version.clone(),
        modules,
        total_decls,
        undocumented,
        readme_html: index.map(|page| page.html.clone()),
        generated_at: row.docs_generated_at.unwrap_or(row.created_at).to_rfc3339(),
        other_versions,
    };
    Ok(Html(web::docs_index(
        lang,
        &page,
        state.viewer(&headers).await.as_ref(),
    )))
}

/// `GET /docs/{owner}/{name}/{version}/{module}`
///
/// # Errors
///
/// `404` when the module page does not exist.
pub async fn docs_module(
    State(state): State<AppState>,
    Language(lang): Language,
    Path((owner, name, version, module)): Path<(String, String, String, String)>,
    headers: HeaderMap,
) -> PageResult<Html<String>> {
    let (id, _package, row) = resolve_version(&state, &owner, &name, &version).await?;
    let pages = repo::docs_pages(&state.pool, row.id).await?;
    let Some(page) = pages.iter().find(|page| page.path == module) else {
        return Err(PageError::not_found(format!(
            "{id} {version} has no module `{module}`."
        )));
    };
    let modules: Vec<ModuleLink> = pages
        .iter()
        .filter(|page| !page.path.is_empty())
        .map(|page| {
            ModuleLink {
                path: page.path.clone(),
                decls: usize::try_from(page.decls).unwrap_or(0),
                undocumented: usize::try_from(page.undocumented).unwrap_or(0),
            }
        })
        .collect();
    let model = DocModulePage {
        id,
        version: row.version.clone(),
        module: page.path.clone(),
        html: page.html.clone(),
        modules,
        source_file: page.source_file.clone(),
        decls: usize::try_from(page.decls).unwrap_or(0),
        undocumented: usize::try_from(page.undocumented).unwrap_or(0),
    };
    Ok(Html(web::docs_module(
        lang,
        &model,
        state.viewer(&headers).await.as_ref(),
    )))
}

async fn resolve_version(
    state: &AppState,
    owner: &str,
    name: &str,
    version: &str,
) -> PageResult<(PackageId, aevia_registry_db::models::Package, Version)> {
    let requested = PackageId::new(owner, name)?;
    let Some((package, owner_row)) =
        repo::package_by_handle(&state.pool, requested.owner(), &requested.name_key()).await?
    else {
        return Err(PageError::not_found(format!(
            "There is no package called {requested}."
        )));
    };
    let Some(row) = repo::version(&state.pool, package.id, version).await? else {
        return Err(PageError::not_found(format!(
            "{requested} has no version {version}."
        )));
    };
    // The canonical spelling comes from the database, not from the URL, so a
    // link typed in the wrong case still resolves and then redirects the reader
    // to the name the author chose.
    let id = PackageId::new(&owner_row.handle, &package.name)?;
    Ok((id, package, row))
}

/// `GET /account`
///
/// # Errors
///
/// `403` when nobody is signed in, `E_DB` when a read fails.
pub async fn account(
    State(state): State<AppState>,
    Language(lang): Language,
    headers: HeaderMap,
) -> PageResult<Html<String>> {
    let Some((_, publisher)) = state.current(&headers).await else {
        return Err(PageError::forbidden("Sign in to see your account: /login"));
    };
    let keys = repo::signing_keys(&state.pool, publisher.id).await?;
    let packages = repo::packages_for_publisher(&state.pool, publisher.id).await?;
    let page = AccountPage {
        viewer: crate::state::viewer_of(&publisher),
        keys: keys
            .iter()
            .map(|key| {
                KeyView {
                    id: key.id,
                    kind: key.kind.clone(),
                    fingerprint: key.fingerprint.clone(),
                    label: key.label.clone(),
                    created_at: key.created_at.to_rfc3339(),
                    revoked: key.revoked_at.is_some(),
                }
            })
            .collect(),
        packages: cards_of(&packages),
    };
    Ok(Html(web::account(lang, &page)))
}

/// The form of `POST /account/keys`.
#[derive(Deserialize)]
pub struct KeyForm {
    /// The ASCII-armoured public key.
    pub armored: String,
    /// A label the human will recognise.
    pub label: Option<String>,
}

/// `POST /account/keys`
///
/// # Errors
///
/// `403` when nobody is signed in, `400` when the key does not parse.
pub async fn account_add_key(
    State(state): State<AppState>,
    headers: HeaderMap,
    Form(form): Form<KeyForm>,
) -> PageResult<Response> {
    let Some((_, publisher)) = state.current(&headers).await else {
        return Err(PageError::forbidden("Sign in before adding a key."));
    };
    let key = aevia_registry_core::signature::SignerKey::detect(&form.armored)?;
    let fingerprint = key.fingerprint()?;
    repo::add_signing_key(
        &state.pool,
        publisher.id,
        key.kind.as_str(),
        &fingerprint,
        &key.armored,
        form.label.as_deref(),
    )
    .await?;
    repo::append_log(
        &state.pool,
        "key",
        &format!("{} added a {} key", publisher.handle, key.kind.as_str()),
        serde_json::json!({
            "publisher": publisher.handle,
            "fingerprint": fingerprint,
            "kind": key.kind.as_str(),
        }),
    )
    .await?;
    Ok(Redirect::to("/account?key=added").into_response())
}

/// `POST /account/keys/{id}/revoke`
///
/// # Errors
///
/// `403` when nobody is signed in, `404` when the key is not theirs.
pub async fn account_revoke_key(
    State(state): State<AppState>,
    Path(id): Path<i64>,
    headers: HeaderMap,
) -> PageResult<Response> {
    let Some((_, publisher)) = state.current(&headers).await else {
        return Err(PageError::forbidden("Sign in before revoking a key."));
    };
    if !repo::revoke_signing_key(&state.pool, publisher.id, id).await? {
        return Err(PageError::not_found(
            "That key is not yours, or is already revoked.",
        ));
    }
    Ok(Redirect::to("/account?key=revoked").into_response())
}

/// `GET /login`
///
/// # Errors
///
/// Never fails; a missing provider is shown as unavailable.
pub async fn login(
    State(state): State<AppState>,
    Language(lang): Language,
    Query(query): Query<BTreeMap<String, String>>,
    headers: HeaderMap,
) -> PageResult<Html<String>> {
    let next = crate::state::safe_next(query.get("next").map(String::as_str));
    let viewer = state.viewer(&headers).await;
    Ok(Html(web::login(
        lang,
        viewer.as_ref(),
        &next,
        state.config.github.is_some(),
        state.config.orcid.is_some(),
    )))
}

/// `POST /logout`
///
/// # Errors
///
/// `E_DB` when the session cannot be removed.
pub async fn logout(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> PageResult<Response> {
    if let Some(id) = crate::http::cookie(&headers, crate::http::SESSION_COOKIE) {
        repo::delete_session(&state.pool, &id).await?;
    }
    let mut response = Redirect::to("/").into_response();
    response.headers_mut().insert(
        crate::http::set_cookie_header(),
        crate::http::clear_session_cookie(),
    );
    Ok(response)
}

/// `GET /log`
///
/// # Errors
///
/// `E_DB` when the log cannot be read.
pub async fn log(
    State(state): State<AppState>,
    Language(lang): Language,
    headers: HeaderMap,
) -> PageResult<Html<String>> {
    let entries = repo::log_entries(&state.pool, 0, LOG_PAGE).await?;
    let head = repo::log_head(&state.pool).await?;
    let views: Vec<LogEntryView> = entries.iter().map(log_view).collect();
    let (seq, hash) = head.as_ref().map_or((0, "0".repeat(64)), |entry| {
        (entry.seq, entry.entry_hash.clone())
    });
    Ok(Html(web::log_page(
        lang,
        &views,
        seq,
        &hash,
        state.viewer(&headers).await.as_ref(),
    )))
}

// --------------------------------------------------------------------- spec

/// `GET /spec`
///
/// # Errors
///
/// Never fails: a deployment without a mounted specification renders the
/// explanation of how to mount one.
pub async fn spec_index(
    State(state): State<AppState>,
    Language(lang): Language,
    headers: HeaderMap,
) -> PageResult<Html<String>> {
    Ok(Html(web::spec_index(
        state.spec.book(),
        lang,
        state.viewer(&headers).await.as_ref(),
    )))
}

/// `GET /spec/all`
///
/// # Errors
///
/// Never fails, for the same reason as [`spec_index`].
pub async fn spec_all(
    State(state): State<AppState>,
    Language(lang): Language,
    headers: HeaderMap,
) -> PageResult<Html<String>> {
    Ok(Html(web::spec_all(
        state.spec.book(),
        lang,
        state.viewer(&headers).await.as_ref(),
    )))
}

/// `GET /spec/{slug}`
///
/// # Errors
///
/// `404` when the specification has no such chapter.
pub async fn spec_chapter(
    State(state): State<AppState>,
    Language(lang): Language,
    Path(slug): Path<String>,
    headers: HeaderMap,
) -> PageResult<Html<String>> {
    let book = state.spec.book();
    let Some(chapter) = book.chapter(&slug) else {
        return Err(
            PageError::not_found(format!("The specification has no chapter `{slug}`."))
                .in_lang(lang),
        );
    };
    Ok(Html(web::spec_page(
        book,
        chapter,
        lang,
        state.viewer(&headers).await.as_ref(),
    )))
}

/// `GET /lang/{code}` — remembers a language and returns to the page.
///
/// A GET, not a POST: choosing a language is not a state change anyone needs to
/// protect, and a link works from every page without a form. The redirect target
/// is taken from the `Referer` and reduced to its path, so the endpoint cannot
/// be turned into an open redirect.
pub async fn set_language(
    State(state): State<AppState>,
    Path(code): Path<String>,
    headers: HeaderMap,
) -> Response {
    let language = aevia_registry_core::Lang::parse(&code).unwrap_or(aevia_registry_core::Lang::En);
    let target = headers
        .get(axum::http::header::REFERER)
        .and_then(|value| value.to_str().ok())
        .and_then(|referer| url::Url::parse(referer).ok())
        .map(|url| {
            match url.query() {
                | Some(query) => format!("{}?{query}", url.path()),
                | None => url.path().to_string(),
            }
        })
        .filter(|path| path.starts_with('/') && !path.starts_with("//"))
        .unwrap_or_else(|| "/".to_string());
    let mut response = Redirect::to(&target).into_response();
    response.headers_mut().insert(
        crate::http::set_cookie_header(),
        crate::http::language_cookie(
            language.code(),
            crate::http::is_secure(&state.config.base_url),
        ),
    );
    response
}

/// `GET /static/site.css`
pub async fn site_css() -> impl IntoResponse {
    (
        StatusCode::OK,
        [
            (header::CONTENT_TYPE, "text/css; charset=utf-8"),
            (header::CACHE_CONTROL, "public, max-age=300"),
        ],
        web::SITE_CSS,
    )
}

/// `GET /static/app.js`
pub async fn app_js() -> impl IntoResponse {
    (
        StatusCode::OK,
        [
            (header::CONTENT_TYPE, "text/javascript; charset=utf-8"),
            (header::CACHE_CONTROL, "public, max-age=300"),
        ],
        web::APP_JS,
    )
}

/// `GET /healthz`
///
/// # Errors
///
/// `503` when the database does not answer.
pub async fn healthz(State(state): State<AppState>) -> Response {
    match sqlx::query_scalar::<_, i32>("SELECT 1")
        .fetch_one(&state.pool)
        .await
    {
        | Ok(_) => {
            (
                StatusCode::OK,
                axum::Json(serde_json::json!({
                    "status": "ok",
                    "registry": state.config.registry_name,
                    "time": Utc::now().to_rfc3339(),
                })),
            )
                .into_response()
        },
        | Err(error) => {
            (
                StatusCode::SERVICE_UNAVAILABLE,
                axum::Json(serde_json::json!({
                    "status": "degraded",
                    "error": error.to_string(),
                })),
            )
                .into_response()
        },
    }
}

// ------------------------------------------------------------- conversions

fn cards_of(items: &[PackageSummary]) -> Vec<CrateCard> {
    items
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
        .collect()
}

fn owner_view(publisher: &Publisher) -> OwnerView {
    OwnerView {
        kind: publisher.kind.clone(),
        handle: publisher.handle.clone(),
        display_name: publisher.display_name.clone(),
        avatar_url: publisher.avatar_url.clone(),
    }
}

fn version_view(row: &Version) -> VersionView {
    VersionView {
        version: row.version.clone(),
        checksum: row.checksum.clone(),
        size: u64::try_from(row.size_bytes).unwrap_or(0),
        published_at: row.created_at.to_rfc3339(),
        yanked: row.yanked,
        docs: row.docs_status == "built",
        key_kind: row.key_kind.clone(),
        fingerprint: row.fingerprint.clone(),
        downloads: row.downloads,
    }
}

fn log_view(entry: &aevia_registry_db::models::LogEntry) -> LogEntryView {
    LogEntryView {
        seq: entry.seq,
        kind: entry.kind.clone(),
        summary: entry.summary.clone(),
        hash: entry.entry_hash.clone(),
        prev: entry.prev_hash.clone(),
        at: entry.created_at.to_rfc3339(),
    }
}
