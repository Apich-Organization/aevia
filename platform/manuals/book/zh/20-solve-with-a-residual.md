# 解线性系统并校验残差

两台仪器各测量两个位移的不同线性组合。标定矩阵无量纲，观测量是长度。求解后把未知量代回原模型，检查是否重现观测。

## 先用可手算的用例

~~~aevia,run
import LinearAlgebra as LA

fn residual_ok(): Bool =
    let a = [[2.0, 1.0], [1.0, 3.0]]
    let b = [5.0 m, 10.0 m]
    match LA.linsolve(a, b) with
    | Ok(x) -> LA.norm(a @ x - b) < 1e-9 m
    | Err(_) -> false

test "substituting the solution recovers the observations" = residual_ok()

fn main(): IO Unit = do
    let a = [[2.0, 1.0], [1.0, 3.0]]
    let b = [5.0 m, 10.0 m]
    match LA.linsolve(a, b) with
    | Ok(x) -> do
        println(x)
        println(LA.norm(a @ x - b))
    | Err(error) -> println(error)
~~~

第一行是 `2x₀ + x₁ = 5 m`，第二行是 `x₀ + 3x₁ = 10 m`，解为 `[1 m, 3 m]`。未知向量、残差和容差都带长度量纲。改数据前先对文件运行 `aevia test`。

## 诊断求解结果

求解返回错误时，先检查输入形状与矩阵的秩，再考虑容差。求解成功而残差较大时，检查矩阵方向、各行缩放、观测精度。即使残差小，各行近相关时仍需检查条件性，小观测扰动可能造成明显不同的位移。

## 比较两个系统

把第二行换为 `[4.0, 2.0]`，先预测第二条方程增加了多少信息。再改为 `[4.0, 2.000001]`，将第二条观测变动一微米，比较所得解。说明哪个输出衡量拟合，哪个变化反映敏感程度。 [张量运算](06-tensors.md)
