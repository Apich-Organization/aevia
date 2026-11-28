//! Signature verification: `OpenPGP` first, SSH keys second.
//!
//! # The key is the account
//!
//! There is no password, no API token and no session cookie in the publish
//! path. A publisher proves who they are by signing the statement in
//! [`crate::statement`] with a key they hold, and the registry accepts the
//! publish because that key is already bound to a verified account. The binding
//! is established once, through a browser flow (see the server's device
//! authorisation), and the private key never leaves the publisher's machine.
//!
//! That is a deliberate departure from `cargo login`. A bearer token is a
//! secret that has to be transported, stored and rotated; a signature is a
//! proof that is generated on demand and cannot be reused. The trade is that
//! key management becomes the publisher's job — which is where `gpg-agent` and
//! `ssh-agent` already are, with hardware tokens behind them.
//!
//! # What is accepted
//!
//! * **`OpenPGP`**, detached and ASCII-armoured: `gpg --detach-sign --armor`.
//!   This is the recommended path, because `GnuPG` supports offline and hardware
//!   keys, expiring subkeys and revocation certificates.
//! * **SSH**, the `SSHSIG` format produced by `ssh-keygen -Y sign`, for
//!   Ed25519 keys. The signed-data construction is rebuilt byte for byte from
//!   `PROTOCOL.sshsig`, so a signature made by OpenSSH verifies here without
//!   shelling out to `ssh-keygen`.
//!
//! Anything else is refused with the reason, never silently ignored.

use base64::Engine as _;
use pgp::composed::Deserializable as _;
use pgp::composed::DetachedSignature;
use pgp::composed::SignedPublicKey;
use pgp::packet::Signature;
use pgp::packet::SignatureType;
use pgp::types::KeyDetails as _;
use pgp::types::Tag;
use sha2::Digest as _;

use crate::error::E_KEY;
use crate::error::E_SIGNATURE;
use crate::error::Error;
use crate::error::Result;

/// The SSHSIG namespace expected on signatures made for this registry.
pub const SSH_NAMESPACE: &str = "aevia";

const MAX_KEY_BYTES: usize = 256 * 1024;
const MAX_SIGNATURE_BYTES: usize = 64 * 1024;

/// Warning surfaced when an SSH key is registered or used instead of `OpenPGP`.
pub const SSH_PREFERENCE_WARNING: &str = "SSH keys are supported, but OpenPGP (GPG) keys are \
    preferred; for stronger release security, we recommend using an OpenPGP key backed by a \
    hardware key (such as a YubiKey or OpenPGP smartcard).";

/// Which signature system a key belongs to.
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum KeyKind {
    /// An `OpenPGP` key, as `GnuPG` stores it.
    #[serde(rename = "openpgp")]
    OpenPgp,
    /// An SSH key used through `ssh-keygen -Y sign`.
    #[serde(rename = "ssh")]
    Ssh,
}

impl KeyKind {
    /// The wire name, `openpgp` or `ssh`.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            | Self::OpenPgp => "openpgp",
            | Self::Ssh => "ssh",
        }
    }

    /// Parses the wire name.
    #[must_use]
    pub fn parse(text: &str) -> Option<Self> {
        match text {
            | "openpgp" | "pgp" | "gpg" => Some(Self::OpenPgp),
            | "ssh" => Some(Self::Ssh),
            | _ => None,
        }
    }

    /// Policy warning shown when a key kind other than `OpenPGP` is registered or
    /// used to sign a release.
    #[must_use]
    pub const fn preference_warning(self) -> Option<&'static str> {
        match self {
            | Self::OpenPgp => None,
            | Self::Ssh => Some(SSH_PREFERENCE_WARNING),
        }
    }
}

/// A public key as a publisher uploaded it.
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct SignerKey {
    /// The system the key belongs to.
    pub kind: KeyKind,
    /// The key itself, ASCII-armoured.
    pub armored: String,
}

impl SignerKey {
    /// Builds a key, sniffing the kind from the armour.
    ///
    /// # Errors
    ///
    /// `E_KEY` when the armour is neither an `OpenPGP` public key block nor an
    /// OpenSSH public key line.
    pub fn detect(armored: &str) -> Result<Self> {
        let text = armored.trim();
        let kind = if text.contains("BEGIN PGP PUBLIC KEY BLOCK") {
            KeyKind::OpenPgp
        } else if text.starts_with("ssh-") || text.starts_with("ecdsa-") {
            KeyKind::Ssh
        } else {
            return Err(Error::new(
                E_KEY,
                "the key is neither an `-----BEGIN PGP PUBLIC KEY BLOCK-----` block nor an \
                 OpenSSH public key line",
            ));
        };
        Ok(Self {
            kind,
            armored: text.to_string(),
        })
    }

