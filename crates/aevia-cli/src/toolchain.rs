//! Install verified toolchains side by side and select one with a single state file.

use std::env;
use std::fs;
use std::io::ErrorKind;
use std::io::{
    self,
};
use std::path::Path;
use std::path::PathBuf;
use std::process::Command;
use std::process::ExitCode;

const TOOLS: [&str; 3] = ["aevia", "aevia-lsp", "aevia-pkg"];
const MARKER: &str = ".aevia-toolchain-launcher";
const STATE: &str = "active-toolchain";
/// Usage for the toolchain subcommand.
pub const USAGE: &str = "Usage: aevia toolchain install [stable|nightly|VERSION] [--no-activate]\n       aevia toolchain update [stable|nightly]\n       aevia toolchain use VERSION\n       aevia toolchain list\n       aevia toolchain run VERSION [AEVIA-ARGS...]";

fn invalid(message: impl Into<String>) -> io::Error {
    io::Error::new(ErrorKind::InvalidInput, message.into())
}

fn executable(
    directory: &Path,
    name: &str,
) -> PathBuf {
    directory.join(format!("{name}{}", env::consts::EXE_SUFFIX))
}

#[allow(
    clippy::single_call_fn,
    reason = "reads a standalone CLI installation location"
)]
fn home() -> io::Result<PathBuf> {
    env::var_os("AEVIA_HOME")
        .filter(|path| !path.is_empty())
        .map(PathBuf::from)
        .or_else(|| {
            let (variable, folder) = if cfg!(windows) {
                ("LOCALAPPDATA", "aevia")
            } else {
                ("HOME", ".aevia")
            };
            env::var_os(variable).map(|path| PathBuf::from(path).join(folder))
        })
        .ok_or_else(|| invalid("set AEVIA_HOME to the toolchain directory"))
}

fn release_tag(input: &str) -> io::Result<String> {
    if let Some(date) = input.strip_prefix("nightly-") {
        let mut fields = date.split('-');
        if let (Some(year), Some(month), Some(day), None) =
            (fields.next(), fields.next(), fields.next(), fields.next())
            && year.len() == 4
            && month.len() == 2
            && day.len() == 2
            && year.bytes().all(|c| c.is_ascii_digit())
            && month.parse::<u8>().is_ok_and(|m| (1..=12).contains(&m))
            && day.parse::<u8>().is_ok_and(|d| (1..=31).contains(&d))
            && month.bytes().chain(day.bytes()).all(|c| c.is_ascii_digit())
        {
            return Ok(input.to_owned());
        }
    } else {
        let version = input.strip_prefix('v').unwrap_or(input);
        let (numbers, pre) = version
            .split_once('-')
            .map_or((version, None), |(core, suffix)| (core, Some(suffix)));
        if version.len() <= 80
            && numbers.split('.').count() == 3
            && numbers.split('.').all(|n| {
                !n.is_empty()
                    && (n == "0" || !n.starts_with('0'))
                    && n.bytes().all(|c| c.is_ascii_digit())
            })
            && pre.is_none_or(|suffix| {
                suffix.split('.').all(|part| {
                    !part.is_empty()
                        && part.bytes().all(|c| c.is_ascii_alphanumeric() || c == b'-')
                        && (part == "0"
                            || !part.starts_with('0')
                            || !part.bytes().all(|c| c.is_ascii_digit()))
                })
            })
        {
            return Ok(format!("v{version}"));
        }
    }
    Err(invalid(format!("invalid toolchain version {input:?}")))
}

