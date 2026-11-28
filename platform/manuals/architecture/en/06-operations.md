# Platform operations

The production layout comprises a public reverse proxy, a private registry application and PostgreSQL, and durable archive storage. The optional book runner is hosted separately under a dedicated rootless Podman account. Bind databases and private execution endpoints only to their required network interfaces.

## Build and configure

Build the registry image from the platform context and the sibling language-crate context. The reduced workspace manifest used in the image is checked for drift against the actual compiler workspace. The application uses a database URL, a secret used to verify sessions, a public base URL, and explicit paths for archive data and document mounts. See the [deployment runbook](https://github.com/Apich-Organization/aevia/blob/v0.1/platform/docs/DEPLOY.md) for environment variables, Compose, Quadlet and backup commands.

## Roll out and check

After deployment, validate the published origin over HTTPS, the health endpoint and a registry read. Verify document pages for both languages and test an offline HTML download. If the playground is enabled, run a short sample and observe success and error handling. Confirm the registry's environment and mounts exclude the private signing and container control interfaces used by administrators and other services.

Use a pinned image and preserve PostgreSQL and archive backups. An upgrade follows its migration compatibility rules. Check restores against a separate database and a separate archive destination. Select a database and archive snapshot from the same release state, then verify the index against available archives before directing traffic to a restored service.

## Update content

The mount directories provide canonical Markdown sources, or a generated JSON reference. A poller detects changes and loads the candidate snapshot before publishing it. Stage complete directory or file replacements where possible. Keep counterpart chapter slugs the same in both languages. A failed reload leaves the last valid snapshot active and records a diagnostic for the operator.

## Service topology

~~~text
Public TLS proxy → registry listener → PostgreSQL + durable archives
                        ├→ read-only spec, stdlib and manual mounts
                        └→ private worker + bearer token → rootless runtime
~~~

The configured public origin controls cookie security, origin checking and generated URLs. Trust forwarded addresses only when the proxy replaces the headers. Restrict PostgreSQL and the worker to the services that use them. HTTPS sessions use host-only cookies, and authentication responses suppress caching and referrer data. A language cookie records preference. An explicit `lang` query produces a stable URL that can be shared.

## Content publication and recovery

The binary and mounted content have separate lifecycles. Put complete first-party editions in read-only mounts. A directory mount sees an atomic rename inside the directory, while an individual bind-mounted file can continue to refer to its old inode. Stage all related files first and rotate a directory pointer within the mounted tree where possible.

An independent watcher for each source runs filesystem work off the async request executor. It observes a candidate across two polls. Scans and reads are bounded. On Unix the stamp includes device, inode and change time in addition to length and modification time. The parser checks the stamp before and after loading. A successful candidate replaces a shared `Arc` under a short lock, while requests with earlier snapshots can finish. Invalid JSON, missing published spec translations or modules, and incomplete bilingual manuals preserve the last good version. Repeated identical errors are suppressed and the source is retried. A deliberate removal of a protected spec chapter or standard-library module takes effect on a fresh start.

## Acceptance tests for a deployment

Verify HTTPS and a live package query. Read matching chapters in both languages and check the standalone HTML download. Fetch `/llms.txt`, a linked Markdown chapter and `/stdlib/catalog.json`. Confirm that sitemap URLs and canonical links point to the configured public origin. Replace content in a staging deployment and verify recovery after invalid input. If the runner is enabled, test a successful program, a failing program and a timeout. Use the [deployment runbook](https://github.com/Apich-Organization/aevia/blob/v0.1/platform/docs/DEPLOY.md) for commands and restoration. `main.rs`, `config.rs`, `reload.rs` and `state.rs` are the runtime entry points.
