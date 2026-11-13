# 10 — rssn 后端映射

Aevia 不重新实现计算机代数。rssn 提供项表示、重写引擎、量纲层、数值内核、
JIT 和仿真；Aevia 提供语法、类型/量纲检查、闭包、记录、张量形状、模块、
IO 和标准库编排。这一章定义两者的契约：哪些能力直接映射，哪些要包一层，
哪些是已知缺口。

## 10.0 反射优先：没有手写算子绑定

Aevia **不维护** `sin -> rssn::sin`、`det -> rssn::det` 这样的绑定表。启动/
构建时读取 RSSN 的 **Kernel Manifest**，生成 `Rssn` 模块：

```aevia
Rssn.sin(0.5)                       # Derived 的数值重载：Real
Rssn.sin(x)                         # Derived 的符号重载：Dyn<term>
Rssn.det(M)                         # Overlay：Tensor<[n,n],Real> -> Real
Rssn.dynamic("myplugin_softplus", args)  # 插件/叶子：没有签名
```

- `signature_source = Declared | Overlay | Derived` → 类型化声明；
- `Derived` 是“由注册信息推出”的保守签名：`eval` → `(Real,…) -> Real`
  （谓词 `-> Bool`），非叶子另加 `(Term,…) -> Term`；它不区分
  `Tensor`/`Quantity`/`Int`，所以 IDE 要标“保守推断”；
- `None`（叶子/无调用）→ `dynamic` 声明；RSSN 会做**签名级**校验
  （`check_args`，参数不被任何 overload 接受时报 `OpError::Type`），但
  不做 Aevia 的完整类型/量纲检查，那由静态检查器或 `Rssn.typed` 负责；
- 新算子只要在 RSSN 注册，下一次构建就出现，不需要改 Aevia 编译器；
- `aevia rssn --emit` 的**源码拼写现在也是精确的**（task-31）：维度参数写成
  Aevia 的量纲变量（`Real[T]`、`Real[T^(1/2)]`、`Tensor<2, Real[T^(1/2)]>`、
  `Unit[T]`），变量名在声明内局部（`T`、`T2`…）并**在参数与返回间共享**
  ——`sde_wiener(a3: Real[T]): Tensor<2, Real[T^(1/2)]>` 里的 `T` 与
  `T^(1/2)` 是同一个变量。`Dyn<term>` 只保留在**确实无法拼写**的地方：
  manifest 自己声明为 `Dyn(rep, any)` 的签名（`quantity`/`convert` 的值参数
  与结果、以及 derived 签名）、**秩变量张量**（`transpose`/`rank`/`dot`/
  `norm`/`sum`；Aevia 声明需要具体秩，`Tensor<n, τ>` 是 `E0304`）、类型
  参数、未知行 `Table`、`Device`、变体、host opaque、`Never`。生成的
  `Units.aev`/`Constants.aev` 无条件通过 check；`Rssn.aev` 也通过——生成器
  对每条声明只发**字面量** `op` + `key` 的 `Rssn.invoke`，checker 按 pinned
  overload 校验这两个字面量并把结果类型定为该 overload 的声明结果（每次调用
  新维度变量；`Rssn.sde_wiener` → `Tensor<2, Real[T^(1/2)]>`）。非字面量
  key 保持动态 `Value`，运行时答 `E1002`/`E1003`/`E1005`；checker 不重新
  解析或执行该 key（见 §13.2.2）。这套验收在**干净进程**里由
  `aevia-rssn` 的 `emit_typecheck`（`the_emitted_modules_type_check_with_no_diagnostics`）
  断言：emit 后三个生成模块 **0 diagnostics**；`cargo test -p aevia-rssn`
  为 78 项通过；
- 静态重载用 `OpKey { rule_set, name, signature_hash }`；typed 调用走
  `Rssn.call_key(key, …)` 钉死重载；dynamic 调用走 `call_value`，多注册时
  按 `Match` 格选、并列报 `E1002`（§13.4.1）。`resolve_overload` 只用于
  “先解析再缓存”，且只缓存 `Best::Unique` 的结果。

完整的内核侧契约见 rssn 仓库的 `docs/KERNEL.md`，
Aevia 侧规则见 [13 — Extensibility](13-extensibility.md)。

## 10.1 rssn 的能力边界

rssn 的核心入口是：

```rust
let s = Session::new();
let x = s.sym("x");
let answer = s.compute(diff(sin(x) * cos(x), x), &Config::new())?;
```

- `Session` 拥有一个 hash-consed 项图；`Term<'s>` 是带生命周期的句柄。
- `Config` 指定规则集、目标相位（`Symbolic`/`Numeric`）、绑定、假设、预算、
  量纲模式、量纲假设、自定义基量纲。
- `compute` 把请求复制到模板图的克隆中，运行 e-graph 重写，抽取答案，
  再把答案导入回会话。
- `Answer` 包含 `term`、`value`、`error`、`reduced`、`dimension`、
  `diagnostics`、`report`。
- `Term::eval(bindings)` 不做重写，直接数值求值。
- `Term::compile(inputs)` 把闭式编译成可调用函数；`jit` feature 用
  Cranelift（0.137）。
- `Term::as_tensor()` 把嵌套 `list` 项转成 `(shape, Vec<f64>)`。
- `Term::to_latex()`/`to_typst()`/`to_pretty()` 输出排版。
- `sim::scenario::run(name, json)` 运行物理仿真；
  `sim::scenario::SCENARIOS` 是场景名的唯一来源（2026-11-04 测量 29 个）。
- `Session::register_unit(UnitDef)` 把单位注册进该 session 的 registry；
  `Session::manifest()` 用 `KernelManifest::build_with_units` 生成，所以
  session 私有单位会出现在 manifest 里。
- **SDE manifest 面**：`rules::sde` 把三个无回调算子暴露成 manifest 算子
  （两个噪声原语 `sde_wiener`/`sde_levy_areas`，加方案元数据
  `sde_scheme_info(name) -> Tensor<1, Real>`）；`features.sde` 为
  true 时宿主可用 capability `core.sde`。SDE **求解循环**需要 host 回调
  （`&Fn + Sync`），不在 manifest 里（kernel CO-03）；Aevia 的
  `sdeint_diag`/`sdeint_general` 在宿主侧调用 `kernels::sde`。

Aevia 直接复用的部分：

| 能力 | rssn 模块 |
|---|---|
| 精确数（大整数/有理数/浮点） | `graph::Number` |
| 项构造与 hash-consing | `graph::Graph`、`Session::call_value`/`Session::parse` |
| 重写引擎与规则集 | `graph::Engine`、`rules::*` |
| 微积分/代数/方程/ODE/PDE | `rules::calculus`、`solve`、`ode`、`pde` 等 |
| SDE 噪声原语 | `rules::sde`（manifest 算子）、`kernels::sde`（积分器） |
| 量纲推断 | `graph::dim` |
| 单位解析 | `rules::units` |
| 数值内核 | `kernels::*` |
| 线性代数 | `kernels`/`faer`/`rules::linalg` |
| FFT | `kernels::fft` |
| ODE 积分 | `kernels::ode`、`sim::integrators` |
| 仿真 | `sim::scenario` |
| JIT | `backend::jit`、`Term::compile` |
| 排版 | `io::latex`、`io::typst`、`io::pretty` |

Aevia 自己实现的部分：

- 词法/语法/布局/格式化；
- 类型与量纲的**编译期**检查；
- 闭包、记录、变体、字符串、集合、`IO`；
- 张量的宿主表示、形状检查、广播、切片；
- 模块、包、REPL、LSP；
- 标准库的编排与类型化包装；
- `Dyn` 的运行时量纲检查。

