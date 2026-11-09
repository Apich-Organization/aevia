//! The transparency log page.
//!
//! The log is the registry's answer to the question a package manager cannot
//! otherwise answer: "was this version ever different?" Every publish, yank and
//! key binding is appended to a hash chain, and the head hash is the value a
//! mirror or a client can pin. The page shows the chain rather than a summary
//! of it, because a summary is exactly what an attacker who controls the
//! database could rewrite.

use aevia_registry_core::Lang;
use aevia_registry_core::vm::LogEntryView;
use aevia_registry_core::vm::Viewer;
use leptos::prelude::AnyView;
use leptos::prelude::*;

use crate::Msg;
use crate::date_only;
use crate::layout;
use crate::shorten;

// Page headings and body text.
const TITLE: Msg = Msg::new("transparency log", "透明日志");
const LEDE: Msg = Msg::new(
    "Every publish, yank and key binding, in order.",
    "每次发布、撤回与密钥绑定，按顺序排列。",
);
const ABOUT: Msg = Msg::new(
    "Each entry commits to the hash of the entry before it, so the chain can only \
     be extended, never rewritten in place. A client that pins the head hash can \
     ask the registry to prove that the history it serves is the history it served \
     yesterday; if a version were altered or removed, the hashes after it would no \
     longer match and the difference would be visible here.",
    "每个条目都承诺前一条目的哈希，因此这条链只能追加，不能就地重写。固定链头哈希的客户端可以要求\
     注册中心证明：它今天提供的历史就是昨天提供的历史。如果某个版本被改动或删除，其后的哈希将不再\
     匹配，差异在这里一目了然。",
);
const EMPTY: Msg = Msg::new("The log is empty.", "日志为空。");

// The head summary.
const HEAD_SEQ: Msg = Msg::new("head sequence", "链头序号");
const HEAD_HASH: Msg = Msg::new("head hash", "链头哈希");

// Table headers.
const SEQ_COLUMN: Msg = Msg::new("seq", "序号");
const KIND_COLUMN: Msg = Msg::new("kind", "类型");
const SUMMARY_COLUMN: Msg = Msg::new("summary", "摘要");
const HASH_COLUMN: Msg = Msg::new("hash", "哈希");
const DATE_COLUMN: Msg = Msg::new("date", "日期");

/// Renders `/log`.
///
/// `head_seq` and `head_hash` are passed separately from the entries because
/// the head is what a reader compares against a pinned value; it must be shown
/// even when the page below it fails to list anything.
pub fn log_page(
    lang: Lang,
    entries: &[LogEntryView],
    head_seq: i64,
    head_hash: &str,
    viewer: Option<&Viewer>,
) -> String {
    let rows: Vec<AnyView> = if entries.is_empty() {
        vec![
            view! {
                <tr>
                    <td colspan="5" class="muted">{EMPTY.of(lang)}</td>
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
        lang,
        TITLE.of(lang),
        "/log",
        viewer,
        view! {
            <section class="page-head">
                <h1>{TITLE.of(lang)}</h1>
                <p>{LEDE.of(lang)}</p>
            </section>
            <p>{ABOUT.of(lang)}</p>
            <dl class="log-head">
                <div>
                    <dt>{HEAD_SEQ.of(lang)}</dt>
                    <dd>{head_seq}</dd>
                </div>
                <div>
                    <dt>{HEAD_HASH.of(lang)}</dt>
                    <dd>{head.clone()}</dd>
                </div>
            </dl>
            <table>
                <thead>
                    <tr>
                        <th scope="col">{SEQ_COLUMN.of(lang)}</th>
                        <th scope="col">{KIND_COLUMN.of(lang)}</th>
                        <th scope="col">{SUMMARY_COLUMN.of(lang)}</th>
                        <th scope="col">{HASH_COLUMN.of(lang)}</th>
                        <th scope="col">{DATE_COLUMN.of(lang)}</th>
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
        let html = log_page(
            Lang::En,
            &entries,
            41,
            "abcdef0123456789abcdef0123456789",
            None,
        );
        assert!(html.contains("apich/units-extra 1.2.0"));
        assert!(html.contains("head sequence"));
        assert!(html.contains("41"));
        assert!(html.contains("abcdef0123456789…"), "hashes are shortened");
    }
}
