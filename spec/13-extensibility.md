# 13 — 可扩展性与 RSSN 集成

本章是规范的一部分，并且**优先于**其他章节中任何“暂不支持/以后评估”的
表述。Aevia 不按 `v1`/`v2` 分层：语言能力一次设计到位；后端尚未实现的
部分用 **capability 诊断**（`E1001`）报告，而不是删掉语言特性或让用户
等到下一个大版本。

本章的详细后端接口契约在 rssn 仓库的 `docs/KERNEL.md`（Kernel Manifest、动态
调用 ABI、插件）、`docs/DIMENSIONS.md` 与 `docs/CALLBACKS.md`。
这里定义 Aevia 侧的规范性契约。

---

## 13.1 三条公理

1. **单一算子注册表。** RSSN 的 `OpTable` 是唯一事实来源。Aevia 不维护
   “支持哪些算子”的清单；它读取 RSSN 的 **Kernel Manifest**，生成
   `Rssn` 模块。新算子只要在 RSSN 注册，Aevia 下一次构建就能看见。
2. **单一量纲/单位层。** `rssn::dim` 的 `DimTerm`/`DimArena`/`UnitRegistry`
   是运行时与编译期共享的规范层。Aevia 的 `Dim` 是它的类型级镜像；
   `Units.*` 由 `manifest.units` 生成，不维护第二张单位表。
3. **动态调用是正式能力。** 没有类型签名的算子仍可通过
   `Rssn.dynamic(name, args)` 调用；类型化 façade 是静态体验的增量，不是
   能力门槛。RSSN 在动态路径上做**签名级**校验（`check_args`），但不做
   量纲/类型推导；`Dyn<term>` 兜底让这条校验对多数 derived 算子很弱，
   完整检查仍由 Aevia 静态检查器或 `Rssn.typed` 负责。

这三条合起来解决两个长期问题：

- **1000+ 算子不能靠手写绑定。** 绑定会漂移、会漏、会随 RSSN 演进而腐烂。
- **新能力不能靠“下个版本”补。** 语言、manifest、插件 ABI 现在就定，
  后端按 capability 逐步补齐，用户不会遇到“语言升级才能用”的断崖。

---

## 13.2 Kernel Manifest 与生成的 `Rssn` 模块

### 13.2.1 Manifest

RSSN 在启动时提供确定性的 `KernelManifest`，至少包含：

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

`OpInfo` 的实际字段：`keys: [OpKey]`（每个 overload 一个，`keys[0]` 是
主重载）、`name`、`aliases`、`category`、`arity`、`overloads`、
`signature_source`、`flags`、`binder`、`cost`、`eval`、`higher_order`、
`purity`、`determinism`、`thread_safety`、`stability`、`since`、`doc`、
`examples`、`dim_rule`、`backend_support`、`dynamic_reason`、`rule_set`、
`jit`。完整 schema 见根目录文档 §3.1。

`manifest_hash` 是确定性 bincode 的 BLAKE3；`to_json()` 只供人读。
`units` 是 `UnitValue`，带
`name`/`display`/`dimension`/`scale`/`offset`/`affine`/`aliases`/
`prefixable`/`category`/`since`，生成 `Units.*` 只读 manifest 即可。
`KernelManifest::build_with_units(graph, sets, plugins, &registry)` 从调用方
的 registry 读单位，`build(...)` 从进程级表读；`Session::manifest()` 用前者，
所以 `Session::register_unit` 注册的单位会进入**该** session 的 manifest，
不会泄漏到别的 session。

`value_kinds: [ValueKindInfo]` 描述每个 `Value` 变体能否入项图
（`in_term_graph`）以及不能时的替代做法；生成器按它决定 façade 的参数
位置能不能接受该类型（§13.3.1）。本次测量 17 个变体中 10 个
`in_term_graph = true`。

`constants: [ConstantValue]` 是物理常数表（30 条、18 条精确），字段
`name`/`symbol`/`value: ScaleValue`/`dimension: DimExpr`/`unit`/`exact`/
`uncertainty`/`source`；`Units.Constants` 由它生成（§13.6.5）。

`features.plugin_kinds` 说明这个构建能加载哪种插件（默认
`["in-process"]`；开 `plugin-cdylib` 后多一个 `"cdylib"`）。
`features.sde`（本轮新增）说明 `sde` rule set 已安装、SDE 噪声原语
（`sde_wiener`/`sde_levy_areas`）与方案元数据算子 `sde_scheme_info` 可从
manifest 调用；宿主侧的 capability 名是 `core.sde`。`Features` 是 manifest
的一部分，新增字段/算子会改变 `manifest_hash`（本轮 hash 从 `1920638b…`
变为 `8ed92a22…`，随后因新增 `sde_scheme_info` 变为 `8d2c3764…`）。

### 13.2.2 生成规则

Aevia 构建时：

1. 读取 manifest；校验 `api_version`（不兼容报 `E1004`）。
2. 对每个 `OpInfo`：
   - `signature_source = Declared | Overlay | Derived`：生成
     `Rssn.<name>` 类型化声明（**每条 overload 一个**）；
   - `signature_source = None`：生成 `Rssn.dynamic`；
   - 别名：生成 `Rssn.<alias>`；冲突时用 `Rssn.<rule_set>.<name>`。
3. typed 声明同时记住 `OpInfo.keys` 里对应的 `OpKey`，调用必须钉死该
   overload，静态选择与运行时执行不会分歧。公开形式是
   `Rssn.call_key(key, …)`；当前生成器生成等价的
   `Rssn.invoke(name, signature_hash, args)`（同一钉死语义，错误通道不同：
   生成的 `fn` 必须返回声明类型，所以 kernel 失败会 raise 而不是返回
   `Err`）。
4. 生成 `Rssn.Version`、`Rssn.ManifestHash`、`Rssn.Capabilities`、
   `Rssn.ValueKinds`、`Rssn.Constants`、`Rssn.Units`。
5. 生成 LSP 补全、文档、签名存根；存根里记录每条声明的来源。

当前实测覆盖（`rssn 3.0.0-rc.1`，2026-11-05；live manifest 是唯一事实
来源）：1382 个算子 **100% 有签名**（32 个 rule set），0 unexplained。
来源是 **7 declared + 25 overlay + 1350 derived**；overload 分布是 1201 个
1 条、178 个 2 条、2 个 3 条、1 个 5 条（共 1568 条签名）。这些数字是
**测量**，不是要写死进生成器的常量。

**精度分档**（生成器必须区分）：

| 来源 | 数量 | 精度 | 生成什么 |
|---|---|---|---|
| `Declared` | 7 | 精确 | typed |
| `Overlay` | 25 | 精确 | typed |
| `Derived` | 1350 | 形态级 | typed，但只有 `Real`/`Bool`/`Term` |
| `None` | 0 | 无（叶子/无调用） | `dynamic` |

