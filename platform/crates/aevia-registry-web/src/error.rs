//! The error page.
//!
//! Error pages are reached by people who are already having a bad time, so this
//! one states the three things they need — what the server answered, which
//! condition it was, and what to do next — and nothing else. The message is
//! rendered as text, never as markup: it can carry a package name or a
//! fragment of user input, and the escape happens in `view!`.

use aevia_registry_core::vm::Viewer;
use leptos::prelude::*;

use crate::layout;

/// Renders an error document.
///
/// The status is shown as a number rather than mapped to a phrase so that it
/// matches what `curl -i` reported; the code is the registry's own stable
/// identifier, which is what a bug report should quote.
pub fn error_page(
    status: u16,
    code: &str,
    message: &str,
    viewer: Option<&Viewer>,
) -> String {
    layout::page(
        &format!("error {status}"),
        "",
        viewer,
        view! {
            <section class="error">
                <p class="error-status">{status}</p>
                <h1>{code.to_string()}</h1>
                <p>{message.to_string()}</p>
                <p class="actions">
                    <a class="btn" href="/">"back to the registry"</a>
                    <a class="btn btn-ghost" href="/crates">"browse crates"</a>
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
}
