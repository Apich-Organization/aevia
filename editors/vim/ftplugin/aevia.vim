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

function! s:AeviaExec(subcmd) abort
  let l:cli = get(g:, 'aevia_cli', 'aevia')
  if !executable(l:cli)
    echoerr 'Aevia CLI not found: ' . l:cli
    return
  endif
  let l:file = expand('%:p')
  let l:args = [l:cli, a:subcmd]
  if a:subcmd !=# 'repl'
    if empty(l:file) || &buftype !=# ''
      echoerr 'Aevia: save the buffer to an .aev file first'
      return
    endif
    update
    call add(l:args, l:file)
  endif
  let l:directory = empty(l:file) ? getcwd() : fnamemodify(l:file, ':h')
  let l:manifest = findfile('aevia.toml', escape(l:directory, ' ,;') . ';')
  let l:cwd = empty(l:manifest) ? l:directory : fnamemodify(l:manifest, ':p:h')
  if has('nvim-0.11')
    botright new
    call jobstart(l:args, {'term': v:true, 'cwd': l:cwd})
    startinsert
  elseif has('nvim')
    botright new
    call termopen(l:args, {'cwd': l:cwd})
    startinsert
  elseif has('terminal')
    call term_start(l:args, {'cwd': l:cwd})
  else
    execute '!' . join(map(l:args, 'shellescape(v:val, 1)'))
  endif
endfunction

command! -buffer AeviaRun call <SID>AeviaExec('run')
command! -buffer AeviaTest call <SID>AeviaExec('test')
command! -buffer AeviaRepl call <SID>AeviaExec('repl')

if !exists('b:current_compiler')
  silent! compiler aevia
endif

let b:undo_ftplugin = 'setlocal commentstring< comments< formatoptions< expandtab< shiftwidth<'
      \ . ' softtabstop< tabstop< foldmethod< iskeyword< suffixesadd< include< matchpairs<'
      \ . ' | silent! delcommand -buffer AeviaRun | silent! delcommand -buffer AeviaTest'
      \ . ' | silent! delcommand -buffer AeviaRepl'

let &cpo = s:save_cpo
unlet s:save_cpo
