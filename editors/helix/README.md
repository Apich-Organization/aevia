# Aevia for Helix / Aevia 的 Helix 配置

Install the language definition and queries with Helix, Git, and a C/C++
toolchain on `PATH`. Put `aevia-lsp` on `PATH` for hover and diagnostics.

安装需要在 `PATH` 中提供 Helix、Git 和 C/C++ 工具链。将 `aevia-lsp` 加入
`PATH` 后即可使用悬停文档和诊断。

```sh
sh editors/helix/scripts/install.sh --build
hx --health aevia
```

On Windows use PowerShell 7. / Windows 上使用 PowerShell 7。

```powershell
pwsh -File editors/helix/scripts/install.ps1 -Build
hx --health aevia
```

The build flag compiles only the Aevia parser with a temporary Helix
configuration and installs it under `runtime/grammars`. Omit the flag to
install the language definition and queries alone. Pass a custom configuration
directory as the last argument in sh or with `-ConfigDir` in PowerShell. Check
the paths Helix uses with `hx --health`.

构建参数使用临时 Helix 配置，仅编译 Aevia 解析器，并将其安装到
`runtime/grammars`。省略构建参数可单独安装语言定义和查询。自定义配置目录
可作为 sh 脚本的最后一个参数，或通过 PowerShell 的 `-ConfigDir` 指定。
用 `hx --health` 查看 Helix 使用的路径。

Run `sh editors/helix/test/install.sh` on Linux or macOS to verify installation
with an existing grammar selection and check the loaded parser and queries.

在 Linux 或 macOS 上运行 `sh editors/helix/test/install.sh`，可验证已有语法选择
时的安装，以及 Helix 加载的解析器和查询。

Run `:write` to save a source file, then use the shell or another terminal
to run, test, and open a REPL. From the package root, `:sh aevia run` runs
the package entry. Hover documentation is available with `Space` + `k`.

使用 `:write` 保存源文件，再通过 shell 或其他终端运行、测试或打开 REPL。
在包的根目录可以用 `:sh aevia run` 运行入口。按 `Space` + `k` 查看
悬停文档。

To debug an Aevia file, put `aevia` on `PATH`, save the file, set a breakpoint,
and launch the template with `:debug-start source path/to/main.aev`.

调试 Aevia 文件时，将 `aevia` 加入 `PATH`，保存并设置断点，再通过
`:debug-start source path/to/main.aev` 启动调试模板。
