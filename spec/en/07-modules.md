# 07 — Modules, packages and visibility

The module system is **file-level, functor-free and free of implicit global state**: a file is
a module, and a package is a set of modules plus a manifest. Name resolution is complete at
compile time; there is no dynamic import at run time.

## 7.1 Modules

### 7.1.1 A file is a module

```
src/
  kinematics.aev          -> module Kinematics
  thermo/
    ideal_gas.aev         -> module Thermo.IdealGas
    cycles.aev            -> module Thermo.Cycles
```

- A module name is determined by the path relative to `src/`, in `UpperCamelCase` segments; it
  is case-sensitive, and a file holds exactly one module.
- A file may write `module Thermo.IdealGas`. When imported, the declared name MUST match the
  source path, otherwise `E0601`. A package dependency's import alias belongs to the importer's
  namespace and does not rename the module at its definition (`src/Model.aev` of package
  `Library` may declare `module Model` or `module Library.Model`; a user importing it as
  `Renamed.Model` need not change it). An entry file run directly has no import-path context
  and is not checked.
- Module lookup and execution respect the defining package's scope: `import Helper` inside a
  dependency finds that dependency's `src/Helper.aev` and is never overridden by an
  application module of the same name. Load nodes are de-duplicated by (canonical file, owning
  package); a cycle is reported on the full node as `E0602`.
- `import Pkg` uses the `entry` of the dependency's own manifest.

### 7.1.2 Declaration order

Top-level declarations may refer to one another, including forward references and recursion:
the compiler first collects all top-level names and signatures and then checks the bodies.
Unit and dimension declarations may also come later; units from other modules are supplied by
`import`.

```aevia
fn f(x: Int): Int = g(x) + 1
fn g(x: Int): Int = x * 2
```

### 7.1.3 Exports

All top-level declarations are exported by default; `private` hides one:

```aevia
private let internal_scale = 1.0e-3
private fn helper(x: Real[L]): Real[L] = x * internal_scale

fn public_api(x: Real[L]): Real[L] = helper(x)
```

- `private` is allowed only on `let`, `fn`, `type`, `dimension`, `unit` and `class`, otherwise
  `E0202`; a private declaration is still visible inside its module.
- The constructors of a variant type are exported with the type and cannot be made `private`
  individually; the constructors of a `private type` do not enter the public interface.
- Instances are always exported with the module (separate compilation would otherwise lose
  them); there is no `private instance` (`E0202`).

## 7.2 Import and open

### 7.2.1 `import`

```aevia
open Units.SI
import Math as M
import Data.Csv { read_csv, Table }
```

- `import M` binds `M` as a qualified name, used as `M.f`; `import M as N` aliases it.
- `import M { f, g }` is a selective open and equals `open M { f, g }`.
- Imports are idempotent; repeating one is not an error.
- A module that cannot be found is `E0607`, and the diagnostic gives the expected file path.

### 7.2.2 `open`

- `open M` brings every exported name of `M` into scope unqualified; `open M { f, g }` brings
  only the listed names.
- `open` affects only the current module and is not transitive (there is no re-export).
- A local definition shadows an opened name. If two modules `open` the same name **with
  different types**, the error is `E0603`; the same type (one module opened twice) is not a
  clash.
- `import` and `open` can bring in values (functions, constants, unit values), nominal variant
  constructors and **transparent type aliases** (after `import A { Pair }` or
  `open Physics.Ising` you can write `Params`); an alias is exported with the interface and
  expanded in the importer.

**Units do not depend on imports.** The kernel unit table is global: with no `import` or
`open`, both `3 km` and `to(m, x)` work (there is no module-level unit namespace, §5.9). A unit
a user declares with `unit` is exported with its module. A unit value cannot be reached by the
qualified name `Units.SI.km`.

### 7.2.3 Qualified names

`Math.sqrt`, `Thermo.IdealGas.pressure`: a qualified name starts at a module root; there are no
relative imports. It is a lookup path and introduces no binding. Types, constructors and values
can all be qualified, provided the module is imported first.

### 7.2.4 Name resolution order

An unqualified name `x` is looked up in:

1. local bindings (parameters, `let`, patterns);
2. the current module's top-level declarations;
3. names the current module `open`s;
4. the Prelude;
5. otherwise `E0301`.

Earlier steps shadow later ones; a clash is `E0603` only inside step 3.

