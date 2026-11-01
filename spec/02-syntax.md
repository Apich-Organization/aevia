# 02 — 语法

本章定义 Aevia 的声明、表达式、模式和类型语法。完整的 EBNF 在
[附录 A](appendix-a-grammar.md)；本章解释**为什么**这样写，以及那些容易歧义
的地方如何解析。

## 2.1 一个源文件的结构

```aevia
module Kinematics            # 可选，必须与文件路径一致

import Units.SI as SI        # 限定访问；单位字面量仍可用
import Math { sqrt, pi }     # 选择性打开
open SI                      # 打开单位值，之后 to(m, x) 可直接用

dimension Currency           # 自定义基量纲
unit EUR : Currency = 1      # 单位声明

type Vec2 = { x: Real[L], y: Real[L] }

fn fall_time(h: Float[L]): Float[T] = sqrt(2 * h / g)

fn main(): IO Unit = do
    ...
```

一个文件是**一个模块**。顶层声明按顺序出现，顺序不影响名字解析（除了单位
声明必须在使用它的单位字面量之前被解析——实现可以在解析完全部声明后统一
解析单位名）。顶层可以是：

- `module`（最多一次，且必须是第一条非注释声明）；
- `import` / `open`；
- `dimension` / `unit`；
- `type` / `class` / `instance` / `derive`；
- `fn` / `let`；
- `test`。

`private` 可以加在 `type`、`unit`、`fn`、`let`、`class` 前，表示不导出。
默认全部导出。

## 2.2 声明

### 2.2.1 函数声明

```aevia
fn kinetic_energy(m: Real[M], v: Real[L/T]): Real[M*L^2/T^2] =
    m * v^2 / 2
```

```
fn_decl ::= "fn" IDENT [ type_params ] "(" [ params ] ")" [ ":" type ] "=" expr
params  ::= param { "," param }
param   ::= pattern [ ":" type ]
```

- 参数可以有类型标注，也可以省略；省略时由函数体和调用点推断。
- 参数可以是模式：`fn norm((x, y): (Real[L], Real[L])): Real[L] = sqrt(x^2 + y^2)`。
- 返回类型标注在 `)` 之后、`=` 之前。
- 函数体是一个表达式；多行时是一个缩进块：

```aevia
fn mean<d>(xs: Tensor<1, Real[d]>): Real[d] =
    let n = length(xs)
    sum(xs) / n
```

- 函数声明是**递归**的：函数名在函数体内可见。
- `fn f(x) = e` 是 `let rec f = fn (x) => e` 的语法糖。

### 2.2.2 值声明

```aevia
let g = 9.80665 m/s^2
let (x, y) = (1.0 m, 2.0 m)
let rec fact = fn (n: Int) => if n == 0 then 1 else n * fact(n - 1)
```

```
let_decl ::= "let" [ "rec" ] pattern [ ":" type ] "=" expr
```

- 普通 `let` **不是**递归的：`let x = x + 1` 中的 `x` 指向外层同名绑定
  （若没有则报未定义）。
- `let rec` 让绑定名在自身定义中可见，主要用于递归 lambda。
- 顶层 `let` 与局部 `let` 语义相同。
- 模式绑定要求右侧类型与模式匹配；不匹配是编译错误（除非模式是 irrefutable
  的，如变量、通配符、记录、元组）。

### 2.2.3 类型声明

```aevia
type Vec2 = { x: Real[L], y: Real[L] }

type Shape =
    | Circle(Real[L])
    | Rect(Real[L], Real[L])

type Result<a, e> = | Ok(a) | Err(e)
```

```
type_decl ::= "type" UIDENT [ type_params ] [ "=" type_body ]
type_body ::= type
            | "|" constructor { "|" constructor }
            | NEWLINE INDENT { "|" constructor NEWLINE } DEDENT
constructor ::= UIDENT [ "(" [ type { "," type } ] ")" ]
```

- `type Name = T` 是**透明别名**：`Vec2` 与 `{ x: Real[L], y: Real[L] }`
  完全相同。
