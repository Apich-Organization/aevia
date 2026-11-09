//! The error page.
//!
//! Error pages are reached by people who are already having a bad time, so this
//! one states the three things they need — what the server answered, which
//! condition it was, and what to do next — and nothing else. The message is
//! rendered as text, never as markup: it can carry a package name or a
//! fragment of user input, and the escape happens in `view!`.

use aevia_registry_core::Lang;
use aevia_registry_core::vm::Viewer;
use leptos::prelude::*;

use crate::Msg;
use crate::layout;

const TITLE: Msg = Msg::new("error", "错误");
const HOME: Msg = Msg::new("back to the registry", "返回注册中心");
const BROWSE: Msg = Msg::new("browse crates", "浏览包");
const CODE_HINT: Msg = Msg::new(
    "Quote this code in a bug report.",
    "反馈问题时请附上这个错误码。",
);

/// Renders an error document.
///
/// The status is shown as a number rather than mapped to a phrase so that it
/// matches what `curl -i` reported; the code is the registry's own stable
/// identifier, which is what a bug report should quote.
pub fn error_page(
    lang: Lang,
    status: u16,
    code: &str,
    message: &str,
    viewer: Option<&Viewer>,
) -> String {
    layout::page(
        lang,
        &format!("{} {status}", TITLE.of(lang)),
        "",
        viewer,
        view! {
            <section class="error">
                <p class="error-status">{status}</p>
                <h1>{code.to_string()}</h1>
                <p>{message.to_string()}</p>
                <p class="note">{CODE_HINT.of(lang)}</p>
                <p class="actions">
                    <a class="btn" href="/">{HOME.of(lang)}</a>
                    <a class="btn btn-ghost" href="/crates">{BROWSE.of(lang)}</a>
                </p>
            </section>
        },
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_error_page_escapes_the_message() {
        let html = error_page(
            Lang::En,
            404,
            "E_NOT_FOUND",
            "No package <script>alert(1)</script> here",
            None,
        );
        assert!(html.contains("404"));
        assert!(html.contains("E_NOT_FOUND"));
        assert!(
            html.contains("&lt;script&gt;alert(1)&lt;/script&gt;"),
            "the message is escaped"
        );
        assert!(
            !html.contains("<script>alert"),
            "no raw script element from the message"
        );
        assert!(html.contains("href=\"/\""), "there is a way home");
    }

    #[test]
    fn the_error_page_speaks_chinese_too() {
        let html = error_page(Lang::Zh, 404, "E_NOT_FOUND", "没有这个包。", None);
        assert!(html.contains("返回注册中心"), "the way home is translated");
        assert!(html.contains("没有这个包。"), "the message is the server's");
    }
}
