# 开始使用 Aevia

Aevia 将物理量纲纳入程序的类型检查。本书从带量纲的值入手，用类型表达模型中的方程，对照已知结果验证模型，再将它打包供其他程序使用。[语言规范](/spec?lang=zh)定义语言规则，[标准库](/stdlib?lang=zh)说明可调用的功能。

## 使用工具链

从注册表的[安装页面](/cli?lang=zh)安装工具链。将可执行文件添加到 PATH，再运行 `aevia --version`。CLI 与可选的注册表客户端分开安装。本地运行示例只需要 `aevia`，下载或发布包还需要 `aevia-pkg`。

运行 `aevia` 启动 REPL。先输入 `2 m + 30 cm`，再输入 `:type 2 m + 30 cm`，查看结果的长度量纲。输入 `2 m + 30 s`，对照诊断中的预期量纲和实际量纲。用 `:help` 查看命令，用 `:quit` 结束会话。

## 编写并运行文件

将下列程序保存为 `fall.aev`，或在交互版中选择**运行**。

```aevia,run
fn main(): IO Unit = do
    let height: Float[Length] = 20.0 m
    let gravity: Float[Length/Time^2] = 9.80665 m/s^2
    let elapsed = sqrt(2.0 * height / gravity)
    println(elapsed)
```

运行 `aevia check fall.aev`，只检查程序而不启动 `main`，然后执行 `aevia run fall.aev`。这个例子中的小数按浮点值计算，中间量 2h/g 的量纲为时间平方，结果约为 2.02 秒。把 `height` 改成 `20.0 s`，并给 `elapsed` 标注 `Float[Time]`，观察检查器怎样在需要时间的位置发现量纲错误。

## 在线与离线学习

可运行代码块都是完整程序。浏览器运行具有资源上限，可以输出结果和诊断，文件与网络均不可访问。多模块导入、输入文件和长时间运算可在 CLI 中完成。单页版和下载版包含同一组课程，离线练习时将每个可运行示例分别保存为文件。

继续阅读[值与函数](02-values-and-functions.md)，了解 Aevia 如何表示计算。

## 动手检验

把网页中可运行的同一段源码保存到本地，比较输出。编辑后先运行 `aevia check`。有源码区间诊断时先修第一条，再重新检查。检查通过而运行失败时，用第二组输入区分运行时定义域错误和命令参数错误。分别试一次导致类型错误的修改和一次仅改变结果的修改。
