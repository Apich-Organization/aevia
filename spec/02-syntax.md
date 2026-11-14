# 02 — 语法

本章定义声明、表达式、模式和类型的语法，并说明容易歧义处如何解析。完整 EBNF 在
[附录 A](appendix-a-grammar.md)，本章不重复。

## 2.1 源文件结构

```aevia
module Kinematics            # 可选，必须是第一条声明

import Units.SI as SI        # 限定访问
import Math { sqrt, pi }     # 选择性打开
open Units.SI                # 打开单位与常量

dimension Currency           # 自定义基量纲
unit EUR : Currency = 1      # 单位声明

type Vec2 = { x: Real[L], y: Real[L] }

fn fall_time(h: Float[L]): Float[T] = sqrt(2 * h / g)
```

一个文件是一个模块。顶层声明的顺序不影响名字解析（单位名在全部声明解析后统一解析）。
顶层可以是 `module`（至多一次，且第一）、`import`/`open`、`dimension`/`unit`、
`type`/`class`/`instance`/`derive`、`fn`/`let`、`test`。`private` 可加在 `type`、
`dimension`、`unit`、`fn`、`let`、`class` 前表示不导出，默认全部导出。`open` 的操作数
是模块路径，不能是 `import … as` 引入的别名（§7.2）。

## 2.2 声明

### 2.2.1 函数

```aevia
fn kinetic_energy(m: Real[M], v: Real[L/T]): Real[M*L^2/T^2] =
    m * v^2 / 2

fn norm((x, y): (Real[L], Real[L])): Real[L] = sqrt(x^2 + y^2)
```

- 参数可带类型标注，省略时由函数体和调用点推断；参数可以是模式。
- 返回类型写在 `)` 之后、`=` 之前；函数体是一个表达式，多行时是块（§1.4.4）。
- 函数声明是递归的：`fn f(x) = e` 是 `let rec f = fn (x) => e` 的语法糖。

### 2.2.2 值

```aevia
let g = 9.80665 m/s^2
let (x, y) = (1.0 m, 2.0 m)
let rec fact = fn (n: Int) => if n == 0 then 1 else n * fact(n - 1)
```

- 普通 `let` 不是递归的：`let x = x + 1` 的右侧 `x` 指向外层绑定，没有则 `E0301`。
  `let rec` 让名字在自身定义中可见。
- 顶层与局部 `let` 语义相同。模式必须与右侧类型匹配（`E0308`）；构造器模式绑定
  （`let Some(x) = e`）按不可驳处理。

### 2.2.3 类型

```aevia
type Vec2 = { x: Real[L], y: Real[L] }

type Shape =
    | Circle(Real[L])
    | Rect(Real[L], Real[L])

type Result<a, e> = | Ok(a) | Err(e)
```

- `type Name = T` 是**透明别名**。
- `type Name`（无 `=`）是**抽象类型**，构造器与操作由标准库或插件提供。
- `type Name = | C1(…) | C2(…)` 是**名义变体类型**，构造器是值/函数。单行变体 MUST 以 `|`
  开头以区别于别名（`type T = A(Int) | B` 是 `E0202`）；缩进形式可省略首个 `|`。
- 递归类型的递归出现须在函数箭头右侧、记录字段、元组或构造器参数中（正性，§3.6.4）。
- 构造器名在模块内全局可见；跨模块同名用 `Module.Ctor` 限定。

### 2.2.4 类型类与实例

```aevia
class Show a where
    show(x: a): String

instance Show for Real[d] where
    show(x) = format_quantity(x)

derive Eq, Show for Vec2
```

- 类至少有一个类型参数；方法只有签名，默认实现由 `derive` 或插件提供。
- 实例头是 `instance Class for Type`，`Real[d]` 中的 `d` 隐式全称量化。一致性与孤儿规则
  见 §7.4。
- `derive` 为记录和变体生成常见实例。

### 2.2.5 单位与量纲

```aevia
dimension Currency
unit USD : Currency = 1
unit EUR : Currency = 1.08 USD
unit m : L = 1 prefixable
unit degC : Θ = 1 K offset 273.15 K
```

语法与语义见第 05 章。

### 2.2.6 测试

