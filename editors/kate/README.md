# Aevia for Kate

KSyntaxHighlighting XML definition (`aevia.xml`) and built-in LSP Client configuration (`lspclient-settings.json`) for hover documentation, diagnostics, completion, and formatting.

## Installation

1. Copy `aevia.xml` to:
   - Windows: `%LOCALAPPDATA%\org.kde.syntax-highlighting\syntax\`
   - Linux: `~/.local/share/org.kde.syntax-highlighting/syntax/`
2. Merge `lspclient-settings.json` into Kate's LSP Client settings (*Settings → Configure Kate → LSP Client → User Server Settings*) and restart Kate.

## Running & Testing `.aev` Files

Configure Kate's **External Tools** plugin (*Settings → Configure Kate → Plugins → External Tools*, then *Settings → Configure Kate → External Tools → Add*):

- **Aevia Run**
  - Executable: `aevia`
  - Arguments: `run "%{Document:FilePath}"`
  - Working directory: `%{Document:Path}`
  - Save: Current Document
  - Output: Display in Pane
- **Aevia Test**
  - Executable: `aevia`
  - Arguments: `test "%{Document:FilePath}"`
  - Working directory: `%{Document:Path}`
  - Save: Current Document
  - Output: Display in Pane

Hover documentation is provided automatically by `aevia-lsp` when the cursor rests on a symbol.
