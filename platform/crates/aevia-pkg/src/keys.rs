//! Finding the publisher's key, signing with it, and exporting its public half.
//!
//! # The client never holds a private key
//!
//! `aevia-pkg` shells out to the tools that already own the key — `gpg` for
//! OpenPGP, `ssh-keygen` for SSH — instead of linking an OpenPGP or Ed25519
//! implementation. That is deliberate:
//!
//! * the private key stays where it belongs, behind `gpg-agent` or
//!   `ssh-agent`, and can be a hardware token that never leaves its card;
//! * a passphrase prompt is the agent's prompt, so this program never sees a
//!   passphrase, never stores one and cannot leak one;
//! * the signature format is the tool's own, so it is exactly what every other
//!   consumer of that key will produce.
//!
//! The cost is that the machine must have the tool. When it does not, the
//! failure is reported as a usage error (exit `2`) naming the tool and how to
//! install it, and there is **no fallback**: publishing unsigned is never an
//! option, because an unsigned archive would have to be trusted by the
//! registry on the client's word alone.
//!
//! # Parsing `gpg` output
//!
//! `gpg --list-secret-keys` prints prose in the user's language, wrapped for
//! the terminal. It is for humans. `--with-colons` prints one colon-separated
//! record per line with stable field positions, and that is the only form
//! parsed here: the fingerprint is the `fpr` record that follows a `sec`
//! record, never a substring of a pretty-printed line.

use std::io::Write as _;
use std::path::Path;
use std::path::PathBuf;
use std::process::Command;
use std::process::Stdio;

use aevia_registry_core::signature::KeyKind;
use aevia_registry_core::signature::SignerKey;

use crate::config::Config;
use crate::error::CliError;

/// The SSH signature namespace, matching the registry's verifier.
///
/// The namespace is part of what is signed (`PROTOCOL.sshsig` §3), so a
/// signature made for one purpose cannot be replayed as another. `aevia` here
/// and `aevia` on the server are the same constant written twice because the
/// wire format requires both sides to agree.
pub const SSH_NAMESPACE: &str = "aevia";

/// Where a key's secret half lives.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum KeyOrigin {
    /// GnuPG holds it; signing names it by fingerprint.
    Gpg,
    /// A file pair on disk, `id_ed25519` and `id_ed25519.pub`.
    Ssh {
        /// The public key file, read for export.
        public_path: PathBuf,
        /// The private key file, passed to `ssh-keygen -Y sign -f`.
        private_path: PathBuf,
    },
}

/// A secret key this machine can sign with.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LocalKey {
    /// OpenPGP or SSH.
    pub kind: KeyKind,
    /// The fingerprint, spelled the way its own tool spells it.
    pub fingerprint: String,
    /// A human label: the OpenPGP user id, or the SSH key's comment.
    pub identity: String,
    /// Where the secret half lives.
    pub origin: KeyOrigin,
    /// Whether the registry can verify this key's signatures.
    ///
    /// An `ssh-rsa` key can sign, but the registry verifies `ssh-ed25519`
    /// only, so publishing with one would fail at the last step. Listing it as
    /// if it worked would be a trap, so it is marked and never auto-selected.
    pub supported: bool,
}

impl LocalKey {
    /// A one-line description for `key list` and error messages.
    #[must_use]
    pub fn label(&self) -> String {
        if self.identity.is_empty() {
            format!("{} {}", self.kind.as_str(), self.fingerprint)
        } else {
            format!(
                "{} {} ({})",
                self.kind.as_str(),
                self.fingerprint,
                self.identity
            )
        }
    }

    /// The reason the key cannot be used, when it cannot.
    #[must_use]
    pub fn unsupported_reason(&self) -> Option<&'static str> {
        if self.supported {
            None
        } else {
            Some("the registry verifies `ssh-ed25519` keys only")
        }
    }
}

