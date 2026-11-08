//! `aevia-pkg add` and `aevia-pkg verify`: fetching and checking a version.
//!
//! # What "verified" means here
//!
//! Three independent things are checked before a byte is written to disk:
//!
//! 1. **The archive matches the digest the registry published.** The
//!    `X-Aevia-Checksum` header is compared against the bytes actually
//!    received, so a truncated or substituted download fails even before the
//!    signature is looked at.
//! 2. **The signature covers a statement naming this exact archive.** The
//!    statement is parsed and its name, version and checksum are compared with
//!    what was asked for. A valid signature over some *other* statement would
//!    otherwise pass.
//! 3. **The signature verifies under the registered key.** The key is fetched
//!    from the registry's public-key endpoint, its fingerprint is compared with
//!    the one that signed, and the detached signature is checked with
//!    [`aevia_registry_core::signature::verify_detached`].
//!
//! The key is fetched from the registry rather than taken from the download's
//! own headers, because a header travels with the archive: trusting it would
//! let whoever served the bytes also choose the key they are checked against.
//!
//! # Why `add` refuses an unknown directory
//!
//! A vendored tree that `aevia-pkg` did not install has no recorded digest, so
//! there is nothing to compare against. Overwriting it could destroy local
//! edits; trusting it could vendor something nobody verified. It is refused,
//! with the command that resolves it.

use std::path::Path;

use aevia_registry_core::PackageId;
use aevia_registry_core::signature::SignerKey;
use aevia_registry_core::statement::PublishStatement;
use aevia_registry_core::tarball;
use aevia_registry_core::vm::human_bytes;

use crate::App;
use crate::error::CliError;
use crate::package;

/// The file that records which archive was unpacked into a vendor directory.
const MARKER: &str = ".aevia-pkg-checksum";

/// The armour headers that start a detached signature.
const ARMOUR: [&str; 2] = [
    "-----BEGIN PGP SIGNATURE-----",
    "-----BEGIN SSH SIGNATURE-----",
];

/// A downloaded version that passed every check.
#[derive(Clone, Debug)]
pub struct Verified {
    /// The package.
    pub id: PackageId,
    /// The version that was downloaded.
    pub version: String,
    /// `sha256:<hex>` of the bytes in [`Verified::bytes`].
    pub checksum: String,
    /// The fingerprint of the key that signed it.
    pub fingerprint: String,
    /// `openpgp` or `ssh`.
    pub key_kind: String,
    /// The account the key is bound to, as `handle (kind)`.
    pub publisher: Option<String>,
    /// The archive.
    pub bytes: Vec<u8>,
}

/// `aevia-pkg verify`: download one version and check it end to end.
///
/// # Errors
///
/// The registry's error, a checksum mismatch, a missing statement, a signature
/// that does not verify, or a key that is not bound to an account.
pub async fn verify(
    app: &App,
    id: &PackageId,
    version: &str,
) -> Result<Verified, CliError> {
    let download = app.client.download(id.owner(), id.name(), version).await?;

    // The index is only consulted when a header is missing: it is the same
    // registry, but it is a second, independently stored copy of the digest and
    // the fingerprint, so a proxy that stripped headers cannot silence the
    // check.
    let index = if download.checksum.is_none() || download.fingerprint.is_none() {
        Some(app.client.index(id.owner(), id.name()).await?)
    } else {
        None
    };
    let record = index.as_ref().and_then(|records| {
        records
            .iter()
            .find(|record| record.vers == version)
            .map(|record| (record.cksum.clone(), record.signature.fingerprint.clone()))
    });

    let checksum = download
        .checksum
        .clone()
        .or_else(|| record.as_ref().map(|(cksum, _)| cksum.clone()))
        .ok_or_else(|| {
            CliError::new(format!(
                "the registry sent no checksum for `{id}@{version}`, so the archive cannot be \
                 verified"
            ))
            .with_next_step("report this to the registry; `aevia-pkg` will not unpack an archive it cannot check")
        })?;
    tarball::verify_checksum(&download.bytes, &checksum).map_err(CliError::from)?;

    let payload = app.client.signature(id.owner(), id.name(), version).await?;
    let (statement_text, signature) = split_signature_payload(&payload)?;
    let statement = PublishStatement::parse(&statement_text).map_err(|error| {
        CliError::new(format!(
            "the signed statement does not parse: {}",
            error.message
        ))
    })?;
    if statement.name != *id {
        return Err(CliError::new(format!(
            "the signature covers `{}`, not `{id}`",
            statement.name
        ))
        .with_next_step("the registry served another package's signature; report this"));
    }
    if statement.version != version {
        return Err(CliError::new(format!(
            "the signature covers version `{}`, not `{version}`",
            statement.version
        ))
        .with_next_step("the registry served another version's signature; report this"));
    }
    if !statement.checksum.eq_ignore_ascii_case(&checksum) {
        return Err(CliError::new(format!(
            "the signature covers {}, but the archive is {checksum}",
            statement.checksum
        ))
        .with_next_step("the archive was altered in transit or at rest; do not use it"));
    }

    let fingerprint = download
        .fingerprint
        .clone()
        .or_else(|| record.as_ref().map(|(_, fingerprint)| fingerprint.clone()))
        .ok_or_else(|| {
            CliError::new(format!(
                "the registry named no signing key for `{id}@{version}`"
            ))
            .with_next_step("report this to the registry")
        })?;

    let status = app.client.key_status(&fingerprint).await?;
    if !status.registered {
        return Err(CliError::new(format!(
            "the archive was signed by `{fingerprint}`, which is not bound to any account"
        ))
        .with_next_step("the key may have been revoked; treat this download as untrusted"));
    }
    let armored = app.client.public_key(&fingerprint).await?;
    let signer = SignerKey::detect(&armored).map_err(CliError::from)?;
    // The endpoint is addressed by fingerprint, but the server chooses what to
    // return: comparing the key it sent with the key that was asked for is what
    // makes the lookup meaningful.
    let actual = signer.fingerprint().map_err(CliError::from)?;
    if !actual.eq_ignore_ascii_case(&fingerprint) {
        return Err(CliError::new(format!(
            "the registry returned key `{actual}` when `{fingerprint}` was requested"
        ))
        .with_next_step("treat this download as untrusted and report it"));
    }
    aevia_registry_core::signature::verify_detached(&signer, &signature, statement_text.as_bytes())
        .map_err(CliError::from)?;

    Ok(Verified {
        id: id.clone(),
        version: version.to_string(),
        checksum,
        fingerprint,
        key_kind: signer.kind.as_str().to_string(),
        publisher: status.publisher.map(|publisher| publisher.label()),
        bytes: download.bytes,
    })
}

