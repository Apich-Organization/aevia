# Appendix B — Diagnostic catalogue

Diagnostics are part of the language experience. An error message MUST contain the code, the
primary location and expected versus actual, and, when it can be determined, at least one
actionable fix. This appendix lists the codes, severities and the message format. Every code in
the tables has an emission site in the compiler (checked by `grep` over all crates); exceptions
are noted in the "Status" paragraphs.

## B.1 Format

```
error[E0401]: dimension mismatch
  --> src/kinematics.aev:4:31
   |
 4 | fn f(h: Float[L]): Float[T] = sqrt(2 * h * g)
   |                               ^^^^^^^^^^^^^^^ expected T, found L/T
   |
   = left:  L/T
   = right: T
   = difference: L/T^2
```

- The first line is `severity[code]: message`; `-->` gives file:line:column; then come the
  source line and underline labels (kept short).
- `=` lines are notes (canonical forms, differences, related declarations); `help:` is a fix,
  at most two, ordered by confidence.
- An error with several locations uses several underline labels or a `= note:` pointing at the
  definition.

## B.2 Severity

| Level | Meaning |
|---|---|
| `error` | the program cannot compile or run |
| `warning` | the program runs but may be wrong |
| `note` | explanatory information, never alone |

## B.3 Lexical errors (E01xx)

| Code | Message | Typical fix |
|---|---|---|
| `E0101` | tab character in indentation | replace with spaces |
| `E0102` | inconsistent indentation | align to an existing level |
| `E0103` | expected an indented block | indent the next line |
| `E0104` | unclosed string/comment/bracket | add the closer |
| `E0105` | invalid character / escape | delete or replace |
| `E0106` | invalid numeric literal | check underscores and exponent |
| `E0107` | invalid superscript sequence | use `^n` |
| `E0108` | expected a unit after the numeric literal | write `3 * x` or add a unit |

## B.4 Syntax errors (E02xx)

| Code | Message | Typical fix |
|---|---|---|
| `E0201` | empty block | add an expression |
| `E0202` | syntax error (including `private` on a disallowed declaration, and `_` evaluated outside a call at run time) | check the nearby tokens |
| `E0203` | ragged tensor literal | make every level the same length |
| `E0204` | invalid pattern | check the constructor or fields |
| `E0205` | invalid operator section | use `(+)` or a lambda |
| `E0206` | reserved word used as identifier | rename |
| `E0207` | ambiguous type application | add a space before `<` or parenthesise the callee |
| `E0208` | duplicate field | delete the duplicate |
| `E0209` | invalid unit literal (including `3 (2)`: the parentheses do not hold a unit expression) | check the unit expression |
| `E0210` | unknown attribute | check the attribute name or arguments |

## B.5 Type errors (E03xx)

| Code | Message | Typical fix |
|---|---|---|
| `E0301` | unbound name | import or fix the spelling |
| `E0302` | type mismatch (including an unsolvable type-level `Nat` equation, an unknown capability name, a `to_system` name that is not a literal) | check annotations and arguments |
| `E0303` | infinite type (occurs check) | annotate or change the recursion |
| `E0304` | ambiguous type (including an unsolved `Nat` variable, a non-linear `Nat` equation, a shape variable with no binder, a missing `from_system<D>`) | add a type annotation |
| `E0305` | not a function | check the call |
| `E0306` | missing record field | add the field or change the type |
| `E0307` | constructor arity mismatch | add or remove arguments |
| `E0308` | pattern type mismatch (including a `Prop` used as a guard) | change the pattern |
| `E0309` | non-exhaustive match | add arms |
| `E0310` | no instance for class | import or add an instance |
| `E0311` | overlapping instances | merge or delete |
| `E0312` | non-positive recursive type | move it to the right of the arrow |
| `E0313` | IO action in a pure context | use `do` or return `IO` |
| `E0314` | cannot infer rank/element type (empty tensor, `read_npy` without a rank) | add a `Tensor<n, …>` annotation / `read_npy<2>` |
| `E0315` | wrong number of arguments | add or remove arguments |
| `E0316` | duplicate definition | rename or delete |
| `E0317` | cannot derive instance / unsupported parameter kind | write the instance by hand |
| `E0318` | function is less generic than its declared type parameters | make the body valid for every choice, or replace the parameters with the concrete types the body needs |

