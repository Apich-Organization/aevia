"""Check the runtime files shipped in the VS Code and JetBrains archives."""

import io
import json
import sys
import xml.etree.ElementTree as ET
from pathlib import Path
from zipfile import ZipFile


def single_file(paths, pattern):
    matches = sorted(path for path in paths if Path(path).match(pattern))
    if len(matches) != 1:
        raise SystemExit(f"Expected one {pattern}, found {matches}")
    return matches[0]


def require_files(archive, files):
    corrupt = archive.testzip()
    if corrupt is not None:
        raise SystemExit(f"Corrupt archive entry {corrupt}")
    missing = files.difference(archive.namelist())
    if missing:
        raise SystemExit(f"Missing package entries {sorted(missing)}")


def verify_vsix(path):
    with ZipFile(path) as archive:
        require_files(archive, {
            "extension.vsixmanifest",
            "extension/extension.js",
            "extension/language-configuration.json",
            "extension/node_modules/vscode-languageclient/lib/node/main.js",
            "extension/syntaxes/aevia.tmLanguage.json",
        })
        package = json.loads(archive.read("extension/package.json"))
        name = f"{package['name']}-{package['version']}.vsix"
        if package["name"] != "aevia" or path.name != name:
            raise SystemExit(f"Unexpected VS Code package name {path.name}, expected {name}")


def verify_jetbrains(path):
    with ZipFile(path) as archive:
        plugin = single_file(archive.namelist(), f"*/lib/{path.stem}.jar")
        require_files(archive, {plugin})
        with ZipFile(io.BytesIO(archive.read(plugin))) as jar:
            require_files(jar, {
                "META-INF/aevia-terminal.xml",
                "dev/aevia/jetbrains/AeviaDebugAdapterFactory.class",
                "dev/aevia/jetbrains/AeviaLanguageServerFactory.class",
                "textmate/aevia/syntaxes/aevia.tmLanguage.json",
            })
            manifest = ET.fromstring(jar.read("META-INF/plugin.xml"))
            if manifest.findtext("id") != "dev.aevia.jetbrains":
                raise SystemExit("Unexpected JetBrains plugin ID")


if __name__ == "__main__":
    directory = Path(sys.argv[1])
    vsix = single_file(directory.iterdir(), "*.vsix")
    jetbrains = single_file(directory.iterdir(), "*.zip")
    verify_vsix(vsix)
    verify_jetbrains(jetbrains)
    print(f"Verified {vsix.name} and {jetbrains.name}")
