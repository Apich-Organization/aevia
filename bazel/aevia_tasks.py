"""Aevia compiler, editor and platform workflows."""

import argparse
from contextlib import redirect_stdout
import hashlib
import json
import os
from pathlib import Path
import re
import shutil
import sys
import tempfile
import tomllib
from zipfile import ZIP_DEFLATED, ZipFile

from rssn_tasks import GENERATED, archive_checksum, generate_contracts


_RELEASE_INPUTS = "release-inputs.json"
# Both Linux release runners use Ubuntu 24.04.
_RELEASE_GLIBC = (2, 39)


def bins(build, directory=None):
    path = directory or build.extra[0]
    suffix = ".exe" if "windows" in build.target else ""
    return path / ("aevia" + suffix), path / ("aevia-lsp" + suffix)


def verify_kernel_features(build):
    rssn = build.mirror / "rssn/Cargo.toml"
    bridge = build.root / "crates/aevia-rssn/Cargo.toml"
    features = tomllib.loads(rssn.read_text(encoding="utf-8"))["features"]
    forwarding = tomllib.loads(bridge.read_text(encoding="utf-8"))["features"]["all-kernel-features"]
    pending = [edge.removeprefix("rssn/") for edge in forwarding if edge.startswith("rssn/")]
    enabled = set()
    while pending:
        feature = pending.pop()
        if feature in features and feature not in enabled:
            enabled.add(feature)
            pending.extend(features[feature])
    if missing := features.keys() - enabled:
        raise ValueError(
            f"all-kernel-features in {bridge} must forward every RSSN release feature. "
            f"Missing: {', '.join(sorted(missing))}"
        )


def cargo_profile(build, normal=()):
    if build.release:
        return build.profile(), {}
    if "--release" not in build.settings["build"]:
        return list(normal), {}
    return ["--release", "--all-features"], {
        "CARGO_PROFILE_RELEASE_OPT_LEVEL": "3",
        "CARGO_PROFILE_RELEASE_CODEGEN_UNITS": build.codegen_units,
        "CARGO_PROFILE_RELEASE_LTO": build.lto,
        "CARGO_PROFILE_RELEASE_INCREMENTAL": "false",
    }


def source_digests(inputs):
    projects = {}
    for path, source in sorted(inputs.items()):
        project, _, relative = path.partition("/")
        if project not in projects:
            projects[project] = hashlib.sha256()
        with source.open("rb") as stream:
            content = hashlib.file_digest(stream, "sha256")
        projects[project].update(relative.encode("utf-8") + b"\0" + content.digest())
    return {project: digest.hexdigest() for project, digest in projects.items()}


def artifacts(build, workflow):
    flags, overrides = cargo_profile(build)
    optimized = "--release" in flags
    if workflow == "toolchain":
        features = []
        if optimized:
            verify_kernel_features(build)
            features = ["--features", "aevia-rssn/all-kernel-features"]
        build.cargo(["build", "-p", "aevia-cli", "-p", "aevia-lsp", *flags, *features], env=overrides)
        names = ["aevia", "aevia-lsp"]
    else:
        names = ["aevia-pkg"]
        packages = ["-p", "aevia-pkg"]
        if workflow == "platform_binaries":
            packages += ["-p", "aevia-registry-server"]
            names += ["aevia-playground", "aevia-registry"]
        build.cargo(["build", *packages, *flags], cwd=build.root / "platform", env=overrides)
    suffix = ".exe" if "windows" in build.target else ""
    profile = "release" if optimized else None
    build.copy_artifacts("bin", build.cargo_output(profile), [name + suffix for name in names])
    if optimized:
        receipt = build.outputs["bin"] / _RELEASE_INPUTS
        staged = {path: build.mirror / path for path in build.inputs}
        receipt.write_text(json.dumps(source_digests(staged), sort_keys=True) + "\n", encoding="utf-8")


