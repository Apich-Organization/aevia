#!/usr/bin/env python3
"""Exercise deployment backup and restore with a local Podman stand-in."""

import gzip
import io
import os
from pathlib import Path
import shlex
import shutil
import subprocess
import sys
import tarfile
from tempfile import TemporaryDirectory
import unittest


DEPLOY = Path(__file__).resolve().parents[1] / "infra" / "deploy.sh"
STAMP = "20261120T120000Z"
SQL = b"CREATE TABLE package (id integer);\n"

PODMAN = """\
import gzip
import os
from pathlib import Path
import sys
import time

root = Path(os.environ["FIXTURE_ROOT"])
mode = os.environ["FIXTURE_MODE"]
args = sys.argv[1:]

def run_job(name, *, compressed=True):
    (root / f"{name}.started").touch()
    if mode == "parallel":
        other = "archive" if name == "db" else "db"
        deadline = time.monotonic() + 3
        while not (root / f"{other}.started").exists():
            if time.monotonic() >= deadline:
                sys.exit(77)
            time.sleep(0.01)
    if mode != f"empty-{name}":
        suffix = os.environ.get("FIXTURE_SUFFIX", "")
        data = (root / f"{name}{suffix}.gz").read_bytes()
        sys.stdout.buffer.write(data if compressed else gzip.decompress(data))
    sys.exit(71 if mode == f"fail-{name}" else 0)

if args == ["compose", "version"]:
    sys.exit(0)
if args[0] == "compose":
    command = args[7:]
    if command == ["ps", "--quiet", "--status", "running", "registry"]:
        if mode == "running":
            print("registry-fixture")
    elif command[:4] == ["exec", "-T", "db", "sh"]:
        operation = command[-1]
        if operation.startswith("pg_dump "):
            run_job("db", compressed="--format=plain --compress=gzip:9" in operation)
        elif operation.startswith("psql "):
            assert "--single-transaction --file=-" in operation
            (root / "restored.sql").write_bytes(sys.stdin.buffer.read())
            sys.exit(73 if mode == "fail-psql" else 0)
        else:
            raise AssertionError(command)
    else:
        raise AssertionError(command)
elif args == ["volume", "exists", "aevia_aevia-data"]:
    sys.exit(1 if mode == "no-volume" else 0)
elif args[0] == "run":
    assert args[args.index("--entrypoint") + 1] == "/usr/bin/tar"
    volume = args[args.index("--volume") + 1]
    if "-i" in args:
        assert volume == "aevia_aevia-data:/data" and "-xzf" in args
        (root / "restored.tar.gz").write_bytes(sys.stdin.buffer.read())
        sys.exit(0)
    assert volume == "aevia_aevia-data:/data:ro"
    assert "--exclude=*.partial" in args
    run_job("archive")
else:
    raise AssertionError(args)
"""


def write_archive(path, payload):
    with tarfile.open(path, "w:gz") as archive:
        entry = tarfile.TarInfo("package.aevpkg")
        entry.size = len(payload)
        archive.addfile(entry, io.BytesIO(payload))


