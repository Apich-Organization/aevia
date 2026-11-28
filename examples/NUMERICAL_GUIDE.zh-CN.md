# 在 Aevia 中进行数值计算

运行示例中的可执行断言：

```sh
bazel run //:aevia -- test examples/dimensioned_numerics.aev
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
| `native_backend.aev` | 本机会话编译、独立坐标参考量、带量纲的列及量纲校验 |
| `native_execution.aev` | 混合量纲表达式、反向梯度、设备列数据及几何单位制相对论 |
| `native_algebra_models.aev` | 浮点与精确矩阵、样条、多项式、分解及逼近对象 |
| `native_kernel_families.aev` | 编码、有限域、排列、拓扑、同调及稳健统计 |
| `native_discrete_objects.aev` | 精确群、李代数、几何点、复数包围及有状态随机流 |
| `native_linear_tensors.aev` | 张量列车和压缩线性代数对象 |
| `native_mechanics.aev` | 有限元材料、模型与带单位应力 |
| `native_jacobian.aev` | 原生 Jacobian 提供器与结构化回调 |
| `native_ode_objects.aev` | 位置与速度各自的参考量、原生轨迹及求解预算 |
| `native_particle_engines.aev` | 分子动力学几何、SI 粒子能量、带单位多重网格、GPE 归一化及分步波函数范数 |
| `native_flow_conservation.aev` | DG/FVM 的守恒，及保留各分量量纲的预备精确气体 Riemann 求解 |
| `native_continuum_models.aev` | BEM 几何、周期 Helmholtz 批量求解、Ricci 曲率、混合量纲气体通量、超弹性应力与有限元 |
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

`Rssn_numerical_vector` 提供 `num_signal_hilbert`、
`num_signal_analytic_signal`、`num_signal_analytic_envelope` 和
`num_signal_zoom_fft`。同一模块还提供有偏自协方差 `num_signal_autocovariance`、
自相关 `num_signal_autocorrelation` 和 Yule–Walker 偏自相关
`num_signal_partial_autocorrelation`。传入样本张量和最大延迟，结果包含从零到该值的所有延迟。样本以米计时，自协方差以平方米计，两种相关系数均无量纲。

运行 `aevia rssn --describe num_signal_autocovariance` 可查看当前内核提供的算子文档、重载和 Aevia 绑定名称。

`Rssn_numerical_vector.num_signal_filtfilt(b, a, samples)` 对边缘做奇反射后执行
正反两次滤波。滤波器系数无量纲，输出保留样本数和量纲。系数无效或稳态无定义时返回定义域错误。
[压力样本示例](zero_phase_filter.aev)

`Rssn_numerical_vector.num_cauchy_sharp_root_bound` 使用按幂次升序排列的复系数，
返回 Cauchy 根界。系数均为正实数时，`num_enestrom_kakeya_bounds` 返回含
`lower`、`upper` 半径的记录。自变量无量纲，系数共用的量纲会抵消。
`Rssn_numerical_special` 提供单位圆盘内的三个 Koebe 界。
[根界示例](polynomial_root_bounds.aev)

用 `Rssn_numerical_special.num_mp_real_round` 构造无量纲记录
`{mantissa, exponent, precision}`，三个字段均为精确整数。
`num_mp_real_compare` 精确比较记录值。加、减、乘算子分别接收以位计的
结果精度和舍入模式，例如 `"nearest"`（等距取偶）或 `"up"`。
[精确二进制多精度示例](dyadic_real.aev)

`Rssn_FiniteField.poly_mul_ntt_mod(a, b, p)` 在奇素数模下做精确系数卷积。
变换长度为不小于结果长度的最小二次幂，须整除 `p - 1`。
`Rssn_NumberTheory.extended_gcd_u64` 返回具名 Bézout 系数。
[模多项式示例](modular_polynomials.aev)

`num_timeseries_kalman_filter` 和 `num_timeseries_kalman_smooth` 接收线性状态空间模型。
状态分量共用一个量纲，观测分量可以共用另一个量纲。滤波返回状态均值、协方差矩阵和对数似然，
最后一个参数是对数似然使用的正观测参考量。平滑按每条观测返回一个状态均值。
同一模块的 `num_timeseries_integrate` 可还原多阶差分。
`num_timeseries_rolling_moments(samples, window, ddof)` 计算带量纲序列的完整滑动窗口。
返回的 `mean` 保留样本量纲，`variance` 使用平方量纲。总体方差取 `ddof=0`，
样本方差取 `ddof=1`，此时窗口需至少包含两个样本。
[带量纲跟踪示例](kalman_tracking.aev)

用 `Rssn_numerical_matrix.num_control_c2d_zoh(a, b, c, d, dt)`
按带量纲的采样周期离散化线性状态空间模型。返回的记录可直接传给
`num_control_simulate_discrete(model, x0, inputs)`。对于不等间隔的时间网格，
可用 `num_control_simulate_continuous(a, b, c, d, x0, inputs, times)`。
第一行输出使用初始状态，并计入输入的直接馈通项。
[带量纲仿真示例](measured_state_space.aev)

将同一条记录传给 `num_control_discrete_n_states`、`num_control_discrete_n_inputs`
和 `num_control_discrete_n_outputs`，即可获取状态、输入和输出的数量。
`num_control_discrete_poles` 返回复数特征值，`num_control_discrete_is_stable`
检查所有极点是否严格位于单位圆内。可控性矩阵和可观性矩阵分别保留 `b` 和 `c`
的量纲。上述接口接受离散模型，其中状态矩阵 `a` 无量纲。

对无量纲复数 Taylor 系数，`Rssn_Special.pade_complex` 返回包含分子和分母的记录。
用 `Rssn_Special.eval_pade_complex` 可在多个复数点复用该记录。
`recenter_taylor_complex` 按顺序移动 Taylor 多项式的展开中心，
`wynn_epsilon_complex` 对部分和序列做收敛加速。
[可运行的延拓测试](../tests/domain_analytic_continuation.aev)

`Rssn_Special.holonomic_order`、`continue_holonomic` 和
`holonomic_monodromy` 接收无量纲多项式系数矩阵，每行按幂次升序排列。
第 `j` 行乘以第 `j` 阶导数，初始状态从函数值开始逐阶列出。计算单值化矩阵时
传入闭合路径。`borel_pade_complex` 在有理 Borel 变换的正实积分轴上遇到极点时，
取确定的上侧积分。

`num_continuation_laurent_fft` 接收复数回调、量纲相同的中心和正半径、
负幂与正幂的阶数、采样数和求值预算。系数对应局部变量 `(z - center) / radius`，
保留回调输出的单位，起始幂次由 `first_power` 给出。结果还包含半径和实际采样数。
[可运行示例](../tests/domain_holonomic_borel_laurent.aev)

`num_native_irfft_plan_new` 按固定的实数输出长度准备逆实数 FFT。用同一计划反复调用 `num_native_irfft_plan_apply`，结果保留频谱的物理量纲。`num_native_irfft_plan_output_len` 返回实数样本数，`num_native_irfft_plan_spectrum_len` 返回所需的非负频率分量数。四个和五个实数样本都需要三个频率分量。[可运行示例](native_kernel_families.aev)

`Rssn_Optimizer.solve_linear_program` 在有限下界和可选上界下最小化线性目标。
`upper_present` 标记哪些上界生效，每行约束选择 `Le`、`Ge` 或 `Eq`。
若坐标、目标值、约束右端的量纲依次为 D、E、C，目标系数的量纲为 E/D，
约束系数的量纲为 C/D。分别传入量纲为 D、E、C 的正参考量。结果包含状态、
带量纲坐标及可选目标值。`simplex_budgeted` 对变量施加非负约束，可选择
`Dantzig`、`SteepestEdge` 或 `Bland` 定价，两个阶段共用一个枢轴预算。
[可运行的线性规划示例](../tests/domain_linear_programming.aev)

`num_stats_simple_linear_regression` 接收各自带量纲的预测张量和响应张量。斜率的单位为响应单位除以预测单位，截距保留响应单位。内核在极端尺度下缩放样本，以处理超出浮点范围的中间矩。[可运行示例](native_kernel_families.aev)

`num_polynomial_model_try_long_division` 对坐标参考量相同、系数按降幂排列的多项式做长除法。除式可有前导零，零多项式作除式会返回错误。商的系数单位为被除式单位除以除式单位，余式系数保留被除式单位。[可运行示例](native_algebra_models.aev)

`Rssn_ComplexPolynomial` 接收按次数升序排列的无量纲复系数。`durand_kerner` 和 `aberth_ehrlich` 返回与次数相同数量的候选根，以及每个根的相对后向误差和重建归一化系数时的最大偏差。非零常数返回空张量。`laguerre` 接收复数初值，返回可选的候选根及其残差。零多项式和未收敛的求解会返回错误。后向误差衡量多项式的扰动，重根或病态根的残差很小时，坐标误差仍可能较大。

`Rssn_RiemannHilbert` 使用形状为 `2 × 2` 的复数张量。矩阵加减要求元素量纲一致，乘积的量纲为两个输入量纲的乘积，逆矩阵使用倒数量纲，Frobenius 范数保留原量纲。`evaluate` 可复用无量纲配点记录，其中包含 `(-1, 1)` 内严格递增的节点、正求积权重以及形状为 `n × 2 × 2` 的跃变密度张量。求值点须位于闭区间 `[-1, 1]` 对应的复平面线段之外。 `Rssn_numerical_callback.num_riemann_hilbert_solve_typed` 通过无量纲矩阵跃变回调生成该记录，验证两个端点为单位矩阵，并返回实际回调次数。

标量接口 `num_riemann_hilbert_cauchy_checked_typed`、`num_riemann_hilbert_plemelj_checked_typed` 和 `num_riemann_hilbert_factor_checked_typed` 报告绝对误差估计、收敛状态及回调次数。Cauchy 和 Plemelj 保留密度的量纲。标量分解从 `-1` 处的主辐角开始采样连续跃变相位，并分别报告指数化前后的误差。`num_complex_cauchy_fft_derivatives_checked_typed` 对局部坐标 `(z - center) / radius` 求导，因此返回张量具有统一量纲。双倍采样网格的结果差作为经验误差估计。

带量纲的圆形轮廓和解析回调可使用 `Rssn_numerical_callback` 的 `num_complex_root_circle_moments_typed`、`num_complex_root_circle_winding_typed` 和 `num_complex_root_circle_rouche_samples_typed`。矩基于 `(z - center) / radius` 的幂，返回值记录两种采样网格的差异。绕数要求两个网格的结果收敛到同一个整数附近。Rouché 接口报告严格不等式在各采样点成立的情况。回调接收已换算到相干单位的坐标，结果记录实际回调次数。

`num_native_categorical_new` 用有限非负相对权重预备采样表，同一张表可配合一个或多个会话随机流使用。`num_native_categorical_categories` 的计数包含零权重项。`num_native_categorical_sample` 返回按原权重顺序排列的零基索引，`num_native_categorical_fill` 按与连续单次抽样相同的顺序返回张量。[可运行示例](native_discrete_objects.aev)

`num_grid_simplex_positive_fraction` 返回线性水平集在边、三角形或四面体中为正的比例。传入对应的两个、三个或四个无量纲顶点值。边上的比例计算能保留可表示的极小开口，并处理差值超过 Float 范围的有限异号输入。[可运行示例](native_continuum_models.aev)

`num_analysis_homotopy_system_scaled` 接收归一化系数、指数矩阵、方程偏移量以及根和残差的物理参考量。结果按起始路径顺序排列，并用有限路径标记和偏移量标出带根坐标的路径。数值接口最多接受六个变量、4,096 个项、每个方程 16 次及 256 条路径。[可运行示例](native_kernel_families.aev)

`num_analysis_wavelets_dwt_plan_new` 按固定长度、小波和边界模式准备单层变换。`num_analysis_wavelets_dwt_plan_analyze` 返回保留样本量纲的近似和细节系数。`num_analysis_wavelets_dwt_plan_synthesize` 要求两个频带量纲一致，并恢复原信号长度。`num_analysis_wavelets_dwt_plan_signal_len` 和 `num_analysis_wavelets_dwt_plan_coeff_len` 分别返回所需信号与频带长度。[可运行示例](native_kernel_families.aev)

原生图算子支持 Bellman–Ford、有向无环图最短路、最大流、多源 BFS、强连通分量缩点图，以及无向图的桥和割点。
生成的 `Rssn_Numeric` 模块提供 Welch 相干度和正逆小波包变换。

`num_native_ordering_metrics` 接收 CSR 邻接偏移、邻居编号和从新位置映射到旧顶点的零基置换，返回整数半带宽 `half_bandwidth` 和下三角轮廓 `profile`。每条边按无向边处理，重复条目对两个指标均无影响。

`num_descriptive_histogram_edges` 按给定区间统计观测值个数。边界必须有限、严格递增并与观测值具有相同量纲。落在内部边界上的观测值计入右侧区间，最后一个区间包含最高边界。

`num_matfun_powm_psd` 接收无量纲的对称半正定矩阵和有限实数指数。正指数保留零特征值，负指数要求正定矩阵。合法输入的零次幂为单位矩阵。

`num_kdtree_nearest_batch` 接收可移植 k-d 树和查询矩阵，每行一个查询。结果按查询顺序返回零基索引和距离，距离保留模型的坐标量纲。整个批次只重建一次树。[可运行示例](portable_models.aev)

`num_native_real_roots_find_roots_in` 在有限闭区间内寻找归一化多项式的实根。端点、绝对容差和正坐标参考量须具有相同量纲。返回的有序实根保留该量纲，并包含位于端点的根。[可运行示例](native_discrete_objects.aev)

`num_finite_field_gf256_scale` 用同一个系数缩放整条字节向量。`num_finite_field_gf256_mul_add` 将缩放后的源向量逐字节异或到等长目标向量。两个算子与 Reed–Solomon 使用相同的 GF(256) 多项式。[可运行示例](native_kernel_families.aev)

`num_analysis_convergence_levin_condition_numbers` 返回 Levin 变换各个可用阶数的无量纲条件数。级数项允许带共同量纲，变换名称为 `U`、`T` 或 `V`。无穷值表示条件数不可用或无界。

`Rssn_ComplexDynamics` 使用无量纲二次映射 `z² + c`。逃逸函数返回越过半径的步数、
`escaped` 标志和可选的势估计。迭代预算耗尽时，势估计为 `None`。`quadratic_orbit`
包含起点。`quadratic_multiplier` 验证指定次数的迭代是否闭合，再返回数值分类。
`quadratic_periodic_points` 返回基周期符合要求的候选点和残差。`external_ray`
接收以圈为单位的角度，返回射线点与 Böttcher 坐标的相对残差。

自定义映射可调用 `Rssn_numerical_callback.num_complex_dynamics_newton_basin_checked_typed`，传入映射、导数、坐标尺度和残差尺度。导数奇异或迭代用尽时返回 `root = None`。回调预算用尽时返回错误。`num_complex_dynamics_koenigs_checked_typed` 先检查不动点闭合误差，然后返回截断坐标估计、带量纲的闭合误差和实际计算次数。

`num_analysis_ode_fixed_final` 在等步长网格上求值已保留的原生右端项程序，返回末态并恢复各分量的物理单位。方法 `0`、`1` 和 `2` 分别为 Euler、Heun 和经典 RK4，求值预算至少为步数的 1、2 或 4 倍。[可运行示例](native_ode_objects.aev)

`num_analysis_ode_etdrk4_diag` 为每个分量接收一个逆时间量纲的线性速率，并用已保留的原生程序求值非线性项。实指数权重在强耗散步进中保留可表示的稳态。[可运行示例](native_ode_objects.aev)

`num_native_geodesic_model_equatorial_timelike` 初始化未来指向的 Kerr–Newman 赤道类时轨道。物理参考系用几何长度表示质量、自旋和电荷。`num_native_geodesic_model_run_kerr_newman_geodesic_sampled` 按正整数步距保留坐标，并保留初始点及最终到达点。[可运行示例](native_execution.aev)

`num_native_ising_measure_seeded` 在热化后按指定步距测量每格点能量、绝对磁化与响应。物理参考系接收开尔文温度。结果包含最终自旋、完整采样区间数及 Binder 累积量。所有样本的磁化均为零时，Binder 返回 `None`。[可运行示例](native_particle_engines.aev)

`num_native_spin_potts_heat_bath_probabilities_d1`、`num_native_spin_potts_heat_bath_probabilities_d2` 和 `num_native_spin_potts_heat_bath_probabilities_d3` 读取零基格点的颜色条件概率。结果按颜色排列，且无量纲。查询不改变自旋或随机流。[可运行示例](native_particle_engines.aev)

`num_native_gpe_run_ground_state` 在指定的正尺寸网格上返回逆面积量纲的概率密度。步数或虚时间增量为零时返回归一化初态密度。[可运行示例](native_particle_engines.aev)

`num_native_fmm_prepare2d` 固定位置和开边界 FMM 选项。`num_native_fmm_plan_count2d` 返回所需源强度数目。`num_native_fmm_evaluate2d` 接收新的源强度和正的参考量。每个输入位置对应一行场值，量纲为源强度除以长度。`num_analysis_nbody_chain_accelerations` 按原始粒子顺序返回各粒子的三个带量纲加速度分量。[可运行示例](native_particle_engines.aev)

`num_native_engine_neighbours_build_d3` 在稀疏周期盒中只索引占用单元。`num_native_engine_compute_forces_d3` 使用同一近邻搜索，在结果中保留力及能量的物理量纲。[可运行示例](native_particle_engines.aev)

`num_native_quantum_schrodinger_plan_new` 在二维周期网格上预备固定势能。长度、带符号时间步、约化普朗克常数、质量和势能分别使用 SI 量纲。`num_native_quantum_schrodinger_plan_evolve` 复用预备相位，返回带符号的经过时间与末态，复振幅的量纲为长度的负一次方，展平后 x 轴变化最快。步数或时间步为零时保留初始波函数。[可运行示例](native_particle_engines.aev)

`num_grid_quad_element` 根据逆时针排列的顶点和力学参考量，计算 8×8 的 Q4 平面应力刚度矩阵。顶点整体平移时单元刚度不变，矩阵元参考量纲为力除以位移。[可运行示例](native_continuum_models.aev)

`num_fv_config_euler` 和 `num_fv_config_shallow_water` 配置有限体积数值通量。在选项记录中令 `riemann = "Hlle"` 可选择用 Einfeldt 波速估计的双波 HLLE 通量。记录同时指定空间重构和 CFL 数。[可运行示例](native_flow_conservation.aev)

`num_grid_hyper_prepare_point` 为固定无量纲变形梯度保留本构数据。`num_grid_hyper_point_piola`、`num_grid_hyper_point_tangent` 和 `num_grid_hyper_point_energy_density` 分别返回应力、材料切线及单位参考体积能量，三者均为压力量纲。`num_grid_hyper_point_tangent_action` 沿指定无量纲梯度方向直接计算应力增量。[可运行示例](native_continuum_models.aev)

`num_backend_call_batch_columns` 接收显式行数，以及按符号声明顺序排列的带量纲一维张量，每个已编译输入对应一列。各列保留自身单位，输出是按行顺序排列的带量纲向量。工作量足够大时使用当前 CPU 线程池。[可运行示例](native_backend.aev)

`num_native_sse_new` 使用随机种子、能量参考量和以能量倒数为量纲的逆温度准备量子 XXZ 自旋采样器。`num_native_sse_sweep` 在显式预算下推进一次扫掠，`num_native_sse_sample` 读取当前展开阶、总磁化强度和交错磁化强度。扫掠过程中耗尽预算会使对应句柄失效。同一种子在此版本内可复现轨迹。[可运行示例](native_particle_engines.aev)

`num_native_mm_compute_density_and_forces` 以旧版 SPH 模型的约化单位重算密度、压力和力。`num_native_mm_state` 按输入顺序返回当前粒子数据，位置、速度和力各有两个分量。[可运行示例](native_particle_engines.aev)

`num_native_md_morse_radial` 接收参考量制、粒子间距、势阱深度、逆长度参数和平衡间距。返回带物理量纲的能量 `energy` 和径向力 `radial_force`。平衡位置的能量为零，正的径向力指向间距增大的方向。[可运行示例](native_particle_engines.aev)

使用 `num_native_engine_rdf_merge_d1`、`num_native_engine_rdf_merge_d2` 或 `num_native_engine_rdf_merge_d3` 合并各自累积的径向分布帧。两个累积器共用分箱、截断半径和参考量制。每帧以自身粒子数和体积计算理想气体参考量。结果包含带量纲的半径和无量纲的密度比。[可运行示例](native_particle_engines.aev)

`num_analysis_pde_method_of_lines_1d_timed` 使用 `Radau5` 或 `Bdf1` 至 `Bdf5` 推进一维场。边界类型和已编译程序按左、右顺序传入。每个程序接收带时间量纲的参数。`Dirichlet` 返回场值，`Neumann` 返回外法向导数，量纲为场除以长度。用 `num_analysis_pde_field_data` 读取带量纲的场和末时刻。[可运行示例](native_continuum_models.aev)

`num_native_epd_goursat_samples` 用两条离散采样的特征边界求解 Euler–Poisson–Darboux 方程的 Goursat 问题。两组坐标均须严格递增，查询坐标按 `[rho, sigma]` 逐行传入。两条边界的场量纲须一致，并在公共角点取相同的值。结果保留该场量纲和查询顺序。[可运行示例](native_epd_goursat.aev)

`num_grid_solve_helmholtz_multigrid` 在结构化网格上求解 Helmholtz 问题，已编译空间源项的量纲为场除以长度平方。控制选项中的 `V`、`W` 或 `Fmg` 选择循环。`num_grid_multigrid_result` 返回归一化的场、物理参考量和相对残差历史。用数据乘以 `value_reference` 恢复带量纲的场值。[可运行示例](native_continuum_models.aev)

`num_dg_advection_prepare1d` 为固定周期网格和带量纲速度准备 Legendre–Gauss–Lobatto 权重。`num_dg_advection_step_prepared1d` 接收带量纲节点场和符合 CFL 上限的时间增量，按单元顺序返回更新后带原量纲的场。[可运行示例](native_flow_conservation.aev)

`num_grid_gas_prepare_riemann` 在力学参考系中固定左右两侧的归一化理想气体状态 `[密度, 速度, 压力]`。`num_grid_gas_sample_riemann` 和 `num_grid_gas_sample_riemann_batch` 接收带量纲的自相似速度 `x/t`，分别返回带量纲的密度、速度和压力。真空区的密度和压力为零，速度等于 `x/t`。[可运行示例](native_flow_conservation.aev)

`num_triangle_prepare` 为指定的 P1 或 P2 次数保留三角网格几何及求积数据。`num_triangle_solve_prepared` 接收采用同一物理参考系的扩散反应问题，返回三角有限元解。每次调用会求值当前问题的系数、源项和边界值。`num_triangle_prepared_dofs` 的计数包含自由节点和给定边界值的节点。[可运行示例](native_continuum_models.aev)

`num_grid_fem_cartesian_prepare` 为零边界值的均匀单位正方形或立方体保留 Q1 刚度谱。物理边长由网格参考系的长度决定。`num_grid_fem_cartesian_solve_prepared` 在每个求积点求值一次源项，量纲为场除以长度平方。`num_grid_fem_cartesian_nodal_shape` 包含边界节点，输出数据按 x 轴最快排列。[可运行示例](native_continuum_models.aev)

`num_grid_bem_prepare3` 为每个面片边界类型固定的曲面预备稠密内部 Laplace 求解。`num_grid_bem_plan_panel_count` 返回 `num_grid_bem_solve3_prepared` 所需的系数个数。每个系数使用对应面片的势参考量或通量参考量（势除以长度）。`num_grid_bem_eval3` 返回带量纲的内部势。对封闭物体外部的 Laplace 问题，`num_grid_bem_exterior` 求无穷远处衰减的解，`num_grid_bem_eval3_exterior` 返回带量纲的外部势。外域通量按物体外法向计。求值器会检查解的内外域。[可运行示例](native_continuum_models.aev)

`num_analysis_symplectic_integrate_sampled` 推进可分离系统的每个时间步，保存初态、指定步距上的状态和末态。结果包含步数以及各自带量纲的位置和速度。[可运行示例](native_ode_objects.aev)

`num_analysis_physics_explicit_euler` 支持 `Forward`、`Midpoint` 和 `Heun`，接收有符号时间增量，自动缩短最后一步并到达请求时间。它按各分量的物理量纲恢复状态。Forward 每步求值一次，Midpoint 和 Heun 每步求值两次。[可运行示例](native_ode_objects.aev)

`num_grid_sm_advance_advection_diffusion_3d_modes` 使用谱法前向欧拉推进现有傅里叶系数，按 x 方向变化最快的顺序排列。系数采用不归一化的正向 FFT 约定。网格间距、速度、扩散率和时间步长按输入参考量制归一化，结果保留场量参考。[可运行示例](native_continuum_models.aev)

`num_grid_fourier_helmholtz_batch` 复用移位后的傅里叶符号求解多个周期源项。每行一个源项，量纲为场量除以长度平方。移位量的量纲为长度的负二次方。将展平的结果 `data` 乘以 `value_reference` 即可按输入顺序恢复带量纲的场。`num_native_geometry_ricci_curvature` 用一组有限差分采样求出对称度规的 Ricci 张量和 Ricci 标量。两者的量纲分别为坐标量纲的负二次方和该量纲除以度规量纲。[可运行示例](native_continuum_models.aev)

`num_grid_legacy_fdm_solve_wave_dirichlet_sampled` 从带量纲的初始位移与位移速率出发，推进二维或三维齐次 Dirichlet 波动。时间步可以带符号，结果保留初态、指定步距的状态和末态，每行数据保留位移量纲。[可运行示例](native_continuum_models.aev)

`num_analysis_yee_plane_wave_sample` 读取 Yee 求解器的正入射 TF/SF 辅助线。结果分别记录带物理单位的电场、磁场和各自时间。磁场采样位置位于电场采样位置前方半个网格。[可运行示例](yee_plane_wave.aev)

`num_native_fdtd_model_run_final` 推进原有的横磁 FDTD 模型，返回 `(width, height)` 网格上的带量纲电场及经过时间。网格各轴尺寸须为正，可以只有一个单元。长度和波速参考量确定时间步 `L/(2c)`，源项使用零基网格索引和带量纲频率。步数为零时返回零场。`num_native_fdtd_model_run` 则保留第 1、6、11 步等采样结果。[可运行示例](native_continuum_models.aev)

`num_grid_cnm_solve_heat_equation_2d_cn_adi_fixed_boundary` 求解二维热方程并复用每轴的三对角分解。初始场为四条边提供定常边界值。数据、网格间距、热扩散率和时间步均使用网格参考系。[可运行示例](native_continuum_models.aev)

`num_ns_divergence_summary` 返回最大绝对散度、均方根散度、有效流体面积或体积以及有符号的外向通量。切单元的范数按实际流体测度计算。二维通量是面积除以时间，三维通量是体积除以时间。[可运行示例](native_continuum_models.aev)

`num_linear_simplex_map_new` 从归一化顶点和正坐标参考量准备单纯形。`num_linear_simplex_map_volume` 返回无符号的物理测度。`num_linear_integrate_simplex_map` 可将同一映射用于多个已编译标量场，积分值与误差的量纲均为场量纲乘以单纯形测度的量纲。[可运行示例](native_continuum_models.aev)

`num_analysis_coordinates_cartesian_to_spherical` 和 `num_analysis_coordinates_spherical_to_cartesian` 转换固定三元向量。笛卡尔坐标与半径共用同一参考长度，极角从 +z 起算，方位角从 +x 起算，角度均使用弧度。[可运行示例](physical_functions.aev)

`num_analysis_complex_scalar_method_recip` 计算复数倒数，在极端分量下使用带缩放的计算。输出量纲为输入量纲的倒数。[可运行示例](native_kernel_families.aev)

`num_fresnel_sc_interval` 返回两端点之间的菲涅耳正弦与余弦积分，可保留相邻或远场端点之间的小差值，并支持无穷端点。[可运行示例](physical_functions.aev)

`num_special_elliptic_pi_plan_new` 为固定特征数和参数准备第三类椭圆积分。使用 `num_special_elliptic_pi_plan_complete` 读取完全积分，或将弧度振幅传给 `num_special_elliptic_pi_plan_eval` 与 `num_special_elliptic_pi_plan_eval_many`。批量结果按输入顺序返回。[可运行示例](physical_functions.aev)

`num_special_ext_bessel_in_log_abs` 返回整数阶第一类变形 Bessel 函数绝对值的自然对数。在某些较大的有限参数下，即使 `num_special_ext_bessel_in_scaled` 的缩放值下溢，对数仍为有限值。非有限对数会返回定义域错误，例如非零阶函数在零点的对数。[可运行示例](physical_functions.aev)

`num_native_cheb_from_values` 从第二类 Chebyshev 结点上的 1 到 65 个样本构造可移植插值模型，首个样本位于上端点。两个端点共用坐标量纲，样本具有独立的纵坐标量纲。用 `num_native_cheb_eval` 查询。[可运行示例](physical_functions.aev)

`Rssn_numerical_special` 提供 `num_special_ext_carlson_rg`、`num_special_ext_jacobi_amplitude`
和 `num_special_ext_jacobi_elliptic_batch`。批量函数按输入逐行返回，三列为 `sn`、`cn`、`dn`。
已知内部间断点时，可使用 `num_analysis_quadrature_gk_breakpoints_scaled`，传入归一化区间、断点及输入输出的物理参考量。
`num_grid_dimensional_solve_transport_sampled_nd` 推进全部时间步并保存所选快照及端点。
`num_grid_physics_fvm_burgers_nd_with_boundaries` 使用速度参考系演化单元平均值。每轴可选 `Periodic` 或 `Outflow`，时间步须满足算子文档中的逐轴 CFL 条件。全周期网格保持单元平均值之和。[可运行示例](native_flow_conservation.aev)

`num_linear_bsr_matmul_dense` 将块稀疏算子乘以二维数组，数组的每一列是一个右端项。结果行数等于算子行数，列数等于右端项数。参考尺度与量纲分别为两个输入的乘积。[可运行示例](native_linear_tensors.aev)

`num_linear_sparse_native_lu_solve_many`、`num_linear_sparse_native_cholesky_solve_many` 和 `num_linear_sparse_native_solve_auto_many` 只分解一次稀疏矩阵即可求解多列右端项。自动接口返回所用算法和解数组。结果的参考尺度和量纲分别为右端项除以矩阵的对应量。

`num_linear_amg_general_precondition_many` 对每行右端项从零初值执行一次一般 AMG 循环。各行共用同一层次和工作区，输出量纲为右端项除以算子的量纲。`num_linear_amg_general_workspace_len` 返回一次循环所需的临时槽位数量。[可运行示例](native_linear_tensors.aev)

`num_linear_train_inner_workspace_len` 返回两个同形张量列车进行压缩内积所需的临时槽位数量。

`num_linear_sylvester_plan_new` 用两个系数量纲相同的原生方阵准备 `A X + X B = C`。`num_linear_lyapunov_plan_new` 只做一次 Schur 分解来准备 `A X + X A^T = C`。两种计划都可通过 `num_linear_sylvester_plan_solve` 求解多个右端项。解的量纲为右端项量纲除以系数量纲。[可运行示例](native_linear_tensors.aev)

`num_native_ewald_reciprocal_plan_new` 在固定晶胞、屏蔽参数和截断下准备倒空间波矢对。`num_native_ewald_reciprocal_plan_evaluate` 可在同一晶胞和参考系的不同粒子构型之间复用这些波矢，返回能量、逐粒子力和维里张量。[可运行示例](native_particle_engines.aev)

`num_native_sph_density_plan_new_d1`、`num_native_sph_density_plan_new_d2` 和 `num_native_sph_density_plan_new_d3` 根据当前粒子坐标、边界和最大平滑长度准备邻域。`num_native_sph_density_plan_evaluate_d1`、`num_native_sph_density_plan_evaluate_d2` 和 `num_native_sph_density_plan_evaluate_d3` 接收新的质量和平滑长度。结果按原粒子顺序返回，量纲为质量除以长度的空间维数次幂。[可运行示例](native_particle_engines.aev)

`num_analysis_pic_assignment_new` 为固定粒子位置准备周期 PIC 插值权重。`num_analysis_pic_assignment_deposit` 返回每单位长度的网格电荷，`num_analysis_pic_assignment_gather` 在同一粒子位置对网格场插值并保留其量纲。[可运行示例](native_particle_engines.aev)

`num_native_split_linear_plan_new` 为一到三维周期网格上的固定势准备传播计划，每轴点数可以为奇数或偶数。`num_native_split_prepare_linear` 沿用居中求解器的网格和参考量。`num_native_split_linear_plan_size` 返回展平后的网格点数。`num_native_split_linear_plan_evolve` 对多组波函数复用预计算相位并保留振幅量纲。负时间步沿反方向传播。[可运行示例](native_particle_engines.aev)

`num_functional_l2_grid_new` 为带量纲的坐标轴准备梯形权重。`num_functional_l2_grid_norm` 返回范数，量纲为采样值量纲乘以坐标轴量纲的平方根。`num_functional_l2_grid_norm_and_normalize` 一次归约同时求得范数与归一化采样，后者的量纲为坐标轴量纲的负二分之一次幂。测度为零时无归一化结果，范数溢出时无可表示的标量结果。[可运行示例](physical_functions.aev)

`num_analysis_stochastic_jump_sample_batch` 从同一随机流连续采样跳跃标记。行按抽取顺序排列，各列保留标记量纲。空批次和失败的批次均保留随机流状态。[可运行示例](sde_adaptive_jump.aev)

`num_analysis_sobol_point_at` 按无符号 32 位索引取点而不推进序列。`num_analysis_sobol_seek` 改变生成器的位置，下一次抽取前进一步。最大索引之后回到原点。[可运行示例](native_discrete_objects.aev)


`Rssn_numerical_callback` 提供带量纲的 Wirtinger 导数、柯西–黎曼与保角诊断、
笛卡尔 Laplacian、局部倍率与旋转、极坐标残差及复步长导数。极坐标诊断分别接受
径向步长和无量纲角步长。一阶笛卡尔诊断复用同一组四点采样，实解析复步长导数
只需一次采样。步长无法在浮点中区分或回调预算不足时返回错误。
[可运行测试](../tests/domain_conformal_stencil.aev)。

`Rssn_Univalent` 提供归一化单位圆盘上的系数与导数诊断。Taylor 系数按升幂
从常数项开始传入，Grunsky 矩阵和测试向量使用复数张量。Bieberbach、Grunsky
与 Becker 计算有限数值条件。Loewner 使用固定弧度驱动角推进一个径向步长。
[可运行测试](../tests/domain_univalent.aev)。

类型化数值回调还可用单个 64 点圆估计 Schwarz 导数，并在有限网格上检查
Nehari 条件、星形性和凸性。Schwarz 导数的量纲为坐标量纲平方的倒数。
这些采样诊断要求显式回调预算，并可拒绝浮点分辨率不足的输入。
[可运行测试](../tests/domain_univalent_sampled.aev)。

`Rssn_Special` 的保角算子可根据三对点拟合齐次 Möbius 变换、用补零后的
多项式系数行计算 Bergman 核，以及在上半平面内部评估给定的 Schwarz–Christoffel
映射。积分返回是否达到指定容差、绝对误差估计及实际采样次数。
[可运行测试](../tests/domain_conformal_kernels.aev)。

Szegő 求解可预先计算并复用边界点、Cauchy 权重和中心，供 Szegő 核、
Bergman 核与 Riemann 映射多次求值。检查式映射要求从中心到查询点的直线
位于采样边界内部。两个回调入口均显式限制采样点数和回调次数。
[可运行测试](../tests/domain_conformal_szego.aev)。

Riemann 覆盖算子返回分支循环的紧凑表示和零基片层置换。无穷远处的局部
环路按 `1/z` 坐标定向。数值 monodromy 的系数矩阵按行索引 `w` 的幂、
按列索引 `z` 的幂，并自动连接路径尾点和起点。接触无法分辨的分支点时返回错误。
带量纲的割线跳跃回调分别接受法线角度和坐标步长。
[可运行测试](../tests/domain_riemann_covering.aev)。

若要固定圆周采样，可使用 `num_contour_residue_infinity_fixed_typed`；
回调预算需覆盖至少 32 个节点。自适应轮廓积分、主值和留数还返回绝对误差估计
及收敛状态。实部和虚部复用同一批复数回调采样。
[可运行测试](../tests/domain_contour_compatibility.aev)。

`Rssn_Contour` 构造带有统一坐标量纲的开路径和闭路径。`starts` 和 `ends`
保存各段的起点和终点。`arc_segments` 是圆弧段的零基索引，依次对应 `arc_centers`、
`arc_radii`、`theta_start` 和 `theta_end`。角度用无量纲弧度表示，扫角的符号决定
方向。`bromwich` 和 `hankel` 的 `closed` 为 `false`。`indented_semicircle`
先在单位半径坐标系内为每个指定极点构造缩进，再恢复原坐标量纲。


`Rssn_Contour.deform` 在两个带量纲的复数端点之间构造开路径。输入包括极点向量、
割线起点向量、正的极点避让距离，以及逐一对应的无量纲弧度角。极点与割线合计最多
64 个。路径可以沿同一分支绕过割线起点。构造器无法验证避让距离或路径与割线不相交时
会返回错误。可复用返回的路径，也可调用
`Rssn_numerical_callback.num_contour_integrate_deformed_checked_typed`
一次完成路径构造与积分。[可运行测试](../tests/domain_contour_deformation.aev)。

`Rssn_numerical_callback.num_contour_integrate_checked_typed` 接收路径、坐标与函数值的参考尺度、
绝对容差、每个分量的求积区间预算以及回调总预算。最后一个参数是返回复数的回调。
结果包含 `value`、估计绝对误差 `error`、求值次数 `evaluations` 和收敛标志 `converged`。
区间预算耗尽时，有限估计可带 `converged = false` 返回。回调失败、样本非有限或
回调次数预算耗尽时返回错误。

`num_analysis_levin_plan_new` 为固定相位、相位导数和频率准备结点与权重。`num_analysis_levin_plan_nodes` 返回带量纲结点。可用 `num_analysis_levin_plan_integrate_samples` 传入同序振幅样本，或用 `num_analysis_levin_plan_integrate` 传入已编译振幅。复积分量纲为振幅量纲与坐标量纲的乘积。[可运行示例](physical_functions.aev)

`Rssn_Special.complex_jacobi_p` 和其他 `complex_*` 正交多项式入口接收无量纲参数与坐标，次数为 0 到 4096 的整数。`complex_spherical_harmonic` 还接收整数阶数，余纬和方位角用弧度表示。这组入口通过 `Result<Complex, OpError>` 返回结果。[可运行示例](complex_special_functions.aev)

同一模块的 Weierstrass σ 和 ζ 接收两个无量纲复半周期。
σ 在格点为零，ζ 在格点返回极点错误。实现缓存约化格的 θ 系数，并在格胞外恢复准周期修正。

`Rssn_Special` 提供复阶主值 Bessel `J` 和 `I`、实阶 Anger `J` 和
Weber `E`，以及以 `ber`、`bei`、`ker`、`kei` 命名字段的 Kelvin 函数。
这些接口的参数均为无量纲。指数积分 `E_n` 和 Legendre 主值 `Q_n` 接收
0 到 4096 的整数。虚部的正负零决定割线的边界取值。奇点和非有限结果返回错误。

`Rssn_numerical_callback` 中带类型的复数求根回调可以分别指定坐标和残差参考量。
`num_complex_root_muller_typed` 需要三个初值。Halley 接口接收一个初值，以及
残差和对坐标的一阶、二阶导数。Homotopy 接口使用对坐标和时间的导数，将已知的
根从无量纲时间 0 跟踪至 1。成功时返回原坐标量纲的根和实际回调求值次数。

`num_complex_root_delves_lyness_typed` 通过对函数和一阶导数采样，估计无极点
函数在圆内的零点。半径用作坐标参考量，所得根保留圆的量纲。需要经过认证的
包络时可使用整个圆盘的认证算子。

`Rssn_numerical_callback` 中的 `num_saddle_*_typed` 算子共用带量纲的鞍点记录。
`num_saddle_find_typed` 接收复数初值、坐标参考量、归一化斜率容差和回调预算。
相位无量纲，一阶和二阶导数的量纲分别为坐标的一次和二次倒数。首项近似、二阶修正
和路径积分的结果量纲均为振幅乘坐标。二阶修正要求相位和振幅在坐标参考量十分之一
为半径的圆盘内解析。路径积分接收带量纲的半程长度和以结果量纲表示的绝对容差。

`Rssn_ComplexDisk` 用 `[实部, 虚部, 半径]` 表示复平面上的闭圆盘。坐标和半径
使用同一物理量纲。`make` 接收复数圆心和非负半径。`point` 接收实部和虚部，
构造零半径的点圆盘。`contains` 将无法确定的边界点视为可能属于圆盘，
`inside_strictly` 验证严格包含。`mul` 和 `divide` 按乘除法组合量纲。
无法证实分母圆盘与零分离时，`divide` 返回 `Ok(None)`。

`variable` 和 `constant` 可构造无量纲一阶自动微分对象。`value` 和 `derivative`
分别保存函数值和导数的包络圆盘。`jet_*` 系列函数在复合运算时传播一阶导数。
`matvec`、`matmul` 和 `solve` 使用最后一轴保存圆盘坐标。`solve` 在无法认证解的包络时
返回 `Ok(None)`。

初等 Jet 函数同时包络主值和一阶导数。例如 `jet_acosh` 的导数分别使用 `z - 1`
和 `z + 1` 的主值平方根。若输入圆盘碰到分支割线或无法认证包络，返回可选结果的
Jet 接口返回 `Ok(None)`。`sec`、`csc`、`cot`、`sech`、`csch` 和 `coth` 也在无法
证实分母与零分离时返回 `Ok(None)`。初等指数运算采用有界的参数缩减，输入圆盘
的模上界需不超过 `2^20` 才能给出有限包络。

`erf`、`gamma`、`log_gamma` 和 `bessel_j` 包络整个无量纲圆盘。遇到极点、
分支割线或超出有限计算范围时，可能返回 `Ok(None)`。在 `Rssn_numerical_callback`
中使用 `num_complex_disk_certify_*_typed` 进行认证时，回调须包络整个圆盘或复数盒。
标量 Krawczyk 检验需要两次回调，系统检验需要三次，每段圆弧的 Rouché 检验需要两次。

`num_native_ball_values_and_jacobian_at_terms` 在同一点联合计算多项式的值及 Jacobian。`num_native_ball_eval_value_and_jacobian_terms` 在复数盒上返回两者的包围。值保留残差量纲，Jacobian 条目量纲为残差除以坐标量纲。[可运行示例](native_discrete_objects.aev)

`num_analysis_series_sum_terms` 接收带量纲的级数前缀和有限项数预算。估计和启发式绝对误差与输入项量纲一致，`terms` 是实际消耗的项数。无法给出估计时返回 `None`。[可运行示例](physical_functions.aev)

`num_linear_csc_transpose_matvec` 直接从 CSC 列计算转置矩阵与向量的乘积。`num_linear_csc_matvec_into` 和 `num_linear_csc_transpose_matvec_into` 复用目标向量及其物理参考量。返回错误时目标向量保持原值。[可运行示例](native_linear_tensors.aev)

`num_grid_fem_residual_estimator_new` 为固定单纯形空间保存几何数据。常系数使用 `num_grid_fem_residual_estimator_indicators`，变系数使用 `num_grid_fem_residual_estimator_indicators_var`。结果包含归一化单元指标及物理参考量，量纲为场量纲乘以长度的 `(维数 + 2)/2` 次幂，再除以时间。[可运行示例](native_continuum_models.aev)

`num_linear_csc_norms` 一次计算 Frobenius 范数、诱导 1-范数和诱导无穷范数，各字段保留矩阵元素量纲。[可运行示例](native_linear_tensors.aev)

`num_stats_kruskal_wallis` 比较样本数可以不同且量纲相同的多个组。输入展平样本及零基 CSR 偏移。秩统计量、卡方右尾概率和自由度均无量纲。[可运行示例](physical_functions.aev)

`num_combinatorics_bell_batch` 按输入顺序返回各索引的 Bell 数。可用 `Float` 表示时为 `Some`，溢出时为 `None`。`num_number_theory_prime_sqrt_plan_new` 检查素数模数并准备平方根方法。`num_number_theory_prime_sqrt_plan_modulus` 返回模数。`num_number_theory_prime_sqrt_plan_sqrt` 返回较小平方根，对二次非剩余返回 `None`。[可运行示例](native_discrete_objects.aev)

`num_native_multivector_mul_central` 将 Cl(3,0) 多向量乘以标量和赝标量组成的中心因子。因子的两个系数共用量纲，各输出系数的量纲为两个输入量纲的乘积。[可运行示例](native_discrete_objects.aev)

`num_brent_with_bracket` 返回根、升序终区间和实际回调次数。根、区间和绝对容差共用坐标量纲，回调残差可以使用独立量纲。[可运行示例](native_ode_objects.aev)

`num_native_multilinear_grid_new` 为重复查询保存各轴长度可以不同、量纲独立的直线网格。采样值按第一轴变化最快的顺序展平。`num_native_multilinear_grid_eval` 接收各轴的坐标，返回保留所存采样值量纲的结果。[可运行示例](physical_functions.aev)

`num_robust_huber_location_with_scale` 使用正的尺度和无量纲调节常数拟合 Huber 位置。尺度与结果均保留样本量纲。[可运行示例](physical_functions.aev)

`num_dual_powi_with_second_derivative` 对归一化整数幂在 `t = 0` 沿 `val + t * deriv` 返回函数值、一阶导与二阶方向导。[可运行示例](native_autodiff.aev)

`num_native_hessian_diagonal_scaled` 计算标量回调沿各坐标的二阶导数。`num_native_hessian_diagonal_with_steps_scaled` 接收各坐标正的带量纲步长。结果为输出单位除以坐标单位的平方。[可运行示例](native_autodiff.aev)

`num_analysis_newton_cg_tape_plan_new` 保存计算带的快照并选定目标输出。给 `num_analysis_newton_cg_tape_plan_minimize` 传入新的初始点、参数和选项，即可复用求解缓冲区。结果保留各坐标与目标函数的量纲，并报告求值次数。[可运行示例](native_autodiff.aev)

`num_linear_tensordot` 对两个原生数组的指定轴对做缩并，保留其余轴的顺序，物理参考量相乘。[可运行示例](native_linear_tensors.aev)

`num_native_lie_from_matrices` 从对交换子封闭的精确方阵基构造李代数。`num_native_lie_bracket` 返回原基下的精确系数。[可运行示例](native_discrete_objects.aev)

`num_native_tape_column_gather` 准备用于逐行选择来源列的计算带。调用 `num_native_tape_eval_columns` 时，第一个输入列是无量纲的零基偏移量，按向零方向截断，之后是带量纲的来源列。各来源列与结果使用计划声明的同一个物理量纲。[可运行示例](native_execution.aev)

`num_analysis_polynomial_antidifference_new` 为带量纲原点和正步长编译升次幂系数。`num_analysis_polynomial_antidifference_eval` 计算给定坐标的原差分函数值。`num_analysis_polynomial_antidifference_difference` 直接使用区间宽度计算邻近点的差值，并保留系数量纲。[可运行示例](physical_functions.aev)

`num_native_sum_indefinite_sum` 保留被求和项的量纲。`num_native_sum_indefinite_product` 将各正项除以显式参考量再取乘积。两者都接收带量纲的起点与步长，并在采样前检查格点所需的求值次数是否超出预算。[可运行示例](physical_functions.aev)

`num_linear_randomized_svd` 通过带种子的随机投影近似主要奇异三元组。将归一化的 `s` 中各项乘以 `singular_value_reference`，即可得到矩阵物理单位下的奇异值。[可运行示例](native_linear_tensors.aev)

`num_linear_geometric_level_size` 返回零基层号对应的内部网格点数。`num_linear_geometric_restrict` 和 `num_linear_geometric_prolong` 都接收细层的层号。限制算子使用全权重得到相邻粗网格，延拓算子在齐次 Dirichlet 边界下插值回细网格。两者保留场的量纲和参考尺度。[可运行示例](native_linear_tensors.aev)

当 `a`、`b` 为正实数且 `c > a + b` 时，`num_analysis_hypergeometric_hyp2f1_cplx` 可计算 Gauss 超几何函数在 `z = 1` 的值。端点算法避免构造可能溢出的 Gamma 因子。[可运行示例](physical_functions.aev) · [NIST 公式](https://dlmf.nist.gov/15.4.E20)

`num_special_ln_bessel_k` 接收无量纲实阶和正的无量纲自变量，返回第二类修正 Bessel 函数的自然对数。即使原值太小而无法用 `num_special_bessel_k` 表示，对数仍可求得。[可运行示例](physical_functions.aev)

`num_analysis_meijer_meijer_g_real` 计算无量纲实值 Meijer G 函数。当阶数组合为 `(1, 0)`、上参数列表为空、下参数只有 `b` 时，非负实轴上的算法合并计算 `z^b exp(-z)` 的幂和指数因子，并保留可表示的次正规结果。[可运行示例](physical_functions.aev)

`num_special_exact_chebyshev_tu_mod` 返回 `T_n(x)` 与 `U_n(x)` 的余数，分别为 `item0` 和 `item1`。阶数和模数支持无符号 64 位整数的完整范围，`x` 为有符号 64 位整数。模数为零时返回 `None`。[可运行示例](native_kernel_families.aev)

`num_analysis_holonomic_expression_series` 按升次幂返回 `holonomic` 表达式的精确系数，最多返回指定的截断项数。整数次幂使用截断递推。级数未定义时返回 `None`，请求超过原生项数上限时报告错误。[可运行示例](native_kernel_families.aev)

`num_topology_persistent_homology` 接收带量纲的过滤值，以及用零基偏移和顶点索引表示的变长单形。重复单形在最早的过滤值进入。结果保留过滤量纲，并用布尔掩码标记永续区间。[可运行示例](native_kernel_families.aev)

`num_complex_winding_number` 计算同一物理量纲的复数多边形绕原点的圈数，自动连接首尾顶点。逆时针计正数，经过原点的路径会报告定义域错误。[可运行示例](native_kernel_families.aev)

`num_norm` 和 `num_distance` 保留坐标量纲，并在需要时缩放中间平方项。`num_normalize` 对有限非零向量返回无量纲方向，即使原向量的范数超过 Float 范围。[可运行示例](native_kernel_families.aev)

`num_reflect` 接收无量纲的单位法线，保留输入向量的量纲。即使中间校正项溢出，仍可得到可表示的反射分量。[可运行示例](native_kernel_families.aev)

`num_linear_arnoldi` 返回无量纲 Krylov 基、归一化 Hessenberg 矩阵和残差 `beta`。将 `hessenberg` 和 `beta` 乘以 `operator_reference` 可恢复算子量纲。[可运行示例](native_linear_tensors.aev)

`num_native_qs_factor_pair` 接收无量纲整数和二次筛限制句柄，按升序返回一对经过验证的非平凡因子。若在给定限制内找不到因子对，返回 `None`。[可运行示例](native_discrete_objects.aev)

`num_native_number_exact_falling_factorial` 计算从 `n` 个元素中选取 `k` 个不同元素的有序选法数，返回精确整数。不可能的选择返回零，有效的空选择返回一。[可运行示例](native_kernel_families.aev)

`num_permgroup_power_exact` 计算零基置换的有符号精确整数次幂。负指数作用于逆置换。原生算子检查所有像是否构成双射。[可运行示例](native_kernel_families.aev)

`num_analysis_regression_value_and_gradient` 用一次残差计算求得已保留线性回归的损失和梯度。预测量纲为 D、响应量纲为 E 时，损失量纲为 E²，截距导数为 E，各斜率导数为 E·D。梯度沿用系数顺序。[可运行示例](native_algebra_models.aev)

`num_homology_chain_betti_mod_p` 返回已保留链复形各次数的 Betti 数，系数域由精确的 64 位无符号素数指定。实射影平面的示例展示了特征二和奇特征下结果的差异。[可运行示例](native_kernel_families.aev)

`num_inverse_erfc` 接收从零到二的 Float 概率。对数细化使极小概率一直到最小正 Float 的结果保持精度。`num_analysis_special_complex_lambert_w_complex` 接收整数分支编号，并区分负实轴割线的两侧。[可运行示例](native_kernel_families.aev)

`num_native_rs_encoder_new` 预备可复用的 Reed–Solomon 编码器。`num_native_rs_encoder_encode` 返回原字节和随后附上的校验字节，`num_native_rs_encoder_parity_symbols` 读取保存的校验长度。整个码字不超过 255 字节。[可运行示例](native_kernel_families.aev)

`num_native_affine_ifs_new` 使用非负相对权重预备无量纲仿射映射。`num_native_affine_ifs_generate` 复用该句柄，并接收起点、保留点数、预热步数和精确的 64 位无符号种子。结果为两列坐标，不含起点。[可运行示例](native_kernel_families.aev)

`num_analysis_complex_scalar_method_sqrt` 返回复数主平方根，保留实轴附近可表示的微小分量。结果量纲为输入量纲的平方根。[可运行示例](native_kernel_families.aev)

`num_native_graphics_prepared_triangle_new` 使用与三个顶点坐标同量纲的正参考量预备三角形。`num_native_graphics_prepared_triangle_intersect` 检查单条射线，`num_native_graphics_prepared_triangle_intersect_parallel` 用共同的无量纲方向检查至多 4,096 个起点。省略上界时射线区间向上无界。批量结果以 `hit_mask` 标记命中行，命中的射线参数与交点保留坐标量纲，法线无量纲。[可运行示例](native_discrete_objects.aev)

`num_native_dd_hypot` 接收两个量纲相同的双双精度句柄并求模长，`num_native_cdd_norm_dd` 接收复双双精度句柄并返回双双精度模长。对两种结果都可调用 `num_native_dd_parts`，通过 `item0` 和 `item1` 分别读取带量纲的高、低分量。`num_native_cdd_recip` 缩放极端分量，返回量纲为原量纲倒数的复双双精度句柄。[可运行示例](native_discrete_objects.aev)

`num_distributions_weibull_quantiles` 按输入顺序计算下尾分位，`num_distributions_weibull_inverse_survival` 直接接收上尾概率。结果保留尺度量纲。分位为无穷大时，原生 `Float` 接口报告定义域错误。[可运行示例](physical_functions.aev)

`num_native_overlap_save_plan_new` 保存带量纲滤波器的频谱。单个录音使用 `num_native_overlap_save_plan_apply`，批量使用 `num_native_overlap_save_plan_apply_batch` 并传入展平采样值及零基行偏移。重复偏移表示空行，结果带有自己的行偏移。结果量纲为采样和滤波系数的量纲乘积。[可运行示例](physical_functions.aev)

`num_interp_ext_pchip_many` 和 `num_interp_ext_akima_many` 接收查询坐标张量。查询坐标与插值结点量纲相同，结果保留纵坐标的量纲及查询顺序。在线协方差累加器也可通过 `num_native_online_covariance_push_frequency_batch` 接收每行一个无量纲、非负整数频数。总计数是各行频数之和，等价于按频数重复观测行。

`num_analysis_sde_drift_implicit_sampled` 使用给定的布朗增量推进全部时间步，保留初态、指定步距上的状态和终态。状态量纲及回调求值预算沿用完整路径的约定。

`num_number_ext_mobius_sieve` 计算从零开始的一段连续整数的 Möbius 函数值。`num_number_ext_solve_linear_congruence` 返回最小非负解及其周期，无解时返回 `None`。

同一个数值张量只有一个元素量纲。混合位置、速度等数据应使用记录、元组，
或包含独立量纲见证的 `Tensor<1, Value>`。原生表达式带为每个坐标、参数和
输出分别声明正参考量；梯度分量恢复输出/相应输入量纲，Hessian 的 `(i, j)`
项恢复输出/(输入_i × 输入_j) 量纲。
`num_polyfit_normalized` 按 `x/coordinate_scale` 的幂拟合多项式，每个系数均保留纵坐标量纲。
用 `num_multivariate_normal_log_pdf_scaled` 计算多元正态对数密度时，需要显式提供参考测度。物理密度的量纲是坐标量纲的负向量维数次幂。

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
默认构建包含 Cranelift、wgpu 和 NumPy 文件支持。用标准 CLI 运行原生执行示例：

```sh
bazel run //:aevia -- test examples/native_execution.aev
```

[数值变换用例](../tests/domain_numerical_transforms.aev) 演示了
`Rssn_numerical_callback` 中带误差及调用预算的 Mellin、Hilbert 和
逆 Laplace 回调，以及 `Rssn_NumericalTransforms` 中的 Borel 多项式和
Meijer G。坐标须无量纲，绝对容差采用回调结果的单位。有限积分的误差估计
不包含截掉的尾部。逆 Laplace 通过比较两种分辨率估计误差。

## 对带量纲的平面区域进行三角剖分

`Rssn_Meshgen.advancing_front` 按目标边长填充简单多边形。
`Rssn_Meshgen.constrained_delaunay` 将从零开始的线段端点对保留为三角网中的边链。
约束线段相交时可能插入顶点。两者都接收与坐标范围接近的正参考长度和显式的输出顶点
预算，预算允许 3 至 4096 个顶点。返回值包含带量纲的成对坐标和从零开始的三角形三元组。
[平面网格示例和检查](../tests/domain_planar_meshgen.aev)

## 操作带量纲的 CAD 网格

`Rssn_Cad.mesh_new` 接受三列长度坐标和三列从零开始的三角形索引。闭合且面朝向一致时，
`signed_volume` 返回带量纲的体积。读写内存中的 STL 时需提供坐标单位的长度，检查相邻
关系前先焊接网格。[CAD 网格示例和检查](../tests/domain_cad_mesh.aev)
`read_step_text` 读取由直线围成的平面 STEP 实体，按文本声明的单位为每个实体返回一个
带量纲的网格句柄。在 Aevia 中，先以文件读取权限调用 `read_file(path)`，再将返回
的文本传给 `Rssn_Cad.read_step_text`。语言绑定可以使用宿主的文件 API 读取文本，
再调用 `num_cad_step_read_text`。

## 复用带量纲的插值网格与积分函数

`Rssn_Interpolation.prepare_bilinear(xs, ys, cells)` 保留直角网格。两个坐标轴须严格
递增，值张量的形状为 `[len(ys), len(xs)]`。两个轴和值可以使用不同的量纲。
`interpolate(grid, x, y, cell_unit)` 将越界坐标限制在网格边界，返回与网格值
同量纲的结果。后续查询可以复用句柄。[带量纲的网格示例](../tests/domain_interpolation.aev)

`Rssn_ParallelQuadrature.gauss_legendre` 和 `clenshaw_curtis` 在带量纲的区间上对已编译
的标量函数积分。参数中的被积函数单位用于校验量纲，结果的量纲是坐标与函数值的乘积。
编译时设置的工作预算须覆盖积分采样。`sum_compensated` 保留输入向量的量纲并使用
确定性的补偿求和。[固定阶积分示例](../tests/domain_parallel_quadrature.aev)

运行 `aevia devices` 查看可用的 RSSN 后端选择器。[带量纲的编译示例](native_backend.aev)
使用当前选择。可加上 `--rssn-backend cpu` 或 `--rssn-backend cranelift` 来比较执行结果。

会话对象每次调用都检查种类、所属会话及存活状态。`num_native_release`
成功后使句柄的所有副本失效；存在存活依赖时，父对象的释放请求会被拒绝，
应先释放依赖对象。临时反向微分回调句柄在回调结束后失效。编译回调通过父对象
依赖或独立拥有的快照保留执行代码；快照保留的存储计入预算。可移植模型则保存版本化定义数据，查询
时验证并重建原生模型，具有不同的生命周期和重建开销。

自适应求解器显式提供容差、步长约束与有限工作预算。原生旧接口没有的控制项
会被拒绝。时间容差不消除空间离散误差；变步长通常不保持固定步长方法的辛性。
随机拒绝步重用有界的耦合噪声离散；分辨率或预算不足时返回错误，不重抽噪声
来掩盖拒绝。Lévy 方法仅在 alpha=2 时支持布朗高阶修正。

## 分析预计算传递函数

`Rssn_Control` 接受按降幂排列的分子与分母系数，使用统一归一化尺度下的时间和频率。
同一个模型可用于频率求值、连接、阶跃响应与裕量分析。阶跃响应要求分子次数不超过分母次数。

```aevia
import Rssn_Control as Control

