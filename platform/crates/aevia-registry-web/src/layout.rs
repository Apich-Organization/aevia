//! The chrome every page shares: document head, top bar, footer.
//!
//! The layout is a function rather than a Leptos component because pages are
//! rendered one at a time and the result is a `String`. That keeps the page
//! functions free of component wiring and means a missing viewer, or an
//! identity without an avatar, cannot fail to render — the two states the
//! chrome has are both ordinary values here.
//!
//! # Language
//!
//! Every label in the chrome is a [`Msg`], so the top bar, the footer and the
//! skip link are translated. The language switch stores the reader's
//! preference, while explicit documentation URLs carry `?lang=en` or
//! `?lang=zh` so links are stable when shared.

use aevia_registry_core::Lang;
use aevia_registry_core::vm::OwnerView;
use aevia_registry_core::vm::Viewer;
use leptos::prelude::AnyView;
use leptos::prelude::IntoAny;
use leptos::prelude::*;

use crate::Msg;
use crate::lang_attr;
use crate::render;

const SKIP: Msg = Msg::new("skip to content", "跳到正文");
const NAV_CRATES: Msg = Msg::new("crates", "包");
const NAV_DOCS: Msg = Msg::new("docs", "文档");
const NAV_STDLIB: Msg = Msg::new("stdlib", "标准库");
const NAV_CLI: Msg = Msg::new("cli", "命令行");
const NAV_SPEC: Msg = Msg::new("spec", "语言规范");
const NAV_BOOK: Msg = Msg::new("book", "教程");
const NAV_ARCHITECTURE: Msg = Msg::new("architecture", "架构");
const NAV_PRIMARY: Msg = Msg::new("Primary", "主导航");
const SIGN_IN: Msg = Msg::new("sign in", "登录");
const ACCOUNT: Msg = Msg::new("account", "账户");
const SIGN_OUT: Msg = Msg::new("sign out", "退出");
const FOOTER_WHAT: Msg = Msg::new("Aevia package registry", "Aevia 包注册中心");
const FOOTER_TEXT: Msg = Msg::new("Documentation in plain text", "纯文本文档");
const FOOTER_SITES: Msg = Msg::new("Sibling sites", "相关站点");
const SWITCH_TO: Msg = Msg::new("switch language", "切换语言");

/// Wraps a page body in the full document.
///
/// `here` is the section the page belongs to, used only to mark the matching
/// top-bar link with `aria-current`; an unknown value simply marks nothing,
/// which is the right behaviour for a page that is not in the nav.
pub fn page<V>(
    lang: Lang,
    title: &str,
    here: &str,
    viewer: Option<&Viewer>,
    body: V,
) -> String
where
    V: RenderHtml,
{
    let description = match lang {
        | Lang::En => {
            format!(
                "{title}. Aevia scientific computing language, documentation and package registry."
            )
        },
        | Lang::Zh => format!("{title}。Aevia 科学计算语言、文档与包注册中心。"),
    };
    let document = view! {
        <html lang=lang_attr(lang)>
            <head>
                <meta charset="utf-8"/>
                <meta name="description" content=description/>
                <meta name="viewport" content="width=device-width, initial-scale=1"/>
                <title>{format!("{title} · aevia")}</title>
                <link rel="stylesheet" href="/static/site.css"/>
                <link rel="icon" href="data:,"/>
            </head>
            <body>
                <a class="skip" href="#main">{SKIP.of(lang)}</a>
                <header class="topbar">
                    <a class="wordmark" href="/">"aevia"</a>
                    <nav class="topnav" aria-label=NAV_PRIMARY.of(lang)>
                        {nav_link(lang, "/crates", NAV_CRATES, here)}
                        {nav_link(lang, "/docs", NAV_DOCS, here)}
                        {nav_link(lang, "/stdlib", NAV_STDLIB, here)}
                        {nav_link(lang, "/cli", NAV_CLI, here)}
                        {nav_link(lang, "/spec", NAV_SPEC, here)}
                        {nav_link(lang, "/book", NAV_BOOK, here)}
                        {nav_link(lang, "/architecture", NAV_ARCHITECTURE, here)}
                        <a href="https://github.com/Apich-Organization/aevia" rel="noreferrer">
                            "github"
                        </a>
                    </nav>
                    <div class="topbar-right">
                        {language_switch(lang)}
                        {viewer_chrome(lang, viewer)}
                    </div>
                </header>
                <main id="main">{body}</main>
                <footer class="footer">
                    <div class="footer-inner">
                        <p>{FOOTER_WHAT.of(lang)}</p>
                        <nav aria-label=FOOTER_SITES.of(lang)>
                            <a href="https://aevia.apich.org">"aevia.apich.org"</a>
                            <a href="https://rssn.apich.org">"rssn.apich.org"</a>
                            <a href=format!("/llms.txt?lang={}", lang.code())>{FOOTER_TEXT.of(lang)}</a>
                        </nav>
                    </div>
                </footer>
                <script src="/static/app.js" defer></script>
            </body>
        </html>
    };
    format!("<!DOCTYPE html>{}", render(|| document))
}

