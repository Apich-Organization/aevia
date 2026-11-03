# 附录 C — 修订记录

本附录记录规范的每一次审计和实质性修改。修订号与 `README.md` 的
“Design Draft”状态对应。

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
