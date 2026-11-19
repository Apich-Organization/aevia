# Registry HTTP API

[简体中文](API.zh.md)

All paths are relative to `https://aevia.apich.org`.

Requests and responses generally use JSON. The sparse index, key and signature endpoints return text, and archive downloads return gzip bytes. Registry errors use this shape.

```json
{"error": {"code": "E_SIGNATURE", "message": "the OpenPGP signature does not verify …"}}
```

| Code | Status | Meaning |
|---|---|---|
| `E_NAME` | 400 | not an `owner/name` identity, or a name that breaks the rules |
| `E_VERSION` | 400 | not a SemVer version, or it carries `+build` metadata (`1.2.3` and `1.2.3-rc.1` are accepted) |
| `E_DEPENDENCY` | 400 | invalid dependency or malformed `/resolve` request |
| `E_RESOLVE` | 422 | `POST /api/v1/resolve`: the requirements cannot be satisfied together (the message is the explanation) |
| `E_INTEGRITY` | 500 | the stored archive no longer hashes to the checksum the catalogue and the signature name |
| `E_MANIFEST` | 400 | `aevia.toml` inside the archive does not parse, or names a different package |
| `E_TARBALL` | 400 or 413 | the archive is malformed, exceeds its size limits, or differs from the signed digest |
| `E_STATEMENT` | 400 | malformed, expired, wrong kind, or a nonce that was already spent |
| `E_SIGNATURE` | 403 | the signature does not verify |
| `E_KEY` | 403 | the key is not bound to any account, or is malformed |
| `E_FORBIDDEN` | 403 | the signer may not publish under that scope |
| `E_NOT_FOUND` | 404 | no such package, version or key |
| `E_CONFLICT` | 409 | the version is already published or the key is already bound |
| `E_DB` | 500 | the registry could not complete the request |
| `E_RATE_LIMIT` | 429 | retry after the `Retry-After` interval |

## Reading

### `GET /api/v1/stats`

```json
{"packages": 12, "versions": 31, "downloads": 4096, "publishers": 7}
```

### `GET /api/v1/search?q=&sort=&page=`

`sort` accepts `relevance`, `downloads`, `recent`, and `name`. Page numbers start at 1, with 20 results per page.

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
record for a version wins. A yank updates the affected record’s `yanked` flag. Unknown fields must be ignored by readers.

```json
{"name":"apich/plot","vers":"2.1.0","deps":[
   {"package":"apich/units","req":"^1.2"},
   {"package":"apich/linalg","req":">=0.3, <0.5","import":"Matrix"}],
 "cksum":"sha256:7cb292ee…","yanked":false,"published_at":"2026-11-08T11:50:50Z",
 "docs":true,"size":10240,
 "signature":{"kind":"openpgp","fingerprint":"7AECB528…","signature":"-----BEGIN PGP SIGNATURE-----…"}}
```

Each dependency record names a registry package `package` in lower-case `owner/name` form and a canonical version requirement `req`, such as `^1.2`, `~1.2.3`, `>=1.2, <2`, `1.*`, `=1.2.3`, or `*`. `import` gives the import root when it differs from the default for the package name. The default root of `units-extra` is `UnitsExtra`. [Module specification](../../spec/en/07-modules.md)

Send the index’s `ETag` back as `If-None-Match` to receive 304 when the catalogue record is unchanged. `Cache-Control` is
`no-cache`, so a yank reaches a client at its next resolution.

The index identifies the signing key and archive checksum. Retrieve the signature receipt for the exact signed statement. The records are listed in publication order.

The resolver selects the highest version satisfying the requirements, with at most one version per compatibility series by default (`1.x`, `0.3.x`, or `0.0.4`). A pre-release qualifies when a requirement names a pre-release of the same `major.minor.patch`. An already locked version remains selectable after a yank.

### `GET /api/v1/crates/{owner}/{name}/dependents`

Reverse dependencies: the newest non-yanked version of each package that
depends on this one.

```json
{"package": "apich/units", "total": 2,
 "dependents": [{"package": "apich/plot", "version": "2.1.0", "req": "^1.2"}]}
```

`dependents` holds at most 100 entries ordered by name. `total` counts every distinct dependent.

### `POST /api/v1/resolve`

Runs the resolver server-side over the catalogue, for clients that would rather
not download one index file per transitive package.

```json
{"dependencies": [{"package": "apich/plot", "req": "^2", "import": "Plot"}],
 "locked": [{"package": "apich/units", "version": "1.1.0"}],
 "unify": "series"}
```

`locked` versions are preferred, and stay selectable if yanked. `unify` is
`series` by default, which allows incompatible majors to coexist. Use `package` for a single version per package. A successful response returns HTTP 200.

```json
{"packages": [{"package": "apich/plot", "version": "2.1.0", "checksum": "sha256:…", "yanked": false}],
 "edges": [{"from": "request", "import": "Plot", "to": "apich/plot 2.1.0"}]}
```

`422 E_RESOLVE` carries the resolver’s explanation in `message`. A malformed request returns `400 E_DEPENDENCY`.

### `GET /api/v1/download/{owner}/{name}/{version}`

The archive endpoint supports validation with `If-None-Match` and returns the following headers.

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

The armoured public key as `text/plain`. Certificates of retired keys remain accessible for historical verification. `/keys` reports current binding status.

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
 "key": {"kind": "openpgp", "armored": "<armoured OpenPGP public key>"},
 "description": "optional", "license": "Apache-2.0", "keywords": ["units"], "repository": "https://…"}
