# RSSN 集成需求（Aevia ↔ RSSN）

> 状态：**契约 + 现状**。RSSN 0.2.13 已经实现 Kernel Manifest、动态
> `Value`/`call_value`、签名/重载、插件、`rssn::dim`、`Term` 遍历/替换、
> `fork/merge`、结构化结果和 JIT lowering。本文现在记录**实际 API**与
> **剩余缺口**，不再描述“尚未实现的需求”。
>
> 当前唯一 P0 构建阻塞是 `signatures.overlay.json` 被 `.gitignore` 排除，
> 干净 clone 下 `include_str!` 失败；其余 P1/P2 是性能/覆盖项。
>
> 本文不按“v1/v2”分期。P0/P1/P2 是**实现依赖顺序**，不是语言版本；
> Aevia 不会因为某项未完成就砍掉对应语言能力，而是让该能力在运行时报
> `E1001 backend capability missing`，并在编译期给出明确的 capability 诊断。

---

## 0. 结论

Aevia 不再为 RSSN 的每个算子手写 Rust/C 绑定。采用三层结构：

1. **RSSN 是唯一算子注册表。** 每个 rule set 在注册算子时同时提供
   **签名、量纲规则、文档、稳定性、别名、重载**。RSSN 在启动时生成
   **Kernel Manifest**（确定性 bincode/CBOR；JSON 仅供人读），描述全部 rule set、算子、单位、基量纲、
   能力和版本。
2. **Aevia 编译器消费 manifest，自动生成 `Rssn` 模块。** 有签名的算子生成
   类型化声明；没有签名的算子生成 `Dynamic` 声明，仍可调用。Aevia 仓库里
   没有“算子绑定表”，只有生成器和少量手写策略。
3. **动态调用是正式能力，不是降级路径。** `Rssn.call("op", args)` 接受
   `Value` 数组，返回 `Result<Value, OpError>`；类型化 façade 只是它的
   静态视图。新算子只要在 RSSN 注册，Aevia 下一次构建就能看见；不需要改
   Aevia 编译器。

量纲/单位同样只有一套：RSSN 的 `rssn::dim`（`graph/dim.rs` + `rules/units.rs` 的公开视图）是运行时与编译期共享的规范层；Aevia 的 `Dim` 是它
的类型级镜像，`Units.SI` 由 `UnitRegistry` 生成，不再维护第二张单位表。

---

## 1. 现状核查（2026-11-01，已对照源码与 manifest）

RSSN 侧的适配已经落地，本文从“需求”改为“契约 + 现状”。实际版本与审计结果：

```
rssn 0.2.13 · api 1
1369 operators in 30 rule sets
799 signed (58.4%; 7 declared, 792 inferred), 570 explained, 0 unexplained
declared: 7   inferred: 792   overlay: 0
units: 59   base dimensions: 7
```

- **Kernel Manifest 已实现**：`Session::manifest() -> Arc<KernelManifest>`，
  `manifest_json()` 供人读，`manifest_hash: [u8;32]` 是确定性 BLAKE3。
- **动态 ABI 已实现**：`Session::call_value` / `call_ho`，`Value`/`OpError`
  是公开类型；`Kernel` trait 也有 `call`/`call_ho`/`register`。
- **签名/重载/稳定键已实现**：`OpInfo.keys`/`overloads`、
  `OpDescriptor.overloads`、`OpKey { rule_set, name, signature_hash }`；
  `signature_audit` 报告 0 个“既无签名又无解释”的算子。
- **插件已实现**：`Session::register(Box<dyn Plugin>)`、`Plugin`/
  `HostCallback`/`Registry`；`SimplePlugin` 可注册算子/规则/单位。
- **量纲/单位已统一**：`rssn::dim` 公开 `DimTerm`/`DimArena`/
  `Dimension`/`UnitRegistry`/`UnitDef`/`Scale`；`manifest.units` 是
  `Vec<UnitValue>`（不是 `UnitDef`），携带显示名、`DimExpr`、精确
  `ScaleValue`、仿射 offset。