```aevia
test "fall time from 20 m" =
    approx_eq(fall_time(20.0 m), 2.019 s, 0.001 s)

test "energy is positive" =
    forall(fn (v: Real[L/T]) => kinetic_energy(1 kg, v) >= 0 J)
```

名称是字符串，体是 `Bool`（或 `Prop`，§8.13）表达式；`aevia test` 收集并运行。`forall`
是标准库函数，不是关键字，其谓词只有一个参数。

### 2.2.7 属性

```aevia
@stable
@deprecated("use new_api")
fn old_api(x: Real[1]): Real[1] = ...

@op("mydomain/softplus")
fn softplus(x: Real[1]): Real[1] = ln(1 + exp(x))
```

- 已知属性：`@stable`、`@experimental`、`@deprecated`（稳定性，§13.8）、`@jit`（请求 JIT；
  后端没有 `core.jit` 时 `E1001`）、`@op`、`@rule`（注册 RSSN 算子与重写规则，§13.5）、
  `@bench`（§9.12）。
- 未知属性是 `E0210`；属性参数必须是编译期常量。

## 2.3 表达式

### 2.3.1 块

块是 `let`/`fn` 声明序列加一个最终表达式（§1.4.4）；`let` 的作用域延伸到块尾。

```aevia
let y =
    let a = 1
    let b = 2
    a + b
```

### 2.3.2 `let … in`

```aevia
let x = let y = 2 in y^2 + 1
```

`let … in` 是非递归的单行形式；递归用块中的 `let rec` 或 `fn`。

### 2.3.3 `if`

```aevia
let s = if x > 0 then "positive" else "non-positive"
```

`if` 是表达式，必须有 `else`，`else if` 可连写；两个分支类型必须统一（量纲也要统一）。
多行写法的 `else` 与 `if` 同列（§1.4.4）。

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

- 分支按顺序尝试；守卫 `if` 在模式匹配成功后求值，必须是 `Bool`；分支体可以是块。
- 非穷尽是 `E0309`，冗余分支是警告 `W0201`；穷尽性分析见 §2.4。
- 三种布局：分支缩进在 `match` 之下；分支与 `match` 同列；或**行内**写在 `with` 之后
  （括号内没有 `NEWLINE`，所以 `println(match o with | Some(v) -> v | None -> 0)` 只能
  行内）。行内分支的体到下一个 `|` 或外层表达式结束处为止，嵌套的行内 `match` 会吞掉
  后面的分支，须加括号。格式化器在表达式内部输出行内形式，行尾输出块形式。

### 2.3.5 lambda

```aevia
let square = fn (x: Real[L]) => x^2
let add = fn (x, y) => x + y
let inc = fn x => x + 1
let f = fn (x: Real[L]) =>
    let y = x^2
    y + 1
```

参数与返回类型可省略；闭包按词法捕获；`fn (x, y) => …` 接受两个参数，没有自动柯里化
（`curry`/`uncurry`，§8.3）。

### 2.3.6 调用与占位符

```aevia
f(x, y)
g()(x)                 # 调用返回的函数
map(_, xs)             # fn f => map(f, xs)
map(f, _)              # fn ys => map(f, ys)
```

调用必须写括号，实参以逗号分隔，没有可变参数，调用左结合。`_` 作为实参表示缺一个参数，
整个调用变成 lambda，参数按 `_` 出现顺序排列；`_` 在其他位置是 `E0202`。

### 2.3.7 运算符与管道

```aevia
let y = x |> square |> sqrt          # sqrt(square(x))
let z = sqrt <| square <| x          # 同上
let w = xs |> map(f, _) |> sum
```

- `a |> f` 与 `f <| a` 都等价于 `f(a)`；右侧是函数值，不自动柯里化，多参函数用占位符
  （`xs |> map(f, _)`）。两者同级，混用时按出现顺序左结合，建议只用一种。
- 部分应用 section（`(== 1)`、`(* 2)`）是 `E0205`，写 `fn y => y == 1`。

> **状态：** 裸运算符 `(+)` 已能解析，但名字尚未绑定到函数（`E0301`）；现在请写 lambda。

### 2.3.8 记录

