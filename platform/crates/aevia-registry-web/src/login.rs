//! The sign-in page.
//!
//! It exists so that the choice of identity provider is the reader's, not the
//! server's. Redirecting `/login` straight at GitHub would quietly demote ORCID
//! researchers to second-class users, and this registry is for a scientific
//! language where an ORCID iD is the more natural identifier.
//!
//! The page also states the model plainly — no password is stored, the provider
//! vouches for the handle — because a sign-in page is where a cautious reader
//! decides whether to trust the site at all.

use aevia_registry_core::vm::Viewer;
use leptos::prelude::*;

use crate::layout;
use crate::query_param;

/// Renders `/login`.
///
/// `github` and `orcid` say which providers this deployment has configured; a
/// provider that is not configured is shown as unavailable rather than hidden,
/// so an operator can see at a glance what is missing.
pub fn login(
    viewer: Option<&Viewer>,
    next: &str,
    github: bool,
    orcid: bool,
) -> String {
    let next_github = format!("/auth/github/start?next={}", query_param(next));
    let next_orcid = format!("/auth/orcid/start?next={}", query_param(next));
    layout::page(
        "sign in",
        "",
        viewer,
        view! {
            <section class="signin">
                <h1>"sign in"</h1>
                <p class="lede">
                    "The registry keeps no passwords. Your provider vouches for your handle, and \
                     a key you hold signs every publish."
                </p>
                <div class="provider-list">
                    {if github {
                        view! {
                            <a class="provider" href=next_github>
                                <span class="provider-name">"Continue with GitHub"</span>
                                <span class="provider-note">
                                    "also enables publishing under an organisation"
                                </span>
                            </a>
                        }
                        .into_any()
                    } else {
                        view! {
                            <span class="provider provider-off">
                                <span class="provider-name">"GitHub"</span>
                                <span class="provider-note">"not configured on this registry"</span>
                            </span>
                        }
                        .into_any()
                    }}
                    {if orcid {
                        view! {
                            <a class="provider" href=next_orcid>
                                <span class="provider-name">"Continue with ORCID"</span>
                                <span class="provider-note">
                                    "for researchers who already have an iD"
                                </span>
                            </a>
                        }
                        .into_any()
                    } else {
                        view! {
                            <span class="provider provider-off">
                                <span class="provider-name">"ORCID"</span>
                                <span class="provider-note">"not configured on this registry"</span>
                            </span>
                        }
                        .into_any()
                    }}
                </div>
                <p class="note">
                    "Signing in creates a publisher identity only. Publishing needs a signing key \
                     bound to it: run "
                    <code>"aevia-pkg login"</code>
                    " and approve the code it prints."
                </p>
            </section>
        },
    )
}
