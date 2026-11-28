# 符号计算

Aevia 能保留精确表达式，并由 RSSN 对其做变换。先求出公式再代入具体输入时可以使用这种计算。保留待变换函数的纯性，并在有助于理解运算时显式标注参数的量纲。

## 对位置模型求导

给定起始速度 v 与恒定加速度 a，位置为 v t + a t²/2。导数的量纲为速度。在两个时间点求值，观察速度变化。

```aevia,run
fn position(t: Real[Time]): Real[Length] =
    (2 m / 1 s) * t + (3 m / 1 s^2) * t^2 / 2

fn main(): IO Unit = do
    let speed = diff(position)
    println(position(4 s))
    println(speed(0 s))
    println(speed(4 s))
```

四秒时的位置恰好为 32 m。导数从零时刻的 2 m/s 变为四秒时的 14 m/s。测试零时刻的值有助于发现遗漏的系数，测试非零时刻有助于发现遗漏的幂次。

## 选择精确计算或数值计算

`diff(f)` 返回可调用的导函数。定积分接收函数和上下界。符号积分可能保留尚未求值的项，精确根也可以采用符号形式。要得到数值，使用 `approx` 并处理其结果。求根时，分析解集和从给定位置出发的数值求解是不同的操作。通过方程残差验证结果，多根问题可尝试不同初值。

对带单位的函数求导时，结果量纲为输出量纲除以输入量纲。积分则将两者相乘。在每次变换的边界跟踪量纲，让模型修改的影响可见。

## 查找 RSSN 算子并传入经过检查的函数

运行 `aevia rssn --describe num_native_par_map_slice` 查看当前链接内核的签名、量纲规则
及可用绑定。`Rssn.call_ho` 分别接收普通值列表和经过检查的回调列表。每组内部都按签名中的
顺序排列。这个示例将带量纲的向量和两个整数控制量作为普通值，回调将距离转换为时间。

```aevia,run
fn round_trip_time(distance: Float[Length]): Float[Time] =
    (distance / 1.0 m) * 2.0 s

fn round_trip_times(): Result<Tensor<1, Float[Time]>, OpError> =
    let ordinary = [to_value([1.0 m, 2.0 m, 3.0 m]), to_value(1), to_value(3)]
    match Rssn.call_ho("num_native_par_map_slice", ordinary, [to_callback(round_trip_time)]) with
    | Ok(value) -> from_value<Tensor<1, Float[Time]>>(value)
    | Err(error) -> Err(error)

test "the returned values retain order and time units" =
    round_trip_times() == Ok([2.0 s, 4.0 s, 6.0 s])

fn main(): IO Unit = do
    println(round_trip_times())
```

`from_value` 校验返回张量的秩、元素类型和量纲。回调同步执行，句柄在本次调用期间有效。
使用 `aevia rssn --audit` 查看链接内核的覆盖情况，或运行
`aevia rssn --emit --out generated --namespaces` 导出其模块。

## 改变实验

用表示恒定加加速度的三次项替换恒定加速度项，选取单位使位置仍为长度。先预测导数的量纲和零时刻的值，再运行修改后的程序。

## 动手检验

选一个远离奇点的点，对符号导数求值。再用原函数的中心差分近似同一导数，将步长减半后继续比较。相符说明模型在该邻域的结果一致。定义域假设和单点求值分别约束符号计算的不同环节。
