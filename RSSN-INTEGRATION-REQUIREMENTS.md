# RSSN 集成需求（Aevia ↔ RSSN）

> 状态：**设计约束，非建议**。Aevia 的语言设计已经把 RSSN 当作同一项目的
> 内核；下面列出的接口是 Aevia 规范能够成立的前提。标注 **P0** 的项在
> Aevia 编译器可以端到端运行之前必须完成；**P1** 是完整能力/可维护性所需；
> **P2** 是性能与生态项，但接口必须现在定下来，避免以后改 ABI。
>
> 本文不按“v1/v2”分期。这里的 P0/P1/P2 是**实现依赖顺序**，不是语言版本；
> Aevia 不会因为某项未完成就砍掉对应语言能力，而是让该能力在运行时报
> `E1001 backend capability missing`，并在编译期给出明确的 capability 诊断。

---

## 0. 结论

Aevia 不再为 RSSN 的每个算子手写 Rust/C 绑定。采用三层结构：

1. **RSSN 是唯一算子注册表。** 每个 rule set 在注册算子时同时提供
   **签名、量纲规则、文档、稳定性、别名、重载**。RSSN 在启动时生成
   **Kernel Manifest**（JSON/CBOR），描述全部 rule set、算子、单位、基量纲、
   能力和版本。
2. **Aevia 编译器消费 manifest，自动生成 `Rssn` 模块。** 有签名的算子生成
   类型化声明；没有签名的算子生成 `Dynamic` 声明，仍可调用。Aevia 仓库里
   没有“算子绑定表”，只有生成器和少量手写策略。
3. **动态调用是正式能力，不是降级路径。** `Rssn.call("op", args)` 接受
   `Value` 数组，返回 `Result<Value, OpError>`；类型化 façade 只是它的
   静态视图。新算子只要在 RSSN 注册，Aevia 下一次构建就能看见；不需要改
   Aevia 编译器。

量纲/单位同样只有一套：RSSN 的 `rssn-dim`（从 `graph/dim.rs` 与
`rules/units.rs` 抽出）是运行时与编译期共享的规范层；Aevia 的 `Dim` 是它
的类型级镜像，`Units.SI` 由 `UnitRegistry` 生成，不再维护第二张单位表。

---

## 1. 现状核查（已对照源码）

| 能力 | 现状 | 对 Aevia 的影响 |
|---|---|---|
| 算子注册 | `OpTable` 是开放的，`OpDescriptor { name, arity, flags, cost, binder, eval }`；`OpTable::iter()` 可枚举 | 可以反射，但**没有类型签名/文档/重载/稳定性**，Aevia 无法生成类型化接口 |
| 规则集 | `RuleSet` + `Installer` + `Engine::install`；`rules::standard()` 约 30 个 rule set | 可以整体安装；但 rule set 的版本/能力没有 manifest |
| 动态调用 | `Session::call(name, args)` 按名字调用已注册算子；`Term::eval` 只做数值 | 没有统一的 `Value` 动态 ABI；高阶算子/回调无法表达 |
| 签名 | `OpDescriptor` 只有 arity，没有参数/返回类型 | 1000+ 算子无法逐个手写 Aevia 绑定 |
| 重载 | 同名不同 arity 会 `OpConflict` | `solve`/`norm`/`integrate` 等需要按类型/arity 分派 |
| 量纲 | `graph::dim` 有 `DimTerm`/`DimArena`/`DimRule`；`Dimension` 是快照，指数是 `String` | Aevia 需要结构化、可序列化、可比较的规范形；字符串指数不可靠 |
| 单位 | `rules::units` 内部 `HashMap<&str, Def>`，`Def` 私有；`UnitInfo.scale: f64` | Aevia 无法枚举/注册单位；`f64` 丢失精确尺度；仿射信息只有 `affine: bool` |
| 单位文本 | `dimension_from_text` 先查单位表；`L` 是升、`T` 是特斯拉 | Aevia 若传 `L/T` 会得到错误量纲；必须用全名或结构化入口 |
| 单位值 | `quantity(v,u)` 是惰性项，没有 eval；`Answer.value` 只对字面数生效 | `3 m + x` 进入 RSSN 会丢掉单位；符号量无法数值化 |
| 线程 | `Session` 是 `RefCell`，`Term<'s>` 借 `&Session` | 符号值不能跨线程；`par_map` 对 `Real` 不可用 |
| `Term` | 只有私有 `session/node`；没有 children/substitute/free_symbols | `diff(f)` 返回闭包只能重跑 `compute`；闭包性检查拿不到自由符号 |
| `compute` | `prepare` 每次 `graph.clone()`，`OpTable` 深拷贝；`DimArena` 是共享 `Arc<RwLock>` | 单次化简成本与算子表大小相关；并发请求会污染基量纲 |
| JIT | tape 有 `Select` 但没有 lowering；没有 load/index；`OpDescriptor.eval` 是 fn pointer | 带分支/索引/插件的函数无法 JIT |
| 结构化结果 | `solve` 返回根列表；重数/条件在求解器内部；`odeint` 接受扁平 `&[f64]` | `Solutions` 只能填默认值；记录状态需要 Aevia 自己展平 |

