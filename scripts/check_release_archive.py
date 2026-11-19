#!/usr/bin/env python3
"""Check the Aevia release archive contract used by both installers."""

import hashlib
from pathlib import Path, PurePosixPath
import re
import sys
import tarfile
import zipfile


TARGETS = {
    "x86_64-unknown-linux-gnu",
    "aarch64-unknown-linux-gnu",
    "x86_64-apple-darwin",
    "aarch64-apple-darwin",
    "x86_64-pc-windows-msvc",
    "aarch64-pc-windows-msvc",
}


def verify(archive: Path) -> None:
    match = re.fullmatch(
        r"(aevia-(?:[0-9]+\.[0-9]+\.[0-9]+(?:-[0-9A-Za-z.-]+)?|nightly-[0-9]{4}-[0-9]{2}-[0-9]{2})"
        r"-((?:x86_64|aarch64)-[a-z-]+))\.(zip|tar\.gz)",
        archive.name,
    )
    if not match or match[2] not in TARGETS:
        raise ValueError(f"unexpected release archive name {archive.name}")
    root, target, extension = match.groups()
    windows = target.endswith("-windows-msvc")
    if windows != (extension == "zip"):
        raise ValueError(f"unexpected archive format for {target}")

    sidecar = archive.with_name(archive.name + ".sha256").read_text(encoding="ascii")
    with archive.open("rb") as stream:
        digest = hashlib.file_digest(stream, "sha256").hexdigest()
    if sidecar.split() != [digest, archive.name]:
        raise ValueError(f"checksum sidecar does not match {archive.name}")

    binaries = ["aevia", "aevia-pkg", "aevia-lsp"]
    if target.endswith("-linux-gnu"):
        binaries.append("aevia-playground")
    required = {
        *(name + (".exe" if windows else "") for name in binaries),
        "LICENSE",
        "README.md",
        "README.zh-CN.md",
        "Aevia-Language-Specification.html",
        "Aevia-Language-Specification.en.html",
        "platform/install.sh",
        "platform/install.ps1",
        "platform/stdlib-docs/stdlib-docs.json",
        "examples/kinematics.aev",
    }
    if windows:
        with zipfile.ZipFile(archive) as bundle:
            files = {member.filename for member in bundle.infolist() if not member.is_dir()}
            commit = bundle.read(f"{root}/RSSN-COMMIT")
    else:
        with tarfile.open(archive, "r:gz") as bundle:
            files = {member.name for member in bundle if member.isfile()}
            with bundle.extractfile(f"{root}/RSSN-COMMIT") as stream:
                commit = stream.read()
    if not re.fullmatch(rb"[0-9a-f]{40}\r?\n", commit):
        raise ValueError("RSSN-COMMIT must name a Git commit")
    if any(PurePosixPath(name).parts[0] != root for name in files):
        raise ValueError("archive files must live under the single distribution directory")
    missing = {f"{root}/{name}" for name in required} - files
    if missing:
        raise ValueError(f"archive omitted {', '.join(sorted(missing))}")
    print(f"Verified {archive.name} ({len(files)} files)")


if __name__ == "__main__":
    verify(Path(sys.argv[1]))
