# 05 — 单位

第 04 章把量纲变成了类型；本章把单位变成**编译期已知的尺度**。一个单位不
进入类型，它只提供三样东西：

1. 一个量纲 `d`；
2. 一个尺度 `s : Real[1]`（相对相干 SI 单位的倍数）；
3. 一个仿射偏移 `o : Real[d]`（仅仿射单位）。

单位字面量 `3.2 km` 在编译期折叠为 `3200.0`（相干 SI 值），类型是 `Float[L]`。
`km` 本身不留在值里；需要显示时，用 `to(km, x)` 把显示单位附加到值上。

## 5.1 单位声明

```aevia
dimension Currency                  # 自定义基量纲
unit USD : Currency = 1             # 基单位
unit EUR : Currency = 1.08 USD      # 派生单位

unit m : L = 1 prefixable           # 基单位，允许 SI 前缀
unit km : L = 1000 m                # 显式派生
unit h : T = 3600 s
unit N : M*L/T^2 = 1 kg*m/s^2
unit degC : Θ = 1 K offset 273.15 K # 仿射单位
```

### 5.1.1 语法

```
dimension_decl ::= [ "private" ] "dimension" UIDENT [ "=" dim ]
unit_decl ::= [ "private" ] "unit" unit_name ":" dim "=" unit_rhs [ "prefixable" ]
unit_rhs  ::= unit_scale [ unit_expr ] [ "offset" unit_scale unit_expr ]
unit_scale ::= number [ "/" number ] | "pi" [ "/" number ]
```

- `dimension`/`unit` 默认**导出**；`private` 只在本模块可见。导出的
  量纲/单位进入 Kernel Manifest 和 registry，下游包可以直接 `import`。
- 用户自定义量纲别名可以导出（例如 `dimension Currency`），不需要改
  标准库；`Units.SI` 只是 registry 的一个视图。

- `unit_name` 是标识符或带 `°` 的特殊名（`°C`、`°F`）。
- `unit_scale` 是正有理数，或 `pi` 的有理倍数。`1/1000`、`pi/180` 合法；
  `1.5` 合法（按精确十进制转有理数？见下）。
- `unit_expr` 是单位表达式（§5.2）。省略时表示基单位，尺度必须是 1，
  量纲必须是基量纲。
- `offset` 只用于仿射单位；偏移量的量纲必须等于单位量纲。
- `prefixable` 表示自动生成 SI 前缀版本。

**尺度精度**：`unit_scale` 中的十进制字面量按**精确十进制**解释（`1.5` 是
`3/2`，`0.3048` 是 `381/1250`），不是 `Float`。这让 `ft → m` 这类换算精确。
单位声明是编译期求值，不允许运行时表达式或 `approx`；需要近似尺度时，
写足够精度的十进制字面量（它仍按精确有理数处理）。

### 5.1.2 基单位与派生单位

- `unit m : L = 1` 声明 `m` 是 `L` 的相干基单位，尺度 1。
- `unit km : L = 1000 m` 声明派生单位，尺度 1000。
- 一个量纲可以有多个基单位吗？MUST NOT。每个基量纲有且只有一个尺度为 1 的
  基单位；其他都是派生。重复声明报 `E0505`。
- 派生单位的尺度在编译期计算；循环定义报 `E0506`。
- 单位声明可以跨模块导入；`private unit` 不导出。

### 5.1.3 单位命名空间

单位名住在**独立的命名空间**中：

- 单位字面量 `3 m` 中的 `m` 在单位命名空间解析。
- 普通表达式中的 `m` 是值命名空间中的单位值 `Unit[L]`（如果 `open` 了）。
- `let m = 5` 不会改变 `3 m` 的含义（仍是 3 米）；要乘以变量，写 `3 * m`。
- 单位名冲突时，最长匹配优先；`ms` 如果是毫秒，就解析为毫秒，而不是
  `m*s`。要写米乘秒，写 `m*s`。
- 单位名可以限定：`Units.SI.km`。在字面量中限定名只允许出现在单位位置；
  推荐 `open Units.SI` 后使用短名。

