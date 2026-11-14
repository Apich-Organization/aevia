" Vim syntax file for Aevia.
scriptencoding utf-8
if exists('b:current_syntax') | finish | endif

syn case match

syn keyword aeviaKeyword as class derive dimension do else fn for if import in instance
syn keyword aeviaKeyword let match module open private rec test then type unit where with
syn keyword aeviaBoolean true false oo nan

" Declarations: highlight the name that follows the introducer.
syn match aeviaFnDecl      "\<fn\>\s\+\%(rec\s\+\)\?\zs\K\k*"
syn match aeviaTypeDecl    "\<\%(type\|class\|instance\|dimension\)\>\s\+\zs\u\k*"
syn match aeviaUnitDecl    "\<unit\>\s\+\zs[[:alpha:]°µμΩÅ_][[:alnum:]_°µμΩÅ]*"
syn match aeviaModulePath  "\<\%(module\|import\|open\)\>\s\+\zs\u\k*\%(\.\u\k*\)*"
syn match aeviaAttribute   "@\K\k*"

syn match aeviaType        "\<\u\k*\>"
syn match aeviaCall        "\<\K\k*\ze("

" Numbers and quantities (spec 1.8.1: number + identifier text is a unit).
syn match aeviaNumber "\<0[xX]\x\+\%(_\x\+\)*\>"
syn match aeviaNumber "\<0[oO][0-7]\+\%(_[0-7]\+\)*\>"
syn match aeviaNumber "\<0[bB][01]\+\%(_[01]\+\)*\>"
syn match aeviaNumber "\<\d\+\%(_\d\+\)*\%(\.\d\+\%(_\d\+\)*\)\?\%([eE][-+]\?\d\+\)\?"
syn match aeviaUnit   "\%(\<\d[0-9_]*\%(\.\d[0-9_]*\)\?\%([eE][-+]\?\d\+\)\?\s*\)\@<=\%([[:alpha:]°µμΩÅ][[:alnum:]_°µμΩÅ.]*\|%\)\%(\^-\?\d\+\|\^([-0-9/]\+)\|[⁰¹²³⁴⁵⁶⁷⁸⁹⁻⁺]\+\)\?\%([*·/][[:alpha:]°µμΩÅ][[:alnum:]_°µμΩÅ.]*\%(\^-\?\d\+\|\^([-0-9/]\+)\|[⁰¹²³⁴⁵⁶⁷⁸⁹⁻⁺]\+\)\?\)*"

syn match aeviaOperator "|>\|<|\|->\|=>\|<-\|::\|\.\.\.\|\.\.=\|\.\.\|==\|!=\|<=\|>=\|&&\|||\|[≤≥≠√]\|[-+*/^@!?=<>|·×÷−&:]"

" Strings with interpolation and escapes.
syn region aeviaInterp contained matchgroup=aeviaInterpDelim start="{" end="}" contains=TOP
syn match  aeviaEscape contained "\\\%(u{\x\+}\|[ntr\\\"{}]\)"
syn region aeviaString start=+"+ skip=+\\\\\|\\"+ end=+"+ contains=aeviaEscape,aeviaInterp oneline
syn region aeviaString start=+"""+ end=+"""+ contains=aeviaEscape,aeviaInterp
syn match  aeviaRawString +\<r"[^"]*"+

" Comments: nestable block, doc, line.
syn region aeviaBlockComment start="#-" end="-#" contains=aeviaBlockComment,aeviaTodo
syn match  aeviaDocComment   "^\s*##.*$" contains=aeviaTodo
syn match  aeviaComment      "#\%(-\|#\)\@!.*$" contains=aeviaTodo
syn match  aeviaShebang      "\%^#!.*$"
syn keyword aeviaTodo contained TODO FIXME XXX NOTE

hi def link aeviaKeyword      Keyword
hi def link aeviaBoolean      Boolean
hi def link aeviaFnDecl       Function
hi def link aeviaCall         Function
hi def link aeviaTypeDecl     Type
hi def link aeviaType         Type
hi def link aeviaUnitDecl     Constant
hi def link aeviaUnit         Special
hi def link aeviaModulePath   Include
hi def link aeviaAttribute    PreProc
hi def link aeviaNumber       Number
hi def link aeviaOperator     Operator
hi def link aeviaString       String
hi def link aeviaRawString    String
hi def link aeviaEscape       SpecialChar
hi def link aeviaInterpDelim  Delimiter
hi def link aeviaComment      Comment
hi def link aeviaBlockComment Comment
hi def link aeviaDocComment   SpecialComment
hi def link aeviaShebang      PreProc
hi def link aeviaTodo         Todo

let b:current_syntax = 'aevia'
