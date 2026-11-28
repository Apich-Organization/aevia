"""Regression tests for the source and ABI checks on release artifacts."""

import hashlib
import json
from pathlib import Path
import subprocess
import sys
from tempfile import TemporaryDirectory
from types import SimpleNamespace
import unittest
from unittest.mock import Mock, patch
from zipfile import ZipFile

from aevia_tasks import (
    dispatch,
    package_release,
    release_smoke,
    source_digests,
    verify_linux_release_abi,
    verify_release_archive,
    verify_release_distribution,
    verify_release_inputs,
)
from check_release_archive import verify as check_release_archive


class ReleaseInputsTest(unittest.TestCase):
    def setUp(self):
        temporary = TemporaryDirectory()
        self.addCleanup(temporary.cleanup)
        root = Path(temporary.name)
        self.checkouts = {name: root / name for name in ("aevia", "rssn")}
        self.inputs = {}
        self.commits = {}
        for project, checkout in self.checkouts.items():
            checkout.mkdir()
            self.git(project, "init", "-q")
            self.git(project, "config", "user.name", "Aevia release test")
            self.git(project, "config", "user.email", "test@example.invalid")
            source = checkout / "Cargo.toml"
            source.write_text('[workspace.package]\nversion = "0.1.0-beta.1"\n', encoding="utf-8")
            self.inputs[f"{project}/Cargo.toml"] = source
            self.git(project, "add", "Cargo.toml")
            self.git(project, "commit", "-qm", "Initial release inputs")
            self.commits[project] = self.git(project, "rev-parse", "HEAD")
        self.toolchain = root / "toolchain"
        self.platform = root / "platform"
        self.toolchain.mkdir()
        self.platform.mkdir()
        digests = source_digests(self.inputs)
        for directory, receipt in ((self.toolchain, digests), (self.platform, {"aevia": digests["aevia"]})):
            (directory / "release-inputs.json").write_text(json.dumps(receipt), encoding="utf-8")

    def git(self, project, *args):
        return subprocess.run(
            ["git", *args], cwd=self.checkouts[project], check=True,
            text=True, capture_output=True,
        ).stdout.strip()

    def verify(self):
        build = SimpleNamespace(inputs=self.inputs, checkout=self.checkouts.__getitem__,
                                run=lambda args, **kwargs: subprocess.run(args, check=True, **kwargs))
        verify_release_inputs(build, self.commits["aevia"], self.commits["rssn"],
                              self.toolchain, self.platform)

    def test_accepts_matching_clean_inputs(self):
        self.verify()

    def test_rejects_a_dirty_source_checkout(self):
        self.inputs["rssn/Cargo.toml"].write_text("# dirty\n", encoding="utf-8")
        with self.assertRaisesRegex(ValueError, "clean rssn checkout"):
            self.verify()

    def test_rejects_untracked_source_inputs(self):
        (self.checkouts["aevia"] / "Cargo.local.toml").write_text("# untracked\n", encoding="utf-8")
        with self.assertRaisesRegex(ValueError, "clean aevia checkout"):
            self.verify()

    def test_rejects_ignored_files_selected_as_release_inputs(self):
        checkout = self.checkouts["rssn"]
        (checkout / ".git/info/exclude").write_text("/Cargo.lock\n", encoding="utf-8")
        lockfile = checkout / "Cargo.lock"
        lockfile.write_text("# ignored lockfile\n", encoding="utf-8")
        self.assertEqual(self.git("rssn", "status", "--porcelain"), "")
        self.verify()  # An ignored file outside the build inputs is harmless.
        self.inputs["rssn/Cargo.lock"] = lockfile
        with self.assertRaisesRegex(ValueError, r"tracked by Git: rssn/Cargo\.lock"):
            self.verify()

    def test_rejects_an_unrelated_revision(self):
        self.commits["rssn"] = "0" * 40
        with self.assertRaisesRegex(ValueError, "rssn must be checked out"):
            self.verify()

    def test_rejects_artifacts_left_over_from_an_older_clean_revision(self):
        self.inputs["aevia/Cargo.toml"].write_text("# different source\n", encoding="utf-8")
        self.git("aevia", "commit", "-qam", "Advance the source revision")
        self.commits["aevia"] = self.git("aevia", "rev-parse", "HEAD")
        with self.assertRaisesRegex(ValueError, "was not built from the selected release sources"):
            self.verify()

    def test_distribution_comes_from_the_selected_commit(self):
        checkout = self.checkouts["aevia"]
        commit = self.commits["aevia"]
        manifest = checkout / "Cargo.toml"
        manifest.write_text('[workspace.package]\nversion = "0.2.0"\n', encoding="utf-8")
        self.git("aevia", "commit", "-qam", "Advance the Aevia version")
        build = SimpleNamespace(workspace=checkout,
                                run=lambda args, **kwargs: subprocess.run(args, check=True, **kwargs))
        self.assertEqual(verify_release_distribution(build, commit, "0.1.0-beta.1"), "0.1.0-beta.1")
        self.assertEqual(verify_release_distribution(build, commit, "nightly-2026-11-28"), "0.1.0-beta.1")
        with self.assertRaisesRegex(ValueError, "valid calendar date"):
            verify_release_distribution(build, commit, "nightly-2026-02-30")

    def test_packaging_stops_before_writing_a_mislabeled_release(self):
        checkout = self.checkouts["aevia"]
        output = checkout.parent / "dist"
        build = SimpleNamespace(workspace=checkout, assert_native=lambda: None,
                                run=lambda args, **kwargs: subprocess.run(args, check=True, **kwargs))
        with self.assertRaisesRegex(ValueError, r"Distribution 9\.9\.9 must match the Aevia version"):
            package_release(build, ["--distribution", "9.9.9", "--rssn-commit", self.commits["rssn"],
                                    "--dist-dir", str(output)])
        self.assertFalse(output.exists())


