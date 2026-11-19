# Registry security

[简体中文](SECURITY.zh.md)

## Publication and signing

The registry issues a five-minute statement for each requested publish or yank. A publish statement binds the package name, version, and SHA-256 digest. It is authorised by a registered signing key, and the nonce can be consumed once. PostgreSQL commits the version, dependency metadata, and transparency entry together after the archive is durably installed. A client can verify the stored archive against the signature and the digest, including for a yanked release. [HTTP API](API.md)

OpenPGP supports signing subkeys certified by the registered primary key. New actions require a current certificate and signing key. The registry evaluates the certificate supplied at binding time. If a key is compromised, revoke its binding in the account page. A retired key remains available for verifying historical releases but cannot authorise new actions. SSH signatures use Ed25519 and the `aevia` SSHSIG namespace. [OpenPGP specification](https://www.rfc-editor.org/rfc/rfc9580.html)

Package archives are bounded by compressed size, decompressed size, and entry count. Entry names are validated for portable relative paths. Documentation builds parse source as data and never execute package code. The site sanitizes all package Markdown and restricts its browser content policy to same-origin scripts and styles.

## Identity and sessions

GitHub and ORCID verify publisher identity. OAuth state is bound to the initiating browser by a provider-specific cookie and is consumed on callback. Browser sessions expire after 30 days. New sessions store an HMAC-derived lookup key in PostgreSQL, with the random session value held in a host-only HTTPS cookie. Session cookies use `HttpOnly`, `SameSite=Lax`, and `Secure` on HTTPS. Browser mutations also check request origin. The API for signed publication and yanking accepts signing-key proofs independently of browser sessions.

GitHub account and organization bindings use numeric provider IDs. The Aevia namespace stays with its original publisher row when a GitHub login changes. Organization grants require active membership and App installation access for the same numeric organization ID. Starting a new check clears previous grants. A completed check grants access for at most one day. [Legacy GitHub binding procedure](DEPLOY.md#bind-legacy-github-accounts-during-upgrade)

Requests to challenge, publish, sign-in, device, resolver, and execution endpoints have per-client and service-wide limits. Clients reaching a request limit receive HTTP 429 and a `Retry-After` value. Configure forwarded addresses only behind a proxy that overwrites the incoming address header. [Deployment](DEPLOY.md)

## Tutorial execution

Execution is optional. A dedicated worker accepts authenticated requests from the registry, then runs each source file in a fresh rootless Podman container with no network or shared host mounts. The worker enforces input, output, time and concurrency limits. The container constrains CPU, memory, processes, and writable storage, and the interpreter starts with an empty Aevia capability set. The registry remains separate from the worker’s Podman socket and filesystem. [Worker deployment](DEPLOY.md#enable-book-execution)

## Transparency and reporting

The append-only log links each entry to the previous entry’s hash. Retain a known head hash to check for changes to previously recorded history. The log gives readers a way to compare published history across observations. [Registry design](../manuals/architecture/en/05-registry.md)

Report vulnerabilities privately through the repository’s [security reporting channel](../../SECURITY.md).
