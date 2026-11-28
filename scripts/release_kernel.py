#!/usr/bin/env python3
"""Select and verify the RSSN revision consumed by the release workflow."""

from __future__ import annotations

import argparse
import re
import subprocess
import tomllib
from pathlib import Path


VERSION = re.compile(r"[0-9]+\.[0-9]+\.[0-9]+(?:-[0-9A-Za-z.-]+)?(?:\+[0-9A-Za-z.-]+)?")


def read_toml(path: Path) -> dict:
    with path.open("rb") as stream:
        return tomllib.load(stream)


def locked_rssn_version(lockfile: Path) -> str:
    matches = [
        package["version"]
        for package in read_toml(lockfile).get("package", [])
        if package.get("name") == "rssn" and "source" not in package
    ]
    if len(matches) != 1 or not VERSION.fullmatch(matches[0]):
        raise ValueError(f"{lockfile} must contain one versioned RSSN path dependency")
    return matches[0]


def select_ref(version: str, requested: str, dry_run: bool) -> str:
    tag = f"refs/tags/v{version}"
    if not requested or requested in (tag, f"v{version}"):
        return tag
    if not dry_run:
        raise ValueError(f"RSSN ref {requested!r} must be {tag} for this release")
    if requested.startswith("refs/tags/") or (
        requested.startswith("v") and VERSION.fullmatch(requested[1:])
    ):
        raise ValueError(f"RSSN tag {requested!r} must match the locked tag {tag}")
    subprocess.run(["git", "check-ref-format", "--allow-onelevel", requested], check=True)
    return requested


def commit_at(checkout: Path, ref: str) -> str:
    return subprocess.check_output(
        ["git", "-C", str(checkout), "rev-parse", "--verify", f"{ref}^{{commit}}"],
        text=True,
    ).strip()


def verify_checkout(checkout: Path, version: str, ref: str) -> str:
    manifest = read_toml(checkout / "Cargo.toml")
    package = manifest["package"]
    package_version = package.get("version")
    if package_version == {"workspace": True}:
        package_version = manifest["workspace"]["package"]["version"]
    if package.get("name") != "rssn" or package_version != version:
        raise ValueError(f"RSSN ref {ref!r} has package version {package_version!r}, expected {version}")

    commit = commit_at(checkout, "HEAD")
    if ref.startswith("refs/tags/"):
        tag = f"refs/tags/v{version}"
        if ref != tag or commit_at(checkout, tag) != commit:
            raise ValueError(f"RSSN checkout must be at the locked tag {tag}")
    return commit


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--lockfile", default=Path("Cargo.lock"), type=Path)
    commands = parser.add_subparsers(dest="command", required=True)
    select = commands.add_parser("select")
    select.add_argument("--ref", default="")
    select.add_argument("--dry-run", default="false", choices=("true", "false"))
    verify = commands.add_parser("verify")
    verify.add_argument("--ref", required=True)
    verify.add_argument("checkout", type=Path)
    args = parser.parse_args()

    try:
        version = locked_rssn_version(args.lockfile)
        if args.command == "select":
            ref = select_ref(version, args.ref, args.dry_run == "true")
            print(f"ref={ref}")
        else:
            print(f"commit={verify_checkout(args.checkout, version, args.ref)}")
    except (KeyError, ValueError, OSError, subprocess.CalledProcessError) as error:
        parser.error(str(error))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