class ReleaseArchiveTest(unittest.TestCase):
    def setUp(self):
        self.temporary = TemporaryDirectory()
        self.addCleanup(self.temporary.cleanup)
        self.scratch = Path(self.temporary.name)
        self.target = "x86_64-pc-windows-msvc"
        self.commit = "a" * 40

    def archive(self, distribution):
        directory = f"aevia-{distribution}-{self.target}"
        archive = self.scratch / (directory + ".zip")
        with ZipFile(archive, "w") as bundle:
            for name in [
                "aevia.exe", "aevia-pkg.exe", "aevia-lsp.exe", "LICENSE", "README.md", "README.zh-CN.md",
                "Aevia-Language-Specification.html", "Aevia-Language-Specification.en.html", "platform/install.sh",
                "platform/install.ps1", "platform/stdlib-docs/stdlib-docs.json", "examples/kinematics.aev",
            ]:
                bundle.writestr(f"{directory}/{name}", "data")
            bundle.writestr(f"{directory}/RSSN-COMMIT", f"{self.commit}\n")
        digest = hashlib.sha256(archive.read_bytes()).hexdigest()
        archive.with_name(archive.name + ".sha256").write_text(f"{digest}  {archive.name}\n", encoding="ascii")
        return archive

    def build(self):
        def python(_script, *args):
            return subprocess.run(
                [sys.executable, Path(__file__).with_name("check_release_archive.py"), *args],
                capture_output=True, check=True, text=True,
            )

        return SimpleNamespace(target=self.target, scratch=self.scratch, workspace=self.scratch, python=python)

    def test_rejects_the_wrong_rssn_revision_or_target(self):
        archive = self.archive("0.1.0-beta.1")
        check_release_archive(archive, expected_target=self.target, expected_rssn_commit=self.commit)
        with self.assertRaisesRegex(ValueError, "Archive RSSN commit"):
            check_release_archive(archive, expected_rssn_commit="b" * 40)
        with self.assertRaisesRegex(ValueError, "Archive targets"):
            check_release_archive(archive, expected_target="aarch64-pc-windows-msvc")

    def test_archive_smoke_verifies_revisions_before_execution(self):
        build = self.build()
        archive = self.archive("0.1.0-beta.1")
        with patch("aevia_tasks.release_smoke") as smoke:
            args = ["--archive", str(archive), "--rssn-commit"]
            with self.assertRaises(subprocess.CalledProcessError) as error:
                dispatch(build, "release-smoke", [*args, "b" * 40])
            self.assertIn("Archive RSSN commit", error.exception.stderr)
            smoke.assert_not_called()
            dispatch(build, "release-smoke", [*args, self.commit])
            self.assertEqual(smoke.call_args.kwargs, {"expected_version": "0.1.0-beta.1"})

    def test_nightly_archive_smoke_can_check_the_committed_version(self):
        build = self.build()
        archive = self.archive("nightly-2026-11-28")
        with patch("aevia_tasks.release_smoke") as smoke:
            verify_release_archive(build, archive, rssn_commit=self.commit, expected_version="0.1.0-beta.1")
            self.assertEqual(smoke.call_args.kwargs, {"expected_version": "0.1.0-beta.1"})
            verify_release_archive(build, archive)
            self.assertEqual(smoke.call_args.kwargs, {"expected_version": None})

    def test_smoke_rejects_a_mislabeled_executable_before_running_suites(self):
        run = Mock(return_value=SimpleNamespace(stdout="aevia 0.1.0-beta.1\n"))
        build = SimpleNamespace(target=self.target, assert_native=lambda: None, run=run)
        with self.assertRaisesRegex(ValueError, r"Expected packaged CLI version 0\.2\.0"):
            release_smoke(build, self.scratch, expected_version="0.2.0")
        run.assert_called_once_with([self.scratch / "aevia.exe", "--version"], capture_output=True, text=True)


class LinuxReleaseAbiTest(unittest.TestCase):
    def verify(self, version_info):
        build = SimpleNamespace(run=lambda *_args, **_kwargs: SimpleNamespace(stdout=version_info))
        verify_linux_release_abi(build, [Path("aevia")])

    def test_allows_a_weak_reference_above_the_baseline(self):
        self.verify("Name: GLIBC_2.43 Flags: WEAK Version: 3\nName: GLIBC_2.39 Flags: none Version: 2")

    def test_rejects_a_strong_reference_above_the_baseline(self):
        with self.assertRaisesRegex(ValueError, r"requires GLIBC_2\.43"):
            self.verify("Name: GLIBC_2.43 Flags: none Version: 3")


if __name__ == "__main__":
    unittest.main()
