//! `Content-Length` framing over a byte stream.
//!
//! The protocol frames a JSON message with a header block: one or more
//! `Name: value` lines, then an empty line, then exactly `Content-Length`
//! bytes of UTF-8 JSON. The length counts **bytes**, not characters — a
//! message containing `∫` is three bytes longer than its `char` count, and one
//! containing an emoji is four.
//!
//! Reading is deliberately forgiving in the ways that cannot corrupt the
//! stream (a bare `\n` instead of `\r\n`, header names in any case, unknown
//! headers) and strict in the way that can (the length must be exact). A body
//! that is not JSON is reported as an error the caller can answer and then
//! continue: the frame was consumed, so the next message is still in sync.

use std::io::BufRead;
use std::io::Write;
use std::io::{
    self,
};

/// Why a message could not be read or written.
#[derive(Debug)]
pub enum TransportError {
    /// The stream failed.
    Io(io::Error),
    /// The header block had no `Content-Length`.
    MissingLength,
    /// The length was invalid or appeared more than once.
    InvalidLength,
    /// The body was not JSON.
    Json(serde_json::Error),
}

impl std::fmt::Display for TransportError {
    fn fmt(
        &self,
        f: &mut std::fmt::Formatter<'_>,
    ) -> std::fmt::Result {
        match self {
            | Self::Io(error) => write!(f, "transport: {error}"),
            | Self::MissingLength => f.write_str("transport: a message had no Content-Length"),
            | Self::InvalidLength => f.write_str("transport: invalid or duplicate Content-Length"),
            | Self::Json(error) => write!(f, "transport: the body was not JSON: {error}"),
        }
    }
}

impl std::error::Error for TransportError {}

impl From<io::Error> for TransportError {
    fn from(error: io::Error) -> Self {
        Self::Io(error)
    }
}

/// Reads framed messages.
pub struct Reader<R> {
    inner: R,
}

impl<R: BufRead> Reader<R> {
    /// Reads from `inner`.
    pub fn new(inner: R) -> Self {
        Self { inner }
    }

    /// The next message, or `None` at end of stream.
    ///
    /// # Errors
    ///
    /// [`TransportError`] when the stream fails, the frame has a missing or
    /// invalid length, or the body is not JSON. In the last case the frame has
    /// been consumed and the next call is still in sync.
    pub fn next_message(&mut self) -> Result<Option<serde_json::Value>, TransportError> {
        let mut length = None;
        let mut started = false;
        loop {
            let Some(line) = read_line(&mut self.inner)? else {
                return if started {
                    Err(
                        io::Error::new(io::ErrorKind::UnexpectedEof, "incomplete header block")
                            .into(),
                    )
                } else {
                    Ok(None)
                };
            };
            started = true;
            if line.is_empty() {
                break;
            }
            if let Some((name, value)) = line.split_once(':') {
                if name.trim().eq_ignore_ascii_case("content-length") {
                    let value = value.trim();
                    if length.is_some()
                        || value.is_empty()
                        || !value.bytes().all(|b| b.is_ascii_digit())
                    {
                        return Err(TransportError::InvalidLength);
                    }
                    length = Some(
                        value
                            .parse::<usize>()
                            .map_err(|_| TransportError::InvalidLength)?,
                    );
                }
            }
        }
        let Some(length) = length else {
            return Err(TransportError::MissingLength);
        };
        if length == 0 {
            return Err(TransportError::InvalidLength);
        }
        let mut body = Vec::new();
        body.try_reserve_exact(length).map_err(|_| {
            TransportError::Io(io::Error::new(
                io::ErrorKind::InvalidData,
                "message body is too large",
            ))
        })?;
        body.resize(length, 0);
        self.inner.read_exact(&mut body)?;
        let value = serde_json::from_slice(&body).map_err(TransportError::Json)?;
        Ok(Some(value))
    }
}

/// Reads one line without its terminator, or `None` at end of stream.
fn read_line(reader: &mut impl BufRead) -> Result<Option<String>, TransportError> {
    let mut bytes = Vec::new();
    let read = reader.read_until(b'\n', &mut bytes)?;
    if read == 0 {
        return Ok(None);
    }
    if bytes.last() != Some(&b'\n') {
        return Err(io::Error::new(io::ErrorKind::UnexpectedEof, "incomplete header line").into());
    }
    while bytes
        .last()
        .is_some_and(|byte| *byte == b'\n' || *byte == b'\r')
    {
        bytes.pop();
    }
    String::from_utf8(bytes).map(Some).map_err(|_| {
        TransportError::Io(io::Error::new(
            io::ErrorKind::InvalidData,
            "a header line was not UTF-8",
        ))
    })
}

/// Frames messages for a writer.
pub struct Writer<W> {
    inner: W,
}

impl<W: Write> Writer<W> {
    /// Writes to `inner`.
    pub fn new(inner: W) -> Self {
        Self { inner }
    }

