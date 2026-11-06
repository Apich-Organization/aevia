# 附录 C — 修订记录

本附录记录规范的每一次审计和实质性修改。修订号与 `README.md` 的
“Design Draft”状态对应。下列审计版本保留当时测量的 API 版本与计数；
当前集成使用 RSSN `api_version = 2`。版本 2 增加 manifest 源路径元数据，
二进制布局按 API 版本区分；迁移可使用自描述 JSON 或 CBOR。

## Revision 0 — 初始设计（2026-10-31）

- 建立语言定位：纯函数式、量纲即类型、符号/数值双相、rssn 后端。
- 完成 00–12 章与附录 A/B。
- 关键决定：
  - 量纲是类型，单位是编译期尺度；
  - `Int`/`Real`/`Float`/`Complex` 四种标量表示；
  - 十进制字面量默认 `Float`，整数默认 `Int`；
  - HM + 量纲代数 + 类型类 + 行多态；
  - 张量秩静态、形状动态；
  - `Point` 仿射点（标量、Float）；
  - `Dyn` 动态量纲逃逸；
  - 纯核心 + `IO`；
  - rssn 负责符号/数值/仿真，Aevia 负责语法/类型/运行时。

## Revision 1 — 第一轮审计（类型/语法/易用性）

第一轮发现并修复的主要问题：

1. **零不再量纲多态。** `0` 是 `Int`；零长度写 `0 m`。删除了
   “零乘任何值吸收量纲”的类型特例，增加 `E0406`。
2. **`Float` 严格是 f64。** 不再允许“符号 Float”；`Real + Float` 的结果
   改为 `Real`（精确优先），消除了“类型通过但运行时 panic”的洞。
3. **符号 Bool 改为 `Prop`。** `Bool` 只有 `true`/`false`；`Real` 比较
   返回 `Prop`；`if`/`match` 只接受 `Bool`；增加 `decide`/`assuming`/
   `by_cases`。
4. **`Dyn` 改为 `Dyn<ρ>`。** 保留表示；运行时量纲类型改名 `DimValue`；
   补 `unit_pow_dyn`；`check` 返回 `Scalar<ρ,d>`。
5. **`solve` 不再返回 `Set`。** 改为 `Solutions<ρ,d>`（根 + 重数 +
   条件）；`Set` 要求 `TotalOrd`，`Float` 不是 `TotalOrd`。
6. **`diff` 统一为一参返回函数。** 增加 `diff_at`；数值导数改名
   `numeric_deriv`；ODE 右端函数改名 `rhs`。
7. **`@` 按秩分派。** 2×2 `matmul`、2×1 `matvec`、1×2 `vecmat`、
   1×1 `dot`；修复 `A @ x` 示例。
8. **单位与张量算术补齐。** `Tensor * Unit`、`Tensor / Unit`；修复
   `0.02 / m`、`R / ohm`。
9. **`Point` 仿射语义收紧。** 仿射单位只接受 `Point`；`value_in`/
   `to` 对温差报 `E0510`；`Point + Scalar<ρ,Θ>` 明确允许。
10. **行统一加 lacks 约束。** 记录更新新增字段要求 `row lacks l`；
    重复字段 `E0208`。
11. **文法重写。** 布局改为 `NEWLINE INDENT ... DEDENT` 块/布局组；
    增加 `LT_APP`、`UNIT_NAME`、`SUPERSCRIPT`、`ELLIPSIS`、`PERCENT`；
    单行变体要求前导 `|`；lambda 支持 `fn x =>` 简写；`forall` 改为
    上下文关键字；`return` 改为 Prelude 函数。
12. **示例修正。** `fall_time` 统一 `Float`；`open Units.SI`；
    `linsolve`；`line` 直接接受带量纲张量。

## Revision 2 — 第二轮审计（后端/自洽/对抗）

第二轮发现并修复的主要问题：

1. **rssn 项图不带单位。** 静态单位在编译期折叠；`Real`/`Float` 值在宿主
   侧带显示单位；rssn 只看到无量纲数值/符号。避免了 `quantity` 无 eval
   和 `L`→升、`T`→特斯拉的文本歧义。
2. **量纲文本只用全名。** 桥接层使用 `Length`/`Mass`/`Time`/`Current`/
   `Temperature`/`Amount`/`Luminosity`，禁止单字母。
3. **数值答案的量纲取 `Answer.dimension`**，不从 `Answer.term` 重算。
4. **`compute` 克隆成本/`DimArena` 共享** 列入后端缺口；建议 `OpTable`
   用 `Arc`、新增 `prepare(config)`、基量纲只在模板期注册。
5. **`Term` 缺遍历/替换/自由符号 API** 列入缺口；`diff` 闭包需要
   `substitute` 或直接使用 `graph::Graph`。
6. **JIT 收缩承诺。** 当时只支持无分支的 `Float` 表达式；`if`/张量索引
   由解释器执行。
7. **`assuming` 收缩为单元谓词。** `x > 0`、`x >= 0`、`x != 0`、`x ∈ ℤ`；
   一般等式/不等式报 `E0804`。
8. **`Solutions` 后端降级。** 当时只保证 `roots`；`multiplicities` 填 1，
   `conditions` 为空，文档标注。
9. **ODE 方法/事件。** `OdeOpts` 增加 `steps`（Rk4）；events 只允许
   Dopri5；`RateOf` 递归定义标量/记录/张量。
10. **`par_map` 要求 `Parallel`。** 符号 `Real` 不并行，避免跨会话
    `TermId` 静默错误。
11. **一致性修复。** 删除残留 `OdeState`；`solve` 线性版本统一为
    `linsolve`；`deriv` 改名；`forall` 接受 `Prop`；删除 `E0703`；
    修正交叉引用/警告码；补 `E0316/E0317`、`E0406`、`DimValue` 等。
12. **标准库签名统一为元组多参。** 修复 `min/max`、`column`、`odeint`、
    `grad` 等柯里化写法；`mean/std/var` 统一 `div_rep(ρ)`。

## Revision 3 — 最终一致性

第三轮聚焦“修复是否真的闭合”，又关闭了以下问题：

1. **`Dyn` 的类型指导语义。** `dyn`/`check` 要求具体量纲，编译器把 `d`
   物化为 `DimValue`；泛型用 `dyn_with`/`check_with`。解决“类型擦除后
   运行时拿不到量纲”的矛盾。
2. **`by_cases` 限制为 `Real[d]`。** 避免 `Int`/`Bool` 分支无法构造符号
   `ite` 项。
3. **`Point + 温差` 只接受 `Float[Θ]`。** 精确温差必须先 `to_float`；
   `25 °C + 10 K` 报 `E0302`，提示写 `10.0 K`。
4. **`Show` 实例显式量纲多态。** `Show (Float[d])`、`Show (Tensor<n,a>)`、
   `Show (Solutions<ρ,d>)`、`Show (Point[d])`；否则 `20.0 m` 无法打印。
5. **绘图默认参数。** `line`/`scatter`/`hist` 使用默认 `PlotOpts`；
   `line_with` 接受完整记录；`label` 设置标签。示例不再依赖不完整的
   记录字面量。
