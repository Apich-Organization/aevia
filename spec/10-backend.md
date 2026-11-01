# 10 — rssn 后端映射

Aevia 不重新实现计算机代数。rssn 提供项表示、重写引擎、量纲层、数值内核、
JIT 和仿真；Aevia 提供语法、类型/量纲检查、闭包、记录、张量形状、模块、
IO 和标准库编排。这一章定义两者的契约：哪些能力直接映射，哪些要包一层，
哪些是已知缺口。

## 10.0 反射优先：没有手写算子绑定

Aevia **不维护** `sin -> rssn::sin`、`det -> rssn::det` 这样的绑定表。启动/
构建时读取 RSSN 的 **Kernel Manifest**，生成 `Rssn` 模块：

```aevia
Rssn.dynamic("sin", [to_value(x)])  # Inferred：动态；Aevia 的 sin 是 typed
Rssn.dynamic("diff", [f, x]) # Inferred/request：动态
Rssn.dynamic("meijer_g", args) # 无签名/实验性/插件（name/alias）
```

- `signature_source = Declared | Overlay` → 类型化声明；
- `Inferred` / 无签名 → `dynamic` 声明，运行时检查类型/量纲；推断签名
  默认 `(Real,...,Real) -> Real`，不可信，不能当 typed façade；
- 新算子只要在 RSSN 注册，下一次构建就出现，不需要改 Aevia 编译器；
- 静态重载用 `OpKey { rule_set, name, signature_hash }`；运行时 0.2.13 仍
  按 name+arity 选择，同 arity 多型暂不生成 typed 声明（§13.4.1）。

完整接口清单见根目录 [`RSSN-INTEGRATION-REQUIREMENTS.md`](../RSSN-INTEGRATION-REQUIREMENTS.md)，
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
  Cranelift。
- `Term::as_tensor()` 把嵌套 `list` 项转成 `(shape, Vec<f64>)`。
- `Term::to_latex()`/`to_typst()`/`to_pretty()` 输出排版。
- `sim::scenario::run(name, json)` 运行物理仿真。

Aevia 直接复用的部分：

| 能力 | rssn 模块 |
|---|---|
| 精确数（大整数/有理数/浮点） | `graph::Number` |
| 项构造与 hash-consing | `graph::Graph`、`Session::call` |
| 重写引擎与规则集 | `graph::Engine`、`rules::*` |
| 微积分/代数/方程/ODE/PDE | `rules::calculus`、`solve`、`ode`、`pde` 等 |
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
  `x ∈ ℤ`）转成 `Facts`。一般等式/不等式通过 `Rssn.assume` 动态接口传给
  RSSN；后端不支持时返回 `E1001`（`core.assume.general`），而不是静默丢弃。

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

`Session::call(op, args)` 构造项；不触发重写。Aevia 的 `+` 在 `Real`/`Float`
符号路径上就是一次 `Session::call`，O(1)。

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
let r = Rssn.dynamic("myplugin/foo", [x, y])
match r with
| Ok(v) -> ...
| Err(e) -> ...
```

- `Rssn.dynamic` 的参数/返回值是 `Value`，运行时检查类型/量纲；
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
导出；`Units.SI` 从 `UnitRegistry` 生成，不维护第二张表。

### 10.3.7 仿真

```rust
let json = rssn::sim::scenario::run("ising", params)?;
```

Aevia 的 `simulate(name, json)` 直接调用；`Physics.*` 包装器由 RSSN 的
场景 manifest 生成（名称、参数 schema、返回 schema），新增场景不需要手写
Aevia 包装。`ising` 返回扁平的 `Vec<i8>`，包装器按 `width`/`height` 行主序
reshape 成 `Tensor<2, Int>`；`sim` 的错误是字符串，包装器按前缀映射成
`SimError` 枚举。

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
- FFI/导出：0.2.13 的 `Config` 只有 `dimension(symbol, text)`，没有
  `dimension_term`；因此边界必须传**全名**（`Length`、`Mass`、`Time`、
  `Current`、`Temperature`、`Amount`、`Luminosity`），**绝不使用单字母**
  （`L` 是升，`T` 是特斯拉，`N` 是牛顿，`J` 是焦耳）。静态路径不传量纲，
  所以这不影响正常程序。
- 用户显式请求 rssn 的量纲诊断：`Config::dimensions(Advisory)`。

基量纲在模板构建期注册，不在每次请求的 `prepare` 里注册；否则
`DimArena` 的共享 `Arc` 会被并发请求污染。

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

`Session` 内部是 `RefCell`，`Term<'s>` 借用 `Session`。Aevia 的桥：

