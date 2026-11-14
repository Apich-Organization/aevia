# 10 — rssn 后端映射

Aevia 不重新实现计算机代数。rssn 提供项表示、重写引擎、量纲层、数值内核、JIT 和仿真；
Aevia 提供语法、类型/量纲检查、闭包、记录、张量形状、模块、IO 和标准库编排。本章定义两者的
契约：哪些能力直接映射，哪些要包一层，哪些是已知缺口。

## 10.0 反射优先：没有手写算子绑定

Aevia **不维护** `sin -> rssn::sin`、`det -> rssn::det` 这样的绑定表。构建时读取 rssn 的
**Kernel Manifest**，生成 `Rssn` 模块（契约见 [13 — 可扩展性](13-extensibility.md) §13.2）：

```aevia
Rssn.sin(0.5)                       # Derived 的数值重载：Real
Rssn.sin(x)                         # Derived 的符号重载：Dyn<term>
Rssn.det(M)                         # Overlay：Tensor<[n,n],Real> -> Real
Rssn.dynamic("myplugin_softplus", args)  # 插件/叶子：没有签名
```

- `signature_source = Declared | Overlay | Derived` → 类型化声明。`Derived` 是“由注册信息推出”的
  保守签名：`eval` → `(Real,…) -> Real`（谓词 `-> Bool`），非叶子另加 `(Term,…) -> Term`；它不区分
  `Tensor`/`Quantity`/`Int`，IDE 标“保守推断”。
- `None`（叶子/无调用）→ `dynamic` 声明。rssn 做**签名级**校验（`check_args`，参数不被任何
  overload 接受时报 `OpError::Type`），完整的类型/量纲检查由静态检查器或 `Rssn.typed` 负责。
- 新算子只要在 rssn 注册，下一次构建就出现，不改 Aevia 编译器。
- 生成源的拼写是精确的：维度参数写成 Aevia 的量纲变量（`Real[T]`、`Real[T^(1/2)]`），变量名在声明内局部
  并**在参数与返回间共享**——`sde_wiener(a3: Real[T]): Tensor<2, Real[T^(1/2)]>` 里的 `T` 与 `T^(1/2)`
  是同一个变量。`Dyn<term>` 只保留在确实无法拼写的位置：manifest 自己声明为 `Dyn(rep, any)` 的签名、
  秩变量张量（`transpose`/`rank`/`dot`/`norm`/`sum`；`Tensor<n, τ>` 是 `E0304`）、类型参数、未知行
  `Table`、`Device`、变体、host opaque、`Never`。
- 生成的 `Units.aev`/`Constants.aev`/`Rssn.aev` 经过类型检查且无诊断：生成器对每条声明只发字面量
  `op` + `key` 的 `Rssn.invoke`，checker 按钉死的 overload 校验这两个字面量并把结果类型定为该 overload 的
  声明结果（每次调用新维度变量）。非字面量 key 保持动态 `Value`，运行时答 `E1002`/`E1003`/`E1005`。
  该验收由 `aevia-rssn` 的 `emit_typecheck` 测试断言。
- 静态重载用 `OpKey { rule_set, name, signature_hash }`；typed 调用经 `Rssn.call_key` 钉死重载；
  dynamic 调用走 `call_value`，多注册时按 `Match` 格选、并列报 `E1002`（§13.4.1）。

内核侧契约见 rssn 仓库的 `docs/KERNEL.md`。

## 10.1 rssn 的能力边界

```rust
let s = Session::new();
let x = s.sym("x");
let answer = s.compute(diff(sin(x) * cos(x), x), &Config::new())?;
```

- `Session` 拥有 hash-consed 项图；`Term<'s>` 是带生命周期的句柄。
- `Config` 指定规则集、目标相位（`Symbolic`/`Numeric`）、绑定、假设、预算、量纲模式、量纲假设、自定义基量纲。
- `compute` 把请求复制到模板图的克隆中，运行 e-graph 重写，抽取答案，再导入回会话。`Answer` 含 `term`、
  `value`、`error`、`reduced`、`dimension`、`diagnostics`、`report`。
- `Term::eval(bindings)` 不重写，直接数值求值；`Term::compile(inputs)` 编译闭式（`jit` feature 用 Cranelift）；
  `Term::as_tensor()` 把嵌套 `list` 项转成 `(shape, Vec<f64>)`；`to_latex()`/`to_typst()`/`to_pretty()` 排版。
- `sim::scenario::run(name, json)` 运行仿真；`sim::scenario::SCENARIOS` 是场景名的唯一来源（当前 29 个）。
- `Session::register_unit(UnitDef)` 注册单位；`Session::manifest()` 用 `KernelManifest::build_with_units`，
  所以 session 私有单位出现在该 session 的 manifest 里。