---

## 2. 目标架构

```
Aevia source
   │  typecheck (Dim = type-level mirror of rssn_dim::DimTerm)
   ▼
Core IR ──► Aevia runtime ──► Kernel ABI ──► RSSN
   │                            │
   │                            ├─ manifest()        (全部算子/签名/单位/能力)
   │                            ├─ call(op, args)    (动态 Value ABI)
   │                            ├─ call_ho(op, args, callbacks)
   │                            └─ register(plugin)  (用户/第三方扩展)
   ▼
generated `Rssn` module
   ├─ typed declarations (from Signature)
   └─ dynamic declarations (no signature / experimental)
```

三条硬规则：

1. **Aevia 不维护算子清单。** 任何 `Rssn.foo` 的存在性由 manifest 决定；
   Aevia 仓库只有生成器和 `signature overlay`（补 RSSN 暂时缺的签名）。
2. **Aevia 不维护单位表。** `Units.SI` 从 `UnitRegistry` 生成；用户
   `dimension`/`unit` 声明注册进同一个 registry。
3. **动态调用永远可用。** 即使某个算子没有签名，`Rssn.call` 仍能调用；
   类型化 façade 是增量，不是能力门槛。

---

## 3. RSSN 必须新增/修改的接口

### 3.1 P0 — Kernel Manifest

新增 `rssn-api` crate（或 `rssn::manifest` 模块），提供：

```rust
pub const API_VERSION: u32 = 1;

#[derive(Serialize, Deserialize)]
pub struct KernelManifest {
    pub rssn_version: String,          // semver
    pub api_version: u32,
    pub manifest_hash: [u8; 32],       // 规范化 JSON/CBOR 的 BLAKE3
    pub features: Features,
    pub rule_sets: Vec<RuleSetInfo>,
    pub ops: Vec<OpInfo>,
    pub units: Vec<UnitDef>,
    pub base_dimensions: Vec<BaseDimInfo>,
}

#[derive(Serialize, Deserialize)]
pub struct Features {
    pub dynamic_call: bool,
    pub plugin_abi: u32,
    pub jit: bool,
    pub jit_branch: bool,
    pub jit_index: bool,
    pub parallel_symbolic: bool,
    pub exact_units: bool,
    pub affine_units: bool,
    pub user_base_dimensions: bool,
}
```

`RuleSetInfo`：

```rust
pub struct RuleSetInfo {
    pub name: String,
    pub version: String,
    pub stability: Stability,
    pub since: String,
    pub capabilities: Vec<String>,     // "calculus", "ode", "units", ...
    pub ops: Vec<String>,              // op keys
}
```

`OpInfo`：

```rust
pub struct OpInfo {
    pub key: OpKey,                    // 稳定键，见 3.3
    pub name: String,
    pub aliases: Vec<String>,
    pub category: String,
    pub arity: Arity,
    pub overloads: Vec<Signature>,
    pub flags: OpFlagsInfo,            // associative/commutative/heavy/...
    pub binder: Option<BinderInfo>,
    pub cost: u32,
    pub eval: EvalKind,                // None | Scalar | Dynamic | HostCallback
    pub higher_order: bool,
    pub purity: Purity,                // Pure | ReadOnly | Effectful
    pub determinism: Determinism,      // Deterministic | Randomized | EnvDependent
    pub thread_safety: ThreadSafety,   // Send | SessionBound | NotThreadSafe
    pub stability: Stability,          // Stable | Experimental | Deprecated
    pub since: String,
    pub doc: String,                   // Markdown
    pub examples: Vec<Example>,
    pub dim_rule: Option<DimRuleInfo>,
    pub backend_support: BackendSupport, // interpreter/jit/ffi/cuda...
}
```

