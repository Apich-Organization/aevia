//! Server-rendered pages for the Aevia package registry.
//!
//! # Why every page is a `String`
//!
//! The registry is a server-rendered site: there is no WASM bundle, no
//! hydration pass and no client-side router. Each page function therefore
//! returns one complete HTML document, and the HTTP layer writes it straight
//! to the response. Keeping the boundary at `String` means the server needs to
//! know nothing about Leptos, and a page can be exercised in a unit test
//! without a runtime, a database or a socket.
//!
//! # Rendering model
//!
//! Leptos 0.8 removed the `leptos::ssr::render_to_string` helper that earlier
//! versions exposed; the equivalent is `RenderHtml::to_html`. [`render`] wraps
//! that in the closure shape the pages are written against, and installs an
//! owner scope so that a view built inside the closure has a reactive scope to
//! attach to. Nothing here is reactive — the view models are plain data — but
//! the scope keeps that invariant true if a page later grows a signal.
//!
//! # Escaping
//!
//! Text is never escaped by hand. `view!` escapes every text node and every
//! attribute value it emits, which is why the only raw insertions in this
//! crate are the `inner_html` attributes fed from the server-side markdown
//! renderer (`readme_html` and the documentation `html` field).

#![forbid(unsafe_code)]

mod account;
mod cli;
mod crates;
mod docs;
mod error;
mod home;
mod layout;
mod log;
mod login;
mod spec;
mod styles;
mod user;

pub use account::account;
pub use account::activate;
pub use cli::cli_page;
pub use crates::crate_list;
pub use crates::crate_page;
pub use docs::docs_index;
pub use docs::docs_landing;
pub use docs::docs_module;
pub use error::error_page;
pub use home::home;
pub use log::log_page;
pub use login::login;
pub use spec::spec_all;
pub use spec::spec_index;
pub use spec::spec_page;
pub use styles::APP_JS;
pub use styles::SITE_CSS;
pub use user::user_page;

use aevia_registry_core::Lang;
use leptos::prelude::Owner;
use leptos::prelude::RenderHtml;

/// One string, in both languages.
///
/// The pair lives next to the markup that uses it rather than in a central
/// catalogue: a translator editing `crates.rs` should not have to open a second
/// file to find the label they are looking at, and a deleted section should
/// take its strings with it. `of` is the only way to read one, so a page cannot
/// accidentally render the wrong language.
pub(crate) struct Msg {
    en: &'static str,
    zh: &'static str,
}

impl Msg {
    /// Builds a pair.
    pub(crate) const fn new(
        en: &'static str,
        zh: &'static str,
    ) -> Self {
        Self { en, zh }
    }

    /// The string in `lang`.
    pub(crate) fn of(
        &self,
        lang: Lang,
    ) -> &'static str {
        match lang {
            | Lang::En => self.en,
            | Lang::Zh => self.zh,
        }
    }
}

/// The `lang` attribute for the document element.
pub(crate) fn lang_attr(lang: Lang) -> &'static str {
    match lang {
        | Lang::En => "en",
        | Lang::Zh => "zh-CN",
    }
}

/// Renders one view into a document string.
///
/// This exists instead of calling `to_html` at each call site so that every
/// page is rendered inside an owner scope. A page that panics on a missing
/// owner would be a production incident triggered by markup; giving the view a
/// root here removes that class of failure from the whole crate.
pub(crate) fn render<V, F>(view: F) -> String
where
    F: FnOnce() -> V,
    V: RenderHtml,
{
    Owner::new().with(move || view().to_html())
}

/// Truncates a hash or fingerprint to a length a table column can hold.
///
/// Full OpenPGP fingerprints are 40 hexadecimal characters; a table that shows
/// them all forces every other column to wrap. The prefix is enough to match a
/// key against `gpg --list-keys`, and the full value is shown on the detail
/// panel of the package page.
pub(crate) fn shorten(
    value: &str,
    keep: usize,
) -> String {
    let mut characters = value.chars();
    let head: String = characters.by_ref().take(keep).collect();
    if characters.next().is_some() {
        format!("{head}…")
    } else {
        head
    }
}

/// Groups an integer with commas.
///
/// Download counts are the one number on these pages large enough that a
/// reader cannot group digits by eye at a glance.
pub(crate) fn thousands(value: i64) -> String {
    let digits = value.unsigned_abs().to_string();
    let mut grouped = String::with_capacity(digits.len() + digits.len() / 3 + 1);
    if value < 0 {
        grouped.push('-');
    }
    for (index, character) in digits.chars().enumerate() {
        if index > 0 && (digits.len() - index).is_multiple_of(3) {
            grouped.push(',');
        }
        grouped.push(character);
    }
    grouped
}

/// The date part of an RFC 3339 timestamp.
///
/// The view models carry timestamps as strings, and the pages show a day, not
/// a second: a publish time to the second invites the reader to infer an
/// ordering the log already states explicitly.
pub(crate) fn date_only(timestamp: &str) -> &str {
    timestamp.get(..10).unwrap_or(timestamp)
}

/// Percent-encodes a value for use in a query string.
///
/// The pages build their own links (search, sort, paging) and the crate
/// deliberately has no `url` dependency, so the small subset of encoding a
/// query value needs lives here. Everything outside the unreserved set is
/// escaped, which is the conservative choice: a search term is user text.
pub(crate) fn query_param(value: &str) -> String {
    let mut encoded = String::with_capacity(value.len());
    for byte in value.bytes() {
        match byte {
            | b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                encoded.push(char::from(byte));
            },
            | b' ' => encoded.push('+'),
            | _ => encoded.push_str(&format!("%{byte:02X}")),
        }
    }
    encoded
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hashes_keep_their_prefix() {
        assert_eq!(shorten("0123456789abcdef0123", 8), "01234567…");
        assert_eq!(shorten("abc", 8), "abc");
    }

    #[test]
    fn large_counts_are_grouped() {
        assert_eq!(thousands(0), "0");
        assert_eq!(thousands(999), "999");
        assert_eq!(thousands(1204), "1,204");
        assert_eq!(thousands(1234567), "1,234,567");
        assert_eq!(thousands(-1204), "-1,204");
    }

    #[test]
    fn query_values_are_encoded() {
        assert_eq!(query_param("units extra"), "units+extra");
        assert_eq!(query_param("a/b&c"), "a%2Fb%26c");
        assert_eq!(query_param("plain-1.0_~"), "plain-1.0_~");
    }

    #[test]
    fn dates_survive_short_input() {
        assert_eq!(date_only("2026-11-08T09:00:00Z"), "2026-11-08");
        assert_eq!(date_only("bad"), "bad");
    }
}
