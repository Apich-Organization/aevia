//! Markdown to HTML with the registry's conventions.
//!
//! # Why the events are rewritten instead of the string
//!
//! A README is written by the package author, so it may contain anything the
//! `CommonMark` dialect accepts: tables, footnotes, fenced blocks with an info
//! string, relative links to the sibling `.md` files `aevia doc` writes. The
//! three conventions the site needs — a `language-*` class on fenced code, a
//! `target`/`rel` pair on outbound links, and extensionless module links — are
//! decided from the parser's own events, where the fence info and the link
//! destination are still separate values. Editing the produced HTML would mean
//! searching for `<a href=` with a hand-written parser and hoping no attribute
//! value ever contains that text.
//!
//! # Why the output is sanitised
//!
//! A README is written by whoever published the package, and it is served from
//! the same origin as `/account` — where a signed-in reader's session cookie
//! lives. That combination is the definition of stored cross-site scripting:
//! an `<img src=x onerror="fetch('/account/keys',{method:'POST',…})">` in a
//! README would run with the reader's session and could bind the attacker's
//! key to the reader's account. Passing author HTML through unsanitised is not
//! acceptable here, however ordinary the README.
//!
//! The fix keeps the HTML authors actually use — badges, `<details>`,
//! `<kbd>`, tables, images — and drops everything that can execute or reach
//! outside the page: scripts, event handlers, `style`, frames and `javascript:`
//! URLs. `ammonia` is the same sanitiser docs.rs uses, which is why the
//! allowlist below looks the way it does: it is the set of tags that render a
//! README without giving it a way to run.

use pulldown_cmark::CodeBlockKind;
use pulldown_cmark::CowStr;
use pulldown_cmark::Event;
use pulldown_cmark::Options;
use pulldown_cmark::Parser;
use pulldown_cmark::Tag;
use pulldown_cmark::TagEnd;
use std::borrow::Cow;
use std::sync::LazyLock;

use crate::html::push_escaped;

const OPTIONS: Options = Options::ENABLE_TABLES
    .union(Options::ENABLE_FOOTNOTES)
    .union(Options::ENABLE_STRIKETHROUGH)
    .union(Options::ENABLE_TASKLISTS);

/// Renders Markdown to HTML with the registry's conventions.
///
/// Tables, footnotes, strikethrough and task lists are enabled because a
/// README uses all four and a dialect that silently drops a table is a dialect
/// that loses data. Fenced blocks carry a `language-<info>` class so the site's
/// highlighter can find them; absolute `http(s)` links open in a new tab with
/// `rel="noopener noreferrer"`; a relative link to a `.md` file becomes the
/// extensionless route of the page `aevia doc` wrote for it.
///
/// # Examples
///
/// ```
/// let html = aevia_registry_docs::render_markdown("[m](Kinematics.md)");
/// assert!(html.contains("href=\"Kinematics\""), "{html}");
/// // Every link carries `rel`, sanitised or not.
/// assert!(html.contains("rel=\"noopener noreferrer\""), "{html}");
/// ```
#[must_use]
pub fn render_markdown(markdown: &str) -> String {
    render(markdown, None)
}

/// Renders a platform manual chapter with its edition-specific link resolver.
///
/// Fences marked `aevia,run` carry an extra class so the book can progressively
/// enhance them. Package documentation never marks its examples this way.
#[must_use]
pub fn render_manual_markdown(
    markdown: &str,
    resolve_link: &dyn Fn(&str) -> String,
) -> String {
    render(markdown, Some(resolve_link))
}

fn render(
    markdown: &str,
    manual_links: Option<&dyn Fn(&str) -> String>,
) -> String {
    let mut needs_sanitiser = false;
    let mut rendered = String::with_capacity(markdown.len().saturating_add(markdown.len() / 2));
    pulldown_cmark::html::push_html(
        &mut rendered,
        Parser::new_ext(markdown, OPTIONS).map(|event| {
            needs_sanitiser = needs_sanitiser || !is_plain_prose(&event);
            adapt(event, manual_links)
        }),
    );
    if needs_sanitiser {
        sanitise(&rendered)
    } else {
        rendered
    }
}

/// True for renderer-generated prose whose HTML requires no attributes or
/// normalisation. Raw HTML, links, and all other events use the full policy.
#[allow(
    clippy::single_call_fn,
    reason = "keeps the plain-prose allowlist separate from rendering and URL adaptation"
)]
fn is_plain_prose(event: &Event<'_>) -> bool {
    match event {
        | Event::Start(Tag::Paragraph | Tag::Emphasis | Tag::Strong | Tag::Strikethrough)
        | Event::End(
            TagEnd::Paragraph | TagEnd::Emphasis | TagEnd::Strong | TagEnd::Strikethrough,
        )
        | Event::SoftBreak => true,
        | Event::Text(text) | Event::Code(text) => {
            !text.chars().any(|character| {
                character.is_control() || matches!(character, '<' | '>' | '&' | '\u{a0}')
            })
        },
        | _ => false,
    }
}

