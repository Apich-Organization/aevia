scriptencoding utf-8
set encoding=utf-8
set nomore

let s:runtime = expand('<sfile>:p:h:h')
execute 'set runtimepath^=' . fnameescape(s:runtime)
filetype plugin indent on
syntax enable

function! s:stack(row, fragment, groups) abort
  let col = stridx(getline(a:row), a:fragment) + 1
  if col == 0
    call assert_report('missing fixture text ' . string(a:fragment))
    return
  endif
  call assert_equal(a:groups, map(synstack(a:row, col), 'synIDattr(v:val, "name")'), a:fragment)
endfunction

function! s:load(source) abort
  let temp = tempname() . '.aev'
  try
    call writefile(a:source, temp)
    execute 'edit ' . fnameescape(temp)
    call assert_equal('aevia', &l:filetype)
    call assert_equal('aevia', &l:syntax)
  finally
    call delete(temp)
  endtry
endfunction

let s:corpus = join(readfile(s:runtime . '/../tree-sitter-aevia/test/corpus/strings.txt'), "\n")
let s:cases = split(s:corpus, '==================\n[^\n]\+\n==================\n\n')
let s:cases = map(s:cases, { _, body -> split(split(body, '\n\n------------------\n')[0], "\n") })
call assert_equal(5, len(s:cases), 'the shared string corpus')
let s:string = ['aeviaString']
let s:inside = s:string + ['aeviaInterp']
let s:keyword = ['aeviaKeyword']

call s:load(s:cases[0])
call s:stack(1, '## text', s:string)
call s:stack(1, '#- literal', s:string)
call s:stack(1, '\0', s:string + ['aeviaEscape'])
call s:stack(1, '\u{1F600}', s:string + ['aeviaEscape'])
call s:stack(1, '1 + 2', s:inside + ['aeviaNumber'])
call s:stack(1, 'tail', s:string)

call s:load(s:cases[1])
call s:stack(5, '#- text -#', s:string)
call s:stack(2, '# text', s:string)
call s:stack(3, '""doubled""', s:string)
call s:stack(4, '\0', s:string + ['aeviaEscape'])
call s:stack(7, 'let count', s:keyword)

call s:load(s:cases[2])
call s:stack(1, '# raw', s:inside + ['aeviaInterpGroup', 'aeviaRawString'])
call s:stack(1, '.first.inner', s:inside)
call s:stack(2, 'one', s:inside + ['aeviaProtectedString'])
call s:stack(3, 'let fields', s:keyword)
call s:stack(3, '\{ a', s:inside + ['aeviaInterpGroup', 'aeviaInterpDelim'])
call s:stack(4, '{}', s:inside + ['aeviaProtectedRawString'])
call s:stack(5, '2.75', s:inside + ['aeviaString', 'aeviaInterp', 'aeviaNumber'])

call s:load(s:cases[3] + ['let after = 5'])
call s:stack(1, '#- literal', s:string)
call s:stack(1, '#- nested', s:inside + ['aeviaBlockComment', 'aeviaBlockComment'])
call s:stack(1, '# line', s:inside + ['aeviaComment'])
call s:stack(2, 'suffix', s:string)
call s:stack(3, 'let after', s:keyword)

call s:load(s:cases[4])
call s:stack(1, 'never closed', s:string)
call s:stack(2, 'never closed', ['aeviaRawString'])
call s:stack(3, 'let after', s:keyword)

call s:load(['let text = "\q \u{Q} tail"'])
call s:stack(1, '\q', s:string + ['aeviaInvalidEscape'])
call s:stack(1, '{Q}', s:string + ['aeviaInvalidEscape'])
call s:stack(1, 'tail', s:string)

" Jump past a long literal before requesting its earlier lines.
call s:load(['let long = """'] + repeat(['  #- literal -#'], 120) + ['"""', 'let after = 5'])
call s:stack(123, 'let after', s:keyword)
call s:stack(121, '#- literal', s:string)

if !empty(v:errors)
  for error in v:errors
    echomsg error
  endfor
  cquit
endif
qa!
