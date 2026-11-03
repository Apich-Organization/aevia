# RSSN 集成需求（Aevia ↔ RSSN）

> 状态：**契约 + 现状**（对应 `rssn 0.2.13`，commit `95099fa0`）。
> RSSN 已经实现 Kernel Manifest、动态 `Value`/`call_value`/`call_key`、
> 重载解析（`kernel::overload`）、**100% 签名覆盖**（`Declared`/`Overlay`/
> `Derived` 三种来源）、物理常数表、插件（含 cdylib ABI）、`rssn::dim`、
> 自定义基量纲、`Config::dimension_term`、`Term` 遍历/替换、`fork/merge`、
> 结构化结果和 JIT lowering。本文记录**实际 API**与**剩余缺口**。
>
> 上一轮的 P0 构建阻塞（`signatures.overlay.json` 被忽略导致干净 clone
> 不可构建）已在 `0852123a` 关闭。当前没有 P0；剩余项是精度、性能与
> 构建配置级。
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
2. **Aevia 编译器消费 manifest，自动生成 `Rssn` 模块。** 1369 个算子现在
   **全部有签名**：`Declared`（20，含 13 条 overlay）是精确的，
   `Derived`（1349）是“由注册信息推出”的保守签名。两者都可以生成类型化
   声明；只有 `SignatureSource::None`（叶子/无调用）才只能动态。Aevia
   仓库里没有“算子绑定表”，只有生成器和 `signatures.overlay.json` 的
   只读镜像。
3. **动态调用是正式能力，不是降级路径。** `Rssn.call("op", args)` 接受
   `Value` 数组，返回 `Result<Value, OpError>`；类型化 façade 只是它的
   静态视图。新算子只要在 RSSN 注册，Aevia 下一次构建就能看见；不需要改
   Aevia 编译器。

量纲/单位同样只有一套：RSSN 的 `rssn::dim`（`graph/dim.rs` +
`rules/units.rs` 的公开视图）是运行时与编译期共享的规范层；Aevia 的 `Dim`
是它的类型级镜像，`Units.*` 由 `manifest.units` 生成，不再维护第二张单位表。

重载也只有一套：`kernel::overload` 的 `Match` 格
（`Exact > Widened > Shaped > Generic > No`）是唯一判定规则；Aevia 的
静态解析必须与它同构；运行时用 `call_key` 落到生成时的 `OpKey`，或用
`resolve_overload` 拿 key（只有 `Best::Unique` 的结果可再交给 `call_key`）。

---

## 1. 现状核查（2026-11-01，commit `95099fa0`，已对照源码与 manifest）

RSSN 侧的适配已经落地，本文从“需求”改为“契约 + 现状”。实际版本与审计结果：

```
rssn 0.2.13 · api 1
manifest_hash (32B) = 98fd35fab1ab3bf0b7e65e4bd81bc3e7
                      0590bab0ef2443826a3494a5f2bbba16
                      # KernelManifest::hash_hex() 给出这 64 个 hex 字符
                      # 显示用 hash_u64 = f03babb1fa35fd98（前 8 字节小端）
1369 operators in 30 rule sets
1369 typed (100.0%), 0 explained, 0 unexplained
  declared: 20 (7 declared + 13 overlay)   derived: 1349   overlay: 13
  units: 75   base dimensions: 7   constants: 30 (18 exact)   plugins: 0
  plugin kinds: in-process   value kinds: 17 (11 in term graph)
features: jit=false, jit_branch=false, jit_index=false,
          dynamic_call=true, plugin_abi=1, higher_order=true,
          exact_units=true, affine_units=true,
          user_base_dimensions=true, structured_results=true
overloads per op: 1 -> 1190, 2 -> 177, 3 -> 2
```

- **Kernel Manifest 已实现**：`Session::manifest() -> Arc<KernelManifest>`，
  `manifest_json()` 供人读，`manifest_hash: [u8;32]` 是确定性 BLAKE3，
  `hash_hex()` 给出 64 个 hex 字符（宿主应该 pin 这个），`hash_u64()` 是
  前 8 字节的小端显示值。manifest 还有 `value_kinds`、`constants`、
  `features.plugin_kinds`。
- **动态 ABI 已实现**：`Session::call_value` / `call_ho` / `call_key`，
  `Value`/`OpError` 是公开类型。`95099fa0` 起两条重要修正：
  - `call_value` 会调 `check_args`：没有 overload 接受时返回
    `OpError::Type`。但**这个检查很弱**——`Derived` 的符号重载参数是
    `Dyn<term>`，而 `Dyn` 对任何值都算 `Generic`，所以 1349 个 derived
    算子实际上接受任何参数（`sin("x")` 会返回一个 `Term`）。它只在“所有
    签名都是具体类型”的少数算子上真正生效（如 `eq`/`lit`/`pow`）；
  - `Kernel::call(&OpKey, …)` 改为转发 `call_key`，**钉死**重载，不再只按
    name 分发。
- **签名覆盖 100%**：`SignatureSource` 现在是
  `Declared | Derived | Overlay | None`（`Inferred` 已改名 `Derived`）。
  `Derived` 不是猜名字，而是从注册信息推：有 `eval` → 数值重载
  `(Real,…) -> Real`（谓词 `-> Bool`）；非叶子算子另有一条符号重载
  `(Term,…) -> Term`；`Binder` 位置的参数是 `Symbol`；变长参数是
  `Variadic`。因此 1369 个算子全部可生成 typed 声明。
- **重载解析真正生效了**：177 个算子有 2 条重载（数值 + 符号），2 个有 3
  条；`OpInfo.keys` 与 `overloads` 一一对应。`resolve_overload` 在
  “数值重载 / 符号重载”之间选，**并列时返回 `OpError::Ambiguous`**（不再
  返回不可执行的零 hash key）；只有完全没有签名的算子才给零 hash。
  注意：实参是 `Term`/`Dyn` 时两条重载都会以 `Generic` 通过，所以符号
  表达式上 `resolve_overload` 会歧义——那正是 `OpInfo.keys` 存在的理由。
