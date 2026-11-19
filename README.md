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

## Work with packages

```console
aevia init model
cd model
aevia check --all
aevia run
aevia test
```

The manifest `aevia.toml` defines the entry and dependencies. Consult the [package guide](platform/manuals/book/en/10-modules-and-packages.md) for local dependencies, the lockfile, documentation and publication. The client [aevia-pkg](platform/README.md) provides registry access and signed publishing.

## Install the beta

Once `v0.1.0-beta.1` is public, download the archive for your operating system and CPU from [GitHub Releases](https://github.com/Apich-Organization/aevia/releases). Verify it with the accompanying `.sha256` file, then extract it and add the directory containing `aevia` to your PATH. The archive also contains `aevia-lsp`, `aevia-pkg`, the language specification, and the examples.

To install from a source checkout using the release installers, pass the beta version explicitly. The default release lookup selects stable releases.

```console
sh platform/install.sh --version 0.1.0-beta.1
```

```powershell
./platform/install.ps1 -Version 0.1.0-beta.1
```

## Build from source

Use Rust 1.100 or later. Place [RSSN](https://github.com/Apich-Organization/rssn/tree/v0.3) beside this checkout so `aevia` and `rssn` share a parent directory. Use the RSSN revision paired with the Aevia version you are building.

```console
cargo build --locked -p aevia-cli
cargo build --locked --manifest-path platform/Cargo.toml -p aevia-pkg
```

The platform has a separate Cargo workspace and a [deployment guide](platform/docs/DEPLOY.md). The language workspace runs tests with `cargo test --locked --workspace`. For a focused check, select the modified package or a named test target. `cargo +nightly fmt --all --check` checks the language workspace's formatting.

Generate the standalone specification with `python3 tools/bundle_spec.py` and Pandoc. Generate and check the standard library sources with `aevia-stdlib-gen`. `aevia doc --stdlib --json` generates the bilingual platform reference bundle. Add `--lang zh` to write the Markdown pages in Chinese.

## License

[Apache License 2.0](LICENSE).