/// `aevia-pkg verify <owner/name>@<version>`.
///
/// # Errors
///
/// As [`verify`], plus a malformed target.
pub async fn verify_command(
    app: &App,
    spec: &str,
) -> Result<(), CliError> {
    let (id, version) = package::parse_target(spec)?;
    let Some(version) = version else {
        return Err(CliError::usage(format!(
            "`{spec}` names no version; write it as `{id}@0.2.0`"
        )));
    };
    let verified = verify(app, &id, &version).await?;
    println!("verified {} {}", verified.id, verified.version);
    println!("checksum  {}", verified.checksum);
    println!(
        "key       {} {}{}",
        verified.key_kind,
        verified.fingerprint,
        match &verified.publisher {
            | Some(publisher) => format!(" ({publisher})"),
            | None => String::new(),
        }
    );
    Ok(())
}

/// `aevia-pkg add`: download, verify, unpack into `vendor/<name>/`.
///
/// # Errors
///
/// As [`verify`], plus a refusal when the vendor directory already holds
/// something else, or a filesystem failure while unpacking.
pub async fn add(
    app: &App,
    spec: &str,
    dir: Option<&Path>,
) -> Result<(), CliError> {
    let (id, requested) = package::parse_target(spec)?;
    let version = match requested {
        | Some(version) => version,
        | None => {
            let page = app.client.crate_page(id.owner(), id.name()).await?;
            page.selected.version
        },
    };
    let verified = verify(app, &id, &version).await?;

    let base = match dir {
        | Some(dir) => dir.to_path_buf(),
        | None => std::env::current_dir()
            .map_err(|error| CliError::new(format!("cannot read the current directory: {error}")))
            .map(|cwd| package::find_root(&cwd).unwrap_or(cwd))?,
    };
    let dest = base.join("vendor").join(id.name());

    if dest.exists() {
        let marker = std::fs::read_to_string(dest.join(MARKER)).ok();
        let recorded = marker
            .as_deref()
            .and_then(|text| text.lines().next())
            .map(str::trim)
            .unwrap_or_default()
            .to_string();
        if recorded.eq_ignore_ascii_case(&verified.checksum) {
            println!(
                "{} {} is already vendored in `{}`",
                verified.id,
                verified.version,
                dest.display()
            );
            print_dependency_line(&id);
            return Ok(());
        }
        return Err(CliError::new(format!(
            "`{}` already exists and {}",
            dest.display(),
            if recorded.is_empty() {
                "was not installed by `aevia-pkg`, so its contents are unknown".to_string()
            } else {
                format!("holds {recorded}, not {}", verified.checksum)
            }
        ))
        .with_next_step(format!(
            "remove `{}` and run the command again to vendor {} {}",
            dest.display(),
            verified.id,
            verified.version
        )));
    }

    let written = tarball::unpack(&verified.bytes, &dest).map_err(CliError::from)?;
    std::fs::write(
        dest.join(MARKER),
        format!(
            "{}\n{} {}\n",
            verified.checksum, verified.id, verified.version
        ),
    )
    .map_err(|error| {
        CliError::new(format!(
            "cannot record the checksum in `{}`: {error}",
            dest.display()
        ))
    })?;

    println!(
        "vendored {} {} into `{}` ({} files, {})",
        verified.id,
        verified.version,
        dest.display(),
        written.len(),
        human_bytes(verified.bytes.len() as u64)
    );
    println!(
        "checksum  {} (signed by {} {})",
        verified.checksum, verified.key_kind, verified.fingerprint
    );
    print_dependency_line(&id);
    Ok(())
}