Manifest MUST 是**确定性**的：同一 RSSN 构建、同一 rule set 集合产生同一
字节序列（键排序、无 HashMap 迭代顺序、无时间戳）。Aevia 用
`manifest_hash` 做缓存键和兼容性检查。

CLI/FFI：

```rust
impl Engine {
    pub fn manifest(&self) -> &KernelManifest;
}
impl Session {
    pub fn manifest(&self) -> &KernelManifest;
}
```

C ABI：

```c
const char* rssn_manifest_json(void);       // 调用者释放
uint64_t    rssn_manifest_hash(void);
```

### 3.2 P0 — 动态 Value/Call ABI

新增统一的 `Value`，不要用 `f64` 数组硬编码：

```rust
pub enum Value {
    Int(BigInt),
    Real(Number),                 // 精确/符号
    Float(f64),
    Complex(ComplexValue),        // 精确或 C64
    Bool(bool),
    String(String),
    Unit(UnitValue),              // 单位值（不是 quantity）
    Quantity(QuantityValue),      // 值 + 单位（显示/FFI 用）
    Tensor(TensorValue),          // 元素 Value，rank/shape 元数据
    Sparse(SparseValue),          // 3.8 决策
    Record(RecordValue),
    Variant(VariantValue),
    Term(TermRef),                // 符号项句柄
    Function(CallbackRef),        // 高阶参数
    Dyn(DynValue),                // 值 + DimTerm
    Opaque(OpaqueValue),
}

pub struct CallCtx {
    pub target: Target,           // Symbolic | Numeric | Approx
    pub budget: Budget,
    pub dimension_mode: DimensionMode,
    pub assumptions: Vec<Assumption>,
    pub trace: Option<&mut Trace>,
}

pub trait Kernel {
    fn call(&self, op: &OpKey, args: &[Value], ctx: &CallCtx)
        -> Result<Value, OpError>;
    fn call_ho(&self, op: &OpKey, args: &[Value], fns: &[CallbackRef],
               ctx: &CallCtx) -> Result<Value, OpError>;
    fn register(&mut self, plugin: Box<dyn Plugin>) -> Result<PluginId, PluginError>;
}
```

`OpError` 必须是结构化枚举，不要字符串：

```rust
pub enum OpError {
    UnknownOp(OpKey),
    Arity { expected: Arity, got: usize },
    Type { expected: TypeExpr, got: TypeExpr, arg: usize },
    Dimension(DimError),
    NotNumeric(String),
    NotReduced(String),
    Domain(String),
    BudgetExceeded,
    Unsupported { backend: String, feature: String },
    Plugin(PluginError),
}
```

`Session` 的便利包装：

```rust
impl Session {
    pub fn call_value(&self, name: &str, args: &[Value], ctx: &CallCtx)
        -> Result<Value, OpError>;
    pub fn value_of(&self, term: Term<'_>) -> Result<Value, OpError>;
    pub fn term_of(&self, value: &Value) -> Result<Term<'_>, OpError>;
}
```

C ABI（CBOR 或 JSON；二进制优先）：

```c
rssn_value_t* rssn_call(rssn_session_t*, const char* op, const rssn_value_t* args, size_t n, rssn_ctx_t*);
void          rssn_value_free(rssn_value_t*);
```

### 3.3 P0 — 签名、重载、稳定键

`OpDescriptor` 扩展（保留现有字段，新增可选字段）：

```rust
pub struct OpDescriptor {
    pub name: Arc<str>,
    pub arity: Arity,
    pub flags: OpFlags,
    pub cost: u32,
    pub binder: Option<Binder>,
    pub eval: Option<EvalFn>,              // 保留，标量快速路径
    pub eval_dyn: Option<Arc<dyn EvalDyn>>,// 新：可捕获/可插件
    pub overloads: Vec<Signature>,         // 新
    pub doc: Arc<str>,                     // 新
    pub category: Arc<str>,                // 新
    pub aliases: Vec<Arc<str>>,            // 新
    pub stability: Stability,              // 新
    pub since: Arc<str>,                   // 新
    pub purity: Purity,                    // 新
    pub determinism: Determinism,          // 新
    pub thread_safety: ThreadSafety,       // 新
}
```

`Signature`：

