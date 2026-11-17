; Runnable `fn main` entry point
(
  (function_declaration
    name: (identifier) @run)
  (#eq? @run "main")
  (#set! tag aevia-run)
)

; Runnable `test` block
(
  (keyword) @run
  (#eq? @run "test")
  (#set! tag aevia-test)
)