    /// The key's fingerprint, in the form its own tool prints it.
    ///
    /// * `OpenPGP`: upper-case hexadecimal, no spaces — what `gpg --fingerprint`
    ///   shows once its spaces are removed.
    /// * SSH: `SHA256:<base64>` — what `ssh-keygen -lf` shows.
    ///
    /// # Errors
    ///
    /// `E_KEY` when the key does not parse.
    pub fn fingerprint(&self) -> Result<String> {
        check_key_size(&self.armored)?;
        match self.kind {
            | KeyKind::OpenPgp => {
                let fingerprint = openpgp_key(&self.armored)?.fingerprint().to_string();
                Ok(fingerprint.replace(' ', "").to_ascii_uppercase())
            },
            | KeyKind::Ssh => {
                let blob = ssh_key_blob(&self.armored)?;
                Ok(ssh_fingerprint(&blob))
            },
        }
    }

    /// Checks that this key is valid and supported for signing and verifying
    /// releases (any valid `OpenPGP` key, or an `ssh-ed25519` SSH key).
    ///
    /// # Errors
    ///
    /// `E_KEY` when the key cannot be parsed or uses an unsupported SSH
    /// algorithm.
    pub fn validate(&self) -> Result<()> {
        check_key_size(&self.armored)?;
        match self.kind {
            | KeyKind::OpenPgp => {
                let _ = openpgp_key(&self.armored)?;
            },
            | KeyKind::Ssh => {
                let blob = ssh_key_blob(&self.armored)?;
                let _ = ssh_ed25519_verifying_key(&blob)?;
            },
        }
        Ok(())
    }
}

/// Verifies a detached signature over `message`.
///
/// # Errors
///
/// `E_SIGNATURE` when the signature does not verify, `E_KEY` when either the
/// key or the signature does not parse. The two are kept apart because the
/// first means "wrong key or tampered archive" and the second means "wrong
/// format", and a publisher needs to tell them apart.
pub fn verify_detached(
    key: &SignerKey,
    signature: &str,
    message: &[u8],
) -> Result<()> {
    verify(key, signature, message, None)
}

/// Verifies a new registry action using the stored certificate.
///
/// The UTC Unix timestamp applies to the certificate, signing subkey and
/// detached signature. Historical archive verification uses `verify_detached`.
///
/// # Errors
///
/// `E_KEY` for a malformed or inactive certificate and `E_SIGNATURE` if no active
/// signing key verifies this statement.
pub fn verify_current(
    key: &SignerKey,
    signature: &str,
    message: &[u8],
    now: i64,
) -> Result<()> {
    verify(key, signature, message, Some(now))
}

fn verify(
    key: &SignerKey,
    signature: &str,
    message: &[u8],
    now: Option<i64>,
) -> Result<()> {
    check_key_size(&key.armored)?;
    if signature.len() > MAX_SIGNATURE_BYTES {
        return Err(Error::new(
            E_SIGNATURE,
            "the detached signature is over 64 KiB",
        ));
    }
    match key.kind {
        | KeyKind::OpenPgp => {
            let key = openpgp_key(&key.armored)?;
            let signature = openpgp_signature(signature)?;
            verify_openpgp(&key, &signature, message, now)
        },
        | KeyKind::Ssh => verify_ssh(&key.armored, signature, message),
    }
}

fn check_key_size(armored: &str) -> Result<()> {
    if armored.len() > MAX_KEY_BYTES {
        return Err(Error::new(E_KEY, "the public key is over 256 KiB"));
    }
    Ok(())
}

