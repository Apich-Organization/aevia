//! The registry's sparse-index text, as an [`Index`].
//!
//! `GET /api/v1/index/{owner}/{name}` returns one JSON object per line, one per
//! published version, oldest first (the format is specified in
//! `platform/docs/API.md`). This module parses that text without any
//! dependency: a small, strict JSON reader, the record type, and
//! [`SparseIndex`], which holds the records of many packages and answers the
//! resolver.
//!
//! Unknown fields are ignored, so a newer registry can add data without
//! breaking an older toolchain; a *missing* field the resolver needs
//! (`vers`, `cksum`, or a dependency's `package`/`req`) is an error naming the
//! line.

use std::collections::BTreeMap;
use std::fmt;

use crate::index::Dependency;
use crate::index::Index;
use crate::index::IndexError;
use crate::index::Summary;
use crate::names::default_import;
use crate::req::VersionReq;
use crate::version::Version;

/// A malformed index line.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SparseError {
    /// One-based line number within the text.
    pub line: usize,
    /// What is wrong.
    pub message: String,
}

impl fmt::Display for SparseError {
    fn fmt(
        &self,
        formatter: &mut fmt::Formatter<'_>,
    ) -> fmt::Result {
        write!(formatter, "index line {}: {}", self.line, self.message)
    }
}

impl std::error::Error for SparseError {}

/// One dependency of a published version.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SparseDependency {
    /// The registry key of the package (`owner/name`, lower-case name).
    pub package: String,
    /// The requirement.
    pub req: VersionReq,
    /// The import root the dependent uses, when it differs from
    /// [`default_import`] of the package.
    pub import: Option<String>,
}

impl SparseDependency {
    /// The import root this dependency is imported under.
    #[must_use]
    pub fn label(&self) -> String {
        self.import
            .clone()
            .unwrap_or_else(|| default_import(&self.package))
    }
}

/// The signature recorded with a version.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SparseSignature {
    /// `openpgp` or `ssh`.
    pub kind: String,
    /// The signing key's fingerprint.
    pub fingerprint: String,
    /// The armoured detached signature over the publish statement.
    pub signature: String,
}

/// One index line.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SparseRecord {
    /// `owner/name` as published.
    pub name: String,
    /// The version.
    pub version: Version,
    /// What it depends on.
    pub deps: Vec<SparseDependency>,
    /// `sha256:<hex>` of the archive.
    pub checksum: String,
    /// Whether the version is yanked.
    pub yanked: bool,
    /// The archive size, when the registry says.
    pub size: Option<u64>,
    /// The signature, when the registry includes it.
    pub signature: Option<SparseSignature>,
}

// ---------------------------------------------------------------------------
// A strict, minimal JSON reader.

#[derive(Debug, Clone, PartialEq)]
enum Json {
    Null,
    Bool(bool),
    Number(f64),
    String(String),
    Array(Vec<Json>),
    Object(Vec<(String, Json)>),
}

impl Json {
    fn get(
        &self,
        key: &str,
    ) -> Option<&Json> {
        match self {
            | Self::Object(fields) => {
                fields
                    .iter()
                    .rev()
                    .find(|(name, _)| name == key)
                    .map(|(_, value)| value)
            },
            | _ => None,
        }
    }

    fn as_str(&self) -> Option<&str> {
        match self {
            | Self::String(text) => Some(text),
            | _ => None,
        }
    }
}

struct Reader<'a> {
    bytes: &'a [u8],
    position: usize,
}

const MAX_DEPTH: usize = 32;

