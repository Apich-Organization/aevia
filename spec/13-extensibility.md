# 13 — 可扩展性与 RSSN 集成

本章是规范的一部分，并且**优先于**其他章节中任何“暂不支持/以后评估”的
表述。Aevia 不按 `v1`/`v2` 分层：语言能力一次设计到位；后端尚未实现的
部分用 **capability 诊断**（`E1001`）报告，而不是删掉语言特性或让用户
等到下一个大版本。

本章的详细后端接口清单在项目根目录
[`RSSN-INTEGRATION-REQUIREMENTS.md`](../RSSN-INTEGRATION-REQUIREMENTS.md)。
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
`units` 是 `UnitValue`，0.2.13 起带
`name`/`display`/`dimension`/`scale`/`offset`/`affine`/`aliases`/
`prefixable`/`category`/`since`，生成 `Units.*` 只读 manifest 即可。

`value_kinds: [ValueKindInfo]` 描述每个 `Value` 变体能否入项图
（`in_term_graph`）以及不能时的替代做法；生成器按它决定 façade 的参数
位置能不能接受该类型（§13.3.1）。

`constants: [ConstantValue]` 是物理常数表（30 条、18 条精确），字段
`name`/`symbol`/`value: ScaleValue`/`dimension: DimExpr`/`unit`/`exact`/
`uncertainty`/`source`；`Units.Constants` 由它生成（§13.6.5）。

`features.plugin_kinds` 说明这个构建能加载哪种插件（默认
`["in-process"]`；开 `plugin-cdylib` 后多一个 `"cdylib"`）。

### 13.2.2 生成规则

Aevia 构建时：

1. 读取 manifest；校验 `api_version`（不兼容报 `E1004`）。
2. 对每个 `OpInfo`：
   - `signature_source = Declared | Overlay | Derived`：生成
     `Rssn.<name>` 类型化声明（**每条 overload 一个**）；
   - `signature_source = None`：生成 `Rssn.dynamic`；
   - 别名：生成 `Rssn.<alias>`；冲突时用 `Rssn.<rule_set>.<name>`。
3. typed 声明同时记住 `OpInfo.keys` 里对应的 `OpKey`，调用走
   `Rssn.call_key(key, …)`，静态选择与运行时执行不会分歧。
4. 生成 `Rssn.Version`、`Rssn.ManifestHash`、`Rssn.Capabilities`、
   `Rssn.ValueKinds`、`Rssn.Constants`、`Rssn.Units`。
5. 生成 LSP 补全、文档、签名存根；存根里记录每条声明的来源。

当前 RSSN 0.2.13（`95099fa0`）的实测覆盖：1369 个算子 **100% 有签名**，
0 unexplained。来源是 **7 declared + 13 overlay + 1349 derived**；
overload 分布是 1190 个 1 条、177 个 2 条、2 个 3 条。

**精度分档**（生成器必须区分）：

| 来源 | 数量 | 精度 | 生成什么 |
|---|---|---|---|
| `Declared` | 7 | 精确 | typed |
| `Overlay` | 13 | 精确 | typed |
| `Derived` | 1349 | 形态级 | typed，但只有 `Real`/`Bool`/`Term` |
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
“100% typed”不等于“100% 精确”：1349 个算子的签名不区分
`Tensor`/`Quantity`/`Int`，Aevia 的 IDE 应把它们标成“保守推断”。

Aevia 仓库**不允许**出现按算子名手写的绑定文件。只允许：

```
tools/gen_rssn_module.aev        # 生成器
signatures.overlay.json          # RSSN 侧的签名修正；Aevia 只读镜像
```

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
0.2.13 的取值是：

- `in_term_graph = true`：`VInt`/`VReal`/`VFloat`/`VBool`/`VString`/
  `VSymbol`/`VUnit`/`VQuantity`/`VTensor`/`VTerm`/`VDyn`；
- `in_term_graph = false`：`VComplex`（仅虚部为 0 时降为 `Float` 入图，
  否则 `Unsupported("complex literals")`）、`VSparse`/`VRecord`/
  `VVariant`/`VFunction`/`VOpaque`（`term_of_value` 返回
  `Unsupported`）。

`false` 的变体只能喂给有 `eval_dyn` 的算子，或作为返回值；生成器不能假设
所有 `Value` 都能参与重写。

### 13.3.2 调用

```aevia
Rssn.call    : (String, Tensor<1, Value>) -> Result<Value, OpError>
Rssn.dynamic : (String, Tensor<1, Value>) -> Result<Value, OpError>
Rssn.typed   : (String, Tensor<1, Value>, TypeRep) -> Result<a, OpError>
Rssn.overloads       : String -> Tensor<1, (OpKey, Signature)>
Rssn.resolve_overload: (String, Tensor<1, Value>) -> Result<OpKey, OpError>
Rssn.call_key        : (OpKey, Tensor<1, Value>) -> Result<Value, OpError>
Rssn.Constants       : Tensor<1, ConstantInfo>
Rssn.Units           : Tensor<1, UnitInfo>
```

