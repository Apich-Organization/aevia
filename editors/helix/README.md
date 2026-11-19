# Aevia for Helix / Aevia 的 Helix 配置

Install the language definition and tree-sitter queries, build the grammar
with a C compiler, and check the language-server setup.

安装语言定义和 tree-sitter 查询，使用 C 编译器构建语法，再检查语言服务器设置。

```sh
sh editors/helix/scripts/install.sh
hx --grammar build
hx --health aevia
```

On Windows use `pwsh editors/helix/scripts/install.ps1` for the first step.
If your Helix installation uses a custom runtime, pass the configuration
directory as the installer argument and ensure Helix can find that runtime.

Windows 上第一步改用 `pwsh editors/helix/scripts/install.ps1`。若使用自定义的
Helix 运行时，将配置目录传给安装脚本，并确认 Helix 能找到该运行时。

Run `:write` to save a source file, then use the shell or another terminal
to run, test, and open a REPL. From the package root, `:sh aevia run` runs
the package entry. Hover documentation is available with `Space` + `k`.

使用 `:write` 保存源文件，再通过 shell 或其他终端运行、测试或打开 REPL。
在包的根目录可以用 `:sh aevia run` 运行入口。按 `Space` + `k` 查看
悬停文档。

To debug an Aevia file, save it, set a breakpoint, and launch the template
with `:debug-start source path/to/main.aev`.

调试 Aevia 文件时，先保存并设置断点，再通过
`:debug-start source path/to/main.aev` 启动调试模板。