#[allow(
    clippy::single_call_fn,
    reason = "OpenPGP verification has certificate and signing subkey rules"
)]
fn verify_openpgp(
    key: &SignedPublicKey,
    signed: &DetachedSignature,
    message: &[u8],
    now: Option<i64>,
) -> Result<()> {
    if let Some(now) = now {
        if !primary_current(key, now) {
            return Err(Error::new(
                E_KEY,
                "the registered OpenPGP certificate is expired, revoked, or has no current self-signature",
            ));
        }
        if !created(&signed.signature).is_some_and(|signed_at| {
            signed_at <= now.saturating_add(300)
                && signature_current(&signed.signature, now.max(signed_at))
        }) {
            return Err(Error::new(
                E_SIGNATURE,
                "the OpenPGP signature is expired or has an invalid creation time",
            ));
        }
    }
    if signed.verify(&key.primary_key, message).is_ok() {
        return Ok(());
    }
    for subkey in &key.public_subkeys {
        if now.is_some_and(|now| {
            subkey.signatures.iter().any(|binding| {
                binding.typ() == Some(SignatureType::SubkeyRevocation)
                    && created(binding).is_some_and(|created| created <= now)
                    && binding
                        .verify_subkey_binding(&key.primary_key, &subkey.key)
                        .is_ok()
            })
        }) {
            continue;
        }
        let binding = subkey
            .signatures
            .iter()
            .filter(|binding| {
                binding.typ() == Some(SignatureType::SubkeyBinding)
                    && now.is_none_or(|now| signature_current(binding, now))
                    && binding
                        .verify_subkey_binding(&key.primary_key, &subkey.key)
                        .is_ok()
            })
            .max_by_key(|binding| created(binding));
        let Some(binding) = binding else {
            continue;
        };
        if binding.key_flags().sign()
            && binding.embedded_signature().is_some_and(|back_signature| {
                back_signature
                    .verify_primary_key_binding(&subkey.key, &key.primary_key)
                    .is_ok()
            })
            && now.is_none_or(|now| key_current(&subkey.key, binding, now))
            && signed.verify(&subkey.key, message).is_ok()
        {
            return Ok(());
        }
    }
    Err(Error::new(
        E_SIGNATURE,
        "no eligible OpenPGP key verifies this statement",
    ))
}

#[allow(
    clippy::single_call_fn,
    reason = "primary certificate validity requires its own self-signature and revocation evaluation"
)]
fn primary_current(
    key: &SignedPublicKey,
    now: i64,
) -> bool {
    if key.details.revocation_signatures.iter().any(|revocation| {
        created(revocation).is_some_and(|created| created <= now)
            && revocation.verify_key(&key.primary_key).is_ok()
    }) {
        return false;
    }
    let direct = key.details.direct_signatures.iter().filter(|signature| {
        signature_current(signature, now) && signature.verify_key(&key.primary_key).is_ok()
    });
    let certified = key.details.users.iter().flat_map(|user| {
        user.signatures.iter().filter(|signature| {
            matches!(
                signature.typ(),
                Some(
                    SignatureType::CertGeneric
                        | SignatureType::CertPersona
                        | SignatureType::CertCasual
                        | SignatureType::CertPositive
                )
            ) && signature_current(signature, now)
                && signature
                    .verify_certification(&key.primary_key, Tag::UserId, &user.id)
                    .is_ok()
        })
    });
    direct
        .chain(certified)
        .max_by_key(|signature| created(signature))
        .is_some_and(|signature| key_current(&key.primary_key, signature, now))
}

fn created(signature: &Signature) -> Option<i64> {
    signature.created().map(|at| i64::from(at.as_secs()))
}

fn signature_current(
    signature: &Signature,
    now: i64,
) -> bool {
    created(signature).is_some_and(|created| {
        created <= now
            && signature
                .signature_expiration_time()
                .is_none_or(|duration| {
                    duration.as_secs() == 0
                        || now < created.saturating_add(i64::from(duration.as_secs()))
                })
    })
}

fn key_current(
    key: &impl pgp::types::KeyDetails,
    binding: &Signature,
    now: i64,
) -> bool {
    let created = i64::from(key.created_at().as_secs());
    created <= now
        && binding.key_expiration_time().is_none_or(|duration| {
            duration.as_secs() == 0 || now < created.saturating_add(i64::from(duration.as_secs()))
        })
}

fn openpgp_key(armored: &str) -> Result<pgp::composed::SignedPublicKey> {
    let (key, _headers) =
        pgp::composed::SignedPublicKey::from_string(armored).map_err(|error| {
            Error::new(
                E_KEY,
                format!("the OpenPGP public key does not parse: {error}"),
            )
        })?;
    Ok(key)
}

