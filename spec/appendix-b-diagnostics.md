# 附录 B — 诊断目录

诊断是语言体验的一部分。错误信息 MUST 包含代码、主位置、期望/实际，并在能确定时给出至少一条可执行的修正建议。
本附录列出错误码、严重级别与消息格式；表中的每个代码都在编译器中有发射点（`grep` 全部 crate 核对），例外在“状态”说明中注明。

## B.1 格式

```
error[E0401]: dimension mismatch
  --> src/kinematics.aev:4:31
   |
 4 | fn f(h: Float[L]): Float[T] = sqrt(2 * h * g)
   |                               ^^^^^^^^^^^^^^^ expected T, found L/T
   |
   = left:  L/T
   = right: T
   = difference: L/T^2
```

- 第一行 `severity[code]: message`；`-->` 给出文件:行:列；其后是源码行与下划线标签（尽量短）。
- `=`：附注（规范形、差异、相关声明）；`help:`：修正建议，至多两条，按置信度排序。
- 多位置错误用多个下划线标签或 `= note:` 指向定义处。

## B.2 严重级别

| 级别 | 含义 |
|---|---|
| `error` | 程序不能编译/运行 |
| `warning` | 程序可运行，但可能有问题 |
| `note` | 解释性信息，不单独出现 |

## B.3 词法错误（E01xx）

| 代码 | 消息 | 典型修复 |
|---|---|---|
| `E0101` | tab character in indentation | 用空格替换 |
| `E0102` | inconsistent indentation | 对齐到某一层 |
| `E0103` | expected an indented block | 缩进下一行 |
| `E0104` | unclosed string/comment/bracket | 补闭合 |
| `E0105` | invalid character / escape | 删除/替换 |
| `E0106` | invalid numeric literal | 检查下划线/指数 |
| `E0107` | invalid superscript sequence | 用 `^n` |
| `E0108` | expected a unit after the numeric literal | 写 `3 * x` 或补单位 |

## B.4 语法错误（E02xx）

| 代码 | 消息 | 典型修复 |
|---|---|---|
| `E0201` | empty block | 加一个表达式 |
| `E0202` | syntax error（含 `private` 用在不允许的声明上；运行时 `_` 出现在调用之外） | 检查附近记号 |
| `E0203` | ragged tensor literal | 使每层等长 |
| `E0204` | invalid pattern | 检查构造器/字段 |
| `E0205` | invalid operator section | 用 `(+)` 或 lambda |
| `E0206` | reserved word used as identifier | 改名 |
| `E0207` | ambiguous type application | 在 `<` 前加空格或给 callee 加括号 |
| `E0208` | duplicate field | 删除重复字段 |
| `E0209` | invalid unit literal（含 `3 (2)`：括号内不是单位表达式） | 检查单位表达式 |
| `E0210` | unknown attribute | 检查属性名/参数 |

## B.5 类型错误（E03xx）

| 代码 | 消息 | 典型修复 |
|---|---|---|
| `E0301` | unbound name | 导入/拼写 |
| `E0302` | type mismatch（含类型级 `Nat` 方程无解、未知 capability 名、`to_system` 的名字不是字面量） | 检查标注/参数 |
| `E0303` | infinite type (occurs check) | 加标注/改递归 |
| `E0304` | ambiguous type（含未能解出的 `Nat` 变量、非线性 `Nat` 方程、无 binder 的 shape 变量、缺少 `from_system<D>`） | 加类型标注 |
| `E0305` | not a function | 检查调用 |
| `E0306` | missing record field | 补字段/改类型 |
| `E0307` | constructor arity mismatch | 补/删参数 |
| `E0308` | pattern type mismatch（含 `Prop` 作守卫） | 改模式 |
| `E0309` | non-exhaustive match | 补分支 |
| `E0310` | no instance for class | 导入实例/加实例 |
| `E0311` | overlapping instances | 合并/删除 |
| `E0312` | non-positive recursive type | 移到箭头右侧 |
| `E0313` | IO action in a pure context | 用 `do`/返回 `IO` |
| `E0314` | cannot infer rank/element type（空张量、`read_npy` 未给秩） | 加 `Tensor<n, …>` 标注 / `read_npy<2>` |
| `E0315` | wrong number of arguments | 补/删参数 |
| `E0316` | duplicate definition | 改名/删除 |
| `E0317` | cannot derive instance / unsupported parameter kind | 手写实例 |
| `E0318` | function is less generic than its declared type parameters | 让函数体对任意选择成立，或把参数换成函数体需要的具体类型 |

