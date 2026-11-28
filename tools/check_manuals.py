#!/usr/bin/env python3
"""Check bilingual manuals and registry API examples and run book programs."""

import argparse
import json
from pathlib import Path, PurePosixPath
import re
import subprocess
import tempfile


ROOT = Path(__file__).resolve().parent.parent
MANUALS = ROOT / "platform/manuals"
REGISTRY_DOCS = ROOT / "platform/docs"
LANGUAGES = ("en", "zh")
OPEN_FENCE = re.compile(r"^(`{3,}|~{3,})(.*)$")
PROJECT_FILE = re.compile(r"aevia,file=((?:[a-zA-Z][a-zA-Z0-9_]*/)*[a-zA-Z][a-zA-Z0-9_]*\.aev)")
DRAFT_MARKER = re.compile(
    r"^\s*(?:>\s*)?(?:#{1,6}\s*)?(?:(?:status|wip|todo)\s*[:：]|(?:状态|待办)\s*[:：]|(?:wip|todo)\s*$)",
    re.IGNORECASE,
)
CHAPTER_LINK = re.compile(r"\]\(\.?/?([0-9]{2}-[a-z0-9-]+)\.md(?:#[^ )]+)?\)")
API_ROUTE = re.compile(r"`((?:(?:GET|POST) )?/[^`\s]+)`")
JSON_FENCE = re.compile(r"^```json\n(.*?)^```$", re.M | re.S)
API_WIRE_EXAMPLE = re.compile(
    r"^```(?:json\n|\n(?=aevia-(?:publish|yank)-v\d+\n))(.*?)^```$", re.M | re.S
)
NUMERICAL_OPERATOR = re.compile(r"`(num_[a-z0-9_]+)`")
NUMERICAL_LITERAL = re.compile(r'"(num_[a-z0-9_]+)"')


def verify_finished_prose(path: Path, text: str) -> None:
    fence = ""
    for number, line in enumerate(text.splitlines(), start=1):
        if fence:
            if re.fullmatch(re.escape(fence[0]) + "{" + str(len(fence)) + ",} *", line):
                fence = ""
        elif match := OPEN_FENCE.fullmatch(line):
            fence = match[1]
        elif DRAFT_MARKER.match(line):
            raise ValueError(f"{path}:{number}: describe the behavior or requirement directly")



def chapters(root: Path) -> dict[str, str]:
    result = {}
    for path in sorted(root.glob("*.md")):
        text = path.read_text(encoding="utf-8")
        if not re.fullmatch(r"[0-9]{2}-[a-z0-9-]+", path.stem):
            raise ValueError(f"{path}: chapter needs a numbered URL slug")
        if not text.splitlines() or not text.startswith("# "):
            raise ValueError(f"{path}: chapter must begin with a title")
        verify_finished_prose(path, text)
        result[path.stem] = text
    if not result:
        raise ValueError(f"{root}: no chapters")
    for slug, text in result.items():
        for target in CHAPTER_LINK.findall(text):
            if target not in result:
                raise ValueError(f"{root / slug}: missing chapter link {target}")
    return result


def programs(text: str) -> list[dict[str, str]]:
    snippets = []
    project = {}
    fence = ""
    info = ""
    body = []
    for line in text.splitlines():
        if fence:
            if re.fullmatch(re.escape(fence[0]) + "{" + str(len(fence)) + ",} *", line):
                source = "\n".join(body) + "\n"
                if info == "aevia,run":
                    snippets.append({"main.aev": source})
                elif file := PROJECT_FILE.fullmatch(info):
                    name = file[1]
                    if name.casefold() in {path.casefold() for path in project}:
                        raise ValueError(f"duplicate example source path {name}")
                    project[name] = source
                elif info.startswith("aevia,file="):
                    raise ValueError(f"invalid example source path {info}")
                fence, info, body = "", "", []
            elif info == "aevia,run" or info.startswith("aevia,file="):
                body.append(line)
        elif match := OPEN_FENCE.fullmatch(line):
            fence, info = match[1], match[2].strip()
    if fence:
        raise ValueError("unclosed code fence")
    if project:
        if "main.aev" not in project:
            raise ValueError("multi-file example requires a main.aev entry")
        snippets.append(project)
    return snippets