`Derived` 的规则是“由注册信息推出”，不是猜名字：

| 注册信息 | 推出的签名 |
|---|---|
| 有 `eval` | `(Real, …) -> Real` |
| 有 `eval` + `PREDICATE` | `(Real, …) -> Bool` |
| 非叶子 | 另加 `(Term, …) -> Term` |
| `Binder` 在第 k 位 | 第 k 个参数是 `Symbol` |
| `Arity::Variadic` | 尾参数变长 |
| `OpFlags::LEAF` | 无签名（`DynamicReason::OpaquePayload`） |

因此 `sin` 有两条重载：`(Real) -> Real` 与 `(Term) -> Term`（它有 `eval`）。
`det` 是 `HEAVY` 请求、没有 `eval`，所以没有 overlay 时**只有一条**
`(Term) -> Term`——连数值形态都没有，`det(matrix)` 的静态类型会是
`Dyn<term>`；overlay 把它补成 `(Tensor<[n,n],Real>) -> Real`。这就是为什么
“100% typed”不等于“100% 精确”：1350 个算子的签名不区分
`Tensor`/`Quantity`/`Int`，Aevia 的 IDE 应把它们标成“保守推断”。

overlay 也是**维度类型参数**的载体：`sde_wiener`/`sde_levy_areas` 用
`T` 这类维度参数把 `sqrt(h)`（`RootOf{degree:2,index:3}`）和
`Tensor<1, Real[T]>` 写进签名，derivation 推不出这些信息。manifest 里的
签名是精确的，bridge 的 conformance 测试断言 `sde_wiener` 的元素量纲恰是
“一个维度变量上的 `^(1/2)`”；checker 与 `registry().resolve` 都按精确
签名工作。**生成源的拼写现在也是精确的**（task-31）：`render_dim` 把维度
参数写成 Aevia 的量纲变量，变量名在**声明内局部**（`T`、`T2`…）并跨参数
与返回共享——`aevia rssn --emit` 产出的 `Rssn.sde_wiener` 是
`(Int, Int, Int, Real[T]) -> Tensor<2, Real[T^(1/2)]>`，`sde_levy_areas` 是
`(Int, Real[T], Tensor<1, Real>, Int) -> Tensor<1, Real[T]>`；不再有
“open dimension 只能写 `Dyn<term>`”的限制。`Dyn<term>` 只为**确实无法
拼写**的类型保留：manifest 自己开放的 `Dyn(rep, any)`（`quantity`/`convert`
的值参数与结果、derived 签名）、**秩变量张量**（`transpose`/`rank`/`dot`/
`norm`/`sum`；声明需要具体秩）、类型参数、未知行 `Table`、`Device`、变体、
host opaque、`Never`。生成的 `Units.aev`/`Constants.aev` 无条件通过 check；
`Rssn.aev` 也通过——**字面量 pinned key 的结果类型化**：生成器只发
`Rssn.invoke(op, key, …)` 且 `op`/`key` 都是字面量，checker 按 manifest 的
pinned overload 校验并给出该 overload 的声明结果类型
（`Rssn.sde_wiener` → `Tensor<2, Real[T^(1/2)]>`、`Rssn.det` → `Real`…），
每次调用取新的维度变量；**非字面量、未知或歧义的 key** 保持动态 `Value`，
诊断由运行时给出：`E1005`（未知/不匹配的 `signature_hash`）、`E1002`
（歧义的注册/重载）、`E1003`（动态参数或非函数值）。checker 只按 live
manifest 校验两个字符串字面量 key，绝不重新解析或执行它；映射用的是
manifest 自己的签名，不是渲染文本。这套“emit 后 0 diagnostics”的验收在
干净进程里由 `aevia-rssn` 的 `emit_typecheck` 断言
（`the_emitted_modules_type_check_with_no_diagnostics`；
`cargo test -p aevia-rssn` 78 项通过）。新增的
`sde_scheme_info(name) -> Tensor<1, Real>` 没有开放维度，生成源同样是精确
的。**SDE 求解循环则不在 manifest 里**（host 回调，kernel CO-03），宿主侧
`sdeint_*` 不经过 `Rssn` 算子通道；`sde_scheme_info` 是本轮
新增的**元数据算子**（强阶 + Itô/交换/自适应/Lévy 面积四个 flag，dim rule
`NonMetric`，无回调，故能进 manifest）。它覆盖 kernel 的 8 个方案
（snake_case 名，含 2 个自适应行），是 **manifest-only** 的查询，只给强阶、
没有弱阶——弱阶仍由宿主侧 `sde_order`（§8.9.1）给出，两者互补。

> **实现状态（2026-11-05）**：SDE 的宿主侧求解器（`sdeint_diag`/
> `sdeint_general`/`wiener`/`sde_order`/`sde_ensemble_seed`）已实现并有
> 10/10 集成测试（`sde_language` 源码级 5/5）；`Sde` 模块 re-export 这五个
> 入口并声明选项 ADT，checker 已实现 `RateOf`/`DiffusionOf`。能力行
> `core.sde` 在本构建为 enabled（`Rssn.Capabilities` 可见）。**源码级
> 策略构造已验证可用**：`Sde.Fixed({ structure = Sde.Diagonal, method =
> Sde.EulerMaruyama, … })` 通过检查并跑出路径（`Fixed` 与 `Adaptive`、
> `Diagonal` 与 `General(m)` 都验过）；名义变体构造器可以从 `Sde` 导入。
> 透明类型别名（`type X = { … }`）的类型名现在也能经 `import { X }`/`open`
> 引入（task-22 已闭合，例如 `Physics.Ising.Params` 可直接注解；限定名
> 同样可用，§7.2.2/§8.20），因此 SDE 选项 ADT 与其它结构别名类型没有剩余
> 导入缺口。`sde_wiener`/`sde_levy_areas` 的 manifest 签名始终是精确的。

Aevia 仓库**不允许**出现按算子名手写的绑定文件，也**不保留**
`signatures.overlay.json` 的副本。只允许：

```
tools/gen_rssn_module.aev        # 生成器
```

`signatures.overlay.json` 是 RSSN 的构建输入，overlay 在 RSSN 侧已并入
`OpInfo.overloads`；宿主从 manifest 读到的就是应用后的精确签名，
`signature_source` 只用于标注。

CI 检查：生成器输出与提交的生成文件一致；manifest 快照变化必须显式更新。

### 13.2.3 命名空间

- 普通算子：`Rssn.<name>`（`Declared|Overlay|Derived` 都生成 typed；
  只有 `None` 与插件算子走 `Rssn.dynamic`）。