- `type Name`（省略 `= type_body`）是**抽象类型**，只能由标准库/插件提供
  构造器与操作；用户模块不能凭空实现它。`Table`/`Series`/`PlotSpec` 等
  内建类型用这种形式声明。
- `type Name = | C1(...) | C2(...)` 是**名义变体类型**；构造器是值/函数。
  单行变体 MUST 以 `|` 开头，以便与类型别名区分；缩进形式可以省略首个
  `|`（但建议保留）。
- 递归类型允许，但递归出现必须位于函数箭头的右侧、记录字段、元组或构造器
  参数中（正性检查，§3.6.4）。
- 构造器名在模块内全局可见；不同模块的同名构造器用 `Module.Ctor` 限定。

### 2.2.4 类型类与实例

```aevia
class Show a where
    show(x: a): String

instance Show for Real[d] where
    show(x) = format_quantity(x)

instance Show for Vec2 where
    show(v) = "({v.x}, {v.y})"
```

```
class_decl    ::= "class" UIDENT type_param { type_param } "where"
                  NEWLINE INDENT { method_sig NEWLINE } DEDENT
method_sig    ::= IDENT ":" type
                 | IDENT "(" [ params ] ")" [ ":" type ]
instance_decl ::= "instance" UIDENT "for" type
                  [ "where" NEWLINE INDENT { method_def NEWLINE } DEDENT ]
method_def    ::= IDENT "(" [ params ] ")" [ ":" type ] "=" expr
```

- 类声明至少有一个类型参数；方法只有签名；默认实现由 `derive` 或插件提供，不在类声明里写。
- 实例头是 `Class Type`；`Real[d]` 中的 `d` 隐式全称量化。
- 实例必须满足一致性（coherence）：同一类、同一类型头只能有一个实例；
  实例必须定义在类型或类所属的模块中（orphan rule，§7.4）。
- `derive` 为记录/变体自动生成常见实例：

```aevia
derive Eq, Show for Vec2
```

### 2.2.5 单位与量纲声明

```aevia
dimension Currency
unit USD : Currency = 1
unit EUR : Currency = 1.08 USD

unit m : L = 1 prefixable
unit km : L = 1000 m
unit degC : Θ = 1 K offset 273.15 K
```

语法与语义见第 05 章。

### 2.2.6 测试声明

```aevia
test "fall time from 20 m" =
    approx_eq(fall_time(20.0 m), 2.019 s, 0.001 s)

test "energy is positive" =
    forall(fn (m: Real[M], v: Real[L/T]) =>
        kinetic_energy(m, v) >= 0 J)
```

`test` 的名称是字符串，体是 `Bool` 表达式。`aevia test` 收集并运行所有
`test`。`forall` 是标准库的属性测试辅助函数，不是语言关键字（§8.13）。

### 2.2.7 属性

声明和函数可以带属性：

```aevia
@stable
@deprecated("use new_api")
fn old_api(x: Real[1]): Real[1] = ...

@jit
fn hot(x: Float[1]): Float[1] = ...

@op("mydomain/softplus")
fn softplus(x: Real[1]): Real[1] = ln(1 + exp(x))
```

```
attribute ::= "@" IDENT [ "(" [ attr_arg { "," attr_arg } ] ")" ] NEWLINE?
top_decl  ::= { attribute } [ "private" ] decl
attr_arg  ::= [ IDENT "=" ] expr
```

- `@stable`/`@experimental`/`@deprecated` 是稳定性标记（§13.8）。
- `@jit` 请求 JIT；后端没有 `core.jit` 时报 `E1001`。
- `@op` 把函数注册为 RSSN 算子；`@rule` 注册重写规则（§13.5）。
- 未知属性报 `E0210`；属性参数必须是编译期常量。

## 2.3 表达式

### 2.3.1 块

块是一串 `let`/`fn` 声明后跟一个最终表达式：

```aevia
let y =
    let a = 1
    let b = 2
    a + b
```