/// Every secret key this machine holds, OpenPGP first.
///
/// A missing `gpg` is not fatal here: a publisher who only uses SSH keys should
/// still get a list. The warning goes to stderr so `--json` output stays clean.
///
/// # Errors
///
/// `E_KEY` when `~/.ssh` exists but cannot be read.
pub fn discover(extra_ssh: Option<&Path>) -> Result<Vec<LocalKey>, CliError> {
    let mut keys = Vec::new();
    match gpg_keys() {
        | Ok(mut openpgp) => keys.append(&mut openpgp),
        | Err(error) => {
            if !is_missing_tool(&error) {
                return Err(error);
            }
            eprintln!("aevia-pkg: {}", error.message);
        },
    }
    keys.append(&mut ssh_keys(extra_ssh)?);
    Ok(keys)
}

/// The OpenPGP secret keys `gpg` reports.
///
/// # Errors
///
/// A usage error when `gpg` is not installed, `E_KEY` when it fails.
pub fn gpg_keys() -> Result<Vec<LocalKey>, CliError> {
    let output = Command::new("gpg")
        .args(["--batch", "--with-colons", "--list-secret-keys"])
        .output()
        .map_err(|error| missing_tool("gpg", &error))?;
    if !output.status.success() {
        return Err(CliError::new(format!(
            "`gpg --list-secret-keys` failed: {}",
            first_line(&String::from_utf8_lossy(&output.stderr))
        )));
    }
    Ok(parse_gpg_colons(&String::from_utf8_lossy(&output.stdout)))
}

/// Parses the colon-delimited records of `gpg --with-colons`.
///
/// Records come in blocks: a `sec` line, its `fpr`, then `uid` lines, then any
/// `ssb`/`fpr` pairs for subkeys. Only the primary fingerprint is kept, because
/// that is the identity the registry binds and the name `--local-user` accepts
/// when a signing subkey exists. A revoked, expired, invalid or disabled
/// primary is skipped: it cannot sign, and offering it would only produce a
/// failure one step later.
#[must_use]
pub fn parse_gpg_colons(stdout: &str) -> Vec<LocalKey> {
    let mut keys: Vec<LocalKey> = Vec::new();
    let mut pending_primary = false;
    let mut usable = false;
    let mut current: Option<usize> = None;
    for line in stdout.lines() {
        let fields: Vec<&str> = line.split(':').collect();
        let Some(record) = fields.first() else {
            continue;
        };
        match *record {
            | "sec" => {
                usable = !matches!(
                    fields.get(1).copied().unwrap_or_default(),
                    "r" | "e" | "i" | "d" | "n"
                );
                pending_primary = true;
                current = None;
            },
            | "ssb" | "sub" => {
                pending_primary = false;
                current = None;
            },
            | "fpr" => {
                if !pending_primary {
                    continue;
                }
                pending_primary = false;
                let Some(fingerprint) = fields.get(9).filter(|value| !value.is_empty()) else {
                    continue;
                };
                if !usable {
                    continue;
                }
                keys.push(LocalKey {
                    kind: KeyKind::OpenPgp,
                    fingerprint: fingerprint.to_ascii_uppercase(),
                    identity: String::new(),
                    origin: KeyOrigin::Gpg,
                    supported: true,
                });
                current = keys.len().checked_sub(1);
            },
            | "uid" => {
                let Some(index) = current else {
                    continue;
                };
                let Some(uid) = fields.get(9).filter(|value| !value.is_empty()) else {
                    continue;
                };
                if keys.get(index).is_some_and(|key| key.identity.is_empty())
                    && let Some(key) = keys.get_mut(index)
                {
                    key.identity = unescape_colons(uid);
                }
            },
            | _ => {},
        }
    }
    keys
}

