# 05 — 单位

第 04 章把量纲变成了类型；本章把单位变成**编译期已知的尺度**。单位不进入类型，只提供：

1. 一个量纲 `d`；
2. 一个尺度 `s`（相对相干 SI 单位的精确倍数）；
3. 一个仿射偏移 `o : Real[d]`（仅仿射单位）。

单位字面量 `3.2 km` 在编译期折叠为 `3200.0`（相干 SI 值），类型 `Float[L]`；`km` 不留在
值里。需要显示时用 `to(km, x)` 附加显示单位（§5.4）。

## 5.1 单位声明

```aevia
dimension Currency                  # 自定义基量纲
unit USD : Currency = 1             # 基单位
unit EUR : Currency = 1.08 USD      # 派生单位

unit m : Length = 1 prefixable      # 基单位，允许 SI 前缀
unit km : L = 1000 m                # 显式派生
unit N : M*L/T^2 = 1 kg*m/s^2
unit degC : Θ = 1 K offset 273.15 K # 仿射单位
```

### 5.1.1 语法

```
dimension_decl ::= [ "private" ] "dimension" UIDENT [ "=" dim ]
unit_decl      ::= [ "private" ] "unit" unit_name ":" dim "=" unit_rhs [ "prefixable" ]
unit_rhs       ::= unit_scale [ unit_expr ] [ "offset" unit_scale unit_expr ]
unit_scale     ::= number [ "/" number ] | "pi" [ "/" number ]
```

- `dimension`/`unit` 默认**导出**，`private` 仅本模块可见。导出项进入 manifest 与 registry，
  下游包可 `import`。`Units.*` 是 `manifest.units` 的生成视图，用户声明无需改标准库。
- `unit_name` 是标识符，或带 `°` 的特殊名（`°C`、`°F`）。
- `unit_scale` 是正有理数或 `pi` 的有理倍数。十进制字面量按**精确十进制**解释
  （`0.3048` 是 `381/1250`），不是 `Float`，所以 `ft → m` 换算精确。单位声明在编译期求值，
  不允许运行时表达式；要近似尺度就写足够精度的十进制字面量。
- `unit_expr`（§5.2）省略时表示基单位：尺度必须为 1，量纲必须是基量纲。
- `offset` 仅用于仿射单位，偏移量纲必须等于单位量纲（`E0504`）。
- `prefixable` 自动生成 SI 前缀版本（§5.1.4）。

### 5.1.2 基单位与派生单位

- 单位名必须唯一，重复声明报 `E0505`。同一量纲可以有多个尺度为 1 的单位；它们互相等价。
- 单位按声明顺序可见：使用尚未声明的单位是 `E0501`，因此定义不可能成环。
- 派生单位的尺度在编译期按精确有理数计算；若指数使尺度没有精确值（`3 ft^(1/2)`，
  `ft` 的尺度不是完全平方）报 `E0506`。
- 单位声明可跨模块导入；`private unit` 不导出。

### 5.1.3 单位命名空间

单位名与值名是**两个命名空间**：

- 字面量 `3 m` 中的 `m` 在单位命名空间解析；`let m = 5` 不改变 `3 m`。要乘以变量，写 `3 * m`。
- 普通表达式中的单位名是值 `Unit[d]`（全局可见）。
- 单位名冲突时最长匹配优先：`ms` 是毫秒，不是 `m*s`；要写米乘秒，写 `m*s`。
- 单位值不能用限定名访问（`Units.SI.km` 无效）；单位在内核表中全局可见，用短名即可。

单位名与量纲名也是两个命名空间，同一拼写可各有含义；解析器按位置区分：

| 拼写 | 单位位 | 量纲位 |
|---|---|---|
| `L` | 升（`liter`/`litre` 的别名，`prefixable`） | `Length` 的显示别名 |
| `T` | 特斯拉 | `Time` 的显示别名 |
| `G` | 高斯（`1e-4 T`） | 不是引力常数 |
| `h` | 小时 | 不是普朗克常数 |