def release_smoke(build, directory):
    build.assert_native()
    cli, lsp = bins(build, directory)
    suffix = cli.suffix
    build.run([cli, "--version"])
    build.run([directory / ("aevia-pkg" + suffix), "--version"])
    flags = ["--require-default-backends"]
    if build.target.endswith("-linux-gnu"):
        icd_directory = Path("/usr/share/vulkan/icd.d")
        architecture = build.target.split("-", 1)[0]
        candidate_names = (f"lvp_icd.{architecture}.json", "lvp_icd.json")
        icd = next((path for name in candidate_names if (path := icd_directory / name).is_file()
                    and path.stat().st_size), None)
        if icd is None:
            raise ValueError("Install mesa-vulkan-drivers for the Linux release GPU smoke test")
        flags.append("--require-gpu-adapter")
        with tempfile.TemporaryDirectory(prefix="aevia-vulkan-", dir=build.scratch) as runtime:
            build.python("scripts/test_toolchain.py", cli, *flags,
                         env={"VK_DRIVER_FILES": str(icd), "XDG_RUNTIME_DIR": runtime})
    else:
        build.python("scripts/test_toolchain.py", cli, *flags)
    build.run(["node", "editors/test/lsp-smoke.js", lsp, "--initialize-only"])
    build.run(["node", "editors/test/dap-smoke.js", cli])
    build.run(["node", "editors/test/dap-smoke.js", cli, "--exception"])
    for backend in ("ast", "ir"):
        for suite in ("acoustic_storage", "hyperelastic_observer", "native_split_step", "multiprecision"):
            build.run([cli, "--jobs", "1", "--caps=", "--backend", backend,
                       "test", "--no-cache", f"tests/domain_{suite}.aev"])


_RELEASE_FILES = (
    "LICENSE", "README.md", "README.zh-CN.md", "ARCHITECTURE.md", "CHANGELOG.md", "SECURITY.md",
    "Aevia-Language-Specification.md", "Aevia-Language-Specification.html",
    "Aevia-Language-Specification.en.md", "Aevia-Language-Specification.en.html",
    "spec", "examples", "editors", "platform/README.md", "platform/install.sh", "platform/install.ps1",
    "platform/docs", "platform/infra", "platform/manuals", "platform/stdlib-docs",
)
_RELEASE_GUIDES = ("BAZEL.md", "BAZEL.zh-CN.md")


def verify_release_archive(build, archive, *, require_guides=False):
    flags = ["--require-build-guides"] if require_guides else []
    build.python("scripts/check_release_archive.py", archive, *flags)
    with tempfile.TemporaryDirectory(prefix="aevia-extract-", dir=build.scratch) as temp:
        if archive.suffix == ".zip":
            shutil.unpack_archive(archive, temp)
            name = archive.stem
        else:
            shutil.unpack_archive(archive, temp, filter="data")
            name = archive.name.removesuffix(".tar.gz")
        release_smoke(build, Path(temp) / name)


def resolve_commit(build, checkout, revision):
    return build.run(
        ["git", "rev-parse", "--verify", "--end-of-options", f"{revision}^{{commit}}"],
        cwd=checkout, capture_output=True, text=True,
    ).stdout.strip()


def verify_release_inputs(build, aevia_commit, rssn_commit, toolchain, platform):
    for project, commit in (("aevia", aevia_commit), ("rssn", rssn_commit)):
        checkout = build.checkout(project)
        if resolve_commit(build, checkout, "HEAD") != commit:
            raise ValueError(f"{project} must be checked out at {commit} for this release")
        changes = build.run(
            ["git", "status", "--porcelain=v1", "--untracked-files=all"],
            cwd=checkout, capture_output=True, text=True,
        ).stdout
        if changes:
            raise ValueError(f"Create a clean {project} checkout for the release. Local changes:\n{changes}")
        tracked = set(build.run(
            ["git", "ls-files", "--cached", "-z"],
            cwd=checkout, capture_output=True, text=True,
        ).stdout.split("\0"))
        prefix = project + "/"
        untracked_inputs = sorted(
            path for path in build.inputs if path.startswith(prefix) and path.removeprefix(prefix) not in tracked
        )
        if untracked_inputs:
            raise ValueError(f"Release inputs must be tracked by Git: {', '.join(untracked_inputs)}")

    digests = source_digests(build.inputs)
    # Package actions have their own source mirror. Compare with the snapshots
    # actually staged for each build so an intervening checkout cannot relabel an artifact.
    for directory, expected in ((toolchain, digests), (platform, {"aevia": digests["aevia"]})):
        receipt = directory / _RELEASE_INPUTS
        if not receipt.is_file() or json.loads(receipt.read_text(encoding="utf-8")) != expected:
            raise ValueError(f"{directory} was not built from the selected release sources")


