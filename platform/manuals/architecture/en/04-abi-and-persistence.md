# Kernel contracts and persistence

The Kernel Manifest describes a build's operators, overloads, units, physical constants, plugins and capabilities. A host uses it to generate interfaces, choose supported operations and establish cache identity. The manifest hash is derived from deterministic bytes with its hash field zeroed. The hash changes when the registered contract changes.

## Calls and scientific evidence

An operator can be selected by name or by an `OpKey` that pins its signature. Generated Aevia bindings lower values to the kernel ABI and recover them according to the declared representation, shape and dimension. The dynamic call is suitable for interfaces that cannot be expressed in the typed façade. It returns a typed error channel to the host. A term handle belongs to one session. Passing it between sessions requires an explicit import through the host API.

Dimensional rules express relationships between the arguments and the result. Strict checks reject inconsistent dimensions. Advisory checks retain an answer and attach a diagnostic. A callback is scoped to the calling host and carries its checked signature. Callback errors should survive the boundary with their messages intact.

For a graph-backed operator with a derived signature, the bridge consults the manifest and the witnesses from the call. A `Same(index)` rule can establish a dimensionless graph result when the designated input has dimensionless evidence. Native signatures can supply dimensioned result evidence directly. Both paths validate that evidence when converting to a static type.

A pinned native call can borrow a large contiguous `Float` tensor from the interpreter and construct the RSSN tensor directly. Borrowed inputs use the same dimension witness, capability checks and pinned overload as other inputs.

An exact native `Option` tensor carries its declared element schema even when empty. Populated tensors also carry each observed tag and its payload evidence. Both the generated binding and `from_value` use that evidence to check the requested type.

## Binary and text formats

RSSN uses native bincode-next encoding for its binary manifest and compute records. The binary layout of existing public records is checked against the previous Serde encoder. JSON remains the self-describing text form. The deterministic CBOR interface retains the Serde data model and field names. C stores term handles as integers and returns allocated text or byte buffers through explicit ownership functions.

Aevia's checked-module cache uses bincode-next 3.2 native traits and schema fingerprinting. The header contains the cache key and integrity checks. A cache file stores nominal ADT identities in a local table, then reconstructs process IDs when it loads. Imports and source identity form part of the cache key, so a change in an imported interface invalidates dependants. Corruption or version mismatch leads to a cache miss and recompilation.

## Compatibility checks

A wire change must account for data produced by an older binary and for language bindings built from an older manifest. Contract tests exercise deterministic hashes and round trips. Generated adapters are compared to source declarations and replayed signatures. Performance tests should measure encode, decode, and total workload separately so allocation or validation costs remain visible.

## Operation identity travels with the call

~~~text
Registered operators + signatures + units → sorted manifest → BLAKE3 identity
                                                   ↓
Source call → checked overload → typed value conversion → RSSN call
                                                             ↓
                                                value, witness or diagnostic
~~~

A named call supports dynamic dispatch. An `OpKey` pins the expected signature so a caller cannot silently select a different overload with the same spelling. The adapter lowers arguments and validates retrieved values against the declared representation, shape and dimensions. A callback handle is meaningful through its active callback bank. A term or native-object handle is scoped to the session that registered it.

| Boundary | Ownership |
|---|---|
| Direct Rust API | Rust values and typed error results |
| C interface | Opaque handles and byte buffers with matching release functions |
| Shared library plugin | Versioned ABI and native binary messages. Machine code runs in the host process |
| Source plugin | Traced symbolic definitions and rules under the caller's capability scope |
| Package archive | Verification authenticates the publisher and the bytes named by the signed statement |

A manifest hash identifies one installation of rule sets and operator tables. It makes changes observable to bindings and caches. It does not authenticate a shared library or represent the inputs to an individual experiment.

## A cache hit step by step

The driver hashes the source, resolved import bindings, capabilities and compiler identity, together with the digest of each dependency interface in the closure. The `.aevi` envelope carries a format version, key, length and body checksum. Native bincode-next encoding checks the body schema. The decoder maps the symbolic keys of nominal declarations to process-local IDs before it resolves references. An entry with a truncated body or incompatible schema becomes a cache miss. A changed implementation can leave its public interface digest intact, allowing consumers to keep their checked results.

The nominal registry reuses the earliest finalized declaration with the same defining module, name and schema. A large registry remembers interface lookup candidates by module and name. Repeated lookups for opaque host types can share a read lock.

## Source map

In RSSN, follow `kernel/{manifest,signature,codec}.rs` for identity and encoding, then `kernel/{call,callback,native_objects}.rs` and `ffi/mod.rs` for call and ownership checks. Aevia uses `aevia-rssn/src/{codegen,kernel,value}.rs` and `aevia-types/src/rssn_resolve.rs` to connect these records to language types. `aevia-types/src/persist.rs` defines the persisted body. `aevia-core/src/disk_cache.rs` defines the file envelope and lookup identity. [Verification map](07-quality-and-delivery.md)
