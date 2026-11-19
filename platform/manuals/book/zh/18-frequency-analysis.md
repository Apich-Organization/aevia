# 分析振动记录的频率

位置传感器每隔 125 ms 返回一个长度读数。轨迹包含固定的基准位置和较小的周期振动。我们要得到基准位置、振动频率和位移振幅。离散傅里叶变换将记录分解到各个频率格。采样间隔决定每个频率格的频率，变换的归一化约定决定如何从系数求振幅。

## 构造参考信号

从八个样本开始。依次重复 `1.0 m`、`1.2 m`、`1.0 m` 和 `0.8 m`，因此一个周期占四个采样间隔。参考信号的位移振幅为 `0.2 m`。八个点包含两个整周期，信号恰好落在一个频率格上。

将下方程序保存为 `vibration.aev`。`rfft` 接收实数样本，并返回复频谱的非负频率部分。需要重建信号时，应随频谱保留原始长度和采样间隔。`rfftfreq` 根据带单位的采样间隔给出各频率格的 `Float[1/Time]` 值。

```aevia,run
import FFT { rfft, irfft, rfftfreq }
import Math.Complex { magnitude }

let gap: Float[Time] = 0.125 s
let samples: Tensor<1, Float[Length]> = [1.0 m, 1.2 m, 1.0 m, 0.8 m, 1.0 m, 1.2 m, 1.0 m, 0.8 m]
let spectrum = rfft(samples)
let frequencies = rfftfreq(8, gap)
let baseline = magnitude(spectrum[0]) / 8
let amplitude = 2 * magnitude(spectrum[2]) / 8
let reconstructed = irfft(spectrum, 8)

fn close_length(actual: Real[Length], expected: Real[Length]): Bool =
    match decide(abs(actual - expected) < 0.000001 m) with
    | Some(yes) -> yes
    | None -> false

test "the second bin resolves a two-hertz vibration" = frequencies[2] == 2.0 Hz
test "FFT scaling recovers the offset and amplitude" = close_length(baseline, 1.0 m) && close_length(amplitude, 0.2 m)
test "inverse restores the measured series" = max_all(map(fn x => abs(x), reconstructed - samples)) < 0.000001 m

fn main(): IO Unit = do
    println("frequencies: {frequencies}")
    println("baseline: {baseline}")
    println("vibration: {amplitude} at {frequencies[2]}")
```

## 读出频谱

八个样本对应的变换窗口长度为 `8 × 0.125 s = 1 s`，因此相邻频率格相差 `1 Hz`。第二个正频率格对应 `2 Hz` 振动。零频率系数是所有读数之和，除以八即可得到基准位置。对实数信号，正负频率的系数互为共轭。`rfft` 仅存储非负部分，所以将正频率系数的模乘二后除以八，即可还原位移振幅。这条规则适用于内部频率格。零频率格，以及偶数长度下的奈奎斯特频率格，各自只对应一个系数。

两个 `magnitude` 结果都保留输入量纲。`magnitude` 返回 Aevia 的 `Real`，其比较产生命题。`close_length` 用 `decide` 将可判定的命题转换为测试结果。第三项测试的反变换返回 `Float`，可以直接比较最大绝对残差。三项测试分别验证频率轴、归一化和重建。

运行 `aevia test vibration.aev`。保持样本不变，将间隔改为 `0.25 s`。位移振幅仍约为 `0.2 m`，频率则变为 `1 Hz`。更新期望值之前，第一项测试会失败。样本文件必须保留时间标定，才能确定其物理频率。

## 重建奇数长度的记录

当原始记录长度 `N` 为偶数时，`N/2 + 1` 个存储系数中的最后一个位于奈奎斯特频率。长度为五的奇数记录和长度为四的偶数记录都会产生三个系数，其最后一个频率格含义却不同。调用 `irfft` 时显式传入原长度即可区分。下例用与振动记录相同的长度单位创建五点信号。

```aevia,run
import FFT { rfft, irfft, rfftfreq }

let samples: Tensor<1, Float[Length]> = [0.0 m, 0.1 m, 0.2 m, 0.1 m, 0.0 m]
let half = rfft(samples)
let recovered = irfft(half, 5)
let bins = rfftfreq(5, 0.25 s)

test "an odd-length signal keeps its last sample" = shape(recovered) == [5] && abs(recovered[4] - samples[4]) < 0.000001 m

fn main(): IO Unit = do
    println("nonnegative bins: {bins}")
    println("recovered samples: {recovered}")
```

## 观察落在频率格之间的信号

把手写样本换成用 `sin` 生成的 `2.5 Hz` 样本。正弦函数参数用频率乘经过时间再乘 `2π`，所得量无量纲。尝试在同一采样间隔下延长记录，或者先加窗再变换。比较峰值和邻近频率格的幅度分布。记录在周期中途结束时，能量会扩散到多个频率格。加窗会改变扩散形状与振幅比例，所以应用到真实测量数据前，先用已知频率和振幅的参考信号校准。

最后写下所选采样间隔的最高可辨频率。间隔为 `0.125 s` 时，该频率为 `4 Hz`。更高的频率经过采样后可能表现为较低的频率。对同一批样本提高变换的显示分辨率，采样频率上限仍保持不变。
