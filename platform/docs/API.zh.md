# 注册中心 HTTP API

[English](API.md)

以下路径均相对于 `https://aevia.apich.org`。请求和响应通常使用 JSON。稀疏索引、公钥和签名回执使用纯文本，包归档使用 gzip。注册中心错误包含 `error.code` 和 `error.message`。

```json
{"error": {"code": "E_SIGNATURE", "message": "the OpenPGP signature does not verify …"}}
```

| 代码 | 状态 | 含义 |
|---|---|---|
| `E_NAME` | 400 | 包标识不是有效的 `owner/name`，或名称不符合规则 |
| `E_VERSION` | 400 | 版本不符合 SemVer 或含 `+build` 元数据。接受 `1.2.3` 和 `1.2.3-rc.1` |
| `E_DEPENDENCY` | 400 | 依赖无效，或 `/resolve` 请求格式有误 |
| `E_RESOLVE` | 422 | 依赖要求无法同时满足，`message` 给出原因 |
| `E_INTEGRITY` | 500 | 存储的归档与目录和签名中的校验值不一致 |
| `E_MANIFEST` | 400 | 归档中的 `aevia.toml` 无法解析，或声明了不同的包 |
| `E_TARBALL` | 400 或 413 | 归档格式有误、超过大小限制或与已签名的摘要不一致 |
| `E_STATEMENT` | 400 | 声明格式有误、已过期、类型不符，或随机码已用过 |
| `E_SIGNATURE` | 403 | 签名验证失败 |
| `E_KEY` | 403 | 密钥格式有误或尚未绑定到账号 |
| `E_FORBIDDEN` | 403 | 签署者无权在该命名空间发布 |
| `E_NOT_FOUND` | 404 | 包、版本或密钥不存在 |
| `E_CONFLICT` | 409 | 版本已发布或密钥已绑定 |
| `E_DB`、`E_IO` | 500 | 注册中心未能完成请求 |
| `E_RATE_LIMIT` | 429 | 按 `Retry-After` 指定的间隔重试 |
| `E_PUBLISH_BUSY`、`E_PLAYGROUND_BUSY` | 429 | 发布或教程运行服务繁忙 |
| `E_PLAYGROUND_UNAVAILABLE` | 503 | 教程运行服务不可用 |

## 读取包与版本

### `GET /api/v1/stats`

```json
{"packages": 12, "versions": 31, "downloads": 4096, "publishers": 7}
```

### `GET /api/v1/search?q=&sort=&page=`

`sort` 接受 `relevance`、`downloads`、`recent` 和 `name`。页码从 1 开始，每页 20 项。

```json
{"total": 12, "page": 1, "per_page": 20, "query": "units", "sort": "recent",
 "items": [{"id": "apich/units-extra", "description": "Extra unit systems …",
            "version": "0.2.0", "updated_at": "2026-11-08T11:50:50Z",
            "downloads": 3, "has_docs": true}]}
```

### `GET /api/v1/crates/{owner}/{name}`

返回包页面的数据，包括 `id`、`description`、`license`、`repository`、`keywords`、`readme_html`、`versions[]`、`selected`、`owners[]`、`dependencies[]`、`created_at`、`updated_at`、`downloads` 和 `can_manage`。

### `GET /api/v1/index/config.json`

```json
{"dl": "https://aevia.apich.org/api/v1/download",
 "api": "https://aevia.apich.org/api/v1", "registry": "aevia"}
```

### `GET /api/v1/index/{owner}/{name}`

响应为 `text/plain`，按发布时间从早到晚逐行返回 JSON 记录。同一版本以最新记录为准。撤回操作会更新该版本的 `yanked` 字段。客户端须忽略未知字段。

```json
{"name":"apich/plot","vers":"2.1.0","deps":[
   {"package":"apich/units","req":"^1.2"},
   {"package":"apich/linalg","req":">=0.3, <0.5","import":"Matrix"}],
 "cksum":"sha256:7cb292ee…","yanked":false,"published_at":"2026-11-08T11:50:50Z",
 "docs":true,"size":10240,
 "signature":{"kind":"openpgp","fingerprint":"7AECB528…","signature":"-----BEGIN PGP SIGNATURE-----…"}}
```

