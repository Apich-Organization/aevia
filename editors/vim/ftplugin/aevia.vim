" Vim filetype plugin for Aevia.
if exists('b:did_ftplugin') | finish | endif
let b:did_ftplugin = 1

let s:save_cpo = &cpo
set cpo&vim

setlocal commentstring=#\ %s
setlocal comments=:##,:#
setlocal formatoptions-=t formatoptions+=croql
setlocal expandtab shiftwidth=4 softtabstop=4 tabstop=4
setlocal foldmethod=indent
setlocal iskeyword+='
setlocal suffixesadd=.aev
setlocal include=^\\s*\\(import\\\|open\\)\\s\\+
setlocal matchpairs=(:),[:],{:}

let b:undo_ftplugin = 'setlocal commentstring< comments< formatoptions< expandtab< shiftwidth<'
      \ . ' softtabstop< tabstop< foldmethod< iskeyword< suffixesadd< include< matchpairs<'

let &cpo = s:save_cpo
unlet s:save_cpo