```aevia
let p = { x = 1.0 m, y = 2.0 m }
let q = { p with x = 3.0 m }
let r = { x, y }                      # 字段 punning
let a = p.x
```

- 记录是结构化的，字段顺序不影响类型相等；重复字段是 `E0208`；`{}` 是合法的空记录。
- `{ p with x = … }` 产生新记录，未列字段不变，可新增字段。
- 字段访问是优先级最高的后缀运算。

### 2.3.9 变体构造

```aevia
let s = Circle(1.0 m)
let n = None
```

构造器是普通函数/值：`Circle : Real[L] -> Shape`，`None` 是 `Option<a>` 的多态值。构造器名
必须以大写字母开头。

### 2.3.10 张量

```aevia
let v = [1.0, 2.0, 3.0]              # Tensor<[3], Float>
let m = [[1.0, 2.0], [3.0, 4.0]]     # Tensor<[2,2], Float>
let e: Tensor<1, Float> = []         # 空张量需要标注
let q = zeros<Int>(3, 4)             # 显式元素类型；默认 Float
```

- 嵌套字面量构造 rank-n 张量，必须是矩形，否则 `E0203`；元素类型统一推断
  （`[1, 2.0]` 为 `Float`）；无标注的 `[]` 是 `E0314`。
- 显式类型实参写 `f<T>(…)`，`<` 必须紧邻函数名；比较的 `<` 前要有空格（§2.7.2）。

### 2.3.11 索引与切片

```aevia
let x = v[0]                         # 元素
let row = m[1]                       # rank 2 → rank 1
let a = m[0, 1]                      # 元素
let col = m[.., 1]                   # 列
let block = m[0..2, 1..3]            # 子矩阵
let last = v[-1]                     # 负索引：末尾
let ev = v[::2]                      # 步长切片
```

- 索引从 0 起，负数从末尾计；`..` 半开、`..=` 闭，省略端点表示到边界；`a[i]` 在 rank 1
  取元素，在 rank > 1 取第 i 个切片（rank 减一）；`a[i, j]` 要求 rank ≥ 2。
- `a[i:j:s]` 是带步长的切片，`i`、`j`、`s` 都可省略（`s` 缺省为 1），`a[i:j]` 与 `a[i..j]`
  是同一节点。
- `::` 始终是 cons（§2.6），所以 `xs[1::2]` 是 `xs[1 :: 2]`；有起点又要步长时写出终点
  （`xs[1:5:2]`），省略终点时必须省略起点（`xs[::2]`）。`[...]` 内 `:` 是切片分隔符，
  带类型标注的下标要加括号：`xs[(i : Int)]`。
- 越界是运行时 `E0802`；索引是字面量时编译器可静态检查。切片语义上是复制，运行时可共享
  缓冲区，负步长切片总是视图（§6.6.4.1）。

### 2.3.12 `do` 与 IO

```aevia
fn main(): IO Unit = do
    let name = "data.csv"
    text <- read_file(name)
    let rows = parse_csv(text)
    println("rows = {length(rows)}")
    pure(())
```

- `x <- action` 绑定 `IO` 动作的结果；`let` 是纯绑定；裸 `IO` 表达式依次执行，结果丢弃
  （类型不是 `IO Unit` 时警告 `W0202`）；非 `IO` 的语句是 `E0701`。
- `do` 的值是最后一条 `expr` 语句；`do` 语句中不能声明 `fn` 或 `let rec`，需要时先写成块表达式。
- `pure : a -> IO a` 是 Prelude 函数，不是关键字（§9.7.3）。`do` 只接受 `IO`。

### 2.3.13 错误传播 `?`

`e?` 要求 `e : Result<T, E>` 且所在函数返回 `Result<U, E>`：成功得 `T`，失败提前返回
`Err(e)`。`?` 也可用于 `Option`，所在函数返回 `Option`。

### 2.3.14 其他表达式

```aevia
()                                   # unit
(1, 2)                               # 元组
(1)                                  # 括号，不是元组
x : Real[L]                          # 类型标注
x!                                   # 阶乘（Int）
√x                                   # sqrt(x)
```

