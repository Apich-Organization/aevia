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
//! skip link are translated. The switcher is a link to `/lang/{code}`, which
//! sets a cookie and sends the reader back to the page they were on: a language
//! is a preference about a reader, not about a URL, so it belongs in a cookie
//! rather than in every link on the site.

use aevia_registry_core::Lang;
use aevia_registry_core::vm::OwnerView;
use aevia_registry_core::vm::Viewer;
use leptos::prelude::AnyView;
use leptos::prelude::IntoAny;
use leptos::prelude::*;

use crate::Msg;
use crate::lang_attr;
use crate::render;
use crate::styles::APP_JS;

/// The language-spec revision the registry serves.
///
/// The page functions receive no spec metadata — the view models describe
/// packages, not the toolchain — so the footer states a value compiled into
/// this crate. It is one place to change when the served revision moves, and
/// keeping it honest matters: a reader uses it to check that the compiler they
/// run is the one the documentation was built from.
const SPEC_COMMIT: &str = "aevia-spec 0.1";

const SKIP: Msg = Msg::new("skip to content", "跳到正文");
const NAV_CRATES: Msg = Msg::new("crates", "包");
const NAV_DOCS: Msg = Msg::new("docs", "文档");
const NAV_SPEC: Msg = Msg::new("spec", "语言规范");
const NAV_PRIMARY: Msg = Msg::new("Primary", "主导航");
const SIGN_IN: Msg = Msg::new("sign in", "登录");
const ACCOUNT: Msg = Msg::new("account", "账户");
const SIGN_OUT: Msg = Msg::new("sign out", "退出");
const FOOTER_WHAT: Msg = Msg::new("Aevia package registry", "Aevia 包注册中心");
const FOOTER_SPEC: Msg = Msg::new("serving the language spec at", "所服务的语言规范版本");
const FOOTER_SITES: Msg = Msg::new("Sibling sites", "相关站点");
const SWITCH_TO: Msg = Msg::new("switch language", "切换语言");

/// Wraps a page body in the full document.
///
/// `here` is the section the page belongs to, used only to mark the matching
/// top-bar link with `aria-current`; an unknown value simply marks nothing,
/// which is the right behaviour for a page that is not in the nav.
pub(crate) fn page<V>(
    lang: Lang,
    title: &str,
    here: &str,
    viewer: Option<&Viewer>,
    body: V,
) -> String
where
    V: RenderHtml,
{
    let document = view! {
        <html lang=lang_attr(lang)>
            <head>
                <meta charset="utf-8"/>
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
                        {nav_link(lang, "/spec", NAV_SPEC, here)}
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
                        <p>
                            {FOOTER_SPEC.of(lang)}
                            " "
                            <code>{SPEC_COMMIT}</code>
                        </p>
                        <nav aria-label=FOOTER_SITES.of(lang)>
                            <a href="https://aevia.apich.org">"aevia.apich.org"</a>
                            <a href="https://rssn.apich.org">"rssn.apich.org"</a>
                        </nav>
                    </div>
                </footer>
                <script>{APP_JS}</script>
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
fn viewer_chrome(
    lang: Lang,
    viewer: Option<&Viewer>,
) -> AnyView {
    match viewer {
        | None => view! {
            <a class="btn btn-ghost" href="/login">{SIGN_IN.of(lang)}</a>
        }
        .into_any(),
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
pub(crate) fn avatar(owner: &OwnerView) -> AnyView {
    match &owner.avatar_url {
        | Some(url) if !url.is_empty() => view! {
            <img class="avatar" src=url.clone() alt="" width="20" height="20" loading="lazy"/>
        }
        .into_any(),
        | _ => view! {
            <span class="avatar avatar-fallback" aria-hidden="true">{initial(&owner.handle)}</span>
        }
        .into_any(),
    }
}

/// The first character of a handle, for the avatar fallback.
fn initial(handle: &str) -> String {
    handle
        .chars()
        .next()
        .map(|character| character.to_uppercase().to_string())
        .unwrap_or_else(|| "?".to_string())
}

/// A badge naming the identity provider.
pub(crate) fn provider_badge(owner: &OwnerView) -> AnyView {
    view! { <span class="badge">{owner.provider()}</span> }.into_any()
}