impl Reader<'_> {
    fn skip_space(&mut self) {
        while self
            .bytes
            .get(self.position)
            .is_some_and(|byte| matches!(byte, b' ' | b'\t' | b'\r' | b'\n'))
        {
            self.position += 1;
        }
    }

    fn peek(&self) -> Option<u8> {
        self.bytes.get(self.position).copied()
    }

    fn expect(
        &mut self,
        byte: u8,
    ) -> Result<(), String> {
        if self.peek() == Some(byte) {
            self.position += 1;
            Ok(())
        } else {
            Err(format!(
                "expected `{}` at offset {}",
                char::from(byte),
                self.position
            ))
        }
    }

    fn value(
        &mut self,
        depth: usize,
    ) -> Result<Json, String> {
        if depth > MAX_DEPTH {
            return Err("nested too deeply".to_string());
        }
        self.skip_space();
        match self.peek() {
            | None => Err("unexpected end of line".to_string()),
            | Some(b'{') => self.object(depth),
            | Some(b'[') => self.array(depth),
            | Some(b'"') => self.string().map(Json::String),
            | Some(b't') => self.literal("true", Json::Bool(true)),
            | Some(b'f') => self.literal("false", Json::Bool(false)),
            | Some(b'n') => self.literal("null", Json::Null),
            | Some(b'-' | b'0'..=b'9') => self.number(),
            | Some(other) => {
                Err(format!(
                    "unexpected `{}` at offset {}",
                    char::from(other),
                    self.position
                ))
            },
        }
    }

    fn literal(
        &mut self,
        word: &str,
        value: Json,
    ) -> Result<Json, String> {
        if self.bytes[self.position..].starts_with(word.as_bytes()) {
            self.position += word.len();
            Ok(value)
        } else {
            Err(format!("unexpected token at offset {}", self.position))
        }
    }

    fn number(&mut self) -> Result<Json, String> {
        let start = self.position;
        while self
            .peek()
            .is_some_and(|byte| matches!(byte, b'-' | b'+' | b'.' | b'e' | b'E' | b'0'..=b'9'))
        {
            self.position += 1;
        }
        let text = std::str::from_utf8(&self.bytes[start..self.position])
            .map_err(|_| "invalid number".to_string())?;
        text.parse::<f64>()
            .map(Json::Number)
            .map_err(|_| format!("`{text}` is not a number"))
    }

    fn string(&mut self) -> Result<String, String> {
        self.expect(b'"')?;
        let mut out: Vec<u8> = Vec::new();
        loop {
            let Some(byte) = self.peek() else {
                return Err("unterminated string".to_string());
            };
            self.position += 1;
            match byte {
                | b'"' => break,
                | b'\\' => {
                    let Some(escape) = self.peek() else {
                        return Err("unterminated escape".to_string());
                    };
                    self.position += 1;
                    match escape {
                        | b'"' => out.push(b'"'),
                        | b'\\' => out.push(b'\\'),
                        | b'/' => out.push(b'/'),
                        | b'b' => out.push(8),
                        | b'f' => out.push(12),
                        | b'n' => out.push(b'\n'),
                        | b'r' => out.push(b'\r'),
                        | b't' => out.push(b'\t'),
                        | b'u' => {
                            let first = self.hex4()?;
                            let code = if (0xD800..0xDC00).contains(&first) {
                                // A high surrogate must be followed by a low one.
                                if self.bytes[self.position..].starts_with(b"\\u") {
                                    self.position += 2;
                                    let second = self.hex4()?;
                                    if !(0xDC00..0xE000).contains(&second) {
                                        return Err("invalid surrogate pair".to_string());
                                    }
                                    0x10000 + ((first - 0xD800) << 10) + (second - 0xDC00)
                                } else {
                                    return Err("lone surrogate".to_string());
                                }
                            } else if (0xDC00..0xE000).contains(&first) {
                                return Err("lone surrogate".to_string());
                            } else {
                                first
                            };
                            let character = char::from_u32(code)
                                .ok_or_else(|| "invalid unicode escape".to_string())?;
                            let mut buffer = [0u8; 4];
                            out.extend_from_slice(character.encode_utf8(&mut buffer).as_bytes());
                        },
                        | other => {
                            return Err(format!("unknown escape `\\{}`", char::from(other)));
                        },
                    }
                },
                | 0..=0x1f => return Err("a raw control character in a string".to_string()),
                | other => out.push(other),
            }
        }
        String::from_utf8(out).map_err(|_| "a string is not valid UTF-8".to_string())
    }

    fn hex4(&mut self) -> Result<u32, String> {
        let end = self.position + 4;
        let slice = self
            .bytes
            .get(self.position..end)
            .ok_or_else(|| "short unicode escape".to_string())?;
        let text = std::str::from_utf8(slice).map_err(|_| "bad unicode escape".to_string())?;
        let value = u32::from_str_radix(text, 16).map_err(|_| "bad unicode escape".to_string())?;
        self.position = end;
        Ok(value)
    }

    fn array(
        &mut self,
        depth: usize,
    ) -> Result<Json, String> {
        self.expect(b'[')?;
        let mut items = Vec::new();
        self.skip_space();
        if self.peek() == Some(b']') {
            self.position += 1;
            return Ok(Json::Array(items));
        }
        loop {
            items.push(self.value(depth + 1)?);
            self.skip_space();
            match self.peek() {
                | Some(b',') => self.position += 1,
                | Some(b']') => {
                    self.position += 1;
                    return Ok(Json::Array(items));
                },
                | _ => return Err(format!("expected `,` or `]` at offset {}", self.position)),
            }
        }
    }

    fn object(
        &mut self,
        depth: usize,
    ) -> Result<Json, String> {
        self.expect(b'{')?;
        let mut fields = Vec::new();
        self.skip_space();
        if self.peek() == Some(b'}') {
            self.position += 1;
            return Ok(Json::Object(fields));
        }
        loop {
            self.skip_space();
            let key = self.string()?;
            self.skip_space();
            self.expect(b':')?;
            let value = self.value(depth + 1)?;
            fields.push((key, value));
            self.skip_space();
            match self.peek() {
                | Some(b',') => self.position += 1,
                | Some(b'}') => {
                    self.position += 1;
                    return Ok(Json::Object(fields));
                },
                | _ => return Err(format!("expected `,` or `}}` at offset {}", self.position)),
            }
        }
    }
}

