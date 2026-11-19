# 综合练习——由位置读数估计加速度

用物理模型、输入验证和独立测试构成一次小型分析。小车从静止开始做匀加速运动，测得的位置含有未知偏置。三个采样点的时间间隔均为 h 时，二阶差分 x₂ − 2x₁ + x₀ 等于 a h²，初始位置和速度项都被消去。

## 实现测量模型

将程序保存为 `cart.aev`。用变体让调用者区分计算结果与被拒绝的样本。张量形状要求恰好三个读数。采样间隔保留为显式参数，因此不同采样频率下可以使用同一个分析函数。

```aevia,run
type Estimate = | Acceleration(Float[Length/Time^2]) | InvalidSpacing

fn estimate(samples: Tensor<[3], Float[Length]>, spacing: Float[Time]): Estimate =
    if spacing <= 0.0 s then InvalidSpacing
    else Acceleration((samples[2] - 2 * samples[1] + samples[0]) / spacing^2)

fn describe(result: Estimate): String =
    match result with
    | Acceleration(value) -> "acceleration = {value}"
    | InvalidSpacing -> "sample spacing must be positive"

fn near_expected(samples: Tensor<[3], Float[Length]>, spacing: Float[Time], expected: Float[Length/Time^2]): Bool =
    match estimate(samples, spacing) with
    | Acceleration(value) -> abs(value - expected) < 0.000001 m/s^2
    | InvalidSpacing -> false

test "second difference removes position offset" =
    near_expected([3.0 m, 4.0 m, 7.0 m], 1.0 s, 2.0 m/s^2)

test "uniform motion has zero acceleration" =
    near_expected([10.0 m, 9.0 m, 8.0 m], 0.5 s, 0.0 m/s^2)

test "coincident samples are rejected" =
    match estimate([0.0 m, 1.0 m, 2.0 m], 0.0 s) with
    | InvalidSpacing -> true
    | Acceleration(_) -> false

fn main(): IO Unit = do
    let readings = [1.1 m, 1.6 m, 3.1 m]
    println(describe(estimate(readings, 0.5 s)))
    println(describe(estimate(readings, 0.0 s)))
```

第一条报告的结果为 4 m/s²，第二条说明输入无效的原因。运行 `aevia test cart.aev` 验证估计量的性质。检查器保证模型中长度与时间的计算符合量纲关系，测试验证公式系数和输入策略。

## 改变采样假设

把三个采样时间放到第二个张量中，并检查其差值。根据计时仪器的精度选择判定等间隔的容差。允许不同间隔时，为非均匀采样推导公式，并加入不同时间间隔的测试。给新模型使用独立名字，直到两个公式在等间隔情形下给出相同结果。

## 将分析组织成包

用 `aevia init` 创建包，把估计器和测试移到 `src/Cart.aev`。在入口模块保留报告与文件输入，增加一个预期结果明确的小型输入样本。更大的数据集应包含样本标识及足够的元数据，以便重建采样间隔。共享包前添加文档示例并运行 `aevia test --doc src`。

更大的项目也可以沿用这些步骤：在边界表达物理约束，保持核心计算纯净，验证已知情形，在调用者能处理的位置报告错误。

## 动手检验

在等距采样下把三个位置平移同一距离，预测结果。再向各采样位置加入恒定速度贡献，二阶差分仍会抵消这些变化。然后给中间样本增加已知扰动，运行前手算它对估计值的影响。这些检查分别揭示不变量与对噪声的敏感性。
