# Deploying the Aevia registry

The service is a single binary, `aevia-registry`, that serves the public site, the
JSON API under `/api/v1`, the sparse index, package downloads and `/healthz`. It
needs PostgreSQL and a writable directory for package archives, and nothing else:
the schema is applied by the binary itself at start-up through sqlx migrations,
so there is no SQL to run, no migration file to mount and no database init script
to maintain.

Deployment is three containers — `db`, `registry`, `caddy` — on one host.

```
internet ──► caddy :443 ──► registry :8080 ──► db :5432
             TLS, gzip/zstd     loopback only    private network
             CSP, 24 MB cap     uid 10001         uid 999
```

---

## 1. Prerequisites

| Requirement | Why |
| --- | --- |
| Podman 5.0+ (or Docker 24+ with Compose v2) | Compose provider, `:U` volume ownership, Quadlet |
| aarch64 Linux, or x86_64 with qemu user emulation | the deployment target is ARM; see §2 |
| A DNS `A`/`AAAA` record for `aevia.apich.org` → this host | Caddy's ACME challenge cannot pass otherwise |
| Ports 80 and 443 reachable from the internet | HTTP-01 validation and HTTPS |
| ~2 GB RAM for the registry container, ~1 GB for PostgreSQL | Leptos SSR + rustls; see `mem_limit` in `compose.yaml` |

**On an aarch64 host** (the production case) nothing extra is needed: the image is
built natively.

**On x86_64** you can still build and run the arm64 image through emulation:

```sh
# one-off, needs root; registers the binfmt handlers
docker run --privileged --rm tonistiigi/binfmt --install arm64
# or, on a Podman host
sudo podman run --privileged --rm docker.io/tonistiigi/binfmt --install arm64

docker buildx build --platform linux/arm64 \
    --build-context lang=../crates \
    -f infra/Containerfile.server .
```

Emulated builds of this workspace take a long time (the dependency graph includes
`ring`, `leptos` and `sqlx`). Build on the ARM host, or in CI with native ARM
runners.

### The build context has a second half

The workspace's `crates/aevia-registry-docs` depends on `../crates/aevia-doc` and
`../crates/aevia-syntax` — the *language* repository, checked out next to this
one. A path outside the build context cannot be `COPY`ed, so the image build
takes a second, read-only build context named `lang`:

```sh
docker build -f infra/Containerfile.server \
    --build-context lang=../crates \
    -t localhost/aevia/registry:0.1.0 .
```

`deploy.sh build` and `compose.yaml` (`additional_contexts`) pass it for you.
`infra/lang-workspace.toml` reproduces the two tables those crates inherit from
the language workspace root; if the language repository adds a
`.workspace = true` dependency to `aevia-doc`, `aevia-syntax`, `aevia-fmt` or
`aevia-dim`, add the key there too.

---

## 2. First deployment

```sh
cd platform/infra

# 1. Configuration
cp .env.example .env
chmod 600 .env
$EDITOR .env                     # see §3 for the two required secrets

# 2. Build the registry image (native on ARM)
./deploy.sh build

# 3. Start everything; `up` waits for the database to be healthy first
./deploy.sh up

# 4. Watch it come up — the first start runs the migrations
./deploy.sh logs registry
```

The first `up` does, in order: creates the compose network and the two volumes,
starts PostgreSQL and waits for `pg_isready`, builds the postgres image if it is
not present, starts the registry (which applies migrations and then binds
`0.0.0.0:8080`), then starts Caddy, which requests a certificate for
`aevia.apich.org` on first request.

Verify:

```sh
curl -fsS https://aevia.apich.org/healthz        # {"status":"ok"}
curl -fsSI https://aevia.apich.org/ | head -20   # HSTS, CSP, no Server header
./deploy.sh ps
```

`/healthz` returns 200 only when the database answers. It is not a liveness
probe for the process — a 503 there means "up but degraded", and the container
healthcheck will start failing after three 30 s intervals.

---

## 3. Secrets

Only two values have to be generated; everything else has a safe default.

