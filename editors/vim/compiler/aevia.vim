" Vim compiler file for Aevia.
if exists('current_compiler') | finish | endif
let current_compiler = 'aevia'

let s:cpo_save = &cpo
set cpo&vim

CompilerSet makeprg=aevia\ --color\ never\ run\ %:S
CompilerSet errorformat=%Eerror%*[^:]:\ %m,%Wwarning%*[^:]:\ %m,%Z\ \ -->\ %f:%l:%c,%-G%.%#

let &cpo = s:cpo_save
unlet s:cpo_save
