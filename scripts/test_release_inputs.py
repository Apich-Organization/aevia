"""Regression tests for the source and ABI checks on release artifacts."""

import json
from pathlib import Path
import subprocess
from tempfile import TemporaryDirectory
from types import SimpleNamespace
import unittest

from aevia_tasks import source_digests, verify_linux_release_abi, verify_release_inputs


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
            source.write_text(f"# {project}\n", encoding="utf-8")
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
