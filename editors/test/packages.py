"""Check the runtime files shipped in the VS Code and JetBrains archives."""

import argparse
import io
import json
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
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("directory", type=Path)
    parser.add_argument("--editor", choices=("all", "vscode"), default="all")
    options = parser.parse_args()
    vsix = single_file(options.directory.iterdir(), "*.vsix")
    verify_vsix(vsix)
    print(f"Verified {vsix.name}")
    if options.editor == "all":
        jetbrains = single_file(options.directory.iterdir(), "*.zip")
        verify_jetbrains(jetbrains)
        print(f"Verified {jetbrains.name}")
