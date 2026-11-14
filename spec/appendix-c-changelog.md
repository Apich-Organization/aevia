# 附录 C — 修订记录与已知实现边界

本附录记录 Aevia 规范与实现的演进、各章不再保留的“状态故事”，以及明确记录在案的未实现边界。规范正文只说**现在是什么**；历史在这里。本附录是资料性的。

---

## Revision 23（2026-11-14）：规范全面审校与英文版

本轮把规范与实现逐章对照，压缩重复、把状态说明移出正文，并新增英文版（`spec/en/`，写作与术语规则见 `spec/STYLE.md`）。机器读取的两章（08、11）保持形态不变，由 `cargo test -p aevia-std --test spec_signatures --test spec_programs` 守住。

**全书层面**

- 新增英文版：每个中文章节在 `spec/en/<同名>` 有对应英文文件，章节号、锚点、代码块与诊断码与中文一致。
- 状态说明（“实现状态（日期）”、“Revision N 新增”、“task-NN 已闭合”）从正文移出，归入本附录；正文只保留一行 *状态*。
- 实测数字（算子数、单位数、测试数）不再散落在各章：manifest 规模在 §10.8 一处给出，其余各章指向 `aevia rssn`。

**本批章节（08、10–13、README、本附录）**

| 文件 | 修正 | 增补 | 删除/移出 |
|---|---|---|---|
| `08-stdlib.md` | §8.19/§8.20 的 manifest 计数与“323/323”“309 条”“37 个模块”等过期数字（现为 1620 个算子、41 个模块文件，覆盖由测试断言）；`Rssn_Numeric` 的 typed 计数改为“全部 typed”；§8.7.1 中文段落里夹杂的英文句子 | 稀疏、FFT、序列操作等小节合并为一次叙述，规则不变 | 各节“实现状态”故事（`par_map`、`args`、SDE、稀疏、Plot、NPY、仿真）压缩为一行状态；Revision 号旁注；`.workbuddy-ai` 审计文档的引用 |
| `10-backend.md` | §10.0/§10.8 的 manifest 数字（1382→1620，`manifest_hash` 变更）；§10.5.1“`par_map` 顺序执行”改为工作线程副本（§9.7.2）；§10.9 “量纲字面量折叠为精确有理数、打印 `sqrt(800000/196133) s`”的偏差说明（现 `fall_time(20.0 m)` 打印 `2.0196199771025523 s`） | §10.3.7 指向生成的 `Rssn_Sim_<Name>` 模块；§10.8 缺口表合并重复项 | task-31/emit_typecheck 的叙述、重复的 SDE 状态段、双语混写的 §10.6.5 英文摘要 |
| `11-examples.md` | 11.1–11.5 输出改为 `aevia run` 的实际输出；11.2 终点时间改为 3 s（原 10 s 时抛体已落地、高度为负）；11.3 `series` 的结果形态与 `solve` 的 `Solutions` 记录；11.10 的变量遮蔽警告 | 说明 11.8 是库模块（无 `main`） | 重复的“展示了…的好处”段落 |
| `12-rationale.md` | 12.8.7 并行符号值（`fork/merge` 的说法换成 §9.7.2 的实际行为）；12.10 路线图由工期估算改为实现进展表 | — | M0–M6 的周数、验收清单的重复项 |
| `13-extensibility.md` | §13.7 registry：此前写“设计完成、实现未开始、依赖报 `E0605`”，实际 `aevia add/update/fetch/tree`、`aevia pkg publish/yank/search/verify` 与解析器已实现（Revision 15）；§13.8.2 capability 表补上 `core.callback`、`core.plugin.source` 并以实际 12 行为准；§13.9.5 稀疏“只检查秩”改为已检查细化形状；删除不存在的 `aevia fix` 与 `tools/gen_rssn_module.aev` | 13.2.2 精度分档表与 §13.4.1 评分规则合并 | manifest hash 变迁（`1920638b…→8ed92a22…→8d2c3764…`）、task-31/task-22 叙述、重复的 SDE 状态、各节“实现状态”长段 |
| `README.md` | “Revision 13”→23；“1476 operators in 37 rule sets” 等计数改为指向 `aevia rssn`；阅读指南表与章节标题对齐 | 中文版（原先 `spec/README.md` 只有英文） | — |
| `appendix-c-changelog.md` | §2 的“37 个标准库模块”→41；§5 已知边界清单更新（SDE 事件、形状细化已实现，registry 已实现） | 本条目 | Revision 12–22 的细节压缩为要点 |

