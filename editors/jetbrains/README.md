# Aevia for JetBrains IDEs

* Highlighting: the bundled **TextMate** plugin, fed the same grammar as VS Code
  (`../vscode/syntaxes`, copied in at build time).
* Language server: **[LSP4IJ](https://plugins.jetbrains.com/plugin/23257-lsp4ij)**, which works in every
  JetBrains IDE (Community editions included), unlike the native LSP API of the paid ones.

The server is `$AEVIA_LSP` if set, else `aevia-lsp --stdio` from `PATH`, else `aevia lsp --stdio`.
Hover documentation, diagnostics, completion, formatting, and semantic tokens are provided by `aevia-lsp`.

## Running & Testing `.aev` Files

* **Run Aevia File** (`aevia run <file>`): *Run → Run Aevia File*, editor context menu, or `Ctrl+Alt+R`.
* **Test Aevia File** (`aevia test <file>`): *Run → Test Aevia File*, editor context menu, or `Ctrl+Alt+T`.
* The `aevia` CLI is resolved from `$AEVIA_CLI`, then `PATH`, then `~/.cargo/bin` / `~/.local/bin` / `target/`.

## Build

```bash
# against an installed IDE (no download), e.g. RustRover:
AEVIA_IDE_PATH="C:/Program Files/JetBrains/RustRover 2025.2" gradle buildPlugin
# or against IntelliJ IDEA Community 2025.2 (downloaded):
gradle buildPlugin
```
The zip lands in `build/distributions/`. Install with *Settings → Plugins → ⚙ → Install Plugin from Disk…*
(LSP4IJ is pulled in as a dependency from the Marketplace). `gradle runIde` starts a sandbox IDE.

Language-server diagnostics and the server log are under *View → Tool Windows → Language Servers*.
