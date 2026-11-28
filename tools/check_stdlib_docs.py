#!/usr/bin/env python3
"""Render and verify the English and Chinese Aevia standard-library reference."""

import argparse
import json
from pathlib import Path
import re
import subprocess
import tempfile


ROOT = Path(__file__).resolve().parent.parent
SOURCE = ROOT / "crates/aevia-std"
REFERENCE = ROOT / "platform/stdlib-docs"
HAN = re.compile(r"[\u3400-\u9fff]")
IDENTITY = ("kind", "name", "signature", "private", "doc_hidden")


def require(condition, message):
    if not condition:
        raise ValueError(message)


def render(binary, directory, language):
    command = [str(binary), "--color=never", "doc", "--stdlib", "--lang", language,
               "--json", "--out", str(directory)]
    result = subprocess.run(command, cwd=ROOT, capture_output=True, encoding="utf-8", timeout=180)
    require(result.returncode == 0 and not result.stderr.strip(),
            f"{language}: aevia doc failed\n{result.stdout}{result.stderr}")
    return (directory / "stdlib-docs.json").read_bytes()


def declaration_identity(module):
    return [tuple(entry[key] for key in IDENTITY) for entry in module["decls"]]


def check_edition(directory, language, modules, wanted):
    actual = {path.name for path in directory.iterdir()}
    require(actual == wanted, f"{language}: missing {sorted(wanted - actual)}, extra {sorted(actual - wanted)}")
    index = (directory / "index.md").read_text(encoding="utf-8")
    declarations = 0
    for module in modules:
        path = module["path"]
        label = f"{language}/{path}"
        page = (directory / f"{path}.md").read_text(encoding="utf-8")
        require(f"({path}.md)" in index, f"{label}: module missing from index")
        prose = module["doc"].strip()
        require(prose and prose in page, f"{label}: missing rendered module description")
        require(language != "zh" or HAN.search(prose), f"{label}: missing Chinese module description")
        for entry in module["decls"]:
            if entry["private"] or entry["doc_hidden"]:
                continue
            name = f'{entry["kind"]} {entry["name"]}'
            prose = entry["doc"].strip()
            signature = f'```aevia\n{entry["signature"]}\n```'
            # Match the declaration's own section so identical descriptions or
            # overload names elsewhere in the same module cannot mask omissions.
            sections = [section for section in page.split("\n## ")[1:]
                        if section.startswith(f"`{name}`\n")]
            require(prose and any(signature in section and prose in section for section in sections),
                    f"{label}: missing signature or description for {name}")
            require(language != "zh" or HAN.search(prose), f"{label}: missing Chinese description for {name}")
            declarations += 1
    return declarations


def verify(binary, output):
    en, zh = output / "en", output / "zh"
    english = render(binary, en, "en")
    chinese = render(binary, zh, "zh")
    require(english == chinese, "the generated JSON bundle varies with the Markdown edition")
    bundle = json.loads(english)
    en_modules, zh_modules = bundle["modules"], bundle["zh_modules"]
    sources = {f.relative_to(SOURCE).as_posix() for f in (SOURCE / "stdlib").rglob("*.aev")}
    paths = {module["path"] for module in en_modules}
    require(len(paths) == len(en_modules), "duplicate standard-library module paths")
    require(sources == {module["file"] for module in en_modules}, "documented modules differ from library sources")
    for en_module, zh_module in zip(en_modules, zh_modules, strict=True):
        require((en_module["path"], en_module["file"], declaration_identity(en_module))
                == (zh_module["path"], zh_module["file"], declaration_identity(zh_module)),
                f'{en_module["path"]}: module or declaration differs between editions')
    wanted = {f"{path}.md" for path in paths} | {"index.md", "stdlib.md", "stdlib-docs.json"}
    counts = [check_edition(directory, language, modules, wanted)
              for directory, language, modules in [(en, "en", en_modules), (zh, "zh", zh_modules)]]
    require(counts[0] == counts[1], "the editions have different declaration counts")
    print(f"Verified {len(paths)} modules and {counts[0]} rendered declarations in each edition.", flush=True)
    require({path.name for path in REFERENCE.iterdir()} == wanted, "the versioned reference has missing or extra files")
    changed = sorted(name for name in wanted if (en / name).read_bytes() != (REFERENCE / name).read_bytes())
    require(not changed, f"the versioned reference needs regeneration: {', '.join(changed)}")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--aevia", required=True, type=Path, help="path to a built aevia executable")
    args = parser.parse_args()
    try:
        with tempfile.TemporaryDirectory(prefix="aevia-stdlib-docs-") as directory:
            verify(args.aevia.resolve(), Path(directory))
    except (OSError, ValueError, subprocess.TimeoutExpired) as error:
        raise SystemExit(str(error)) from error


if __name__ == "__main__":
    main()