6. **ODE 展平契约。** 字段声明顺序、深度优先、行主序；`Tensor<1,s>` 用
   `Boxed`；桥接层负责扁平 `f64` 与记录之间的转换。
7. **`Real` 的 `!=`。** 增加 `Prop` 版本；`assuming(x != 0, ...)` 可用。
8. **`Answer.dimension` 的 `Option`。** `None` 时回退静态量纲；数值路径
   用 `Advisory`/`Strict`。
9. **删除陈旧的 `quantity` 构造语句和 `L/T` 文本。** 静态项图无量纲；
   全名只用于 FFI/导出。
10. **`Float` 的 `sqrt` 永远走 f64**；删除“Float 保持符号”的错误描述。
11. **`inv` 统一返回 `Result`**；`Map` 不再声称依赖 `Hash`；`Solutions.
    conditions` 改为 `Tensor<1, String>`（Revision 6 又改为
    `Tensor<1, Condition>`）；`read_csv` 统一 `DataError`。
12. **JIT 触发条件去掉 `if`/索引**；与后端缺口一致。

## Revision 4 — 可扩展性与 RSSN 集成

这一轮不再把问题留到“v2”，而是把长期可维护性写进语言契约：

1. **反射优先的算子集成。** Aevia 不维护算子绑定表；读取 RSSN Kernel
   Manifest，生成 `Rssn` 模块。有签名 → 类型化；无签名 → `Rssn.dynamic`。
   1000+ 算子从第一天全部可见；新算子不需要改 Aevia 编译器。
2. **动态 `Value`/`call` ABI。** `Rssn.call`/`Rssn.dynamic` 是正式能力，
   不是降级路径；`OpError` 结构化。RSSN 只选注册、不校验签名；类型/
   量纲检查由 Aevia 静态检查器或 `Rssn.typed` 负责。
3. **签名/重载/稳定键。** `OpKey { rule_set, name, signature_hash }`；
   `Signature`、`overloads`、`Stability`、`Since`、`Doc` 进入 manifest；
   CI 审计“每个算子有签名或 dynamic_reason”。
4. **插件/宿主回调。** `@op` 把 Aevia 函数注册为 RSSN 算子；
   插件可注册算子/重写/单位/量纲/JIT lowering。
5. **量纲/单位统一。** 抽出 `rssn::dim`：`DimTerm`/`DimArena`/`UnitRegistry`；
   精确 `Scale { rat, pi }`、仿射 offset、用户单位/基量纲注册；Aevia 的
   `Units.*` 是 `manifest.units` 的生成视图（Revision 6 起），不再有第二张表。
6. **中心 registry。** 内容寻址 + 签名 + capability；包清单/索引/发布/
   yank 协议现在就定。
7. **形状细化。** `Tensor<[3,4],τ>` 作为 refinement；类型级 `Nat` 只做
   线性算术；`Matrix` 是别名；细化 → 秩-only 安全隐式转换。
8. **`Point` 张量。** `Tensor<shape, Point[d]>` 支持元素级点算术；
   温度场不再用“K 值 + 约定”。
9. **`Table<ρ>` 行多态 schema。** `read_csv<ρ>`、`column` 类型级字段查找；
   `Table<Dynamic>` 保留给未知 schema。
10. **设备/GPU/分布式。** `Device` 是运行时值；`on`/`to_device`/
    `@jit(target)`；不是张量类型参数，避免类型组合爆炸。
11. **稀疏。** `Sparse<F,shape,τ>` 核心类型，`Layout` 类；`@` 按布局分派。
12. **效果/宏的边界。** 不做用户 effect handlers/宏；扩展点是算子/插件/
    registry；`IO` 保持单一类型，capability 是运行时权限。
13. **稳定性模型取代 v1/v2。** `@stable`/`@experimental`/`@deprecated`；
    后端缺失报 `E1001`，不删语言特性。

### 仍存在的风险（Revision 5 时点）

- `compute` 的深拷贝（以及当时 `DimArena` 的共享）仍需 RSSN 优化；可用
  批处理规避。（Revision 7 起 `Graph::clone` 已深拷贝 arena。）
- `Solutions.multiplicities` 空表示“未报告”，不是“全 1”；`complete`
  目前硬编码 true，不能当证明。
- `Point` 的表示是 Float + UnitId；符号绝对温度仍用温差/`Dyn`。
- 形式化语义/证明尚未做；用 conformance/属性测试/与 rssn 交叉检查。
- GPU/分布式/稀疏的**后端实现**可能晚于语言设计；缺失时报 capability，
  不删语法。
- `Term` 替换/自由符号、`fork/merge`、结构化结果已在 0.2.13 实现；
  Revision 4 的对应风险已关闭。


---

## 待办清单（审计后关闭）

- [x] 统一 `dim` / `dimension` 关键字。
- [x] 统一 `E0202` / `E0309` 非穷尽匹配错误码。
- [x] 修正 `forall` 测试语法。
- [x] 补 `point` 的签名。
- [x] 修正 `Tensor<1, Real[e>]` 等笔误。
- [x] 统一 `reshape<m>` 写法。
- [x] 检查所有示例的类型/量纲推导。
- [x] 检查单位表与 rssn 的一致性说明。
- [x] 检查 `Point` 与 `Float`/`Real` 的交互。
- [x] 检查 `RateOf` 的递归规则。

## Revision 5 — 对齐 RSSN 0.2.13 实际实现

RSSN 的适配已经落地，这一轮把规范从“设计契约”改成“实际 API 契约”：

1. **Manifest 实际字段**：`KernelManifest` 有 `features`/`rule_sets`/`ops`/
   `units: Vec<UnitValue>`/`base_dimensions`/`plugins`；`manifest_hash` 是
   确定性 bincode 的 BLAKE3，JSON 只供人读。
2. **实际入口名**：`Session::manifest() -> Arc`、`call_value`、`call_ho`、
   `register(Box<dyn Plugin>)`；`OpInfo.keys`（不是 `key`）；`Value::Symbol`
   （不是 `VString`）；`TypeExpr::Dim(Box<TypeExpr>, DimExpr)`（不是
   `Real(dim)`）。
3. **实际覆盖**：rssn 0.2.13 有 1369 个算子、30 个 rule set、799 有签名
   （58.4%，其中 7 declared / 792 inferred）、570 个有 `dynamic_reason`、
   0 unexplained；Aevia 的可信 typed façade 只有 7 个，其余 1362 个走
   `Rssn.dynamic`。（这是 Revision 5 时点的数字；Revision 7 起 1369 个
   全部有签名。）
4. **量纲/单位**：`rssn::dim` 公开 `DimTerm`/`DimArena`/`Dimension`/
   `UnitRegistry`/`UnitDef`/`Scale`；`DimTerm::factors(&arena)` 返回
   `(String, BigRational)`；`Dimension` 仍是可打印字符串快照。
5. **Term/线程**：`Term` 借 `&Session`，`TermRef` 是 owned handle；
   `children`/`substitute`/`free_symbols`/`to_value`/`handle` 已实现；
   `fork/merge` 已实现。