def verify_linux_release_abi(build, binaries):
    for binary in binaries:
        versions = build.run(
            ["readelf", "--version-info", "--wide", binary],
            capture_output=True, text=True, env={"LC_ALL": "C"},
        ).stdout
        # Weak version references do not prevent the loader from starting.
        strong = re.findall(r"\bName: GLIBC_(\d+(?:\.\d+)+)\s+Flags: none\b", versions)
        requirements = {tuple(map(int, version.split("."))) for version in strong}
        unsupported = sorted(version for version in requirements if version > _RELEASE_GLIBC)
        if unsupported:
            names = ", ".join("GLIBC_" + ".".join(map(str, version)) for version in unsupported)
            raise ValueError(f"{binary.name} requires {names}. Build the Linux release with the Ubuntu 24.04 ABI")


def package_release(build, args, *, from_binaries=False):
    build.assert_native()
    parser = argparse.ArgumentParser(description="Package and verify a native Aevia distribution")
    parser.add_argument("--distribution", required=True)
    parser.add_argument("--rssn-commit", required=True)
    parser.add_argument("--source-commit", default="HEAD")
    parser.add_argument("--dist-dir", type=Path, default=Path("dist"))
    if not from_binaries:
        parser.add_argument("--registry-image", action="store_true")
    if from_binaries:
        parser.add_argument("--toolchain-bin", type=Path, required=True)
        parser.add_argument("--platform-bin", type=Path, required=True)
    options = parser.parse_args(args)
    if not re.fullmatch(r"[0-9]+\.[0-9]+\.[0-9]+(?:-[0-9A-Za-z.-]+)?|nightly-\d{4}-\d{2}-\d{2}", options.distribution):
        parser.error("--distribution must be a release version or nightly-YYYY-MM-DD")
    if not re.fullmatch(r"[0-9a-f]{40}", options.rssn_commit):
        parser.error("--rssn-commit must be a 40-character Git commit")
    cwd = Path(os.getenv("BUILD_WORKING_DIRECTORY", str(build.workspace)))
    commit = resolve_commit(build, build.workspace, options.source_commit)
    if from_binaries:
        toolchain = (cwd / options.toolchain_bin).resolve()
        platform = (cwd / options.platform_bin).resolve()
        guides = build.run(
            ["git", "ls-tree", "--name-only", commit, "--", *_RELEASE_GUIDES],
            cwd=build.workspace, capture_output=True, text=True,
        ).stdout.splitlines()
    else:
        if (build.lto, build.codegen_units) != ("fat", "1"):
            raise ValueError("Run //:package_release with --config=release-max")
        toolchain, platform = build.extra
        verify_release_inputs(build, commit, options.rssn_commit, toolchain, platform)
        guides = _RELEASE_GUIDES
    out = (cwd / options.dist_dir).resolve()
    out.mkdir(parents=True, exist_ok=True)
    name = f"aevia-{options.distribution}-{build.target}"
    windows = build.target.endswith("-windows-msvc")
    linux = build.target.endswith("-linux-gnu")
    ship_registry = linux and (not from_binaries or build.target.startswith("x86_64-"))
    with_registry_image = not from_binaries and options.registry_image
    if with_registry_image and not linux:
        parser.error("--registry-image requires a native Linux distribution")
    suffix = ".exe" if windows else ""
    with tempfile.TemporaryDirectory(prefix="aevia-package-", dir=out) as temp:
        temporary = Path(temp)
        stage = temporary / name
        stage.mkdir()
        for directory, names in ((toolchain, ("aevia", "aevia-lsp")), (platform, ("aevia-pkg",))):
            for binary in names:
                shutil.copy2(directory / (binary + suffix), stage)
        if linux:
            shutil.copy2(platform / "aevia-playground", stage)
            if ship_registry:
                shutil.copy2(platform / "aevia-registry", stage)
            if not from_binaries:
                verify_linux_release_abi(build, sorted(stage.iterdir()))
        (stage / "RSSN-COMMIT").write_text(f"{options.rssn_commit}\n", encoding="ascii", newline="\n")
        sources = temporary / "sources.tar"
        with sources.open("wb") as stream:
            build.run(["git", "archive", "--format=tar", commit, "--", *_RELEASE_FILES, *guides],
                      cwd=build.workspace, stdout=stream)
        shutil.unpack_archive(sources, stage, filter="data")
        sources.unlink()
        archive = Path(shutil.make_archive(str(temporary / name), "zip" if windows else "gztar",
                                           root_dir=temporary, base_dir=name))
        with archive.open("rb") as stream:
            digest = hashlib.file_digest(stream, "sha256").hexdigest()
        checksum = archive.with_name(archive.name + ".sha256")
        checksum.write_text(f"{digest}  {archive.name}\n", encoding="ascii", newline="\n")
        verify_release_archive(build, archive, require_guides=not from_binaries)
        if ship_registry:
            registry = temporary / "aevia-registry"
            shutil.copy2(platform / "aevia-registry", registry)
            registry_files = [registry]
            if with_registry_image:
                variant = f"linux-{build.target.split('-', 1)[0]}"
                image_archive = temporary / f"aevia-registry-{variant}.tar"
                registry_image(build, ["--registry", str(registry), "--tag", f"aevia-registry:{variant}",
                                       "--archive", str(image_archive)])
                registry_files += [image_archive, image_archive.with_suffix(".tar.sha256")]
            registry_out = out / "registry"
            registry_out.mkdir(exist_ok=True)
            for item in registry_files:
                item.replace(registry_out / item.name)
        archive.replace(out / archive.name)
        checksum.replace(out / checksum.name)
    print(out / archive.name)


