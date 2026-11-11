# HTTP API

Base: `https://aevia.apich.org`. Every path below is relative to it.

JSON in, JSON out, except the two endpoints that return text (the sparse index
and the signature receipt) and the archive download. Errors are always:

```json
{"error": {"code": "E_SIGNATURE", "message": "the OpenPGP signature does not verify …"}}
```

| Code | Status | Meaning |
|---|---|---|
| `E_NAME` | 400 | not an `owner/name` identity, or a name that breaks the rules |
| `E_VERSION` | 400 | not a SemVer version, or it carries `+build` metadata (`1.2.3` and `1.2.3-rc.1` are accepted) |
| `E_DEPENDENCY` | 400 | a dependency of the published manifest is a bare `path`, depends on the package itself, names a package that is not on the registry, or has no non-yanked version satisfying its requirement; or a `/resolve` request is malformed |
| `E_RESOLVE` | 422 | `POST /api/v1/resolve`: the requirements cannot be satisfied together (the message is the explanation) |
| `E_INTEGRITY` | 500 | the stored archive no longer hashes to the checksum the catalogue and the signature name |
| `E_MANIFEST` | 400 | `aevia.toml` inside the archive does not parse, or names a different package |
| `E_TARBALL` | 400 | the archive is malformed, too large, or not the one that was signed |
| `E_STATEMENT` | 400 | malformed, expired, wrong kind, or a nonce that was already spent |
| `E_SIGNATURE` | 403 | the signature does not verify |
| `E_KEY` | 403 | the key is not bound to any account, or is malformed |
| `E_FORBIDDEN` | 403 | the signer may not publish under that scope |
| `E_NOT_FOUND` | 404 | no such package, version or key |
| `E_CONFLICT` | 409 | the version is already published; a key is already bound |
| `E_DB` | 500 | the registry could not complete the request |

## Reading

### `GET /api/v1/stats`

```json
{"packages": 12, "versions": 31, "downloads": 4096, "publishers": 7}
```

### `GET /api/v1/search?q=&sort=&page=`

`sort` is `relevance`, `downloads`, `recent` or `name`; `page` is one-based;
20 results per page.

```json
{"total": 12, "page": 1, "per_page": 20, "query": "units", "sort": "recent",
 "items": [{"id": "apich/units-extra", "description": "Extra unit systems …",
            "version": "0.2.0", "updated_at": "2026-11-08T11:50:50Z",
            "downloads": 3, "has_docs": true}]}
```

### `GET /api/v1/crates/{owner}/{name}`

The whole package page as data: `id`, `description`, `license`, `repository`,
`keywords`, `readme_html`, `versions[]`, `selected`, `owners[]`,
`dependencies[]`, `created_at`, `updated_at`, `downloads`, `can_manage`.

### `GET /api/v1/index/config.json`

```json
{"dl": "https://aevia.apich.org/api/v1/download",
 "api": "https://aevia.apich.org/api/v1", "registry": "aevia"}
```

### `GET /api/v1/index/{owner}/{name}`

`text/plain`, one JSON record per line, oldest published first. The newest
record for a version wins; a yank rewrites only the `yanked` flag of the record
it applies to. Unknown fields must be ignored by readers.

```json
{"name":"apich/plot","vers":"2.1.0","deps":[
   {"package":"apich/units","req":"^1.2"},
   {"package":"apich/linalg","req":">=0.3, <0.5","import":"Matrix"}],
 "cksum":"sha256:7cb292ee…","yanked":false,"published_at":"2026-11-08T11:50:50Z",
 "docs":true,"size":10240,
 "signature":{"kind":"openpgp","fingerprint":"7AECB528…","signature":"-----BEGIN PGP SIGNATURE-----…"}}
```

`deps` is what a resolver needs. `package` is `owner/name` with the name folded
to lower case; `req` is a version requirement in its canonical rendering
(`^1.2`, `~1.2.3`, `>=1.2, <2`, `1.*`, `=1.2.3`, `*`; the meaning is
`spec/07-modules.md` §7.5.1.1); `import` is the import root the dependent uses,
present only when it differs from the default for the package name
(`units-extra` → `UnitsExtra`). A published manifest cannot contain a bare
`path` dependency, so every entry has a registry identity.

