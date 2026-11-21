; Runnable `fn main` entry point
(
  (function_declaration
    name: (identifier) @run)
  (#eq? @run "main")
  (#set! tag aevia-run)
)

; The compiler's test header is `test` followed by a string name and `=`.
; The lexical grammar can attach `test` to a number on the preceding line.
(
  (source_file
    [(keyword) @run
     (quantity (unit_expression . (unit_name) @run .)) @_quantity]
    . (block_comment)*
    . [(string) (raw_string)] @_name
    . (block_comment)*
    . (operator) @_assignment)
  (#eq? @run "test")
  (#match? @_quantity "\\n[ \\t]*test\\z")
  (#match? @_name "\"\\z")
  (#eq? @_assignment "=")
  (#set! tag aevia-test)
)