def registry_image(build, args):
    build.assert_native()
    if not build.target.endswith("-linux-gnu"):
        raise ValueError("The registry image requires a native Linux build")
    parser = argparse.ArgumentParser(description="Package a Bazel-built registry binary in a Linux image")
    parser.add_argument("--registry", type=Path)
    parser.add_argument("--tag", required=True)
    parser.add_argument("--archive", type=Path)
    options = parser.parse_args(args)
    caller = Path(os.getenv("BUILD_WORKING_DIRECTORY", str(build.workspace)))
    if options.registry:
        registry = (caller / options.registry).resolve(strict=True)
    else:
        directory, = build.extra
        registry = directory / "aevia-registry"
    platform = build.root / "platform"
    with tempfile.TemporaryDirectory(prefix="aevia-registry-image-", dir=build.scratch) as temporary:
        context = Path(temporary)
        binary = context / "aevia-registry"
        shutil.copyfile(registry, binary)
        binary.chmod(0o755)
        build.run(["docker", "build", "--target", "optimized-runtime",
                   "--build-context", f"optimized={context}", "-f", "infra/Containerfile.server",
                   "--tag", options.tag, "."], cwd=platform)
    result = build.run(["docker", "run", "--rm", "--network=none", options.tag], cwd=platform,
                       capture_output=True, text=True, check=False)
    if result.returncode != 2 or "DATABASE_URL" not in (result.stdout + result.stderr):
        raise ValueError(f"Registry startup smoke failed ({result.returncode}):\n{result.stdout}{result.stderr}")
    if options.archive:
        archive = (caller / options.archive).resolve()
        archive.parent.mkdir(parents=True, exist_ok=True)
        build.run(["docker", "save", "--output", archive, options.tag], cwd=platform)
        archive_checksum(archive)


