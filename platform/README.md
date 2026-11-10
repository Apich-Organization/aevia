# aevia platform — registry and documentation site

The service behind <https://aevia.apich.org>: a package registry for the Aevia
language, a documentation site built by the compiler, and the client that
publishes to them. Everything is Rust — `axum` for HTTP, `leptos` for the pages,
`sqlx` for PostgreSQL — and there is no Node.js, no bundler and no WASM bundle
anywhere in the build.

```
crates.io          →  aevia.apich.org/crates
docs.rs            →  aevia.apich.org/docs
the design standard →  aevia.apich.org/spec   (bilingual, rendered from spec/*.md)
cargo login        →  aevia-pkg login          (no token; a key binding)
cargo publish      →  aevia-pkg publish        (signed statement, not a bearer token)
```

The site speaks English and Chinese; the switch is in the top bar and the choice
is remembered in a cookie. `?lang=zh` works on any page and wins over the cookie,
so a link can name its language.

## What is here

| Path | What it is |
|---|---|
| `crates/aevia-registry-core` | Names, manifests, tarballs, signed statements, signature verification, index records, view models. No I/O, no database, no network. |
| `crates/aevia-registry-db` | The PostgreSQL schema and every query. Migrations are embedded and applied by the binary at start-up. |
| `crates/aevia-registry-docs` | Turns a published package into the HTML pages under `/docs`, using the language's own `aevia-doc` crate. |
| `crates/aevia-registry-web` | The Leptos pages. Server-rendered to `String`; no hydration, no client-side router. |
| `crates/aevia-registry-server` | The binary: routes, OAuth, publish verification, archive storage, background documentation builds. |
| `crates/aevia-pkg` | The client: `login`, `publish`, `yank`, `search`, `info`, `add`, `verify`, `key`, `status`. `aevia pkg …` in the language toolchain forwards to it. |
| `infra/` | Debian 13 container images, a Podman-compose stack, Quadlet units, a Caddyfile and a deploy script. |
| `docs/` | [Architecture](docs/ARCHITECTURE.md), [API](docs/API.md), [security model](docs/SECURITY.md), [deployment](docs/DEPLOY.md). |

## Quick start

```console
# 1. A database. Any PostgreSQL 14+ will do; the container in infra/ is one way.
podman run -d --name aevia-db -e POSTGRES_USER=aevia -e POSTGRES_PASSWORD=aevia \
    -e POSTGRES_DB=aevia -p 55432:5432 postgres:18-trixie

# 2. The server. It applies the schema itself on first start.
export DATABASE_URL=postgres://aevia:aevia@127.0.0.1:55432/aevia
export AEVIA_SESSION_SECRET=$(openssl rand -hex 32)
export AEVIA_STORAGE_DIR=$PWD/target/packages
export AEVIA_BASE_URL=http://127.0.0.1:8080
cargo run -p aevia-registry-server

# 3. The site.
open http://127.0.0.1:8080
```

Building the workspace needs Rust 1.97 or newer. The `aevia-registry-docs` crate
depends on `../crates/aevia-doc` — the language repository checked out next to
this one — because documentation is built by the compiler that will run the
package. Build with `--no-default-features -p aevia-registry-docs` only if you
are working on the platform without that checkout.

## Development

`scripts/dev.sh` wraps the quick start in one command — a PostgreSQL container
on `127.0.0.1:55432` and the environment the server reads:

```console
./scripts/dev.sh up      # start the database, print the exports
./scripts/dev.sh run     # up, then cargo run -p aevia-registry-server
./scripts/dev.sh reset   # drop and recreate the schema
./scripts/dev.sh down    # stop and remove the container
```

The tests that need a database read `AEVIA_TEST_DATABASE_URL` and skip
themselves when it is unset; point it at that container
(`postgres://aevia:aevia@127.0.0.1:55432/aevia`) to run them. The specification
pages are read from `AEVIA_SPEC_DIR`, by default `../spec`.

## Publishing

There is no `cargo login` equivalent that hands out a token, and no long-lived
credential on disk. A publish is authorised by a signature over a short,
canonical statement:

```console
aevia-pkg login            # binds your key to your account, once
aevia-pkg publish          # from a package directory with aevia.toml
```

(`aevia pkg …` works too: the language CLI forwards to `aevia-pkg`, which it
finds on `PATH`, beside itself, or at `$AEVIA_PKG`.)

