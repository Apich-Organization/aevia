# 附录 C — 修订记录与已知实现边界

本附录记录 Aevia 语言规范与实现的演进历程、当前功能支持状态以及明确记录在案的未实现边界。

---

## 1. 规范演进总结

- **Revision 0 (2026-10-31)**：确立语言核心定位（纯函数式、量纲作为类型、静态秩与动态形状张量、符号/数值双相、RSSN 计算内核后端）。
- **Revisions 1–6 (2026-11-01 ~ 2026-11-04)**：完成双向类型检查、单位推导与仿射温标收敛、行多态与模式匹配、ODE/PDE 统一接口规范、严格诊断码（E0100–E1000）定义。
- **Revisions 7–10 (2026-11-05 ~ 2026-11-07)**：
  - **标准库就绪**：实现 §8.20 标准库模块层级（`Math`、`Units.SI`、`Calculus`、`LinearAlgebra`、`Sde` 等 37 个模块），支持全路径与解构导入。
  - **原生 JIT 支持**：完成 `aevia-core/src/jit.rs`，在 `feature = "jit"` 下将纯 Float/Bool 核心 IR 降译为 RSSN/Cranelift 机器码磁带与循环执行。
  - **Rayon 并行求值**：在 `aevia-core/src/parallel.rs` 中实现 `par_map` 多工作线程副本（Replica）求值，由 CLI `--jobs N` 设定线程池。
  - **二进制 I/O**：实现 `read_npy` / `write_npy` 张量数据持久化。
- **Revision 11 (2026-11-08 当前版本)**：
  - 修复 `BigRational` 零底负幂 panic 与十进制超大幂次 DoS 隐患。
  - 增强词法分析器支持 Unicode 上标括号 `⁽` `⁾` 及严格符号语法。
  - 修复 LSP 传输层零长度消息死循环及 `did_change` 未跟踪文档解包崩溃。
  - 强化符号引擎 `0^(-n)` 奇点处理、分数根保护及除法渲染括号包裹。
  - 修复张量 Cons 维度一致性校验与测量值零除异常。
  - 完善 RSSN FFI 批量计算 `diagnostic_codes` 机器可读编码及确定性 `bincode-next` 序列化。
  - 修复稠密特征值求解器 0x0 与 1x1 边界条件。

---

## 2. 真实已实现能力与状态对齐

针对历史审计文档中可能存在的过时记录，当前真实代码状态说明如下：

1. **`par_map` 并行求值**：已实现。通过 Rayon 线程池与独立求值器副本（`Replica`）并行映射，保持确定性输入输出顺序，并通过 `--jobs N` 控制并发度。
2. **`@jit` 机器码生成**：已实现。在启用 `jit` feature 时，通过 `aevia-core/src/jit.rs` 将纯数值分支与自尾递归循环降级为 Cranelift 机器码执行；不支持的高阶与闭包结构安全回退至解释器。
3. **标准库模块导入**：已实现。37 个标准库 `.aev` 源码模块位于 `crates/aevia-std/stdlib/`，支持 `import Math { sqrt }` 与 `open Units.SI` 等语法。
4. **NPY 数据读写**：已实现。支持 `read_npy<rank>` 与 `write_npy` 的静态秩检验与文件数据读取。

---

## 3. 记录在案的未实现边界（Known Gaps）

以下 5 项是规范中已规划但当前代码尚未实现的核心空白，在编译期或运行期会显式报告相应能力诊断码（`E1001` 或 `E0605`）：

1. **模块级增量编译接口缓存（`.aevi`）**：
   - 当前 `aevia-cli` 仅具备全程序文本级诊断缓存（`.aevia/cache/check-*.txt`），未实现单个模块的序列化二进制接口文件（`.aevi`）。每次编译均从源码完整遍历解析与检查依赖树。
2. **块状稀疏张量存储（Block CSR / BSR）**：
   - 语法与类型系统已接受 `Block`/`Bcsr` 格式，但运行时底层仅具备 `Coords` (COO) 与 `Compressed` (CSR/CSC) 存储模式。构造 Block 矩阵时触发 `E1001("storage.sparse.block")`。
3. **源码级插件与算子动态注册（`core.plugin.source`）**：
   - 源代码中编写的 `@op` 与 `@rule` 无法在运行时直接编译为 RSSN 动态原生算子，`Rssn.register` 调用显式受限于 `E1001("core.plugin.source")`。目前仅支持以 Rust 编写并链接原生插件。
4. **非交换随机微分方程求解方案（`sde.noncommutative`）**：
   - SDE 积分器中的 Milstein/Heun 修正目前仅支持交换噪声结构。传入非交换噪声结构（`commutative: false`）或自适应 Stratonovich 求解器时触发 `E1001("sde.noncommutative" / "sde.interpretation")`。
5. **远程包注册表与 SemVer 依赖解析**：
   - 包管理器 `aevia add` 仅支持本地文件系统路径依赖。指定语义化版本号范围从远程索引拉取依赖（`foo = "^1.0"`）触发 `E0605`。
