# 诊断计算失败

一次计时实验测得距离和经过的时间。距离除以时间得到速度。我们用同一个程序排查读数单位错误和经过时间为零的情况。编译器可以在运行前发现单位错误。除法错误则取决于实际传入计算的值。

## 从已知结果开始

将下面的程序保存为 `reading.aev`。这里使用精确的 `Real` 量，因此当时间为零时，除法会报告错误。

```aevia,run
type Reading = { distance: Real[Length], elapsed: Real[Time] }

fn speed(distance: Real[Length], elapsed: Real[Time]): Real[Length/Time] =
    distance / elapsed

fn report(reading: Reading): String =
    let velocity = speed(reading.distance, reading.elapsed)
    "speed = {velocity}"

fn main(): IO Unit = do
    let reading: Reading = { distance = 12 m, elapsed = 3 s }
    println(report(reading))
```

先运行 `aevia check reading.aev`，再运行 `aevia run reading.aev`。检查应通过，程序应输出 `speed = 4 m/s`。修改程序时保留这组输入。处理错误读数之后，用它确认正常计算仍然正确。

## 结合上下文阅读编译诊断

将 `elapsed = 3 s` 改为 `elapsed = 3 m`，然后再次检查。错误代码是 `E0401`，源代码标注显示 `expected T, found L`，即期望时间量纲，实际得到长度量纲。对照 `Reading` 的定义可以看到，`elapsed` 字段的标注要求时间，而改动后的字面量表示长度。诊断可能为整条记录画下划线，因为编译器在这一处比较标注和推断出的各字段类型。逐个核对字段后，恢复原始读数。

在已连接 `aevia-lsp` 或 `aevia lsp --stdio` 的编辑器中重复同样的修改，查看打开的缓冲区中的诊断。将鼠标悬停在 `speed` 上可以查看它的参数和结果类型。如果 `Reading` 声明在别处，可以使用「转到定义」。比较编辑器和终端中的诊断前先保存文件，让两者使用同一份源代码。

## 在 REPL 中单独验证计算

在恢复后的文件所在目录中启动 `aevia repl`，再依次输入以下内容。如果在别处启动 REPL，请填写实际路径。

```text
:load reading.aev
:type speed
speed(12 m, 3 s)
speed(1 m, 2 s)
:quit
```

两次调用分别得到 `4 m/s` 和 `0.5 m/s`。类型检查命令显示两个参数都带有量纲，结果的量纲是长度除以时间。第二次调用还验证了分数形式的速度，省去了修改报告代码的步骤。输入 `:help` 查看可用命令，或输入 `:reset` 开始一个新会话。VS Code 中的 **Aevia: Open REPL** 也能打开这个交互环境。

## 沿调用栈找到运行时错误的输入

将原程序复制为 `reading-zero.aev`，把 `elapsed` 改为 `0 s`。先运行 `aevia check reading-zero.aev`，再运行 `aevia run reading-zero.aev`。类型检查仍然通过。运行时，程序在 `distance / elapsed` 处报告 `E0801`，并给出 `report` 和 `main` 中的调用位置。从出错的表达式沿调用栈向上找，直到定位经过时间是在哪里传入程序的。

要在出错前查看现场，请保存零时间版本，并在 `distance / elapsed` 所在行设置断点。在安装了 Aevia 扩展的 VS Code 中选择 **Aevia: Debug Current File**。支持自定义调试适配器协议配置的编辑器可以启动 `aevia debug --stdio`，然后在启动请求的 `program` 中指定已保存的文件路径。在调用栈中查看调用者，在 Variables 面板确认 `elapsed` 为 `0 s`。还可以在 `report` 中设置断点，然后用「步入」跟踪对 `speed` 的调用。监视窗口可按名称查看当前暂停帧中的绑定。

调试适配器使用串行的源码求值器。如果程序要读取输入，请在启动配置中提供 `stdin` 字符串列表。最终请使用正常运行命令以及实际要采用的执行选项测试模型。

## 动手定义输入处理规则

添加一个接收 `Reading` 并返回 `Result<Real[Length/Time], String>` 的函数。时间为零或为负时返回错误，其余情况返回速度。精确 `Real` 值的比较会产生逻辑命题。用 `decide(reading.elapsed > 0 s) == Some(true)` 作为接受读数的条件，测试两个精确速度是否相等时也可用同样的写法。修改报告，让它能显示这两种结果。分别测试零时间、负时间和原始的三秒读数。前两种情况应产生你指定的错误消息，第三种仍应得到 `4 m/s`。用同一组输入进行手算、REPL 检查和 `aevia test` 验证。[验证科学计算结果](11-testing.md)
