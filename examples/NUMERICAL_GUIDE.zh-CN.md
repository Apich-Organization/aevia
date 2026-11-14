# 在 Aevia 中进行数值计算

运行示例中的可执行断言：

```sh
cargo run -p aevia-cli -- test examples/dimensioned_numerics.aev
```

数值算法由 RSSN 实现；Aevia 负责语言绑定、量纲见证、回调及对象生命周期。
带单位的数据在数值计算前转换为相干单位，并检查输入之间的量纲关系。
输出张量、记录字段及动态值保留实际的物理量纲。

| 示例 | 展示内容 |
| --- | --- |
| `dimensioned_numerics.aev` | 插值、归一化多项式拟合、回归、矩阵函数、自动微分、积分和优化 |
| `physical_functions.aev` | 生成的 SI 函数契约、函数内积与投影 |
| `option_numerics.aev` | 带类型的 Option、统计结果及几何交点 |
| `native_autodiff.aev` | Dual 回调、作用域内反向微分及导数量纲 |
| `native_backend.aev` | 本机会话编译、独立坐标参考量及批量调用 |
| `native_execution.aev` | 混合量纲表达式、反向梯度、设备列数据及几何单位制相对论 |
| `native_algebra_models.aev` | 浮点与精确矩阵、样条、多项式、分解及逼近对象 |
| `native_kernel_families.aev` | 编码、有限域、排列、拓扑、同调及稳健统计 |
| `native_discrete_objects.aev` | 精确群、李代数、几何点、复数包围及有状态随机流 |
| `native_linear_tensors.aev` | 张量列车和压缩线性代数对象 |
| `native_mechanics.aev` | 有限元材料、模型与带单位应力 |
| `native_jacobian.aev` | 原生 Jacobian 提供器与结构化回调 |
| `native_ode_objects.aev` | 位置与速度各自的参考量、原生轨迹及求解预算 |
| `native_particle_engines.aev` | 分子动力学几何、SI 粒子能量、带单位多重网格、GPE 归一化及分步波函数范数 |
| `native_flow_conservation.aev` | DG、FVM、AMR 的守恒及各分量参考量 |
| `native_continuum_models.aev` | BEM 几何、混合量纲气体通量、超弹性应力参考量、Poisson 有限元与四边形刚度 |
| `portable_models.aev` | 可移植 k-d 树、B 样条、RBF 及边界斜率 |
| `ode_adaptive_geometric.aev` | 保留所选公式的自适应几何积分、稠密输出及事件 |
| `ode_adaptive_dae.aev` | 隐式与半显式 DAE、各分量量纲及参考量 |
| `ode_adaptive_dde.aev` | 时滞方程、历史函数及已接受步的插值 |
| `pde_adaptive_heat.aev` | 1D–3D 显式、后向 Euler、Crank–Nicolson 和 ADI |
| `pde_adaptive_wave_event.aev` | 位移与速度的独立量纲及终止事件 |
| `pde_adaptive_advanced.aev` | FEM、FVM、DG 与谱方法 |
| `sde_adaptive_implicit.aev` | 自适应隐式布朗方法及可复现路径 |
| `sde_adaptive_jump.aev` | 保留到达时刻和跳跃标记的跳扩散 |
| `sde_adaptive_levy.aev` | 明确参考时间约定的稳定 Lévy 噪声 |
| `resampling.aev` | Bootstrap、Jackknife、样本与统计量的独立量纲 |
| `sparse_spectral_dimensions.aev` | Lanczos、Arnoldi、移位求逆、AMG 及谱量纲 |

具体函数通过生成的 `Rssn_numerical_vector`、`Rssn_numerical_matrix`、
`Rssn_numerical_stats`、`Rssn_numerical_special` 等模块调用。
静态可表达的接口使用声明的类型；原生对象、运行时形状及其他动态接口通过
受检查的 `Value` 传递，并用 `from_value<T>` 恢复类型。
`Rssn.dynamic(name, args)` 提供统一的 `Result<Value, OpError>` 调用入口。
静态可表达的原生零参数操作使用 `f()` 并返回 `Result`；导入模块不会执行
构造器或探测可选后端。