- 规则集限定：`Rssn.<rule_set>.<name>` 只是命名空间；运行时仍按 name/arity。
- 插件：`Rssn.<plugin>.<op>`。
- 动态：`Rssn.dynamic("diff", args)   # args: Tensor<1, Value>`。RSSN 的
  `call_value` 只按 name/alias 查找，**不支持** `rule_set/name` 限定名；
  限定名是 Aevia 的命名空间，不是运行时键。要钉死重载用
  `Rssn.call_key(key, args)`，`key` 来自 `Rssn.overloads(name)` 或
  `OpInfo.keys`。

名字冲突时，短名只保留**最稳定**的那个（`Stable` 优先，`since` 早者
优先）；其余必须用限定名。生成器在 manifest 中记录短名映射。

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

`Value` 是**动态边界**的类型，不是核心语言的隐式子类型。核心语言仍然是
静态的；`Value` 只出现在 `Rssn.dynamic`、插件、FFI、反射 API 中。

**可入项图性**：这张表由 `manifest.value_kinds` 给出，Aevia 不维护第二份。
本次测量（17 个变体）是：

- `in_term_graph = true`（10 个）：`VInt`/`VReal`/`VFloat`/`VBool`/
  `VString`/`VSymbol`/`VQuantity`/`VTensor`/`VTerm`/`VDyn`；
- `in_term_graph = false`（7 个）：`VComplex`（仅虚部为 0 时降为 `Float`
  入图，否则 `Unsupported("complex literals")`）、`VUnit`（单位是定义不是
  数：`term_of` 拒绝它，过边界用 `quantity(value, unit)`；只有已声明的单位
  参数位置才直接收 `Value::Unit`）、`VSparse`/`VRecord`/`VVariant`/
  `VFunction`/`VOpaque`（`term_of_value` 返回 `Unsupported`）。

`false` 的变体只能喂给有 `eval_dyn` 的算子，或作为返回值；生成器不能假设
所有 `Value` 都能参与重写。

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

- `Rssn.call` 是 `Rssn.dynamic` 的别名，只按短名/别名查找；不支持
  `rule_set/name` 限定名。短名歧义报 `E1002`。
- `Rssn.call_key` 是“不解析、直接执行”的入口；它收 name +
  `signature_hash`（32 hex 字符），两者合起来就是 `OpKey` 的可写形式。
  `signature_hash` 全零表示“该算子没有声明重载”。key 与当前 manifest 不符
  时报 `E1005`（提示重新生成模块）；kernel 侧参数偏离钉住签名时报
  `UnknownOp`，在 Aevia 边界变成错误值。
- `Rssn.resolve_overload` 暴露运行时选择结果，供缓存/诊断用：成功返回
  `"rule_set.name@signature_hash"`；并列与“没有签名接受”都报
  `OpError::Ambiguous`（后者 `count` 是签名总数）。
- `Rssn.overloads` 返回每条 overload 的渲染签名文本。
- 参数用 `to_value(x)` 注入；返回值用 `from_value(v)` 转回静态类型。
- 没有签名（`SignatureSource::None`）的算子、插件算子、实验性入口走
  `dynamic`。标准集当前没有 `None`，所以 `dynamic` 在标准集上只是插件
  通道。
- `type_rep<T>()` 保存检查器解析出的具体类型；它是不透明值，普通记录不能
  伪造。`Rssn.typed` 执行动态调用后按该见证检查表示、形状及量纲；不匹配或
  缺少证据返回 `Err(OpError)`。成功结果仍是 `Value`，通过 `from_value<T>`
  验证并转换；未经运行时类型字典具体化的泛型不能生成 `TypeRep`。
- 高阶算子用 `Rssn.call_ho(name, args, fns)`；`to_callback` 从当前会话的
  已检查源码闭包和具体函数类型生成不透明回调。当前桥支持具体数值、`Bool`、
  `String` 及递归张量和闭合记录，并检查已知形状、量纲及空容器的独立元素证据；
  当前数值求解器的回调仅用 `Float` 标量与向量。内建或其他 native 函数需先包装
  成源码 lambda。
  回调在原线程同步执行，外层调用结束即失效；跨线程调用和同作用域重入被拒绝，
  回调错误经 `OpError` 传回。空列表就是一次动态调用。
- `Rssn.register`/`Rssn.register_plugin` 接受 host `Plugin`；源码插件由
  `@op`/`@rule` 与 `load_plugin` 发布（§13.5.2）。`core.plugin.source` 在有
  进程内插件的构建中为 `enabled = true`；`core.callback` 表示受限回调桥可用。

### 13.3.3 错误

`OpError` 是结构化变体：

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

`E1003` 是“没有匹配的算子/重载”，携带候选签名列表；`E1001` 是
“后端 capability 缺失”，携带 capability 名和最小 RSSN 版本。

---

## 13.4 重载、签名与解析

### 13.4.1 重载

RSSN 允许同名多签名，判定规则在 `kernel::overload` 里：

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

- 每个参数单独分类，整个调用的分数是**最弱**的那个参数；
- `Int -> Real`、`Int|Real|Float -> Complex` 是 `Widened`，反向不成立；
- 同构造器但元素/量纲未被签名钉死是 `Shaped`；类型变量和 `Dyn` 是
  `Generic`；
- `best` 内部并列最高 → `Ambiguous`，不静默取第一个；
- `score_types`/`best_types` 与 `score`/`best` 共用同一个取分折叠：Aevia
  在编译期解析一次，得到的数与 kernel 运行时会得到的是同一个。

**当前标准集的实际情况**（本次测量）：1382 个算子分布在 1382 个唯一名字
上，每个名字只有 1 个注册；但**每个注册内部有 1–5 条重载**（1201 个 1 条、
178 个 2 条、2 个 3 条、1 个 5 条），因为有 `eval` 的算子同时有数值与符号
两条，overlay 再补精确签名。所以要把两件事分开：

- **注册选择**（`resolve_op`）在 `fitting.len() == 1` 处直接返回，对标准集
  不做评分；跨注册并列只可能由插件触发。
- **重载选择**（`resolve_overload` → `overload::best`）对注册内的 1–5 条
  签名**真的执行**：`sin(0.5)` 选 `(Real) -> Real`，`sin(x)`（`x` 是
  `Symbol`）选 `(Term) -> Term`。但实参本身是 `Term`/`Dyn` 时，数值重载
  的 `Real` 也会以 `(_, Dyn(_)) → Generic` 通过，两条同分 → `Ambiguous`。
  所以 `resolve_overload` 只适合具体类型的实参；符号表达式用
  `OpInfo.keys` 的 key。

