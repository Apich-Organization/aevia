//! Markdown to HTML with the registry's conventions.
//!
//! # Why the events are rewritten instead of the string
//!
//! A README is written by the package author, so it may contain anything the
//! CommonMark dialect accepts: tables, footnotes, fenced blocks with an info
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

use crate::html::escape;

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
    let mut options = Options::empty();
    options.insert(Options::ENABLE_TABLES);
    options.insert(Options::ENABLE_FOOTNOTES);
    options.insert(Options::ENABLE_STRIKETHROUGH);
    options.insert(Options::ENABLE_TASKLISTS);
    let parser = Parser::new_ext(markdown, options);
    let mut rendered = String::with_capacity(markdown.len() + markdown.len() / 2);
    pulldown_cmark::html::push_html(&mut rendered, transform(parser));
    sanitise(&rendered)
}

/// Rewrites the events whose rendering the site's conventions change.
fn transform<'a>(parser: Parser<'a>) -> impl Iterator<Item = Event<'a>> + 'a {
    parser.flat_map(|event| adapt(event))
}

/// Rewrites one event, or returns it unchanged.
fn adapt<'a>(event: Event<'a>) -> Option<Event<'a>> {
    match event {
        | Event::Start(Tag::CodeBlock(CodeBlockKind::Fenced(info))) => {
            let language = info
                .split([' ', '\t', ','])
                .next()
                .map(language_class)
                .unwrap_or_default();
            let open = if language.is_empty() {
                "<pre><code>".to_string()
            } else {
                format!("<pre><code class=\"language-{language}\">")
            };
            Some(Event::Html(CowStr::Boxed(open.into_boxed_str())))
        },
        | Event::End(TagEnd::CodeBlock) => Some(Event::Html(CowStr::Borrowed("</code></pre>\n"))),
        | Event::Start(Tag::Link {
            link_type,
            dest_url,
            title,
            id,
        }) => {
            if is_absolute_http(&dest_url) {
                let mut open = String::from("<a href=\"");
                open.push_str(&escape(&dest_url));
                open.push('"');
                if !title.is_empty() {
                    open.push_str(" title=\"");
                    open.push_str(&escape(&title));
                    open.push('"');
                }
                open.push_str(" rel=\"noopener noreferrer\" target=\"_blank\">");
                Some(Event::Html(CowStr::Boxed(open.into_boxed_str())))
            } else {
                let rewritten = rewrite_md_link(&dest_url);
                if rewritten == dest_url.as_ref() {
                    Some(Event::Start(Tag::Link {
                        link_type,
                        dest_url,
                        title,
                        id,
                    }))
                } else {
                    Some(Event::Start(Tag::Link {
                        link_type,
                        dest_url: CowStr::Boxed(rewritten.into_boxed_str()),
                        title,
                        id,
                    }))
                }
            }
        },
        | other => Some(other),
    }
}

/// The class-safe form of a fence's info string.
fn language_class(info: &str) -> String {
    info.chars()
        .filter(|character| {
            character.is_ascii_alphanumeric() || matches!(character, '-' | '_' | '+')
        })
        .map(|character| character.to_ascii_lowercase())
        .collect()
}

/// Whether a link destination is an absolute `http` or `https` URL.
fn is_absolute_http(url: &str) -> bool {
    let lowered = url.to_ascii_lowercase();
    lowered.starts_with("http://") || lowered.starts_with("https://")
}

/// Whether a URL carries a scheme, so it is not a relative path.
fn has_scheme(url: &str) -> bool {
    let Some(colon) = url.find(':') else {
        return false;
    };
    let scheme = &url[..colon];
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
fn rewrite_md_link(url: &str) -> String {
    if url.is_empty()
        || url.starts_with('#')
        || url.starts_with('/')
        || url.starts_with("//")
        || has_scheme(url)
    {
        return url.to_string();
    }
    let cut = url.find(['?', '#']).unwrap_or(url.len());
    let (path, rest) = url.split_at(cut);
    if path.to_ascii_lowercase().ends_with(".md") {
        format!("{}{rest}", &path[..path.len() - 3])
    } else {
        url.to_string()
    }
}

/// Runs the rendered HTML through the sanitiser.
///
/// The allowlist is the default set plus the handful of elements a README
/// genuinely uses, and a few attributes that make images and collapsible
/// sections work. `rel` is forced on every link so a target=_blank page cannot
/// reach back through `window.opener`.
fn sanitise(html: &str) -> String {
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
    builder.clean(html).to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

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
