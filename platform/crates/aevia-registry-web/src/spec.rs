//! The language specification, served from the repository that owns it.
//!
//! # Why the site renders the chapters instead of a bundled edition
//!
//! The language repository already ships `Aevia-Language-Specification.md`, a
//! single file stitched together by `tools/bundle_spec.py`. Serving *that* would
//! mean the site shows whatever the last person remembered to regenerate. This
//! module reads `spec/*.md` itself, in the same order the bundler uses, so the
//! site and the repository cannot disagree — and the single-page view is
//! produced here rather than checked in.
//!
//! # Why the interface is bilingual but the text is not
//!
//! The normative chapters are written in Chinese; the index and the ecosystem
//! are English. Translating 480 KB of normative text is a job for a translator
//! with review, not for a build step, and a machine translation of a language
//! specification is worse than none — a reader cannot tell which sentences are
//! load-bearing. So the arrangement is explicit rather than hidden:
//!
//! * every label, heading and navigation element is bilingual;
//! * every chapter carries an English title and, where one has been written, an
//!   English abstract, so an English reader can find and understand the shape of
//!   a chapter before reading the normative text;
//! * the Chinese text is presented as the normative one, labelled as such.

use aevia_registry_core::Lang;
use aevia_registry_core::vm::SpecBook;
use aevia_registry_core::vm::SpecChapter;
use aevia_registry_core::vm::SpecHeading;
use aevia_registry_core::vm::Viewer;
use leptos::prelude::AnyView;
use leptos::prelude::IntoAny;
use leptos::prelude::*;

use crate::Msg;
use crate::layout;
use crate::thousands;

const TITLE: Msg = Msg::new("language specification", "语言规范");
const LEDE: Msg = Msg::new(
    "The normative design of Aevia: grammar, typing rules, dimensions, units, \
     runtime and the rssn backend. The Chinese text is normative; \
     English editions are published chapter by chapter, and a chapter without one \
     carries an English abstract.",
    "Aevia 的规范设计：文法、类型规则、量纲、单位、运行时与 rssn 后端。以中文为准，\
     英文版逐章提供；尚无英文版的章节附英文摘要。",
);
const SINGLE_PAGE: Msg = Msg::new("read it as one page", "单页阅读全文");
const BACK_TO_INDEX: Msg = Msg::new("all chapters", "全部章节");
const CHAPTER: Msg = Msg::new("chapter", "第");
const NORMATIVE: Msg = Msg::new("normative", "规范性");
const INFORMATIVE: Msg = Msg::new("informative", "说明性");
const WORDS: Msg = Msg::new("words", "字");
const ABSTRACT: Msg = Msg::new("English abstract", "英文摘要");
const ON_THIS_PAGE: Msg = Msg::new("on this page", "本页目录");
const PREVIOUS: Msg = Msg::new("previous", "上一章");
const NEXT: Msg = Msg::new("next", "下一章");
const CONTENTS: Msg = Msg::new("contents", "目录");
const REVISION: Msg = Msg::new("revision", "修订版");
const NO_ENGLISH_YET: Msg = Msg::new(
    "This chapter has no English edition yet. The Chinese text, which is normative, is shown \
     below.",
    "本章尚无英文版。",
);
const NOT_NORMATIVE_NOTE: Msg = Msg::new(
    "The Chinese text below is normative. The abstract is a reading aid, not a \
     substitute for it.",
    "以下中文正文为规范文本。英文摘要仅用于导航，不能替代正文。",
);
const NO_SPEC: Msg = Msg::new(
    "The specification is not mounted on this deployment. Set AEVIA_SPEC_DIR to \
     the language repository's spec/ directory.",
    "本次部署没有挂载语言规范。请把 AEVIA_SPEC_DIR 指向语言仓库的 spec/ 目录。",
);
const CHAPTERS: Msg = Msg::new("chapters", "章");
const CHAPTER_COLUMN: Msg = Msg::new("chapter", "章节");
const TITLE_COLUMN: Msg = Msg::new("title", "标题");
const WORDS_COLUMN: Msg = Msg::new("words", "字数");