6. **结构化结果**：`Solutions` 的 `multiplicities` 空表示“未报告”，不是
   “全 1”；`OdeSolutionView`/`SimResult` 已实现。
7. **JIT**：`Select`/`Extern1`/`Gather` 已 lowering；默认构建
   `features.jit=false`，`@jit` 必须报 `E1001("core.jit")`。
8. **当时唯一的 P0 构建阻塞**：`signatures.overlay.json` 被 `.gitignore`
   的 `**/*.json` 排除，干净 clone 下 `include_str!` 失败。（Revision 6
   时已由 RSSN 修复。）
9. **章节更新**：13 章、10.8、08.6/8.11/8.18/8.19、README 与根目录
   `RSSN-INTEGRATION-REQUIREMENTS.md` 已按实际 API/数值重写。

## Revision 6 — 对齐 RSSN `0852123a`（重载解析、Value 边界、单位扩充）

RSSN 上游又推进了一轮，这一版把规范里“等 RSSN 支持”的几处**关闭**，并把
新增的契约写进去。审计：`rssn 0.2.13`、`api 1`、1369 算子 / 30 rule set /
799 有签名（7 declared、792 inferred）/ 570 explained / 0 unexplained /
75 单位 / 7 基量纲。（Revision 7 起改为 1369 全部有签名。）`manifest_hash` 的 32 字节十六进制以
`ee077387a6aba5e4` 开头；`signature-audit` 显示的
`e4a5aba6877307ee` 是它的 `hash_u64`（小端），不是前缀。

1. **重载解析落地。** 新增 `kernel::overload`：`Match` 格
   （`Exact > Widened > Shaped > Generic > No`）、`score`、`best`；
   `Session::resolve_overload`/`call_key`/`overloads`；`OpError::Ambiguous`。
   规范 13.4.1 从“运行时只按 arity”改成“静态解析与 RSSN 的格同构，typed
   调用用 `call_key` 钉死”。`E1002` 现在有真实的运行时来源。
2. **Value 边界成为契约。** `manifest.value_kinds: [ValueKindInfo]` 给出每个
   `Value` 变体的 `in_term_graph` 与替代做法；13.3.1 改为“读 manifest，
   不维护第二份”。
3. **单位表扩充到 75 项并带元数据。** `UnitValue` 新增
   `aliases`/`prefixable`/`category`/`since`；天文（`au/ly/pc/Rsun/Msun/
   Mearth/jup`）与 CGS（`dyn/erg/Gal/P/St/G/Mx/Ba/Oe`）已注册，
   `Units.Astronomy`/`Units.CGS` 可从 manifest 生成；CODATA 常数仍缺。
4. **结构化量纲入口。** `Config::dimension_term`、`Graph::with_dim_arena`、
   `Graph::assume_dimension_value`；未知基名（`Currency`）在符号路径上可用。
   `dimension` 可用，`unit` 仍受限（`expr_to_dim` 只认 7 个内置基名）。
5. **`TypeExpr::Symbol`。** 自由符号不再是 `Var("Symbol")`；03 章补充
   `Symbol ≡ Real[δ]`（`δ` 弱）的对应规则。
6. **结构化结果语义修正。** `Solutions.complete` 不再硬编码 true：
   返回 `list(...)` 才 true，单根为 false；`OdeSolutionView.dimensions`
   写入 record。
7. **构建阻塞关闭。** `signatures.overlay.json` 已入库，`build.rs` 在缺失
   时写空 overlay 到 `$OUT_DIR`；干净 clone 可构建。
8. **章节更新**：13（manifest/value_kinds/overload/call_key/units）、
   10.8（剩余缺口重排）、08.6/8.19（Solutions/Rssn API）、05.3（单位
   状态）、03.3（Symbol）、README 与根目录
   `RSSN-INTEGRATION-REQUIREMENTS.md`。
9. **重载的“实际生效范围”写清楚。** 当时标准集 1369 个名字各只有 1 个
   注册、至多 1 个 overload，`score`/`best` 不执行；`call_value` 不校验
   签名，且无候选匹配时会退回第一个 arity 匹配。`resolve_overload` 非
   `Best::Unique` 时返回零 hash 的不可执行 key；`Kernel::call(&OpKey)`
   只按 name 分发。（后两条在 Revision 8 被上游改掉：`resolve_overload`
   并列报 `Ambiguous`，`Kernel::call` 转发 `call_key`。）这三条写进
   13.4.1 与根文档 §1.3/§3.3.1。
   （Revision 7 起已变：每个注册 1–3 条重载，`best` 在注册内执行。）
10. **`Solutions.conditions` 改为 `Tensor<1, Condition>`**，`Condition`
    是 `{ kind, text, expression? }`；`SimResult` 记录含 `schema` 字段。
11. **单位表以 `manifest.units` 为唯一来源**（不是 `UnitRegistry` 的视图）；
    `UnitValue` 带 `aliases`/`prefixable`/`category`/`since`。
12. **`manifest_hash` 的两种显示**：32 字节十六进制以
    `ee077387a6aba5e4` 开头；`signature-audit` 的
    `e4a5aba6877307ee` 是 `hash_u64`（小端），不能当哈希前缀 pin。

## Revision 7 — 对齐 RSSN `111b71d0`（100% 签名、常数、自定义基量纲、cdylib）

这一轮上游把上一版剩下的三个大缺口都补了，规范随之从“7 个可信 typed +
动态兜底”改成“全部 typed + 精度分档”。

1. **签名覆盖 100%。** `SignatureSource::Inferred` 改名为 `Derived`，
   语义也变了：它不再是从名字猜，而是从注册信息推——有 `eval` →
   `(Real,…) -> Real`（谓词 `-> Bool`），非叶子另加 `(Term,…) -> Term`，
   `Binder` 位置是 `Symbol`，变长参数是 `Variadic`。1369 个算子全部有
   签名：7 declared + 13 overlay + 1349 derived。
2. **精度分档写进规范。** “全部 typed”不等于“全部精确”：只有 20 个是
   精确的，1349 个是形态级（不区分 `Tensor`/`Quantity`/`Int`）。生成器必须
   标注来源，IDE 把 `Derived` 显示为“保守推断”。13.2.2 与 10.8 都按这个
   重写。
3. **重载真正生效。** 有 `eval` 的算子现在有 2 条重载（数值/符号），
   177 个算子双重重载、2 个三重重载；`resolve_overload` 会按参数类型选。
   注册内并列被它吞成零 hash key，`OpError::Ambiguous` 仍只由多注册
   （插件）触发。`call_value` 仍是注册级 arity 选择，不校验签名。
4. **物理常数进 manifest。** `constants: [ConstantValue]`（30 条、18 条
   精确、12 条带不确定度）。05.8 的常数表按 manifest 符号重写
   （`e` 是基本电荷、`g_n` 是标准重力、`epsilon_0`/`mu_0` 是测量值）；
   `Units.Constants` 不再需要 Aevia 本地 overlay。
