use aevia_registry_core::Lang;
use aevia_registry_core::vm::ManualChapter;
use aevia_registry_core::vm::ManualKind;
use aevia_registry_core::vm::Viewer;
use leptos::prelude::*;

use crate::Msg;
use crate::lang_attr;
use crate::layout::page;
use crate::render;
use crate::styles::SITE_CSS;

const CHAPTERS: Msg = Msg::new("Chapters", "章节");
const ON_PAGE: Msg = Msg::new("On this page", "本页目录");
const SINGLE_PAGE: Msg = Msg::new("Read on one page", "单页阅读");
const DOWNLOAD: Msg = Msg::new("Download HTML", "下载 HTML");
const PREVIOUS: Msg = Msg::new("Previous chapter", "上一章");
const NEXT: Msg = Msg::new("Next chapter", "下一章");
const UNAVAILABLE: Msg = Msg::new("This manual is unavailable.", "暂时无法阅读本手册。");
const PLAYGROUND_LABEL: Msg = Msg::new("Aevia source code", "Aevia 源代码");
const PLAYGROUND_RUN: Msg = Msg::new("Run", "运行");
const PLAYGROUND_RUNNING: Msg = Msg::new("Running…", "正在运行…");
const PLAYGROUND_TIMEOUT: Msg = Msg::new(
    "The program reached the time limit.",
    "程序达到运行时间上限。",
);
const PLAYGROUND_LIMIT: Msg = Msg::new(
    "The program reached the output limit.",
    "程序达到输出量上限。",
);
const PLAYGROUND_EMPTY: Msg = Msg::new("Completed with no output.", "程序运行结束，无输出。");
const PLAYGROUND_FAILED: Msg = Msg::new(
    "The program did not complete successfully.",
    "程序未能成功运行。",
);
const PLAYGROUND_BUSY: Msg = Msg::new(
    "The runner is busy. Try again shortly.",
    "运行服务繁忙，请稍后重试。",
);
const PLAYGROUND_INPUT: Msg = Msg::new(
    "Enter up to 16 KiB of Aevia source.",
    "请输入不超过 16 KiB 的 Aevia 源代码。",
);
const PLAYGROUND_UNAVAILABLE: Msg = Msg::new("The runner is unavailable.", "运行服务暂不可用。");

fn link(
    kind: ManualKind,
    suffix: &str,
    lang: Lang,
) -> String {
    format!("{}{suffix}?lang={}", kind.route(), lang.code())
}

fn chapter_link(
    kind: ManualKind,
    slug: &str,
    lang: Lang,
) -> String {
    link(kind, &format!("/{slug}"), lang)
}

fn chapters(
    kind: ManualKind,
    edition: &[ManualChapter],
    lang: Lang,
    current: Option<&str>,
    combined: bool,
) -> impl RenderHtml {
    view! {
        <nav aria-label=CHAPTERS.of(lang)>
            <ol class="manual-toc">
                {edition.iter().map(|chapter| {
                    let active = current == Some(chapter.slug.as_str());
                    let href = if combined {
                        format!("#section-{}", chapter.slug)
                    } else {
                        chapter_link(kind, &chapter.slug, lang)
                    };
                    view! {
                        <li><a class="spec-link" aria-current=active.then_some("page") href=href>
                            {chapter.title.clone()}
                        </a></li>
                    }
                }).collect_view()}
            </ol>
        </nav>
    }
    .into_owned()
}

/// Contents and edition links for a manual.
#[must_use]
pub fn manual_index(
    kind: ManualKind,
    edition: &[ManualChapter],
    lang: Lang,
    viewer: Option<&Viewer>,
) -> String {
    let body = view! {
        <article class="manual-index">
            <h1>{kind.title(lang)}</h1>
            {if edition.is_empty() {
                view! { <p>{UNAVAILABLE.of(lang)}</p> }.into_any()
            } else {
                view! {
                    <p class="manual-actions">
                        <a href=link(kind, "/all", lang)>{SINGLE_PAGE.of(lang)}</a>
                        <a href=link(kind, "/download", lang)>{DOWNLOAD.of(lang)}</a>
                    </p>
                    {chapters(kind, edition, lang, None, false)}
                }.into_any()
            }}
        </article>
    };
    page(lang, kind.title(lang), kind.route(), viewer, body)
}

