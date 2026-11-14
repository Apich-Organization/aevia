# Appendix A — Complete grammar

This appendix is the normative EBNF. Notation:

- `{ X }` zero or more; `[ X ]` optional; `|` alternative.
- Terminals are written `"..."`; `NEWLINE`, `INDENT`, `DEDENT` and `EOF` are layout tokens
  （§1.4）。
- `INT`、`FLOAT`、`STRING`、`IDENT`、`UIDENT`、`UNIT_NAME`、`SUPERSCRIPT`、
  `LT_APP` and `ELLIPSIS` are lexical tokens (§1.2). A `UIDENT` starts with an ASCII capital; other names are
  an `IDENT`. `qname` is a qualified name.

## A.1 Layout

```
NEWLINE   ::= at the end of every logical line
INDENT    ::= when indentation increases
DEDENT    ::= when indentation decreases
```

The layout algorithm is in §1.4. Blocks and layout groups are both written `NEWLINE INDENT … DEDENT`:

```
block        ::= NEWLINE INDENT { block_stmt } expr NEWLINE? DEDENT
layout_group ::= NEWLINE INDENT expr NEWLINE? DEDENT
block_stmt   ::= let_decl | fn_decl
```

After `NEWLINE INDENT` the parser chooses `block` if it sees `let`/`fn` and `layout_group` otherwise;
`NEWLINE?` allows for an inner block that has already consumed the line end.

## A.2 Programs and declarations

```
program        ::= [ module_decl ] { import_decl } { top_decl } EOF

module_decl    ::= "module" module_path NEWLINE
module_path    ::= UIDENT { "." UIDENT }

import_decl    ::= "import" module_path [ "as" UIDENT ]
                   [ "{" name_list "}" ] NEWLINE
                 | "open" module_path [ "{" name_list "}" ] NEWLINE
name_list      ::= name { "," name }
name           ::= IDENT | UIDENT

top_decl       ::= { attribute } [ "private" ] decl
attr_arg       ::= [ IDENT "=" ] expr
attribute      ::= "@" IDENT [ "(" [ attr_arg { "," attr_arg } ] ")" ] NEWLINE?
decl           ::= dimension_decl
                 | unit_decl
                 | type_decl
                 | class_decl
                 | instance_decl
                 | derive_decl
                 | fn_decl
                 | let_decl
                 | test_decl

dimension_decl ::= "dimension" UIDENT [ "=" dim ] NEWLINE

unit_decl      ::= "unit" unit_name ":" dim "=" unit_rhs
                   [ "prefixable" ] NEWLINE
unit_rhs       ::= unit_scale [ unit_expr ] [ "offset" unit_scale unit_expr ]
unit_scale     ::= [ "-" ] number [ "/" number ]
                 | "pi" [ "/" number ]
unit_name      ::= UNIT_NAME | IDENT | qname

type_decl      ::= "type" UIDENT [ type_params ] [ "=" type_body ] NEWLINE?
type_body      ::= type
                 | "|" constructor { "|" constructor }
                 | NEWLINE INDENT { "|" constructor NEWLINE } DEDENT
constructor    ::= UIDENT [ "(" [ type { "," type } [ "," ] ] ")" ]

type_params    ::= "<" type_param { "," type_param } ">"
type_param     ::= IDENT [ ":" kind ]
kind           ::= "Type" | "Dim" | "Nat" | "Row" | "Rep" | "Format" | "Shape"

class_decl     ::= "class" UIDENT type_param { type_param } "where"
                   NEWLINE INDENT { method_sig NEWLINE } DEDENT
method_sig     ::= IDENT ":" type
                 | IDENT "(" [ params ] ")" [ ":" type ]

instance_decl  ::= "instance" UIDENT "for" type
                   [ "where" NEWLINE INDENT { method_def NEWLINE } DEDENT ]
method_def     ::= IDENT "(" [ params ] ")" [ ":" type ] "=" expr NEWLINE?

derive_decl    ::= "derive" name_list "for" type NEWLINE

fn_decl        ::= "fn" IDENT [ type_params ] "(" [ params ] ")" [ ":" type ]
                   "=" expr NEWLINE?
params         ::= param { "," param } [ "," ]
param          ::= pattern [ ":" type ]

let_decl       ::= "let" [ "rec" ] pattern [ ":" type ] "=" expr NEWLINE?

test_decl      ::= "test" STRING "=" expr NEWLINE?
```

- A one-line `type_body` variant list MUST start with `|` to distinguish it from an alias; the indented form may omit the first `|`.
- The trailing `NEWLINE?` of a declaration allows the expression to be a block.
- `import M` binds qualified names only; `import M { … }` and `open M { … }` bring in selected names; `open M` opens
  every export (§7.2).

## A.3 Expressions