/// Renders `/spec`.
///
/// # Errors
///
/// Never fails: an empty book renders the "not mounted" notice, which is a
/// deployment problem the reader can report, not a 500.
#[must_use]
pub fn spec_index(
    book: &SpecBook,
    lang: Lang,
    viewer: Option<&Viewer>,
) -> String {
    if book.chapters.is_empty() {
        return layout::page(
            lang,
            TITLE.of(lang),
            "/spec",
            viewer,
            view! {
                <section class="empty-page">
                    <h1>{TITLE.of(lang)}</h1>
                    <p>{NO_SPEC.of(lang)}</p>
                </section>
            },
        );
    }
    let rows: Vec<AnyView> = book
        .chapters
        .iter()
        .map(|chapter| {
            let href = format!("/spec/{}", chapter.slug);
            let number = chapter.number.clone();
            let title = list_title(chapter, lang).to_string();
            let other = secondary_title(chapter, lang);
            let other = if other == title {
                String::new()
            } else {
                other.to_string()
            };
            let words = thousands(i64::try_from(chapter.words).unwrap_or(0));
            let badge = if chapter.normative {
                view! { <span class="badge">{NORMATIVE.of(lang)}</span> }.into_any()
            } else {
                view! { <span class="badge badge-quiet">{INFORMATIVE.of(lang)}</span> }.into_any()
            };
            view! {
                <tr>
                    <td class="num">{number}</td>
                    <td>
                        <a href=href>{title}</a>
                        <span class="row-sub">{other}</span>
                    </td>
                    <td class="num">{words}</td>
                    <td>{badge}</td>
                </tr>
            }
            .into_any()
        })
        .collect();
    let words = thousands(i64::try_from(book.words()).unwrap_or(0));
    let chapters = book.chapters.len();
    let revision = book.revision.clone();
    layout::page(
        lang,
        TITLE.of(lang),
        "/spec",
        viewer,
        view! {
            <section class="spec-intro">
                <h1>{TITLE.of(lang)}</h1>
                <p class="lede">{LEDE.of(lang)}</p>
                <p class="meta">
                    {format!("{} {revision}", REVISION.of(lang))}
                    " · "
                    {format!("{chapters} {}", CHAPTERS.of(lang))}
                    " · "
                    {format!("{words} {}", WORDS.of(lang))}
                </p>
                <p class="more">
                    <a class="btn" href="/spec/all">{SINGLE_PAGE.of(lang)}</a>
                </p>
            </section>
            <table class="spec-table">
                <thead>
                    <tr>
                        <th scope="col">{CHAPTER_COLUMN.of(lang)}</th>
                        <th scope="col">{TITLE_COLUMN.of(lang)}</th>
                        <th scope="col">{WORDS_COLUMN.of(lang)}</th>
                        <th scope="col"></th>
                    </tr>
                </thead>
                <tbody>{rows}</tbody>
            </table>
        },
    )
}

