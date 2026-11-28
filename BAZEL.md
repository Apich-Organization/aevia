# Build Aevia and RSSN with Bazel

[简体中文](BAZEL.zh-CN.md)

If the executable is named `bazelisk`, use it in place of `bazel` in these commands. For Windows, choose an NTFS output root with `--output_user_root` when the default volume cannot create junctions.

Each repository retains its own Bazel module and Cargo lockfile. RSSN builds independently. Aevia uses the sibling `../rssn` checkout by default, including uncommitted API and implementation changes. CI and releases continue to check out the RSSN revision resolved by the respective workflow. Both repositories use Bazel 7.7.1, Python 3.11 or later and the Rust toolchain required by their Cargo manifests.

```text
work/
  aevia/    MODULE.bazel
  rssn/     MODULE.bazel
```

To verify a paired change in GitHub Actions, push its RSSN revision first and enter the branch, tag or commit in the CI workflow’s `rssn_ref` input. The resolver pins one commit for all jobs. Release CI uses the commit selected by the release workflow.

You can select an existing RSSN worktree from Aevia. Bazel stages declared inputs in a stable, side-by-side local mirror to preserve Cargo path dependency semantics.

```sh
bazel test --override_module=rssn=/work/rssn-worktree //:joint_test
```

| Command | Purpose |
| --- | --- |
| `bazel build //:toolchain` | Build the CLI and LSP |
| `bazel build //:toolchain_minimal //:toolchain_profiling` | Build the CLI with default features disabled or with the profiling Cargo profile |
| `bazel run //:aevia -- --version` | Run the Bazel-built CLI |
| `bazel run //:aevia -- rssn --manifest-json > manifest.json` | Export the linked RSSN manifest for audit and bindings tools |
| `bazel run @rssn//:refresh_bindings_from_manifest -- --manifest manifest.json` | Refresh the linked RSSN version's language bindings from the exported manifest |
| `bazel run //:benchmark -- interp\|matmul\|startup` | Measure IR workloads, dense matrix multiplication, or RSSN bridge startup |
| `bazel run //:benchmark -- startup-phases` | Report per-phase median times for starting an empty Aevia program in the test profile |
| `bazel run //:benchmark -- tail-call\|dense-tensors\|dense-matmul` | Run the dedicated tail-call, boxed/dense tensor, or language matrix-product timing harness |
| `bazel run //:benchmark -- dense-axis\|dense-minmax` | Compare dense and boxed storage for axis sums or full-tensor minima in the test profile |
| `bazel run //:benchmark -- cli` | Exercise the `@bench` CLI and its error reporting |
| `bazel run --config=release-local //:aevia -- bench program.aev` | Measure user programs annotated with `@bench` using an optimized CLI |
| `bazel run //:benchmark_ci` / `bazel run //:benchmark_ci -- --full` | Run the CI smoke checks or every benchmark workload |
| `bazel run //:profile -- run program.aev` | Sample an Aevia command on x86-64 Windows with an unstripped CLI and, on MSVC, an adjacent PDB |
| `bazel run //:check` | Check the Rust workspace without linking test binaries |
| `bazel run //:fmt` / `bazel test //:fmt_test` | Format both workspaces with nightly Rustfmt, or check formatting |
| `bazel test //:rustdoc_test //:platform_rustdoc_test` | Build Rust API documentation for both workspaces |
| `bazel run //:platform_dev -- run` | Start the local PostgreSQL instance and run the registry |
| `bazel run //:registry_image -- --tag IMAGE` | Build the registry and its runtime image |
| `bazel run //:postgres_image` | Build the local PostgreSQL runtime image for Compose or Quadlet |
| `bazel run //:playground_image -- --tag IMAGE` | Build the Aevia CLI and its playground worker image |
| `bazel test //:test` | Aevia Rust tests, installer and toolchain tests, and contract checks |
| `bazel test //:joint_test` | Core tests and contract checks for both repositories |
| `bazel test //:integration_test` | All-feature tests, PostgreSQL, editor suites and language bindings across both repositories |
| `bazel test //:rust_all_features_test` | Aevia Rust tests with all features |
| `bazel test //:joint_all_features_test` | RSSN and Aevia all-feature Rust tests |
| `bazel test --define=rssn_bounded_tests=1 //:joint_all_features_test` | Run both workspaces while discarding completed integration binaries one batch at a time |
| `bazel test //:platform_scripts_test` | Deployment and development script fixtures |
| `bazel test //:platform_test` | PostgreSQL integration and compiler-free documentation crate tests |
| `bazel test //:editor_config_test //:editor_protocol_test` | Editor configuration, Tree-sitter parser, LSP and DAP |
| `bazel test //:editor_kate_test //:editor_zed_runnables_test //:editor_zed_wasm_test` | Kate highlighting, Zed queries and the WASI extension |
| `bazel test //:clippy_test //:platform_clippy_test` | Strict Clippy for the language and platform workspaces |
| `bazel test //:release_inputs_test` | Check the release source provenance and Linux ABI gates |
| `bazel test //:editor_package_test` | Build and inspect the VS Code and JetBrains packages |
| `bazel run //:editor_packages` | Package editor extensions, by default into `dist/editor-extensions/` |
| `bazel test //:editor_vscode_test` / `bazel run //:editor_packages -- --editor vscode` | Test or package the VS Code extension using Node.js alone |
| `bazel test @rssn//:bindings_test` | Run all four RSSN language suites |
| `bazel test //:rssn_api_audit_test` / `bazel run //:rssn_api_audit -- --report` | Verify the RSSN callable inventory or print its audit report |
| `bazel run //:refresh_kernel` | Refresh RSSN generators, the Aevia API inventory, stdlib sources and coverage |
| `bazel run //:refresh_stdlib` / `bazel run //:refresh_spec` | Refresh stdlib sources or specification documents |
| `bazel run //:refresh_stdlib_reference` | Validate both reference editions and refresh the checked-in pages and JSON. Pass `-- --check` to verify them or `-- --aevia /path/to/aevia` to reuse a native CLI from the paired source. |