```
expr           ::= let_expr
                 | if_expr
                 | match_expr
                 | fn_expr
                 | do_expr
                 | annot_expr
                 | block
                 | layout_group

let_expr       ::= "let" [ "rec" ] pattern [ ":" type ] "=" expr "in" expr
if_expr        ::= "if" expr "then" expr "else" expr

match_expr     ::= "match" expr "with" match_body
match_body     ::= NEWLINE INDENT { match_arm } DEDENT
                 | NEWLINE { match_arm }          # arms in the column of `match`
                 | inline_arm { inline_arm }      # arms on the line of `with` (§2.3.4)
match_arm      ::= "|" pattern [ "if" expr ] "->" expr NEWLINE?
inline_arm     ::= "|" pattern [ "if" expr ] "->" expr

fn_expr        ::= "fn" ( "(" [ params ] ")" | IDENT ) [ ":" type ] "=>" expr

do_expr        ::= "do" NEWLINE INDENT { do_stmt } DEDENT
do_stmt        ::= "let" pattern [ ":" type ] "=" expr NEWLINE
                 | pattern "<-" expr NEWLINE
                 | expr NEWLINE?

annot_expr     ::= pipe_expr [ ":" type ]

pipe_expr      ::= or_expr { ( "|>" | "<|" ) or_expr }
or_expr        ::= and_expr [ "||" or_expr ]
and_expr       ::= cmp_expr [ "&&" and_expr ]
cmp_expr       ::= cons_expr [ cmp_op cons_expr ]
cmp_op         ::= "==" | "!=" | "<" | "<=" | ">" | ">="
cons_expr      ::= add_expr [ "::" cons_expr ]
add_expr       ::= mul_expr { ( "+" | "-" ) mul_expr }
mul_expr       ::= unary_expr { ( "*" | "/" | "·" | "×" | "÷" | "@" ) unary_expr }
unary_expr     ::= ( "-" | "+" | "!" | "√" ) unary_expr
                 | power_expr
power_expr     ::= postfix_expr [ "^" unary_expr ]
postfix_expr   ::= primary { postfix }
postfix        ::= type_app | call | index | field | "?" | "!"
type_app       ::= LT_APP type_arg { "," type_arg } ">"
call           ::= "(" [ expr { "," expr } [ "," ] ] ")"
index          ::= "[" slice { "," slice } "]"
slice          ::= ".."
                 | expr ".." [ expr ]
                 | expr "..=" expr
                 | ":" [ expr ] [ ":" [ expr ] ]
                 | expr ":" [ expr [ ":" expr ] ]
                 | expr
field          ::= "." name

primary        ::= quantity_lit
                 | literal
                 | IDENT
                 | qname
                 | "(" expr ")"
                 | tuple
                 | record_expr
                 | tensor_lit
                 | "(" operator ")"
                 | "_"

qname          ::= UIDENT { "." UIDENT }
tuple          ::= "(" expr "," expr { "," expr } [ "," ] ")"

record_expr    ::= "{" [ record_body ] "}"
record_body    ::= expr "with" field_init { "," field_init } [ "," ]
                 | field_init { "," field_init } [ "," ]
field_init     ::= IDENT "=" expr
                 | IDENT

tensor_lit     ::= "[" [ expr { "," expr } [ "," ] ] "]"

operator       ::= "+" | "-" | "*" | "/" | "@" | "^" | "==" | "!="
                 | "<" | "<=" | ">" | ">=" | "&&" | "||" | "::"

literal        ::= INT | FLOAT | STRING | "true" | "false" | "(" ")"
quantity_lit   ::= ( INT | FLOAT ) unit_expr
```

**Notes**

- `LT_APP` is a `<` touching the preceding identifier (§2.7.2); `record_body` uses `with` to tell an update from a literal.
- `quantity_lit` arises only when a numeric literal is immediately followed by a unit expression (§1.8.1).
- `"_"` is valid only as a call argument (§2.3.6); `"(" operator ")"` parses, but the name is not yet bound
  (status note in §2.3.7).
- A `qname` starts with a `UIDENT`; `Physics.Ising.run` parses as a `qname` plus the field `.run`.
- Postfix `!` MUST touch the end of its operand (§2.7.5).

## A.4 Patterns

```
pattern        ::= "_"
                 | IDENT
                 | UIDENT
                 | UIDENT "(" [ pattern { "," pattern } ] ")"
                 | literal_pattern
                 | "{" [ field_pat { "," field_pat } ] "}"
                 | "(" pattern { "," pattern } ")"
                 | "[" [ pattern { "," pattern } [ "," ".." IDENT ] ] "]"
                 | pattern "as" IDENT
                 | "(" pattern ":" type ")"

field_pat      ::= IDENT "=" pattern
                 | IDENT

literal_pattern ::= INT | STRING | "true" | "false" | "(" ")"
```

- In a `match` arm a bare `UIDENT` is a nullary constructor pattern; in a binding position it is a variable (§2.4).
- Floating and negative literals are not patterns (`E0204`); use a guard.