规范名一律用全名（`Length`/`Mass`/`Time`/…），单字母只用于源码与显示。

### 5.1.4 前缀

| 符号 | 因子 | 符号 | 因子 | 符号 | 因子 | 符号 | 因子 |
|---|---|---|---|---|---|---|---|
| `Q` quetta | 10^30 | `P` peta | 10^15 | `h` hecto | 10^2 | `µ` micro | 10^-6 |
| `R` ronna | 10^27 | `T` tera | 10^12 | `da` deca | 10^1 | `n` nano | 10^-9 |
| `Y` yotta | 10^24 | `G` giga | 10^9 | `d` deci | 10^-1 | `p` pico | 10^-12 |
| `Z` zetta | 10^21 | `M` mega | 10^6 | `c` centi | 10^-2 | `f` femto | 10^-15 |
| `E` exa | 10^18 | `k` kilo | 10^3 | `m` milli | 10^-3 | `a` atto | 10^-18 |
| | | | | | | `z`/`y`/`r`/`q` | 10^-21/-24/-27/-30 |

- 前缀只加在 `prefixable` 单位上。`kg` 是 SI 基单位，前缀加在 `g`（`1/1000 kg`，
  `prefixable`）上；解析时**精确匹配优先于前缀分解**，所以 `kg` 是基单位而不是 `k`+`g`。
- 最长匹配：`dam` 是 `da`+`m`，不是 `d`+`am`。
- 前缀不叠加：`kkm` 不是单位，报 `E0501`。
- 二进制前缀 `Ki`、`Mi`、`Gi`、`Ti` 仅用于信息单位（`bit`、`B`）。

## 5.2 单位表达式

```
unit_expr   ::= unit_factor { ("*" | "·" | "/") unit_factor }
unit_factor ::= unit_atom [ "^" exponent ]
unit_atom   ::= unit_name | "(" unit_expr ")" | "%"
```

- `*` 与 `·` 同义；`/` 左结合。指数是整数字面量或括号中的有理数：`m^2`、`s^-1`、`m^(1/2)`。
- 表达式内不允许数字（指数除外），不允许空白并置。
- 仿射单位只能单独出现，出现在乘积/幂中报 `E0508`（§5.6.5）。

### 5.2.1 单位值

单位表达式在值层是 `Unit[d]` 值：

```aevia
let speed_unit = km / h          # Unit[L/T]，尺度 1000/3600
let v = 100 * speed_unit         # Real[L/T]，值 250/9
```

`100 speed_unit` 不是单位字面量（`speed_unit` 不是单位名），报 `E0501`；写 `100 * speed_unit`。
`Unit[d]` 支持 `u * v : Unit[d*e]`、`u / v : Unit[d/e]`、`u ^ n : Unit[d^n]`（`n` 为字面量），
也可比较（同量纲）、打印、作参数。单位不是数值，不能直接与 `Real` 相加。

### 5.2.2 单位字面量的求值

```
value = number * scale(unit_expr) + offset(unit_expr)
type  = Scalar<rep(number), dim(unit_expr)>
```

- 线性单位 `offset = 0`：`3 km` → `3000`，`Real[L]`。
- 仿射单位只能单独出现：`25 °C` → `25 + 273.15 = 298.15`，`Point[Θ]`（§5.6）。
- `number` 为 `Int` 时默认 `Real`（精确），为 `Float` 时默认 `Float`：`3 km : Real[L]`，
  `3.0 km : Float[L]`。期望类型可覆盖表示：`let x: Real[L] = 3.0 m` 得精确 `3`；
  `fall_time(20 m)` 中 `20 m` 实例化为 `Float[L]`。
- 尺度含 `pi` 时结果可能是符号表达式：`30 deg` → `pi/6 : Real[1]`；期望 `Float` 时为
  `0.5235987755982988`。

