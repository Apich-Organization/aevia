//! The account page and the device-activation page.
//!
//! Both are about the same object — the publisher's signing keys — from two
//! sides. The account page is where keys are bound and revoked; the activation
//! page is where a command-line login is confirmed. They live together because
//! a reader debugging a publish will move between them, and the vocabulary
//! (kind, fingerprint, label) has to match.

use aevia_registry_core::Lang;
use aevia_registry_core::vm::AccountPage;
use aevia_registry_core::vm::Viewer;
use leptos::prelude::AnyView;
use leptos::prelude::*;

use crate::Msg;
use crate::crates::card_in;
use crate::date_only;
use crate::layout;
use crate::layout::avatar;
use crate::thousands;

// Page chrome.
const TITLE: Msg = Msg::new("account", "账户");
const LEDE: Msg = Msg::new(
    "Your identity, signing keys and packages.",
    "你的身份、签名密钥和包。",
);

// The viewer line.
const PACKAGE_SUFFIX: Msg = Msg::new("packages", "个包");
const KEY_SUFFIX: Msg = Msg::new("keys", "个密钥");

// Signing keys.
const SIGNING_KEYS: Msg = Msg::new("signing keys", "签名密钥");
const KIND: Msg = Msg::new("kind", "类型");
const FINGERPRINT: Msg = Msg::new("fingerprint", "指纹");
const LABEL: Msg = Msg::new("label", "标签");
const BOUND: Msg = Msg::new("bound", "绑定于");
const STATE: Msg = Msg::new("state", "状态");
const NO_KEYS: Msg = Msg::new(
    "No signing keys are bound. Publishing requires one.",
    "尚未绑定签名密钥。发布必须先绑定一个。",
);
const REVOKED: Msg = Msg::new("revoked", "已吊销");
const REVOKE: Msg = Msg::new("revoke", "吊销");

// Adding a key.
const ADD_KEY: Msg = Msg::new("add a signing key", "添加签名密钥");
const ADD_KEY_NOTE: Msg = Msg::new(
    "Paste an ASCII-armoured public key. The registry checks that the key can \
     verify a signature before it binds it.",
    "粘贴 ASCII 装甲格式的公钥。注册中心会先确认该密钥能够验证签名，再将其绑定。",
);
const PUBLIC_KEY: Msg = Msg::new("public key", "公钥");
const LABEL_PLACEHOLDER: Msg = Msg::new("laptop, CI, offline", "笔记本、CI、离线");
const BIND_KEY: Msg = Msg::new("bind key", "绑定密钥");

// The viewer's own packages.
const YOUR_PACKAGES: Msg = Msg::new("your packages", "你的包");
const NO_PACKAGES: Msg = Msg::new("No packages yet. Publish with ", "还没有包。使用 ");
const NO_PACKAGES_END: Msg = Msg::new(".", " 发布。");

// Device activation.
const APPROVE: Msg = Msg::new("approve this device", "批准此设备");
const APPROVE_NOTE: Msg = Msg::new(
    "A command-line client is asking to sign in as you. Confirm that the code \
     below is the one shown in the terminal.",
    "一个命令行客户端正在请求以你的身份登录。请确认下方验证码与终端中显示的一致。",
);
const KEY_KIND: Msg = Msg::new("key kind", "密钥类型");
const CANCEL: Msg = Msg::new("cancel", "取消");

