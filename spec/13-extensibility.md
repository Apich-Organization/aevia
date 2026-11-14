# 13 — 可扩展性与 RSSN 集成

本章是规范的一部分，并且**优先于**其他章节中任何“暂不支持/以后评估”的表述。Aevia 不按 `v1`/`v2` 分层：语言能力一次设计到位；后端尚未实现的部分用
**capability 诊断**（`E1001`）报告，而不是删掉语言特性。

后端接口的详细契约在 rssn 仓库的 `docs/KERNEL.md`（Kernel Manifest、动态调用 ABI、插件）、`docs/DIMENSIONS.md` 与 `docs/CALLBACKS.md`；
本章定义 Aevia 侧的规范性契约。

---

## 13.1 三条公理

1. **单一算子注册表。** rssn 的 `OpTable` 是唯一事实来源。Aevia 不维护“支持哪些算子”的清单；它读取 **Kernel Manifest**，生成 `Rssn` 模块。
   新算子在 rssn 注册后，Aevia 下一次构建就能看见。
2. **单一量纲/单位层。** `rssn::dim` 的 `DimTerm`/`DimArena`/`UnitRegistry` 是运行时与编译期共享的规范层；Aevia 的 `Dim` 是它的类型级镜像，
   `Units.*` 由 `manifest.units` 生成，不维护第二张单位表。
3. **动态调用是正式能力。** 没有类型签名的算子仍可经 `Rssn.dynamic(name, args)` 调用；类型化 façade 是静态体验的增量，不是能力门槛。rssn 在动态路径上做
   **签名级**校验（`check_args`），不做量纲/类型推导；完整检查由 Aevia 静态检查器或 `Rssn.typed` 负责。

这两个结论合起来：**上千个算子不能靠手写绑定**（会漂移、会漏、会腐烂）；**新能力不能靠“下个版本”补**（语言、manifest、插件 ABI 现在就定，后端按 capability 逐步补齐）。

---

## 13.2 Kernel Manifest 与生成的 `Rssn` 模块

### 13.2.1 Manifest

rssn 在启动时提供确定性的 `KernelManifest`：

```
KernelManifest
  rssn_version : String
  api_version  : Int
  manifest_hash: Bytes32
  features     : Features
  rule_sets    : [RuleSetInfo]
  ops          : [OpInfo]
  units        : [UnitValue]
  base_dimensions : [BaseDimInfo]
  plugins      : [PluginInfo]
  constants    : [ConstantValue]
  value_kinds  : [ValueKindInfo]
```

- `OpInfo`：`keys: [OpKey]`（每个 overload 一个，`keys[0]` 是主重载）、`name`、`aliases`、`category`、`arity`、`overloads`、`signature_source`、`flags`、
  `binder`、`cost`、`eval`、`higher_order`、`purity`、`determinism`、`thread_safety`、`stability`、`since`、`doc`、`examples`、`dim_rule`、
  `backend_support`、`dynamic_reason`、`rule_set`、`jit`。完整 schema 见 rssn 仓库根目录文档 §3.1。
- `manifest_hash` 是确定性 bincode 的 BLAKE3；`to_json()` 只供人读。
- `units: [UnitValue]` 带 `name`/`display`/`dimension`/`scale`/`offset`/`affine`/`aliases`/`prefixable`/`category`/`since`；生成 `Units.*` 只读它即可。
  `KernelManifest::build_with_units(graph, sets, plugins, &registry)` 从调用方的 registry 读单位，`build(...)` 从进程级表读；`Session::manifest()`
  用前者，所以 `Session::register_unit` 注册的单位只进入**该** session 的 manifest。
- `value_kinds: [ValueKindInfo]` 描述每个 `Value` 变体能否入项图（`in_term_graph`）及不能时的替代做法（§13.3.1）。
- `constants: [ConstantValue]` 是物理常数表，字段 `name`/`symbol`/`value: ScaleValue`/`dimension: DimExpr`/`unit`/`exact`/`uncertainty`/`source`；
  `Units.Constants` 由它生成（§13.6.5）。
- `features.plugin_kinds` 说明构建能加载哪种插件（默认 `["in-process"]`；开 `plugin-cdylib` 多一个 `"cdylib"`）；`features.sde` 说明 SDE 噪声原语与方案元数据算子
  可从 manifest 调用（宿主能力名 `core.sde`）。`Features` 属于 manifest：新增字段或算子会改变 `manifest_hash`。

### 13.2.2 生成规则

构建时：

1. 读取 manifest，校验 `api_version`（不兼容报 `E1004`）。
2. 对每个 `OpInfo`：`signature_source = Declared | Overlay | Derived` → 生成 `Rssn.<name>` 类型化声明（**每条 overload 一个**）；`None` → `Rssn.dynamic`；别名 → `Rssn.<alias>`，
   冲突时用 `Rssn.<rule_set>.<name>`。
3. typed 声明记住 `OpInfo.keys` 里对应的 `OpKey`，调用钉死该 overload，静态选择与运行时执行不会分歧。公开形式是 `Rssn.call_key(key, …)`；生成器实际生成等价的
   `Rssn.invoke(name, signature_hash, args)`（同一钉死语义，错误通道不同：生成的 `fn` 必须返回声明类型，所以 kernel 失败会 raise 而不是返回 `Err`）。
4. 生成 `Rssn.Version`、`Rssn.ManifestHash`、`Rssn.Capabilities`、`Rssn.ValueKinds`、`Rssn.Constants`、`Rssn.Units`。
5. 生成 LSP 补全、文档、签名存根，并记录每条声明的来源。

