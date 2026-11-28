"""Validate the shipped configuration for editors without a package build."""

import ast
import json
from pathlib import Path
import tomllib
import xml.etree.ElementTree as ET

import yaml

EDITORS = Path(__file__).resolve().parents[1]
LSP = ["aevia-lsp", "--stdio"]
DAP = ["aevia", "debug", "--stdio"]


def load_json(path):
    text = (EDITORS / path).read_text(encoding="utf-8")
    return json.loads("\n".join(line for line in text.splitlines() if not line.lstrip().startswith("//")))


with (EDITORS / "helix/languages.toml").open("rb") as file:
    helix = tomllib.load(file)
helix_lsp = helix["language-server"]["aevia-lsp"]
assert [helix_lsp["command"], *helix_lsp["args"]] == LSP
language = next(item for item in helix["language"] if item["name"] == "aevia")
assert "aev" in language["file-types"] and "aevia-lsp" in language["language-servers"]
debugger = language["debugger"]
assert debugger["transport"] == "stdio"
assert [debugger["command"], *debugger["args"]] == DAP
assert any(template["request"] == "launch" and "program" in template["args"] for template in debugger["templates"])
grammar = next(item for item in helix["grammar"] if item["name"] == "aevia")
assert (EDITORS / "helix" / grammar["source"]["path"] / "src/parser.c").is_file()

kate = ET.parse(EDITORS / "kate/aevia.xml").getroot()
assert kate.attrib["name"] == "Aevia" and "*.aev" in kate.attrib["extensions"].split(";")
assert load_json("kate/lspclient-settings.json")["servers"]["aevia"]["command"] == LSP
kate_debugger = load_json("kate/dap-settings.json")["dap"]["aevia"]
assert kate_debugger["run"]["command"] == DAP
assert kate_debugger["configurations"]["launch"]["request"]["program"] == "${file}"

with (EDITORS / "sublime/Aevia/Aevia.sublime-syntax").open(encoding="utf-8") as file:
    syntax = yaml.safe_load(file)
assert "aev" in syntax["file_extensions"] and "main" in syntax["contexts"]
build = load_json("sublime/Aevia/Aevia.sublime-build")
assert build["cmd"] == ["aevia", "run", "$file"] and build["selector"] == syntax["scope"]
assert ["aevia", "test", "$file"] in [variant["cmd"] for variant in build["variants"]]
sublime_lsp = load_json("sublime/LSP-aevia/LSP-aevia.sublime-settings")
assert sublime_lsp["command"] == LSP and sublime_lsp["selector"] == syntax["scope"]
assert "LSP" in load_json("sublime/LSP-aevia/dependencies.json")["*"]["*"]
load_json("sublime/Aevia/Aevia.sublime-settings")
commands = load_json("sublime/Aevia/Aevia.sublime-commands")
assert any(command["command"] == "aevia_open_repl" for command in commands)
for path in (EDITORS / "sublime").rglob("*.py"):
    ast.parse(path.read_text(encoding="utf-8"), filename=str(path))
ET.parse(EDITORS / "sublime/Aevia/Comments.tmPreferences")
print("Helix, Kate and Sublime Text configurations validated")
