#!/usr/bin/env python3
"""Run both installer entry points against local release and CLI fixtures."""

import argparse
import hashlib
import json
import os
from pathlib import Path
import platform
import shutil
import subprocess
import sys
import tarfile
from tempfile import TemporaryDirectory
import zipfile

ROOT = Path(__file__).resolve().parents[1]
REPO = "github.com/example/fixture"
LATEST = "v1.2.3"
BETA = "v2.0.0-beta.1"
NIGHTLY = "nightly-2026-11-20"
WINDOWS = os.name == "nt"

GH = r'''
import hashlib
import json
import os
from pathlib import Path
import shutil
import subprocess
import sys
import zipfile

options = sys.argv[1:]
mode = os.environ["FIXTURE_MODE"]
logs = Path(os.environ["FIXTURE_GH_LOG"])
with logs.open("a", encoding="utf-8") as log:
    log.write(json.dumps(options) + "\n")
fixtures = Path(os.environ["FIXTURE_DIR"])

if options == ["release", "verify-asset", "--help"]:
    sys.exit(0)
if options[:3] == ["api", "--hostname", "github.com"]:
    end = next(arg for arg in options if arg.startswith("repos/example/fixture/"))
    if end.endswith("/latest"):
        latest = None if mode == "stable_absent" else "v1.2.3"
        if latest is not None:
            assert "--jq" in options and "immutable == true" in options[-1]
            print(latest)
        sys.exit(0)
    pages = json.loads((fixtures / "pages.json").read_text())
    if "--paginate" in options:
        assert end.endswith("?per_page=100") and options[-2] == "--jq", options
        for page in pages:
            subprocess.run(["jq", "-r", options[-1]], input=json.dumps(page), text=True, check=True)
    else:
        assert end == "repos/example/fixture/releases" and "per_page=100" in options, options
        page = int(next(arg for arg in options if arg.startswith("page=")).split("=", 1)[1])
        print(json.dumps(pages[page - 1] if page <= len(pages) else []))
elif options[:2] == ["release", "download"]:
    _, _, tag, repo_flag, repo, pattern, archive, second_pattern, sha_name, dest_flag, dest = options
    assert (repo_flag, repo, pattern, second_pattern, sha_name, dest_flag) == (
        "--repo", "github.com/example/fixture", "--pattern", "--pattern", archive + ".sha256", "--dir"
    )
    tag_prefix = "aevia-" + tag.removeprefix("v") + "-"
    assert archive.startswith(tag_prefix), archive
    source = fixtures / archive
    download = Path(dest) / archive
    if mode in {"missing_bundle", "missing_executable", "empty_executable"}:
        with zipfile.ZipFile(source) as original, zipfile.ZipFile(download, "w") as changed:
            for entry in original.infolist():
                relative = Path(entry.filename)
                if mode == "missing_executable" and relative.name == "aevia-pkg.exe":
                    continue
                name = f"other-bundle/{relative.name}" if mode == "missing_bundle" else entry.filename
                content = b"" if mode == "empty_executable" and relative.name == "aevia-lsp.exe" else original.read(entry)
                changed.writestr(name, content)
    else:
        shutil.copyfile(source, download)
    digest = hashlib.sha256(download.read_bytes()).hexdigest()
    if mode == "bad_hash":
        digest = "1" * 64
    checksum_name = "some-other-name" if mode == "wrong_name" else archive
    (Path(dest) / sha_name).write_bytes(f"{digest} *{checksum_name}\r\n".encode())
elif options[:2] == ["release", "verify-asset"]:
    _, _, tag, local_path, repo_switch, repo = options
    assert (repo_switch, repo) == ("--repo", "github.com/example/fixture")
    assert Path(local_path).name.startswith("aevia-" + tag.removeprefix("v") + "-")
    assert Path(local_path).is_file()
    if mode == "bad_attestation":
        sys.exit(8)
else:
    raise AssertionError(options)
'''