test "稳定的一阶传递函数" =
    match Control.transfer_new_proper([2.0], [1.0, 1.0]) with
    | Ok(model) -> Control.is_stable(model) == Ok(true) && Control.release_transfer(model) == Ok(true)
    | Err(_) -> false
```

[传递函数示例和检查](../tests/domain_control_transfer.aev)

`Rssn_Control.simulate_pid` 接受归一化误差、正采样间隔和可选限幅，`pid_response` 使用已经
准备好的传递函数。`identify_era` 根据输入向量与逐采样、逐输出排列的张量辨识离散模型。
可用 `era_matrices` 检查矩阵，或用 `impulse_response` 计算脉冲响应。[PID 与 ERA 测试](../tests/domain_control_trajectories.aev)

`Rssn_Control.prepare_mpc` 创建离散预测控制计划。`mpc_closed_loop` 返回初态及后续控制状态。
同一个计划可复用于多次轨迹计算，使用结束后调用 `release_mpc`。[MPC 示例和检查](../tests/domain_control_mpc.aev)

### 预备目标函数与标量求根

`Rssn_Optimizer` 可将内置函数、归一化计算带或回归模型预备为可重复使用的
目标函数。计算带采用单位尺度参考量并指定输出槽。使用 `prepare_checked`
时，非有限求值返回错误。预备目标可保留盒约束，也可复用目标函数并设置新的边界。
`minimize` 接受求解器名称、初始点、迭代与求值限额以及显式种子，返回最佳点、
目标值、终止原因和求值次数。

`regression_problem` 使用模型保存的归一化参考量。将带量纲系数和响应参考量
传给 `residuals`，即可得到以一致单位表示的拟合值减观测值。
`Rssn_numerical_vector` 中带类型的求根入口接收标量回调、有序区间、正容差和
求值限额。Newton 与 Halley 可使用显式导数。
参见[预备优化示例](../tests/domain_optimizer.aev)。

源码覆盖表位于 `tools/rssn-api-audit/inventory.tsv`，语言可达性报告位于
`tools/rssn-api-audit/RSSN-COVERAGE.md`。`object_binding!` 从类型声明生成参数
个数、模式、编解码和注册信息；简单函数与记录使用经审阅的量纲策略生成绑定。
`bazel test //:rssn_api_audit_test` 用一次源码扫描检查快照和所有清点出的可调用
接口是否有绑定；只有源码解析证明是同一个 Rust 项目时，重导出才继承覆盖。
宏及未解析重导出的指纹仍单独保留，不能据此宣称运行时可调用。