    /// Writes one message and flushes it.
    ///
    /// # Errors
    ///
    /// The stream's own error.
    pub fn send(
        &mut self,
        message: &serde_json::Value,
    ) -> io::Result<()> {
        let body = serde_json::to_vec(message)
            .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
        write!(self.inner, "Content-Length: {}\r\n\r\n", body.len())?;
        self.inner.write_all(&body)?;
        self.inner.flush()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    fn framed(body: &str) -> Vec<u8> {
        let mut bytes = format!("Content-Length: {}\r\n\r\n", body.len()).into_bytes();
        bytes.extend_from_slice(body.as_bytes());
        bytes
    }

    #[test]
    fn a_message_round_trips() {
        let mut writer = Writer::new(Vec::new());
        let message = serde_json::json!({ "jsonrpc": "2.0", "id": 1, "method": "initialize" });
        writer.send(&message).expect("writes");
        let bytes = writer.inner;
        let mut reader = Reader::new(Cursor::new(bytes));
        assert_eq!(reader.next_message().expect("reads"), Some(message));
        assert_eq!(reader.next_message().expect("eof"), None);
    }

    #[test]
    fn several_messages_in_one_stream_are_read_in_order() {
        let mut bytes = framed(r#"{"id":1}"#);
        bytes.extend(framed(r#"{"id":2}"#));
        let mut reader = Reader::new(Cursor::new(bytes));
        assert_eq!(reader.next_message().expect("one").expect("some")["id"], 1);
        assert_eq!(reader.next_message().expect("two").expect("some")["id"], 2);
        assert!(reader.next_message().expect("end").is_none());
    }

    #[test]
    fn the_length_counts_bytes_not_characters() {
        // `∫` is three bytes and one character: a character-counted length
        // would cut the body short.
        let body = r#"{"text":"∫"}"#;
        let bytes = framed(body);
        let mut reader = Reader::new(Cursor::new(bytes));
        let value = reader.next_message().expect("reads").expect("some");
        assert_eq!(value["text"], "∫");
    }

    #[test]
    fn an_astral_character_survives_framing() {
        let body = r#"{"text":"😀"}"#;
        let mut reader = Reader::new(Cursor::new(framed(body)));
        let value = reader.next_message().expect("reads").expect("some");
        assert_eq!(value["text"], "😀");
    }

    #[test]
    fn header_names_are_case_insensitive_and_a_bare_newline_is_tolerated() {
        let body = r#"{"id":7}"#;
        let bytes = format!("content-length: {}\n\n{}", body.len(), body).into_bytes();
        let mut reader = Reader::new(Cursor::new(bytes));
        assert_eq!(
            reader.next_message().expect("reads").expect("some")["id"],
            7
        );
    }

    #[test]
    fn a_body_that_is_not_json_is_an_error_but_the_stream_stays_in_sync() {
        let mut bytes = framed("this is not json");
        bytes.extend(framed(r#"{"id":9}"#));
        let mut reader = Reader::new(Cursor::new(bytes));
        assert!(matches!(
            reader.next_message(),
            Err(TransportError::Json(_))
        ));
        assert_eq!(
            reader.next_message().expect("reads").expect("some")["id"],
            9
        );
    }

    #[test]
    fn a_frame_without_a_length_is_refused() {
        let bytes = b"X-Nothing: 1\r\n\r\n{}".to_vec();
        let mut reader = Reader::new(Cursor::new(bytes));
        assert!(matches!(
            reader.next_message(),
            Err(TransportError::MissingLength)
        ));
    }

    #[test]
    fn truncated_headers_are_not_clean_eof() {
        for bytes in [
            b"Content-Length: 2".as_slice(),
            b"Content-Length: 2\r\n".as_slice(),
        ] {
            let mut reader = Reader::new(Cursor::new(bytes));
            assert!(
                matches!(reader.next_message(), Err(TransportError::Io(e)) if e.kind() == io::ErrorKind::UnexpectedEof)
            );
        }
    }

    #[test]
    fn invalid_and_duplicate_lengths_are_refused() {
        for header in [
            "Content-Length: +2\r\n",
            "Content-Length: -1\r\n",
            "Content-Length: 2\r\ncontent-length: 3\r\n",
        ] {
            let frame = format!("{header}\r\n{{}}");
            let mut reader = Reader::new(Cursor::new(frame));
            assert!(
                matches!(reader.next_message(), Err(TransportError::InvalidLength)),
                "{header:?}"
            );
        }
    }

    #[test]
    fn impossible_body_allocation_is_reported() {
        let frame = format!("Content-Length: {}\r\n\r\n", usize::MAX);
        let mut reader = Reader::new(Cursor::new(frame));
        assert!(
            matches!(reader.next_message(), Err(TransportError::Io(e)) if e.kind() == io::ErrorKind::InvalidData)
        );
    }
}
