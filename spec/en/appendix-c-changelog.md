# Appendix C — Revision log and known implementation limits

This appendix records the evolution of the Aevia specification and its implementation, the "status stories" the chapters no longer carry, and the limits that are explicitly on record as unimplemented. The normative chapters say only **what is true now**; history lives here. This appendix is informative.

---

## Revision 23 (2026-11-14): full review against the implementation, and an English edition

This pass compared the specification with the implementation chapter by chapter, condensed repetition, moved status notes out of the text, and added an English edition (`spec/en/`; writing and terminology rules in `spec/STYLE.md`). The two machine-read chapters (08 and 11) keep their shape and are guarded by `cargo test -p aevia-std --test spec_signatures --test spec_programs`.

**Whole book**

- New English edition: every Chinese chapter has a counterpart `spec/en/<same name>`, with the same section numbers, anchors, code blocks and diagnostic codes.
- Status notes ("implementation status (date)", "added in Revision N", "task-NN closed") moved out of the chapters into this appendix; a chapter keeps one line of *Status*.
- Measured figures (operator, unit and test counts) no longer scatter across chapters: the manifest size is given once, in §10.8, and the other chapters point to `aevia rssn`.

**This batch (08, 10–13, README, this appendix)**

| File | Corrections | Additions | Removals / moved out |
|---|---|---|---|
| `08-stdlib.md` | Stale figures in §8.19/§8.20 such as the manifest counts, "323/323", "309 declarations", "37 modules" (now 1620 operators and 41 module files, with coverage asserted by tests); the typed count of `Rssn_Numeric` became "all typed"; an English sentence inside a Chinese paragraph in §8.7.1 | Sparse, FFT and sequence-operation subsections merged into one narrative with the rules unchanged | "Implementation status" stories (`par_map`, `args`, SDE, sparse, Plot, NPY, simulation) compressed into one line of status; Revision-number asides; references to the `.workbuddy-ai` audit document |
| `10-backend.md` | Manifest figures in §10.0/§10.8 (1382 → 1620, new `manifest_hash`); §10.5.1 "`par_map` runs sequentially" became worker-thread replicas (§9.7.2); the §10.9 deviation note ("dimensioned literals fold to exact rationals, printing `sqrt(800000/196133) s`"; `fall_time(20.0 m)` now prints `2.0196199771025523 s`) | §10.3.7 points to the generated `Rssn_Sim_<Name>` modules; the gap list of §10.8 merged duplicates | The task-31/`emit_typecheck` narrative, a duplicated SDE status paragraph, the English summary mixed into §10.6.5 |
| `11-examples.md` | The outputs of 11.1–11.5 now match `aevia run`; the end time of 11.2 is 3 s (at 10 s the projectile had landed and the height was negative); the shape of `series` and the `Solutions` record of `solve` in 11.3; the diagnostics of 11.9 match `aevia check`; shadowing warnings in 11.10 | A note that 11.8 is a library module (no `main`) | Repeated "this shows the benefit of…" paragraphs |
| `12-rationale.md` | 12.8.7, parallel symbolic values (the `fork/merge` wording replaced by the actual behaviour of §9.7.2); 12.10, the roadmap changed from effort estimates to a progress table | — | The week counts of M0–M6, duplicated acceptance items |
| `13-extensibility.md` | §13.7 registry: it said "design complete, implementation not started, dependencies report `E0605`", whereas `aevia add/update/fetch/tree`, `aevia pkg publish/yank/search/verify` and the resolver are implemented (Revision 15); the capability table of §13.8.2 gains `core.callback` and `core.plugin.source`, with the actual twelve rows authoritative; "sparse checks only the rank" in §13.9.5 became "refined shapes are checked"; the non-existent `aevia fix` and `tools/gen_rssn_module.aev` removed | The precision-tier table of 13.2.2 merged with the scoring rules of §13.4.1 | The history of manifest hashes (`1920638b…→8ed92a22…→8d2c3764…`), task-31/task-22 narratives, repeated SDE status, long "implementation status" paragraphs of several sections |
| `README.md` | "Revision 13" → 23; counts such as "1476 operators in 37 rule sets" replaced by a pointer to `aevia rssn`; the reading-guide table aligned with chapter titles | A Chinese edition (`spec/README.md` used to be English only) | — |
| `appendix-c-changelog.md` | "37 standard-library modules" → 41 in §2; the known-limits list of §5 updated (SDE events and shape refinement are implemented, the registry is implemented) | This entry | Revisions 12–22 condensed to key points |

