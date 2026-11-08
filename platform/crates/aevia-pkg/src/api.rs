//! The registry's HTTP API, as this client speaks it.
//!
//! Everything under `/api/v1` is described here once, so a command never
//! builds a URL by hand and never decides on its own what a non-2xx response
//! means.
//!
//! # Two rules this module enforces
//!
//! * **A failure is printed as `code: message`.** The server answers errors
//!   with `{"error":{"code":"E_SIGNATURE","message":"…"}}`; the code is the
//!   stable half and is kept in front of the sentence so a publisher can search
//!   for it. A non-JSON failure (a proxy's HTML page, a 502) is reported with
//!   its status and the first line of the body, never swallowed.
//! * **Nothing is trusted before it is checked.** Responses that carry a
//!   statement, a checksum or a signature are returned verbatim; the commands
//!   above compare them against what they asked for.
//!
//! The publish body is assembled by hand rather than with `reqwest`'s
//! `multipart` feature: two parts with fixed names do not justify a feature
//! flag, and writing the body makes the exact bytes on the wire visible here,
//! which is what a protocol with no API token deserves.

use std::time::Duration;

use aevia_registry_core::index::IndexRecord;
use aevia_registry_core::vm::CrateList;
use aevia_registry_core::vm::CratePage;

use crate::error::CliError;

/// How long a single request may take, including a 16 MiB upload or download.
const REQUEST_TIMEOUT: Duration = Duration::from_secs(300);

/// A challenge the server issued for a statement to sign.
#[derive(Clone, Debug, serde::Deserialize)]
pub struct Challenge {
    /// The single-use nonce, echoed inside the statement.
    #[serde(default)]
    pub nonce: String,
    /// The exact bytes to sign, rendered by the server.
    pub statement: String,
    /// When the statement dies, RFC 3339.
    #[serde(default)]
    pub expires_at: String,
}

/// What the registry answers when a publish is accepted.
#[derive(Clone, Debug, serde::Deserialize)]
pub struct PublishAccepted {
    /// `owner/name`.
    pub name: String,
    /// The version that was stored.
    pub version: String,
    /// `sha256:<hex>` of the archive it stored.
    pub checksum: String,
    /// Where the documentation will appear, when there is any.
    #[serde(default)]
    pub docs_url: Option<String>,
    /// The transparency-log entry the publish appended.
    #[serde(default)]
    pub log: Option<LogPosition>,
}

/// A position in the transparency log.
#[derive(Clone, Debug, serde::Deserialize)]
pub struct LogPosition {
    /// The entry's index.
    pub seq: i64,
    /// The entry's hash.
    pub hash: String,
}

/// A publisher identity, as the server reports it.
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Publisher {
    /// The GitHub login or ORCID iD.
    pub handle: String,
    /// `github` or `orcid`.
    #[serde(default)]
    pub kind: String,
}

impl Publisher {
    /// `apich (GitHub)`, for a terminal line.
    #[must_use]
    pub fn label(&self) -> String {
        if self.kind.is_empty() {
            self.handle.clone()
        } else {
            format!("{} ({})", self.handle, self.kind)
        }
    }
}

/// A device-authorisation code the publisher approves in a browser.
#[derive(Clone, Debug, serde::Deserialize)]
pub struct DeviceStart {
    /// The secret the client polls with; never shown to the publisher.
    pub device_code: String,
    /// The short code the publisher types, `ABCD-EFGH`.
    pub user_code: String,
    /// Where to type it.
    pub verification_uri: String,
    /// When the code dies, RFC 3339.
    #[serde(default)]
    pub expires_at: String,
    /// How often to poll, in seconds.
    #[serde(default)]
    pub interval: u64,
}

impl DeviceStart {
    /// The polling interval, never below the server's floor of five seconds.
    #[must_use]
    pub fn interval(&self) -> Duration {
        Duration::from_secs(self.interval.max(5))
    }
}

/// The state of a device authorisation.
#[derive(Clone, Debug, serde::Deserialize)]
pub struct DevicePoll {
    /// `pending`, `approved`, `denied` or `expired`.
    pub status: String,
    /// Who the key was bound to, once approved.
    #[serde(default)]
    pub publisher: Option<Publisher>,
}

