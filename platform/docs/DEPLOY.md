# Deploy the Aevia platform

[简体中文](DEPLOY.zh.md)

The registry needs PostgreSQL 18, durable package storage, and network access to any identity providers you enable. The registry applies its database migrations before accepting requests. A typical deployment uses rootless Podman and Caddy on an x86-64 or ARM64 Linux host. [System design](../manuals/architecture/en/05-registry.md) · [Operations design](../manuals/architecture/en/06-operations.md)

## Start the registry

From the Aevia checkout, prepare the configuration and build the image. A Podman Compose provider is required. The host must allow the rootless edge process to listen on TCP ports 80 and 443, and on UDP 443 if you want HTTP/3. Point the public DNS name to the edge before enabling automatic TLS.

```sh
cd platform/infra
umask 077
cp .env.example .env
openssl rand -hex 32   # AEVIA_SESSION_SECRET
openssl rand -hex 24   # POSTGRES_PASSWORD and the password in DATABASE_URL
${EDITOR:-vi} .env
./deploy.sh build
./deploy.sh up
./deploy.sh logs registry
```

Set `AEVIA_DOMAIN` to the public DNS name and `AEVIA_BASE_URL` to the matching HTTPS origin. Set `AEVIA_ACME_EMAIL` to a monitored address. Keep `db` as the database hostname in Compose. The registry’s internal listener is also available at `127.0.0.1:8080` on the host.

Caddy is the only public listener. It manages TLS, sets HSTS and compresses responses. The registry applies browser security policies directly. Caddy replaces `X-Forwarded-For` before forwarding to the private service for rate limiting. Keep `AEVIA_TRUST_PROXY=true` only when requests go through a proxy you control that replaces this header. Use the [Quadlet units](../infra/quadlet/README.md) when systemd should manage the containers directly.

```sh
curl -fsS https://aevia.apich.org/healthz
curl -fsSI https://aevia.apich.org/book
./deploy.sh ps
```

`/healthz` reports whether PostgreSQL can answer. The registry reports HTTP 503 while the database is unavailable. `./deploy.sh logs db` and `./deploy.sh logs registry` show the underlying errors.

## Configure sign-in

Enable GitHub, ORCID or both. Readers can browse without an identity provider. Publishers need an account and a bound signing key.

- Register a GitHub App with the callback `AEVIA_BASE_URL/auth/github/callback`. Copy its client ID and client secret into `AEVIA_GITHUB_CLIENT_ID` and `AEVIA_GITHUB_CLIENT_SECRET`. Organization publishing requires active membership and access to the corresponding App installation. Grant the App read access to organization membership.
- Register an ORCID OAuth client with the callback `AEVIA_BASE_URL/auth/orcid/callback` and set `AEVIA_ORCID_CLIENT_ID` and `AEVIA_ORCID_CLIENT_SECRET`. For an ORCID sandbox client, use `AEVIA_ORCID_BASE=https://sandbox.orcid.org`. The registry requests `/authenticate`.

Provider secrets and the session key live in the registry environment. Rotate `AEVIA_SESSION_SECRET` to invalidate sessions issued with that key. To change the PostgreSQL password after initialization, update the database role and the application connection string together. Editing the initialization environment alone leaves the stored password in place.

### Bind legacy GitHub accounts during upgrade

Migration `0006_github_identity` preserves publisher row IDs, Aevia namespaces, signing keys and package ownership. It ends existing GitHub browser sessions and discards handle-based organisation grants. The upgraded registry requires a separately verified numeric GitHub ID to sign in to an existing GitHub publisher row. A GitHub rename keeps the original Aevia namespace, including when someone else later uses that GitHub login.