- **物理常数已进 manifest**：`constants: Vec<ConstantValue>`，30 条、18 条
  精确；字段 `name`/`symbol`/`value`/`dimension`/`unit`/`exact`/
  `uncertainty`/`source`。`Units.Constants` 终于可以从 manifest 生成。
- **插件 ABI 完整**：`PluginKind { InProcess, Cdylib }`，
  `kernel::cdylib`（`ENTRY_SYMBOL = rssn_plugin_v1`、`RssnBuf`/
  `RssnHost`/`RssnOpEntry`/`RssnPluginDesc`、bincode 字节缓冲 ABI），
  `Session::load_plugin(path)` 与 `cdylib::load(path)`，由
  `plugin-cdylib` feature 控制（默认关闭；`features.plugin_kinds`
  在默认构建里是 `["in-process"]`）。
- **量纲/单位已统一**：`rssn::dim` 公开 `DimTerm`/`DimArena`/
  `Dimension`/`UnitRegistry`/`UnitDef`/`Scale`；`manifest.units` 是
  `Vec<UnitValue>`，带 `aliases`/`prefixable`/`category`/`since`。
- **自定义基量纲已支持**：`Dim` 变成“7 个 SI 基 + 有序旁表”，
  `expr_to_dim` 接受任意基名，`Config::base_dimension(name)` 把基名送进
  arena；`unit EUR : Currency` 现在可用（只要宿主声明了 `Currency`）。
- **结构化量纲入口**：`Config::dimension_term(symbol, DimExpr)`、
  `Graph::with_dim_arena_mut`（`&mut self`，写时复制）、
  `Graph::assume_dimension_value`。`DimExpr::to_term` 现在也接受符号指数
  （`Length^n`），把它 intern 成原子；但 `from_term` 会丢掉符号原子，
  所以是“写得进、读不回”。
- **Term/线程已补**：`Term::children`/`substitute`/`free_symbols`/
  `to_value`/`handle`；`TermRef` 是 owned handle；`Session::fork`/`merge`。
- **结构化结果已实现**：`Solutions`、`OdeSolutionView`、`SimResult` 是
  `Value::Record` + `FlattenSchema`。`Solutions::new` 现在默认
  `complete = false`，要声明穷尽得显式 `.complete()`；
  `solve_structured` 按 `list(...)` 形状设置它。`OdeSolutionView.dimensions`
  已写入 record。
- **ODE 事件覆盖扩展**：`Dopri5`、`Radau5`、`BDF`、`Rosenbrock23` 与
  定步长（Euler/Heun/RK4 的 `solve_fixed_events`）都能定位事件。
- **类型系统补齐**：`TypeExpr::Symbol`、`OpError::Ambiguous`。
- **JIT**：`Select`/`Extern1`/`Gather` 已 lowering；但默认构建
  `features.jit = false`，所以 manifest 报告 `jit_branch=false`、
  `jit_index=false`。这是构建配置，不是接口缺失。

### 1.1 构建阻塞：已关闭（保持关闭）

`signatures.overlay.json` 已入库，`build.rs::write_overlay` 无条件把它复制
到 `$OUT_DIR`（缺失时写空 overlay），`manifest.rs` 用
`include_str!(concat!(env!("OUT_DIR"), "/signatures.overlay.json"))`。
干净 clone 可构建，`cargo run --bin signature-audit` 可直接跑。

`111b71d0` 把 overlay 从空文件扩到 13 个算子的 28 条精确签名
（`diff`/`solve`/`taylor`/`det`/`trace`/`norm`/`transpose`/`rank`/`dot`/
`cross`/`identity`/`convert`/`quantity`；`quantity`/`convert` 各 3 条，
其余各 2 条），所以 `overlay: 13` 是真实内容，不再是 0。

### 1.2 签名质量：100% 覆盖，但精度分三档

“全部 typed”不等于“全部精确”。Aevia 必须按来源分档生成：

| 来源 | 数量 | 精度 | Aevia 的用法 |
|---|---|---|---|
| `Declared` | 7 | 精确（算子自己声明） | 直接生成 typed |
| `Overlay` | 13 | 精确（人工写） | 直接生成 typed |
| `Derived` | 1349 | **保守**：只有“数值/符号”两态 | 生成 typed，但类型是 `Real`/`Bool`/`Term` |
| `None` | 0（标准集） | 无 | 只能 `Rssn.dynamic` |

`Derived` 的保守性具体是：

- 参数一律 `Real`（数值重载）或 `Term`（符号重载），**不区分**
  `Tensor`/`Quantity`/`Int`。`det` 是 `HEAVY` 请求、没有 `eval`，所以没有
  overlay 时只有 `(Term) -> Term`——`det(matrix)` 的静态类型会是
  `Dyn<term>`，而它真正要的是 `Tensor<[n,n],Real> -> Real`。这 13 个
  overlay 算子正是为此存在。
- 符号重载的返回是 `Dyn<term>`，不是 `Real`。因此 `sin(x)` 的静态类型是
  `Dyn<term>`，不是 `Real`；要拿到数值必须保证参数是数值。
- 没有 `Quantity` 重载：`Real[L]` 会以 `Shaped`/`Generic` 落到符号重载，
  静态类型变 `Dyn<term>`。Aevia 的 `Float[L]` 精确性来自自己的类型检查，
  不是 RSSN 签名。

结论：**typed façade 覆盖 100%，但只有 20 个算子的签名是“精确”的**。
`Derived` 足够支撑“名字/arity/返回形态”的静态检查，不足以支撑
张量/量纲级别的精确签名。把 overlay 当精度杠杆，而不是当“能不能用”的
开关。