- 不支持单元素元组（`(1,)` 是 `E0202`），用值本身或记录。
- 类型标注 `e : T` 优先级最低，作用于整个表达式。
- `x!` 的规则见 §2.7.5。`√x` 是 `sqrt(x)` 的前缀写法，操作数类型必须在该处已知
  （`√x` 对已标注的 `x` 有效，`√4.0` 尚不能推断，写 `sqrt(4.0)`）。

## 2.4 模式

```aevia
let (x, y) = p
let { x, y } = point
let [a, b, ..rest] = xs

match shape with
| Circle(r) -> …
| Rect(w, h) if w > h -> …
| _ -> …
```

文法见附录 A.4。

- 变量模式绑定名字；`_` 不绑定；`p as x` 在匹配 `p` 的同时绑定整个值；`(p : T)` 标注。
- **绑定位置中的大写名字是变量。** 在 `let`、`do` 中的 `let`、`<-`、函数与 lambda 参数中，
  不带括号的 `UIDENT`（`let A = …`、`fn step(M: Tensor<2, Float>)`）绑定变量，因为科学
  代码惯用大写命名矩阵和常量。绑定位置的构造器模式必须带参数表（`let Some(x) = …`）；
  无参构造器只在 `match` 分支中按构造器匹配。绑定的变量遮蔽同名的值（包括单位名），
  但不影响 `3 A` 这类单位字面量。
- 构造器模式使用检查器确定的名义类型与构造器序号，显示名相同不代表同一构造器。
- 记录模式只匹配列出的字段（开放匹配）；`{ x, y }` 绑定同名字段，`{ x = p }` 重命名；
  元组模式要求元数相同。
- 张量模式只支持 rank 1：`[a, b, ..rest]`。无 `..rest` 要求长度精确相等；有 `..rest` 要求
  长度不小于前缀，rest 保留元素表示、单位和名义 schema，并共享原存储。
- 同一模式内绑定名必须唯一（含简写、`as`、rest），否则 `E0308`。
- 浮点字面量不是合法模式（`E0204`），用守卫。
- 穷尽性：保留多字段相关性（`(true,true)` 与 `(false,false)` 不覆盖 `(true,false)`）；
  `[]` 与 `[head, ..tail]` 覆盖全部长度；守卫不参与覆盖；分析有固定的工作与深度预算，
  无法证明穷尽时要求补充 `_`，绝不假定已覆盖。

## 2.5 类型语法

```aevia
Real[L/T]
Float[1]
Tensor<2, Float[L]>
Tensor<[3, ?], Real[d]>
{ x: Real[L], ...r }
(a, b) -> c
forall d. Real[d] -> Real[d^2]
Point[Θ]
Result<T, E>
IO Unit
```

文法见附录 A.5。要点：

- `Real`、`Float`、`Complex` 不带方括号等价于 `[1]`。
- `Tensor<2, Float>` 的 `2` 是秩；`Tensor<[3,4], Float>` 是细化形状，`?` 表示该维未知。
- `Unit[d]` 是单位值的类型（§5.2）；`Unit` 不带方括号是 unit 类型 `()`。
- 元组类型写 `(A, B)`；`A * B` 不是类型，`*` 只出现在量纲中。
- `type_param` 的 kind（`Type`、`Dim`、`Nat`、`Row`、`Rep`、`Format`、`Shape`）可省略，
  由使用位置推断。声明上的 `<…>` 已隐含全称量化，`forall` 通常省略。
- `IO` 作用于一个原子类型：`IO Unit`、`IO (Result<T, E>)`。

### 2.5.1 量纲语法

- `dim` 是 `*`、`/` 连接的因子，因子可带指数 `^n` 或 `^(p/q)`，`1` 是无量纲；`/` 左结合，
  `L/T^2` = `L/(T^2)`。
- 首字母大写的名字是基量纲或别名，小写名是量纲变量，只能出现在 `[…]` 中；裸写 `d` 是
  类型变量。
- 量纲别名（`Velocity`、`Force`）在检查时展开，打印时优先显示别名。

## 2.6 运算符优先级与结合性

从低到高：