```rust
pub struct Signature {
    pub type_params: Vec<TypeParam>,       // name + kind + bounds
    pub params: Vec<TypeExpr>,
    pub ret: TypeExpr,
    pub constraints: Vec<Constraint>,      // Eq/Ord/TotalOrd/Show/...
    pub effects: EffectRow,                // Pure | IO | ...
    pub dim: Option<DimRuleInfo>,          // 与 DimRule 对应
    pub variadic: Option<Variadic>,        // 仅 zeros/fill 等
}

pub enum TypeExpr {
    Int, Real(DimExpr), Float(DimExpr), Complex(DimExpr),
    Bool, String, Unit(DimExpr), Point(DimExpr),
    Tensor { rank: NatExpr, shape: Option<Vec<NatExpr>>, elem: Box<TypeExpr> },
    Sparse { format: String, rank: NatExpr, elem: Box<TypeExpr> },
    Record(Vec<(String, TypeExpr)>),
    Variant(Vec<(String, Vec<TypeExpr>)>),
    Function(Vec<TypeExpr>, Box<TypeExpr>),
    Table(RowExpr),
    Dyn(RepExpr),
    Device,
    Opaque(String),
    Var(String),
}

pub struct OpKey {
    pub rule_set: Arc<str>,
    pub name: Arc<str>,
    pub signature_hash: [u8; 16],          // 无签名时为零
}
```

**重载**：`OpTable` 允许同名多签名。`OpId` 仍然唯一；`by_name` 改为
`HashMap<Arc<str>, Vec<OpId>>`。`register` 在“同名 + 同签名”时幂等，在
“同名 + 不同签名”时新增重载，而不是 `OpConflict`。`lookup(name)` 返回
主重载；`lookup_overload(name, &[TypeExpr])` 返回最具体匹配。

**稳定键**：Aevia 不依赖 `OpId` 数字（注册顺序会变），使用
`OpKey { rule_set, name, signature_hash }`。`manifest_hash` + `OpKey` 是
跨版本缓存键。

**签名覆盖**：

- 新算子用 `#[rssn_op]` 宏注册，自动生成 `Signature`、文档、示例。
- 现有 1000+ 算子先由 `signature_infer` 推断：
  - `Arity::Fixed(n)` + `eval` 存在 + `DimRule` → 默认
    `(Scalar, ..., Scalar) -> Scalar`；
  - `PREDICATE` → `-> Bool`；
  - `HEAVY` → 返回 `Term`/`Opaque`；
  - `LIST`/`Tensor` 参数按命名约定（`_list`、`_tensor`、`_vec`）推断。
- 推断结果写入 `signatures.overlay.toml`（RSSN 仓库内），可人工修正。
- CI 门禁：`cargo run -p rssn-api --bin signature-audit` 必须报告
  **每个算子**要么有 `Signature`，要么 `dynamic_reason` 非空；否则失败。
  这样 1000+ 算子不需要一次写完，但**不允许无声遗漏**。

### 3.4 P0 — 插件与宿主回调 ABI

`EvalFn` 是 `fn(&[f64]) -> f64`，无法捕获 Aevia 闭包。新增：

```rust
pub trait EvalDyn: Send + Sync {
    fn eval(&self, args: &[Value], ctx: &CallCtx) -> Result<Value, OpError>;
    fn signature(&self) -> Signature;
}

pub trait Plugin: Send + Sync {
    fn manifest(&self) -> PluginManifest;
    fn register(&self, reg: &mut Registry<'_>) -> Result<(), PluginError>;
    fn call(&self, op: &OpKey, args: &[Value], ctx: &CallCtx)
        -> Result<Value, OpError>;
}

pub trait HostCallback: Send + Sync {
    fn call(&self, args: &[Value]) -> Result<Value, OpError>;
    fn signature(&self) -> Signature;
}
```

`Registry` 提供：

```rust
pub fn register_op(&mut self, desc: OpDescriptor) -> Result<OpId, OpError>;
pub fn register_rewrites(&mut self, tier: Tier, texts: &[&str]) -> Result<(), OpError>;
pub fn register_dim_rule(&mut self, op: OpId, rule: DimRule) -> Result<(), OpError>;
pub fn register_unit(&mut self, unit: UnitDef) -> Result<(), OpError>;
pub fn register_base_dimension(&mut self, name: &str) -> Result<BaseId, OpError>;
```