def verify_source(aevia: Path, label: str, sources: dict[str, str]) -> None:
    with tempfile.TemporaryDirectory(prefix="aevia-manual-") as directory:
        for relative, source in sources.items():
            path = Path(directory).joinpath(*PurePosixPath(relative).parts)
            path.parent.mkdir(parents=True, exist_ok=True)
            path.write_text(source, encoding="utf-8")
        runner = [str(aevia), "--max-stack", "16", "--budget", "100000", "--jobs", "1",
                  "--color", "never", "--caps="]
        verbs = ["run", "test"] if any(re.search(r"^test ", source, re.MULTILINE)
                                      for source in sources.values()) else ["run"]
        for verb in verbs:
            result = subprocess.run([*runner, verb, "main.aev"], cwd=directory, capture_output=True,
                                    encoding="utf-8", errors="replace", timeout=30)
            if result.returncode:
                raise ValueError(f"{label} ({verb}):\n{result.stdout}{result.stderr}")


def verify_registry_api(directory: Path) -> None:
    english, chinese = ((directory / filename).read_text(encoding="utf-8")
                        for filename in ("API.md", "API.zh.md"))
    en_routes, zh_routes = (set(API_ROUTE.findall(text)) for text in (english, chinese))
    if en_routes != zh_routes:
        raise ValueError(f"API routes differ: English only {sorted(en_routes - zh_routes)}, "
                         f"Chinese only {sorted(zh_routes - en_routes)}")
    examples = API_WIRE_EXAMPLE.findall(english)
    if not all(any(example.startswith(f"aevia-{action}-v") for example in examples)
               for action in ("publish", "yank")):
        raise ValueError("API.md must include both canonical signed statements")
    if sorted(examples) != sorted(API_WIRE_EXAMPLE.findall(chinese)):
        raise ValueError("registry API wire examples must match in English and Chinese")
    print(f"checked {len(en_routes)} bilingual registry API routes and {len(examples)} wire examples")


def verify_lsp_capabilities(aevia: Path | None) -> None:
    paths = (ROOT / "spec/en/09-runtime.md", ROOT / "spec/09-runtime.md")
    examples = []
    for path in paths:
        blocks = JSON_FENCE.findall(path.read_text(encoding="utf-8"))
        candidates = [block for block in blocks if '"semanticTokensProvider"' in block]
        if len(candidates) != 1:
            raise ValueError(f"{path}: expected one LSP capabilities example")
        examples.append(json.loads(candidates[0]))
    if examples[0] != examples[1]:
        raise ValueError("LSP capabilities examples differ between translations")
    if aevia:
        request = json.dumps({"jsonrpc": "2.0", "id": 1, "method": "initialize",
                              "params": {"capabilities": {}}}).encode("utf-8")
        frame = f"Content-Length: {len(request)}\r\n\r\n".encode("ascii") + request
        run = subprocess.run([str(aevia.resolve()), "lsp", "--stdio"], input=frame,
                             capture_output=True, timeout=30, cwd=ROOT)
        header, _, body = run.stdout.partition(b"\r\n\r\n")
        length = re.search(rb"^Content-Length: ([0-9]+)\r?$", header, re.M | re.I)
        if run.returncode or length is None or int(length[1]) != len(body):
            raise ValueError(f"invalid response to LSP initialize request: {run.stderr!r}")
        result = json.loads(body).get("result", {}).get("capabilities")
        if result != examples[0]:
            raise ValueError("the bilingual LSP capabilities examples differ from the server response")
    print("checked the bilingual LSP capabilities examples")


def verify_stdlib_tree(modules: list[dict]) -> None:
    prefixes = {}
    for module in modules:
        components = module["path"].split(".")
        for depth, name in enumerate(components):
            prefixes.setdefault(tuple(components[:depth + 1]), f'{"  " * depth}{name}')
    expected = "\n".join(prefixes.values())
    tree = re.compile(r"^## 8\.20 [^\n]+\n\n```\n(.*?)\n```$", re.M | re.S)
    for path in (ROOT / "spec/en/08-stdlib.md", ROOT / "spec/08-stdlib.md"):
        match = tree.search(path.read_text(encoding="utf-8"))
        if match is None or match[1] != expected:
            raise ValueError(f"The module tree in {path} must match the generated embedded library reference.")
    print(f"checked the bilingual tree of {len(modules)} embedded library modules")