**本批章节（01–04、附录 A）**

| 文件 | 修正 | 增补 | 删除/移出 |
|---|---|---|---|
| `01-lexical.md` | NFC 只作用于标识符；反引号标识符未实现（E0105，改为状态说明）；`return` 不是 Prelude 函数（应为 `pure`）；`1e`、`1.` 非法字面量；未知转义是 E0105；`_` 只能作调用实参；后缀 `!` 须紧贴操作数 | E0104 空插值 `{}`、E0108/E0209 的 `3 (…)` 情形；µ/Ω/Å/° 特殊字符表与字面量默认类型表 | “Revision 13”旁注；与 §2.6 重复的优先级表 |
| `02-syntax.md` | `open` 不能打开 `import … as` 别名；`forall` 谓词只有一个参数；`return(())`→`pure(())`；`xs[1::2]` 是 cons；`do` 内不能声明 `fn`/`let rec`；单元素元组是 E0202；`(+)` 能解析但未绑定（状态）；`√x` 需已知操作数类型；浮点/负数字面量不是模式；已知属性集合（含 `@bench`） | E0208、E0314、E0701、E1001；`match` 三种布局 | 全部行内 EBNF（与附录 A 重复）；§2.7.1 重复段落 |
| `03-types.md` | `Int / Int` 为 `Real`；`Real` 比较得 `Prop`，`if` 中用 `Prop` 是 E0302；索引规则（标量下标消轴、切片保轴，旧“只接受全标量下标”有误）；`oo m` 非法（写 `oo * 1 m`）；`return`/`bind`/`panic` 不是用户函数；`TotalFloat` 未实现；E0307 与 E0315 的区分 | §3.5.5 归约与线性代数的量纲规则表；Nat 方程要点 | “Revision 12/13”与附录 C Revision 9 旁注；重复的签名表（留在 §8.4）；弱变量与 RSSN `Symbol` 的冗长叙述 |
| `04-dimensions.md` | `L^0.5` 非法（写 `L^(1/2)`）；E0408/E0409 保留未触发；E0411 含义；`Dyn` 不匹配是运行时 E0412 而非 `panic`；删除不存在的 `try_add`/`try_mul`/`unit_pow_dyn`/`lerp`；§4.9.5 示例改为 `det_unit`/`pow_dyn`/`check`；§4.3 统一算法重写 | E0318 与刚性参数入错误表；`pow_dyn`、`det_unit`、`value_of` 签名；`check` 结果类型须标注（E0304） | 与 §3.5.5 重复的张量表；含糊的伪代码；“此前被静默接受”的叙述 |
| `appendix-a-grammar.md` | `nat` 补 `*` 与括号；负数字面量不是模式；`dimension_decl` 多余的 `private`；`return`/上下文关键字的错误说法 | — | 与 §2.6 重复的优先级表 |

**本批章节（05–07、09、附录 B）**

