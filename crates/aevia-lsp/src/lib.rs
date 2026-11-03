//! `aevia-lsp` — the editor adapter.
//!
//! This crate is the *only* place in the tree that knows what a client is. It
//! reads `Content-Length`-framed JSON-RPC over a byte stream, keeps a document
//! store, and translates between the protocol's vocabulary and
//! [`aevia_analysis`]. Every semantic answer comes from that crate; nothing
//! here type-checks anything.
//!
//! The split is deliberate. `aevia-analysis` must be usable by a web
//! playground, a CLI and a Zed extension that never speak this protocol, so it
//! has no `lsp-types`, no JSON and no `textDocument` vocabulary — a test in
//! that crate fails the build if one appears. Conversely, every protocol
//! decision (which capability to advertise, what `-32601` means, when to clear
//! diagnostics) lives here and nowhere else.
//!
//! ```
//! use std::io::Cursor;
//!
//! let mut stream = Vec::new();
//! for message in [
//!     r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{}}"#,
//!     r#"{"jsonrpc":"2.0","method":"exit"}"#,
//! ] {
//!     stream.extend(format!("Content-Length: {}\r\n\r\n{message}", message.len()).into_bytes());
//! }
//! let replies = Vec::new();
//! let exit = aevia_lsp::serve(Cursor::new(stream), replies);
//! assert_eq!(exit, aevia_lsp::Exit::Ungraceful);
//! ```
//!
//! # Concurrency
//!
//! Reading and writing are separate threads, joined by channels, and the
//! dispatcher sits between them. That is not decoration: a client that sends a
//! request while the server is publishing diagnostics for a large file must
//! not be able to deadlock the pair, and a single-threaded loop that writes
//! synchronously can.

#![forbid(unsafe_code)]

pub mod protocol;
pub mod server;
pub mod transport;

use std::io::{BufReader, Read, Write};
use std::sync::mpsc::{Receiver, Sender, channel};
use std::thread;

pub use server::{Control, Server};

/// How the session ended.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Exit {
    /// `shutdown` then `exit`, or the client closed the stream.
    Graceful,
    /// `exit` without `shutdown`: the protocol asks the process to exit 1.
    Ungraceful,
}

/// One message on its way in.
enum Incoming {
    /// A parsed message.
    Message(serde_json::Value),
    /// A frame whose body was not JSON.
    Malformed(String),
}

/// Runs one session to completion.
///
/// Reads framed messages from `input`, writes framed messages to `output`, and
/// returns when the client sends `exit` or closes the stream.
pub fn serve<R, W>(input: R, output: W) -> Exit
where
    R: Read + Send + 'static,
    W: Write + Send + 'static,
{
    let (to_client, from_server): (Sender<serde_json::Value>, Receiver<serde_json::Value>) =
        channel();
    let (to_server, from_client): (Sender<Incoming>, Receiver<Incoming>) = channel();

    // The writer owns the output for the whole session. Nothing in the
    // dispatcher can block on it: `send` hands the message to this thread and
    // returns.
    let writer = thread::spawn(move || {
        let mut writer = transport::Writer::new(output);
        for message in from_server {
            if writer.send(&message).is_err() {
                break;
            }
        }
    });

    // The reader owns the input. It ends at end of stream or at the first
    // framing error it cannot recover from; either way the dispatcher sees the
    // channel close.
    thread::spawn(move || {
        let mut reader = transport::Reader::new(BufReader::new(input));
        loop {
            let incoming = match reader.next_message() {
                Ok(Some(message)) => Incoming::Message(message),
                Ok(None) => break,
                Err(transport::TransportError::Json(error)) => Incoming::Malformed(error.to_string()),
                // A broken frame cannot be resynchronised, so the session ends
                // rather than inventing message boundaries.
                Err(_) => break,
            };
            if to_server.send(incoming).is_err() {
                break;
            }
        }
    });

    let mut server = Server::new(to_client.clone());
    let mut exit = Exit::Graceful;
    for incoming in from_client {
        match incoming {
            Incoming::Message(message) => {
                if let Control::Exit { graceful } = server.handle(message) {
                    if !graceful {
                        exit = Exit::Ungraceful;
                    }
                    break;
                }
            }
            Incoming::Malformed(detail) => server.parse_error(&detail),
        }
    }

    // Dropping the last sender closes the writer's channel, which is what lets
    // the join below finish: a server that returned before its last
    // `publishDiagnostics` was written would lose it.
    drop(server);
    drop(to_client);
    let _ = writer.join();
    exit
}

/// Runs a session on the process's standard streams.
#[must_use]
pub fn serve_stdio() -> Exit {
    serve(std::io::stdin(), std::io::stdout())
}
