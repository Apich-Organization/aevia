"""Builds the single-file specification editions from `spec/` (Chinese) and
`spec/en/` (English).

    python tools/bundle_spec.py [--revision N] [--no-html] [--lang zh|en]

Outputs, at the repository root:

    Aevia-Language-Specification.md        Chinese Markdown (kept for compatibility)
    Aevia-Language-Specification.html      Chinese HTML
    Aevia-Language-Specification.en.md     English Markdown
    Aevia-Language-Specification.en.html   English HTML

A chapter without an English file is included in Chinese, flagged in the English
edition, so the two books always have the same shape.

The HTML edition is rendered chapter by chapter with pandoc (`gfm`), then
assembled: every heading id and every in-book link is prefixed with its chapter
(`04-dimensions--42-…`) so ids cannot collide across chapters, and the relative
links between chapters (`03-types.md#3.2`) become working in-page anchors. The
page has a sticky chapter sidebar, light/dark themes, a language switch, copy
buttons and a print stylesheet; it needs no network.

The single-file editions are generated, never hand-edited.
"""

import argparse
import html
import pathlib
import re
import shutil
import subprocess
import sys

ROOT = pathlib.Path(__file__).resolve().parent.parent
SPEC = ROOT / "spec"
REPO_BLOB = "https://github.com/Apich-Organization/aevia/blob/main"
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
KEYWORDS = (
    "as class derive dimension do else false fn for if import in instance let match "
    "module open private rec test then true type unit where with forall"
).split()

TEXT = {
    "zh": {
        "title": "Aevia 语言规范",
        "edition": "单文件版 · 修订",
        "contents": "目录",
        "other": "English",
        "other_href": "Aevia-Language-Specification.en.html",
        "note": "本文件由 `spec/` 下的章节按顺序合并生成（`tools/bundle_spec.py`）。",
        "copy": "复制",
        "copied": "已复制",
        "fallback": "本章尚无英文版，以下为中文原文。",
        "html_lang": "zh-CN",
    },
    "en": {
        "title": "Aevia Language Specification",
        "edition": "Single-file edition · Revision",
        "contents": "Contents",
        "other": "中文",
        "other_href": "Aevia-Language-Specification.html",
        "note": "Generated from the chapters under `spec/en/` by `tools/bundle_spec.py`.",
        "copy": "copy",
        "copied": "copied",
        "fallback": "This chapter has no English edition yet; the Chinese text follows.",
        "html_lang": "en",
    },
}


def out_name(lang: str, ext: str) -> str:
    suffix = "" if lang == "zh" else ".en"
    return f"Aevia-Language-Specification{suffix}.{ext}"


def slug_of(name: str) -> str:
    stem = name[:-3]
    return "index" if stem.lower() == "readme" else stem


def read(path: pathlib.Path) -> str:
    return path.read_text(encoding="utf-8").replace("\r\n", "\n")


def load_chapters(lang: str):
    """[(name, slug, text, is_fallback)] in reading order."""
    chapters = []
    for name in ORDER:
        zh = SPEC / name
        en = SPEC / "en" / name
        if lang == "en" and en.exists():
            chapters.append((name, slug_of(name), read(en), False))
        else:
            chapters.append((name, slug_of(name), read(zh), lang == "en"))
    return chapters


def first_heading(text: str, fallback: str) -> str:
    for line in text.splitlines():
        if line.startswith("# "):
            return line[2:].strip()
    return fallback


def short(heading: str) -> str:
    """`00 — 总览：Aevia 是什么…` is listed by its part before the colon."""
    return re.split(r"[：:]", heading, maxsplit=1)[0].strip()


def detect_revision(explicit):
    if explicit:
        return explicit
    changelog = read(SPEC / "appendix-c-changelog.md")
    found = re.findall(r"Revision (\d+)", changelog)
    if found:
        return max(found, key=int)
    readme = read(SPEC / "README.md")
    found = re.search(r"Revision (\d+)", readme)
    return found.group(1) if found else "1"


# ----------------------------------------------------------------- Markdown


def link_to_anchor(match: re.Match) -> str:
    target = match.group(2)
    if target.startswith("../"):
        return f"[{match.group(1)}]({REPO_BLOB}/{target[3:]})"
    chapter = re.match(r"^([\w-]+)\.md(?:#.*)?$", target)
    if chapter:
        return f"[{match.group(1)}](#ch-{slug_of(chapter.group(1) + '.md')})"
    return match.group(0)