| 文件 | 修正 | 增补 | 删除/移出 |
|---|---|---|---|
| `05-units.md` | `kkm` 为 `E0501`（非 `E0503`）；`E0505` 是重复单位名（同一量纲可有多个尺度 1 单位）；`E0506` 是“尺度对该指数无精确值”（先用后声明为 `E0501`，不存在单位环）；单位全局可见、不能用 `Units.SI.km` 限定；`25.0 °C + 10 K` 合法，仅 `Real` 变量报 `E0302`；`100 u` 示例（`u` 是原子质量单位）；`e_charge` 不存在；`kat` 量纲 | 保留码 `E0502`/`E0503`/`E0507`、`W0501` 无发射点的状态说明；`delta_to` 显示套用偏移的已知缺陷 | 日期实现状态段、单位/常数计数、rssn 3.0.0-rc.1 旁注 |
| `06-semantics.md` | `decide : Prop -> Option<Bool>`；`dsolve` 示例量纲错误；`point(300.0, K)` 须 `to(°C, …)` 才显示 `26.85 °C`；栈规则（CLI 默认 1024 MiB、深度改由剩余栈限制、库默认 2048、`max_io_depth`）；相位表 | `E0804` 含 `assuming` 为假；`E0811` 入错误表 | `with_budget` 冗述（留作状态）、重复示例 |
| `07-modules.md` | 清单只读 `[package]`（`name`/`version`/`entry`）与 `[dependencies]`，其余表/键为 `E0605`；`private` 仅限六类声明（`private instance` 为 `E0202`）；`E0603` 仅在同名异类型时；`E0608` 仅嵌入 API，CLI 缺 `main` 无码；单位不依赖 import；`aevia doc` 输出 Markdown | 名义身份规则凝练为要点 | 约 40 行日期实现叙述 |
| `09-runtime.md` | crate 表；`:dims` 的基量纲名；CLI 清单与 `--help`/源码一致；磁盘缓存状态不再称 registry 客户端未实现；默认栈 1024 MiB | `publish`/`login`/`search`/`info`/`yank`/`verify`/`docs`/`watch`、`aevia-pkg publish` 选项、`AEVIA_PKG`/`AEVIA_REGISTRY`；`--no-cache`/`--out`/`--jobs`；`--help` “no registry” 残句的状态说明 | 日期注记、重复的 registry 文字 |
| `appendix-b-diagnostics.md` | 各码含义对照实际发射（`E0108`/`E0209`、`E0314`、`E0402`、`E0411`、`E0412`、`E0701`/`E0702`、`E0805`、`E0902`、`E1001`–`E1006`）；示例改为真实输出 | `W0602` 入表；`W0102`/`W0203` 标保留；`E0806`–`E0809` 被内建借用的说明 | 保留 B.1–B.17 编号，无整节删除 |

**审校中发现、不在本批范围的陈旧点**：`09-runtime.md` §9.11 末尾仍写“registry 客户端仍未实现（依赖只有 path 依赖）”，与 §13.7 和 Revision 15 不符，应由该章的批次更正。

---

## 1. 规范演进总结

- **Revision 0 (2026-10-31)**：确立语言核心定位（纯函数式、量纲作为类型、静态秩与动态形状张量、符号/数值双相、rssn 后端）。
- **Revisions 1–6 (2026-11-01 ~ 2026-11-04)**：双向类型检查、单位推导与仿射温标、行多态与模式匹配、ODE/PDE 统一接口、诊断码（E0100–E1000）。
- **Revisions 7–10 (2026-11-05 ~ 2026-11-07)**：标准库模块树（§8.20）；原生 JIT（`aevia-core/src/jit.rs`，`jit` feature）；`par_map` 的工作线程副本求值（`--jobs N`）；`read_npy`/`write_npy`。
- **Revision 11 (2026-11-08)**：修复 `BigRational` 零底负幂 panic 与超大幂次 DoS；词法支持 Unicode 上标括号；LSP 零长度消息死循环与 `did_change` 崩溃；符号引擎 `0^(-n)` 奇点与分数根保护；张量 Cons 维度校验；测量值零除；RSSN FFI 批量计算的机器可读 `diagnostic_codes`；稠密特征值 0×0/1×1 边界。

## 2. 当前实现状态（对齐旧审计文档的过时记录）