## 7.3 Dependencies, interfaces and identity

- The module dependency graph MUST be acyclic; a cyclic import is `E0602`. Type, class and
  instance dependencies are ordered too, and instance resolution needs the relevant modules
  already checked.
- Each module exports an **interface**: types and constructors, value signatures, units, class
  declarations, instances and alias definitions. An alias MUST carry its expansion, or the
  importer cannot expand `type Vec2 = { x: Float, y: Float }` (exporting only the name gives
  `E0301`).
- A module merges only the interfaces in its own dependency closure; it does not gain another
  module's interface because that module happened to be checked earlier. There are no implicit
  global instances: an instance is visible only from imported modules, and the standard
  library's common instances are imported by the Prelude.
- An interface can be cached as a `.aevi` file; the invalidation key is the source hash, the
  dependency-interface hashes, the capability set and the compiler build (§9.11.3).
- **Nominal identity is symbolic.** A class key is `(defining module path, class name)`; an
  instance key adds the canonical head type (aliases expanded first); an ADT's identity is the
  canonical source directory of the defining package (with its name and version) plus the
  source path inside the package, and the readable `module` name is kept separately. Two local
  packages of the same name and version at different source directories therefore have
  different type and instance keys; two import aliases of one package share an identity.
  `AdtId` is only an in-process witness and does not enter interfaces, caches or cross-process
  identity.
- Stand-alone sources and virtual sessions (the REPL, unsaved buffers) also have a stable
  source identity; identity uses no `FileId`, whole-file content hash or random nonce.
- An ADT's declaration schema is the canonical encoding of the nominal type graph reachable
  from it (constructor names and order, expanded field types, parameter kinds,
  alpha-normalised dimension bindings); an unrelated source edit does not change it.
- A private class and its local instances keep an exact run-time key but do not enter the
  public interface; a class method passed as a value or captured by a closure keeps its full
  `ClassKey`.
- A public transparent alias carries its resolved template; a private helper alias is expanded
  at its definition, so consumers never see it.

## 7.4 Type-class instances and modules

- Importing a module imports its classes, instances and aliases. Imported instances take part
  in **global coherence** checking (overlap is `E0311`, and the message names both modules)
  but are not re-checked for orphans.
- Orphan rule: an instance MUST be defined in the module of the type or of the class,
  otherwise `E0604`; instances generated by `derive` follow the same rule.
- A `private class` is visible only in its defining module, and its instances do not enter the
  interface.
- At run time dispatch uses witnesses at **quantified call sites** only (§9.2.3); the checker
  MUST be able to prove that the witnesses of candidate instances are pairwise disjoint,
  otherwise it reports `E0311`/`E0310` at check time. It never guesses from run-time shapes.

## 7.5 Packages

### 7.5.1 Manifest

`aevia.toml`:

```toml
[package]
name = "kinematics"
version = "0.1.0"
entry = "src/main.aev"      # optional; default src/main.aev

[dependencies]
Units = "^1.2"                                        # official scope: aevia/units
Plot = { package = "apich/plot", version = "~2.1" }   # registry package
Kinematics = { path = "../kinematics" }               # local path package
Dev = { path = "../dev", package = "apich/dev", version = "^0.3" }  # path for local work, version once published
```

Status: the manifest reader reads **only** `[package]` (keys `name`, `version`, `entry`) and
`[dependencies]`. Other tables (`[compat]`, `[capabilities]`, `[build]`, `[profile.*]`) and
other `[package]` keys (`edition`, `authors`, `description`) are `E0605`. Description, licence,
keywords and repository are supplied as options to `aevia publish` (§9.13). Capability
declarations have no run-time permission check; only `--caps` exists (§9.14). There is no
`git` dependency (`E0605`).

- `name` is the package name; `version` is SemVer 2.0 `major.minor.patch`, optionally with
  `-pre-release`; a published version MUST NOT carry `+build` (two versions differing only in
  build metadata have equal precedence and the registry cannot tell them apart).
- A dependency has one of three sources: registry, path, or both together. The registry is
  content-addressed and signed.
- The `entry` MUST define `main : IO Unit` for `aevia run`. Without `main` the CLI reports
  `this program has no main` (no diagnostic code, exit code 1); a library package is only
  `import`ed.

#### 7.5.1.1 Dependency declarations and version requirements

