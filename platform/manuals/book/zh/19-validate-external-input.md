# 先验证输入，再分配计算资源

实验的样本数来自文本。成功解析的整数仍可能太小而无法汇总，或太大而超过预算。在分配张量或运行求解器之前分别作出判断，让语法错误与模型策略各有明确出口。

## 先解析，再应用策略

~~~aevia,run
type SampleCount = | Count(Int) | NotAnInteger | OutsideRange

fn sample_count(text: String): SampleCount =
    match parse_int(text) with
    | Err(_) -> NotAnInteger
    | Ok(n) ->
        if n >= 2 && n <= 10000 then Count(n)
        else OutsideRange

fn report(input: String): String =
    match sample_count(input) with
    | Count(n) -> "accepted {n} samples"
    | NotAnInteger -> "enter a whole number"
    | OutsideRange -> "enter a count from 2 through 10000"

test "minimum sample count is allowed" = report("2") == "accepted 2 samples"
test "fractional text fails before range checking" = report("2.5") == "enter a whole number"
test "unbounded input cannot reach the allocation stage" =
    report("1000000000000000000000") == "enter a count from 2 through 10000"

fn main(): IO Unit = do
    println(report("24"))
    println(report("2.5"))
    println(report("0"))
~~~

保存为 `sample_count.aev`。程序接受 `24`，为 `2.5` 报语法问题，为 `0` 报范围问题。运行 `aevia test sample_count.aev`。第三个用例中的长整数可以被 Aevia 表示，因此分配前仍需检查范围。

## 接入一个外部来源

按 [IO 与数据](05-io-and-data.md)说明扩展 `main`，从参数列表取得样本数。另行决定缺少参数时如何提示。报告错误时保留输入的任务名或数据行，方便定位。网页示例使用固定文本，因此无需终端输入即可演练成功和错误路径。

只把带 `Count` 标签的值交给分配样本的代码。多个入口共享范围时，把范围检查放在同一个函数。若两类任务有不同预算，就用类型化参数传入限制，并分别测试策略。

## 检查边界

为 `10000`、`10001`、`-1` 与空字符串补上测试，先预测分支再执行。处理测量时可改用 `parse_float`，在输入边界明确选择单位，再将带量纲的参数交给模型。 [量纲和单位](03-dimensions-and-units.md)
