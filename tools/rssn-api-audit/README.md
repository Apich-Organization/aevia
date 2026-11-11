# RSSN kernel API inventory

`cargo run -p rssn-api-audit -- --report` lists public kernel and backend source APIs,
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