`login` prints a code and a URL. You open the URL, sign in with GitHub or ORCID,
and confirm the key's fingerprint; the terminal polls until the binding exists.
After that, `publish` needs only the private key — which never leaves
`gpg-agent` or `ssh-agent`.

```
aevia-publish-v1
name: apich/units-extra
version: 0.2.0
checksum: sha256:7cb292ee…
nonce: 4d1a…
expires: 1794000300
```

The server issues that text (nonce and expiry included), the client signs the
exact bytes with `gpg --detach-sign --armor` or `ssh-keygen -Y sign`, and the
server verifies it before anything is written. A captured signature names one
archive and dies in five minutes, so replaying it publishes nothing — the
registry spends the nonce and refuses the second attempt.

## The three surfaces

**`/`** — what the registry is, how many packages it holds, and the three ideas
behind it: identities come from GitHub or ORCID, publishing is a signature, and
documentation is built by the compiler.

**`/crates`** — search, sort, and a page per package with its versions,
dependencies, README, checksums, signing key and yank state. Names are scoped to
a verified identity (`apich/units-extra`), so a package cannot be squatted
before its owner exists.

**`/docs`** — the documentation site. Each version's pages are rendered at
publish time from the `##` comments in its source by `aevia-doc`, so the site
shows the API the compiler sees, not a second parser's guess. Pages are stored
with the version and never rebuilt from a later source tree.

**`/spec`** — the language specification, rendered from the language
repository's `spec/*.md` at start-up: one page per chapter with a sticky
sidebar, a per-page table of contents and CJK-aware anchors, plus `/spec/all`,
the single-page edition the bundler produces, generated on request instead of
checked in. The chapters are normative Chinese; every chapter carries an English
title and abstract so an English reader can find what they need. Point
`AEVIA_SPEC_DIR` at the `spec/` directory (the container mounts it read-only).

## Where this departs from the Rust ecosystem

These are deliberate, and each is argued in [ARCHITECTURE.md](docs/ARCHITECTURE.md):

| Cargo / crates.io | Here | Why |
|---|---|---|
| `cargo login` stores a bearer token | a key binding plus a signed statement per action | a token is a transferable secret; a signature is a proof that cannot be replayed |
| flat global namespace, first-come-first-served | `owner/name` scoped to a verified GitHub or ORCID identity | no squatting, no ten-minute name reservation, no support tickets |
| `cargo publish` allows deletes | versions are immutable; `yank` only | a deleted version breaks every lock file that names it |
| index records carry a checksum | index records carry the checksum *and* the signature that authorised it | provenance can be checked before the download, from the index alone |
| no public append-only record | a hash-chained transparency log at `/log` | the registry cannot quietly rewrite its own history |
| docs built by a separate service, days later | built at publish, by the same compiler | the page cannot describe an API the package does not have |

## Deployment

`infra/` targets **aarch64 Linux with rootless Podman** and Debian 13 images:

```console
cd infra
cp .env.example .env && $EDITOR .env     # set the database password and session secret
./deploy.sh build && ./deploy.sh up
```

The full operator guide — DNS, the GitHub App and ORCID application, backups,
upgrades, the Quadlet alternative — is [docs/DEPLOY.md](docs/DEPLOY.md).

## Tests

```console
cargo test --workspace
```

The tests come in three layers:

* **Unit** — name and manifest rules, deterministic packing, statement
  canonicalisation, signature verification, SQL helpers, page rendering in both
  languages, the specification loader, the documentation builder.
* **HTTP integration** (`crates/aevia-registry-server/tests/api.rs`) — real
  requests through the real router against a real PostgreSQL: health, stats,
  challenge validation, a full signed publish that is then replayed and refused,
  index, download, yank, 404s and the language switch. They **skip themselves**
  unless `AEVIA_TEST_DATABASE_URL` is set:

  ```console
  docker run -d --name aevia-tests-db \
      -e POSTGRES_USER=aevia -e POSTGRES_PASSWORD=aevia \
      -e POSTGRES_DB=aevia -p 55433:5432 postgres:18-trixie
  AEVIA_TEST_DATABASE_URL=postgres://aevia:aevia@127.0.0.1:55433/aevia cargo test --workspace
  ```

  The signed-publish test shells out to `gpg` and skips when it is absent.
* **Client** (`crates/aevia-pkg`) — the CLI parser, config round-trips, and the
  statement-mismatch refusals.

`scripts/dev.sh` brings up the same database for development; CI runs the
database-backed job on every push.

## Licence

Apache-2.0, matching the language repository.