## 10.2 标量映射

### 10.2.1 精确数

| Aevia | rssn |
|---|---|
| `Int`（任意精度） | `Number::Int(BigInt)` |
| `Real` 精确有理数 | `Number::Rat(BigRational)` |
| `Float` | `f64` / `Number::Float` |
| `Complex` 数值 | `num_complex::Complex64`（宿主）或 `a + b*i` 项 |
| `Complex` 符号 | `a + b*i` 项 |

- `Session::int(i64)` 只接受 `i64`；超出范围时用
  `Session::parse(&n.to_string())` 构造大整数。
- `Session::rational(i64, i64)` 同理；大有理数用文本解析。
- `Real` 的 `Exact(Number)` 直接映射；`Sym(TermId)` 映射到 rssn 项。
- `Float` 的 `F64` 不进入 rssn；`Sym` 项含浮点字面量时由 rssn 处理。
- `Complex` 的数值路径用 `Complex64`；符号路径构造 `add(re, mul(im, i))`。

### 10.2.2 符号

```rust
let name = format!("{base}#{id}");     // 内部唯一名
let term = session.sym(&name);
```

- 显示名与内部名分开；`symbol("x")` 的内部名是 `x#1`，显示 `x`。
- 静态路径的项图无量纲，**不**给符号传量纲；量纲由 Aevia 类型检查器
  负责。只有 FFI/导出时才用全名（`Length/Time`）传给 `Config::dimension`。
- 同一符号在多次 `compute` 中复用同一个 `TermId`；`Session` 的项图负责
  hash-consing。
- 符号的假设（`positive`、`real`、`integer`）通过 `Config::assume` 传入；
  Aevia 的 `assuming` 把简单 `Prop`（`x > 0`、`x >= 0`、`x != 0`、
  `x ∈ ℤ`）转成 `Facts`。**一般等式/不等式目前没有语言入口**：`Rssn`
  的固定成员里没有 `assume`（§8.19），运行时也没有对应 arm；后端也没有
  `core.assume.general` 能力行。所以 `assuming` 之外的假设不可用，不是
  “传给 RSSN 后返回 E1001”。（把一般假设暴露成 `Rssn.assume` 是待定的
  扩展，需要同时定义 capability 行。）

## 10.3 算子映射：生成的 façade，不是手写绑定

下面表格是**语义映射的示例**，不是 Aevia 仓库里的绑定文件。实际实现由
manifest 生成；新算子不需要改这些表。表格用来解释 Aevia 的语法糖如何
落到 RSSN 的结构算子/请求上。

### 10.3.1 核心算术

| Aevia | rssn 算子 |
|---|---|
| `a + b` | `add(a, b)` |
| `a - b` | `add(a, mul(-1, b))`（rssn 的减法糖） |
| `a * b` | `mul(a, b)` |
| `a / b` | `mul(a, pow(b, -1))` |
| `a ^ n` | `pow(a, n)` |
| `-a` | `mul(-1, a)` |
| `abs(a)` | `abs(a)` |
| `sqrt(a)` | `sqrt(a)` |
| `sin(a)` | `sin(a)` |
| `exp(a)` | `exp(a)` |
| `ln(a)` | `ln(a)` |
| `atan2(y,x)` | `atan2(y, x)` |

`Session::call_value(name, args)` 解析重载、校验参数，再构造项并只做该算子
自身的局部重写（`add` 就是一次 n 元展平），不跑全局重写循环；整段表达式
用 `Session::parse`。`Kernel::call(&OpKey, …)` 转发 `call_key`，**钉死**
`OpKey` 指向的重载，不再只按 name 分发；`Kernel::call_ho` 同样按 key 找
注册。Aevia 的 `+` 在 `Real`/`Float` 符号路径上就是一次
`call_value("add", ...)`。

### 10.3.2 重型请求

| Aevia | rssn 请求 |
|---|---|
| `diff(f)` | `diff(term, ξ)` |
| `antiderivative(f)` | `integral(term, ξ)` |
| `defint(f,a,b)` | `defint(term, ξ, a, b)` |
| `limit(f,a)` | `limit(term, ξ, a)` |
| `series(f,a,n)` | `series(term, ξ, a, n)` |
| `sum(f,a,b)` | `sum(term, κ, a, b)` |
| `product(f,a,b)` | `product(term, κ, a, b)` |
| `solve(f)` | `solve(term, ξ)` |
| `solve_complex(f)` | `solve_complex(term, ξ)` |
| `nsolve(f,x0)` | `nsolve(term, ξ, x0)` |
| `dsolve(eq,y,t)` | `dsolve(eq, y(t))` |
| `simplify(x)` | `compute(term, Config::symbolic())` |
| `approx(x)` | `compute(term, Config::numeric(tol))` |

这些请求用 `Target::Symbolic` 或 `Target::Numeric`；结果从 `Answer` 取。

### 10.3.3 数值内核

| Aevia | rssn |
|---|---|
| `defint`（数值） | `kernels::quadrature` |
| `odeint` | `kernels::ode` / `sim::integrators` |
| `sdeint_diag`/`sdeint_general` | `kernels::sde`（Aevia 侧包装；噪声原语 `sde_wiener`/`sde_levy_areas` 与方案元数据 `sde_scheme_info` 是 manifest 算子） |
| `fft` | `kernels::fft`（rustfft） |
| `eig`/`svd`/`qr`/`lu` | `kernels`/`faer` |
| `interp1`/`spline` | `kernels` |
| `minimize` | `kernels::optimize` |
| 特殊函数 | `kernels::special` |

数值内核直接吃 `f64`/`ndarray`，不经过项图。Aevia 的 `Float` 张量路径优先
走这里。

### 10.3.4 线性代数

- `Float`/`Complex` 矩阵：用 rssn 的数值内核（faer）。
- `Real` 精确/符号矩阵：小矩阵用 `rules::linalg`；大矩阵若无法数值化，
  返回 `Err` 或保持符号。
- `det` 的无量纲限制在 Aevia 层；rssn 的 `det` 仍可算带量纲矩阵，但
  Aevia 不暴露那个重载。
- `linsolve(A,b)` 的量纲 `e/d` 由 Aevia 静态检查；rssn 的答案量纲用于校验。

### 10.3.5 未知/新算子

任何不在生成 façade 里的算子走动态调用：

```aevia
let r = Rssn.dynamic("foo", [to_value(x), to_value(y)])
match r with
| Ok(v) -> ...
| Err(e) -> ...
```

- `Rssn.dynamic` 的参数/返回值是 `Value`；RSSN 会按签名检查参数形态
  （不匹配报 `OpError::Type`），完整类型/量纲仍由 Aevia 侧负责；
- 新算子注册到 RSSN 后，重新生成 `Rssn` 模块即可获得类型化声明；
- 插件/用户算子不需要 Aevia 编译器改动；
- 动态调用失败返回 `OpError`，Aevia 转成 `E1003`/`E1001`。

### 10.3.6 单位

**Aevia 的项图不带单位。** 单位字面量在编译期折叠为相干 SI 数值；`Real`/
`Float` 值在宿主侧带一个可选的显示单位。rssn 看到的只是无量纲数值和符号
表达式，例如 `3 m + x` 在项图里是 `3 + x`（`x` 是无量纲符号），量纲由
Aevia 的静态类型保证。

理由：当前 rssn 的 `quantity` 没有数值 eval，且 `dimension_from_text` 会把
`L` 解析成升、`T` 解析成特斯拉；把单位放进静态项图会同时破坏数值路径和
量纲路径。RSSN 仍应提供 `Value::Quantity` 的 eval 供 FFI/显示使用，但
Aevia 的静态路径不依赖它。单位只在两个边界出现：

