# Run the registry with Quadlet

[中文部署指南](../../docs/DEPLOY.zh.md) · [Deployment guide](../../docs/DEPLOY.md)

Quadlet runs the same registry, database and edge as the Compose stack. Choose one service manager on each host. The units use an edge network for Caddy and the registry, and a separate internal network for the registry and PostgreSQL.

## Install

Run these commands from `platform/infra/quadlet` as the rootless Podman user. The image build uses Bazel and the adjacent RSSN checkout. Adjust the read-only source mount paths in `aevia-registry.container` if the Aevia checkout is outside `~/aevia`.

```sh
umask 077
mkdir -p ~/.config/containers/systemd ~/.config/aevia
mkdir -p ~/.local/share/aevia/caddy/data ~/.local/share/aevia/caddy/config
cp aevia*.network aevia*.volume aevia*.container ~/.config/containers/systemd/
cp ../.env.example ~/.config/aevia/aevia.env
cp ../Caddyfile ~/.config/aevia/Caddyfile
${EDITOR:-vi} ~/.config/aevia/aevia.env
```

Set `AEVIA_BASE_URL` to the public origin in `aevia.env`. Replace the example `AEVIA_SESSION_SECRET` with the output of `openssl rand -hex 32`. Set a database password and use it in `DATABASE_URL`, whose host is `db` on the internal network. Put `POSTGRES_USER`, `POSTGRES_PASSWORD`, and `POSTGRES_DB` in `~/.config/aevia/db.env`. Those values must match `DATABASE_URL`. Restrict the environment files to the current user with mode `600`. To customize the public host or ACME contact, set `AEVIA_DOMAIN` and `AEVIA_ACME_EMAIL` in `~/.config/aevia/caddy.env`. If the file is absent, Caddy uses the defaults from `Caddyfile`.

```sh
../deploy.sh build
(cd ../../.. && bazel run //:postgres_image)
systemctl --user daemon-reload
systemctl --user start aevia-caddy.service
systemctl --user status aevia-db.service aevia-registry.service aevia-caddy.service
```

The user’s `default.target` starts these units. Starting Caddy also starts the registry and PostgreSQL through the service dependencies. The registry publishes `127.0.0.1:8080` for local checks. PostgreSQL is reachable through the internal network. Enable linger if the services should run while the user is logged out. Run `journalctl --user -u aevia-registry.service -f` to follow logs.

Rebuild a changed registry image with `../deploy.sh build` and run `systemctl --user restart aevia-registry.service`. The specification, standard-library reference, and manuals refresh from their read-only mounts. Optional tutorial execution uses the separate [worker deployment](../../docs/DEPLOY.md#enable-book-execution).

## Back up

Run `systemctl --user stop aevia-registry.service` before preparing a consistent database and archive pair. Leave `aevia-db.service` running. The shell commands create compressed files in a new private directory and remove the directory if a snapshot fails.

```sh
(
  set -eu
  umask 077
  backup=$(mktemp -d ./aevia-backup.XXXXXXXX)
  trap 'rm -rf -- "$backup"' EXIT
  trap 'exit 1' HUP INT TERM
  podman exec aevia-db sh -c \
    'pg_dump --format=plain --compress=gzip:9 --no-owner --clean --if-exists -U "$POSTGRES_USER" -d "$POSTGRES_DB"' \
    > "$backup/aevia-db.sql.gz"
  podman run --rm --network=none --user 0:0 -v aevia-data:/data:ro \
    --entrypoint /usr/bin/tar localhost/aevia/registry:0.1.0 \
    --exclude='*.partial' -czf - -C /data . > "$backup/aevia-data.tar.gz"
  test -s "$backup/aevia-db.sql.gz"
  test -s "$backup/aevia-data.tar.gz"
  trap - EXIT
  printf 'Backup written to %s\n' "$backup"
)
```

Restart the registry with `systemctl --user start aevia-registry.service`. Keep the pair off-site and rehearse recovery with an independent database and archive volume. Decompress the entire SQL dump to a private temporary file before passing it to `psql -X --single-transaction --set ON_ERROR_STOP=1 --file=-`. Restore archive files into the corresponding named Podman volume with the registry stopped.

## 中文

Quadlet 通过无根 Podman 和 systemd 管理注册中心、数据库和 TLS 代理。Caddy 与注册中心共用边缘网络，PostgreSQL 则位于单独的内部网络。在同一主机上选择 Compose 或 Quadlet 作为服务管理方式。

从 `platform/infra/quadlet` 执行上面的安装命令。镜像构建需要 Bazel 和相邻的 RSSN 工作树。将单元文件复制到 `~/.config/containers/systemd/`，并复制 Caddyfile 和环境配置。仓库检出位置在 `~/aevia` 以外时，修改 `aevia-registry.container` 的只读文档挂载路径。

`~/.config/aevia/aevia.env` 保存注册中心环境。将 `AEVIA_BASE_URL` 设为公开访问地址，把 `AEVIA_SESSION_SECRET` 替换为 `openssl rand -hex 32` 的输出，并在 `DATABASE_URL` 中使用新设定的密码和主机名 `db`。`db.env` 中的 `POSTGRES_USER`、`POSTGRES_PASSWORD` 和 `POSTGRES_DB` 必须与连接字符串相符。要自定义 TLS 域名或 ACME 邮箱，可在 `caddy.env` 中设置 `AEVIA_DOMAIN` 和 `AEVIA_ACME_EMAIL`。文件不存在时，Caddy 使用 `Caddyfile` 中的默认值。将各环境文件的权限设为 `600`。

构建注册中心和数据库镜像后，执行 `systemctl --user daemon-reload` 并启动 `aevia-caddy.service`。服务依赖会启动注册中心和数据库。注册中心发布到 `127.0.0.1:8080`，便于在本机验证。PostgreSQL 通过内部网络访问。使用 `journalctl --user -u aevia-registry.service -f` 查看应用日志。如果希望登出后继续运行，启用用户 linger。

更新镜像后重启 `aevia-registry.service` 即可应用二进制改动。语言规范、标准库参考和手册会从只读挂载自动刷新。教程代码运行使用独立的工作进程。 [部署教程运行服务](../../docs/DEPLOY.zh.md#启用教程运行功能)

备份时保持 PostgreSQL 运行，并执行 `systemctl --user stop aevia-registry.service` 停止注册中心，以便获得匹配的 SQL 和包归档。上方命令将压缩文件写入新建的专用目录，任一备份命令失败时会清理该目录。完成后执行 `systemctl --user start aevia-registry.service`。恢复 SQL 前，先将整个转储解压到私有临时文件，再交给 `psql`。请另存成对的备份，并在独立数据库和归档卷中演练恢复。 [备份与恢复](../../docs/DEPLOY.zh.md#备份恢复与升级)