/// The SSH public keys in `~/.ssh/*.pub`, plus `extra` when given.
///
/// Only public files are enumerated: a directory of private keys cannot be
/// told apart from a directory of certificates by name, and `ssh-keygen -y`
/// would prompt for every passphrase in it. `extra` may name either half of a
/// key pair, because `--ssh ~/.ssh/id_ed25519` is what people type.
///
/// # Errors
///
/// `E_KEY` when `extra` names something that is not a key, or when a public key
/// file cannot be read or parsed.
pub fn ssh_keys(extra: Option<&Path>) -> Result<Vec<LocalKey>, CliError> {
    let mut out = Vec::new();
    if let Some(dir) = ssh_dir()
        && let Ok(entries) = std::fs::read_dir(&dir)
    {
        let mut paths: Vec<PathBuf> = entries
            .flatten()
            .map(|entry| entry.path())
            .filter(|path| {
                path.extension()
                    .is_some_and(|extension| extension.eq_ignore_ascii_case("pub"))
            })
            .collect();
        paths.sort();
        for path in paths {
            if let Some(key) = ssh_key_from(&path)? {
                out.push(key);
            }
        }
    }
    if let Some(path) = extra {
        let key = ssh_key_from(path)?.ok_or_else(|| {
            CliError::new(format!("`{}` is not an SSH public key", path.display()))
                .with_next_step("pass the `.pub` file, or the private key next to it")
        })?;
        if !out
            .iter()
            .any(|existing| existing.fingerprint == key.fingerprint)
        {
            out.push(key);
        }
    }
    Ok(out)
}

/// Reads exactly one SSH key by path, for `login --ssh`.
///
/// `--ssh` names a key the user chose, so a path that is not a key is an error
/// rather than "no keys found": silently falling back to `~/.ssh` would bind
/// whichever key happened to be listed first.
///
/// # Errors
///
/// `E_KEY` when `path` names neither a public key nor a private key whose
/// public half can be derived.
pub fn ssh_key_at(path: &Path) -> Result<LocalKey, CliError> {
    ssh_key_from(path)?.ok_or_else(|| {
        CliError::new(format!("`{}` is not an SSH key", path.display()))
            .with_next_step("pass the `.pub` file, or the private key next to it")
    })
}

/// Reads one SSH key, accepting either the public or the private half.
fn ssh_key_from(path: &Path) -> Result<Option<LocalKey>, CliError> {
    let public_path = if path
        .extension()
        .is_some_and(|extension| extension.eq_ignore_ascii_case("pub"))
    {
        path.to_path_buf()
    } else {
        PathBuf::from(format!("{}.pub", path.display()))
    };
    let line = if public_path.is_file() {
        std::fs::read_to_string(&public_path).map_err(|error| {
            CliError::new(format!("cannot read `{}`: {error}", public_path.display()))
        })?
    } else if path.is_file() {
        // A private key with no `.pub` beside it: `ssh-keygen -y` derives the
        // public half without touching the key.
        let output = Command::new("ssh-keygen")
            .arg("-y")
            .arg("-f")
            .arg(path)
            .stdin(Stdio::null())
            .output()
            .map_err(|error| missing_tool("ssh-keygen", &error))?;
        if !output.status.success() {
            return Ok(None);
        }
        String::from_utf8_lossy(&output.stdout).to_string()
    } else {
        return Ok(None);
    };
    let trimmed = line.trim();
    if trimmed.is_empty() {
        return Ok(None);
    }
    let signer = SignerKey::detect(trimmed).map_err(CliError::from)?;
    let fingerprint = signer.fingerprint().map_err(CliError::from)?;
    let supported = trimmed.starts_with("ssh-ed25519 ");
    let identity = trimmed
        .split_whitespace()
        .skip(2)
        .collect::<Vec<_>>()
        .join(" ");
    let private_path = if public_path
        .extension()
        .is_some_and(|extension| extension.eq_ignore_ascii_case("pub"))
    {
        let stem = public_path.to_string_lossy().to_string();
        PathBuf::from(
            stem.trim_end_matches(".pub")
                .trim_end_matches(".PUB")
                .to_string(),
        )
    } else {
        path.to_path_buf()
    };
    Ok(Some(LocalKey {
        kind: KeyKind::Ssh,
        fingerprint,
        identity,
        origin: KeyOrigin::Ssh {
            public_path,
            private_path,
        },
        supported,
    }))
}

/// `~/.ssh`, or nothing when there is no home directory.
fn ssh_dir() -> Option<PathBuf> {
    let home = std::env::var_os("HOME").or_else(|| std::env::var_os("USERPROFILE"))?;
    if home.is_empty() {
        return None;
    }
    Some(PathBuf::from(home).join(".ssh"))
}