Aevia 的 `@op` 生成 `Plugin`/`HostCallback`；用户可以把 Aevia
函数注册为 RSSN 算子，供符号重写/数值内核调用。插件动态库 ABI 用
`extern "C"` + CBOR `Value`，版本号在 manifest 中。

### 3.5 P0 — 量纲/单位统一（`rssn-dim`）

把 `graph/dim.rs` 与 `rules/units.rs` 抽成独立 crate `rssn-dim`，Aevia 直接
依赖它。必须公开：

```rust
pub struct Scale { pub rat: BigRational, pub pi: i32 }   // 精确，不用 f64

pub struct UnitDef {
    pub name: String,
    pub aliases: Vec<String>,
    pub dimension: DimTerm,
    pub scale: Scale,
    pub offset: Option<BigRational>,   // 仿射：SI = v*scale + offset
    pub prefixable: bool,
    pub affine: bool,
    pub display: String,               // "m", "°C"
    pub category: String,
    pub since: String,
}

pub trait UnitRegistry {
    fn list(&self) -> &[UnitDef];
    fn lookup(&self, name: &str) -> Option<&UnitDef>;
    fn resolve(&self, expr: &str) -> Result<ResolvedUnit, DimError>;
    fn register(&mut self, unit: UnitDef) -> Result<(), DimError>;
    fn format(&self, dim: &DimTerm, value: &Value) -> String;
    fn snapshot(&self) -> UnitRegistrySnapshot;
}

pub struct ResolvedUnit {
    pub dimension: DimTerm,
    pub scale: Scale,
    pub offset: Option<BigRational>,
    pub affine: bool,
}
```

`DimTerm` 必须提供结构化访问：

```rust
impl DimTerm {
    pub fn factors(&self) -> Vec<(BaseId, BigRational)>;
    pub fn vars(&self) -> Vec<(DimVarId, BigRational)>;
    pub fn mul(&self, other: &Self) -> Self;
    pub fn pow(&self, exponent: &BigRational) -> Self;
    pub fn to_dimension(&self, arena: &DimArena) -> Dimension;
    pub fn from_dimension(d: &Dimension, arena: &DimArena) -> Result<Self, DimError>;
    pub fn to_string(&self, arena: &DimArena) -> String;
}
```

`Dimension` 的 `factors: Vec<(String, String)>` 改为
`Vec<(BaseId, BigRational)>`（或提供 `factors_rat()`）；字符串只用于显示。

`DimArena`：

```rust
impl DimArena {
    pub fn base_names(&self) -> &[String];
    pub fn define_base(&mut self, name: &str) -> BaseId;
    pub fn intern(&mut self, name: &str) -> BaseId;
    pub fn snapshot(&self) -> DimArenaSnapshot;
    pub fn restore(&mut self, snap: &DimArenaSnapshot);
}
```

**单位文本**：`dimension_from_text` 必须先按**全名**解析
（`Length`/`Mass`/`Time`/`Current`/`Temperature`/`Amount`/`Luminosity`），
再尝试别名；**禁止**把单字母 `L`、`T`、`N`、`J` 当作量纲名。新增
`Config::dimension_term(symbol, DimTerm)` 结构化入口，Aevia 静态路径只走
这个入口；文本只用于 FFI/人类输入。

**`quantity` 数值化**：`quantity(v,u)` 必须有 `eval` 或 `Value::Quantity`
路径，让 `3 m + x` 在数值相位得到 `3 + x` 并保留 `DimTerm`；或者明确
规定 Aevia 静态路径不把 `quantity` 放进项图（当前规范采用后者），但
`Value::Quantity` 仍要支持显示/FFI/`Dyn`。

**用户自定义量纲/单位**：

- `Config::define_base_dimension` 必须在请求期安全（不污染模板）；
- `UnitRegistry::register` 支持用户单位、别名、前缀、仿射；
- `dimension`/`unit` 声明可 `private` 或导出；导出信息进入 manifest；
- Aevia 的 `Units.SI` 是 `UnitRegistry` 的生成视图，不是第二张表。

**`Point`/仿射**：`UnitRegistry` 的 `affine`/`offset` 足够表达 `°C`/`°F`；
Aevia 的 `Point` 是静态包装，运行时 lower 成 `Float` + `UnitId`；RSSN 的
`quantity`/`convert` 必须能精确处理 offset（已有，保留）。

### 3.6 P0 — Session/Term/线程