/// Rewrites one event, or returns it unchanged.
#[allow(
    clippy::single_call_fn,
    reason = "the Markdown event adapter isolates link and code-fence transformations"
)]
fn adapt<'a>(
    event: Event<'a>,
    manual_links: Option<&dyn Fn(&str) -> String>,
) -> Event<'a> {
    match event {
        | Event::Start(Tag::CodeBlock(kind)) => {
            let info = match &kind {
                | CodeBlockKind::Fenced(info) => info.as_ref(),
                | CodeBlockKind::Indented => "",
            };
            let mut words = info.split([',', ' ', '\t']).filter(|word| !word.is_empty());
            let language = words.next().map_or_else(String::new, language_class);
            if language.is_empty() {
                return Event::Html(CowStr::Borrowed("<pre><code>"));
            }
            let interactive =
                manual_links.is_some() && language == "aevia" && words.any(|word| word == "run");
            let extra = if interactive {
                " interactive-example"
            } else {
                ""
            };
            let open = format!("<pre><code class=\"language-{language}{extra}\">");
            Event::Html(CowStr::Boxed(open.into_boxed_str()))
        },
        | Event::End(TagEnd::CodeBlock) => Event::Html(CowStr::Borrowed("</code></pre>\n")),
        | Event::Start(Tag::Link {
            link_type,
            dest_url,
            title,
            id,
        }) => {
            let rewritten = match manual_links {
                | Some(resolve) => Cow::Owned(resolve(&dest_url)),
                | None => rewrite_md_link(&dest_url),
            };
            if is_absolute_http(&rewritten) {
                let mut open = String::from("<a href=\"");
                push_escaped(&mut open, &rewritten);
                open.push('"');
                if !title.is_empty() {
                    open.push_str(" title=\"");
                    push_escaped(&mut open, &title);
                    open.push('"');
                }
                open.push_str(" rel=\"noopener noreferrer\" target=\"_blank\">");
                Event::Html(CowStr::Boxed(open.into_boxed_str()))
            } else {
                let dest_url = if rewritten.as_ref() == dest_url.as_ref() {
                    dest_url
                } else {
                    CowStr::Boxed(rewritten.into_owned().into_boxed_str())
                };
                Event::Start(Tag::Link {
                    link_type,
                    dest_url,
                    title,
                    id,
                })
            }
        },
        | other => other,
    }
}

/// The class-safe form of a fence's info string.
#[allow(
    clippy::single_call_fn,
    reason = "fence class names use an independently defined character set"
)]
fn language_class(info: &str) -> String {
    info.chars()
        .filter(|character| {
            character.is_ascii_alphanumeric() || matches!(character, '-' | '_' | '+')
        })
        .map(|character| character.to_ascii_lowercase())
        .collect()
}

/// Whether a link destination is an absolute `http` or `https` URL.
#[allow(
    clippy::single_call_fn,
    reason = "HTTP link classification determines which links open a new window"
)]
fn is_absolute_http(url: &str) -> bool {
    url.split_once("://").is_some_and(|(scheme, _)| {
        scheme.eq_ignore_ascii_case("http") || scheme.eq_ignore_ascii_case("https")
    })
}

/// Whether a URL carries a scheme, so it is not a relative path.
#[allow(
    clippy::single_call_fn,
    reason = "RFC-style scheme syntax distinguishes relative documentation links"
)]
fn has_scheme(url: &str) -> bool {
    let Some((scheme, _)) = url.split_once(':') else {
        return false;
    };
    !scheme.is_empty()
        && scheme
            .chars()
            .next()
            .is_some_and(|character| character.is_ascii_alphabetic())
        && scheme.chars().all(|character| {
            character.is_ascii_alphanumeric() || matches!(character, '+' | '-' | '.')
        })
}

/// Rewrites a relative link to a `.md` file as the route of its page.
///
/// `aevia doc` names a module page `Kinematics.md`; the site serves that page
/// at `/docs/<package>/<version>/Kinematics`. Dropping the extension is the
/// whole rewrite, and a fragment such as `Kinematics.md#fn-circle` survives it
/// because anchors are stable across the two renderers. Anything that is not a
/// relative `.md` link — an absolute URL, an anchor, a path with a scheme, a
/// link to `notes.txt` — is left exactly as written.
#[allow(
    clippy::single_call_fn,
    reason = "package documentation route rewriting is tested independently of HTML emission"
)]
fn rewrite_md_link(url: &str) -> Cow<'_, str> {
    if url.is_empty() || url.starts_with('#') || url.starts_with('/') || has_scheme(url) {
        return Cow::Borrowed(url);
    }
    let cut = url.find(['?', '#']).unwrap_or(url.len());
    let (path, rest) = url.split_at(cut);
    if let Some((stem, extension)) = path.rsplit_once('.')
        && extension.eq_ignore_ascii_case("md")
    {
        Cow::Owned(format!("{stem}{rest}"))
    } else {
        Cow::Borrowed(url)
    }
}

