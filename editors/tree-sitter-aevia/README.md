# Aevia Tree-sitter grammar / Aevia Tree-sitter 语法

The grammar supplies lexical highlighting for [Helix](../helix/README.md) and
[Zed](../zed/README.md). It highlights ordinary, raw and multiline strings,
including escape sequences and nested interpolation. Ordinary strings with a
missing closing quote recover at the line break. Quantity suffixes may cross
a line break inside brackets or interpolation, where the compiler suspends
layout. The Aevia language server provides compiler diagnostics and semantic
tokens.

这套语法为 [Helix](../helix/README.md) 和 [Zed](../zed/README.md) 提供词法高亮。
支持普通、原始和多行字符串，以及转义和嵌套插值。普通字符串缺少闭合引号时，
高亮在换行后恢复。编译器在括号或插值内暂停布局检查，量纲后缀也可在这些位置
跨行。编译器诊断和语义标记由 Aevia 语言服务器提供。

From this directory, run Tree-sitter CLI 0.25.10 to regenerate the checked-in
parser and verify the grammar and highlight queries.

在当前目录使用 Tree-sitter CLI 0.25.10 重新生成已纳入版本控制的解析器，
并验证语法和高亮查询。

```sh
tree-sitter generate
tree-sitter test
cc -std=c11 -fsyntax-only -I src src/parser.c src/scanner.c
```
