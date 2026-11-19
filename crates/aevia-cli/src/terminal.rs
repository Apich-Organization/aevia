//! Terminal styling for CLI-owned text. Program output and LSP frames bypass it.

use crate::ColorChoice;
use std::fmt;
use std::io::IsTerminal;
use std::io::Write;
use std::io::{
    self,
};
use std::sync::atomic::AtomicU8;
use std::sync::atomic::Ordering;

/// Opens a local document or HTTP URL in the default application.
///
/// # Errors
///
/// Returns an error if the system opener cannot start.
#[cfg(unix)]
pub fn open_default(target: &std::ffi::OsStr) -> io::Result<()> {
    let command = if cfg!(target_os = "macos") {
        "open"
    } else {
        "xdg-open"
    };
    std::process::Command::new(command)
        .arg(target)
        .spawn()
        .map(drop)
}

/// Uses Windows file associations without passing the target through a shell.
///
/// # Errors
///
/// Returns an error for an invalid target or if the shell cannot open it.
#[cfg(windows)]
#[allow(
    unsafe_code,
    reason = "the Windows Shell API accepts a native pointer and COM initialization"
)]
pub fn open_default(target: &std::ffi::OsStr) -> io::Result<()> {
    use std::os::windows::ffi::OsStrExt as _;
    use windows_sys::Win32::System::Com::COINIT_APARTMENTTHREADED;
    use windows_sys::Win32::System::Com::CoInitializeEx;
    use windows_sys::Win32::System::Com::CoUninitialize;
    use windows_sys::Win32::UI::Shell::SEE_MASK_FLAG_NO_UI;
    use windows_sys::Win32::UI::Shell::SEE_MASK_NOASYNC;
    use windows_sys::Win32::UI::Shell::SHELLEXECUTEINFOW;
    use windows_sys::Win32::UI::Shell::ShellExecuteExW;
    use windows_sys::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;

    let mut wide: Vec<_> = target.encode_wide().collect();
    if wide.contains(&0) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "the document path contains NUL",
        ));
    }
    wide.push(0);
    let size = u32::try_from(std::mem::size_of::<SHELLEXECUTEINFOW>())
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidInput, error))?;
    let mut request = SHELLEXECUTEINFOW {
        cbSize: size,
        fMask: SEE_MASK_FLAG_NO_UI | SEE_MASK_NOASYNC,
        lpFile: wide.as_ptr(),
        nShow: SW_SHOWNORMAL,
        ..SHELLEXECUTEINFOW::default()
    };
    let apartment = u32::try_from(COINIT_APARTMENTTHREADED)
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidInput, error))?;
    // SAFETY: the reserved argument is null and the documented STA flag is valid.
    let status = unsafe { CoInitializeEx(std::ptr::null(), apartment) };
    if status < 0 {
        return Err(io::Error::other(format!(
            "could not initialize the Windows Shell (HRESULT {status:#x})"
        )));
    }
    // SAFETY: request has the documented size and points to a NUL-terminated
    // native string that remains alive until the synchronous call returns.
    let opened = unsafe { ShellExecuteExW(&raw mut request) };
    let error = (opened == 0).then(io::Error::last_os_error);
    // SAFETY: this balances the successful COM initialization on this thread.
    unsafe { CoUninitialize() };
    error.map_or(Ok(()), Err)
}

static CHOICE: AtomicU8 = AtomicU8::new(0);

pub fn configure(choice: ColorChoice) {
    CHOICE.store(
        match choice {
            | ColorChoice::Auto => 0,
            | ColorChoice::Always => 1,
            | ColorChoice::Never => 2,
        },
        Ordering::Relaxed,
    );
}

fn choice() -> ColorChoice {
    match CHOICE.load(Ordering::Relaxed) {
        | 1 => ColorChoice::Always,
        | 2 => ColorChoice::Never,
        | _ => ColorChoice::Auto,
    }
}