fn installed(
    root: &Path,
    tag: &str,
) -> io::Result<PathBuf> {
    let directory = root.join("toolchains").join(release_tag(tag)?);
    if TOOLS
        .iter()
        .all(|name| executable(&directory, name).is_file())
    {
        Ok(directory)
    } else {
        Err(io::Error::new(
            ErrorKind::NotFound,
            format!("toolchain {tag} is not installed"),
        ))
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Channel {
    Stable,
    Nightly,
    Exact,
}

impl Channel {
    fn parse(input: &str) -> io::Result<Self> {
        match input {
            | "stable" => Ok(Self::Stable),
            | "nightly" | "dev" => Ok(Self::Nightly),
            | "exact" => Ok(Self::Exact),
            | _ => Err(invalid(format!("unknown toolchain channel {input:?}"))),
        }
    }

    const fn name(self) -> &'static str {
        match self {
            | Self::Stable => "stable",
            | Self::Nightly => "nightly",
            | Self::Exact => "exact",
        }
    }
}

fn active(root: &Path) -> io::Result<(String, Channel)> {
    let state = fs::read_to_string(root.join(STATE))?;
    let (tag, channel) = state
        .trim_end()
        .split_once('\n')
        .ok_or_else(|| invalid("invalid toolchain selection"))?;
    Ok((release_tag(tag)?, Channel::parse(channel)?))
}

// A staging directory lives on the destination filesystem for atomic renames.
// Only directories allocated by this process get removed.
struct Staging(PathBuf);

impl Staging {
    fn new(parent: &Path) -> io::Result<Self> {
        for attempt in 0..128 {
            let directory = parent.join(format!(".aevia-stage-{}-{attempt}", std::process::id()));
            match fs::create_dir(&directory) {
                | Ok(()) => return Ok(Self(directory)),
                | Err(error) if error.kind() == ErrorKind::AlreadyExists => (),
                | Err(error) => return Err(error),
            }
        }
        Err(io::Error::new(
            ErrorKind::AlreadyExists,
            "cannot allocate toolchain staging directory",
        ))
    }
}

impl Drop for Staging {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn select(
    root: &Path,
    tag: &str,
    channel: Channel,
) -> io::Result<()> {
    installed(root, tag)?;
    let stage = Staging::new(root)?;
    let pending = stage.0.join(STATE);
    fs::write(&pending, format!("{tag}\n{}\n", channel.name()))?;
    fs::rename(pending, root.join(STATE))
}

#[allow(
    clippy::single_call_fn,
    reason = "registers an unpacked release as a transaction"
)]
fn register(
    root: &Path,
    input: &str,
    unpacked: &Path,
    channel: Channel,
    activate: bool,
) -> io::Result<()> {
    let tag = release_tag(input)?;
    let toolchains = root.join("toolchains");
    fs::create_dir_all(&toolchains)?;
    let destination = toolchains.join(&tag);
    if !destination.exists() {
        let stage = Staging::new(&toolchains)?;
        let payload = stage.0.join("payload");
        fs::create_dir(&payload)?;
        for name in TOOLS {
            fs::copy(executable(unpacked, name), executable(&payload, name))?;
        }
        let kernel_commit = unpacked.join("RSSN-COMMIT");
        if kernel_commit.is_file() {
            fs::copy(kernel_commit, payload.join("RSSN-COMMIT"))?;
        }
        if let Err(error) = fs::rename(payload, &destination)
            && installed(root, &tag).is_err()
        {
            return Err(error);
        }
    }
    let directory = installed(root, &tag)?;
    let bin = root.join("bin");
    fs::create_dir_all(&bin)?;
    let marker = bin.join(MARKER);
    // Refuse to claim a directory that already contains unmanaged binaries.
    if !marker.is_file() && TOOLS.iter().any(|name| executable(&bin, name).exists()) {
        return Err(invalid(format!(
            "move the existing executables out of {} before installing managed toolchains",
            bin.display()
        )));
    }
    fs::OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(false)
        .open(marker)?;
    let launcher = executable(&directory, "aevia");
    for name in TOOLS {
        let link = executable(&bin, name);
        if !link.exists() {
            match fs::hard_link(&launcher, &link) {
                | Ok(()) => (),
                | Err(error) if error.kind() == ErrorKind::AlreadyExists => (),
                | Err(error) => return Err(error),
            }
        }
    }
    if activate {
        select(root, &tag, channel)?;
    }
    println!("Installed {tag} in {}", directory.display());
    Ok(())
}

fn spawn(
    root: &Path,
    tool: &str,
    tag: &str,
    args: impl IntoIterator<Item = impl AsRef<std::ffi::OsStr>>,
) -> io::Result<ExitCode> {
    let directory = installed(root, tag)?;
    let mut command = Command::new(executable(&directory, tool));
    command.args(args).env("AEVIA_HOME", root);
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt as _;
        Err(command.exec())
    }
    #[cfg(not(unix))]
    {
        command.status().map(exit_code)
    }
}

#[cfg_attr(
    unix,
    allow(
        clippy::single_call_fn,
        reason = "Windows also uses this conversion for its launcher"
    )
)]
fn exit_code(status: std::process::ExitStatus) -> ExitCode {
    ExitCode::from(
        status
            .code()
            .and_then(|code| u8::try_from(code).ok())
            .unwrap_or(1),
    )
}

#[allow(
    clippy::single_call_fn,
    reason = "entry point for the installed executable shims"
)]
/// Dispatches to the active installation when started from a managed launcher.
pub fn launch() -> Option<ExitCode> {
    let current = env::current_exe().ok()?;
    let bin = current.parent()?;
    let tool = current.file_stem()?.to_str()?;
    if bin.file_name()? != "bin" || !TOOLS.contains(&tool) || !bin.join(MARKER).is_file() {
        return None;
    }
    let root = bin.parent()?;
    let result = match active(root) {
        | Ok((tag, _)) => spawn(root, tool, &tag, env::args_os().skip(1)),
        | Err(error) if error.kind() == ErrorKind::NotFound && tool == "aevia" => {
            let mut args = env::args().skip(1);
            if args.next().as_deref() == Some("toolchain") {
                return Some(report(execute(&args.collect::<Vec<_>>(), Some(root))));
            }
            Err(error)
        },
        | Err(error) => Err(error),
    };
    Some(result.unwrap_or_else(|error| {
        eprintln!("{tool}: cannot start active Aevia toolchain: {error}");
        ExitCode::FAILURE
    }))
}

