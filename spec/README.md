# Aevia Language Specification

**Aevia — Design Standard**

Aevia is a pure functional language for scientific computing in which
**dimensions are part of the type system**. A quantity is not a number with a
runtime tag; it is a value whose type records its physical dimension. The
compiler rejects `3 m + 2 s`, `sin(3 kg)` and `exp(1 m)` before any code runs.

The language is designed to sit on top of the `rssn` computer-algebra and
scientific-computing engine. Symbolic and numeric evaluation are two phases of
the same expression; units, dimensional inference, rewriting, numerical kernels,
JIT compilation and physics simulations are provided by the backend. Aevia does
not maintain a second operator list or a second unit table: it consumes the
RSSN **Kernel Manifest** and `rssn-dim` registry, and generates its `Rssn`
module from them. See [13 — Extensibility](13-extensibility.md) and the
[backend requirements](../RSSN-INTEGRATION-REQUIREMENTS.md).

> This document is the **normative design**. It is written to be implementable:
> the grammar is complete, the typing rules are stated, the runtime architecture
> is fixed, and every backend dependency is identified. Examples and rationale
> are non-normative.

---

## Reading guide

| Chapter | Contents | Normative |
|---|---|---|
| [00 — Overview](00-overview.md) | Goals, non-goals, principles, ten-minute tour | Yes (goals) |
| [01 — Lexical structure](01-lexical.md) | Source encoding, literals, units, layout | Yes |
| [02 — Syntax](02-syntax.md) | Declarations, expressions, patterns, modules | Yes |
| [03 — Type system](03-types.md) | Kinds, types, inference, classes, effects | Yes |
| [04 — Dimensions](04-dimensions.md) | Dimension algebra, typing rules, polymorphism, `Dyn` | Yes |
| [05 — Units](05-units.md) | Unit declarations, prefixes, affine points, constants | Yes |
| [06 — Semantics](06-semantics.md) | Evaluation, values, symbolic/numeric phases, tensors | Yes |
| [07 — Modules and packages](07-modules.md) | Namespaces, visibility, packages, tests | Yes |
| [08 — Standard library](08-stdlib.md) | Prelude and library signatures | Mostly yes |
| [09 — Runtime](09-runtime.md) | Pipeline, interpreter, REPL, CLI, LSP, performance | Yes (architecture) |
| [10 — rssn backend](10-backend.md) | Mapping to rssn, bridge design, gaps and risks | Yes |
| [11 — Examples](11-examples.md) | Worked programs from kinematics to simulation | No |
| [12 — Rationale](12-rationale.md) | Alternatives, trade-offs, known limits, roadmap | No |
| [13 — Extensibility](13-extensibility.md) | RSSN reflection, generated `Rssn`, plugins, registry, shape/device/sparse decisions | Yes |
| [Appendix A — Grammar](appendix-a-grammar.md) | Complete EBNF | Yes |
| [Appendix B — Diagnostics](appendix-b-diagnostics.md) | Error codes and message format | Yes |
| [Appendix C — Changelog](appendix-c-changelog.md) | Audit revisions | Informational |

后端接口清单：[`RSSN-INTEGRATION-REQUIREMENTS.md`](../RSSN-INTEGRATION-REQUIREMENTS.md)。这是
Aevia 规范能够成立的前提，不是可选的实现细节。

## Conformance language

The key words **MUST**, **MUST NOT**, **REQUIRED**, **SHOULD**, **SHOULD NOT**
and **MAY** are to be interpreted as described in RFC 2119. A conforming
implementation MUST accept every program in this specification's test corpus,
MUST reject every program marked ill-typed, and MUST produce the specified
diagnostic category (the exact wording is not normative).

## Design status

This is a **design standard**, not a frozen implementation. Chapters marked
normative are intended to be implemented as written; the changelog records every
audit round. The current text is **Revision 4**, after three rounds of
independent audits plus the extensibility/backend-integration redesign. There is
**no v1/v2 language split**: every capability is designed in; a backend that
does not yet implement one reports a capability diagnostic (`E1001`) instead of
removing the language feature.

## A one-sentence summary

> Aevia makes the dimension of a quantity a type, the unit a compile-time scale,
> the formula a first-class value, and the numeric result a deliberate request.

## Quick taste

```aevia
module Kinematics

open Units.SI

let g = 9.80665 m/s^2

fn fall_time(h: Float[L]): Float[T] =
    sqrt(2 * h / g)

fn main(): IO Unit = do
    let h = 20.0 m
    println("fall time from {h} = {fall_time(h)}")
```

```
$ aevia run kinematics.aev
fall time from 20 m = 2.019 s
```

The next chapter explains why this program type-checks, and what the language
does when it does not.