/// Whether a key is bound to an account.
#[derive(Clone, Debug, serde::Deserialize)]
pub struct KeyStatus {
    /// `true` when the key speaks for a publisher.
    pub registered: bool,
    /// The account it speaks for.
    #[serde(default)]
    pub publisher: Option<Publisher>,
}

/// An archive and the provenance the server attached to it.
#[derive(Clone, Debug)]
pub struct Download {
    /// The gzip bytes.
    pub bytes: Vec<u8>,
    /// `X-Aevia-Checksum`, when the server sent it.
    pub checksum: Option<String>,
    /// `X-Aevia-Signature-Fingerprint`, when the server sent it.
    pub fingerprint: Option<String>,
}

/// A client for one registry.
#[derive(Clone, Debug)]
pub struct Client {
    base: String,
    http: reqwest::Client,
}

impl Client {
    /// Builds a client for `base`, the registry root such as
    /// `https://aevia.apich.org`.
    ///
    /// # Errors
    ///
    /// A usage error when the URL cannot be parsed or the TLS backend cannot be
    /// initialised, both of which are configuration mistakes rather than
    /// transient failures.
    pub fn new(base: &str) -> Result<Self, CliError> {
        let base = base.trim().trim_end_matches('/').to_string();
        if !base.starts_with("http://") && !base.starts_with("https://") {
            return Err(CliError::usage(format!(
                "`{base}` is not an HTTP URL; the registry is written as `https://aevia.apich.org`"
            )));
        }
        let http = reqwest::Client::builder()
            .user_agent(concat!("aevia-pkg/", env!("CARGO_PKG_VERSION")))
            .timeout(REQUEST_TIMEOUT)
            .build()
            .map_err(|error| {
                CliError::new(format!("cannot initialise the HTTP client: {error}"))
            })?;
        Ok(Self { base, http })
    }

    /// The registry root, without a trailing slash.
    #[must_use]
    pub fn base(&self) -> &str {
        &self.base
    }

    fn api(
        &self,
        path: &str,
    ) -> String {
        let url = format!("{}/api/v1{path}", self.base);
        // The only trace this client emits: which endpoint was called. It is
        // off unless `AEVIA_PKG_LOG` asks for it, and it is what a bug report
        // about "the wrong thing was published" needs first.
        tracing::debug!("requesting {url}");
        url
    }

    /// Asks for a statement authorising a publish.
    ///
    /// # Errors
    ///
    /// `E_NETWORK` when the registry cannot be reached, otherwise the server's
    /// own error.
    pub async fn publish_challenge(
        &self,
        name: &str,
        version: &str,
        checksum: &str,
    ) -> Result<Challenge, CliError> {
        let body = serde_json::json!({
            "name": name,
            "version": version,
            "checksum": checksum,
        });
        self.post_json("/publish/challenge", &body).await
    }

    /// Asks for a statement authorising a yank or a restore.
    ///
    /// # Errors
    ///
    /// As [`Client::publish_challenge`].
    pub async fn yank_challenge(
        &self,
        name: &str,
        version: &str,
        yanked: bool,
    ) -> Result<Challenge, CliError> {
        let body = serde_json::json!({
            "name": name,
            "version": version,
            "yanked": yanked,
        });
        self.post_json("/yank/challenge", &body).await
    }

    /// Uploads a signed envelope and the archive it covers.
    ///
    /// # Errors
    ///
    /// As [`Client::publish_challenge`], plus `E_UPLOAD` when the response is
    /// not the acceptance document this endpoint promises.
    pub async fn publish(
        &self,
        envelope: &str,
        archive: &[u8],
    ) -> Result<PublishAccepted, CliError> {
        let boundary = multipart_boundary(archive);
        let body = multipart_body(&boundary, envelope, archive);
        let response = self
            .http
            .post(self.api("/publish"))
            .header(
                reqwest::header::CONTENT_TYPE,
                format!("multipart/form-data; boundary={boundary}"),
            )
            .body(body)
            .send()
            .await;
        let response = self.check(response).await?;
        response.json::<PublishAccepted>().await.map_err(|error| {
            CliError::new(format!(
                "the registry accepted the publish but its answer did not parse: {error}"
            ))
            .with_next_step(
                "the archive may well be stored; run `aevia-pkg info` before publishing again",
            )
        })
    }

