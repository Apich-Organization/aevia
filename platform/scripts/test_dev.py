#!/usr/bin/env python3
"""Check the developer stack with isolated container and Cargo fixtures."""

import json
import os
from pathlib import Path
import shutil
import subprocess
import sys
from tempfile import TemporaryDirectory
import unittest


DEV = Path(__file__).with_name("dev.sh")
TOOLS = r"""
import json
import os
from pathlib import Path
import sys

tool = Path(sys.argv[0]).name
args = sys.argv[1:]
state_path = Path(os.environ["DEV_FIXTURE_STATE"])
state = state_path.read_text()
with open(os.environ["DEV_FIXTURE_TRACE"], "a", encoding="utf-8") as trace:
    trace.write(json.dumps([tool, *args]) + "\n")

if tool == "cargo":
    keys = ("AEVIA_BIND", "AEVIA_BASE_URL", "AEVIA_STORAGE_DIR", "AEVIA_SPEC_DIR")
    values = {key: os.environ.get(key) for key in keys}
    Path(os.environ["DEV_FIXTURE_CARGO"]).write_text(json.dumps(values))
elif tool == "openssl":
    print("1" * 64)
elif args[:2] == ["container", "inspect"]:
    sys.exit(state == "absent")
elif args[:2] == ["inspect", "--format"]:
    assert args[2] == "{{.State.Running}}"
    print("true" if state == "running" else "false")
elif args[0] in {"run", "start"}:
    state_path.write_text("running")
elif args[0] == "stop":
    state_path.write_text("stopped")
elif args[0] == "rm":
    state_path.write_text("absent")
elif args[:2] == ["exec", "aevia-dev-db"]:
    assert args[2] in ("pg_isready", "psql")
    if args[2] == "psql":
        sys.exit(int(os.environ.get("DEV_FIXTURE_SQL_STATUS", "0")))
else:
    raise AssertionError([tool, *args])
"""


class DevTests(unittest.TestCase):
    def setUp(self):
        temp = TemporaryDirectory(prefix="aevia-dev-")
        self.addCleanup(temp.cleanup)
        scratch = Path(temp.name)
        self.root = scratch / "checkout with 'apostrophe"
        self.platform = self.root / "platform"
        self.script = self.platform / "scripts" / "dev.sh"
        self.script.parent.mkdir(parents=True)
        shutil.copyfile(DEV, self.script)
        self.bin = scratch / "bin"
        self.bin.mkdir()
        for name in ("podman", "docker", "cargo", "openssl"):
            command = self.bin / name
            command.write_text(f"#!{sys.executable}\n{TOOLS}", encoding="utf-8")
            command.chmod(0o755)
        for name in ("sh", "dirname", "sed"):
            command = shutil.which(name)
            assert command is not None
            (self.bin / name).symlink_to(command)

        self.trace = scratch / "trace.jsonl"
        self.state = scratch / "state.txt"
        self.cargo = scratch / "cargo.json"
        self.env = {
            "PATH": str(self.bin),
            "DEV_FIXTURE_TRACE": str(self.trace),
            "DEV_FIXTURE_STATE": str(self.state),
            "DEV_FIXTURE_CARGO": str(self.cargo),
        }

    def dev(self, command, *, state="absent", environment=None):
        self.state.write_text(state, encoding="utf-8")
        self.trace.unlink(missing_ok=True)
        return subprocess.run(
            ["sh", self.script, command],
            env=self.env | (environment or {}),
            capture_output=True,
            text=True,
            timeout=10,
        )

    def commands(self):
        return [json.loads(line) for line in self.trace.read_text().splitlines()]

    def test_default_loopback_and_exports_round_trip_through_sh(self):
        result = self.dev("up")
        self.assertEqual(result.returncode, 0, result.stderr)
        exports = "\n".join(line for line in result.stdout.splitlines() if line.startswith("export "))
        script = exports + "\n" + (
            'printf "%s\\n" "$AEVIA_BIND" "$AEVIA_BASE_URL" "$AEVIA_STORAGE_DIR" "$AEVIA_SPEC_DIR"'
        )
        shell = subprocess.run(["sh"], input=script, text=True, capture_output=True, env=self.env)
        self.assertEqual(shell.returncode, 0, shell.stderr)
        self.assertEqual(shell.stdout.splitlines(), [
            "127.0.0.1:8080", "http://127.0.0.1:8080",
            str(self.platform / "target" / "packages"), str(self.root / "spec"),
        ])
        self.assertTrue(all(call[0] in ("podman", "openssl") for call in self.commands()))
        launch = next(call for call in self.commands() if call[:2] == ["podman", "run"])
        self.assertIn("127.0.0.1:55432:5432", launch)

    def test_run_keeps_explicit_listener_and_origin(self):
        env = {"AEVIA_BIND": "[::1]:9089", "AEVIA_BASE_URL": "http://[::1]:9089"}
        result = self.dev("run", state="running", environment=env)
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(json.loads(self.cargo.read_text()), env | {
            "AEVIA_STORAGE_DIR": str(self.platform / "target" / "packages"),
            "AEVIA_SPEC_DIR": str(self.root / "spec"),
        })
        self.assertEqual(self.commands()[-1], ["cargo", "run", "--package", "aevia-registry-server"])

    def test_docker_fallback_starts_existing_stopped_container(self):
        (self.bin / "podman").unlink()
        result = self.dev("up", state="stopped")
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertIn(["docker", "start", "aevia-dev-db"], self.commands())
        self.assertFalse(any(call[:2] == ["docker", "run"] for call in self.commands()))

    def test_down_is_idempotent_and_stops_before_removal(self):
        result = self.dev("down", state="running")
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(self.commands()[-2:], [
            ["podman", "stop", "aevia-dev-db"], ["podman", "rm", "--force", "aevia-dev-db"],
        ])
        result = self.dev("down")
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(self.commands(), [["podman", "container", "inspect", "aevia-dev-db"]])

    def test_reset_uses_one_transaction_and_reports_sql_exit(self):
        result = self.dev("reset", state="running", environment={"DEV_FIXTURE_SQL_STATUS": "13"})
        self.assertEqual(result.returncode, 13, result.stderr)
        sql = self.commands()[-1]
        self.assertIn("--single-transaction", sql)
        self.assertEqual(sql[-4:], [
            "--command", "DROP SCHEMA IF EXISTS public CASCADE", "--command", "CREATE SCHEMA public",
        ])
        self.assertNotIn("schema dropped", result.stdout)


if __name__ == "__main__":
    unittest.main()
