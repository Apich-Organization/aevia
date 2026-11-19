# Aevia for JetBrains IDEs / Aevia 的 JetBrains 插件

Use a JetBrains IDE from the 2025.2 series or newer with the TextMate and
[LSP4IJ](https://plugins.jetbrains.com/plugin/23257-lsp4ij) plugins enabled.
The Aevia plugin uses LSP4IJ for editor language features and source debugging.

使用 2025.2 系列或更新版本的 JetBrains IDE，并启用 TextMate 和
[LSP4IJ](https://plugins.jetbrains.com/plugin/23257-lsp4ij) 插件。
Aevia 插件通过 LSP4IJ 提供代码编辑和源码调试功能。

## Run and debug / 运行与调试

- **Run Aevia File** and **Test Aevia File** are in the Run and editor context menus.
  **Run Aevia File** 和 **Test Aevia File** 可从 Run 菜单或编辑器上下文菜单启动。
- **Open Aevia REPL** starts a terminal with the CLI. Enable the IDE Terminal plugin to use it.
  **Open Aevia REPL** 在终端中启动 CLI，需要启用 IDE 的 Terminal 插件。
- To debug, create a **Debug Adapter Protocol** configuration and select the **Aevia**
  server. Set the `.aev` file and place a breakpoint on an executable line.
  调试时创建 **Debug Adapter Protocol** 配置，选择 **Aevia** 服务器，指定
  `.aev` 文件并在可执行行上设置断点。

The IDE looks for the tools on `PATH`, in `~/.cargo/bin`, and in
`~/.local/bin`. Use `AEVIA_LSP` or `AEVIA_CLI` to select an executable.
Open **View → Tool Windows → Language Servers** for the server log.

IDE 会在 `PATH`、`~/.cargo/bin` 和 `~/.local/bin` 中查找工具。
可通过 `AEVIA_LSP` 或 `AEVIA_CLI` 指定可执行文件。在
**View → Tool Windows → Language Servers** 中查看服务器日志。

## Build and install / 构建与安装

Download `aevia-jetbrains-<version>.zip` from the release assets or extract it
from the `aevia-editor-extensions` artifact of a [CI run][ci]. Open
**Settings → Plugins → Install Plugin from Disk** and select the ZIP.

从发布附件下载 `aevia-jetbrains-<版本>.zip`，或从 [CI 运行][ci]的
`aevia-editor-extensions` 构建产物中解压该文件。打开
**Settings → Plugins → Install Plugin from Disk** 并选择 ZIP 文件。

To build from source with JDK 21 and Gradle 8.14.3, run `gradle buildPlugin`
from `editors/jetbrains`. The ZIP appears in `build/distributions/`. To build
against a local IDE distribution, set `AEVIA_IDE_PATH` to its installation directory.

若要从源码构建，请使用 JDK 21 和 Gradle 8.14.3，在 `editors/jetbrains`
中运行 `gradle buildPlugin`。ZIP 文件会生成在 `build/distributions/` 下。
若要使用本地 IDE 构建，将 `AEVIA_IDE_PATH` 设为其安装目录。

[ci]: https://github.com/Apich-Organization/aevia/actions/workflows/ci.yml