## 5.3 单位表

标准库 `Units.SI` 提供下列单位（节选）。

### 5.3.1 SI 基单位

| 单位 | 量纲 | 说明 |
|---|---|---|
| `m` | `L` | 米，prefixable |
| `kg` | `M` | 千克（`g` prefixable） |
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
| `lm` | `Lum` | `cd*sr` |
| `lx` | `Lum/L^2` | `lm/m^2` |
| `Bq` | `1/T` | `1/s` |
| `Gy`, `Sv` | `L^2/T^2` | `J/kg` |
| `kat` | `Amt/T` | `mol/s` |
| `rad`, `sr` | `1` | 无量纲，尺度 1 |

### 5.3.3 常用非 SI 单位

| 单位 | 量纲 | 定义 |
|---|---|---|
| `min` / `h` / `d` / `week` / `yr` | `T` | `60 s` / `3600 s` / `86400 s` / `604800 s` / `31557600 s` |
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
| `bar` / `atm` / `mmHg` / `psi` | `Pressure` | `100000 Pa` / `101325 Pa` / `133.322387415 Pa` / `6894.757293168 Pa` |
| `cal` | `Energy` | `4.184 J` |
| `Wh` | `Energy` | `3600 J`（`kWh` 是前缀形式） |
| `hp` | `Power` | `745.6998715822702 W` |
| `kn` / `mph` | `L/T` | `1852/3600 m/s` / `1609.344/3600 m/s` |
| `ft` / `in` / `yd` / `mi` / `nmi` | `L` | `0.3048` / `0.0254` / `0.9144` / `1609.344` / `1852` `m` |
| `acre` | `L^2` | `4046.8564224 m^2` |
| `gal` | `L^3` | `0.003785411784 m^3` |
| `oz` / `lb` | `M` | `0.028349523125 kg` / `0.45359237 kg` |
| `lbf` | `Force` | `4.4482216152605 N` |
| `°C` | `Θ` | 仿射：`1 K offset 273.15 K` |
| `°F` | `Θ` | 仿射：`5/9 K offset 255.3722222222222 K` |
| `bit` / `B` / `KiB` | `1` | 信息：`1` / `8 bit` / `1024 B` |

信息单位是无量纲的，因为大量公式把 bit 当概率/熵用。需要独立的 `Information` 基量纲时，
声明 `dimension Information`（kernel 的 `Config` 须声明 `base_dimension("Information")`）。

### 5.3.4 单位模块

| 模块 | 内容 |
|---|---|
| `Units.SI` | 基单位、派生单位、前缀、常用非 SI |
| `Units.Imperial` | 英制长度/质量/体积/压强/温度 |
| `Units.Astronomy` | `au`、`pc`、`ly`、`Rsun`、`Msun`、`Mearth`、`jup`（`Mpc` 由 `pc` 加前缀） |
| `Units.CGS` | `cm`、`g`、`dyn`、`erg`、`Gal`、`P`、`St`、`G`、`Mx`、`Ba`、`Oe` |
| `Units.Constants` | 物理常数（§5.8） |
| `Units.Natural`、`Units.Planck`、`Units.Atomic`、`Units.Geometrized` | 单位制下的常数模块（§5.11.1） |

状态：以上模块均可 `open`，内容从 kernel manifest 生成（`manifest.units`、`manifest.constants`、
`manifest.unit_systems`），每个 `UnitValue` 带 `aliases`/`prefixable`/`category`/`since`。单位是
**session 私有**的：`Session::register_unit` 注册的单位只出现在该 session 的 manifest 中。

## 5.4 换算与显示

### 5.4.1 三个操作

```aevia
let d = 3.2 km              # Float[L]，值 3200.0
let e = to(m, d)            # 值 3200.0，显示单位 m
let f = value_in(km, d)     # Float[1]，值 3.2
let g = base(d)             # 值 3200.0，去掉显示单位
```