| 级别 | 运算符 | 结合性 |
|---|---|---|
| 1 | `\|>` `<\|` | 左（混用时按出现顺序） |
| 2 | `\|\|` | 右 |
| 3 | `&&` | 右 |
| 4 | `==` `!=` `<` `<=` `>` `>=` | 非结合（连写是 `E0202`） |
| 5 | `::` | 右 |
| 6 | `+` `-` | 左 |
| 7 | `*` `/` `@` `·` `×` `÷` | 左 |
| 8 | 前缀 `-` `+` `!` `√` | 前缀 |
| 9 | `^` | 右 |
| 10 | 调用、索引、`.field`、`?`、后缀 `!` | 后缀 |

`..` 与 `..=` 只出现在索引中。`-x^2` = `-(x^2)`；`2^3^2` = `2^(3^2)`；`a * b @ c` =
`(a * b) @ c`，矩阵乘建议加括号；`x == y == z` 写作 `x == y && y == z`。`if`/`match`/`fn`/`let`
的体尽可能向右延伸，用括号消除歧义。`|` 只用于 match 分支和变体声明。

## 2.7 歧义与解决

### 2.7.1 数值后的标识符

`3 m` 是单位字面量，`3 x` 也是（`x` 非单位则 `E0501`），见 §1.8.1。

### 2.7.2 类型实参的 `<`

`zeros<Float>(3, 4)` 中 `<` 紧邻 `zeros`，按类型实参解析；`a < b` 中 `<` 前有空格，按比较
解析。解析器在 `ident<` 后尝试类型实参，失败则回退为比较，因此格式化器 MUST 在比较的
`<` 前保留空格。相邻的 `<…>` 只在 callee 是函数名（或其限定名）时合法；`3<Int>(2)`、
`f()<Int>` 是 `E0207`，提示加空格或给 callee 加括号。

### 2.7.3 `{` 是记录，不是块

块只用缩进，`{…}` 永远是记录：`if x then { y = 1 } else { y = 2 }` 是记录表达式。

### 2.7.4 `|` 的角色

`|` 出现在 `match` 分支（`| pattern -> expr`）和变体声明（`| Ctor(T)`）；逻辑或是两个字符
`||`；没有匿名 union 类型。

### 2.7.5 `!` 的前缀与后缀

- 前缀 `!x` 是逻辑非；`x != y` 的 `!=` 是单个记号。
- 后缀 `x!` 是阶乘，`x!!` 是双阶乘，`!` MUST 紧贴前一个操作数的末尾（无空白），操作数
  类型为 `Int`；`n !` 是语法错误。后缀 `!` 比前缀 `-` 和 `^` 结合得更紧：`-3!` = `-6`，
  `2^3!` = `2^6`。需要对复合表达式求阶乘时写 `factorial(n + 1)`。

> **状态：** 紧跟右括号的 `(n + 1)!` 目前被解析器拒绝（`E0202`），应写 `factorial(n + 1)`。

### 2.7.6 一元 `-`

一元 `-` 优先级低于 `^`、高于 `*`：`-2^2` = `-4`，`(-2)^2` = `4`；`a - -b` 合法。

## 2.8 去糖

下列去糖形式只用于类型检查和核心 IR（§9.2.2），不是用户语法。

| 语法 | 去糖 |
|---|---|
| `fn f(x) = e` | `let rec f = fn (x) => e` |
| `a \|> f`、`f <\| a` | `f(a)` |
| `√x` | `sqrt(x)` |
| `x!`、`x!!` | `factorial(x)`、`double_factorial(x)` |
| `a @ b` | `matmul_rank(a, b)`（按秩分派到 matmul/matvec/vecmat/dot） |
| `a * b`（张量） | `mul_elementwise(a, b)` |
| `{ p with x = e }` | 记录更新原语 |
| `f(_, x)` | `fn a => f(a, x)` |
| `do …` | `bind`/`pure` 链 |
| `[a, b, c]` | `tensor([a, b, c])` |
| `a[i, j]`、`a[i..j]`、`a[i:j:s]` | `index(a, [i, j])`、`slice(a, i, j)`、`slice_step(a, i, j, s)` |

## 2.9 完整例子

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

这个例子用到记录、字段访问、单位、量纲推断、`Float` 与 `Real` 混用、张量、ODE 积分、
`do` 和字符串插值（第 11 章逐行解释）。第 03 章说明这些表达式的类型从何而来。