`call_value` 按 name/arity 选注册，然后调 `check_args`：没有任何 overload
接受参数时返回 `OpError::Type`。但符号重载的 `Dyn<term>` 对任何值都是
`Generic`，所以 1350 个 derived 算子几乎总能通过——这条检查只在签名全部
具体（`eq`/`lit`/`pow`）时才真正拦得住。它不在数值/符号之间做选择（那由
算子的 `eval` 决定）。

Aevia 的静态解析规则（与 RSSN 的格同构，供 Aevia 自己的重载/插件使用）：

1. 按 arity 过滤；
2. 逐参数分类，取最弱；
3. 分数最高且唯一 → 选中；
4. 并列最高 → `E1002`；
5. 没有匹配 → 尝试 `Dynamic`；仍失败 → `E1003`。

运行时三条入口，职责不同：

- `Session::call_key(&OpKey, …)`（固有方法）**不重新解析**，按
  `rule_set` + `signature_hash` 找注册；它钉死重载，并要求这些参数能被
  钉住签名接受（`overload::score(pinned, args).is_some()`），否则报
  `UnknownOp`。
- `Session::call_value(name, …)` 按 name/arity 找注册；多个候选时用
  `overload::score` 选最具体，并列报 `OpError::Ambiguous`；参数不被任何
  overload 接受时报 `OpError::Type`（`check_args`）。
- `Kernel::call(&OpKey, …)` 转发 `call_key`，因此**也钉死**重载；通过
  `dyn Kernel` 传递不会丢信息。

`Session::resolve_overload(name, args)` 用于“先解析、再缓存”：

- `best` 为 `Unique` → 返回带 `signature_hash` 的 key，可直接给 `call_key`；
- 算子完全没有签名 → 返回零 hash 的 key，`call_key` 接受它；
- 并列 → `Err(OpError::Ambiguous)`，**不会**给出不可执行的零 hash key；
- 没有任何签名接受这些参数 → 也返回 `Err(OpError::Ambiguous)`（`count` 是
  签名总数）；它不重新推导单个签名的拒绝理由。

所以 `resolve_overload` 的返回值总是可以安全地交给 `call_key`。

`OpKey.signature_hash` 是 `Signature::hash()` 的前 16 字节（确定性 bincode
的 BLAKE3），伪造/过期/参数偏离的 key 返回 `UnknownOp`。

### 13.4.2 签名覆盖

覆盖率是 100%：本次测量 1382 个算子都有签名（`0 unexplained`，32 个
rule set）。签名有四个来源，优先级从高到低：

1. `Declared`：`OpDescriptor::new(...).signature(...)`，算子自己写（7 个）；
2. `Overlay`：`signatures.overlay.json` 里人工写的修正（25 个；在 RSSN 侧
   并入 manifest，Aevia 不持有副本）；
3. `Derived`：`kernel::infer::derive` 从注册信息推出（1350 个）；
4. `None`：叶子/无调用形式，`dynamic_reason` 说明原因（标准集没有）。

规则：

- `Derived` 是**推导**，不是猜测：它只读 `eval`/`PREDICATE`/`Binder`/
  `Variadic`/`LEAF`，不读算子名；所以它是“保守但可靠”的。
- 精度不够时写 overlay，而不是改推导器。overlay 只应该出现在“这条签名
  必须精确”的算子上（张量、符号、单位参数）。
- CI 的 `signature-audit` 报告覆盖率与来源分布；`Derived` 占比高不阻止
  构建，但阻止把对应算子标成“精确”。
- 插件用 `Registry::register_op` 声明签名；`Registry` 拒绝“既无签名又无
  `dynamic_reason`”的注册。

这保证“能力完整”和“类型精度”可以分开推进，但不会有算子被静默遗忘。

---

## 13.5 插件与用户扩展

### 13.5.1 插件

插件不是新语法，而是**普通 Aevia 包 + 清单标记 + 属性**：

```toml
# aevia.toml
[plugin]
kind = "aevia"          # 进程内；对应 RSSN 的 PluginKind::InProcess
abi = 1
```

`kind` 的两个取值对应 RSSN 的 `PluginKind`：

| `[plugin] kind` | RSSN | 加载方式 |
|---|---|---|
| `"aevia"` / `"in-process"` | `PluginKind::InProcess` | `Session::register(Box<dyn Plugin>)` |
| `"cdylib"` | `PluginKind::Cdylib` | `Session::load_plugin(path)`（需 `plugin-cdylib`） |

`cdylib` 的 C ABI 是**字节缓冲**：入口符号 `rssn_plugin_v1`，参数与返回值
都是 bincode 编码的 `Vec<Value>`/`Result<Value, String>`，不是
`#[repr(C)]` 的 `Value` 镜像。`cdylib::load` 会校验 `PLUGIN_ABI` 和
`RssnPluginDesc`；`Session::load_plugin` = `load` + `register`。

这个模块由 `plugin-cdylib` feature 控制（默认关闭，`full` 才包含），因为
加载动态库会在宿主进程里执行任意代码、没有沙箱。Aevia 在加载前必须查
`manifest.features.plugin_kinds`：没有 `"cdylib"` 时报
`E1001("core.plugin.cdylib")`，而不是让加载器抛一个底层错误。

```aevia
module MyDomain

@op("mydomain/softplus")
fn softplus(x: Real[1]): Real[1] = ln(1 + exp(x))

@rule("softplus(x) => ln(1 + exp(x))")
let softplus_rule = ()

dimension MyDim
unit myunit : Length = 0.5 m
```

编译后注册到 RSSN。Aevia 的 `Rssn.register` 是 `Session::register(Box<dyn Plugin>)` 的包装；
`Rssn.register_plugin` 是同一函数的别名：

```aevia
Rssn.register(plugin)          # -> Result<PluginId, PluginError>
```

插件可以：

- 注册算子 + 签名 + eval 回调；
- 注册重写规则（declarative tier）；
- 注册量纲规则；
- 注册单位/基量纲；
- 注册 JIT lowering（可选）。

### 13.5.2 Aevia 函数作为 RSSN 算子

`@op` 把 Aevia 函数发布为 RSSN 算子，`@rule` 发布为重写规则：

```aevia
@op("mydomain/softplus")
fn softplus(x: Real[1]): Real[1] = ln(1 + exp(x))

@rule("mydomain/sinh-exp")
fn sinh_exp(x: Real[1]) = (sinh(x), (exp(x) - exp(-x)) / 2)
```