## A.5 Types

```
type           ::= [ "forall" type_param { "," type_param } "." ] fn_type
fn_type        ::= type_postfix [ "->" type ]
type_postfix   ::= type_atom { "[" IDENT "]" }
type_atom      ::= "Int"
                 | "Real" [ "[" dim "]" ]
                 | "Float" [ "[" dim "]" ]
                 | "Complex" [ "[" dim "]" ]
                 | "Bool"
                 | "Prop"
                 | "String"
                 | "(" ")"
                 | "Tensor" "<" shape "," type ">"
                 | "Matrix" "<" nat "," nat "," type ">"
                 | "Sparse" "<" format_name "," shape "," type ">"
                 | "Table" "<" row_type ">"
                 | "Point" "[" dim "]"
                 | "Unit" [ "[" dim "]" ]
                 | "Set" "<" type ">"
                 | "Option" "<" type ">"
                 | "Result" "<" type "," type ">"
                 | "Solutions" "<" type "," dim ">"
                 | "Dyn" "<" rep ">"
                 | "RateOf" "<" type ">"
                 | "SymFun" "<" dim "," dim ">"
                 | "IO" type
                 | "{" [ field_type { "," field_type } [ "," ]
                         [ "..." IDENT ] ] "}"
                 | "(" type { "," type } ")"
                 | qname [ "<" type_arg { "," type_arg } ">" ]
                 | IDENT
                 | "(" type ")"

field_type     ::= IDENT ":" type
type_arg       ::= type | nat | dim | shape
format_name    ::= IDENT | UIDENT
shape          ::= nat | "[" [ shape_item { "," shape_item } ] "]"
shape_item     ::= nat | "?"
row_type       ::= type { "+" type }
nat            ::= nat_term { "+" nat_term }
nat_term       ::= nat_atom { "*" nat_atom }
nat_atom       ::= INT | IDENT | "(" nat ")"
rep            ::= "Int" | "Real" | "Float" | "Complex" | IDENT
```

- `Real`/`Float`/`Complex` without brackets mean `[1]`; bare `Unit` is `()` and `Unit[d]` is the type of unit
  values; `Tensor<0, τ>` equals `τ`; `RateOf<s>` is a built-in type function (§8.9).
- `(A, B)` is a tuple type and `(A, B) -> C` a multi-argument function type; `A * B` is not a type.
- The first argument of `Tensor` is parsed as a `shape` and the argument of `Dyn` as a `rep`; other `type_arg`s are parsed
  by kind.

## A.6 Dimensions and units

```
dim            ::= dim_factor { ( "*" | "/" | "·" ) dim_factor }
dim_factor     ::= dim_atom [ "^" exponent ]
dim_atom       ::= "1" | UIDENT | IDENT | "(" dim ")"

exponent       ::= [ "-" ] INT
                 | "(" [ "-" ] INT "/" INT ")"

unit_expr      ::= unit_factor { ( "*" | "·" | "/" ) unit_factor }
unit_factor    ::= unit_atom [ "^" exponent | SUPERSCRIPT+ ]
unit_atom      ::= unit_name | "(" unit_expr ")" | "%"
```

- In a `dim`, a `UIDENT` is a base dimension or alias and an `IDENT` a dimension variable; `/` is left-associative (`L/T^2` = `L/(T^2)`).
- `·` is normalised to `*` in the lexer (§1.9).
- Numbers are not allowed inside a unit expression (exponents excepted); a `unit_name` may carry a prefix or be `°C` (§1.8).
- An affine unit cannot appear in a product or power (`E0508`).

## A.7 Operator precedence

See §2.6. `..`/`..=` occur only in indices and take no part in expression precedence; how the slice stride `:` relates to
`::` is in §2.3.11.

## A.8 Lexical tokens (summary)

```
IDENT          ::= lower XID_Continue { XID_Continue | "'" | "₀".."₉" }
UIDENT         ::= upper XID_Continue { XID_Continue | "'" | "₀".."₉" }
INT            ::= dec_int | "0x" hex { hex } | "0o" oct { oct } | "0b" bin { bin }
FLOAT          ::= dec_int "." dec_int [ exponent ]
                 | dec_int exponent
STRING         ::= '"' { char | escape | "{" expr "}" } '"'
                 | 'r"' { char } '"'
                 | '"""' ... '"""'
UNIT_NAME      ::= "°" [ UIDENT ] | IDENT
SUPERSCRIPT    ::= "⁰".."⁹" | "⁻" | "⁺"
LT_APP         ::= "<" touching the preceding identifier (no space)
ELLIPSIS       ::= "..."
NEWLINE        ::= "\n"
INDENT         ::= produced by the layout algorithm
DEDENT         ::= produced by the layout algorithm
```

The full lexical rules are in chapter 01. `forall` is a contextual keyword (§1.5.2); `pure` is a Prelude function, not a
keyword.
