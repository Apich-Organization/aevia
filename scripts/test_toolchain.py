#!/usr/bin/env python3
"""Exercise the compiled CLI and its version launchers without network access."""

import argparse
import os
from pathlib import Path
import shutil
import stat
import subprocess
import tempfile


def test_default_backends(cli, scratch: Path, require_gpu_adapter: bool) -> None:
    report = {}
    for line in cli("--color=never", "devices").stdout.splitlines():
        key, separator, value = line.partition(":")
        if separator:
            report[key.strip()] = value.strip()
    for feature in ("Cranelift", "GPU (wgpu)", "RSSN NumPy"):
        assert report.get(feature) == "enabled", report
    assert report.get("Policy") == "Auto", report
    assert report.get("RSSN backend") in {"tiered", "interpreter"}, report

    root = Path(__file__).resolve().parent.parent
    cli("--jobs", "1", "--caps=", "test", "--no-cache", str(root / "tests/jit_native.aev"), timeout=90)
    example = str(root / "examples/native_backend.aev")
    for selector in ("auto", "cpu", "cpu-parallel", "interpreter"):
        cli("--jobs", "1", "--caps=", "--rssn-backend", selector, "test", "--no-cache", example)
    if report["RSSN backend"] == "tiered":
        cli("--jobs", "1", "--caps=", "--rssn-backend", "cranelift", "test", "--no-cache", example)
    if require_gpu_adapter:
        selectors = report.get("RSSN choices", "").split(", ")
        device = next((name for name in selectors if name.startswith("gpu:wgpu:")), None)
        assert device is not None, report
        for selector in ("gpu", device):
            details = cli("--color=never", "--rssn-backend", selector, "devices").stdout
            assert f"RSSN backend: {device}" in details, details
            for evaluator in ("ast", "ir"):
                cli("--jobs", "1", "--caps=gpu", "--backend", evaluator,
                    "--rssn-backend", selector, "test", "--no-cache", example)
        print("Verified an RSSN wgpu device through Aevia AST and IR")
    program = scratch / "stock-npy.aev"
    program.write_text('''
import Data.Npy as Npy

fn main(): IO Unit = do
    written <- Npy.write_npy("stock-array.npy", [[1.25, -2.0, 3.5], [4.0, 0.0, 6.75]])
    match written with
    | Err(_) -> println("NumPy write failed")
    | Ok(_) -> do
        result <- Npy.read_npy<2>("stock-array.npy")
        match result with
        | Err(_) -> println("NumPy read failed")
        | Ok(data) -> if shape(data) == [2, 3] && data[0, 1] == -2.0 && data[1, 2] == 6.75 then
            println("NPY_DEFAULT_OK")
        else println("NumPy array changed after round-trip")
''', encoding="utf-8")
    result = cli("--jobs", "1", "--caps", "fs", "run", "--no-cache", str(program), cwd=scratch)
    assert result.stdout.strip() == "NPY_DEFAULT_OK", result
    assert (scratch / "stock-array.npy").read_bytes().startswith(b"\x93NUMPY")
    print("Verified default backend selection, the JIT suite, and NumPy I/O")


def test(binary: Path, scratch: Path, require_default_backends: bool = False,
         require_gpu_adapter: bool = False) -> None:
    home = scratch / "Toolchains with spaces"
    unpacked = scratch / "Archive with spaces"
    unpacked.mkdir()
    suffix = ".exe" if os.name == "nt" else ""

    env = dict(os.environ, AEVIA_HOME=str(home))
    env.pop("AEVIA_JOBS", None)
    env.pop("RAYON_NUM_THREADS", None)

    def cli(*arguments, executable=binary, code=0, cwd=None, timeout=45):
        result = subprocess.run(
            [str(executable), *arguments],
            env=env,
            capture_output=True,
            text=True,
            cwd=cwd,
            timeout=timeout,
        )
        assert result.returncode == code, result
        return result

    def register(tag, *, extras=(), code=0):
        return cli("toolchain", "register", tag, str(unpacked),
                   "--home", str(home), "--channel", "exact", *extras, code=code)

    def copy_cli(destination):
        shutil.copy2(binary, destination)
        if os.name == "nt":
            # Bazel's read-only artifact is being copied into a mutable fixture.
            destination.chmod(destination.stat().st_mode | stat.S_IWRITE)

    version = cli("--version").stdout
    if require_default_backends or require_gpu_adapter:
        test_default_backends(cli, scratch, require_gpu_adapter)
    assert cli("toolchain", "list").stdout == ""
    for name in ("aevia", "aevia-lsp", "aevia-pkg"):
        copy_cli(unpacked / (name + suffix))

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
    copy_cli(legacy_cli)
    copy_cli(unpacked / ("aevia-pkg" + suffix))
    cli("toolchain", "register", "1.2.4", str(unpacked), "--home", str(unmanaged),
        "--channel", "exact", code=2)
    assert cli("--version", executable=legacy_cli).stdout == version
    print("Verified registration, launcher forwarding, activation, pins and failure recovery")


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("binary", type=Path)
    parser.add_argument("--require-default-backends", action="store_true")
    parser.add_argument("--require-gpu-adapter", action="store_true")
    args = parser.parse_args()
    with tempfile.TemporaryDirectory(prefix="aevia-toolchain-test-") as directory:
        test(args.binary.resolve(), Path(directory), args.require_default_backends,
             args.require_gpu_adapter)