每条依赖中的 `package` 为小写 `owner/name`，`req` 为规范化版本要求，例如 `^1.2`、`~1.2.3`、`>=1.2, <2`、`1.*`、`=1.2.3` 或 `*`。导入根不同于包的默认值时，`import` 给出实际名称。包 `units-extra` 的默认导入根为 `UnitsExtra`。 [模块与依赖规范](../../spec/07-modules.md)

将索引的 `ETag` 随 `If-None-Match` 发回。目录记录未变时，服务器返回 304。响应的 `Cache-Control` 为 `no-cache`，客户端重新验证索引条目时即可看到撤回。普通命令行解析可以复用本地条目，`aevia pkg update` 会刷新所选的缓存索引。

索引提供签名密钥和归档摘要。校验签名所需的原始声明可从签名回执获取。解析器选择满足要求的最高版本，默认每个兼容系列最多选一个版本，例如 `1.x`、`0.3.x` 或 `0.0.4`。仅当某项要求明确指定相同 `major.minor.patch` 的预发布版本时，该预发布版本才符合要求。十进制预发布标识符按数值排序，超过机器整数范围时也遵循这一规则。已锁定的版本在撤回后仍可选用。

### `GET /api/v1/crates/{owner}/{name}/dependents`

返回每个依赖此包的包中最新未撤回的版本。`dependents` 按名称排序并最多返回 100 项。`total` 统计所有不同的依赖包。

```json
{"package": "apich/units", "total": 2,
 "dependents": [{"package": "apich/plot", "version": "2.1.0", "req": "^1.2"}]}
```

## 解析依赖

`POST /api/v1/resolve` 在服务器端解析根依赖和可选的锁定版本。默认 `unify` 为 `series`，允许不同兼容系列并存。若需要每个包只选一个版本，使用 `package`。优先选用已锁定的版本，包括已撤回的锁定版本。

```json
{"dependencies": [{"package": "apich/plot", "req": "^2", "import": "Plot"}],
 "locked": [{"package": "apich/units", "version": "1.1.0"}],
 "unify": "series"}
```

成功时返回 HTTP 200。

```json
{"packages": [{"package": "apich/plot", "version": "2.1.0", "checksum": "sha256:…", "yanked": false}],
 "edges": [{"from": "request", "import": "Plot", "to": "apich/plot 2.1.0"}]}
```

输入格式有误时返回 `400 E_DEPENDENCY`。要求存在冲突时返回 `422 E_RESOLVE`，原因见 `message`。

## 下载与核验

### `GET /api/v1/download/{owner}/{name}/{version}`

支持通过 `If-None-Match` 发起条件请求。归档响应包含以下标头。

| 标头 | 值 |
|---|---|
| `X-Aevia-Checksum` | `sha256:<hex>` |
| `X-Aevia-Signature-Fingerprint` | 签名密钥指纹 |
| `ETag` | 用双引号包裹的校验值 |
| `Cache-Control` | `public, max-age=31536000, immutable` |

### `GET /api/v1/download/{owner}/{name}/{version}/signature`

以 `text/plain` 返回发布声明的签名原文，紧接 ASCII 装甲格式的分离签名。定位第一行装甲标记即可分离两部分。验证时保留声明的原始字节，包括最后一个字段后的换行。

### `GET /api/v1/keys?fingerprint=…&kind=openpgp|ssh`

```json
{"registered": true, "publisher": {"handle": "apich", "kind": "github", "display_name": "…"}}
```

`kind` 可省略。SSH 指纹采用 `SHA256:<base64>`，其中可能含有 `/`。请在查询参数中传递指纹。

### `GET /api/v1/keys/public?fingerprint=…`

以 `text/plain` 返回装甲公钥。已停用的公钥保留以供历史签名验证。`/keys` 报告当前绑定状态。

### `GET /api/v1/log?from=0&limit=100`

```json
{"head": {"seq": 3, "hash": "a6261193…", "at": "2026-11-08T12:01:00Z"},
 "entries": [{"seq": 1, "kind": "publish", "summary": "apich published apich/units-extra 0.2.0",
              "hash": "…", "prev": "0000…", "at": "…"}]}
```

`kind` 为 `publish`、`yank` 或 `key`。条目按序号升序返回，从 `from` 指定的序号开始，包含该序号。`limit` 默认为 100，取值限制在 1 至 500。

`head` 和 `entries` 来自同一次数据库快照。请求超出日志末尾时仍返回当时的链头。空日志返回 `head: null`。