def platform_test(build):
    build.assert_native()
    if not build.env.get("AEVIA_TEST_DATABASE_URL"):
        raise ValueError("AEVIA_TEST_DATABASE_URL is required for PostgreSQL integration tests")
    root = build.root / "platform"
    build.run(["sh", "infra/check-lang-workspace.sh", ".."], cwd=root)
    # The GnuPG socket path may be shorter than Bazel's TEST_TMPDIR.
    with tempfile.TemporaryDirectory(prefix="aevia-gpg-", dir="/tmp" if os.name != "nt" else None) as temporary:
        flags, overrides = cargo_profile(build, ["--all-features"])
        env = {**overrides, "GNUPGHOME": temporary, "AEVIA_REQUIRE_TEST_FIXTURES": "1"}
        build.run([
            "gpg", "--batch", "--pinentry-mode", "loopback", "--passphrase", "",
            "--quick-generate-key", "Aevia Bazel Test <test@aevia.invalid>", "ed25519", "cert", "1d",
        ], env=env)
        listing = build.run(["gpg", "--batch", "--with-colons", "--list-secret-keys"], env=env,
                            capture_output=True, text=True)
        fingerprint = next(line.split(":")[9] for line in listing.stdout.splitlines() if line.startswith("fpr:"))
        build.run([
            "gpg", "--batch", "--pinentry-mode", "loopback", "--passphrase", "",
            "--quick-add-key", fingerprint, "ed25519", "sign", "1d",
        ], env=env)
        try:
            build.cargo(["test", "--workspace", *flags], cwd=root, env=env)
            release = ["--release"] if "--release" in flags else []
            build.cargo(["test", *release, "-p", "aevia-registry-docs", "--no-default-features"], cwd=root, env=env)
        finally:
            build.run(["gpgconf", "--kill", "gpg-agent"], env=env, check=False)


def editors(build):
    build.python("editors/test/configs.py")
    build.run(["bash", "-n", "editors/helix/scripts/install.sh"])
    build.run(["bash", "editors/helix/test/install.sh"])
    tree = build.root / "editors/tree-sitter-aevia"
    build.run(["node", "--check", "grammar.js"], cwd=tree)
    build.run(["cc", "-std=c11", "-fsyntax-only", "-I", "src", "src/parser.c", "src/scanner.c"], cwd=tree)
    build.run([
        "vim", "-Nu", "NONE", "-i", "NONE", "-n", "-es",
        "--cmd", f"set runtimepath^={build.root / 'editors/vim'}",
        "--cmd", "filetype plugin indent on", "--cmd", "syntax enable", "examples/symbolic.aev",
        "-c", "if &filetype !=# 'aevia' || &syntax !=# 'aevia' || exists(':AeviaRun') != 2 || exists(':AeviaTest') != 2 || exists(':AeviaRepl') != 2 | cquit | endif",
        "-c", "quit",
    ])
    build.run([
        "emacs", "--batch", "-Q", "-l", "editors/emacs/aevia-mode.el", "examples/symbolic.aev",
        "--eval", "(unless (and (derived-mode-p 'aevia-mode) (commandp 'aevia-run-file) (commandp 'aevia-test-file) (commandp 'aevia-repl)) (error \"Aevia mode failed to register\"))",
    ])

def zed_wasm(build):
    root = build.root / "editors/zed"
    build.cargo(["clippy", "--target", "wasm32-wasip2", "--all-targets", "--", "-D", "warnings"], cwd=root, target=False)
    build.cargo(["build", "--target", "wasm32-wasip2"], cwd=root, target=False)