**覆盖是测量，不是常量。** `rssn 3.0.0-rc.1` 上（`aevia rssn` 给出 live 数字）：1620 个算子 100% 有签名，来源 99 declared + 103 overlay + 1418 derived。
精度分档（生成器必须区分）：

| 来源 | 精度 | 生成什么 |
|---|---|---|
| `Declared` | 精确 | typed |
| `Overlay` | 精确 | typed |
| `Derived` | 形态级 | typed，但只有 `Real`/`Bool`/`Term` |
| `None` | 无（叶子/无调用） | `dynamic` |

`Derived` 是“由注册信息推出”，不是猜名字：

| 注册信息 | 推出的签名 |
|---|---|
| 有 `eval` | `(Real, …) -> Real` |
| 有 `eval` + `PREDICATE` | `(Real, …) -> Bool` |
| 非叶子 | 另加 `(Term, …) -> Term` |
| `Binder` 在第 k 位 | 第 k 个参数是 `Symbol` |
| `Arity::Variadic` | 尾参数变长 |
| `OpFlags::LEAF` | 无签名（`DynamicReason::OpaquePayload`） |

因此 `sin` 有两条重载 `(Real) -> Real` 与 `(Term) -> Term`；`det` 是 `HEAVY` 请求、没有 `eval`，没有 overlay 时只有 `(Term) -> Term`，`det(matrix)` 的静态类型会是
`Dyn<term>`，overlay 把它补成 `(Tensor<[n,n],Real>) -> Real`。所以“100% typed”不等于“100% 精确”：`Derived` 不区分 `Tensor`/`Quantity`/`Int`，IDE 标“保守推断”。

overlay 也是**维度类型参数**的载体：`sde_wiener`/`sde_levy_areas` 用 `T` 这类维度参数把 `sqrt(h)`（`RootOf{degree:2,index:3}`）与 `Tensor<1, Real[T]>` 写进签名，derivation
推不出这些。生成源的拼写是精确的（§10.0）：`aevia rssn --emit` 产出的 `Rssn.sde_wiener` 是 `(Int, Int, Int, Real[T]) -> Tensor<2, Real[T^(1/2)]>`。`Dyn<term>` 只为确实无法拼写的类型
保留（§10.0）。

**字面量 pinned key 的结果类型化。** 生成器只发 `Rssn.invoke(op, key, …)` 且 `op`/`key` 都是字面量；checker 按 live manifest 的 pinned overload 校验这两个字符串并给出该 overload 的声明结果类型
（`Rssn.det` → `Real`），每次调用取新的维度变量。**非字面量、未知或歧义的 key** 保持动态 `Value`，诊断由运行时给出：`E1005`（未知/不匹配的 `signature_hash`）、`E1002`（歧义）、
`E1003`（动态参数或非函数值）。checker 不重新解析或执行 key。

SDE 求解循环不在 manifest 里（host 回调，kernel CO-03）；宿主侧 `sdeint_*` 不经过 `Rssn` 算子通道，`sde_scheme_info` 是 manifest-only 的元数据查询（只给强阶，弱阶是宿主侧 `sde_order`，§8.9.1）。

**仓库规则。** Aevia 仓库不允许出现按算子名手写的绑定文件，也不保留 `signatures.overlay.json` 的副本（它是 rssn 的构建输入，已并入 `OpInfo.overloads`）。只允许生成器
（`aevia rssn --emit`，在 `aevia-rssn` 内；标准库的生成视图由 `aevia-stdlib-gen` 写出）。CI 检查：生成器输出与提交的生成文件一致；manifest 快照变化必须显式更新。

### 13.2.3 命名空间

- 普通算子：`Rssn.<name>`（`Declared|Overlay|Derived` 都 typed；只有 `None` 与插件算子走 `Rssn.dynamic`）。
- 规则集限定：`Rssn.<rule_set>.<name>` 只是命名空间；运行时仍按 name/arity。rssn 的 `call_value` 只按 name/alias 查找，**不支持** `rule_set/name`；要钉死重载用
  `Rssn.call_key(key, args)`，`key` 来自 `Rssn.overloads(name)` 或 `OpInfo.keys`。
- 插件：`Rssn.<plugin>.<op>`。
- 领域模块：`import Rssn_NumberTheory`（`Rssn_<Domain>`，CamelCase）给出按数学领域分组、带类型与 `Result` 通道的包装器（§10.3.8）；`Rssn_<rule_set>` 仍按 kernel 分组并保持可用。

名字冲突时，短名只保留**最稳定**的那个（`Stable` 优先，`since` 早者优先），其余用限定名；生成器在 manifest 中记录短名映射。

---

## 13.3 动态值与动态调用

### 13.3.1 `Value`

```aevia
type Value =
    | VInt(Int)
    | VReal(Real[1])
    | VFloat(Float[1])
    | VComplex(Complex[1])
    | VBool(Bool)
    | VString(String)
    | VSymbol(String)                 # 自由符号，不是 String
    | VUnit(Unit[1])
    | VQuantity(Quantity)
    | VTensor(TensorData)             # shape + 行主序 Value
    | VSparse(SparseData)
    | VRecord(Record)
    | VVariant(Variant)
    | VTerm(TermRef)                  # owned handle
    | VFunction(Callback)
    | VDyn(Dyn<Real>)
    | VOpaque(Opaque)
```

`Value` 是**动态边界**的类型，不是核心语言的隐式子类型；它只出现在 `Rssn.dynamic`、插件、FFI、反射 API 中。

**可入项图性**由 `manifest.value_kinds` 给出，Aevia 不维护第二份。当前（17 个变体）：