/// One top-bar link, marked when it points at the current section.
fn nav_link(
    lang: Lang,
    href: &'static str,
    label: Msg,
    here: &str,
) -> AnyView {
    let current = (href == here).then_some("page");
    view! { <a href=href aria-current=current>{label.of(lang)}</a> }.into_any()
}

/// The two-language switch.
///
/// Both languages are listed rather than a single toggle button: a reader who
/// does not read the current language cannot be expected to recognise a
/// two-letter code, but they can recognise their own language's name.
#[allow(
    clippy::single_call_fn,
    reason = "the language selector owns language links and accessibility labels"
)]
fn language_switch(lang: Lang) -> AnyView {
    let link = |target: Lang| {
        let current = (target == lang).then_some("page");
        view! {
            <a
                class="lang"
                href=format!("/lang/{}", target.code())
                aria-current=current
                lang=target.code()
            >
                {target.label()}
            </a>
        }
        .into_any()
    };
    view! {
        <div class="langswitch" aria-label=SWITCH_TO.of(lang)>
            {link(Lang::En)}
            {link(Lang::Zh)}
        </div>
    }
    .into_any()
}

/// The right-hand side of the top bar: either a sign-in entry point or the
/// viewer's identity and the two actions that follow from being signed in.
///
/// The sign-out control is a form because signing out is a state change; a
/// link would let a prefetcher or a crawler end a session.
#[allow(
    clippy::single_call_fn,
    reason = "the signed-in and signed-out navigation states share one layout section"
)]
fn viewer_chrome(
    lang: Lang,
    viewer: Option<&Viewer>,
) -> AnyView {
    match viewer {
        | None => {
            view! {
                <a class="btn btn-ghost" href="/login">{SIGN_IN.of(lang)}</a>
            }
            .into_any()
        },
        | Some(viewer) => {
            let handle = viewer.handle.clone();
            let profile = viewer.as_owner().profile_url();
            view! {
                <div class="who">
                    {avatar(&viewer.as_owner())}
                    <a class="who-handle" href=profile>{handle}</a>
                    <a href="/account">{ACCOUNT.of(lang)}</a>
                    <form class="inline" method="post" action="/logout">
                        <button type="submit" class="linkish">{SIGN_OUT.of(lang)}</button>
                    </form>
                </div>
            }
            .into_any()
        },
    }
}

/// A publisher's avatar, with a monogram when the provider did not give one.
///
/// The fallback is not decoration: an ORCID iD has no avatar, and a broken
/// image icon next to every ORCID owner would read as a registry fault rather
/// than as a provider without pictures.
pub fn avatar(owner: &OwnerView) -> AnyView {
    match &owner.avatar_url {
        | Some(url) if !url.is_empty() => {
            view! {
                <img class="avatar" src=url.clone() alt="" width="20" height="20" loading="lazy"/>
            }
            .into_any()
        },
        | _ => view! {
            <span class="avatar avatar-fallback" aria-hidden="true">{initial(&owner.handle)}</span>
        }
        .into_any(),
    }
}

/// The first character of a handle, for the avatar fallback.
#[allow(
    clippy::single_call_fn,
    reason = "avatar fallback extracts and uppercases a Unicode character"
)]
fn initial(handle: &str) -> String {
    handle.chars().next().map_or_else(
        || "?".to_string(),
        |character| character.to_uppercase().to_string(),
    )
}

/// A badge naming the identity provider.
pub fn provider_badge(owner: &OwnerView) -> AnyView {
    view! { <span class="badge">{owner.provider()}</span> }.into_any()
}
