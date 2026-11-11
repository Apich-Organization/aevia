//! The canonical text a publisher signs.
//!
//! # Why a statement instead of an API token
//!
//! `cargo login` hands the registry a long-lived bearer token that can publish
//! anything the account owns, from anywhere, until someone remembers to revoke
//! it — and it sits in `~/.cargo/credentials` in plain text. The Aevia registry
//! has no such token. A publish is authorised by a signature over a short,
//! canonical statement that names exactly one package, one version and one
//! archive digest, and carries a server-issued nonce with a five-minute life:
//!
//! ```text
//! aevia-publish-v1
//! name: apich/units-extra
//! version: 0.2.0
//! checksum: sha256:9f2c…
//! nonce: 4d1a…
//! expires: 1794000300
//! ```
//!
//! Three properties follow from that, and each is a deliberate difference from
//! the Cargo model:
//!
//! * **A captured signature is worthless.** It names one archive and dies in
//!   five minutes; replaying it publishes nothing new.
//! * **Nothing long-lived is stored.** The private key never leaves the
//!   publisher's machine — it stays in `gpg-agent` or `ssh-agent`, which is
//!   where a key belongs.
//! * **The credential cannot be widened.** There is no scope to escalate: the
//!   statement is the whole authority, and it is checked against the archive
//!   the server actually received.
//!
//! The exact bytes matter. Both sides render the statement with
//! [`Statement::render`] and sign *those bytes*; the server re-renders from its
//! own record and compares before verifying, so a client cannot be tricked into
//! signing a statement whose meaning differs from its text.

use std::fmt::Write as _;

use crate::error::E_STATEMENT;
use crate::error::Error;
use crate::error::Result;
use crate::name::PackageId;

/// The first line of a publish statement.
pub const PUBLISH_TAG: &str = "aevia-publish-v1";

/// The first line of a yank statement.
pub const YANK_TAG: &str = "aevia-yank-v1";

/// How long a statement stays valid, in seconds.
///
/// Five minutes is long enough for a slow upload over a mobile link and short
/// enough that an intercepted statement is useless by the time anyone reads it.
pub const STATEMENT_LIFETIME_SECONDS: i64 = 300;

/// A signed request to publish one version of one package.
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct PublishStatement {
    /// The package the statement authorises.
    pub name: PackageId,
    /// The version being published.
    pub version: String,
    /// `sha256:<hex>` of the archive the signature covers.
    pub checksum: String,
    /// The server's single-use nonce.
    pub nonce: String,
    /// Unix seconds after which the statement is refused.
    pub expires: i64,
}

impl PublishStatement {
    /// Builds a statement for `name`/`version` with the archive's checksum.
    #[must_use]
    pub fn new(
        name: PackageId,
        version: impl Into<String>,
        checksum: impl Into<String>,
        nonce: impl Into<String>,
        expires: i64,
    ) -> Self {
        Self {
            name,
            version: version.into(),
            checksum: checksum.into(),
            nonce: nonce.into(),
            expires,
        }
    }

    /// The exact bytes to sign.
    #[must_use]
    pub fn render(&self) -> String {
        let mut out = String::new();
        out.push_str(PUBLISH_TAG);
        out.push('\n');
        let _ = writeln!(out, "name: {}", self.name);
        let _ = writeln!(out, "version: {}", self.version);
        let _ = writeln!(out, "checksum: {}", self.checksum);
        let _ = writeln!(out, "nonce: {}", self.nonce);
        let _ = writeln!(out, "expires: {}", self.expires);
        out
    }

    /// Parses a statement, refusing anything outside the canonical form.
    ///
    /// # Errors
    ///
    /// `E_STATEMENT` for a wrong tag, a missing or repeated field, a field out
    /// of order, a malformed version or checksum.
    pub fn parse(text: &str) -> Result<Self> {
        let fields = parse_fields(
            text,
            PUBLISH_TAG,
            &["name", "version", "checksum", "nonce", "expires"],
        )?;
        let name = fields[0].parse::<PackageId>()?;
        let version = parse_version(&fields[1])?;
        let checksum = parse_checksum(&fields[2])?;
        let nonce = parse_nonce(&fields[3])?;
        let expires = parse_expires(&fields[4])?;
        Ok(Self {
            name,
            version,
            checksum,
            nonce,
            expires,
        })
    }