```
block ::= INDENT { block_stmt } expr DEDENT
block_stmt ::= let_decl NEWLINE | fn_decl NEWLINE
```

块的值是最后一个表达式的值。块中 `let` 的作用域延伸到块尾。块必须至少有一个
最终表达式；只有 `let` 的块是语法错误（`E0201`）。

### 2.3.2 `let ... in`

单行表达式可以用 ML 风格的 `let ... in`：

```aevia
let x = let y = 2 in y^2 + 1
```

```
let_expr ::= "let" pattern [ ":" type ] "=" expr "in" expr
```

`let ... in` 是非递归的。递归用块中的 `let rec` 或 `fn`。

### 2.3.3 `if`

```aevia
let s = if x > 0 then "positive" else "non-positive"

let y =
    if x > 0 then
        sqrt(x)
    else
        -sqrt(-x)
```

```
if_expr ::= "if" expr "then" expr "else" expr
```

`if` 是表达式，必须有 `else`。两个分支的类型必须统一（量纲也要统一）。
`else if` 可以连写。

### 2.3.4 `match`

```aevia
fn area(s: Shape): Real[L^2] =
    match s with
    | Circle(r) -> pi * r^2
    | Rect(w, h) -> w * h

fn sign(x: Real[1]): Int =
    match x with
    | 0 -> 0
    | n if n > 0 -> 1
    | _ -> -1
```

```
match_expr ::= "match" expr "with" match_body
match_body ::= NEWLINE INDENT { match_arm } DEDENT
             | NEWLINE { match_arm }          # 分支与 match 同列
match_arm  ::= "|" pattern [ "if" expr ] "->" expr NEWLINE?
```

- 分支按顺序尝试；`_` 是通配符。
- 守卫 `if` 在模式匹配成功后求值，必须是 `Bool`。
- 穷尽性由编译器检查；非穷尽是错误 `E0309`，冗余分支是警告 `W0201`。
- 分支体可以是块。

### 2.3.5 `fn` 表达式（lambda）

```aevia
let square = fn (x: Real[L]) => x^2
let add = fn (x, y) => x + y
let compose = fn (f, g) => fn (x) => f(g(x))
```

```
fn_expr ::= "fn" ( "(" [ params ] ")" | IDENT ) [ ":" type ] "=>" expr
```

- 参数类型可省略；返回类型可省略。
- 多行 lambda：

```aevia
let f = fn (x: Real[L]) =>
    let y = x^2
    y + 1
```

- 函数是闭包；捕获是词法的。
- 没有自动柯里化：`fn (x, y) => ...` 接受两个参数，不是 `x -> y -> ...`。
  需要柯里化时用 `curry`/`uncurry`（§8.3）。

### 2.3.6 函数调用与占位符

```aevia
f(x)
f(x, y)
g()(x)                 # 调用返回的函数
map(_, xs)             # 等价于 fn f => map(f, xs)
map(f, _)              # 等价于 fn xs => map(f, xs)
```

- 调用必须写括号。`f x` 不是调用。
- 参数之间用逗号分隔；没有可变参数。
- `_` 作为实参表示“这里缺一个参数”，整个调用变成 lambda，参数按 `_` 出现
  的顺序排列。`_` 不能与普通表达式混用在同一参数位置。
- 调用是左结合的：`f(x)(y)` 先调用 `f(x)`，再调用结果。

### 2.3.7 运算符与管道

```aevia
let y = x |> square |> sqrt          # sqrt(square(x))
let z = sqrt <| square <| x          # sqrt(square(x))
let w = xs |> map(f) |> sum
```

- `a |> f` 等价于 `f(a)`；`f <| a` 等价于 `f(a)`。
- `|>` 与 `<|` 同级，混用时按出现顺序左结合；建议只用一种方向。
- 管道让“数据流”从左到右阅读，适合多步变换。

### 2.3.8 记录