The Linux benchmark job runs the tail-call and CLI smoke workloads. Enable
`full_benchmarks` when dispatching CI to run the full suite. The job records
timings in its log.

Set `AEVIA_TEST_DATABASE_URL` to a database suitable for testing. The suite creates a disposable GnuPG key. Depending on the suite, editor tests use Node.js, PyYAML, Vim, Emacs, Helix, `ksyntaxhighlighter6` or `tree-sitter`. The Zed WASI target requires Clippy and `wasm32-wasip2`. Editor packaging needs npm, Java 21 and Gradle. RSSN header, manifest, witness and binding tasks can also run from Aevia, for example `bazel run @rssn//:refresh_witness` and `bazel build @rssn//:manifest`. Refresh targets publish only the selected generated paths after generation succeeds and check for concurrent edits before replacing them.

## Releases and build timings

```sh
bazel run --config=release-max //:package_release -- \
  --distribution 0.1.0-beta.1 --rssn-commit "$(git -C ../rssn rev-parse HEAD)"
```

Before building the release binaries, Bazel runs RSSN's LLVM pipeline probe.
The release target builds with all Aevia and RSSN features, the LLVM optimizer,
x86-64-v3 (AVX2/FMA) or AArch64 Advanced SIMD (NEON), fat LTO and one codegen unit. It creates and checks the archive
and checksum under `dist/`. It runs the packaged CLI and LSP, checks NumPy `.npy`
I/O and the Cranelift JIT, and exercises the AST and IR backends. The Linux smoke
uses Mesa Lavapipe from `mesa-vulkan-drivers` to exercise a wgpu adapter. CI uses
this release target with clean Aevia and RSSN checkouts. Packaging verifies the requested commits and compares the source snapshot of
each binary build with the packaging inputs. A versioned archive must use the
version in the selected Aevia commit. Nightly archives use a calendar date.
Both verify the packaged CLI's version and the archive's RSSN commit. The Linux
x86-64 registry binary used for the published image is written to `dist/registry/`.