- `in_term_graph = true`（10）：`VInt`/`VReal`/`VFloat`/`VBool`/`VString`/`VSymbol`/`VQuantity`/`VTensor`/`VTerm`/`VDyn`；
- `in_term_graph = false`（7）：`VComplex`（仅虚部为 0 时降为 `Float` 入图，否则 `Unsupported("complex literals")`）、`VUnit`（单位是定义不是数，过边界用
  `quantity(value, unit)`；只有已声明的单位参数位置才直接收 `Value::Unit`）、`VSparse`/`VRecord`/`VVariant`/`VFunction`/`VOpaque`。

`false` 的变体只能喂给有 `eval_dyn` 的算子或作为返回值；生成器不能假设所有 `Value` 都能参与重写。

### 13.3.2 调用

```aevia
Rssn.call    : (String, Tensor<1, Value>) -> Result<Value, OpError>
Rssn.dynamic : (String, Tensor<1, Value>) -> Result<Value, OpError>
Rssn.typed   : (String, Tensor<1, Value>, TypeRep) -> Result<Value, OpError>
Rssn.overloads       : String -> Tensor<1, String>
Rssn.resolve_overload: (String, Tensor<1, Value>) -> Result<String, OpError>
Rssn.call_key        : (String, String, Tensor<1, Value>) -> Result<Value, OpError>
Rssn.Constants       : Tensor<1, String>     # 常数符号；数值在生成的 Constants 模块
Rssn.Units           : Tensor<1, String>     # 单位名；单位值在生成的 Units 模块
```

- `Rssn.call` 是 `Rssn.dynamic` 的别名，只按短名/别名查找；短名歧义报 `E1002`。
- `Rssn.call_key` “不解析、直接执行”：收 name + `signature_hash`（32 hex 字符），合起来是 `OpKey` 的可写形式；全零表示该算子没有声明重载。key 与当前 manifest 不符报
  `E1005`（提示重新生成模块）；kernel 侧参数偏离钉住签名时报 `UnknownOp`，在 Aevia 边界变成错误值。
- `Rssn.resolve_overload` 暴露运行时选择结果，供缓存/诊断用：成功返回 `"rule_set.name@signature_hash"`；并列与“没有签名接受”都报 `OpError::Ambiguous`。`Rssn.overloads` 返回每条
  overload 的渲染签名。
- 参数用 `to_value(x)` 注入，返回值用 `from_value(v)` 转回静态类型。无签名的算子、插件算子、实验性入口走 `dynamic`；标准集当前没有 `None`，所以 `dynamic` 在标准集上只是插件通道。
- `type_rep<T>()` 保存检查器解析出的具体类型；它是不透明值，普通记录不能伪造。`Rssn.typed` 执行动态调用后按该见证检查表示、形状及量纲；不匹配或缺少证据返回 `Err(OpError)`。
  成功结果仍是 `Value`，经 `from_value<T>` 验证并转换；未经运行时类型字典具体化的泛型不能生成 `TypeRep`。
- 高阶算子用 `Rssn.call_ho(name, args, fns)`；`to_callback` 从已检查源码闭包生成不透明回调，限制见 §8.19。回调在原线程同步执行，外层调用结束即失效；
  跨线程调用和同作用域重入被拒绝，回调错误经 `OpError` 传回；空列表就是一次动态调用。
- `Rssn.register`/`Rssn.register_plugin` 接受 host `Plugin`；源码插件由 `@op`/`@rule` 与 `load_plugin` 发布（§13.5.2）。`core.plugin.source` 在有进程内插件的构建中 enabled；
  `core.callback` 表示受限回调桥可用。

### 13.3.3 错误

```aevia
type OpError =
    | UnknownOp(String)
    | Arity(Int, Int)
    | Type(TypeRep, TypeRep, Int)
    | Ambiguous(String, Int)         # name, 并列数
    | Dimension(DimError)
    | NotNumeric(String)
    | NotReduced(String)
    | Domain(String)
    | BudgetExceeded
    | Unsupported(String, String)   # backend, feature
    | Plugin(String)
    | ForeignTerm(String, String)   # expected session, got session
    | Codec(String)
    | Host(String)
    | Internal(String)
```

`E1003` 是“没有匹配的算子/重载”，携带候选签名列表；`E1001` 是“后端 capability 缺失”，携带 capability 名和最小 rssn 版本。

---

## 13.4 重载、签名与解析

### 13.4.1 重载

rssn 允许同名多签名，判定在 `kernel::overload` 里：

```aevia
type Match = | No | Generic | Shaped | Widened | Exact   # 有序
matches_type : (TypeExpr, TypeExpr) -> Match
matches      : (TypeExpr, Value) -> Match
score        : (Signature, Tensor<1, Value>) -> Option<Nat>
score_types  : (Signature, Tensor<1, TypeExpr>) -> Option<Nat>
best         : (Tensor<1, Signature>, Tensor<1, Value>) -> Best
best_types   : (Tensor<1, Signature>, Tensor<1, TypeExpr>) -> Best
type Best    = | None | Unique(Int) | Ambiguous(Tensor<1, Int>)
```

- 每个参数单独分类，整个调用的分数是**最弱**的那个参数。
- `Int -> Real`、`Int|Real|Float -> Complex` 是 `Widened`，反向不成立；同构造器但元素/量纲未被签名钉死是 `Shaped`；类型变量和 `Dyn` 是 `Generic`。
- `best` 内部并列最高 → `Ambiguous`，不静默取第一个。
- `score_types`/`best_types` 与 `score`/`best` 共用同一取分折叠：Aevia 在编译期解析一次，得到的数与 kernel 运行时一致。

标准集里每个名字只有 1 个注册，但**每个注册内部有 1–5 条重载**（有 `eval` 的算子同时有数值与符号两条，overlay 再补精确签名）。两件事要分开：