## B.6 量纲错误（E04xx）

| 代码 | 消息 | 典型修复 |
|---|---|---|
| `E0401` | dimension mismatch | 使两边同量纲 |
| `E0402` | exponent must be a dimensionless number / finite | 指数必须无量纲（精确相位的无穷指数也是此码，§6.3.4） |
| `E0403` | transcendental function requires dimensionless argument | 除以单位 |
| `E0404` | dimensionful base with a run-time exponent | 字面量指数/`pow_dyn` |
| `E0405` | invalid affine point operation | 用温差/`lerp` |
| `E0406` | bare `0` used where a dimensioned value is expected | 写 `0 m` |
| `E0407` | unknown dimension/alias | 声明/导入 |
| `E0408` | unsatisfiable dimension constraint | 保留码 |
| `E0409` | dimension occurs check | 保留码 |
| `E0410` | dimensionful `det`/`product` requires `Dyn<ρ>` | 除以单位/`det_unit` |
| `E0411` | incompatible tensor element types or dimensions | 统一元素类型/量纲 |
| `E0412` | `Dyn`/kernel dimension check failed（运行时） | 检查运行时量纲 |

状态：`E0408`、`E0409` 是保留码，实现中不可达：量纲统一是自由阿贝尔群上的指数线性统一，含非零系数变量的残差总可解，唯一的失败是
非零常量残差，即 `E0401`；也不会出现 occurs 状态（证明见 `aevia-types/tests/diagnostics_coverage.rs`）。

## B.7 单位错误（E05xx）

| 代码 | 消息 | 典型修复 |
|---|---|---|
| `E0501` | unknown unit（含叠加前缀 `kkm`、先用后声明） | 导入/拼写 |
| `E0502` | unit not imported | 保留码 |
| `E0503` | prefix cannot be applied | 保留码 |
| `E0504` | affine offset dimension mismatch | 检查 offset |
| `E0505` | duplicate unit | 删除重复 |
| `E0506` | the scale has no exact value for that exponent | 检查定义/指数 |
| `E0507` | prefixed unit conflicts with explicit unit | 保留码 |
| `E0508` | affine unit in product/power | 单独使用 |
| `E0509` | `to`/`value_in` unit measures a different dimension | 换单位 |
| `E0510` | affine unit used on a temperature difference | 用 `delta_to` |
| `E0511` | unit has no exact scale in the module's unit system（§5.11.4） | 写单位制的数，或 `to_system` |
| `E0512` | unknown unit system（§5.11.1） | 用 kernel 提供的名字（natural、planck、atomic、geometrized） |

状态：`E0502`、`E0503`、`E0507` 是保留码：没有模块级单位命名空间（§5.9），对应状态不存在。

## B.8 模块错误（E06xx）

| 代码 | 消息 |
|---|---|
| `E0601` | module name does not match path |
| `E0602` | cyclic import |
| `E0603` | the same name opened by two modules with different types |
| `E0604` | orphan instance |
| `E0605` | invalid package manifest |
| `E0606` | dependency version conflict |
| `E0607` | module/package not found |
| `E0608` | no runnable module/entry point（嵌入 API） |

## B.9 效果错误（E07xx）

| 代码 | 消息 |
|---|---|
| `E0701` | `do` statement or `<-` expects an `IO` action |
| `E0702` | `?` requires a `Result`/`Option` and a matching return boundary |

## B.10 运行时错误（E08xx）