### 1.3 剩余 P1 集成缺口（不是构建阻塞，但影响能力）

1. **`Derived` 签名没有张量/量纲重载。** 见 §1.2；1349 个算子的 typed
   声明是“形态级”而非“语义级”。补 overlay 是唯一提高精度的办法。
2. **`Value` 变体的可入项图性要按 `manifest.value_kinds` 处理。**
   `Complex`（仅虚部为 0）、`Sparse`、`Record`、`Variant`、`Function`、
   `Opaque` 是 `in_term_graph = false`；只能喂 `eval_dyn` 算子或作返回值。
3. **cdylib 是 opt-in 构建。** 默认 `features.plugin_kinds = ["in-process"]`；
   用 `kind = "cdylib"` 的插件前必须确认 capability，否则报
   `E1001("core.plugin.cdylib")`。cdylib 在本进程执行任意代码，没有沙箱。
4. **常数是值，不是单位。** `manifest.constants` 给出精确值/量纲/单位，
   但它们不在 `UnitRegistry` 里，不能当单位参与 `quantity` 解析；而且
   `e`（基本电荷）与 `Math.e`（自然常数）同名，`open` 两个模块会冲突。
5. **`manifest.units` 与 `UnitRegistry` 的字段不完全等价。** 生成器以
   manifest 为准；`UnitRegistry` 只用于 `resolve`/`format`。
6. **`Solutions.complete` 不是证明。** `Solutions::new` 现在默认 `false`
   （这是对的），`solve_structured` 按 `list(...)` 形状设置；
   `multiplicities` 空仍表示未报告。Aevia 自己构造时不要无条件
   `.complete()`。
7. **符号指数：RSSN 支持，Aevia 不支持。** `DimExpr::to_term` 现在能把
   `Length^n` intern 成原子，所以桥接不再报错；但 Aevia 的**语言**仍然
   拒绝静态符号指数（§4.12.3 / §12.8.2 的设计决定）。不要因为后端能做就
   重新打开这个设计。
8. **`Session` 仍是 `RefCell`。** 跨线程要 `fork/merge`。
9. **`compute` 仍 `graph.clone()`。** `OpTable` 已是 `Arc`、`DimArena`
   写时复制，剩下的是节点表的浅拷贝；`Session::prepare` 未公开。
10. **基量纲只经 `Config` 声明。** `DimArena` 是**写时复制**：clone 共享
    arena，第一次写才分离（`with_dim_arena_mut`/`define_base_dimension`
    会先 detach）。所以 `prepare` 里的注册是隔离的；直接改 `Session` 的
    模板图才会影响后续请求。Aevia 只用 `Config::base_dimension`。
11. **`manifest_hash` 有两种形式。** `hash_hex()` 是 64 个 hex 字符
    （`98fd35fa…`），`hash_u64()` 是前 8 字节小端（`f03babb1fa35fd98`）。
    Aevia 必须 pin `hash_hex()`，不能 pin 那个 u64。
12. **版本漂移。** `api_version=1`、`rssn 0.2.13`；Aevia 必须 pin
    `api_version` + `manifest_hash`，并跑 conformance。

---

## 2. 目标架构（与实现对齐）

```
Aevia source
   │  typecheck (Dim = type-level mirror of rssn::dim::DimTerm)
   ▼
Core IR ──► Aevia runtime ──► rssn::kernel ──► RSSN
   │                            │
   │                            ├─ Session::manifest()        Arc<KernelManifest>
   │                            ├─ Session::resolve_overload(name, args) -> OpKey  # 仅 Unique 可执行
   │                            ├─ Session::call_key(&OpKey, args, ctx)      # 定死重载
   │                            ├─ Session::call_value(name, args, ctx)      # 运行时选重载
   │                            ├─ Session::call_ho(name, args, fns, ctx)
   │                            ├─ Session::overloads(name) -> [(OpKey, Signature)]
   │                            ├─ Session::register(Box<dyn Plugin>)
   │                            └─ Session::load_plugin(path)   # feature = plugin-cdylib
   ▼
generated `Rssn` module
   ├─ typed declarations (Declared|Overlay|Derived -> Rssn.<name>, 走 call_key)
   ├─ dynamic declarations (None -> Rssn.dynamic, 走 call_value)
   ├─ Rssn.Constants       (manifest.constants)
   ├─ Rssn.Units           (manifest.units)
   └─ Rssn.ValueKinds      (manifest.value_kinds)
```

三条硬规则不变：

1. **Aevia 不维护算子清单。** `Rssn` 由 manifest 生成；Aevia 仓库只有
   生成器和 `signatures.overlay.json` 的**只读镜像**（用于补 RSSN 暂时缺的
   签名，最终仍应回写 RSSN）。
2. **Aevia 不维护单位表和常数表。** `Units.*` 从 `manifest.units` 生成，
   `Units.Constants` 从 `manifest.constants` 生成（30 条，18 条精确）；
   用户 `dimension`/`unit` 注册进同一套 `DimArena`/`UnitRegistry`，
   自定义基量纲经 `Config::base_dimension` 声明。
3. **动态调用永远可用。** `Rssn.dynamic` 不是降级路径；它是插件算子、
   叶子算子（`SignatureSource::None`）和实验性入口的正式通道。

4. **静态选择与运行时执行不能分歧。** typed 声明用 `Rssn.call_key(key, …)`
   调用生成时拿到的 `OpKey`，运行时不再解析；`Kernel::call(&OpKey)` 现在
   也转发 `call_key`，所以通过 trait 对象传递同样钉死。`resolve_overload`
   只在 `Unique` 或“无签名”时返回 key，并列直接报 `Ambiguous`——它的返回值
   总能安全交给 `call_key`（§3.3.1）。