/// Picks the key a command should use.
///
/// The order is explicit flag, then the config's default, then the only key on
/// the machine. Two or more candidates without a default is an error rather
/// than a guess: publishing under the wrong identity is not recoverable (the
/// registry binds a version to whoever signed it), so the client asks instead
/// of choosing.
///
/// # Errors
///
/// A usage error when no key can be chosen or the named one is not present.
pub fn resolve(
    keys: &[LocalKey],
    explicit: Option<&str>,
    config: &Config,
) -> Result<LocalKey, CliError> {
    if let Some(fingerprint) = explicit {
        return find(keys, fingerprint).cloned().ok_or_else(|| {
            CliError::usage(format!(
                "no secret key matches `{fingerprint}` among the {} key(s) on this machine",
                keys.len()
            ))
            .with_next_step("run `aevia-pkg key list` to see what is available")
        });
    }
    if let Some(fingerprint) = &config.default_key {
        return find(keys, fingerprint).cloned().ok_or_else(|| {
            CliError::new(format!(
                "the default key `{fingerprint}` from `config.toml` is not on this machine"
            ))
            .with_next_step("run `aevia-pkg login` to bind a key that is")
        });
    }
    match keys {
        | [] => Err(CliError::new(
            "no signing key found; `aevia-pkg` signs every publish with a key you hold",
        )
        .with_next_step(
            "create one with `gpg --quick-generate-key \"You <you@example.com>\" ed25519 sign 2y`, \
             then run `aevia-pkg login`",
        )),
        | [only] => Ok(only.clone()),
        | many => {
            let mut message =
                String::from("several keys are available; choose one with `--key FINGERPRINT`:");
            for key in many {
                message.push_str("\n  ");
                message.push_str(&key.label());
            }
            Err(CliError::usage(message).with_next_step(
                "or run `aevia-pkg login --key FINGERPRINT` to make it the default",
            ))
        },
    }
}

/// Finds a key by fingerprint, full or trailing key id.
///
/// A trailing match is allowed only for eight or more hexadecimal characters,
/// which is GnuPG's own rule for a key id: it is what people read off a
/// signature and type, and eight hex digits are still 32 bits of the full
/// fingerprint rather than an arbitrary prefix.
#[must_use]
pub fn find<'a>(
    keys: &'a [LocalKey],
    fingerprint: &str,
) -> Option<&'a LocalKey> {
    let wanted = fingerprint.trim();
    keys.iter()
        .find(|key| same_fingerprint(&key.fingerprint, wanted))
}

fn same_fingerprint(
    known: &str,
    wanted: &str,
) -> bool {
    if known.eq_ignore_ascii_case(wanted) {
        return true;
    }
    let hexadecimal =
        |text: &str| text.chars().all(|character| character.is_ascii_hexdigit()) && text.len() >= 8;
    hexadecimal(known)
        && hexadecimal(wanted)
        && known
            .to_ascii_uppercase()
            .ends_with(&wanted.to_ascii_uppercase())
}

/// Signs `statement` and returns an ASCII-armoured detached signature.
///
/// The bytes signed are exactly the ones passed in — the caller hands over the
/// statement text it has already checked against what it asked for — so there
/// is no re-rendering here that could drift from what the server will verify.
///
/// # Errors
///
/// A usage error when the signing tool is missing, `E_SIGNATURE` when it
/// refuses or produces something that is not a signature.
pub fn sign(
    key: &LocalKey,
    statement: &str,
) -> Result<String, CliError> {
    match (&key.kind, &key.origin) {
        | (KeyKind::OpenPgp, _) => sign_openpgp(&key.fingerprint, statement),
        | (KeyKind::Ssh, KeyOrigin::Ssh { private_path, .. }) => sign_ssh(private_path, statement),
        | (KeyKind::Ssh, KeyOrigin::Gpg) => Err(CliError::new(
            "this SSH key has no private key file to sign with",
        )),
    }
}