1. **显示/排版**：`show`/`to_latex` 在宿主侧把数值和显示单位拼成文本；
   需要 rssn 排版时，用 `quantity(v, "<相干 SI 单位>")` 包一层，仅用于输出。
2. **导出/FFI**：写 CSV/NPY/JSON 时在宿主侧附单位元数据，不进入项图。

`Point` 在进入 rssn 前转成 K 的 `Float`；`Dyn<ρ>` 的运行时量纲是
`rssn::dim::DimTerm`。启动时做单位表一致性检查（§5.10），但只用于显示/
导出；`Units.*` 从 `manifest.units` 生成，不维护第二张表。

### 10.3.7 仿真

```rust
let json = rssn::sim::scenario::run("ising", params)?;
```

Aevia 的 `simulate(name, json)` 直接调用；场景名的唯一来源是 kernel 的
`sim::scenario::SCENARIOS`（本次测量 29 个，含 `ising_wolff`），宿主不手写
第二份清单。`Physics.*` 的类型化包装器是**Aevia 源模块**（§8.17），内部调用
`simulate` 并把记录与 JSON 互相转换；kernel 目前的场景参数仍是 JSON，
`ScenarioSchema`（DESIGN.md §9 的设计）尚未实现，所以包装器不是从“场景
manifest”自动生成的——新增场景需要新增一个包装模块。`ising` 返回扁平的
`Vec<i8>`，包装器按 `width`/`height` 行主序 reshape 成 `Tensor<2, Int>`；
`sim` 的错误是字符串，包装器按前缀映射成 `SimError` 枚举。

### 10.3.8 领域模块：`Rssn_<Domain>`

平铺的 `Rssn.<op>` 与按 rule set 分组的 `Rssn_<rule_set>` 都沿用 **kernel 的**组织方式；
它们说不出“`invmod` 取一个模数、返回一个剩余”：`invmod` 的 kernel 签名是派生的
`Real -> Real`，而 `discrete` 这个 rule set 同时装着有限域、图、编码和密码学。
领域模块是 Aevia 一侧的分组，由 **manifest + 一份小的声明式 overlay**
（`crates/aevia-rssn/domains.overlay`）生成：

* `domain`/`family`：算子到领域（`NumberTheory`、`Combinatorics`、`Group`、
  `FiniteField`、`Graph`、`Coding`、`Crypto`、`Topology` 等 29 个）的归属，
  按 `rule_set:glob` 匹配，先匹配者胜。**每个 rule set 必须被某个 family 认领**，
  kernel 新增 rule set 或算子而 overlay 没有归属时覆盖检查失败。
* 四档暴露（witnessed 见下一条）：**typed**（`sig`，Aevia 类型写成 `Result<T, OpError>`，包装器由签名
  推导）、**dynamic**（自动生成的 `Result<Value, OpError>` 包装器，新算子零编辑即可
  调用）、**excluded**（`exclude`，必须写理由）。`mode D typed` 的领域里，
  既无 `sig`、`dynamic` 也无 `exclude` 的算子是覆盖错误；`sig` 的参数个数与 manifest
  的 arity 不符、`sig` 指向已不存在的算子同样失败。
* **witnessed 档（自动类型化）**：介于 typed 与 dynamic 之间。kernel 的引擎每次运行结束时，
  请求（如 `factorint(360)`）与答案（`list(list(2, 3), ...)`）同处一个等价类，这本身就是一条
  类型判断。kernel 把这些判断记录为"sort 事实"、按参数种类聚类取并、再逐条**重放**见证调用，
  得到 `signatures.witnessed.json`（rssn `docs/KERNEL.md` §3a）；manifest 里每个重载带
  `overload_sources`，见证调用在 `witnesses` 中。领域模块对**没有** `sig`、但有可用 Aevia
  类型表达的见证重载的算子，生成与 typed 档同形的包装器
  （`fn divisors(n: Int): Result<Tensor<1, Int>, OpError>`），文档里引用见证调用。
  参数名由类型推出（`n` 整数、`x` 标量、`xs` 向量、`m` 矩阵、`t3` 三阶张量、`flag` 布尔、`v` 句柄；同类重复时编号 `x1`、`x2`）。
  类型映射：`Int`→`Int`，有理数/符号表达式/符号→`Real`，浮点→`Float`，`Bool`→`Bool`，
  矩形列表→`Tensor<秩, 元素>`；元组、不透明对象、参差列表没有通用 Aevia 类型，
  该重载留在 dynamic 档。一个算子有多个见证重载时，证据最多的占用算子名，其余以
  `_int`/`_real`/`_float` 后缀区分。**没有人手写这些签名**：corpus 就是 kernel 自己的测试集，
  kernel 新增并测试一个算子后，重新挖掘即在 Aevia 中自动获得类型。
  见证签名是**证据而非全称证明**：参数侧可能比算子实际接受的更窄（`Rssn.dynamic` 始终可用），
  结果由 `from_value<T>` 校验，越出观测范围时得到 `Err`，绝不会得到类型错误的值。
  `dynamic op | 理由` 仍可把某个算子强制留在 dynamic 档。
* 转换是**泛型**的，不是逐函数写的：参数 `to_value(x)`，结果 `from_value<T>(v)`。
  丰富类型用 `codec Type to_fn from_fn` 声明一对转换函数（每个类型一对，不是每个
  算子一对）：`Perm`（0 起始，边界上 ±1，kernel 的排列是 1 起始）、`Graph`/`Group`
  （包装不透明 `Value` 句柄的名义类型）、`PermGroup`。
* 错误通道：kernel 失败是 `Err(OpError)`；图后端算子在参数越出定义域时返回**未约化的
  调用项**（`invmod(6, 15)`），`from_value` 把它报告为带该项文本的 `TypeMismatch`，
  不再是一个笼统的“does not prove”。