#[allow(
    clippy::single_call_fn,
    reason = "OpenPGP armor and packet decoding are distinct from key verification"
)]
fn openpgp_signature(armored: &str) -> Result<pgp::composed::DetachedSignature> {
    let text = armored.trim();
    if !text.contains("BEGIN PGP SIGNATURE") {
        return Err(Error::new(
            E_SIGNATURE,
            "the signature is not ASCII-armoured; sign with `gpg --detach-sign --armor`",
        ));
    }
    let (signature, _headers) =
        pgp::composed::DetachedSignature::from_string(text).map_err(|error| {
            Error::new(
                E_SIGNATURE,
                format!("the OpenPGP signature does not parse: {error}"),
            )
        })?;
    Ok(signature)
}

/// The raw `ssh-ed25519` key blob from an `authorized_keys` line.
fn ssh_key_blob(armored: &str) -> Result<Vec<u8>> {
    let line = armored
        .lines()
        .map(str::trim)
        .find(|line| !line.is_empty() && !line.starts_with('#'))
        .ok_or_else(|| Error::new(E_KEY, "the SSH key is empty"))?;
    let mut fields = line.split_whitespace();
    let algorithm = fields
        .next()
        .ok_or_else(|| Error::new(E_KEY, "the SSH key line is empty"))?;
    let encoded = fields
        .next()
        .ok_or_else(|| Error::new(E_KEY, "the SSH key line has no key material"))?;
    if !algorithm.starts_with("ssh-") && !algorithm.starts_with("ecdsa-") {
        return Err(Error::new(
            E_KEY,
            format!("`{algorithm}` is not an SSH public key type"),
        ));
    }
    base64::engine::general_purpose::STANDARD
        .decode(encoded)
        .map_err(|error| Error::new(E_KEY, format!("the SSH key is not base64: {error}")))
}

/// `SHA256:<base64 without padding>`, matching `ssh-keygen -lf`.
#[allow(
    clippy::single_call_fn,
    reason = "the OpenSSH fingerprint format is an independent encoding contract"
)]
fn ssh_fingerprint(blob: &[u8]) -> String {
    let digest = sha2::Sha256::digest(blob);
    let encoded = base64::engine::general_purpose::STANDARD_NO_PAD.encode(digest);
    format!("SHA256:{encoded}")
}

/// The `SSHSIG` armour, parsed into its parts.
struct SshSignature {
    public_key: Vec<u8>,
    namespace: String,
    reserved: Vec<u8>,
    hash_algorithm: String,
    signature: Vec<u8>,
}

#[allow(
    clippy::single_call_fn,
    reason = "SSHSIG wire framing is checked before trusting any field"
)]
fn parse_ssh_signature(armored: &str) -> Result<SshSignature> {
    let text = armored.trim();
    let body = text
        .strip_prefix("-----BEGIN SSH SIGNATURE-----")
        .and_then(|rest| rest.strip_suffix("-----END SSH SIGNATURE-----"))
        .ok_or_else(|| {
            Error::new(
                E_SIGNATURE,
                "the signature is not an SSHSIG block; sign with `ssh-keygen -Y sign -n aevia`",
            )
        })?;
    let cleaned: String = body.split_whitespace().collect();
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(cleaned)
        .map_err(|error| {
            Error::new(
                E_SIGNATURE,
                format!("the SSHSIG block is not base64: {error}"),
            )
        })?;
    let mut reader = SshReader::new(&bytes);
    let magic = reader
        .raw(6)
        .ok_or_else(|| Error::new(E_SIGNATURE, "the SSHSIG block is too short"))?;
    if magic != b"SSHSIG" {
        return Err(Error::new(E_SIGNATURE, "the SSHSIG magic is missing"));
    }
    let version = reader
        .u32()
        .ok_or_else(|| Error::new(E_SIGNATURE, "the SSHSIG block has no version"))?;
    if version != 1 {
        return Err(Error::new(
            E_SIGNATURE,
            format!("SSHSIG version {version} is not supported"),
        ));
    }
    let public_key = reader
        .string()
        .map(<[u8]>::to_vec)
        .ok_or_else(|| Error::new(E_SIGNATURE, "the SSHSIG block has no public key"))?;
    let namespace = reader
        .utf8()
        .ok_or_else(|| Error::new(E_SIGNATURE, "the SSHSIG block has no namespace"))?;
    let reserved = reader
        .string()
        .map(<[u8]>::to_vec)
        .ok_or_else(|| Error::new(E_SIGNATURE, "the SSHSIG block has no reserved field"))?;
    let hash_algorithm = reader
        .utf8()
        .ok_or_else(|| Error::new(E_SIGNATURE, "the SSHSIG block has no hash algorithm"))?;
    let signature = reader
        .string()
        .map(<[u8]>::to_vec)
        .ok_or_else(|| Error::new(E_SIGNATURE, "the SSHSIG block has no signature"))?;
    Ok(SshSignature {
        public_key,
        namespace,
        reserved,
        hash_algorithm,
        signature,
    })
}