使用 `bazel run //:refresh_kernel` 更新契约、绑定及清单。验证时优先运行对应的原生
契约测试与 AST/IR 示例目标；无需为每个适配器反复运行整个工作区测试。

## 复用精确的模算术计划

`Rssn_Modular` 为无符号整数提供有类型的预计算计划。模数可能是偶数时可以使用 Barrett，
在奇数模数下可使用 Montgomery 连续运算编码后的剩余类。输入和输出都是精确整数。
计划使用结束后，可调用 `release_barrett` 或 `release_montgomery` 释放会话资源。

```aevia
import Rssn_Modular as Mod

test "合数模数下的乘法" =
    match Mod.barrett_new(12) with
    | Ok(plan) ->
        let product = Mod.barrett_mul(plan, 7, 7)
        product == Ok(1) && Mod.release_barrett(plan) == Ok(true)
    | Err(_) -> false
```

[模算术测试](../tests/domain_modular.aev)覆盖 128 位约减、已释放的句柄和超出范围的编码输入。

### 马尔可夫链蒙特卡洛

`Rssn_Mcmc` 提供显式设种的 PCG32 流和收敛诊断。将带量纲的单条链传给
`autocorrelation`、`effective_sample_size` 或 `geweke`。调用 `split_rhat`
时，每条链占张量的一行，所有行采用同一物理量纲。诊断结果均无量纲。
用完确定性随机流后，可调用 `Rssn_Mcmc.release` 释放。