### 5.1.4 前缀

`prefixable` 的单位自动生成 SI 前缀版本：

| 前缀 | 符号 | 因子 |
|---|---|---|
| quetta | `Q` | 10^30 |
| ronna | `R` | 10^27 |
| yotta | `Y` | 10^24 |
| zetta | `Z` | 10^21 |
| exa | `E` | 10^18 |
| peta | `P` | 10^15 |
| tera | `T` | 10^12 |
| giga | `G` | 10^9 |
| mega | `M` | 10^6 |
| kilo | `k` | 10^3 |
| hecto | `h` | 10^2 |
| deca | `da` | 10^1 |
| deci | `d` | 10^-1 |
| centi | `c` | 10^-2 |
| milli | `m` | 10^-3 |
| micro | `µ` | 10^-6 |
| nano | `n` | 10^-9 |
| pico | `p` | 10^-12 |
| femto | `f` | 10^-15 |
| atto | `a` | 10^-18 |
| zepto | `z` | 10^-21 |
| yocto | `y` | 10^-24 |
| ronto | `r` | 10^-27 |
| quecto | `q` | 10^-30 |

规则：

- 前缀只加在 `prefixable` 单位上；`kg` 是特例：SI 基单位是千克，但前缀
  加在 `g` 上（`mg`、`µg`、`kg`）。`g` 声明为 `1/1000 kg prefixable`，
  `kg` 本身是基单位；解析 `kg` 时优先精确匹配基单位，而不是 `k`+`g`。
- 前缀解析最长匹配：`dam` 是 `da`+`m`（十米），不是 `d`+`am`。
- 前缀不能叠加：`kkm` 非法 `E0503`。
- 二进制前缀 `Ki`、`Mi`、`Gi`、`Ti` 只用于信息单位（`bit`、`B`），不用于
  物理单位。
- 前缀单位与显式单位冲突时（例如同时声明 `unit ms : T = 1/1000 s` 和
  `prefixable s`），显式声明优先；若两者尺度不同，报 `E0507`。

## 5.2 单位表达式

```
unit_expr ::= unit_factor { ("*" | "·" | "/") unit_factor }
unit_factor ::= unit_atom [ "^" exponent ]
unit_atom ::= unit_name | "(" unit_expr ")" | "%"
```

- `*` 与 `·` 同义；`/` 左结合。
- 指数是整数字面量或括号中的有理数：`m^2`、`s^-1`、`m^(1/2)`。
- 单位表达式内不允许数字（除了指数），不允许空白并置。
- 仿射单位（`degC`、`degF`）只能单独出现；出现在乘积/幂中报 `E0508`。
- 单位表达式可以包含单位变量：
  ```
  let u = km/h
  let v = 100 * u
  ```
  但 `100 u` 不是单位字面量（`u` 不是单位名），会报 `E0501`；写
  `100 * u`。

### 5.2.1 单位值

单位表达式在值层是一个 `Unit[d]` 值：

```aevia
let u = km / h          # Unit[L/T]，尺度 1000/3600
let v = 100 * u         # Real[L/T]，值 27.777...
```

`Unit[d]` 支持：

```
u * v : Unit[d*e]
u / v : Unit[d/e]
u ^ n : Unit[d^n]       # n 字面量
```

单位值可以比较（同量纲）、打印、作为参数传递。单位不是数值，不能直接与
`Real` 相加。

### 5.2.2 单位字面量的求值

`number unit_expr` 的求值：

```
value = number * scale(unit_expr) + offset(unit_expr)
type  = Scalar<rep(number), dim(unit_expr)>
```

- 线性单位：`offset = 0`。`3 km` → `3000`，`Real[L]`。
- 仿射单位：只有单独单位时合法。`25 °C` → `25 * 1 + 273.15 = 298.15`，
  `Point[Θ]`。
- `number` 是 `Int` 时，结果默认 `Real`（精确）；`number` 是 `Float` 时，
  结果默认 `Float`。`3 km` 默认 `Real[L]`，`3.0 km` 默认 `Float[L]`。