同一个数值张量只有一个元素量纲。混合位置、速度等数据应使用记录、元组，
或包含独立量纲见证的 `Tensor<1, Value>`。原生表达式带为每个坐标、参数和
输出分别声明正参考量；梯度分量恢复输出/相应输入量纲，Hessian 的 `(i, j)`
项恢复输出/(输入_i × 输入_j) 量纲。
带单位多项式使用归一化坐标，避免把不同幂次的系数放进同量纲数值张量。

原生参考系接口明确区分带单位输入和归一化系数。`num_grid_frame_new` 接受
正的长度、时间及场值参考量；`num_grid_mechanical_references` 接受长度、
时间、质量及温度参考量。文档标明为归一化的普通 `Float` 系数或数组按这些
参考量解释，并拒绝带单位输入。`{ normalized, reference }` 等结果表示两者
的乘积；其中的归一化数组本身无量纲。不同物理分量使用各自的参考量。
降维声学使用质量/长度^d 的密度和能量/长度^d 的压力；二维超弹性能量按单位
面外厚度给出。

泛型和不透明接口使用明确的具体化：浮点/精确矩阵、常量空间维数、采样初值、
有限提供器描述、拥有其存储的编译函数，以及受认证的会话对象。清单声明了
每种具体化及其形状和工作预算；Rust 泛型签名不代表支持任意 Aevia 类型或维数。
可变切片接口返回或更新受检查的拥有型数据；Rust 借用布局和原始指针不作为
语言值暴露。JIT/GPU 接口在相应 RSSN 功能未启用时返回 `Unsupported`。
普通示例使用默认功能即可。可从 CLI 构建可选后端及 NPY 文件适配器：

```sh
cargo run -p aevia-cli --features jit,gpu-wgpu,npy -- test examples/native_execution.aev
```

`npy` 启用 NPY 文件支持。后端功能标志仅启用相应适配器；实际后端及 GPU
设备的可用性仍在运行时检查。

会话对象每次调用都检查种类、所属会话及存活状态。`num_native_release`
成功后使句柄的所有副本失效；存在存活依赖时，父对象的释放请求会被拒绝，
应先释放依赖对象。临时反向微分回调句柄在回调结束后失效。编译回调通过父对象
依赖或独立拥有的快照保留执行代码；快照保留的存储计入预算。可移植模型则保存版本化定义数据，查询
时验证并重建原生模型，具有不同的生命周期和重建开销。

自适应求解器显式提供容差、步长约束与有限工作预算。原生旧接口没有的控制项
会被拒绝。时间容差不消除空间离散误差；变步长通常不保持固定步长方法的辛性。
随机拒绝步重用有界的耦合噪声离散；分辨率或预算不足时返回错误，不重抽噪声
来掩盖拒绝。Lévy 方法仅在 alpha=2 时支持布朗高阶修正。

源码覆盖表位于 `tools/rssn-api-audit/inventory.tsv`，语言可达性报告位于
`tools/rssn-api-audit/RSSN-COVERAGE.md`。`object_binding!` 从类型声明生成参数
个数、模式、编解码和注册信息；简单函数与记录使用经审阅的量纲策略生成绑定。
`cargo run -p rssn-api-audit -- --coverage-check` 独立检查所有清点出的可调用
接口是否有绑定；只有源码解析证明是同一个 Rust 项目时，重导出才继承覆盖。
宏及未解析重导出的指纹仍单独保留，不能据此宣称运行时可调用。

使用 `tools/sync-kernel.sh` 更新契约、绑定及清单。验证时优先运行对应的原生
契约测试与 AST/IR 示例目标；无需为每个适配器反复运行整个工作区测试。