```sh
# 32 random bytes, hex-encoded → 64 characters. Signs session cookies.
openssl rand -hex 32

# The database password, used twice: in POSTGRES_PASSWORD and inside DATABASE_URL.
openssl rand -base64 24 | tr -d '/+=' | cut -c1-32
```

Put the first into `AEVIA_SESSION_SECRET` and the second into both
`POSTGRES_PASSWORD` and the password part of `DATABASE_URL`. The two must match
or the registry will fail to connect on first start.

Rotating `AEVIA_SESSION_SECRET` invalidates every session cookie; users are
logged out but nothing else breaks.

`POSTGRES_PASSWORD` is only read when the data directory is empty — the
entrypoint creates the role once. Changing it later does not change the
database role; do that with `ALTER ROLE` through `deploy.sh` + `psql`.

`.env` is git-ignored, excluded from the build context by `.dockerignore`, and
never copied into an image.

---

## 4. GitHub App and ORCID

Both integrations are optional. Leave the variables empty and the corresponding
"sign in with…" buttons are not offered; publishing then relies on the token
flow only.

### GitHub App

1. <https://github.com/settings/apps/new> (personal) or the organisation's
   *Settings → Developer settings → GitHub Apps → New GitHub App*.
2. **Homepage URL**: `https://aevia.apich.org`
3. **Callback URL**: `https://aevia.apich.org/auth/github/callback`
4. **User authorisation → Request user authorization (OAuth) during
   installation**: enabled — the registry needs a user token to know *who* is
   publishing.
5. **Permissions** — repository: *Contents: read* is enough; the registry never
   writes to GitHub. Organisation: *Members: read*.
6. Copy the **Client ID** and **Client secret** into `AEVIA_GITHUB_CLIENT_ID` /
   `AEVIA_GITHUB_CLIENT_SECRET`. Nothing else is needed: no App ID and no
   private key.

Organisation membership is read from the signed-in user's own installation
list (`GET /user/installations`) during the callback. The registry therefore
never holds a key that can act as the App, and an operator never has to mount a
PEM. The App must be *installed* on an organisation for its members to publish
under that organisation's scope; installing it grants the registry nothing
beyond the ability to see that the installation exists.

No scopes are requested in the OAuth flow beyond the App's own permissions —
GitHub ignores `scope` for Apps, and requesting repository scopes would make the
consent screen ask for far more than publishing needs.

### ORCID