## B.6 Dimension errors (E04xx)

| Code | Message | Typical fix |
|---|---|---|
| `E0401` | dimension mismatch | give both sides the same dimension |
| `E0402` | exponent must be a dimensionless number / finite | the exponent must be dimensionless (an infinite exponent in the exact phase uses this code too, §6.3.4) |
| `E0403` | transcendental function requires dimensionless argument | divide by a unit |
| `E0404` | dimensionful base with a run-time exponent | literal exponent / `pow_dyn` |
| `E0405` | invalid affine point operation | use a difference or `lerp` |
| `E0406` | bare `0` used where a dimensioned value is expected | write `0 m` |
| `E0407` | unknown dimension/alias | declare or import |
| `E0408` | unsatisfiable dimension constraint | reserved |
| `E0409` | dimension occurs check | reserved |
| `E0410` | dimensionful `det`/`product` requires `Dyn<ρ>` | divide by a unit / `det_unit` |
| `E0411` | incompatible tensor element types or dimensions | unify the element type or dimension |
| `E0412` | `Dyn`/kernel dimension check failed (run time) | check the run-time dimensions |

Status: `E0408` and `E0409` are reserved and unreachable in the implementation. Dimension
unification is exponent-linear unification over a free abelian group; a residual with a
non-zero coefficient on a variable is always solvable, so the only failure is a non-zero
constant residual, which is `E0401`, and no occurs state arises (proof in
`aevia-types/tests/diagnostics_coverage.rs`).

## B.7 Unit errors (E05xx)

| Code | Message | Typical fix |
|---|---|---|
| `E0501` | unknown unit (including a stacked prefix `kkm` and use before declaration) | import or fix the spelling |
| `E0502` | unit not imported | reserved |
| `E0503` | prefix cannot be applied | reserved |
| `E0504` | affine offset dimension mismatch | check the offset |
| `E0505` | duplicate unit | delete the duplicate |
| `E0506` | the scale has no exact value for that exponent | check the definition or exponent |
| `E0507` | prefixed unit conflicts with explicit unit | reserved |
| `E0508` | affine unit in product/power | use it alone |
| `E0509` | `to`/`value_in` unit measures a different dimension | change the unit |
| `E0510` | affine unit used on a temperature difference | use `delta_to` |
| `E0511` | unit has no exact scale in the module's unit system (§5.11.4) | write the number in system units, or `to_system` |
| `E0512` | unknown unit system (§5.11.1) | use a name the kernel provides (natural, planck, atomic, geometrized) |

Status: `E0502`, `E0503` and `E0507` are reserved: there is no module-level unit namespace
(§5.9), so the corresponding states do not exist.

## B.8 Module errors (E06xx)

| Code | Message |
|---|---|
| `E0601` | module name does not match path |
| `E0602` | cyclic import |
| `E0603` | the same name opened by two modules with different types |
| `E0604` | orphan instance |
| `E0605` | invalid package manifest |
| `E0606` | dependency version conflict |
| `E0607` | module/package not found |
| `E0608` | no runnable module/entry point (embedding API) |

## B.9 Effect errors (E07xx)

| Code | Message |
|---|---|
| `E0701` | `do` statement or `<-` expects an `IO` action |
| `E0702` | `?` requires a `Result`/`Option` and a matching return boundary |

## B.10 Run-time errors (E08xx)

| Code | Message | Recoverable |
|---|---|---|
| `E0801` | division by zero | exact: no; floating point: IEEE |
| `E0802` | shape/index error | no |
| `E0803` | free symbols in `approx` | yes |
| `E0804` | cannot decide symbolic condition / assumption is false | no |
| `E0805` | stack overflow / tail-call or IO budget exceeded | no |
| `E0806` | domain error | yes |
| `E0807` | numerical method did not converge | yes |
| `E0808` | budget exhausted | yes |
| `E0809` | internal type/dimension invariant violated | no |
| `E0810` | IO error | yes |
| `E0811` | task failure: a result that cannot cross threads, a task awaited twice, or a stopped worker (§9.7.3) | yes |
| `E0899` | panic | no |

