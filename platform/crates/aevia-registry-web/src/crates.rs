//! The crate list and the crate page.
//!
//! These two pages carry most of the registry's information density, so they
//! share one card renderer and one date/size vocabulary: a reader who learns to
//! scan the list can scan the detail page without relearning the layout.

use aevia_registry_core::Lang;
use aevia_registry_core::vm::CrateCard;
use aevia_registry_core::vm::CrateList;
use aevia_registry_core::vm::CratePage;
use aevia_registry_core::vm::Viewer;
use leptos::prelude::AnyView;
use leptos::prelude::IntoAny;
use leptos::prelude::*;

use crate::Msg;
use crate::date_only;
use crate::layout;
use crate::layout::avatar;
use crate::layout::provider_badge;
use crate::query_param;
use crate::shorten;
use crate::thousands;

// Page chrome.
const TITLE: Msg = Msg::new("crates", "包");
const LEDE: Msg = Msg::new(
    "Packages published by verified identities.",
    "由已验证身份发布的包。",
);

// Search and sort.
const SEARCH: Msg = Msg::new("search", "搜索");
const SEARCH_PLACEHOLDER: Msg = Msg::new("search packages", "搜索包");
const SEARCH_LABEL: Msg = Msg::new("Search packages", "搜索包");
const SORT_ORDER: Msg = Msg::new("Sort order", "排序");
const SORT_RELEVANCE: Msg = Msg::new("relevance", "相关度");
const SORT_DOWNLOADS: Msg = Msg::new("downloads", "下载");
const SORT_RECENT: Msg = Msg::new("recent", "最近");
const SORT_NAME: Msg = Msg::new("name", "名称");

// Column labels, shared by the list table and the package page.
const PACKAGE: Msg = Msg::new("package", "包");
const DESCRIPTION: Msg = Msg::new("description", "描述");
const VERSION: Msg = Msg::new("version", "版本");
const UPDATED: Msg = Msg::new("updated", "更新于");
const DOWNLOADS: Msg = Msg::new("downloads", "下载");

// The package list.
const ONE_PACKAGE: Msg = Msg::new("1 package", "1 个包");
const PACKAGE_SUFFIX: Msg = Msg::new("packages", "个包");
const NO_RESULTS: Msg = Msg::new("No packages matched this search.", "没有匹配此搜索的包。");
const NO_RESULTS_HINT: Msg = Msg::new(
    "Try a shorter term, or publish the first one: ",
    "试试更短的词，或发布第一个包：在包目录中运行 ",
);
const NO_RESULTS_HINT_END: Msg = Msg::new(" from the package directory.", "。");
const FILTER_EMPTY: Msg = Msg::new("No rows match the filter.", "没有匹配筛选条件的行。");
const PREVIOUS: Msg = Msg::new("← previous", "← 上一页");
const NEXT: Msg = Msg::new("next →", "下一页 →");
const PAGINATION: Msg = Msg::new("Pagination", "分页");
/// The pager's position line. The two languages put the numbers in different
/// places, so the sentence is a template rather than a prefix and a suffix.
const PAGE_OF: Msg = Msg::new("page {page} of {last}", "第 {page} 页，共 {last} 页");

// A package card.
const NO_DESCRIPTION: Msg = Msg::new("No description published.", "未发布描述。");
const DOWNLOAD_SUFFIX: Msg = Msg::new("downloads", "次下载");

// The package page.
const DOCS: Msg = Msg::new("docs", "文档");
const REPOSITORY: Msg = Msg::new("repository", "仓库");
const YANK: Msg = Msg::new("yank", "撤回");
const YANKED: Msg = Msg::new("yanked", "已撤回");
const COPY: Msg = Msg::new("copy", "复制");
const COPY_COMMAND: Msg = Msg::new("Copy the install command", "复制安装命令");
const README: Msg = Msg::new("readme", "README");
const NO_README: Msg = Msg::new("This package published no README.", "此包未发布 README。");
const VERSIONS: Msg = Msg::new("versions", "版本");
const DEPENDENCIES: Msg = Msg::new("dependencies", "依赖");
const SECTIONS: Msg = Msg::new("Sections", "分区");
const NO_VERSIONS: Msg = Msg::new("No versions have been published.", "尚未发布任何版本。");
const NO_DEPENDENCIES: Msg = Msg::new("No dependencies.", "无依赖。");
const DOWNLOAD: Msg = Msg::new("download", "下载");
const NOT_STATED: Msg = Msg::new("not stated", "未声明");
const OWNERS: Msg = Msg::new("owners", "发布者");