    /// Whether the statement has run out of time at `now`.
    #[must_use]
    pub fn is_expired(
        &self,
        now: i64,
    ) -> bool {
        self.expires < now
    }
}

/// A signed request to yank or restore one version.
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct YankStatement {
    /// The package.
    pub name: PackageId,
    /// The version.
    pub version: String,
    /// `true` to yank, `false` to restore.
    pub yanked: bool,
    /// The server's single-use nonce.
    pub nonce: String,
    /// Unix seconds after which the statement is refused.
    pub expires: i64,
}

impl YankStatement {
    /// Builds a yank statement.
    #[must_use]
    pub fn new(
        name: PackageId,
        version: impl Into<String>,
        yanked: bool,
        nonce: impl Into<String>,
        expires: i64,
    ) -> Self {
        Self {
            name,
            version: version.into(),
            yanked,
            nonce: nonce.into(),
            expires,
        }
    }

    /// The exact bytes to sign.
    #[must_use]
    pub fn render(&self) -> String {
        let mut out = String::new();
        out.push_str(YANK_TAG);
        out.push('\n');
        let _ = writeln!(out, "name: {}", self.name);
        let _ = writeln!(out, "version: {}", self.version);
        let _ = writeln!(out, "yanked: {}", self.yanked);
        let _ = writeln!(out, "nonce: {}", self.nonce);
        let _ = writeln!(out, "expires: {}", self.expires);
        out
    }

    /// Parses a yank statement.
    ///
    /// # Errors
    ///
    /// `E_STATEMENT` under the same rules as [`PublishStatement::parse`].
    pub fn parse(text: &str) -> Result<Self> {
        let fields = parse_fields(
            text,
            YANK_TAG,
            &["name", "version", "yanked", "nonce", "expires"],
        )?;
        let name = fields[0].parse::<PackageId>()?;
        let version = parse_version(&fields[1])?;
        let yanked = match fields[2].as_str() {
            | "true" => true,
            | "false" => false,
            | other => {
                return Err(Error::new(
                    E_STATEMENT,
                    format!("`yanked` is `{other}`; it is `true` or `false`"),
                ));
            },
        };
        let nonce = parse_nonce(&fields[3])?;
        let expires = parse_expires(&fields[4])?;
        Ok(Self {
            name,
            version,
            yanked,
            nonce,
            expires,
        })
    }

    /// Whether the statement has run out of time at `now`.
    #[must_use]
    pub fn is_expired(
        &self,
        now: i64,
    ) -> bool {
        self.expires < now
    }
}

/// Either statement kind.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Statement {
    /// A publish.
    Publish(PublishStatement),
    /// A yank or restore.
    Yank(YankStatement),
}

impl Statement {
    /// Parses whichever statement the text claims to be.
    ///
    /// # Errors
    ///
    /// `E_STATEMENT` for an unknown tag or a malformed body.
    pub fn parse(text: &str) -> Result<Self> {
        let tag = text.lines().next().unwrap_or_default().trim_end();
        match tag {
            | PUBLISH_TAG => Ok(Self::Publish(PublishStatement::parse(text)?)),
            | YANK_TAG => Ok(Self::Yank(YankStatement::parse(text)?)),
            | other => {
                Err(Error::new(
                    E_STATEMENT,
                    format!(
                        "`{other}` is not a statement tag; expected `{PUBLISH_TAG}` or `{YANK_TAG}`"
                    ),
                ))
            },
        }
    }

    /// The exact bytes that were signed.
    #[must_use]
    pub fn render(&self) -> String {
        match self {
            | Self::Publish(statement) => statement.render(),
            | Self::Yank(statement) => statement.render(),
        }
    }