- **Term/线程已补**：`Term::children`/`substitute`/`free_symbols`/
  `to_value`/`handle`；`TermRef` 是 owned handle；`Session::fork`/`merge`。
- **结构化结果已实现**：`Solutions`、`OdeSolutionView`、`SimResult` 是
  `Value::Record` + `FlattenSchema`；`solve_structured` 是便捷入口。
- **JIT**：`Select`/`Extern1`/`Gather` 已 lowering；但**默认构建**
  `features.jit = false`（未开 `jit` feature），所以 manifest 当前报告
  `jit_branch=false`、`jit_index=false`。这不是接口缺失，是构建配置。

### 1.1 必须修的构建问题

`src/kernel/manifest.rs` 用
`include_str!("../../signatures.overlay.json")`，但该文件被 `.gitignore`
的 `**/*.json` 忽略，干净 checkout 里不存在，`cargo build` 直接失败：

```
error: couldn't read `src\kernel\../../signatures.overlay.json`
```

为了让审计跑起来，本次在 RSSN 根目录补了一个最小
`signatures.overlay.json`（`{"ops":{}}`），但它是 ignored 的。RSSN 必须
二选一：

1. 把 overlay 文件纳入版本控制（`.gitignore` 加
   `!signatures.overlay.json` 并 `git add -f`），或
2. `build.rs` 在缺失时写一个空的 `$OUT_DIR/signatures.overlay.json`，再用
   `include_str!(concat!(env!("OUT_DIR"), "/signatures.overlay.json"))`；
   或去掉 `include_str!`，运行时读 `CARGO_MANIFEST_DIR` 路径并在缺失时用
   空 overlay。`include_str!` 本身没有“可选”形态，不能直接 `unwrap_or`。

在这之前，**干净 clone 不可构建**，Aevia 的生成器无法工作。这是当前唯一
的 P0 构建阻塞。

### 1.2 类型覆盖率

- 799/1369（58.4%）有签名，但其中只有 **7 个 declared**、792 个
  inferred、0 overlay。推断签名默认 `(Real,...,Real) -> Real`，**不可信**；
  Aevia 的可信 typed façade 只有 7 个，792 个必须走 `Rssn.dynamic`。
- 570 个无签名，但 `dynamic_reason` 明确：`request`（549）、`variadic`
  （20）、`fully_polymorphic`（1）。它们也走 `Rssn.dynamic`。
- `overlay: 0` 说明当前所有签名来自声明或推断；`signatures.overlay.json`
  目前是空的。后续人工修正应写进这个文件，而不是改推断器。

Aevia 的生成器必须同时处理两种：有签名 → `Rssn.<name>`；无签名 →
`Rssn.dynamic`。不能因为 58.4% 就只生成一半。

### 1.3 剩余 P1 集成缺口（不是构建阻塞，但影响能力）

1. **重载运行时只按 arity。** `Session::call_value(name, args)` 用
   `lookup_arity` 取第一个匹配；`OpKey.signature_hash` 不参与运行时选择。
   Aevia 的类型化 façade 可能静态选中一个重载、运行时执行另一个。需要
   RSSN 增加 `call_key(&OpKey, ...)` 或 `call_with_signature`；在它存在前，
   Aevia 只对 `Declared`/`Overlay` 且同 arity 无歧义的签名生成 typed 声明。
2. **`manifest.units` 是 `UnitValue`，缺 `aliases`/`prefixable`/`category`/
   `since`。** 生成 `Units.SI` 的别名/前缀需要 `UnitRegistry`，不能只读
   manifest；这仍是“单一事实来源”，但生成器要多读一个接口。
3. **`UnitRegistry::register` 只认 7 个内置基名。** `unit EUR : Currency`
   这类自定义基量纲单位在 0.2.13 报 “not a base dimension”；需要
   `expr_to_dim` 接受 `DimArena` 的全部基名，或 `user_base_dimensions`
   降为 false。
4. **Astronomy/CGS/Constants 不在 RSSN 单位表。** `au/pc/ly/Mpc`、
   `dyn/erg/Gal/P` 和 CODATA 常数没有注册；`Units.Astronomy` 等模块不能
   从 manifest 生成，需要 RSSN 注册或明确标为 Aevia 本地扩展。
