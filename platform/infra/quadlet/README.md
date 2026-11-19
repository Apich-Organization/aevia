# Run the registry with Quadlet

[中文部署指南](../../docs/DEPLOY.zh.md) · [Deployment guide](../../docs/DEPLOY.md)

Quadlet runs the same registry, database and edge as the Compose stack. Choose one service manager on each host. The units use an edge network for Caddy and the registry, and a separate internal network for the registry and PostgreSQL.

## Install

Run these commands from `platform/infra/quadlet` as the rootless Podman user. Adjust the read-only source mount paths in `aevia-registry.container` if the Aevia checkout is outside `~/aevia`.

```sh
umask 077
mkdir -p ~/.config/containers/systemd ~/.config/aevia
mkdir -p ~/.local/share/aevia/caddy/data ~/.local/share/aevia/caddy/config
cp aevia*.network aevia*.volume aevia*.container ~/.config/containers/systemd/
cp ../.env.example ~/.config/aevia/aevia.env
cp ../Caddyfile ~/.config/aevia/Caddyfile
${EDITOR:-vi} ~/.config/aevia/aevia.env
```

Set the public origin and database connection in `aevia.env`. Put only `POSTGRES_USER`, `POSTGRES_PASSWORD`, and `POSTGRES_DB` in `~/.config/aevia/db.env`. The credentials must match the registry connection string. For a custom public host or ACME contact, place `AEVIA_DOMAIN` and `AEVIA_ACME_EMAIL` in `~/.config/aevia/caddy.env`.

```sh
../deploy.sh build
podman build -f ../Containerfile.postgres -t localhost/aevia/postgres:18-trixie ..
systemctl --user daemon-reload
systemctl --user start aevia-caddy.service
systemctl --user status aevia-db.service aevia-registry.service aevia-caddy.service
```

The generated units start at `default.target`. Enable linger if they should run while the user is logged out. Run `journalctl --user -u aevia-registry.service -f` to follow logs.

Rebuild a changed registry image with `../deploy.sh build` and run `systemctl --user restart aevia-registry.service`. The specification and manuals refresh from their read-only mounts. Optional tutorial execution uses the separate [worker deployment](../../docs/DEPLOY.md#enable-book-execution).

## Back up

Stop `aevia-registry.service` while preparing a consistent database and archive pair. Leave `aevia-db.service` running. The shell commands create the catalogue dump and package copy in the current directory.

```sh
umask 077
podman exec aevia-db sh -c \
  'pg_dump --no-owner --clean --if-exists -U "$POSTGRES_USER" -d "$POSTGRES_DB"' > aevia.sql
gzip aevia.sql
podman run --rm --network=none --user 0:0 -v aevia-data:/data:ro \
  --entrypoint /usr/bin/tar localhost/aevia/registry:0.1.0 \
  --exclude='*.partial' -czf - -C /data . > aevia-data.tar.gz
```

Keep the pair off-site and rehearse recovery with an independent database and archive volume. The SQL dump restores with `psql --single-transaction --set ON_ERROR_STOP=1 --file=-` against the database. Restore archive files into the corresponding named Podman volume with the registry stopped.

## 中文

Quadlet 通过无根 Podman 和 systemd 管理注册中心、数据库和 TLS 代理。Caddy 与注册中心共用边缘网络，PostgreSQL 则位于单独的内部网络。在同一主机上选择 Compose 或 Quadlet 作为服务管理方式。

从 `platform/infra/quadlet` 执行上面的安装命令。将单元文件复制到 `~/.config/containers/systemd/`，再复制 Caddyfile 和环境配置。如果仓库检出位置不是 `~/aevia`，请修改 `aevia-registry.container` 的只读文档挂载路径。

`~/.config/aevia/aevia.env` 保存注册中心环境。`db.env` 只需 `POSTGRES_USER`、`POSTGRES_PASSWORD` 和 `POSTGRES_DB`，且必须与注册中心连接字符串相符。要自定义 TLS 域名或 ACME 邮箱，可在 `caddy.env` 中设置 `AEVIA_DOMAIN` 和 `AEVIA_ACME_EMAIL`。将各环境文件限制为当前用户读取。

构建注册中心和数据库镜像后，执行 `systemctl --user daemon-reload` 并启动 `aevia-caddy.service`。生成的单元会依赖注册中心和数据库。使用 `journalctl --user -u aevia-registry.service -f` 查看应用日志。如果希望登出后继续运行，启用用户 linger。

更新镜像后重启 `aevia-registry.service` 即可应用二进制改动。语言规范和手册会从只读挂载自动刷新。教程代码运行使用独立的工作进程。 [部署教程运行服务](../../docs/DEPLOY.zh.md#启用教程运行功能)

备份时保持 PostgreSQL 运行并停止注册中心，以便获得匹配的 SQL 和包归档。上方命令将这两份数据写入当前目录。请另存成对的备份，并在独立数据库和归档卷中演练恢复。 [备份与恢复](../../docs/DEPLOY.zh.md#备份恢复与升级)