fn sign_openpgp(
    fingerprint: &str,
    statement: &str,
) -> Result<String, CliError> {
    let mut child = Command::new("gpg")
        .args([
            "--batch",
            "--yes",
            "--armor",
            "--detach-sign",
            "--local-user",
            fingerprint,
            "--output",
            "-",
        ])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|error| missing_tool("gpg", &error))?;
    // The statement is a few hundred bytes, far below a pipe buffer, so writing
    // it before reading stdout cannot deadlock.
    if let Some(mut stdin) = child.stdin.take() {
        stdin.write_all(statement.as_bytes()).map_err(|error| {
            CliError::new(format!("cannot pass the statement to `gpg`: {error}"))
        })?;
    }
    let output = child
        .wait_with_output()
        .map_err(|error| CliError::new(format!("`gpg --detach-sign` did not finish: {error}")))?;
    if !output.status.success() {
        return Err(CliError::new(format!(
            "`gpg --detach-sign` failed for key `{fingerprint}`: {}",
            first_line(&String::from_utf8_lossy(&output.stderr))
        ))
        .with_next_step(
            "check that the key has a signing-capable subkey with \
             `gpg --list-secret-keys --with-colons`",
        ));
    }
    let signature = String::from_utf8_lossy(&output.stdout).to_string();
    if !signature.contains("BEGIN PGP SIGNATURE") {
        return Err(CliError::new(
            "`gpg` returned no armoured signature; refusing to publish unsigned",
        ));
    }
    Ok(signature)
}

fn sign_ssh(
    private_path: &Path,
    statement: &str,
) -> Result<String, CliError> {
    // `ssh-keygen -Y sign` takes a file and writes `<file>.sig`; there is no
    // stdin/stdout mode. The temporary file holds only the statement, which is
    // public anyway — it is the server's own text — so nothing secret touches
    // the disk.
    let file = TempFile::new("statement")?;
    std::fs::write(file.path(), statement).map_err(|error| {
        CliError::new(format!("cannot write `{}`: {error}", file.path().display()))
    })?;
    let signature_path = PathBuf::from(format!("{}.sig", file.path().display()));
    let _signature = TempFile::claim(signature_path.clone());
    let output = Command::new("ssh-keygen")
        .arg("-Y")
        .arg("sign")
        .arg("-f")
        .arg(private_path)
        .arg("-n")
        .arg(SSH_NAMESPACE)
        .arg(file.path())
        .stdin(Stdio::null())
        .output()
        .map_err(|error| missing_tool("ssh-keygen", &error))?;
    if !output.status.success() {
        return Err(CliError::new(format!(
            "`ssh-keygen -Y sign` failed for `{}`: {}",
            private_path.display(),
            first_line(&String::from_utf8_lossy(&output.stderr))
        ))
        .with_next_step(
            "a passphrase-protected key must be unlocked first: load it with \
             `ssh-add` and keep the agent running",
        ));
    }
    let signature = std::fs::read_to_string(&signature_path).map_err(|error| {
        CliError::new(format!(
            "`ssh-keygen` wrote no signature to `{}`: {error}",
            signature_path.display()
        ))
    })?;
    if !signature.contains("BEGIN SSH SIGNATURE") {
        return Err(CliError::new(
            "`ssh-keygen` returned no SSHSIG block; refusing to publish unsigned",
        ));
    }
    Ok(signature)
}

/// The public half of a key, ASCII-armoured, as the registry stores it.
///
/// # Errors
///
/// A usage error when `gpg` is missing, `E_KEY` when the export is empty.
pub fn public_key(key: &LocalKey) -> Result<String, CliError> {
    match &key.origin {
        | KeyOrigin::Gpg => {
            let output = Command::new("gpg")
                .args(["--batch", "--armor", "--export", &key.fingerprint])
                .stdin(Stdio::null())
                .output()
                .map_err(|error| missing_tool("gpg", &error))?;
            if !output.status.success() {
                return Err(CliError::new(format!(
                    "`gpg --export {}` failed: {}",
                    key.fingerprint,
                    first_line(&String::from_utf8_lossy(&output.stderr))
                )));
            }
            let armored = String::from_utf8_lossy(&output.stdout).to_string();
            if !armored.contains("BEGIN PGP PUBLIC KEY BLOCK") {
                return Err(CliError::new(format!(
                    "`gpg` exported no public key for `{}`",
                    key.fingerprint
                )));
            }
            Ok(armored)
        },
        | KeyOrigin::Ssh { public_path, .. } => {
            let armored = std::fs::read_to_string(public_path).map_err(|error| {
                CliError::new(format!("cannot read `{}`: {error}", public_path.display()))
            })?;
            Ok(armored.trim().to_string())
        },
    }
}

