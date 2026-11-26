# Aevia 示例

[English](README.md)

在仓库根目录使用 `aevia run examples/FILE.aev` 运行完整程序。包含断言的示例用 `aevia test examples/FILE.aev` 验证。

| 示例 | 学习内容 |
|---|---|
| [运动学](kinematics.aev)与[单位](units.aev) | 精确与近似量、单位转换、仿射温度 |
| [张量](tensors.aev)与[稀疏张量](sparse.aev) | 矩阵乘积、归约、存储格式 |
| [矩阵运算](matrix_operations.aev) | 稠密矩阵构造（`fill`、`linspace`、`eye`、`diag`）、`trace`、`tril`/`triu` 与 `set` |
| [微积分](calculus.aev)与[符号表达式](symbolic.aev) | 求导、积分、极限、解方程 |
| [模块](modules/main.aev) | 两个文件之间的导入 |
| [传感器校准](sensor_calibration.aev)与[弛豫拟合](relaxation_fit.aev) | 带量纲残差、质量阈值和模型拟合 |
| [绘制传感器测量结果](plot_series.aev) | 共用坐标轴、误差条、直方图和 SVG、HTML、GIF 动画导出 |
| [仿真 IO 与可视化](sim_io_visualization.aev) | 阻尼振子 CSV/NPY 导出与 SVG、HTML、GIF 动画可视化 |
| [并行计算](parallel.aev) | 保序映射、归约、并行任务 |
| [数值计算指南](NUMERICAL_GUIDE.zh-CN.md) | RSSN 示例、自适应求解器、随机轨迹和带量纲模型 |
| [自然单位](natural.aev)与 [RSSN 算子](rssn.aev) | 自然单位下的量纲检查和带类型的内核接口 |

[教程](https://aevia.apich.org/book?lang=zh)按顺序介绍小程序，并提供改变输入和验证结果的练习。
