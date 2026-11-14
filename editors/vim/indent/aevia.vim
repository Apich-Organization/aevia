" Vim indent script for Aevia: the offside rule (spec 1.4).
" Keep the previous line's indent, add one level after a block introducer.
if exists('b:did_indent') | finish | endif
let b:did_indent = 1

setlocal indentexpr=GetAeviaIndent()
setlocal indentkeys=o,O,0=else,0=\|
setlocal nolisp noautoindent nosmartindent

let b:undo_indent = 'setlocal indentexpr< indentkeys< lisp< autoindent< smartindent<'

if exists('*GetAeviaIndent') | finish | endif

function! GetAeviaIndent() abort
  let lnum = prevnonblank(v:lnum - 1)
  while lnum > 0 && getline(lnum) =~# '^\s*#'
    let lnum = prevnonblank(lnum - 1)
  endwhile
  if lnum == 0 | return 0 | endif
  let ind = indent(lnum)
  " strip a trailing line comment (not inside a string, good enough here)
  let line = substitute(getline(lnum), '\s\+#\%([^-].*\)\?$', '', '')
  if line =~# '\%(=\|->\|=>\|<-\|\<do\|\<then\|\<else\|\<with\|\<where\)\s*$'
    let ind += shiftwidth()
  endif
  " `else` lines up with their `if`; `|` branches with `match` or one deeper.
  if getline(v:lnum) =~# '^\s*else\>'
    let ind = max([0, ind - shiftwidth()])
  endif
  return ind
endfunction
