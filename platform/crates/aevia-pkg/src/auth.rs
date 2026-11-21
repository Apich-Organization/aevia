//! `aevia-pkg login`: the device-authorisation flow that binds a key.
//!
//! # Why a browser, and why a code
//!
//! This replaces `cargo login`. There is no token to paste, because the key
//! *is* the credential; what has to happen once is the binding between that key
//! and a verified identity (a GitHub login or an ORCID iD). Proving the
//! identity requires the provider's OAuth flow, which requires a browser — but
//! the browser may be on another machine, so the client shows a short code and
//! a URL and polls.
//!
//! The device code never leaves the terminal-to-registry channel: the
//! publisher only sees `ABCD-EFGH`, which is worthless without the device code
//! being polled. That is the same trade RFC 8628 makes, and it is what lets
//! `aevia-pkg login` work over SSH.
//!
//! On approval the client writes exactly one thing: the fingerprint. The
//! private key stays in `gpg-agent` or `ssh-agent`, and the registry stores the
//! public half.

use std::path::Path;
use std::time::Duration;

use tokio::time::Instant;

use crate::App;
use crate::config::Config;
use crate::error::CliError;
use crate::keys;

/// How long to poll when the server does not say when the code dies.
const FALLBACK_LIFETIME_SECONDS: u64 = 900;

/// `aevia-pkg login`: bind a key to an account.
///
/// # Errors
///
/// A usage error for `--key` together with `--ssh`, a key that cannot be used,
/// a device flow the publisher denies or lets expire, or a config file that
/// cannot be written.
#[allow(
    clippy::single_call_fn,
    reason = "the login command owns the authorization flow and its polling deadline"
)]
pub async fn login(
    app: &App,
    key: Option<&str>,
    ssh: Option<&Path>,
    label: Option<&str>,
) -> Result<(), CliError> {
    if key.is_some() && ssh.is_some() {
        return Err(CliError::usage(
            "`--key` and `--ssh` name two different keys; pass one of them",
        ));
    }
    let local = match ssh {
        | Some(path) => keys::ssh_key_at(path)?,
        | None => keys::resolve(&keys::discover(None)?, key, &app.config)?,
    };
    if let Some(reason) = local.unsupported_reason() {
        return Err(
            CliError::new(format!("`{}` cannot be bound: {reason}", local.fingerprint))
                .with_next_step(
                    "prefer an OpenPGP (GPG) key backed by a hardware key, or generate an \
                     `ssh-ed25519` key with `ssh-keygen -t ed25519`",
                ),
        );
    }

    let armored = keys::public_key(&local)?;
    let label = label.map_or_else(|| format!("aevia-pkg on {}", hostname()), str::to_string);
    let start = app
        .client
        .device_start(local.kind.as_str(), &armored, &label)
        .await?;
    keys::warn_key_preference(local.kind, app.quiet, &start.warnings);

    // The code and the URL are the command's output, so they go to stdout; the
    // polling chatter goes to stderr, where a `--json` pipe cannot see it.
    println!(
        "open {} and enter {}",
        start.verification_uri, start.user_code
    );
    println!(
        "sign in with GitHub or ORCID to bind {} {}",
        local.kind.as_str(),
        local.fingerprint
    );
    if !app.quiet {
        eprintln!("aevia-pkg: waiting for approval…");
    }

    // Convert the reported wall-clock expiry once, then use the same
    // monotonic deadline for both the wait interval and the network request.
    let remaining = chrono::DateTime::parse_from_rfc3339(&start.expires_at).map_or(
        Duration::from_secs(FALLBACK_LIFETIME_SECONDS),
        |expires| {
            expires
                .signed_duration_since(chrono::Utc::now())
                .to_std()
                .unwrap_or_default()
        },
    );
    let now = Instant::now();
    let deadline = now.checked_add(remaining).unwrap_or(now);
    let interval = start.interval();
    let expired = || {
        CliError::new(format!(
            "the device code expired before it was approved at {}",
            start.verification_uri
        ))
        .with_next_step("run `aevia-pkg login` again for a fresh code")
    };

    let publisher = loop {
        let now = Instant::now();
        if now >= deadline {
            return Err(expired());
        }
        let wake = now
            .checked_add(interval)
            .map_or(deadline, |next| next.min(deadline));
        tokio::time::sleep_until(wake).await;
        if Instant::now() >= deadline {
            return Err(expired());
        }
        let poll = tokio::time::timeout_at(deadline, app.client.device_poll(&start.device_code))
            .await
            .map_err(|_| expired())??;
        match poll.status.as_str() {
            | "approved" => {
                break poll.publisher.ok_or_else(|| {
                    CliError::new("the registry approved the code but named no publisher account")
                        .with_next_step("run `aevia-pkg status` and report what it says")
                })?;
            },
            | "denied" => {
                return Err(CliError::new(
                    "the code was denied in the browser, so the key is not bound",
                )
                .with_next_step("run `aevia-pkg login` again if that was a mistake"));
            },
            | "expired" => {
                return Err(
                    CliError::new("the device code expired before it was approved")
                        .with_next_step("run `aevia-pkg login` again for a fresh code"),
                );
            },
            | _ => {},
        }
    };

    // Only the fingerprint and its kind are written: enough to sign as this
    // publisher without asking which key, and nothing that could act as a
    // secret if the file were copied.
    let config = Config {
        registry: Some(app.client.base().to_string()),
        default_key: Some(local.fingerprint.clone()),
        default_key_kind: Some(local.kind),
    };
    let path = config.save(&app.home)?;
    println!("bound {} to {}", local.fingerprint, publisher.label());
    println!("wrote {}", path.display());
    Ok(())
}

/// The machine's name, for a default key label.
#[allow(
    clippy::single_call_fn,
    reason = "device labels account for platform-specific hostname discovery"
)]
fn hostname() -> String {
    ["COMPUTERNAME", "HOSTNAME", "HOST"]
        .iter()
        .filter_map(|name| std::env::var(name).ok())
        .map(|value| value.trim().to_string())
        .find(|value| !value.is_empty())
        .unwrap_or_else(|| "this machine".to_string())
}