/// Detect each stream independently: redirecting stdout must not disable stderr.
#[must_use]
pub fn enabled(
    choice: ColorChoice,
    stderr: bool,
) -> bool {
    match choice {
        | ColorChoice::Always => true,
        | ColorChoice::Never => false,
        | ColorChoice::Auto => {
            if std::env::var_os("NO_COLOR").is_some_and(|value| !value.is_empty())
                || std::env::var("TERM").is_ok_and(|term| term == "dumb")
                || std::env::var("CLICOLOR").is_ok_and(|value| value == "0")
            {
                return false;
            }
            let terminal = if stderr {
                io::stderr().is_terminal()
            } else {
                io::stdout().is_terminal()
            };
            // Old Windows consoles do not interpret ANSI. Modern terminal hosts
            // advertise VT support through these environment variables.
            let ansi = !cfg!(windows)
                || std::env::var_os("WT_SESSION").is_some()
                || std::env::var_os("ANSICON").is_some()
                || std::env::var("ConEmuANSI").is_ok_and(|value| value == "ON")
                || std::env::var("TERM").is_ok_and(|term| term != "dumb");
            terminal && ansi
        },
    }
}

#[must_use]
pub fn paint(
    text: &str,
    code: &str,
    stderr: bool,
) -> String {
    if !text.is_empty() && enabled(choice(), stderr) && !text.contains('\u{1b}') {
        format!("\x1b[{code}m{text}\x1b[0m")
    } else {
        text.to_string()
    }
}

/// Read colour policy before full parsing so usage errors respect --color too.
#[allow(
    clippy::single_call_fn,
    reason = "resolves color before full option parsing can report usage errors"
)]
pub fn from_args(args: &[String]) -> ColorChoice {
    let mut selected = ColorChoice::Auto;
    let mut args = args.iter();
    while let Some(arg) = args.next() {
        if arg == "--" {
            break;
        }
        let value = if arg == "--color" {
            args.next().map(String::as_str)
        } else {
            arg.strip_prefix("--color=")
        };
        match value {
            | Some("always") => selected = ColorChoice::Always,
            | Some("never") => selected = ColorChoice::Never,
            | Some("auto") => selected = ColorChoice::Auto,
            | _ => {},
        }
    }
    selected
}

#[allow(
    clippy::single_call_fn,
    reason = "styles CLI-owned messages independently of language program output"
)]
fn decorate(
    text: &str,
    stderr: bool,
) -> String {
    if !enabled(choice(), stderr) || text.contains('\u{1b}') {
        return text.to_string();
    }
    let mut rendered = String::new();
    for piece in text.split_inclusive('\n') {
        let line = piece.trim_end_matches('\n');
        let trimmed = line.trim();
        // Expression results separate the inferred type from the value.
        if !stderr && let Some((ty, value)) = line.split_once(" = ") {
            rendered.push_str(&paint(ty, "36", false));
            rendered.push_str(" = ");
            rendered.push_str(value);
            if piece.ends_with('\n') {
                rendered.push('\n');
            }
            continue;
        }
        let code = if trimmed.is_empty() {
            None
        } else if trimmed == "ok"
            || trimmed.contains(": ok")
            || trimmed.ends_with("... ok")
            || trimmed.starts_with("loaded ")
            || trimmed.starts_with("wrote ")
            || trimmed.starts_with("added ")
            || trimmed == "session cleared"
            || trimmed.contains(", 0 failed")
            || trimmed.starts_with("0 failed")
        {
            Some("32")
        } else if trimmed.contains("warning") || trimmed.contains("note:") {
            Some("33")
        } else if stderr || trimmed.contains("FAILED") || trimmed.contains(" failed") {
            Some("31")
        } else if trimmed.ends_with(':')
            || trimmed.starts_with("aevia —")
            || trimmed.starts_with("Aevia —")
            || trimmed.starts_with("Aevia ")
            || trimmed.starts_with("aevia ") && trimmed.contains(" — ")
        {
            Some("1;36")
        } else if trimmed.starts_with("aevia ")
            || trimmed.starts_with("--")
            || trimmed.starts_with(':')
            || trimmed.starts_with("-h,")
            || trimmed.starts_with("-V,")
        {
            Some("36")
        } else {
            None
        };
        if let Some(code) = code {
            rendered.push_str(&paint(line, code, stderr));
        } else {
            rendered.push_str(line);
        }
        if piece.ends_with('\n') {
            rendered.push('\n');
        }
    }
    rendered
}

pub fn write(
    args: fmt::Arguments<'_>,
    stderr: bool,
    newline: bool,
) {
    let styled = decorate(&args.to_string(), stderr);
    if stderr {
        let mut stream = io::stderr().lock();
        let _ = stream.write_all(styled.as_bytes());
        if newline {
            let _ = stream.write_all(b"\n");
        }
    } else {
        let mut stream = io::stdout().lock();
        let _ = stream.write_all(styled.as_bytes());
        if newline {
            let _ = stream.write_all(b"\n");
        }
    }
}
