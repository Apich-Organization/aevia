#!/usr/bin/env python3
"""Exercise the compiled CLI and its version launchers without network access."""

import os
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile


def test(binary: Path, scratch: Path) -> None:
    home = scratch / "Toolchains with spaces"
    unpacked = scratch / "Archive with spaces"
    unpacked.mkdir()
    suffix = ".exe" if os.name == "nt" else ""

    def cli(*arguments, executable=binary, code=0):
        result = subprocess.run(
            [str(executable), *arguments],
            env=dict(os.environ, AEVIA_HOME=str(home)),
            capture_output=True,
            text=True,
            timeout=45,
        )
        assert result.returncode == code, result
        return result

    def register(tag, *, extras=(), code=0):
        return cli("toolchain", "register", tag, str(unpacked),
                   "--home", str(home), "--channel", "exact", *extras, code=code)

    version = cli("--version").stdout
    assert cli("toolchain", "list").stdout == ""
    for name in ("aevia", "aevia-lsp", "aevia-pkg"):
        shutil.copy2(binary, unpacked / (name + suffix))

    register("v0.1.0-beta.1", extras=("--no-activate",))
    launcher = home / "bin" / ("aevia" + suffix)
    assert cli("toolchain", "list", executable=launcher).stdout.splitlines() == ["  v0.1.0-beta.1"]
    cli("toolchain", "use", "v0.1.0-beta.1", executable=launcher)
    assert cli("--version", executable=launcher).stdout == version
    assert cli("--version", executable=home / "bin" / ("aevia-pkg" + suffix)).stdout == version
    assert cli("toolchain", "list", executable=launcher).stdout.startswith("* v0.1.0-beta.1")
    assert cli("toolchain", "run", "0.1.0-beta.1", "--version", executable=launcher).stdout == version

    nightly = "nightly-2026-11-18"
    register(nightly, extras=("--no-activate",))
    listing = cli("toolchain", "list").stdout.splitlines()
    assert listing == [f"  {nightly}", "* v0.1.0-beta.1 (exact)"], listing
    cli("toolchain", "use", nightly, executable=launcher)
    assert cli("toolchain", "list").stdout.startswith(f"* {nightly} (exact)")
    assert "pinned" in cli("toolchain", "update", executable=launcher, code=2).stderr
    assert "invalid" in register("../outside", code=2).stderr
    assert "invalid" in register("v0.1.0-beta.01", code=2).stderr
    assert "not installed" in cli("toolchain", "use", "1.2.3", code=1).stderr
    (unpacked / ("aevia-pkg" + suffix)).unlink()
    register("1.2.3", code=1)
    assert not (home / "toolchains/v1.2.3").exists()
    assert cli("--version", executable=launcher).stdout == version

    # An existing flat install remains executable and a failed registration
    # must leave it intact.
    unmanaged = scratch / "Legacy install"
    legacy_bin = unmanaged / "bin"
    legacy_bin.mkdir(parents=True)
    legacy_cli = legacy_bin / ("aevia" + suffix)
    shutil.copy2(binary, legacy_cli)
    shutil.copy2(binary, unpacked / ("aevia-pkg" + suffix))
    cli("toolchain", "register", "1.2.4", str(unpacked), "--home", str(unmanaged),
        "--channel", "exact", code=2)
    assert cli("--version", executable=legacy_cli).stdout == version
    print("Verified registration, launcher forwarding, activation, pins and failure recovery")


if __name__ == "__main__":
    with tempfile.TemporaryDirectory(prefix="aevia-toolchain-test-") as directory:
        test(Path(sys.argv[1]).resolve(), Path(directory))