采样入口位于 `Rssn_numerical_stats` 的 `num_mcmc_*_typed` 函数中，输入包括
归一化坐标、种子，以及限定样本数、预热步数和抽稀间隔的配置。Metropolis–Hastings
接收对数目标密度，Hamiltonian 蒙特卡洛还需要其梯度。Gibbs 的条件采样回调
接收从零开始的坐标编号、当前状态和一个均匀抽样值。采样结果按行存放，并附带
提议与接受次数。未提供联合对数目标密度时，Gibbs 返回空的对数密度张量。
参见[确定性回归示例](../tests/domain_mcmc.aev)。

Rssn_Statrs.prepare 接收分布名称和对应的参数个数。密度、矩和尾概率可复用同一会话句柄。sf 直接计算上尾概率。用完后可通过 release_distribution 释放。


## 精确稀疏矩阵与预备素域

`Rssn_SparseExact.field` 验证无符号 64 位素数并创建会话内的素域。
后续算术、约化和批量求逆可直接复用该素域。分母可被素数整除时，
`reduce_matrix` 返回 `None`。批量求逆遇到零剩余时也返回 `None`。
`reducer` 返回可独立使用的 `Rssn_Modular` Barrett 句柄。

`rational_rref` 与 `prime_rref` 返回约化稀疏矩阵以及从零开始编号的主元列。
`rcm_order` 返回方阵稀疏结构缓存排序的正反两个方向。
调用 `solve_dixon_multi` 时，每一行放置一组右端项。返回值保留同一行约定
和精确有理数系数。奇异系统返回 `None`。
参见[精确稀疏示例](../tests/domain_sparse_exact.aev)。

