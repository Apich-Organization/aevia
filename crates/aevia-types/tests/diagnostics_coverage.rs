//! Every diagnostic code in appendix B is reachable — or listed as not.
//!
//! PLAN.md §3 rule 3: "Every diagnostic code the spec documents is reachable
//! from a test." This table is the audit of that rule for the front end. Each
//! code has one disposition:
//!
//! * [`Cover::Front`] — a program that produces the code, run here through the
//!   lexer, parser and checker, so the claim is exercised rather than asserted;
//! * [`Cover::OtherCrate`] — emitted by a crate the checker cannot reach
//!   (`aevia-core`'s evaluator, the bridge, the loader); the reason names the
//!   owning file and the emission site (a `file:line` pointer as of this
//!   writing — line numbers move when the runtime changes, the file is the
//!   stable part, so a runtime owner should search the named file/function);
//! * [`Cover::Unreachable`] — no implementation state produces it, with the
//!   concrete reason (usually a missing distinction in another crate).
//!
//! The table is also checked against [`documented_codes`], so a code added to
//! appendix B cannot silently fall out of the audit. After a runtime change,
//! re-running this test re-verifies the front-end half; the `OtherCrate`
//! pointers are documentation for the runtime owner, not executed claims.

use aevia_syntax::lex;
use aevia_syntax::parse_module;
use aevia_types::Scheme;
use aevia_types::Type;
use aevia_types::check::Export;
use aevia_types::check::Import;
use aevia_types::check::check_module;
use aevia_types::check::check_module_with_imports;