- **注册选择**（`resolve_op`）在 `fitting.len() == 1` 处直接返回，对标准集不评分；跨注册并列只可能由插件触发。
- **重载选择**（`resolve_overload` → `overload::best`）对注册内的签名真正执行：`sin(0.5)` 选 `(Real) -> Real`，`sin(x)`（`x` 是 `Symbol`）选 `(Term) -> Term`。
  但实参本身是 `Term`/`Dyn` 时，数值重载也会以 `Generic` 通过，两条同分 → `Ambiguous`；所以 `resolve_overload` 只适合具体类型的实参，符号表达式用 `OpInfo.keys` 的 key。

`call_value` 按 name/arity 选注册，再调 `check_args`：没有任何 overload 接受参数时返回 `OpError::Type`。但符号重载的 `Dyn<term>` 对任何值都是 `Generic`，所以 derived 算子几乎总能通过——
这条检查只在签名全部具体（`eq`/`lit`/`pow`）时真正拦得住；它不在数值/符号之间做选择（由算子的 `eval` 决定）。

Aevia 自己的静态解析规则（与 rssn 的格同构，供 Aevia 自己的重载/插件使用）：

1. 按 arity 过滤；2. 逐参数分类，取最弱；3. 分数最高且唯一 → 选中；4. 并列最高 → `E1002`；5. 没有匹配 → 尝试 `Dynamic`；仍失败 → `E1003`。

运行时三条入口：

- `Session::call_key(&OpKey, …)` **不重新解析**，按 `rule_set` + `signature_hash` 找注册，钉死重载，并要求参数能被钉住签名接受（`overload::score(pinned, args).is_some()`），否则 `UnknownOp`。
- `Session::call_value(name, …)` 按 name/arity 找注册；多个候选时用 `overload::score` 选最具体，并列报 `OpError::Ambiguous`；参数不被接受报 `OpError::Type`。
- `Kernel::call(&OpKey, …)` 转发 `call_key`，因此也钉死重载；经 `dyn Kernel` 传递不丢信息。

`Session::resolve_overload(name, args)` 用于“先解析、再缓存”：`best` 为 `Unique` 返回带 `signature_hash` 的 key，可直接给 `call_key`；算子完全没有签名返回零 hash key（`call_key` 接受）；并列或没有任何签名接受
都是 `Err(OpError::Ambiguous)`，**不会**给出不可执行的零 hash key。所以返回值总可以安全交给 `call_key`。`OpKey.signature_hash` 是 `Signature::hash()` 的前 16 字节（确定性 bincode 的 BLAKE3），
伪造/过期/参数偏离的 key 返回 `UnknownOp`。

### 13.4.2 签名覆盖

覆盖率 100%（`aevia rssn` 报 `0 dynamic-only`）。来源优先级从高到低：

1. `Declared`：`OpDescriptor::new(...).signature(...)`，算子自己写；
2. `Overlay`：`signatures.overlay.json` 里人工写的修正（在 rssn 侧并入 manifest，Aevia 不持有副本）；
3. `Derived`：`kernel::infer::derive` 从注册信息推出；
4. `None`：叶子/无调用形式，`dynamic_reason` 说明原因（标准集没有）。

规则：`Derived` 只读 `eval`/`PREDICATE`/`Binder`/`Variadic`/`LEAF`，不读算子名，所以保守但可靠；精度不够时写 overlay，而不是改推导器（overlay 只用于“这条签名必须精确”的算子：
张量、符号、单位参数）；CI 的 `signature-audit` 报告覆盖率与来源分布，`Derived` 占比高不阻止构建，但阻止把对应算子标成“精确”；插件用 `Registry::register_op` 声明签名，
`Registry` 拒绝“既无签名又无 `dynamic_reason`”的注册。这让“能力完整”和“类型精度”分开推进，而不会有算子被静默遗忘。

---

## 13.5 插件与用户扩展

### 13.5.1 插件

插件不是新语法，而是**普通 Aevia 包 + 清单标记 + 属性**：

```toml
# aevia.toml
[plugin]
kind = "aevia"          # 进程内；对应 rssn 的 PluginKind::InProcess
abi = 1
```

| `[plugin] kind` | rssn | 加载方式 |
|---|---|---|
| `"aevia"` / `"in-process"` | `PluginKind::InProcess` | `Session::register(Box<dyn Plugin>)` |
| `"cdylib"` | `PluginKind::Cdylib` | `Session::load_plugin(path)`（需 `plugin-cdylib`） |

`cdylib` 的 C ABI 是**字节缓冲**：入口符号 `rssn_plugin_v1`，参数与返回值是 bincode 编码的 `Vec<Value>`/`Result<Value, String>`，不是 `#[repr(C)]` 的 `Value` 镜像；`cdylib::load` 校验
`PLUGIN_ABI` 和 `RssnPluginDesc`，`Session::load_plugin` = `load` + `register`。该模块由 `plugin-cdylib` feature 控制（默认关闭，`full` 才包含），因为加载动态库会在宿主进程里执行任意代码、没有沙箱。
Aevia 在加载前必须查 `manifest.features.plugin_kinds`：没有 `"cdylib"` 时报 `E1001("core.plugin.cdylib")`，而不是让加载器抛底层错误。

```aevia
module MyDomain

@op("mydomain/softplus")
fn softplus(x: Real[1]): Real[1] = ln(1 + exp(x))

@rule("softplus(x) => ln(1 + exp(x))")
let softplus_rule = ()

dimension MyDim
unit myunit : Length = 0.5 m
```

`Rssn.register`（别名 `register_plugin`）是 `Session::register(Box<dyn Plugin>)` 的包装：`Rssn.register(plugin)  # -> Result<PluginId, PluginError>`。插件可以注册算子 + 签名 + eval 回调、
重写规则、量纲规则、单位/基量纲，以及（可选）JIT lowering。

