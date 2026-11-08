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
| `E_VERSION` | 400 | not `major.minor.patch` |
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

`text/plain`, one JSON record per line, oldest first. The newest record for a
version wins; a yank rewrites only the `yanked` flag of the record it applies to.

```json
{"name":"apich/units-extra","vers":"0.2.0","deps":[],"cksum":"sha256:7cb292ee…",
 "yanked":false,"published_at":"2026-11-08T11:50:50Z","docs":true,"size":10240,
 "signature":{"kind":"openpgp","fingerprint":"7AECB528…","signature":"-----BEGIN PGP SIGNATURE-----…"}}
```

The signature is over the publish statement for that version, so a client can
check provenance from the index alone.

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

### `POST /api/v1/yank/challenge` and `POST /api/v1/yank`

Challenge: `{"name", "version", "yanked": true|false}` → same shape as the
publish challenge. Then:

```json
{"statement": "<the exact text>", "signature": "<armoured>", "key": {"kind": "openpgp", "armored": "…"}}
```

`204` on success. Yanking is reversible: `"yanked": false` restores.

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
| `/activate?code=` | approve a device key (GET form, POST approve) |
| `/static/site.css`, `/static/app.js` | assets |
| `/healthz` | `200` when the database answers, `503` otherwise |

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