```rust
pub struct Term {
    pub fn id(&self) -> TermId;
    pub fn op(&self) -> OpId;
    pub fn children(&self) -> &[TermId];
    pub fn substitute(&self, map: &[(TermId, TermId)]) -> Term;
    pub fn free_symbols(&self) -> Vec<SymbolId>;
    pub fn dimension(&self) -> Option<Dimension>;
    pub fn to_value(&self) -> Result<Value, OpError>;
}
```

`Term` 不再借 `&Session`；改为 `(SessionId, TermId)` 的 owned handle，使用
时校验 session。这样 `Term` 可以存入 Aevia 的 `Value`/闭包。

并行：

- `Session` 可以保持单线程；新增 `Session::fork() -> Session` 和
  `Session::merge(&mut self, other: &Session) -> Result<(), MergeError>`，
  通过 `Value`/`Term` 序列化合并。
- `par_map` 对符号值用 fork/merge；对 `Float` 直接用线程池。
- `DimArena` 在 fork 时深拷贝（COW）；请求期只读。

性能：

- `OpTable` 用 `Arc` 共享，`prepare` 不再深拷贝；
- `Session::prepare(config) -> Prepared` 复用模板；
- `compute_batch` 对同模板请求批量执行；
- `DimArena` 的 `Arc<RwLock>` 改为 COW snapshot，请求之间不共享可变状态。

### 3.7 P1 — JIT

- `Select` 必须 lowering；`if` 可以 JIT。
- 增加 load/index 指令，支持 `Tensor` 索引。
- `OpDescriptor` 增加 `jit: Option<JitLowering>`；插件算子可以注册
  lowering，否则回退解释器。
- JIT 缓存键包含 `manifest_hash` + `OpKey` + 目标设备。
- 数值内核的 `f64` 快路径与 `Value` 动态路径分离；类型化 façade 优先
  JIT，动态调用走解释器。

### 3.8 P1 — 结构化结果

- `Solutions`：`roots: Vec<Value>`, `multiplicities: Vec<u32>`,
  `conditions: Vec<Condition>`；求解器返回结构化结果，不只是 `list(...)`。
- ODE：`OdeSolution { t, y, flatten_order, event }`；记录状态按字段声明
  顺序展平；提供 `flatten_schema` 元数据。
- 仿真：`SimResult` 带 schema/reshape 元数据；`SCENARIOS` 生成 manifest。
- 统计/拟合：返回带量纲的 `Value`，不要只返回 `f64`。

---

## 4. Aevia 侧如何消费

### 4.1 生成 `Rssn` 模块

构建时：

1. 读取 `KernelManifest`。
2. 对每个 `OpInfo`：
   - 有 `overloads` → 生成类型化声明 `Rssn.<name>(...)`；
   - 无签名 → 生成 `Rssn.dynamic("<key>", args)`；
   - 有别名 → 生成别名，冲突时加 `Rssn.<rule_set>.<name>`。
3. 生成 `Rssn.Capabilities` 常量、`Rssn.Version`、`Rssn.ManifestHash`。
4. 生成文档/补全/签名存根（LSP）。

Aevia 仓库只保留：

```
tools/gen_rssn_module.aev   # 生成器
rssn.signatures.overlay.toml # 仅补 RSSN 缺的签名
```

### 4.2 动态 fallback

```aevia
# 有签名：静态检查
let y = Rssn.sin(x)

# 无签名/实验性：动态调用，运行时检查
let z = Rssn.dynamic("special/meijer_g", [a, b, z])

# 任意新算子：同一个入口
let w = Rssn.call("my_plugin/foo", [x, y])
```

`Rssn.dynamic` 的返回类型是 `Result<Value, OpError>`；Aevia 提供
`Rssn.typed<T>(...)` 在运行时把 `Value` 转成 `T`（带类型/量纲检查）。

### 4.3 重载解析

- 先按 arity 过滤；
- 再按参数类型/量纲匹配；
- 多个候选时选最具体；歧义报 `E1002`；
- 没有匹配时尝试 `Dynamic`；仍失败报 `E1003`，并列出候选签名。

### 4.4 版本与缓存

- `manifest_hash` 进入 Aevia 构建缓存键；
- `api_version` 不兼容时编译器报 `E1004`；
- 算子 `stability` 映射到 Aevia 的 `@stable`/`@experimental`/
  `@deprecated`；`@deprecated` 只警告，不删除。

---

## 5. 版本、稳定性、兼容性