**决定：用追踪而不是回调。** kernel 的扩展语言就是它的项语言：算子是定义
`name(a, b) := body`，规则是重写 `name: lhs => rhs`。Aevia 函数以**追踪**变成
它们：用带各参数已检查量纲的新符号调用函数，它构造的项就是定义体。这不需要
第二个编译器，并继承 kernel 的全部能力——`mydomain/softplus` 成为定义之后，
kernel 可以调用它、对它求导、穿过它化简与积分、数值求值，这是不透明的宿主
回调做不到的（回调也无法跨线程，`Rc` 解释器不是 `Send`）。

- 代价是被检查的：结果依赖于对参数的控制流（`if x > 0`、对值的 `match`）或
  做 IO 的函数追踪不出项，注册报 `E1006` 并给出原因。
- 规则函数返回二元组 `(lhs, rhs)`，参数就是规则的模式变量；`lhs` 必须是算子
  应用（追踪时会被局部化简，所以 `x * 0` 不是合法的左端）。
- 发布发生在模块的函数绑定之后、顶层 `let` 求值之前，所以顶层绑定已经可以经
  `Rssn.dynamic("mydomain/softplus", …)` 调用它。每个加载的模块注册一个进程内
  插件 `aevia/<module>#n`，属于当前线程的 kernel 会话（§13.6）。
- **源码算子是一等算子（2026-11-08）。** 发布顺序是：先把每个 `@op` 追踪成
  kernel 定义，并把检查得到的参数/结果量纲作为该算子的**量纲签名**一起注册
  （rssn `Registry::register_dim_signature`：参数与结果是签名变量的单项式，
  每次使用时由实参量纲求解变量，`fn area(r: Real[d]): Real[d^2]` 的
  `demo/area(r)` 在 kernel 的量纲推断里是 `dim(r)^2`）；然后把模块里该函数的
  绑定换成包装：**全部实参是数值**（或无自由符号的闭式项）时执行 Aevia 函数体，
  **有符号实参**时构造算子结点 `demo/softplus(x)`（kernel 项，按句柄携带），
  结果量纲由签名给出，实参量纲与签名冲突是 `E0401`；最后才追踪 `@rule`，所以
  规则可以谈论源码算子（`fn r(x: Real[1]) = (softplus(-x), softplus(x) - x)`）。
  `diff`/`simplify`/`integral` 等请求作用在算子结点上时，kernel 看到的是算子、
  它的定义和关于它的全部规则。kernel 拒绝插件时恢复原绑定。`Rssn.dynamic`
  的结果用 `show` 显示所携带的数或项及其量纲见证（`repr` 仍是不透明的
  `<Value>`，因为它不能从文本重建）。
- **动态加载**：`load_plugin(path) : String -> IO (Result<Int, String>)` 在运行期
  加载、检查、求值一个源码文件（含其导入），发布其中的 `@op`/`@rule`，返回发布
  数量；需要 `fs` 能力。

`@op("mydomain/softplus")` 里的 `mydomain/` 是**算子名的一部分**（插件
命名空间），不是 `rule_set/name` 限定符；kernel 的定义头允许 `/` 分隔的标识符
段，`call_value` 按完整字符串查找。

### 13.5.3 安全

- 插件必须签名；默认只加载 registry 信任的插件。
- 插件声明 capabilities（`fs`、`net`、`unsafe`）；未声明的能力在沙箱中
  被拒绝。
- 插件 ABI 版本不匹配报 `E1006`。

---

## 13.6 量纲与单位：统一到 `rssn::dim`

### 13.6.1 规范层

`rssn::dim` 提供：

- `DimTerm`：基量纲/变量的有理指数乘积，结构化访问
  `factors(&arena)`/`vars(&arena)`；
- `DimArena`：基量纲/变量 interning，可快照/恢复；
- `UnitRegistry`：单位名、别名、精确尺度 `Scale { rat, pi }`、仿射
  offset、前缀、显示、类别、since；
- `Dimension`：**可打印快照**，指数是字符串；精确比较/计算用 `DimTerm`，
  不要解析 `Dimension.factors`。

Aevia 的 `Dim` 是 `DimTerm` 的类型级镜像；两者的规范形、基名、指数运算
必须一致。编译器不复制单位表；`Units.*` 是 `manifest.units` 的生成视图
（`UnitValue` 已带别名/前缀/类别/since，不必再读 `UnitRegistry`）。本次
测量 `manifest.units` 有 82 项。单位属于 **session**：
`Session::register_unit(UnitDef)` 写该 session 的 registry，
`KernelManifest::build_with_units` 才把它写进 manifest；进程级
`KernelManifest::build` 看到的是进程表。插件注册的单位只属于安装插件的
session。

结构化入口是 `Config::dimension_term(symbol, DimExpr)`：它不解析文本，未知
基名会被 intern，结构化假设在文本假设之后应用（同名时结构化胜）。
`DimExpr::to_term` 写 term；`DimExpr::try_from_term` 是 term 可表示量纲的
精确逆（`Length^n` 读回 `Length^n`）；`from_term` 是全函数版，读不回时给
`DimExpr::Any` 而不是错误量纲。

`OpInfo.dim_rule` 的 `DimRuleInfo` 本轮新增 `RootOf { degree, index }`：
结果量纲是**第 `index` 个参数**量纲的 `degree` 次根。旧 `Root(n)` 固定取
第 0 个参数的根，表达不了 `sde_wiener(seed, m, steps, h)` 的
`sqrt(h)`（`h` 是第 4 个参数）；它的 manifest wire form 是
`{"root_of":{"degree":2,"index":3}}`。宿主读 `dim_rule` 时应按变体匹配，
不要假设只有 `Root`/`Same`/`Product`。

自定义基量纲由 `Config::base_dimension(name)` 声明（幂等）：它把基名送进
`DimArena`，`Dim` 的内部表示从固定的 7 个 SI 指数扩成“7 个 SI + 有序
旁表”，`expr_to_dim` 接受任意基名。这是 `unit EUR : Currency` 能成立的
前提——`dimension_term` 只能为 arena 里已有的基名产出 `DimTerm`。

### 13.6.2 单位声明

```aevia
dimension Currency            # 自定义基量纲
unit USD : Currency = 1       # 基单位
unit EUR : Currency = 1.08 USD
unit myunit : Length = 0.5 m  # 内置基量纲
```

生成器为每个 `dimension` 在 `Config` 上声明一次
`base_dimension("<Name>")`，然后才注册单位；否则单位注册成功但 `DimTerm`
无法生成（arena 里没有该基名）。

- `dimension` 注册到 `DimArena`（编译期经 `DimExpr`，运行时经
  `Config::dimension_term` / `Graph::assume_dimension_value`）；`unit`
  注册到 `UnitRegistry`。
- 默认导出；`private` 只在本模块可见。
- 导出信息进入 manifest；下游包可以直接 `import` 并使用。
- 用户单位可以是仿射的（`unit °X : Temperature = 1 K offset 273.15`），
  但仿射单位只能单独使用。