| 代码 | 消息 | 可恢复 |
|---|---|---|
| `E0801` | division by zero | 精确：否；浮点：IEEE |
| `E0802` | shape/index error | 否 |
| `E0803` | free symbols in `approx` | 是 |
| `E0804` | cannot decide symbolic condition / assumption is false | 否 |
| `E0805` | stack overflow / tail-call or IO budget exceeded | 否 |
| `E0806` | domain error | 是 |
| `E0807` | numerical method did not converge | 是 |
| `E0808` | budget exhausted | 是 |
| `E0809` | internal type/dimension invariant violated | 否 |
| `E0810` | IO error | 是 |
| `E0811` | task failure: a result that cannot cross threads, a task awaited twice, or a stopped worker（§9.7.3） | 是 |
| `E0899` | panic | 否 |

状态：个别内建函数借用 `E0806`–`E0809` 报告自己的参数/内部失败（例如 `approx_map`、`degree`），码的类别以上表为准，消息以内建
自述为准。

## B.11 IO 错误（E09xx）

| 代码 | 消息 |
|---|---|
| `E0901` | file not found |
| `E0902` | permission denied（含未授予 capability，§9.14） |
| `E0903` | parse error in data file |
| `E0904` | unsupported format |
| `E0905` | plot rendering failed |

## B.12 警告（W0xxx）

| 代码 | 消息 |
|---|---|
| `W0101` | unused variable |
| `W0102` | unused import（保留码） |
| `W0103` | shadowed binding |
| `W0201` | redundant match arm |
| `W0202` | discarded IO result |
| `W0203` | unreachable code（保留码） |
| `W0301` | unresolved dimension defaulted to 1 |
| `W0302` | `Real`/`Float` mix uses the exact `Real` path |
| `W0303` | unused symbol |
| `W0501` | display unit dropped（尚无发射点，§5.4.2） |
| `W0601` | unused private declaration |
| `W0602` | deprecated name used：声明上的 `@deprecated("...")`（§2.2.7/§13.8.1） |

状态：`W0102` 与 `W0203` 是保留码——检查器只看到打平的名字表，选择性导入与 `open M` 无法区分；语言没有 `return`/`break`，AST 无法表示
不可达语句。`W0501` 目前没有发射点。

## B.13 消息写作规范

1. **说人话。** `expected Real[L], found Real[T]` 好于 `type unification failed`。
2. **指小范围。** 下划线覆盖最小错误子树。
3. **给差异。** 量纲错误列出 `left`、`right`、`difference` 的规范形。
4. **给一个修正。** 只在置信度高时给 `help`，不列五个猜测。
5. **不要重复。** 同一错误只报一次，其余用 `note` 指向主位置。
6. **保留源码。** 错误信息中的源码行不截断运算符，必要时换行。
7. **可解析。** 代码和位置稳定，便于 IDE/CI 解析。

## B.14 示例：类型错误

```
error[E0302]: type mismatch
  --> t.aev:2:14
   |
 2 | let n: Int = 1.5
   |              ^^^ a bare numeric literal cannot have type Int
   |
```

## B.15 示例：单位错误

```
error[E0501]: unknown unit `kms`
  --> t.aev:2:9
   |
 2 | let d = 3.2 kms
   |         ^^^^^^^
   |
help: did you mean `km`?
```

## B.16 示例：运行时形状错误

```
error[E0802]: matmul requires left columns (2) = right rows (3)
  --> r.aev:5:13
   |
 5 |     let C = A @ B
   |             ^^^^^
   |
```

## B.17 扩展/后端错误（E10xx）

| 代码 | 消息 | 说明 |
|---|---|---|
| `E1001` | backend capability or witness missing | 携带 capability 名与最小 RSSN 版本；kernel 缺少量纲见证、回调句柄失效也用此码 |
| `E1002` | ambiguous operator/overload | 列出候选签名 |
| `E1003` | no matching operator/overload | 列出候选签名/动态入口；回调元数/形状不符也用此码 |
| `E1004` | incompatible RSSN api_version / kernel result fields | 要求升级/降级；kernel 缺少或多出已声明的记录字段 |
| `E1005` | no pinned overload / signature hash mismatch | 重新生成 `Rssn` 模块 |
| `E1006` | plugin ABI/load/registration failure | 检查插件签名/版本 |

这些是**环境/后端**错误，不是用户程序的类型错误；诊断必须给出可执行的替代方案（例如“安装带 `core.gpu` 的 RSSN”或“用
`Rssn.dynamic`”）。

下一附录记录规范的修订历史。
