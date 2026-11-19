# 量纲与单位

量纲是在类型层计算的基本量之积。长度除以时间是 `Length/Time`。单位给出某个量纲的比例和显示约定。科学程序常以一种单位接收数据、以另一种单位报告结果，而模型的类型签名保持一致。

## 编写带量纲的签名

用距离和经过时间计算速度。将来调整调用时，如果交换了参数位置，检查器就能发现。

```aevia,run
fn pace(distance: Float[Length], elapsed: Float[Time]): Float[Length/Time] =
    distance / elapsed

fn main(): IO Unit = do
    let distance = 2.4 km
    let elapsed = 720.0 s
    let speed = pace(distance, elapsed)
    println(speed)
    println(value_in(m, distance))
```

`value_in(m, distance)` 给出以米计的数值。在需要向其他工具交付无量纲数字的边界使用它，计算时保留原来的量。如果希望返回以米显示的量，使用 `to(m, distance)`。

## 区分点与差值

`25.0 °C` 这样的仿射温度是一个点。两个读数的差是温度间隔。在此模型中，两个温度点相加没有对应物理意义。

```aevia,run
fn main(): IO Unit = do
    let earlier = 18.0 °C
    let later = 25.0 °C
    let change = later - earlier
    println(value_in(K, later))
    println(change)
```

绝对温度以开尔文计约为 298.15，温差为 7 开尔文。转换温度点需要考虑仿射偏置，两个读数相减时偏置抵消。

## 选择精确量与近似量

已知的有理比例和单位恒等式适合使用精确值。数值求解器的浮点接口应接收符合量纲要求的值。`exp` 等函数要求无量纲参数。在衰减模型中写 `exp(-t / lifetime)`，其中 `t` 与 `lifetime` 都是时间量。

试着给 `pace` 传入压力和时间，阅读诊断中的量纲约定。然后修改温度示例，报告 15 °C 与 20 °C 的差。成组的带量纲读数见[张量与线性代数](06-tensors.md)。
