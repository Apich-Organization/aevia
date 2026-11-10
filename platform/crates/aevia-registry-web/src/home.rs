//! The front page.
//!
//! This is the only page that has to answer, in one screen, what the registry
//! is, how to install from it, and why it is not crates.io. The order of the
//! sections is that argument: a sentence, the command, an example of the
//! language, the size of the registry, then the three properties that make it
//! worth using.

use aevia_registry_core::Lang;
use aevia_registry_core::vm::CrateCard;
use aevia_registry_core::vm::SiteStats;
use aevia_registry_core::vm::Viewer;
use leptos::prelude::AnyView;
use leptos::prelude::*;

use crate::Msg;
use crate::crates::card_in;
use crate::layout;
use crate::thousands;

/// The snippet in the hero.
///
/// It is the kinematics example from the language tour, deliberately small:
/// three lines that show a signature, a `##` doc comment and a call with a
/// dimensioned literal, which is the shortest honest sample of what a package
/// on this registry contains.
const SNIPPET: &str = "fn fall_time(h: Real[L]): Real[T] = sqrt(2 * h / g)\n\n## The time a body takes to fall from height `h` under gravity `g`.\nlet t = fall_time(3.2 m)";

const TITLE: Msg = Msg::new(
    "signed packages for the Aevia language",
    "为 Aevia 语言提供带签名的包",
);
const HEADLINE: Msg = Msg::new(
    "Signed packages for the Aevia language",
    "为 Aevia 语言提供带签名的包",
);
const LEDE: Msg = Msg::new(
    "Every version is signed by a verified publisher, checked against the archive it names, \
     and recorded in a public hash chain.",
    "每个版本都由已验证的发布者签名，与它声明的归档逐字节比对，并记入公开的哈希链。",
);
const COPY_LABEL: Msg = Msg::new("Copy the install command", "复制安装命令");
const COPY: Msg = Msg::new("copy", "复制");
const BROWSE: Msg = Msg::new("browse crates", "浏览包");
const READ_DOCS: Msg = Msg::new("read the docs", "阅读文档");
const STAT_PACKAGES: Msg = Msg::new("packages", "包");
const STAT_VERSIONS: Msg = Msg::new("versions", "版本");
const STAT_DOWNLOADS: Msg = Msg::new("downloads", "下载");
const STAT_PUBLISHERS: Msg = Msg::new("publishers", "发布者");
const RECENT: Msg = Msg::new("recently published", "最近发布");
const IDENTITIES_TITLE: Msg = Msg::new("Verified identities", "身份已验证");
const IDENTITIES_BODY: Msg = Msg::new(
    "A publisher is a GitHub login or an ORCID iD proven through OAuth, and a package name is \
     scoped to that identity, so a name cannot be claimed before its owner exists.",
    "发布者是经 OAuth 验证的 GitHub 账号或 ORCID iD，包名绑定到该身份，因此一个名字不可能在其\
     主人出现之前被抢注。",
);
const SIGNATURE_TITLE: Msg = Msg::new("Signature-based publishing", "以签名发布");
const SIGNATURE_BODY: Msg = Msg::new(
    "There are no API tokens. A publish is an archive, a manifest and a signature over a \
     canonical statement; the registry checks the signature against a key bound to the publisher.",
    "这里没有 API 令牌。一次发布是归档、清单，以及对一段规范化声明的签名；注册中心用绑定到\
     发布者的密钥验证该签名。",
);
const DOCS_TITLE: Msg = Msg::new("Docs built by the compiler", "文档由编译器生成");
const DOCS_BODY: Msg = Msg::new(
    "The compiler reads `##` comments at publish time and renders them from the same syntax tree \
     it type-checks, so the documentation cannot drift from the code it describes.",
    "编译器在发布时读取 `##` 注释，并从它做类型检查的同一棵语法树渲染页面，因此文档不会与它\
     描述的代码脱节。",
);

