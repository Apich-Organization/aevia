# 附录 B — 诊断目录

诊断是语言体验的一部分。Aevia 的错误信息 MUST 包含：代码、主位置、期望/实际、
以及至少一条可执行的修正建议（如果能确定）。本附录列出错误码、严重级别和
消息格式。

## B.1 格式

```
error[E0401]: dimension mismatch
  --> src/kinematics.aev:7:13
   |
 7 |     sqrt(2 * h * g)
   |              ^^^^^ expected Real[T^2], found Real[L^2/T^2]
   |
   = left:  L^2/T^2
   = right: T^2
   = difference: L^2/T^4
help: did you mean `2 * h / g`?
```

- 第一行：`severity[code]: message`。
- `-->`：文件:行:列。
- 源码行 + 下划线标签；标签尽量短。
- `=`：附注（规范形、差异、相关声明）。
- `help:`：修正建议；最多两条，按置信度排序。
- 多位置错误用多个 `-->` 或 `= note: ...` 指向定义处。

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
| `E0105` | invalid character | 删除/替换 |
| `E0106` | invalid numeric literal | 检查下划线/指数 |
| `E0107` | invalid superscript sequence | 用 `^n` |
| `E0108` | expected a unit after numeric literal | 写 `3 * x` 或补单位 |

## B.4 语法错误（E02xx）

| 代码 | 消息 | 典型修复 |
|---|---|---|
| `E0201` | empty block | 加一个表达式 |
| `E0202` | syntax error | 检查附近记号 |
| `E0203` | ragged tensor literal | 使每层等长 |
| `E0204` | invalid pattern | 检查构造器/字段 |
| `E0205` | invalid operator section | 用 `(+)` 或 lambda |
| `E0206` | reserved word used as identifier | 改名 |
| `E0207` | ambiguous type application | 在 `<` 前加空格或加括号 |
| `E0208` | duplicate field | 删除重复字段 |
| `E0209` | invalid unit literal | 检查单位表达式 |
| `E0210` | unknown attribute | 检查属性名/参数 |

## B.5 类型错误（E03xx）

| 代码 | 消息 | 典型修复 |
|---|---|---|
| `E0301` | unbound name | 导入/拼写 |
| `E0302` | type mismatch | 检查标注/参数 |
| `E0303` | infinite type (occurs check) | 加标注/改递归 |
| `E0304` | ambiguous type | 加类型标注 |
| `E0305` | not a function | 检查调用 |
| `E0306` | missing record field | 补字段/改类型 |
| `E0307` | constructor arity mismatch | 补/删参数 |
| `E0308` | pattern type mismatch | 改模式 |
| `E0309` | non-exhaustive match | 补分支 |
| `E0310` | no instance for class | 导入实例/加实例 |
| `E0311` | overlapping instances | 合并/删除 |
| `E0312` | non-positive recursive type | 移到箭头右侧 |
| `E0313` | IO in pure context | 用 `do`/返回 IO |
| `E0314` | cannot infer rank | 加 `Tensor<n, ...>` 标注 |
| `E0315` | wrong number of arguments | 补/删参数 |
| `E0316` | duplicate definition | 改名/删除 |
| `E0317` | cannot derive instance | 手写实例 |

## B.6 量纲错误（E04xx）

| 代码 | 消息 | 典型修复 |
|---|---|---|
| `E0401` | dimension mismatch | 使两边同量纲 |
| `E0402` | dimension used as exponent | 指数必须无量纲 |
| `E0403` | transcendental function requires dimensionless argument | 除以单位 |
| `E0404` | dimensionful base with runtime exponent | 字面量指数/`pow_dyn` |
| `E0405` | invalid affine point operation | 用温差/`lerp` |
| `E0406` | bare `0` used where a dimensioned value is expected | 写 `0 m` |
| `E0407` | unknown dimension/alias | 声明/导入 |
| `E0408` | unsatisfiable dimension constraint | 检查公式 |
| `E0409` | dimension occurs check | 简化量纲 |
| `E0410` | dimensionful `det`/`product` requires `Dyn<ρ>` | 除以单位/`det_unit` |
| `E0411` | inconsistent tensor element dimensions | 统一元素量纲 |
| `E0412` | `Dyn` dimension check failed (runtime) | 检查运行时量纲 |

警告：

| 代码 | 消息 |
|---|---|
| `W0301` | unresolved dimension defaulted to 1 |
| `W0302` | `Real`/`Float` mix uses the exact `Real` path; performance hint |

## B.7 单位错误（E05xx）