def editor_packages(build, args):
    vscode = build.root / "editors/vscode"
    jetbrains = build.root / "editors/jetbrains"
    with tempfile.TemporaryDirectory(dir=build.scratch) as temporary:
        destination = Path(temporary) / "extensions"
        destination.mkdir()
        build.run(["npm", "ci", "--ignore-scripts"], cwd=vscode)
        build.run(["node", "--check", "extension.js"], cwd=vscode)
        build.run(["npm", "test"], cwd=vscode)
        build.run(["node", "-e", 'JSON.parse(require("node:fs").readFileSync("syntaxes/aevia.tmLanguage.json", "utf8"))'],
                  cwd=vscode)
        build.run(["npm", "run", "package", "--", "--out", destination], cwd=vscode)
        build.run(["gradle", "--no-daemon", "buildPlugin", "verifyPluginStructure"], cwd=jetbrains)
        for package in (jetbrains / "build/distributions").glob("*.zip"):
            shutil.copy2(package, destination)
        build.python("editors/test/packages.py", destination)
        if not os.getenv("TEST_TMPDIR"):
            zed = build.root / "editors/zed"
            build.cargo(["build", "--release", "--target", "wasm32-wasip2"], cwd=zed, target=False, env={
                "CARGO_PROFILE_RELEASE_OPT_LEVEL": "3",
                "CARGO_PROFILE_RELEASE_LTO": build.lto,
                "CARGO_PROFILE_RELEASE_CODEGEN_UNITS": build.codegen_units,
                "CARGO_PROFILE_RELEASE_INCREMENTAL": "false",
            })
            wasm = build.target_dir / "wasm32-wasip2/release/zed_aevia.wasm"
            if not wasm.is_file():
                raise ValueError("Zed extension was not built as a WebAssembly component")
            with wasm.open("rb") as binary:
                if binary.read(8) != b"\0asm\x0d\0\x01\0":
                    raise ValueError("Zed output is not a WebAssembly component")
            version = tomllib.loads((build.root / "Cargo.toml").read_text(encoding="utf-8"))["workspace"]["package"]["version"]
            for editor in ("emacs", "helix", "kate", "sublime", "tree-sitter-aevia", "vim", "zed"):
                directory = build.root / "editors" / editor
                archive = destination / f"aevia-{editor}-{version}.zip"
                with ZipFile(archive, "w", compression=ZIP_DEFLATED) as package:
                    for source in sorted(directory.rglob("*")):
                        relative = source.relative_to(directory)
                        if source.is_file() and not set(relative.parts) & {"target", "node_modules", "test", "extension.wasm"}:
                            package.write(source, relative.as_posix())
                    if editor == "zed":
                        package.write(wasm, "extension.wasm")
                with ZipFile(archive) as package:
                    if not package.namelist() or package.testzip():
                        raise ValueError(f"Invalid editor package: {archive}")
            out = Path(args[0]).absolute() if args else build.workspace / "dist/editor-extensions"
            out.mkdir(parents=True, exist_ok=True)
            for package in list(destination.iterdir()):
                shutil.copy2(package, out)
                shutil.copy2(archive_checksum(package), out)


def contracts(build):
    cli, _ = bins(build)
    build.tool("aevia-stdlib-gen", "--check")
    build.tool("rssn-api-audit", "--codecs-check")
    with tempfile.TemporaryDirectory(dir=build.scratch) as directory:
        manifest = Path(directory) / "rssn-manifest.json"
        with manifest.open("wb") as stream:
            build.run([cli, "rssn", "--manifest-json"], stdout=stream)
        build.tool("rssn-api-audit", "--verify", "--manifest", manifest)
        build.tool("rssn-api-audit", "--domains-check", "--manifest", manifest)
    build.python("tools/check_stdlib_docs.py", "--aevia", cli)
    build.python("tools/bundle_spec.py", "--check")
    build.python("tools/check_manuals.py", "--aevia", cli)
    for example in ("sensor_calibration", "relaxation_fit"):
        build.run([cli, "--caps=", "--jobs", "1", "test", f"examples/{example}.aev"])


def refresh_stdlib(build):
    build.tool("aevia-stdlib-gen", "--write")