fn parse_json(line: &str) -> Result<Json, String> {
    let mut reader = Reader {
        bytes: line.as_bytes(),
        position: 0,
    };
    let value = reader.value(0)?;
    reader.skip_space();
    if reader.position != reader.bytes.len() {
        return Err(format!("trailing text at offset {}", reader.position));
    }
    Ok(value)
}

// ---------------------------------------------------------------------------
// Records

/// Parses one index line.
///
/// # Errors
///
/// [`SparseError`] (with `line` 1) for malformed JSON or a missing or invalid
/// required field.
pub fn parse_line(line: &str) -> Result<SparseRecord, SparseError> {
    record_from(line, 1)
}

fn record_from(
    line: &str,
    number: usize,
) -> Result<SparseRecord, SparseError> {
    let fail = |message: String| {
        SparseError {
            line: number,
            message,
        }
    };
    let json = parse_json(line).map_err(&fail)?;
    if !matches!(json, Json::Object(_)) {
        return Err(fail("a record is a JSON object".to_string()));
    }
    let text = |key: &str| -> Result<String, SparseError> {
        json.get(key)
            .and_then(Json::as_str)
            .map(str::to_string)
            .ok_or_else(|| fail(format!("`{key}` is missing or not a string")))
    };
    let name = text("name")?;
    let vers = text("vers")?;
    let version = Version::parse(&vers).map_err(|error| fail(format!("`vers`: {error}")))?;
    let checksum = text("cksum")?;
    let mut deps = Vec::new();
    match json.get("deps") {
        | None | Some(Json::Null) => {},
        | Some(Json::Array(items)) => {
            for item in items {
                let package = item
                    .get("package")
                    .and_then(Json::as_str)
                    .ok_or_else(|| fail("a dependency has no `package`".to_string()))?;
                let req = item
                    .get("req")
                    .and_then(Json::as_str)
                    .ok_or_else(|| fail(format!("dependency `{package}` has no `req`")))?;
                let req = VersionReq::parse(req)
                    .map_err(|error| fail(format!("dependency `{package}`: {error}")))?;
                let import = item
                    .get("import")
                    .and_then(Json::as_str)
                    .map(str::to_string);
                deps.push(SparseDependency {
                    package: package.to_ascii_lowercase(),
                    req,
                    import,
                });
            }
        },
        | Some(_) => return Err(fail("`deps` is not an array".to_string())),
    }
    let yanked = matches!(json.get("yanked"), Some(Json::Bool(true)));
    let size = match json.get("size") {
        | Some(Json::Number(number)) if *number >= 0.0 && number.fract() == 0.0 => {
            Some(*number as u64)
        },
        | _ => None,
    };
    let signature = json.get("signature").and_then(|signature| {
        Some(SparseSignature {
            kind: signature.get("kind")?.as_str()?.to_string(),
            fingerprint: signature.get("fingerprint")?.as_str()?.to_string(),
            signature: signature.get("signature")?.as_str()?.to_string(),
        })
    });
    Ok(SparseRecord {
        name,
        version,
        deps,
        checksum,
        yanked,
        size,
        signature,
    })
}

