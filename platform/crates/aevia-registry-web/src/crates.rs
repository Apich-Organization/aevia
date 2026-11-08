//! The crate list and the crate page.
//!
//! These two pages carry most of the registry's information density, so they
//! share one card renderer and one date/size vocabulary: a reader who learns to
//! scan the list can scan the detail page without relearning the layout.

use aevia_registry_core::vm::CrateCard;
use aevia_registry_core::vm::CrateList;
use aevia_registry_core::vm::CratePage;
use aevia_registry_core::vm::Viewer;
use leptos::prelude::AnyView;
use leptos::prelude::IntoAny;
use leptos::prelude::*;

use crate::date_only;
use crate::layout;
use crate::layout::avatar;
use crate::layout::provider_badge;
use crate::query_param;
use crate::shorten;
use crate::thousands;

/// The sort orders the tabs offer, in the order they are shown.
///
/// Kept here rather than derived from the data so that the tab strip is stable:
/// a query with no matches still shows every way of re-running it.
const SORTS: [&str; 4] = ["relevance", "downloads", "recent", "name"];

/// A package card, linked at `href`.
///
/// The same card appears on the front page, the docs landing page and the
/// account page; only the destination differs, which is why the link is a
/// parameter rather than something the card infers from its context.
pub(crate) fn card(
    item: &CrateCard,
    href: &str,
) -> AnyView {
    let owner = format!("{}/", item.id.owner());
    let name = item.id.name().to_string();
    let description = item
        .description
        .clone()
        .unwrap_or_else(|| "No description published.".to_string());
    let version = item.version.clone();
    let updated_at = item.updated_at.clone();
    let date = date_only(&item.updated_at).to_string();
    let downloads = thousands(item.downloads);
    view! {
        <article class="card">
            <h3 class="card-title">
                <a href=href.to_string()>
                    <span class="owner">{owner}</span>
                    {name}
                </a>
            </h3>
            <p class="card-desc">{description}</p>
            <p class="card-meta">
                <code>{version}</code>
                <span aria-hidden="true">"·"</span>
                <time datetime=updated_at>{date}</time>
                <span aria-hidden="true">"·"</span>
                <span>{format!("{downloads} downloads")}</span>
            </p>
        </article>
    }
    .into_any()
}

/// Renders `/crates`.
///
/// The search box and the sort tabs are ordinary links and a GET form, so the
/// page is fully usable with scripting off; the inline script only filters the
/// rows already on the page while the reader types.
pub fn crate_list(
    list: &CrateList,
    viewer: Option<&Viewer>,
) -> String {
    let query = query_param(&list.query);
    let sort = list.sort.clone();
    let tabs: Vec<AnyView> = SORTS
        .iter()
        .map(|key| {
            let current = (list.sort == *key).then_some("page");
            view! {
                <li>
                    <a href=format!("/crates?q={query}&sort={key}") aria-current=current>
                        {*key}
                    </a>
                </li>
            }
            .into_any()
        })
        .collect();

    let rows: Vec<AnyView> = list
        .items
        .iter()
        .map(|item| {
            let id = item.id.to_path();
            let owner = format!("{}/", item.id.owner());
            let name = item.id.name().to_string();
            let description = item.description.clone().unwrap_or_else(|| "—".to_string());
            let search = format!(
                "{id} {} {}",
                item.description.clone().unwrap_or_default(),
                item.version
            );
            let version = item.version.clone();
            let updated_at = item.updated_at.clone();
            let date = date_only(&item.updated_at).to_string();
            let downloads = thousands(item.downloads);
            view! {
                <tr data-name=search>
                    <th scope="row">
                        <a href=format!("/crates/{id}")>
                            <span class="owner">{owner}</span>
                            {name}
                        </a>
                    </th>
                    <td>{description}</td>
                    <td>
                        <code>{version}</code>
                    </td>
                    <td>
                        <time datetime=updated_at>{date}</time>
                    </td>
                    <td class="num">{downloads}</td>
                </tr>
            }
            .into_any()
        })
        .collect();

    let total = thousands(list.total);
    let summary = if list.total == 1 {
        "1 package".to_string()
    } else {
        format!("{total} packages")
    };

    let results = if list.items.is_empty() {
        view! {
            <div class="empty">
                <p>"No packages matched this search."</p>
                <p class="muted">
                    "Try a shorter term, or publish the first one: "
                    <code>"aevia pkg publish"</code>
                    " from the package directory."
                </p>
            </div>
        }
        .into_any()
    } else {
        view! {
            <table id="crate-table">
                <caption>{summary}</caption>
                <thead>
                    <tr>
                        <th scope="col">"package"</th>
                        <th scope="col">"description"</th>
                        <th scope="col">"version"</th>
                        <th scope="col">"updated"</th>
                        <th scope="col">"downloads"</th>
                    </tr>
                </thead>
                <tbody>{rows}</tbody>
            </table>
            <p id="crate-filter-empty" class="muted" hidden>
                "No rows match the filter."
            </p>
        }
        .into_any()
    };

    let pager = (list.total > 0).then(|| {
        let previous = list.has_previous().then(|| {
            let page = list.page.saturating_sub(1);
            view! {
                <a href=format!("/crates?q={query}&sort={sort}&page={page}")>"← previous"</a>
            }
        });
        let next = list.has_next().then(|| {
            let page = list.page.saturating_add(1);
            view! { <a href=format!("/crates?q={query}&sort={sort}&page={page}")>"next →"</a> }
        });
        let last = list.last_page();
        view! {
            <nav class="pager" aria-label="Pagination">
                {previous}
                {next}
                <span class="spacer muted">
                    {format!("page {} of {last}", list.page)}
                </span>
            </nav>
        }
    });

    layout::page(
        "crates",
        "/crates",
        viewer,
        view! {
            <section class="page-head">
                <h1>"crates"</h1>
                <p>"Packages published by verified identities."</p>
            </section>
            <form class="search" method="get" action="/crates" role="search">
                <input
                    type="search"
                    name="q"
                    value=list.query.clone()
                    placeholder="search packages"
                    aria-label="Search packages"
                    data-filter="#crate-table"
                />
                <button type="submit">"search"</button>
            </form>
            <ul class="tabs" aria-label="Sort order">{tabs}</ul>
            {results}
            {pager}
        },
    )
}

