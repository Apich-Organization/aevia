" Vim 8/9 (not Neovim): register aevia-lsp with the common LSP clients that
" are installed. Neovim is handled by plugin/aevia.lua.
if exists('g:loaded_aevia_plugin') || has('nvim') | finish | endif
let g:loaded_aevia_plugin = 1

" prabirshrestha/vim-lsp (+ vim-lsp-settings is not needed)
augroup aevia_lsp
  autocmd!
  if exists('*lsp#register_server')
    autocmd User lsp_setup call lsp#register_server({
          \ 'name': 'aevia-lsp',
          \ 'cmd': {server_info -> ['aevia-lsp', '--stdio']},
          \ 'allowlist': ['aevia'],
          \ 'root_uri': {server_info -> lsp#utils#path_to_uri(
          \   lsp#utils#find_nearest_parent_file_directory(lsp#utils#get_buffer_path(), ['aevia.toml', '.git/']))},
          \ })
  endif
augroup END

" yegappan/lsp
if exists('*LspAddServer')
  call LspAddServer([{
        \ 'name': 'aevia-lsp',
        \ 'filetype': ['aevia'],
        \ 'path': 'aevia-lsp',
        \ 'args': ['--stdio'],
        \ 'rootSearch': ['aevia.toml', '.git'],
        \ }])
endif
