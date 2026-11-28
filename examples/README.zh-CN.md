# Aevia 示例

[English](README.md)

在仓库根目录使用 `aevia run examples/FILE.aev` 运行完整程序。包含断言的示例用 `aevia test examples/FILE.aev` 验证。

| 示例 | 学习内容 |
|---|---|
| [运动学](kinematics.aev)与[单位](units.aev) | 精确与近似量、单位转换、仿射温度 |
| [张量](tensors.aev)与[稀疏张量](sparse.aev) | 矩阵乘积、归约、存储格式 |
| [矩阵运算](matrix_operations.aev) | 稠密矩阵构造（`fill`、`linspace`、`eye`、`diag`）、`trace`、`tril`/`triu` 与 `set` |
| [三对角弹簧链](tridiagonal_spring_chain.aev) | 只用对角线求解带量纲方程组 |
| [微积分](calculus.aev)与[符号表达式](symbolic.aev) | 求导、积分、极限、解方程 |
| [模块](modules/main.aev) | 两个文件之间的导入 |
| [传感器校准](sensor_calibration.aev)与[弛豫拟合](relaxation_fit.aev) | 带量纲残差、质量阈值和模型拟合 |
| [绘制传感器测量结果](plot_series.aev) | 共用坐标轴、误差条、直方图和 SVG、HTML、GIF 动画导出 |
| [仿真 IO 与可视化](sim_io_visualization.aev) | 阻尼振子 CSV/NPY 导出与 SVG、HTML、GIF 动画可视化 |
| [并行计算](parallel.aev) | 保序映射、归约、并行任务 |
| [频谱分析](frequency_analysis.aev) | 将零频移至中心，并从半谱重建奇数长度的带量纲信号 |
| [Kalman 跟踪](kalman_tracking.aev) | 滤波和平滑带量纲观测、还原差分信号并计算滑动统计量 |
| [带量纲状态空间仿真](measured_state_space.aev) | 离散化带量纲模型并仿真连续和离散响应 |
| [多项式根界](polynomial_root_bounds.aev) | 计算多项式根的圆盘或圆环界，以及单位圆盘内的 Koebe 增长界 |
| [零相位滤波](zero_phase_filter.aev) | 平滑带量纲的压力脉冲，保留采样位置和单位 |
| [带量纲向量更新](measured_vector_updates.aev) | 用速度和时间向量批量更新位置 |
| [带量纲网格](measured_mesh.aev) | 用带量纲坐标三角剖分，计算面积和体积，提取四面体边界面 |
| [组合与概率](combinatorial_probabilities.aev) | 用精确整数计数计算对数二项式系数、多项式系数和 Beta 分位数 |
| [模多项式乘法](modular_polynomials.aev) | 计算系数卷积，查看精确原根和 Bézout 系数 |
| [精确二进制多精度运算](dyadic_real.aev) | 以明确的精度和舍入规则比较与计算多精度值 |
| [精确模向量](exact_modular_vectors.aev) | 使用全宽余数、模点积、批量逆元和宽整数模幂 |
| [宽整数同余方程](wide_congruences.aev) | 求解周期超过 128 位的广义中国剩余定理问题 |
| [精确矩阵标准形](exact_matrix_normal_form.aev) | 对带量纲系数做归一化并求出 Smith 标准形的两个变换矩阵 |
| [多项式校准](polynomial_calibration.aev) | 拟合带量纲轨迹，复合时钟校准，再批量求值 |
| [数值计算指南](NUMERICAL_GUIDE.zh-CN.md) | RSSN 示例、自适应求解器、随机轨迹和带量纲模型 |
| [自然单位](natural.aev)与 [RSSN 算子](rssn.aev) | 自然单位下的量纲检查和带类型的内核接口 |

[教程](https://aevia.apich.org/book?lang=zh)按顺序介绍小程序，并提供改变输入和验证结果的练习。