5. **`Config::dimension_term` 不存在。** 只有 `dimension(symbol, text)`；
   Aevia 静态路径不传量纲，FFI 用全名文本。若要让“结构化优先”成立，
   RSSN 需要增加 `dimension_term`。
6. **`Value` 的部分变体不能入项图。** `Sparse`/`Record`/`Variant`/
   `Function`/`Opaque` 在 `term_of_value` 返回 `Unsupported`；它们只能
   喂给 `eval_dyn` 算子或作为返回值。Aevia 生成器必须标注每个变体的
   “可入项图/仅 eval_dyn/仅返回”，否则 `Sparse`/`Solutions` 章节会误导。
7. **`Solutions.complete` 硬编码 true，`OdeSolutionView.dimensions` 在
   `to_value` 中丢弃。** Aevia 只能把 `complete` 当“求解器未报不完整”，
   不能当证明；`dimensions` 需要从结构化结果直接读，不能靠 `Value`。
8. **插件 `cdylib` ABI 未实现。** 当前只有进程内 Rust `Plugin` trait；
   `kind = "cdylib"` 是设计目标，未实现前 `Rssn.register` 只接受进程内
   插件，动态库加载报 `E1001("core.plugin.cdylib")`。
9. **符号参数没有 `TypeExpr`。** `Value::Symbol` 对应
   `TypeExpr::Var("Symbol")`，但没有 `type_params` 绑定；`diff`/`solve`
   等以符号为参数的算子无法生成精确 typed 声明，只能 `dynamic`。

---

## 2. 目标架构（与实现对齐）

```
Aevia source
   │  typecheck (Dim = type-level mirror of rssn::dim::DimTerm)
   ▼
Core IR ──► Aevia runtime ──► rssn::kernel ──► RSSN
   │                            │
   │                            ├─ Session::manifest()      Arc<KernelManifest>
   │                            ├─ Session::call_value(name, args, ctx)
   │                            ├─ Session::call_ho(name, args, fns, ctx)
   │                            └─ Session::register(Box<dyn Plugin>)
   ▼
generated `Rssn` module
   ├─ typed declarations (from OpInfo.overloads)
   └─ dynamic declarations (OpInfo.dynamic_reason)
```

三条硬规则不变：

1. **Aevia 不维护算子清单。** `Rssn` 由 manifest 生成；Aevia 仓库只有
   生成器和 `signatures.overlay.json` 的**只读镜像**（用于补 RSSN 暂时缺的
   签名，最终仍应回写 RSSN）。
2. **Aevia 不维护单位表。** `Units.SI` 从 `UnitRegistry`/manifest 生成；
   用户 `dimension`/`unit` 注册进同一个 registry。
3. **动态调用永远可用。** `Rssn.dynamic` 不是降级路径；它是 570 个
   request/variadic/全多态算子的正式入口。

---

## 3. RSSN 契约（实际 API）

以下签名按 `rssn 0.2.13` 源码整理；Aevia 生成器按这些名字生成，不再使用
早期草案里的 `register_plugin`、`OpInfo.key`、`Value::VFloat` 等名字。

### 3.1 Kernel Manifest

```rust
pub const API_VERSION: u32 = 1;

pub struct KernelManifest {
    pub rssn_version: String,
    pub api_version: u32,
    pub manifest_hash: [u8; 32],       // BLAKE3 over deterministic bincode
    pub features: Features,
    pub rule_sets: Vec<RuleSetInfo>,
    pub ops: Vec<OpInfo>,
    pub units: Vec<UnitValue>,         // 不是 UnitDef
    pub base_dimensions: Vec<BaseDimInfo>,
    pub plugins: Vec<PluginInfo>,
}

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
    pub structured_results: bool,
    pub higher_order: bool,
}
```

`RuleSetInfo`：

```rust
pub struct RuleSetInfo {
    pub name: String,
    pub version: String,
    pub stability: Stability,
    pub since: String,
    pub capabilities: Vec<String>,
    pub ops: Vec<String>,              // qualified OpKey strings
}
```