/// Parses a whole index file. A later record for the same version replaces an
/// earlier one (a yank rewrites only the flag of the record it applies to, but
/// a client must not break if a mirror appends instead). Blank lines are
/// skipped. The result is sorted by version.
///
/// # Errors
///
/// The first malformed line.
pub fn parse_text(text: &str) -> Result<Vec<SparseRecord>, SparseError> {
    let mut records: Vec<SparseRecord> = Vec::new();
    for (position, line) in text.lines().enumerate() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let record = record_from(line, position + 1)?;
        match records
            .iter_mut()
            .find(|held| held.version == record.version)
        {
            | Some(held) => *held = record,
            | None => records.push(record),
        }
    }
    records.sort_by(|left, right| left.version.cmp(&right.version));
    Ok(records)
}

/// Many packages' index records, answering the resolver.
#[derive(Clone, Debug, Default)]
pub struct SparseIndex {
    packages: BTreeMap<String, Vec<SparseRecord>>,
}

impl SparseIndex {
    /// An empty index.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Adds (or replaces) one package from its index text.
    ///
    /// # Errors
    ///
    /// [`SparseError`] for a malformed line.
    pub fn insert_text(
        &mut self,
        package: &str,
        text: &str,
    ) -> Result<(), SparseError> {
        let records = parse_text(text)?;
        self.packages.insert(package.to_ascii_lowercase(), records);
        Ok(())
    }

    /// Adds (or replaces) one package's parsed records.
    pub fn insert_records(
        &mut self,
        package: &str,
        records: Vec<SparseRecord>,
    ) {
        self.packages.insert(package.to_ascii_lowercase(), records);
    }

    /// The record of one version.
    #[must_use]
    pub fn record(
        &self,
        package: &str,
        version: &Version,
    ) -> Option<&SparseRecord> {
        self.packages
            .get(&package.to_ascii_lowercase())?
            .iter()
            .find(|record| &record.version == version)
    }

    /// All records of a package, ascending.
    #[must_use]
    pub fn records(
        &self,
        package: &str,
    ) -> Option<&[SparseRecord]> {
        self.packages
            .get(&package.to_ascii_lowercase())
            .map(Vec::as_slice)
    }

    /// The package keys held, sorted.
    pub fn packages(&self) -> impl Iterator<Item = &str> {
        self.packages.keys().map(String::as_str)
    }
}