5. **自定义基量纲可用。** `Dim` 变成“7 个 SI 基 + 有序旁表”，
   `expr_to_dim` 接受任意基名，`Config::base_dimension(name)` 把基名送进
   `DimArena`；`unit EUR : Currency` 不再是 `E1001`。规范要求经 `Config`
   声明，`prepare` 在克隆图上定义（`Graph::clone` 深拷贝 arena）。
6. **cdylib 插件 ABI。** `PluginKind { InProcess, Cdylib }`，
   `kernel::cdylib`（`rssn_plugin_v1`、bincode 字节缓冲、`RssnBuf`/
   `RssnHost`/`RssnOpEntry`/`RssnPluginDesc`），`Session::load_plugin`，
   `plugin-cdylib` feature 控制；`features.plugin_kinds` 让宿主在加载前
   拒绝。13.5.1 补了 ABI 表。
7. **移除已关闭的缺口**：10.8 删掉 CODATA 缺失、自定义基量纲注册失败、
   cdylib 未实现；13.6 删掉 `E1001("core.units.custom_base")`；
   新增 13.6.5（常数）与 13.6.6（线程/arena）。
8. **`manifest_hash` 更新**为 `9f1613fde0f150ce…`（显示值
   `ce50f1e0fd13169f` 是 `hash_u64` 小端）。（Revision 8 起又是
   `98fd35fa…` / `f03babb1fa35fd98`。）

## Revision 8 — 对齐 RSSN `95099fa0`（动态路径收口、COW arena、ODE 事件）

这一轮上游没有改语言契约，但把上一版剩下的四个“运行时不可靠”点收口了，
其中两个直接推翻 Revision 7 的结论。

1. **`call_value` 现在校验参数。** `resolve_op` 调用 `check_args`：没有
   任何 overload 接受参数时返回 `OpError::Type`，并指出第一个不被接受的
   参数；只有“所有候选都没有签名”时才放行。所以 dynamic 路径不再是
   “类型错误静默执行”。
2. **`Kernel::call(&OpKey, …)` 改为转发 `call_key`。** 它现在**钉死**重载，
   不再只按 `key.name` 分发。Revision 7 里“trait 会丢钉死”的说法作废。
3. **`resolve_overload` 并列时返回 `OpError::Ambiguous`。** 它不再返回
   零 hash 的不可执行 key；零 hash 只留给“完全没有签名”的算子。因此它的
   返回值总是可以安全交给 `call_key`。Revision 7 里“被静默吞成零 hash”
   的说法作废。
4. **`Solutions::new` 默认 `complete = false`**，新增 `.complete()` 构造器。
   “未报告”和“已穷尽”现在是两个显式状态。
5. **`KernelManifest::hash_hex()`** 给出 64 个 hex 字符，宿主应该 pin 它；
   `hash_u64()` 只是前 8 字节小端。Revision 7 的“pin 32 字节哈希”有了
   正式 API。
6. **`DimArena` 改为写时复制。** `Graph::clone` 共享 arena，
   `with_dim_arena_mut`/`define_base_dimension` 写入前 detach；
   `Graph::with_dim_arena` 同时改名 `with_dim_arena_mut`（`&mut self`）。
   Revision 7 的“深拷贝”描述作废，但隔离结论仍成立。
7. **符号指数桥接打通。** `DimExpr::to_term` 把 `Length^n` intern 成原子
   （`DimTerm::base_symbolic`/`var_symbolic`）。Aevia 的**语言**仍拒绝静态
   符号指数（§4.12.3），04 章补了一句“桥接能表示 ≠ 语言该支持”。
8. **ODE 事件覆盖扩展。** `Dopri5`/`Radau5`/`BDF`/`Rosenbrock23` 与定步长
   （`solve_fixed_events`）都能定位事件；10.8 的第 6 条从“只覆盖部分方法”
   改成“方法差异只剩定步长需要 `steps`”。
9. **`manifest_hash` 更新**为 `98fd35fab1ab3bf0…`（`hash_u64`
   `f03babb1fa35fd98`）。

## Revision 9 — 对齐 RSSN 3.0.0-rc.1、并发核验与派发/扩展边界（2026-11-04）

这一轮做三件事：把规范从“0.2.13 契约”升级到**实测的 3.0.0-rc.1**；把上一轮
审计后仍与实现不符的条目改正（含以核验结果为准的规范修正）；把“派发/
扩展边界”这一长期含糊的设计写成明确的规范决定。原则不变：**不删语言
特性**，后端/实现缺失用 capability 诊断；不以“v2”推脱。

### 1. 后端对齐：rssn 3.0.0-rc.1（实测）

- `rssn 3.0.0-rc.1`、`api_version = 1`；**1379 个算子 / 31 个 rule set**；
  签名来源 **7 declared + 22 overlay + 1350 derived**（1565 条 overload：
  1198×1、178×2、2×3、1×5）；**82 个单位**（本轮新增 7 个，见 §5）；
  30 条常数（18 条精确）；value kinds 17 个（10 个可进项图）；
  `SCENARIOS` 29 个（含 `ising_wolff`）；`features.jit = false`。
- `manifest_hash.hash_hex()` =
  `1920638b60fbdae5717552cf577fb41690473129868aebce5de02ae7580c04d9`。
  宿主 pin 64 个 hex 字符；`hash_u64()` 只是显示便利。**不再 pin 任何
  计数/哈希常量**：Aevia 的 conformance 测试直接读活的 manifest（算子数、
  来源分布、单位表、常数表、能力标志、哈希），manifest 变化时以测试失败
  提示重新生成，而不是让文档与代码各自漂移。
- overlay 从 13 个算子扩到 **22**：原 13 个（`diff`/`solve`/`taylor`/
  `quantity`/`convert`/`det`/`trace`/`norm`/`transpose`/`rank`/`dot`/
  `cross`/`identity`）加 `and`/`or`/`xor`/`not`/`implies`/`iff`/`fftconv`/
  `interpolate`/`sum`。**精确签名 29 条**（7+22），其余 1350 条是形态级
  `Derived`。
- 这期间上游收口的接口事实（详见根目录 `RSSN-INTEGRATION-REQUIREMENTS.md`
  的实测段落）：重载只有**一条解析路径**（`call_value` 校验实参；
  `resolve_overload` 只返回可执行 key、并列报 `Ambiguous`；`call_key` 不再
  重解析且会校验钉死的 overload；`Kernel::call` 转发 `call_key`）；
  `Session::register_unit` 与 `KernelManifest::build_with_units`（单位随
  session 拥有，插件单位不污染别的 session）；`score`/`score_types` 与
  `best`/`best_types` 共用同一折叠；`DimExpr::try_from_term` 是 `to_term`
  的精确逆，`from_term` 是全函数并回答 `Any` 而不是静默错误量纲；
  `ValueKind::Unit` 不再入项图；稀疏改用 faer（`sprs-rssn` 移除）；
  Cranelift 0.137（`jit` feature，默认关）；`Solutions` 记录、
  `CallCtx::substitutions`/trace 都已生效。