- **语言不按 v1/v2 分层。** 所有能力在设计里存在；未实现的后端能力用
  `capability` 诊断，而不是删语言特性。
- `Stability { Stable, Experimental, Deprecated }`：
  - `Stable`：语义/签名在 major 内不变；
  - `Experimental`：可改，但必须保留旧名至少一个 minor；
  - `Deprecated`：至少一个 minor 的警告期，之后才可移除。
- `rssn-api-version` 是整数，不兼容变更 +1；`manifest_hash` 每次构建变化。
- Aevia 编译器 pin `api_version` + `manifest_hash` 范围；CI 对 manifest
  做快照测试。
- 算子别名/旧名进入 manifest；Aevia 生成 `deprecated` 别名。

---

## 6. 实现顺序（依赖顺序，不是语言版本）

| 阶段 | RSSN 交付 | Aevia 可解锁 |
|---|---|---|
| K0 | Manifest + `Value` + `call` + `OpTable::iter` 已有 | 生成 `Rssn` 动态模块；1000+ 算子全部可见 |
| K1 | 签名/重载/覆盖审计 + `#[rssn_op]` | 类型化 façade、补全、文档 |
| K2 | `rssn-dim` + `UnitRegistry` + 结构化 `DimTerm` | Aevia 单位/量纲统一；用户单位/量纲 |
| K3 | `Term` 遍历/替换/自由符号 + `prepare`/COW | `diff` 闭包、并行、性能 |
| K4 | 插件/宿主回调 + 动态库 ABI | 用户自定义算子、Aevia 函数进 RSSN |
| K5 | JIT `Select`/index + 插件 lowering | 分支/索引/插件 JIT |
| K6 | 结构化 `Solutions`/ODE/Sim | 完整结果类型 |

任何阶段未完成时，Aevia 仍可运行：对应算子走动态 fallback；对应能力报
`E1001`（带 capability 名），不删语言特性。

---

## 7. 验收标准

1. **1000+ 算子可见性**：`manifest.ops.len() >= 1000`，Aevia 生成的
   `Rssn` 模块覆盖 100%；无签名算子有 `dynamic_reason`。
2. **零手写绑定**：Aevia 仓库没有按算子名手写的绑定文件；只有生成器和
   overlay。
3. **动态调用冒烟**：对每个 rule set 至少 1 个算子做 `call` 往返；
   1000 个算子的 manifest 加载 < 50 ms（release）。
4. **量纲往返**：`DimTerm -> Dimension -> DimTerm` 对随机 10k 量纲相等；
   `UnitRegistry` 对每个单位 `scale`/`offset` 精确往返。
5. **单位文本安全**：`"L"` 不再被解析为升作为量纲；`"Length/Time"`
   正确；`"m/s"` 仍作为单位文本。
6. **重载**：同名不同 arity/类型可注册；Aevia 重载解析无歧义。
7. **插件**：示例插件注册一个算子 + 一个重写规则 + 一个单位；Aevia 不需
   重新编译即可调用。
8. **线程**：`fork/merge` 的符号结果与串行一致；`DimArena` 无跨请求污染。
9. **JIT**：带 `if`/索引的函数可 JIT；无 lowering 的插件算子回退解释器。
10. **兼容性**：`api_version` 不匹配时 Aevia 给出明确错误；`manifest_hash`
    变化使缓存失效但不误用旧签名。

---

## 8. 取舍与风险

- **反射优先 vs 生成代码**：反射让新算子零成本可见，但动态调用有开销。
  解决：类型化 façade + JIT；动态路径只用于未签名/实验性/插件算子。
- **manifest 体积**：1000+ 算子 + 签名/文档可能几 MB。解决：CBOR + 压缩；
  Aevia 只加载编译所需子集，LSP 可懒加载。
- **签名推断错误**：默认 `Scalar -> Scalar` 可能把张量算子标错。解决：
  overlay + CI 审计 + 运行时 `Value` 检查；错误签名只影响静态体验，不
  影响动态调用。
- **`DimTerm` 公开**：会暴露内部表示。解决：只公开结构化访问器和
  `DimArena` 快照，不暴露 `DimRepr`。
- **插件安全**：动态库可执行任意代码。解决：插件签名/沙箱/能力声明；
  Aevia 默认只加载受信任 registry 的插件。
- **不按 v1/v2 分层**：实现压力大。解决：capability 诊断 + 动态 fallback，
  语言设计一次到位，后端按 K0–K6 补齐。