```

A successful publish returns HTTP 201. The `ssh` key kind accepts `ssh-ed25519` public keys.

```json
{"name": "apich/units-extra", "version": "0.2.0", "checksum": "sha256:…",
 "docs_url": "/docs/apich/units-extra/0.2.0", "log": {"seq": 1, "hash": "…"},
 "warnings": []}
```

When signed with an `ssh` (`ssh-ed25519`) key, `warnings` includes a non-fatal
recommendation to switch to an OpenPGP (PGP) key, ideally hardware-backed. OpenPGP
signatures return `warnings: []` (or omit empty warnings).

The server combines envelope metadata with `aevia.toml` and `aevia-pkg.toml` from the archive. A publication must include a description and either a license expression or a nonempty license file. The returned documentation URL may take a short time to become available.

Before anything is written the server checks the archive's `aevia.toml`
dependencies (`E_DEPENDENCY`): none may be a bare `path`, none may name the
package itself, each must name a package on the registry, and each requirement
must be satisfied by some non-yanked published version. The archive is durably installed before the catalogue, dependency rows and log entry commit in one database transaction.

### `POST /api/v1/yank/challenge` and `POST /api/v1/yank`

Send `name`, `version`, and `yanked` to request a statement. Use `yanked: true` to yank or `yanked: false` to restore. The reply has the same fields as the publish challenge. Sign that statement and submit it with the key.

```json
{"statement": "<the exact text>", "signature": "<armoured>", "key": {"kind": "openpgp", "armored": "…"}}
```

`204` on success. Yanking is reversible: `"yanked": false` restores. The flag and
its log entry change in one transaction, so the index and the transparency log
never disagree. A yanked version is invisible to new resolutions but stays
selectable by a lock file that already names it. The archive and checksum remain available.

### `POST /api/v1/auth/device`

```json
{"kind": "openpgp", "public_key": "<armoured OpenPGP public key>", "label": "work laptop"}
```

`kind: "openpgp"` (preferably hardware-backed) is recommended. When `kind` is
`"ssh"`, only `ssh-ed25519` keys are accepted (`403` `E_KEY` otherwise) and
`warnings` includes a recommendation to prefer OpenPGP and hardware keys:

```json
{"device_code": "…", "user_code": "ABCD-EFGH",
 "verification_uri": "https://aevia.apich.org/activate?code=ABCD-EFGH",
 "expires_at": "2026-11-08T12:15:00Z", "interval": 5,
 "warnings": []}
```

`409` if the key is already bound to an account.

### `GET /api/v1/auth/device/{device_code}`

```json
{"status": "pending"}
{"status": "approved", "publisher": {"handle": "apich", "kind": "github", "display_name": "…"}}
```

`status` is `pending`, `approved`, `denied` or `expired`. Poll every `interval`
seconds until the status changes from `pending`.

## Interactive tutorial

`POST /api/v1/playground/run` accepts a JSON object with a single `source` string containing up to 16 KiB of Aevia code. It runs the program in a fresh restricted container when a worker is configured.

```json
{"source":"fn main(): IO Unit = do\n    println(\"Hello\")\n"}
```

The result has `status` (`success`, `failed`, `timeout`, or `output_limit`), `stdout` and `stderr`. HTTP 429 indicates capacity or a request rate limit. HTTP 503 means the worker is unavailable. [Worker configuration](DEPLOY.md#enable-book-execution)

## Pages

| Path | What it serves |
|---|---|
| `/` | the front page |
| `/crates?q=&sort=&page=` | search by name, description and keywords with `q` |
| `/cli` | how the site and the `aevia` command line fit together, command by command |
| `/users/{handle}` | a publisher's public page: identity, join date, packages |
| `/feed.xml` | Atom feed of the 50 newest releases |
| `/sitemap.xml`, `/robots.txt` | crawler guidance, with `/account`, `/auth` and `/api` disallowed in `robots.txt` |
| `/badge/{owner}/{name}.svg` | `aevia \| 1.2.0` badge for a README (newest non-yanked version) |
| `/crates/{owner}/{name}` | package page |
| `/crates/{owner}/{name}/{version}` | the same page pinned to a version |
| `POST /crates/{owner}/{name}/yank` | yank from the browser (session auth, form `version`, `yanked`) |
| `/docs` | documentation landing |
| `/docs/{owner}/{name}` | redirects to the newest documented version |
| `/docs/{owner}/{name}/{version}` | module list, README and coverage. Returns 202 while pending |
| `/docs/{owner}/{name}/{version}/{module}` | one module |
| `/account` | keys and packages (session required) |
| `POST /account/keys` | add a key (form `armored`, `label`) |
| `POST /account/keys/{id}/revoke` | revoke a key |
| `/login?next=` | choose a provider |
| `/auth/{github,orcid}/start` | begin OAuth |
| `/auth/{github,orcid}/callback` | OAuth return |
| `POST /logout` | end the session |
| `/log` | the transparency log |
| `/spec` | the specification chapter list |
| `/book`, `/architecture` | bilingual tutorials and architecture |
| `/book/{slug}`, `/architecture/{slug}` | one localized manual chapter |
| `/book/all`, `/architecture/all` | single-page manuals |
| `/book/download`, `/architecture/download` | standalone HTML attachments |
| `/stdlib` | the standard library reference |
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

`GET /lang/zh` sets the preference cookie and returns to a same-origin `Referer`, removing its prior `lang` query parameter.

## Statements

The canonical text, exactly as it must be signed. Both kinds are LF-terminated
and every field appears once in the order shown.

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
