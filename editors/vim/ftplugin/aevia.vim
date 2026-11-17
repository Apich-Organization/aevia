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

if !exists(':AeviaRun')
  function! s:AeviaExec(subcmd) abort
    if empty(expand('%'))
      echohl WarningMsg | echom 'Aevia: save the buffer before running' | echohl None
      return
    endif
    silent! update
    let l:cmd = 'aevia ' . a:subcmd . ' ' . shellescape(expand('%:p'))
    if exists(':terminal') == 2 && has('nvim')
      execute 'botright split | terminal ' . l:cmd
    else
      execute '!' . l:cmd
    endif
  endfunction

  command! -buffer AeviaRun call <SID>AeviaExec('run')
  command! -buffer AeviaTest call <SID>AeviaExec('test')
endif

if !exists('b:current_compiler')
  silent! compiler aevia
endif

let b:undo_ftplugin = 'setlocal commentstring< comments< formatoptions< expandtab< shiftwidth<'
      \ . ' softtabstop< tabstop< foldmethod< iskeyword< suffixesadd< include< matchpairs<'
      \ . ' | silent! delcommand AeviaRun | silent! delcommand AeviaTest'

let &cpo = s:save_cpo
unlet s:save_cpo