- 期望类型可以覆盖表示：`fall_time(20 m)` 中 `h : Float[L]`，`20 m` 实例化
  为 `Float[L]`；`let x: Real[L] = 3.0 m` 把 `3.0 m` 实例化为精确的
  `Real[L]`（`3`）。
- 尺度是 `pi` 的倍数时，结果可能是符号表达式：`30 deg` → `pi/6`，
  `Real[1]`；期望 `Float` 时求值为 `0.5235987755982988`。

## 5.3 单位表

标准库 `Units.SI` 提供以下单位（节选）。**单位名与量纲别名不共享拼写**：
`l`/`liter` 是升；不要写 `L` 作为单位（`L` 是 `Length` 的显示别名）；
`T` 在单位列是特斯拉，在量纲列是 `Time` 的显示别名。规范名一律用全名
（`Length`/`Mass`/`Time`/...），单字母只用于源码/显示。

### 5.3.1 SI 基单位

| 单位 | 量纲 | 说明 |
|---|---|---|
| `m` | `L` | 米，prefixable |
| `kg` | `M` | 千克（基单位；`g` prefixable） |
| `s` | `T` | 秒，prefixable |
| `A` | `I` | 安培，prefixable |
| `K` | `Θ` | 开尔文，prefixable |
| `mol` | `Amt` | 摩尔，prefixable |
| `cd` | `Lum` | 坎德拉，prefixable |

### 5.3.2 SI 派生单位

| 单位 | 量纲 | 定义 |
|---|---|---|
| `Hz` | `1/T` | `1/s` |
| `N` | `M*L/T^2` | `kg*m/s^2` |
| `Pa` | `M/(L*T^2)` | `N/m^2` |
| `J` | `M*L^2/T^2` | `N*m` |
| `W` | `M*L^2/T^3` | `J/s` |
| `C` | `I*T` | `A*s` |
| `V` | `M*L^2/(I*T^3)` | `W/A` |
| `F` | `I^2*T^4/(M*L^2)` | `C/V` |
| `Ω` / `ohm` | `M*L^2/(I^2*T^3)` | `V/A` |
| `S` | `I^2*T^3/(M*L^2)` | `1/Ω` |
| `Wb` | `M*L^2/(I*T^2)` | `V*s` |
| `T` | `M/(I*T^2)` | `Wb/m^2` |
| `H` | `M*L^2/(I^2*T^2)` | `Wb/A` |
| `lm` | `J` | `cd*sr` |
| `lx` | `J/L^2` | `lm/m^2` |
| `Bq` | `1/T` | `1/s` |
| `Gy` | `L^2/T^2` | `J/kg` |
| `Sv` | `L^2/T^2` | `J/kg` |
| `kat` | `N/T` | `mol/s` |
| `rad` | `1` | 无量纲，尺度 1 |
| `sr` | `1` | 无量纲，尺度 1 |

### 5.3.3 常用非 SI 单位