    /// Sends a signed yank statement.
    ///
    /// # Errors
    ///
    /// As [`Client::publish_challenge`].
    pub async fn yank(
        &self,
        body: &serde_json::Value,
    ) -> Result<(), CliError> {
        let response = self.http.post(self.api("/yank")).json(body).send().await;
        self.check(response).await?;
        Ok(())
    }

    /// Runs a search.
    ///
    /// # Errors
    ///
    /// As [`Client::publish_challenge`].
    pub async fn search(
        &self,
        query: &str,
        sort: &str,
        page: u32,
    ) -> Result<CrateList, CliError> {
        let response = self
            .http
            .get(self.api("/search"))
            .query(&[("q", query), ("sort", sort), ("page", &page.to_string())])
            .send()
            .await;
        let response = self.check(response).await?;
        response
            .json::<CrateList>()
            .await
            .map_err(|error| CliError::new(format!("the search answer did not parse: {error}")))
    }

    /// Fetches one package's page.
    ///
    /// # Errors
    ///
    /// `E_NOT_FOUND` when the registry does not know the package, otherwise as
    /// [`Client::publish_challenge`].
    pub async fn crate_page(
        &self,
        owner: &str,
        name: &str,
    ) -> Result<CratePage, CliError> {
        let response = self
            .http
            .get(self.api(&format!("/crates/{owner}/{name}")))
            .send()
            .await;
        let response = self.check(response).await?;
        response
            .json::<CratePage>()
            .await
            .map_err(|error| CliError::new(format!("the package page did not parse: {error}")))
    }

    /// Downloads one version's archive.
    ///
    /// The bytes are returned untouched, with the checksum and fingerprint
    /// headers: comparing them against the signature is the caller's job,
    /// because only the caller knows which version it asked for.
    ///
    /// # Errors
    ///
    /// As [`Client::crate_page`].
    pub async fn download(
        &self,
        owner: &str,
        name: &str,
        version: &str,
    ) -> Result<Download, CliError> {
        let response = self
            .http
            .get(self.api(&format!("/download/{owner}/{name}/{version}")))
            .send()
            .await;
        let response = self.check(response).await?;
        let header = |name: &str| {
            response
                .headers()
                .get(name)
                .and_then(|value| value.to_str().ok())
                .map(|value| value.trim().to_string())
        };
        let checksum = header("x-aevia-checksum");
        let fingerprint = header("x-aevia-signature-fingerprint");
        let bytes = response.bytes().await.map_err(|error| {
            CliError::new(format!("the download did not complete: {error}"))
                .with_next_step("run the command again; a partial archive is never written")
        })?;
        Ok(Download {
            bytes: bytes.to_vec(),
            checksum,
            fingerprint,
        })
    }

    /// Fetches the signature that authorised one version.
    ///
    /// # Errors
    ///
    /// As [`Client::crate_page`].
    pub async fn signature(
        &self,
        owner: &str,
        name: &str,
        version: &str,
    ) -> Result<String, CliError> {
        let response = self
            .http
            .get(self.api(&format!("/download/{owner}/{name}/{version}/signature")))
            .send()
            .await;
        let response = self.check(response).await?;
        response
            .text()
            .await
            .map_err(|error| CliError::new(format!("the signature could not be read: {error}")))
    }

    /// Fetches a version's index records, oldest first.
    ///
    /// # Errors
    ///
    /// As [`Client::crate_page`], plus `E_JSON` for a line that is not a
    /// record.
    pub async fn index(
        &self,
        owner: &str,
        name: &str,
    ) -> Result<Vec<IndexRecord>, CliError> {
        let response = self
            .http
            .get(self.api(&format!("/index/{owner}/{name}")))
            .send()
            .await;
        let response = self.check(response).await?;
        let text = response
            .text()
            .await
            .map_err(|error| CliError::new(format!("the index could not be read: {error}")))?;
        let mut records = Vec::new();
        for line in text.lines().filter(|line| !line.trim().is_empty()) {
            records.push(IndexRecord::from_line(line).map_err(CliError::from)?);
        }
        Ok(records)
    }