def build_markdown(lang: str, chapters, revision: str) -> str:
    t = TEXT[lang]
    toc = "\n".join(
        f"{index}. [{short(first_heading(text, name))}](#ch-{slug})"
        for index, (name, slug, text, _) in enumerate(chapters, start=1)
    )
    parts = [
        f"# {t['title']}\n\n**{t['edition']} {revision}**\n\n> {t['note']}\n\n---\n\n"
        f"## {t['contents']}\n\n{toc}\n\n---\n"
    ]
    for name, slug, text, fallback in chapters:
        body = re.sub(r"\[([^\]]+)\]\(([^)]+)\)", link_to_anchor, text.rstrip())
        flag = f"> {t['fallback']}\n\n" if fallback else ""
        parts.append(f'\n<a id="ch-{slug}"></a>\n\n{flag}{body}\n\n')
    return "".join(parts)


# --------------------------------------------------------------------- HTML


def pandoc_fragment(pandoc: str, text: str) -> str:
    result = subprocess.run(
        [pandoc, "--from=gfm", "--to=html5", "--no-highlight", "--wrap=none"],
        input=text,
        text=True,
        encoding="utf-8",
        capture_output=True,
        check=True,
    )
    return result.stdout


CODE_TOKEN = re.compile(
    r"(?P<comment>#[^\n]*)|(?P<string>\"(?:[^\"\\\n]|\\.)*\")|"
    r"(?P<number>\b\d+(?:\.\d+)?(?:[eE][+-]?\d+)?\b)|(?P<word>\b[A-Za-z_]\w*\b)"
)


def highlight(code: str) -> str:
    """A tiny tokenizer for `aevia` blocks; `code` is already HTML-escaped."""

    def paint(match: re.Match) -> str:
        kind = match.lastgroup
        text = match.group(0)
        if kind == "word":
            return f'<span class="kw">{text}</span>' if text in KEYWORDS else text
        return f'<span class="{kind[0]}">{text}</span>'

    return CODE_TOKEN.sub(paint, code)


def number_sections(fragment: str) -> str:
    return fragment


def namespace(fragment: str, slug: str, known: dict) -> str:
    """Prefixes ids and rewrites links so chapters can share one document."""
    ids = set(re.findall(r'\bid="([^"]+)"', fragment))
    fragment = re.sub(
        r'\bid="([^"]+)"', lambda m: f'id="{slug}--{m.group(1)}"', fragment
    )

    def href(match: re.Match) -> str:
        target = html.unescape(match.group(1))
        if target.startswith("#"):
            anchor = target[1:]
            if anchor in ids:
                return f'href="#{slug}--{anchor}"'
            return match.group(0)
        if target.startswith("../"):
            return f'href="{REPO_BLOB}/{target[3:]}"'
        chapter = re.match(r"^([\w-]+)\.md(?:#(.*))?$", target)
        if chapter:
            other = slug_of(chapter.group(1) + ".md")
            anchor = chapter.group(2)
            if anchor and anchor in known.get(other, ()):
                return f'href="#{other}--{anchor}"'
            return f'href="#ch-{other}"'
        return match.group(0)

    return re.sub(r'href="([^"]+)"', href, fragment)


def finish_code(fragment: str) -> str:
    def block(match: re.Match) -> str:
        lang = match.group(1) or ""
        code = match.group(2)
        label = f' data-lang="{lang}"' if lang else ""
        if lang == "aevia":
            code = highlight(code)
        return f'<pre class="code"{label}><code>{code}</code></pre>'

    return re.sub(
        r'<pre(?: class="([\w-]+)")?><code>(.*?)</code></pre>',
        block,
        fragment,
        flags=re.S,
    )


def toc_entries(fragment: str):
    """[(level, id, plain text)] for h2/h3, from the namespaced fragment."""
    entries = []
    for level, ident, inner in re.findall(
        r'<h([23]) id="([^"]+)">(.*?)</h\1>', fragment, flags=re.S
    ):
        entries.append((int(level), ident, html.unescape(re.sub(r"<[^>]+>", "", inner))))
    return entries


