-- Neovim 0.11+ native LSP config: `vim.lsp.enable('aevia')` after adding this
-- directory to 'runtimepath' (the file is picked up from `lsp/`).
local command = vim.fn.executable('aevia-lsp') == 1
  and { 'aevia-lsp', '--stdio' }
  or { 'aevia', 'lsp', '--stdio' }

return {
  cmd = command,
  filetypes = { 'aevia' },
  -- Package root is the directory with `aevia.toml` (spec 7); fall back to git.
  root_markers = { 'aevia.toml', '.git' },
}