| 单位 | 量纲 | 定义 |
|---|---|---|
| `min` | `T` | `60 s` |
| `h` | `T` | `3600 s` |
| `d` | `T` | `86400 s` |
| `°` / `deg` | `1` | `pi/180` |
| `′` / `arcmin` | `1` | `pi/10800` |
| `″` / `arcsec` | `1` | `pi/648000` |
| `l` / `liter` | `L^3` | `1/1000 m^3` |
| `t` | `M` | `1000 kg` |
| `eV` | `Energy` | `1.602176634e-19 J` |
| `Da` / `u` | `M` | `1.66053906660e-27 kg` |
| `au` | `L` | `149597870700 m` |
| `pc` | `L` | `3.0856775814913673e16 m` |
| `ly` | `L` | `9460730472580800 m` |
| `Å` / `angstrom` | `L` | `1e-10 m` |
| `bar` | `Pressure` | `100000 Pa` |
| `atm` | `Pressure` | `101325 Pa` |
| `mmHg` | `Pressure` | `133.322387415 Pa` |
| `cal` | `Energy` | `4.184 J` |
| `kWh` | `Energy` | `3.6e6 J` |
| `hp` | `Power` | `745.6998715822702 W` |
| `kn` | `L/T` | `1852/3600 m/s` |
| `mph` | `L/T` | `1609.344/3600 m/s` |
| `ft` | `L` | `0.3048 m` |
| `in` | `L` | `0.0254 m` |
| `yd` | `L` | `0.9144 m` |
| `mi` | `L` | `1609.344 m` |
| `nmi` | `L` | `1852 m` |
| `acre` | `L^2` | `4046.8564224 m^2` |
| `gal` | `L^3` | `0.003785411784 m^3` |
| `oz` | `M` | `0.028349523125 kg` |
| `lb` | `M` | `0.45359237 kg` |
| `psi` | `Pressure` | `6894.757293168 Pa` |
| `°C` | `Θ` | 仿射：`1 K offset 273.15 K` |
| `°F` | `Θ` | 仿射：`5/9 K offset 255.3722222222222 K` |
| `bit` | `1` | 信息，尺度 1 |
| `B` | `1` | `8 bit` |
| `KiB` | `1` | `1024 B` |

信息单位是否应该是无量纲？当前规范是；如果需要独立的 `Information` 基量纲，用户
可以 `dimension Information` 并声明自己的单位。标准库不强制，因为大量
公式把 bit 当无量纲概率/熵使用。

### 5.3.4 单位模块

| 模块 | 内容 |
|---|---|
| `Units.SI` | 基单位、派生单位、前缀、常用非 SI |
| `Units.Imperial` | 英制长度/质量/体积/压强/温度 |
| `Units.Astronomy` | au、pc、ly、Mpc、太阳质量、木星质量 |
| `Units.CGS` | cm、g、dyn、erg、Gal、P |
| `Units.Natural` | `c = 1`、`hbar = 1`、`eV` 系 |
| `Units.Constants` | 物理常数（§5.8） |

> **0.2.13 实现状态**：RSSN 的 `UnitRegistry` 当前有 59 个单位，覆盖 SI
> 基/派生、常见英制、`degC`/`degF`；**没有** `au/pc/ly/Mpc`、`dyn/erg/Gal/P`
> 和 CODATA 常数。`Units.Astronomy`/`CGS`/`Constants` 在 RSSN 注册这些
> 条目之前是 Aevia 的本地 overlay，属于“单一事实来源”的显式例外；不能
> 从 manifest 生成。

`Units.Natural` 通过“单位制”而不是“改常数”实现：它导出一组新的单位，
其中 `c`、`hbar` 的数值是 1。用户 `open Units.Natural` 后，`1 GeV` 等写法
成立；切回 SI 用 `open Units.SI`（名字冲突时限定）。

## 5.4 换算与显示

### 5.4.1 三个操作

```aevia
let d = 3.2 km              # Float[L]，值 3200.0
let e = to(m, d)            # Float[L]，值 3200.0，显示单位 m
let f = value_in(km, d)     # Float[1]，值 3.2
let g = base(d)             # Float[L]，值 3200.0，去掉显示单位
```

- `to(u, x)`：要求 `x : Scalar<ρ,d>` 且 `u : Unit[d]`；返回值不变，只附加
  显示单位。对 `Float` 和 `Real` 都适用。
- `value_in(u, x)`：等价于 `x / u`，返回无量纲数值。
- `base(x)`：去掉显示单位，返回相干 SI 值。
- `to` 不改变类型，也不改变值；它只影响 `show`、`print`、绘图标签。
- **仿射单位只接受 `Point[d]`**：`to(°C, p)`、`value_in(°C, p)` 合法；
  对 `Scalar<ρ,Θ>`（温差）使用仿射单位报 `E0510`。温差显示用
  `delta_to(°C, dT)`，它只用尺度、不用偏移。
- 例子：`value_in(°C, point(298.15, K)) = 25`；`value_in(°C, 298.15 K)`
  是错误（`298.15 K` 是温差）。

