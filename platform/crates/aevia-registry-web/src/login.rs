//! The sign-in page.
//!
//! It exists so that the choice of identity provider is the reader's, not the
//! server's. Redirecting `/login` straight at GitHub would quietly demote ORCID
//! researchers to second-class users, and this registry is for a scientific
//! language where an ORCID iD is the more natural identifier.
//!
//! The page also states the model plainly — no password is stored, the provider
//! vouches for the handle — because a sign-in page is where a cautious reader
//! decides whether to trust the site at all.

use aevia_registry_core::Lang;
use aevia_registry_core::vm::Viewer;
use leptos::prelude::*;

use crate::Msg;
use crate::layout;
use crate::query_param;

const TITLE: Msg = Msg::new("sign in", "登录");
const LEDE: Msg = Msg::new(
    "The registry keeps no passwords. Your provider vouches for your handle, and a key you hold      signs every publish.",
    "注册中心不保存任何密码。由你的身份提供方证明你的账号，每一次发布都由你持有的密钥签名。",
);
const GITHUB: Msg = Msg::new("Continue with GitHub", "使用 GitHub 登录");
const GITHUB_NOTE: Msg = Msg::new(
    "also enables publishing under an organisation",
    "同时可在组织名下发布",
);
const ORCID: Msg = Msg::new("Continue with ORCID", "使用 ORCID 登录");
const ORCID_NOTE: Msg = Msg::new(
    "for researchers who already have an iD",
    "适合已有 ORCID iD 的研究者",
);
const NOT_CONFIGURED: Msg = Msg::new("not configured on this registry", "本次部署未配置");
const NOTE: Msg = Msg::new(
    "Signing in creates a publisher identity only. Publishing needs a signing key bound to it: run ",
    "登录只会创建一个发布者身份。发布还需要绑定到它的签名密钥：运行 ",
);
const NOTE_TAIL: Msg = Msg::new(
    " and approve the code it prints.",
    "，并在浏览器里批准它打印出的验证码。",
);

/// Renders `/login`.
///
/// `github` and `orcid` say which providers this deployment has configured; a
/// provider that is not configured is shown as unavailable rather than hidden,
/// so an operator can see at a glance what is missing.
pub fn login(
    lang: Lang,
    viewer: Option<&Viewer>,
    next: &str,
    github: bool,
    orcid: bool,
) -> String {
    let next_github = format!("/auth/github/start?next={}", query_param(next));
    let next_orcid = format!("/auth/orcid/start?next={}", query_param(next));
    layout::page(
        lang,
        TITLE.of(lang),
        "",
        viewer,
        view! {
            <section class="signin">
                <h1>{TITLE.of(lang)}</h1>
                <p class="lede">{LEDE.of(lang)}</p>
                <div class="provider-list">
                    {if github {
                        view! {
                            <a class="provider" href=next_github>
                                <span class="provider-name">{GITHUB.of(lang)}</span>
                                <span class="provider-note">{GITHUB_NOTE.of(lang)}</span>
                            </a>
                        }
                        .into_any()
                    } else {
                        view! {
                            <span class="provider provider-off">
                                <span class="provider-name">"GitHub"</span>
                                <span class="provider-note">{NOT_CONFIGURED.of(lang)}</span>
                            </span>
                        }
                        .into_any()
                    }}
                    {if orcid {
                        view! {
                            <a class="provider" href=next_orcid>
                                <span class="provider-name">{ORCID.of(lang)}</span>
                                <span class="provider-note">{ORCID_NOTE.of(lang)}</span>
                            </a>
                        }
                        .into_any()
                    } else {
                        view! {
                            <span class="provider provider-off">
                                <span class="provider-name">"ORCID"</span>
                                <span class="provider-note">{NOT_CONFIGURED.of(lang)}</span>
                            </span>
                        }
                        .into_any()
                    }}
                </div>
                <p class="note">
                    {NOTE.of(lang)}
                    <code>"aevia-pkg login"</code>
                    {NOTE_TAIL.of(lang)}
                </p>
            </section>
        },
    )
}