- `to(u, x)`：`x : Scalar<ρ,d>`、`u : Unit[d]`；值与类型不变，只附加显示单位。
- `value_in(u, x)` 等价于 `x / u`，返回无量纲数值。
- `base(x)` 去掉显示单位，返回相干 SI 值。
- 单位量纲与值不符是 `E0509`。
- **仿射单位只接受 `Point[d]`**：`value_in(°C, point(298.15, K))` 是 `25`；对温差
  （`Scalar<ρ,Θ>`）用仿射单位报 `E0510`，所以 `value_in(°C, 298.15 K)` 错误。温差的显示用
  `delta_to(°C, dT)`，它只用尺度、不用偏移。

状态：`delta_to(°C, 10.0 K)` 打印为 `-263.15 °C`（仍套用了偏移），与上述语义不符，属已知缺陷。

### 5.4.2 显示单位的传播

运行时每个标量可带一个可选的显示单位：

- `x + y`、`x - y`：两边显示单位相同则保留，否则回到 SI。
- `x * y`、`x / y`：一方无量纲且带显示单位时保留另一方的显示，否则丢弃。
- `to` 显式设置，`base` 显式清除；函数调用默认丢弃（`abs` 等保持形状的除外）。

显示单位是**元数据**，不参与相等、类型或序列化：`3.2 km == 3200 m` 为 `true`。

状态：丢弃显示单位的警告 `W0501` 尚无发射点。

### 5.4.3 打印

- 有显示单位时按该单位换算后打印（`3.2 km`、`25 °C`）。
- 没有时选“最佳单位”：优先 SI 派生单位（`N`、`J`、`Pa`），否则基单位乘积
  （`kg*m/s^2`）；分数指数正确打印（`T^(1/2)` → `s^(1/2)`）。
- `show` 保证可读；`repr` 保证可解析回同一个值。
- 打印精度由 `Float` 的最短往返表示决定；`Real` 精确值打印为分数或根式。

## 5.5 角度与无量纲单位

- `rad` 是尺度为 1 的无量纲单位；`deg`/`°` 的尺度是 `pi/180`。
- `sin(30 deg)` 与 `sin(30)`（30 弧度）都合法，`sin` 只要求无量纲；类型系统不区分弧度与纯数。
- `atan2(y, x)` 返回无量纲弧度；用 `to(deg, ...)` 显示为角度。

## 5.6 仿射单位与 `Point`

### 5.6.1 问题与类型

温度有两种用法：**绝对温度**（仿射空间中的点，`25 °C` = 298.15 K）与**温差**（向量，可相加、缩放）。
混为一谈会让 `20 °C + 30 °C` 得到错误的 50 °C 或 596.3 K。

```
Point[Θ]        绝对温度（仿射点）
Float[Θ]        温差（向量）
```

- 只有仿射单位字面量产生 `Point[d]`：`25 °C`、`77 °F`；线性单位字面量产生温差：`25 K` 是温差。
- 用开尔文写绝对温度用 `point(298.15, K)`。
- `Point` 只支持 `Float` 表示；符号温度用 `Real[Θ]` 的温差。
- `Point` 可作张量元素（`Tensor<[nx,ny], Point[Θ]>` 是温度场），元素级运算按点规则；
  温度场的平均用 `centroid`，不是 `mean`（§13.9.2）。

### 5.6.2 运算

```
p - q        : Float[d]        两点相减得温差
p + x, x + p : Point[d]        点加温差
p - x        : Point[d]        点减温差
lerp(p,q,t)  : Point[d]        仿射组合，t : Float[1]
p + q, k * p, p / k, p / K   : 错误 E0405
```

- `Point` 可比较：`p == q`、`p < q`；`min`、`max` 返回 `Point`；
  `centroid : Tensor<1,Point[d]> -> Point[d]`。
