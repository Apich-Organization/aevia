"""Regenerates the single-file specification from `spec/`.

    python tools/bundle_spec.py [--revision N] [--no-html]

`Aevia-Language-Specification.md` is the chapters of `spec/` in reading order,
each preceded by a marker comment; the table of contents lists each chapter's
first heading. `Aevia-Language-Specification.html` is rendered from it with
pandoc when pandoc is on the PATH (skipped otherwise, with a note). Run it after
editing any chapter; the single-file edition is generated, never hand-edited.
"""

import argparse
import pathlib
import re
import shutil
import subprocess
import sys

ROOT = pathlib.Path(__file__).resolve().parent.parent
SPEC = ROOT / "spec"
ORDER = [
    "README.md",
    "00-overview.md",
    "01-lexical.md",
    "02-syntax.md",
    "03-types.md",
    "04-dimensions.md",
    "05-units.md",
    "06-semantics.md",
    "07-modules.md",
    "08-stdlib.md",
    "09-runtime.md",
    "10-backend.md",
    "11-examples.md",
    "12-rationale.md",
    "13-extensibility.md",
    "appendix-a-grammar.md",
    "appendix-b-diagnostics.md",
    "appendix-c-changelog.md",
]


def title(text: str, name: str) -> str:
    for line in text.splitlines():
        if line.startswith("# "):
            heading = line[2:].strip()
            # The index chapter is the specification itself.
            return heading + (" — Index" if name == "README.md" else "")
    return name


def short(heading: str) -> str:
    # `00 — 总览：Aevia 是什么…` is listed by its chapter name only.
    return re.split(r"[：:]", heading, maxsplit=1)[0].strip()


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--revision", default=None)
    parser.add_argument("--no-html", action="store_true")
    args = parser.parse_args()
    out_md = ROOT / "Aevia-Language-Specification.md"
    revision = args.revision
    if revision is None and out_md.exists():
        found = re.search(r"Revision (\d+)", out_md.read_text(encoding="utf-8"))
        revision = found.group(1) if found else "1"
    chapters = []
    for name in ORDER:
        text = (SPEC / name).read_text(encoding="utf-8").replace("\r\n", "\n")
        chapters.append((name, text))
    toc = "\n\n".join(
        f"{index}. {short(title(text, name))}"
        for index, (name, text) in enumerate(chapters, start=1)
    )
    parts = [
        "# Aevia Language Specification\n\n"
        f"**Single-file edition — Revision {revision}**\n\n"
        "> 本文件由 `spec/` 下的章节按顺序合并生成（`tools/bundle_spec.py`）；"
        "章节内的相对链接在单文件版中不再可用。\n\n---\n\n"
        "## Table of contents\n\n" + toc + "\n\n\n---\n\n"
    ]
    for name, text in chapters:
        parts.append(f"\n<!-- ===== {name} ===== -->\n\n{text.rstrip()}\n\n")
    out_md.write_text("".join(parts), encoding="utf-8", newline="\n")
    print(f"wrote {out_md.name} ({len(chapters)} chapters, revision {revision})")
    if args.no_html:
        return 0
    pandoc = shutil.which("pandoc")
    if pandoc is None:
        print("pandoc not found: the HTML edition was not regenerated", file=sys.stderr)
        return 0
    out_html = ROOT / "Aevia-Language-Specification.html"
    subprocess.run(
        [
            pandoc,
            str(out_md),
            "--standalone",
            "--from=gfm",
            "--metadata",
            f"title=Aevia Language Specification — Revision {revision}",
            "--metadata",
            "lang=zh-CN",
            "-o",
            str(out_html),
        ],
        check=True,
    )
    print(f"wrote {out_html.name}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
