# Aevia for VS Code / Aevia 的 VS Code 扩展

Edit, run, test, and debug Aevia source in VS Code. Install the Aevia CLI and
language server on the same host as the extension. The editor picks up
`.aev` files and provides highlighting, diagnostics, hover information,
completions, navigation, edits, and formatting.

在 VS Code 中编辑、运行、测试和调试 Aevia 源码。请将 Aevia CLI 和语言服务器
安装到扩展所在的主机上。打开 `.aev` 文件即可使用高亮、诊断、悬停提示、补全、
跳转、编辑和格式化。

String highlighting covers ordinary, raw and triple-quoted strings, including
escapes and nested interpolation. Unclosed ordinary or raw string text ends at
the line break. Interpolation expressions can continue across lines. The
language server reports invalid escapes and interpolation expressions.

字符串高亮支持普通、原始和三引号字符串，也识别转义和嵌套插值。未闭合的普通或
原始字符串文本到换行处结束。插值表达式可以跨行。语言服务器会报告无效的转义和
插值表达式。

| Command / 命令 | Behavior / 功能 |
|---|---|
| **Aevia: Run Current File** | Saves and runs the file in a task terminal. / 保存后在任务终端中运行。 |
| **Aevia: Test Current File** | Saves and runs tests. An individual test's CodeLens runs only that test name. / 保存后运行测试。单个测试的 CodeLens 按精确名称运行该测试。 |
| **Aevia: Debug Current File** | Launches the debugger for the saved file. Set a line breakpoint before launching. / 调试已保存的当前文件，可在启动前设置行断点。 |
| **Aevia: Open REPL** | Opens an interactive terminal in the package directory. / 在包目录中打开交互式终端。 |
| **Aevia: Restart Language Server** | Restarts the connection without closing open editors. / 重启语言服务器连接。 |

## Debug configuration / 调试配置

Add an Aevia launch configuration under `configurations` in `.vscode/launch.json`.
`args` goes to the program and `stdin` supplies lines to read. Use breakpoints,
stepping, the call stack and the Variables view to inspect a run.

在 `.vscode/launch.json` 的 `configurations` 列表中加入 Aevia 启动配置。
`args` 是传给程序的参数，`stdin` 为程序提供输入行。可通过断点、单步、调用栈
和 Variables 面板查看运行状态。

```json
{
  "type": "aevia",
  "request": "launch",
  "name": "Debug Aevia File",
  "program": "${file}",
  "args": [],
  "stdin": [],
  "stopOnEntry": true
}
```

## Settings and installation / 设置与安装

Configure `aevia.server.path` and `aevia.cli.path` in your user or remote settings
when tools are outside `PATH`. VS Code activates the extension in trusted
workspaces.

当工具未加入 `PATH` 时，在用户设置或远程设置中指定 `aevia.server.path`
和 `aevia.cli.path`。扩展在受信任的工作区中启动。

Download `aevia-<version>.vsix` from the release assets or extract it from the
`aevia-editor-extensions` artifact of a [CI run][ci]. For a source build, install
Node.js 22 or later and run `bazel run //:editor_packages -- --editor vscode`
from the repository root. Open **Extensions: Install from VSIX** and select the
`.vsix` in `dist/editor-extensions`.

从发布附件下载 `aevia-<版本>.vsix`，或从 [CI 运行][ci]的
`aevia-editor-extensions` 构建产物中解压该文件。若要从源码构建，请安装
Node.js 22 或更新版本，并在仓库根目录运行
`bazel run //:editor_packages -- --editor vscode`。打开
**Extensions: Install from VSIX** 并选择 `dist/editor-extensions` 中的 `.vsix`。

Run `bazel test //:editor_vscode_test` to verify the extension commands and
TextMate highlighting.

运行 `bazel test //:editor_vscode_test` 验证扩展命令和 TextMate 高亮。

[ci]: https://github.com/Apich-Organization/aevia/actions/workflows/ci.yml