**Other chapters (01–07, 09, appendices A/B)**: per-chapter records are collected by spec-batch-a (01, 02, appendix A) and spec-batch-b (03–07, 09, appendix B) and will be added here once their final reports arrive.

**Stale points found in review, outside this batch**: the end of §9.11 in `09-runtime.md` still says "the registry client is still unimplemented (dependencies are path-only)", which contradicts §13.7 and Revision 15; it should be corrected by that chapter's batch.

---

## 1. Summary of the specification's evolution

- **Revision 0 (2026-10-31)**: fixed the language core (pure functional, dimensions as types, static rank with dynamic-shape tensors, symbolic/numeric phases, the rssn backend).
- **Revisions 1–6 (2026-11-01 ~ 2026-11-04)**: bidirectional type checking, unit derivation and affine temperature scales, row polymorphism and pattern matching, a unified ODE/PDE interface, diagnostic codes (E0100–E1000).
- **Revisions 7–10 (2026-11-05 ~ 2026-11-07)**: the standard-library module tree (§8.20); native JIT (`aevia-core/src/jit.rs`, the `jit` feature); worker-replica evaluation of `par_map` (`--jobs N`); `read_npy`/`write_npy`.
- **Revision 11 (2026-11-08)**: fixed a `BigRational` panic for a negative power of zero and a DoS on huge exponents; Unicode superscript brackets in the lexer; a zero-length-message loop and a `did_change` crash in the LSP; the `0^(-n)` singularity and fractional-root guards in the symbolic engine; tensor Cons dimension checks; measurement division by zero; machine-readable `diagnostic_codes` for RSSN FFI batch computation; 0×0/1×1 boundary cases of the dense eigenvalue solver.

## 2. Current implementation status (correcting stale audit records)

1. **`par_map` parallel evaluation**: implemented (Rayon + independent evaluator replicas, input order, `--jobs N`).
2. **`@jit`**: implemented (the `jit` feature; pure numeric branches and self tail-recursive loops are lowered to Cranelift, unsupported constructs fall back to the interpreter); the default build has `core.jit` off.
3. **Standard-library module import**: implemented; 41 `.aev` module files in `crates/aevia-std/stdlib/`.
4. **NPY reading and writing**: implemented (with a static rank check).

## 3. Revision 12 (2026-11-08): caching, `Bcsr`, source operators, non-commutative SDE

1. **Caching.** rssn's `Session` request-level answer cache (on by default; keyed by the request node, template and configuration fingerprint; runs that hit the wall-clock limit are not cached; invalidated wholesale when a plugin or unit is registered) and a sub-term lemma cache (`CachePolicy::lemmas`, off by default); Aevia's per-module on-disk interface cache `.aevi` (§9.11; `AdtId`s are replaced by symbol keys when written).
2. **`Bcsr` (§8.4.1).** rssn gained `kernels::bsr::Bsr`; Aevia gained `SparseFormat::Bcsr`, `to_bcsr`/`block_size`, and automatic block choice in `convert<Bcsr>`; preconditioned `cg_with`, Cholesky-first `spsolve` for symmetric positive definite matrices, and sparse slicing. `E1001("storage.sparse.block")` was removed.
3. **Source operators (§13.5.2).** `@op` registers a dimension signature from the checked dimensions; the module binding is replaced by a wrapper (numeric arguments run the body, symbolic arguments build an operator node); `@rule` is traced after the operators; the rule parser accepts `ns/name`.
4. **Non-commutative SDE (§8.9.1).** Stratonovich non-commutative Milstein, non-commutative SRK, adaptive Stratonovich (exact Itô conversion); the method name selects the scheme family and interpretation, the noise structure selects the kernel; `terms`/`cells` have defaults; record state with general noise.
5. **Features that errored on use.** `symfun` application builds an undefined-function term; `dsolve` was added (§6.4.5); `@jit(target = Cpu)` is accepted and `Gpu`/`Distributed` report `E1001` at check time; the single-file specification is built by `tools/bundle_spec.py`.
6. **Long tensor literals.** Literals over 128 elements no longer give `E0304`: variable chains are followed iteratively and duplicate numeric constraints are deduplicated, taking the check of a 2000-element literal from ~7 s to 0.6 s.
7. **rssn limits.** `limit(sqrt(x+1) - sqrt(x), x, oo)` is now found by a Puiseux substitution; fixed an error in the l'Hôpital pre-check (`limit(sqrt(x)*(sqrt(x+1) - sqrt(x)), x, oo)` should be 1/2).

## 4. Revisions 13–22

