-- Neovim 0.11+ native LSP config: `vim.lsp.enable('aevia')` after adding this
-- directory to 'runtimepath' (the file is picked up from `lsp/`).
return {
  cmd = { 'aevia-lsp', '--stdio' },
  filetypes = { 'aevia' },
  -- Package root is the directory with `aevia.toml` (spec 7); fall back to git.
  root_markers = { 'aevia.toml', '.git' },
}