`Rssn_Special.complex_carlson_*` 提供五个无量纲复数算子。
`complex_carlson_rj(x, y, z, p)` 接受实部非负的 `x`、`y`、`z` 和实部为正的
`p`。`p` 与前三个参数之一取同一分支且相等时，可按相应的 `R_D` 求值。
奇异参数返回 `Err`。`R_G(x, 0, 0)` 等于 `x` 的主平方根的一半。

`Rssn_Special.to_polar` 和 `from_polar` 支持带量纲的复坐标。半径保留坐标量纲，
主辐角使用弧度。`complex_abs_squared` 返回平方量纲。`unwrap_phase` 接受避开零点
的带量纲采样。对无量纲路径，`continue_complex_root` 可沿路径追踪选定的根分支。
相邻采样点间的真实辐角变化需小于 π。

`Rssn_Special.barycentric_interpolate` 接受带量纲的横坐标和纵坐标。
在同一个小型网格上重复查询时会复用节点权重。无量纲多项式按降幂存放系数，
`polynomial_power` 接受整数指数，`polynomial_gcd` 接受相对系数容差。

`Rssn_Statistics.fit_glm` 接受无量纲样本、分布族、链接函数、可选的观测权重，
以及明确的迭代次数和容差。它返回具名诊断结果，并在模型中保留所用链接。
`predict_glm(model, rows)` 复用该链接预测响应均值。传入的设计矩阵无需截距列。