- SDE：`rules::sde` 把无回调的 `sde_wiener`/`sde_levy_areas` 与方案元数据 `sde_scheme_info(name) ->
  Tensor<1, Real>` 暴露成 manifest 算子（`features.sde`，能力 `core.sde`）。求解循环需要 host 回调
  （`&Fn + Sync`），不在 manifest 里；Aevia 的 `sdeint_diag`/`sdeint_general` 在宿主侧调用 `kernels::sde`。

Aevia 直接复用的部分：

| 能力 | rssn 模块 |
|---|---|
| 精确数（大整数/有理数/浮点） | `graph::Number` |
| 项构造与 hash-consing | `graph::Graph`、`Session::call_value`/`Session::parse` |
| 重写引擎与规则集 | `graph::Engine`、`rules::*` |
| 微积分/代数/方程/ODE/PDE | `rules::calculus`、`solve`、`ode`、`pde` 等 |
| SDE 噪声原语 | `rules::sde`（manifest 算子）、`kernels::sde`（积分器） |
| 量纲推断、单位解析 | `graph::dim`、`rules::units` |
| 数值内核（线性代数、FFT、ODE、优化、特殊函数） | `kernels::*`（`faer`、rustfft）、`sim::integrators` |
| 仿真 | `sim::scenario` |
| JIT | `backend::jit`、`Term::compile` |
| 排版 | `io::latex`、`io::typst`、`io::pretty` |

Aevia 自己实现：词法/语法/布局/格式化；类型与量纲的**编译期**检查；闭包、记录、变体、字符串、集合、`IO`；
张量的宿主表示、形状检查、广播、切片；模块、包、REPL、LSP；标准库编排与类型化包装；`Dyn` 的运行时量纲检查。

## 10.2 标量映射

### 10.2.1 精确数

| Aevia | rssn |
|---|---|
| `Int`（任意精度） | `Number::Int(BigInt)` |
| `Real` 精确有理数 | `Number::Rat(BigRational)` |
| `Float` | `f64` / `Number::Float` |
| `Complex` 数值 | `num_complex::Complex64`（宿主）或 `a + b*i` 项 |
| `Complex` 符号 | `a + b*i` 项 |

- `Session::int`/`rational` 只接受 `i64`；超出范围用 `Session::parse(&n.to_string())`。
- `Real` 的 `Exact(Number)` 直接映射，`Sym(TermId)` 映射到 rssn 项；`Float` 的 `F64` 不进入 rssn。
- `Complex` 的数值路径用 `Complex64`；符号路径构造 `add(re, mul(im, i))`。两条路径在 Aevia 侧统一，
  因为 rssn 的 `Number` 没有复数。

### 10.2.2 符号

```rust
let name = format!("{base}#{id}");     // 内部唯一名
let term = session.sym(&name);
```

- 显示名与内部名分开：`symbol("x")` 的内部名是 `x#1`，显示 `x`；同一符号在多次 `compute` 中复用同一 `TermId`。
- 静态路径的项图无量纲，**不**给符号传量纲，量纲由 Aevia 类型检查器负责；只有 FFI/导出才用全名传给
  `Config::dimension`。
- 符号假设（`positive`、`real`、`integer`）经 `Config::assume` 传入；`assuming` 把单元谓词（`x > 0`、
  `x >= 0`、`x != 0`、`x ∈ ℤ`）转成 `Facts`。**一般等式/不等式没有语言入口**（`Rssn` 没有 `assume` 成员，
  也没有 `core.assume.general` 能力行）。

## 10.3 算子映射：生成的 façade，不是手写绑定

下表是**语义映射的示例**，用来解释 Aevia 的语法糖如何落到 rssn 的结构算子/请求上；实际绑定由 manifest 生成。

### 10.3.1 核心算术

| Aevia | rssn 算子 |
|---|---|
| `a + b` | `add(a, b)` |
| `a - b` | `add(a, mul(-1, b))` |
| `a * b` | `mul(a, b)` |
| `a / b` | `mul(a, pow(b, -1))` |
| `a ^ n` | `pow(a, n)` |
| `-a` | `mul(-1, a)` |
| `abs`/`sqrt`/`sin`/`exp`/`ln`/`atan2` | 同名算子 |

`Session::call_value(name, args)` 解析重载、校验参数，构造项并只做该算子自身的局部重写（`add` 就是一次 n 元展平），
不跑全局重写循环；整段表达式用 `Session::parse`。`Kernel::call(&OpKey, …)` 转发 `call_key`，**钉死**
`OpKey` 指向的重载；`Kernel::call_ho` 同样按 key 找注册。

### 10.3.2 重型请求

| Aevia | rssn 请求 |
|---|---|
| `diff(f)` | `diff(term, ξ)` |
| `antiderivative(f)` | `integral(term, ξ)` |
| `defint(f,a,b)` / `limit(f,a)` / `series(f,a,n)` | `defint` / `limit` / `series` |
| `sum(f,a,b)` / `product(f,a,b)` | `sum(term, κ, a, b)` / `product(term, κ, a, b)` |
| `solve(f)` / `solve_complex(f)` / `nsolve(f,x0)` | `solve` / `solve_complex` / `nsolve` |
| `dsolve(eq,y,t)` | `dsolve(eq, y(t))` |
| `simplify(x)` / `approx(x)` | `compute(term, Config::symbolic())` / `compute(term, Config::numeric(tol))` |