impl Index for SparseIndex {
    fn summaries(
        &self,
        package: &str,
    ) -> Result<Vec<Summary>, IndexError> {
        let records = self
            .packages
            .get(&package.to_ascii_lowercase())
            .ok_or_else(|| IndexError::missing(package))?;
        Ok(records
            .iter()
            .map(|record| {
                Summary {
                    version: record.version.clone(),
                    yanked: record.yanked,
                    dependencies: record
                        .deps
                        .iter()
                        .map(|dep| {
                            Dependency {
                                label: dep.label(),
                                package: dep.package.clone(),
                                req: dep.req.clone(),
                            }
                        })
                        .collect(),
                }
            })
            .collect())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::resolve::Options;
    use crate::resolve::Root;
    use crate::resolve::resolve;

    const SIGNED: &str = r#"{"name":"apich/units-extra","vers":"0.2.0","deps":[],"cksum":"sha256:7cb292ee","yanked":false,"published_at":"2026-11-08T11:50:50Z","docs":true,"size":10240,"signature":{"kind":"openpgp","fingerprint":"7AECB528","signature":"-----BEGIN PGP SIGNATURE-----\n\nabc\n-----END PGP SIGNATURE-----\n"}}"#;

    #[test]
    fn the_documented_record_parses() {
        let record = parse_line(SIGNED).expect("parses");
        assert_eq!(record.name, "apich/units-extra");
        assert_eq!(record.version, Version::new(0, 2, 0));
        assert_eq!(record.checksum, "sha256:7cb292ee");
        assert!(!record.yanked);
        assert_eq!(record.size, Some(10240));
        let signature = record.signature.expect("signature");
        assert_eq!(signature.kind, "openpgp");
        assert!(signature.signature.starts_with("-----BEGIN PGP"));
        assert!(signature.signature.contains("\nabc\n"));
    }

    #[test]
    fn dependencies_carry_req_and_an_optional_import() {
        let line = r#"{"name":"o/a","vers":"1.0.0-rc.1","cksum":"sha256:00","deps":[{"package":"O/Units-Extra","req":">=1.2, <2"},{"package":"o/plot","req":"^0.3","import":"Charts"}],"future_field":{"x":[1,2,{"y":null}]}}"#;
        let record = parse_line(line).expect("parses");
        assert_eq!(record.deps.len(), 2);
        assert_eq!(record.deps[0].package, "o/units-extra");
        assert_eq!(record.deps[0].label(), "UnitsExtra");
        assert_eq!(record.deps[0].req.to_string(), ">=1.2, <2");
        assert_eq!(record.deps[1].label(), "Charts");
        assert!(record.version.is_prerelease());
    }

    #[test]
    fn malformed_lines_name_the_line_and_the_field() {
        for (text, needle) in [
            ("not json", "unexpected"),
            ("[]", "JSON object"),
            (r#"{"vers":"1.0.0","cksum":"x"}"#, "`name`"),
            (r#"{"name":"o/a","vers":"1.0","cksum":"x"}"#, "`vers`"),
            (r#"{"name":"o/a","vers":"1.0.0"}"#, "`cksum`"),
            (
                r#"{"name":"o/a","vers":"1.0.0","cksum":"x","deps":[{"req":"1"}]}"#,
                "no `package`",
            ),
            (
                r#"{"name":"o/a","vers":"1.0.0","cksum":"x","deps":[{"package":"o/b","req":"^x"}]}"#,
                "`o/b`",
            ),
            (
                r#"{"name":"o/a","vers":"1.0.0","cksum":"x","deps":3}"#,
                "not an array",
            ),
            (r#"{"name":"o/a"} trailing"#, "trailing"),
            (r#"{"name":"o/a\ud800"}"#, "surrogate"),
        ] {
            let error = parse_text(&format!("\n{text}")).expect_err(text);
            assert_eq!(error.line, 2, "{text}");
            assert!(
                error.message.contains(needle),
                "`{text}`: {}",
                error.message
            );
        }
    }

    #[test]
    fn json_strings_decode_escapes_and_surrogate_pairs() {
        let json = parse_json(r#"{"a":"xé😀\"\\\/\n"}"#).expect("parses");
        assert_eq!(
            json.get("a").and_then(Json::as_str),
            Some("x\u{e9}\u{1f600}\"\\/\n")
        );
    }

    #[test]
    fn json_nesting_is_bounded() {
        let deep = format!("{}1{}", "[".repeat(100), "]".repeat(100));
        assert!(parse_json(&deep).is_err());
    }

    #[test]
    fn a_later_record_replaces_an_earlier_one_and_the_text_is_sorted() {
        let text = "{\"name\":\"o/a\",\"vers\":\"1.1.0\",\"cksum\":\"c2\"}\n\n\
                    {\"name\":\"o/a\",\"vers\":\"1.0.0\",\"cksum\":\"c1\"}\n\
                    {\"name\":\"o/a\",\"vers\":\"1.1.0\",\"cksum\":\"c2\",\"yanked\":true}\n";
        let records = parse_text(text).expect("parses");
        assert_eq!(records.len(), 2);
        assert_eq!(records[0].version, Version::new(1, 0, 0));
        assert!(records[1].yanked);
    }

    #[test]
    fn the_sparse_index_drives_the_resolver_end_to_end() {
        let mut index = SparseIndex::new();
        index
            .insert_text(
                "o/plot",
                "{\"name\":\"o/plot\",\"vers\":\"1.0.0\",\"cksum\":\"c\",\"deps\":[{\"package\":\"o/units\",\"req\":\"^1\"}]}\n\
                 {\"name\":\"o/plot\",\"vers\":\"1.1.0\",\"cksum\":\"c\",\"deps\":[{\"package\":\"o/units\",\"req\":\"^1.2\"}]}\n",
            )
            .expect("plot");
        index
            .insert_text(
                "o/units",
                "{\"name\":\"o/units\",\"vers\":\"1.0.0\",\"cksum\":\"c\"}\n\
                 {\"name\":\"o/units\",\"vers\":\"1.2.0\",\"cksum\":\"c\"}\n\
                 {\"name\":\"o/units\",\"vers\":\"1.3.0\",\"cksum\":\"c\",\"yanked\":true}\n",
            )
            .expect("units");
        let root = Root {
            id: "app".to_string(),
            dependencies: vec![Dependency::new("o/plot", VersionReq::parse("^1").unwrap())],
        };
        let resolution = resolve(&[root], &index, &Options::default()).expect("resolves");
        let names: Vec<String> = resolution
            .packages
            .iter()
            .map(ToString::to_string)
            .collect();
        assert_eq!(names, ["o/plot 1.1.0", "o/units 1.2.0"]);
        assert!(index.record("O/Units", &Version::new(1, 2, 0)).is_some());
    }
}