---

## 3. RSSN 契约（实际 API）

以下签名按 `rssn 0.2.13` 源码整理；Aevia 生成器按这些名字生成。早期草案
里的 `OpInfo.key`（应为 `keys`）、`Value::VFloat`（应为 `Value::Float`）
不再使用；`register_plugin` 保留为 `Session::register` 的 Aevia 别名
（见 §3.4），RSSN 侧没有这个名字。

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
    pub constants: Vec<ConstantValue>,   // 30 条物理常数（18 条精确）
    pub value_kinds: Vec<ValueKindInfo>, // 每个 Value 变体的边界契约
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
    pub plugin_kinds: Vec<String>,      // "in-process"；开了 plugin-cdylib 才有 "cdylib"
}
```

`ConstantValue`：

```rust
pub struct ConstantValue {
    pub name: String,                  // "speed of light in vacuum"
    pub symbol: String,                // "c"
    pub value: ScaleValue,             // 精确有理数，相干 SI 单位
    pub dimension: DimExpr,
    pub unit: String,                  // "m/s"
    pub exact: bool,                   // 定义值 vs 测量值
    pub uncertainty: Option<ScaleValue>,
    pub source: String,                // "SI 2019 definition" / "CODATA 2022"
}
```

`SignatureSource`：

```rust
pub enum SignatureSource {
    Declared,   // 算子自己声明
    Derived,    // 从注册信息推出（数值/符号两态）
    Overlay,    // signatures.overlay.json 人工写
    None,       // 没有签名；dynamic_reason 说明原因
}
```

`Derived` 的推导规则（`kernel::infer::derive`）：

| 注册信息 | 推出的签名 |
|---|---|
| 有 `eval` | 数值重载 `(Real, …) -> Real` |
| 有 `eval` + `PREDICATE` | 数值重载 `(Real, …) -> Bool` |
| 非叶子（任何有调用形式的算子） | 符号重载 `(Term, …) -> Term` |
| `Binder` 在第 k 位 | 第 k 个参数是 `Symbol` |
| `Arity::Variadic` | 最后一条参数变长 |
| `LEAF` | 无签名（`DynamicReason::OpaquePayload`） |

`Term` 是 `Dyn(RepExpr { repr: "term", dim: Any })`。一个有 `eval` 的算子
同时有数值和符号两条重载，这正是引擎的行为：`sin(0.5)` 折成数，
`sin(x)` 留符号。

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
    pub signature_source: SignatureSource, // Declared | Derived | Overlay | None
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

```rust
impl KernelManifest {
    pub fn hash_hex(&self) -> String;   // 64 个 hex 字符，宿主 pin 这个
    pub fn hash_u64(&self) -> u64;      // 前 8 字节的小端值，只用于 ABI/显示
}
```

两者不是同一个值：`hash_hex()` 是完整 32 字节的大端显示，
`hash_u64()` 是前 8 字节按小端读成 `u64`。

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
        -> Result<Value, OpError>;                       // 运行时解析重载
    pub fn resolve_overload(&self, name: &str, args: &[Value])
        -> Result<OpKey, OpError>;                       // 静态/缓存用
    pub fn call_key(&self, key: &OpKey, args: &[Value], ctx: &CallCtx<'_>)
        -> Result<Value, OpError>;                       // 不解析，直接执行
    pub fn overloads(&self, name: &str) -> Vec<(OpKey, Signature)>;
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
    Ambiguous { name: String, count: usize },   // 多个 overload 并列最具体
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
    Symbol,                            // 自由符号，区别于 String
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
    pub signature_hash: [u8; 16],      // 无签名时全零；`Signature::hash()` 的前 16 字节
}

impl OpKey {
    pub fn new(rule_set: &str, name: &str, signature: Option<&Signature>) -> Self;
    pub fn qualified(&self) -> String; // `rule_set.name`
}
```

`OpDescriptor` 已扩展（实际字段）：`eval_dyn: Option<Arc<dyn EvalDyn>>`、
`overloads: Vec<Signature>`、`doc`、`category`、`aliases`、`stability`、
`since`、`purity`、`determinism`、`thread_safety`、`dynamic_reason`、
`examples`、`backend`、`rule_set`、`overload: bool`、`higher_order`、
`jit: Option<Lowering>`。

**重载**：`OpTable` 允许同名多签名；`OpInfo.keys` 每个 overload 一个；
`OpKey` 是持久化/缓存键，`OpId` 只在本进程内有效。

#### 3.3.1 重载解析（`kernel::overload`，0.2.13 新增）

```rust
pub enum Match { No, Generic, Shaped, Widened, Exact }   // 有序格
pub fn matches_type(param: &TypeExpr, got: &TypeExpr) -> Match;
pub fn score(signature: &Signature, args: &[Value]) -> Option<u32>;
pub enum Best { None, Unique(usize), Ambiguous(Vec<usize>) }
pub fn best(signatures: &[Signature], args: &[Value]) -> Best;
```

- 每个参数单独分类，整个调用的分数是**最弱**的那个参数（不是平均）。
- `Int -> Real`、`Int|Real|Float -> Complex` 是 `Widened`；反向不成立。
- 同一构造器但元素/量纲未被签名钉死是 `Shaped`；类型变量和 `Dyn` 是
  `Generic`。

**实际生效范围**（`95099fa0` 起）：标准集仍是“每个名字 1 个注册”，但
**每个注册内部有 1–3 条重载**（1190 个 1 条、177 个 2 条、2 个 3 条），
因为 `Derived` 会给有 `eval` 的算子同时生成数值和符号两条。