/// Renders `/spec/{slug}`.
///
/// Returns `None` when the chapter does not exist, so the caller can serve its
/// own 404 rather than an empty page.
#[must_use]
pub fn spec_page(
    book: &SpecBook,
    chapter: &SpecChapter,
    lang: Lang,
    viewer: Option<&Viewer>,
) -> String {
    let sidebar = sidebar(book, Some(&chapter.slug), lang);
    let title = primary_title(chapter, lang).to_string();
    let other = secondary_title(chapter, lang);
    let other = if other == title {
        String::new()
    } else {
        other.to_string()
    };
    let number = chapter.number.clone();
    let words = thousands(i64::try_from(chapter.words).unwrap_or(0));
    let badge = if chapter.normative {
        view! { <span class="badge">{NORMATIVE.of(lang)}</span> }.into_any()
    } else {
        view! { <span class="badge badge-quiet">{INFORMATIVE.of(lang)}</span> }.into_any()
    };
    let english = (lang == Lang::En)
        .then_some(chapter.html_en.as_ref())
        .flatten();
    let abstract_block = match (&chapter.summary_en, lang) {
        | _ if english.is_some() => ().into_any(),
        | (Some(summary), Lang::En) => {
            view! {
                <aside class="abstract">
                    <h2>{ABSTRACT.of(lang)}</h2>
                    <p>{summary.clone()}</p>
                    <p class="note">{NO_ENGLISH_YET.of(lang)}</p>
                    <p class="note">{NOT_NORMATIVE_NOTE.of(lang)}</p>
                </aside>
            }
            .into_any()
        },
        | _ => ().into_any(),
    };
    let (toc, html) = match english {
        | Some(html) => (toc_block(&chapter.toc_en, lang), html.clone()),
        | None => (toc_block(&chapter.toc, lang), chapter.html.clone()),
    };
    let previous = book.previous(&chapter.slug).map(|chapter| {
        (
            format!("/spec/{}", chapter.slug),
            list_title(chapter, lang).to_string(),
        )
    });
    let next = book.next(&chapter.slug).map(|chapter| {
        (
            format!("/spec/{}", chapter.slug),
            list_title(chapter, lang).to_string(),
        )
    });
    layout::page(
        lang,
        &title,
        "/spec",
        viewer,
        view! {
            <div class="spec-shell">
                {sidebar}
                <article class="spec-body">
                    <header class="spec-head">
                        <p class="eyebrow">{chapter_label(lang, &number)}</p>
                        <h1>{title.clone()}</h1>
                        <p class="subtitle">{other}</p>
                        <p class="meta">
                            {badge}
                            " "
                            {format!("{words} {}", WORDS.of(lang))}
                        </p>
                    </header>
                    {abstract_block}
                    {toc}
                    <div class="prose" inner_html=html></div>
                    {chapter_nav(lang, previous, next)}
                </article>
            </div>
        },
    )
}

/// Renders `/spec/all`: the single-page edition, produced on request.
#[must_use]
pub fn spec_all(
    book: &SpecBook,
    lang: Lang,
    viewer: Option<&Viewer>,
) -> String {
    let sidebar = sidebar(book, None, lang);
    let toc: Vec<AnyView> = book
        .chapters
        .iter()
        .map(|chapter| {
            let href = format!("#ch-{}", chapter.slug);
            let number = chapter.number.clone();
            let title = list_title(chapter, lang).to_string();
            view! {
                <li>
                    <a href=href>
                        <span class="num">{number}</span>
                        {title}
                    </a>
                </li>
            }
            .into_any()
        })
        .collect();
    let sections: Vec<AnyView> = book
        .chapters
        .iter()
        .map(|chapter| {
            let id = format!("ch-{}", chapter.slug);
            let title = list_title(chapter, lang).to_string();
            let other = secondary_title(chapter, lang);
            let other = if other == title {
                String::new()
            } else {
                other.to_string()
            };
            let html = match (lang, &chapter.html_en) {
                | (Lang::En, Some(html)) => html.clone(),
                | _ => chapter.html.clone(),
            };
            view! {
                <section class="spec-chapter" id=id>
                    <h2>{title}</h2>
                    <p class="subtitle">{other}</p>
                    <div class="prose" inner_html=html></div>
                </section>
            }
            .into_any()
        })
        .collect();
    let revision = book.revision.clone();
    layout::page(
        lang,
        TITLE.of(lang),
        "/spec",
        viewer,
        view! {
            <div class="spec-shell">
                {sidebar}
                <article class="spec-body">
                    <header class="spec-head">
                        <p class="eyebrow">
                            {format!("{} {revision}", REVISION.of(lang))}
                        </p>
                        <h1>{TITLE.of(lang)}</h1>
                        <p class="meta">
                            <a href="/spec">{BACK_TO_INDEX.of(lang)}</a>
                        </p>
                    </header>
                    <nav class="spec-toc" aria-label=CONTENTS.of(lang)>
                        <h2>{CONTENTS.of(lang)}</h2>
                        <ol>{toc}</ol>
                    </nav>
                    {sections}
                </article>
            </div>
        },
    )
}

