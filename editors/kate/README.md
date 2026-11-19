# Aevia for Kate / Aevia 的 Kate 配置

1. Copy [aevia.xml](aevia.xml) into the KSyntaxHighlighting syntax directory.
   将 [aevia.xml](aevia.xml) 复制到 KSyntaxHighlighting 的语法目录。
   - Linux `~/.local/share/org.kde.syntax-highlighting/syntax/`
   - Windows `%LOCALAPPDATA%\org.kde.syntax-highlighting\syntax\`
2. Enable **LSP Client**, then merge [lspclient-settings.json](lspclient-settings.json)
   into **LSP Client → User Server Settings** and restart Kate.
   启用 **LSP Client**，将 [lspclient-settings.json](lspclient-settings.json) 合并到
   **LSP Client → User Server Settings**，然后重启 Kate。

Enable **External Tools** to configure run and test actions. Use executable
`aevia`, arguments from the table, working directory `%{Document:Path}`, and
choose to save the current document and display output in a pane.

启用 **External Tools** 后可配置运行和测试动作。可执行文件设为 `aevia`，
参数见下表，工作目录设为 `%{Document:Path}`。选择保存当前文档并在面板显示输出。

| Action / 动作 | Arguments / 参数 |
|---|---|
| Aevia Run | `run "%{Document:FilePath}"` |
| Aevia Test | `test "%{Document:FilePath}"` |

For source debugging, enable Kate's **GDB** plugin and set its user
configuration file to [dap-settings.json](dap-settings.json). Select **aevia /
launch** in the Debug settings and set the Aevia source file. In Kate's
Terminal tool view, run `aevia repl` for an interactive session.

调试源码时，启用 Kate 的 **GDB** 插件，并将其用户配置文件设为
[dap-settings.json](dap-settings.json)。在 Debug 设置中选择 **aevia /
launch** 并指定 Aevia 源文件。可在 Kate 的 Terminal 面板运行 `aevia repl`
进入交互式会话。