- `Session::resolve_overload(name, args)` 在这 1–3 条里用 `best` 选；
  **并列时返回 `OpError::Ambiguous`**，不再返回零 hash 的不可执行 key。
  只有“完全没有签名”的算子才给零 hash（`call_key` 接受它）。
- `Session::call_value(name, args)` 选注册后调 `check_args`：没有任何
  overload 接受参数时返回 `OpError::Type`。**注意它的实际力度**：符号
  重载的参数是 `Dyn<term>`，`Dyn` 匹配任何值（`Generic`），所以有符号
  重载的算子几乎总能通过；只有全部签名都具体（`eq`/`lit`/`pow` 这类）
  才会真的拒绝。它**不**在数值/符号之间做选择——那由算子的 `eval` 决定。
- `Session::call_key` 不调 `check_args`：它只按 key 找注册并执行；arity
  与“值能否入项图”仍会检查。
- `Kernel::call(&OpKey, args)` 现在转发 `call_key`，**钉死**重载。

三条运行时入口，职责不同：

| 入口 | 是否解析 | 是否钉死 | 是否校验参数 |
|---|---|---|---|
| `Session::call_value(name, args)` | 注册级 arity + 跨注册评分 | 否 | 弱（`check_args`；`Dyn` 兜底几乎总通过） |
| `Session::call_key(&OpKey, args)` | 否 | 是（`rule_set` + `signature_hash`） | 否（信任调用方） |
| `Kernel::call(&OpKey, args)` | 否 | 是（转发 `call_key`） | 否 |

`call_key` 不做签名校验是有意的：它钉死的是“哪个重载”，调用方既然拿到了
那个 key，就已经承诺参数符合它。生成器从 `OpInfo.keys` 拿 key、按同一条
签名构造参数，天然满足；手写 `call_key` 才需要自己保证（arity 与值能否
入项图仍会检查）。

- 多个注册并列最高分 → `OpError::Ambiguous`；没有任何候选匹配时，
  `check_args` 返回 `OpError::Type`（只有全部候选都没有签名时才放行）。
- `resolve_overload` 的返回值可以直接交给 `call_key`：`Unique` 给带 hash
  的 key，无签名算子给零 hash key，歧义则直接 `Err(Ambiguous)`。
- `SignatureSource::None` 的算子（叶子/无调用）：arity 唯一时直接用；
  manifest 里有 `dynamic_reason`。标准集当前没有这种算子。

Aevia 侧映射：`Ambiguous` → `E1002`；`Arity`/`UnknownOp`/`Type` → `E1003`
（`Type` 可以更精确地映射成 `E0301` 一类，取决于调用点是否在动态边界）。

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

#### 3.4.1 插件种类与 cdylib ABI（`111b71d0` 新增）

```rust
pub enum PluginKind { InProcess, Cdylib }

pub struct PluginManifest {
    pub name: String,
    pub version: String,
    pub abi: u32,
    pub kind: PluginKind,        // 新增
    pub capabilities: Vec<String>,
    pub description: String,
}
```

`kernel::cdylib` 的 C ABI 是**字节缓冲**，不是 `#[repr(C)]` 的 `Value`
镜像：

```c
typedef struct { uint8_t *ptr; size_t len; } RssnBuf;

typedef struct {
    uint32_t abi;                                  /* RSSN_PLUGIN_ABI */
    void (*log)(uint32_t level, const uint8_t *msg, size_t len);  /* 可空 */
} RssnHost;

/* bincode(Vec<Value>) -> bincode(Result<Value, String>) */
typedef RssnBuf (*RssnCallFn)(void *state, const uint8_t *args, size_t len);

typedef struct {
    const char    *name;
    uint32_t       arity_min;
    uint32_t       arity_max;      /* 0xFFFFFFFF = variadic */
    const uint8_t *signatures;     /* bincode(Vec<Signature>) */
    size_t         signatures_len;
    RssnCallFn     call;
} RssnOpEntry;

typedef struct {
    uint32_t           abi;
    const char        *name;
    const char        *version;
    const char        *description;
    const RssnOpEntry *ops;
    size_t             op_count;
    void *(*init)(const RssnHost *host);          /* 可空 */
    void  (*free_buf)(RssnBuf buffer);            /* 可空；释放插件返回的缓冲 */
} RssnPluginDesc;

/* 必须导出的入口 */
const RssnPluginDesc *rssn_plugin_v1(const RssnHost *host);
```

入口符号是 `rssn_plugin_v1`；`init`/`free_buf`/`log` 都是可选函数指针，
`free_buf` 缺失时宿主无法回收插件返回的缓冲，所以实际插件都应提供它。`cdylib::load(path)` 校验 `PLUGIN_ABI`，
`Session::load_plugin(path)` = `load` + `register`。整个模块由
`plugin-cdylib` feature 控制（默认关闭，`full` 才包含），因为加载动态库
会在宿主进程里执行任意代码，没有沙箱。

Aevia 的 `[plugin] kind = "cdylib"` 必须先读
`features.plugin_kinds`；没有 `"cdylib"` 时报
`E1001("core.plugin.cdylib")`，而不是在加载器里失败。

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
`offset: Option<ScaleValue>`/`affine`/`aliases`/`prefixable`/`category`/
`since`；`ScaleValue` 用 `numer: BigInt`、`denom: BigInt`、`pi: i32`。
所以 Aevia 生成 `Units.SI` 只需要 manifest，不需要为了别名/前缀去读
`UnitRegistry`。

单位表当前 75 项：SI 基/派生、英制、`degC`/`degF`、角度、天文
（`au`/`ly`/`pc`/`Rsun`/`Msun`/`Mearth`/`jup`）、CGS
（`dyn`/`erg`/`Gal`/`P`/`St`/`G`/`Mx`/`Ba`/`Oe`）。`Oe` 的 scale 带
`pi^-1`，这正是 `Scale { rat, pi }` 存在的理由。