// The release client and verifier are the same scripts used for bootstrapping.
// This also keeps the asset and channel rules identical for updates.
fn fetch(
    root: &Path,
    selection: &str,
    activate: bool,
) -> io::Result<ExitCode> {
    let channel = Channel::parse(selection).unwrap_or(Channel::Exact);
    let explicit = if channel == Channel::Exact {
        Some(release_tag(selection)?)
    } else {
        None
    };
    fs::create_dir_all(root)?;
    let stage = Staging::new(root)?;
    let mut command;
    if cfg!(windows) {
        let script = stage.0.join("install.ps1");
        fs::write(&script, include_str!("../../../platform/install.ps1"))?;
        command = Command::new("powershell.exe");
        command
            .args([
                "-NoProfile",
                "-NonInteractive",
                "-ExecutionPolicy",
                "Bypass",
                "-File",
            ])
            .arg(script)
            .arg("-HomeDirectory")
            .arg(root)
            .arg("-NoPath");
        if let Some(tag) = explicit {
            command.args(["-Version", &tag]);
        } else {
            command.args(["-Channel", channel.name()]);
        }
        if !activate {
            command.arg("-NoActivate");
        }
    } else {
        let script = stage.0.join("install.sh");
        fs::write(&script, include_str!("../../../platform/install.sh"))?;
        command = Command::new("sh");
        command.arg(script).arg("--home").arg(root).arg("--no-path");
        if let Some(tag) = explicit {
            command.args(["--version", &tag]);
        } else {
            command.args(["--channel", channel.name()]);
        }
        if !activate {
            command.arg("--no-activate");
        }
    }
    command.status().map(exit_code)
}

fn execute(
    args: &[String],
    launcher_root: Option<&Path>,
) -> io::Result<ExitCode> {
    let resolve_home = || launcher_root.map_or_else(home, |root| Ok(root.to_owned()));
    let words: Vec<&str> = args.iter().map(String::as_str).collect();
    match words.as_slice() {
        | [] | ["help" | "--help" | "-h"] => println!("{USAGE}"),
        | ["install"] => return fetch(&resolve_home()?, "stable", true),
        | ["install", "--no-activate"] => return fetch(&resolve_home()?, "stable", false),
        | ["install", selection] => return fetch(&resolve_home()?, selection, true),
        | ["install", selection, "--no-activate"] => {
            return fetch(&resolve_home()?, selection, false);
        },
        | ["update", selection @ ("stable" | "nightly" | "dev")] => {
            return fetch(&resolve_home()?, selection, true);
        },
        | ["update"] => {
            let root = resolve_home()?;
            let (_, channel) = active(&root)?;
            if channel == Channel::Exact {
                return Err(invalid(
                    "this installation is pinned; choose update stable or update nightly",
                ));
            }
            return fetch(&root, channel.name(), true);
        },
        | ["use", tag] => select(&resolve_home()?, &release_tag(tag)?, Channel::Exact)?,
        | ["run", tag, rest @ ..] => {
            return spawn(&resolve_home()?, "aevia", tag, rest.iter().copied());
        },
        | ["list"] => {
            let root = resolve_home()?;
            let selected = match active(&root) {
                | Ok((tag, channel)) => Some((tag, channel)),
                | Err(error) if error.kind() == ErrorKind::NotFound => None,
                | Err(error) => return Err(error),
            };
            let entries = match fs::read_dir(root.join("toolchains")) {
                | Ok(entries) => entries,
                | Err(error) if error.kind() == ErrorKind::NotFound => {
                    return Ok(ExitCode::SUCCESS);
                },
                | Err(error) => return Err(error),
            };
            let mut versions = Vec::new();
            for entry in entries {
                if let Ok(tag) = entry?.file_name().into_string()
                    && release_tag(&tag).is_ok_and(|canonical| canonical == tag)
                    && installed(&root, &tag).is_ok()
                {
                    versions.push(tag);
                }
            }
            versions.sort_unstable();
            for tag in versions {
                if let Some((active_tag, channel)) = &selected
                    && *active_tag == tag
                {
                    println!("* {tag} ({})", channel.name());
                } else {
                    println!("  {tag}");
                }
            }
        },
        | [
            "register",
            tag,
            unpacked,
            "--home",
            root,
            "--channel",
            channel,
            rest @ ..,
        ] if matches!(rest, [] | ["--no-activate"]) => {
            register(
                Path::new(root),
                tag,
                Path::new(unpacked),
                Channel::parse(channel)?,
                rest.is_empty(),
            )?;
        },
        | _ => return Err(invalid(USAGE)),
    }
    Ok(ExitCode::SUCCESS)
}

#[allow(clippy::single_call_fn, reason = "entry point for toolchain commands")]
/// Handles installation and version selection commands.
pub fn run(args: &[String]) -> ExitCode {
    report(execute(args, None))
}

fn report(result: io::Result<ExitCode>) -> ExitCode {
    result.unwrap_or_else(|error| {
        eprintln!("aevia toolchain: {error}");
        ExitCode::from(if error.kind() == ErrorKind::InvalidInput {
            2
        } else {
            1
        })
    })
}