```aevia
let p = { x = 1.0 m, y = 2.0 m }
let q = { p with x = 3.0 m }
let r = { x, y }                      # 字段 punning
let a = p.x
```

```
record_expr   ::= "{" [ record_body ] "}"
record_body   ::= expr "with" field_init { "," field_init }
                | field_init { "," field_init }
field_init    ::= IDENT "=" expr | IDENT
```

- 记录是结构化的；字段顺序不影响类型相等。
- `{ p with x = ... }` 产生一个新记录，未列出的字段保持不变；可以新增字段。
- 字段访问 `p.x` 是后缀运算符，优先级最高。
- 空记录 `{}` 是合法的，类型为 `{}`。

### 2.3.9 变体构造

```aevia
let s = Circle(1.0 m)
let o = Some(3)
let n = None
```

构造器是普通函数/值；`Circle` 的类型是 `Real[L] -> Shape`，`None` 是
`Option<a>` 的多态值。构造器名必须以大写 `UIDENT` 开头；这是语法要求。

### 2.3.10 张量

```aevia
let v = [1.0, 2.0, 3.0]              # Tensor<[3], Float>
let m = [[1.0, 2.0], [3.0, 4.0]]     # Tensor<[2,2], Float>
let e: Tensor<1, Float> = []         # 空张量需要标注
let z = zeros(3, 4)                  # 默认 Float；Tensor<2, Float>
let q = zeros<Int>(3, 4)             # 显式元素类型
```

```
tensor_lit ::= "[" [ expr { "," expr } ] "]"
```

- 嵌套的方括号字面量构造 rank-n 张量；必须是矩形，否则 `E0203`。
- 元素类型由所有元素统一推断；`[1, 2.0]` 统一为 `Float`。
- 空字面量 `[]` 的元素类型和 rank 无法推断，必须有类型标注。
- 显式类型实参写作 `f<T>(...)`，`<` 必须紧邻函数名，不能有空格；
  `a < b` 比较运算要求 `<` 前有空格或左侧不是可类型应用的名字。
- `zeros`、`ones`、`fill` 等构造器在没有约束时默认 `Float`。

### 2.3.11 索引与切片

```aevia
let x = v[0]                         # 元素
let row = m[1]                       # rank 2 -> rank 1
let a = m[0, 1]                      # 元素
let col = m[.., 1]                   # 列
let block = m[0..2, 1..3]            # 子矩阵
let last = v[-1]                     # 负索引：最后一个
```

```
index ::= "[" slice { "," slice } "]"
slice ::= [ expr ] [ ".." [ expr ] | "..=" expr ]
```

- 索引从 0 开始；负索引从末尾计。
- `..` 是半开区间，`..=` 是闭区间；省略端点表示到边界。
- `a[i]` 在 rank 1 上取元素，在 rank > 1 上取第 i 个切片（rank 减一）。
- `a[i, j]` 要求 rank 2；更多下标依此类推。
- 越界是运行时错误 `E0802`；编译器在索引为字面量时可以静态检查。
- 切片产生新张量（语义上是复制；运行时在可能时共享底层缓冲区）。

### 2.3.12 `do` 与 IO

```aevia
fn main(): IO Unit = do
    let name = "data.csv"
    text <- read_file(name)
    let rows = parse_csv(text)
    println("rows = {length(rows)}")
    return ()
```

```
do_expr ::= "do" NEWLINE INDENT { do_stmt } DEDENT
do_stmt ::= "let" pattern [ ":" type ] "=" expr NEWLINE
          | pattern "<-" expr NEWLINE
          | expr NEWLINE
```

- `x <- action` 绑定 `IO` 动作的结果。
- `let` 是纯绑定。
- 裸 `IO` 表达式按顺序执行，结果被丢弃（若类型不是 `IO Unit`，警告
  `W0202`）。
- `return(e)` 是 Prelude 函数，类型 `a -> IO a`，不是关键字。
- `do` 块的值是最后一个 `expr` 语句的值；最后一条是 `let` 时是 `()`。
- `do` 只接受 `IO`；类型检查器拒绝其他 monad。