- `p + x` / `p - x` 的 `x` 须是 `Float[Θ]`。精确的 `Real[Θ]` 变量不能直接参与（`E0302`，须
  `to_float`）；字面量 `10 K` 按期望类型实例化为 `Float`，所以 `25.0 °C + 10 K` 合法。
- `to(°C, p)` 设置显示单位，`value_in(°C, p)` 返回摄氏数值，`base(p)` 返回 K 值 `Float[Θ]`。

```
point     : (Float[1], Unit[d]) -> Point[d]
to        : (Unit[d], Scalar<ρ,d>) -> Scalar<ρ,d>      | (Unit[d], Point[d]) -> Point[d]
value_in  : (Unit[d], Scalar<ρ,d>) -> Scalar<ρ,1>      # 仿射单位时 E0510
value_in  : (Unit[d], Point[d]) -> Float[1]
base      : Scalar<ρ,d> -> Scalar<ρ,d>                 | Point[d] -> Float[d]
delta_to  : (Unit[d], Scalar<ρ,d>) -> Scalar<ρ,d>      # 只用尺度，不用偏移
unit_pow_dyn : (Unit[d], Int) -> Dyn<Real>             # 运行时指数
lerp      : (Point[d], Point[d], Float[1]) -> Point[d]
```

`point(25.0, °C)` 是 298.15 K；第一个参数是以该单位计的数值系数。

### 5.6.3 为什么开尔文是线性的

`K` 是线性单位（尺度 1、偏移 0），所以 `300 K` 在类型上是温差：开尔文刻度同时用于绝对温度与
温差，单位本身无法区分；摄氏/华氏是仿射刻度，字面量天然是点。

```aevia
let T1 = 25.0 °C            # Point[Θ]
let T2 = point(300.0, K)    # Point[Θ]
let dT = T2 - T1            # Float[Θ]
let T3 = T1 + 10.0 K        # Point[Θ]
```

### 5.6.4 仿射单位在单位表达式中

`°C/s`、`°C*m` 没有意义，解析器拒绝（`E0508`）。`°C` 只能单独作字面量单位，或作
`to`/`value_in` 的参数。

## 5.7 换算的实现

- 尺度在编译期折叠为精确有理数（或含 `pi` 的符号值）；`3 km` 直接生成 `3000`。
- 运行时只保留显示单位的名字与换算信息：`to(u, x)` 设置显示单位，打印时用 `x / u.scale`；
  仿射换算 `value_in(°C, p) = (p - 273.15 K) / 1 K`。
- 单位相等：同量纲且尺度相同（仿射还要偏移相同）。`m` 与 `100 cm` 是不同单位值，但同量纲，
  `to` 可互换。

## 5.8 物理常数

`Units.Constants` 由 `manifest.constants` 生成，符号与 RSSN 完全一致：

| 类别 | 常数（SI 单位） |
|---|---|
| 精确 | `c` (`m/s`)、`g_n` (`m/s^2`)、`h`、`hbar` (`J*s`)、`k_B` (`J/K`)、`N_A` (`1/mol`)、`e` (`C`)、`F` (`C/mol`)、`R` (`J/(mol*K)`)、`M_u` (`kg/mol`)、`T_tp` (`K`)、`atm` (`Pa`)、`G_0` (`S`)、`K_J` (`Hz/V`)、`R_K` (`ohm`)、`Phi_0` (`Wb`)、`K_cd` (`lm/W`)、`dnu_Cs` (`Hz`) |
| 测量 | `G` (`m^3/(kg*s^2)`)、`alpha` (`1`)、`R_inf` (`1/m`)、`a_0` (`m`)、`epsilon_0` (`F/m`)、`mu_0` (`N/A^2`)、`mu_B` (`J/T`)、`m_e`、`m_p`、`m_n`、`m_u` (`kg`)、`sigma_SB` (`W/(m^2*K^4)`) |