class BackupTests(unittest.TestCase):
    def setUp(self):
        fixture = TemporaryDirectory(prefix="aevia-deploy-")
        self.addCleanup(fixture.cleanup)
        self.root = Path(fixture.name)
        infra = self.root / "platform" / "infra"
        infra.mkdir(parents=True)
        self.script = infra / "deploy.sh"
        self.backups = infra / "backups"
        shutil.copyfile(DEPLOY, self.script)
        (infra / ".env").write_text(f"AEVIA_SESSION_SECRET={'1' * 64}\n", encoding="utf-8")
        mocks = self.root / "bin"
        mocks.mkdir()
        podman = mocks / "podman"
        podman.write_text(f"#!{sys.executable}\n{PODMAN}", encoding="utf-8")
        podman.chmod(0o755)
        date = mocks / "date"
        date.write_text(f"#!/bin/sh\nprintf '%s\\n' '{STAMP}'\n", encoding="utf-8")
        date.chmod(0o755)
        for name in ("gzip", "gunzip"):
            executable = shutil.which(name)
            assert executable is not None
            wrapper = mocks / name
            wrapper.write_text(
                "#!/bin/sh\n"
                'if [ "$FIXTURE_MODE" = partial-sql ] && [ "$1" != -t ]; then\n'
                "  printf 'SELECT 1;\\n'; exit 2\n"
                "fi\n"
                f'exec {shlex.quote(executable)} "$@"\n',
                encoding="utf-8",
            )
            wrapper.chmod(0o755)
        self.temp = self.root / "tmp"
        self.temp.mkdir()
        self.env = os.environ | {
            "PATH": str(mocks) + os.pathsep + os.environ["PATH"],
            "FIXTURE_ROOT": str(self.root),
            "TMPDIR": str(self.temp),
        }
        (self.root / "db.gz").write_bytes(gzip.compress(SQL))
        write_archive(self.root / "archive.gz", b"fixture package")
        self.sql = self.backups / f"aevia-db-{STAMP}.sql.gz"
        self.archive = self.backups / f"aevia-data-{STAMP}.tar.gz"

    def deploy(self, *arguments, mode="ok"):
        return subprocess.run(
            ["sh", self.script, *arguments],
            env=self.env | {"FIXTURE_MODE": mode, "AEVIA_RESTORE_YES": "1"},
            capture_output=True,
            text=True,
            timeout=10,
            check=False,
        )

    def assert_unpublished(self):
        self.assertEqual(list(self.backups.glob("*")), [])

    def test_backups_overlap_and_keep_plain_sql_restore_format(self):
        result = self.deploy("backup", mode="parallel")
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(set(self.backups.iterdir()), {self.sql, self.archive})
        self.assertEqual(gzip.decompress(self.sql.read_bytes()), SQL)
        with tarfile.open(self.archive) as archive:
            with archive.extractfile("package.aevpkg") as package:
                self.assertEqual(package.read(), b"fixture package")
        restore = self.deploy("restore", self.sql)
        self.assertEqual(restore.returncode, 0, restore.stderr)
        self.assertEqual((self.root / "restored.sql").read_bytes(), SQL)
        self.assertEqual(list(self.temp.iterdir()), [])
        restore = self.deploy("restore", self.archive)
        self.assertEqual(restore.returncode, 0, restore.stderr)
        self.assertEqual((self.root / "restored.tar.gz").read_bytes(), self.archive.read_bytes())

    def test_failed_or_empty_decompression_never_starts_psql(self):
        sql = self.root / "-backup.sql.gz"
        for mode, data in [
            ("ok", gzip.compress(SQL)[:-4]),
            ("ok", gzip.compress(b"")),
            ("partial-sql", gzip.compress(SQL)),
        ]:
            with self.subTest(mode=mode, size=len(data)):
                sql.write_bytes(data)
                result = self.deploy("restore", sql, mode=mode)
                self.assertNotEqual(result.returncode, 0, result.stderr)
                self.assertFalse((self.root / "restored.sql").exists())
                self.assertEqual(list(self.temp.iterdir()), [])

    def test_sql_error_removes_the_temporary_input(self):
        sql = self.root / "backup.sql.gz"
        sql.write_bytes(gzip.compress(SQL))
        result = self.deploy("restore", sql, mode="fail-psql")
        self.assertEqual(result.returncode, 73, result.stderr)
        self.assertEqual((self.root / "restored.sql").read_bytes(), SQL)
        self.assertEqual(list(self.temp.iterdir()), [])

    def test_each_failed_or_empty_snapshot_leaves_no_visible_or_staged_files(self):
        for mode in ["fail-db", "empty-db", "fail-archive", "empty-archive", "no-volume"]:
            with self.subTest(mode=mode):
                result = self.deploy("backup", mode=mode)
                self.assertNotEqual(result.returncode, 0, result.stderr)
                self.assert_unpublished()

    def test_live_registry_is_rejected_before_either_snapshot(self):
        result = self.deploy("backup", mode="running")
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("stop the registry", result.stderr)
        self.assertFalse((self.root / "db.started").exists())
        self.assertFalse((self.root / "archive.started").exists())
        self.assert_unpublished()

    def test_repeated_timestamp_cannot_overwrite_an_existing_pair(self):
        self.backups.mkdir()
        self.sql.write_bytes(b"previous sql")
        self.archive.write_bytes(b"previous archive")
        result = self.deploy("backup")
        self.assertNotEqual(result.returncode, 0)
        self.assertEqual(self.sql.read_bytes(), b"previous sql")
        self.assertEqual(self.archive.read_bytes(), b"previous archive")
        self.assertEqual(set(self.backups.iterdir()), {self.sql, self.archive})

    def test_conflicting_archive_rolls_back_only_the_new_sql_file(self):
        self.backups.mkdir()
        self.archive.write_bytes(b"previous archive")
        result = self.deploy("backup")
        self.assertNotEqual(result.returncode, 0)
        self.assertEqual(self.archive.read_bytes(), b"previous archive")
        self.assertEqual(set(self.backups.iterdir()), {self.archive})

    def test_concurrent_backups_publish_one_matching_pair(self):
        (self.root / "db-other.gz").write_bytes(gzip.compress(SQL + b"SELECT 2;\n"))
        write_archive(self.root / "archive-other.gz", b"other fixture package")
        args = ["sh", self.script, "backup"]
        env = self.env | {"FIXTURE_MODE": "parallel"}
        with subprocess.Popen(args, env=env, stderr=subprocess.PIPE) as first:
            with subprocess.Popen(
                args, env=env | {"FIXTURE_SUFFIX": "-other"}, stderr=subprocess.PIPE
            ) as other:
                _, first_error = first.communicate(timeout=10)
                _, other_error = other.communicate(timeout=10)
        self.assertEqual(sorted([first.returncode, other.returncode]), [0, 1])
        suffix = "" if first.returncode == 0 else "-other"
        self.assertEqual(set(self.backups.iterdir()), {self.sql, self.archive})
        self.assertEqual(self.sql.read_bytes(), (self.root / f"db{suffix}.gz").read_bytes())
        self.assertEqual(self.archive.read_bytes(), (self.root / f"archive{suffix}.gz").read_bytes())
        self.assertIn(b"could not publish", first_error if first.returncode else other_error)


if __name__ == "__main__":
    unittest.main()