- 自定义基量纲：`Config::base_dimension` 声明基名，`expr_to_dim` 接受任意
  基名，`Dim` 用旁表保存非 SI 指数；所以 `dimension Currency` +
  `unit EUR : Currency` 都可用。`features.user_base_dimensions` 是
  **硬编码 true**（不是 feature 门控），所以每个构建都支持；唯一按构建
  变化的是 `jit*` 与 `plugin_kinds`。
- 基名经 `Config::base_dimension(name)` 声明即可：`prepare` 在**克隆的**
  图上 `define_base_dimension`；`DimArena` 是写时复制，第一次写就 detach，
  所以不会污染模板或并发请求（§13.6.6）。

### 13.6.3 `Dyn`

`Dyn<ρ>` 的运行时量纲是 `DimTerm`，不是字符串。`dyn`/`check` 是类型指导
原语：具体 `d` 在编译期物化为 `DimTerm`；泛型用 `dyn_with`/`check_with`
显式传入。`check` 返回 `Result`。

### 13.6.4 `Point` 与仿射

`Point[d]` 是静态包装；运行时是 `Float` + `UnitId`。`UnitRegistry` 的
`affine`/`offset` 是唯一事实来源。`Point` 的算术规则在 Aevia 层实现，
RSSN 的 `quantity`/`convert` 负责显示/换算。

### 13.6.5 物理常数

`manifest.constants` 是 `Vec<ConstantValue>`（30 条、18 条精确）：

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

`Units.Constants` 由它生成，规则：

- 符号原样使用（`c`、`g_n`、`h`、`hbar`、`k_B`、`N_A`、`e`、`G`、
  `alpha`、`R`、`F`、`sigma_SB`、`m_e`、`m_p`、`m_n`、`a_0`、`R_inf`、
  `mu_0`、`epsilon_0`、`mu_B`、`M_u`、`m_u`、`T_tp`、`atm`、`G_0`、`K_J`、
  `R_K`、`Phi_0`、`K_cd`、`dnu_Cs`）；
- `exact = true` 的常数没有 `uncertainty`；`exact = false` 的有；
- 常数是**值**，不是单位：不能出现在 `quantity(v, u)` 的单位位置；
- `e` 是基本电荷，与 `Math.e` 同名。`open Units.Constants` 与 `Math.e`
  同时可见时，`e` 必须限定（`Math.e` / `Units.Constants.e`）。

### 13.6.6 线程与 arena 隔离

`DimArena` 在模板里是 `Arc<RwLock>`，`Graph::clone` **共享**它；写入时
`with_dim_arena_mut`/`define_base_dimension` 先看 `Arc::strong_count`，大于 1
就克隆一份再写（写时复制）。因此：

- `Config::base_dimension(name)` 是安全路径：`prepare` 在克隆图上
  `define_base_dimension`，第一次写就 detach，请求之间互不影响；
- 不定义任何基量纲的请求共享模板的 arena，不付拷贝成本；
- 不要直接对 `Session` 的模板图调用 `define_base_dimension`——那会让后续
  请求都看到这个基名；
- 跨线程仍用 `Session::fork`/`merge`，不共享 `Session`（它是 `RefCell`）。

---

## 13.7 中心包 registry

Aevia 现在就有中心 registry 的设计，不把它推到“以后”。registry 是
**内容寻址 + 签名 + 能力声明**的包索引。

> **实现状态（2026-11-04）**：**设计已完成，实现尚未开始。** 当前包管理只
> 支持 **path 依赖**（`aevia.toml` 的 `[dependencies]` + `aevia.lock`）；
> registry 依赖、git 依赖和 `publish`/`yank`/`aevia add <version>` 都会报
> `E0605`（`crates/aevia-core/src/manifest.rs` 明确“there is no registry”）。
> 本节描述的是待实现的协议，不是可用功能；`spec/09 §9.11` 的状态说明与
> 本节一致。

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

```
GET /index.json                     # 全部包名 + 最新版本 + 摘要
GET /pkg/{name}/{version}.tar.zst   # 内容寻址的包
GET /pkg/{name}/{version}.sig       # 签名
GET /search?q=...                   # 搜索
POST /publish                       # 发布（签名）
POST /yank/{name}/{version}         # 撤回
```

索引项至少包含：name、version、content_hash、deps、aevia_range、
rssn_api、capabilities、operators、units、dimensions、stability、yanked。

### 13.7.3 可复现

- 包内容哈希覆盖源码、manifest、生成的 `Rssn` 模块签名；
- 构建不执行任意脚本；
- 依赖解析使用内容哈希 + semver；
- registry 镜像必须可验证签名。

---

## 13.8 稳定性与 capability（取代 v1/v2）

### 13.8.1 稳定性标记

```aevia
@stable
@experimental
@deprecated("use foo2")
```

- `Stable`：major 内签名/语义不变；
- `Experimental`：可改，但旧名至少保留一个 minor；
- `Deprecated`：至少一个 minor 的警告期。

### 13.8.2 Capability

后端能力用 capability 名描述。**名字的唯一来源是一张 capability 表**：
它由 Kernel Manifest 的 `Features` 派生，`Rssn.Capabilities` 只列出
`enabled = true` 的行（`aevia-rssn` 的 `Capabilities::table()`）。当前构建
的完整表（本次测量）：

```
core.dynamic_call
core.jit                    # false：默认构建无 jit feature
core.jit_branch             # false（同上）
core.jit_index              # false（同上）
core.parallel_symbolic
core.higher_order
core.structured_results
core.sde                   # 由 features.sde 派生；SDE 噪声原语可用
core.callback              # true：同线程作用域回调，支持具体结构化签名
core.plugin.source         # 源码级 @op/@rule/load_plugin（需 in-process 插件）
core.plugin
core.plugin.in-process
core.plugin.cdylib         # 仅 plugin-cdylib 构建
dim.exact_units
dim.affine_units
dim.user_base_dimensions
```

本构建 `Rssn.Capabilities` 实际列出的 enabled 行（2026-11-04 实测）：
`core.dynamic_call`、`core.parallel_symbolic`、`core.higher_order`、
`core.sde`、`core.structured_results`、`dim.exact_units`、
`dim.affine_units`、`dim.user_base_dimensions`、`core.plugin`、
`core.plugin.in-process`——jit 三行与两个 host-bridge 行是 `false`，不会出现
在 `Rssn.Capabilities` 里。