- 精确常数是定义值或其精确派生，无 `uncertainty`；测量常数是 CODATA 2022 值，带标准不确定度。
  两类在 manifest 中都是精确有理数（`ScaleValue`），`exact` 只表示“定义值”，不表示存储类型。
- 常数不是单位：有量纲、可参与运算，但不能出现在单位位置。核心语言不携带不确定度，
  需要时用 `Measurement`（§8.14）。
- `pi`、`e`（自然常数）、`i` 在 `Math` 中。`Units.Constants.e` 是**基本电荷**，与 `Math.e` 同名，
  同时 `open` 时须限定。

每个条目的元数据：

```
type ConstantInfo = {
    name: String,              # "speed of light in vacuum"
    symbol: String,            # "c"
    value: Scalar<Real, d>,    # 相干 SI 值
    dimension: Dim,
    unit: String,              # "m/s"
    exact: Bool,
    uncertainty: Option<Scalar<Float, d>>,
    source: String,
}
```

## 5.9 单位错误

| 代码 | 情况 |
|---|---|
| `E0501` | 未知单位名（含不可叠加的前缀 `kkm`、先使用后声明） |
| `E0502` | 单位未导入/未打开（保留码） |
| `E0503` | 单位不可加前缀（保留码） |
| `E0504` | 仿射单位偏移量纲不匹配 |
| `E0505` | 重复的单位名 |
| `E0506` | 尺度对该指数没有精确值 |
| `E0507` | 前缀单位与显式单位冲突（保留码） |
| `E0508` | 仿射单位出现在乘积/幂中 |
| `E0509` | `to`/`value_in` 的单位量纲与值不匹配 |
| `E0510` | 对温差使用仿射单位显示 |
| `E0511`/`E0512` | 单位制错误（§5.11） |

状态：`E0502`/`E0503`/`E0507` 是保留码，当前不可达：单位表是 kernel 全表加本地声明，没有模块级
单位命名空间，“未导入”“禁止前缀”“前缀/显式冲突”三种状态都不存在；编译器不会用别的错误冒充它们。

## 5.10 与 rssn 单位层的衔接

1. **单一事实来源。** `Units.*` 是 `manifest.units` 的生成视图；用户 `dimension`/`unit` 声明注册进
   同一个 registry 并回流到 manifest。`UnitRegistry` 只用于 `resolve`/`format` 这类运行时服务。
2. **项图不带单位。** `Real`/`Float` 在宿主侧是相干 SI 数值；仅在 rssn 排版/导出时用
   `quantity(v, "<相干 SI 单位>")` 包一层，不进入 `compute`。
3. **仿射单位在桥接层消解。** `Point` 进入 rssn 前转成 K 的 `Float`；`to(°C, ...)` 只发生在显示层。
4. **registry 往返。** 仅对纯有理指数，经 `Dimension::exact_factors` 做
   `DimTerm → Dimension → DimTerm` 精确往返；符号指数不支持往返，不得解析 `Dimension.factors` 文本。
   `UnitDef → scale/offset → UnitDef` 必须精确往返。
5. **`DimensionMode::Strict` 只用于 FFI/导出。** 静态路径项图无量纲，用 `Off`；`Dyn<ρ>` 的量纲
   由宿主保存为 `DimTerm`。

## 5.11 单位制

物理学家常把几个常数取作 1（`c = 1`、`ħ = 1`）。Aevia 把这做成**模块级的量纲约化**：模块里
*写出来*的量纲随单位制化简，而每个值仍是诚实的 SI 量。

### 5.11.1 选择单位制

由模块头选择，名字是标识符：

```aevia
module Hep with units natural
```

kernel 提供四个单位制（`manifest.unit_systems`）；头里写其他名字是 `E0512`，诊断列出可选名字。

