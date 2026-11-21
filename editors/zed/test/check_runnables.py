"""Check the exact set of Zed gutter captures on an Aevia fixture."""

import argparse
from pathlib import Path
import re
import subprocess


TEST_DIR = Path(__file__).resolve().parent
ZED_DIR = TEST_DIR.parent
CAPTURE = re.compile(r"\b\d+ - run, start: \((\d+), (\d+)\), .*text: `([^`]+)`")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--tree-sitter", default="tree-sitter")
    parser.add_argument("--grammar-dir", type=Path, default=ZED_DIR.parent / "tree-sitter-aevia")
    args = parser.parse_args()

    fixture = TEST_DIR / "runnables.aev"
    rows = fixture.read_text(encoding="utf-8").splitlines()
    expected = set()
    for row, line in enumerate(rows):
        if line.startswith("# run "):
            name = line.removeprefix("# run ")
            column = rows[row - 1].encode("utf-8").index(name.encode("utf-8"))
            expected.add((row - 1, column, name))

    query = ZED_DIR / "languages/aevia/runnables.scm"
    try:
        result = subprocess.run(
            [args.tree_sitter, "query", str(query), str(fixture)],
            cwd=args.grammar_dir, check=True, capture_output=True, text=True, timeout=15,
        )
    except subprocess.TimeoutExpired:
        raise SystemExit("Tree-sitter did not finish the Aevia runnable query in 15 seconds") from None
    observed = [(int(row), int(column), name) for row, column, name in CAPTURE.findall(result.stdout)]
    if sorted(observed) != sorted(expected):
        raise SystemExit(f"Expected {sorted(expected)}, found {sorted(observed)}")
    print(f"Verified {len(expected)} Aevia runnables")


if __name__ == "__main__":
    main()