### 13.5.2 Aevia 函数作为 rssn 算子

`@op` 把 Aevia 函数发布为 rssn 算子，`@rule` 发布为重写规则：

```aevia
@op("mydomain/softplus")
fn softplus(x: Real[1]): Real[1] = ln(1 + exp(x))

@rule("mydomain/sinh-exp")
fn sinh_exp(x: Real[1]) = (sinh(x), (exp(x) - exp(-x)) / 2)
```

**决定：用追踪而不是回调。** kernel 的扩展语言就是它的项语言：算子是定义 `name(a, b) := body`，规则是重写 `name: lhs => rhs`。Aevia 函数以**追踪**变成它们：
用带各参数已检查量纲的新符号调用函数，它构造的项就是定义体。这不需要第二个编译器，并继承 kernel 的全部能力——kernel 可以调用它、对它求导、穿过它化简与积分、数值求值，
这是不透明的宿主回调做不到的（回调也不能跨线程，`Rc` 解释器不是 `Send`）。

- 代价是被检查的：结果依赖于对参数的控制流（`if x > 0`、对值的 `match`）或做 IO 的函数追踪不出项，注册报 `E1006` 并给出原因。
- 规则函数返回二元组 `(lhs, rhs)`，参数就是规则的模式变量；`lhs` 必须是算子应用（追踪时会被局部化简，所以 `x * 0` 不是合法的左端）。
- 发布发生在模块函数绑定之后、顶层 `let` 求值之前，所以顶层绑定已可经 `Rssn.dynamic("mydomain/softplus", …)` 调用。每个加载的模块注册一个进程内插件 `aevia/<module>#n`，
  属于当前线程的 kernel 会话。
- **源码算子是一等算子。** 发布顺序：先把每个 `@op` 追踪成 kernel 定义，并把检查得到的参数/结果量纲作为**量纲签名**一并注册（`Registry::register_dim_signature`：参数与结果是签名变量的单项式，
  每次使用由实参量纲求解；`fn area(r: Real[d]): Real[d^2]` 的 `demo/area(r)` 在 kernel 的量纲推断里是 `dim(r)^2`）；再把模块里该函数的绑定换成包装：**全部实参是数值**
  （或无自由符号的闭式项）时执行 Aevia 函数体，**有符号实参**时构造算子结点 `demo/softplus(x)`，结果量纲由签名给出，实参量纲与签名冲突是 `E0401`；最后才追踪 `@rule`，所以规则可以谈论
  源码算子。`diff`/`simplify`/`integral` 作用在算子结点上时，kernel 看到的是算子、它的定义和关于它的全部规则。kernel 拒绝插件时恢复原绑定。
- **动态加载**：`load_plugin(path) : String -> IO (Result<Int, String>)` 在运行期加载、检查、求值一个源码文件（含其导入），发布其中的 `@op`/`@rule`，返回发布数量；需要 `fs` 能力。

`@op("mydomain/softplus")` 里的 `mydomain/` 是**算子名的一部分**（插件命名空间），不是 `rule_set/name` 限定符；kernel 的定义头允许 `/` 分隔的标识符段，`call_value` 按完整字符串查找。

### 13.5.3 安全

- 插件必须签名；默认只加载 registry 信任的插件。
- 插件声明 capabilities（`fs`、`net`、`unsafe`）；未声明的能力在沙箱中被拒绝。
- 插件 ABI 版本不匹配报 `E1006`。

---

## 13.6 量纲与单位：统一到 `rssn::dim`

### 13.6.1 规范层

`rssn::dim` 提供：`DimTerm`（基量纲/变量的有理指数乘积，结构化访问 `factors(&arena)`/`vars(&arena)`）、`DimArena`（interning，可快照/恢复）、
`UnitRegistry`（单位名、别名、精确尺度 `Scale { rat, pi }`、仿射 offset、前缀、显示、类别、since），以及 `Dimension`（**可打印快照**，指数是字符串；精确比较/计算用 `DimTerm`，
不要解析 `Dimension.factors`）。

Aevia 的 `Dim` 是 `DimTerm` 的类型级镜像；两者的规范形、基名、指数运算必须一致。编译器不复制单位表；`Units.*` 是 `manifest.units` 的生成视图（当前 82 项）。
单位属于 **session**：`Session::register_unit(UnitDef)` 写该 session 的 registry，`build_with_units` 才把它写进 manifest；进程级 `build` 看到进程表；插件注册的单位只属于安装插件的 session。

结构化入口是 `Config::dimension_term(symbol, DimExpr)`：不解析文本，未知基名被 intern，结构化假设在文本假设之后应用（同名时结构化胜）。`DimExpr::to_term` 写 term；`try_from_term`
是 term 可表示量纲的精确逆（`Length^n` 读回 `Length^n`）；`from_term` 是全函数版，读不回时给 `DimExpr::Any` 而不是错误量纲。

`OpInfo.dim_rule` 的 `DimRuleInfo` 有 `RootOf { degree, index }`：结果量纲是**第 `index` 个参数**量纲的 `degree` 次根（旧 `Root(n)` 固定取第 0 个参数，表达不了
`sde_wiener(seed, m, steps, h)` 的 `sqrt(h)`）；manifest wire form 是 `{"root_of":{"degree":2,"index":3}}`。宿主读 `dim_rule` 时应按变体匹配，不要假设只有 `Root`/`Same`/`Product`。