The body carries an `ETag`; send it back as `If-None-Match` and the registry
answers `304` while nothing was published or yanked. `Cache-Control` is
`no-cache`, so a yank reaches a client at its next resolution.

The signature is over the publish statement for that version, so a client can
check provenance from the index alone. Versions are listed in publication
order; a client must sort by version itself.

Resolution rules a client implements (the `aevia-resolve` crate is the
reference): highest version satisfying every requirement; a pre-release only
when a requirement names one of the same `major.minor.patch`; yanked versions
only when already locked; at most one version per compatibility series
(`1.x`, `0.3.x`, `0.0.4`).

### `GET /api/v1/crates/{owner}/{name}/dependents`

Reverse dependencies: the newest non-yanked version of each package that
depends on this one.

```json
{"package": "apich/units", "total": 2,
 "dependents": [{"package": "apich/plot", "version": "2.1.0", "req": "^1.2"}]}
```

`dependents` holds at most 100 entries ordered by name; `total` counts every
distinct dependent.

### `POST /api/v1/resolve`

Runs the resolver server-side over the catalogue, for clients that would rather
not download one index file per transitive package.

```json
{"dependencies": [{"package": "apich/plot", "req": "^2", "import": "Plot"}],
 "locked": [{"package": "apich/units", "version": "1.1.0"}],
 "unify": "series"}
```

`locked` versions are preferred, and stay selectable if yanked. `unify` is
`series` (default; incompatible majors may coexist) or `package` (one version
per package). `200`:

```json
{"packages": [{"package": "apich/plot", "version": "2.1.0", "checksum": "sha256:…", "yanked": false}],
 "edges": [{"from": "request", "import": "Plot", "to": "apich/plot 2.1.0"}]}
```

`422 E_RESOLVE` carries the resolver's explanation in `message`; `400
E_DEPENDENCY` is a malformed request.

### `GET /api/v1/download/{owner}/{name}/{version}`

The archive bytes, with:

| Header | Value |
|---|---|
| `X-Aevia-Checksum` | `sha256:<hex>` |
| `X-Aevia-Signature-Fingerprint` | the key that signed |
| `ETag` | the checksum, quoted |
| `Cache-Control` | `public, max-age=31536000, immutable` |

### `GET /api/v1/download/{owner}/{name}/{version}/signature`

`text/plain`: the publish statement **exactly as signed**, immediately followed
by the armoured detached signature. A client splits at the first armour header
and verifies the statement's bytes — including the newline that ends its last
field.

### `GET /api/v1/keys?fingerprint=…&kind=openpgp|ssh`

```json
{"registered": true, "publisher": {"handle": "apich", "kind": "github", "display_name": "…"}}
```

`kind` is optional. The fingerprint is a query parameter, not a path segment,
because an SSH fingerprint is `SHA256:<base64>` and base64 contains `/`.

### `GET /api/v1/keys/public?fingerprint=…`

`text/plain`: the armoured public key. Used to verify a download.

### `GET /api/v1/log?from=0&limit=100`

```json
{"head": {"seq": 3, "hash": "a6261193…", "at": "2026-11-08T12:01:00Z"},
 "entries": [{"seq": 1, "kind": "publish", "summary": "apich published apich/units-extra 0.2.0",
              "hash": "…", "prev": "0000…", "at": "…"}]}
```

`kind` is `publish`, `yank` or `key`. `limit` is capped at 500.

## Writing

### `POST /api/v1/publish/challenge`

```json
{"name": "apich/units-extra", "version": "0.2.0", "checksum": "sha256:<64 hex>"}
```

```json
{"nonce": "4d1a…", "statement": "aevia-publish-v1\nname: …\nexpires: 1794000300\n",
 "expires_at": "2026-11-08T12:05:00Z"}
```

Sign the `statement` bytes as they are. Do not re-render them.

### `POST /api/v1/publish`

`multipart/form-data`:

| Part | Type | Content |
|---|---|---|
| `envelope` | `application/json` | the JSON below |
| `package` | `application/gzip` | the `.tar.gz` |

```json
{"statement": "<the exact text>", "signature": "<armoured detached signature>",
 "key": {"kind": "openpgp", "armored": "<armoured public key>"},
 "description": "optional", "license": "Apache-2.0", "keywords": ["units"], "repository": "https://…"}