/// Whether `gpg` can be run at all, for `status`.
#[must_use]
pub fn gpg_available() -> bool {
    Command::new("gpg")
        .arg("--version")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .is_ok_and(|status| status.success())
}

/// Whether `ssh-keygen` can be run at all, for `status`.
#[must_use]
pub fn ssh_keygen_available() -> bool {
    Command::new("ssh-keygen")
        .arg("-?")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .is_ok()
}

fn missing_tool(
    tool: &str,
    error: &std::io::Error,
) -> CliError {
    let next_step = match tool {
        | "gpg" => {
            "install GnuPG (`apt install gnupg`, `brew install gnupg`, \
                    or https://gnupg.org/download/) and run the command again"
        },
        | _ => {
            "install OpenSSH (`apt install openssh-client`) or publish with an \
                OpenPGP key instead"
        },
    };
    CliError::usage(format!("cannot run `{tool}`: {error}")).with_next_step(next_step)
}

fn is_missing_tool(error: &CliError) -> bool {
    error.exit == crate::error::Exit::Usage && error.message.starts_with("cannot run `")
}

fn first_line(text: &str) -> String {
    text.lines()
        .map(str::trim)
        .find(|line| !line.is_empty())
        .unwrap_or("no message")
        .to_string()
}

/// Decodes the `\xNN` escapes `gpg` uses inside colon records.
///
/// A user id may contain `:`, which would otherwise split a record in two and
/// shift every field after it.
fn unescape_colons(value: &str) -> String {
    let characters: Vec<char> = value.chars().collect();
    let mut out = String::with_capacity(value.len());
    let mut index = 0;
    while index < characters.len() {
        if characters.get(index) == Some(&'\\') && characters.get(index + 1) == Some(&'x') {
            let hex: String = characters
                .get(index + 2..index + 4)
                .map(|digits| digits.iter().collect())
                .unwrap_or_default();
            if hex.len() == 2
                && let Ok(byte) = u8::from_str_radix(&hex, 16)
            {
                out.push(char::from(byte));
                index += 4;
                continue;
            }
        }
        if let Some(character) = characters.get(index) {
            out.push(*character);
        }
        index += 1;
    }
    out
}

/// A file that deletes itself, so a failed signing leaves no statement behind.
struct TempFile {
    path: PathBuf,
}

impl TempFile {
    fn new(kind: &str) -> Result<Self, CliError> {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|elapsed| elapsed.as_nanos())
            .unwrap_or(0);
        let path = std::env::temp_dir().join(format!(
            "aevia-pkg-{}-{nanos}-{kind}.txt",
            std::process::id()
        ));
        Ok(Self { path })
    }

    fn claim(path: PathBuf) -> Self {
        Self { path }
    }

    fn path(&self) -> &Path {
        &self.path
    }
}