`OpInfo`（实际字段名）：

```rust
pub struct OpInfo {
    pub keys: Vec<OpKey>,              // 每个 overload 一个；keys[0] 是主重载
    pub name: String,
    pub aliases: Vec<String>,
    pub category: String,
    pub arity: ArityInfo,
    pub overloads: Vec<Signature>,
    pub signature_source: SignatureSource, // Declared | Inferred | Overlay | None
    pub flags: OpFlagsInfo,
    pub binder: Option<BinderInfo>,
    pub cost: u32,
    pub eval: EvalKind,                // None | Scalar | Dynamic
    pub higher_order: bool,
    pub purity: Purity,
    pub determinism: Determinism,
    pub thread_safety: ThreadSafety,
    pub stability: Stability,
    pub since: String,
    pub doc: String,
    pub examples: Vec<Example>,
    pub dim_rule: Option<DimRuleInfo>,
    pub backend_support: BackendSupport,
    pub dynamic_reason: Option<String>, // request/variadic/fully_polymorphic
    pub rule_set: String,
    pub jit: Option<String>,           // lowering 名，None 由后端决定
}
```

`manifest_hash` 的确定性规则：列表排序、无时间戳/指针/arena id/hash-map
顺序；哈希覆盖确定性 bincode，`manifest_hash` 字段本身清零。`to_json()` 只
用于人读，不是哈希输入。

### 3.2 动态 Value/Call ABI

```rust
pub enum Value {
    Int(BigInt),
    Real(Number),                      // 精确有理数或符号实数
    Float(f64),
    Complex(ComplexValue),             // Exact { re, im } | Float { re, im }
    Bool(bool),
    String(String),
    Symbol(String),                    // 自由符号，不是 String
    Unit(UnitValue),
    Quantity(QuantityValue),
    Tensor(TensorValue),               // shape + Vec<Value>，行主序
    Sparse(SparseValue),
    Record(RecordValue),
    Variant(VariantValue),
    Term(TermRef),                     // owned handle，跨会话会被拒绝
    Function(CallbackRef),
    Dyn(DynValue),
    Opaque(OpaqueValue),
}
```

`Session` 的实际入口：

```rust
impl Session {
    pub fn manifest(&self) -> Arc<KernelManifest>;
    pub fn manifest_json(&self) -> String;
    pub fn call_value(&self, name: &str, args: &[Value], ctx: &CallCtx<'_>)
        -> Result<Value, OpError>;
    pub fn call_ho(&self, name: &str, args: &[Value], fns: &[CallbackRef],
                   ctx: &CallCtx<'_>) -> Result<Value, OpError>;
    pub fn value_of(&self, term: Term<'_>) -> Result<Value, OpError>;
    pub fn term_of(&self, value: &Value) -> Result<Term<'_>, OpError>;
    pub fn solve_structured(&self, equation: Term<'_>, variable: Term<'_>)
        -> Result<Solutions, OpError>;
    pub fn register(&mut self, plugin: Box<dyn Plugin>)
        -> Result<PluginId, PluginError>;
    pub fn fork(&self) -> Session;
    pub fn merge(&mut self, other: &Session) -> Result<(), MergeError>;
}
```

`CallCtx` 的实际字段：

```rust
pub struct CallCtx<'a> {
    pub target: Target,                // Symbolic | Numeric { tolerance }
    pub budget: Budget,
    pub dimension_mode: DimensionMode, // Off | Advisory | Strict
    pub assumptions: Vec<Assumption>,
    pub bindings: Vec<(String, f64)>,
    pub substitutions: Vec<(String, Value)>,
    pub callbacks: Vec<Arc<dyn HostCallback>>,
    pub trace: Option<&'a RefCell<Trace>>,
}
```

`OpError` 的实际变体（Aevia 必须映射，不能只匹配旧草案的子集）：

