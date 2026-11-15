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

static CHOICE: AtomicU8 = AtomicU8::new(0);

pub(crate) fn configure(choice: ColorChoice) {
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
pub(crate) fn enabled(
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

pub(crate) fn paint(
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
pub(crate) fn from_args(args: &[String]) -> ColorChoice {
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

pub(crate) fn write(
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