### 5.4.2 显示单位的传播

运行时每个标量可以带一个可选的显示单位。规则：

- `x + y` / `x - y`：如果两边显示单位相同，保留；否则丢弃，回到 SI。
- `x * y` / `x / y`：如果一方无量纲且带显示单位，保留另一方的显示；
  否则丢弃。
- `to(u, x)` 显式设置。
- `base(x)` 显式清除。
- 函数调用默认丢弃显示单位（除了 `to`、`value_in`、`abs` 等保持形状的）。

显示单位是**元数据**，不参与相等、类型或序列化。`3.2 km == 3200 m` 为
`true`；打印时按各自的显示单位。

### 5.4.3 打印

```
3.2 km
3200 m
25 °C
1.5e-3 m
2.99792458e8 m/s
```

- 有显示单位时，数值按该单位换算后打印。
- 没有显示单位时，选一个“最佳单位”：优先 SI 派生单位（`N`、`J`、`Pa`），
  否则基单位乘积；指数为 1 的用 `m`、`s`，复杂量纲用 `kg*m/s^2`。
- `show` 保证可读；`repr` 保证可解析回同一个值（带单位文本）。
- 打印精度由 `Float` 的最短往返表示决定；`Real` 精确值打印为分数或根式。

## 5.5 角度与无量纲单位

- `rad` 是无量纲单位，尺度 1。
- `deg` / `°` 是无量纲单位，尺度 `pi/180`。
- `30 deg` 的值是 `pi/6`（符号）或 `0.5235987755982988`（Float）。
- `sin(30 deg)` 合法；`sin(30)` 也合法（30 弧度），因为 `sin` 只要求
  无量纲。这是 SI 的语义，不是漏洞。
- 如果用户想区分“弧度”和“纯数”，用 `rad` 单位标注；类型系统不区分。
- `atan2(y, x)` 返回无量纲弧度；用 `to(deg, ...)` 显示为角度。

## 5.6 仿射单位与 `Point`

### 5.6.1 问题

温度有两种用法：

- **绝对温度**：`T = 298.15 K`，是仿射空间中的一个点；
- **温差**：`ΔT = 25 K`，是向量，可以相加、缩放。

`25 °C` 是绝对温度（273.15 + 25 K），不是 25 K 的温差。如果把它们混为一谈，
`20 °C + 30 °C` 会得到 50 °C（错误）或 596.3 K（也错误）。

### 5.6.2 类型

```
Point[Θ]        绝对温度（仿射点）
Float[Θ]        温差（向量）
```

- 只有仿射单位字面量产生 `Point[d]`：`25 °C`、`77 °F`。
- 线性单位字面量产生 `Float[d]`/`Real[d]`：`25 K` 是温差，不是绝对温度。
- 绝对温度用开尔文写时，用 `point(298.15, K)`。
- `Point` 只支持 `Float` 表示；符号温度用 `Real[Θ]` 的温差。
- `Point` 可以是张量元素：`Tensor<[nx,ny], Point[Θ]>` 是温度场。元素级
  `+`/`-` 按点规则（`Point - Point -> Float[Θ]`，`Point + Float[Θ] ->
  Point`，`Point + Point` 报 `E0405`）。温度场的平均用
  `centroid`，不是 `mean`（§13.9.2）。

### 5.6.3 运算

```
p - q        : Float[d]        两个点相减得到温差
p + x        : Point[d]        点加温差
x + p        : Point[d]
p - x        : Point[d]        点减温差
p + q        : 错误 E0405
k * p        : 错误 E0405
p / k        : 错误 E0405
lerp(p,q,t)  : Point[d]        仿射组合，t : Float[1]
```

- `Point` 可以比较：`p == q`、`p < q`（沿仿射线）。
- `Point` 与温差：`p + x`/`p - x` 只接受 `x : Float[Θ]`（`Point` 本身是
  Float）。精确温差 `Real[Θ]` 必须先 `to_float`（返回 `Result`）；因此
  `25 °C + 10.0 K` 合法，`25 °C + 10 K` 报 `E0302`（提示写 `10.0 K` 或
  `to_float(10 K)`）。