自定义基量纲由 `Config::base_dimension(name)` 声明（幂等）：把基名送进 `DimArena`，`Dim` 的内部表示从固定的 7 个 SI 指数扩成“7 个 SI + 有序旁表”，`expr_to_dim` 接受任意基名；
这是 `unit EUR : Currency` 能成立的前提（`dimension_term` 只能为 arena 里已有的基名产出 `DimTerm`）。

### 13.6.2 单位声明

```aevia
dimension Currency            # 自定义基量纲
unit USD : Currency = 1       # 基单位
unit EUR : Currency = 1.08 USD
unit myunit : Length = 0.5 m  # 内置基量纲
```

生成器为每个 `dimension` 在 `Config` 上声明一次 `base_dimension("<Name>")`，然后才注册单位；否则单位注册成功但 `DimTerm` 无法生成。

- `dimension` 注册到 `DimArena`（编译期经 `DimExpr`，运行时经 `Config::dimension_term` / `Graph::assume_dimension_value`）；`unit` 注册到 `UnitRegistry`。
- 默认导出，`private` 只在本模块可见；导出信息进入 manifest，下游包可以直接 `import`。
- 用户单位可以是仿射的（`unit °X : Temperature = 1 K offset 273.15`），但仿射单位只能单独使用。
- `features.user_base_dimensions` 是硬编码 `true`（每个构建都支持）；按构建变化的只有 `jit*` 与 `plugin_kinds`。

### 13.6.3 `Dyn`

`Dyn<ρ>` 的运行时量纲是 `DimTerm`，不是字符串。`dyn`/`check` 是类型指导原语：具体 `d` 在编译期物化为 `DimTerm`；泛型用 `dyn_with`/`check_with` 显式传入；`check` 返回 `Result`。

### 13.6.4 `Point` 与仿射

`Point[d]` 是静态包装；运行时是 `Float` + `UnitId`。`UnitRegistry` 的 `affine`/`offset` 是唯一事实来源。`Point` 的算术规则在 Aevia 层实现，rssn 的 `quantity`/`convert` 负责显示/换算。

### 13.6.5 物理常数

`manifest.constants` 是 `Vec<ConstantValue>`（当前 30 条、18 条精确）：

```rust
pub struct ConstantValue {
    pub name: String,        // "speed of light in vacuum"
    pub symbol: String,      // "c"
    pub value: ScaleValue,   // 精确有理数，相干 SI
    pub dimension: DimExpr,
    pub unit: String,        // "m/s"
    pub exact: bool,
    pub uncertainty: Option<ScaleValue>,
    pub source: String,      // "SI 2019 definition" / "CODATA 2022"
}
```

`Units.Constants` 由它生成：符号原样使用（`c`、`g_n`、`h`、`hbar`、`k_B`、`N_A`、`e`、`G`、`alpha`、`R`、`F`、`sigma_SB`、`m_e`、`m_p`、`m_n`、`a_0`、`R_inf`、`mu_0`、`epsilon_0`、
`mu_B`、`M_u`、`m_u`、`T_tp`、`atm`、`G_0`、`K_J`、`R_K`、`Phi_0`、`K_cd`、`dnu_Cs`）；`exact = true` 的没有 `uncertainty`；常数是**值**，不是单位，不能出现在 `quantity(v, u)` 的单位位置；
`e` 是基本电荷，与 `Math.e` 同名，两者同时可见时必须限定（`Math.e` / `Units.Constants.e`）。

### 13.6.6 线程与 arena 隔离

`DimArena` 在模板里是 `Arc<RwLock>`，`Graph::clone` **共享**它；写入时 `with_dim_arena_mut`/`define_base_dimension` 看到 `Arc::strong_count > 1` 就克隆再写（写时复制）。因此：
`Config::base_dimension(name)` 是安全路径（`prepare` 在克隆图上定义，请求之间互不影响）；不定义基量纲的请求共享模板 arena，不付拷贝成本；不要直接对 `Session` 的模板图调用
`define_base_dimension`；跨线程仍用 `Session::fork`/`merge`，不共享 `Session`（`RefCell`）。

---

## 13.7 中心包 registry

registry 是**内容寻址 + 签名 + 能力声明**的包索引，服务实现在 `platform/`（`aevia-registry-*` 与 `aevia-pkg`），部署为 `aevia.apich.org`。

**状态**：已实现——`aevia add owner/name[@req]`、`aevia update`、`aevia fetch`、`aevia tree`（均支持 `--offline`）与 `aevia pkg publish/yank/search/verify`；依赖解析由
`aevia-resolve`（SemVer 2.0、回溯求解、锁文件带 `checksum`，§7.5.1/§7.5.2）完成；git 依赖与未知清单键仍是 `E0605`。已知缺口见附录 C Revision 15。

### 13.7.1 包清单

```toml
[package]
name = "aevia-thermo"
version = "0.3.0"
edition = "2026"
license = "MIT"
content_hash = "blake3:..."

[deps]
aevia-std = "^1.0"

[compat]
aevia = ">=1.0, <2.0"
rssn_api = "1"

[capabilities]
requires = ["fs"]
provides = ["thermo"]

[operators]
exports = ["thermo/enthalpy", "thermo/entropy"]

[units]
exports = ["J", "kJ", "cal"]

[dimensions]
exports = ["Entropy"]
```

### 13.7.2 协议

逻辑操作（HTTP 路径以 `platform/` 的 `/api/v1` 为准）：

| 操作 | 说明 |
|---|---|
| 读取索引 | 全部包名 + 最新版本 + 摘要；带 `ETag` |
| 下载包 / 签名 | 内容寻址的归档，下载时校验存储字节的摘要 |
| 搜索 | `aevia pkg search` |
| 发布（签名） | 校验：不得有纯 path 依赖、不得依赖自身、依赖必须存在且有未 yank 的满足版本；版本允许预发布、拒绝 build 元数据；发布在一个事务内 |
| 撤回（yank） | 不删除内容，只阻止新解析 |
| 解析 / 被依赖 | 服务端解析依赖；列出某包的依赖者 |