- **Aevia 侧不再维护 `signatures.overlay.json` 镜像**：bridge 直接消费
  manifest 携带的 overlay；仓库里没有第二份签名表。§4.1/§4.2 与 13.2 已按
  此改写。
- **JIT 语义**：默认构建 `features.jit = false`，因此 `@jit` 在检查期报
  `E1001("core.jit")`（不再是“被接受但不做事”）。JIT 的代码生成仍未实现，
  属已记录的实现缺口。

### 2. 规范修正（以实测行为为准）

- **§6.9.3 / §9.3 调用与资源界限**：非尾递归受**调用深度 2048**限制
  （`Interp::max_depth`），解释器线程栈由 `--max-stack <MiB>` 指定
  （默认 256 MiB，不是规范旧文的 8 MB）；不终止的尾循环由**连续尾调用
  预算**（默认 100,000,000，`--budget`）限制；表达式嵌套另有 16,384 节点
  上限。三者超限都是 `E0805` 并带调用栈。这是“合法程序终止、runaway 被
  报告”的保证在新尾调用实现下的精确形式。
- **§9.9 CLI**：`--max-stack` 的单位是 **MiB**，`--budget` 的单位是**尾
  调用次数**（不是秒）；`--release`/`--jobs` 在当前解释器实现中没有效果，
  CLI 明确提示而不是静默忽略；`--out`、`--locked`、`clean`、`lsp` 已实现。
  `--locked` 只校验锁文件，不涉及缓存。
- **§9.11 构建缓存**：当前实现**不写** `.aevi`/`.aevia/target` 接口缓存，
  每次编译重新检查全部模块；`clean` 诚实地删除可能存在的 `.aevia/` 并
  通常报告“没有可清理的内容”。增量编译是已记录的性能缺口（见 §9 末尾）。
- **§6.10 / §9.7.2 `par_map`**：语义保证（结果顺序与输入一致、线程数不影响
  结果）不变；**当前求值器单线程，`par_map` 顺序执行**，明确记录为实现
  偏差。真正的并行需要会话级并行或线程化求值器。
- **§1.8.1 单位字面量邻接**：补写“单位表达式内的 `*`/`·`/`/` 一直属于单位
  表达式，要把量乘以变量或函数调用必须先用括号结束单位字面量”，例如
  `(3.0 m/s) * exp(-t)`；`3 m * x` 会把 `m*x` 当单位表达式（`x` 不是单位时
  `E0501`）。这是既有规则的可执行化说明，不是新规则。
- **§11 完整示例**：修正了示例自身的错误——变量绑定用大写（按 A.4 那是
  零参构造器模式）；多语句 `match` 分支体需要 `do`（它是 IO 动作序列）；
  `(3 m)^n` 需要先绑定 `n : Int`；量的乘法需要括号结束单位字面量。示例的
  类型/量纲推导与输出同步修正。**注意**：`spec/11` 依赖标准库模块
  （`import Math { sqrt }`、`open Units.SI`），而 §8.20 的模块层级本轮尚未
  实现，导入仍是 `E0607`；§8.17 的 `Physics.*` 类型化包装器同样未实现。
  `spec/11`、`spec/08` 里已如实标注这一实现状态，不假装已可运行。
- **附录 B**：新增 **`W0602` deprecated name used**（`@deprecated` 使用点
  警告）。其余“已文档但尚未可触发”的代码继续列在 STATUS 的已知缺口里，
  不在规范里假装可达。

### 3. 本轮新增的语言与运行时能力

- **类型类之外的 `Dyn` 检查器**（§3.11/§4.9）：`dyn`/`check`/`dyn_with`/
  `check_with`/`dim_of`/`value_of`/`pow_dyn`、`DimValue`、`DimError` 与
  `Dyn` 的 `+`/`-`/`*`/`/`/`^`/`sqrt` 规则全部有检查器签名；`dyn`/`check`
  在调用点把具体量纲物化并记录（`dyn_calls`），运行期据此做 `E0412` 校验；
  `Dyn` 与 `Scalar` 之间没有隐式转换（显式 `check`）。`Show`/`Eq` 对
  `Dyn` 可用（§4.9.4）。
- **属性成为真语义**（§2.2.7/§9.6.1/§13.8.1）：`@jit` 按 `core.jit`
  门控；`@stable`/`@experimental`/`@deprecated` 被记录，使用 deprecated 名字
  警告 `W0602`；`@op`/`@rule` 按 `core.plugin.source` 门控（本构建缺失，
  报命名 capability），不再静默无效。源码级插件包装（§13.5.2）仍未实现。
- **`Rssn` 动态接口的四个入口可达**（§8.19）：`Rssn.typed` 端到端可用
  （返回 `Result<Value, OpError>`，`TypeRep` 是不透明运行时类型，`from_value`
  做窄化——这是本实现的诚实静态形式，而不是虚假的 `Result<a, OpError>`）；
  `Rssn.call_ho` 的空回调表转发到内核、非空回调报
  `E1001("core.callback")`（Aevia 闭包不是 `Send+Sync`）；
  `Rssn.register`/`register_plugin` 报 `E1001("core.plugin.source")`
  （源码级插件打包未实现；宿主 Rust 插件走嵌入 API）。这两个 capability
  是本轮新增的“实现能力”名。
- **张量与单位的算术**（§3.5.3/§4.5.2）：`Tensor * Unit`、`Unit * Tensor`、
  `Tensor / Unit` 对字面量张量也成立（元素表示不被强制改变）；同秩张量的
  逐元素 `*`/`/` 与张量 `^` 字面量指数按规范实现。
- **ODE 的 `RateOf`**（§8.9）：`odeint`/`odeint_with`/`solve_ivp` 的右端
  按内建类型函数 `RateOf<s>` 检查（标量除以 T、张量/记录递归；`Int`/
  `Bool`/函数字段报 `E0302`）；右端必须是双参。`OdeOpts<s>` 目前只做记录
  字面量的字段检查（不完全按声明记录检查），是记录在案的遗留项。
- **Chapter 8 的名称补齐**：`par_map`（`Parallel` 内建类；Int/Float 及其
  张量；符号 `Real` 报 `E0310`）与 `rng : Int -> (() -> Float)`（纯、确定性
  生成器）进入 Prelude；`set`/`sum(a, axis)`/`at` 的秩 overload 补齐；
  单位名与函数名在调用位置正确分命名空间（`at(t, 0)` 是函数，`3 at` 是
  单位）；`f()` 支持零参函数值应用。**当前 chapter 8 覆盖：315 个名字，
  实现 + 命名 capability gap 已闭合**（`read_npy`/`write_npy` 仍报
  `E1001"io.binary"`；blocked 为 0）。
- **运行时/IR（§6、§9.3-§9.5）**：`reshape` 在连续视图上保持视图（否则
  复制）；`sum` 对 dense 与 boxed Float 使用 Kahan/pairwise；空张量归约按
  元素相位定义（Float 空 `sum=0.0`、`prod=1.0`、`mean=NaN`，Real 空
  `mean=E0802`）；混合字面量张量按 `join` 选择 f64 存储；`EvalError` 栈
  保留最内层失败帧与其调用点（§9.3.4）；`approx` 对定义域外形式表达式
  报 `E0806`（不是 `Ok(NaN)`）；`decide` 接受精确比较擦除后的 Bool 形式；
  `Dyn` 算术/`check` 的运行期校验见 §3；`par_map`/`rng` 运行期 arm 已实现
  （`par_map` 顺序执行）。
