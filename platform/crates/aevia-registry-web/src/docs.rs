//! The three documentation pages.
//!
//! The documentation builder extracts `##` comments from the published
//! modules. These pages present its output in a landing page, a version index
//! with coverage counts, and a page for each module. The version and module
//! views borrow rendered HTML from their view models for synchronous SSR.
//!
//! The coverage numbers are stated as counts as well as a percentage, because a
//! percentage hides the difference between a package with three declarations
//! and one with three hundred.

use aevia_registry_core::Lang;
use aevia_registry_core::PackageId;
use aevia_registry_core::vm::CrateCard;
use aevia_registry_core::vm::DocModulePage;
use aevia_registry_core::vm::DocsIndexPage;
use aevia_registry_core::vm::Viewer;
use leptos::either::Either;
use leptos::prelude::AnyView;
use leptos::prelude::*;

use crate::Msg;
use crate::crates::card_in;
use crate::layout;

// Page headings and body text.
const TITLE: Msg = Msg::new("documentation", "文档");
const LEDE: Msg = Msg::new(
    "Built from ## comments by the compiler, at publish time.",
    "由编译器在发布时根据 ## 注释生成。",
);
const ABOUT: Msg = Msg::new(
    "A declaration carries its documentation next to it. When a version is \
     published, the compiler parses the sources, collects the ## comment above \
     each public declaration and renders the module pages you see here. There is \
     no separate documentation build to run and no second source of truth: the \
     text you read is the text that was compiled.",
    "声明旁边的注释就是它的文档。版本发布时，编译器解析源文件，收集每个公开声明上方的 \
     ## 注释，并渲染出这里看到的模块页面。不需要另外跑一次文档构建，也不存在第二份事实来源：\
     你读到的文本就是编译时读到的文本。",
);
const EMPTY: Msg = Msg::new(
    "No documentation has been built yet. Publish a package and the compiler will \
     build it.",
    "还没有构建任何文档。发布一个包，编译器就会自动构建。",
);
const DOCUMENTED_PACKAGES: Msg = Msg::new("documented packages", "已写注释的包");

// The search form.
const SEARCH_PLACEHOLDER: Msg = Msg::new("search documentation", "搜索文档");
const SEARCH_LABEL: Msg = Msg::new("Search documentation", "搜索文档");
const SEARCH: Msg = Msg::new("search", "搜索");

// Table headers and section headings.
const MODULE_COLUMN: Msg = Msg::new("module", "模块");
const DECLARATIONS_COLUMN: Msg = Msg::new("declarations", "声明");
const UNDOCUMENTED_COLUMN: Msg = Msg::new("undocumented", "未写注释");
const MODULES: Msg = Msg::new("modules", "模块");
const NO_MODULES: Msg = Msg::new("This version documents no modules.", "该版本没有模块文档。");
const README: Msg = Msg::new("readme", "README");
const OTHER_VERSIONS: Msg = Msg::new("other documented versions", "其他已写注释的版本");

// Coverage and navigation.
const COVERAGE_LABEL: Msg = Msg::new("documented declarations", "已写注释的声明");
const BUILT: Msg = Msg::new("built", "构建于");
const PACKAGE_PAGE: Msg = Msg::new("package page", "包页面");
const PACKAGE_INDEX: Msg = Msg::new("← package index", "← 包索引");
const DOC_INDEX: Msg = Msg::new("documentation index", "文档索引");

/// The state of a release whose documentation pages are unavailable.
#[must_use]
pub fn docs_build_status(
    lang: Lang,
    id: &PackageId,
    version: &str,
    message: &str,
) -> String {
    let title = format!("{id} {version}");
    layout::page(
        lang,
        &title,
        "/docs",
        None,
        view! {
            <article class="manual-index">
                <h1>{title.as_str()}</h1>
                <p>{message}</p>
                <p><a href=format!("/crates/{id}/{version}")>{PACKAGE_PAGE.of(lang)}</a></p>
            </article>
        },
    )
}

/// The coverage sentence.
///
/// The counts are interpolated, so the two languages cannot share one template:
/// Chinese puts the total first and parenthesises the percentage with a
/// full-width bracket.
#[allow(
    clippy::single_call_fn,
    reason = "documentation coverage uses different word order in English and Chinese"
)]
fn coverage(
    lang: Lang,
    documented: usize,
    total: usize,
    percent: u32,
) -> String {
    match lang {
        | Lang::En => format!("{documented} of {total} declarations documented ({percent}%)"),
        | Lang::Zh => format!("{total} 条声明中 {documented} 条已写注释（{percent}%）"),
    }
}

/// The source line above a module's declarations.
#[allow(
    clippy::single_call_fn,
    reason = "module source summaries use a separate localized format"
)]
fn module_source(
    lang: Lang,
    source_file: &str,
    decls: usize,
    undocumented: usize,
) -> String {
    match lang {
        | Lang::En => {
            format!("{source_file} · {decls} declarations · {undocumented} undocumented")
        },
        | Lang::Zh => format!("{source_file} · {decls} 条声明 · {undocumented} 条未写注释"),
    }
}