/// Prints the `aevia.toml` line to paste, in the toolchain's own spelling.
fn print_dependency_line(id: &PackageId) {
    println!();
    println!("add this to `aevia.toml`:");
    println!();
    println!(
        "{} = {{ path = \"vendor/{}\" }}",
        package::import_root(id.name()),
        id.name()
    );
}

/// Splits a signature response into the statement it covers and the signature.
///
/// The registry serves the publish statement and the armoured signature over it
/// in one plain-text body, because a detached signature on its own proves
/// nothing: the verifier needs the exact bytes that were signed, and the
/// statement carries a server-issued nonce and expiry that cannot be
/// reconstructed locally. The split is by the first armour header, so the order
/// inside the body does not matter as long as the statement comes first.
///
/// # Errors
///
/// `E_SIGNATURE` when there is no armour block, or nothing before it.
pub fn split_signature_payload(payload: &str) -> Result<(String, String), CliError> {
    let mut offset = 0;
    let mut start = None;
    for line in payload.split_inclusive('\n') {
        if ARMOUR
            .iter()
            .any(|header| line.trim_start().starts_with(header))
        {
            start = Some(offset);
            break;
        }
        offset += line.len();
    }
    let Some(start) = start else {
        return Err(
            CliError::new("the registry served no armoured signature for this version")
                .with_next_step("the archive cannot be verified without one; do not use it"),
        );
    };
    // The statement's exact bytes, including the newline that ends its last
    // field: that newline is part of what was signed, so trimming it would
    // verify a different string than the publisher signed.
    let statement = payload.get(..start).unwrap_or_default().to_string();
    let signature = payload.get(start..).unwrap_or_default().trim().to_string();
    if statement.is_empty() || !statement.ends_with('\n') {
        return Err(CliError::new(
            "the registry served a signature but not the statement it covers, so nothing can be \
             verified",
        )
        .with_next_step(
            "report this to the registry: `/api/v1/download/{owner}/{name}/{version}/signature` \
             must return the publish statement followed by the signature",
        ));
    }
    Ok((statement, signature))
}

#[cfg(test)]
mod tests {
    use super::*;

    const STATEMENT: &str = "aevia-publish-v1\n\
                             name: apich/units-extra\n\
                             version: 0.2.0\n\
                             checksum: sha256:abababababababababababababababababababababababababababababababab\n\
                             nonce: 0123456789abcdef0123456789abcdef\n\
                             expires: 1794000300\n";

    #[test]
    fn a_statement_followed_by_a_signature_splits_at_the_armour() {
        let payload = format!(
            "{STATEMENT}-----BEGIN PGP SIGNATURE-----\n\nabc\n-----END PGP SIGNATURE-----\n"
        );
        let (statement, signature) = split_signature_payload(&payload).expect("splits");
        assert_eq!(statement, STATEMENT, "the signed bytes, newline included");
        assert!(signature.starts_with("-----BEGIN PGP SIGNATURE-----"));
        assert!(signature.ends_with("-----END PGP SIGNATURE-----"));
    }

    #[test]
    fn an_ssh_signature_splits_the_same_way() {
        let payload = format!(
            "{STATEMENT}\n-----BEGIN SSH SIGNATURE-----\nAAAA\n-----END SSH SIGNATURE-----\n"
        );
        let (_, signature) = split_signature_payload(&payload).expect("splits");
        assert!(signature.starts_with("-----BEGIN SSH SIGNATURE-----"));
    }

    #[test]
    fn a_signature_without_a_statement_is_refused() {
        let error =
            split_signature_payload("-----BEGIN PGP SIGNATURE-----\nabc\n").expect_err("refused");
        assert!(error.message.contains("statement"), "{}", error.message);
        assert!(error.next_step.is_some());
    }

    #[test]
    fn a_body_with_no_armour_is_refused() {
        let error = split_signature_payload("not a signature").expect_err("refused");
        assert!(
            error.message.contains("armoured signature"),
            "{}",
            error.message
        );
    }
}
