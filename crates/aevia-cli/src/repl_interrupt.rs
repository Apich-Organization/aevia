//! Ctrl-C belongs to the current submission, never to the lifetime of the REPL.

use aevia_core::CancellationToken;
use aevia_core::EvalError;
use std::rc::Rc;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use std::sync::atomic::Ordering;

/// Installs a handler only when the first submission starts. Before that,
/// Rustyline handles input cancellation itself (and an unfinished pipe keeps
/// the operating system's ordinary signal behavior).
pub(crate) struct Signals {
    token: CancellationToken,
    active: Arc<AtomicBool>,
    installed: bool,
}

impl Signals {
    pub(crate) fn new(token: CancellationToken) -> Self {
        Self {
            token,
            active: Arc::new(AtomicBool::new(false)),
            installed: false,
        }
    }

    pub(crate) fn begin(&mut self) -> Result<Activity, String> {
        if !self.installed {
            let active = self.active.clone();
            let token = self.token.clone();
            ctrlc::set_handler(move || {
                if active.load(Ordering::Acquire) {
                    token.cancel();
                }
            })
            .map_err(|error| format!("cannot install the REPL interrupt handler: {error}"))?;
            self.installed = true;
        }
        self.token.reset();
        self.active.store(true, Ordering::Release);
        Ok(Activity {
            active: self.active.clone(),
        })
    }
}

pub(crate) struct Activity {
    active: Arc<AtomicBool>,
}

impl Drop for Activity {
    fn drop(&mut self) {
        self.active.store(false, Ordering::Release);
    }
}

/// Program input uses bounded foreground polls, never a background reader.
/// This also covers Ctrl-C arriving between IO actions and the next input read.
pub(crate) fn line_reader(token: CancellationToken) -> Rc<dyn Fn() -> Result<String, EvalError>> {
    Rc::new(move || read_program_line(&token))
}

#[cfg(unix)]
fn read_program_line(token: &CancellationToken) -> Result<String, EvalError> {
    use nix::errno::Errno;
    use nix::poll::PollFd;
    use nix::poll::PollFlags;
    use nix::poll::poll;
    use std::io::Write;
    use std::os::fd::AsFd;
    let input = std::io::stdin();
    std::io::stdout().flush().map_err(io_error)?;
    let mut bytes = Vec::new();
    loop {
        if token.is_cancelled() {
            return Err(interrupted());
        }
        let mut descriptors = [PollFd::new(
            input.as_fd(),
            PollFlags::POLLIN | PollFlags::POLLHUP,
        )];
        match poll(&mut descriptors, 25_u16) {
            | Ok(0) | Err(Errno::EINTR) => continue,
            | Err(error) => return Err(io_error(error)),
            | Ok(_) => {},
        }
        if token.is_cancelled() {
            return Err(interrupted());
        }
        // One byte avoids buffering a later line that belongs to a future
        // program read or the REPL. The terminal retains canonical editing.
        let mut byte = [0u8; 1];
        match nix::unistd::read(input.as_fd(), &mut byte) {
            | Ok(0) => break,
            | Ok(_) if byte[0] == b'\n' => break,
            | Ok(_) => bytes.push(byte[0]),
            | Err(Errno::EINTR | Errno::EAGAIN) => continue,
            | Err(error) => return Err(io_error(error)),
        }
    }
    if token.is_cancelled() {
        return Err(interrupted());
    }
    String::from_utf8(bytes)
        .map(|line| line.trim_end_matches('\r').to_string())
        .map_err(io_error)
}

#[cfg(windows)]
fn read_program_line(token: &CancellationToken) -> Result<String, EvalError> {
    use crossterm::event::Event;
    use crossterm::event::KeyCode;
    use crossterm::event::KeyEventKind;
    use crossterm::event::KeyModifiers;
    use crossterm::event::{
        self,
    };
    use std::io::IsTerminal;
    use std::io::Write;
    use std::time::Duration;
    if !std::io::stdin().is_terminal() {
        // Piped REPL source has already reached EOF before evaluation starts.
        if token.is_cancelled() {
            return Err(interrupted());
        }
        let mut line = String::new();
        std::io::stdin().read_line(&mut line).map_err(io_error)?;
        if token.is_cancelled() {
            return Err(interrupted());
        }
        return Ok(line.trim_end_matches(['\r', '\n']).to_string());
    }
    crossterm::terminal::enable_raw_mode().map_err(io_error)?;
    struct RawMode;
    impl Drop for RawMode {
        fn drop(&mut self) {
            let _ = crossterm::terminal::disable_raw_mode();
        }
    }
    let _raw = RawMode;
    let mut output = std::io::stdout().lock();
    output.flush().map_err(io_error)?;
    let mut line = String::new();
    loop {
        if token.is_cancelled() {
            return Err(interrupted());
        }
        if !event::poll(Duration::from_millis(25)).map_err(io_error)? {
            continue;
        }
        match event::read().map_err(io_error)? {
            | Event::Key(key) if key.kind != KeyEventKind::Release => {
                match key.code {
                    | KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                        token.cancel();
                        return Err(interrupted());
                    },
                    | KeyCode::Char('d' | 'z') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                        return Ok(line);
                    },
                    | KeyCode::Enter => {
                        write!(output, "\r\n").map_err(io_error)?;
                        output.flush().map_err(io_error)?;
                        return Ok(line);
                    },
                    | KeyCode::Tab => {
                        line.push('\t');
                        write!(output, "\t").map_err(io_error)?;
                    },
                    | KeyCode::Backspace => {
                        if line.pop().is_some() {
                            write!(output, "\x08 \x08").map_err(io_error)?;
                        }
                    },
                    | KeyCode::Char(ch)
                        if !key
                            .modifiers
                            .intersects(KeyModifiers::CONTROL | KeyModifiers::ALT) =>
                    {
                        line.push(ch);
                        write!(output, "{ch}").map_err(io_error)?;
                    },
                    | _ => {},
                }
            },
            | _ => {},
        }
        output.flush().map_err(io_error)?;
    }
}

fn io_error(error: impl std::fmt::Display) -> EvalError {
    EvalError::new("E0810", error.to_string())
}

fn interrupted() -> EvalError {
    EvalError::new("E0812", "evaluation interrupted")
}