**结构化入口（0.2.13 新增）**：

```rust
impl Config {
    pub fn dimension_term(self, symbol: &str, dim: DimExpr) -> Self;
    pub fn base_dimension(self, name: &str) -> Self;
}
impl Graph {
    pub fn with_dim_arena_mut<R>(&mut self, body: impl FnOnce(&mut DimArena) -> R) -> R;
    pub fn assume_dimension_value(&mut self, symbol: SymbolId, term: DimTerm);
    pub fn define_base_dimension(&mut self, name: &str);
}
```

`with_dim_arena_mut` 是 `&mut self`：`DimArena` 是**写时复制**的，clone 共享
arena，第一次写才 detach（见 §3.6）。`DimExpr::to_term` 现在也接受符号
指数（`Length^n`），把它 intern 成原子；Aevia 语言仍不支持静态符号指数，
所以这条只影响桥接层。

`dimension_term` 不解析文本：`DimExpr::to_term` 遇到未知基名（`Currency`）
会把它 intern 进 arena；结构化假设在文本假设之后应用，同名时结构化胜。
`Config::dimension(symbol, text)` 仍存在，但只用于 FFI/人类输入。`L` 是升、
`T` 是特斯拉，禁止单字母量纲名。

**自定义基量纲（`111b71d0` 新增）**：

```rust
impl Config {
    pub fn base_dimension(self, name: &str) -> Self;   // 幂等
}
```

`Dim` 从 `[Exp; 7]` 变成“7 个 SI 基 + 按名排序的旁表”，`expr_to_dim`
接受任意基名；`Config::base_dimension` 负责把基名送进 `DimArena`。
因此 `unit EUR : Currency = 1.08 USD` 现在可用，前提是宿主在 `Config` 上
声明 `base_dimension("Currency")`——`prepare` 会在克隆图上定义它，
`dimension_term` 才能为这个基名产出 `DimTerm`。

### 3.5.1 物理常数

```rust
pub struct ConstantValue {
    pub name: String, pub symbol: String,
    pub value: ScaleValue, pub dimension: DimExpr, pub unit: String,
    pub exact: bool, pub uncertainty: Option<ScaleValue>, pub source: String,
}
pub mod dim::constants {
    pub struct PhysicalConstant { /* 同上，用 Scale / &'static str */ }
    pub fn all() -> &'static [PhysicalConstant];
    pub fn lookup(symbol: &str) -> Option<&'static PhysicalConstant>;
    pub fn with_dimension(dimension: &DimExpr) -> Vec<&'static PhysicalConstant>;
}
```

`manifest.constants` 是 `all()` 的 manifest 形式，按 `symbol` 排序：
30 条、18 条 `exact = true`，12 条带 `uncertainty`。**所有**值都以精确
有理数（`ScaleValue`）存储，测量值也一样——`exact` 只表示“这是定义值”，
不表示“这是 `Float`”；只有 `to_quantity` 在 `pi != 0` 时才降为 `Float`。
它们是**值**，不是单位：不能出现在 `quantity(v, u)` 的单位位置，也不在
`UnitRegistry` 里。`e` 是基本电荷（不是自然常数），`G` 是引力常数
（不是高斯——高斯只在单位表里）。

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
`DimArena` 在模板里是 `Arc<RwLock>`，`Graph::clone` **共享**它，写时才
detach（`with_dim_arena_mut`/`define_base_dimension` 先 `Arc::strong_count`
检查再克隆）。所以 `prepare` 的基量纲定义是按请求隔离的，不定义基量纲的
请求也不付拷贝成本。

### 3.7 JIT

实际 lowering 已包含 `Select`、`Extern1`、`Gather`；默认构建
`features.jit=false`，开启 `jit` feature 后 `jit_branch/jit_index` 为 true。
Aevia 生成器必须读 `Features`，不能假设 JIT 可用；`@jit` 在
`features.jit=false` 时报 `E1001("core.jit")`。

### 3.8 结构化结果

`Solutions { variable, roots, multiplicities, conditions, complete }`；
`multiplicities` **空表示未报告**，不是“全 1”。`complete` 的语义：
求解器返回 `list(...)` 时为 `true`（穷尽形式），返回单个值时是 `false`；
`Solutions::new` 默认也是 `false`，要声明穷尽必须显式 `.complete()`。
Aevia 必须保留这个区别，不能把 `complete = true` 当数学证明。

`OdeSolutionView` 有 `t/y/flatten_order/events/nfev/rejected/dimensions`；
`dimensions` 现在同时写进 record（每分量一个量纲名，未知为空串），所以
宿主读 `Value` 也能拿到，不必回 Rust 取。`SimResult` 的记录有
`scenario/t/fields/schema` 四个字段（Rust struct 只有前三个，`schema` 是
`FlattenSchema` 自描述）。`Solutions.conditions` 是 `Condition { kind, text,
expression? }` 的列表，不是字符串列表。它们都是 `Value::Record` +
`FlattenSchema`。

### 3.9 Value 变体的边界契约

`manifest.value_kinds` 是 `Vec<ValueKindInfo>`：

```rust
pub struct ValueKindInfo {
    pub kind: ValueKind,        // int/real/float/.../opaque
    pub in_term_graph: bool,    // 能否经 term_of 进入项图
    pub note: String,           // 不能时的替代做法
}
```

| `in_term_graph` | 变体 |
|---|---|
| `true` | `Int`、`Real`、`Float`、`Bool`、`String`、`Symbol`、`Unit`、`Quantity`、`Tensor`、`Term`、`Dyn` |
| `false` | `Complex`（虚部为 0 时才降为 `Float` 入图）、`Sparse`、`Record`、`Variant`、`Function`、`Opaque` |