The shared `bazel/rssn_suites.bzl` plan selects which RSSN fixtures run in `//:contracts_test` and which also run against release binaries. The plan runs each selected fixture under AST and IR. Both packaging targets and `//:release_smoke` use the same release fixture list.
this release target with clean Aevia and RSSN checkouts. Packaging verifies the
requested commits and compares the source snapshot of each binary build with
the packaging inputs. Linux packages for both architectures contain the registry
and playground. CI passes `--registry-image` to `//:package_release` on Linux.
Bazel builds and tests that image using the same native release binary. CI attaches both image archives and publishes a
multi-architecture manifest for versioned releases.

`bazel run --config=release-max //:editor_packages` produces the VS Code and
JetBrains plugins, the Zed WASI extension, and archives for Emacs, Helix, Kate,
Sublime Text, Tree-sitter and Vim. CI attaches the tested packages with checksums.

Linux release runners use Ubuntu 24.04. The packaging target runs `readelf`
and checks that the shipped binaries require glibc 2.39 or earlier.

Build `//:toolchain` for the CLI and LSP alone, or `//:platform_cli` for the
registry client. Recheck an archive on its target with `bazel run
//:release_smoke -- --archive /path/to/aevia.tar.gz`. Pass `--rssn-commit SHA`
to check the bundled RSSN revision against a known commit.
`//:package_release_from_binaries` takes `--toolchain-bin` and `--platform-bin`
for testing existing binaries. Supply their source commits when invoking that
target. For a release with verified build receipts, use `//:package_release`.
Releases require a host LLVM SDK matching `rustc -vV`. Pass
`--repo_env=RSSN_LLVM_CONFIG=/path/to/llvm-config` to select it explicitly.

Windows build hosts also require `--repo_env=RSSN_LLVM_IMPORT_LIBRARY` to point
to the import library for the host LLVM C++ API. Set `--repo_env=RUSTC` and
`--repo_env=CARGO` to select a compatible toolchain when needed. CI accepts
target-specific paths through `RSSN_RELEASE_LLVM_CONFIGS`,
`RSSN_RELEASE_LLVM_IMPORT_LIBRARIES`, `RSSN_RELEASE_RUSTCS` and
`RSSN_RELEASE_CARGOS`, keyed by Rust target triple. For Linux x86-64, Linux Arm64 and
macOS Arm64, release CI asks the RSSN Bazel module to fetch the checksum-pinned
LLVM 23.1.5 SDK when no explicit path is configured. For Windows and Intel
macOS, configure the matching toolchain and optionally select a prepared runner
through the `RSSN_RELEASE_RUNNERS` JSON map keyed by Rust target triple.

`--config=release` uses ThinLTO and 16 codegen units. The release workflow uses
`--config=release-max` for fat LTO and one codegen unit. Use
`--config=release-local` to measure O3 without the LLVM plugin or LTO. Cross-build with
`--define=rust_target=<triple>` and
`--repo_env=RSSN_TARGET_ENV=/absolute/path/to/toolchains.json` to supply the
target linker, SDK and C toolchain. Rust tests can use a configured Cargo
target runner. Installer and protocol tests need a host that can execute the
corresponding artifacts natively.

Use `--define=rssn_timings=1` for Cargo unit timings and `--profile=/path/profile.gz` for Bazel action timings. Local incremental state defaults to `.bazel-cache/`. On Windows, `--repo_env=BAZEL_SCI_CACHE=C:/b/a` sets a shorter path. See `docs/BAZEL.md` in RSSN for cross-toolchain examples, caching and comparisons.
