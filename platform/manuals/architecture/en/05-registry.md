# Package registry and documentation

The Axum server serves an API for clients and HTML for browsers. PostgreSQL stores the catalogue, identities and append-only transparency records. A separate archive store holds package contents identified by version and checksum. Leptos builds HTML from view models, and the documentation renderer turns package README files and source comments into sanitized HTML.

## The publication transaction

~~~text
name + version + SHA-256 → challenge → canonical statement ← signing key
    signed statement + archive
        → verify and consume nonce → check owner, archive and dependencies
        → DB transaction → insert catalogue rows and pending docs state
        → write durable archive → append transparency record → commit
                                                             → wake docs worker
~~~

The authorization checks the signing key, publisher and package owner, including explicitly authorized organization ownership. Archive structure and size are checked before source parsing. The database transaction commits the version, relations and transparency entry only after archive storage succeeds. A transaction failure can leave an unreferenced digest, which a retry with identical bytes can reuse. A consumed challenge cannot be replayed. Downloads serve the recorded checksum and implement conditional requests and HEAD. Counts are batched separately from package publication.

The documentation worker claims pending versions within a database transaction. It verifies the durable archive and renders the compiler's module descriptions and visible declarations in a bounded blocking task. Declarations marked `@hidden` or `@doc(hidden)` are omitted from the HTML and its counts. Visible private declarations carry a badge. The renderer applies one shared sanitization policy to package Markdown. The generated pages and build result commit together. An error is recorded for the affected version and other jobs can proceed. An interrupted database transaction releases its pending claim.

Simple Markdown paragraphs render directly from escaped text and inline formatting events. Content with links, tables or raw HTML goes through the full renderer and sanitizer. Builds without the compiler renderer recognize the common single-line declaration visibility attributes.

## Public reading and private execution

The first-party Markdown representations and the JSON standard-library catalog come from the same validated snapshots as the public HTML. Readers can discover them through `/llms.txt` or HTML response links. The sitemap lists published chapters and translations. Package data has independent JSON search, resolution and index routes. These endpoints publish data and require no execution permissions.

~~~text
browser POST /api/v1/playground/run
    → origin, rate and size checks → authenticated private worker
    → rootless Podman, no network, read-only image, small writable /work
    → capped output, timeout or process exit → text in browser
~~~

The registry has no container-control socket. The worker accepts source, not a host path, and starts a fixed entry point in a locally available image. A user namespace, capability dropping and CPU, memory, process and output limits constrain the child independently of language capabilities.

## Source map

Start in `aevia-registry-server/src/api/publish.rs`, then follow the signed format and archive rules in `aevia-registry-core/src/{statement,tarball}.rs`. The catalogue, log and queue are implemented by `aevia-registry-db`, `docs_queue.rs` and `storage.rs`. The request boundaries live in `http.rs`, `limits.rs`, `playground.rs` and `sandbox.rs`. `seo.rs` describes data discovery and canonical links. [Deployment lifecycle](06-operations.md)
