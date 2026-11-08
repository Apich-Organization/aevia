# Quadlet units

The same three services as `../compose.yaml`, expressed as rootless Podman
systemd units. Use one or the other, not both: compose and Quadlet would fight
over the same container names and ports.

Quadlet is the better fit on a long-lived host. systemd owns the lifecycle, so
containers start after the network is up, restart on failure, log to journald,
and survive reboots without `restart: unless-stopped` bookkeeping.

## Install

```sh
# 1. Units: copy into the user's Quadlet directory.
mkdir -p ~/.config/containers/systemd
cp aevia.network aevia-db.volume aevia-data.volume \
   aevia-db.container aevia-registry.container aevia-caddy.container \
   ~/.config/containers/systemd/

# 2. Configuration and TLS material.
mkdir -p ~/.config/aevia ~/.local/share/aevia/caddy/data ~/.local/share/aevia/caddy/config
cp ../.env.example ~/.config/aevia/aevia.env
chmod 700 ~/.config/aevia
chmod 600 ~/.config/aevia/aevia.env
$EDITOR ~/.config/aevia/aevia.env          # DATABASE_URL, POSTGRES_PASSWORD, AEVIA_SESSION_SECRET
cp ../Caddyfile ~/.config/aevia/Caddyfile

# 3. Build the two local images (Quadlet never builds; it only runs).
podman build --build-context lang="$PWD/../../../crates" \
    -f ../Containerfile.server -t localhost/aevia/registry:0.1.0 ../../..
podman build -f ../Containerfile.postgres -t localhost/aevia/postgres:18-trixie ..

# 4. Generate the systemd units from the Quadlet files.
systemctl --user daemon-reload

# 5. Start. `enable` is implicit: the generator wires WantedBy=default.target.
systemctl --user start aevia-caddy.service
```

`systemctl --user enable` is not needed and will refuse: Quadlet units are
generated at `daemon-reload` time and are already wanted by `default.target`.
Use `systemctl --user start|stop|restart`.

To keep the user session (and therefore the containers) alive when you are not
logged in:

```sh
loginctl enable-linger "$USER"
```

## Day-to-day

```sh
systemctl --user status aevia-registry.service aevia-db.service aevia-caddy.service
journalctl --user -u aevia-registry.service -f
journalctl --user -u aevia-caddy.service --since '1 hour ago'
podman ps --filter name=aevia
```

Rebuild after a source change — the image is local, so `AutoUpdate` does not
apply to it:

```sh
podman build --build-context lang="$PWD/../../../crates" \
    -f ../Containerfile.server -t localhost/aevia/registry:0.1.0 ../../..
systemctl --user restart aevia-registry.service
```

Caddy tracks `caddy:2` from Docker Hub. `AutoUpdate=registry` makes it eligible
for `podman auto-update`, which needs a timer:

```sh
systemctl --user enable --now podman-auto-update.timer
podman auto-update --dry-run
```

## Paths

| What | Where |
| --- | --- |
| Units | `~/.config/containers/systemd/aevia-*.{network,volume,container}` |
| Environment | `~/.config/aevia/aevia.env` (mode 0600) |
| Caddyfile | `~/.config/aevia/Caddyfile` |
| Caddy state | `~/.local/share/aevia/caddy/{data,config}` |
| Database | volume `aevia-db` |
| Archives | volume `aevia-data` |

`EnvironmentFile=%h/.config/aevia/aevia.env` is read by Podman, not by systemd:
the file does not need systemd `EnvironmentFile=` syntax beyond `KEY=value`.

## Backup and restore

Quadlet has no compose wrapper, so `../deploy.sh` does not drive it. The
equivalent commands:

```sh
podman exec aevia-db sh -c \
  'pg_dump --no-owner --clean --if-exists -U "$POSTGRES_USER" -d "$POSTGRES_DB"' \
  | gzip -9 > "aevia-db-$(date -u +%Y%m%dT%H%M%SZ).sql.gz"

podman run --rm --user 0:0 -v aevia-data:/data:ro \
  localhost/aevia/registry:0.1.0 tar -czf - -C /data . > aevia-data.tar.gz
```

Restore with `gunzip -c file.sql.gz | podman exec -i aevia-db sh -c 'psql -v ON_ERROR_STOP=1 -U "$POSTGRES_USER" -d "$POSTGRES_DB"'`.

## Notes

- `:U` on both data volumes is what makes the non-root containers able to write.
  Drop it only if the volumes already carry the right ownership.
- `Requires=`/`After=` chain the units in dependency order, so starting
  `aevia-caddy.service` pulls in the registry, which pulls in the database.
  Stopping the database while the registry runs is still allowed — systemd does
  not enforce stop ordering across `Requires=`.