// The version and dependency tables.
const PUBLISHED: Msg = Msg::new("published", "发布于");
const SIZE: Msg = Msg::new("size", "大小");
const KEY: Msg = Msg::new("key", "密钥");
const STATUS: Msg = Msg::new("status", "状态");
const ARCHIVE: Msg = Msg::new("archive", "归档");
const IMPORT: Msg = Msg::new("import", "导入");
const PATH: Msg = Msg::new("path", "路径");

// The side panel.
const LICENSE: Msg = Msg::new("license", "许可证");
const CREATED: Msg = Msg::new("created", "创建于");
const CHECKSUM: Msg = Msg::new("checksum", "校验和");
const SIGNED_BY: Msg = Msg::new("signed by", "签名者");

/// The sort orders the tabs offer, in the order they are shown.
///
/// Kept here rather than derived from the data so that the tab strip is stable:
/// a query with no matches still shows every way of re-running it. The key is
/// the query value; the label is the tab text.
const SORTS: [(&str, Msg); 4] = [
    ("relevance", SORT_RELEVANCE),
    ("downloads", SORT_DOWNLOADS),
    ("recent", SORT_RECENT),
    ("name", SORT_NAME),
];

/// A package card, linked at `href`.
///
/// The same card appears on the front page, the docs landing page and the
/// account page; only the destination differs, which is why the link is a
/// parameter rather than something the card infers from its context.
///
/// This entry point renders English. The front page and the docs landing page
pub(crate) fn card_in(
    lang: Lang,
    item: &CrateCard,
    href: &str,
) -> AnyView {
    let owner = format!("{}/", item.id.owner());
    let name = item.id.name().to_string();
    let description = item
        .description
        .clone()
        .unwrap_or_else(|| NO_DESCRIPTION.of(lang).to_string());
    let version = item.version.clone();
    let updated_at = item.updated_at.clone();
    let date = date_only(&item.updated_at).to_string();
    let downloads = thousands(item.downloads);
    let download_count = format!("{downloads} {}", DOWNLOAD_SUFFIX.of(lang));
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
                <span>{download_count}</span>
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
    lang: Lang,
    list: &CrateList,
    viewer: Option<&Viewer>,
) -> String {
    let query = query_param(&list.query);
    let sort = list.sort.clone();
    let tabs: Vec<AnyView> = SORTS
        .iter()
        .map(|(key, label)| {
            let current = (list.sort == *key).then_some("page");
            let text = label.of(lang);
            view! {
                <li>
                    <a href=format!("/crates?q={query}&sort={key}") aria-current=current>
                        {text}
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
        ONE_PACKAGE.of(lang).to_string()
    } else {
        format!("{total} {}", PACKAGE_SUFFIX.of(lang))
    };

    let results = if list.items.is_empty() {
        view! {
            <div class="empty">
                <p>{NO_RESULTS.of(lang)}</p>
                <p class="muted">
                    {NO_RESULTS_HINT.of(lang)}
                    <code>"aevia pkg publish"</code>
                    {NO_RESULTS_HINT_END.of(lang)}
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
                        <th scope="col">{PACKAGE.of(lang)}</th>
                        <th scope="col">{DESCRIPTION.of(lang)}</th>
                        <th scope="col">{VERSION.of(lang)}</th>
                        <th scope="col">{UPDATED.of(lang)}</th>
                        <th scope="col">{DOWNLOADS.of(lang)}</th>
                    </tr>
                </thead>
                <tbody>{rows}</tbody>
            </table>
            <p id="crate-filter-empty" class="muted" hidden>
                {FILTER_EMPTY.of(lang)}
            </p>
        }
        .into_any()
    };

    let pager = (list.total > 0).then(|| {
        let previous = list.has_previous().then(|| {
            let page = list.page.saturating_sub(1);
            view! {
                <a href=format!("/crates?q={query}&sort={sort}&page={page}")>
                    {PREVIOUS.of(lang)}
                </a>
            }
        });
        let next = list.has_next().then(|| {
            let page = list.page.saturating_add(1);
            view! {
                <a href=format!("/crates?q={query}&sort={sort}&page={page}")>{NEXT.of(lang)}</a>
            }
        });
        let last = list.last_page();
        let position = PAGE_OF
            .of(lang)
            .replace("{page}", &list.page.to_string())
            .replace("{last}", &last.to_string());
        view! {
            <nav class="pager" aria-label=PAGINATION.of(lang)>
                {previous}
                {next}
                <span class="spacer muted">{position}</span>
            </nav>
        }
    });

    layout::page(
        lang,
        TITLE.of(lang),
        "/crates",
        viewer,
        view! {
            <section class="page-head">
                <h1>{TITLE.of(lang)}</h1>
                <p>{LEDE.of(lang)}</p>
            </section>
            <form class="search" method="get" action="/crates" role="search">
                <input
                    type="search"
                    name="q"
                    value=list.query.clone()
                    placeholder=SEARCH_PLACEHOLDER.of(lang)
                    aria-label=SEARCH_LABEL.of(lang)
                    data-filter="#crate-table"
                />
                <button type="submit">{SEARCH.of(lang)}</button>
            </form>
            <ul class="tabs" aria-label=SORT_ORDER.of(lang)>{tabs}</ul>
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
    lang: Lang,
    page: &CratePage,
    viewer: Option<&Viewer>,
) -> String {
    let id = page.id.to_path();
    let owner = format!("{}/", page.id.owner());
    let name = page.id.name().to_string();
    let description = page
        .description
        .clone()
        .unwrap_or_else(|| NO_DESCRIPTION.of(lang).to_string());
    let install = page.install_command();
    let selected_version = page.selected.version.clone();

    let docs_button = page.selected.docs.then(|| {
        view! {
            <a class="btn" href=format!("/docs/{id}/{selected_version}")>{DOCS.of(lang)}</a>
        }
    });
    let repository_button = page.repository.as_ref().map(|url| {
        view! {
            <a class="btn btn-ghost" href=url.clone() rel="noreferrer">{REPOSITORY.of(lang)}</a>
        }
    });
    let yank_button = page.can_manage.then(|| {
        let version = page.selected.version.clone();
        view! {
            <form class="inline" method="post" action=format!("/crates/{id}/yank")>
                <input type="hidden" name="version" value=version/>
                <button type="submit" class="btn btn-ghost btn-warn">{YANK.of(lang)}</button>
            </form>
        }
    });

    let readme = match &page.readme_html {
        | Some(html) => view! { <div class="readme" inner_html=html.clone()></div> }.into_any(),
        | None => view! { <p class="muted">{NO_README.of(lang)}</p> }.into_any(),
    };

    let version_rows: Vec<AnyView> = if page.versions.is_empty() {
        vec![
            view! {
                <tr>
                    <td colspan="7" class="muted">{NO_VERSIONS.of(lang)}</td>
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
                        <a href=format!("/docs/{id}/{number}")>{DOCS.of(lang)}</a>
                    }
                    .into_any()
                } else {
                    view! { <span class="muted">"—"</span> }.into_any()
                };
                let yanked = if version.yanked {
                    view! { <span class="badge badge-yank">{YANKED.of(lang)}</span> }.into_any()
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
                            <a href=format!("/api/v1/download/{id}/{number}")>{DOWNLOAD.of(lang)}</a>
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
                    <td colspan="2" class="muted">{NO_DEPENDENCIES.of(lang)}</td>
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
        .unwrap_or_else(|| NOT_STATED.of(lang).to_string());
    let repository = match &page.repository {
        | Some(url) => view! { <a href=url.clone() rel="noreferrer">{url.clone()}</a> }.into_any(),
        | None => view! { <span class="muted">{NOT_STATED.of(lang)}</span> }.into_any(),
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
        lang,
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
                            aria-label=COPY_COMMAND.of(lang)
                        >
                            {COPY.of(lang)}
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
                        <nav class="section-tabs" aria-label=SECTIONS.of(lang)>
                            <a href="#readme">{README.of(lang)}</a>
                            <a href="#versions">{VERSIONS.of(lang)}</a>
                            <a href="#dependencies">{DEPENDENCIES.of(lang)}</a>
                        </nav>
                        <section id="readme">
                            <h2>{README.of(lang)}</h2>
                            {readme}
                        </section>
                        <section id="versions">
                            <h2>{VERSIONS.of(lang)}</h2>
                            <table>
                                <thead>
                                    <tr>
                                        <th scope="col">{VERSION.of(lang)}</th>
                                        <th scope="col">{PUBLISHED.of(lang)}</th>
                                        <th scope="col">{SIZE.of(lang)}</th>
                                        <th scope="col">{KEY.of(lang)}</th>
                                        <th scope="col">{STATUS.of(lang)}</th>
                                        <th scope="col">{DOCS.of(lang)}</th>
                                        <th scope="col">{ARCHIVE.of(lang)}</th>
                                    </tr>
                                </thead>
                                <tbody>{version_rows}</tbody>
                            </table>
                        </section>
                        <section id="dependencies">
                            <h2>{DEPENDENCIES.of(lang)}</h2>
                            <table>
                                <thead>
                                    <tr>
                                        <th scope="col">{IMPORT.of(lang)}</th>
                                        <th scope="col">{PATH.of(lang)}</th>
                                    </tr>
                                </thead>
                                <tbody>{dependency_rows}</tbody>
                            </table>
                        </section>
                    </div>
                    <aside class="side">
                        <h2>{PACKAGE.of(lang)}</h2>
                        <dl class="meta">
                            <div>
                                <dt>{LICENSE.of(lang)}</dt>
                                <dd>{license}</dd>
                            </div>
                            <div>
                                <dt>{REPOSITORY.of(lang)}</dt>
                                <dd>{repository}</dd>
                            </div>
                            <div>
                                <dt>{CREATED.of(lang)}</dt>
                                <dd>
                                    <time datetime=created_at>{created}</time>
                                </dd>
                            </div>
                            <div>
                                <dt>{UPDATED.of(lang)}</dt>
                                <dd>
                                    <time datetime=updated_at>{updated}</time>
                                </dd>
                            </div>
                            <div>
                                <dt>{DOWNLOADS.of(lang)}</dt>
                                <dd class="mono">{downloads}</dd>
                            </div>
                        </dl>
                        <h2>{OWNERS.of(lang)}</h2>
                        <ul class="owners">{owners}</ul>
                        <h2>{format!("{} {selected_version}", VERSION.of(lang))}</h2>
                        <dl class="meta">
                            <div>
                                <dt>{SIZE.of(lang)}</dt>
                                <dd class="mono">{selected_size}</dd>
                            </div>
                            <div>
                                <dt>{CHECKSUM.of(lang)}</dt>
                                <dd class="mono">{checksum}</dd>
                            </div>
                            <div>
                                <dt>{SIGNED_BY.of(lang)}</dt>
                                <dd class="mono">{fingerprint}</dd>
                            </div>
                            <div>
                                <dt>{PUBLISHED.of(lang)}</dt>
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
        let html = crate_page(Lang::En, &page(), None);
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
        let html = crate_page(Lang::En, &bare, None);
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
        let html = crate_list(Lang::En, &list, None);
        assert!(html.contains("No packages matched this search."));
        assert!(html.contains("aevia pkg publish"));
    }

    #[test]
    fn a_list_renders_in_chinese() {
        let list = CrateList {
            total: 0,
            page: 1,
            per_page: 20,
            query: String::new(),
            sort: "relevance".to_string(),
            items: Vec::new(),
        };
        let html = crate_list(Lang::Zh, &list, None);
        assert!(html.contains(r#"<html lang="zh-CN">"#));
        assert!(html.contains("没有匹配此搜索的包。"));
        assert!(html.contains("相关度"), "the sort tabs are translated");
    }
}
