//! Signature verification: OpenPGP first, SSH keys second.
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
//! * **OpenPGP**, detached and ASCII-armoured: `gpg --detach-sign --armor`.
//!   This is the recommended path, because GnuPG supports offline and hardware
//!   keys, expiring subkeys and revocation certificates.
//! * **SSH**, the `SSHSIG` format produced by `ssh-keygen -Y sign`, for
//!   Ed25519 keys. The signed-data construction is rebuilt byte for byte from
//!   `PROTOCOL.sshsig`, so a signature made by OpenSSH verifies here without
//!   shelling out to `ssh-keygen`.
//!
//! Anything else is refused with the reason, never silently ignored.

use base64::Engine as _;
use pgp::composed::Deserializable as _;
use pgp::types::KeyDetails as _;
use sha2::Digest as _;

use crate::error::E_KEY;
use crate::error::E_SIGNATURE;
use crate::error::Error;
use crate::error::Result;

/// Which signature system a key belongs to.
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum KeyKind {
    /// An OpenPGP key, as GnuPG stores it.
    #[serde(rename = "openpgp")]
    OpenPgp,
    /// An SSH key used through `ssh-keygen -Y sign`.
    #[serde(rename = "ssh")]
    Ssh,
}

impl KeyKind {
    /// The wire name, `openpgp` or `ssh`.
    #[must_use]
    pub fn as_str(self) -> &'static str {
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
    /// `E_KEY` when the armour is neither an OpenPGP public key block nor an
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
    /// * OpenPGP: upper-case hexadecimal, no spaces — what `gpg --fingerprint`
    ///   shows once its spaces are removed.
    /// * SSH: `SHA256:<base64>` — what `ssh-keygen -lf` shows.
    ///
    /// # Errors
    ///
    /// `E_KEY` when the key does not parse.
    pub fn fingerprint(&self) -> Result<String> {
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
    match key.kind {
        | KeyKind::OpenPgp => {
            let key = openpgp_key(&key.armored)?;
            let signature = openpgp_signature(signature)?;
            signature.verify(&key, message).map_err(|error| {
                Error::new(
                    E_SIGNATURE,
                    format!("the OpenPGP signature does not verify over the statement: {error}"),
                )
            })
        },
        | KeyKind::Ssh => verify_ssh(&key.armored, signature, message),
    }
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
fn ssh_fingerprint(blob: &[u8]) -> String {
    let digest = sha2::Sha256::digest(blob);
    let encoded = base64::engine::general_purpose::STANDARD_NO_PAD.encode(digest);
    format!("SHA256:{encoded}")
}

/// The `SSHSIG` armour, parsed into its parts.
struct SshSignature {
    public_key: Vec<u8>,
    namespace: String,
    hash_algorithm: String,
    signature: Vec<u8>,
}

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
        .string()
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
    let _reserved = reader.string();
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
        hash_algorithm,
        signature,
    })
}

fn verify_ssh(
    armored_key: &str,
    armored_signature: &str,
    message: &[u8],
) -> Result<()> {
    let signature = parse_ssh_signature(armored_signature)?;
    let key_blob = ssh_key_blob(armored_key)?;
    if signature.public_key != key_blob {
        return Err(Error::new(
            E_SIGNATURE,
            "the signature was made by a different key than the one presented",
        ));
    }
    let (algorithm, key_material) = split_ssh_key(&key_blob)?;
    if algorithm != "ssh-ed25519" {
        return Err(Error::new(
            E_KEY,
            format!(
                "`{algorithm}` keys are not supported yet; the registry verifies `ssh-ed25519` \
                 keys, and OpenPGP keys of any kind"
            ),
        ));
    }
    let key_bytes: [u8; 32] = key_material
        .try_into()
        .map_err(|_| Error::new(E_KEY, "the Ed25519 public key is not 32 bytes"))?;
    let verifying_key = ed25519_dalek::VerifyingKey::from_bytes(&key_bytes)
        .map_err(|error| Error::new(E_KEY, format!("the Ed25519 key is invalid: {error}")))?;

    // `PROTOCOL.sshsig` §3: the signed data is the magic, the namespace, the
    // reserved field, the hash algorithm and the hash of the message, each as
    // an SSH string.
    let mut signed = Vec::new();
    push_ssh_string(&mut signed, b"SSHSIG");
    push_ssh_string(&mut signed, signature.namespace.as_bytes());
    push_ssh_string(&mut signed, b"");
    push_ssh_string(&mut signed, signature.hash_algorithm.as_bytes());
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
    push_ssh_string(&mut signed, &digest);

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
) {
    out.extend_from_slice(&(value.len() as u32).to_be_bytes());
    out.extend_from_slice(value);
}

/// A minimal reader for the SSH wire encoding.
struct SshReader<'a> {
    bytes: &'a [u8],
    offset: usize,
}

impl<'a> SshReader<'a> {
    fn new(bytes: &'a [u8]) -> Self {
        Self { bytes, offset: 0 }
    }

    fn u32(&mut self) -> Option<u32> {
        let end = self.offset.checked_add(4)?;
        let slice = self.bytes.get(self.offset..end)?;
        self.offset = end;
        Some(u32::from_be_bytes([slice[0], slice[1], slice[2], slice[3]]))
    }

    fn string(&mut self) -> Option<&'a [u8]> {
        let length = self.u32()? as usize;
        let end = self.offset.checked_add(length)?;
        let slice = self.bytes.get(self.offset..end)?;
        self.offset = end;
        Some(slice)
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
}