### 2.3.13 错误传播 `?`

```aevia
fn load(path: String): IO (Result<Table<Dynamic>, DataError>) =
    read_csv_dyn(path)
```

`e?` 要求 `e : Result<T, E>`，所在函数返回 `Result<U, E>`；成功时得到 `T`，
失败时提前返回 `Err(e)`。`?` 也可以用于 `Option`，所在函数返回 `Option`。

### 2.3.14 其他表达式

```aevia
()                                   # unit
(1, 2)                               # tuple
(1)                                  # 括号，不是 tuple
x : Real[L]                          # 类型标注
x!                                   # 阶乘（Int）
√x                                   # sqrt(x)
```

- 元组用逗号；`(1,)` 是单元素元组（当前规范要求 `(1, ())` 或记录）。
  为避免歧义，当前规范 **不支持**单元素元组；用记录或直接返回值。
- 类型标注 `e : T` 优先级最低，作用于整个表达式。
- `x!` 仅当 `x : Int`；`x!!` 是双阶乘。
- `√x` 是 `sqrt(x)` 的前缀语法糖。

## 2.4 模式

```aevia
fn f(p: (Real[L], Real[L])): Real[L] =
    let (x, y) = p
    sqrt(x^2 + y^2)

match shape with
| Circle(r) -> ...
| Rect(w, h) if w > h -> ...
| _ -> ...

let { x, y } = point
let [a, b, ..rest] = xs
```

```
pattern ::= "_" | IDENT | UIDENT | literal
          | UIDENT [ "(" [ pattern { "," pattern } ] ")" ]
          | "{" [ field_pat { "," field_pat } ] "}"
          | "(" pattern { "," pattern } ")"
          | "[" [ pattern { "," pattern } [ "," ".." IDENT ] ] "]"
          | pattern "as" IDENT
          | "(" pattern ":" type ")"
field_pat ::= IDENT [ "=" pattern ]
```

- 变量模式绑定名字；`_` 不绑定。
- 构造器模式按名称匹配；参数模式递归匹配。
- 记录模式只匹配列出的字段（开放匹配）；`{ x, y }` 绑定同名字段。
- 张量模式只支持 rank 1 的字面量结构：`[a, b, ..rest]`。
- `p as x` 在匹配 `p` 的同时把整个值绑定到 `x`。
- 浮点字面量不能作为模式（NaN 与精确相等的问题）；用守卫。
- 模式匹配必须穷尽；记录模式和变量模式是 irrefutable 的。

## 2.5 类型语法

```aevia
Real[L]
Real[L/T]
Real[M*L^2/T^2]
Float[1]
Tensor<2, Float[L]>
Tensor<n, Real[d]>
{ x: Real[L], y: Real[L] }
{ x: Real[L], ...r }
(a, b) -> c
forall d. Real[d] -> Real[d^2]
Point[Θ]
Result<T, E>
IO Unit
```

```
type ::= [ "forall" type_param { "," type_param } "." ] fn_type
fn_type ::= type_postfix [ "->" type ]
type_postfix ::= type_atom { "[" IDENT "]" }
type_atom ::= "Int" | "Real" [ "[" dim "]" ] | "Float" [ "[" dim "]" ]
            | "Complex" [ "[" dim "]" ] | "Bool" | "Prop" | "String" | "()"
            | "Tensor" "<" shape "," type ">"
            | "Matrix" "<" nat "," nat "," type ">"
            | "Sparse" "<" format_name "," shape "," type ">"
            | "Table" "<" row_type ">"
            | "Point" "[" dim "]"
            | "Unit" [ "[" dim "]" ]
            | "Set" "<" type ">" | "Result" "<" type "," type ">"
            | "Option" "<" type ">" | "Solutions" "<" type "," dim ">"
            | "Dyn" "<" rep ">" | "RateOf" "<" type ">"
            | "SymFun" "<" dim "," dim ">" | "IO" type
            | "{" [ field_type { "," field_type } [ "," "..." IDENT ] ] "}"
            | "(" type { "," type } ")"
            | qname [ "<" type_arg { "," type_arg } ">" ]
            | IDENT
            | "(" type ")"
field_type ::= IDENT ":" type
qname ::= UIDENT { "." UIDENT }
format_name ::= IDENT | UIDENT
shape ::= nat | "[" [ shape_item { "," shape_item } ] "]"
shape_item ::= nat | "?"
row_type ::= type { "+" type }
```

