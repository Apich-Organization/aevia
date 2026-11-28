# 部署 Aevia 平台

[English](DEPLOY.md)

注册中心使用 PostgreSQL 18 和持久化包归档，并需要连接已启用的身份提供商。进程先应用数据库迁移，再开放服务。典型部署使用 x86-64 或 ARM64 Linux、无根 Podman 和 Caddy。 [注册中心设计](../manuals/architecture/zh/05-registry.md) · [运维设计](../manuals/architecture/zh/06-operations.md)

## 启动注册中心

从 Aevia 仓库开始配置并构建镜像。主机需要一个 Podman Compose 提供程序。允许无根代理监听 TCP 80 和 443 端口。使用 HTTP/3 时还需要 UDP 443。配置公网 DNS，使 Caddy 能为站点签发证书。

```sh
cd platform/infra
umask 077
cp .env.example .env
openssl rand -hex 32   # AEVIA_SESSION_SECRET
openssl rand -hex 24   # POSTGRES_PASSWORD 及 DATABASE_URL 中的密码
${EDITOR:-vi} .env
./deploy.sh build
./deploy.sh up
./deploy.sh logs registry
```

从源码构建时，请确保 Bazel 可访问相邻的 RSSN 工作区。`./deploy.sh build` 将注册中心
镜像构建转交给 Bazel。首次执行 `./deploy.sh up` 时，如果本地没有注册中心镜像，会先用
同一目标构建再启动 Compose。在 Aevia 仓库根目录可以直接运行以下命令。

```sh
bazel run //:registry_image -- --tag localhost/aevia/registry:0.1.0
bazel build //:registry_server
bazel run //:registry_image_from_binary -- \
  --binary bazel-bin/registry_server/bin/aevia-registry \
  --tag localhost/aevia/registry:0.1.0
```

第一条命令构建注册中心并生成镜像。最后一条把已有的注册中心可执行文件打包成镜像，
例如使用上一条 Bazel 构建的产物。两种镜像目标均默认使用 Podman，也接受 `--engine docker`
和多个 `--tag` 参数。Compose 启动生成的 `localhost/aevia/registry:0.1.0` 镜像，并应用
数据库及内容挂载配置。

将 `AEVIA_DOMAIN` 设为公网域名，`AEVIA_BASE_URL` 设为对应的 HTTPS 源，`AEVIA_ACME_EMAIL` 设为有人关注的联系邮箱。Compose 中的数据库主机名是 `db`。主机可以通过 `127.0.0.1:8080` 访问内部注册中心监听端口。

Caddy 面向公网，管理 TLS、设置 HSTS 并压缩响应。注册中心直接设置浏览器安全响应头。Caddy 转发前会重写 `X-Forwarded-For`，供注册中心进行限流。仅在请求通过受控且会重写此标头的代理时启用 `AEVIA_TRUST_PROXY=true`。由 systemd 直接管理容器时，可使用 [Quadlet 单元](../infra/quadlet/README.md)。

```sh
curl -fsS https://aevia.apich.org/healthz
curl -fsSI https://aevia.apich.org/book
./deploy.sh ps
```

`/healthz` 检查 PostgreSQL 是否响应。数据库不可用时返回 HTTP 503。可用 `./deploy.sh logs db` 和 `./deploy.sh logs registry` 排查原因。

共享数据库的注册中心实例通过 PostgreSQL advisory lock 依次执行启动迁移。开放服务前，注册中心会校验 SQLx 记录的迁移校验和。如果启动时报告 `cannot apply the schema`，请在注册中心日志中定位失败的迁移，修复数据库问题后再重试。若校验和不一致，请使用与现有迁移历史匹配的二进制，或恢复与部署版本兼容的备份。

涉及数据库结构变更的升级应安排维护窗口。迁移 `0003_pg18_features` 会在事务结束前一直持有 `versions` 表的 `ACCESS EXCLUSIVE` 锁，阻塞对该表的读写。对现有数据库应用此迁移时，请先暂停注册中心流量。

## 配置登录

可以只启用 GitHub 或 ORCID，也可以同时启用。浏览无需登录。发布者需要账户和已绑定的签名密钥。