SHELL_CLI = r'''#!/bin/sh
set -eu
case "$1" in
    toolchain)
        for arg do printf '%s\n' "$arg"; done >> "$FIXTURE_CLI_LOG"
        ;;
    completions)
        case "$2" in
            bash) printf '%s\n' 'complete -W "run test" aevia' ;;
            zsh) printf '%s\n' '#compdef aevia' ;;
            fish) printf '%s\n' 'complete -c aevia -a run' ;;
            *) exit 7 ;;
        esac
        ;;
    *) exit 7 ;;
esac
'''

WINDOWS_CLI = '''
using System;
using System.IO;
public class FixtureToolchain {
    public static int Main(string[] args) {
        File.AppendAllLines(Environment.GetEnvironmentVariable("FIXTURE_CLI_LOG"), args);
        return 0;
    }
}
'''


def archive_name(tag, target):
    extension = "zip" if WINDOWS else "tar.gz"
    return f"aevia-{tag.removeprefix('v')}-{target}.{extension}"


def release(tag, target, *, immutable=True, draft=False, checksum=True):
    archive = archive_name(tag, target)
    names = [archive, f"{archive}.sha256"] if checksum else [archive]
    return {
        "tag_name": tag,
        "draft": draft,
        "prerelease": tag.startswith("nightly-"),
        "immutable": immutable,
        "assets": [{"name": name, "state": "uploaded"} for name in names],
    }


def create_assets(root, fixture, target, tags=(LATEST, BETA, NIGHTLY)):
    for tag in tags:
        name = archive_name(tag, target)
        bundle = name.removesuffix(".zip").removesuffix(".tar.gz")
        executable_suffix = ".exe" if WINDOWS else ""
        filenames = [f"{program}{executable_suffix}" for program in ("aevia", "aevia-lsp", "aevia-pkg")]
        if WINDOWS:
            with zipfile.ZipFile(root / name, "w") as archive:
                for filename in filenames:
                    archive.write(fixture, f"{bundle}/{filename}")
        else:
            with tarfile.open(root / name, "w:gz") as archive:
                for filename in filenames:
                    archive.add(fixture, f"{bundle}/{filename}")


def prepare(root):
    machine = platform.machine().lower()
    arch = "aarch64" if machine in {"arm64", "aarch64"} else "x86_64"
    system = "pc-windows-msvc" if WINDOWS else ("apple-darwin" if sys.platform == "darwin" else "unknown-linux-gnu")
    target = f"{arch}-{system}"

    gh_file = root / "gh_fixture.py"
    gh_file.write_text(GH, encoding="utf-8")
    if WINDOWS:
        (root / "gh.cmd").write_text(
            '@"%FIXTURE_PYTHON%" "%FIXTURE_GH%" %*\n@exit /b %errorlevel%\n', encoding="ascii"
        )
        fixture = root / "fixture.exe"
        env = os.environ | {"FIXTURE_EXE": str(fixture), "FIXTURE_SOURCE": WINDOWS_CLI}
        subprocess.run(
            ["powershell.exe", "-NoProfile", "-NonInteractive", "-Command",
             "$ErrorActionPreference = 'Stop'; Add-Type -TypeDefinition $env:FIXTURE_SOURCE "
             "-OutputAssembly $env:FIXTURE_EXE -OutputType ConsoleApplication"],
            env=env, check=True, timeout=60,
        )
    else:
        gh = root / "gh"
        gh.write_text('#!/bin/sh\nexec "$FIXTURE_PYTHON" "$FIXTURE_GH" "$@"\n', encoding="ascii")
        gh.chmod(0o755)
        fixture = root / "fixture"
        fixture.write_text(SHELL_CLI, encoding="ascii")
        fixture.chmod(0o755)

    create_assets(root, fixture, target)
    pages = [[
        release("nightly-2026-11-30", target, draft=True),
        release("nightly-2026-11-29", target, immutable=False),
        release("nightly-2026-11-28", target, checksum=False),
        release("nightly-2026-11-27", "another-target"),
        release("nightly-2026-99-99", target),
        release("nightly-2026-11-19", target),
    ] + [release("a-note", target)] * 94, [release(NIGHTLY, target)]]
    (root / "pages.json").write_text(json.dumps(pages), encoding="utf-8")

    return target, os.environ | {
        "PATH": str(root) + os.pathsep + os.environ["PATH"],
        "AEVIA_GITHUB_REPO": REPO.removeprefix("github.com/"),
        "FIXTURE_DIR": str(root),
        "FIXTURE_GH": str(gh_file),
        "FIXTURE_PYTHON": sys.executable,
    }


