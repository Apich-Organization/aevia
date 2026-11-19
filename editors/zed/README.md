# Aevia for Zed / Aevia 的 Zed 扩展

Install the extension from this checkout by selecting `editors/zed` in
**zed: install dev extension**. Zed loads the pinned grammar from the
repository. To test a local grammar revision, run
`pwsh editors/zed/scripts/prepare-dev.ps1` and select the staging directory it prints.

从本地检出安装时，在 **zed: install dev extension** 选择 `editors/zed`。
Zed 从仓库读取指定版本的语法。若要先试用本地语法改动，可运行
`pwsh editors/zed/scripts/prepare-dev.ps1`，并选择脚本输出的暂存目录。

With an Aevia source file open, **task: spawn** lists **Run Aevia file**,
**Test Aevia file**, and **Aevia REPL**. Use the gutter runnables for `fn main`
and `test` declarations. Open a project at its Aevia package root to run tasks
with relative data paths.

打开 Aevia 源文件后，**task: spawn** 中有 **Run Aevia file**、
**Test Aevia file** 和 **Aevia REPL**。也可使用 `fn main` 和 `test` 声明旁的
运行图标。运行依赖相对数据路径的任务时，请从 Aevia 包的根目录打开项目。

Choose **Aevia** in the debugger's launch menu, or add a scenario to
`.zed/debug.json`. To resolve a source file from the worktree root, pass its
relative path in `program`.

在调试启动菜单中选择 **Aevia**，或将配置加入 `.zed/debug.json`。
`program` 可以填写相对于当前项目根目录的源文件路径。

```json
[
  {
    "label": "Debug current Aevia file",
    "adapter": "aevia",
    "request": "launch",
    "program": "$ZED_FILE",
    "stopOnEntry": true
  }
]
```