Status: a few built-ins borrow `E0806`–`E0809` to report their own argument or internal
failures (for example `approx_map`, `degree`); the class of the code is as in the table and the
message is whatever the built-in states.

## B.11 IO errors (E09xx)

| Code | Message |
|---|---|
| `E0901` | file not found |
| `E0902` | permission denied (including a capability not granted, §9.14) |
| `E0903` | parse error in data file |
| `E0904` | unsupported format |
| `E0905` | plot rendering failed |

## B.12 Warnings (W0xxx)

| Code | Message |
|---|---|
| `W0101` | unused variable |
| `W0102` | unused import (reserved) |
| `W0103` | shadowed binding |
| `W0201` | redundant match arm |
| `W0202` | discarded IO result |
| `W0203` | unreachable code (reserved) |
| `W0301` | unresolved dimension defaulted to 1 |
| `W0302` | `Real`/`Float` mix uses the exact `Real` path |
| `W0303` | unused symbol |
| `W0501` | display unit dropped (no emission site yet, §5.4.2) |
| `W0601` | unused private declaration |
| `W0602` | deprecated name used: `@deprecated("...")` on the declaration (§2.2.7/§13.8.1) |

Status: `W0102` and `W0203` are reserved. The checker sees only a flattened name table, so a
selective import cannot be told from `open M`; the language has no `return` or `break`, so the
AST cannot represent an unreachable statement. `W0501` currently has no emission site.

## B.13 Writing messages

1. **Speak plainly.** `expected Real[L], found Real[T]` beats `type unification failed`.
2. **Point small.** The underline covers the smallest erroneous subtree.
3. **Give the difference.** A dimension error lists the canonical forms `left`, `right` and
   `difference`.
4. **Give one fix.** Offer `help` only with high confidence; do not list five guesses.
5. **Do not repeat.** Report one error once and point to the primary location with `note`.
6. **Preserve source.** A source line in a message does not truncate operators; wrap if needed.
7. **Be parseable.** Codes and positions are stable, for IDE and CI parsing.

## B.14 Example: type error

```
error[E0302]: type mismatch
  --> t.aev:2:14
   |
 2 | let n: Int = 1.5
   |              ^^^ a bare numeric literal cannot have type Int
   |
```

## B.15 Example: unit error

```
error[E0501]: unknown unit `kms`
  --> t.aev:2:9
   |
 2 | let d = 3.2 kms
   |         ^^^^^^^
   |
help: did you mean `km`?
```

## B.16 Example: run-time shape error

```
error[E0802]: matmul requires left columns (2) = right rows (3)
  --> r.aev:5:13
   |
 5 |     let C = A @ B
   |             ^^^^^
   |
```

## B.17 Extension and backend errors (E10xx)

| Code | Message | Notes |
|---|---|---|
| `E1001` | backend capability or witness missing | carries the capability name and the minimum RSSN version; also used for a missing kernel dimension witness or an invalid callback handle |
| `E1002` | ambiguous operator/overload | lists the candidate signatures |
| `E1003` | no matching operator/overload | lists candidate signatures or the dynamic entry; also used for a callback arity or shape mismatch |
| `E1004` | incompatible RSSN api_version / kernel result fields | asks to upgrade or downgrade; also when the kernel omits or adds a declared record field |
| `E1005` | no pinned overload / signature hash mismatch | regenerate the `Rssn` module |
| `E1006` | plugin ABI/load/registration failure | check the plugin signature or version |

These are **environment/backend** errors, not type errors in the user's program; a diagnostic
MUST offer an actionable alternative (for example "install an RSSN with `core.gpu`" or "use
`Rssn.dynamic`").

The next appendix records the specification's revision history.