fn ssh_ed25519_verifying_key(blob: &[u8]) -> Result<ed25519_dalek::VerifyingKey> {
    let (algorithm, key_material) = split_ssh_key(blob)?;
    if algorithm != "ssh-ed25519" {
        return Err(Error::new(
            E_KEY,
            format!(
                "`{algorithm}` SSH keys are not supported; register an OpenPGP (GPG) key \
                 (recommended) or an `ssh-ed25519` SSH key"
            ),
        ));
    }
    let key_bytes: [u8; 32] = key_material
        .try_into()
        .map_err(|_| Error::new(E_KEY, "the Ed25519 public key is not 32 bytes"))?;
    ed25519_dalek::VerifyingKey::from_bytes(&key_bytes)
        .map_err(|error| Error::new(E_KEY, format!("the Ed25519 key is invalid: {error}")))
}

#[allow(
    clippy::single_call_fn,
    reason = "SSH verification has its own namespace, key identity and signing-data rules"
)]
fn verify_ssh(
    armored_key: &str,
    armored_signature: &str,
    message: &[u8],
) -> Result<()> {
    let signature = parse_ssh_signature(armored_signature)?;
    if signature.namespace != SSH_NAMESPACE {
        return Err(Error::new(
            E_SIGNATURE,
            format!(
                "the SSHSIG namespace is `{}`, expected `{SSH_NAMESPACE}`; sign with \
                 `ssh-keygen -Y sign -n {SSH_NAMESPACE}`",
                signature.namespace
            ),
        ));
    }
    let key_blob = ssh_key_blob(armored_key)?;
    if signature.public_key != key_blob {
        return Err(Error::new(
            E_SIGNATURE,
            "the signature was made by a different key than the one presented",
        ));
    }
    let verifying_key = ssh_ed25519_verifying_key(&key_blob)?;

    // `PROTOCOL.sshsig` §3: the signed data is the raw 6-byte magic preamble
    // `"SSHSIG"` followed by the namespace, the reserved field, the hash
    // algorithm and the hash of the message, each as an SSH string.
    let mut signed = Vec::new();
    signed.extend_from_slice(b"SSHSIG");
    push_ssh_string(&mut signed, signature.namespace.as_bytes())?;
    push_ssh_string(&mut signed, &signature.reserved)?;
    push_ssh_string(&mut signed, signature.hash_algorithm.as_bytes())?;
    let digest: Vec<u8> = match signature.hash_algorithm.as_str() {
        | "sha256" => sha2::Sha256::digest(message).to_vec(),
        | "sha512" => sha2::Sha512::digest(message).to_vec(),
        | other => {
            return Err(Error::new(
                E_SIGNATURE,
                format!("`{other}` is not a hash algorithm SSHSIG defines"),
            ));
        },
    };
    push_ssh_string(&mut signed, &digest)?;

    let (signature_algorithm, raw_signature) = split_ssh_key(&signature.signature)?;
    if signature_algorithm != "ssh-ed25519" {
        return Err(Error::new(
            E_SIGNATURE,
            format!("the signature algorithm `{signature_algorithm}` is not `ssh-ed25519`"),
        ));
    }
    let signature_bytes: [u8; 64] = raw_signature
        .try_into()
        .map_err(|_| Error::new(E_SIGNATURE, "the Ed25519 signature is not 64 bytes"))?;
    let parsed = ed25519_dalek::Signature::from_bytes(&signature_bytes);
    verifying_key
        .verify_strict(&signed, &parsed)
        .map_err(|error| {
            Error::new(
                E_SIGNATURE,
                format!("the SSH signature does not verify over the statement: {error}"),
            )
        })
}

fn split_ssh_key(blob: &[u8]) -> Result<(String, Vec<u8>)> {
    let mut reader = SshReader::new(blob);
    let algorithm = reader
        .utf8()
        .ok_or_else(|| Error::new(E_KEY, "the SSH key blob has no algorithm"))?;
    let material = reader
        .string()
        .map(<[u8]>::to_vec)
        .ok_or_else(|| Error::new(E_KEY, "the SSH key blob has no key material"))?;
    Ok((algorithm, material))
}