| 代码 | 消息 | 典型修复 |
|---|---|---|
| `E0501` | unknown unit | 导入/拼写 |
| `E0502` | unit not imported | `open Units.SI` |
| `E0503` | prefix cannot be applied | 用显式单位 |
| `E0504` | affine offset dimension mismatch | 检查 offset |
| `E0505` | duplicate base unit | 删除重复 |
| `E0506` | unit definition cycle/scale not dimensionless | 检查定义 |
| `E0507` | prefix unit conflicts with explicit unit | 改名/删除 |
| `E0508` | affine unit in product/power | 单独使用 |
| `E0509` | `to` unit dimension mismatch | 换单位 |
| `E0510` | affine display on a temperature difference | 用 `delta_to` |

## B.8 模块错误（E06xx）

| 代码 | 消息 |
|---|---|
| `E0601` | module name does not match path |
| `E0602` | cyclic import |
| `E0603` | ambiguous opened name |
| `E0604` | orphan instance |
| `E0605` | invalid package manifest |
| `E0606` | dependency version conflict |
| `E0607` | module/package not found |
| `E0608` | missing/invalid entry point |

## B.9 效果错误（E07xx）

| 代码 | 消息 |
|---|---|
| `E0701` | `do` block requires `IO` |
| `E0702` | `?` used outside `Result`/`Option` |

## B.10 运行时错误（E08xx）

| 代码 | 消息 | 可恢复 |
|---|---|---|
| `E0801` | division by zero | 精确：否；浮点：IEEE |
| `E0802` | shape/index error | 否 |
| `E0803` | free symbols in `approx` | 是 |
| `E0804` | cannot decide symbolic condition | 否 |
| `E0805` | stack overflow | 否 |
| `E0806` | domain error | 是 |
| `E0807` | numerical method did not converge | 是 |
| `E0808` | budget exhausted | 是 |
| `E0809` | internal type/dimension invariant violated | 否 |
| `E0810` | IO error | 是 |
| `E0899` | panic | 否 |

## B.11 IO 错误（E09xx）

| 代码 | 消息 |
|---|---|
| `E0901` | file not found |
| `E0902` | permission denied |
| `E0903` | parse error in data file |
| `E0904` | unsupported format |
| `E0905` | plot rendering failed |

## B.12 警告（W0xxx）

| 代码 | 消息 |
|---|---|
| `W0101` | unused variable |
| `W0102` | unused import |
| `W0103` | shadowed binding |
| `W0201` | redundant match arm |
| `W0202` | discarded IO result |
| `W0203` | unreachable code |
| `W0301` | unresolved dimension defaulted to 1 |
| `W0302` | `Real`/`Float` mix uses the exact `Real` path; performance hint |
| `W0303` | unused symbol |
| `W0501` | display unit dropped |
| `W0601` | unused private declaration |

## B.13 消息写作规范

1. **说人话。** `expected Real[L], found Real[T]` 比 `type unification failed`
   好。
2. **指小范围。** 下划线覆盖最小错误子树，不要整行。
3. **给差异。** 量纲错误列出 `left`、`right`、`difference` 的规范形。
4. **给一个修正。** 只在置信度高时给 `help`；不要列五个猜测。
5. **不要重复。** 同一错误在多个位置只报一次，其余用 `note` 指向主位置。
6. **保留源码。** 错误信息中的源码行不截断运算符；必要时换行。
7. **可解析。** 代码和位置稳定，便于 IDE/CI 解析。

## B.14 示例：类型错误

```
error[E0302]: type mismatch
  --> src/analysis.aev:12:18
   |
12 |     let m = mean(xs) + 1
   |                      ^ expected Real[L], found Int
   |
   = `mean(xs)` has type Real[L]
help: use `1 m` if you meant one metre
```

## B.15 示例：单位错误

```
error[E0501]: unknown unit `kms`
  --> src/travel.aev:3:14
   |
 3 | let d = 3.2 kms
   |              ^^^ not a unit
   |
help: did you mean `km`?
```

## B.16 示例：运行时形状错误

```
error[E0802]: shape mismatch in `matmul`
  --> src/linalg.aev:8:13
   |
 8 |     let C = A @ B
   |             ^^^^^ left shape [3, 4], right shape [3, 2]
   |
   = matmul requires left columns = right rows
help: transpose B or reshape A
```

## B.17 扩展/后端错误（E10xx）

| 代码 | 消息 | 说明 |
|---|---|---|
| `E1001` | backend capability missing | 携带 capability 名与最小 RSSN 版本 |
| `E1002` | ambiguous operator/overload | 列出候选签名 |
| `E1003` | no matching operator/overload | 列出候选签名/动态入口 |
| `E1004` | incompatible RSSN api_version | 要求升级/降级 |
| `E1005` | manifest/signature hash mismatch | 重新生成 `Rssn` 模块 |
| `E1006` | plugin ABI/load failure | 检查插件签名/版本 |

这些是**环境/后端**错误，不是用户程序类型错误；诊断必须给出可执行的
替代方案（例如“安装带 `core.gpu` 的 RSSN”或“用 `Rssn.dynamic`”）。

下一附录记录规范的修订历史。
