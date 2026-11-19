# Kernel contracts and persistence

The Kernel Manifest describes a build's operators, overloads, units, physical constants, plugins and capabilities. A host uses it to generate interfaces, choose supported operations and establish cache identity. The manifest hash is derived from deterministic bytes with its hash field zeroed. The hash changes when the registered contract changes.

## Calls and scientific evidence

An operator can be selected by name or by an `OpKey` that pins its signature. Generated Aevia bindings lower values to the kernel ABI and recover them according to the declared representation, shape and dimension. The dynamic call is suitable for interfaces that cannot be expressed in the typed façade. It returns a typed error channel to the host. A term handle belongs to one session. Passing it between sessions requires an explicit import through the host API.

Dimensional rules express relationships between the arguments and the result. Strict checks reject inconsistent dimensions. Advisory checks retain an answer and attach a diagnostic. A callback is scoped to the calling host and carries its checked signature. Callback errors should survive the boundary with their messages intact.

## Binary and text formats

RSSN uses native bincode-next encoding for its binary manifest and compute records. The binary layout of existing public records is checked against the previous Serde encoder. JSON remains the self-describing text form. The deterministic CBOR interface retains the Serde data model and field names. C stores term handles as integers and returns allocated text or byte buffers through explicit ownership functions.

Aevia's checked-module cache uses bincode-next 3.2 native traits and schema fingerprinting. The header contains the cache key and integrity checks. A cache file stores nominal ADT identities in a local table, then reconstructs process IDs when it loads. Imports and source identity form part of the cache key, so a change in an imported interface invalidates dependants. Corruption or version mismatch leads to a cache miss and recompilation.

## Compatibility checks

A wire change must account for data produced by an older binary and for language bindings built from an older manifest. Contract tests exercise deterministic hashes and round trips. Generated adapters are compared to source declarations and replayed signatures. Performance tests should measure encode, decode, and total workload separately so allocation or validation costs remain visible.
