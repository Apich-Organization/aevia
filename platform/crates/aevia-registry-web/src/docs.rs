//! The three documentation pages.
//!
//! Documentation on this registry is not written twice. The compiler renders
//! `##` comments from the modules it type-checks at publish time, and these
//! pages only present that output: a landing page that explains the rule, an
//! index per package version that shows what is covered, and one page per
//! module with the rendered declarations.
//!
//! The coverage numbers are stated as counts as well as a percentage, because a
//! percentage hides the difference between a package with three declarations
//! and one with three hundred.

use aevia_registry_core::vm::CrateCard;
use aevia_registry_core::vm::DocModulePage;
use aevia_registry_core::vm::DocsIndexPage;
use aevia_registry_core::vm::Viewer;
use leptos::prelude::AnyView;
use leptos::prelude::*;

use crate::crates::card;
use crate::layout;

/// Renders `/docs`.
///
/// The search box posts to the same page as a GET form so that a shared link
/// carries the query; there is no separate results route to keep in sync.
pub fn docs_landing(
    crates: &[CrateCard],
    viewer: Option<&Viewer>,
) -> String {
    let cards: Vec<AnyView> = crates
        .iter()
        .map(|item| {
            let href = format!("/docs/{}/{}", item.id.to_path(), item.version);
            card(item, &href)
        })
        .collect();

    let body = if crates.is_empty() {
        view! {
            <p class="muted">
                "No documentation has been built yet. Publish a package and the compiler will \
                 build it."
            </p>
        }
        .into_any()
    } else {
        view! { <div class="cards">{cards}</div> }.into_any()
    };

    layout::page(
        "documentation",
        "/docs",
        viewer,
        view! {
            <section class="page-head">
                <h1>"documentation"</h1>
                <p>"Built from `##` comments by the compiler, at publish time."</p>
            </section>
            <p>
                "A declaration carries its documentation next to it. When a version is \
                 published, the compiler parses the sources, collects the `##` comment above \
                 each public declaration and renders the module pages you see here. There is \
                 no separate documentation build to run and no second source of truth: the \
                 text you read is the text that was compiled."
            </p>
            <form class="search" method="get" action="/docs" role="search">
                <input
                    type="search"
                    name="q"
                    placeholder="search documentation"
                    aria-label="Search documentation"
                />
                <button type="submit">"search"</button>
            </form>
            <section>
                <h2>"documented packages"</h2>
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
pub fn docs_index(
    page: &DocsIndexPage,
    viewer: Option<&Viewer>,
) -> String {
    let id = page.id.to_path();
    let owner = format!("{}/", page.id.owner());
    let name = page.id.name().to_string();
    let version = page.version.clone();
    let percent = page.documented_percent();
    let documented = page.total_decls.saturating_sub(page.undocumented);
    let coverage = format!(
        "{documented} of {} declarations documented ({percent}%)",
        page.total_decls
    );
    let generated_at = page.generated_at.clone();
    let generated = crate::date_only(&page.generated_at).to_string();

    let rows: Vec<AnyView> = page
        .modules
        .iter()
        .map(|module| {
            let path = module.path.clone();
            let href = format!("/docs/{id}/{version}/{path}");
            view! {
                <tr>
                    <th scope="row">
                        <a href=href>
                            <code>{path}</code>
                        </a>
                    </th>
                    <td class="num">{module.decls}</td>
                    <td class="num">{module.undocumented}</td>
                </tr>
            }
            .into_any()
        })
        .collect();

    let module_table = if page.modules.is_empty() {
        view! { <p class="muted">"This version documents no modules."</p> }.into_any()
    } else {
        view! {
            <table>
                <thead>
                    <tr>
                        <th scope="col">"module"</th>
                        <th scope="col">"declarations"</th>
                        <th scope="col">"undocumented"</th>
                    </tr>
                </thead>
                <tbody>{rows}</tbody>
            </table>
        }
        .into_any()
    };

    let readme = match &page.readme_html {
        | Some(html) => view! {
            <section>
                <h2>"readme"</h2>
                <div class="readme" inner_html=html.clone()></div>
            </section>
        }
        .into_any(),
        | None => ().into_any(),
    };

    let other_versions: Vec<AnyView> = page
        .other_versions
        .iter()
        .map(|other| {
            view! {
                <li>
                    <a href=format!("/docs/{id}/{other}")>{other.clone()}</a>
                </li>
            }
            .into_any()
        })
        .collect();

    layout::page(
        &format!("{id} {version} docs"),
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
                        " · built "
                        <time datetime=generated_at>{generated}</time>
                        " · "
                        <a href=format!("/crates/{id}")>"package page"</a>
                    </p>
                </header>
                <p>{coverage}</p>
                <div
                    class="meter"
                    role="progressbar"
                    aria-valuenow=percent
                    aria-valuemin="0"
                    aria-valuemax="100"
                    aria-label="documented declarations"
                >
                    <span style=format!("width: {percent}%")></span>
                </div>
                <section>
                    <h2>"modules"</h2>
                    {module_table}
                </section>
                {readme}
                <section>
                    <h2>"other documented versions"</h2>
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
pub fn docs_module(
    page: &DocModulePage,
    viewer: Option<&Viewer>,
) -> String {
    let id = page.id.to_path();
    let version = page.version.clone();
    let module = page.module.clone();
    let undocumented = page.undocumented;
    let decls = page.decls;
    let source_file = page.source_file.clone();
    let header = format!("{source_file} · {decls} declarations · {undocumented} undocumented");

    let modules: Vec<AnyView> = page
        .modules
        .iter()
        .map(|entry| {
            let current = (entry.path == module).then_some("page");
            let href = format!("/docs/{id}/{version}/{}", entry.path);
            view! {
                <li>
                    <a href=href aria-current=current>{entry.path.clone()}</a>
                </li>
            }
            .into_any()
        })
        .collect();

    layout::page(
        &format!("{id} {module}"),
        "/docs",
        viewer,
        view! {
            <div class="docs-layout">
                <aside class="docs-side">
                    <h2>"modules"</h2>
                    <ul>{modules}</ul>
                    <p class="small">
                        <a href=format!("/docs/{id}/{version}")>"← package index"</a>
                    </p>
                </aside>
                <div class="docs-main">
                    <header class="doc-head">
                        <h1>{module.clone()}</h1>
                        <p>{header}</p>
                        <p>
                            <a href=format!("/crates/{id}")>"package page"</a>
                            " · "
                            <a href=format!("/docs/{id}/{version}")>"documentation index"</a>
                        </p>
                    </header>
                    <div class="doc-body" inner_html=page.html.clone()></div>
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
        let html = docs_index(&index(), None);
        assert!(html.contains("10 of 12 declarations documented (83%)"));
        assert!(html.contains("/docs/apich/units-extra/1.2.0/Kinematics"));
        assert!(html.contains("Thermo.IdealGas"));
        assert!(html.contains("width: 83%"));
    }

    #[test]
    fn a_module_page_highlights_the_current_module() {
        let page = DocModulePage {
            id: "apich/units-extra".parse().expect("valid identity"),
            version: "1.2.0".to_string(),
            module: "Kinematics".to_string(),
            html: "<h2>fall_time</h2>".to_string(),
            modules: index().modules,
            source_file: "src/kinematics.aev".to_string(),
            decls: 8,
            undocumented: 2,
        };
        let html = docs_module(&page, None);
        assert!(html.contains("src/kinematics.aev · 8 declarations · 2 undocumented"));
        assert!(html.contains("aria-current=\"page\">Kinematics</a>"));
        assert!(
            html.contains("<h2>fall_time</h2>"),
            "raw documentation HTML"
        );
    }
}