    /// Starts a device authorisation for a public key.
    ///
    /// # Errors
    ///
    /// As [`Client::publish_challenge`].
    pub async fn device_start(
        &self,
        kind: &str,
        public_key: &str,
        label: &str,
    ) -> Result<DeviceStart, CliError> {
        let body = serde_json::json!({
            "kind": kind,
            "public_key": public_key,
            "label": label,
        });
        self.post_json("/auth/device", &body).await
    }

    /// Asks the registry whether the publisher approved a device code.
    ///
    /// # Errors
    ///
    /// As [`Client::publish_challenge`].
    pub async fn device_poll(
        &self,
        device_code: &str,
    ) -> Result<DevicePoll, CliError> {
        let response = self
            .http
            .get(self.api(&format!("/auth/device/{device_code}")))
            .send()
            .await;
        let response = self.check(response).await?;
        response
            .json::<DevicePoll>()
            .await
            .map_err(|error| CliError::new(format!("the device status did not parse: {error}")))
    }

    /// Asks whether a fingerprint is bound to a publisher.
    ///
    /// # Errors
    ///
    /// As [`Client::publish_challenge`].
    pub async fn key_status(
        &self,
        fingerprint: &str,
    ) -> Result<KeyStatus, CliError> {
        let response = self
            .http
            .get(self.api("/keys"))
            .query(&[("fingerprint", fingerprint)])
            .send()
            .await;
        let response = self.check(response).await?;
        response
            .json::<KeyStatus>()
            .await
            .map_err(|error| CliError::new(format!("the key status did not parse: {error}")))
    }

    /// Fetches a key's public half, for verifying a download.
    ///
    /// The status endpoint answers `registered: true` and nothing else, so the
    /// key itself comes from here; a verifier that trusted the download's own
    /// header instead would be checking the archive against a key the archive
    /// chose.
    ///
    /// # Errors
    ///
    /// As [`Client::publish_challenge`].
    pub async fn public_key(
        &self,
        fingerprint: &str,
    ) -> Result<String, CliError> {
        let response = self
            .http
            .get(self.api("/keys/public"))
            .query(&[("fingerprint", fingerprint)])
            .send()
            .await;
        let response = self.check(response).await?;
        response
            .text()
            .await
            .map_err(|error| CliError::new(format!("the public key could not be read: {error}")))
    }

    async fn post_json<T: serde::de::DeserializeOwned>(
        &self,
        path: &str,
        body: &serde_json::Value,
    ) -> Result<T, CliError> {
        let response = self.http.post(self.api(path)).json(body).send().await;
        let response = self.check(response).await?;
        response.json::<T>().await.map_err(|error| {
            CliError::new(format!(
                "the registry's answer to `{path}` did not parse: {error}"
            ))
        })
    }

    /// Turns a transport failure or a non-2xx answer into a [`CliError`].
    async fn check(
        &self,
        response: Result<reqwest::Response, reqwest::Error>,
    ) -> Result<reqwest::Response, CliError> {
        let response = response.map_err(|error| {
            CliError::new(format!("cannot reach `{}`: {error}", self.base)).with_next_step(
                "check the URL with `--registry`, and that the network allows the connection",
            )
        })?;
        let status = response.status();
        if status.is_success() {
            return Ok(response);
        }
        let body = response.text().await.unwrap_or_default();
        Err(server_error(status.as_u16(), &body))
    }
}

/// The `code: message` a server error document carries.
#[derive(serde::Deserialize)]
struct ErrorEnvelope {
    error: ErrorBody,
}

#[derive(serde::Deserialize)]
struct ErrorBody {
    code: String,
    #[serde(default)]
    message: String,
}

/// Reads the registry's error document, falling back to the raw body.
///
/// The fallback matters: a reverse proxy in front of the registry answers with
/// HTML, and "the registry answered 502" plus a line of that HTML is far more
/// actionable than a JSON parse error.
fn server_error(
    status: u16,
    body: &str,
) -> CliError {
    if let Ok(envelope) = serde_json::from_str::<ErrorEnvelope>(body.trim()) {
        if !envelope.error.message.is_empty() {
            return CliError::new(format!(
                "{}: {}",
                envelope.error.code, envelope.error.message
            ));
        }
        return CliError::new(envelope.error.code);
    }
    let snippet: String = body
        .lines()
        .map(str::trim)
        .find(|line| !line.is_empty())
        .unwrap_or("no body")
        .chars()
        .take(200)
        .collect();
    CliError::new(format!("the registry answered {status}: {snippet}"))
}