注意：类型级乘积 `A * B` **不**表示元组；元组用 `(A, B)`。`*` 只出现在
量纲表达式里。

- `Real` 不带方括号等价于 `Real[1]`；`Float`、`Complex` 同理。
- `Tensor<2, Float>` 的 `2` 是秩（rank shape）；`Tensor<[3,4], Float>` 是细化形状。
- `Point[Θ]` 是仿射点（§5.6）。
- `Unit[d]` 是单位值的类型（§5.2）；`Unit` 不带方括号等价于 `()`。
- 类型参数 `<d: Dim>`、`<n: Nat>`、`<r: Row>` 的 kind 可省略，由使用位置
  推断。
- `forall` 通常省略；声明上的 `<...>` 已经隐含全称量化。

### 2.5.1 量纲语法

```
dim ::= dim_term { ("*" | "/" | "·") dim_term }
dim_term ::= dim_atom [ "^" exponent ]
dim_atom ::= "1" | UIDENT | IDENT | "(" dim ")"
exponent ::= ["-"] integer | "(" rational ")"
```

- `L/T` 左结合：`L/T^2` = `L/(T^2)`。
- `L^2/T` = `(L^2)/T`。
- `L^(1/2)` 是 `L` 的平方根量纲；`sqrt` 产生它。
- `1` 是无量纲。
- 量纲别名（`Velocity`、`Force`）在类型检查时展开；打印时优先显示别名。
- 量纲变量 `d` 只能出现在 `[...]` 中；裸写 `d` 是类型变量，不是量纲。

## 2.6 运算符优先级与结合性

从低到高：

| 级别 | 运算符 | 结合性 | 说明 |
|---|---|---|---|
| 1 | `\|>` `<\|` | 左（混用时按出现顺序） | 管道 |
| 2 | `\|\|` | 右 | 逻辑或 |
| 3 | `&&` | 右 | 逻辑与 |
| 4 | `==` `!=` `<` `<=` `>` `>=` | 非结合 | 比较，不能连写 |
| 5 | `::` | 右 | cons |
| 5 | `..` `..=` | 非结合 | 仅索引中使用 |
| 6 | `+` `-` | 左 | 加、减 |
| 7 | `*` `/` `@` `·` `×` `÷` | 左 | 乘、除、矩阵乘 |
| 8 | `-` `+` `!` `√`（前缀） | 前缀 | 一元 |
| 9 | `^` | 右 | 幂 |
| 10 | `f(...)` `a[i]` `p.x` `e?` `e!` | 后缀 | 调用/索引/字段/传播/阶乘 |

- `-x^2` = `-(x^2)`。
- `2^3^2` = `2^(3^2)`。
- `a * b @ c` = `(a * b) @ c`（同级别左结合）。矩阵乘法建议加括号。
- `x == y == z` 是语法错误；写 `x == y && y == z`。
- `if`/`match`/`fn`/`let` 的体尽可能向右延伸；用括号或块消除歧义。
- `|` 只用于 match 分支和变体声明，不是逻辑或。

## 2.7 语法歧义与解决

### 2.7.1 数值后的标识符

`3 m` 是单位字面量，不是 `3 * m`。`3 x` 在语法上也是单位字面量；若 `x`
不是单位，报 `E0501`。要乘以变量，写 `3 * x`。这条规则在词法章已经固定，
解析器不需要符号表。

### 2.7.2 类型实参的 `<`