### 10.3.3 数值内核

| Aevia | rssn |
|---|---|
| `defint`（数值） | `kernels::quadrature` |
| `odeint` | `kernels::ode` / `sim::integrators` |
| `sdeint_diag`/`sdeint_general` | `kernels::sde`（Aevia 侧包装） |
| `fft` | `kernels::fft`（rustfft） |
| `eig`/`svd`/`qr`/`lu` | `kernels`/`faer` |
| `interp1`/`spline`、`minimize`、特殊函数 | `kernels::*` |

数值内核直接吃 `f64`/`ndarray`，不经过项图；`Float` 张量路径优先走这里。

### 10.3.4 线性代数

- `Float`/`Complex` 矩阵：rssn 数值内核（faer）；`Real` 精确/符号小矩阵：`rules::linalg`；大矩阵无法数值化时
  返回 `Err` 或保持符号。
- `det` 的无量纲限制在 Aevia 层（rssn 的 `det` 仍可算带量纲矩阵，Aevia 不暴露那个重载）；`linsolve(A,b)` 的量纲
  `e/d` 由 Aevia 静态检查，rssn 的答案量纲用于校验。

### 10.3.5 未知/新算子

任何不在生成 façade 里的算子走动态调用：

```aevia
let r = Rssn.dynamic("foo", [to_value(x), to_value(y)])
match r with
| Ok(v) -> ...
| Err(e) -> ...
```

参数/返回值是 `Value`；rssn 按签名检查参数形态，完整的类型/量纲检查仍在 Aevia 侧。新算子注册后重新生成 `Rssn`
模块即可获得类型化声明；插件/用户算子不需要编译器改动；失败返回 `OpError`，Aevia 转成 `E1003`/`E1001`。

### 10.3.6 单位

**Aevia 的项图不带单位。** 单位字面量在编译期折叠为相干 SI 数值；`Real`/`Float` 值在宿主侧带可选的显示单位。
rssn 看到的是无量纲数值和符号表达式（`3 m + x` 在项图里是 `3 + x`），量纲由静态类型保证。

理由：rssn 的 `quantity` 没有数值 eval，且 `dimension_from_text` 把 `L` 解析成升、`T` 解析成特斯拉；把单位放进
静态项图会同时破坏数值路径和量纲路径（更多取舍见 §12.2）。单位只在两个边界出现：

1. **显示/排版**：`show`/`to_latex` 在宿主侧拼接数值与显示单位；需要 rssn 排版时用 `quantity(v, "<相干 SI 单位>")` 包一层，仅用于输出。
2. **导出/FFI**：写 CSV/NPY/JSON 时在宿主侧附单位元数据。

`Point` 进入 rssn 前转成 K 的 `Float`；`Dyn<ρ>` 的运行时量纲是 `rssn::dim::DimTerm`。`Units.*` 从
`manifest.units` 生成，不维护第二张表；启动时的单位表一致性检查（§5.10）只用于显示/导出。

### 10.3.7 仿真

```rust
let json = rssn::sim::scenario::run("ising", params)?;
```

`simulate(name, json)` 直接调用；场景名唯一来源是 kernel 的 `SCENARIOS`，宿主不手写第二份清单。类型化包装器
`Physics.*`（§8.17）是 Aevia 源模块，内部调用 `simulate` 并在记录与 JSON 间转换；`Rssn_Sim_<Name>` 模块由
rssn 的场景目录（`rssn::sim::schema`）在导入时生成（§8.17）。`ising` 返回扁平 `Vec<i8>`，包装器按
`width`/`height` 行主序 reshape 成 `Tensor<2, Int>`；`sim` 的错误是字符串，包装器按前缀映射成 `SimError`。

### 10.3.8 领域模块：`Rssn_<Domain>`

平铺的 `Rssn.<op>` 与按 rule set 分组的 `Rssn_<rule_set>` 沿用 kernel 的组织方式，说不出“`invmod` 取一个模数、返回一个
剩余”。领域模块是 Aevia 一侧的分组，由 **manifest + 一份声明式 overlay**（`crates/aevia-rssn/domains.overlay`）生成：

- **归属。** `domain`/`family` 把算子按 `rule_set:glob` 归到 29 个领域（`NumberTheory`、`Combinatorics`、`Group`、
  `FiniteField`、`Graph`、`Coding`、`Crypto`、`Topology`、`Numeric` 等），先匹配者胜。每个 rule set 必须被某个
  family 认领；kernel 新增 rule set 或算子而 overlay 没有归属时覆盖检查失败。