/// Renders `/crates/{owner}/{name}`.
///
/// The page is one document with three anchor sections rather than three
/// routes: a version table, a dependency list and a README are all short enough
/// that a reader scrolls, and a scroll keeps the install line and the sidebar
/// in view while they compare versions.
pub fn crate_page(
    page: &CratePage,
    viewer: Option<&Viewer>,
) -> String {
    let id = page.id.to_path();
    let owner = format!("{}/", page.id.owner());
    let name = page.id.name().to_string();
    let description = page
        .description
        .clone()
        .unwrap_or_else(|| "No description published.".to_string());
    let install = page.install_command();
    let selected_version = page.selected.version.clone();

    let docs_button = page.selected.docs.then(|| {
        view! {
            <a class="btn" href=format!("/docs/{id}/{selected_version}")>"docs"</a>
        }
    });
    let repository_button = page.repository.as_ref().map(|url| {
        view! {
            <a class="btn btn-ghost" href=url.clone() rel="noreferrer">"repository"</a>
        }
    });
    let yank_button = page.can_manage.then(|| {
        let version = page.selected.version.clone();
        view! {
            <form class="inline" method="post" action=format!("/crates/{id}/yank")>
                <input type="hidden" name="version" value=version/>
                <button type="submit" class="btn btn-ghost btn-warn">"yank"</button>
            </form>
        }
    });

    let readme = match &page.readme_html {
        | Some(html) => view! { <div class="readme" inner_html=html.clone()></div> }.into_any(),
        | None => view! { <p class="muted">"This package published no README."</p> }.into_any(),
    };

    let version_rows: Vec<AnyView> = if page.versions.is_empty() {
        vec![
            view! {
                <tr>
                    <td colspan="7" class="muted">"No versions have been published."</td>
                </tr>
            }
            .into_any(),
        ]
    } else {
        page.versions
            .iter()
            .map(|version| {
                let number = version.version.clone();
                let published_at = version.published_at.clone();
                let date = date_only(&version.published_at).to_string();
                let size = version.size_human();
                let fingerprint = shorten(&version.fingerprint, 16);
                let docs = if version.docs {
                    view! {
                        <a href=format!("/docs/{id}/{number}")>"docs"</a>
                    }
                    .into_any()
                } else {
                    view! { <span class="muted">"—"</span> }.into_any()
                };
                let yanked = if version.yanked {
                    view! { <span class="badge badge-yank">"yanked"</span> }.into_any()
                } else {
                    view! { <span class="muted">"—"</span> }.into_any()
                };
                view! {
                    <tr>
                        <th scope="row">
                            <code>{number.clone()}</code>
                        </th>
                        <td>
                            <time datetime=published_at>{date}</time>
                        </td>
                        <td class="num">{size}</td>
                        <td>
                            <code class="mono">{fingerprint}</code>
                        </td>
                        <td>{yanked}</td>
                        <td>{docs}</td>
                        <td>
                            <a href=format!("/api/v1/download/{id}/{number}")>"download"</a>
                        </td>
                    </tr>
                }
                .into_any()
            })
            .collect()
    };

    let dependency_rows: Vec<AnyView> = if page.dependencies.is_empty() {
        vec![
            view! {
                <tr>
                    <td colspan="2" class="muted">"No dependencies."</td>
                </tr>
            }
            .into_any(),
        ]
    } else {
        page.dependencies
            .iter()
            .map(|dependency| {
                view! {
                    <tr>
                        <th scope="row">
                            <code>{dependency.import.clone()}</code>
                        </th>
                        <td>
                            <code>{dependency.path.clone()}</code>
                        </td>
                    </tr>
                }
                .into_any()
            })
            .collect()
    };

    let owners: Vec<AnyView> = page
        .owners
        .iter()
        .map(|owner| {
            view! {
                <li>
                    {avatar(owner)}
                    <a href=owner.profile_url()>{owner.handle.clone()}</a>
                    {provider_badge(owner)}
                </li>
            }
            .into_any()
        })
        .collect();

    let license = page
        .license
        .clone()
        .unwrap_or_else(|| "not stated".to_string());
    let repository = match &page.repository {
        | Some(url) => view! { <a href=url.clone() rel="noreferrer">{url.clone()}</a> }.into_any(),
        | None => view! { <span class="muted">"not stated"</span> }.into_any(),
    };
    let created_at = page.created_at.clone();
    let updated_at = page.updated_at.clone();
    let created = date_only(&page.created_at).to_string();
    let updated = date_only(&page.updated_at).to_string();
    let downloads = thousands(page.downloads);
    let checksum = page.selected.checksum.clone();
    let fingerprint = page.selected.fingerprint.clone();
    let selected_published_at = page.selected.published_at.clone();
    let selected_published = date_only(&page.selected.published_at).to_string();
    let selected_size = page.selected.size_human();

    layout::page(
        &id,
        "/crates",
        viewer,
        view! {
            <article class="crate">
                <header class="crate-head">
                    <h1>
                        <span class="owner">{owner}</span>
                        {name}
                    </h1>
                    <p class="lede">{description}</p>
                    <div class="install">
                        <code id="install-crate">{install}</code>
                        <button
                            type="button"
                            class="copy"
                            data-copy="#install-crate"
                            aria-label="Copy the install command"
                        >
                            "copy"
                        </button>
                    </div>
                    <div class="actions">
                        {docs_button}
                        {repository_button}
                        {yank_button}
                    </div>
                </header>
                <div class="crate-body">
                    <div class="crate-main">
                        <nav class="section-tabs" aria-label="Sections">
                            <a href="#readme">"readme"</a>
                            <a href="#versions">"versions"</a>
                            <a href="#dependencies">"dependencies"</a>
                        </nav>
                        <section id="readme">
                            <h2>"readme"</h2>
                            {readme}
                        </section>
                        <section id="versions">
                            <h2>"versions"</h2>
                            <table>
                                <thead>
                                    <tr>
                                        <th scope="col">"version"</th>
                                        <th scope="col">"published"</th>
                                        <th scope="col">"size"</th>
                                        <th scope="col">"key"</th>
                                        <th scope="col">"status"</th>
                                        <th scope="col">"docs"</th>
                                        <th scope="col">"archive"</th>
                                    </tr>
                                </thead>
                                <tbody>{version_rows}</tbody>
                            </table>
                        </section>
                        <section id="dependencies">
                            <h2>"dependencies"</h2>
                            <table>
                                <thead>
                                    <tr>
                                        <th scope="col">"import"</th>
                                        <th scope="col">"path"</th>
                                    </tr>
                                </thead>
                                <tbody>{dependency_rows}</tbody>
                            </table>
                        </section>
                    </div>
                    <aside class="side">
                        <h2>"package"</h2>
                        <dl class="meta">
                            <div>
                                <dt>"license"</dt>
                                <dd>{license}</dd>
                            </div>
                            <div>
                                <dt>"repository"</dt>
                                <dd>{repository}</dd>
                            </div>
                            <div>
                                <dt>"created"</dt>
                                <dd>
                                    <time datetime=created_at>{created}</time>
                                </dd>
                            </div>
                            <div>
                                <dt>"updated"</dt>
                                <dd>
                                    <time datetime=updated_at>{updated}</time>
                                </dd>
                            </div>
                            <div>
                                <dt>"downloads"</dt>
                                <dd class="mono">{downloads}</dd>
                            </div>
                        </dl>
                        <h2>"owners"</h2>
                        <ul class="owners">{owners}</ul>
                        <h2>{format!("version {selected_version}")}</h2>
                        <dl class="meta">
                            <div>
                                <dt>"size"</dt>
                                <dd class="mono">{selected_size}</dd>
                            </div>
                            <div>
                                <dt>"checksum"</dt>
                                <dd class="mono">{checksum}</dd>
                            </div>
                            <div>
                                <dt>"signed by"</dt>
                                <dd class="mono">{fingerprint}</dd>
                            </div>
                            <div>
                                <dt>"published"</dt>
                                <dd>
                                    <time datetime=selected_published_at>{selected_published}</time>
                                </dd>
                            </div>
                        </dl>
                    </aside>
                </div>
            </article>
        },
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use aevia_registry_core::name::PackageId;
    use aevia_registry_core::vm::DependencyView;
    use aevia_registry_core::vm::OwnerView;
    use aevia_registry_core::vm::VersionView;

    fn version(
        number: &str,
        yanked: bool,
    ) -> VersionView {
        VersionView {
            version: number.to_string(),
            checksum: "sha256:9f86d081884c7d659a2feaa0c55ad015a3bf4f1b2b0b822cd15d6c15b0f00a08"
                .to_string(),
            size: 4096,
            published_at: "2026-11-01T10:00:00Z".to_string(),
            yanked,
            docs: true,
            key_kind: "openpgp".to_string(),
            fingerprint: "0123456789ABCDEF0123456789ABCDEF01234567".to_string(),
            downloads: 12,
        }
    }

    fn page() -> CratePage {
        let id: PackageId = "apich/units-extra".parse().expect("valid identity");
        CratePage {
            id,
            description: Some("Units and constants the compiler can check.".to_string()),
            license: Some("Apache-2.0".to_string()),
            repository: Some("https://github.com/apich/units-extra".to_string()),
            keywords: vec!["units".to_string()],
            readme_html: Some("<p>Units.</p>".to_string()),
            versions: vec![version("1.2.0", true), version("1.1.0", false)],
            selected: version("1.2.0", true),
            owners: vec![OwnerView {
                kind: "github".to_string(),
                handle: "apich".to_string(),
                display_name: Some("Apich".to_string()),
                avatar_url: None,
            }],
            dependencies: vec![DependencyView {
                import: "Kinematics".to_string(),
                path: "../kinematics".to_string(),
            }],
            created_at: "2026-01-02T00:00:00Z".to_string(),
            updated_at: "2026-11-01T10:00:00Z".to_string(),
            downloads: 1204,
            can_manage: true,
        }
    }

    #[test]
    fn a_crate_page_shows_the_install_line_versions_and_yank() {
        let html = crate_page(&page(), None);
        assert!(html.contains("aevia pkg add apich/units-extra@1.2.0"));
        assert!(html.contains("1.2.0"), "the selected version appears");
        assert!(html.contains("1.1.0"), "every version appears");
        assert!(
            html.contains("badge badge-yank"),
            "the yanked badge appears"
        );
        assert!(html.contains(">yanked</span>"));
        assert!(html.contains("/api/v1/download/apich/units-extra/1.2.0"));
        assert!(html.contains("sha256:9f86d081884c7d659a2feaa0c55ad015"));
        assert!(html.contains("Kinematics"), "dependencies are listed");
        assert!(html.contains("yank"), "a manager sees the yank control");
    }

    #[test]
    fn a_crate_page_renders_without_optional_fields() {
        let mut bare = page();
        bare.description = None;
        bare.license = None;
        bare.repository = None;
        bare.readme_html = None;
        bare.versions = Vec::new();
        bare.dependencies = Vec::new();
        bare.owners = Vec::new();
        bare.can_manage = false;
        let html = crate_page(&bare, None);
        assert!(html.contains("No description published."));
        assert!(html.contains("No versions have been published."));
        assert!(html.contains("No dependencies."));
        assert!(!html.contains("badge badge-yank"));
    }

    #[test]
    fn a_list_says_when_it_is_empty() {
        let list = CrateList {
            total: 0,
            page: 1,
            per_page: 20,
            query: "nothing".to_string(),
            sort: "relevance".to_string(),
            items: Vec::new(),
        };
        let html = crate_list(&list, None);
        assert!(html.contains("No packages matched this search."));
        assert!(html.contains("aevia pkg publish"));
    }
}
