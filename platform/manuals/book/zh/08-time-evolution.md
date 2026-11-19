# 随时间演化的数值模型

演化方程把一个规律转化为轨迹。初始条件、输出时间和所用单位都是模型的一部分。先研究具有闭式解的模型，再使用通用求解器。

## 推导参考值

一阶弛豫满足 dx/dt = −(x − x_eq)/τ。经过时间和时间常数必须有相同量纲。解析解的指数取这两个量的比。

```aevia,run
fn relax(initial: Float[Length], equilibrium: Float[Length], age: Float[Time], tau: Float[Time]): Float[Length] =
    equilibrium + (initial - equilibrium) * exp(-age / tau)

fn main(): IO Unit = do
    let ages = [0.0 s, 1.0 s, 2.0 s, 4.0 s]
    let trajectory = map(fn t => relax(5.0 m, 1.0 m, t, 2.0 s), ages)
    println(trajectory)
    println(trajectory[2])
```

零时刻的位置为 5 m。经过一个时间常数后，位置为 (1 + 4/e) m，约 2.4715 m。先验证这两个事实，再修改模型。增加一个初始条件等于平衡值的用例，此时每个采样点的状态都保持不变。

## 准备求解器输入

时间积分器需要一个变化率函数。为状态指定物理量纲，其变化率则是状态量纲除以时间。耦合系统中不同的状态若有不同量纲，应选择能表达各字段约定的求解器或回调形式。绝对容差使用状态量的单位，相对容差使用比值。

在 [Ode](/stdlib/Ode?lang=zh) 中查阅常微分方程工具。仓库的[数值示例](https://github.com/Apich-Organization/aevia/tree/v0.1/examples)涵盖自适应 DAE、时滞模型、事件检测、边界条件、随机噪声和偏微分方程。先使用短时间区间和有限、有效的控制参数，再扩大实验范围。

## 判断结果是否满足需要

对照预期终点时间和状态。缩小容差后重跑，观察结果及被接受的时间步数量。模型预测守恒律时，在多个点检查约束。错误的方程或初始条件不能靠缩小容差修正。

## 动手检验

用参考解记录终点误差。保持时间区间不变，根据方法收紧容差或缩短步长，分别比较终状态误差和计算量。求解器可能因事件提前停止，也可能返回收敛诊断。提取最后一个时间样本前先检查结果。