1. **`par_map` 并行求值**：已实现（Rayon + 独立求值器副本，输入序，`--jobs N`）。
2. **`@jit`**：已实现（`jit` feature；纯数值分支与自尾递归降为 Cranelift，不支持的结构回退解释器）；默认构建 `core.jit` 关闭。
3. **标准库模块导入**：已实现，41 个 `.aev` 模块文件在 `crates/aevia-std/stdlib/`。
4. **NPY 读写**：已实现（静态秩校验）。

## 3. Revision 12（2026-11-08）：缓存、`Bcsr`、源码算子、非交换 SDE

1. **缓存。** rssn `Session` 请求级答案缓存（默认开启；键为请求结点、模板与配置指纹；撞到墙钟上限的运行不缓存；注册插件/单位时整体失效）与子项引理缓存（`CachePolicy::lemmas`，默认关闭）；
   Aevia 按模块的磁盘接口缓存 `.aevi`（§9.11，`AdtId` 落盘时换成符号键）。
2. **`Bcsr`（§8.4.1）。** rssn 新增 `kernels::bsr::Bsr`；Aevia 新增 `SparseFormat::Bcsr`、`to_bcsr`/`block_size`、`convert<Bcsr>` 自动选块；`cg_with` 预条件共轭梯度、对称正定的 Cholesky 优先 `spsolve`、稀疏切片。
   `E1001("storage.sparse.block")` 删除。
3. **源码算子（§13.5.2）。** `@op` 以检查得到的量纲注册量纲签名；模块绑定换成包装（数值实参执行函数体、符号实参构造算子结点）；`@rule` 在算子之后追踪；规则解析器接受 `ns/name`。
4. **非交换 SDE（§8.9.1）。** Stratonovich 非交换 Milstein、非交换 SRK、自适应 Stratonovich（精确 Itô 转换）；方法名选格式族与积分解释，噪声结构选内核；`terms`/`cells` 有默认值；记录状态 + 一般噪声。
5. **使用即报错的功能。** `symfun` 应用构造未定函数项；新增 `dsolve`（§6.4.5）；`@jit(target = Cpu)` 被接受，`Gpu`/`Distributed` 在检查期报 `E1001`；单文件规范改由 `tools/bundle_spec.py` 生成。
6. **长张量字面量。** 超过 128 个元素的字面量不再 `E0304`：变量链迭代跟随，重复数值约束去重，2000 元素字面量的检查从 ~7 s 降到 0.6 s。
7. **rssn 极限。** `limit(sqrt(x+1) - sqrt(x), x, oo)` 经 Puiseux 代换求出；修正 l'Hôpital 预判的一个错误（`limit(sqrt(x)*(sqrt(x+1) - sqrt(x)), x, oo)` 应为 1/2）。

## 4. Revision 13–22

**Revision 13（2026-11-07 ~ 11-09）：信任边界与规则收紧。**
单位表达式的空白规则（§1.8.1）：空白是单位与算术的分界，`3 m * x` 是 `3 m` 乘 `x`，紧贴的 `3 m*x` 仍是单位表达式；未解出的量纲变量作指数底数时令其为 `1`（§3.4.3）；声明的量纲参数是刚性的，
违反是 `E0318`（§4.4.1）；`show` 保留 `Float` 的相位（`10.0` 与 `10` 不同，§6.12.3）；`Set<τ>` 有了值与操作（§8.1.5.1）；运行时信任边界（§9.2.3）：运行时执行检查器的证明而不重新证明；
字面量折叠（§3.10.4）：一个字面量只舍入一次，闭合的字面量算术树是编译期常量；单位制 `module M with units natural|planck|atomic|geometrized`（§5.11，`E0511`/`E0512`）；§9.9 命令与 flag 对齐实际 CLI。

**Revision 14（11-10）：rssn 绑定自动化。** 领域模块 `Rssn_<Domain>`（§10.3.8）：manifest + 声明式 overlay，泛型转换，错误走 `Result`，覆盖检查与报告；维度证据；丰富类型 `Zmod`/`Perm`/`Graph`/`Group`/`PermGroup`。
仍缺：`Zmod<n>`/`GF<p>` 的类型级模数与运算符重载（§10.3.8 列出所需的语言改动）。