生成器直接读这张表：`false` 的变体只能传给有 `eval_dyn` 的算子，或作为
返回值；不能出现在需要重写的参数位置。

---

## 4. Aevia 侧如何消费

### 4.1 生成 `Rssn` 模块

构建时：

1. 读取 `KernelManifest`；校验 `api_version`。
2. 对每个 `OpInfo`：
   - `signature_source = Declared | Overlay | Derived` → 生成类型化声明
     `Rssn.<name>(...)`（**每条 overload 一个**），并记住对应的 `OpKey`；
     调用走 `Rssn.call_key(key, …)`，静态选择与运行时执行不会分歧；
   - `signature_source = None` → 生成 `Rssn.dynamic("name", args)`；
   - 有别名 → 生成别名，冲突时加 `Rssn.<rule_set>.<name>`（仅命名空间，
     运行时仍按 name 查）。
3. 生成 `Rssn.Capabilities`（来自 `Features`，含 `plugin_kinds`）、
   `Rssn.Version`、`Rssn.ManifestHash`、`Rssn.ValueKinds`、
   `Rssn.Constants`（来自 `manifest.constants`）、`Rssn.Units`
   （来自 `manifest.units`）。
4. 生成文档/补全/签名存根（LSP）。

**精度标注**：生成器必须在存根里记录每条声明的来源
（`declared`/`overlay`/`derived`）。`derived` 的签名是“形态级”的，
Aevia 的类型检查器可以接受它，但 IDE 应显示为“保守推断”，不要假装
它和 `det : Tensor<[n,n],Real> -> Real` 一样精确。

Aevia 仓库只保留：

```
tools/gen_rssn_module.aev   # 生成器
signatures.overlay.json     # 仅补 RSSN 缺的签名（与 RSSN 仓库同格式）
```

### 4.2 动态 fallback

```aevia
# Derived 也是 typed：数值重载选 Real，符号重载选 Dyn<term>
let a = Rssn.sin(0.5)        # Real
let b = Rssn.sin(x)          # Dyn<term>（x 是符号）

# 插件/叶子算子：没有签名，走 dynamic
let z = Rssn.dynamic("myplugin_softplus", [to_value(x)])

# 任意新算子：同一个入口（短名；不支持 rule_set/name 限定名）
let w = Rssn.call("foo", [to_value(x), to_value(y)])
```

`Rssn.dynamic` 的返回类型是 `Result<Value, OpError>`。它在边界上会做
**签名级**检查（参数是否被某个 overload 接受），但不会做 Aevia 的完整
类型/量纲检查；那由 `Rssn.typed<T>(...)` 或静态检查器负责。

### 4.3 重载解析

两层，职责分开：

**编译期**（Aevia 自己）：

- 先按 arity 过滤，再按参数类型/量纲匹配（与 `kernel::overload` 的
  `Match` 格同构：`Exact > Widened > Shaped > Generic > No`）；
- 多个候选并列最具体 → `E1002`；
- 没有匹配 → 尝试 `Dynamic`；仍失败 → `E1003`，列出候选签名。

**运行期**（RSSN）：

- typed 调用走 `call_key(key, …)`，不再解析，`OpKey.signature_hash` 必须
  来自 `OpInfo.keys`；伪造/过期的 key 报 `UnknownOp`；
- dynamic 调用走 `call_value(name, …)`：多注册时按同一格选，并列返回
  `OpError::Ambiguous`（`E1002`）；参数不被任何 overload 接受时返回
  `OpError::Type`（`E1003`/`E0301`），**不会**静默执行到不合适的重载；
- `Kernel::call(&OpKey, …)` 与 `call_key` 同义，可以放心通过 trait 对象
  传递；
- 需要“先解析再缓存”时用 `resolve_overload` 拿 `OpKey`：`Unique` 给可执行
  key，无签名算子给零 hash key，歧义直接 `Err(Ambiguous)`。

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
| K1 ✅ | 签名覆盖 100%（7 declared + 13 overlay + 1349 derived）、0 unexplained | 1369 个算子全部可生成 typed；其中 20 个精确、1349 个形态级 |
| K1.5 ✅ | `kernel::overload` + `resolve_overload`/`call_key`/`overloads` + `OpError::Ambiguous` | typed 调用可钉死 `OpKey`；多注册时歧义可见（标准集无多注册） |
| K2 ✅ | `rssn::dim` + `UnitRegistry` + `DimTerm` + `dimension_term` + 自定义基量纲（`base_dimension`）+ 30 条常数 | `Units.SI`/`Astronomy`/`CGS`/`Constants` 全部可从 manifest 生成；用户自定义基量纲可用 |
| K3 ✅ | `Term` 遍历/替换/自由符号 + `TermRef` + `fork/merge` 已实现；COW 仍待优化 | `diff` 闭包、并行 |
| K4 ✅ | 插件/宿主回调 + `PluginKind` + cdylib ABI（`plugin-cdylib` feature） | 用户自定义算子、Aevia 函数进 RSSN；cdylib 需 capability 检查 |
| K5 ◐ | `Select`/`Extern1`/`Gather` 已 lowering；默认构建 `jit=false` | 开启 feature 后分支/索引 JIT；否则 `E1001` |
| K6 ✅ | `Solutions`/`OdeSolutionView`/`SimResult` 已实现 | 完整结果类型 |

任何阶段未完成时，Aevia 仍可运行：对应算子走动态 fallback；对应能力报
`E1001`（带 capability 名），不删语言特性。

---

## 7. 验收标准

1. **1000+ 算子可见性**：当前 `manifest.ops.len() == 1369`，全部有签名
   （`unexplained == 0`）；Aevia 生成的 `Rssn` 模块覆盖 100%，其中
   7 declared + 13 overlay 精确、1349 derived 形态级。
