# Aevia `plan.md` vs. Implementation — Gap Analysis

> Reviewed: 2026-06-06 | Codebase: `/home/pana/dev/aevia`

---

## ✅ Fully Implemented

| Feature | Plan Section | Evidence |
|---|---|---|
| 7 SI base dimensions + unit aliases (`type Acceleration = m / s^2`) | §2.1, §2.3 | `src/types/dim.rs`, `checker.rs` |
| Variable declarations with dimensional annotations (`let mass: kg`) | §2.2 | `ast.rs` `Stmt::Let`, `checker.rs` |
| Structural physical types (`pub struct Particle { ... }`) | §2.3 | `ast.rs` `Item::Struct`, parser |
| `pub` / `pub(crate)` / private visibility | §3.1 | `ast.rs` `Visibility`, `lint/mod.rs` |
| Hierarchical `mod` declarations (inline & file) | §3.2 | `src/modules/mod.rs`, `parser/items.rs` |
| `use` paths + aliasing via `as` | §3.3 | `ast.rs` `Item::Use`, `modules/mod.rs` |
| `op` DSL: `properties`, `simplify`, `egraph` blocks | §4 | `src/ops/`, `parser/items.rs` |
| Expression-bodied functions (`:=`) | §5.1 | `ast.rs` `FunctionBody::Expression` |
| Block-bodied functions (`{}`) | §5.2 | `ast.rs` `FunctionBody::Block` |
| `if` / `else` control flow | §6.1 | `ast.rs` `Expr::If`, lowering |
| `loop` / `break` | §6.1 | `ast.rs` `Expr::Loop`, `Stmt::Break` |
| `unsafe transmute` | §6.3 | `ast.rs` `Expr::UnsafeTransmute`, lowering |
| `///` and `//!` doc comments + `# Physical Context` | §8 | `src/parser/docs.rs` |
| `Aevia.toml` manifest (`[package]`, `[profile]`, `[dependencies]`, `[kernels]`) | §9.1 | `src/manifest.rs` |
| `aevia new` | §9.3 | `src/project.rs`, `commands/mod.rs` |
| `aevia build` → `.rssn` binary snapshot | §9.3 | `commands/mod.rs` |
| `aevia run` → JIT execution | §9.3 | `commands/mod.rs` |
| `aevia check` → dimensional type check | §9.3 | `commands/mod.rs` |
| `aevia fmt` → formatter | §9.3 | `src/fmt/mod.rs`, `commands/mod.rs` |
| `aevia lint` → static analysis + unused-import + unsafe warnings | §9.3 | `src/lint/mod.rs` |
| `aevia doc` → Markdown/HTML generation | §9.3 | `src/doc/mod.rs` |
| `aevia shell` → interactive REPL | §9.3 | `src/shell/mod.rs` |
| `aevia test` → `.ae` test harness | devplan §5 | `src/test_runner/mod.rs` |
| JIT pipeline: Parser → Dim check → E-graph → Fusion → RSSN | §10 | `src/pipeline/mod.rs`, `fusion/mod.rs` |
| Single-file run shorthand (`aevia file.ae`) | §9.3 | `src/cli.rs` |
| Physical literal suffixes (`1.0_s`, `9.8_m/s^2`) | §2.2 | `parser/expr.rs` |
| `#[jit_kernel]` / `#[simplify_fusion]` attributes (parse + apply) | §7.2 | `ast.rs` `Attribute`, `fusion/mod.rs` |
| `kernels/` precompiled kernel stubs | §9.2 | `manifest.rs`, `commands/mod.rs` |
| Integration tests (phase5–8) | devplan | `tests/phase5_tools.rs` … `phase8_fusion.rs` |

---

## ⚠️ Partially Implemented / Gaps

### 1. `match` expressions (§6.2) — **Missing from AST and Parser**

The spec shows:
```ae
match op_result {
    Some(v: kg) => print(v),
    None        => log("invalid"),
}
```
**Current state:** There is no `Expr::Match` variant in `ast.rs`, no match-expression parser in `parser/expr.rs`, and no lowering for it. The `Expr::If` covers simple branching, but `match` with pattern guards is unimplemented.