```

`201`:

```json
{"name": "apich/units-extra", "version": "0.2.0", "checksum": "sha256:…",
 "docs_url": "/docs/apich/units-extra/0.2.0", "log": {"seq": 1, "hash": "…"}}
```

The envelope's metadata is optional: the server falls back to `aevia-pkg.toml`
inside the archive, then to the README.

Before anything is written the server checks the archive's `aevia.toml`
dependencies (`E_DEPENDENCY`): none may be a bare `path`, none may name the
package itself, each must name a package on the registry, and each requirement
must be satisfied by some non-yanked published version. The catalogue row, its
dependency rows, the archive file and the log entry are written in one
transaction.

### `POST /api/v1/yank/challenge` and `POST /api/v1/yank`

Challenge: `{"name", "version", "yanked": true|false}` → same shape as the
publish challenge. Then:

```json
{"statement": "<the exact text>", "signature": "<armoured>", "key": {"kind": "openpgp", "armored": "…"}}
```

`204` on success. Yanking is reversible: `"yanked": false` restores. The flag and
its log entry change in one transaction, so the index and the transparency log
never disagree. A yanked version is invisible to new resolutions but stays
selectable by a lock file that already names it; its archive and checksum are
unchanged.

### `POST /api/v1/auth/device`

```json
{"kind": "openpgp", "public_key": "<armoured>", "label": "work laptop"}
```

```json
{"device_code": "…", "user_code": "ABCD-EFGH",
 "verification_uri": "https://aevia.apich.org/activate?code=ABCD-EFGH",
 "expires_at": "2026-11-08T12:15:00Z", "interval": 5}
```

`409` if the key is already bound to an account.

### `GET /api/v1/auth/device/{device_code}`

```json
{"status": "pending"}
{"status": "approved", "publisher": {"handle": "apich", "kind": "github", "display_name": "…"}}
```

`status` is `pending`, `approved`, `denied` or `expired`. Poll every `interval`
seconds; stop on anything but `pending`.

## Pages

| Path | What it serves |
|---|---|
| `/` | the front page |
| `/crates?q=&sort=&page=` | search and listing |
| `/crates/{owner}/{name}` | package page |
| `/crates/{owner}/{name}/{version}` | the same page pinned to a version |
| `POST /crates/{owner}/{name}/yank` | yank from the browser (session auth, form `version`, `yanked`) |
| `/docs` | documentation landing |
| `/docs/{owner}/{name}` | redirects to the newest documented version |
| `/docs/{owner}/{name}/{version}` | module list, README, documented share |
| `/docs/{owner}/{name}/{version}/{module}` | one module |
| `/account` | keys and packages (session required) |
| `POST /account/keys` | add a key (form `armored`, `label`) |
| `POST /account/keys/{id}/revoke` | revoke a key |
| `/login?next=` | choose a provider |
| `/auth/{github,orcid}/start` | begin OAuth |
| `/auth/{github,orcid}/callback` | OAuth return |
| `POST /logout` | end the session |
| `/log` | the transparency log |
| `/spec` | the specification's chapter list |
| `/spec/{slug}` | one chapter, e.g. `/spec/04-dimensions` |
| `/spec/all` | the single-page edition, generated on request |
| `/lang/{code}` | remembers `en` or `zh` in a cookie and returns to the page |
| `/activate?code=` | approve a device key (GET form, POST approve) |
| `/static/site.css`, `/static/app.js` | assets |
| `/healthz` | `200` when the database answers, `503` otherwise |

## Language

Every page renders in English or Chinese. The language comes from, in order:
`?lang=` on the request, the `aevia_lang` cookie (set by `/lang/{code}`), the
`Accept-Language` header, and finally English. The `lang` attribute on `<html>`
follows it, so a browser's font and line-breaking rules apply correctly.

`GET /lang/zh` sets the cookie and redirects to the `Referer`'s path — reduced
to a same-site path, so the endpoint is not an open redirect.

## Statements

The canonical text, exactly as it must be signed. Both kinds are LF-terminated
and every field appears once, in this order; anything else is refused.

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

Statements live five minutes. The nonce is spent by the first request that
carries it, whatever the outcome of the remaining checks.