/// Renders `/docs`.
///
/// The search box posts to the same page as a GET form so that a shared link
/// carries the query; there is no separate results route to keep in sync.
#[must_use]
pub fn docs_landing(
    lang: Lang,
    crates: &[CrateCard],
    viewer: Option<&Viewer>,
) -> String {
    let cards: Vec<AnyView> = crates
        .iter()
        .map(|item| {
            let href = format!("/docs/{}/{}", item.id.to_path(), item.version);
            card_in(lang, item, &href)
        })
        .collect();

    let body = if crates.is_empty() {
        view! { <p class="muted">{EMPTY.of(lang)}</p> }.into_any()
    } else {
        view! { <div class="cards">{cards}</div> }.into_any()
    };

    layout::page(
        lang,
        TITLE.of(lang),
        "/docs",
        viewer,
        view! {
            <section class="page-head">
                <h1>{TITLE.of(lang)}</h1>
                <p>{LEDE.of(lang)}</p>
            </section>
            <p>{ABOUT.of(lang)}</p>
            <form class="search" method="get" action="/docs" role="search">
                <input
                    type="search"
                    name="q"
                    placeholder=SEARCH_PLACEHOLDER.of(lang)
                    aria-label=SEARCH_LABEL.of(lang)
                />
                <button type="submit">{SEARCH.of(lang)}</button>
            </form>
            <section>
                <h2>{DOCUMENTED_PACKAGES.of(lang)}</h2>
                {body}
            </section>
        },
    )
}

/// Renders `/docs/{owner}/{name}/{version}`.
///
/// The coverage bar is placed above the module table because the table is
/// where a reader decides what to read next, and whether the package is
/// documented at all changes how they read it.
#[must_use]
pub fn docs_index(
    lang: Lang,
    page: &DocsIndexPage,
    viewer: Option<&Viewer>,
) -> String {
    let id = page.id.to_path();
    let owner = format!("{}/", page.id.owner());
    let name = page.id.name();
    let version = page.version.as_str();
    let percent = page.documented_percent();
    let documented = page.total_decls.saturating_sub(page.undocumented);
    let coverage = coverage(lang, documented, page.total_decls, percent);
    let title = match lang {
        | Lang::En => format!("{id} {version} docs"),
        | Lang::Zh => format!("{id} {version} 文档"),
    };
    let generated_at = page.generated_at.as_str();
    let generated = crate::date_only(generated_at);

    let module_table = if page.modules.is_empty() {
        Either::Left(view! { <p class="muted">{NO_MODULES.of(lang)}</p> })
    } else {
        let rows: Vec<_> = page
            .modules
            .iter()
            .map(|module| {
                let path = module.path.as_str();
                view! {
                    <tr>
                        <th scope="row">
                            <a href=format!("/docs/{id}/{version}/{path}")>
                                <code>{path}</code>
                            </a>
                        </th>
                        <td class="num">{module.decls}</td>
                        <td class="num">{module.undocumented}</td>
                    </tr>
                }
            })
            .collect();
        Either::Right(view! {
            <table>
                <thead>
                    <tr>
                        <th scope="col">{MODULE_COLUMN.of(lang)}</th>
                        <th scope="col">{DECLARATIONS_COLUMN.of(lang)}</th>
                        <th scope="col">{UNDOCUMENTED_COLUMN.of(lang)}</th>
                    </tr>
                </thead>
                <tbody>{rows}</tbody>
            </table>
        })
    };

    let readme = page.readme_html.as_deref().map_or_else(
        // The empty erased view supplies the existing SSR placeholder.
        || Either::Right(().into_any()),
        |html| {
            Either::Left(view! {
                <section>
                    <h2>{README.of(lang)}</h2>
                    <div class="readme" inner_html=html></div>
                </section>
            })
        },
    );

    let other_versions: Vec<_> = page
        .other_versions
        .iter()
        .map(|other| {
            view! {
                <li>
                    <a href=format!("/docs/{id}/{other}")>{other.as_str()}</a>
                </li>
            }
        })
        .collect();

    layout::page(
        lang,
        &title,
        "/docs",
        viewer,
        view! {
            <article>
                <header class="page-head">
                    <h1>
                        <span class="owner">{owner}</span>
                        {name}
                    </h1>
                    <p>
                        <code>{format!("{id} {version}")}</code>
                        " · "
                        {BUILT.of(lang)}
                        " "
                        <time datetime=generated_at>{generated}</time>
                        " · "
                        <a href=format!("/crates/{id}")>{PACKAGE_PAGE.of(lang)}</a>
                    </p>
                </header>
                <p>{coverage}</p>
                <progress
                    class="meter"
                    value=percent
                    max="100"
                    aria-label=COVERAGE_LABEL.of(lang)
                ></progress>
                <section>
                    <h2>{MODULES.of(lang)}</h2>
                    {module_table}
                </section>
                {readme}
                <section>
                    <h2>{OTHER_VERSIONS.of(lang)}</h2>
                    <ul>{other_versions}</ul>
                </section>
            </article>
        },
    )
}

