# Security model

## What is being protected

| Asset | Where it lives | What a compromise would cost |
|---|---|---|
| Package archives | the archive volume | arbitrary code delivered to every user of a package |
| The catalogue | PostgreSQL | wrong versions, wrong owners, a registry that lies |
| Publisher identities | `publishers`, `signing_keys` | impersonation: publishing as someone else |
| Sessions | an HttpOnly cookie | control of an account's keys and packages |
| The log | `publish_log` | the ability to rewrite history unnoticed |

## Trust boundaries

1. **The registry trusts a signature, not a caller.** Every mutating API request
   carries a signed statement. There is no API token to steal, no session to
   ride, and no ambient authority: the statement names one package, one version
   and one archive digest.
2. **The registry trusts GitHub and ORCID for identity**, and nothing else.
   It never sees a password and stores no provider token after the sign-in
   request completes.
3. **The registry trusts the compiler for documentation.** Pages are rendered
   from the package's own source by `aevia-doc`; the registry does not
   re-implement the language and cannot disagree with it.
4. **The registry does not trust package content.** Archives are parsed for a
   manifest, unpacked only in memory for documentation, and served byte for
   byte. No package code is ever executed.

## Publish authorisation

The full check list is in [ARCHITECTURE.md](ARCHITECTURE.md#publishing-step-by-step).
The properties that matter:

* **Replay is useless.** The statement carries a server-issued nonce with a
  five-minute life, and the nonce is spent by an atomic
  `UPDATE … WHERE consumed_at IS NULL … RETURNING`. A captured request fails on
  its second use with `E_STATEMENT`.
* **A signature covers the bytes, not a summary.** The server compares the
  statement text it issued with the text that was signed, then verifies the
  signature over that text. It never re-renders and compares fields.
* **The archive is bound to the statement.** `sha256(archive)` must equal the
  digest inside the signed statement, and the `aevia.toml` *inside* the archive
  must name the same package and version. A valid signature over a statement
  therefore cannot be reused for different bytes.
* **Scope is checked separately from identity.** A key proves who is asking; the
  statement's owner segment is checked against that publisher's own handle or an
  organisation membership confirmed through the GitHub App.

## Identity binding

A key may sign for exactly one account: `signing_keys` is unique on
`(kind, fingerprint)`, and `approve_device` binds a key only when no other
publisher holds it.

Binding happens once, in a browser:

```
aevia-pkg login  →  POST /auth/device  →  {device_code (32 bytes), user_code (8 chars)}
                                            │
                          human opens /activate?code=…  →  signs in  →  approves
                                            │
                          GET /auth/device/{device_code}  →  approved
```

The short code is deliberately low-entropy because a human retypes it. It
authorises nothing on its own: approving requires a signed-in session, it
expires in fifteen minutes, and guessing one gets an attacker the ability to…
approve someone else's key, which they cannot do without a session.

## Sessions

* 32 random bytes from the OS generator, stored as the primary key of
  `sessions`.
* Cookie flags: `HttpOnly`, `SameSite=Lax`, `Path=/`, and `Secure` whenever the
  public origin is HTTPS. `Lax` is what stops a cross-site form from acting as
  the reader; there is no separate CSRF token, and that is the trade recorded
  here rather than left implicit.
* 30-day lifetime, deleted on sign-out, purged hourly once expired.
* The cookie is only for the browser. The API ignores it entirely.

## OAuth

Both providers use a `state` value stored server-side (`oauth_states`) and
deleted on use, so a callback cannot be forged or replayed. The redirect target
is sanitised by `safe_next`: only a same-site path survives, which closes the
open-redirect hole that would otherwise turn the registry's sign-in link into a
phishing hop.

## Upload safety

An archive from the internet is hostile until proven otherwise:

* 16 MiB compressed and 64 MiB uncompressed caps, 4096 entries.
* Absolute paths, `..` components, drive letters and NUL bytes are refused.
* Symlinks, hardlinks and device nodes are refused outright — a package is
  regular files or it is not a package.
* Documentation is built from the archive *in memory*; nothing from a publish
  is ever extracted to the filesystem except the archive itself.
* Archives are written to a `.partial` file and renamed, so a reader never sees
  a half-written package.

## Author content on a privileged origin

`/crates/{owner}/{name}` shows a README the package author wrote, and it is
served from the same origin as `/account`. Unsanitised HTML there would be
stored cross-site scripting with the reader's session in reach — a README could
`fetch('/account/keys', {method: 'POST'})` and bind an attacker's key to a
maintainer's account.

`render_markdown` therefore runs every page through `ammonia` with an allowlist
that keeps what READMEs actually use (badges, `<details>`, `<kbd>`, tables,
images) and drops scripts, event handlers, `style`, frames and `javascript:`
URLs. Every link is given `rel="noopener noreferrer"`.

Caddy adds a Content-Security-Policy on top; it is a second line, not the fix.

## The transparency log

`publish_log` is append-only and hash-chained:
`entry_hash = sha256(prev_hash ‖ kind ‖ summary ‖ payload)`. Appends take a
transaction-scoped advisory lock, so two concurrent publishes cannot fork the
chain.

A client that records the head hash can later re-read the chain and prove that
nothing before that head changed. It does not prevent an operator from rewriting
history; it makes a rewrite detectable by anyone who kept a head.

## Secrets

| Secret | Where it comes from | Notes |
|---|---|---|
| `AEVIA_SESSION_SECRET` | environment, ≥32 bytes hex | rotating it signs everyone out; that is the intended blast radius |
| `DATABASE_URL` | environment | the database is a container on a private network with no published port |
| GitHub client secret, ORCID client secret | environment | never logged |
| GitHub App private key | a file mounted read-only | optional; only needed for organisation membership |

Nothing is baked into an image. `infra/.env` is git-ignored, excluded from the
build context, and `deploy.sh` refuses to start with the placeholder password.

## Known gaps

Named rather than hidden:

* **No rate limiting.** Challenge issuance and sign-in are unthrottled; the
  deployment expects a proxy in front. Adding a token bucket keyed by client IP
  is the obvious next step, and `AEVIA_TRUST_PROXY` already exists so the key
  would be meaningful.
* **OpenPGP key expiry is not enforced.** A signature made by a key that has
  since expired still verifies, because the registry checks the signature and
  the binding, not the key's own validity period. Revocation *is* enforced:
  a revoked key cannot sign.
* **SSH signatures are Ed25519 only.** RSA and ECDSA SSH keys are refused with a
  message rather than accepted without checking.
* **No package content analysis.** The registry stores what was signed; it does
  not scan for malicious code, and the signature proves provenance, not intent.
* **No account recovery.** Losing every bound key means losing the ability to
  publish; a new key can be bound through the browser as long as the provider
  account is still accessible.

## Reporting

Security issues should go to the maintainers privately rather than into a
public issue. The repository's `SECURITY.md` in the language project names the
current contact.