- **工具链（§9.8-§9.12）**：REPL 补 `:ast`/`:core`/`:time`/`:latex`；CLI
  补 `clean`、`lsp [--stdio]`、`rssn --emit --out`、`--locked`、`test
  --filter`、`check --all`、`doc --open`；LSP 补 code actions（加类型标注、
  温度单位换算、提取 let、修量纲错误）与 semantic tokens，诊断可带未使用
  绑定；`bench` 报中位数 + 95% CI 并提示构建 profile；`doc` 报告未文档化
  声明。编辑器仍按 §9.10 的“标准 LSP 3.17、协议无关语义层”分层。
- **内核（D:/rssn，本轮）**：§5.3.3 的 7 个单位注册进内核单位表
  （`Da`/`u`、`kn`、`mph`、`acre`、`bit`、`B`/`byte`、`KiB`；精确尺度；
  `bit`/`B`/`Da` 可加前缀）；**`Solutions.multiplicities`/`conditions`**
  不再是恒空：只在解确为“一元有理多项式、根为精确数”时按无平方分解填
  重数（无理/符号根保持“未报告”），`conditions` 填解本身给出的
  `NonZero` 条件（参数在分母），`complete` 是求解器的真实回答；18 条
  kernel-requirements 审计项逐条复核（含 4 条此前无直接测试的项）无回归；
  内核文档（KERNEL/DESIGN/DIMENSIONS/checklist/archive）同步到实测数字。

### 4. 记录在案的实现偏差（不是“以后再说”，是当前契约）

1. **十进制“量字面量”的精确折叠。** 裸小数默认 `Float`（f64），但带单位
   的十进制量（如 `3.2 km`、`0.1 m`）在编译期按**精确有理数**折叠；其静态
   表示仍默认 `Float[L]`，在精确上下文（与 Int/Real 混算）走精确路径，
   纯 Float 上下文才落成 f64。因此 `0.1 m * 3 == 0.3 m` 为真，而
   `0.1 + 0.2` 仍是 f64 的 `0.30000000000000004`。§3.10.1 的“精确十进制
   转换”规则不变，但“`3.0 m` 的值 3.0”要读作“精确 3，在 Float 运算处
   转 f64”。这是实现为保证精确性做的选择，记录在此以免被当成 bug。
   **相关已知缺陷**：静态 `Float` 与运行期表示目前可能不一致
   （`fn f(x: Float): Float = x + 1` 返回精确 3），这是类型健全性问题，
   不是该偏差的许可；修复方向是拒绝 Real→Float 或在字面量降级处按记录的
   表示强制转换（见 STATUS 已知缺口）。
2. **`par_map` 顺序执行**（见 §2）。
3. **`Rssn.typed` 的静态类型是 `Result<Value, OpError>`**，不是规范理想的
   `Result<a, OpError>`：本构建的 `TypeRep` 没有类型见证，虚构一个新鲜
   `a` 会重新引入 `∀a.a`；窄化由 `from_value` 完成。`call_ho`/`register`
   是“类型正确但 capability 门控”。三者都是诚实的能力边界，不是静默
   降级。
4. **类实例派发仍按运行时形状**，跨模块的类/实例/别名导出尚未实现
   （§7.1.3/§7.4 未兑现）；这是已复现的正确性缺陷（多原语头实例会静默
   互相覆盖、跨模块实例不可见），已列入下一轮的**最高优先级**工作，
   并配套“实例键 + 见证派发”的设计（见 §5）。
5. **没有 `@jit` 代码生成**、没有 `.aevi` 接口缓存、没有真正并行的
   `par_map`、没有标准库模块层级（§8.20）与 `Physics.*` 包装器；这些是
   实现缺口，不是规范删除的特性（capability/诊断按 §13.8 报告）。
6. **单一 RNG 语义未统一**：语言侧 `rng`/`sample_n` 与核侧 SDE 使用的
   RNG 流不同；当前承诺是**同构建可复现**，不是跨平台逐位一致。

### 5. 派发与扩展边界（新增 §12.8.13 的规范决定）

这一轮把长期含糊的问题写成明确决定（理由见 §12.8.13 与设计评审记录）：

- **不做运行时多方法派发**。实例由类型静态选定；类型被擦除后，运行期只
  按**规范见证**（ADT 的模块+名字身份、`NumKind`、张量秩+存储类、记录
  字段集合、Bool/String/Unit/Prop）选择实例；见证冲突是**检查期**的
  `E0311`/`E0310`，不是运行期错误。函数、`Dyn`、高阶值没有见证，不能用
  实例头部。
- **不做多参数类/superclass/关联类型/实例上下文/通用默认方法/用户数值类
  重载**。§3.8.1 的多参数类句子删除；§8.7.1 的 `class Distribution dist d`
  改写为名义 `Distribution<d>` 加内建 overload（v1 的用户自定义分布关闭，
  记录为扩展性损失，若将来需要，用 `RateOf` 式类型函数或专门的泛型类设计，
  而不是把类系统扩成关系型）。
- **不做按类型或按量纲的单态化**。类型/量纲擦除后，`mean<L>` 与 `mean<T>`
  必须是同一份代码（§4.4.4、§9.2.3、§11.8）；性能来自调用路径/表示特化/
  张量内核，而不是为每个类型生成代码。`expr_types` 可用于**表示**特化，
  但必须先修好静态 Float/运行期表示的偏差。
- **扩展点是**：ADT、记录、模块、结构化 `derive`（封闭集合
  `{Eq, Ord, TotalOrd, Show, Hash}`）、RSSN 算子/插件、`Dyn`、包 registry；
  不是宏、不是 `quote`/AST 片段、不是构建脚本（§12.8.5、§13.9.7、§9.14）。
- **`Show` 是保证也是钩子**：§3.8.4 列出的结构值（记录、`Solutions` 等）
  在没有用户实例时由内建结构打印兜底（保证）；用户可为自己的名义类型写
  实例覆盖（钩子），实例体在检查期按替换后的方法签名检查。
- **下一轮工作顺序**（记录在 PLAN）：① 实例/类/别名的模块接口 +
  `Class@?` 静态全局缺陷修复 + 见证派发（正确性）；② 单遍 `check --all`
  （~10×，不依赖缓存）；③ 最小 SDE（定步长对角噪声 Euler–Maruyama）；
  ④ 性能 stage-now（分配计数门、帧复用、字面量预解析、启动延迟）；
  ⑤ 增量编译接口缓存（先符号身份，后缓存）。

## Revision 10 — 第二轮实施：派发键、标准库模块树、SDE、Nat 与工具链（2026-11-05）

本轮把 Revision 9 列出的工作顺序中的前四项实施到可测试状态，并完成了设计评审
后的边界决定。原则不变：静态解析与运行期一致，能力缺失用诊断而不是删特性。