**Revision 15（11-10）：包平台与依赖解析。**
`aevia-resolve`（SemVer 2.0 `Version`/`VersionReq`、带冲突回跳的回溯解析器、稀疏索引读取器，与穷举搜索 2000 轮随机对照）；清单支持 `Units = "^1.2"` 与 `{ package = "owner/name", version = … }`，
锁文件新增 `checksum`（来源 `registry+aevia`），`E0605` 对 git/未知键仍拒绝；`platform/` 的索引记录 `deps`（`package`/`req`/可选 `import`），发布时校验（无纯 path 依赖、依赖存在且有未 yank 的满足版本），
发布与 yank 各在一个事务内，`POST /api/v1/resolve`、dependents 端点；客户端 `aevia add/update/fetch/tree`（`--offline`）与 `aevia-pkg` 的 `index`/`vendor`/`lock [--check]`。
仍缺：按包的依赖名字空间（同名导入根指向不同版本仍是 `E0606`）；`[patch]` 覆盖；`aevia-pkg tree/lock` 只读根清单；镜像 `[source]` 替换；解压后的源树无逐文件摘要。

**Revision 16（11-12）：见证式自动类型化（§10.3.8）。** kernel 把引擎的等式判断（请求与答案同处一个等价类）记录为 sort 事实，经重放、聚类、变异探测生成 `signatures.witnessed.json`；Aevia 为没有 `sig` 的算子生成同形的类型化包装器
（dynamic 档从 1298 降到 871，新增 453 个自动类型化算子）；不透明句柄 `Value`；见证重载的量纲证据；精确/符号稀疏矩阵（`sp_*`）与 D-finite 判定（`holonomic_*`）随之可用。
仍缺：见证参数名由类型推出；元组与参差列表留在 dynamic 档；多重载以 `_int`/`_real`/`_float` 后缀区分。

**Revision 17（11-12）：数值算子组、维度无关名字、GPU `f64`。** `Rssn_Numeric` 吸收 rssn `numeric` 规则集新增的 73 个算子（稳健统计、窗与谱、小波、Krylov/稀疏/低秩/张量、有限差分、复变特殊函数、表达式算子）；
约定：结果是 `Float`（内核只编码浮点）、方法/小波/统计量是符号（每个名字只 `let` 一次）；维度无关的名字（`heat_fd` 等的空间维数是嵌套深度，符号 PDE 与仿射变换同理）；
rssn 的 wgpu 设备以双单精度提供 `Dtype::F64`，新增启动融合规划器 `device::plan`（Aevia 侧 `Gpu(n)` 仍报 `E1001`）；见证包装器参数名由类型推出。

**Revision 18（11-13）：按秩/维数命名的多签名。** overlay 的 `sig op as name(..)` 可对同一算子重复；`codec Type - from_fn`（只解码类型）；有限差分的 `_2d`/`_3d`；秩多态张量算子 `tensor_norm_<r>`/`tensor_inner_<r>`/`tensordot_<ra>_<rb>_<k>`
与分解 `hosvd`/`hooi`/`cp_als`/`tt_svd`/`tt_dense`（r = 2–4）；混合列表与列表访问器 `from_value<Tensor<1, Value>>`；`Rssn_Numeric` 全部 typed。
仍缺：秩超过 4 的张量算子与分解仍走 `Rssn.dynamic`。

**Revision 19（11-13）：语言侧补全。** 秩 2–4 的 `sum`/`prod`/`mean`/…/`transpose`（秩 ≥ 2 反转轴序）；`Result` 的 `map`/`and_then`/`unwrap_or`/`is_ok`/`is_err`/`ok`/`ok_or`；`first`/`last`/`cumsum`/`cumprod`/`argmax`/`argmin`、
`round(x, n)`、`is_nan`/`is_finite`/`is_infinite`；诊断建议（`E0407` 指出 `m` 是单位、拼写建议、记录字段提示）；`aevia fmt` 保留语句间空行。