- 创建 GitHub App，将回调地址设为 `AEVIA_BASE_URL/auth/github/callback`。将客户端 ID 与密钥填入 `AEVIA_GITHUB_CLIENT_ID` 和 `AEVIA_GITHUB_CLIENT_SECRET`。以组织身份发布需要有效的组织成员身份与对应 App 安装的访问权限。请授予 App 读取组织成员身份的权限。
- 创建 ORCID OAuth 客户端，将回调地址设为 `AEVIA_BASE_URL/auth/orcid/callback`，并设置 `AEVIA_ORCID_CLIENT_ID` 和 `AEVIA_ORCID_CLIENT_SECRET`。使用沙箱客户端时设置 `AEVIA_ORCID_BASE=https://sandbox.orcid.org`。注册中心请求 `/authenticate` 权限。

提供商密钥和会话密钥保存在注册中心环境中。轮换 `AEVIA_SESSION_SECRET` 会使使用旧密钥签发的会话失效。初始化之后更换 PostgreSQL 密码时，同时修改数据库角色和应用连接字符串。数据库角色的密码需要在数据库中修改。

### 升级时绑定已有 GitHub 身份

迁移 `0006_github_identity` 保留发布者记录 ID、Aevia 命名空间、签名密钥和包所有权。它终止旧 GitHub 浏览器会话并移除按登录名记录的组织授权。升级后，必须独立核验 GitHub 数字 ID，才能登录已有的 GitHub 发布者账户。GitHub 登录名发生变更后，原有 Aevia 命名空间仍归原账户，即使后来有另一个 GitHub 账户使用旧登录名。