/// Renders `/account`.
///
/// The revoke control is a form per key rather than a single form with a
/// select: revocation is destructive and should take one deliberate click on
/// the row that names the key, not a choice made in a dropdown.
pub fn account(
    lang: Lang,
    page: &AccountPage,
) -> String {
    let owner = page.viewer.as_owner();
    let handle = page.viewer.handle.clone();
    let display_name = page
        .viewer
        .display_name
        .clone()
        .unwrap_or_else(|| page.viewer.handle.clone());
    let profile = owner.profile_url();
    let provider = page.viewer.as_owner().provider();

    let key_rows: Vec<AnyView> = page
        .keys
        .iter()
        .map(|key| {
            let kind = key.kind.clone();
            let fingerprint = key.fingerprint.clone();
            let label = key.label.clone().unwrap_or_else(|| "—".to_string());
            let created_at = key.created_at.clone();
            let created = date_only(&key.created_at).to_string();
            let state = if key.revoked {
                view! { <span class="badge badge-yank">{REVOKED.of(lang)}</span> }.into_any()
            } else {
                let id = key.id;
                view! {
                    <form class="inline" method="post" action=format!("/account/keys/{id}/revoke")>
                        <button type="submit" class="btn btn-ghost btn-warn">
                            {REVOKE.of(lang)}
                        </button>
                    </form>
                }
                .into_any()
            };
            view! {
                <tr>
                    <th scope="row">{kind}</th>
                    <td>
                        <code class="mono">{fingerprint}</code>
                    </td>
                    <td>{label}</td>
                    <td>
                        <time datetime=created_at>{created}</time>
                    </td>
                    <td>{state}</td>
                </tr>
            }
            .into_any()
        })
        .collect();

    let key_table = if page.keys.is_empty() {
        view! {
            <p class="muted">{NO_KEYS.of(lang)}</p>
        }
        .into_any()
    } else {
        view! {
            <table>
                <thead>
                    <tr>
                        <th scope="col">{KIND.of(lang)}</th>
                        <th scope="col">{FINGERPRINT.of(lang)}</th>
                        <th scope="col">{LABEL.of(lang)}</th>
                        <th scope="col">{BOUND.of(lang)}</th>
                        <th scope="col">{STATE.of(lang)}</th>
                    </tr>
                </thead>
                <tbody>{key_rows}</tbody>
            </table>
        }
        .into_any()
    };

    let packages: Vec<AnyView> = page
        .packages
        .iter()
        .map(|item| card_in(lang, item, &format!("/crates/{}", item.id.to_path())))
        .collect();

    let package_section = if page.packages.is_empty() {
        view! {
            <p class="muted">
                {NO_PACKAGES.of(lang)}
                <code>"aevia pkg publish"</code>
                {NO_PACKAGES_END.of(lang)}
            </p>
        }
        .into_any()
    } else {
        view! { <div class="cards">{packages}</div> }.into_any()
    };

    let key_count = thousands(i64::try_from(page.keys.len()).unwrap_or(i64::MAX));
    let package_count = thousands(i64::try_from(page.packages.len()).unwrap_or(i64::MAX));
    let counts = format!(
        " · {provider} · {package_count} {} · {key_count} {}",
        PACKAGE_SUFFIX.of(lang),
        KEY_SUFFIX.of(lang)
    );

    layout::page(
        lang,
        TITLE.of(lang),
        "/account",
        Some(&page.viewer),
        view! {
            <section class="page-head">
                <h1>{TITLE.of(lang)}</h1>
                <p>{LEDE.of(lang)}</p>
            </section>
            <div class="viewer-card">
                {avatar(&owner)}
                <div>
                    <p class="viewer-name">{display_name}</p>
                    <p class="viewer-meta">
                        <a href=profile>{handle}</a>
                        {counts}
                    </p>
                </div>
            </div>
            <section>
                <h2>{SIGNING_KEYS.of(lang)}</h2>
                {key_table}
            </section>
            <section>
                <h2>{ADD_KEY.of(lang)}</h2>
                <p class="muted">{ADD_KEY_NOTE.of(lang)}</p>
                <form class="form-grid" method="post" action="/account/keys">
                    <label>
                        {PUBLIC_KEY.of(lang)}
                        <textarea
                            name="armored"
                            rows="8"
                            placeholder="-----BEGIN PGP PUBLIC KEY BLOCK-----"
                            required
                        ></textarea>
                    </label>
                    <label>
                        {LABEL.of(lang)}
                        <input
                            type="text"
                            name="label"
                            placeholder=LABEL_PLACEHOLDER.of(lang)
                        />
                    </label>
                    <div>
                        <button type="submit" class="btn">{BIND_KEY.of(lang)}</button>
                    </div>
                </form>
            </section>
            <section>
                <h2>{YOUR_PACKAGES.of(lang)}</h2>
                {package_section}
            </section>
        },
    )
}

/// Renders `/activate?code=…`.
///
/// The confirmation is a POST carrying the same code the page was opened with,
/// so that approving a device is an explicit action by the signed-in viewer and
/// not a side effect of following a link someone else sent them.
pub fn activate(
    lang: Lang,
    user_code: &str,
    viewer: Option<&Viewer>,
    key_kind: &str,
    fingerprint: &str,
) -> String {
    layout::page(
        lang,
        APPROVE.of(lang),
        "/account",
        viewer,
        view! {
            <section class="activate">
                <h1>{APPROVE.of(lang)}</h1>
                <p>{APPROVE_NOTE.of(lang)}</p>
                <p class="code">{user_code.to_string()}</p>
                <dl class="meta">
                    <div>
                        <dt>{KEY_KIND.of(lang)}</dt>
                        <dd>{key_kind.to_string()}</dd>
                    </div>
                    <div>
                        <dt>{FINGERPRINT.of(lang)}</dt>
                        <dd class="mono">{fingerprint.to_string()}</dd>
                    </div>
                </dl>
                <form class="actions" method="post" action="/activate">
                    <input type="hidden" name="user_code" value=user_code.to_string()/>
                    <button type="submit" class="btn">{APPROVE.of(lang)}</button>
                    <a class="btn btn-ghost" href="/">{CANCEL.of(lang)}</a>
                </form>
            </section>
        },
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use aevia_registry_core::vm::KeyView;

    fn account_page() -> AccountPage {
        AccountPage {
            viewer: Viewer {
                handle: "apich".to_string(),
                kind: "github".to_string(),
                display_name: Some("Apich".to_string()),
                avatar_url: None,
            },
            keys: vec![KeyView {
                id: 7,
                kind: "openpgp".to_string(),
                fingerprint: "0123456789ABCDEF".to_string(),
                label: Some("laptop".to_string()),
                created_at: "2026-01-02T00:00:00Z".to_string(),
                revoked: false,
            }],
            packages: Vec::new(),
        }
    }

    #[test]
    fn an_account_page_names_the_key_and_offers_revocation() {
        let html = account(Lang::En, &account_page());
        assert!(html.contains("/account/keys/7/revoke"));
        assert!(html.contains("0123456789ABCDEF"));
        assert!(html.contains("laptop"));
        assert!(html.contains("No packages yet."));
    }

    #[test]
    fn an_account_page_renders_in_chinese() {
        let html = account(Lang::Zh, &account_page());
        assert!(html.contains(r#"<html lang="zh-CN">"#));
        assert!(html.contains("签名密钥"));
        assert!(html.contains("还没有包。"));
    }

    #[test]
    fn an_activation_page_carries_the_code_into_the_form() {
        let html = activate(Lang::En, "ABCD-1234", None, "openpgp", "0123456789ABCDEF");
        assert!(html.contains("ABCD-1234"));
        assert!(html.contains("name=\"user_code\" value=\"ABCD-1234\""));
        assert!(html.contains("openpgp"));
    }
}
