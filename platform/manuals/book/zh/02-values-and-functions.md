# 值与函数

`let` 将一个名字绑定到一个值。转换因子和模型假设可以只定义一次，并放在使用它们的函数旁边。函数接收参数值并返回一个值。即使函数体可以推断，签名仍说明了函数对使用者的约定。

## 预测增长趋势

从等比数列的几项开始。倍率和项数属于不同类型的输入，因此在函数中分别标注。记录用字段名组织相关的量。

```aevia,run
fn sample(initial: Float, ratio: Float, step: Int): Float =
    initial * ratio ^ step

fn main(): IO Unit = do
    let forecast = { day_1 = sample(100.0, 1.1, 1), day_7 = sample(100.0, 1.1, 7) }
    println(forecast.day_7)
```

最后一个数约为 194.87。保持初值不变，增加项数。打印足够多的位数时可以看到小数近似的影响。即使方程很简单，验证科学计算结果也通常需要容差。

## 纯函数与局部名字

代码块通常先写局部绑定，再给出表示结果的最后一个表达式。匿名函数如 `fn (x, y) => x + y` 可以捕获外层绑定。需要测试或复用一段操作时，把它提取成具名函数。调用多参数函数时，在同一对括号内列出各参数。如果管道需要一元函数，可显式写匿名函数，或用占位符构造调用。

运行下列程序。传给 `map` 的回调捕获了 `offset`。

```aevia,run
fn adjusted(values: Tensor<1, Float>, offset: Float): Tensor<1, Float> =
    map(fn x => x + offset, values)

fn main(): IO Unit = do
    let baseline = [3.0, 4.5, 5.5]
    let result = adjusted(baseline, 0.25)
    println(result)
    println(mean(result))
```

## 先查看类型再判断结果

在 REPL 中定义上述函数后，试试 `:type adjusted`，再传入一个标量。错误定位到参数位置，不会自动把标量转成单元素张量。在文件中，给模型边界加有意义的签名，让检查器推断中间绑定。

练习时，在预测记录中增加 `ratio` 字段，并写一个函数同时返回第七天的预测值及其相对初值的倍率。如果预测带有物理单位，继续阅读[量纲与单位](03-dimensions-and-units.md)。

## 动手检验

给纯函数命名，绑定两个输入，并调用两次。在内层作用域声明一个同名绑定，预测函数读取哪个值。然后改成显式参数，并比较调用点。调用者需要改变该值，或调用处应清晰展示输入时，使用显式参数。
