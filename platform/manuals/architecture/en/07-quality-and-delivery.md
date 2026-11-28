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

A versioned release begins when a matching `vMAJOR.MINOR.PATCH` tag is pushed. A prerelease such as `v0.1.0-beta.1` uses the same workflow. Run **Release → Run workflow** with **dry_run** enabled to check the candidate before tagging. Both workflows run the complete CI gate on the build commit, assemble archives and checksums, and attach the assets to a draft. Verify the draft's commit and contents before publishing. GitHub locks the tag and assets and attests them when the release is published. RSSN's crates.io job also requires approval in the `crates-io` environment.

Nightly builds run on the repository's default branch and publish a new immutable prerelease under `nightly-YYYY-MM-DD` in UTC. Each dated tag is used once. Nightly runs do not publish to crates.io or update the registry container. Aevia pins the RSSN checkout to an exact commit. For versioned publication the RSSN tag must match Aevia's locked RSSN version. A nightly uses the configured `RSSN_REF` or the RSSN `v0.3` branch, and still checks its package version against the lockfile.

The installers match the Aevia archive name to the Rust target triple, using ZIP on Windows and tar.gz on Linux and macOS. They verify the SHA-256 sidecar and the immutable release attestation before passing anything to the toolchain manager. Managed executables keep each installed tag in its own directory and select all three tools through one active version. The `stable` selector follows the latest published stable release, `nightly` follows the newest dated build for the target, and an explicit version stays pinned.

## Artifact lineage

~~~text
source commit + lockfiles + pinned kernel
    ├→ lint, Rust tests, PostgreSQL and portable C/C++ contracts
    ├→ generated references and executable examples
    ├→ editor protocol tests → verified editor extension packages
    └→ native matrix → runtime probes → archives and SHA-256
                         ↓ all required gates succeed
                 versioned draft or dated nightly
~~~

Default and all-feature language jobs have distinct cache keys. Platform tests can require real database and signing fixtures. RSSN exercises native numerical code and the software Vulkan path on Linux. Each C/C++ smoke runs with the platform linker and ABI. These executable boundaries complement reference values, metamorphic tests and checked error contracts.

The manual checker pairs chapter sources, assembles multi-file examples and invokes the toolchain. Standard-library gates compare generated source, rendered declarations and the reference used by the site. LSP and DAP checks drive real protocol processes. Release packaging consumes the editor builds that passed CI. It also probes native toolchain archives and validates names and sidecars against the installer contract.

A failed release gate identifies the producer to examine. Check registration and generator tables for kernel contract diffs, CLI rendering and source comments for reference diffs, and the target triple and archive contents for packaging failures. Hosted release jobs check tag identity and existing releases before upload. A later release uses its own immutable tag, and the installer can keep and run previous published versions alongside it.
