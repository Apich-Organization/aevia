# Aevia for Zed

Tree-sitter syntax highlighting, outline, runnables, and `aevia-lsp` language server integration (hover documentation, diagnostics, completion, formatting, semantic tokens).

## Development Install

1. Run `pwsh editors/zed/scripts/prepare-dev.ps1`.
2. In Zed, open the Command Palette and run `zed: install dev extension`, then select `editors/zed`.

## Running & Testing `.aev` Files

`languages/aevia/runnables.scm` tags `fn main` (`aevia-run`) and `test` blocks (`aevia-test`) for Zed's runnable gutter buttons.

Add the following to your Zed `tasks.json` (`zed: open tasks` from the Command Palette) to bind those tags and run/test any `.aev` file:

```json
[
  {
    "label": "aevia run",
    "command": "aevia",
    "args": ["run", "$ZED_FILE"],
    "tags": ["aevia-run"]
  },
  {
    "label": "aevia test",
    "command": "aevia",
    "args": ["test", "$ZED_FILE"],
    "tags": ["aevia-test"]
  }
]
```

## Hover Documentation & Semantic Tokens

Hover documentation (`K` / mouse hover) is provided automatically by `aevia-lsp`.
For semantic token highlighting, optionally add to Zed's `settings.json`:

```json
{
  "languages": {
    "Aevia": {
      "semantic_tokens": "combined"
    }
  }
}
```