/// The chapter list that sits beside every page.
fn sidebar(
    book: &SpecBook,
    current: Option<&str>,
    lang: Lang,
) -> AnyView {
    let items: Vec<AnyView> = book
        .chapters
        .iter()
        .map(|chapter| {
            let href = format!("/spec/{}", chapter.slug);
            let active = (Some(chapter.slug.as_str()) == current).then_some("page");
            let number = chapter.number.clone();
            let title = list_title(chapter, lang).to_string();
            let class = if active.is_some() {
                "spec-link current"
            } else {
                "spec-link"
            };
            view! {
                <li>
                    <a class=class href=href aria-current=active>
                        <span class="num">{number}</span>
                        <span>{title}</span>
                    </a>
                </li>
            }
            .into_any()
        })
        .collect();
    view! {
        <nav class="spec-nav" aria-label=CONTENTS.of(lang)>
            <a class="spec-home" href="/spec">{BACK_TO_INDEX.of(lang)}</a>
            <ol>{items}</ol>
            <a class="spec-all" href="/spec/all">{SINGLE_PAGE.of(lang)}</a>
        </nav>
    }
    .into_any()
}

/// The "on this page" list.
fn toc_block(
    toc: &[SpecHeading],
    lang: Lang,
) -> AnyView {
    let headings: Vec<AnyView> = toc
        .iter()
        .filter(|heading| heading.level == 2)
        .map(|heading| {
            let href = format!("#{}", crate::query_param(&heading.id));
            let class = if heading.level == 3 {
                "toc-h3"
            } else {
                "toc-h2"
            };
            view! {
                <li class=class>
                    <a href=href>{heading.text.clone()}</a>
                </li>
            }
            .into_any()
        })
        .collect();
    if headings.is_empty() {
        return ().into_any();
    }
    view! {
        <details class="onpage" open=true>
            <summary>{ON_THIS_PAGE.of(lang)}</summary>
            <ol>{headings}</ol>
        </details>
    }
    .into_any()
}

/// The previous/next pair at the foot of a chapter.
fn chapter_nav(
    lang: Lang,
    previous: Option<(String, String)>,
    next: Option<(String, String)>,
) -> AnyView {
    let previous_block = match previous {
        | Some((href, title)) => {
            view! {
                <a class="chapter-link previous" href=href>
                    <span class="direction">{PREVIOUS.of(lang)}</span>
                    <span class="title">{title}</span>
                </a>
            }
            .into_any()
        },
        | None => ().into_any(),
    };
    let next_block = match next {
        | Some((href, title)) => {
            view! {
                <a class="chapter-link next" href=href>
                    <span class="direction">{NEXT.of(lang)}</span>
                    <span class="title">{title}</span>
                </a>
            }
            .into_any()
        },
        | None => ().into_any(),
    };
    view! { <nav class="chapter-nav">{previous_block}{next_block}</nav> }.into_any()
}

/// The chapter's title in the reader's language.
///
/// The Chinese chapters carry a Chinese `#` heading and an English translation
/// in the store; the English index page is the reverse. `primary_title` picks
/// the one that matches the interface language, falling back to whatever the
/// chapter has.
fn primary_title(
    chapter: &SpecChapter,
    lang: Lang,
) -> &str {
    match lang {
        | Lang::En if !chapter.title_en.is_empty() => &chapter.title_en,
        | _ => &chapter.title,
    }
}

/// "chapter 04" / "第 04 章".
///
/// A template with a prefix would render "第 04", which is not Chinese; the
/// unit belongs after the number, so the two languages need their own shape.
fn chapter_label(
    lang: Lang,
    number: &str,
) -> String {
    match lang {
        | Lang::En => format!("{} {number}", CHAPTER.of(lang)),
        | Lang::Zh => format!("{}{number} 章", CHAPTER.of(lang)),
    }
}

/// The title a list shows: the short form, so the number column is not doubled.
fn list_title(
    chapter: &SpecChapter,
    lang: Lang,
) -> &str {
    match lang {
        | Lang::En if !chapter.title_en.is_empty() => &chapter.title_en,
        | _ => &chapter.short_title,
    }
}

/// The title in the *other* language, shown muted underneath.
fn secondary_title(
    chapter: &SpecChapter,
    lang: Lang,
) -> &str {
    match lang {
        | Lang::En => &chapter.title,
        | Lang::Zh if !chapter.title_en.is_empty() => &chapter.title_en,
        | Lang::Zh => "",
    }
}
