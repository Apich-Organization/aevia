# Aevia for Helix

Tree-sitter highlighting, textobjects, and `aevia-lsp` integration (hover documentation, diagnostics, completion, formatting, semantic tokens).

## Installation

1. Run `pwsh editors/helix/scripts/install.ps1` (or `bash editors/helix/scripts/install.sh` on Linux/macOS).
2. Build the grammar: `hx --grammar build`.
3. Verify: `hx --health aevia`.

## Running & Testing `.aev` Files

Helix does not ship per-language build configs in `languages.toml`, so you can run `.aev` files directly with `:sh` or bind shortcuts in `~/.config/helix/config.toml` (`%AppData%\helix\config.toml` on Windows):

```toml
[keys.normal]
"C-r" = ":sh aevia run %{buffer_name}"
"C-t" = ":sh aevia test %{buffer_name}"
```

Hover documentation is available with `Space` + `k` (provided by `aevia-lsp`).