def documented_operator_exists(name: str, operators: set[str]) -> bool:
    # A generated higher-order facade uses the kernel operator's name with a
    # typed suffix. The published catalogue lists the canonical kernel name.
    return name in operators or name.removesuffix("_typed") in operators


def verify_numerical_guides(operators: set[str]) -> None:
    guides = (ROOT / "examples/NUMERICAL_GUIDE.md", ROOT / "examples/NUMERICAL_GUIDE.zh-CN.md")
    english, chinese = (
        set(NUMERICAL_OPERATOR.findall(path.read_text(encoding="utf-8"))) for path in guides
    )
    if english != chinese:
        raise ValueError(f"numerical guide operators differ: English only {sorted(english - chinese)}, "
                         f"Chinese only {sorted(chinese - english)}")
    missing = {name for name in english if not documented_operator_exists(name, operators)}
    if missing:
        raise ValueError(f"numerical guides name operators absent from the generated reference: {sorted(missing)}")
    print(f"checked {len(english)} bilingual numerical guide operator names")


def verify_documented_operators(operators: set[str], paths: list[Path]) -> None:
    seen: set[str] = set()
    for path in paths:
        for line, text in enumerate(path.read_text(encoding="utf-8").splitlines(), 1):
            names = NUMERICAL_LITERAL.findall(text)
            if path.suffix == ".md":
                names.extend(NUMERICAL_OPERATOR.findall(text))
            for name in names:
                if not documented_operator_exists(name, operators):
                    raise ValueError(f"{path}:{line} names unknown numerical operator {name}")
                seen.add(name)
    print(f"checked {len(seen)} numerical operator references across {len(paths)} documentation and example sources")


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--aevia", type=Path, help="path to a built aevia executable")
    args = parser.parse_args()
    verify_registry_api(REGISTRY_DOCS)
    verify_lsp_capabilities(args.aevia)
    reference = ROOT / "platform/stdlib-docs/stdlib-docs.json"
    documented = json.loads(reference.read_text(encoding="utf-8"))
    verify_stdlib_tree(documented["modules"])
    operators = {operator["name"] for operator in documented["operators"]}
    verify_numerical_guides(operators)
    sources = list(ROOT.glob("README*.md"))
    sources += [ROOT / "ARCHITECTURE.md", ROOT / "CHANGELOG.md"]
    for directory in ["examples", "spec", "platform/docs", "platform/manuals"]:
        sources.extend((ROOT / directory).rglob("*.md"))
    sources.extend((ROOT / "examples").rglob("*.aev"))
    verify_documented_operators(operators, sorted(sources))
    for path in sorted((ROOT / "spec").rglob("*.md")) + sorted(REGISTRY_DOCS.glob("*.md")):
        verify_finished_prose(path, path.read_text(encoding="utf-8"))
    total = 0
    for name in ("book", "architecture"):
        en, zh = (chapters(MANUALS / name / language) for language in LANGUAGES)
        if en.keys() != zh.keys():
            raise ValueError(f"{name}: English only {sorted(en.keys() - zh.keys())}, "
                             f"Chinese only {sorted(zh.keys() - en.keys())}")
        for slug, text in en.items():
            snippets = programs(text)
            if snippets != programs(zh[slug]):
                raise ValueError(f"{name}/{slug}: runnable examples differ between translations")
            if args.aevia:
                for index, source in enumerate(snippets, start=1):
                    verify_source(args.aevia.resolve(), f"{name}/{slug}, example {index}", source)
            total += len(snippets)
        print(f"checked {len(en)} paired {name} chapters")
    print(f"checked {total} runnable examples")


if __name__ == "__main__":
    try:
        main()
    except (ValueError, subprocess.TimeoutExpired) as error:
        raise SystemExit(str(error)) from error