**Revision 13 (2026-11-07 ~ 11-09): trust boundary and tightened rules.**
Whitespace rule for unit expressions (§1.8.1): whitespace separates a unit from arithmetic, so `3 m * x` is `3 m` times `x` while the tight `3 m*x` is still a unit expression; an unresolved dimension variable used as the base of an exponent is made `1` (§3.4.3); declared dimension parameters are rigid and a violation is `E0318` (§4.4.1); `show` keeps the phase of a `Float` (`10.0` differs from `10`, §6.12.3); `Set<τ>` gained values and operations (§8.1.5.1); the run-time trust boundary (§9.2.3): the run time executes the checker's proof and does not re-prove it; literal folding (§3.10.4): a literal is rounded only once, and a closed tree of literal arithmetic is a compile-time constant; unit systems `module M with units natural|planck|atomic|geometrized` (§5.11, `E0511`/`E0512`); the commands and flags of §9.9 aligned with the actual CLI.

**Revision 14 (11-10): automating rssn bindings.** Domain modules `Rssn_<Domain>` (§10.3.8): manifest + declarative overlay, generic conversions, errors through `Result`, a coverage check and report; dimension evidence; rich types `Zmod`/`Perm`/`Graph`/`Group`/`PermGroup`. Still missing: type-level moduli and operator overloading for `Zmod<n>`/`GF<p>` (§10.3.8 lists the language changes needed).

**Revision 15 (11-10): package platform and dependency resolution.**
`aevia-resolve` (SemVer 2.0 `Version`/`VersionReq`, a backtracking solver with conflict-directed backjumping, a sparse-index reader, cross-checked against exhaustive search over 2000 random rounds); manifests accept `Units = "^1.2"` and `{ package = "owner/name", version = … }`, lock files gain `checksum` (source `registry+aevia`), and `E0605` still rejects git and unknown keys; the `platform/` index records `deps` (`package`/`req`/optional `import`) and checks on publish (no path-only dependencies, dependencies exist and have a non-yanked satisfying version), publish and yank each run in one transaction, with `POST /api/v1/resolve` and a dependents endpoint; client commands `aevia add/update/fetch/tree` (`--offline`) and `aevia-pkg` `index`/`vendor`/`lock [--check]`.
Still missing: per-package dependency namespaces (same-named import roots pointing to different versions remain `E0606`); `[patch]` overrides; `aevia-pkg tree/lock` read only the root manifest; `[source]` replacement for mirrors; no per-file digests for unpacked source trees.

**Revision 16 (11-12): witnessed automatic typing (§10.3.8).** The kernel records the engine's equality judgements (a request and its answer in one equivalence class) as sort facts, and replay, clustering and mutation probing produce `signatures.witnessed.json`; Aevia generates same-shape typed wrappers for operators with no `sig` (the dynamic tier fell from 1298 to 871 operators, adding 453 automatically typed ones); opaque `Value` handles; dimension evidence for witnessed overloads; exact/symbolic sparse matrices (`sp_*`) and D-finite decision (`holonomic_*`) became available. Still missing: witnessed parameter names are inferred from types; tuples and ragged lists stay in the dynamic tier; several overloads are told apart by `_int`/`_real`/`_float` suffixes.

**Revision 17 (11-12): numeric operator groups, dimension-independent names, GPU `f64`.** `Rssn_Numeric` absorbed the 73 operators newly added to rssn's `numeric` rule set (robust statistics, windows and spectra, wavelets, Krylov/sparse/low-rank/tensor, finite differences, complex special functions, expression operators); conventions: results are `Float` (the kernel encodes only floats) and method/wavelet/statistic names are symbols (bind each name once with `let`); dimension-independent names (the spatial dimension of `heat_fd` etc. is the nesting depth, likewise symbolic PDEs and affine transforms); rssn's wgpu device provides `Dtype::F64` as double-single, and a launch-fusion planner `device::plan` was added (on the Aevia side `Gpu(n)` still reports `E1001`); parameter names of witnessed wrappers are inferred from types.

**Revision 18 (11-13): multiple signatures named by rank/dimension.** An overlay's `sig op as name(..)` may repeat for one operator; `codec Type - from_fn` (decode-only types); `_2d`/`_3d` finite differences; rank-polymorphic tensor operators `tensor_norm_<r>`/`tensor_inner_<r>`/`tensordot_<ra>_<rb>_<k>` and decompositions `hosvd`/`hooi`/`cp_als`/`tt_svd`/`tt_dense` (r = 2–4); mixed lists and the list accessor `from_value<Tensor<1, Value>>`; `Rssn_Numeric` fully typed. Still missing: tensor operators and decompositions beyond rank 4 still go through `Rssn.dynamic`.