/// Renders `/docs/{owner}/{name}/{version}/{module}`.
///
/// The sidebar lists every module rather than only siblings: a reader arriving
/// from a search result needs to see the shape of the package, not just where
/// they landed.
#[must_use]
pub fn docs_module(
    lang: Lang,
    page: &DocModulePage,
    viewer: Option<&Viewer>,
) -> String {
    let id = page.id.to_path();
    let version = page.version.as_str();
    let module = page.module.as_str();
    let header = module_source(lang, &page.source_file, page.decls, page.undocumented);
    let docs_path = format!("/docs/{id}/{version}");

    let modules: Vec<_> = page
        .modules
        .iter()
        .map(|entry| {
            let path = entry.path.as_str();
            let current = (path == module).then_some("page");
            view! {
                <li>
                    <a href=format!("{docs_path}/{path}") aria-current=current>{path}</a>
                </li>
            }
        })
        .collect();

    layout::page(
        lang,
        &format!("{id} {module}"),
        "/docs",
        viewer,
        view! {
            <div class="docs-layout">
                <aside class="docs-side">
                    <h2>{MODULES.of(lang)}</h2>
                    <ul>{modules}</ul>
                    <p class="small">
                        <a href=docs_path.as_str()>{PACKAGE_INDEX.of(lang)}</a>
                    </p>
                </aside>
                <div class="docs-main">
                    <header class="doc-head">
                        <h1>{module}</h1>
                        <p>{header}</p>
                        <p>
                            <a href=format!("/crates/{id}")>{PACKAGE_PAGE.of(lang)}</a>
                            " · "
                            <a href=docs_path.as_str()>{DOC_INDEX.of(lang)}</a>
                        </p>
                    </header>
                    <div class="doc-body" inner_html=page.html.as_str()></div>
                </div>
            </div>
        },
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use aevia_registry_core::vm::ModuleLink;

    fn index() -> DocsIndexPage {
        DocsIndexPage {
            id: "apich/units-extra".parse().expect("valid identity"),
            version: "1.2.0".to_string(),
            modules: vec![
                ModuleLink {
                    path: "Kinematics".to_string(),
                    decls: 8,
                    undocumented: 2,
                },
                ModuleLink {
                    path: "Thermo.IdealGas".to_string(),
                    decls: 4,
                    undocumented: 0,
                },
            ],
            total_decls: 12,
            undocumented: 2,
            readme_html: Some("<p>Units.</p>".to_string()),
            generated_at: "2026-11-01T10:00:00Z".to_string(),
            other_versions: vec!["1.1.0".to_string()],
        }
    }

    #[test]
    fn a_docs_index_states_coverage_and_links_modules() {
        let html = docs_index(Lang::En, &index(), None);
        assert!(html.contains("10 of 12 declarations documented (83%)"));
        assert!(html.contains("/docs/apich/units-extra/1.2.0/Kinematics"));
        assert!(html.contains("Thermo.IdealGas"));
        assert!(html.contains("<progress"));
        assert!(html.contains("value=\"83\""));
        assert!(html.contains("<div class=\"readme\"><p>Units.</p></div>"));
    }

    #[test]
    fn empty_documentation_omits_the_module_table_and_readme() {
        let mut page = index();
        page.modules.clear();
        page.total_decls = 0;
        page.undocumented = 0;
        page.readme_html = None;
        let html = docs_index(Lang::Zh, &page, None);
        assert!(html.contains("<html lang=\"zh-CN\">"));
        assert!(html.contains("该版本没有模块文档。"));
        assert!(html.contains("value=\"100\""));
        assert!(!html.contains("<table>"));
        assert!(!html.contains("class=\"readme\""));
    }

    #[test]
    fn a_module_page_highlights_the_current_module() {
        let page = DocModulePage {
            id: "apich/units-extra".parse().expect("valid identity"),
            version: "1.2.0".to_string(),
            module: "Kinematics".to_string(),
            html: "<h2>fall_time</h2>".to_string(),
            modules: index().modules,
            source_file: "src/force & motion.aev".to_string(),
            decls: 8,
            undocumented: 2,
        };
        let html = docs_module(Lang::En, &page, None);
        assert!(html.contains("src/force &amp; motion.aev · 8 declarations · 2 undocumented"));
        assert!(html.contains("aria-current=\"page\">Kinematics</a>"));
        assert_eq!(
            html.matches("aria-current=\"page\">Kinematics</a>").count(),
            1
        );
        assert!(
            html.contains("<div class=\"doc-body\"><h2>fall_time</h2></div>"),
            "raw documentation HTML"
        );
    }
}