升级前停止所有旧注册中心实例。启动新版本时先禁用 GitHub 登录，逐条审核旧记录。根据可信历史账户记录或 App 安装记录，验证用户或组织的 GitHub 数字 ID。个人身份还应通过展示先前信任的发布签名密钥控制权，或既有的维护者联系方式进行确认。检查该发布者现有的签名密钥，并调查来源不明的绑定。仅凭当前 GitHub 登录名不足以证明旧命名空间的所有权。 [按 ID 查询 GitHub 账户](https://docs.github.com/en/rest/users/users#get-a-user-using-their-id)

只更新已经审核的记录。在 `psql` 中使用 `\set` 将 `registry_id`、`registry_kind`（`github` 或 `github-org`）、`registry_handle`、`verified_github_id` 设为核验过的值，然后开始事务。

```sql
BEGIN;
UPDATE publishers
SET github_id = :'verified_github_id'::bigint
WHERE id = :'registry_id'::bigint
  AND kind = :'registry_kind'
  AND kind IN ('github', 'github-org')
  AND handle = :'registry_handle'
  AND github_id IS NULL
RETURNING id, kind, handle, github_id;
```

确认只返回一条预期记录，再删除该发布者可能存在的浏览器会话并提交。返回记录不符、没有更新或提供商 ID 重复时，回滚并继续审核。未确认的旧身份保持禁止浏览器登录。恢复 GitHub 登录后，组织成员需要重新登录以取得新的授权。

```sql
DELETE FROM sessions WHERE publisher_id = :'registry_id'::bigint;
COMMIT;
```

## 更新文档

Compose 和 Quadlet 以只读方式挂载下列目录。注册中心定期检查变化，后台渲染完成后更新当前文档。请一起部署双语改动，并原子替换单个文件。

| Aevia 仓库中的来源 | 挂载位置 | 网页路径 |
| --- | --- | --- |
| `spec/` | `/spec` | `/spec` |
| `platform/stdlib-docs/` | `/stdlib-docs` | `/stdlib` |
| `platform/manuals/book/{en,zh}/` | `/manuals/book/{en,zh}` | `/book` |
| `platform/manuals/architecture/{en,zh}/` | `/manuals/architecture/{en,zh}` | `/architecture` |

手册的两个版本使用相同的章节文件名，每篇以一级标题开头并包含正文。`/book/download` 和 `/architecture/download` 提供对应语言的独立 HTML。已发布包的 API 文档由编译器根据签名归档生成，存入 PostgreSQL。`AEVIA_DOCS_ENABLED=false` 使后续发布跳过包文档生成。

## 验证公开发现入口

先将 `AEVIA_BASE_URL` 设为公共 HTTPS 源，再发布链接。读取 `/sitemap.xml`，核对章节 URL 的语言参数。检查章节的 `Link` 响应头，核对规范 URL 和译本链接。`/llms.txt?lang=zh` 提供 Markdown 章节及 `/stdlib/catalog.json`。只挂载 Markdown 标准库时提供渲染页面，并省略 JSON 目录。

将关联文件一起准备，通过目录内原子重命名进行切换。加载完成前保留各语言的完整版本。在预演部署中换入损坏内容，确认原文和目录仍可读，再换回有效内容。对热更新文档使用只读目录挂载，使重命名和文件更改都能被注册中心观察到。

## 启用教程运行功能

在专用 Linux 主机或专用无根 Podman 账户下运行 `aevia-playground`。环境需要 cgroup v2，
并为账户分配从属 UID 和 GID 范围。从仓库根目录构建工作进程，以及包含适用于该主机架构的 CLI 的镜像。

```sh
bazel build --config=release //:platform_binaries
bazel run //:playground_image -- --tag localhost/aevia/playground:release
```

打包已有的配套 CLI 时，使用 `bazel run //:playground_image_from_binary -- --aevia /path/to/aevia --tag localhost/aevia/playground:release`。

将 `bazel-bin/platform_binaries/bin/aevia-playground` 安装到工作账户的 `~/.local/bin/aevia-playground`，再建立权限为 `0600` 的 `~/.config/aevia/playground.env`。

```dotenv
AEVIA_PLAYGROUND_BIND=127.0.0.1:9081
AEVIA_PLAYGROUND_IMAGE=localhost/aevia/playground:release
AEVIA_PLAYGROUND_TOKEN=<openssl rand -hex 32 的输出>
```

将 [aevia-playground.service](../infra/playground/aevia-playground.service) 安装到该账户的 `~/.config/systemd/user/`，然后启动。

```sh
systemctl --user daemon-reload
systemctl --user enable --now aevia-playground.service
journalctl --user -u aevia-playground.service -f
```

在注册中心环境中将 `AEVIA_PLAYGROUND_TOKEN` 设为相同密钥。注册中心直接运行在同一台主机上时，可设置 `AEVIA_PLAYGROUND_URL=http://127.0.0.1:9081/run`。容器中或远端的注册中心应通过权限受控的 TLS 代理连接工作进程的回环监听端口，例如 `AEVIA_PLAYGROUND_URL=https://worker.example.org/run`。运行镜像位于工作账户自己的 Podman 存储中。如果在工作主机上使用 Caddy，可以在示例中设置 TLS 域名和注册中心所在的源网络。 [工作进程代理示例](../infra/playground/Caddyfile.example)注册中心无需访问该账户的 Podman 套接字、归档目录或数据库。

配置工作进程后，教程中标记为 `aevia,run` 的代码块可以编辑和执行。每次运行接收一个 Aevia 源文件。输入上限为 16 KiB，两个输出流各为 16 KiB，工作进程运行时限为七秒。Podman 进一步限制 CPU、内存和进程数。程序的 Aevia 能力集合为空，容器不连接外部网络，每次都使用新容器和临时工作目录。留空两个注册中心的运行配置变量即可提供静态教程。

## 备份、恢复与升级

先运行 `./deploy.sh stop registry`，使备份期间目录数据和归档稳定。保持 PostgreSQL 运行，再调用 `./deploy.sh backup`。脚本并行生成压缩 SQL 转储和归档快照，待两项均成功后保存文件。将 `backups/` 中时间戳相同的两个文件复制到其他主机，保留目录数据和已发布归档。

恢复时使用匹配的部署环境，保持注册中心停止、PostgreSQL 运行，恢复成对的 SQL 和归档后再启动应用。脚本先将整个 SQL 转储解压到私有临时文件，再在一个事务中替换目录数据。如需更换临时目录，可将 `TMPDIR` 设为能够容纳未压缩转储的文件系统。恢复之前请保留现有数据的备份。`./deploy.sh restore` 会提示确认，自动恢复可以设置 `AEVIA_RESTORE_YES=1`。

```sh
./deploy.sh stop registry
./deploy.sh restore backups/aevia-db-20261118T120000Z.sql.gz
./deploy.sh restore backups/aevia-data-20261118T120000Z.tar.gz
./deploy.sh up
```

更新到已验证的源码版本后，用 `./deploy.sh upgrade` 重建注册中心镜像并重新创建容器。应用在启动时执行迁移。部署涉及数据库变更的版本之前，请先完成备份，并在独立主机演练恢复。升级 PostgreSQL 主版本应使用新数据卷，再从备份恢复。
