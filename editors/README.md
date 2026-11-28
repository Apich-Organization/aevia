# Aevia editor support / Aevia 编辑器支持

Install `aevia` and `aevia-lsp` on your editor's `PATH`. Use `aevia lsp --stdio`
in a client that accepts a custom server command if you installed the CLI alone.
The server supplies diagnostics, hover, navigation, completion, signature help,
rename, formatting, quick fixes, semantic highlighting for whole documents or
visible ranges, and document-local symbol highlights for `.aev` files.

将 `aevia` 和 `aevia-lsp` 安装到编辑器的 `PATH` 中。若只安装了 CLI，可在
支持自定义服务器命令的客户端中配置 `aevia lsp --stdio`。语言服务器为
`.aev` 文件提供诊断、悬停提示、符号跳转、补全、签名提示、重命名、格式化、
快速修复、整篇文档或指定可见范围的语义高亮，以及文档内的同符号高亮。

| Editor / 编辑器 | Setup / 安装 |
|---|---|
| VS Code | [Extension / 扩展](vscode/README.md) |
| JetBrains IDEs | [Plugin / 插件](jetbrains/README.md) |
| Zed | [Extension / 扩展](zed/README.md) |
| GNU Emacs | [Major mode / 主模式](#emacs) |
| Vim / Neovim | [Runtime files / 运行时文件](#vim--neovim) |
| Helix | [Language and grammar / 语言和语法](helix/README.md) |
| Sublime Text | [Syntax and LSP / 语法和语言服务器](#sublime-text) |
| Kate | [Syntax and LSP / 语法和语言服务器](kate/README.md) |

## Run, test, and inspect / 运行、测试与调试

From the package root, run `aevia run`, `aevia test path/to/file.aev`, or
`aevia repl` in an interactive terminal. Within the REPL, `:help` lists commands
and `:type EXPR` inspects an expression. Editor launchers operate on saved files.

在项目根目录的交互式终端运行 `aevia run`、`aevia test path/to/file.aev`
或 `aevia repl`。在 REPL 中输入 `:help` 查看命令，输入 `:type EXPR` 查看表达式
类型。编辑器中的运行命令使用已保存的文件。

VS Code, JetBrains, Zed, and Kate launch the Aevia debugger from their debugger UI.
Put line breakpoints on executable expressions and use step in, over, or out,
the call stack, and the stopped frame's lexical bindings. Watches and hover
inspect binding names. Debug sessions execute the typed AST serially and take
program input lines from the launch configuration's `stdin` list. Any editor
with a DAP client can start the adapter using `aevia debug --stdio` and a
`launch` request with the source path in `program`.

VS Code、JetBrains、Zed 和 Kate 可从调试界面启动 Aevia 调试器。在可执行表达式所在
的行设置断点，使用步入、步过和步出，并查看调用栈及当前执行帧的词法绑定。
监视和悬停可查看绑定名称对应的值。调试会话串行执行带类型的 AST，程序输入取自
启动配置中的 `stdin` 字符串列表。其他支持 DAP 的编辑器可以启动
`aevia debug --stdio`，并在 `launch` 请求中通过 `program` 传入源文件路径。

## Emacs

Add `editors/emacs` to `load-path`, then enable Eglot or lsp-mode. Run and
test with `C-c C-r` and `C-c C-t`. Open an interactive terminal REPL with
`M-x aevia-repl` or `C-c C-z`. Set `aevia-cli-command` for a custom CLI path.

将 `editors/emacs` 加入 `load-path`，然后启用 Eglot 或 lsp-mode。
`C-c C-r` 运行当前文件，`C-c C-t` 测试，`M-x aevia-repl` 或 `C-c C-z`
打开交互式 REPL。通过 `aevia-cli-command` 配置 CLI 路径。

```elisp
(add-to-list 'load-path "/path/to/aevia/editors/emacs")
(require 'aevia-mode)
(add-hook 'aevia-mode-hook #'eglot-ensure)  ; or lsp-deferred
```

To debug with Dape, save the buffer and select the following configuration
using `M-x dape`. 使用 Dape 调试时，先保存缓冲区，再通过 `M-x dape`
选择以下配置。

```elisp
(with-eval-after-load 'dape
  (add-to-list 'dape-configs
               '(aevia modes (aevia-mode)
                       command "aevia"
                       command-args ("debug" "--stdio")
                       :request "launch"
                       :program dape-buffer-default)))
```

## Vim / Neovim

Add `editors/vim` to `runtimepath` and enable filetype plugins. In an Aevia
buffer, use `:AeviaRun`, `:AeviaTest`, and `:AeviaRepl`. Set `g:aevia_cli`
to select the CLI executable. Neovim 0.11 or later starts Aevia's LSP client
on file open. Set `g:aevia_lsp_autostart` to `v:false` for manual setup.
Use vim-lsp or yegappan/lsp in Vim. With coc.nvim, configure the server as follows.

将 `editors/vim` 加入 `runtimepath` 并启用文件类型插件。在 Aevia 缓冲区使用
`:AeviaRun`、`:AeviaTest` 和 `:AeviaRepl`，可用 `g:aevia_cli` 指定 CLI。
Neovim 0.11 及以上版本打开文件时会启动 LSP；将 `g:aevia_lsp_autostart` 设为
`v:false` 可改用手动配置。Vim 支持 vim-lsp 或 yegappan/lsp。coc.nvim 可使用
以下服务器配置。

```json
{
  "languageserver": {
    "aevia": {
      "command": "aevia-lsp",
      "args": ["--stdio"],
      "filetypes": ["aevia"],
      "rootPatterns": ["aevia.toml"]
    }
  }
}
```

With nvim-dap installed, add the configuration below and use
`:DapToggleBreakpoint` and `:DapContinue` on a saved Aevia source file.
安装 nvim-dap 后，添加以下配置，并在已保存的源文件中使用
`:DapToggleBreakpoint` 和 `:DapContinue`。

```lua
local dap = require('dap')
dap.adapters.aevia = { type = 'executable', command = 'aevia', args = { 'debug', '--stdio' } }
dap.configurations.aevia = {
  { type = 'aevia', request = 'launch', name = 'Debug Aevia file', program = '${file}' },
}
```

## Sublime Text

Install the LSP package. From **Preferences → Browse Packages**, copy or
symlink [Aevia](sublime/Aevia) and [LSP-aevia](sublime/LSP-aevia) into
`Packages/`. Save the source before invoking **Build** or selecting **Test**
under **Tools → Build With**. Install Terminus to open an interactive terminal
with **Aevia: Open REPL**.

安装 LSP 包。打开 **Preferences → Browse Packages**，将
[Aevia](sublime/Aevia) 和 [LSP-aevia](sublime/LSP-aevia) 复制或链接到
`Packages/`。保存源文件后使用 **Build** 运行，或在 **Tools → Build With**
下选择 **Test**。安装 Terminus 后，可用 **Aevia: Open REPL** 打开交互式终端。

## Verification / 验证

Run the protocol smoke checks against your toolchain build.
使用构建好的工具链运行协议冒烟测试。

```sh
node editors/test/lsp-smoke.js target/debug/aevia-lsp
node editors/test/dap-smoke.js target/debug/aevia
npm --prefix editors/vscode test
```