- `min(p,q)`、`max(p,q)` 返回 `Point`；`centroid : Tensor<1,Point[d]> -> Point[d]`
  是点的平均（`mean` 只接受标量）。
- `to(°C, p)` 设置显示单位；`value_in(°C, p)` 返回摄氏数值。
- `base(p)` 返回 `Float[Θ]` 的 K 值（去掉点标记）。
- `Point` 不能与 `Unit` 相乘/相除；`p / K` 报 `E0405`。

构造函数与换算函数：

```
point     : (Float[1], Unit[d]) -> Point[d]
to        : (Unit[d], Scalar<ρ,d>) -> Scalar<ρ,d>
to        : (Unit[d], Point[d]) -> Point[d]
value_in  : (Unit[d], Scalar<ρ,d>) -> Scalar<ρ,1>      # 仿射单位时 E0510
value_in  : (Unit[d], Point[d]) -> Float[1]
base      : Scalar<ρ,d> -> Scalar<ρ,d>
base      : Point[d] -> Float[d]
delta_to  : (Unit[d], Scalar<ρ,d>) -> Scalar<ρ,d>     # 只用尺度，不用偏移
unit_pow_dyn : (Unit[d], Int) -> Dyn<Real>            # 运行时指数
lerp      : (Point[d], Point[d], Float[1]) -> Point[d]
```

`point(300.0, K)` 的第一个参数是以 `K` 为单位的数值系数，类型 `Float[1]`；
`point(25.0, °C)` 是 298.15 K。`delta_to(°C, 10.0 K)` 把温差 10 K 显示为
“10 °C 的温差”（尺度 1，无偏移），与绝对温度 `10 °C` 区分。

### 5.6.4 为什么开尔文是线性的

`K` 是线性单位（尺度 1、偏移 0），所以 `300 K` 在类型上是温差。这看起来
不对称，但它是物理上诚实的：开尔文刻度同时用于绝对温度和温差，单位本身
无法区分。Celsius/Fahrenheit 是仿射刻度，字面量天然是点。要写绝对温度：

```aevia
let T1 = 25.0 °C            # Point[Θ]
let T2 = point(300.0, K)    # Point[Θ]
let dT = T2 - T1            # Float[Θ]
let T3 = T1 + 10.0 K        # Point[Θ]
```

### 5.6.5 仿射单位在单位表达式中

`°C/s`、`°C*m` 没有意义；单位解析器拒绝仿射单位出现在乘积/幂中
（`E0508`）。`°C` 只能单独作为字面量单位，或作为 `to`/`value_in` 的参数。

## 5.7 单位换算的实现

单位值在运行时是：

```
Unit {
    name: String,
    dim: Dim,
    scale: Real,        # 相干 SI 倍数，精确有理或符号
    offset: Option<Real>,  # 仅仿射
    prefixable: bool,
}
```

- 尺度在编译期折叠；`3 km` 直接生成 `3000`，不保留 `km` 的乘法。
- 单位表在编译期是常量；运行时只保留显示单位的名字和换算所需的信息。
- `to(u, x)` 在运行时把 `x` 的显示单位设为 `u`；打印时用 `x / u.scale`。
- 仿射换算：`value_in(°C, p) = (p - 273.15 K) / 1 K`。
- 单位相等：同量纲且尺度相同（仿射还要偏移相同）。`m` 与 `100 cm` 是
  不同单位值，但同量纲；`to` 可以互换。

## 5.8 物理常数

`Units.Constants` 提供 CODATA 2022 的常数（节选）：