def run_case(root, env, target, name, selector=(), *, tag=None, channel=None,
             message=None, mode="valid", configure_shell=False, no_activate=False,
             installer_home=None, verify_asset=False):
    scratch = root / name
    scratch.mkdir()
    prefix = scratch / "Aevia with spaces and an 'apostrophe"
    home = scratch / "profile"
    home.mkdir()
    ghrc = scratch / "gh.jsonl"
    cli_log = scratch / "cli.txt"
    args = list(selector)
    if WINDOWS:
        command = ["powershell.exe", "-NoProfile", "-NonInteractive", "-ExecutionPolicy", "Bypass",
                   "-File", str(ROOT / "install.ps1"), "-HomeDirectory", installer_home or str(prefix)]
        args = [value.replace("--channel", "-Channel").replace("--version", "-Version") for value in args]
        if not configure_shell:
            args.append("-NoPath")
        if no_activate:
            args.append("-NoActivate")
    else:
        command = ["sh", ROOT / "install.sh", "--home", prefix]
        if not configure_shell:
            args.append("--no-path")
        if no_activate:
            args.append("--no-activate")
        if configure_shell:
            (home / ".bashrc").touch()
            (home / ".zshrc").touch()
            (home / ".config" / "fish").mkdir(parents=True)
    local = env | {
        "FIXTURE_MODE": mode, "FIXTURE_GH_LOG": str(ghrc), "FIXTURE_CLI_LOG": str(cli_log),
        "HOME": str(home), "SHELL": "/bin/bash", "XDG_CONFIG_HOME": str(home / ".config"),
    }
    result = subprocess.run([*command, *args], env=local, cwd=scratch, text=True, encoding="utf-8",
                            errors="replace", capture_output=True, timeout=90)
    if tag is None:
        assert result.returncode != 0, result
        assert message in (result.stdout + result.stderr).lower(), result
        assert not cli_log.exists(), name
        assert not list(home.iterdir()), name
    else:
        assert result.returncode == 0, result
        actual = cli_log.read_text(encoding="utf-8").splitlines()
        assert actual[:3] == ["toolchain", "register", tag], actual
        bundle = archive_name(tag, target).removesuffix(".zip").removesuffix(".tar.gz")
        assert Path(actual[3]).name == bundle, actual
        flags = ["--home", str(prefix), "--channel", channel]
        if no_activate:
            flags.append("--no-activate")
        assert actual[4:] == flags, actual
        if configure_shell:
            assert (prefix / "completions" / "aevia.bash").is_file()
            assert (prefix / "completions" / "zsh" / "_aevia").is_file()
            assert (home / ".config" / "fish" / "completions" / "aevia.fish").is_file()
            check_home = subprocess.run(
                ["sh", "-c", '. "$HOME/.profile"; printf "%s" "${PATH%%:*}"'],
                env=local, text=True, capture_output=True, check=True,
            )
            assert check_home.stdout == str(prefix / "bin"), check_home
        else:
            assert not list(home.iterdir()), name

    calls = [json.loads(line) for line in ghrc.read_text().splitlines()] if ghrc.exists() else []
    actual_checks = [args for args in calls if args[:2] == ["release", "verify-asset"] and args[-1] == REPO]
    assert len(actual_checks) == int(tag is not None or mode == "bad_attestation" or verify_asset), calls
    if installer_home is not None:
        assert not calls, calls
    if channel == "exact":
        assert all(args[0] != "api" for args in calls), calls
    print(f"{name}: passed", flush=True)


