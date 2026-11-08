//! The front page.
//!
//! This is the only page that has to answer, in one screen, what the registry
//! is, how to install from it, and why it is not crates.io. The order of the
//! sections is that argument: a sentence, the command, an example of the
//! language, the size of the registry, then the three properties that make it
//! worth using.

use aevia_registry_core::vm::CrateCard;
use aevia_registry_core::vm::SiteStats;
use aevia_registry_core::vm::Viewer;
use leptos::prelude::AnyView;
use leptos::prelude::*;

use crate::crates::card;
use crate::layout;
use crate::thousands;

/// The snippet in the hero.
///
/// It is the kinematics example from the language tour, deliberately small:
/// three lines that show a signature, a `##` doc comment and a call with a
/// dimensioned literal, which is the shortest honest sample of what a package
/// on this registry contains.
const SNIPPET: &str = "fn fall_time(h: Real[L]): Real[T] = sqrt(2 * h / g)\n\n## The time a body takes to fall from height `h` under gravity `g`.\nlet t = fall_time(3.2 m)";

/// Renders `/`.
///
/// `recent` is truncated to six here rather than by the caller: the front page
/// is the only consumer that wants a fixed count, and keeping the cut next to
/// the grid means the grid and the count cannot drift apart.
pub fn home(
    stats: &SiteStats,
    recent: &[CrateCard],
    viewer: Option<&Viewer>,
) -> String {
    let cards: Vec<AnyView> = recent
        .iter()
        .take(6)
        .map(|item| card(item, &format!("/crates/{}", item.id.to_path())))
        .collect();

    let packages = thousands(stats.packages);
    let versions = thousands(stats.versions);
    let downloads = thousands(stats.downloads);
    let publishers = thousands(stats.publishers);

    layout::page(
        "signed packages for the Aevia language",
        "/",
        viewer,
        view! {
            <section class="hero">
                <h1>"Signed packages for the Aevia language"</h1>
                <p class="lede">
                    "Every version is signed by a verified publisher, checked against the \
                     archive it names, and recorded in a public hash chain."
                </p>
                <div class="install">
                    <code id="install-home">"aevia pkg add <owner>/<name>"</code>
                    <button
                        type="button"
                        class="copy"
                        data-copy="#install-home"
                        aria-label="Copy the install command"
                    >
                        "copy"
                    </button>
                </div>
                <div class="actions">
                    <a class="btn" href="/crates">"browse crates"</a>
                    <a class="btn btn-ghost" href="/docs">"read the docs"</a>
                </div>
            </section>
            <div class="hero-grid">
                <pre class="code"><code>{SNIPPET}</code></pre>
                <div class="stats">
                    <div class="stat">
                        <span class="stat-value">{packages}</span>
                        <span class="stat-label">"packages"</span>
                    </div>
                    <div class="stat">
                        <span class="stat-value">{versions}</span>
                        <span class="stat-label">"versions"</span>
                    </div>
                    <div class="stat">
                        <span class="stat-value">{downloads}</span>
                        <span class="stat-label">"downloads"</span>
                    </div>
                    <div class="stat">
                        <span class="stat-value">{publishers}</span>
                        <span class="stat-label">"publishers"</span>
                    </div>
                </div>
            </div>
            <section>
                <h2>"recently published"</h2>
                <div class="cards">{cards}</div>
            </section>
            <div class="cols">
                <section class="col">
                    <h2>"Verified identities"</h2>
                    <p>
                        "A publisher is a GitHub login or an ORCID iD proven through OAuth, and \
                         a package name is scoped to that identity. `apich/units` can only be \
                         published by `apich`."
                    </p>
                </section>
                <section class="col">
                    <h2>"Signature-based publishing"</h2>
                    <p>
                        "There are no API tokens. A publish is an archive, a manifest and a \
                         signature over a canonical statement; the registry checks the \
                         signature against a key bound to the publisher."
                    </p>
                </section>
                <section class="col">
                    <h2>"Docs built by the compiler"</h2>
                    <p>
                        "The compiler reads `##` comments at publish time and renders them from \
                         the same syntax tree it type-checks, so the documentation cannot drift \
                         from the code it describes."
                    </p>
                </section>
            </div>
        },
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_front_page_shows_the_install_line_and_the_counts() {
        let stats = SiteStats {
            packages: 1204,
            versions: 3821,
            downloads: 45678,
            publishers: 92,
        };
        let html = home(&stats, &[], None);
        assert!(
            html.contains("aevia pkg add &lt;owner&gt;/&lt;name&gt;"),
            "the install line is present and escaped"
        );
        assert!(html.contains("1,204"), "package count");
        assert!(html.contains("3,821"), "version count");
        assert!(html.contains("45,678"), "download count");
        assert!(html.contains("92"), "publisher count");
        assert!(
            html.contains("fall_time"),
            "the language snippet is present"
        );
        assert!(html.contains("href=\"/auth/github/start\""), "signed out");
    }

    #[test]
    fn the_front_page_lists_at_most_six_crates() {
        let items: Vec<CrateCard> = (0..9)
            .map(|index| CrateCard {
                id: format!("apich/pkg{index}").parse().expect("valid identity"),
                description: None,
                version: "0.1.0".to_string(),
                updated_at: "2026-11-01T10:00:00Z".to_string(),
                downloads: index,
                has_docs: false,
            })
            .collect();
        let html = home(&SiteStats::default(), &items, None);
        assert!(html.contains("apich/pkg5"));
        assert!(!html.contains("apich/pkg6"), "the seventh card is cut");
    }
}