**Revision 20（11-13）：FFT 族与序列操作。** `fft`/`ifft` 接受 `Complex`/`Float`/`Int`；`fft2`/`fftn`/`fft_axis`/`fftn_axes`/`rfft2`/`rfftn`/`irfft*`/`fftshift` 等多维变换（行列分解）；`fft_norm(x, Forward)` 的归一化修正为正向 `1/N`；
秩 ≥ 2 的 `reverse`/`filter`/`any`/`all`/`find`/`first`/`last`/`take`/`drop`/`concat` 按第一轴的行（`reverse` 的语义变化：不再展平）、`reverse_axis`；单位建议的并列决胜（编辑距离 → 包含最小编辑 → 常用单位 → 字典序）。

**Revision 21（11-13）：kernel 诊断进入 `Err(OpError)`。** rssn 新增 `Diag`/`DiagKind` 与 `Session::explain`；桥在算子留下自己的未约化调用时取原因，`Rssn.dynamic` 的 `Err` 形如
`rssn.diag.<kind>: <包装器名>: argument `<参数名>` (#<位置>): <消息>`；类型化调用位置抛出 `E0807`；`Rssn_Numeric` 新增 `poisson_fd_bc`/`heat_fd_bc`/`wave_fd_bc`、`advect_fd_upwind`、`tensor_contract_*`、`tucker_dense_*`、`cp_dense_*`
等。仍缺：只有 `rules::numeric::TABLES` 的算子记录原因；`OpError` 没有 `code(e)`/`message(e)` 访问器。

**Revision 22（11-13）：秩不再有上限。** 秩多态操作的签名是以秩为参数的**模板**（`builtins::rank_schemes_for`），检查器在内置候选全部失败时按实参的秩实例化模板；秩 5、6 的 `fftn`/`sum` 与秩 2 走同一路径；
`fold`/`fold_left`/`sort`/`sort_by`/`sort_float`/`zip`/`enumerate` 在秩 ≥ 2 上由元素级（展平）改为**按行**（语义变化）；新增 `sort_by_key`、`sort_axis`、`argsort`、`rfftn_axes`/`irfftn_axes`；`sort_by` 换成稳定归并排序。
仍缺：用户函数不能对秩多态；`cumsum`/`cumprod`/`argmax`/`argmin` 对秩 ≥ 2 没有按轴形式。

## 5. 记录在案的未实现边界（Known Gaps）

1. **包依赖**：registry 依赖、SemVer 解析和带校验的本地副本已实现（Revision 15）；`git` 依赖不存在；按包的依赖名字空间、`[patch]` 覆盖、镜像 `[source]` 替换尚未实现。
2. **设备后端**：`Gpu`/`Distributed` 只是可构造的运行时值；`on`/`to_device`/`@jit(target = Gpu(..))` 报 `E1001("core.gpu")`/`E1001("core.distributed")`；`Device.map` 等批量原语未作为语言原语暴露（§10.6.5）。
3. **稀疏**：稀疏值不能进入 rssn 项图；用户自定义格式要等 rssn 开放 `Format`；`gmres_with`/`bicgstab_with` 不接受精确元素；没有稀疏特征值/SVD（§8.4.1、§13.9.5）。
4. **SDE**：Brownian 桥稠密输出与积分中途的终止事件未提供（`sde_at` 是线性弦，`sde_events` 是事后截断）；解释器侧闭包 `!Send`，并行系综回退为顺序（§8.9.1）。
5. **ODE**：多重打靶/配置法 BVP；`AutoSwitch` 按步数预算判定而非特征值估计（§8.9）。
6. **类型系统**：`Zmod<n>`/`GF<p>` 类型级模数；用户函数对秩多态；符号指数（§12.8.2）。
7. **Plot**：只有 HTML/SVG-inline 渲染器，无独立 `.svg`/PNG/JPEG 输出（§8.12）。
8. **REPL**：Ctrl-C 取消当前求值（§9.10）。