def verify_real_cli(root, env, target, executable):
    # Start with a digest- and attestation-verified fixture containing the
    # actual CLI. The initial update executes the CLI's embedded installer.
    create_assets(root, executable, target, (LATEST,))
    home = root / "Managed home"
    env = env | {
        "AEVIA_HOME": str(home),
        "FIXTURE_MODE": "valid",
        "FIXTURE_GH_LOG": str(root / "real-cli-gh.jsonl"),
    }

    def invoke(command, *args, local=env, status=0):
        result = subprocess.run([str(command), "toolchain", *args], env=local,
                                capture_output=True, text=True, timeout=120)
        assert result.returncode == status, result
        return result.stdout

    invoke(executable, "install", "stable")
    launcher = home / "bin" / ("aevia.exe" if WINDOWS else "aevia")
    listed = invoke(launcher, "list")
    assert listed.splitlines() == [f"* {LATEST} (stable)"], listed
    invoke(launcher, "update")
    invoke(launcher, "update", local=env | {"FIXTURE_MODE": "bad_attestation"}, status=1)
    assert invoke(launcher, "list") == listed
    print("embedded installer, active launcher, and failed update recovery: passed", flush=True)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--aevia", type=Path, help="also exercise the compiled CLI and its embedded installer")
    options = parser.parse_args()
    with TemporaryDirectory(prefix="aevia-install-tests-") as tmp:
        root = Path(tmp)
        target, env = prepare(root)
        run_case(root, env, target, "default-stable", tag=LATEST, channel="stable")
        run_case(root, env, target, "release-tag", ("--version", BETA), tag=BETA, channel="exact")
        run_case(root, env, target, "beta-version", ("--version", BETA[1:]), tag=BETA, channel="exact")
        run_case(root, env, target, "latest-nightly", ("--channel", "nightly"), tag=NIGHTLY, channel="nightly")
        run_case(root, env, target, "latest-dev", ("--channel", "dev"), tag=NIGHTLY, channel="nightly")
        run_case(root, env, target, "dated-nightly", ("--version", NIGHTLY), tag=NIGHTLY, channel="exact")
        run_case(root, env, target, "side-by-side", ("--version", BETA), tag=BETA, channel="exact", no_activate=True)
        run_case(root, env, target, "no-stable", mode="stable_absent", message="no immutable stable release")
        run_case(root, env, target, "bad-hash", mode="bad_hash", message="sha-256 mismatch")
        run_case(root, env, target, "bad-checksum-name", mode="wrong_name", message="invalid checksum")
        run_case(root, env, target, "bad-attestation", mode="bad_attestation", message="attestation verification failed")
        run_case(root, env, target, "invalid-version", ("--version", "../etc"), message="invalid release version")
        run_case(root, env, target, "invalid-semver", ("--version", "2.0.0-01"), message="invalid release version")
        run_case(root, env, target, "invalid-date", ("--version", "nightly-2026-00-14"),
                 message="invalid release version")
        run_case(root, env, target, "multiline-version", ("--version", "1.2.3\n../etc"),
                 message="invalid release version")
        if WINDOWS:
            run_case(root, env, target, "wrong-provider", installer_home="Env:AEVIA_FIXTURE_INSTALL",
                     message="installation path must resolve to the filesystem")
            run_case(root, env, target, "missing-bundle", mode="missing_bundle",
                     message="invalid archive directory", verify_asset=True)
            run_case(root, env, target, "missing-executable", mode="missing_executable",
                     message="invalid executable: aevia-pkg.exe", verify_asset=True)
            run_case(root, env, target, "empty-executable", mode="empty_executable",
                     message="invalid executable: aevia-lsp.exe", verify_asset=True)
        if not WINDOWS:
            run_case(root, env, target, "shell-setup", tag=LATEST, channel="stable", configure_shell=True)
        if options.aevia:
            verify_real_cli(root, env, target, options.aevia.resolve())


if __name__ == "__main__":
    main()