/// Runs the rendered HTML through the sanitiser.
///
/// The allowlist is the default set plus the handful of elements a README
/// genuinely uses, and a few attributes that make images and collapsible
/// sections work. `rel` is forced on every link so a target=_blank page cannot
/// reach back through `window.opener`.
#[allow(
    clippy::single_call_fn,
    reason = "the independent sanitization policy is shared by all Markdown entry points"
)]
fn sanitise(html: &str) -> String {
    static POLICY: LazyLock<ammonia::Builder<'static>> = LazyLock::new(|| {
        let mut builder = ammonia::Builder::default();
        builder
            .add_tags(&[
                "details",
                "summary",
                "kbd",
                "samp",
                "var",
                "figure",
                "figcaption",
                "del",
                "ins",
                "picture",
                "source",
                "abbr",
                "dl",
                "dt",
                "dd",
                "sup",
                "sub",
                "mark",
            ])
            .add_tag_attributes(
                "img",
                &["src", "alt", "title", "width", "height", "loading"],
            )
            .add_tag_attributes("source", &["srcset", "type", "media", "sizes"])
            .add_tag_attributes("details", &["open"])
            .add_tag_attributes("abbr", &["title"])
            // `target` is allowed because `link_rel` above guarantees the
            // `noopener noreferrer` that makes a new tab safe.
            .add_tag_attributes("a", &["target"])
            .add_tag_attributes("code", &["class"])
            .add_tag_attributes("pre", &["class"])
            .add_tag_attributes("span", &["class"])
            .add_tag_attributes("div", &["class"])
            .add_tag_attributes("th", &["align", "scope", "colspan", "rowspan"])
            .add_tag_attributes("td", &["align", "colspan", "rowspan"])
            .link_rel(Some("noopener noreferrer"));
        builder
    });
    POLICY.clean(html).to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn skipping_the_html_parser_preserves_rendering() {
        let plain = [
            "",
            "Ordinary prose with an apostrophe's mark and \"quotes\".",
            "中文和 λ 都保留。",
            "`inline code` and **bold**, *emphasis*, ~~struck~~.",
            "A soft\nwrap.\n\nA second paragraph.",
        ];
        let checked = [
            "A hard  \nwrap.",
            "`vector<T>` & non-breaking\u{a0}space.",
            "[module](Kinematics.md) and <strong>inline HTML</strong>",
            "# Heading\n\n- Item",
            "```aevia\nlet x = 1\n```",
            "A\0B",
            "An entity &amp; a line\rbreak.",
        ];
        for (cases, plain_only) in [(&plain[..], true), (&checked[..], false)] {
            for source in cases {
                assert_eq!(
                    Parser::new_ext(source, OPTIONS).all(|event| is_plain_prose(&event)),
                    plain_only,
                    "{source:?}"
                );
                let mut old_html = String::new();
                pulldown_cmark::html::push_html(
                    &mut old_html,
                    Parser::new_ext(source, OPTIONS).map(|event| adapt(event, None)),
                );
                assert_eq!(render_markdown(source), sanitise(&old_html), "{source:?}");
            }
        }
    }

    #[test]
    fn a_table_survives() {
        let html = render_markdown("| Module | Decls |\n|---|---|\n| `A` | 2 |\n");
        assert!(html.contains("<table>"), "{html}");
        assert!(html.contains("<td><code>A</code></td>"), "{html}");
    }

    #[test]
    fn a_fenced_aevia_block_carries_the_language_class() {
        let html = render_markdown("```aevia\nfn f(): Int = 1\n```\n");
        assert!(
            html.contains("<pre><code class=\"language-aevia\">"),
            "{html}"
        );
        // The code is escaped, not interpreted.
        assert!(html.contains("fn f(): Int = 1"), "{html}");
    }

    #[test]
    fn a_relative_md_link_becomes_a_page_route() {
        let html = render_markdown("See [`Thermo`](Thermo.IdealGas.md#fn-k) and [n](notes.txt).");
        assert!(html.contains("href=\"Thermo.IdealGas#fn-k\""), "{html}");
        assert!(html.contains("href=\"notes.txt\""), "{html}");
    }

    #[test]
    fn an_absolute_link_opens_in_a_new_tab() {
        let html = render_markdown("[home](https://example.com/a?b=c)");
        // The sanitiser may reorder attributes, so each is checked on its own.
        assert!(
            html.contains("href=\"https://example.com/a?b=c\""),
            "{html}"
        );
        assert!(html.contains("target=\"_blank\""), "{html}");
        assert!(html.contains("rel=\"noopener noreferrer\""), "{html}");
        // A relative link must not gain the attributes.
        let relative = render_markdown("[m](Kinematics.md)");
        assert!(!relative.contains("target="), "{relative}");
    }

    #[test]
    fn a_script_block_is_removed_and_the_rest_survives() {
        let html = render_markdown("# Hi\n\n<script>alert(1)</script>\n\nAfter.\n");
        assert!(!html.to_ascii_lowercase().contains("<script"), "{html}");
        assert!(html.contains("<h1>Hi</h1>"), "{html}");
        assert!(html.contains("After."), "{html}");
    }

    #[test]
    fn the_word_script_in_prose_is_not_a_tag() {
        let html = render_markdown("A `<script>` tag.");
        assert!(html.contains("&lt;script&gt;"), "{html}");
    }
}