### 1. 派发与复用（正确性优先）

- **实例身份改为符号键。** `ClassKey`/`InstanceKey`/`AdtKey` 由「定义模块 + 名字
  + 规范化头部（标量表示、张量秩/形状、ADT 结构参数、记录标签集）」构成，
  `AdtId` 只作为进程内见证；运行期和检查器用同一个 `InstanceKey::mangle()`
  生成实例体全局名，修掉了 `Class@?::method` 让所有原语头实例互相覆盖的
  静默错误。`DictTarget` 把调用点钉死到 `Instance(key)` / `Witness{candidates}`
  / `Builtin(ShowStructural)`，IR 的 `CallKind::Dictionary` 携带 target，AST 与
  IR 解释器同规则派发。
- **类/实例/别名随模块接口导出**（§7.1.3/§7.4）：`ModuleInterface` 携带
  `classes`/`instances`/`alias_defs`，driver 按拓扑序传给
  `check_module_with_interfaces`；导入实例合并进 ClassTable（重叠仍 `E0311`，
  孤儿规则不重复检查），`private class` 只在本地，`private instance` 非法；
  别名的定义随接口导出，导入方可以展开它。同一类型名在两个模块里现在是两个
  不同的名义类型（构造器按定义模块绑定）。
- **见证派发。** 具体头部静态钉死；量化调用点按规范见证（ADT 模块+名字、
  `NumKind`、张量秩+存储类、记录标签集、Bool/String/Unit/Prop）选择；见证
  冲突是检查期 `E0311`/`E0310`，不再运行期猜形状。函数、`Dyn`、高阶值没有
  见证。`rows_overlap`/张量 `heads_overlap` 修正为按字段数/秩比较。
- **`Show` 语义**：§3.8.4 列出的结构值（记录、`Dyn`、单位值等）在没有用户实例
  时由内建结构打印兜底（保证）；用户可为自己的名义类型写实例覆盖（钩子），
  实例体按替换后的方法签名检查。`derive` 封闭到
  `{Eq, Ord, TotalOrd, Show, Hash}`，`E0317` 点名缺哪个字段/实例；多参数类声明
  报 `E0317`（§3.8.1 的多参数句子删除）。
- **`Distribution` 改为名义 `Distribution<d>` ADT**（§8.7.1），v1 不开放用户
  自定义分布，这个扩展性损失被记录而不是隐藏。

### 2. 标准库模块树（§8.20）

- 37 个模块源（29 个由 §8 章节生成 + 8 个手写：`Units/*` 六个、
  `Physics/Ising`、`Physics/Geodesic`；新增 `Sde`）经 `include_str!` 嵌入
  `aevia-std`，`aevia_std::stdlib` 暴露 `source`/`is_module_name`/`module_paths`；
  加载器在普通根失配时回退到嵌入源（合成 `stdlib/A/B.aev` 路径参与后序/环检测，
  `SourceMap` 显示 `stdlib://A/B.aev`），本地文件优先。
- `import Math { sqrt }`、`open Units.SI`、`import Data.Csv { read_csv, column,
  Table }`、`import Physics.Ising` 从用户源码可用；`spec/11` 的九个完整程序
  **全部 9/9** 通过类型检查（11.2 的 `RateOf` 被导入再导出遮蔽、11.4 的
  refined/rank-only 形状单向统一是两个真实检查器缺陷，已修复）。
- `Units/*` 是文档性模块：单位名本来就是 ambient `Unit[d]` 值，不导出绑定
  （§5.3.4）；`Units.Constants` 由 live manifest 生成。`Physics.Ising` 有 4×4
  端到端运行测试。
- 第 8 章覆盖率：**322 个名字，320 实现，2 个具名 `io.binary` 缺口
  （`read_npy`/`write_npy`），0 blocked，100% 有归属**。

### 3. SDE（内核 + 语言）

- **内核**新增 `rules::sde`（第 32 个 rule set）与两个无回调算子：
  `sde_wiener(seed, m, steps, h) -> Tensor<2, Real[T^(1/2)]]`（`DimRule::RootOf
  { degree: 2, index: 3 }`，取第 4 个参数的平方根；manifest 线格式
  `{"root_of":{"degree":2,"index":3}}`）与
  `sde_levy_areas(seed, h, dw, terms) -> Tensor<1, Real[T]]`；新增
  `Features.sde = true`；`SdeError` 带数据并实现 `Display`，`SdePath` 增加
  provenance 字段；求解器循环（EM/Milstein 族/SRK/Heun/自适应/非交换/集合）
  保持为 Rust API，因为回调无法越过 manifest ABI（内核清单 CO-03）。
- **语言侧**：新 `Sde` 模块与 §8.9.1 规范——`SdeMethod`（名称编码 Itô/
  Stratonovich）、`SdeStructure`（Diagonal / General(m) /
  GeneralNoncommutative(m)）、`SdeFixedOpts<T>`/`SdeAdaptiveOpts<T>`/
  `SdeOpts<T>`、`SdePath<s>`，入口 `sdeint_diag`/`sdeint_general`/`wiener`/
  `sde_order`/`sde_ensemble_seed`。`DiffusionOf<τ>` 内建类型函数（叶子量纲
  除以 `T^(1/2)`，与 `RateOf` 同构）。定步长可交换路径使用语言 RNG 流
  （与 `rng(seed)` 一致，`wiener` 与路径消费同一组增量），非交换/自适应
  使用内核 xoshiro 流；只承诺同构建可复现。错误沿用
  `E0802/E0806/E0807/E1001`（`core.sde`、`sde.noncommutative`、
  `sde.interpretation`、`sde.general.record`），不新增附录 B 码。
- `aevia-rssn` 自动吸收 manifest：`Rssn.sde_wiener`/`Rssn.sde_levy_areas` 类型化
  声明、`core.sde` 能力行、`T^(1/2)` 元素维度 conformance 测试；kernel 实测
  **1381 算子 / 32 rule set / 1567 overload（7 declared + 24 overlay + 1350
  derived）/ hash `8ed92a22…`**。
- 明确推迟：events/根查找、dense output、记录状态 + 一般噪声、非均匀定步长
  网格、用户自定义噪声结构、manifest 求解器算子；`aevia rssn --emit` 对开放
  维度参数仍保守地拼成 `Dyn<term>`（manifest/检查器精确，生成源码拼写是已知
  限制）。

### 4. 类型级 Nat 线性统一

- 形状 extent 支持真正的类型级 `Nat` 变量（`ty::Nat = Lit | Var | Add | Mul`，
  `Shape::Refined(Vec<Option<Nat>>)`），`Scheme.nats` 参与量化/实例化/默认化；
  统一把方程化为线性形式，单变量解出非负整数（`n+1~3` ⇒ 2、`2n~6` ⇒ 3）、
  多变量等式保留重试、常数残差 `E0302`、非线性 `E0304`。`reshape`/`zeros_of`/
  matmul/索引界限按解出的 extent 检查；实例头不允许 Nat 变量（`E0304`）。
