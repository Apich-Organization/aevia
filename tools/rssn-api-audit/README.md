# RSSN kernel API inventory

`cargo run -p rssn-api-audit -- --report` lists public kernel, backend and simulation source APIs,
including conditional declarations. `--check` compares the live inventory with
the checked-in snapshot; after reviewing intentional source/API changes, run
`--write` to update it. The check also rejects an ABI descriptor whose declared
Rust path is no longer a public function.

Each row states a fully qualified Rust path, an inventory classification, the
names of any numerical ABI exports, its `cfg` attributes, and the Rust
signature. A `declared` row records manifest source provenance and verified declared signature keys, not proof that every value or
backend can execute it. Functions with no descriptor provenance are reported separately, with callback,
generic, opaque and method shapes highlighted. An unmapped Rust function can
still be reachable through an older hand-written rule; the tool does not claim
it is unreachable. The dynamic manifest and namespace coverage
checks are the authority for the callable build.

The inventory includes public parent-type inherent methods in private child modules when they directly import that type from `super`; other private-module paths require review.

The inventory parses Rust syntax; it does not expand macros, evaluate `cfg`,
resolve type aliases or infer the public API exposed by `pub use`. Unexpanded
module-level macros and public re-exports are recorded and fingerprinted, so
changes do not disappear silently; their exported functions still need review.

The independent RSSN-side `cargo run -p rssn-api-gen -- --bindings-report`,
`--bindings-check`, and `--bindings-write` generate the
checked-in RSSN numerical binding fragment from public Rust function syntax and
`rssn/tools/rssn-api-gen/numerical-policy.tsv`. Each policy row explicitly
chooses the ABI name, namespace, dimension relation and nonfinite-output
contract. Existing exports may include a sixth field preserving their established
ABI-specific description; new descriptions come from Rust documentation. Rust
signatures and source paths always come from the source;
unsupported, conditional, generic, missing or non-public functions are rejected.
The generated function pointer is checked by Rust against its adapter type.
RSSN compiles the checked-in fragment and generation does not depend on this Aevia tool.

The RSSN generator commands `--structured-report`, `--structured-check`, and
`--structured-write` generate
record codecs from public named Rust structs and `rssn/tools/rssn-api-gen/structured-policy.tsv`. Named contracts permit
multiple dimensional interpretations for one Rust result type while sharing one codec.
The tool rejects private, generic, conditional, unsupported or missing fields,
and requires a reviewed dimension formula for every numeric field on bound
policies. `unbound` rows generate reusable codecs only; they do not register an
operator or assert a dimension. Generated record destructuring makes field
addition or removal a Rust compilation error until the policy is reviewed.

## Manifest coverage report

`RSSN-COVERAGE.md` (generated, do not edit) is the manifest-level counterpart of the
source inventory: for every kernel operator family it states how Aevia exposes it
(curated typed wrapper, generated dynamic wrapper, or excluded with a reason) and
whether the exact-typed flat `Rssn.<op>` facade covers it. It is produced from the
live Kernel Manifest and `crates/aevia-rssn/domains.overlay`;
`cargo test -p aevia-rssn --test domains` fails when it is stale or when an
operator is neither bound nor excluded, and `AEVIA_BLESS=1` rewrites it.

## Source-derived composable bindings

`cargo run -p rssn-api-audit -- --codecs-{report,check,write}` (or RSSN's
independent `rssn-api-gen` with the same commands) generates
`rssn/src/rules/numerical_abi/codec_generated.rs` from
`rssn/tools/rssn-api-gen/codec-policy.tsv`. Each policy row supplies only a
public Rust path, an ABI name, its existing numerical namespace, and a reviewed
dimension policy. Parameter names, borrowed slice conversion, arity, argument
validation, result structure and the actual kernel call come from Rust source
and shared codecs. Adding a supported function requires one policy row and
regeneration; Aevia's `Rssn` and domain modules appear from the live manifest.
No Aevia numerical implementation or dispatch arm is needed.

These additional raw-number exports deliberately accept dimensionless values.
The existing quantity-aware exports retain their dimensional contracts. A raw
export does not silently strip quantities. For example,
`Rssn.num_stats_ext_t_test_one_sample` returns a record with `statistic`,
`p_value` and optional `df`; `Rssn.num_geometry_convex_hull_2d` takes and returns
an N-by-2 tensor. Tuple results use named `item0`, `item1`, etc. fields.

Supported codecs include checked signed/unsigned integers, real scalars,
booleans, integer and real vectors, rectangular matrices, fixed-width point
arrays, optional results, tuple records and public result records generated
from `structured-policy.tsv`. Borrowed row slices are reconstructed from owned
matrices for the duration of the kernel call. Unsupported Rust shapes fail
compilation or generation; generic closures, stateful handles and callbacks
continue through their dedicated checked ABI adapters. Do not count an
unregistered Rust helper or an unbound record codec as language coverage.

CI checks the generated fragments and complete source inventory separately.
Focused runtime tests in `aevia-rssn/tests/source_generated.rs` exercise actual
Aevia calls, not just generated-text matching.

Expression AD is exposed as `num_ad_forward_gradient`,
`num_ad_reverse_gradient`, `num_ad_hessian`, `num_ad_jacobian` and `num_ad_vjp`
in `Rssn_numerical_vector` (also the flat `Rssn` facade). They take expression
text, a tensor of variable names, and a point tensor. Jacobian/VJP take a tensor
of expression strings; VJP also takes output weights. These operations use the
existing RSSN dual/reverse/hyper-dual tape and have no finite-difference step.
They reject opaque operations and nonsmooth instructions rather than falling
back to approximate derivatives. All parameters are dimensionless.

For a controlled verification pass, run the host numerical unit target plus
`cargo test -p aevia-rssn --test kernel_contracts --test source_generated`.
`kernel_contracts` runs RSSN's existing safe ABI, AD and callback regression
sources in Aevia's build profile, avoiding a second scientific-engine build
under RSSN's differently optimized test profile. The C-pointer manifest test
remains in RSSN's separate `callbacks_ffi` target, keeping the host tests' unsafe
code prohibition intact. The complete source inventories now include simulation
APIs as well as kernels and backends.
