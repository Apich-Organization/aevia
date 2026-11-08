//! The transparency log page.
//!
//! The log is the registry's answer to the question a package manager cannot
//! otherwise answer: "was this version ever different?" Every publish, yank and
//! key binding is appended to a hash chain, and the head hash is the value a
//! mirror or a client can pin. The page shows the chain rather than a summary
//! of it, because a summary is exactly what an attacker who controls the
//! database could rewrite.

use aevia_registry_core::vm::LogEntryView;
use aevia_registry_core::vm::Viewer;
use leptos::prelude::AnyView;
use leptos::prelude::*;

use crate::date_only;
use crate::layout;
use crate::shorten;

/// Renders `/log`.
///
/// `head_seq` and `head_hash` are passed separately from the entries because
/// the head is what a reader compares against a pinned value; it must be shown
/// even when the page below it fails to list anything.
pub fn log_page(
    entries: &[LogEntryView],
    head_seq: i64,
    head_hash: &str,
    viewer: Option<&Viewer>,
) -> String {
    let rows: Vec<AnyView> = if entries.is_empty() {
        vec![
            view! {
                <tr>
                    <td colspan="5" class="muted">"The log is empty."</td>
                </tr>
            }
            .into_any(),
        ]
    } else {
        entries
            .iter()
            .map(|entry| {
                let at = entry.at.clone();
                let date = date_only(&entry.at).to_string();
                view! {
                    <tr>
                        <td class="num">{entry.seq}</td>
                        <th scope="row">{entry.kind.clone()}</th>
                        <td>{entry.summary.clone()}</td>
                        <td>
                            <code class="mono">{shorten(&entry.hash, 16)}</code>
                        </td>
                        <td>
                            <time datetime=at>{date}</time>
                        </td>
                    </tr>
                }
                .into_any()
            })
            .collect()
    };

    let head = shorten(head_hash, 32);

    layout::page(
        "transparency log",
        "/log",
        viewer,
        view! {
            <section class="page-head">
                <h1>"transparency log"</h1>
                <p>"Every publish, yank and key binding, in order."</p>
            </section>
            <p>
                "Each entry commits to the hash of the entry before it, so the chain can only \
                 be extended, never rewritten in place. A client that pins the head hash can \
                 ask the registry to prove that the history it serves is the history it served \
                 yesterday; if a version were altered or removed, the hashes after it would no \
                 longer match and the difference would be visible here."
            </p>
            <dl class="log-head">
                <div>
                    <dt>"head sequence"</dt>
                    <dd>{head_seq}</dd>
                </div>
                <div>
                    <dt>"head hash"</dt>
                    <dd>{head.clone()}</dd>
                </div>
            </dl>
            <table>
                <thead>
                    <tr>
                        <th scope="col">"seq"</th>
                        <th scope="col">"kind"</th>
                        <th scope="col">"summary"</th>
                        <th scope="col">"hash"</th>
                        <th scope="col">"date"</th>
                    </tr>
                </thead>
                <tbody>{rows}</tbody>
            </table>
        },
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_log_shows_the_head_and_each_entry() {
        let entries = vec![LogEntryView {
            seq: 41,
            kind: "publish".to_string(),
            summary: "apich/units-extra 1.2.0".to_string(),
            hash: "abcdef0123456789abcdef0123456789".to_string(),
            prev: "0000".to_string(),
            at: "2026-11-01T10:00:00Z".to_string(),
        }];
        let html = log_page(&entries, 41, "abcdef0123456789abcdef0123456789", None);
        assert!(html.contains("apich/units-extra 1.2.0"));
        assert!(html.contains("head sequence"));
        assert!(html.contains("41"));
        assert!(html.contains("abcdef0123456789…"), "hashes are shortened");
    }
}