## 发布和撤回

### `POST /api/v1/publish/challenge`

摘要格式为 `sha256:` 加 64 位十六进制字符串。用包名称、版本和摘要请求待签名声明。

```json
{"name": "apich/units-extra", "version": "0.2.0", "checksum": "sha256:<64 hex>"}
```

```json
{"nonce": "4d1a…", "statement": "aevia-publish-v1\nname: …\nexpires: 1794000300\n",
 "expires_at": "2026-11-08T12:05:00Z"}
```

对返回的 `statement` 原始字节签名，保留最后一个换行。

### `POST /api/v1/publish`

请求采用 `multipart/form-data`。

| 部分 | 类型 | 内容 |
|---|---|---|
| `envelope` | `application/json` | 声明、分离签名、公钥和可选发布元数据 |
| `package` | `application/gzip` | 与签名摘要一致的 `.tar.gz` 文件 |

```json
{"statement": "<the exact text>", "signature": "<armoured detached signature>",
 "key": {"kind": "openpgp", "armored": "<armoured OpenPGP public key>"},
 "description": "optional", "license": "Apache-2.0", "keywords": ["units"], "repository": "https://…"}
```

成功时返回 HTTP 201。

```json
{"name": "apich/units-extra", "version": "0.2.0", "checksum": "sha256:…",
 "docs_url": "/docs/apich/units-extra/0.2.0", "log": {"seq": 1, "hash": "…"},
 "warnings": []}
```

`ssh` 类型仅接受 `ssh-ed25519` 公钥。SSH 签名成功时，`warnings` 会建议改用 OpenPGP，优先考虑硬件密钥。使用 OpenPGP 时，响应可以省略空的 `warnings`，也可以返回 `warnings: []`。

服务端合并信封、归档中的 `aevia.toml` 和 `aevia-pkg.toml` 所提供的元数据。发布包须含描述，以及许可表达式或非空许可文件。文档排队期间访问 `docs_url` 返回 202。

注册中心读取 `[package]` 和 `[dependencies]`，并接受 `[lints]` 表。lint 配置由工具链验证。归档入口取自 `[package] entry`，默认是 `src/main.aev`。发布清单中的 `[lib]` 表会返回 `E_MANIFEST`。

服务端在写入前检查 `aevia.toml` 中的依赖。依赖须指向注册中心中的其他包，每项版本要求须有至少一个未撤回版本可满足。本地 `path` 依赖和自依赖返回 `E_DEPENDENCY`。归档持久化后，服务端在同一事务中写入目录、依赖和透明日志。

### `POST /api/v1/yank/challenge` 和 `POST /api/v1/yank`

提交 `name`、`version` 和 `yanked` 获取声明。设为 `yanked: true` 执行撤回，设为 `yanked: false` 恢复版本。响应字段与发布质询相同。对声明签名后，将原始声明、分离签名和公钥提交至撤回端点。

```json
{"statement": "<the exact text>", "signature": "<armoured>", "key": {"kind": "openpgp", "armored": "…"}}
```

成功时返回 204。撤回标志及相应透明日志在同一事务中更新。撤回的版本不参与新的依赖求解。已锁定的版本仍可选用，归档和校验值也仍可获取。

## 绑定签名密钥

### `POST /api/v1/auth/device`

```json
{"kind": "openpgp", "public_key": "<armoured OpenPGP public key>", "label": "work laptop"}
```

建议使用 OpenPGP，优先考虑硬件密钥。SSH 仅支持 `ssh-ed25519`，其他 SSH 密钥返回 `403 E_KEY`。SSH 请求的 `warnings` 会提示改用 OpenPGP 和硬件密钥。

```json
{"device_code": "…", "user_code": "ABCD-EFGH",
 "verification_uri": "https://aevia.apich.org/activate?code=ABCD-EFGH",
 "expires_at": "2026-11-08T12:15:00Z", "interval": 5,
 "warnings": []}
```

在浏览器打开验证地址，登录并核对密钥指纹，再确认绑定。密钥已绑定时返回 409。

### `GET /api/v1/auth/device/{device_code}`

```json
{"status": "pending"}
{"status": "approved", "publisher": {"handle": "apich", "kind": "github", "display_name": "…"}}
```

`status` 可为 `pending`、`approved`、`denied` 或 `expired`。每隔 `interval` 秒轮询，直到状态不再为 `pending`。若获批准，响应包含 `publisher`。