- `Rssn.call` 是 `Rssn.dynamic` 的别名，只按短名/别名查找；不支持
  `rule_set/name` 限定名。短名歧义报 `E1002`。
- `Rssn.call_key` 是唯一“不解析、直接执行”的入口；`key` 必须来自
  `OpInfo.keys`/`Rssn.overloads`。`signature_hash` 全零表示“该算子没有
  声明重载”。
- `Rssn.resolve_overload` 暴露运行时选择结果，供缓存/诊断用。
- 参数用 `to_value(x)` 注入；返回值用 `from_value(v)` 转回静态类型。
- 没有签名（`SignatureSource::None`）的算子、插件算子、实验性入口走
  `dynamic`。标准集当前没有 `None`，所以 `dynamic` 在标准集上只是插件
  通道。
- `Rssn.typed` 在运行时把 `Value` 转成静态类型 `a`，做类型与量纲检查；
  失败返回 `OpError.Type`/`OpError.Dimension`。
- 高阶算子用 `Rssn.call_ho(name, args, fns)`；`fns` 是 `Callback`，可以
  包装 Aevia 闭包。

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
score        : (Signature, Tensor<1, Value>) -> Option<Nat>
best         : (Tensor<1, Signature>, Tensor<1, Value>) -> Best
type Best    = | None | Unique(Int) | Ambiguous(Tensor<1, Int>)
```

- 每个参数单独分类，整个调用的分数是**最弱**的那个参数；
- `Int -> Real`、`Int|Real|Float -> Complex` 是 `Widened`，反向不成立；
- 同构造器但元素/量纲未被签名钉死是 `Shaped`；类型变量和 `Dyn` 是
  `Generic`；
- `best` 内部并列最高 → `Ambiguous`，不静默取第一个。

**当前标准集的实际情况**：1369 个算子分布在 1369 个唯一名字上，每个名字
只有 1 个注册；但**每个注册内部有 1–3 条重载**（1190 个 1 条、177 个 2 条、
2 个 3 条），因为有 `eval` 的算子同时有数值与符号两条。所以要把两件事分开：

- **注册选择**（`resolve_op`）在 `fitting.len() == 1` 处直接返回，对标准集
  不做评分；跨注册并列只可能由插件触发。
- **重载选择**（`resolve_overload` → `overload::best`）对注册内的 2–3 条
  签名**真的执行**：`sin(0.5)` 选 `(Real) -> Real`，`sin(x)`（`x` 是
  `Symbol`）选 `(Term) -> Term`。但实参本身是 `Term`/`Dyn` 时，数值重载
  的 `Real` 也会以 `(_, Dyn(_)) → Generic` 通过，两条同分 → `Ambiguous`。
  所以 `resolve_overload` 只适合具体类型的实参；符号表达式用
  `OpInfo.keys` 的 key。

`call_value` 按 name/arity 选注册，然后调 `check_args`：没有任何 overload
接受参数时返回 `OpError::Type`。但符号重载的 `Dyn<term>` 对任何值都是
`Generic`，所以 1349 个 derived 算子几乎总能通过——这条检查只在签名全部
具体（`eq`/`lit`/`pow`）时才真正拦得住。它不在数值/符号之间做选择（那由
算子的 `eval` 决定）。

Aevia 的静态解析规则（与 RSSN 的格同构，供 Aevia 自己的重载/插件使用）：

1. 按 arity 过滤；
2. 逐参数分类，取最弱；
3. 分数最高且唯一 → 选中；
4. 并列最高 → `E1002`；
5. 没有匹配 → 尝试 `Dynamic`；仍失败 → `E1003`。

运行时三条入口，职责不同：

- `Session::call_key(&OpKey, …)`（固有方法）**不解析、不校验参数**，按
  `rule_set` + `signature_hash` 找注册；钉死重载，参数由调用方保证。
- `Session::call_value(name, …)` 按 name/arity 找注册；多个候选时用
  `overload::score` 选最具体，并列报 `OpError::Ambiguous`；参数不被任何
  overload 接受时报 `OpError::Type`（`check_args`）。
- `Kernel::call(&OpKey, …)` 转发 `call_key`，因此**也钉死**重载；通过
  `dyn Kernel` 传递不会丢信息。

`Session::resolve_overload(name, args)` 用于“先解析、再缓存”：

- `best` 为 `Unique` → 返回带 `signature_hash` 的 key，可直接给 `call_key`；
- 算子完全没有签名 → 返回零 hash 的 key，`call_key` 接受它；
- 并列 → `Err(OpError::Ambiguous)`，**不会**给出不可执行的零 hash key。

所以 `resolve_overload` 的返回值总是可以安全地交给 `call_key`。

`OpKey.signature_hash` 是 `Signature::hash()` 的前 16 字节（确定性 bincode
的 BLAKE3），伪造/过期的 key 返回 `UnknownOp`。

### 13.4.2 签名覆盖

0.2.13 起覆盖率是 100%：1369 个算子都有签名（`0 unexplained`）。
签名有四个来源，优先级从高到低：

1. `Declared`：`OpDescriptor::new(...).signature(...)`，算子自己写；
2. `Overlay`：`signatures.overlay.json` 里人工写的修正（当前 13 条）；
3. `Derived`：`kernel::infer::derive` 从注册信息推出（当前 1349 条）；
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

`@op` 把 Aevia 函数包装成 `HostCallback`，注册为 RSSN 算子。
这样用户可以用 Aevia 写领域算子，同时享受 RSSN 的符号重写/数值内核：

```aevia
@op("mydomain/softplus")
fn softplus(x: Real[1]): Real[1] = ln(1 + exp(x))
```

限制：回调必须是纯的、线程安全的（或标记 `SessionBound`）；符号重写
规则必须显式注册，不能从函数体自动推导。

`@op("mydomain/softplus")` 里的 `mydomain/` 是**算子名的一部分**（插件
命名空间），不是 `rule_set/name` 限定符；运行时 `call_value` 仍按完整
字符串查找，不能拆成 `rule_set` + `name`。

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
（`UnitValue` 已带别名/前缀/类别/since，不必再读 `UnitRegistry`）。

结构化入口是 `Config::dimension_term(symbol, DimExpr)`：它不解析文本，未知
基名会被 intern，结构化假设在文本假设之后应用（同名时结构化胜）。

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
- 自定义基量纲（`111b71d0` 起）：`Config::base_dimension` 声明基名，
  `expr_to_dim` 接受任意基名，`Dim` 用旁表保存非 SI 指数；所以
  `dimension Currency` + `unit EUR : Currency` 都可用。
  `features.user_base_dimensions` 是**硬编码 true**（不是 feature 门控），
  所以每个构建都支持；唯一按构建变化的是 `jit*` 与 `plugin_kinds`。
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

后端能力用 capability 名描述：

```
core.symbolic
core.numeric
core.tensor
core.linalg
core.ode
core.pde
core.fft
core.stats
core.units
core.jit
core.jit.branch
core.jit.index
core.plugin
core.parallel_symbolic
core.gpu
core.distributed
```

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

### 13.9.5 稀疏与布局

**决定：`Sparse<F, shape, τ>` 是核心类型构造器，`F` 是格式类型。**

- 格式：`Csr`、`Csc`、`Coo`、`Block`、`Bcsr`；0.2.13 是封闭集合，用户
  自定义格式要等 RSSN 开放 `Format` kind（当前插件只能注册算子/单位）。
- `Layout` 类：`nnz`、`to_dense`、`from_dense`、`matmul`、`transpose`。
- `@` 按布局分派：dense×sparse、sparse×dense、sparse×sparse。
- `Sparse` 与 `Tensor` 之间显式转换；不隐式互转。RSSN 的 `TypeExpr::Sparse`
  只有 `rank`，所以 Aevia 的 `Sparse<F,shape,τ>` 在边界降为 rank + 运行时
  shape；细化形状由 Aevia 自己检查。
- 稀疏不是库补丁，而是类型系统的一部分；`shape` 细化同样适用。

### 13.9.6 效果与能力

**决定：不做用户自定义 effect handlers；`IO` 保持单一类型。**

- 扩展点是**算子/插件/registry**，不是语言级效果系统。
- `IO` 动作可以声明所需 **capability**（`fs`、`net`、`env`、`rand`、
  `gpu`）；这是运行时权限，不是类型参数。
- `with_cap(cap, thunk)` 在沙箱中执行；没有能力时 IO 动作失败。
- 这样避免 effect row 对全部标准库签名的侵入，也避免“v2 才有自定义
  效果”的断层。需要 DSL/代码生成时用插件/算子，不用宏。

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
| 中心 registry？ | 是，现在设计并实现协议 |
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
| `E1001` | 后端 capability 缺失（`core.gpu`、`core.jit.branch` 等） |
| `E1002` | 算子/重载歧义 |
| `E1003` | 没有匹配的算子/重载 |
| `E1004` | RSSN `api_version` 不兼容 |
| `E1005` | manifest/签名哈希不匹配 |
| `E1006` | 插件 ABI/加载失败 |

这些错误属于**环境/后端**，不是用户程序类型错误；诊断必须给出
capability 名、最小 RSSN 版本和可执行的替代方案。
