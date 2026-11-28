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

Run source builds from a checkout whose RSSN sibling is available to Bazel.
`./deploy.sh build` forwards the registry image build to Bazel. On a first
`./deploy.sh up` with no local registry image, it builds that image through the
same target before starting Compose. It also builds the local PostgreSQL image
when needed through `bazel run //:postgres_image`. From the Aevia repository root, the
underlying commands are as follows.

```sh
bazel run //:registry_image -- --tag localhost/aevia/registry:0.1.0
bazel build //:registry_server
bazel run //:registry_image_from_binary -- \
  --binary bazel-bin/registry_server/bin/aevia-registry \
  --tag localhost/aevia/registry:0.1.0
```

The first command builds and images the registry in one step. The last
command packages a supplied registry executable, such as the output of the
preceding Bazel build. Both image targets default to Podman and accept
`--engine docker` and repeated `--tag` flags. Compose starts the resulting
`localhost/aevia/registry:0.1.0` image with the configured database and content
mounts.

Set `AEVIA_DOMAIN` to the public DNS name and `AEVIA_BASE_URL` to the matching HTTPS origin. Set `AEVIA_ACME_EMAIL` to a monitored address. Keep `db` as the database hostname in Compose. The registry’s internal listener is also available at `127.0.0.1:8080` on the host.

Caddy is the only public listener. It manages TLS, sets HSTS and compresses responses. The registry applies browser security policies directly. Caddy replaces `X-Forwarded-For` before forwarding to the private service for rate limiting. Keep `AEVIA_TRUST_PROXY=true` only when requests go through a proxy you control that replaces this header. Use the [Quadlet units](../infra/quadlet/README.md) when systemd should manage the containers directly.

```sh
curl -fsS https://aevia.apich.org/healthz
curl -fsSI https://aevia.apich.org/book
./deploy.sh ps
```

`/healthz` reports whether PostgreSQL can answer. The registry reports HTTP 503 while the database is unavailable. `./deploy.sh logs db` and `./deploy.sh logs registry` show the underlying errors.

Registry instances sharing a database serialize startup migrations with a PostgreSQL advisory lock. The registry checks the recorded SQLx migration checksums before accepting requests. If startup reports `cannot apply the schema`, check the registry log for the failing migration and correct the database condition before retrying. A checksum mismatch requires matching the binary to the installed migration history or restoring a compatible backup.

Schedule a maintenance window for a schema-changing upgrade. Migration `0003_pg18_features` holds an `ACCESS EXCLUSIVE` lock on `versions` until its transaction ends. This blocks reads and writes to that table. Pause registry traffic when applying this migration to an existing database.

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

## Verify public discovery

Set `AEVIA_BASE_URL` to the public HTTPS origin before publishing links. Fetch `/sitemap.xml` and check that public chapter URLs include the language parameter. Inspect the `Link` headers of a chapter to check its canonical URL and translated alternatives. `/llms.txt?lang=en` links to chapter Markdown and the generated catalog at `/stdlib/catalog.json`. A site with local Markdown-only stdlib files serves the rendered modules and omits the catalog.

Stage related files together and rotate the mount's directory entries atomically. Keep each edition intact until it has been loaded. To observe recovery, deploy an invalid bundle on a staging instance, confirm that the previous text and catalog remain available, and replace it with a valid bundle. For hot-swapped documents use read-only directory mounts so both renames and file changes are visible to the registry.

## Enable book execution

Run `aevia-playground` on a dedicated Linux host or dedicated rootless Podman account with cgroup v2 and a subordinate UID/GID range. From the checkout root, build the worker and a local image containing the CLI for that host architecture.

```sh
bazel build --config=release //:platform_binaries
bazel run //:playground_image -- --tag localhost/aevia/playground:release
```

To package a matching existing CLI, use `bazel run //:playground_image_from_binary -- --aevia /path/to/aevia --tag localhost/aevia/playground:release`.

Install `bazel-bin/platform_binaries/bin/aevia-playground` at `~/.local/bin/aevia-playground` for the worker account. Set these values in `~/.config/aevia/playground.env`, mode `0600`.

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

Run `./deploy.sh stop registry` before taking a backup so the catalogue and archive tree stay stable. Keep PostgreSQL running, then run `./deploy.sh backup`. It creates the compressed SQL dump and archive snapshot concurrently and publishes them after both succeed. Copy both timestamp-matched files from `backups/` off the host to retain the catalogue and the published archives.

Restore into a matching deployment with the registry stopped and PostgreSQL running. Restore the matching SQL and archive pair, then start the registry. The script decompresses the entire SQL dump to a private temporary file before replacing catalogue objects in one transaction. Set `TMPDIR` to a filesystem with space for the uncompressed dump if needed. Keep a copy of the current data before restoring. `./deploy.sh restore` requires confirmation, or `AEVIA_RESTORE_YES=1` for an unattended restore.

```sh
./deploy.sh stop registry
./deploy.sh restore backups/aevia-db-20261118T120000Z.sql.gz
./deploy.sh restore backups/aevia-data-20261118T120000Z.tar.gz
./deploy.sh up
```

Pull a tested source revision and use `./deploy.sh upgrade` to rebuild the registry image and recreate the containers. Migrations run at startup. Before deploying a schema-changing upgrade, take a fresh backup and rehearse restoration on a separate host. For a PostgreSQL major-version upgrade, create a new database volume and restore from a dump rather than reusing the old data directory.
