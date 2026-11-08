//! The account page and the device-activation page.
//!
//! Both are about the same object — the publisher's signing keys — from two
//! sides. The account page is where keys are bound and revoked; the activation
//! page is where a command-line login is confirmed. They live together because
//! a reader debugging a publish will move between them, and the vocabulary
//! (kind, fingerprint, label) has to match.

use aevia_registry_core::vm::AccountPage;
use aevia_registry_core::vm::Viewer;
use leptos::prelude::AnyView;
use leptos::prelude::*;

use crate::crates::card;
use crate::date_only;
use crate::layout;
use crate::layout::avatar;
use crate::thousands;

/// Renders `/account`.
///
/// The revoke control is a form per key rather than a single form with a
/// select: revocation is destructive and should take one deliberate click on
/// the row that names the key, not a choice made in a dropdown.
pub fn account(page: &AccountPage) -> String {
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
                view! { <span class="badge badge-yank">"revoked"</span> }.into_any()
            } else {
                let id = key.id;
                view! {
                    <form class="inline" method="post" action=format!("/account/keys/{id}/revoke")>
                        <button type="submit" class="btn btn-ghost btn-warn">"revoke"</button>
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
            <p class="muted">
                "No signing keys are bound. Publishing requires one."
            </p>
        }
        .into_any()
    } else {
        view! {
            <table>
                <thead>
                    <tr>
                        <th scope="col">"kind"</th>
                        <th scope="col">"fingerprint"</th>
                        <th scope="col">"label"</th>
                        <th scope="col">"bound"</th>
                        <th scope="col">"state"</th>
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
        .map(|item| card(item, &format!("/crates/{}", item.id.to_path())))
        .collect();

    let package_section = if page.packages.is_empty() {
        view! {
            <p class="muted">
                "No packages yet. Publish with "
                <code>"aevia pkg publish"</code>
                "."
            </p>
        }
        .into_any()
    } else {
        view! { <div class="cards">{packages}</div> }.into_any()
    };

    let key_count = thousands(i64::try_from(page.keys.len()).unwrap_or(i64::MAX));
    let package_count = thousands(i64::try_from(page.packages.len()).unwrap_or(i64::MAX));

    layout::page(
        "account",
        "/account",
        Some(&page.viewer),
        view! {
            <section class="page-head">
                <h1>"account"</h1>
                <p>"Your identity, signing keys and packages."</p>
            </section>
            <div class="viewer-card">
                {avatar(&owner)}
                <div>
                    <p class="viewer-name">{display_name}</p>
                    <p class="viewer-meta">
                        <a href=profile>{handle}</a>
                        {format!(" · {provider} · {package_count} packages · {key_count} keys")}
                    </p>
                </div>
            </div>
            <section>
                <h2>"signing keys"</h2>
                {key_table}
            </section>
            <section>
                <h2>"add a signing key"</h2>
                <p class="muted">
                    "Paste an ASCII-armoured public key. The registry checks that the key can \
                     verify a signature before it binds it."
                </p>
                <form class="form-grid" method="post" action="/account/keys">
                    <label>
                        "public key"
                        <textarea
                            name="armored"
                            rows="8"
                            placeholder="-----BEGIN PGP PUBLIC KEY BLOCK-----"
                            required
                        ></textarea>
                    </label>
                    <label>
                        "label"
                        <input type="text" name="label" placeholder="laptop, CI, offline"/>
                    </label>
                    <div>
                        <button type="submit" class="btn">"bind key"</button>
                    </div>
                </form>
            </section>
            <section>
                <h2>"your packages"</h2>
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
    user_code: &str,
    viewer: Option<&Viewer>,
    key_kind: &str,
    fingerprint: &str,
) -> String {
    layout::page(
        "approve this device",
        "/account",
        viewer,
        view! {
            <section class="activate">
                <h1>"approve this device"</h1>
                <p>
                    "A command-line client is asking to sign in as you. Confirm that the code \
                     below is the one shown in the terminal."
                </p>
                <p class="code">{user_code.to_string()}</p>
                <dl class="meta">
                    <div>
                        <dt>"key kind"</dt>
                        <dd>{key_kind.to_string()}</dd>
                    </div>
                    <div>
                        <dt>"fingerprint"</dt>
                        <dd class="mono">{fingerprint.to_string()}</dd>
                    </div>
                </dl>
                <form class="actions" method="post" action="/activate">
                    <input type="hidden" name="user_code" value=user_code.to_string()/>
                    <button type="submit" class="btn">"approve this device"</button>
                    <a class="btn btn-ghost" href="/">"cancel"</a>
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
        let html = account(&account_page());
        assert!(html.contains("/account/keys/7/revoke"));
        assert!(html.contains("0123456789ABCDEF"));
        assert!(html.contains("laptop"));
        assert!(html.contains("No packages yet."));
    }

    #[test]
    fn an_activation_page_carries_the_code_into_the_form() {
        let html = activate("ABCD-1234", None, "openpgp", "0123456789ABCDEF");
        assert!(html.contains("ABCD-1234"));
        assert!(html.contains("name=\"user_code\" value=\"ABCD-1234\""));
        assert!(html.contains("openpgp"));
    }
}