fn push_ssh_string(
    out: &mut Vec<u8>,
    value: &[u8],
) -> Result<()> {
    let length = u32::try_from(value.len())
        .map_err(|_| Error::new(E_SIGNATURE, "an SSH signature field is too long"))?;
    out.extend_from_slice(&length.to_be_bytes());
    out.extend_from_slice(value);
    Ok(())
}

/// A minimal reader for the SSH wire encoding.
struct SshReader<'a> {
    bytes: &'a [u8],
    offset: usize,
}

impl<'a> SshReader<'a> {
    const fn new(bytes: &'a [u8]) -> Self {
        Self { bytes, offset: 0 }
    }

    fn raw(
        &mut self,
        len: usize,
    ) -> Option<&'a [u8]> {
        let end = self.offset.checked_add(len)?;
        let slice = self.bytes.get(self.offset..end)?;
        self.offset = end;
        Some(slice)
    }

    fn u32(&mut self) -> Option<u32> {
        let bytes = self.raw(4)?.try_into().ok()?;
        Some(u32::from_be_bytes(bytes))
    }

    fn string(&mut self) -> Option<&'a [u8]> {
        let length = usize::try_from(self.u32()?).ok()?;
        self.raw(length)
    }

    fn utf8(&mut self) -> Option<String> {
        String::from_utf8(self.string()?.to_vec()).ok()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_openpgp_key_is_detected() {
        let key = SignerKey::detect(
            "-----BEGIN PGP PUBLIC KEY BLOCK-----\n\nx\n-----END PGP PUBLIC KEY BLOCK-----",
        )
        .expect("detected");
        assert_eq!(key.kind, KeyKind::OpenPgp);
    }

    #[test]
    fn an_ssh_key_is_detected() {
        let key = SignerKey::detect("ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAIExample user@host")
            .expect("detected");
        assert_eq!(key.kind, KeyKind::Ssh);
    }

    #[test]
    fn garbage_is_not_a_key() {
        let error = SignerKey::detect("hello").expect_err("refused");
        assert_eq!(error.code, E_KEY);
    }

    #[test]
    fn a_non_armoured_signature_is_refused_with_instructions() {
        let key =
            SignerKey::detect("ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAIExample").expect("detected");
        let error = verify_detached(&key, "AAAA", b"text").expect_err("refused");
        assert!(
            error.message.contains("ssh-keygen -Y sign"),
            "{}",
            error.message
        );
    }

    #[test]
    fn the_ssh_fingerprint_matches_ssh_keygen() {
        // The key material of a well-known test key; the fingerprint form is
        // the one `ssh-keygen -lf` prints.
        let blob = base64::engine::general_purpose::STANDARD
            .decode("AAAAC3NzaC1lZDI1NTE5AAAAIOMqqnkVzrm0SdG6UOoqKLsabgH5C9okWi0dh2l9GKJl")
            .expect("base64");
        let fingerprint = ssh_fingerprint(&blob);
        assert!(fingerprint.starts_with("SHA256:"), "{fingerprint}");
        assert!(!fingerprint.ends_with('='), "padding is stripped");
    }

    #[test]
    fn the_ssh_reader_rejects_truncated_input() {
        let mut reader = SshReader::new(&[0, 0, 0, 9, b'a']);
        assert_eq!(reader.string(), None);
    }

    #[test]
    fn ssh_validation_accepts_ed25519_and_refuses_rsa_and_warns() {
        let ed = SignerKey::detect(
            "ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAIOMqqnkVzrm0SdG6UOoqKLsabgH5C9okWi0dh2l9GKJl \
             apich@laptop",
        )
        .expect("detected");
        ed.validate().expect("ed25519 validates");
        assert!(KeyKind::Ssh.preference_warning().is_some());
        assert!(KeyKind::OpenPgp.preference_warning().is_none());

        // An SSH-RSA key parses as OpenSSH line format, then validation refuses it cleanly.
        let mut rsa_blob = Vec::new();
        push_ssh_string(&mut rsa_blob, b"ssh-rsa").expect("algorithm");
        push_ssh_string(&mut rsa_blob, &[1, 0, 1]).expect("key material");
        let rsa_line = format!(
            "ssh-rsa {} apich@rsa",
            base64::engine::general_purpose::STANDARD.encode(&rsa_blob)
        );
        let rsa = SignerKey::detect(&rsa_line).expect("detected");
        let err = rsa.validate().expect_err("rsa refused at validation");
        assert_eq!(err.code, E_KEY);
        assert!(err.message.contains("ssh-ed25519"), "{}", err.message);
    }
}
