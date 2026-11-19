# 在保留单位的同时复用模型

假设两种仪器分别报告读数的可能区间。一种测量距离，另一种测量经过的时间。两个区间都需要计算中点和宽度。算式相同，计算结果却要保留输入的量纲。先建立一份共用的测量模型，再为两种实验各自使用的类型起一个易读的名字。

## 为共用计算引入量纲参数

将下面的程序保存为 `intervals.aev`。区间的两个端点使用同一量纲，构造函数在端点顺序有误时返回明确的结果。

```aevia,run
type Interval<d> = { lower: Float[d], upper: Float[d] }
type DistanceInterval = Interval<Length>
type TimingInterval = Interval<Time>
type IntervalResult<d> = | Accepted(Interval<d>) | Reversed

fn interval<d>(lower: Float[d], upper: Float[d]): IntervalResult<d> =
    if lower <= upper then Accepted({ lower = lower, upper = upper })
    else Reversed

fn midpoint<d>(bounds: Interval<d>): Float[d] =
    (bounds.lower + bounds.upper) / 2.0

fn width<d>(bounds: Interval<d>): Float[d] =
    bounds.upper - bounds.lower

fn describe<d>(reading: IntervalResult<d>): String =
    match reading with
    | Accepted(bounds) -> "midpoint = {midpoint(bounds)}, width = {width(bounds)}"
    | Reversed -> "lower must be at most upper"

test "a length midpoint preserves its dimension" =
    midpoint({ lower = 2.0 m, upper = 6.0 m }) == 4.0 m

fn main(): IO Unit = do
    let distance: DistanceInterval = { lower = 2.0 m, upper = 6.0 m }
    let timing: TimingInterval = { lower = 1.0 s, upper = 3.0 s }
    println(describe(interval(distance.lower, distance.upper)))
    println(describe(interval(timing.lower, timing.upper)))
    println(describe(interval(6.0 m, 2.0 m)))
```

运行 `aevia run intervals.aev`。距离区间的中点和宽度都是 `4.0 m`，时间区间的中点和宽度都是 `2.0 s`，端点颠倒的情况会输出提示。再运行 `aevia test intervals.aev`，把距离中点与可以手算的结果比较。

先看 `midpoint<d>`。每次调用的两个端点共用一个量纲 `d`，结果也带有这个量纲。两个操作数的量纲相同，相加后量纲不变。除以无量纲的 `2.0` 也不会改变量纲。`width<d>` 延续相同的量纲关系，但计算的是另一个量。在这种关系成立的地方，可以传入长度、时间或其他量纲的区间。

## 用类型别名命名不同用途

`DistanceInterval` 是 `Interval<Length>` 的名称，`TimingInterval` 是 `Interval<Time>` 的名称。它们都是透明别名。只要记录的字段符合要求，就可以传给接收相应区间的函数。当两种测量同时出现在一个模型中，这些名称能让类型标注更清楚。如果把时间区间交给只接受长度区间的函数，编译仍会失败，因为展开别名后，各字段的类型不同。

再看 `IntervalResult<d>`。它是具有独立类型身份的变体，用不同的构造器表示两种结果。类型检查器可以验证匹配是否覆盖两种情况。端点顺序取决于实际的值，因此 `interval` 在运行时检查顺序。`DistanceInterval` 类型的记录字面量也能装入顺序颠倒的端点。若需要在输入边界明确判断一组端点是否有效，就调用 `interval`，在结果被接受后再调用 `midpoint` 和 `width`。

## 验证抽象的适用边界

尝试把 `{ lower = 2.0 m, upper = 6.0 s }` 传给 `midpoint`。这两个字段找不到共同的量纲 `d`。再把 `midpoint` 的函数体改成 `bounds.lower + 1.0 m` 并检查文件。增加一米要求操作数是长度，而函数声明承诺支持任意量纲，因此编译器会拒绝这个函数体。恢复原代码。如果某种仪器确实需要一米的固定偏移量，就为这项操作声明只接受长度的函数签名。

示例测试使用的浮点数很小，其中点可以精确表示。对于会累积舍入误差的数据，请选择量纲合适的容差。同一个模型可以同时包含长度测试与独立的时间测试，后者使用以秒表示的容差。

## 动手扩展并共享模型

编写 `pad<d>(bounds: Interval<d>, margin: Float[d]): Interval<d>`。从下端点减去扩展量，在上端点加上扩展量。用于真实数据前，先规定扩展量为负的含义。当扩展量为 `0.5 m` 时，距离区间应扩大为 `1.5 m` 至 `6.5 m`。当扩展量为 `0.5 s` 时，同一个函数应把时间区间扩大为 `0.5 s` 至 `3.5 s`。再检查把时间扩展量传给距离区间的调用是否被拒绝。

准备共享时，将类型、纯函数和测试放在模型模块中，在入口文件中处理输入转换和输出。把量纲关系和端点顺序规则写在使用者首次接触模型的位置。分别为两种仪器编写一个小程序，导入同一组函数，然后再次运行模型模块的测试。[模块和包](10-modules-and-packages.md)
