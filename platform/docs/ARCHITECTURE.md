# Architecture

## The shape

```
                      ┌──────────────────────────────────────────────┐
   browser ──────────▶│  aevia-registry (axum)                       │
                      │                                              │
   aevia-pkg ────────▶│  /api/v1/*   JSON        ┌── Leptos pages ──┐│
                      │  /crates /docs  HTML     │  rendered to a   ││
                      │                          │  String, no WASM ││
                      │                          └──────────────────┘│
                      └───────┬──────────────────────────┬───────────┘
                              │                          │
                   ┌──────────▼─────────┐      ┌─────────▼──────────┐
                   │ PostgreSQL         │      │ archive volume     │
                   │ (a container; no   │      │ {owner}/{name}/    │
                   │  host database)    │      │   {version}.tar.gz │
                   └────────────────────┘      └────────────────────┘
                              │
                   ┌──────────▼─────────────────────────────────────┐
                   │ aevia-doc  (the language's own doc generator)  │
                   │ runs in a blocking task at publish time        │
                   └────────────────────────────────────────────────┘
```

One binary serves the site, the API and the documentation. They share a
database, an archive store and one notion of identity; splitting them into
services would mean three places to keep a signature check identical.

## Why the crates are split this way

`aevia-registry-core` has no I/O at all. It parses, validates, hashes, packs and
verifies — and both the server and the client depend on it. That is what makes
the wire format a fact rather than a convention: the canonical statement the
client signs is produced by the same function the server uses to re-derive it,
so the two cannot drift.

`aevia-registry-db` owns SQL. `aevia-registry-docs` owns the compiler
integration. `aevia-registry-web` owns markup and receives only view models, so a
page can be tested without a database and a query can change without touching a
template. `aevia-registry-server` is the only crate that knows about all of
them.

## Publishing, step by step

```
client                                   server
  │  POST /api/v1/publish/challenge        │
  │  {name, version, checksum}             │
  │───────────────────────────────────────▶│  nonce = 16 random bytes
  │                                        │  statement = render(...)
  │◀───────────────────────────────────────│  INSERT challenges(nonce, statement, +5min)
  │  {nonce, statement, expires_at}        │
  │                                        │
  │  gpg --detach-sign --armor             │
  │  (the exact bytes of `statement`)      │
  │                                        │
  │  POST /api/v1/publish  (multipart)     │
  │  envelope + package.tar.gz             │
  │───────────────────────────────────────▶│  1 parse the statement
  │                                        │  2 sha256(archive) == statement.checksum
  │                                        │  3 UPDATE challenges … RETURNING  (single use)
  │                                        │  4 statement text == the one we issued
  │                                        │  5 fingerprint → signing_keys → publisher
  │                                        │  6 verify signature over the exact bytes
  │                                        │  7 publisher may publish as `owner`
  │                                        │  8 aevia.toml inside says the same name+version
  │                                        │  9 INSERT version, write archive, append log
  │◀───────────────────────────────────────│  201 {name, version, checksum, log}
  │                                        │  └ spawn: build documentation
```

The order matters. Steps 1–4 are cheap and reject replays; step 6 is the only
thing that can authorise the write; step 8 is what stops a tarball from
smuggling a different package past a valid signature. Nothing is written to the
catalogue before step 9.

Two details are easy to get wrong and are worth stating:

* **The nonce is spent before the signature is checked** (step 3). A failed
  attempt therefore costs a challenge, which is the right price: a valid
  signature can only ever be used once, and a verifier that consumed the nonce
  after verification would let two concurrent requests race on the same one.
* **The statement is compared byte for byte** (step 4). The server does not
  re-render from the parsed fields and compare those; it compares the text,
  because the signature covers text.

## Verifying a download

```
aevia-pkg add apich/units-extra
  │ GET /api/v1/download/apich/units-extra/0.2.0
  │   ← bytes + X-Aevia-Checksum + X-Aevia-Signature-Fingerprint
  │ sha256(bytes) == header                       ← integrity
  │ GET /api/v1/download/…/signature
  │   ← "aevia-publish-v1\nname: …\nexpires: …\n" + armoured signature
  │ split at the armour header                    ← the statement is the signed bytes
  │ GET /api/v1/keys/public?fingerprint=…         ← the key, from the registry
  │ verify(signature, statement, key)             ← provenance
```