另有**规范声明、但当前 manifest 没有对应 `Features` 字段**的名字；它们可以
出现在诊断里，`Rssn.Capabilities` 不会列出：`core.symbolic`、`core.numeric`、
`core.tensor`、`core.linalg`、`core.ode`、`core.pde`、`core.fft`、
`core.stats`、`core.units`、`core.assume.general`、`core.gpu`、
`core.distributed`、`io.binary`。

**存储类稀疏**（`storage.sparse`，§13.9.5/§8.4.1）原先是一个命名缺口，自 2026-11-07
起**已启用**：`Value::Sparse` 是运行时值，`Csr`/`Csc`/`Coo` 可构造；块格式
`Bcsr`（类型里也写作 `Block`）随后也成为真实格式（`to_bcsr`、`convert<Bcsr>`，
见 §8.4.1），不再有 `storage.sparse.block` 这个缺口，也不再报对应的 `E1001`。
它是 Aevia 自己的存储类，不由 RSSN manifest 的
`Features` 字段发布，所以不出现在 `Rssn.Capabilities` 里。

Aevia 编译器在类型检查后检查 capability；缺失时：

- 编译期：如果静态可知（如 `@jit` 目标），报 `E1001`；
- 运行时：动态调用返回 `OpError.Unsupported`，Aevia 转成 `E1001`。

**不删语言特性**。例如 GPU 未实现时，`on(Gpu(0), ...)` 仍类型正确，运行
时报 `E1001("core.gpu")`；不会出现“这个语法只有 v2 才有”。

---

## 13.9 已决定的扩展设计

以下项目以前被标为“v2 再评估”；现在全部做出决定，并进入规范。

### 13.9.1 形状细化（shape refinement）

**决定：支持，作为 refinement，不是完整依赖类型。**

- `Tensor<n, τ>`：只带秩，形状存在（当前形式）。
- `Tensor<[n1, ..., nk], τ>`：形状细化；`n_i` 是类型级 `Nat`，可用
  `?` 表示未知。
- `Matrix<n, m, τ>` 是 `Tensor<[n, m], τ>` 的别名。
- 张量字面量自动获得细化形状；`zeros_of<[3,4]>()` 等构造器可显式指定。
- `matmul : Tensor<[m,k],τ> × Tensor<[k,n],τ> -> Tensor<[m,n],τ>`；
  秩-only 版本保留，运行时检查。
- `reshape<[n1, ..., nk]>(a)` 始终返回 `Result`；形状静态已知时元素数在
  编译期检查、结果恒为 `Ok`，否则运行时检查。`reshape_dyn` 总是运行时检查。
- 索引：细化形状 + 字面量索引 → 编译期越界检查；动态索引仍运行时检查。
- 细化 → 秩-only 是安全隐式转换；秩-only → 细化用 `refine_shape`
  返回 `Option`/`Result`。
- 类型级 `Nat` 支持字面量、`+`、`*` 和线性等式；不做一般递归/证明。
  这足以表达矩阵乘法、reshape、拼接，不需要完整依赖类型。

### 13.9.2 `Point` 张量

**决定：支持 `Tensor<shape, Point[d]>`。**

- 元素级 `+`/`-` 按点规则：`Point - Point -> Float[d]`，
  `Point + Float[d] -> Point`，`Point + Point` 报 `E0405`。
- `mean` 只接受 `Float`/`Real`；点的平均用 `centroid : Tensor<1,Point[d]> -> Point[d]`。
- 温度场可以直接是 `Tensor<[nx,ny], Point[Θ]>`；不再用“K 值 + 约定”。
- `Point` 张量的存储是 `Float` + `UnitId`；`Point` 的仿射偏移只在
  元素级运算时处理。

### 13.9.3 `Table` 行多态 schema

**决定：`Table<ρ>` 使用行多态静态 schema，动态 `Table` 仍保留。**

- `Table<ρ>`：`ρ` 是行类型，字段名 + 类型 + 量纲。
- `read_csv<ρ>(path)` 从表头单位推断字段类型；无法推断时报
  `DataError.Schema`，用户可给显式 `ρ`。
- `column` 是类型级字段查找：`column(t, "time")` 的结果类型由 `ρ` 的字段
类型决定。
- `select`/`join`/`rename` 保持行多态；新增字段要求 `lacks` 约束。
- 动态 CSV（未知 schema）返回 `Table<Dynamic>`；`Dynamic` 是特殊记录类型，
  不是普通字段可投影的行。`to_table<ρ>` 做运行时校验。
- 这样数据管道和核心语言一样有静态 schema，不再只是“读入后赶紧转记录”。

### 13.9.4 设备、GPU、分布式

**决定：设备是运行时值，不是张量类型参数。**

- `Device = Cpu | Gpu(Int) | Distributed(Cluster)`。
- `Tensor` 类型不变；值带 `device` 元数据。
- `to_device : (Tensor<n,τ>, Device) -> IO (Tensor<n,τ>)`。
- `on : (Device, () -> a) -> IO a` 是作用域执行；纯计算可以 `on` 到
  设备，分配/传输是 IO。
- `@jit(target = Gpu(0))` 可指定 JIT 目标；没有 `core.gpu` 时运行时报
  `E1001`。
- 分布式张量用 `Sharded<Mesh, Tensor<...>>` 库类型；`Mesh` 是运行时
  拓扑，不是类型参数。这样不会为设备/分片生成类型组合爆炸。

> **实现状态（2026-11-08）**：`Device` 值、`to_device`/`on`/`device_of` 已
> 类型化；没有 `core.gpu` 时 `to_device`/`on` 报 `E1001("core.gpu")`，与
> 设计一致。`@jit(target = Cpu)` 与 `@jit` 等价；`@jit(target = Gpu(n))` 与
> `@jit(target = Distributed(..))` 被语言接受，在检查期（目标静态可知）报
> `E1001("core.gpu")`/`E1001("core.distributed")`；其他参数是 `E0210`。
> 属性参数里"构造器作用于常量"（`Gpu(0)`）算编译期常量（§2.2.7）。
> `Gpu`/`Distributed` 没有后端，只是可构造的运行时值。

### 13.9.5 稀疏与布局

**决定：`Sparse<F, shape, τ>` 是核心类型构造器，`F` 是格式类型。**

- 格式：`Csr`、`Csc`、`Coo`、`Bcsr`（`Block` 是它的同义写法）；当前是封闭集合，用户
  自定义格式要等 RSSN 开放 `Format` kind（当前插件只能注册算子/单位）。
- `Layout` 类（设计上）：`nnz`、`to_dense`、`from_dense`、`matmul`、
  `transpose`；当前实现是内建函数，不是可实例化的类型类。