CSS = """
:root{--bg:#fbfbf9;--fg:#1f2328;--muted:#656d76;--line:#d8dee4;--accent:#0b5cad;
--code:#f3f4f1;--kw:#8250df;--str:#0a7a3d;--num:#b35900;--com:#6e7781;--side:#f4f5f2}
@media (prefers-color-scheme:dark){:root:not([data-theme=light]){--bg:#15181c;--fg:#d8dde3;
--muted:#8b949e;--line:#2d333b;--accent:#6cb2ff;--code:#1c2128;--kw:#c297ff;--str:#7ee2a0;
--num:#f0a45d;--com:#7d8590;--side:#1a1e23}}
:root[data-theme=dark]{--bg:#15181c;--fg:#d8dde3;--muted:#8b949e;--line:#2d333b;--accent:#6cb2ff;
--code:#1c2128;--kw:#c297ff;--str:#7ee2a0;--num:#f0a45d;--com:#7d8590;--side:#1a1e23}
*{box-sizing:border-box}html{scroll-behavior:smooth;scroll-padding-top:1rem}
body{margin:0;background:var(--bg);color:var(--fg);font:16px/1.7 system-ui,"Segoe UI",
"PingFang SC","Noto Sans SC","Microsoft YaHei",sans-serif}
.layout{display:grid;grid-template-columns:19rem minmax(0,1fr);min-height:100vh}
nav.side{position:sticky;top:0;height:100vh;overflow:auto;padding:1.2rem 1rem;background:var(--side);
border-right:1px solid var(--line);font-size:.9rem}
nav.side h1{font-size:1.05rem;margin:0 0 .2rem}nav.side .meta{color:var(--muted);margin:0 0 1rem;font-size:.8rem}
nav.side a{color:inherit;text-decoration:none;display:block;padding:.15rem .4rem;border-radius:4px}
nav.side a:hover{background:var(--line)}nav.side a.on{color:var(--accent);font-weight:600}
nav.side details>summary{cursor:pointer;list-style:none}nav.side details a.sub{padding-left:1.4rem;
font-size:.82rem;color:var(--muted)}.tools{display:flex;gap:.5rem;margin:.6rem 0 1rem}
.tools a,.tools button{font:inherit;font-size:.8rem;padding:.15rem .6rem;border:1px solid var(--line);
border-radius:999px;background:transparent;color:var(--fg);cursor:pointer;text-decoration:none}
main{padding:2rem clamp(1rem,4vw,3.5rem) 6rem;max-width:62rem}
section.chapter{margin-bottom:4rem;padding-top:.5rem}
h1,h2,h3,h4{line-height:1.3}h2{margin-top:2.2rem;border-bottom:1px solid var(--line);padding-bottom:.25rem}
a{color:var(--accent)}code{background:var(--code);padding:.08em .3em;border-radius:4px;font-size:.9em;
font-family:ui-monospace,"Cascadia Code",Consolas,monospace}
pre.code{background:var(--code);padding:.8rem 1rem;border-radius:6px;overflow:auto;position:relative;
border:1px solid var(--line);line-height:1.5}pre.code code{background:none;padding:0;font-size:.86rem}
pre.code[data-lang]::before{content:attr(data-lang);position:absolute;top:.2rem;right:.6rem;
font-size:.68rem;color:var(--muted)}
pre.code button{position:absolute;bottom:.3rem;right:.4rem;font-size:.7rem;opacity:0;
border:1px solid var(--line);background:var(--bg);color:var(--fg);border-radius:4px;cursor:pointer}
pre.code:hover button{opacity:1}
.kw{color:var(--kw)}.s{color:var(--str)}.n{color:var(--num)}.c{color:var(--com);font-style:italic}
table{border-collapse:collapse;display:block;overflow:auto;margin:1rem 0}
th,td{border:1px solid var(--line);padding:.35rem .7rem;text-align:left;vertical-align:top}
th{background:var(--side)}blockquote{margin:1rem 0;padding:.1rem 1rem;border-left:4px solid var(--line);
color:var(--muted)}.fallback{padding:.5rem 1rem;border:1px dashed var(--line);border-radius:6px;
color:var(--muted)}
@media (max-width:60rem){.layout{display:block}nav.side{position:static;height:auto;
border-right:0;border-bottom:1px solid var(--line)}}
@media print{nav.side,.tools,pre.code button{display:none}.layout{display:block}
main{max-width:none;padding:0}section.chapter{break-before:page}a{color:inherit}
pre.code{white-space:pre-wrap;break-inside:avoid}}
"""