| 常数 | 类型 | 值 |
|---|---|---|
| `c` | `Real[L/T]` | `299792458 m/s`（精确） |
| `g_0` | `Real[L/T^2]` | `9.80665 m/s^2`（精确） |
| `h` | `Real[Energy*T]` | `6.62607015e-34 J*s`（精确） |
| `hbar` | `Float[Energy*T]` | `1.054571817e-34 J*s` |
| `k_B` | `Real[Energy/Θ]` | `1.380649e-23 J/K`（精确） |
| `N_A` | `Real[1/N]` | `6.02214076e23 /mol`（精确） |
| `e` | `Real[Charge]` | `1.602176634e-19 C`（精确） |
| `m_e` | `Float[M]` | `9.1093837015e-31 kg` |
| `m_p` | `Float[M]` | `1.67262192369e-27 kg` |
| `G` | `Float[L^3/(M*T^2)]` | `6.67430e-11 m^3/(kg*s^2)` |
| `eps_0` | `Float[Capacitance/L]` | `8.8541878128e-12 F/m` |
| `mu_0` | `Float[Inductance/L]` | `1.25663706212e-6 H/m` |
| `alpha` | `Float[1]` | `7.2973525693e-3` |
| `R` | `Float[Energy/(N*Θ)]` | `8.31446261815324 J/(mol*K)` |
| `F` | `Real[Charge/N]` | `96485.33212 C/mol` |
| `sigma_SB` | `Float[Power/(L^2*Θ^4)]` | `5.670374419e-8 W/(m^2*K^4)` |

- 精确常数用 `Real`（有理数或符号）；测量常数用 `Float`。
- 常数不是单位；它们有量纲，可以参与运算。
- `pi`、`e`（自然常数）、`i` 在 `Math` 中，不在 `Units.Constants`。
  注意 `e` 与基本电荷 `e` 重名；基本电荷用 `e_charge` 或 `q_e`。标准库
  用 `q_e`，避免与自然常数冲突。
- 常数的不确定度：核心语言不携带不确定度；需要时用 `Measurement` 库类型
  （§8.14）。

## 5.9 单位错误

| 代码 | 情况 |
|---|---|
| `E0501` | 未知单位名 |
| `E0502` | 单位未导入/未打开 |
| `E0503` | 前缀不能叠加或单位不可加前缀 |
| `E0504` | 仿射单位偏移量纲不匹配 |
| `E0505` | 重复的基单位 |
| `E0506` | 单位定义循环或尺度量纲不为 1 |
| `E0507` | 前缀单位与显式单位冲突 |
| `E0508` | 仿射单位出现在乘积/幂中 |
| `E0509` | `to` 的单位量纲与值不匹配 |
| `E0510` | 对温差使用仿射单位显示 |

## 5.10 与 rssn 单位层的衔接

rssn 的 `rssn::dim` 提供 `DimTerm`/`DimArena`/`UnitRegistry`；`units` 规则集
只是 registry 的一个消费者。Aevia 的单位层是编译期视图，`Units.SI` 由
`UnitRegistry` 生成，并且有 `Point` 类型。两者衔接方式：

1. **单一 registry。** `Units.SI` 是 `UnitRegistry` 的生成视图；Aevia 不
   维护第二张表。用户 `dimension`/`unit` 声明注册进同一个 registry。
2. **项图不带单位。** `Real`/`Float` 值在宿主侧是相干 SI 数值；需要
   rssn 排版/导出时，才用 `quantity(v, "<相干 SI 单位>")` 包一层，仅用于
   输出，不进入 `compute`。
3. **仿射单位在桥接层消解。** `Point` 在进入 rssn 前转成 K 的 `Float`；
   `to(°C, ...)` 只发生在显示层。
4. **registry 往返。** 仅对**纯有理指数**，经 `Dimension::exact_factors`
   做 `DimTerm -> Dimension -> DimTerm` 精确往返；符号指数不支持往返，
   不得解析 `Dimension.factors` 文本。`UnitDef -> scale/offset -> UnitDef`
   必须精确往返。
5. **rssn 的 `DimensionMode::Strict` 只用于 FFI/导出。** 静态路径的项图
   无量纲，用 `Off`；`Dyn<ρ>` 的量纲由宿主保存为 `DimTerm`，不依赖字符串。

下一章定义求值：一个 `Real` 值在什么时候是精确数、什么时候是符号项、
什么时候变成 `Float`，以及张量和 IO 如何运行。