def dispatch(build, workflow, args):
    if workflow in ("toolchain", "platform_cli", "platform_binaries"):
        artifacts(build, workflow)
    elif workflow in ("package-release", "package-release-from-binaries"):
        package_release(build, args, from_binaries=workflow.endswith("-from-binaries"))
    elif workflow == "registry-image":
        registry_image(build, args)
    elif workflow == "release-inputs-test":
        module_paths = [str(Path(__file__).absolute().parent), sys.path[0]]
        build.python("scripts/test_release_inputs.py", env={"PYTHONPATH": os.pathsep.join(module_paths)})
    elif workflow == "release-smoke":
        parser = argparse.ArgumentParser(description="Verify an Aevia release archive or staged binaries")
        source = parser.add_mutually_exclusive_group(required=True)
        source.add_argument("--archive", type=Path)
        source.add_argument("--bin-dir", type=Path)
        inputs = parser.parse_args(args)
        cwd = Path(os.getenv("BUILD_WORKING_DIRECTORY", str(build.workspace)))
        if inputs.archive:
            verify_release_archive(build, (cwd / inputs.archive).resolve())
        else:
            release_smoke(build, (cwd / inputs.bin_dir).resolve())
    elif workflow == "check":
        build.cargo(["check", *(args or ["--workspace", "--all-targets"])])
    elif workflow in ("clippy", "platform-clippy"):
        root = build.root / "platform" if workflow == "platform-clippy" else build.root
        build.cargo(["clippy", "--workspace", "--all-targets", "--all-features", "--", "-D", "warnings"], cwd=root)
    elif workflow == "rust-tests":
        build.assert_runnable()
        flags, overrides = cargo_profile(build, build.settings["build"])
        if "--all-features" in flags or "--all-features" in args:
            verify_kernel_features(build)
        build.cargo(["test", "--workspace", *flags, *args], env=overrides)
    elif workflow == "platform-test":
        platform_test(build)
    elif workflow == "platform-scripts":
        build.python("platform/scripts/test_dev.py")
        build.python("platform/scripts/test_deploy.py")
        build.run(["sh", "platform/infra/check-lang-workspace.sh", "."])
    elif workflow == "editors":
        editors(build)
    elif workflow == "editor-kate":
        build.python("editors/kate/test/check_strings.py", *args)
    elif workflow == "editor-zed-runnables":
        build.python("editors/zed/test/check_runnables.py", *args)
    elif workflow == "editor-zed-wasm":
        zed_wasm(build)
    elif workflow == "editor-protocol":
        build.assert_native()
        cli, lsp = bins(build)
        build.run(["node", "editors/test/lsp-smoke.js", lsp])
        build.run(["node", "editors/test/dap-smoke.js", cli])
        build.run(["node", "editors/test/dap-smoke.js", cli, "--exception"])
    elif workflow == "editor-packages":
        editor_packages(build, args)
    elif workflow == "toolchain-test":
        build.assert_native()
        cli, _ = bins(build)
        build.python("platform/scripts/test_installers.py", "--aevia", cli)
        build.python("scripts/test_toolchain.py", cli)
    elif workflow == "contracts":
        build.assert_native()
        contracts(build)
    elif workflow == "refresh-kernel":
        build.assert_native()
        with build.refresh([*GENERATED, "aevia/tools/rssn-api-audit/inventory.tsv",
                            "aevia/tools/rssn-api-audit/RSSN-COVERAGE.md",
                            "aevia/crates/aevia-std/stdlib/**/*.aev",
                            "aevia/crates/aevia-std/src/stdlib_table.rs"]):
            generate_contracts(build, "write")
            build.tool("rssn-api-audit", "--write-checked")
            refresh_stdlib(build)
            build.cargo(["test", "-p", "aevia-rssn", "--test", "domains"], env={"AEVIA_BLESS": "1"})
    elif workflow == "refresh-stdlib":
        with build.refresh(["aevia/crates/aevia-std/stdlib/**/*.aev", "aevia/crates/aevia-std/src/stdlib_table.rs"]):
            refresh_stdlib(build)
    elif workflow == "refresh-spec":
        with build.refresh(["aevia/Aevia-Language-Specification*"]):
            build.python("tools/bundle_spec.py", *args)
    elif workflow == "cli":
        build.assert_native()
        cli, _ = bins(build)
        # Keep the runner's command trace off the CLI's stdout.
        with redirect_stdout(sys.stderr):
            build.run([cli, *args], cwd=Path(os.getenv("BUILD_WORKING_DIRECTORY", str(build.workspace))))
    else:
        raise ValueError(f"unknown Aevia workflow: {workflow}")
