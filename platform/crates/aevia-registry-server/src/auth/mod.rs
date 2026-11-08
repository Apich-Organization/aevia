//! Sign-in: GitHub App user authorisation and ORCID.
//!
//! # Why there are no passwords
//!
//! The registry never stores a credential. Signing in delegates the proof of
//! identity to GitHub or ORCID, and the registry keeps only what those
//! providers assert: a handle, a display name, an avatar. There is nothing here
//! to leak, nothing to reset, and no password policy to get wrong.
//!
//! # Why the GitHub side is an App and not an OAuth App
//!
//! An OAuth App can read a user's public profile and nothing else. A GitHub App
//! additionally knows which installations the user can reach, which is what
//! lets the registry confirm organisation membership: publishing as
//! `apich-organization/units` requires the App to be installed on that
//! organisation *and* the signed-in user to have access to the installation.
//! That check is what stops a package from claiming an organisation's scope.
//!
//! Both flows end the same way: a `publishers` row and a session cookie. The
//! device-authorisation flow in [`device`] then binds a signing key to that row,
//! and from that point the browser is not needed to publish.

pub mod device;
pub mod github;
pub mod orcid;

use aevia_registry_db::models::Publisher;

use crate::state::AppState;

/// The provider a publisher signed in with.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Provider {
    /// GitHub, through a GitHub App.
    Github,
    /// ORCID, through its OAuth server.
    Orcid,
}

impl Provider {
    /// The path segment, `github` or `orcid`.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            | Self::Github => "github",
            | Self::Orcid => "orcid",
        }
    }
}

/// A person as a provider described them.
pub struct Identity {
    /// `github` or `orcid`.
    pub kind: &'static str,
    /// The normalised handle.
    pub handle: String,
    /// The provider's display name.
    pub display_name: Option<String>,
    /// The provider's avatar.
    pub avatar_url: Option<String>,
}

/// Records or refreshes the publisher for an identity.
///
/// # Errors
///
/// `E_DB` when the row cannot be written.
pub async fn upsert(
    state: &AppState,
    identity: &Identity,
) -> Result<Publisher, aevia_registry_core::Error> {
    aevia_registry_db::repo::upsert_publisher(
        &state.pool,
        identity.kind,
        &identity.handle,
        identity.display_name.as_deref(),
        identity.avatar_url.as_deref(),
    )
    .await
}