---

### 2. `while` / `for` loop keywords (§6.1) — **Missing**

The spec lists `for`, `while`, `loop` as supported control flow. Only `loop { ... }` with `break` is in the AST and parser. `while` and `for` have no AST variants, no parser rules, and no lowering paths.

> The `loop` lowering in `lowering/mod.rs` approximates all loops as `for_loop(0, 1000, 1, body)` — a fixed-iteration stub, not a true `while` or bounded `for`.

---

### 3. `elseif` keyword (§6.1) — **Missing**

The spec shows `elseif` as a distinct keyword (not `else if`). Current `Expr::If` chains via nested `If` nodes but `elseif` is not a parsed keyword in `parser/expr.rs`. The parser only sees `if ... { } else { }` (no `elseif` token).

---

### 4. `macro_rules!` expansion (§7.1) — **Parse-only, no expansion**

The spec includes `macro_rules! create_vector_type { ... }`. The parser has no grammar for `macro_rules!` at all — it cannot even parse the syntax. There's no `Item::Macro` in `ast.rs`. This is listed as a Phase 4 deliverable in `devplan.md` but was not finished.

---

### 5. `tensor[N, M]` types (§7.2, §6.3) — **No type system support**

The spec references `tensor[1024, 1024]` as a type annotation for function parameters and return types. While `unsafe transmute` is in the AST, the type system (`types/dim.rs`, `types/checker.rs`) has no concept of a tensor shape — it only tracks SI dimensional exponents. The `tensor[...]` annotation cannot be parsed as a `DimExpr` and would silently fail.

---

### 6. `aevia shell` `:load` does only a check, not JIT run (§9.3) — **Minor**

The spec says `aevia shell` supports "running single file code." `:load <file>` in the current REPL is an alias for `:check` (dimensional check only). It does not execute the file via JIT as the `:run` intent implies.

---

### 7. Build profiles / GPU backend (§9.1, Phase 6) — **Stub only**

`Aevia.toml` parses `[profile.release] backend = "rssn-gpu"` correctly, but the build/run commands ignore profile settings entirely — they always use the default CPU JIT. No GPU dispatch (CUDA/Metal) is wired up. This is a known Phase 6 item.

---

### 8. `aevia run` always calls with `args = vec![1.0; params.len()]` — **UX gap**

When running a function, the JIT is always invoked with `1.0` for every parameter. There is no way for the user to pass actual argument values via the CLI. Useful for demos but not for real computation.

---

### 9. LSP / editor support (Phase 6) — **Not started**

The devplan §6 lists "editor support (syntax highlighting, basic LSP)" as a Phase 6 goal. Nothing in the codebase relates to LSP, TextMate grammars, or tree-sitter.

---

### 10. Documentation website (Phase 6) — **Not started**

`aevia doc` generates local Markdown but there is no static site generator, deployment pipeline, or documentation website.

---

## Summary Table

| Area | Status |
|---|---|
| Core type system (7 SI dims, aliases, structs) | ✅ Complete |
| Parser (functions, ops, modules, imports, attributes, docs) | ✅ Complete |
| `loop` + `break` + `if`/`else` | ✅ Complete |
| Lowering → RSSN DAG + JIT | ✅ Complete |
| E-graph saturation + kernel fusion | ✅ Complete |
| All CLI subcommands wired up | ✅ Complete |
| `match` expressions | ❌ Missing |
| `while` / `for` loop keywords | ❌ Missing |
| `elseif` keyword | ❌ Missing |
| `macro_rules!` parsing + expansion | ❌ Missing |
| `tensor[N,M]` type annotation | ❌ Missing |
| Shell `:load` does JIT run (not just check) | ⚠️ Partial |
| Build profile / GPU backend selection | ⚠️ Stub |
| CLI `aevia run` with user-supplied args | ⚠️ Always `1.0` |
| LSP / editor integration | ❌ Not started (Phase 6) |
| Documentation website | ❌ Not started (Phase 6) |
