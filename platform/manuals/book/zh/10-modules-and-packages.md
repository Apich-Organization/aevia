# 模块与包

项目会从一个文件发展为一组命名模块。模块把实现同一目标的代码放在一起，包说明这些模块如何构建与共享。模块的公开导出是项目其他部分依赖的接口。

## 从两个文件开始

在空目录中新建 `Geometry/Shapes.aev` 和 `main.aev`。在 `Geometry/Shapes.aev` 中写入以下代码。

```aevia,file=Geometry/Shapes.aev
module Geometry.Shapes

fn rectangle(width: Real[Length], height: Real[Length]): Real[Length^2] =
    width * height
```

然后在 `main.aev` 中写入以下代码。

```aevia,file=main.aev
import Geometry.Shapes as Shapes

fn main(): IO Unit =
    println(Shapes.rectangle(3 m, 4 m))
```

在此目录运行 `aevia run main.aev`。单独运行的程序可以从入口所在的目录解析导入。保持各级路径的名字和大小写一致。包的导入树来自其源码目录和依赖。

## 创建包

运行 `aevia init specimen`。生成的 `aevia.toml` 定义包名和入口文件。用 `aevia check --all` 检查包，用 `aevia test` 运行测试，用 `aevia doc --out docs` 将公开接口的 `##` 注释渲染成文档。将多文件示例作为独立的运行入口，并说明各自所需的输入。

两个包并行开发时，可以使用本地路径依赖。当协作者需要复现依赖版本时，在应用中保存 `aevia.lock`。离线构建前先获取锁定的注册表内容。用 `aevia --locked check --all` 让意外的依赖解析变动导致检查失败。

## 做好共享准备

用物理含义描述每个公开参数，说明失败条件，并给出带预期结果的小示例。`aevia publish --dry-run` 打包并检查，但不会上传。发布需要使用已注册的签名密钥。[注册表文档](/docs?lang=zh)展示已发布的版本及生成的 API 页面。