/// A boundary that does not occur inside the archive.
///
/// A boundary that appeared in the payload would split the upload in the wrong
/// place; the chance is vanishingly small, but checking costs one scan and
/// removes the question.
fn multipart_boundary(archive: &[u8]) -> String {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|elapsed| elapsed.as_nanos())
        .unwrap_or(0);
    let mut boundary = format!("aevia-pkg-{}-{nanos}", std::process::id());
    while contains(archive, boundary.as_bytes()) {
        boundary.push('x');
    }
    boundary
}

fn contains(
    haystack: &[u8],
    needle: &[u8],
) -> bool {
    if needle.is_empty() || haystack.len() < needle.len() {
        return false;
    }
    haystack
        .windows(needle.len())
        .any(|window| window == needle)
}

/// Assembles the two-part `multipart/form-data` body.
fn multipart_body(
    boundary: &str,
    envelope: &str,
    archive: &[u8],
) -> Vec<u8> {
    let mut body = Vec::with_capacity(archive.len() + envelope.len() + 512);
    body.extend_from_slice(
        format!(
            "--{boundary}\r\n\
             Content-Disposition: form-data; name=\"envelope\"\r\n\
             Content-Type: application/json\r\n\r\n"
        )
        .as_bytes(),
    );
    body.extend_from_slice(envelope.as_bytes());
    body.extend_from_slice(
        format!(
            "\r\n--{boundary}\r\n\
             Content-Disposition: form-data; name=\"package\"; filename=\"package.tar.gz\"\r\n\
             Content-Type: application/gzip\r\n\r\n"
        )
        .as_bytes(),
    );
    body.extend_from_slice(archive);
    body.extend_from_slice(format!("\r\n--{boundary}--\r\n").as_bytes());
    body
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_server_error_document_becomes_code_and_message() {
        let error = server_error(
            422,
            r#"{"error":{"code":"E_SIGNATURE","message":"the signature does not verify"}}"#,
        );
        assert_eq!(error.message, "E_SIGNATURE: the signature does not verify");
        assert_eq!(error.exit, crate::error::Exit::Failure);
    }

    #[test]
    fn an_html_error_names_the_status_and_the_first_line() {
        let error = server_error(502, "<html>\n  <body>Bad gateway</body>\n</html>");
        assert!(error.message.contains("502"), "{}", error.message);
        assert!(error.message.contains("<html>"), "{}", error.message);
    }

    #[test]
    fn the_multipart_body_holds_both_parts() {
        let body = multipart_body("BOUND", "{\"a\":1}", b"archive");
        let text = String::from_utf8_lossy(&body).to_string();
        assert!(text.contains("name=\"envelope\""));
        assert!(text.contains("Content-Type: application/json"));
        assert!(text.contains("{\"a\":1}"));
        assert!(text.contains("name=\"package\""));
        assert!(text.contains("Content-Type: application/gzip"));
        assert!(text.ends_with("--BOUND--\r\n"));
    }

    #[test]
    fn a_boundary_that_occurs_in_the_archive_is_extended() {
        let archive = b"aevia-pkg-1-2";
        let boundary = multipart_boundary(archive);
        assert!(!contains(archive, boundary.as_bytes()));
    }

    #[test]
    fn a_registry_url_must_be_http() {
        let error = Client::new("aevia.apich.org").expect_err("refused");
        assert_eq!(error.exit, crate::error::Exit::Usage);
        assert!(Client::new("https://aevia.apich.org/").is_ok());
    }

    #[test]
    fn the_polling_interval_has_a_floor() {
        let start = DeviceStart {
            device_code: "d".to_string(),
            user_code: "ABCD-EFGH".to_string(),
            verification_uri: "https://aevia.apich.org/activate".to_string(),
            expires_at: String::new(),
            interval: 0,
        };
        assert_eq!(start.interval(), Duration::from_secs(5));
    }
}