1. <https://orcid.org/developer-tools> → *Register for the free ORCID public API*
   (or the sandbox at <https://sandbox.orcid.org/developer-tools> first).
2. **Redirect URI**: `https://aevia.apich.org/auth/orcid/callback`
3. Copy the client ID and secret into `AEVIA_ORCID_CLIENT_ID` /
   `AEVIA_ORCID_CLIENT_SECRET`.
4. Keep `AEVIA_ORCID_BASE=https://orcid.org` in production; use
   `https://sandbox.orcid.org` while testing, together with sandbox credentials.

The flow requests the `/authenticate` scope only: the registry needs the ORCID
iD to attribute a package, not the researcher's employment or publication record.

Both callback URLs are absolute and derived from `AEVIA_BASE_URL`; if you run
under a different hostname, change it there *and* in both provider consoles.

---

## 5. Where the data lives

Four paths, in two places.

| Path | Kind | Contents | Backed up by |
| --- | --- | --- | --- |
| `/var/lib/postgresql/data` | volume `aevia-db` | the whole database: users, packages, index metadata | `deploy.sh backup` (pg_dump) |
| `/var/lib/aevia/packages` | volume `aevia-data` | every published `.aevia` tarball, by content hash | `deploy.sh backup` (tar) |
| `/data` | volume `caddy-data` | TLS certificates, ACME account key | no — re-issued automatically |
| `infra/backups/` | host directory | the `.sql.gz` and `.tar.gz` produced by `backup` | copy off-host |

The first two are the service. They must be backed up together: a database row
points at a tarball, and a tarball without its row is unreachable.

`infra/backups/` is git-ignored and excluded from the build context. It is *not*
a backup on its own — it lives on the same disk as the volumes. Copy it
elsewhere (restic, `rsync`, object storage) on a schedule.

---

## 6. Backup and restore

```sh
./deploy.sh backup
# backups/aevia-db-20261108T101500Z.sql.gz
# backups/aevia-data-20261108T101500Z.tar.gz
```

`backup` runs `pg_dump --no-owner --clean --if-exists` inside the `db` container
and tars the archive volume from a throwaway container. It refuses to write an
empty dump. Both files are timestamped in UTC and are consistent with each other
only if nothing was published between the two steps — for a strict point-in-time
pair, stop the registry for the duration (`./deploy.sh down` / `up`), or accept
that a package published in between simply is not in the dump.

Restore:

```sh
./deploy.sh down
./deploy.sh up                       # let the migrations create an empty schema
./deploy.sh restore backups/aevia-db-20261108T101500Z.sql.gz
./deploy.sh restore backups/aevia-data-20261108T101500Z.tar.gz
```

`restore` prompts before overwriting; set `AEVIA_RESTORE_YES=1` for unattended
runs. The SQL dump is applied with `ON_ERROR_STOP=1`, so a partial restore fails
loudly rather than silently half-applying. Restoring into a database that already
has data is allowed — the dump is `--clean`, so it drops and recreates the
objects — but it will destroy anything published since the dump.

Test a restore into a scratch host at least once before you need it.

---

## 7. Upgrades

```sh
git pull
./deploy.sh upgrade
```

`upgrade` pulls the floating images (Caddy), rebuilds `localhost/aevia/registry`,
and recreates the stack. The database is untouched; migrations run at start-up
and are additive by construction.

**PostgreSQL major versions are not upgraded in place.** Moving from 18 to 19
means: `deploy.sh backup`, bump the `POSTGRES_IMAGE` build arg and the `image:`
tag in `compose.yaml`, start a fresh `aevia-db` volume, and
`deploy.sh restore` the dump. `pg_upgrade` inside the container is possible but
not wired up here.

Rollback: `git checkout <previous commit> && ./deploy.sh upgrade`. Schema
migrations are forward-only; if a release added one, rolling the binary back
leaves the newer schema in place, which older binaries tolerate only if the
migration was additive.

---

## 8. Logs and health

| What | Compose | Quadlet |
| --- | --- | --- |
| registry | `./deploy.sh logs registry` | `journalctl --user -u aevia-registry -f` |
| database | `./deploy.sh logs db` | `journalctl --user -u aevia-db -f` |
| Caddy (JSON access log) | `./deploy.sh logs caddy` | `journalctl --user -u aevia-caddy -f` |

Container logs are capped at 10 MB × 5 files per service (`logging.options` in
`compose.yaml`) or go to journald under Quadlet, so a log flood cannot fill the
disk. `RUST_LOG` defaults to `aevia_registry=info,tower_http=warn`; set it to
`aevia_registry=debug` in `.env` and restart for more detail.

Health:

```sh
curl -fsS https://aevia.apich.org/healthz      # through Caddy — the check that matters
./deploy.sh ps                                 # STATUS column, compose
podman inspect --format '{{.State.Health.Status}}' aevia-registry   # Quadlet
```

---

## 9. Troubleshooting

| Symptom | Cause | Fix |
| --- | --- | --- |
| `registry` restarts every few seconds | `DATABASE_URL` wrong, or the password does not match `POSTGRES_PASSWORD` | `./deploy.sh logs registry`; the first lines name the connection error |
| `registry` restarts, log says `AEVIA_SESSION_SECRET must be 64 hex characters` | secret missing, or generated with `openssl rand -hex 16` | regenerate with `openssl rand -hex 32` |
| `pg_isready` failing, db healthy-looking | the role/database were created from an older `.env` and `POSTGRES_PASSWORD` was changed afterwards | the entrypoint only creates them on an empty volume; `ALTER ROLE aevia PASSWORD '…'` or recreate the volume |
| `pg_isready` failing after a host reboot | the volume was created by root and is not writable by uid 999 | `podman volume inspect aevia-db`, then recreate with `:U` |
| `Permission denied` on `/var/lib/aevia/packages`, rootless Podman | the named volume is owned by the mapped root user, not by the image's uid 10001 | add `:U` to the volume mount (already there in `compose.yaml`/Quadlet); `podman volume rm` and let it be recreated if it was made without it |
| `Permission denied` on a *bind* mount instead of a volume | the host directory is owned by another uid | `podman unshare chown -R 10001:10001 <dir>` |
| Caddy: `obtaining certificate: ... challenge failed` | DNS does not point here yet, or port 80 is blocked | `dig +short aevia.apich.org`; check the firewall; retry with `podman restart aevia-caddy` |
| Caddy: `too many certificates already issued` | Let's Encrypt rate limit hit while iterating | use the staging CA: add `acme_ca https://acme-staging-v02.api.letsencrypt.org/directory` to the global block temporarily |
| Browser: `ERR_SSL_PROTOCOL_ERROR` | something else owns 443 | `ss -ltnp '( sport = :443 )'` |
| `413` on publish | upload over 24 MB | the limit is in `Caddyfile` (`request_body`); raise it there, not in the app |
| Every client IP is `127.0.0.1` | `AEVIA_TRUST_PROXY=false`, or Caddy is bypassed | set it to `true`; check `X-Forwarded-For` reaches the app |
| Build fails: `failed to load manifest for dependency aevia-doc` | the `lang` build context was not passed | `--build-context lang=../crates`, or use `./deploy.sh build` |
| Build fails: `the lock file ... needs to be updated` | `Cargo.lock` is stale relative to a manifest | `cargo update --workspace` in `platform/` and commit, or `--build-arg CARGO_LOCKED=` |

---

## 10. Why Debian 13 and not Alpine

The binary is compiled against glibc. Alpine uses musl, and the two are not
interchangeable at run time: a glibc-linked executable does not start on musl at
all, and the `*-unknown-linux-musl` target is a *different* build, not a smaller
one. Producing one would mean a second toolchain, a second `Cargo.lock`
resolution (musl forces `ring`'s assembly path and changes `sqlx`'s DNS
behaviour), and a binary that is only ever tested in the environment nobody
develops in.

The development machines are Windows x86_64 and the deployment host is aarch64
Linux, so the image is already the one place where "it works on my machine" can
hide. Keeping the same C library as the CI runner removes one variable from that
list. `debian:trixie-slim` is ~30 MB compressed — the difference to Alpine's
~3 MB is not worth a second, differently-linked build of the service.

Debian 13 specifically, not 12: trixie is the current stable, `postgres:18-trixie`
is built on it, and the `libssl3t64` transition means a bookworm image and a
trixie package would disagree about the time64 ABI. One distro release across
builder and runtime avoids that class of problem entirely.

The image does not ship `libssl` at all: the binary uses rustls (`ring` in
`Cargo.lock`, no `openssl-sys`), so there is no system TLS library to keep in
step with the host.

---

## 11. Why the database is a container and not a host service

The database is the one piece of state the service cannot reconstruct. Running it
in a container makes three things explicit that a host `apt install postgresql`
leaves implicit:

- **The version is pinned in git.** `postgres:18-trixie` in `compose.yaml` is the
  same on every host. A host service is whatever `apt` resolved that week, which
  is how staging and production end up on different majors.
- **The data directory has one documented location.** `/var/lib/postgresql/data`
  in a named volume, not `/var/lib/postgresql/18/main` or `/var/lib/pgsql/data`
  depending on the distribution.
- **The network boundary is explicit.** The container publishes no ports; the
  registry reaches it by service name over a private bridge. A host service
  listens on `localhost:5432` by default and is one careless `listen_addresses`
  away from being public.

The cost is real: `pg_upgrade` is now a dump/restore (§7), and `pg_dump` has to
run through `podman exec`. Both are documented above, and both are cheaper than
debugging a production database whose configuration nobody recorded.