/// How one appendix-B code is reached.
enum Cover {
    /// A program the lexer/parser/checker rejects with this code.
    Front(&'static str),
    /// A program checked against an explicitly absent JIT capability.
    NoJit(&'static str),
    /// A program that needs imports in scope to reach the code.
    Imports(&'static str),
    /// Another crate owns it; the checker never emits it.
    OtherCrate(&'static str),
    /// No state in the implementation produces it; the reason says why.
    Unreachable(&'static str),
}

/// Codes the implementation emits that appendix B does not list.
///
/// `W0602` (deprecated name used) is required in spirit by §13.8.1's warning
/// period, but the catalogue has no row for it; the specification must add one.
fn extension_codes() -> &'static [&'static str] {
    &["W0602"]
}

/// Runs `source` through every phase that accepts it and returns the codes.
///
/// A parse error stops the pipeline: the checker must not invent a second
/// diagnostic for a program the parser already rejected (§2.9 recovery).
fn codes_of(source: &str) -> Vec<String> {
    let mut codes = Vec::new();
    let lexed = match lex(0, source) {
        | Ok(lexed) => lexed,
        | Err(diagnostics) => return codes_of_diagnostics(&diagnostics),
    };
    let (module, parse_diagnostics) = parse_module(0, source, &lexed);
    codes.extend(codes_of_diagnostics(&parse_diagnostics));
    if !parse_diagnostics.has_errors() {
        let result = check_module(0, &module);
        codes.extend(codes_of_diagnostics(&result.diagnostics));
    }
    codes
}

/// Runs `source` with two `open`s that bring the same name in (E0603, §7.2.2).
fn codes_of_ambiguous_imports(source: &str) -> Vec<String> {
    let lexed = lex(0, source).expect("lexes");
    let (module, parse_diagnostics) = parse_module(0, source, &lexed);
    assert!(!parse_diagnostics.has_errors());
    let value = |ty: Type| {
        Export::Value {
            scheme: Scheme::mono(ty),
            decl: 0,
            arity: 0,
        }
    };
    let imports: Vec<Import> = vec![
        (
            String::new(),
            [("x".to_string(), value(Type::int()))]
                .into_iter()
                .collect(),
        ),
        (
            String::new(),
            [("x".to_string(), value(Type::string()))]
                .into_iter()
                .collect(),
        ),
    ];
    let result = check_module_with_imports(0, &module, &imports);
    codes_of_diagnostics(&result.diagnostics)
}

fn codes_of_diagnostics(diagnostics: &aevia_syntax::Diagnostics) -> Vec<String> {
    diagnostics
        .items
        .iter()
        .map(|item| item.code.clone())
        .collect()
}

/// The codes appendix B documents, transcribed from the spec.
///
/// This is deliberately a literal list: it is the contract the table below is
/// checked against, and changing one without the other fails the test.
fn documented_codes() -> Vec<&'static str> {
    let mut codes = Vec::new();
    macro_rules! push {
        ($($code:literal),* $(,)?) => { codes.extend([$($code),*]); };
    }
    // B.3 lexical, B.4 syntax.
    push!(
        "E0101", "E0102", "E0103", "E0104", "E0105", "E0106", "E0107", "E0108", "E0201", "E0202",
        "E0203", "E0204", "E0205", "E0206", "E0207", "E0208", "E0209", "E0210",
    );
    // B.5 types.
    push!(
        "E0301", "E0302", "E0303", "E0304", "E0305", "E0306", "E0307", "E0308", "E0309", "E0310",
        "E0311", "E0312", "E0313", "E0314", "E0315", "E0316", "E0317",
    );
    // B.6 dimensions.
    push!(
        "E0401", "E0402", "E0403", "E0404", "E0405", "E0406", "E0407", "E0408", "E0409", "E0410",
        "E0411", "E0412",
    );
    // B.7 units.
    push!(
        "E0501", "E0502", "E0503", "E0504", "E0505", "E0506", "E0507", "E0508", "E0509", "E0510",
        "E0511", "E0512",
    );
    // B.8 modules.
    push!(
        "E0601", "E0602", "E0603", "E0604", "E0605", "E0606", "E0607", "E0608",
    );
    // B.9 effects.
    push!("E0701", "E0702");
    // B.10 runtime.
    push!(
        "E0801", "E0802", "E0803", "E0804", "E0805", "E0806", "E0807", "E0808", "E0809", "E0810",
        "E0899",
    );
    // B.11 IO.
    push!("E0901", "E0902", "E0903", "E0904", "E0905");
    // B.12 warnings.
    push!(
        "W0101", "W0102", "W0103", "W0201", "W0202", "W0203", "W0301", "W0302", "W0303", "W0501",
        "W0601",
    );
    // B.17 extension/backend.
    push!("E1001", "E1002", "E1003", "E1004", "E1005", "E1006");
    codes
}

/// The audit: one disposition per documented code.
fn table() -> Vec<(&'static str, Cover)> {
    use Cover::Front;
    use Cover::Imports;
    use Cover::NoJit;
    use Cover::OtherCrate;
    use Cover::Unreachable;
    vec![
        // ----- B.3 lexical -------------------------------------------------
        ("E0101", Front("fn f() =\n\t1\n")),
        (
            "E0102",
            Front("fn f() =\n      let a = 1\n    let b = 2\n    let c = 3\n"),
        ),
        ("E0103", Front("fn f(): IO Unit = do\n1\n")),
        ("E0104", Front("let x = \"unclosed\n")),
        ("E0105", Front("let x = \"\\q\"\n")),
        ("E0106", Front("let x = 1_\n")),
        // A superscript minus with no digit is an invalid sequence.
        ("E0107", Front("let x = 3 m\u{207B}\n")),
        (
            "E0108",
            // `3 (2)`: the parenthesised part is not a unit at all (the
            // parser distinguishes it from a malformed unit, E0209).
            Front("let x = 3 (2)\n"),
        ),
        // ----- B.4 syntax --------------------------------------------------
        ("E0201", Front("fn f(): Int =\n    let x = 1\n")),
        ("E0202", Front("let x = 1 let y = 2\n")),
        ("E0203", Front("let a = [[1.0, 2.0], [3.0]]\n")),
        ("E0204", Front("let x = match 1 with\n    | 1.5 -> 0\n")),
        (
            "E0205",
            // A partial operator section (`(== 1)`) is not in the grammar;
            // only `(+)` and lambdas are functions (§1.8.5).
            Front("let f = (== 1)\n"),
        ),
        ("E0206", Front("let if = 1\n")),
        (
            "E0207",
            // Adjacent `<...>` on a callee that is not a name: comparison and
            // type application are genuinely ambiguous (§2.7.2).
            Front("let x = 3<Int>\n"),
        ),
        ("E0208", Front("let p = { x = 1, x = 2 }\n")),
        // An empty parenthesised unit literal: well-formed brackets, no unit inside.
        ("E0209", Front("let x = 3 ()\n")),
        ("E0210", Front("@nope\nfn f(): Int = 1\n")),
        // ----- B.5 types ---------------------------------------------------
        ("E0301", Front("let x = missing\n")),
        ("E0302", Front("let x: Int = \"s\"\n")),
        ("E0303", Front("let f = fn (x) => x(x)\n")),
        ("E0304", Front("fn f<d>(x: Real[d]): Dyn<Real> = dyn(x)\n")),
        ("E0305", Front("let x = \"s\"(1)\n")),
        (
            "E0306",
            Front("let p: { x: Int } = { x = 1 }\nlet { y } = p\n"),
        ),
        (
            "E0307",
            Front(
                "fn f(o: Option<Int>): Int = match o with\n    | Some(a, b) -> 1\n    | None -> 0\n",
            ),
        ),
        (
            "E0308",
            Front("fn f(o: Int): Int = match o with\n    | Some(a) -> 1\n    | _ -> 0\n"),
        ),
        (
            "E0309",
            Front("fn f(o: Option<Int>): Int = match o with\n    | Some(a) -> a\n"),
        ),
        ("E0310", Front("let s: String = show(fn (x) => x)\n")),
        (
            "E0311",
            Front(
                "class P a where\n    p(x: a): Int\ntype V = { x: Float }\ninstance P for V where\n    p(v) = 1\ninstance P for V where\n    p(v) = 2\n",
            ),
        ),
        ("E0312", Front("type Loop = Loop -> Int\n")),
        (
            "E0313",
            Front("fn f(): Int = do\n    println(\"x\")\n    1\n"),
        ),
        ("E0314", Front("let e = []\n")),
        ("E0315", Front("fn f(x: Int): Int = x\nlet y = f()\n")),
        ("E0316", Front("fn f(): Int = 1\nfn f(): Int = 2\n")),
        (
            "E0317",
            Front("type F = | F(Int -> Int)\nderive Eq for F\n"),
        ),
        // ----- B.6 dimensions ----------------------------------------------
        ("E0401", Front("let x = 3 m + 2 s\n")),
        ("E0402", Front("let x = 2^(3 m)\n")),
        ("E0403", Front("let x = sin(3 m)\n")),
        ("E0404", Front("fn f(x: Real[L], n: Int): Real = x ^ n\n")),
        ("E0405", Front("fn f(a: Point[Θ], b: Point[Θ]) = a + b\n")),
        ("E0406", Front("let x = 1 m + 0\n")),
        ("E0407", Front("let x: Real[Q] = 1.0\n")),
        (
            "E0408",
            Unreachable(
                "dimension unification is linear over the free abelian group of exponents: a residual equation with a non-zero coefficient on a variable is always solvable by dividing, so the only failure is a non-zero constant residual — exactly E0401 (`DimUnifier` returns one `DimMismatch`). An 'unsatisfiable multi-variable constraint' state does not exist without changing the aevia-dim API (out of scope for aevia-types)",
            ),
        ),
        (
            "E0409",
            Unreachable(
                "for the same linear-exponent reason the occurs check can never fire: `d ~ d·L` solves to `d ~ L` by dividing both sides, so the unifier never needs a cyclic substitution and no occurs state exists (aevia-dim has no occurs-check path in its API). Needs an aevia-dim change, not a checker change",
            ),
        ),
        (
            "E0410",
            Front("let a: Tensor<2, Real[L]> = [[1.0 m, 2.0 m], [3.0 m, 4.0 m]]\nlet d = det(a)\n"),
        ),
        (
            "E0411",
            Front("fn f(a: Tensor<2, Option<Int>>, b: Tensor<2, Option<Int>>) = a @ b\n"),
        ),
        (
            "E0412",
            OtherCrate(
                "aevia-core/src/eval.rs:1752 (`check_with` dimension comparison); a value-level `DimError`/fatal error, §4.9.3",
            ),
        ),
        // ----- B.7 units ---------------------------------------------------
        ("E0501", Front("let x = 3 zz\n")),
        (
            "E0502",
            Unreachable(
                "`E0502` is 'unit not imported', but units have no module-scoped namespace: `Checker::units` is the whole kernel `UnitTable` plus local `unit` declarations, so a unit from a module the program never imported is indistinguishable from a built-in one and no 'not imported' state exists. Needs module-scoped unit visibility (loader + `units` table per module), not a checker fix",
            ),
        ),
        (
            "E0503",
            Unreachable(
                "`UnitError::NotPrefixable` is never constructed: the prefix resolver skips a non-prefixable base and falls through to `Unknown` (E0501), and there is no state that knows an explicit unit exists but rejects its prefix — that needs the same module-scoped unit namespace as E0502",
            ),
        ),
        ("E0504", Front("unit bad : Length = 1 m offset 5 K\n")),
        (
            "E0505",
            Front("unit foo : Length = 1 m\nunit foo : Length = 2 m\n"),
        ),
        ("E0506", Front("let x = 3 ft^(1/2)\n")),
        (
            "E0507",
            Unreachable(
                "there is no prefix-vs-explicit conflict state: a prefixed spelling that is also an explicit unit resolves to that unit, and a skipped prefix falls through to `E0501`. Distinguishing the two needs the same module-scoped unit namespace as E0502",
            ),
        ),
        ("E0508", Front("let x = 3 °C*m\n")),
        (
            "E0509",
            Front("let d: Float[L] = 3.0 m\nlet c = value_in(s, d)\n"),
        ),
        (
            "E0510",
            Front("let t: Float[Θ] = 10 K\nlet c = value_in(°C, t)\n"),
        ),
        // §5.11.4: a charge has no exact scale in Heaviside-Lorentz units.
        ("E0511", Front("module M with units natural\nlet q = 1 C\n")),
        ("E0512", Front("module M with units imperial\nlet x = 1\n")),
        // ----- B.8 modules -------------------------------------------------
        (
            "E0601",
            OtherCrate(
                "aevia-core/src/driver.rs:345 (the loader compares the declared `module` path with the file's path under the package root; the entry/standalone/embedded modules are exempt; regression in aevia-core/tests/modules.rs)",
            ),
        ),
        (
            "E0602",
            OtherCrate(
                "aevia-core/src/driver.rs:134 (import-cycle detection before the checker runs)",
            ),
        ),
        ("E0603", Imports("fn f(): Int = x\n")),
        (
            "E0604",
            Front("instance Show for Int where\n    show(x) = \"n\"\n"),
        ),
        (
            "E0605",
            OtherCrate(
                "aevia-core/src/manifest.rs:121 (first of many manifest-validation sites, e.g. :625, :797, :1090)",
            ),
        ),
        (
            "E0606",
            OtherCrate("aevia-core/src/manifest.rs:365 (dependency version conflict)"),
        ),
        (
            "E0607",
            OtherCrate(
                "aevia-core/src/driver.rs:146 and manifest.rs:227, :377, :699, :928 (module/package not found)",
            ),
        ),
        (
            "E0608",
            OtherCrate("aevia-core/src/lib.rs:545 (entry-point validation)"),
        ),
        // ----- B.9 effects -------------------------------------------------
        ("E0701", Front("fn f(): IO Unit = do\n    1\n")),
        ("E0702", Front("fn f(): Int = Ok(1)?\n")),
        // ----- B.10 runtime ------------------------------------------------
        (
            "E0801",
            OtherCrate("aevia-core/src/builtins.rs:322 and eval.rs:1107, :1646 (division by zero)"),
        ),
        (
            "E0802",
            Front("fn f(m: Tensor<[2, 3], Float>): Float = m[5]\n"),
        ),
        (
            "E0803",
            OtherCrate(
                "aevia-rssn/src/error.rs:252 (`OpError::FreeSymbols` -> E0803, raised through the bridge)",
            ),
        ),
        (
            "E0804",
            OtherCrate("aevia-core/src/builtins.rs:1579 (undecidable symbolic condition)"),
        ),
        (
            "E0805",
            OtherCrate("aevia-core/src/eval.rs:750 and eval_ir.rs:161 (stack depth / tail budget)"),
        ),
        (
            "E0806",
            OtherCrate(
                "aevia-core/src/builtins.rs:2387 (first domain-error site) and aevia-rssn/src/error.rs:263",
            ),
        ),
        (
            "E0807",
            OtherCrate(
                "aevia-core/src/builtins.rs:6052 and aevia-rssn/src/error.rs:257 (numeric non-convergence)",
            ),
        ),
        (
            "E0808",
            OtherCrate(
                "aevia-core/src/builtins.rs:2751 and aevia-rssn/src/error.rs:266 (budget exhausted)",
            ),
        ),
        (
            "E0809",
            OtherCrate(
                "aevia-core/src/eval.rs:1074, :1349, :2270 and eval_ir.rs:99 (internal invariant violated)",
            ),
        ),
        (
            "E0810",
            OtherCrate("aevia-core/src/eval.rs:2964, :2978, :2983 (IO failures)"),
        ),
        (
            "E0899",
            OtherCrate(
                "aevia-core/src/eval.rs:3007 and eval_ir.rs:428; aevia-rssn/src/error.rs:113 (panic)",
            ),
        ),
        // ----- B.11 IO -----------------------------------------------------
        (
            "E0901",
            OtherCrate(
                "aevia-core/src/eval.rs:3275 and builtins.rs:3466 (file not found / read or write failure; asserted directly by aevia-core/tests/io_taxonomy.rs through read_file and read_csv)",
            ),
        ),
        (
            "E0902",
            OtherCrate(
                "aevia-core/src/eval.rs:3276 and builtins.rs:3408 (permission-denied / invalid-argument OS errors mapped to their own code; io_taxonomy.rs)",
            ),
        ),
        (
            "E0903",
            OtherCrate(
                "aevia-core/src/builtins.rs:3485 and :3511 (malformed CSV/JSON/NPY data; io_taxonomy.rs asserts malformed JSON through read_json)",
            ),
        ),
        (
            "E0904",
            OtherCrate(
                "aevia-core/src/builtins.rs:3431, :3547, :5562 (unsupported format/extensions such as read_csv(\"data.npy\"), read_json(\"data.csv\") and an unknown NPY version; io_taxonomy.rs)",
            ),
        ),
        (
            "E0905",
            OtherCrate(
                "aevia-core/src/eval.rs:3215 and :3220 (plot rendering/save failure; io_taxonomy.rs saves to an unwritable path)",
            ),
        ),
        // ----- B.12 warnings -----------------------------------------------
        ("W0101", Front("fn f(x: Int): Int = 1\n")),
        (
            "W0102",
            Unreachable(
                "the checker only sees `add_imports`' flat name→Export table: `Import` carries no span or `open` flag, so an unused selective import is indistinguishable from a name brought in by `open M` (where warning per name would be a false positive). Needs statement-level import tracking in the loader/driver before a warning can be honest",
            ),
        ),
        (
            "W0103",
            Front("fn f(x: Int): Int =\n    let x = 2\n    x\n"),
        ),
        (
            "W0201",
            Front("fn f(b: Bool): Int = match b with\n    | _ -> 1\n    | true -> 2\n"),
        ),
        (
            "W0202",
            Front("fn f(): IO Unit = do\n    read_line()\n    println(\"x\")\n"),
        ),
        (
            "W0203",
            Unreachable(
                "the language has no `return`/`break`/`loop`: a block is statements followed by an obligatory tail expression (`E0201` when it is missing), so nothing can follow the exit and the AST cannot represent an unreachable statement. `match` arms after a wildcard are the redundant-pattern warning W0201, not W0203",
            ),
        ),
        ("W0301", Front("let x = symbol(\"x\")\n")),
        (
            "W0302",
            Front("let r: Real[L] = 1 m\nlet a: Float[L] = 1.0 m\nlet x = r + a\n"),
        ),
        ("W0303", Front("let x = symbol(\"x\")\n")),
        (
            "W0501",
            OtherCrate(
                "aevia-core: **no emission site yet** (grep of aevia-core/src finds none); the display unit travels in `value.rs`'s `NumVal.display` but nothing reports a dropped display unit (§5.4.2) — the runtime owner must add the diagnostic",
            ),
        ),
        (
            "W0601",
            Front("private fn helper(): Int = 1\nfn main(): Int = 2\n"),
        ),
        // ----- B.17 extension/backend --------------------------------------
        ("E1001", NoJit("@jit\nfn hot(x: Float): Float = x * 2.0\n")),
        ("E1002", Front("fn f(x) = Rssn.sin(x)\n")),
        ("E1003", Front("let a = Rssn.sin(1, 2)\n")),
        (
            "E1004",
            OtherCrate(
                "aevia-rssn/src/lib.rs:55 (api_version) and aevia-core/src/kernel.rs:101 (incompatible API)",
            ),
        ),
        (
            "E1005",
            OtherCrate(
                "aevia-core/src/eval.rs:2317 and rssn_module.rs:83, :479 (pinned `OpKey` missing from the manifest)",
            ),
        ),
        (
            "E1006",
            OtherCrate("aevia-rssn/src/error.rs:280 (plugin ABI/load failure)"),
        ),
        (
            "W0602",
            Front("@deprecated(\"use new_api\")\nfn old(x: Int): Int = x\nlet y = old(1)\n"),
        ),
    ]
}

#[test]
fn every_documented_code_has_exactly_one_disposition() {
    let documented = documented_codes();
    let table = table();
    // No duplicate code in the table.
    let mut seen = std::collections::HashSet::new();
    for (code, _) in &table {
        assert!(seen.insert(*code), "`{code}` appears twice in the table");
    }
    // Every documented code is covered ...
    for code in &documented {
        assert!(
            seen.contains(code),
            "appendix B documents `{code}` but the reachability table does not mention it"
        );
    }
    // ... and the table does not invent a code the appendix does not document
    // (codes the implementation adds are listed in `extension_codes`, so the
    // spec gap stays visible).
    let documented: std::collections::HashSet<&str> = documented.iter().copied().collect();
    let extensions: std::collections::HashSet<&str> = extension_codes().iter().copied().collect();
    for (code, _) in &table {
        assert!(
            documented.contains(code) || extensions.contains(code),
            "`{code}` is in the table but not documented in appendix B"
        );
    }
    // The extension list must itself stay covered.
    for code in extension_codes() {
        assert!(
            seen.contains(code),
            "extension `{code}` is missing from the table"
        );
    }
}

#[test]
fn every_frontend_code_is_actually_produced() {
    let mut failures = Vec::new();
    for (code, cover) in table() {
        let produced = match cover {
            | Cover::Front(source) => codes_of(source),
            | Cover::NoJit(source) => {
                let lexed = lex(0, source).expect("lexes");
                let (module, parsed) = parse_module(0, source, &lexed);
                assert!(!parsed.has_errors());
                let options = aevia_types::check::CheckOptions {
                    capabilities: aevia_types::check::Capabilities::from_names(std::iter::empty::<
                        &str,
                    >(
                    )),
                };
                let checked =
                    aevia_types::check::check_module_with_options(0, &module, &[], &options);
                codes_of_diagnostics(&checked.diagnostics)
            },
            | Cover::Imports(source) => codes_of_ambiguous_imports(source),
            | Cover::OtherCrate(_) | Cover::Unreachable(_) => continue,
        };
        if !produced.iter().any(|found| found == code) {
            failures.push(format!("{code}: produced {produced:?}"));
        }
    }
    assert!(
        failures.is_empty(),
        "these codes are marked reachable but no program produces them:\n{}",
        failures.join("\n")
    );
}

#[test]
fn non_frontend_entries_state_a_reason() {
    for (code, cover) in table() {
        let reason = match cover {
            | Cover::Front(_) | Cover::NoJit(_) | Cover::Imports(_) => continue,
            | Cover::OtherCrate(reason) | Cover::Unreachable(reason) => reason,
        };
        assert!(
            !reason.trim().is_empty(),
            "`{code}` is not reachable from the front end without a stated reason"
        );
        assert!(
            reason.contains("E0") || reason.chars().count() > 20,
            "`{code}`'s reason must be concrete: {reason:?}"
        );
    }
}
