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

use crate::App;
use crate::config::Config;
use crate::error::CliError;
use crate::keys;

/// How long to poll when the server does not say when the code dies.
const FALLBACK_LIFETIME_SECONDS: i64 = 900;

/// `aevia-pkg login`: bind a key to an account.
///
/// # Errors
///
/// A usage error for `--key` together with `--ssh`, a key that cannot be used,
/// a device flow the publisher denies or lets expire, or a config file that
/// cannot be written.
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
                .with_next_step("generate an `ssh-ed25519` key with `ssh-keygen -t ed25519`"),
        );
    }

    let armored = keys::public_key(&local)?;
    let label = label
        .map(str::to_string)
        .unwrap_or_else(|| format!("aevia-pkg on {}", hostname()));
    let start = app
        .client
        .device_start(local.kind.as_str(), &armored, &label)
        .await?;

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

    let deadline = chrono::DateTime::parse_from_rfc3339(&start.expires_at)
        .map(|expires| expires.with_timezone(&chrono::Utc))
        .unwrap_or_else(|_| {
            chrono::Utc::now() + chrono::Duration::seconds(FALLBACK_LIFETIME_SECONDS)
        });

    let publisher = loop {
        if chrono::Utc::now() >= deadline {
            return Err(CliError::new(format!(
                "the device code expired before it was approved at {}",
                start.verification_uri
            ))
            .with_next_step("run `aevia-pkg login` again for a fresh code"));
        }
        tokio::time::sleep(start.interval()).await;
        let poll = app.client.device_poll(&start.device_code).await?;
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
fn hostname() -> String {
    ["COMPUTERNAME", "HOSTNAME", "HOST"]
        .iter()
        .filter_map(|name| std::env::var(name).ok())
        .map(|value| value.trim().to_string())
        .find(|value| !value.is_empty())
        .unwrap_or_else(|| "this machine".to_string())
}
