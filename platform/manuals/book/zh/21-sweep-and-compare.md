# 让参数扫描可复现

某台仪器测得的长度偏小。用已知参考值比较几个无量纲增益。目标函数接收单个增益并返回长度误差。每次试验独立，因而可以并行运行同一个目标函数。

## 将目标函数与调度分开

~~~aevia,run
fn prediction(gain: Float, reading: Float[Length]): Float[Length] =
    gain * reading

fn error(gain: Float): Float[Length] =
    abs(prediction(gain, 2.0 m) - 3.0 m)

test "the calibrated gain reproduces the reference" = error(1.5) == 0.0 m
test "a unit gain preserves the uncorrected error" = error(1.0) == 1.0 m

fn main(): IO Unit = do
    let candidates = [0.5, 1.0, 1.5, 2.0]
    let errors = par_map(error, candidates)
    println(candidates)
    println(errors)
    println(sum(errors))
~~~

误差按候选增益的顺序输出。增益为 `1.5` 时标定点的误差为零，其他误差分别为 `2 m`、`1 m` 和 `1 m`，总和是 `4 m`。运行前先写出自己预测的序列。

## 先检查等价，再测速度

保存为 `sweep.aev`。分别用 `aevia --jobs 1 run sweep.aev` 与 `--jobs 4` 运行。再把 `par_map` 改为 `map` 并比较候选顺序和误差张量。小任务的调度成本可能高于计算。测量速度时增加固定网格规模或使用更耗时的纯函数，并随结果记录工作量、构建版本和线程数。 [并行执行与性能分析](12-parallel-and-profiling.md)

## 扩展实验

增加两个已知参考长度，让三个观测共用增益。决定最小化绝对误差之和还是平方误差，改代码前先推导误差单位。用第四个观测检查增益在标定集外的表现。保存结果时附上策略、候选网格、工具链版本和线程数。