```rust
pub enum OpError {
    UnknownOp(OpKey),
    Arity { expected: ArityInfo, got: usize },
    Type { expected: Box<TypeExpr>, got: Box<TypeExpr>, arg: usize },
    Dimension(DimError),
    NotNumeric(String),
    NotReduced(String),
    Domain(String),
    BudgetExceeded,
    Unsupported { backend: String, feature: String },
    Plugin(PluginError),
    ForeignTerm { expected: SessionId, got: SessionId },
    Codec(CodecError),
    Host(String),
    Internal(String),
}
```

### 3.3 签名、重载、稳定键

`Signature` 的实际字段：

```rust
pub struct Signature {
    pub type_params: Vec<TypeParam>,
    pub params: Vec<TypeExpr>,
    pub ret: TypeExpr,
    pub constraints: Vec<Constraint>,
    pub effects: EffectRow,
    pub dim: Option<DimRuleInfo>,
    pub variadic: Option<Variadic>,
}
```

`TypeExpr` 的实际变体（注意 `Dim` 是包装，不是 `Real(dim)`）：

```rust
pub enum TypeExpr {
    Int, Real, Float, Complex, Bool, String,
    Unit(DimExpr), Point(DimExpr),
    Dim(Box<TypeExpr>, DimExpr),       // Real[Length] 的表示
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
    Never,
}
```

`OpKey`：

```rust
pub struct OpKey {
    pub rule_set: String,
    pub name: String,
    pub signature_hash: [u8; 16],      // 无签名时全零
}
```

`OpDescriptor` 已扩展（实际字段）：`eval_dyn: Option<Arc<dyn EvalDyn>>`、
`overloads: Vec<Signature>`、`doc`、`category`、`aliases`、`stability`、
`since`、`purity`、`determinism`、`thread_safety`、`dynamic_reason`、
`examples`、`backend`、`rule_set`、`overload: bool`、`higher_order`、
`jit: Option<Lowering>`。

**重载**：`OpTable` 允许同名多签名；`OpInfo.keys` 每个 overload 一个；
`OpKey` 是持久化/缓存键，`OpId` 只在本进程内有效。

### 3.4 插件与宿主回调

实际入口是 `Session::register`，不是 `register_plugin`：

```rust
pub trait Plugin: Send + Sync {
    fn manifest(&self) -> PluginManifest;
    fn register(&self, reg: &mut Registry<'_>) -> Result<(), PluginError>;
    fn call(&self, op: &OpKey, args: &[Value], ctx: &CallCtx<'_>)
        -> Result<Value, OpError> { ... }
}

pub trait HostCallback: Send + Sync {
    fn call(&self, args: &[Value]) -> Result<Value, OpError>;
    fn signature(&self) -> Signature;
}

impl Registry<'_> {
    pub fn register_op(&mut self, desc: OpDescriptor) -> Result<OpId, OpError>;
    pub fn register_rewrites(&mut self, tier: Tier, texts: &[&str]) -> Result<(), OpError>;
    // 以及 register_unit / register_dim_rule / ...
}
```

`SimplePlugin` 是便捷包装；`NativeCallback<F>` 把闭包包成 `HostCallback`。
`Registry` 拒绝“既无签名又无 `dynamic_reason`”的注册，这是 Aevia 生成器
必须遵守的契约。

### 3.5 量纲/单位（实际 `rssn::dim`）

`DimTerm` 的实际访问器：

```rust
impl DimTerm {
    pub fn factors(&self, arena: &DimArena) -> Vec<(String, BigRational)>;
    pub fn vars(&self, arena: &DimArena) -> Vec<(String, BigRational)>;
    pub fn symbolic_factors(&self, arena: &DimArena) -> Vec<(String, String)>;
    pub fn mul(&self, other: &Self) -> Self;
    pub fn pow(&self, exponent: &BigRational) -> Self;
    pub fn inverse(&self) -> Self;
    pub fn to_dimension(&self, arena: &DimArena) -> Dimension;
    pub fn from_dimension(dimension: &Dimension, arena: &DimArena)
        -> Result<Self, DimError>;
}
```

`DimArena` 的实际入口：`base_names()`、`define_base()`/`intern()`、
`snapshot()`、`restore()`。`DimExpr` 是类型级镜像，提供
`to_term(&mut DimArena)` / `from_term(&DimTerm, &DimArena)`；这是 Aevia
静态 `Dim` 与运行时 `DimTerm` 的桥。