The receipt is self-contained: statement plus signature. A verifier needs no
metadata from the server and no second trust decision — the statement names the
package, the version and the digest, and the signature proves who asked for that
text. The public key is fetched rather than taken from the download, because a
key that arrives with the artifact it signs proves nothing.

## Documentation

`aevia doc` writes Markdown. The registry renders HTML, from the same crate:
`aevia-doc::module_doc` parses each `src/**/*.aev` file with the real lexer and
parser, and `aevia-registry-docs` turns the declarations into pages with stable
anchors (`fn-natural-length`). A file that does not parse is skipped with a
warning rather than failing the whole build — one broken module must not blank a
package's site — and if nothing parses the build is marked `failed` with the
reason recorded on the version.

The build runs in `spawn_blocking` after the response is sent, so a publish does
not wait for a parse. Pages are written in one transaction (`replace_docs`): a
rebuild must not leave a sidebar that disagrees with the page beside it.

Documentation is versioned and immutable, like the archive. A later source tree
never changes an earlier version's pages.

## Identity

Two providers, no passwords:

* **GitHub, through a GitHub App.** The user-authorisation flow proves the
  handle; `/user/installations` proves organisation membership, which is what
  lets `apich-organization/units` be published by a member and nobody else.
* **ORCID.** OAuth with the `/authenticate` scope yields an iD and a name. This
  is the natural identity for a scientific language, so it is a first-class
  option rather than an afterthought.

Both end at the same place: a `publishers` row and a session cookie. The cookie
is only for the browser — publishing never uses it.

**Key binding.** `aevia-pkg login` sends a public key and receives a short code;
a signed-in human approves it at `/activate`, and the key becomes a row in
`signing_keys`. That is the whole of "login". A key may sign for exactly one
account, which is why `signing_keys` is unique on `(kind, fingerprint)`.

## The data model

```
publishers ──┬── signing_keys        (a key may sign for one account)
             ├── packages ─── versions ─┬── docs_pages
             │                          └── (downloads counter)
             └── org_members          (GitHub organisation membership)

challenges      one row per issued nonce, spent atomically
device_codes    one row per `aevia-pkg login`
sessions        browser sessions
oauth_states    OAuth `state` values, single use
publish_log     append-only hash chain
```

Three rules shape it:

1. **A version is immutable.** No column a signature covers is ever updated.
   The mutable ones are `yanked`, the documentation state and the download
   counter. A mistake is fixed by publishing a new version, not by editing
   history.
2. **Names are unique without case.** `packages.name_key` holds `lower(name)`
   and is unique per owner, so `Aevia` and `aevia` cannot both exist while the
   author's spelling is preserved for display.
3. **The log is a chain.** `publish_log.entry_hash` is
   `sha256(prev_hash ‖ kind ‖ summary ‖ payload)`. Appends take a
   transaction-scoped advisory lock so two publishes cannot read the same head
   and fork the chain; readers are never blocked.

## Concurrency

* Every mutating endpoint is a single statement or a transaction; there is no
  read-then-write across a network round trip except where a transaction covers
  it (`approve_device`, `replace_docs`).
* The log is the one place that must serialise, and it does so with
  `pg_advisory_xact_lock`, which is released by the database even if the process
  dies mid-transaction.
* Archive writes go to a `.partial` file and are renamed into place, so a
  download never reads a half-written archive.
* The pool is capped at 16 connections with a 5-second acquire timeout: a
  request that cannot get a connection is better off failing fast than holding a
  proxy slot.

## Known limits

Stated plainly, because a registry that hides its gaps is worse than one that
names them:

* **Versions are `major.minor.patch` only.** `aevia.toml` cannot express a
  pre-release, so the registry refuses one rather than accepting a version the
  toolchain cannot install.
* **SSH signatures are Ed25519 only.** RSA and ECDSA SSH keys are rejected with
  a message saying so; OpenPGP keys of any kind work.
* **No rate limiting.** The API has none yet; the deployment is expected to sit
  behind a proxy that can add it.
* **A module's `##` comment is attached to its first declaration** by
  `aevia-doc`, so a module-level description appears under the first
  declaration on the module page. That is the language toolchain's behaviour,
  not the registry's, and the fix belongs there.
* **Documentation has no search index.** Pages are stored and served; there is
  no full-text index over them.