/// An individual chapter with an optional executable example editor.
#[must_use]
pub fn manual_chapter(
    kind: ManualKind,
    edition: &[ManualChapter],
    index: usize,
    lang: Lang,
    viewer: Option<&Viewer>,
    playground: bool,
) -> String {
    let Some(chapter) = edition.get(index) else {
        return manual_index(kind, edition, lang, viewer);
    };
    let playground = playground && kind == ManualKind::Book;
    let body = view! {
        <div class="spec-shell">
            <aside class="spec-nav">
                <a class="spec-home" href=link(kind, "", lang)>{kind.title(lang)}</a>
                {chapters(kind, edition, lang, Some(&chapter.slug), false)}
            </aside>
            <article class="spec-body">
                <header class="spec-head"><h1>{chapter.title.clone()}</h1></header>
                {(!chapter.toc.is_empty()).then(|| view! {
                    <nav class="manual-local-toc" aria-label=ON_PAGE.of(lang)>
                        <p>{ON_PAGE.of(lang)}</p>
                        <ul>
                            {chapter.toc.iter().map(|heading| view! {
                                <li class=format!("manual-toc-level-{}", heading.level)>
                                    <a href=format!("#{}", heading.id)>{heading.text.clone()}</a>
                                </li>
                            }).collect_view()}
                        </ul>
                    </nav>
                })}
                <div class="prose"
                    data-playground=playground.then_some("/api/v1/playground/run")
                    data-label=playground.then_some(PLAYGROUND_LABEL.of(lang))
                    data-run=playground.then_some(PLAYGROUND_RUN.of(lang))
                    data-running=playground.then_some(PLAYGROUND_RUNNING.of(lang))
                    data-timeout=playground.then_some(PLAYGROUND_TIMEOUT.of(lang))
                    data-limit=playground.then_some(PLAYGROUND_LIMIT.of(lang))
                    data-empty=playground.then_some(PLAYGROUND_EMPTY.of(lang))
                    data-failed=playground.then_some(PLAYGROUND_FAILED.of(lang))
                    data-busy=playground.then_some(PLAYGROUND_BUSY.of(lang))
                    data-input=playground.then_some(PLAYGROUND_INPUT.of(lang))
                    data-unavailable=playground.then_some(PLAYGROUND_UNAVAILABLE.of(lang))
                    inner_html=chapter.html.clone()
                ></div>
                <nav class="manual-actions" aria-label=CHAPTERS.of(lang)>
                    {index.checked_sub(1).and_then(|i| edition.get(i)).map(|previous| view! {
                        <a rel="prev" href=chapter_link(kind, &previous.slug, lang)>{PREVIOUS.of(lang)}</a>
                    })}
                    {edition.get(index.saturating_add(1)).map(|next| view! {
                        <a rel="next" href=chapter_link(kind, &next.slug, lang)>{NEXT.of(lang)}</a>
                    })}
                </nav>
            </article>
        </div>
    };
    page(lang, &chapter.title, kind.route(), viewer, body)
}

fn single_page(
    kind: ManualKind,
    edition: &[ManualChapter],
    lang: Lang,
) -> impl RenderHtml {
    view! {
        <article class="manual-index manual-combined">
            <h1>{kind.title(lang)}</h1>
            {if edition.is_empty() {
                view! { <p>{UNAVAILABLE.of(lang)}</p> }.into_any()
            } else {
                view! {
                    {chapters(kind, edition, lang, None, true)}
                    {edition.iter().map(|chapter| view! {
                        <section class="spec-chapter" id=format!("section-{}", chapter.slug)>
                            <h2>{chapter.title.clone()}</h2>
                            <div class="prose" inner_html=chapter.combined_html.clone()></div>
                        </section>
                    }).collect_view()}
                }.into_any()
            }}
        </article>
    }
    .into_owned()
}

/// A single-page edition in the site chrome.
#[must_use]
pub fn manual_all(
    kind: ManualKind,
    edition: &[ManualChapter],
    lang: Lang,
    viewer: Option<&Viewer>,
) -> String {
    page(
        lang,
        kind.title(lang),
        kind.route(),
        viewer,
        single_page(kind, edition, lang),
    )
}

/// A standalone HTML edition for offline reading.
#[must_use]
pub fn manual_download(
    kind: ManualKind,
    edition: &[ManualChapter],
    lang: Lang,
) -> String {
    let document = render(|| {
        view! {
            <html lang=lang_attr(lang)>
                <head>
                    <meta charset="utf-8"/>
                    <meta name="viewport" content="width=device-width, initial-scale=1"/>
                    <title>{kind.title(lang)}</title>
                    <style>{SITE_CSS}</style>
                </head>
                <body><main>{single_page(kind, edition, lang)}</main></body>
            </html>
        }
    });
    format!("<!DOCTYPE html>{document}")
}