`zeros<Float>(3, 4)` 中 `<` 紧邻 `zeros`，按类型实参解析。`a < b` 中 `<`
前有空格，按比较解析。解析器 SHOULD 在 `ident<` 后尝试类型实参；若失败，
回退为比较。这条规则要求格式化器在 `<` 比较前保留空格。

### 2.7.3 `{` 记录与块

块只用缩进，不用花括号。`{}` 永远是记录。因此 `if x then { y = 1 } else { y = 2 }`
是记录表达式，不是块。

### 2.7.4 `|` 的多重角色

- `match` 分支：`| pattern -> expr`。
- 变体声明：`| Ctor(T)`。
- 逻辑或：`||`（两个字符）。
- 类型 union？当前规范没有匿名 union；用变体类型。

解析器根据上下文区分；`|` 不会出现在表达式中作为二元运算符。

### 2.7.5 `!` 的前缀与后缀

- `!x` 是逻辑非。
- `x!` 是阶乘，仅当 `x` 是 `Int` 或括号表达式，且后面不跟另一个表达式。
- `x != y` 是不等号，词法阶段识别为单个 `!=` 记号。
- `x!!` 是双阶乘；`!!x` 是双重逻辑非（合法但无意义）。

### 2.7.6 `-` 的一元与二元

标准规则：一元 `-` 优先级低于 `^`，高于 `*`。`-2^2` = `-(4)`；
`(-2)^2` = `4`。`a - -b` 合法，等价于 `a - (-b)`。

## 2.8 语法糖与去糖

| 语法 | 去糖 |
|---|---|
| `fn f(x) = e` | `let rec f = fn (x) => e` |
| `a \|> f` | `f(a)` |
| `f <\| a` | `f(a)` |
| `√x` | `sqrt(x)` |
| `x!` | `factorial(x)` |
| `x!!` | `double_factorial(x)` |
| `a @ b` | `matmul_rank(a, b)`（按秩分派到 matmul/matvec/vecmat/dot） |
| `a * b`（张量） | `mul_elementwise(a, b)` |
| `{ p with x = e }` | `{ ...p, x = e }` 的记录更新原语 |
| `f(_, x)` | `fn a => f(a, x)` |
| `do ...` | `bind`/`return` 链 |
| `[a, b, c]` | `tensor([a, b, c])` |
| `a[i, j]` | `index(a, [i, j])` |
| `a[i..j]` | `slice(a, i, j)` |

去糖后的形式不是用户语法；它用于类型检查和解释器的核心 IR。

## 2.9 一个完整例子

```aevia
module Projectile

open Units.SI
import Math { sqrt, atan, sin, cos }

type State = {
    x:  Float[L],
    y:  Float[L],
    vx: Float[L/T],
    vy: Float[L/T],
}

type Rate = {
    x:  Float[L/T],
    y:  Float[L/T],
    vx: Float[L/T^2],
    vy: Float[L/T^2],
}

let g = 9.80665 m/s^2

fn rhs(_t: Float[T], s: State): Rate =
    let speed = sqrt(s.vx^2 + s.vy^2)
    let k = 0.02 / m
    {
        x  = s.vx,
        y  = s.vy,
        vx = -k * speed * s.vx,
        vy = -g - k * speed * s.vy,
    }

fn trajectory(v0: Float[L/T], angle: Float[1]): Tensor<1, State> =
    let s0 = { x = 0.0 m, y = 0.0 m,
               vx = v0 * cos(angle), vy = v0 * sin(angle) }
    let ts = linspace(0.0 s, 10.0 s, 1000)
    odeint(rhs, s0, ts)

fn main(): IO Unit = do
    let path = trajectory(50.0 m/s, 45 deg)
    println("range = {path[-1].x}")
```

这个例子用到了：记录、字段访问、单位、量纲推断、`Float` 与 `Real` 的混合、
张量、ODE 积分、`do` 和字符串插值。第 11 章会逐行解释。

下一章进入类型系统：这些表达式的类型从哪里来，量纲如何参与推断。