Stop all old registry instances before upgrading. Start the upgraded server with GitHub sign-in disabled while reviewing existing bindings. For each user or organisation, verify the GitHub numeric ID against trusted historical account or installation evidence. For a user, confirmation should also include proof of possession of a previously trusted release-signing key or an established maintainer contact. Review that publisher’s existing signing keys and investigate unexpected bindings. A current GitHub login alone cannot establish ownership of the old namespace. [GitHub account ID lookup](https://docs.github.com/en/rest/users/users#get-a-user-using-their-id)

Bind only reviewed records in PostgreSQL. In `psql`, set `registry_id`, `registry_kind` (`github` or `github-org`), `registry_handle`, and `verified_github_id` to the reviewed values with `\set` before entering this transaction.

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

Confirm the result contains exactly the expected row. Then delete any browser sessions for that publisher and commit. Roll back if the row differs, the update affects zero rows, or PostgreSQL reports a duplicate provider ID. Keep unresolved mappings disabled for browser sign-in and escalate conflicts for review. When sign-in is enabled again, GitHub members sign in to obtain fresh organisation grants.

```sql
DELETE FROM sessions WHERE publisher_id = :'registry_id'::bigint;
COMMIT;
```

## Publish documentation updates

Compose and Quadlet mount these directories read-only. The registry polls for changes and refreshes each complete render while serving the previous snapshot during loading. Deploy a bilingual edit together and replace individual files atomically.

| Source under the Aevia checkout | Mount | Site |
| --- | --- | --- |
| `spec/` | `/spec` | `/spec` |
| `platform/stdlib-docs/` | `/stdlib-docs` | `/stdlib` |
| `platform/manuals/book/{en,zh}/` | `/manuals/book/{en,zh}` | `/book` |
| `platform/manuals/architecture/{en,zh}/` | `/manuals/architecture/{en,zh}` | `/architecture` |

Both manual editions use matching chapter filenames, each with an H1 title and a chapter body. `/book/download` and `/architecture/download` provide standalone HTML in the requested language. Compiler-rendered documentation for published packages is stored in PostgreSQL and built from the signed archive. `AEVIA_DOCS_ENABLED=false` skips package documentation for new releases.

## Enable book execution

Run `aevia-playground` on a dedicated Linux host or dedicated rootless Podman account with cgroup v2 and a subordinate UID/GID range. Build or install the Linux `aevia` executable for that host architecture. From the checkout root, build the worker and a local image containing the matching CLI.

```sh
cargo build --locked --release --manifest-path platform/Cargo.toml \
    -p aevia-registry-server --bin aevia-playground
sh platform/infra/build-playground.sh target/release/aevia localhost/aevia/playground:release
```

Install `platform/target/release/aevia-playground` at `~/.local/bin/aevia-playground` for the worker account. Set these values in `~/.config/aevia/playground.env`, mode `0600`.

```dotenv
AEVIA_PLAYGROUND_BIND=127.0.0.1:9081
AEVIA_PLAYGROUND_IMAGE=localhost/aevia/playground:release
AEVIA_PLAYGROUND_TOKEN=<output of openssl rand -hex 32>
```

Install [`aevia-playground.service`](../infra/playground/aevia-playground.service) in the account’s `~/.config/systemd/user/` and start it.

```sh
systemctl --user daemon-reload
systemctl --user enable --now aevia-playground.service
journalctl --user -u aevia-playground.service -f
```

Set `AEVIA_PLAYGROUND_TOKEN` to the same key in the registry environment. If the registry also runs directly on that host, set `AEVIA_PLAYGROUND_URL=http://127.0.0.1:9081/run`. For a registry in a container or on another host, forward the worker’s loopback listener through an access-restricted TLS proxy reachable to the registry and set `AEVIA_PLAYGROUND_URL=https://worker.example.org/run`. The worker needs the runtime image in its own Podman storage. If using Caddy on the worker host, set its public TLS name and the registry’s source network in the provided proxy configuration. [Worker proxy example](../infra/playground/Caddyfile.example) The registry needs neither the image nor access to the worker’s Podman socket, archive storage or database.

A fenced `aevia,run` example in a book chapter becomes editable when a worker is configured. An execution accepts one Aevia source file and has a 16 KiB input cap, 16 KiB per output stream, a seven-second worker deadline, and fixed Podman CPU, memory and process limits. The runtime has no external network access and starts with an empty Aevia capability set. Each run uses a fresh container and temporary working directory. Leave both registry playground variables empty to serve the static tutorial.

## Back up, restore, and upgrade

Use `./deploy.sh stop registry` for a backup with a stable catalogue and archive tree. Run `./deploy.sh backup` with PostgreSQL running. Copy the two timestamp-matched files from `backups/` off the host. The database alone cannot reconstruct published archives.

Restore into a matching deployment with the registry stopped and PostgreSQL running. Restore the matching SQL and archive pair, then start the registry. The SQL restore replaces catalogue objects in one transaction. Keep a copy of the current data before restoring. `./deploy.sh restore` requires confirmation, or `AEVIA_RESTORE_YES=1` for an unattended restore.

```sh
./deploy.sh stop registry
./deploy.sh restore backups/aevia-db-20261118T120000Z.sql.gz
./deploy.sh restore backups/aevia-data-20261118T120000Z.tar.gz
./deploy.sh up
```

Pull a tested source revision and use `./deploy.sh upgrade` to rebuild the registry image and recreate the containers. Migrations run at startup. Before deploying a schema-changing upgrade, take a fresh backup and rehearse restoration on a separate host. For a PostgreSQL major-version upgrade, create a new database volume and restore from a dump rather than reusing the old data directory.
