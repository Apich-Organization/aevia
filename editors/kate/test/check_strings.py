"""Check installed Kate string styles with the KSyntaxHighlighting CLI."""

import argparse
from bisect import bisect_right
import os
from pathlib import Path
import re
import shutil
import subprocess
import tempfile
import xml.etree.ElementTree as ET


HERE = Path(__file__).resolve().parent
ANSI = re.compile(r"\x1b\[[0-?]*[ -/]*[@-~]")
CASES = [
    ("let ordinary", "## quoted #- text -#", "String"),
    ("let ordinary", r"\0", "Escape"),
    ("let ordinary", r"\u{41}", "Escape"),
    ("let ordinary", "2", "Number"),
    ("    # quoted", "# quoted", "String"),
    ("    literal", '""double""', "String"),
    ('    "{4}', "4", "Number"),
    ('    "{4}', r"\0", "Escape"),
    ("let record", "# raw {}", "String"),
    ("let record", "2", "Number"),
    ("let protected", r'\"one\"', "String"),
    ("let protected", "tail", "String"),
    ("let escaped_group", r"\}", "Symbol"),
    ("let escaped_group", "tail", "String"),
    ("let protected_raw", "{} # raw", "String"),
    ("let protected_raw", "5", "Number"),
    ("let comments", "#- outer #- nested -# -#", "Comment"),
    ("let comments", "# real", "Comment"),
    ("} trailing", "}", "Interpolation"),
    ("} trailing", "trailing", "String"),
    ("let raw", r'r"end-of-line \0 {x} # literal', "String"),
    ("let after", "let", "Keyword"),
    ("let after", "4", "Number"),
    ("let invalid", r"\q", "Invalid Escape"),
    ("let invalid", r"\u{Q}", "Invalid Escape"),
    ("let invalid", "tail", "String"),
]


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--highlighter", default="ksyntaxhighlighter6")
    args = parser.parse_args()
    definition = HERE.parent / "aevia.xml"
    fixture = HERE / "strings.aev"
    with tempfile.TemporaryDirectory() as profile:
        syntax = Path(profile) / "org.kde.syntax-highlighting/syntax"
        syntax.mkdir(parents=True)
        shutil.copyfile(definition, syntax / "aevia.xml")
        result = subprocess.run(
            [args.highlighter, "-f", "ansi", "--syntax-trace", "format", str(fixture)],
            env={**os.environ, "XDG_DATA_HOME": profile, "XDG_DATA_DIRS": profile},
            capture_output=True,
            text=True,
            check=True,
        )

    names = [item.attrib["name"] for item in ET.parse(definition).findall(".//itemData")]
    alternatives = "|".join(map(re.escape, sorted(names, key=len, reverse=True)))
    formats = re.compile(r"\b(?:" + alternatives + r")\b")
    trace = ANSI.sub("", result.stdout).splitlines()
    lines = fixture.read_text(encoding="utf-8").splitlines()
    line_rows = []
    row = 0
    for line in lines:
        row = trace.index(line, row)
        line_rows.append(row)
        row += 1
    line_rows.append(len(trace))

    for prefix, snippet, style in CASES:
        line_num, source = next((i, line) for i, line in enumerate(lines) if line.startswith(prefix))
        start = source.index(snippet)
        rows = trace[line_rows[line_num] + 1:line_rows[line_num + 1]]
        transitions = sorted((entry.start(), entry.group()) for row in rows for entry in formats.finditer(row))
        columns = [column for column, _ in transitions]
        for column in range(start, start + len(snippet)):
            index = bisect_right(columns, column) - 1
            actual = transitions[index][1] if index >= 0 else None
            if actual != style:
                raise SystemExit(f"{fixture.name}:{line_num + 1}:{column + 1}: expected {style}, found {actual}")
    print(f"Verified {len(CASES)} Kate syntax probes")


if __name__ == "__main__":
    main()
