//! Help and presentation are exercised through the real CLI, without a package.
use std::process::Command;
use std::process::Output;

fn cli(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_aevia"))
        .args(args)
        .env_remove("NO_COLOR")
        .env_remove("CLICOLOR")
        .output()
        .expect("run CLI")
}

fn text(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes).replace("\r\n", "\n")
}

#[test]
fn overview_explains_default_session_and_where_to_find_details() {
    let output = cli(&["--help", "--color=never"]);
    assert!(output.status.success());
    let stdout = text(&output.stdout);
    assert!(stdout.contains("Start an interactive REPL"), "{stdout}");
    assert!(stdout.contains("aevia help <command>"), "{stdout}");
    assert!(stdout.contains("Exit status:"), "{stdout}");
    assert!(!stdout.contains("no effect"), "{stdout}");
}

#[test]
fn native_commands_have_local_help_without_executing() {
    for command in [
        "run", "repl", "check", "watch", "test", "bench", "fmt", "doc", "init", "add", "update",
        "fetch", "tree", "clean", "lsp", "rssn", "publish", "docs", "pkg",
    ] {
        let output = cli(&[command, "--help", "--color=never"]);
        assert!(
            output.status.success(),
            "{command}: {}",
            text(&output.stderr)
        );
        let stdout = text(&output.stdout);
        assert!(
            stdout.contains(&format!("aevia {command} —")),
            "{command}: {stdout}"
        );
        assert!(stdout.contains("Usage:"), "{command}: {stdout}");
        assert!(stdout.contains("Example"), "{command}: {stdout}");
    }
}

#[test]
fn help_subcommand_matches_help_flag_and_precedes_lock_validation() {
    let direct = cli(&["run", "--help", "--color=never"]);
    let help = cli(&["help", "run", "--color=never"]);
    assert_eq!(text(&direct.stdout), text(&help.stdout));
    let locked = cli(&["--locked", "run", "--help", "--color=never"]);
    assert!(locked.status.success(), "{}", text(&locked.stderr));
    let unknown = cli(&["help", "rnu", "--color=never"]);
    assert_eq!(unknown.status.code(), Some(2));
    assert!(text(&unknown.stderr).contains("unknown command `rnu`"));
}

#[test]
fn explicit_color_policy_applies_to_help_and_usage_errors() {
    let colored = cli(&["--color=always", "--help"]);
    assert!(text(&colored.stdout).contains("\x1b[1;36mUsage:"));
    let plain = cli(&["--color=never", "--help"]);
    assert!(!text(&plain.stdout).contains('\x1b'));
    let usage = cli(&["--budget=not-a-number", "--color=always", "run"]);
    assert_eq!(usage.status.code(), Some(2));
    assert!(text(&usage.stderr).contains("\x1b[31m"));
    let piped = cli(&["--help"]);
    assert!(!text(&piped.stdout).contains('\x1b'));
}

#[test]
fn explicit_color_overrides_environment_and_never_keeps_plain_text() {
    let colored = Command::new(env!("CARGO_BIN_EXE_aevia"))
        .args(["--help", "--color=always"])
        .env("TERM", "dumb")
        .env("NO_COLOR", "1")
        .output()
        .expect("run CLI");
    assert!(colored.status.success());
    assert!(text(&colored.stdout).contains('\x1b'));
    let plain = Command::new(env!("CARGO_BIN_EXE_aevia"))
        .args(["--help", "--color=never"])
        .env("CLICOLOR_FORCE", "1")
        .output()
        .expect("run CLI");
    assert!(!text(&plain.stdout).contains('\x1b'));
}

#[test]
fn program_output_stays_literal_while_cli_statuses_are_colored() {
    let directory = std::env::temp_dir().join(format!("aevia-color-output-{}", std::process::id()));
    std::fs::create_dir_all(&directory).expect("create scratch directory");
    let path = directory.join("main.aev");
    std::fs::write(&path, "fn main(): IO Unit = println(\"literal output\")\n")
        .expect("write program");
    let path = path.to_str().expect("UTF-8 scratch path");
    let program = cli(&["run", path, "--color=always", "--no-cache"]);
    assert!(program.status.success(), "{}", text(&program.stderr));
    assert_eq!(text(&program.stdout), "literal output\n");
    let check = cli(&["check", path, "--color=always"]);
    assert!(check.status.success(), "{}", text(&check.stderr));
    assert!(text(&check.stdout).contains("\x1b[32m"));
    let plain = cli(&["check", path, "--color=never"]);
    assert!(plain.status.success(), "{}", text(&plain.stderr));
    assert!(!text(&plain.stdout).contains('\x1b'));
    assert!(!text(&plain.stderr).contains('\x1b'));
    std::fs::remove_dir_all(directory).expect("remove scratch directory");
}