**Revision 19 (11-13): language-side completion.** `sum`/`prod`/`mean`/…/`transpose` at ranks 2–4 (`transpose` reverses axes for rank ≥ 2); `Result` `map`/`and_then`/`unwrap_or`/`is_ok`/`is_err`/`ok`/`ok_or`; `first`/`last`/`cumsum`/`cumprod`/`argmax`/`argmin`, `round(x, n)`, `is_nan`/`is_finite`/`is_infinite`; diagnostic suggestions (`E0407` says `m` is a unit, spelling suggestions, record-field hints); `aevia fmt` keeps blank lines between statements.

**Revision 20 (11-13): the FFT family and sequence operations.** `fft`/`ifft` accept `Complex`/`Float`/`Int`; multi-dimensional transforms `fft2`/`fftn`/`fft_axis`/`fftn_axes`/`rfft2`/`rfftn`/`irfft*`/`fftshift` etc. (row–column decomposition); the normalisation of `fft_norm(x, Forward)` was fixed to `1/N` forward; `reverse`/`filter`/`any`/`all`/`find`/`first`/`last`/`take`/`drop`/`concat` at rank ≥ 2 act on rows along the first axis (a semantic change for `reverse`: it no longer flattens), plus `reverse_axis`; tie-breaking of unit suggestions (edit distance → minimal-containing edit → common unit → lexicographic).

**Revision 21 (11-13): kernel diagnostics enter `Err(OpError)`.** rssn gained `Diag`/`DiagKind` and `Session::explain`; when an operator leaves its own unreduced call, the bridge fetches the reason, and the `Err` of `Rssn.dynamic` has the form `rssn.diag.<kind>: <wrapper name>: argument `<parameter name>` (#<position>): <message>`; a typed call site raises `E0807`; `Rssn_Numeric` gained `poisson_fd_bc`/`heat_fd_bc`/`wave_fd_bc`, `advect_fd_upwind`, `tensor_contract_*`, `tucker_dense_*`, `cp_dense_*` and others. Still missing: only operators of `rules::numeric::TABLES` record reasons; `OpError` has no `code(e)`/`message(e)` accessors.

**Revision 22 (11-13): rank has no upper limit.** The signatures of rank-polymorphic operations are **templates** over the rank (`builtins::rank_schemes_for`), instantiated by the checker at the rank of the argument when every built-in candidate fails; `fftn`/`sum` at rank 5 and 6 take the same path as rank 2; `fold`/`fold_left`/`sort`/`sort_by`/`sort_float`/`zip`/`enumerate` at rank ≥ 2 changed from element-wise (flattened) to **row-wise** (a semantic change); added `sort_by_key`, `sort_axis`, `argsort`, `rfftn_axes`/`irfftn_axes`; `sort_by` became a stable merge sort. Still missing: user functions cannot be rank-polymorphic; `cumsum`/`cumprod`/`argmax`/`argmin` have no per-axis forms at rank ≥ 2.

## 5. Known limits on record

1. **Package dependencies**: registry dependencies, SemVer resolution and checked local copies are implemented (Revision 15); `git` dependencies do not exist; per-package dependency namespaces, `[patch]` overrides and mirror `[source]` replacement are not implemented.
2. **Device backends**: `Gpu`/`Distributed` are only constructible run-time values; `on`/`to_device`/`@jit(target = Gpu(..))` report `E1001("core.gpu")`/`E1001("core.distributed")`; batch primitives such as `Device.map` are not exposed as language primitives (§10.6.5).
3. **Sparse**: sparse values cannot enter the rssn term graph; user-defined formats wait for rssn to open `Format`; `gmres_with`/`bicgstab_with` do not accept exact elements; there is no sparse eigenvalue solver or SVD (§8.4.1, §13.9.5).
4. **SDE**: Brownian-bridge dense output and terminal events that stop in the middle of integration are not provided (`sde_at` is a linear chord, `sde_events` cuts after the fact); closures are `!Send` in the interpreter, so parallel ensembles fall back to sequential (§8.9.1).
5. **ODE**: multiple shooting / collocation for BVPs; `AutoSwitch` decides by step budget rather than an eigenvalue estimate (§8.9).
6. **Type system**: type-level moduli for `Zmod<n>`/`GF<p>`; rank polymorphism in user functions; symbolic exponents (§12.8.2).
7. **Plot**: only the HTML/SVG-inline renderer, with no standalone `.svg`/PNG/JPEG output (§8.12).
8. **REPL**: Ctrl-C cancelling the current evaluation (§9.10).
