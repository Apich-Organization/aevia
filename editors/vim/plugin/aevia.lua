-- Neovim only: register the server with nvim-lspconfig when it is present,
-- and (on 0.11+) with the native client. Does nothing in plain Vim (this
-- file is skipped there because of the .lua extension).
-- Token types of the server's semantic tokens that have no built-in highlight group.
local links = {
  ['@lsp.type.dimension'] = '@type',
  ['@lsp.type.unit'] = '@constant',
}
for from, to in pairs(links) do
  vim.api.nvim_set_hl(0, from, { link = to, default = true })
end

if vim.lsp.config and vim.lsp.enable then
  if vim.g.aevia_lsp_autostart ~= false then
    vim.lsp.enable('aevia')
  end
else
  local ok, configs = pcall(require, 'lspconfig.configs')
  if ok and not configs.aevia then
    configs.aevia = {
      default_config = {
        cmd = vim.fn.executable('aevia-lsp') == 1
          and { 'aevia-lsp', '--stdio' }
          or { 'aevia', 'lsp', '--stdio' },
        filetypes = { 'aevia' },
        root_dir = function(fname)
          return vim.fs.root(fname, { 'aevia.toml', '.git' }) or vim.fs.dirname(fname)
        end,
        single_file_support = true,
      },
      docs = { description = 'Aevia language server (aevia-lsp)' },
    }
  end
end
