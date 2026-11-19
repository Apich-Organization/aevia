# Package registry and documentation

The Axum server serves an API for clients and HTML for browsers. PostgreSQL stores the catalogue, identities and append-only transparency records. A separate archive store holds package contents identified by version and checksum. Leptos builds HTML from view models and the language docs renderer turns public source comments into Markdown and sanitized HTML.

## Publish a release

A client compresses the package and requests a challenge for its name, version and checksum. It signs precisely the statement returned by the server. The server consumes a short-lived nonce once, verifies the statement and signature against the publisher's registered key, and confirms that the package metadata matches the archive. Authorizations are scoped to the named package owner. OpenPGP and supported SSH signatures are verified against the same canonical statement format.

The catalogue row, index information and transparency-log record describe the same release. Downloads check content identity using the published checksum. Yanking is a version-state change. Consumers retain the information needed to verify a version resolved earlier. Package archive extraction and validation enforce structural limits before documentation is generated.

## Serve different sources as one documentation site

Package pages derive from archive source and persisted build results. The specification and manuals derive from mounted bilingual sources. The standard-library reference is rendered from the CLI's bundle, which carries modules and public declarations from the same embedded sources that users import. Operator implementation documentation belongs to the kernel catalog.

The site swaps in a freshly built document snapshot after a successful reload. In-flight requests can finish reading an earlier snapshot. The standalone HTML downloads combine the same chapters without execution controls.

## Run short book programs

When configured, the web server delegates an example to the private, bounded execution service. The worker invokes an Aevia toolchain in a separate Podman container, with a read-only image, no network and explicit CPU, memory, process and wall-time limits. The web worker returns stdout, stderr and status. It receives no production database credentials. The interactive edition can be served without that service and the static edition remains available.