索引项至少包含：name、version、content_hash、deps（`package`/`req`/可选 `import`）、aevia_range、rssn_api、capabilities、operators、units、dimensions、stability、yanked。

### 13.7.3 可复现

包内容哈希覆盖源码、manifest、生成的 `Rssn` 模块签名；构建不执行任意脚本；依赖解析使用内容哈希 + semver；registry 镜像必须可验证签名。

---

## 13.8 稳定性与 capability（取代 v1/v2）

### 13.8.1 稳定性标记

```aevia
@stable
@experimental
@deprecated("use foo2")
```

`Stable`：major 内签名/语义不变；`Experimental`：可改，但旧名至少保留一个 minor；`Deprecated`：至少一个 minor 的警告期。

### 13.8.2 Capability

后端能力用 capability 名描述。**名字的唯一来源是 capability 表**：由 manifest 的 `Features` 派生，`Rssn.Capabilities` 只列 `enabled = true` 的行（`aevia-rssn` 的 `Capabilities::table()`）。完整表：

```
core.dynamic_call
core.jit                    # false：默认构建无 jit feature
core.jit_branch             # false（同上）
core.jit_index              # false（同上）
core.parallel_symbolic
core.higher_order
core.structured_results
core.sde                    # 由 features.sde 派生
core.callback               # 同线程作用域回调，支持具体结构化签名
core.plugin.source          # 源码级 @op/@rule/load_plugin（需 in-process 插件）
core.plugin
core.plugin.in-process
core.plugin.cdylib          # 仅 plugin-cdylib 构建
dim.exact_units
dim.affine_units
dim.user_base_dimensions
```

默认构建 `Rssn.Capabilities` 实际列出十二行（除 `jit*` 三行与 `plugin.cdylib` 外全部）。另有**规范声明、但 manifest 没有对应 `Features` 字段**的名字，可出现在诊断里，
`Rssn.Capabilities` 不会列出：`core.symbolic`、`core.numeric`、`core.tensor`、`core.linalg`、`core.ode`、`core.pde`、`core.fft`、`core.stats`、`core.units`、`core.assume.general`、
`core.gpu`、`core.distributed`、`io.binary`。

`storage.sparse`（§13.9.5/§8.4.1）是 Aevia 自己的存储类，已启用，不由 manifest 的 `Features` 发布，所以不出现在 `Rssn.Capabilities` 里。

Aevia 编译器在类型检查后检查 capability；缺失时：静态可知（如 `@jit` 目标）→ 编译期 `E1001`；动态调用 → 运行时 `OpError.Unsupported`，Aevia 转成 `E1001`。
**不删语言特性**：GPU 未实现时 `on(Gpu(0), ...)` 仍类型正确，运行时报 `E1001("core.gpu")`。

---

## 13.9 已决定的扩展设计

### 13.9.1 形状细化

**决定：支持，作为 refinement，不是完整依赖类型。**

- `Tensor<n, τ>` 只带秩；`Tensor<[n1, ..., nk], τ>` 是形状细化，`n_i` 是类型级 `Nat`，可用 `?` 表示未知；`Matrix<n, m, τ>` 是 `Tensor<[n, m], τ>` 的别名。
- 张量字面量自动获得细化形状；`zeros_of<[3,4]>()` 等可显式指定。`matmul : Tensor<[m,k],τ> × Tensor<[k,n],τ> -> Tensor<[m,n],τ>`；秩-only 版本保留，运行时检查。
- `reshape<[n1, ..., nk]>(a)` 始终返回 `Result`：形状静态已知时元素数在编译期检查、结果恒为 `Ok`，否则运行时检查；`reshape_dyn` 总是运行时检查。
- 索引：细化形状 + 字面量索引 → 编译期越界检查；动态索引仍运行时检查。
- 细化 → 秩-only 是安全隐式转换；秩-only → 细化用 `refine_shape` 返回 `Option`/`Result`。
- 类型级 `Nat` 支持字面量、`+`、`*` 和线性等式；不做一般递归/证明，足以表达矩阵乘法、reshape、拼接。

### 13.9.2 `Point` 张量

**决定：支持 `Tensor<shape, Point[d]>`。** 元素级 `+`/`-` 按点规则（`Point - Point -> Float[d]`，`Point + Float[d] -> Point`，`Point + Point` 报 `E0405`）；`mean` 只接受 `Float`/`Real`，点的平均用
`centroid : Tensor<1,Point[d]> -> Point[d]`；温度场可以直接是 `Tensor<[nx,ny], Point[Θ]>`；存储是 `Float` + `UnitId`，仿射偏移只在元素级运算时处理。

### 13.9.3 `Table` 行多态 schema

**决定：`Table<ρ>` 使用行多态静态 schema，动态 `Table` 仍保留。**`ρ` 是行类型（字段名 + 类型 + 量纲）；`read_csv<ρ>(path)` 从表头单位推断字段类型，无法推断时报 `DataError.Schema`；
`column(t, "time")` 是类型级字段查找；`select`/`join`/`rename` 保持行多态，新增字段要求 `lacks` 约束；动态 CSV 返回 `Table<Dynamic>`，`to_table<ρ>` 运行时校验（细节见 §8.11）。

### 13.9.4 设备、GPU、分布式

**决定：设备是运行时值，不是张量类型参数。**