A key of `[dependencies]` is an **import root** (`UpperCamelCase`, no `-`; otherwise
`E0605`). Values take these forms:

| Form | Meaning |
|---|---|
| `Units = "^1.2"` | an official-scope package `aevia/units`. The package name is the key in `snake_case` (`UnitsExtra` → `aevia/units_extra`); an acronym key (`FFT`) needs an explicit `package` |
| `Plot = { package = "apich/plot", version = "~2.1" }` | a registry package named `owner/name` |
| `Lib = { path = "../lib" }` | a local path package |
| `Dev = { path = "../dev", package = "apich/dev", version = "^0.3" }` | both: `path` when building this package from source; only `package`/`version` when it is used as a registry package (the `path` directory does not exist). **A published manifest MUST NOT contain a path-only dependency**; the server rejects it at publish time (`E_MANIFEST`) |

`version` is a **version requirement** (Cargo syntax and meaning):

| Form | Meaning |
|---|---|
| `1.2.3`, `^1.2.3` | `>=1.2.3, <2.0.0` (a bare version is `^`) |
| `^0.2.3` / `^0.0.3` | `>=0.2.3, <0.3.0` / exactly `0.0.3` |
| `~1.2.3`, `~1.2`, `~1` | `>=1.2.3, <1.3.0`, `<1.3.0`, `<2.0.0` |
| `1.*`, `1.2.x` | wildcard |
| `>=1.2, <2`, `=1.2.3`, `>1`, `<=1.2` | comparators; a comma means conjunction |
| `*` | any **release** version |

A pre-release (`1.0.0-rc.1`) satisfies a requirement only if some comparator names a
pre-release of the **same** `major.minor.patch`; `^1` and `*` never select a pre-release, so
`aevia update` does not silently adopt a release candidate.

#### 7.5.1.2 Resolution: at most one version per compatibility series

The resolver is `aevia-resolve` (a pure crate with no I/O, shared by the toolchain and the
registry):

1. It satisfies every requirement, the dependencies of each selected version are satisfied
   too, and there is **no dependency cycle** (a cycle is an error and is listed).
2. **At most one version per compatibility series of each package.** The series is the major
   version for `>=1.0` (`1.x`), `0.y` for `0.y`, and `0.0.z` for `0.0.z`. So `1.4` and `1.9`
   never appear together, while `1.x` and `2.x` can coexist with distinct nominal types
   (§7.3). Coexistence needs the two edges to use **different import roots**; the
   program-wide import-root table still maps one root name to one package, otherwise `E0606`
   (the message names the two contending versions).
3. A **locked** version is preferred; otherwise the **highest** compatible version is chosen. A
   `yanked` version can be selected only if the lock file already contains it.
4. The result does not depend on index iteration order; when unsatisfiable, the explanation is
   derivation-style: which requirement could not be met, which candidates were tried and why
   each failed.
5. The implementation is a backtracking solver with conflict-directed backjumping (not PubGrub,
   because one requirement can be satisfied by several series).

The index `deps` of a registry package records only `package`, `req` and, when it differs from
the default import root, `import`.

### 7.5.2 Lock file

`aevia.lock` records each dependency's exact version, source and digest. A build MUST use the
lock file; `aevia update` updates it, and under `--locked` any change is an error. Format
(`version = 1`, fixed fields and order, packages sorted by `(source, import)`, so an unchanged
resolution yields a byte-identical file):

```toml
[[package]]
name = "apich/plot"          # registry package: registry identity owner/name
version = "2.1.3"
import = "Plot"              # import root (one entry per alias, as for path packages)
source = "registry+aevia"    # the registry's logical name, not a URL: a mirror change does not touch the lock
checksum = "sha256:…"        # archive digest from the index (the signature covers it)

[[package]]
name = "dep"                 # path package: [package] name
version = "0.2.0"
import = "Dep"
source = "path+../dep"
hash = "fnv1a64:…"           # digest of manifest + source tree + entry
```

- Paths are relative to the root package; the digest of a path dependency covers the canonical
  manifest, `src/**/*.aev` and the entry file, so changing `entry` or a dependency edge also
  invalidates `--locked`; a symlink cycle in a source directory is `E0607`.
- Index: `.aevia/registry/index/<owner>/<name>` (shareable through `$AEVIA_REGISTRY_DIR`) is
  the verbatim body of `GET /api/v1/index/{owner}/{name}`. Resolution reads only these files;
  when one is missing, `aevia add/update/tree` call `aevia-pkg index` to download it
  (forbidden by `--offline`). **The compiler itself never touches the network.**