- **三档暴露加一档见证。** *typed*：`sig` 给出 Aevia 类型（`Result<T, OpError>`），包装器由签名推导；
  *witnessed*：见下；*dynamic*：自动生成 `Result<Value, OpError>` 包装器，新算子零编辑即可调用；*excluded*：
  `exclude`，必须写理由。`typed` 模式的领域里既无 `sig`、`dynamic` 也无 `exclude` 的算子是覆盖错误；`sig`
  的参数个数与 manifest 的 arity 不符、或指向已不存在的算子同样失败。
- **见证档（自动类型化）。** kernel 的引擎每次运行结束时，请求（`factorint(360)`）与答案（`list(list(2, 3), ...)`）
  同处一个等价类，这本身是一条类型判断；kernel 把这些判断记为“sort 事实”、按参数种类聚类，再逐条**重放**见证调用，
  得到 `signatures.witnessed.json`（rssn `docs/KERNEL.md` §3a）。manifest 里每个重载带 `overload_sources`。
  对没有 `sig`、但有可用 Aevia 类型表达的见证重载，领域模块生成与 typed 档同形的包装器
  （`fn divisors(n: Int): Result<Tensor<1, Int>, OpError>`）。类型映射：`Int`→`Int`，有理数/符号表达式/符号→`Real`，
  浮点→`Float`，`Bool`→`Bool`，矩形列表→`Tensor<秩, 元素>`；元组、不透明对象、参差列表留在 dynamic 档。一个算子有
  多个见证重载时，证据最多的占用算子名，其余以 `_int`/`_real`/`_float` 后缀区分。见证签名是**证据而非全称证明**：
  参数侧可能比算子实际接受的更窄（`Rssn.dynamic` 始终可用），结果由 `from_value<T>` 校验，越出观测范围得到 `Err`，
  不会得到类型错误的值。`dynamic op | 理由` 可强制某算子留在 dynamic 档。
- **转换是泛型的**：参数 `to_value(x)`，结果 `from_value<T>(v)`；丰富类型用 `codec Type to_fn from_fn` 声明一对转换函数
  （每个类型一对）：`Perm`（0 起始，边界上 ±1）、`Graph`/`Group`（包装不透明 `Value` 句柄）、`PermGroup`。
- **错误通道。** kernel 失败是 `Err(OpError)`；图后端算子参数越出定义域时返回未约化的调用项（`invmod(6, 15)`），
  `from_value` 把它报告为带该项文本的 `TypeMismatch`。