```rust
thread_local! {
    static BRIDGE: RefCell<RssnBridge> = RefCell::new(RssnBridge::new());
}

struct RssnBridge {
    session: Session,
    symbols: HashMap<SymbolKey, u32>,   // TermId
    configs: HashMap<ConfigKey, Config>,
}
```

- 每个线程一个 bridge；`par_map` 的工作线程各自初始化。
- 符号值跨线程传递时，`TermId` 不能直接复用；需要重新构造（用项文本或
  结构化复制）。当前 RSSN `Session` 单线程；Aevia 用 `fork/merge` 支持符号并行，数值张量直接并行。
- `compute_all` 在单会话内并行；Aevia 用它批量处理独立的重型请求。

### 10.5.2 项的生命周期

- `Term<'s>` 只在 `compute` 调用期间借用；返回后转成 `u32` id。
- `Session::term_by_id(id)` 取回；id 在会话内稳定。
- `compute` 的答案项已经导入回会话，id 有效。
- `Session` 的项图只增不减；REPL `:reset` 重建会话。

## 10.6 性能策略

### 10.6.1 不要为每个算子调用 `compute`

`compute` 会克隆模板图并运行重写；对 `x + 1` 这种 O(1) 构造，代价高几个
数量级。Aevia 的规则：

- 算术/初等函数：`Session::call` 构造项，不 `compute`。
- `simplify`/`eval`/`diff`/`integrate`/`solve`：`compute`。
- 打印：默认不化简；用户显式 `simplify` 或用 `:latex`（可配置轻量规范化）。
- 比较/`if`：`Prop` 不求值；需要判定时 `decide` 或 `assuming`。

### 10.6.2 缓存

- `Config` 按规则集/假设/模式缓存；避免每次重建。
- 项按结构哈希缓存；重复子表达式复用。
- 编译函数按项哈希 + 输入名缓存。
- 单位表/量纲别名编译期常量。

### 10.6.3 批处理

- 多个独立重型请求用 `compute_all`；它在一个会话内并行准备/运行/导入。
- 张量逐元素符号运算不要用 `compute_all`（每个元素一个请求，开销大）；
  用 `map` 构造项，最后一次性 `simplify`。
- 数值张量用 `F64` 路径，不进项图。

### 10.6.4 JIT

- `Term::compile` 把闭式 `Float` 表达式编译成函数；Aevia 的热点函数走
  这条路径。
- `TieredBackend` 先解释、后台编译；适合 REPL 和交互式。
- JIT 失败回退解释器；不改变语义。

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

RSSN 0.2.13 已经实现 Manifest、动态 `Value`/`call_value`、签名/重载、
插件、`rssn::dim`、`Term` 遍历/替换、`fork/merge`、结构化结果和 JIT
lowering。下面只列**仍然存在**的集成缺口；已关闭的旧缺口不再重复。

1. **`Session` 单线程。** `Session` 仍是 `RefCell`；跨线程要用
   `fork/merge`，不是共享会话。`par_map` 对符号值必须 fork/merge。
2. **`compute` 深拷贝。** `prepare` 仍 `graph.clone()`；单次化简成本与
   图/算子表大小相关。`OpTable` 尚未用 `Arc` 共享，`Session::prepare`
   也未公开。
3. **`DimArena` 请求期污染。** `Graph::clone` 共享 `Arc<RwLock>`；请求期
   `define_base_dimension` 会影响模板。Aevia 必须只在启动期注册基量纲，
   请求期只读。
4. **JIT 默认关闭。** `Select`/`Extern1`/`Gather` 已 lowering，但默认
   构建 `features.jit=false`；`@jit` 必须读 `Features`，否则报
   `E1001("core.jit")`。
5. **`Solutions.multiplicities` 为空。** `solve_structured` 不伪造重数；
   Aevia 的 `Solutions` 必须保留“空 = 未报告”，不能填 1。
