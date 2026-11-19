# 从 CSV 样本得到物理结果

用可复现样本、类型化边界与纯计算处理一个数据文件。文件表头保留单位元数据。数值单元读取为普通浮点数，因此模型明确把读数转换为米每秒。

## 单独检验计算

下方网页示例使用与文件程序相同的公式。

~~~aevia,run
fn summarize(raw: Tensor<1, Float>): Float[Length/Time] =
    mean(raw * 1.0 m/s)

test "the average retains the speed dimension" =
    summarize([1.0, 2.0, 3.0]) == 2.0 m/s

fn main(): IO Unit = do
    println(summarize([1.0, 2.0, 3.0]))
~~~

## 在本地读取文件

在程序旁创建 `run.csv`：

~~~csv
time [s],speed [m/s]
0,1
1,2
2,3
~~~

将下方源码保存为同一工作目录中的 `traffic.aev`，运行 `aevia --caps fs run traffic.aev`。预期输出 `2.0 m/s`。读文件的示例使用本地 CLI，并显式启用文件能力。

~~~aevia
fn summarize(raw: Tensor<1, Float>): Float[Length/Time] =
    mean(raw * 1.0 m/s)

fn main(): IO Unit = do
    loaded <- read_csv<{ time: Float, speed: Float }>("run.csv")
    match loaded with
    | Err(error) -> println(error)
    | Ok(table) ->
        match column(table, "speed") with
        | Err(error) -> println(error)
        | Ok(raw) -> println(summarize(raw))
~~~

## 演练失败边界

先重命名文件，观察 IO 错误，再复原。删去某行的一个字段，观察解析器拒绝不完整的行。恢复后尝试千米每小时的数值，修改 `summarize` 中的单位转换。文件单位是需要与导入策略核对的数据。本示例使用已知样本的单位。来源若可能采用不同单位，读取 `schema(table)` 后选择显式转换或拒绝文件。

列在运行时才知道时可以读取动态表并使用 `column_dyn`。确定列和转换后，让纯模型保留具体签名，扩大数据规模前先和纯测试的结果核对。 [Data 标准库](/stdlib/Data?lang=zh)
