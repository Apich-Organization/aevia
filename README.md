# Aevia

[简体中文](README.zh-CN.md)

Aevia is a functional language for scientific computing with dimensions in its type system. It checks expressions such as `3 m + 2 s` before running the program. RSSN supplies symbolic transformations, numerical algorithms and physical-unit metadata.

## Start here

[Build `aevia` from source](#build-from-source) or see the [online installation guide](https://aevia.apich.org/cli). Confirm the installation with `aevia --version`, then run `aevia` to open the interactive prompt.

```text
2 m + 30 cm
:type 2 m + 30 cm
```

For a file-based example, run [the kinematics model](examples/kinematics.aev).

```console
aevia check examples/kinematics.aev
aevia run examples/kinematics.aev
```

The model compares an exact expression for fall time with its floating-point approximation. Change the height and check how the two forms change.

| Resource | Use it to |
|---|---|
| [The Aevia Book](platform/manuals/book/en/01-getting-started.md) · [Interactive edition](https://aevia.apich.org/book?lang=en) | Learn with complete exercises and runnable examples |
| [Standard library](platform/stdlib-docs/stdlib.md) · [Online reference](https://aevia.apich.org/stdlib?lang=en) | Find callable names, signatures, physical dimensions and examples |
| [Language specification](spec/en/README.md) · [single-page HTML](Aevia-Language-Specification.en.html) | Look up syntax, typing or language semantics |
| [Examples](examples/README.md) | Run a complete program or reproduce a numerical case |
| [Editors](editors/README.md) | Set up diagnostics, navigation, tasks and debugging in supported editors |
| [Architecture](ARCHITECTURE.md) · [Changelog](CHANGELOG.md) | Understand implementation boundaries and releases |

Use `aevia docs --section book --open` to open the hosted manual, or add `--download` to resolve the standalone HTML. `aevia <command> --help` describes a command and its options.

The native toolchain enables RSSN's JIT, wgpu and NumPy I/O features by default. Run `aevia devices` to see the host SIMD support and registered devices. Automatic execution uses Cranelift for eligible hot scalar functions and can offload suitable dense `Float` matrix products while preserving their precision. Use `aevia --rssn-backend cranelift devices` to select the RSSN expression compiler and check the active selection. `--backend` controls the Aevia evaluator and matrix offload policy. In a REPL, `:rssn-backend` lists the choices and `:rssn-backend auto` restores automatic selection.

Aevia detects the available CPUs for parallel language evaluation and grows its worker pool as work arrives. Use `--jobs` to set the worker limit for one invocation or `AEVIA_JOBS` for a shell session. `RAYON_NUM_THREADS` controls the default Rayon pool and also supplies the language worker limit when Aevia has no override. [Parallel evaluation and profiling](platform/manuals/book/en/12-parallel-and-profiling.md)

For text clients, the [documentation index](https://aevia.apich.org/llms.txt) links to localized Markdown chapters and the generated standard-library catalog.

## Work with packages

```console
aevia init model
cd model
aevia check --all
aevia run
aevia test
```

Each source test releases the RSSN native objects it creates when the test ends. File-scope objects remain available to all tests in that file.

The manifest `aevia.toml` defines the entry and dependencies. Consult the [package guide](platform/manuals/book/en/10-modules-and-packages.md) for local dependencies, the lockfile, documentation and publication. The client [aevia-pkg](platform/README.md) provides registry access and signed publishing.

## Install a toolchain

The installers verify the archive's SHA-256 checksum before installing `aevia`, `aevia-lsp`, and `aevia-pkg`. To verify the GitHub release attestation as well, install [GitHub CLI](https://cli.github.com/) with `gh release verify-asset` support and sign in with `gh auth login`. The installers add the toolchain to PATH and set up shell completions.

The default is the latest stable release. To install `v0.1.0-beta.1` once it is published, specify the version explicitly.

```console
curl -fsSL https://aevia.apich.org/install.sh | sh -s -- --version 0.1.0-beta.1
```

```powershell
& ([scriptblock]::Create((irm 'https://aevia.apich.org/install.ps1'))) -Version 0.1.0-beta.1
```

For the latest daily build, use `--channel nightly` on macOS and Linux or `-Channel nightly` on Windows. Use `--version nightly-YYYY-MM-DD` or `-Version nightly-YYYY-MM-DD` to pin a specific daily build. The same options work with [the Unix installer](platform/install.sh) and [the PowerShell installer](platform/install.ps1) in a source checkout.

Once installed, manage toolchains with `aevia toolchain COMMAND`. With `aevia toolchain run VERSION`, put language options and arguments after `VERSION` to pass them to that version of Aevia.

```console
aevia toolchain install nightly --no-activate
aevia toolchain list
aevia toolchain run nightly-YYYY-MM-DD --version
aevia toolchain update stable
```

Replace the date with an installed nightly shown by `list`. Use `aevia toolchain use VERSION` to activate an installed version, or `aevia toolchain update` to follow the active stable or nightly channel. An exact-version pin stays on that version until you switch.

To install manually, find the archive for your operating system and CPU on [GitHub Releases](https://github.com/Apich-Organization/aevia/releases). Compare its SHA-256 against the corresponding `.sha256` and verify the publication with `gh release verify-asset TAG ARCHIVE --repo Apich-Organization/aevia`. Extract it and add the directory containing the binaries to PATH. The archive also includes the specification and examples.

## Build from source

The [Bazel build guide](BAZEL.md) covers local RSSN worktrees, tests, generators, and optimized distributions.

Use Rust 1.100 or later. Place [RSSN](https://github.com/Apich-Organization/rssn/tree/v0.3) beside this checkout so `aevia` and `rssn` share a parent directory. Use the RSSN revision paired with the Aevia version you are building.

```console
bazel build //:toolchain //:platform_cli
bazel test //:rust_all_features_test
```

The platform has a separate Cargo workspace and a [deployment guide](platform/docs/DEPLOY.md). Run `bazel test //:joint_all_features_test` to test the language workspace and the selected RSSN checkout together. For a focused check, pass a named test target with `--test_arg=--test --test_arg=allocations`. `cargo +nightly fmt --all --check` checks the language workspace's formatting.

Run `bazel run //:refresh_spec` with Pandoc to generate the standalone specification, and `bazel run //:refresh_stdlib` to generate the standard library sources. `bazel run //:aevia -- doc --stdlib --json` writes the reference pages and their JSON index. Use `--lang en` for the checked-in reference or `--lang zh` for the Chinese pages. [Bilingual reference check](tools/check_stdlib_docs.py)

## License

[Apache License 2.0](LICENSE).