`UnitRegistry` 的实际 trait：

```rust
pub trait UnitRegistry {
    fn list(&self) -> Vec<UnitDef>;
    fn lookup(&self, name: &str) -> Option<UnitDef>;
    fn resolve(&self, expr: &str) -> Result<ResolvedUnit, DimError>;
    fn register(&mut self, unit: UnitDef) -> Result<(), DimError>;
    fn format(&self, dim: &DimTerm, value: &Value) -> String;
    fn snapshot(&self) -> UnitRegistrySnapshot;
}
```

`UnitDef` 字段：`name`、`aliases`、`dimension: DimExpr`、`scale: Scale`、
`offset: Option<BigRational>`、`prefixable`、`affine`、`display`、
`category`、`since`。`Scale { rat: BigRational, pi: i32 }` 是精确的。

`KernelManifest.units` 是 `Vec<UnitValue>`，字段为
`name`/`display`/`dimension: DimExpr`/`scale: ScaleValue`/
`offset: Option<ScaleValue>`/`affine`；`ScaleValue` 用
`numer: BigInt`、`denom: BigInt`、`pi: i32`。Aevia 生成 `Units.SI` 时读
manifest 的 `UnitValue`，不读 `UnitDef`。

**文本入口**：`Config::dimension(symbol, text)` 仍存在，但 Aevia 静态路径
应优先用 `DimExpr`/`DimTerm` 结构化入口；文本只用于 FFI/人类输入。`L` 是
升、`T` 是特斯拉，禁止单字母量纲名。

### 3.6 Session/Term/线程

`Term<'s>` 仍借 `&Session`；`TermRef` 是 owned `(SessionId, node)`：

```rust
impl Term<'_> {
    pub fn id(&self) -> u32;
    pub fn children(&self) -> Vec<Term<'_>>;
    pub fn substitute(&self, map: &[(Term<'_>, Term<'_>)]) -> Term<'_>;
    pub fn free_symbols(&self) -> Vec<String>;
    pub fn to_value(&self) -> Result<Value, OpError>;
    pub fn handle(&self) -> TermRef;
    pub fn dimension(&self) -> Dimension;
}
```

`Session::fork`/`merge` 已实现；`merge` 导入节点并保留 handle 映射。
`DimArena` 仍是共享 `Arc<RwLock>`，但 `fork` 克隆模板；请求期
`define_base_dimension` 的污染问题仍需在 Aevia 侧避免（只在启动期注册）。

### 3.7 JIT

实际 lowering 已包含 `Select`、`Extern1`、`Gather`；默认构建
`features.jit=false`，开启 `jit` feature 后 `jit_branch/jit_index` 为 true。
Aevia 生成器必须读 `Features`，不能假设 JIT 可用；`@jit` 在
`features.jit=false` 时报 `E1001("core.jit")`。

### 3.8 结构化结果

`Solutions { variable, roots, multiplicities, conditions, complete }`；
`multiplicities` **空表示未报告**，不是“全 1”。`OdeSolutionView` 有
`t/y/flatten_order/events/nfev/rejected/dimensions`；`SimResult` 有
`scenario/t/fields`。它们都是 `Value::Record` + `FlattenSchema`。
Aevia 的 `Solutions` 应保留 `multiplicities` 空语义，不要伪造 1。

---

## 4. Aevia 侧如何消费

### 4.1 生成 `Rssn` 模块

构建时：

1. 读取 `KernelManifest`。
2. 对每个 `OpInfo`：
   - `signature_source = Declared | Overlay` → 生成类型化声明
     `Rssn.<name>(...)`；
   - `signature_source = Inferred | None` → 生成 `Rssn.dynamic("name", args)`；
   - 有别名 → 生成别名，冲突时加 `Rssn.<rule_set>.<name>`。
3. 生成 `Rssn.Capabilities` 常量、`Rssn.Version`、`Rssn.ManifestHash`。
4. 生成文档/补全/签名存根（LSP）。

Aevia 仓库只保留：