- **诊断通道：为什么没有约化。** 数值算子拒绝参数（形状不符、轴越界、系统奇异、Neumann 数据不相容……）时，kernel 只留下
  未约化的调用项。`Session::explain(term)`（`rssn::kernel::{Diag, DiagKind}`）在调用线程上重放该算子的求值器，给出
  `Diag { op, arg, kind, message }`。桥（`Kernel::call_with_witnesses`、`call_key`、`call_key_with_witnesses`）在结果恰好
  是该算子自己的未约化调用、且 `kind` 不是 `Symbolic` 时，返回 `KernelError::from_diag`，代码为稳定的
  `rssn.diag.<kind>`（`arity`、`shape`、`domain`、`singular`、`incompatible`、`not_converged`、`unsupported`、
  `symbolic`、`unspecified`）。`Rssn.dynamic` 在 `Err` 里写成用户形式——函数名、参数名、位置：
  ``Err(rssn.diag.domain: tensordot_2_2_1: argument `axes_a` (#3): an axis is past the rank of the first tensor (rank 2): [2])``。
  参数名取自 overlay 的 `sig`；witnessed/dynamic 包装器只给位置（`argument #3`）。`OpError` 仍是不透明类型，
  `"{e}"` 给出 `代码: 消息`，没有新增构造子或 `Rssn.*` 成员。在**抛出**位置（`Rssn.<op>(..)` 的类型化调用）同一发现是
  `E0807`，附注给出稳定代码。只有 `rules::numeric::TABLES` 的算子记录原因。
- **维度证据。** `sig` 同时声明“该算子是无量纲参数的纯函数”；kernel 声明的 `Dimensionless` 规则、以及参数已知无量纲时的
  `List/NonMetric/Compare/Angle` 规则，会让裸数结果携带 `Scalar(1)` 证据；其余规则（`Same`、`Sum`、`Custom`…）不被信任。
  `list(...)` 项结果在可信时被读回成张量（等长行合并成高阶张量，不等长保留为张量的张量）。
- **运行期生成。** 模块在解析 `import Rssn_NumberTheory` 时由 live manifest 渲染（`codegen::namespace_source`），不提交
  生成源，因此不与所链接的 kernel 漂移。提交的只有 overlay 与覆盖报告 `tools/rssn-api-audit/RSSN-COVERAGE.md`；
  `cargo test -p aevia-rssn --test domains` 重新生成并比对（`AEVIA_BLESS=1` 重写）。

**`Rssn_Numeric`。** rssn 的 `numeric` 规则集（稳健统计、窗与谱、小波、Krylov/稀疏/低秩/张量分解、维度无关的有限差分求解器、
复变特殊函数、表达式算子）全部有 `sig`，没有一个留在 dynamic 档；用户接口与示例见 §8.19.1。规则：

- 结果是浮点，签名用 `Float`/`Tensor<n, Float>`，**不是** `Real`（`from_value<Real>` 只接受精确的 `Int`/`Rat`）；整数性质的
  结果（秩、置换下标）同样以 `Float` 返回。
- 用符号命名的方法、小波、统计量参数类型是 `Real`；每次 `symbol("db2")` 产生新符号，所以每个名字只 `let` 一次再复用。
- **一个内核算子可以有多条 `sig`，各用自己的 Aevia 名字**（overlay 写作 `sig heat_fd as heat_fd_2d(..)`；同名重复是错误）：
  签名只有一个字面量秩，所以维度无关的算子按维数命名（`_2d`/`_3d`），秩多态的张量算子按秩命名
  （`tensor_norm_<r>`、`tensordot_<ra>_<rb>_<k>` 等，见 §8.19.1）。
- **混合列表的解码。** 张量分解的结果是类型各异的部分组成的列表。桥把它保留为带逐元素量纲证据的一维值，
  `from_value<Tensor<1, Value>>` 是通用的“列表访问器”：把第一个轴的各部分逐个作为 `Value` 句柄交出，再对每一部分
  `from_value<T>`。解码器是 `Numeric` 前言里的 Aevia 源码（`hosvd3_from_value` 等），用 `codec Hosvd3 - hosvd3_from_value`
  声明为只解码的类型（`-` 表示没有编码函数；把这样的类型当参数是覆盖检查的错误）。

示例：`tests/domain_number_theory.aev`、`domain_combinatorics.aev`、`domain_finite_fields.aev`、`domain_numeric.aev`。

**语言缺口（未实现）**：`let x: Zmod<17> = 5; x * x ^ -1` 需要三项语言改动，均在 `aevia-types`/`aevia-core`：
(1) 变体类型接受 `Nat` 参数（目前 `E0317`）；(2) `Num`/`Fractional` 类带 `add/sub/mul/neg/from_int/inv` 方法，并让二元运算符
对用户名义类型分派到实例；(3) 整数字面量按期望类型经 `from_int` 提升。在此之前 `Zmod` 的模数是运行期携带的，运算以函数形式给出
（`zmod_mul(x, inv)`）。

## 10.4 量纲桥

量纲/单位的唯一事实来源是 `rssn::dim`（`DimTerm`/`DimArena`/`UnitRegistry`）。Aevia 的 `Dim` 是它的类型级镜像；
`DimValue` 在运行时就是 `DimTerm`，不是字符串。

### 10.4.1 静态到运行时

**静态路径不把量纲传给 rssn**：类型检查器已保证量纲正确，项图里的符号无量纲，`DimensionMode` 设为 `Off`（或 `Advisory`
仅用于诊断）。只有三种场景交出量纲：

- `Dyn<ρ>` 的运行时检查：宿主保存 `DimTerm`，不依赖字符串；
- FFI/导出：优先 `Config::dimension_term(symbol, DimExpr)`（不解析文本，未知基名被 intern）。纯文本入口
  `Config::dimension(symbol, text)` 必须传**全名**（`Length`、`Mass`、`Time`、`Current`、`Temperature`、`Amount`、
  `Luminosity`），**绝不用单字母**（`L` 是升，`T` 是特斯拉，`N` 是牛顿，`J` 是焦耳）；
- 用户显式请求 rssn 的量纲诊断：`Config::dimensions(Advisory)`。

自定义基量纲用 `Config::base_dimension(name)` 声明；`prepare` 在**克隆出来的**图上调用 `define_base_dimension`。
`Graph::clone` 共享 `Arc<RwLock<DimArena>>`，写入时 `strong_count > 1` 就 detach（写时复制），所以请求期注册不会漏进
模板或并发请求，不定义基量纲的请求也不付拷贝成本。不要直接改 `Session` 的模板图。

### 10.4.2 运行时到静态

数值答案的量纲以 `Answer.dimension` 为准，**不要**从 `Answer.term` 重新推断。`Answer.dimension` 是 `Option`：
`DimensionMode::Off` 时为 `None`，回退到静态量纲；要运行时量纲，数值路径必须用 `Advisory` 或 `Strict`。静态路径的
答案量纲已知，`Answer.dimension` 只用于可选的一致性检查；不一致是内部错误 `E0809`。

### 10.4.3 量纲模式

| 场景 | 模式 |
|---|---|
| 普通静态检查通过的代码 | `Off`（项图无量纲） |
| `Dyn`、FFI、运行时构造的项 | `Strict`，用全名 |
| 用户显式请求诊断 | `Advisory` |

Aevia 不暴露 `bypass` 给普通用户；`Dyn<ρ>` 是显式逃逸。

## 10.5 线程与生命周期

### 10.5.1 Session 不是 `Sync`

`Session` 内部是 `RefCell`，`Term<'s>` 借用 `Session`。桥把内核会话放在**线程本地**（`aevia-rssn` 的 `kernel()`）：

```rust
thread_local! {
    static KERNEL: RefCell<Option<Rc<Kernel>>> = const { RefCell::new(None) };
}

pub fn kernel() -> Rc<Kernel>;   // 当前线程的会话，首次使用时创建
```

- 每个线程一个会话；`par_map` 的工作线程各自持有求值器副本和会话（§9.7.2）。
- 符号值不跨线程共享：kernel 的 `TermRef` 带 `SessionId`，别的会话的 handle 被拒绝（`OpError::ForeignTerm`）；不能跨线程的值回退为
  顺序求值。桥不使用 `Session::fork`/`merge`（kernel 提供的原语）。
- `compute_all` 在单会话内并行，但**桥没有使用它**：批量请求按顺序调用；需要并行时应在宿主侧按线程分配会话。

### 10.5.2 项的生命周期

`Term<'s>` 只在 `compute` 调用期间借用，返回后转成 `u32` id；`Session::term_by_id(id)` 取回，id 在会话内稳定；
`compute` 的答案项已导入回会话。项图只增不减，REPL `:reset` 重建会话。

## 10.6 性能策略

### 10.6.1 不要为每个算子调用 `compute`

`compute` 克隆模板图并运行重写；对 `x + 1` 这种 O(1) 构造，代价高几个数量级。规则：

- 算术/初等函数：`Session::call_value` 构造项（只做局部重写），不 `compute`。
- `simplify`/`eval`/`diff`/`integrate`/`solve`：`compute`。
- 打印默认不化简；用户显式 `simplify` 或 `:latex`。
- 比较/`if`：`Prop` 不求值；需要判定时 `decide` 或 `assuming`。

### 10.6.2 缓存

- `Config` 按规则集/假设/模式缓存；项按结构哈希缓存；编译函数按项哈希 + 输入名缓存；单位表/量纲别名是编译期常量。
  rssn 另有请求级答案缓存与可选的子项引理缓存（附录 C Revision 12）。
### 10.6.3 批处理

- 张量逐元素符号运算不要用 `compute_all`（每元素一个请求，开销大）：用 `map` 构造项，最后一次 `simplify`。
- 数值张量用 `F64` 路径，不进项图。

### 10.6.4 JIT

`Term::compile` 把闭式 `Float` 表达式编译成函数；Aevia 的热点函数走这条路径。`TieredBackend` 先解释、后台编译；JIT 失败回退
解释器，不改变语义。默认构建 `features.jit = false`，`@jit` 必须读 `Features`，否则报 `E1001("core.jit")`。

### 10.6.5 设备：数据并行执行

rssn 有两条互相独立的执行轴：

| 轴 | rssn 抽象 | 回答的问题 |
|---|---|---|
| 一个项怎样变成函数 | `Backend` / `Compiled` | 在一个点上求值 |
| 许多行在哪里跑 | `Device` / `Buffer` / `KernelIr` | 在 10⁶ 行上求值；对 10⁴ 个初值积分 |

JIT（§9.6）属于第一条轴，**不是** GPU 路径；`@jit(target = Gpu(n))` 选择第二条轴的设备。

**能力发现。** 构建层：manifest `features.devices`（`cpu`、`cpu-parallel`，启用 `gpu-wgpu` 时再加 `gpu-wgpu`），是编译期
事实，可进 `manifest_hash`；`core.gpu` = `features.devices` 含任一 `gpu-*`。运行层：`rssn::backend::device::registry().list()`
返回 `DeviceInfo`（`id`、`kind`、`dtypes`、`ops`、`parallelism`、`unified_memory`、`deterministic`）；没有适配器不是错误，
只是没有 GPU 设备。Aevia 侧封装在 `aevia_rssn::device`（`available()`、`has_gpu()`、`resolve(Selector)`）。

**`Device` 到 rssn 设备。**

| Aevia | rssn id | 说明 |
|---|---|---|
| `Cpu` | `cpu` | 单线程；参考语义 |
| `Cpu`（`cpu_parallel` 提示，`Selector::CpuParallel`） | `cpu-parallel` | rayon；与 `cpu` **逐位相同** |
| `Gpu(n)` | 第 `n` 个 `DeviceKind::Gpu`（`gpu:wgpu:n`） | 无则 `E1001("core.gpu")`，消息带已注册 GPU 数 |
| `Distributed(c)` | 无 | `E1001("core.distributed")`；不在本设计内 |

**传输是显式的。** `to_device(t, Gpu(n))` 是一次 `upload`（`F64` 张量在只有 `f32` 的设备上转为 `f32`，会改变数值）；`device_of` 读
张量的设备标签；结果回宿主要 `download`，不存在隐式传输。`on(device, thunk)` 是作用域执行：`thunk` 内的批量原语用该设备，
作用域结束释放缓冲区。

**精度合约。** 内核 IR 的语义是 `f64`。`cpu`、`cpu-parallel`：`F64`，与解释器逐位相同（含 NaN、`select`、越界 `gather` 读 NaN），
这是 §9.7.2 “线程数不改变任何结果”在设备上的延伸。`gpu:*`（WGSL）：`F32`，以及**双单精度的 `F64`**——WGSL 没有 `f64`，
wgpu 设备把 `Dtype::F64` 做成一对 `f32 (hi, lo)`，约 48 位精度（RTX 3060 / Vulkan 上 `+ - * /` 相对误差 `4e-15`，`sqrt`
`1e-14`，`exp` `7e-14`，设备测试要求 `5e-12`）；限制：指数范围仍是 `f32` 的（数据请落在 `1e-30 .. 1e30`），`sin`/`cos`/`tan` 在
`|x| > 1e6` 返回 NaN，`F32` 与 `F64` 缓冲区不能混在一次启动里。所以它**不是** IEEE 双精度，也不与 CPU 逐位相同。Aevia 不会
悄悄把 `Float` 结果换成 `f32`；要求位精确的代码选 `Cpu`/`CpuParallel`。

**启动融合。** rssn 的 `device::plan` 把一串逐元素内核融合为一次启动（中间列变成寄存器，不落显存）；融合改变值存放的位置而不是
怎么算，所以在确定性设备上与未融合逐位相同。这是 rssn 侧的显式 API，Aevia 尚未暴露。

**可降低到设备的函数**与 §9.6.1 的 JIT 门控一致：无捕获、`Float` 标量算术、初等函数、比较、`if`（两侧纯，急切求值后 `select`）。
含宿主调用的项（gamma/erf/Bessel 等 `EvalFn`、C 外部函数）不能降到设备，`Device.map` 回退解释器逐行求值，值不变（CPU 设备上）。

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

纯、位确定的版本只在 CPU 设备上承诺；`map_on(Gpu(n), ..)` 是 `IO`，因为分配与传输是效应，结果受设备精度约束。

**状态。** 已实现：rssn 侧完整设备层（注册表、`Buffer`、`KernelIr`、CPU 参考设备、WGSL 发射器、可选 `gpu-wgpu`、
`ensemble_rk4`、归约/matmul/CSR 乘/stencil 等 `device::array`）；manifest `features.devices`；`aevia_rssn::device` 与
`aevia-core` 的 `jit::map_floats`（把纯 `Float` 函数在 CPU 设备上批量应用）。**未实现**：`Device.map`/`map_on`/`reduce`/
`ensemble`/`available` 作为语言原语（需在类型表与内建表加条目并重新生成 `Device.aev`）；`to_device`/`on` 对 `Gpu(n)` 仍报
`E1001("core.gpu")`（张量值尚不携带设备缓冲区句柄）。

## 10.7 错误映射

| rssn 错误 | Aevia 诊断 |
|---|---|
| `ComputeError::Dimension(e)` | `E0401` 或 `E0412`（`Dyn`） |
| `ComputeError::NotNumeric(s)` | `E0803`（自由符号）或 `E0806`（定义域） |
| `ComputeError::Inconsistent(s)` | `E0809`（内部错误） |
| `ComputeError::Rule(e)`、`UnknownOperator(s)` | `E0899`（内部错误） |
| 预算耗尽 | `E0808` |
| `BackendError` | `E0807` 或 `E0899` |

`Answer.diagnostics` 转成 Aevia 警告；`Advisory` 模式下不失败。

## 10.8 已知缺口与风险

**下面的数字是测量，live manifest（`aevia rssn`）是唯一事实来源。** 在 `rssn 3.0.0-rc.1`（`api_version = 2`）上：1620 个算子
（99 declared + 103 overlay + 1418 derived，0 仅动态）、82 个单位、30 个常数（18 个精确）、`manifest_hash = d12fc2b7…`。
rssn 已实现 Manifest、动态 `Value`/`call_value`/`call_key`、重载解析、100% 签名覆盖、物理常数表、cdylib 插件 ABI、`rssn::dim`、
自定义基量纲、`Config::dimension_term`、`fork/merge`、结构化结果和 JIT lowering。下面只列**仍然存在**的集成缺口。

1. **`Session` 单线程。** 跨线程要用 `fork/merge`，不是共享会话；桥当前不跨线程共享符号值（§10.5.1）。
2. **`compute` 仍克隆图。** `OpTable` 是 `Arc`（clone 共享），`DimArena` 写时复制，剩下节点表的浅拷贝成本；`Session::prepare` 未公开。
3. **JIT 默认关闭**（见 §10.6.4）。
4. **`Solutions` 的“未报告”语义。** `multiplicities` 为空表示未报告，不能填 1；kernel 在能证明时填它（一元有理多项式且全部根是精确数时，
   用精确 square-free 分解）；`conditions` 只填构造上可声明的条件；`complete` 按“是否 `list(...)`”区分，`Solutions::new` 默认 `false`。
   Aevia 自己构造时不要无条件声明穷尽。
5. **常数是值，不是单位。** `manifest.constants` 不在 `UnitRegistry` 里，不能参与 `quantity` 解析；`e`（基本电荷）与 `Math.e` 同名，
   `open` 两个模块时必须限定。
6. **100% typed，但只有 `Declared`+`Overlay` 精确。** `Derived` 只区分数值 `(Real,…)` / 符号 `(Term,…)`，不区分 `Tensor`/`Quantity`/`Int`，
   所以 `Derived` 必须标成“保守推断”。
7. **`assuming` 以单元谓词为主**；一般等式/不等式没有语言入口（§10.2.2）。
8. **符号指数：桥接可往返，语言不支持。** `DimExpr::to_term` 把 `Length^n` intern 成原子，`try_from_term` 是其精确逆；Aevia 的静态
   类型位置仍只接受有理指数（§4.12.3）。不要把“后端能表示”误读成“语言该支持”。
9. **仿真参数仍是 JSON**，类型化包装器靠 `Physics.*` 源模块或生成的 `Rssn_Sim_<Name>`（§8.17）。
10. **预算/中断。** rssn 的预算在 `compute` 内；Aevia 外层超时/取消是协作式的，不能强杀。
11. **版本漂移。** Aevia pin `api_version` + `manifest_hash`，并跑 conformance。版本 2 的 manifest 增加 Rust 源路径元数据，二进制布局与版本 1
    不兼容，且没有解码前的版本协商，宿主须事先知道生产者版本。rc 周期内 manifest 仍可能变化，所以文档数字是测量而不是常量。
12. **`manifest_hash` 是 bincode BLAKE3，有两种取法。** `hash_hex()`（64 个 hex 字符，宿主应 pin）与 `hash_u64()`（前 8 字节小端读出，只用于
    ABI/显示）不可互换；JSON 只供人读，不能对 JSON 文本做哈希/比较。
13. **`Value` 的可入项图性按 `manifest.value_kinds` 走**（17 个变体中 10 个可入图、7 个不可，§13.3.1）；稀疏运算必须走 `eval_dyn` 或 Aevia 本地
    布局内核。生成器不要硬编码这张表。
14. **回调与插件桥。** `core.callback` 经同线程作用域句柄调用已检查源码闭包，句柄在外层调用返回后失效（§8.19）；`core.plugin.source` 把 `@op`
    函数追踪为 kernel 定义、`@rule` 为重写，作为进程内插件注册（§13.5.2）。
15. **单位属于 session。** `Session::register_unit` 注册的单位只出现在该 session 的 manifest；进程级 `KernelManifest::build` 给出不同的单位表与
    hash，Aevia 必须用 `Session::manifest()`。
16. **SDE。** manifest 只暴露噪声原语与方案元数据 `sde_scheme_info`（只给强阶，弱阶在宿主侧 `sde_order`，§8.9.1）；求解循环需要 `&Fn + Sync`
    host 闭包，manifest 的 `Value` ABI 传不了，所以保持在宿主侧（`aevia-std::sde`），`sdeint_*` 不是 `Rssn` 算子。

### 10.8.1 缓解措施

- **一致性测试**：对每个标准算子，比较 Aevia 静态量纲与 rssn `Answer.dimension`；不一致即失败。
- **单位表校验**：启动时逐项比较基/派生/仿射单位，只以 `manifest.units` 为准。
- **回退路径**：JIT 失败回退解释器；符号求解失败返回 `Err`，不 panic。
- **预算默认值**：2 s / 1e6 重写；用户可调。
- **版本 pin**：pin `api_version` + `hash_hex()`；升级时跑全量 conformance；文档里的算子/单位计数不进缓存键。
- **桥接层隔离**：所有 rssn 调用集中在 `aevia-rssn`，上层不直接依赖 `rssn` 类型。

## 10.9 一个端到端的调用链

```aevia
fn fall_time(h: Float[L]): Float[T] =
    sqrt(2 * h / g)
```

1. 类型检查：`h : Float[L]`，`g : Float[L/T^2]`，`2*h/g : Float[T^2]`，`sqrt : Float[T]`。
2. 调用 `fall_time(20.0 m)`：`20.0 m` 编译期折叠为 `Float` 值 20.0，带显示单位 `m`。
3. 函数体：`2 * h`、`/ g`、`sqrt` 在 `Float` 上直接用 f64 算术，不进项图；若参数是 `Real` 符号，则分别是 `Session::call_value`
   构造的 `mul`/`pow`/`sqrt` 项（只做局部重写），不 `compute`。
4. `println` 调用 `show`，按显示单位 `s` 打印：`2.0196199771025523 s`。
5. `approx(x)` 对符号/精确值以 `Numeric` 目标运行 `compute`，rssn 返回数值球，Aevia 取中点。

这个链条展示了核心性能策略：**类型/量纲在编译期，算术在项构造，重型求解才进 e-graph，数值热点走 f64/JIT**。

下一章用完整程序展示这些规则如何组合。