impl Drop for TempFile {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.path);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const COLONS: &str = "\
sec:u:255:22:6A1B2C3D4E5F6071:1700000000:1800000000::u:::scESC:::+:::23::0:
fpr:::::::::0123456789ABCDEF0123456789ABCDEF01234567:
grp:::::::::0000000000000000000000000000000000000000:
uid:u::::1700000000::HASH::Apich Example \\x3a maintainer <apich@example.test>::::::::::0:
ssb:u:255:22:1111111111111111:1700000000::::::e:::+:::23:
fpr:::::::::AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA:
sec:r:255:22:2222222222222222:1700000000::::::scESC:::+:::23::0:
fpr:::::::::BBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBB:
";

    #[test]
    fn the_fingerprint_after_sec_is_the_key() {
        let keys = parse_gpg_colons(COLONS);
        assert_eq!(keys.len(), 1, "the revoked key is skipped: {keys:?}");
        assert_eq!(
            keys[0].fingerprint,
            "0123456789ABCDEF0123456789ABCDEF01234567"
        );
        assert_eq!(keys[0].kind, KeyKind::OpenPgp);
        assert_eq!(
            keys[0].identity, "Apich Example : maintainer <apich@example.test>",
            "the `\\x3a` escape is decoded"
        );
    }

    #[test]
    fn a_subkey_fingerprint_is_not_mistaken_for_the_primary() {
        let keys = parse_gpg_colons(COLONS);
        assert!(
            !keys.iter().any(|key| key.fingerprint.starts_with("AAAA")),
            "{keys:?}"
        );
    }

    #[test]
    fn garbage_in_gpg_output_is_ignored_rather_than_panicking() {
        assert!(parse_gpg_colons("").is_empty());
        assert!(parse_gpg_colons("fpr\n\n:::\n").is_empty());
    }

    fn key(fingerprint: &str) -> LocalKey {
        LocalKey {
            kind: KeyKind::OpenPgp,
            fingerprint: fingerprint.to_string(),
            identity: String::new(),
            origin: KeyOrigin::Gpg,
            supported: true,
        }
    }

    #[test]
    fn a_key_id_matches_its_fingerprint() {
        let keys = vec![key("0123456789ABCDEF0123456789ABCDEF01234567")];
        assert!(find(&keys, "0123456789abcdef0123456789abcdef01234567").is_some());
        assert!(find(&keys, "01234567").is_some());
        assert!(
            find(&keys, "0123").is_none(),
            "four digits is too short to be a key id"
        );
        assert!(find(&keys, "FFFFFFFF").is_none());
    }

    #[test]
    fn one_key_is_chosen_automatically_and_many_are_not() {
        let single = vec![key("AAAA1111")];
        let config = Config::default();
        assert!(resolve(&single, None, &config).is_ok());

        let many = vec![key("AAAA1111"), key("BBBB2222")];
        let error = resolve(&many, None, &config).expect_err("ambiguous");
        assert_eq!(error.exit, crate::error::Exit::Usage);
        assert!(error.message.contains("BBBB2222"), "{}", error.message);
    }

    #[test]
    fn the_explicit_flag_beats_the_config() {
        let keys = vec![key("AAAA1111"), key("BBBB2222")];
        let config = Config {
            default_key: Some("BBBB2222".to_string()),
            ..Config::default()
        };
        assert_eq!(
            resolve(&keys, Some("AAAA1111"), &config)
                .expect("explicit")
                .fingerprint,
            "AAAA1111"
        );
        assert_eq!(
            resolve(&keys, None, &config).expect("config").fingerprint,
            "BBBB2222"
        );
    }

    #[test]
    fn a_default_key_that_is_gone_says_so() {
        let keys = vec![key("AAAA1111")];
        let config = Config {
            default_key: Some("CCCC3333".to_string()),
            ..Config::default()
        };
        let error = resolve(&keys, None, &config).expect_err("missing");
        assert!(
            error.message.contains("not on this machine"),
            "{}",
            error.message
        );
    }

    #[test]
    fn an_ssh_key_without_the_ed25519_algorithm_is_marked_unsupported() {
        let dir = std::env::temp_dir().join(format!("aevia-pkg-ssh-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("dir");
        let path = dir.join("id_rsa.pub");
        std::fs::write(
            &path,
            "ssh-rsa AAAAC3NzaC1lZDI1NTE5AAAAIOMqqnkVzrm0SdG6UOoqKLsabgH5C9okWi0dh2l9GKJl \
             apich@laptop\n",
        )
        .expect("write");
        let key = ssh_key_from(&path).expect("read").expect("key");
        assert!(!key.supported);
        assert!(key.unsupported_reason().is_some());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