    /// The package the statement concerns.
    #[must_use]
    pub fn name(&self) -> &PackageId {
        match self {
            | Self::Publish(statement) => &statement.name,
            | Self::Yank(statement) => &statement.name,
        }
    }

    /// The version the statement concerns.
    #[must_use]
    pub fn version(&self) -> &str {
        match self {
            | Self::Publish(statement) => &statement.version,
            | Self::Yank(statement) => &statement.version,
        }
    }

    /// The nonce the server issued.
    #[must_use]
    pub fn nonce(&self) -> &str {
        match self {
            | Self::Publish(statement) => &statement.nonce,
            | Self::Yank(statement) => &statement.nonce,
        }
    }

    /// The expiry, in unix seconds.
    #[must_use]
    pub fn expires(&self) -> i64 {
        match self {
            | Self::Publish(statement) => statement.expires,
            | Self::Yank(statement) => statement.expires,
        }
    }

    /// `publish` or `yank`, for logs and the transparency ledger.
    #[must_use]
    pub fn kind(&self) -> &'static str {
        match self {
            | Self::Publish(_) => "publish",
            | Self::Yank(_) => "yank",
        }
    }

    /// Whether the statement has run out of time at `now`.
    #[must_use]
    pub fn is_expired(
        &self,
        now: i64,
    ) -> bool {
        self.expires() < now
    }
}

fn parse_fields(
    text: &str,
    tag: &str,
    expected: &[&str],
) -> Result<Vec<String>> {
    let mut lines = text.lines();
    match lines.next() {
        | Some(first) if first.trim_end() == tag => {},
        | Some(other) => {
            return Err(Error::new(
                E_STATEMENT,
                format!("`{}` is not `{tag}`", other.trim_end()),
            ));
        },
        | None => return Err(Error::new(E_STATEMENT, "the statement is empty")),
    }
    let mut values = Vec::new();
    for (index, expected_key) in expected.iter().enumerate() {
        let Some(line) = lines.next() else {
            return Err(Error::new(
                E_STATEMENT,
                format!("the statement ends before `{expected_key}`"),
            ));
        };
        let Some((key, value)) = line.split_once(':') else {
            return Err(Error::new(
                E_STATEMENT,
                format!("line {} is not `key: value`", index + 2),
            ));
        };
        if key.trim() != *expected_key {
            return Err(Error::new(
                E_STATEMENT,
                format!(
                    "line {} is `{}`; the canonical order is {}",
                    index + 2,
                    key.trim(),
                    expected.join(", ")
                ),
            ));
        }
        let value = value.trim();
        if value.is_empty() || value.contains('\r') {
            return Err(Error::new(
                E_STATEMENT,
                format!("`{expected_key}` is empty"),
            ));
        }
        values.push(value.to_string());
    }
    if let Some(extra) = lines.find(|line| !line.trim().is_empty()) {
        return Err(Error::new(
            E_STATEMENT,
            format!("`{extra}` follows the statement; nothing may follow `expires`"),
        ));
    }
    Ok(values)
}

fn parse_version(version: &str) -> Result<String> {
    let parsed = aevia_resolve::Version::parse(version).map_err(|error| {
        Error::new(
            E_STATEMENT,
            format!("`{version}` is not a semantic version: {}", error.reason),
        )
    })?;
    if !parsed.build.is_empty() {
        return Err(Error::new(
            E_STATEMENT,
            format!(
                "`{version}` carries build metadata; two versions that differ only in build                  metadata have the same precedence, so a registry cannot tell them apart —                  publish `major.minor.patch` or `major.minor.patch-pre-release`"
            ),
        ));
    }
    Ok(version.to_string())
}

fn parse_checksum(checksum: &str) -> Result<String> {
    let Some(hex_part) = checksum.strip_prefix("sha256:") else {
        return Err(Error::new(
            E_STATEMENT,
            format!("`{checksum}` is not a `sha256:` digest"),
        ));
    };
    if hex_part.len() != 64
        || !hex_part
            .chars()
            .all(|character| character.is_ascii_hexdigit())
    {
        return Err(Error::new(
            E_STATEMENT,
            format!("`{checksum}` is not 64 hexadecimal characters after `sha256:`"),
        ));
    }
    Ok(checksum.to_ascii_lowercase())
}

