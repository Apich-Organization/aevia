# 使用种子的随机模拟

随机模型产生一组可能轨迹的分布。重放一条轨迹，可以检验给定输入是否确定了同一次计算。模拟一组轨迹，则可以检查均值、方差等统计量。本章选择一个能够事先推导终点分布的过程，分别做这两种检验。

## 推导终点分布

设粒子从 0 m 出发，漂移速度恒为 0.5 m/s，布朗噪声的幅度为 0.4 m/√s。在方程 `dX = μ dt + σ dW` 中，一个时间区间内的布朗增量均值为零、方差等于该区间的长度。增量的量纲是时间的平方根，乘以扩散系数后得到以米计的位置变化。漂移项产生的位置变化也以米计。

经过 1 s，位置的期望为 **0.5 m**，标准差为 **0.4 m**。单条轨迹的终点可能与 0.5 m 相差较大。对 64 条独立轨迹取平均，预期的均值标准误为 `0.4 m / sqrt(64)`，即 **0.05 m**。

## 重放单条轨迹，检验轨迹集合

定步长的 Euler–Maruyama 方法在每一步累加确定性漂移和布朗增量。本例的系数恒定，因此除浮点舍入外，该方法在选定网格上能再现终点分布。状态是只含一个长度的向量。`Diagonal` 给状态的每个分量分配独立的噪声通道。长度为 0.125 s 的八步恰好到达所需终点 1 s。

```aevia,run
open Units.SI
open Sde

fn drift(_t: Float[Time], _y: Tensor<[1], Float[Length]>): Tensor<[1], Float[Length/Time]> =
    [0.5 m / (1.0 s)]

fn diffusion(_t: Float[Time], _y: Tensor<[1], Float[Length]>): Tensor<[1], Float[Length/Time^(1/2)]> =
    [0.4 m / sqrt(1.0 s)]

fn options(seed: Int) = Fixed({
    structure = Diagonal, method = EulerMaruyama, seed = seed,
    t0 = 0.0 s, dt = 0.125 s, steps = 8, terms = 0,
})

fn main(): IO Unit =
    match sdeint_diag(drift, diffusion, [0.0 m], options(2718)) with
    | Err(problem) -> println(problem)
    | Ok(first) ->
        match sdeint_diag(drift, diffusion, [0.0 m], options(2718)) with
        | Err(problem) -> println(problem)
        | Ok(repeat) -> do
            let members = sde_ensemble_diag(drift, diffusion, [0.0 m], options(2718), 64)
            match find(is_err, members) with
            | Some(problem) -> println(problem)
            | None -> do
                let endpoints = map(fn member => match member with
                    | Ok(path) -> path.y[length(path.y) - 1][0]
                    | Err(_) -> nan * (1.0 m), members)
                let center = mean(endpoints)
                let spread = std(endpoints)
                let time = first.t[length(first.t) - 1]
                let replay = first.t == repeat.t && first.y == repeat.y
                println({ endpoint = first.y[length(first.y) - 1][0], time = time, replay = replay })
                println({ paths = length(members), mean = center, std = spread })
                println(time == 1.0 s && replay && abs(center - 0.5 m) < 0.15 m && abs(spread - 0.4 m) < 0.15 m)
```

第一条输出应显示 `time = 1.0 s` 和 `replay = true`。使用示例种子时，单条轨迹的终点约为 **1.177 m**。这只是一次抽样。实际验证时，集合的样本均值约为 **0.492 m**，样本标准差约为 **0.380 m**。最后一行应为 `true`。两个统计量各允许 0.15 m 的偏差，对均值而言相当于三个预期标准误。

每条轨迹都以 `Result` 表示。程序先检查是否有成员计算失败，再计算汇总统计量。逐项映射中的模式匹配覆盖了所有分支，在通过检查后不会用到其中的 `NaN` 占位值。出现失败成员时，应先查看报错。删除失败路径可能改变所观察到的分布。

## 按验证目标选择复现方式

种子和数值设置都相同时，轨迹重放可用于定位单条路径的问题。集合求解会为各成员推导不同的种子。某个成员的结果异常时，可用 `sde_ensemble_seed` 求出该成员的种子，再用相同设置单独运行。用同一个基础种子循环求解，只会反复得到同一条轨迹，使样本规模看起来比实际更大。

更换种子后，样本均值和标准差会有所变化。比较理论值时要给抽样波动留出余量。增加独立路径的数量，均值标准误会按样本量平方根的倒数下降。即使种子相同，修改时间网格也可能改变生成的噪声实现。逐路径比较两个步长时，两个网格的布朗增量应来自同一个底层噪声过程。系数随时间或状态变化时，要分别检查时间离散误差和样本数量。[Sde 参考](/stdlib/Sde?lang=zh)介绍了其他数值方法和噪声结构。

## 练习

1. 将扩散系数置零。检查终点是否在舍入误差范围内等于 0.5 m，以及轨迹集合的离散程度是否为零。
2. 将系数增至 `0.8 m / sqrt(1.0 s)`。理论均值仍为 0.5 m，标准差则翻倍。先按照新的标准差和抽样误差调整检查阈值，再运行。
3. 更换种子，比较第一条轨迹和汇总统计量。恢复原种子及时间网格，检查能否复现原结果。仓库的[自适应隐式 SDE 示例](https://github.com/Apich-Organization/aevia/blob/v0.1/examples/sde_adaptive_implicit.aev)演示了试探步被拒绝时如何继续使用同一份布朗噪声实现。
