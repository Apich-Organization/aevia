# 附录 A — 完整文法

本附录是规范性 EBNF。记法：

- `{ X }` 零次或多次；`[ X ]` 可选；`|` 选择。
- 终结符用 `"..."`；`NEWLINE`、`INDENT`、`DEDENT`、`EOF` 是布局记号
  （§1.4）。
- `INT`、`FLOAT`、`STRING`、`IDENT`、`UIDENT`、`UNIT_NAME`、`SUPERSCRIPT`、
  `LT_APP`、`ELLIPSIS` 是词法记号。
- `IDENT` 首字母小写（或 `_`）；`UIDENT` 首字母大写；`qname` 是限定名。

## A.1 布局

```
NEWLINE   ::= 每个逻辑行末尾
INDENT    ::= 缩进增加时
DEDENT    ::= 缩进减少时
```

- 括号 `()`、`[]`、`{}` 内部不产生 `NEWLINE`/`INDENT`/`DEDENT`。
- 空行和纯注释行不产生记号。
- 缩进必须匹配栈中的某一层；否则 `E0102`。
- 块和续行都由 `NEWLINE INDENT ... DEDENT` 表示：

```
block        ::= NEWLINE INDENT { block_stmt } expr NEWLINE? DEDENT
layout_group ::= NEWLINE INDENT expr NEWLINE? DEDENT
block_stmt   ::= let_decl | fn_decl
```

`block` 用于 `let`/`fn` 序列；`layout_group` 用于跨行表达式。解析器在
`NEWLINE INDENT` 后看到 `let`/`fn` 时选 `block`，否则选 `layout_group`。
`NEWLINE?` 允许内层块已经消费了行尾。

## A.2 程序与声明

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

dimension_decl ::= [ "private" ] "dimension" UIDENT [ "=" dim ] NEWLINE

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

- `type_body` 的单行变体 MUST 以 `|` 开头，以便与类型别名区分；缩进形式
  可以省略首个 `|`（但建议保留）。
- 声明末尾的 `NEWLINE?` 允许表达式是块（块已消费行尾）。
- `import M { ... }` 是选择性打开；`import M` 只绑定限定名。
- `open M { ... }` 是选择性打开。

## A.3 表达式

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
                 | NEWLINE { match_arm }          # 分支与 match 同列
match_arm      ::= "|" pattern [ "if" expr ] "->" expr NEWLINE?

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

**歧义说明**

- `LT_APP` 是紧邻标识符的 `<`（无空格）；`a < b` 的 `<` 前有空格，按比较
  解析。格式化器 MUST 在比较的 `<` 前保留空格。
- `record_body` 用 `with` 区分更新与字面量。
- `quantity_lit` 只在数值字面量后紧跟单位表达式时产生；`3 + 4` 不是。
- `match_arm`/`do_stmt` 的 `NEWLINE?` 允许块体已消费行尾。
- `qname` 以 `UIDENT` 开头；`Physics.Ising.run` 解析为 `qname` +
  `.run` 字段。

## A.4 模式

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

- 裸 `UIDENT` 是零参构造器模式；裸 `IDENT` 是绑定。
- 浮点字面量不是合法模式。

## A.5 类型

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
nat            ::= INT | IDENT | nat "+" nat
rep            ::= "Int" | "Real" | "Float" | "Complex" | IDENT
```

- `Real` 不带方括号等价于 `Real[1]`；`Float`、`Complex` 同理。
- `Unit` 不带方括号等价于 `()`；`Unit[d]` 是单位值类型。
- `(A, B)` 是元组类型；`(A, B) -> C` 是多参函数类型。
- `Tensor` 的第一个参数是 `shape`（秩 `n` 或 `[n1,...,nk]`）；`Dyn` 的参数是
  `rep`；其他 `type_arg` 按 kind 解析。解析器在歧义时优先按 `shape`/`rep`
  解析 `Tensor`/`Dyn` 的第一个参数。
- `A * B` 不是合法类型；`*` 只出现在量纲中。
- `Tensor<0, τ>` 定义上等于 `τ`。
- `RateOf<s>` 是内建类型函数（§8.9）。

## A.6 量纲与单位

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

- `dim` 中 `UIDENT` 是基量纲/别名，`IDENT` 是量纲变量。
- `L/T^2` 左结合，等价于 `L/(T^2)`。
- `unit_name` 可以带前缀（`km`）或 `°C`。
- 单位表达式内不允许数字，指数除外。
- 仿射单位不能出现在乘积/幂中（语义检查 `E0508`）。

## A.7 运算符优先级

从低到高：

| 级别 | 运算符 | 结合性 |
|---|---|---|
| 1 | `\|>` `<\|` | 左（混用时按出现顺序） |
| 2 | `\|\|` | 右 |
| 3 | `&&` | 右 |
| 4 | `==` `!=` `<` `<=` `>` `>=` | 非结合 |
| 5 | `::` | 右 |
| 6 | `+` `-` | 左 |
| 7 | `*` `/` `·` `×` `÷` `@` | 左 |
| 8 | 前缀 `-` `+` `!` `√` | 前缀 |
| 9 | `^` | 右 |
| 10 | 类型应用、调用、索引、字段、`?`、后缀 `!` | 后缀 |

`..`/`..=` 只在索引中使用，不参与表达式优先级。

## A.8 词法记号（摘要）

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
LT_APP         ::= "<" 紧邻标识符（无空格）
ELLIPSIS       ::= "..."
NEWLINE        ::= "\n"
INDENT         ::= 由布局算法产生
DEDENT         ::= 由布局算法产生
```

完整词法规则见第 01 章。`for`、`rec`、`in` 是上下文关键字，但仍不能作为
普通标识符。`forall` 只在类型开头（后跟 type_param 和 `.`）时是关键字；
它同时是标准库函数名。`return` 是 Prelude 函数，不是关键字。