fn parse_nonce(nonce: &str) -> Result<String> {
    let valid = (16..=64).contains(&nonce.len())
        && nonce.chars().all(|character| character.is_ascii_hexdigit());
    if !valid {
        return Err(Error::new(
            E_STATEMENT,
            "the nonce is not 16–64 hexadecimal characters; take it from the challenge",
        ));
    }
    Ok(nonce.to_ascii_lowercase())
}

fn parse_expires(expires: &str) -> Result<i64> {
    expires
        .parse::<i64>()
        .map_err(|_| Error::new(E_STATEMENT, format!("`{expires}` is not unix seconds")))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn publish() -> PublishStatement {
        PublishStatement::new(
            "apich/units-extra".parse().expect("name"),
            "0.2.0",
            format!("sha256:{}", "ab".repeat(32)),
            "0123456789abcdef0123456789abcdef",
            1_794_000_300,
        )
    }

    #[test]
    fn a_publish_statement_round_trips_byte_for_byte() {
        let statement = publish();
        let text = statement.render();
        assert_eq!(
            text,
            "aevia-publish-v1\n\
             name: apich/units-extra\n\
             version: 0.2.0\n\
             checksum: sha256:abababababababababababababababababababababababababababababababab\n\
             nonce: 0123456789abcdef0123456789abcdef\n\
             expires: 1794000300\n"
        );
        assert_eq!(PublishStatement::parse(&text).expect("parses"), statement);
    }

    #[test]
    fn a_reordered_statement_is_refused() {
        let text = publish()
            .render()
            .replace("version: 0.2.0\nchecksum", "checksum");
        let error = PublishStatement::parse(&text).expect_err("refused");
        assert_eq!(error.code, E_STATEMENT);
    }

    #[test]
    fn a_trailing_field_is_refused() {
        let text = format!("{}extra: 1\n", publish().render());
        let error = PublishStatement::parse(&text).expect_err("refused");
        assert!(
            error.message.contains("nothing may follow"),
            "{}",
            error.message
        );
    }

    #[test]
    fn a_pre_release_version_is_accepted_and_build_metadata_is_refused() {
        let text = publish()
            .render()
            .replace("version: 0.2.0", "version: 0.2.0-rc.1");
        let statement = PublishStatement::parse(&text).expect("a pre-release is a version");
        assert_eq!(statement.version, "0.2.0-rc.1");
        let text = publish()
            .render()
            .replace("version: 0.2.0", "version: 0.2.0+b1");
        let error = PublishStatement::parse(&text).expect_err("refused");
        assert!(error.message.contains("build metadata"), "{}", error.message);
    }

    #[test]
    fn a_short_nonce_is_refused() {
        let text = publish()
            .render()
            .replace("nonce: 0123456789abcdef0123456789abcdef", "nonce: abc");
        let error = PublishStatement::parse(&text).expect_err("refused");
        assert!(error.message.contains("nonce"), "{}", error.message);
    }

    #[test]
    fn expiry_is_inclusive_of_the_deadline() {
        let statement = publish();
        assert!(!statement.is_expired(1_794_000_300));
        assert!(statement.is_expired(1_794_000_301));
    }

    #[test]
    fn either_kind_parses_through_the_enum() {
        let yank = YankStatement::new(
            "apich/units-extra".parse().expect("name"),
            "0.2.0",
            true,
            "0123456789abcdef",
            1_794_000_300,
        );
        let statement = Statement::parse(&yank.render()).expect("parses");
        assert_eq!(statement.kind(), "yank");
        assert_eq!(statement.version(), "0.2.0");
        assert_eq!(statement.render(), yank.render());
    }

    #[test]
    fn an_unknown_tag_is_refused() {
        let error = Statement::parse("aevia-delete-v1\n").expect_err("refused");
        assert!(
            error.message.contains("not a statement tag"),
            "{}",
            error.message
        );
    }
}
