" Vim compiler file for Aevia.
if exists('current_compiler') | finish | endif
let current_compiler = 'aevia'

let s:cpo_save = &cpo
set cpo&vim

CompilerSet makeprg=aevia\ run\ %:S
CompilerSet errorformat=%E%f:%l:%c:\ error:\ %m,%W%f:%l:%c:\ warning:\ %m,%-G%.%#

let &cpo = s:cpo_save
unlet s:cpo_save