- 前端诊断补齐：`E0108`（数字后括号非单位）、`E0205`（部分运算符 section）、
  `E0207`（callee 非名字的相邻类型应用）、`E0203` ragged 字面量、`W0101`/
  `W0103`/`W0601`/`W0602`；其余码在 `diagnostics_coverage.rs` 中有明确处置
  （运行期/加载器拥有，或带证明的 unreachable 理由）。

### 5. 工具链、测试与性能

- `aevia test --doc`：从 `##` 注释抽取 `aevia` 围栏块，逐块在合成模块中编译；
  块首行 `compile_fail` / `compile_fail EXXXX` 断言失败与错误码；`@compile_fail`
  属性删除（`E0210`）；`compile_fail("<source>")` 保持隔离片段契约。
- `aevia bench` 增加 `--iterations/--warmup`、`--save-baseline`/
  `--compare-baseline`/`--threshold`（默认 5%），JSON baseline 带
  profile/version/median/mean/CI，跨 profile 拒绝比较，回归退出码 1。
- LSP/analysis：文件已保存时通过 `driver` 加载导入图，诊断/悬停/补全/跳转按
  当前 `LoadedModule` 回答，跨模块跳转到定义文件；未保存缓冲区回退单文件并
  在能力中声明；新增 code actions、semantic tokens、typeDefinition、单位跳转、
  `W0101`/`E0309` 透传。缓存与 workspace/symbol 属后续优化。
- `aevia check --all` 改为单遍加载（`driver::load_all_with_roots` +
  `LoadStats`），共享依赖只检查一次；`load_with_roots_and_source` 让脏缓冲区
  保留多模块分析。
- 性能：字面量按检查器记录的表示物化（修掉「静态 `Float`、运行期精确值」的
  相位错位）；`eval_ir` 不再在每次返回时克隆整个调用栈，浮点字面量在 lowering
  时解析一次；Kahan/Neumaier 求和与单元素显示单位修正；分配计数门、符号项
  结构键、启动阶段剖析、Rep 特化 IR、JIT 均按计划后置（不做整体单态化）。
- 文档：`STATUS`/`PLAN`/本附录按实测数字更新；单文件规范版重新生成；
  `RSSN-INTEGRATION-REQUIREMENTS` 与 spec/10/13 对齐新的 manifest 事实与
  `Features.sde`；内核 `KERNEL/DESIGN/DIMENSIONS/checklist/archive` 同步。

### 6. 仍然开放（记录，不隐藏）

- SDE 源码级选项 ADT 依赖「别名/ADT 声明的 kind 推断」（`type Normal<d>` 的
  同类问题）；在它落地前 `sdeint_*` 只能从运行期测试构造选项。
- 增量编译 `.aevi` 接口缓存（先单遍化，缓存 v0 待做）；`aevia rssn --emit`
  开放维度拼写；稀疏张量运行期（`Value::Sparse`/`@` 布局分派）未实现，
  当前只有 `from_dense`/`to_dense`/`nnz` 记录级内建；JIT 代码生成未实现。
- `Spec/11` 依赖的 §8.7.1 `Normal<d>` 静态声明同样等待 kind 推断修复。



### Revision 10 实施补记（2026-11-05 夜 → 傍晚）

第二轮计划完成后又做了一轮逐模块硬化，全部带测试：

- **类型层**：§6.6 穷尽性/有用性改为有界矩阵算法（嵌套载荷、字面量、记录、guard、
  空枚举；W0201 冗余分支；深度上界 24 只会抑制诊断、不会凭空报错）；形状细化在
  matmul/索引/字面量切片/reshape/transpose 中保留；Nat 求解器用 `i128` 线性式，
  覆盖 `n+n~4`、任一侧乘字面量、负解 E0302、大整数、晚绑定后才矛盾（E0302）与
  真正歧义（声明末 E0304）；导入类型/构造器/透明记录别名与泛型别名/ADT 的
  kind 推断（`<d: Dim>`/`<n: Nat>`/`<r: Row>` 及从 body 推断）；属性实参必须是
  编译期常量（E0210）；`min_all`/`max_all` 接受任意秩；调用位置内建函数优先于
  同名单位值（`min` vs minute、`dyn` vs dyne），并保住 `dyn` 的动态量纲特化。
- **标准库与诊断**：§8 签名审计（309 条声明，2 条记录的松散 scheme，18 条人工
  类型/顺序发现全部修复或有主）；`div_rep` 结果细化（`inv`/`linsolve`/`lstsq`/
  `mean`/`var`/`std`/`cov`/`corr`）、`complex_sqrt` 的 `d^(1/2)` 显示、`rank`/
  `shape` 张量类型、NPY 秩放宽（`write_npy` 任意张量、`read_npy` v1 秩 2）、
  `decide` 的 Bool 重载、Measurement 算术；本地 NPY 编解码（第 8 章 323/323，
  0 缺口）；E0601、`args()`、E0901–E0905；精确相位扩展实数采取部分语义
  （`oo*0`/`oo-oo`/`oo/oo` 保持符号项，Float 按 IEEE）。
- **后端与工具链**：`aevia rssn --emit` 用声明内共享的维度变量精确拼写
  `Real[T]`/`Real[T^(1/2)]`/`Unit[T]`，其余 `Dyn<term>` 原因逐条记录；字面量键
  `Rssn.invoke(op, key, …)` 按 manifest 钉死的 overload 定型（每次调用新鲜的维度
  变量；非字面量/未知键保持动态，E1002/E1003/E1005 仍是运行期诊断），因此
  `--emit` 后 `check Rssn/Units/Constants` 各 0 诊断；编辑器侧补
  `references`/`rename`/`prepareRename`（跨文件 WorkspaceEdit、拒绝规则）、
  `signatureHelp`、字段/关键字补全、`workspace/symbol` 与内容哈希+ mtime 的
  分析缓存；`run -- args`、`run --example`。
- **性能**：`sum(a, axis)` 与整张量 `min_all`/`max_all` 的 dense f64 内核
  （512×512 axis sum：525,336 → 7 次分配、~449 ms → ~12.9 ms；min_all ~105 ms →
  ~5.2 ms，debug）；分配计数门、结构性 `Term` 键、`Rc<str>`、惰性内核常量播种
  （空程序启动 61.6 → 8.6 ms）。
- **内核**：Rader 素数长度 FFT + 批量线程化变换、`tan`/`atan2` 区间规则、C API
  维度错误码、三角形自适应求积、HOSVD/Tucker、`sde_scheme_info` 元数据算子
  （manifest **1382 ops / 32 rule set / 25 overlay / 1568 overloads**，hash
  `8d2c3764…654a`）。
- **最终权威套件**：`cargo test --workspace --no-fail-fast -- --test-threads=2`
  → **1266 passed / 0 failed / 5 ignored**（89 个测试目标，exit 0）；单文件规范
  版同步重新生成为 Revision 10。仍开放的项目逐条列在 `STATUS.md` 的
  “Known gaps”（稀疏运行期、`.aevi` 缓存/并行编译、registry、沙箱、JIT、
  非字面量 invoke、KN-06 等）。