- `Device = Cpu | Gpu(Int) | Distributed(Cluster)`；`Tensor` 类型不变，值带 `device` 元数据。
- `to_device : (Tensor<n,τ>, Device) -> IO (Tensor<n,τ>)`；`on : (Device, () -> a) -> IO a` 是作用域执行（分配/传输是 IO）。
- `@jit(target = Gpu(0))` 指定 JIT 目标；没有 `core.gpu` 时报 `E1001`。分布式张量用库类型 `Sharded<Mesh, Tensor<...>>`，`Mesh` 是运行时拓扑。这样不为设备/分片生成类型组合爆炸。

**状态**：`Device` 值、`to_device`/`on`/`device_of` 已类型化；没有 `core.gpu` 时 `to_device`/`on` 报 `E1001("core.gpu")`。`@jit(target = Cpu)` 与 `@jit` 等价；`@jit(target = Gpu(n))` 与
`Distributed(..)` 被语言接受，在检查期报 `E1001("core.gpu")`/`E1001("core.distributed")`；其他参数是 `E0210`。属性参数里“构造器作用于常量”（`Gpu(0)`）算编译期常量（§2.2.7）。
`Gpu`/`Distributed` 没有后端，只是可构造的运行时值（rssn 侧设备层见 §10.6.5）。

### 13.9.5 稀疏与布局

**决定：`Sparse<F, shape, τ>` 是核心类型构造器，`F` 是格式类型。**

- 格式：`Csr`、`Csc`、`Coo`、`Bcsr`（`Block` 是同义写法）；当前是封闭集合，用户自定义格式要等 rssn 开放 `Format` kind（当前插件只能注册算子/单位）。
- `Layout` 类（设计上）：`nnz`、`to_dense`、`from_dense`、`matmul`、`transpose`；当前实现是内建函数，不是可实例化的类型类。
- `@` 按布局分派（dense×sparse、sparse×dense、sparse×sparse）；`Sparse` 与 `Tensor` 之间只有显式转换。rssn 的 `TypeExpr::Sparse` 只有 `rank`，所以 `Sparse<F,shape,τ>` 在边界降为
  rank + 运行时 shape；细化形状由 Aevia 自己检查与传播。

**状态**：已实现，`storage.sparse` 已启用——运行时 `Value::Sparse`、四种布局分派、`Bcsr`、预条件 `cg_with`、非对称 Krylov、切片与秩 N 收缩；细节、精确性与错误码见 §8.4.1。**剩余缺口**：
格式仍是封闭集合；稀疏值不能进入 rssn 项图。

### 13.9.6 效果与能力

**决定：不做用户自定义 effect handlers；`IO` 保持单一类型。** 扩展点是算子/插件/registry，不是语言级效果系统。`IO` 动作可声明所需 capability（`fs`、`net`、`env`、`rand`、`gpu`）——这是运行时
权限，不是类型参数；`with_cap(cap, thunk)` 在沙箱中执行，没有能力时 IO 动作失败。这避免 effect row 侵入全部标准库签名。需要 DSL/代码生成时用插件/算子，不用宏。

**状态**：`with_cap(caps, thunk)` 与 `aevia run --caps` 已实现，语义见 §9.14（能力集按块收窄、嵌套取交集、缺失时 `E0902`）。它与 §13.8.2 的后端能力表是两回事：后者描述后端能做什么，
前者描述程序被允许做什么。

### 13.9.7 宏与语法扩展

**决定：不提供用户宏；语法固定。** 扩展通过算子、插件、registry、`Rssn` 动态调用完成；不引入 `macro_rules`/卫生宏，避免工具链分叉。若将来确需，必须以独立、可选的预处理层出现，不进入核心语法；
当前规范不预留语法。

### 13.9.8 角度与不确定度

- **`Angle` 不是基量纲**：`rad` 无量纲，`deg` 是尺度；`sin`/`cos` 接受 `Real[1]`，`deg` 字面量折叠为 `pi/180`；不引入 `Angle` 基量纲，避免破坏 `exp(i*θ)`、`sin(ωt)` 等公式。
- **`Measurement<d>` 是标准库类型，不是核心类型**：独立误差传播见 §8.14；相关误差用 `Covariance<d,e>` 库类型；核心语言只保证量纲。

### 13.9.9 其他已关闭的问题

见 §12.9 的决策表（`det`、`Real`/`Float` 混合、`Dyn` 错误、`RateOf` 嵌套、`Table` schema、`@`/`*`、`%`、`let` 递归、顶层 IO、registry、量纲别名导出）。

---

## 13.10 兼容性与迁移

- **语言版本**用 semver，不设“v1 语言/v2 语言”：major 才允许破坏性变更；minor 只加能力/算子/库；patch 只修 bug。
- **manifest 兼容**：`api_version` 不兼容 +1；Aevia 编译器 pin 范围；`manifest_hash` 变化使缓存失效，但旧签名仍可用到下一个 major。
- **算子弃用**：`Deprecated` 至少一个 minor 的警告；别名保留。
- **包 registry**：内容哈希 + 签名 + capability；yank 不删除内容，只阻止新解析。
- **迁移**：不强制用户重写代码；弃用算子保留别名，旧签名在 pin 的 `api_version` 内保持可用。当前没有自动迁移命令。

## 13.11 错误码

| 代码 | 情况 |
|---|---|
| `E1001` | 后端 capability 缺失（`core.gpu`、`core.jit_branch` 等） |
| `E1002` | 算子/重载歧义 |
| `E1003` | 没有匹配的算子/重载 |
| `E1004` | rssn `api_version` 不兼容 |
| `E1005` | manifest/签名哈希不匹配 |
| `E1006` | 插件 ABI/加载失败；`@op`/`@rule` 追踪失败 |

这些错误属于**环境/后端**，不是用户程序类型错误；诊断必须给出 capability 名、最小 rssn 版本和可执行的替代方案。
