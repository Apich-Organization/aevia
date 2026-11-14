# Editor support for Aevia

Every plugin here does two things: highlight `.aev` files, and start the
language server (`aevia-lsp --stdio`, or `aevia lsp --stdio`; spec §9.10).
All semantic features (diagnostics, hover, completion, go to definition and
type definition, references, rename, signature help, formatting, code
actions, document/workspace symbols, semantic tokens) come from the server,
so none of the plugins re-implement them.

**Prerequisite for all of them:** `aevia-lsp` (or `aevia`) on `PATH`.

```bash
cargo build --release -p aevia-lsp      # produces target/release/aevia-lsp
```

| Editor | Directory | Highlighting | LSP client | Status |
|---|---|---|---|---|
| VS Code | [`vscode/`](vscode) | TextMate | `vscode-languageclient` | launched with the extension; server starts; grammar tested |
| GNU Emacs | [`emacs/`](emacs) | font-lock | Eglot / lsp-mode | tested in Emacs 31: highlighting, indentation, Eglot connects and shows server diagnostics; lsp-mode untested |
| Vim / Neovim | [`vim/`](vim) | syntax file | vim-lsp, yegappan/lsp, coc, Neovim native | syntax/ftdetect/indent tested in Vim 9; Neovim and LSP clients untested (not installed) |
| Zed | [`zed/`](zed) | tree-sitter | extension (wasm) | extension builds for `wasm32-wasip2`, queries checked; not loaded in Zed yet |
| Helix | [`helix/`](helix) | tree-sitter | built in | queries checked against the grammar; Helix not installed here |
| Sublime Text | [`sublime/`](sublime) | `.sublime-syntax` | `LSP` package | YAML validated; not loaded in Sublime yet |
| Kate | [`kate/`](kate) | KSyntaxHighlighting XML | built-in LSP client | XML well-formed; not loaded in Kate yet |
| JetBrains | [`jetbrains/`](jetbrains) | bundled TextMate | LSP4IJ | compiles against RustRover 2025.2 and the zip contains the TextMate bundle; not loaded in an IDE yet |

[`tree-sitter-aevia/`](tree-sitter-aevia) is the grammar behind Zed and
Helix. It is deliberately *lexical*: Aevia is layout-sensitive and a number
followed by identifiers is a unit literal (spec §1.8.1), so the grammar tokenizes
and recognises declaration heads, and never reports a syntax error — the
server owns those. `src/parser.c` is generated and checked in, so consumers
need no `tree-sitter` CLI; regenerate with `tree-sitter generate`.

[`test/lsp-smoke.js`](test/lsp-smoke.js) drives the server the way an editor
does (initialize, hover, symbols, completion, semantic tokens, formatting, a
diagnostic on a dimension error) and is the quickest way to tell a server
problem from a plugin problem:

```bash
node editors/test/lsp-smoke.js target/release/aevia-lsp
```

## Semantic token legend

The server advertises `type, dimension, unit, operator, function, variable,
enumMember`. `dimension` and `unit` are not standard LSP token types; the
plugins map them to `type`-like highlights (VS Code declares them with
`superType: type`; Neovim links `@lsp.type.dimension`/`@lsp.type.unit`).

## Per-editor quick start

### VS Code
```bash
cd editors/vscode && npm install
code --extensionDevelopmentPath "$PWD"          # try it
npx @vscode/vsce package                        # aevia-0.1.0.vsix, then: code --install-extension aevia-0.1.0.vsix
```
Settings: `aevia.server.path`, `aevia.server.args`, `aevia.trace.server`.

### GNU Emacs
```elisp
(add-to-list 'load-path "/path/to/aevia/editors/emacs")
(require 'aevia-mode)
(add-hook 'aevia-mode-hook #'eglot-ensure)       ; or #'lsp-deferred
```

### Vim / Neovim
Add `editors/vim` to `runtimepath` (or copy its subdirectories into `~/.vim` / `~/.config/nvim`).
Neovim 0.11+ starts the server by itself (`vim.g.aevia_lsp_autostart = false` to disable);
`nvim-lspconfig`, vim-lsp and yegappan/lsp are registered when present. For coc.nvim add to `coc-settings.json`:
```json
{ "languageserver": { "aevia": { "command": "aevia-lsp", "args": ["--stdio"], "filetypes": ["aevia"], "rootPatterns": ["aevia.toml"] } } }
```

### Zed
`pwsh editors/zed/scripts/prepare-dev.ps1`, then command palette → `zed: install dev extension` → pick `editors/zed`.
Optional, for semantic highlighting, in Zed `settings.json`:
```json
{ "languages": { "Aevia": { "semantic_tokens": "combined" } } }
```

### Helix
`pwsh editors/helix/scripts/install.ps1` (or `install.sh`), then `hx --grammar build` and `hx --health aevia`.

### Sublime Text
Install the `LSP` package (Package Control), then copy or symlink `sublime/Aevia` and `sublime/LSP-aevia`
into `Packages/` (`Preferences → Browse Packages…`).

### Kate
Copy `kate/aevia.xml` to `%LOCALAPPDATA%\org.kde.syntax-highlighting\syntax\` (Linux: `~/.local/share/org.kde.syntax-highlighting/syntax/`),
merge `kate/lspclient-settings.json` into Kate's LSP Client settings (Settings → Configure Kate → LSP Client → User Server Settings), restart Kate.

### JetBrains (IntelliJ IDEA, RustRover, PyCharm, CLion, …)
See [`jetbrains/README.md`](jetbrains/README.md).
