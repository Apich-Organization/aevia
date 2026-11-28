# 并行执行与性能测量

各输入可以独立处理，且每项工作的开销大于调度成本时，可以考虑并行。先写出串行计算，保留结果作为对照。

## 对候选输入应用独立模型

下列每个候选输入都是距离。各项开销较小，便于快速比较结果。测量工作线程利用率时，可以换成更耗时的纯数值回调。

```aevia,run
fn score(x: Float[Length]): Float[Length^2] =
    (x - 2.5 m)^2

fn main(): IO Unit = do
    let candidates = [1.0 m, 2.0 m, 3.0 m, 4.0 m]
    println(map(score, candidates))
    println(par_map(score, candidates))
```

本地先运行 `aevia --jobs 1 run candidates.aev`，再用 `--jobs 4` 运行。对照各下标的结果，映射保留输入顺序。对归约，需要留意重新分组的影响，并为对应方法选择合适的组合运算。

`--jobs` 设置本次运行的语言工作线程上限。省略时，Aevia 依次读取 `AEVIA_JOBS`、`RAYON_NUM_THREADS`，或采用当前进程可用的 CPU 数。线程池随工作量增长，直到上限。本例输入较少，两种设置都会在调用线程上执行。若要同时让语言求值和 RSSN 全局 CPU 池使用一个线程，可在设置 `RAYON_NUM_THREADS=1` 后以 `--jobs 1` 运行。

`par_tabulate(n, f)` 对下标 `0` 到 `n - 1` 调用纯函数，并按下标顺序返回结果。输入较多时，工作线程分批生成下标。工作量较小或函数无法传给工作线程时，则在本地求值。

`det`、`inv` 和 `linsolve` 处理较大的稠密精确矩阵时也可以使用这一线程上限。主元按顺序处理，工作线程更新互不依赖的行。这三个算子都给出确定的精确结果。

读取大型 Fortran 列主序 NPY 数组时，转换为行主序的工作可以由当前的 RSSN CPU 池分担，包括有三个或更多非单例轴的数组。比较不同线程数时，分别测量转换和文件读取耗时。

## 查看和选择执行后端

```console
aevia devices
aevia --backend auto run candidates.aev
aevia --backend cpu --jobs 1 run candidates.aev
aevia --rssn-backend cpu-parallel run candidates.aev
```

`auto` 是默认设置。它使用 Core IR 解释器，并可通过 RSSN 的 Cranelift JIT 编译符合条件的热点标量函数。
较大的稠密 `Float` 矩阵乘法也可交给支持原生 F64 的 GPU。`--backend gpu` 还会尝试较小的合格乘法，
以及 wgpu 能在其精度和范围内精确计算的整数值 `Float` 乘法。没有设备、未授予 `gpu` 能力或运算不符合条件时，由 CPU 计算。`cpu-parallel` 在宿主上按工作线程设置运行。
`ir` 和 `ast` 分别选择用于对照的解释器。在 REPL 中，切换解释器会影响后续求值和新定义。

`--rssn-backend` 为可移植表达式的编译选择进程级 RSSN 后端。其 `auto` 设置先使用 CPU
指令带，在支持 Cranelift 的宿主上可在后台编译热点表达式，其他宿主使用解释器。
`aevia devices` 列出可用后端和当前选择。在 REPL 中用 `:rssn-backend` 查看，
用 `:rssn-backend auto` 恢复自动选择。`--backend` 独立控制 Aevia 的求值器和矩阵运算路由。

RSSN 的进程默认后端由 CLI 或嵌入应用控制。Aevia 源码可以为单次编译选择后端句柄。
探测或执行 GPU 运算需要 `gpu` 能力，在收紧权限的 `with_cap` 块内使用先前创建的 GPU 句柄或编译程序时也是如此。

运行 `bazel build //:toolchain_minimal` 构建省去 RSSN 可选 JIT、wgpu 和 NumPy 集成的精简
CLI。可执行文件位于 `bazel-bin/toolchain_minimal/bin/aevia`，Windows 上带 `.exe` 后缀。

## 测量需要比较的部分

把文件和终端 IO 与待测计算分开。使用固定样本、预热和多次重复运行，并保持线程与优化设置一致。比较耗时前先验证各配置的结果。`aevia bench --help` 说明基准声明和基线文件的格式。

优化构建工具链和 CLI 的 `--release` 选项作用在不同阶段。构建设置影响解释器二进制文件，CLI 选项开启对已检查程序的优化。分享测量结果时，一并记录这两组设置和启用的后端功能。

## 在 Windows 上采样

开发工具 `aevia-prof` 可以在 x86-64 Windows 上采样子进程，无需管理员权限。先从仓库根目录
构建保留性能分析符号的 CLI。在 PowerShell 中将 `$profiler` 设为 `aevia-prof.exe` 采样器的路径，
并把带符号的 CLI 作为第一个参数。

```powershell
bazel build //:toolchain_profiling
$env:AEVIA_PROF_FOLDED = "candidates.folded"
& $profiler .\bazel-bin\toolchain_profiling\bin\aevia.exe run candidates.aev
Remove-Item Env:AEVIA_PROF_FOLDED
```

标准错误流显示叶函数的自身采样数和整条调用栈的包含采样数。每份采样对同一函数最多计一次。用 `AEVIA_PROF_TOP` 设置汇总行数，用 `AEVIA_PROF_INTERVAL_US` 设置采样间隔。folded 导出使用同一批忙碌采样，按调用者到叶函数的顺序写入归并后的符号名，保留名字的完整长度。每次导出请用新路径。采样器保留子进程的输出和退出状态，并在标准错误流报告导出失败。

## 继续探索

分别增加输入数和单项计算开销，记录并行版本何时更快，以及内存使用是否上升。如果任务执行 IO，先查看 `spawn` 和 `await` 的语义，并让各任务的副作用相互独立。

## 动手检验

先用两种线程数执行同一个固定任务，保存结果再进行计时。保持模型不变并增大任务，比较并行何时开始有收益，随计时结果记录工作量与特性参数。 [可复现参数扫描](21-sweep-and-compare.md)