JS = """
(function(){
var t=document.documentElement,k='aevia-spec-theme';
try{var s=localStorage.getItem(k);if(s)t.dataset.theme=s}catch(e){}
var tb=document.getElementById('theme');
if(tb)tb.onclick=function(){var d=(t.dataset.theme||(matchMedia('(prefers-color-scheme:dark)').matches?'dark':'light'))==='dark'?'light':'dark';
t.dataset.theme=d;try{localStorage.setItem(k,d)}catch(e){}};
document.querySelectorAll('pre.code').forEach(function(p){var b=document.createElement('button');
b.textContent=COPY;b.onclick=function(){navigator.clipboard&&navigator.clipboard.writeText(p.querySelector('code').innerText).then(function(){b.textContent=COPIED;
setTimeout(function(){b.textContent=COPY},1200)})};p.appendChild(b)});
var links=[].slice.call(document.querySelectorAll('nav.side a[data-ch]'));
var chs=[].slice.call(document.querySelectorAll('section.chapter'));
function spy(){var cur=chs[0];chs.forEach(function(c){if(c.getBoundingClientRect().top<120)cur=c});
links.forEach(function(a){var on=cur&&a.dataset.ch===cur.id;a.classList.toggle('on',on);
if(on){var d=a.closest('details');if(d&&!d.open)d.open=true}})}
addEventListener('scroll',spy,{passive:true});spy();
})();
"""


def build_html(lang: str, chapters, revision: str, pandoc: str) -> str:
    t = TEXT[lang]
    rendered = []
    known = {}
    for name, slug, text, fallback in chapters:
        # Drop the chapter's own H1: the section header supplies it.
        title = first_heading(text, name)
        body = re.sub(r"^# .*\n", "", text, count=1)
        fragment = pandoc_fragment(pandoc, body)
        known[slug] = set(re.findall(r'\bid="([^"]+)"', fragment))
        rendered.append((slug, title, fragment, fallback))
    sections, nav = [], []
    for slug, title, fragment, fallback in rendered:
        fragment = namespace(fragment, slug, known)
        fragment = finish_code(fragment)
        entries = [e for e in toc_entries(fragment) if e[0] == 2]
        notice = f'<p class="fallback">{html.escape(t["fallback"])}</p>' if fallback else ""
        sections.append(
            f'<section class="chapter" id="ch-{slug}"><h1>{html.escape(title)}</h1>'
            f"{notice}{fragment}</section>"
        )
        subs = "".join(
            f'<a class="sub" href="#{ident}">{html.escape(text)}</a>'
            for _, ident, text in entries
        )
        label = html.escape(short(title))
        nav.append(
            f'<details><summary><a data-ch="ch-{slug}" href="#ch-{slug}">{label}</a></summary>{subs}</details>'
            if subs
            else f'<a data-ch="ch-{slug}" href="#ch-{slug}">{label}</a>'
        )
    js = JS.replace("COPIED", repr(t["copied"])).replace("COPY", repr(t["copy"]))
    return f"""<!DOCTYPE html>
<html lang="{t['html_lang']}">
<head><meta charset="utf-8"><meta name="viewport" content="width=device-width,initial-scale=1">
<title>{html.escape(t['title'])} — Revision {revision}</title>
<style>{CSS}</style></head>
<body><div class="layout">
<nav class="side" aria-label="{html.escape(t['contents'])}">
<h1>{html.escape(t['title'])}</h1><p class="meta">{html.escape(t['edition'])} {revision}</p>
<div class="tools"><a href="{t['other_href']}">{t['other']}</a><button id="theme" type="button">◐</button></div>
{''.join(nav)}</nav>
<main>{''.join(sections)}</main></div>
<script>{js}</script></body></html>
"""


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--revision", default=None)
    parser.add_argument("--no-html", action="store_true")
    parser.add_argument("--lang", choices=["zh", "en"], default=None)
    args = parser.parse_args()
    revision = detect_revision(args.revision)
    pandoc = shutil.which("pandoc")
    for lang in [args.lang] if args.lang else ["zh", "en"]:
        chapters = load_chapters(lang)
        md = ROOT / out_name(lang, "md")
        md.write_text(build_markdown(lang, chapters, revision), encoding="utf-8", newline="\n")
        translated = sum(1 for c in chapters if not c[3])
        print(f"wrote {md.name} ({translated}/{len(chapters)} chapters in {lang}, revision {revision})")
        if args.no_html:
            continue
        if pandoc is None:
            print("pandoc not found: the HTML edition was not regenerated", file=sys.stderr)
            continue
        page = ROOT / out_name(lang, "html")
        page.write_text(build_html(lang, chapters, revision, pandoc), encoding="utf-8", newline="\n")
        print(f"wrote {page.name}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