```
tools/gen_rssn_module.aev   # 生成器
signatures.overlay.json     # 仅补 RSSN 缺的签名
```

### 4.2 动态 fallback

```aevia
# 只有 Declared/Overlay 才是 typed；sin 是 Inferred -> dynamic
let y = Rssn.dynamic("sin", [to_value(x)])

# 无签名/实验性：动态调用，运行时检查
let z = Rssn.dynamic("meijer_g", [to_value(a), to_value(b), to_value(z)])

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
- **运行时限制**：0.2.13 的 `call_value` 只按 name+arity 选第一个重载；
  同 arity 多型不生成 typed 声明，只走 `dynamic`，等 `call_key`。

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
| K0 ✅ | Manifest + `Value` + `call_value` + `OpTable::iter` 已实现 | 生成 `Rssn` 动态模块；1369 算子全部可见 |
| K1 ✅ | 签名/重载/覆盖审计已实现；799 有签名（7 declared/792 inferred）、570 explained、0 unexplained | 可信 typed façade 只有 7 个；其余 1362 走 dynamic |
| K2 ✅ | `rssn::dim` + `UnitRegistry` + 结构化 `DimTerm` 已实现 | Aevia 单位/量纲统一；用户单位/量纲 |
| K3 ✅ | `Term` 遍历/替换/自由符号 + `TermRef` + `fork/merge` 已实现；COW 仍待优化 | `diff` 闭包、并行 |
| K4 ✅ | 插件/宿主回调已实现；动态库 ABI 仍需签名/沙箱 | 用户自定义算子、Aevia 函数进 RSSN |
| K5 ◐ | `Select`/`Extern1`/`Gather` 已 lowering；默认构建 `jit=false` | 开启 feature 后分支/索引 JIT；否则 `E1001` |
| K6 ✅ | `Solutions`/`OdeSolutionView`/`SimResult` 已实现 | 完整结果类型 |

任何阶段未完成时，Aevia 仍可运行：对应算子走动态 fallback；对应能力报
`E1001`（带 capability 名），不删语言特性。

---

## 7. 验收标准

1. **1000+ 算子可见性**：当前 `manifest.ops.len() == 1369`；Aevia 生成的
   `Rssn` 模块覆盖 100%（7 trusted typed + 1362 dynamic）；无签名算子都有
   `dynamic_reason`，`signature_audit` 的 `unexplained == 0`。
2. **零手写绑定**：Aevia 仓库没有按算子名手写的绑定文件；只有生成器和
   overlay。
3. **动态调用冒烟**：对每个 rule set 至少 1 个算子做 `call_value` 往返；
   1369 个算子的 manifest 加载 < 50 ms（release；待基准确认）。
4. **量纲往返**：仅对纯有理指数，经 `Dimension::exact_factors` 做
   `DimTerm -> Dimension -> DimTerm` 随机 10k 相等；符号指数不支持往返。
   `UnitRegistry` 对每个单位 `scale`/`offset` 精确往返。
5. **单位文本安全**：`"L"` 不再被解析为升作为量纲；`"Length/Time"`
   正确；`"m/s"` 仍作为单位文本。
6. **重载**：同名不同 arity/类型可注册；Aevia 重载解析无歧义。
7. **插件**：示例插件注册一个算子 + 一个重写规则 + 一个单位；Aevia 不需
   重新编译即可调用。
8. **线程**：`fork/merge` 的符号结果与串行一致；`DimArena` 无跨请求污染。
9. **JIT**：开启 `jit` feature 后，带 `if`/索引的函数可 JIT；默认构建
   `features.jit=false`，`@jit` 报 `E1001("core.jit")`。
10. **兼容性**：`api_version` 不匹配时 Aevia 给出明确错误；`manifest_hash`
    变化使缓存失效但不误用旧签名。

---

## 8. 取舍与风险

- **反射优先 vs 生成代码**：反射让新算子零成本可见，但动态调用有开销。
  解决：类型化 façade + JIT；动态路径只用于未签名/实验性/插件算子。
- **manifest 体积**：1369 算子 + 签名/文档约 2.2 MB JSON。解决：CBOR/bincode；
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