## 运行教程代码

配置运行服务后，`POST /api/v1/playground/run` 接受仅含 `source` 字段的 JSON 对象，最多 16 KiB Aevia 源码。每次执行使用一个新建的受限容器。

```json
{"source":"fn main(): IO Unit = do\n    println(\"Hello\")\n"}
```

响应包含 `status`、`stdout` 和 `stderr`。`status` 为 `success`、`failed`、`timeout` 或 `output_limit`。达到并发容量或请求速率限制时返回 429，执行服务不可用时返回 503。 [运维配置](DEPLOY.zh.md#启用教程运行功能)

## 浏览器页面

| 路径 | 内容 |
|---|---|
| `/` | 首页 |
| `/crates?q=&sort=&page=` | 按包名、描述和关键字搜索 |
| `/cli` | 命令行工具和注册中心使用指南 |
| `/users/{handle}` | 发布者公开信息、加入时间和软件包 |
| `/feed.xml` | 最近 50 个版本的 Atom 订阅 |
| `/sitemap.xml`、`/robots.txt` | 爬虫入口。禁止爬取 `/account`、`/auth` 和 `/api` |
| `/badge/{owner}/{name}.svg` | 最新未撤回版本的徽标 |
| `/crates/{owner}/{name}` | 包页面 |
| `/crates/{owner}/{name}/{version}` | 包的指定版本 |
| `POST /crates/{owner}/{name}/yank` | 在浏览器撤回或恢复版本。需登录，表单含 `version` 和 `yanked` |
| `/docs` | 托管文档首页 |
| `/docs/{owner}/{name}` | 转到最近有文档的版本 |
| `/docs/{owner}/{name}/{version}` | 模块列表、README 和文档覆盖。文档排队时返回 202 |
| `/docs/{owner}/{name}/{version}/{module}` | 单个模块的文档 |
| `/account` | 密钥和软件包，需登录 |
| `POST /account/keys` | 添加密钥，表单含 `armored` 和 `label` |
| `POST /account/keys/{id}/revoke` | 撤销密钥 |
| `/login?next=` | 选择登录服务 |
| `/auth/{github,orcid}/start` | 启动 OAuth 登录 |
| `/auth/{github,orcid}/callback` | OAuth 回调 |
| `POST /logout` | 结束会话 |
| `/log` | 透明日志 |
| `/spec` | 语言规范目录 |
| `/spec/{slug}`，如 `/spec/04-dimensions` | 语言规范章节 |
| `/spec/all` | 单页语言规范，按需生成 |
| `/book`、`/architecture` | 教程和架构手册目录 |
| `/book/{slug}`、`/architecture/{slug}` | 相应语言的章节 |
| `/book/all`、`/architecture/all` | 单页手册 |
| `/book/download`、`/architecture/download` | 独立 HTML 下载 |
| `/stdlib` | 标准库参考 |
| `/lang/{code}` | 用 Cookie 保存 `en` 或 `zh` 语言偏好后返回当前页 |
| `/activate?code=` | 确认绑定密钥。GET 显示表单，POST 确认 |
| `/static/site.css`、`/static/app.js` | 网站资源 |
| `/healthz` | PostgreSQL 可用时返回 200，否则返回 503 |

## 语言偏好

页面语言依次由请求的 `?lang=`、`aevia_lang` Cookie 和 `Accept-Language` 决定，默认英语。`<html>` 的 `lang` 属性采用选定语言，以便浏览器正确选择字体和断行规则。

`GET /lang/zh` 保存语言偏好，并返回同源的 `Referer` 地址，删去该地址之前的 `lang` 参数。通过会话执行的浏览器写操作需要通过来源检查。

## 签名声明

以下是发布与撤回声明的规范形式。按所示顺序为每个字段写一行，每个字段只出现一次。每行以 LF 换行，包括最后一行。签名时直接使用服务端返回的完整声明。

```
aevia-publish-v1
name: apich/units-extra
version: 0.2.0
checksum: sha256:7cb292ee…
nonce: 4d1a…
expires: 1794000300
```

```
aevia-yank-v1
name: apich/units-extra
version: 0.2.0
yanked: true
nonce: 4d1a…
expires: 1794000300
```

声明有效期为五分钟。第一个携带随机码的请求会消耗该随机码，即使随后未通过其他检查。
