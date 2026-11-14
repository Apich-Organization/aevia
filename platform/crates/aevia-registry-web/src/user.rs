//! `/users/{handle}` — a publisher's public page.
//!
//! The account page is private (keys, sessions); this one is what a reader
//! reaches from a package's owner line: who published it, since when, and
//! everything else they publish. It shows no key material — the fingerprints
//! that matter are on each version.

use aevia_registry_core::Lang;
use aevia_registry_core::vm::UserPage;
use aevia_registry_core::vm::Viewer;
use leptos::prelude::AnyView;
use leptos::prelude::*;

use crate::Msg;
use crate::crates::card_in;
use crate::date_only;
use crate::layout;
use crate::layout::avatar;
use crate::layout::provider_badge;
use crate::thousands;

const PACKAGES: Msg = Msg::new("packages", "包");
const NO_PACKAGES: Msg = Msg::new("This publisher has no packages yet.", "该发布者尚无包。");
const JOINED: Msg = Msg::new("joined", "加入于");
const DOWNLOADS: Msg = Msg::new("downloads", "下载");
const VERIFY_AT: Msg = Msg::new("verify this identity at", "在此核实身份：");

/// Renders `/users/{handle}`.
pub fn user_page(
    lang: Lang,
    page: &UserPage,
    viewer: Option<&Viewer>,
) -> String {
    let handle = page.owner.handle.clone();
    let title = page
        .owner
        .display_name
        .clone()
        .filter(|name| !name.is_empty())
        .unwrap_or_else(|| handle.clone());
    let profile = page.owner.profile_url();
    let profile_label = profile.clone();
    let joined = date_only(&page.joined_at).to_string();
    let joined_at = page.joined_at.clone();
    let downloads = thousands(page.downloads);
    let count = thousands(i64::try_from(page.packages.len()).unwrap_or(0));
    let cards: Vec<AnyView> = page
        .packages
        .iter()
        .map(|item| card_in(lang, item, &format!("/crates/{}", item.id.to_path())))
        .collect();
    let packages = if cards.is_empty() {
        view! { <p class="muted">{NO_PACKAGES.of(lang)}</p> }.into_any()
    } else {
        view! { <div class="cards">{cards}</div> }.into_any()
    };
    let page_title = title.clone();
    layout::page(
        lang,
        &page_title,
        "",
        viewer,
        view! {
            <section class="page-head">
                <h1>
                    {avatar(&page.owner)}
                    " "
                    {title}
                    " "
                    {provider_badge(&page.owner)}
                </h1>
                <p>
                    <code>{handle}</code>
                    " · "
                    {JOINED.of(lang)}
                    " "
                    <time datetime=joined_at>{joined}</time>
                    " · "
                    {format!("{count} {}", PACKAGES.of(lang))}
                    " · "
                    {format!("{downloads} {}", DOWNLOADS.of(lang))}
                </p>
                <p class="muted">
                    {VERIFY_AT.of(lang)}
                    " "
                    <a href=profile rel="noreferrer">{profile_label}</a>
                </p>
            </section>
            <section>
                <h2>{PACKAGES.of(lang)}</h2>
                {packages}
            </section>
        },
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use aevia_registry_core::vm::CrateCard;
    use aevia_registry_core::vm::OwnerView;

    fn page() -> UserPage {
        UserPage {
            owner: OwnerView {
                kind: "github".to_string(),
                handle: "apich".to_string(),
                display_name: Some("Apich <Org>".to_string()),
                avatar_url: None,
            },
            joined_at: "2026-10-01T08:00:00Z".to_string(),
            downloads: 4096,
            packages: vec![CrateCard {
                id: "apich/units".parse().expect("valid identity"),
                description: Some("Units".to_string()),
                version: "1.2.0".to_string(),
                updated_at: "2026-11-01T10:00:00Z".to_string(),
                downloads: 4096,
                has_docs: true,
            }],
        }
    }

    #[test]
    fn a_profile_lists_the_packages_and_escapes_the_name() {
        let html = user_page(Lang::En, &page(), None);
        assert!(html.contains("apich/"), "owner prefix of the card");
        assert!(html.contains("Apich &lt;Org&gt;"), "display name is escaped");
        assert!(html.contains("4,096"), "downloads grouped");
        assert!(html.contains("2026-10-01"), "join date");
        assert!(html.contains("https://github.com/apich"), "verification link");
    }

    #[test]
    fn an_empty_profile_says_so_in_chinese() {
        let mut empty = page();
        empty.packages.clear();
        let html = user_page(Lang::Zh, &empty, None);
        assert!(html.contains("该发布者尚无包"));
    }
}
