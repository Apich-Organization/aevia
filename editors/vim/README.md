# Aevia for Vim and Neovim / Aevia 的 Vim 与 Neovim 支持

Add `editors/vim` to `runtimepath`, then enable filetype plugins and syntax
highlighting. The legacy syntax rules highlight ordinary, raw and triple-quoted
strings, including escapes and nested interpolation. Unclosed ordinary or raw
string text ends at the line break. Interpolation can span lines.

将 `editors/vim` 加入 `runtimepath`，并启用文件类型插件和语法高亮。Vim 语法
规则支持普通、原始和三引号字符串，以及转义和嵌套插值。未闭合的普通或原始
字符串文本到换行处结束。插值可以跨行。

Vim and Neovim cache syntax state after scanning from the start of the buffer.
This lets a jump after a long multiline string recover the correct highlighting.
Run the shared string corpus checks from the repository root.

Vim 和 Neovim 从缓冲区开头扫描并缓存语法状态，跳到长多行字符串之后也能
恢复正确的高亮。从仓库根目录运行以下共享字符串语料测试。

```sh
vim -Nu NONE -i NONE -n -es -S editors/vim/test/strings.vim
nvim --headless -u NONE -i NONE -n -S editors/vim/test/strings.vim
```
