# Authoring and translation guide

This file is for people (and agents) who edit the specification. It is not part of the
bundled book.

## Layout

| Path | Role |
|---|---|
| `spec/NN-name.md`, `spec/appendix-*.md`, `spec/README.md` | the **Chinese** text — the source of truth |
| `spec/en/<same file name>` | the **English** edition of the same chapter |
| `tools/bundle_spec.py` | builds `Aevia-Language-Specification.{zh,en}.{md,html}` |
| `platform/crates/aevia-registry-server/src/spec.rs` | serves both editions on `/spec` |

A chapter's English file has the same name, the same section numbers (`§4.2` is `§4.2` in
both languages) and the same code blocks. Anchors are derived from section numbers, so a
cross-reference works in either language. A chapter without `spec/en/<name>` is shown in
Chinese with a notice; it is never silently dropped.

## Machine-consumed files — change with care

Tests read these files, so a rewrite must keep their shape and pass the test afterwards:

| File | Read by | Constraint |
|---|---|---|
| `08-stdlib.md` | `aevia-std/tests/spec_signatures.rs`, `aevia-stdlib-model/src/spec.rs`, `aevia-core/tests/stdlib_coverage.rs` | the signature tables in §8.1–§8.16 (name column, signature column) |
| `11-examples.md` | `aevia-std/tests/spec_programs.rs` | complete programs in fenced `aevia` blocks; the extraction count is asserted |

Run `cargo test -p aevia-std --test spec_signatures --test spec_programs` after touching them.
Never translate identifiers, signatures or code inside those tables; translate only prose.

## Reviewing a chapter against the implementation

1. Run every `aevia` example (`aevia check`, `aevia run`); fix the text or file a bug — never
   leave an example the toolchain rejects, unless it is marked `compile_fail`.
2. For each normative claim, find the implementing crate (`ARCHITECTURE.md`) and confirm it;
   a claim the implementation does not honour either moves to *Status* (a short "implemented /
   partial / planned" note) or is removed. Diagnostic codes must exist in appendix B.
3. Fill genuine gaps: a rule that is implemented but unwritten, an edge case a test covers.
4. Record what changed in appendix C (one line per chapter), not inside the chapter.

## Condensing

The target is roughly **one third shorter** without losing a rule.

- State each rule once, where it belongs; elsewhere link with `§N.M`.
- Move *why* to chapter 12 and history to appendix C. Normative chapters say what, not why
  (except one sentence of motivation per principle).
- Prefer a table or a typing rule to a paragraph; delete examples that restate another example.
- No "revision N added" asides, no "see the audit". Status is a line, not a story.
- Keep RFC 2119 words only where a requirement is meant.

## Translation rules

- Translate meaning, not sentence structure. English prose is direct and uses the present
  tense; do not mirror Chinese run-on sentences.
- Keep identifiers, code, error codes, section numbers and anchors exactly as in Chinese.
- Chinese headings keep `NN —`; English headings do the same, with sentence case.
- Use the glossary below; add new terms to it in the same commit.
- Section numbers in cross-references are the same in both languages.

## Glossary

| 中文 | English |
|---|---|
| 量纲 | dimension |
| 单位 | unit |
| 量纲代数 | dimension algebra |
| 量纲多态 | dimension polymorphism |
| 无量纲 | dimensionless |
| 基量纲 | base dimension |
| 仿射单位 / 仿射点 | affine unit / affine point |
| 精确 / 近似 | exact / approximate |
| 符号标量 | symbolic scalar |
| 相位（符号/数值） | phase (symbolic / numeric) |
| 类型类 | type class |
| 行多态 | row polymorphism |
| 代数数据类型 | algebraic data type |
| 模式匹配 | pattern matching |
| 穷尽性检查 | exhaustiveness checking |
| 尾调用 | tail call |
| 效果 | effect |
| 布局规则 | layout rule |
| 文档注释 | doc comment |
| 张量 / 秩 / 形状 | tensor / rank / shape |
| 广播 | broadcasting |
| 逐元素 | element-wise |
| 内核（rssn） | kernel |
| 算子 | operator |
| 注册表 | registry |
| 清单（Kernel Manifest） | manifest |
| 插件 | plugin |
| 动态逃逸 | dynamic escape (`Dyn`) |
| 规范性 / 资料性 | normative / informative |
| 必须 / 不得 / 应该 / 可以 | MUST / MUST NOT / SHOULD / MAY |
| 诊断 | diagnostic |
| 能力诊断 | capability diagnostic |
| 解释器 | interpreter |
| 求值（严格） | evaluation (strict) |
| 闭包 | closure |
| 记录 | record |
| 变体 | variant |
| 包 / 清单（aevia.toml） | package / manifest |
| 发布 / 撤回 | publish / yank |