- `@` 按布局分派：dense×sparse、sparse×dense、sparse×sparse。
- `Sparse` 与 `Tensor` 之间显式转换；不隐式互转。RSSN 的 `TypeExpr::Sparse`
  只有 `rank`，所以 Aevia 的 `Sparse<F,shape,τ>` 在边界降为 rank + 运行时
  shape；细化形状由 Aevia 自己检查。
- 稀疏不是库补丁，而是类型系统的一部分；`shape` 细化同样适用（目前检查到秩）。

> **实现状态（2026-11-07）**：`Sparse<F, shape, τ>` 是独立的类型构造器，运行时
> 有真正的 `Value::Sparse`（`Csr`/`Csc`/`Coo`/`Bcsr`；`f64` 缓冲与精确装箱两类存储，
> 精确值不舍入），`storage.sparse` 因此**已启用**。`@` 的四种布局分派
> （sparse×dense、dense×sparse、sparse×sparse，加 dense×dense）在 AST 与 IR
> 求值器、在 checker 里都已实现；`Sparse` 与 `Tensor` 之间只有显式转换
> （`from_dense<F>`、`to_dense`、`convert<G>`），checker 不会让一个替代另一个。
> 稀疏值过 RSSN 边界不再稠密化（`aevia-rssn::Val::Sparse` ↔ `Value::Sparse`），
> `term_of_value` 仍拒绝它进入项图。具体的内建函数、精确性与错误码见 §8.4.1。
> `Block`/`Bcsr`（块压缩稀疏行，`br × bc` 稠密块的 CSR；块内允许零填充、
> `nnz` 只数逻辑非零、块形状由 `to_bcsr(A, br, bc)` 显式给出或按填充率自动选取）
> 已是真实格式，`@` 在 `f64` 类用块内核（§8.4.1）。
> 预条件 `cg_with`（`Precond`：Identity/Jacobi/BlockJacobi/Ilu0/Ic0/Auto，块格式用
> rssn 的块 Jacobi 与块 ILU(0)）、`spsolve` 对对称正定矩阵的 Cholesky 优先路径、
> 稀疏切片（`A[i, :]`、`A[r0..r1, c0..c1]`，不稠密化）均已实现，细节见 §8.4.1。
> **剩余缺口**：形状细化只检查秩；格式仍是封闭集合（用户自定义
> `Format` 要等 RSSN 开放）。

### 13.9.6 效果与能力

**决定：不做用户自定义 effect handlers；`IO` 保持单一类型。**

- 扩展点是**算子/插件/registry**，不是语言级效果系统。
- `IO` 动作可以声明所需 **capability**（`fs`、`net`、`env`、`rand`、
  `gpu`）；这是运行时权限，不是类型参数。
- `with_cap(cap, thunk)` 在沙箱中执行；没有能力时 IO 动作失败。
- 这样避免 effect row 对全部标准库签名的侵入，也避免“v2 才有自定义
  效果”的断层。需要 DSL/代码生成时用插件/算子，不用宏。

> **实现状态（2026-11-06）**：`with_cap(caps, thunk)` 与 `aevia run --caps`
> 已实现，语义见 §9.14（能力集按块收窄、嵌套取交集、缺失时 `E0902`）。它与
> §13.8.2 由 kernel `Features` 派生的后端能力表（`core.*`/`dim.*`）是两回事：
> 后者描述后端能做什么，前者描述程序被允许做什么。

### 13.9.7 宏与语法扩展

**决定：不提供用户宏；语法固定。**

- 扩展通过算子、插件、registry、`Rssn` 动态调用完成。
- 不引入 `macro_rules`/卫生宏/语法扩展，避免工具链分叉。
- 如果未来确实需要，必须以**独立、可选的预处理层**出现，不进入核心
  语法；当前规范不预留语法。

### 13.9.8 角度

**决定：`Angle` 不是基量纲；`rad` 无量纲，`deg` 是尺度。**

- `sin`/`cos` 接受 `Real[1]`；`deg` 字面量折叠为 `pi/180`。
- 不引入 `Angle` 基量纲，避免破坏 `exp(i*θ)`、`sin(ωt)` 等公式。

### 13.9.9 不确定度

**决定：`Measurement<d>` 是标准库类型，不是核心类型。**

- 独立误差传播：`Measurement<d> = { value: Float[d], sigma: Float[d] }`。
- 相关误差用 `Covariance<d,e>` 库类型；`linear_fit` 返回协方差。
- 核心语言只保证量纲；不确定度是库的领域模型。

### 13.9.10 其他已关闭的问题

| 问题 | 决定 |
|---|---|
| `det` 默认返回 `Dyn`？ | 否；无量纲 `det` + `det_unit` |
| `Real`/`Float` 混合？ | `Real` 吸收 `Float`（精确优先）；热路径显式 `Float` |
| `Dyn` 错误？ | 算术 `panic` + `try_*`；`check` 返回 `Result` |
| `RateOf` 嵌套？ | 支持标量、记录、元组、张量，递归定义 |
| `Table` 静态 schema？ | 是，`Table<ρ>` 行多态 |
| `@` 还是 `*` 做矩阵乘？ | 保持 `@`；`*` 逐元素 |
| `%` 取模？ | 否；`mod`/`rem` |
| `let` 默认递归？ | 否；`fn` 递归，`let rec` 显式 |
| 顶层 IO？ | 否；`main`/REPL |
| 中心 registry？ | 是；设计完成，实现待做（当前只有 path 依赖，§13.7） |
| 用户量纲别名导出？ | 是，默认导出，`private` 可隐藏 |

---

## 13.10 兼容性与迁移

- **语言版本**用 semver；不设“v1 语言/v2 语言”。major 才允许破坏性
  变更；minor 只加能力/算子/库；patch 只修 bug。
- **manifest 兼容**：`api_version` 不兼容 +1；Aevia 编译器 pin 范围；
  `manifest_hash` 变化使缓存失效，但旧签名仍可用到下一个 major。
- **算子弃用**：`Deprecated` 至少一个 minor 的警告；别名保留。
- **包 registry**：内容哈希 + 签名 + capability；yank 不删除内容，只
  阻止新解析。
- **迁移工具**：`aevia fix` 读取旧 manifest/签名，自动加限定名/别名；
  不强制用户重写代码。

---

## 13.11 错误码

| 代码 | 情况 |
|---|---|
| `E1001` | 后端 capability 缺失（`core.gpu`、`core.jit_branch` 等） |
| `E1002` | 算子/重载歧义 |
| `E1003` | 没有匹配的算子/重载 |
| `E1004` | RSSN `api_version` 不兼容 |
| `E1005` | manifest/签名哈希不匹配 |
| `E1006` | 插件 ABI/加载失败 |

这些错误属于**环境/后端**，不是用户程序类型错误；诊断必须给出
capability 名、最小 RSSN 版本和可执行的替代方案。