| 名字 | 取作 1 的量 | 约化后剩下的生成量纲 |
|---|---|---|
| `natural` | `c`、`hbar`、`k_B`、`epsilon_0`（Heaviside–Lorentz） | 能量、物质的量、发光强度 |
| `planck` | `c`、`hbar`、`G`、`k_B`、`4 pi epsilon_0` | 物质的量、发光强度 |
| `atomic` | `hbar`、`m_e`、`e`、`4 pi epsilon_0` | 温度、物质的量、发光强度 |
| `geometrized` | `c`、`G` | 长度、电流、温度、物质的量、发光强度 |

`Units.Natural` 等常数模块自己声明 `with units <system>`，`c`、`hbar`、`k_B` 在 `Units.Natural`
中由约化本身得到精确的 `1`。

### 5.11.2 约化

设取作 1 的量的量纲为 `q₁ … q_k`、生成量纲为 `g₁ … g_m`，它们在有理数上构成七个 SI 基量纲的一组基。
每个 SI 量纲 `d` 唯一分解为

```
d = Π qᵢ^λᵢ · Π gⱼ^μⱼ
```

`R(d) = Π gⱼ^μⱼ` 称 `d` 的**代表**。量纲为 `d` 的量 `x` 的**单位制值**是 `x / Π Qᵢ^λᵢ`（`Qᵢ` 为精确
SI 值），它是量纲为 `R(d)` 的 SI 量。例：`natural` 中 `R(L) = R(T) = Energy^-1`，`1 m` 的单位制值是
`1 m / (ħ c) ≈ 5.07e24 J⁻¹`。用户基量纲与量纲变量不在 SI 基张成的空间里，约化不改动它们。

带单位制的模块中，**该模块里写出的**量纲标注与单位字面量先约化再检查，所以 `fn f(x: Real[L])` 与
`fn f(x: Real[T])` 的参数在 `natural` 中是同一个类型。

### 5.11.3 值不被重新解释

约化只改变*写下*的量纲，从不改变值：每个量仍是 SI 量，只是类型用代表量纲书写。值可无需转换地
进出带单位制的模块；**导入的绑定保持它们在原模块中的 SI 类型**。

### 5.11.4 没有精确尺度的单位

单位字面量按单位制约化时需要精确尺度。若某个指数不是整数，尺度无理，该单位在此单位制下没有
精确尺度，报 `E0511`（典型：`natural` 中的 `C`，需要 `(ε₀ħc)^(1/2)`）。办法：直接写单位制中的数，
或用 `to_system`（§5.11.5）换算 SI 值。

### 5.11.5 显式换算

```aevia
to_system("natural", x)           # SI 量 x → 其代表量纲 R(d) 的单位制值
from_system<L>("natural", y)      # 单位制值 y → SI 量纲 L 的量
```

- 单位制名必须是字符串字面量，否则 `E0302`；未知名字 `E0512`；参数个数不是 2 是 `E0315`。
- `to_system` 要求实参量纲在检查时已知，否则 `E0304`；结果是同一表示、量纲为 `R(d)` 的 `Scalar`。
- `from_system<D>` 必须写出目标 SI 量纲 `D`（约化会遗忘它：`L` 与 `T` 都是 `Energy^-1`），否则
  `E0304`；`D` 总按 SI 读。实参量纲必须等于 `R(D)`，否则按普通量纲不匹配报告。
- 结果表示与实参相同：`Float`/`Complex` 乘以浮点因子；精确 `Int`/`Real` 在因子是不含 π 的精确尺度时
  保持精确有理数，否则是符号项，不会静默降为浮点。

### 5.11.6 显示

带单位制的模块里，没有自带显示单位的量按单位制的生成量纲显示：`natural` 中能量倒数显示为
`eV^-1`，而不是 `s^2/(m^2*kg)`。这只选择显示单位，值不变；记录、变体与元组逐字段显示。

下一章定义求值：`Real` 在什么时候是精确数、符号项或 `Float`，以及张量和 IO 如何运行。
