# 自适应常微分方程求解

自适应求解器自行选择时间网格。你为它提供变化率、初始状态、所需的时间区间和误差尺度。某次试探步的误差估计过大时，求解器可以缩短步长再试。本章用一个有解析解的问题比较两次积分，考察精度提高后增加了多少计算量。

## 把模型写成变化率

沿用[随时间演化的数值模型](08-time-evolution.md)中的弛豫模型。位置从 5 m 出发，以 2 s 为时间常数趋向 1 m。变化率是相对于平衡位置的位移除以时间常数后取负值。因此，回调接收时间和长度，返回速度。虽然这条规律本身不显含时间，回调仍需接收时间参数。

经过时间 `t` 后的解析位置为 `1 m + 4 m * exp(-t / (2 s))`。指数无量纲。把解析公式和变化率分别定义，便可用解析公式独立检验终点，以及两个已接受时间步之间的位置。

## 用两种容差积分并检查结果

示例选用 `AdaptiveRk4` 和 `OdePhysicalOpts`。绝对容差 `atol` 和状态一样具有长度量纲，相对容差 `rtol` 无量纲。将 `rtol` 设为零，便可单独观察绝对容差的影响。两次积分都从 2 s 的试探步长开始，最大步长也都是 2 s。最小步长及计算量上限约束求解失败时的重试。

```aevia,run
open Units.SI
import Ode { AdaptiveRk4, OdePhysicalOpts, ode_solve }

fn rate(_t: Float[Time], position: Float[Length]): Float[Length/Time] =
    -(position - 1.0 m) / (2.0 s)

fn reference(t: Float[Time]): Float[Length] =
    1.0 m + 4.0 m * exp(-t / (2.0 s))

fn options(tolerance: Float[Length]): OdePhysicalOpts<Float[Length]> = {
    rtol = 0.0, atol = tolerance, method = AdaptiveRk4,
    steps = None, events = None,
    h0 = 2.0 s, h_min = 0.000001 s, h_max = 2.0 s,
    max_steps = 1000, max_evaluations = 10000,
}

fn main(): IO Unit = do
    let finish = 4.0 s
    let probe = 3.25 s
    let loose = ode_solve(rate, 5.0 m, (0.0 s, finish), options(0.001 m), [])
    let tight = ode_solve(rate, 5.0 m, (0.0 s, finish), options(0.00000001 m), [])
    let loose_error = abs(loose.at(finish) - reference(finish))
    let tight_error = abs(tight.at(finish) - reference(finish))
    let dense_error = abs(tight.at(probe) - reference(probe))
    let last_time = tight.t[length(tight.t) - 1]
    println({ reference = reference(finish), endpoint = tight.at(finish), time = last_time })
    println({ loose_error = loose_error, tight_error = tight_error, dense_error = dense_error })
    println({ method = tight.method, loose_steps = length(loose.t) - 1, tight_steps = length(tight.t) - 1 })
    println({ loose_nfev = loose.nfev, tight_nfev = tight.nfev, rejected = tight.rejected })
    println(last_time == finish && tight_error < loose_error && tight_error < 0.000001 m && dense_error < 0.000001 m)
```

解析终点位置约为 **1.54134113 m**。验证时，较松容差的终点误差约为 **6.8 × 10⁻⁵ m**，较严容差的终点误差约为 **1.1 × 10⁻⁹ m**。较严的积分到达了 **4 s**，且在 **3.25 s** 的误差小于 **1 µm**。最后一行应输出 `true`。示例用误差上限判断结果，允许浮点计算存在微小差异。

结果记录中的 `t` 和 `y` 分别存放已接受的时间和对应状态，初始条件也在其中。`t` 的长度减一，便是已接受的积分步数。可用 `at` 在求解区间内查询数值轨迹，包括位于两个已存储时间之间的点。除核对终点状态外，还可用解析值核对 3.25 s 的插值结果。

## 读懂计算量计数

在这次运行中，较严的积分接受了 114 步，较松的积分接受了 8 步。表示变化率函数调用次数的 `nfev` 从 71 增加到 827。较严的积分还拒绝了两次试探步。拒绝表示求解器做了试算，但在推进已接受轨迹之前丢弃了这一步。步数和调用次数可能随实现细节而变化，适合作为诊断数据。

指定容差控制的是局部误差估计。可以用解析参考值、守恒关系或更严容差下的结果，对照自己关心的量来判断精度。缩小容差可能增加计算量，也可能耗尽步数或函数求值配额。遇到这种情况，先查变化率、单位和初始条件。包含相差很大的时间尺度时，可以考虑刚性求解器。可用方法和选项见 [Ode](/stdlib/Ode?lang=zh)。

## 练习

1. 增加 `atol = 0.00001 m` 的第三次积分。记录终点误差和 `nfev`，比较三组结果，看看增加的函数求值带来了多少精度提升。
2. 为 `options` 增加步长上限参数，用它同时设置 `h0` 和 `h_max`。将较松一组的上限设为 `0.25 s` 后重跑，与原来较松一组的终点误差和计算量进行比较。
3. 让积分在位置下降穿过 2 m 时停止。事件函数使用 `(position - 2.0 m) / (1.0 m)`，令 `direction = -1`、`terminal = true`。将事件时间与 `2 s * ln(4)`（约 **2.773 s**）对照。检查最后接受的时间与事件时间是否一致，并在缩短后的区间内查询插值。仓库的[自适应几何积分示例](https://github.com/Apich-Organization/aevia/blob/v0.1/examples/ode_adaptive_geometric.aev)展示了事件和包含不同量纲字段的状态。
