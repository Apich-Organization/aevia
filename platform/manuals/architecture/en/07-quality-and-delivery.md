# Quality, CI and release delivery

A check should cover the boundary where an error becomes visible. A kernel unit test can prove an edge case, but an ABI test is needed to prove that a caller receives it. A documentation source check can prove coverage, but a render check is needed to prove that a reader sees the result.

| Boundary | Checks |
|---|---|
| RSSN algorithms | Exact identities, reference values, finite/non-finite behavior, degenerate input and dimension contracts |
| RSSN engine | Incremental versus full search, stable extraction, repeated and parallel requests |
| Language | Parser and diagnostics, type and dimension rules, AST/IR parity, allocation and performance fixtures |
| ABI | Manifest and record round trips, generated bindings, C headers and smoke tests |
| Docs | Both language editions, generated standard library, executable lessons and package-doc parsing |
| Platform | PostgreSQL migrations and transactions, package and signing workflows, HTML and container build |
| Editors | LSP smoke over stdio, extension syntax or builds, shared grammar validation |

## Keep correctness and performance observable

Use the Rust test targets closest to the modified API and retain expensive integration passes for a combined change. Builds for parallel jobs should use distinct targets or machines. Store caches separately from workspaces. Build release binaries with generic target settings, and take performance measurements with pinned inputs, build profiles and thread counts. Unmeasured optimizations should be expressed as reduced work or allocations, with timing reserved for measured cases.

## Continuous validation and release

The two CI workflows run formatting, lint, tests, contract generation and documentation checks. Aevia also builds the platform and editor extensions. Linux and Windows exercise the main language and kernel workspaces. Platform database integration runs with a disposable PostgreSQL service. The consolidated CI gate requires success from each required job.

The manually dispatched release workflows build target-specific archives, record SHA-256 checksums and create draft releases. The RSSN C libraries are produced explicitly, allowing language consumers to link an ordinary Rust library. Cross-repository tests should identify the RSSN revision used to produce the Aevia artifacts. Review both catalogs, the bilingual specification, example runs and the release notes before publishing a prerelease.