* **诊断通道：为什么没有约化。** 数值算子拒绝参数（形状不符、轴越界、系统奇异、Neumann 数据不相容……）时，kernel 只留下未约化
  的调用项。`Session::explain(term)`（`rssn::kernel::{Diag, DiagKind}`）在**调用线程**上重放该算子的求值器并给出
  `Diag { op, arg, kind, message }`。桥（`Kernel::call_with_witnesses`、`call_key`、`call_key_with_witnesses`）在结果恰好是
  该算子自己的未约化调用、且 `kind` 不是 `Symbolic`（符号参数下保持符号项仍是正确答案）时，不再把项交给 `from_value`，而是返回
  `KernelError::from_diag`：代码为稳定的 `rssn.diag.<kind>`（`arity`、`shape`、`domain`、`singular`、`incompatible`、
  `not_converged`、`unsupported`、`symbolic`、`unspecified`），`diag` 字段保留结构化的 `op`/`arg`/`kind`/`message`。
  `Rssn.dynamic` 在 `Err` 里把它写成用户形式——**函数名、参数名、位置**：
  ``Err(rssn.diag.domain: tensordot_2_2_1: argument `axes_a` (#3): an axis is past the rank of the first tensor (rank 2): [2])``。
  函数名取自调用栈最内层的帧（生成的包装器），参数名取自 `domains.overlay` 的 `sig`（`domains::wrapper_params`）；
  witnessed/dynamic 包装器没有手写参数名，只给位置（`argument #3`）；不是包装器的调用者直接用算子名。
  `OpError` 仍是不透明类型：`"{e}"` 给出 `代码: 消息`，既有的 `match` 与 `Err(_)` 不受影响，没有新增构造子，
  也没有新增 `Rssn.*` 成员（代码是 `"{e}"` 的前缀）。在**抛出**位置（`Rssn.<op>(..)` 的类型化调用）同一发现是
  `E0807`，附注给出稳定代码。只有 `rules::numeric::TABLES` 的算子记录原因；记不出原因的重型算子保持原样（未约化项）。
* 维度证据：`sig` 同时是 overlay 对“该算子是无量纲参数的纯函数”的声明；另外，
  kernel 自己声明的 `Dimensionless` 规则、以及参数已知无量纲时的
  `List/NonMetric/Compare/Angle` 规则，会让裸数结果携带 `Scalar(1)` 证据。其余规则
  （`Same`、`Sum`、`Custom`…）不被信任。`list(...)` 项结果在上述可信时被读回成张量
  （等长行合并成高阶张量，不等长保留为张量的张量）。
* **构建期还是运行期**：运行期。模块在解析 `import Rssn_NumberTheory` 时由 live
  manifest 渲染（`codegen::namespace_source`），不提交生成的 Aevia 源，因此不会与所链接
  的 kernel 漂移。提交的只有 overlay 与覆盖报告
  `tools/rssn-api-audit/RSSN-COVERAGE.md`；测试 `cargo test -p aevia-rssn --test
  domains` 重新生成报告并比对（`AEVIA_BLESS=1` 重写）。领域模块的源码可由 `aevia_rssn::domains::domain_files` 导出审阅
  （CLI 的 `--emit` 目前只导出规则集命名空间）。
* 示例：`tests/domain_number_theory.aev`、`domain_combinatorics.aev`、
  `domain_finite_fields.aev`；`Zmod` 把剩余与模数放在同一个值里，`zmod_inv(x)` 对非单位
  返回 `Err`，混用不同模数返回 `Err`。
* **Numeric 域的数值算子组（rssn `numeric` 规则集）。** 稳健统计（`mad`、`trimmed_mean`、
  `huber_location`、`kde`、`quantile_hf`、`bootstrap_ci`…）、窗与谱（`hann_window`、`fft_re`/`fft_im`/`fft_abs`、
  `ifft_re`/`ifft_im`、`frft_re`/`frft_im`、`cross_correlation`）、小波（`dwt_approx`、`idwt`、`wavedec`、`waverec`、
  `dwt2`、`idwt2`）、Krylov/稀疏/低秩/张量/函数（`minres_solve`、`fgmres_solve`、`bicgstab_l_solve`、`sparse_solve`、
  `amg_solve`、`randomized_singular_values`、`lobpcg_eigenvalues`、`tensordot`、`hosvd`、`tt_svd`、`polynomial_system_roots`…）、
  维度无关的有限差分求解器（`heat_fd`、`wave_fd`、`poisson_fd`、`advect_fd`）、复变特殊函数
  （`lgamma_complex`、`gamma_complex`、`digamma_complex`、`lambert_w_complex`、`bessel_j_complex`、`bessel_i_complex`）
  与表达式算子（`oscillatory_integral`、`anderson_fixed_point`、`newton_cg_minimize`、`ode_solve`、`ode_symplectic`、
  `ode_imex`、`ode_etdrk4`）都归入 `Rssn_Numeric`：75 个算子全部有 `sig`，没有一个留在 dynamic 档
  （秩多态张量与分解按秩给出多条签名，见下）。
  其中的规则：
  - 这些算子的结果是浮点，签名用 `Float`/`Tensor<n, Float>`，**不是** `Real`（`from_value<Real>` 只接受精确的
    `Int`/`Rat`）；整数性质的结果（秩、置换下标）同样以 `Float` 返回。
  - 用符号命名的方法、小波、统计量参数类型是 `Real`（Aevia 的 `symbol(..)` 是 `Real`），表达式参数是 `Real` 项的张量。
    每次 `symbol("db2")` 调用产生新符号（`db2#2`…），所以每个名字只 `let` 一次再复用。
  - 维度无关的名字：`heat_fd`/`wave_fd`/`poisson_fd`/`advect_fd` 的空间维数是场数据的嵌套深度，符号 PDE 是
    `solve_heat_equation`/`solve_wave_equation`/`solve_laplace_equation`/`solve_poisson_equation`，仿射变换是
    `translation`/`scaling`/`shear`/`reflection`/`rotation`（`Rssn_Graphics` 的 family 认领了不带后缀的名字；`_2d`/`_3d` 是别名）。
    签名只有一个字面量秩，所以一个内核算子可以有**多条 `sig`，各用自己的 Aevia 名字**（overlay 写作
    `sig heat_fd as heat_fd_2d(..)`；`Overlay.sigs` 按算子存一个列表，同名重复是错误，只有一条 `sig` 的 overlay 不受影响）。
    有限差分：不带后缀的名字保持一维含义，`heat_fd_2d`/`heat_fd_3d`、`wave_fd_2d`/`wave_fd_3d`、`poisson_fd_2d`/
    `poisson_fd_3d`、`advect_fd_2d`/`advect_fd_3d` 取 `Tensor<2, Float>`/`Tensor<3, Float>` 场并返回同形状的场
    （平流速度是 `Tensor<1, Float>`，每个空间维一个分量，长度不符返回 `Err`）。
  - 秩多态张量算子按秩命名为 `<算子>_<秩>`：`tensor_norm_1..4`、`tensor_inner_1..4`（两个同秩操作数），
    `tensordot_<ra>_<rb>_<k>`（秩 ra、rb 的操作数收缩 k 个轴，`axes_a`/`axes_b` 是 `Tensor<1, Int>`、从 0 起，结果秩为
    ra + rb − 2k，为 0 时是 `Float`；ra、rb ∈ 1..4，1 ≤ k ≤ min(ra, rb)，结果秩至多 4，共 27 个）。
    分解 `hosvd_<r>`、`hooi_<r>`、`cp_als_<r>`、`tt_svd_<r>`、`tt_dense_<r>`（r = 2、3、4）返回 `Numeric` 前言里声明的记录：
    `Hosvd<r>{core, factors, singular_values}`、`Hooi<r>{core, factors, error}`、`CpFit{weights, factors, residual}`、
    `TensorTrain{cores, error}`；`factors` 是 `Tensor<1, Tensor<2, Float>>`，`cores` 是 `Tensor<1, Tensor<3, Float>>`。
  - **混合列表的解码机制。** 内核的分解结果是类型各异的部分组成的列表（张量、张量的列表、一个数）。桥把这种
    结果保留为带逐元素量纲证据的一维值（`kernel.rs` 的 `lift_list` 不再要求每一项都是张量），`from_value<Tensor<1, Value>>`
    是通用的“列表访问器”：把第一个轴的各部分逐个作为 `Value` 句柄交出（均匀张量按行切片，参差列表取各自的证据），
    再对每一部分 `from_value<T>`。解码器是前言里的 Aevia 源码（`hosvd3_from_value` 等），用 `codec Hosvd3 - hosvd3_from_value`
    声明为只解码的类型（`-` 表示没有编码函数，把这样的类型当参数是覆盖检查的错误）。
  - `sig` 即“该算子是无量纲参数的纯函数”的声明，所以 typed 的数值结果带 `Scalar(1)` 证据可以解码；
    没有 `sig` 的算子，其规则是 `Same`/`Product`/`Any` 时结果不被信任（见上“维度证据”）。
  端到端测试：`tests/domain_numeric.aev`（封闭形式核对，见 §8.19.1）。

**语言缺口（尚未实现）**：`let x: Zmod<17> = 5; x * x ^ -1` 需要三项语言改动，均在
`aevia-types`/`aevia-core` 中，不属于绑定层：(1) 变体类型接受 `Nat` 参数（目前
`E0317`）；(2) `Num`/`Fractional` 类带 `add/sub/mul/neg/from_int/inv` 方法，并让二元
运算符对用户名义类型分派到实例；(3) 整数字面量按期望类型经 `from_int` 提升。在此之前
`Zmod` 的模数是运行期携带的，运算以函数形式给出（`zmod_mul(x, inv)`）。

## 10.4 量纲桥

量纲/单位的唯一事实来源是 RSSN 的 `rssn::dim`（`DimTerm`/`DimArena`/
`UnitRegistry`）。Aevia 的 `Dim` 是它的类型级镜像；`DimValue` 在运行时就是
`rssn::dim::DimTerm`，不是字符串。桥接细节见根目录文档 §3.5。

### 10.4.1 静态到运行时

**静态路径不把量纲传给 rssn。** Aevia 的类型检查器已经保证量纲正确；
项图里的符号是无量纲的，rssn 的 `DimensionMode` 设为 `Off`（或
`Advisory` 仅用于诊断）。这避免了文本歧义和 `quantity` 无 eval 的问题。

只有以下场景需要把量纲交给 rssn：

- `Dyn<ρ>` 的运行时检查：宿主保存 `rssn::dim::DimTerm`，不依赖字符串；
- FFI/导出：优先用 `Config::dimension_term(symbol, DimExpr)`（不解析文本，
  未知基名会被 intern）。只有纯文本入口
  `Config::dimension(symbol, text)` 才必须传**全名**（`Length`、`Mass`、
  `Time`、`Current`、`Temperature`、`Amount`、`Luminosity`），**绝不使用
  单字母**（`L` 是升，`T` 是特斯拉，`N` 是牛顿，`J` 是焦耳）。静态路径
  不传量纲，所以这不影响正常程序。
- 用户显式请求 rssn 的量纲诊断：`Config::dimensions(Advisory)`。

自定义基量纲用 `Config::base_dimension(name)` 声明。它只是把名字放进
`Config`；`prepare` 在**克隆出来的**图上调用 `define_base_dimension`。
`Graph::clone` 共享 `Arc<RwLock<DimArena>>`，写入时 `with_dim_arena_mut`
看到 `strong_count > 1` 就 detach（写时复制），所以请求期注册不会漏进
模板或并发请求，不定义基量纲的请求也不付拷贝成本。

### 10.4.2 运行时到静态

数值答案的量纲以 `Answer.dimension` 为准；**不要**从 `Answer.term` 重新
推断（Numeric 目标的 term 是无量纲浮点字面量）。`Answer.dimension` 是
`Option`：`DimensionMode::Off` 时为 `None`，此时回退到静态量纲；要得到
运行时量纲，数值路径必须用 `Advisory` 或 `Strict`。对静态路径，答案量纲
已知，`Answer.dimension` 只用于可选的一致性检查；不一致是内部错误
`E0809`，不是用户错误。

### 10.4.3 量纲模式

| 场景 | 模式 |
|---|---|
| 普通静态检查通过的代码 | `Off`（项图无量纲） |
| `Dyn`、FFI、运行时构造的项 | `Strict`，用全名 |
| 用户显式请求诊断 | `Advisory` |

Aevia 不暴露 `bypass` 给普通用户；`Dyn<ρ>` 是显式逃逸。

## 10.5 线程与生命周期

### 10.5.1 Session 不是 `Sync`

`Session` 内部是 `RefCell`，`Term<'s>` 借用 `Session`。Aevia 的桥把内核会话
放在**线程本地**（`aevia-rssn` 的 `kernel()`）：

```rust
thread_local! {
    static KERNEL: RefCell<Option<Rc<Kernel>>> = const { RefCell::new(None) };
}

pub fn kernel() -> Rc<Kernel>;   // 当前线程的会话，首次使用时创建
```

- 每个线程一个内核会话；`par_map` 的“工作线程”因此会各自创建会话（当前
  `par_map` 顺序执行，见 §9.7.2，所以实际只用一个会话）。
- 符号值跨线程传递时，kernel `TermRef` 带 `SessionId`，别的会话的 handle 会被
  拒绝（`OpError::ForeignTerm`）；当前桥不跨线程共享符号值，也不使用
  `Session::fork`/`merge`——`fork/merge` 只是 kernel 提供的原语。
- `compute_all` 在单会话内并行，但**当前 Aevia 桥没有用它**：批量请求按顺序
  逐个调用；需要并行时应在宿主侧按线程分配会话，而不是假设桥已经这样做。

### 10.5.2 项的生命周期

- `Term<'s>` 只在 `compute` 调用期间借用；返回后转成 `u32` id。
- `Session::term_by_id(id)` 取回；id 在会话内稳定。
- `compute` 的答案项已经导入回会话，id 有效。
- `Session` 的项图只增不减；REPL `:reset` 重建会话。

## 10.6 性能策略

### 10.6.1 不要为每个算子调用 `compute`

`compute` 会克隆模板图并运行重写；对 `x + 1` 这种 O(1) 构造，代价高几个
数量级。Aevia 的规则：

- 算术/初等函数：`Session::call_value` 构造项（只做局部重写），不 `compute`。
- `simplify`/`eval`/`diff`/`integrate`/`solve`：`compute`。
- 打印：默认不化简；用户显式 `simplify` 或用 `:latex`（可配置轻量规范化）。
- 比较/`if`：`Prop` 不求值；需要判定时 `decide` 或 `assuming`。

### 10.6.2 缓存

- `Config` 按规则集/假设/模式缓存；避免每次重建。
- 项按结构哈希缓存；重复子表达式复用。
- 编译函数按项哈希 + 输入名缓存。
- 单位表/量纲别名编译期常量。

### 10.6.3 批处理

- `compute_all` 可以在一个会话内并行准备/运行/导入多个独立请求；**当前
  Aevia 桥没有使用它**，批量请求按顺序逐个调 `compute`。这是已记录的
  性能缺口，不是语义差异。
- 张量逐元素符号运算不要用 `compute_all`（每个元素一个请求，开销大）；
  用 `map` 构造项，最后一次性 `simplify`。
- 数值张量用 `F64` 路径，不进项图。

### 10.6.4 JIT

- `Term::compile` 把闭式 `Float` 表达式编译成函数；Aevia 的热点函数走
  这条路径。
- `TieredBackend` 先解释、后台编译；适合 REPL 和交互式。
- JIT 失败回退解释器；不改变语义。

### 10.6.5 设备：数据并行执行 / Devices: data-parallel execution

**English summary.** rssn has two orthogonal execution axes. `Backend` turns
one closed form into a scalar function (interpreter, tape, Cranelift). A
`Device` (`rssn::backend::device`) runs *one program over many rows* on a CPU
thread pool or a GPU. Aevia maps its `Device` value (`Cpu | Gpu(Int) |
Distributed(Cluster)`, §8.18) to rssn devices; transfers stay explicit.

**两条互相独立的轴。**

| 轴 | rssn 抽象 | 回答的问题 |
|---|---|---|
| 一个项怎样变成函数 | `Backend` / `Compiled` | 在一个点上求值 |
| 许多行在哪里跑 | `Device` / `Buffer` / `KernelIr` | 在 10⁶ 行上求值；对 10⁴ 个初值积分 |

JIT（§9.6）属于第一条轴，**不是** GPU 路径；`@jit(target = Gpu(n))`
选择的是第二条轴的设备。

**能力发现。** 两层，都是事实而不是猜测：

1. 构建层：manifest `features.devices`（字符串列表：`cpu`、`cpu-parallel`，
   以及启用 rssn `gpu-wgpu` feature 时的 `gpu-wgpu`）。这是编译期事实，
   不探测硬件，所以可以进 `manifest_hash`。`core.gpu` 能力 =
   `features.devices` 含任一 `gpu-*`。
2. 运行层：`rssn::backend::device::registry().list()` 返回 `DeviceInfo`
   （`id`、`kind`、`dtypes`、`ops`、`parallelism`、`unified_memory`、
   `deterministic`）。没有适配器不是错误，只是没有 GPU 设备。Aevia 侧封装在
   `aevia_rssn::device`（`available()`、`has_gpu()`、`resolve(Selector)`）。

**`Device` 值到 rssn 设备的映射。**

| Aevia | rssn id | 说明 |
|---|---|---|
| `Cpu` | `cpu` | 单线程；参考语义 |
| `Cpu`（`Device.cpu_parallel` 提示） | `cpu-parallel` | rayon；与 `cpu` **逐位相同** |
| `Gpu(n)` | 注册表中第 `n` 个 `DeviceKind::Gpu`（`gpu:wgpu:n`） | 无则 `E1001("core.gpu")`，消息带已注册 GPU 数 |
| `Distributed(c)` | 无 | 仍为 `E1001("core.distributed")`；不在本设计内 |

`Device.cpu_parallel` 是设计中的新值（`type Device` 增加一个无参构造器
`CpuParallel`）；在类型表实现前，它由 `Selector::CpuParallel` 表示。

**传输是显式的。** `to_device(t, Gpu(n))` = 一次 `upload`（`F64` 张量在只有
`f32` 的设备上转为 `f32`，**这会改变数值**；见下）；`device_of` 读张量的设备
标签；结果回到宿主要 `download`，不存在隐式传输。`on(device, thunk)` 是作用域
执行：`thunk` 内的批量原语（`Device.map`）用该设备，作用域结束时释放缓冲区。

**精度合约。** 内核 IR 的语义是 `f64`（与 tape/解释器相同）。设备在
`DeviceInfo.dtypes` 里声明自己有没有 `F64`：

- `cpu`、`cpu-parallel`：`F64`，与解释器逐位相同（含 NaN、`select`、越界
  `gather` 读 NaN）。这是 §9.7.2 "线程数不改变任何结果" 在设备上的延伸。
- `gpu:*`（WGSL）：`F32`，以及**双单精度（double-single）的 `F64`**。WGSL 没有 `f64`，
  rssn 的 wgpu 设备把 `Dtype::F64` 做成一对 `f32 (hi, lo)`（`hi + lo` 是值，缓冲区里 8 字节，
  着色器里 `array<vec2<f32>>`），主机在上传时拆分、下载时合并；`DeviceInfo::emulated_f64` 置位，
  `dtype_epsilon` 对它返回 `2^-44`。精度约 48 位：在 RTX 3060 (Vulkan) 上 `+ - * /` 相对误差 `4e-15`，
  `sqrt` `1e-14`，`exp` `7e-14`（`F32` 是 `1e-7`），设备测试对每个函数要求 `5e-12`。
  限制：指数范围仍是 `f32` 的（数据请落在 `1e-30 .. 1e30`），`sin`/`cos`/`tan` 在 `|x| > 1e6` 返回 NaN，
  `pow`/`exp` 按指数大小放大相对误差；`F32` 与 `F64` 缓冲区不能混在一次启动里（`DeviceError::Mismatch`）。
  所以它**不是** IEEE 双精度，也不与 CPU 逐位相同；GLSL/OpenCL/CUDA 发射器产出原生 `f64` 源码，但没有设备执行它们。
  `map_columns`（Aevia 的 `BatchProgram::run`）取设备偏好的 dtype，即 GPU 上走 `F64`。
  Aevia 仍**不会**把 `Float` 的结果悄悄换成 `f32`：`Device.map` 在 `Gpu` 上返回的 `Float` 带上述误差，
  文档与错误信息标明；要求位精确的代码选 `Cpu`/`CpuParallel`。

**启动融合（rssn `device::plan`）。** 一串逐元素的内核不必各自启动、各自写回中间列：
`Plan` 是内核图（输入是外部列或更早内核的输出），`plan.fuse()` 把生产者并入读它输出的消费者
（用 `Input` 引用），中间列变成寄存器，不再落到显存；寄存器、输入列与参数重新编号
（消费者的参数跟在生产者之后，重复的外部列共享），仍被别处需要的输出保留为合并后内核的输出且不重算。
消费者按地址读取该列（`Load`、`Len`、`Gather`）或两个内核都有折叠（fold）时，该边保持为一次启动；
合并后校验不通过则撤销。融合改变的是值存放的位置，不是怎么算，所以在确定性设备上融合与未融合的计划
逐位相同（`plan.rs` 测试）。这是显式 API（`Plan::new`、`add`、`result`、`fuse`、`run`），
`array` 里的驱动仍手工构造启动；Aevia 侧尚未暴露。

**可降低到设备的函数。** 与 §9.6.1 的 JIT 门控一致的纯子集：无捕获、`Float`
标量算术、初等函数、比较、`if`（两侧纯，急切求值后 `select`）。含宿主调用的项
（算子的 `EvalFn`、C 外部函数，例如 gamma/erf/Bessel）在 rssn 里是 `Call`/
`Extern`，**不能**降到设备：`KernelIr::from_tape` 返回 `Unsupported`，
`DeviceBackend` 对这种项回退宿主 tape，Aevia 的 `Device.map` 回退解释器逐行
求值。回退不改变值（CPU 设备上）。

**批量原语（设计）。**

```
Device.available : () -> IO (List DeviceInfo)       # 注册表快照
Device.map       : (Float -> Float, Tensor<[n],Float>) -> Tensor<[n],Float>
                                                     # 在 Cpu/CpuParallel 上；纯，确定
Device.map_on    : (Device, Float -> Float, Tensor<[n],Float>) -> IO (Tensor<[n],Float>)
Device.reduce    : (Reduce, Tensor<[n],Float>) -> Float   # Sum | Prod | Min | Max
Device.ensemble  : (OdeSystem, Tensor<[m,k],Float>, Float, Float, Int) -> Tensor<[m,k],Float>
                                                     # 同一程序、k 个初值；固定步长 RK4
```

纯、位确定的版本只在 CPU 设备上承诺；`map_on(Gpu(n), ..)` 是 `IO`，因为分配与
传输是效应，且结果受设备精度约束。

**实现状态（本节撰写时）。**

- 已实现：rssn 侧完整的设备层（注册表、`Buffer`、`KernelIr`、CPU 参考设备、
  WGSL 发射器与黄金测试、可选 `gpu-wgpu` 设备、`ensemble_rk4`）；manifest
  `features.devices`；Aevia 侧 `aevia_rssn::device`（发现、`Selector`、
  `BatchProgram`）与 `aevia-core` 的 `jit::map_floats`（把纯 `Float` 函数在
  CPU 设备上批量应用，有测试）。
- **未实现**：`Device.map`/`map_on`/`reduce`/`ensemble`/`available` 作为语言内
  原语。它们要在类型表（`aevia-types`）和内建表（`aevia-core`）各加条目，
  并由 `aevia-stdlib-gen` 重新生成 `Device.aev`；这些文件不在本设计的改动
  范围内。接入点是 `jit::map_floats`（需要 `--features jit`，因为 Core IR 到
  `Inst` 的降低目前在 JIT 模块里；批量执行本身不需要 Cranelift）。
- `to_device`/`on` 对 `Gpu(n)` 仍报 `E1001("core.gpu")`；接入需要张量值携带设备
  缓冲区句柄（目前 `Tensor` 的设备元数据只有标签），这是下一步，不是本节已做。
- rssn 的 `device::array` 已有归约、融合的 map-reduce、`matmul`、CSR 乘、stencil、all-pairs 与
  `ensemble_dopri5`（`DeviceInfo.ops` 如实标明各设备支持什么）；Aevia 的 `Device.reduce`/`matmul` 语言原语仍未接入。

## 10.7 错误映射

| rssn 错误 | Aevia 诊断 |
|---|---|
| `ComputeError::Dimension(e)` | `E0401` 或 `E0412`（`Dyn`） |
| `ComputeError::NotNumeric(s)` | `E0803`（自由符号）或 `E0806`（定义域） |
| `ComputeError::Inconsistent(s)` | `E0809`（后端不一致，内部错误） |
| `ComputeError::Rule(e)` | `E0899`（内部错误） |
| `ComputeError::UnknownOperator(s)` | `E0899` |
| 预算耗尽 | `E0808` |
| `BackendError` | `E0807` 或 `E0899` |

`Answer.diagnostics` 转成 Aevia 警告；`Advisory` 模式下不失败。

## 10.8 已知缺口与风险（适配后）

**下面的数字是测量（2026-11-04，`rssn 3.0.0-rc.1`），live manifest 是唯一
事实来源。** RSSN 已实现 Manifest、动态
`Value`/`call_value`/`call_key`、重载解析、**100% 签名覆盖**（Declared/
Overlay/Derived）、物理常数表、cdylib 插件 ABI、`rssn::dim`、自定义基量纲、
`Config::dimension_term`、`Term` 遍历/替换、`fork/merge`、结构化结果和
JIT lowering。本轮实测：1382 算子 / 32 rule set / 32 精确（7 declared +
25 overlay）/ 1350 derived / 82 单位 / 30 常数（18 精确）/
`manifest_hash = 8d2c3764…`。下面只列**仍然存在**的集成缺口；已关闭的旧
缺口不再重复（构建阻塞、`UnitValue` 元数据、重载只按 arity、
`dimension_term` 缺失、`TypeExpr::Symbol` 缺失、`Solutions.complete`
硬编码、ODE `dimensions` 丢字段、CODATA 常数缺失、自定义基量纲单位注册、
cdylib 未实现、单位表缺 §5.3.3 的七项，均已关闭）。

1. **`Session` 单线程。** `Session` 仍是 `RefCell`；跨线程要用
   `fork/merge`，不是共享会话。`par_map` 对符号值必须 fork/merge。
2. **`compute` 仍克隆图。** `prepare` 仍 `graph.clone()`；`OpTable` 已经是
   `Arc<OpTable>`（clone 共享），`DimArena` 写时复制，所以剩下的是节点表
   的浅拷贝成本。`Session::prepare` 仍未公开。
3. **基量纲必须经 `Config` 声明。** `DimArena` 是写时复制：clone 共享，
   第一次写才 detach，所以 `prepare` 里的 `define_base_dimension` 是隔离
   的；但如果直接改 `Session` 的模板图，就会影响后续所有请求。Aevia 只用
   `Config::base_dimension`，不碰模板。
4. **JIT 默认关闭。** `Select`/`Extern1`/`Extern2`/`Gather` 已 lowering
   （Cranelift 0.137，由 `jit` feature 门控），但默认构建
   `features.jit=false`；`@jit` 必须读 `Features`，否则报
   `E1001("core.jit")`（checker 侧门控）。
5. **`Solutions` 的两个“未报告”语义。** `multiplicities` 为空表示未报告，
   不能填 1。但 kernel 现在**在能证明时填它**：一元有理多项式 + 全部根是
   精确数时，用精确 square-free 分解给重数；`conditions` 只填构造上可声明
   的条件（如 `-b/a` 的 `a != 0`）。`complete` 在 `solve_structured` 里按
   “是否 `list(...)`”区分；`Solutions::new` 默认 `false`，要声明穷尽得显式
   `.complete()`。Aevia 自己构造时不要无条件声明穷尽。
6. **ODE 事件的方法差异已缩小。** `Dopri5`、`Radau5`、`BDF`、
   `Rosenbrock23` 与定步长（`solve_fixed_events`，覆盖 Euler/Heun/RK4）
   都能定位事件；差别只剩“定步长需要 `steps`”和事件定位的精度。
7. **常数是值，不是单位。** `manifest.constants` 有 30 条（18 条精确），
   `Units.Constants` 可从它生成；但它们不在 `UnitRegistry` 里，不能当单位
   参与 `quantity` 解析。`e` 是基本电荷，与 `Math.e` 同名，`open` 两个模块
   时 `e` 必须限定。
8. **100% typed，但只有 32 个精确。** 7 declared + 25 overlay 是精确的；
   1350 个 `Derived` 只区分“数值 `(Real,…)` / 符号 `(Term,…)`”，不区分
   `Tensor`/`Quantity`/`Int`。Aevia 可以给全部 1382 个生成 typed 声明，
   但必须把 `Derived` 标成“保守推断”，否则 `det` 只会是
   `(Term) -> Term`（它是 HEAVY 请求，没有数值重载）。
9. **`assuming` 仍以单元谓词为主。** 一般等式/不等式**没有语言入口**
   （`Rssn` 没有 `assume` 成员，见 §10.2.2）；后端也没有
   `core.assume.general` 能力行。不是“走动态入口后返回 E1001”。
10. **复数表示。** `Value::Complex` 有 `Exact`/`Float`，但 `Number` 仍
    没有复数；符号复数靠 `i` 项，数值复数靠 `Complex64`。两条路径需在
    Aevia 侧统一。
11. **符号指数：桥接可往返，语言仍不支持。** `DimExpr::to_term` 把
    `Length^n` intern 成原子；`DimExpr::try_from_term` 是 term 可表示量纲的
    精确逆，`from_term` 是全函数版（读不回给 `Any`，不静默丢因子）。
    Aevia 的静态类型位置仍只接受有理指数（§4.12.3）。不要把“后端能表示”
    误读成“语言该支持”。
12. **仿真 JSON schema。** `SimResult` 有结构化结果，但场景参数仍是
    JSON；kernel 的 `ScenarioSchema`（DESIGN.md §9 的设计）尚未实现，所以
    Aevia 的 `Physics.*` 是源模块包装器，不能从 manifest 自动生成；新增
    场景需要新增包装模块。
13. **预算/中断。** rssn 的预算在 `compute` 内；Aevia 外层超时/取消是
    协作式的，不能强杀。
14. **版本漂移。** 当前集成为 `api_version=2`、`rssn 3.0.0-rc.1`；Aevia 必须 pin
    `api_version` + `manifest_hash`，并跑 conformance。版本 2 的 manifest 增加 Rust
    源路径元数据，二进制布局与版本 1 不兼容；当前二进制格式没有解码前版本协商，
    宿主须事先知道生产者版本。迁移可使用自描述 JSON 或 CBOR，并重新核验 manifest
    哈希。rc 周期内 manifest 仍可能变化，所以文档数字是测量而不是常量。
15. **`manifest_hash` 是 bincode BLAKE3，有两种取法。**
    `hash_hex()` 是 64 个 hex 字符（本轮测量 `8d2c37640f409249…`），宿主
    应该 pin 它；`hash_u64()` 是前 8 字节按**小端**读出的 u64
    （`4992400f64372c8d`），只用于 ABI/显示。两者不可互换。JSON 只供人读，
    不能对 JSON 文本做哈希/比较。
16. **自定义基量纲要经 `Config` 声明。** `expr_to_dim` 已接受任意基名，
    但 `DimTerm` 只能为 `DimArena` 里已有的基名生成；Aevia 对每个
    `dimension` 发 `Config::base_dimension(name)`，`prepare` 会在克隆图上
    定义它。不要直接改模板图的 arena。
17. **`Value` 的可入项图性按 `manifest.value_kinds` 走。**
    `term_of_value` 对 `Sparse`/`Record`/`Variant`/`Function`/`Opaque` 返回
    `Unsupported`；`Complex` 仅虚部为 0 时可入图；`Unit` 是定义不是数
    （17 个变体中 10 个可入图、7 个不可），要过边界用
    `quantity(value, unit)`。稀疏运算必须走 `eval_dyn` 或 Aevia 本地布局
    内核。生成器不要硬编码这张表。
18. **Aevia 回调与插件桥。** `core.callback` 经同线程作用域句柄调用已检查
    源码闭包，不把 evaluator 的 `Rc` 图传给其他线程；具体数值、`Bool`、
    `String` 及其张量和闭合记录按完整可用的形状、量纲证据递归校验，空结构
    不凭空推断。数值求解器另限 `Float` 标量与向量。句柄在外层调用返回后失效。
    `core.plugin.source` 可用：`@op` 函数被追踪为 kernel 定义、`@rule` 为重写，
    作为进程内插件注册（§13.5.2）；`Rssn.register` 只接受 host `Plugin`。
19. **单位属于 session。** `Session::register_unit` 注册的单位只出现在该
    session 的 manifest（`KernelManifest::build_with_units`）；进程级
    `KernelManifest::build` 给出不同的单位表与 hash。插件注册的单位只属于
    安装该插件的 session，Aevia 必须用 `Session::manifest()`。
20. **SDE：manifest 只暴露噪声原语 + 方案元数据，求解循环保持 Rust-only。**
    本轮 `Features`
    新增 `sde: bool`（true），宿主 capability 名 `core.sde`；
    `sde_wiener`/`sde_levy_areas` 是 overlay 精确签名（`RootOf{2,3}` /
    `Same(1)`），**`sde_scheme_info(name) -> Tensor<1, Real>`** 是本轮新增的
    第三个 SDE manifest 算子（方案元数据：强阶 + Itô/交换/自适应/Lévy 面积
    四个 flag，dim rule `NonMetric`；无回调，所以能进 manifest），typed 声明
    随 manifest 自动出现；它是 **manifest-only 的元数据查询**，不是求解
    路径（求解循环仍 Rust-only），且**只给强阶、没有弱阶**——弱阶在宿主侧
    `sde_order`（返回 `{strong, weak}`）里，两者互补。它覆盖 kernel 的
    8 个方案（6 个定步长/Stratonovich + 2 个自适应行，snake_case 名字）。
    生成源对维度变量的签名现在也是精确的：`sde_wiener` 的声明是
    `(Int, Int, Int, Real[T]) -> Tensor<2, Real[T^(1/2)]>`、
    `sde_levy_areas` 是 `(Int, Real[T], Tensor<1, Real>, Int) ->
    Tensor<1, Real[T]>`（声明内共享同一维度变量映射）；`Dyn<term>` 只为
    manifest 自己开放的类型（`Dyn(rep, any)`、秩变量张量、类型参数、未知行
    Table、Device、变体、opaque、Never）保留，见 §10.0。manifest 仍是唯一
    精确来源。
    求解器需要 `&Fn + Sync` host 闭包，manifest 的 `Value` ABI 传不了，
    所以 kernel 的 CO-03 把求解循环、ensemble 统计、host-callback drift 和
    `FnMut` adapter 留在宿主侧；Aevia 的 `sdeint_*` 不是 `Rssn` 算子。
    **宿主侧已落地**：`aevia-std::sde` 实现七种方法与 fixed/adaptive 求解，
    `aevia-core` 负责 dispatch/记录展平（`sde_runtime` 10/10 通过，
    源码级 `sde_language` 5/5 通过），`core.sde` 在本构建 enabled；
    **源码级策略记录也已验证可用**（`Sde.Fixed`/`Sde.Adaptive` +
    `Sde.Diagonal`/`Sde.General(m)` 通过检查并跑出路径，见 §13.2.2 状态）；
    记录状态 + 一般噪声现在可用（扩散是带噪声轴的同形状记录，
    `GeneralDiffusionOf<s>`，§8.9.1）；非交换噪声与 Stratonovich 自适应也已
    实现，`E1001("sde.general.record"/"sde.noncommutative"/
    "sde.interpretation")` 不再产生。
    记录顺序/一致性见 §10.3.7。
    `manifest_hash` 变化一方面因为 `Features` 多了这个字段，另一方面本轮
    加了 `sde_scheme_info`（hash 从 `8ed92a22…` 变为 `8d2c3764…`）。

### 10.8.1 缓解措施

- **一致性测试**：对每个标准算子，比较 Aevia 静态量纲与 rssn
  `Answer.dimension`；不一致即失败。
- **单位表校验**：启动时逐项比较基单位/派生单位/仿射单位，并且只以
  `manifest.units`（本次测量 82 项）为准。
- **回退路径**：JIT 失败回退解释器；符号求解失败返回 `Err`，不 panic。
- **预算默认值**：2 s / 1e6 重写；用户可调。
- **版本 pin**：pin `api_version` + `hash_hex()`；升级时跑全量 conformance；
  不把文档里的算子/单位计数写进缓存键。
- **桥接层隔离**：所有 rssn 调用集中在 `aevia-rssn`，上层不直接依赖
  `rssn` 类型。

## 10.9 一个端到端的调用链

```aevia
fn fall_time(h: Float[L]): Float[T] =
    sqrt(2 * h / g)
```

1. 类型检查：`h : Float[L]`，`g : Float[L/T^2]`，`2*h/g : Float[T^2]`，
   `sqrt : Float[T]`。
2. 调用 `fall_time(20.0 m)`：`20.0 m` 编译期折叠为 `Float` 值 20.0，带显示单位
   `m`。
3. 函数体：`2 * h` 构造 `mul(2, h)` 项；`/ g` 构造 `mul(..., pow(g,-1))`；
   `sqrt` 构造 `sqrt(...)` 项。这些都是 `Session::call_value`（只做局部
   重写），不 `compute`。
4. 返回 `Float` 值：`g` 是 `Float` 常量，`2*h/g` 折叠为浮点数；`sqrt` 对
   `Float` 永远走 f64，负数得 NaN，不会保持符号。
5. `println` 调用 `show`：直接得到 `2.01962…`，按显示单位 `s` 打印。
6. 如果用户写 `approx(fall_time(20.0 m))`：`compute` 以 `Numeric` 目标运行，
   rssn 返回数值球，Aevia 取中点。

> **规范 vs 当前实现的偏差（记录在附录 C）**：上面的 4–5 是**设计语义**
> ——静态 `Float` 的值在运行时就是 f64。当前实现把十进制**量纲字面量**在
> 编译期折叠为精确有理数（`20.0 m` 的值是精确的 20），即使静态类型是
> `Float[L]`；于是 `sqrt` 看到精确参数、保持符号，`fall_time(20.0 m)` 打印
> 为 `sqrt(800000/196133) s`，`approx(...)` 才给出数值。这是实现偏差，不是
> 本章语义。

这个链条展示了 Aevia 的核心性能策略：**类型/量纲在编译期，算术在项构造，
重型求解才进 e-graph，数值热点走 f64/JIT**。

下一章用完整程序展示这些规则如何组合。