2. **零手写绑定**：Aevia 仓库没有按算子名手写的绑定文件；只有生成器和
   overlay。
3. **动态调用冒烟**：对每个 rule set 至少 1 个算子做 `call_value` 往返；
   1369 个算子的 manifest 加载 < 50 ms（release；待基准确认）。
4. **量纲往返**：仅对纯有理指数，经 `Dimension::exact_factors` 做
   `DimTerm -> Dimension -> DimTerm` 随机 10k 相等；符号指数不支持往返。
   `UnitRegistry` 对每个单位 `scale`/`offset` 精确往返。
5. **单位文本安全**：`"L"` 不再被解析为升作为量纲；`"Length/Time"`
   正确；`"m/s"` 仍作为单位文本。
6. **重载**：`resolve_overload("sin", [Float])` 选数值重载；
   177 个双重重载算子都要能各选一次；`eq("a","b")` 这类“全部签名具体”的
   算子返回 `OpError::Type`（`sin("x")` 仍会通过，因为符号重载是 `Dyn`
   兜底——这是已知的弱校验，不是 bug）；`Kernel::call` 与 `call_key` 结果
   一致；伪造 `signature_hash` 返回 `UnknownOp`。
7. **插件**：进程内插件注册一个算子 + 一个重写规则 + 一个单位；开启
   `plugin-cdylib` 后，一个导出 `rssn_plugin_v1` 的动态库能被
   `Session::load_plugin` 加载并调用；没有该 feature 时报
   `E1001("core.plugin.cdylib")`。
8. **线程**：`fork/merge` 的符号结果与串行一致；两个并发请求各自声明
   不同的 `base_dimension` 时互不影响。
8b. **Value 边界**：`manifest.value_kinds` 覆盖全部 17 个变体；
    `in_term_graph=false` 的变体经 `term_of` 必须返回 `Unsupported`，
    经 `eval_dyn` 算子必须可用。
9. **JIT**：开启 `jit` feature 后，带 `if`/索引的函数可 JIT；默认构建
   `features.jit=false`，`@jit` 报 `E1001("core.jit")`。
10. **兼容性**：`api_version` 不匹配时 Aevia 给出明确错误；
    `manifest_hash` 变化使缓存失效但不误用旧签名；pin 的是 `hash_hex()`
    （64 hex），不是 `hash_u64()`。
11. **常数**：`Units.Constants` 的 30 个符号与 `manifest.constants` 一一
    对应；18 条 `exact = true` 的常数没有 `uncertainty`；`e`/`G` 不与
    单位表的高斯/自然常数混淆。
12. **自定义基量纲**：`dimension Currency` + `base_dimension("Currency")`
    后，`unit EUR : Currency = 1.08 USD` 注册成功，且
    `DimTerm -> DimExpr -> DimTerm` 往返相等；在克隆图上定义基量纲不会
    影响源图（COW 测试）。
13. **符号指数桥接**：`DimExpr::base("Length").pow(Symbol("n"))` 经
    `to_term` 得到 `Length^n`，两次 `to_term` 相等；但 `from_term` 只读
    常量指数，符号原子会丢，不要用它回读。Aevia 语言仍拒绝静态 `L^n`
    （§4.12.3 的设计决定），两者不矛盾。

---

## 8. 取舍与风险

- **反射优先 vs 生成代码**：反射让新算子零成本可见，但动态调用有开销。
  解决：类型化 façade + JIT；动态路径只用于未签名/实验性/插件算子。
- **manifest 体积**：1369 算子 + 签名/文档约 2.2 MB JSON。解决：CBOR/bincode；
  Aevia 只加载编译所需子集，LSP 可懒加载。
- **`Derived` 签名的精度**：它只说“数值重载 `(Real,…)` / 符号重载
  `(Term,…)`”，不区分 `Tensor`/`Quantity`/`Int`。因此 1349 个算子的
  typed 声明会把 `det(matrix)` 写成 `Dyn<term>`，而不是 `Real`。解决：
  按价值补 overlay（`det`/`trace`/`norm`/`dot`/`cross`/`transpose`/
  `rank`/`identity` 已经补了），并在 IDE 里标 `derived`；不要让用户以为
  它和 declared 一样精确。
- **`resolve_overload` 对“符号实参”会歧义。** 数值重载的参数是 `Real`，
  符号重载是 `Dyn<term>`；当实参本身是 `Term`/`Dyn` 值时，`Real vs Dyn`
  也走 `(_, Dyn(_)) → Generic`，两条重载同分，返回
  `OpError::Ambiguous`。所以 `resolve_overload` 只在实参类型**具体**
  （`Float`/`Int`/`Symbol`/`Tensor`）时可用；符号表达式请直接用
  `OpInfo.keys` 里的 key（或 `call_value`）。这是 typed façade 必须用
  `call_key`、而不是“运行时再解析”的又一个理由。
- **`check_args` 是弱校验。** `Dyn<term>` 兜底让 1349 个 derived 算子几乎
  接受任何参数（`sin("x")` 返回 `Term`）；只有 `eq`/`lit`/`pow` 这类全部
  签名具体的算子会真的报 `OpError::Type`。Aevia 必须自己做类型检查，
  不能把 dynamic 路径当校验边界。
- **`DimTerm` 公开**：会暴露内部表示。解决：只公开结构化访问器和
  `DimArena` 快照，不暴露 `DimRepr`。
- **插件安全**：cdylib 在宿主进程执行任意代码，没有沙箱。解决：它由
  `plugin-cdylib` feature 显式开启，`features.plugin_kinds` 让宿主在加载前
  就能拒绝；Aevia 只从受信任 registry 加载，并要求 capability 声明。
- **不按 v1/v2 分层**：实现压力大。解决：capability 诊断 + 动态 fallback，
  语言设计一次到位，后端按 K0–K6 补齐。