6. **ODE 事件仍只覆盖部分方法。** `events` 主要在 `Dopri5`；`Rk4` 需要
   `steps`，`Radau` 不支持 events。Aevia 的 `OdeOpts` 按此限制。
7. **`manifest.units` 是 `UnitValue`，不是 `UnitDef`。** 它缺
   `aliases`/`prefixable`/`category`/`since`；Aevia 生成 `Units.SI` 时若
   需要别名/前缀元数据，必须读 `UnitRegistry`，不能只读 manifest。
8. **`signatures.overlay.json` 缺失。** `include_str!` 依赖它，但
   `.gitignore` 的 `**/*.json` 把它排除；干净 clone 无法构建。需要把它
   纳入版本控制或改成可选加载。
9. **签名覆盖率 58.4%，但可信 typed 只有 7 个。** 799 有签名（7 declared
   / 792 inferred）；推断签名默认 `(Real,...,Real) -> Real`，不可信。Aevia
   只为 declared/overlay 生成 typed，其余 1362 个走 `Rssn.dynamic`。
10. **`assuming` 仍以单元谓词为主。** 一般等式/不等式走
    `Rssn.assume` 动态入口；后端缺失报 `E1001`。
11. **复数表示。** `Value::Complex` 有 `Exact`/`Float`，但 `Number` 仍
    没有复数；符号复数靠 `i` 项，数值复数靠 `Complex64`。两条路径需在
    Aevia 侧统一。
12. **符号指数。** `DimTerm` 支持符号指数，但 `DimExpr::to_term` 对
    `Pow(..., Exp::Symbol)` 仍报“不能 intern”；Aevia 的静态 `Dyn` 桥接
    要避开。
13. **仿真 JSON schema。** `SimResult` 有结构化结果，但场景参数仍是
    JSON；Aevia 的 `Physics.*` 由 manifest/schema 生成，不能手写。
14. **预算/中断。** rssn 的预算在 `compute` 内；Aevia 外层超时/取消是
    协作式的，不能强杀。
15. **版本漂移。** `api_version=1`、`rssn 0.2.13`；Aevia 必须 pin
    `api_version` + `manifest_hash`，并跑 conformance。
16. **`manifest_hash` 是 bincode BLAKE3。** JSON 只供人读；Aevia 不能对
    JSON 文本做哈希/比较。
17. **自定义基量纲单位注册失败。** `UnitRegistry::register` 的
    `expr_to_dim` 只认 7 个内置基名；`unit EUR : Currency` 报
    “not a base dimension”。Aevia 报 `E1001("core.units.custom_base")`。
18. **`Value::Sparse` 不能入项图。** `term_of_value` 对 Sparse/Record/
    Variant/Function/Opaque 返回 `Unsupported`；稀疏运算必须走 `eval_dyn`
    或 Aevia 本地布局内核。

### 10.8.1 缓解措施

- **一致性测试**：对每个标准算子，比较 Aevia 静态量纲与 rssn
  `Answer.dimension`；不一致即失败。
- **单位表校验**：启动时逐项比较基单位/派生单位/仿射单位。
- **回退路径**：JIT 失败回退解释器；符号求解失败返回 `Err`，不 panic。
- **预算默认值**：2 s / 1e6 重写；用户可调。
- **版本 pin**：`Cargo.toml` 固定 rssn 次版本；升级时跑全量 conformance。
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
   `sqrt` 构造 `sqrt(...)` 项。这些都是 `Session::call`，不 `compute`。
4. 返回 `Float` 值（`g` 是 `Float` 常量，`2*h/g` 折叠为浮点数；
   `sqrt` 对 `Float` 永远走 f64，负数得 NaN，不会保持符号）。
5. `println` 调用 `show`：直接得到 `2.019`，按显示单位 `s` 打印。
6. 如果用户写 `approx(fall_time(20.0 m))`：`compute` 以 `Numeric` 目标运行，
   rssn 返回数值球，Aevia 取中点。

这个链条展示了 Aevia 的核心性能策略：**类型/量纲在编译期，算术在项构造，
重型求解才进 e-graph，数值热点走 f64/JIT**。

下一章用完整程序展示这些规则如何组合。
