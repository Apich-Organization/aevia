# RSSN kernel API inventory

`cargo run -p rssn-api-audit -- --report` lists public kernel, backend and simulation source APIs,
including conditional declarations. `--check` compares the live inventory with
the checked-in snapshot; after reviewing intentional source/API changes, run
`--write` to update it. The check also rejects an ABI descriptor whose declared
Rust path is no longer a public function. `--coverage-check` independently
requires each inventoried callable to have a declared adapter or an exact
path and rationale in [the Rust host interface inventory](source-boundaries.tsv).
The latter covers APIs such as a solver that takes a borrowed mutable Rust
callback or writes into caller-supplied storage. The check rejects stale
entries and requires an entry to be removed when an adapter is declared. Struct-literal and trait-operator
adapter helpers omit that provenance instead of claiming a missing inherent method.

Each row states a fully qualified Rust path, an inventory classification, the
names of any numerical ABI exports, its `cfg` attributes, and the Rust
signature. A `declared` row records manifest source provenance and verified declared signature keys, not proof that every value or
backend can execute it. Functions with no descriptor provenance are reported separately, with callback,
generic, opaque and method shapes highlighted. An unmapped Rust function can
still be reachable through an older hand-written rule; the tool does not claim
it is unreachable. The dynamic manifest and namespace coverage
checks are the authority for the callable build.

The inventory includes public parent-type inherent methods in private child modules when they directly import that type from `super`; other private-module paths require review.

The inventory parses Rust syntax and resolves named local `pub use` functions
and public type methods, including private defining modules and explicit
`#[path]` files. `declared-alias` inherits an existing adapter only when source
resolution proves both paths name the same Rust item. It does not infer
coverage from similar names or signatures. External imports, globs and macros
remain fingerprinted review evidence; macros are not expanded and `cfg` is not
evaluated. Conditional JIT/GPU adapters report their required feature explicitly.

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

Generated numerical contracts use reviewed policies in `contracts.tsv` and
`codec-policy.tsv`; explicit native adapters validate their contracts directly.
Interpolation separates coordinate and ordinate dimensions; linear solves return
RHS/matrix units; regression, PCA, transforms, geometry and density estimators
propagate their mathematical units. Functions such as matrix exponentials and
special transcendental functions require dimensionless inputs. For example,
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
back to approximate derivatives. These expression-text interfaces are
dimensionless. `num_ad_gradient_scaled` and `num_ad_hessian_scaled` additionally
accept physical points, a positive coordinate scale and output scale, restoring
gradient and Hessian units from normalized-coordinate expressions.

Opaque native objects have explicit portable adapters (`num_kdtree_build`,
`num_kdtree_nearest`, `num_kdtree_radius`, `num_bspline_build`,
`num_bspline_query`). Their immutable, versioned defining-data payloads carry
separate unit witnesses; queries validate and reconstruct the native model.
These portable definitions contain no process-local handles or unchecked
serialized internal tree nodes.
Dimensioned nonlinear solvers use explicit reference scales (`*_scaled`), and
resampling uses checked statistic callbacks with separate sample/result units.
See `examples/NUMERICAL_GUIDE.md` for executable language examples.

Stateful reverse AD, compiled backends and exact homology use bounded
session-owned native handles instead. Each use validates kind, owning session
and lifetime; foreign, released and malformed handles fail. Physical domains
are retained separately from handle bytes. Tape gradients propagate output/input
units, and compiled programs use positive per-coordinate/output references.
Reverse-callback handles expire on return; release dependent AD values before
their tape. Only the sealed built-in `NativeAdVar` is admitted as an opaque
callback leaf; ordinary source ADTs and other opaque callback schemas remain
rejected. Generic matrix methods have explicit Float/exact-rational instances.


`extent_pow(D,arg,axis,multiplier)` declares a runtime-shape-dependent result
dimension, used by determinants and multivariate densities. Such results
retain exact runtime unit witnesses through the checked Value interface; they
do not claim a source signature with a compile-time-known shape exponent.

For a controlled verification pass, run the host numerical unit target plus
`cargo test -p aevia-rssn --test kernel_contracts --test source_generated`.
`kernel_contracts` runs RSSN's existing safe ABI, AD and callback regression
sources in Aevia's build profile, avoiding a second scientific-engine build
under RSSN's differently optimized test profile. The C-pointer manifest test
remains in RSSN's separate `callbacks_ffi` target, keeping the host tests' unsafe
code prohibition intact. The complete source inventories now include simulation
APIs as well as kernels and backends.

## Native generic and opaque adapters

`rssn/src/rules/numerical_abi/native_object_api.rs` derives arity, declared
schemas, checked codecs, dimension parameters, source provenance and registration
from typed `object_binding!` declarations. Generic APIs select concrete scalar,
constant-dimension, sampled-provider or owned-function instances explicitly.
Native algorithms remain in RSSN. Aevia transports checked values, physical
witnesses, callback contracts and session capabilities.

Per-coordinate references allow heterogeneous state and derivative units. Native
objects validate kind/session/lifetime and bounded retained storage. Compiled
callbacks either retain parent handles or account for their independently owned
snapshots. JIT/GPU APIs keep their feature requirements; normalized machine IR
has an explicit contract alongside measured program/device interfaces.

Executable examples are indexed in `examples/NUMERICAL_GUIDE.md` and its
Chinese companion. Targeted verification uses `kernel_contracts`, the source
inventory tests, and the AST/IR example targets; it does not require running the
whole workspace suite after each adapter change.