- Source: `aevia fetch` (= `aevia-pkg vendor`) downloads archives, verifies the digest and the
  OpenPGP/SSH signature, unpacks into `.aevia/registry/src/<owner>/<name>/<version>/` and
  writes `.aevia-pkg-checksum`. A build MUST read only these directories and rejects a
  directory without the marker or with a marker that differs from the index digest (`E0607`),
  and a directory whose manifest dependencies disagree with the index record (`E0605`).
- `aevia update` without arguments ignores the lock and takes the newest versions;
  `aevia update owner/name…` moves only the named packages. `aevia tree` prints the dependency
  tree. `aevia clean` keeps `.aevia/registry`; only `aevia clean --registry` removes it.
- `aevia add` resolves the whole candidate graph before staging the manifest and lock; an
  ordinary I/O failure restores the original manifest, and a failed resolution rewrites
  nothing.

Status: the resolver lets incompatible series of one package coexist, but giving each package
its own dependency namespace (a per-package `Roots` lookup) is not implemented, so coexistence
requires distinct import roots.

### 7.5.3 Directory layout

```
kinematics/
  aevia.toml
  aevia.lock
  src/
    main.aev
    kinematics.aev
    thermo/ideal_gas.aev
  tests/fall_time.aev
  examples/projectile.aev
  docs/
```

- Every `.aev` file under `src/` is a module; `main.aev` is the default entry. `aevia test`
  compiles and runs `tests/`.
- `aevia run --example name` resolves `examples/<name>.aev` by name (a name is not a path; one
  with a path separator is rejected) and lists the available examples when it finds none;
  `--example` and a file argument are mutually exclusive.
- `aevia doc` writes Markdown API documentation, by default into `docs/`.

### 7.5.4 Tests

```aevia
# tests/fall_time.aev
open Kinematics

test "fall time from 20 m" =
    approx_eq(fall_time(20.0 m), 2.019 s, 0.001 s)
```

- A `test` declaration is a `Bool` expression; `aevia test` runs them all. `approx_eq(a, b, tol)`
  requires `a`, `b` and `tol` to have the same dimension.
- To test a compile-time error use `compile_fail("<source>")` (§8.13): it hands a source
  fragment to the compiler and requires that **compilation fail**; only fragments without
  imports are supported at present. `@compile_fail` is not an attribute (an unknown attribute
  is `E0210`). Property tests use `forall` (§8.13).

### 7.5.5 Documentation

- A `##` doc comment attaches to the next declaration and contains Markdown; `aevia doc`
  extracts type signatures, units and examples, and reports undocumented declarations in the
  index.
- Code blocks tagged `aevia` in doc comments are compile-checked: `aevia test --doc [FILE…]`
  extracts the fenced blocks and compiles each one; the first line of a block may be
  `compile_fail` or `compile_fail EXXXX`. A snippet compiles as its own synthetic module: the
  declaring module's `import`/`open` lines are copied in, but the module's own declarations
  do not enter the snippet implicitly; import the declaring module explicitly when needed. A
  failing dependency or inherited import does not satisfy `compile_fail`.

## 7.6 Versions and compatibility

- `edition` selects syntax and semantics (the manifest does not read it yet, §7.5.1); libraries
  follow SemVer: a breaking change raises major, a new API raises minor, a fix raises patch.
- A change to a type or dimension signature, to a unit's scale or offset, or adding or removing
  an instance is breaking; adding a unit is minor.

## 7.7 Module errors

| Code | Situation |
|---|---|
| `E0601` | module name does not match its path |
| `E0602` | cyclic import |
| `E0603` | two modules open the same name with different types |
| `E0604` | orphan instance |
| `E0605` | invalid package manifest (unknown table or key, non-SemVer version, `git` dependency, dependency key not a module name, lock and index disagree) |
| `E0606` | dependency version conflict (unsatisfiable requirement, dependency cycle, import-root contention) |
| `E0607` | module or package not found (including registry index not cached, package not fetched, checksum marker mismatch, symlink cycle) |
| `E0608` | the embedding API `aevia_core::run` got no runnable module (the CLI prints only a message when `main` is missing) |

The next chapter gives the standard library's signatures.
