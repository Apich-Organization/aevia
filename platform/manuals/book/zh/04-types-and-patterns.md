# 类型、变体与模式

记录把同时存在的字段放在一起。变体表示多种备选情况，各自可以携带不同的数据。读数可能缺失或无效时，可以用变体表达，用 `match` 在作决策的位置逐个处理。

## 用结果类型表达决策

下列程序按照传感器的物理范围筛选读数。有效读数携带一个长度量，拒绝时返回可用于报告的原因。

```aevia,run
type Reading = | Accepted(Float[Length]) | Rejected(String)

fn calibrate(raw: Float[Length]): Reading =
    if raw < 0.0 m then Rejected("below the sensor range")
    else Accepted(raw + 0.02 m)

fn describe(reading: Reading): String =
    match reading with
    | Accepted(value) -> "calibrated: {value}"
    | Rejected(reason) -> "invalid: {reason}"

fn main(): IO Unit = do
    println(describe(calibrate(0.8 m)))
    println(describe(calibrate(-0.1 m)))
```

删掉 `describe` 的一个分支并检查文件，类型检查器会报告遗漏的情况。当其余情况确实共享同一种响应时，可以使用通配符。有不同科学含义的情况应分别处理。

## 处理 `Option` 与 `Result`

`Some(value)` 和 `None` 表示有值或无值。`Ok(value)` 和 `Err(reason)` 向调用者表示成功或失败。在组合库调用前查看签名，尤其留意数值函数返回的诊断和收敛记录。

可先在 `match` 中明确显示失败。选择替代值需要物理依据。比如，计算湿度平均值时，缺失的湿度值不应计为零。

## 添加回归用例

添加 `OutOfRange` 变体，表示超过 2 m 的测量。更新 `describe`，然后写一个 `test`，覆盖略低于和略高于阈值的情况。有关处理文件时报错的办法，见 [IO、数据与错误](05-io-and-data.md)。

## 动手检验

增加一个传感器缺失的变体，再显式扩展匹配，并测试每个分支。剩余情况确有共同策略时可使用通配模式。各失败情况处理不同时，为每个分支单独编写规则，后续增加状态时也能明确处理位置。