/// Renders `/`.
///
/// `recent` is truncated to six here rather than by the caller: the front page
/// is the only consumer that wants a fixed count, and keeping the cut next to
/// the grid means the grid and the count cannot drift apart.
pub fn home(
    lang: Lang,
    stats: &SiteStats,
    recent: &[CrateCard],
    viewer: Option<&Viewer>,
) -> String {
    let cards: Vec<AnyView> = recent
        .iter()
        .take(6)
        .map(|item| card_in(lang, item, &format!("/crates/{}", item.id.to_path())))
        .collect();

    let packages = thousands(stats.packages);
    let versions = thousands(stats.versions);
    let downloads = thousands(stats.downloads);
    let publishers = thousands(stats.publishers);

    layout::page(
        lang,
        TITLE.of(lang),
        "/",
        viewer,
        view! {
            <section class="hero">
                <h1>{HEADLINE.of(lang)}</h1>
                <p class="lede">{LEDE.of(lang)}</p>
                <div class="install">
                    <code id="install-home">"aevia pkg add <owner>/<name>"</code>
                    <button
                        type="button"
                        class="copy"
                        data-copy="#install-home"
                        aria-label=COPY_LABEL.of(lang)
                    >
                        {COPY.of(lang)}
                    </button>
                </div>
                <div class="actions">
                    <a class="btn" href="/crates">{BROWSE.of(lang)}</a>
                    <a class="btn btn-ghost" href="/docs">{READ_DOCS.of(lang)}</a>
                </div>
            </section>
            <div class="hero-grid">
                <pre class="code"><code>{SNIPPET}</code></pre>
                <div class="stats">
                    <div class="stat">
                        <span class="stat-value">{packages}</span>
                        <span class="stat-label">{STAT_PACKAGES.of(lang)}</span>
                    </div>
                    <div class="stat">
                        <span class="stat-value">{versions}</span>
                        <span class="stat-label">{STAT_VERSIONS.of(lang)}</span>
                    </div>
                    <div class="stat">
                        <span class="stat-value">{downloads}</span>
                        <span class="stat-label">{STAT_DOWNLOADS.of(lang)}</span>
                    </div>
                    <div class="stat">
                        <span class="stat-value">{publishers}</span>
                        <span class="stat-label">{STAT_PUBLISHERS.of(lang)}</span>
                    </div>
                </div>
            </div>
            <section>
                <h2>{RECENT.of(lang)}</h2>
                <div class="cards">{cards}</div>
            </section>
            <div class="cols">
                <section class="col">
                    <h2>{IDENTITIES_TITLE.of(lang)}</h2>
                    <p>{IDENTITIES_BODY.of(lang)}</p>
                </section>
                <section class="col">
                    <h2>{SIGNATURE_TITLE.of(lang)}</h2>
                    <p>{SIGNATURE_BODY.of(lang)}</p>
                </section>
                <section class="col">
                    <h2>{DOCS_TITLE.of(lang)}</h2>
                    <p>{DOCS_BODY.of(lang)}</p>
                </section>
            </div>
        },
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn stats() -> SiteStats {
        SiteStats {
            packages: 1204,
            versions: 3821,
            downloads: 45678,
            publishers: 92,
        }
    }

    #[test]
    fn the_front_page_shows_the_install_line_and_the_counts() {
        let html = home(Lang::En, &stats(), &[], None);
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
        assert!(html.contains("href=\"/login\""), "signed out");
    }

    #[test]
    fn the_chinese_front_page_is_chinese() {
        let html = home(Lang::Zh, &stats(), &[], None);
        assert!(html.contains("为 Aevia 语言提供带签名的包"), "headline");
        assert!(html.contains("最近发布"), "recent section");
        assert!(html.contains("lang=\"zh-CN\""), "document language");
        assert!(
            html.contains("aevia pkg add &lt;owner&gt;/&lt;name&gt;"),
            "the command stays untranslated"
        );
    }

    #[test]
    fn the_front_page_lists_at_most_six_crates() {
        let items: Vec<CrateCard> = (0..9)
            .map(|index| {
                CrateCard {
                    id: format!("apich/pkg{index}").parse().expect("valid identity"),
                    description: None,
                    version: "0.1.0".to_string(),
                    updated_at: "2026-11-01T10:00:00Z".to_string(),
                    downloads: index,
                    has_docs: false,
                }
            })
            .collect();
        let html = home(Lang::En, &stats(), &items, None);
        assert_eq!(html.matches("class=\"card\"").count(), 6);
        assert!(html.contains("apich/pkg5"));
        assert!(!html.contains("apich/pkg6"));
    }
}
