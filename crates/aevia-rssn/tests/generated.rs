//! The generated `Rssn`/`Units`/`Constants` modules, checked as programs.
//!
//! The old conformance test compared the generator against its own
//! arithmetic, so it could not notice a file that does not compile, an
//! operator with no pinned key, or the duplicate `let m_u`. These tests feed
//! the emitted source back through the real parser and the real checker, which
//! is the only claim about generated code worth making.
//!
//! `aevia-types` depends on this crate, so the dev-dependency on it is a
//! cycle. Cargo allows it for dev-dependencies, and the library build never
//! sees it.

use aevia_rssn::{codegen, kernel, registry};
use aevia_syntax::{Diagnostic, SourceMap, lex, parse_module};

/// Parses a generated module, returning it or panicking with the first
/// diagnostic.
fn parse(name: &str, text: &str) -> aevia_syntax::Module {
    let lexed = lex(0, text).unwrap_or_else(|errors| {
        panic!("{name} does not lex: {:?}", errors.items[0]);
    });
    let (module, diagnostics) = parse_module(0, text, &lexed);
    if diagnostics.has_errors() {
        panic!(
            "{name} does not parse: {}",
            diagnostics.render(&SourceMap::new())
        );
    }
    module
}

/// Every generated module, parsed.
#[test]
fn the_generated_modules_parse() {
    let files = codegen::files(registry());
    assert_eq!(files.len(), 3, "one file per module");
    for (name, text) in &files {
        parse(name, text);
    }
}

/// The one diagnostic the checker cannot avoid today.
///
/// `Rssn.invoke(name, hash, values)` is typed as returning the dynamic value
/// type, so a generated `fn` that declares `: Real` cannot derive that type
/// from its body. That is a checker decision (`aevia-types/src/check.rs`,
/// `invoke` in `rssn_member` and the `is_rssn_member(callee, "invoke")` arm),
/// not something the generator can fix: the pinned key *is* the static claim,
/// and the primitive that carries it has no typed form yet. Recorded as a
/// requirement in `.workbuddy-ai/audit/bridge-fixed.md`; until it lands, this
/// is the only error a generated module is allowed to have.
fn is_the_invoke_typing_gap(diagnostic: &Diagnostic) -> bool {
    diagnostic.code == "E0302" && diagnostic.label.contains("found Value")
}

/// Every generated module, type-checked.
///
/// `Units.aev` and `Constants.aev` must be clean — that is where the duplicate
/// `m_u` and the folded `hbar` lived. `Rssn.aev` must have no error other than
/// the `Rssn.invoke` typing gap above: a duplicate `fn`, an unknown unit, a
/// dimension the language cannot write, or a malformed pinned hash would all
/// show up as something else.
#[test]
fn the_generated_modules_type_check() {
    let mut unexplained: Vec<String> = Vec::new();
    let mut gap = 0usize;
    for (name, text) in codegen::files(registry()) {
        let module = parse(&name, &text);
        let checked = aevia_types::check_module(0, &module);
        for diagnostic in &checked.diagnostics.items {
            if is_the_invoke_typing_gap(diagnostic) {
                gap += 1;
                continue;
            }
            unexplained.push(format!("{name}: {diagnostic:?}"));
        }
        if name != "Rssn.aev" && checked.diagnostics.has_errors() {
            panic!(
                "{name} does not type-check: {}",
                checked.diagnostics.render(&SourceMap::new())
            );
        }
    }
    assert!(
        unexplained.is_empty(),
        "{} generated diagnostic(s) that are not the known `Rssn.invoke` typing gap:\n{}",
        unexplained.len(),
        unexplained.join("\n")
    );
    // The gap is per declaration with a declared return type; a cascade would
    // mean something else is wrong.
    let declarations = codegen::operators(registry())
        .matches(" = Rssn.invoke(")
        .count();
    assert!(
        gap <= declarations,
        "{gap} diagnostics for {declarations} declarations: the checker is cascading"
    );
}

/// The module states how many operators it declares, and that number is the
/// manifest's — not the size of the alias-inclusive lookup map.
#[test]
fn the_operator_count_is_the_manifest_count() {
    let manifest = kernel::kernel().manifest();
    assert_eq!(
        registry().op_count(),
        manifest.ops.len(),
        "the registry must count operators, not spellings"
    );
    let text = codegen::operators(registry());
    let declared: usize = text
        .lines()
        .find_map(|line| line.strip_prefix("## operators: "))
        .and_then(|count| count.trim().parse().ok())
        .expect("the generated module states its operator count");
    assert_eq!(declared, manifest.ops.len());

    // One declaration per overload, counted against the manifest's overloads
    // rather than against the generator's own sum.
    let overloads: usize = manifest.ops.iter().map(|op| op.overloads.len()).sum();
    assert_eq!(
        text.matches("Rssn.invoke(").count(),
        overloads,
        "every overload needs exactly one declaration"
    );
}

/// Every quoted hash in the module is a key the manifest actually states, and
/// every key appears: a declaration that pins a key the kernel does not know
/// would be dead on arrival (`E1005`).
#[test]
fn every_declaration_pins_a_key_of_its_operator() {
    let text = codegen::operators(registry());
    let pinned: std::collections::BTreeSet<&str> = text
        .split('"')
        .skip(1)
        .step_by(2)
        .filter(|part| part.len() == 32 && part.chars().all(|ch| ch.is_ascii_hexdigit()))
        .collect();
    assert!(!pinned.is_empty(), "no declaration pins a key");
    assert!(
        !pinned.contains(""),
        "the empty hash names no overload, so nothing can pin it"
    );
    let expected: std::collections::BTreeSet<String> = registry()
        .ops()
        .flat_map(|op| op.keys.iter())
        .map(|key| {
            key.signature_hash
                .iter()
                .map(|byte| format!("{byte:02x}"))
                .collect::<String>()
        })
        .collect();
    let missing: Vec<&String> = expected
        .iter()
        .filter(|hash| !pinned.contains(hash.as_str()))
        .collect();
    assert!(
        missing.is_empty(),
        "{} keys are never pinned, e.g. {:?}",
        missing.len(),
        &missing[..missing.len().min(3)]
    );
}

/// `M_u` (molar mass) and `m_u` (atomic mass) both spell `m_u`; a module
/// cannot define one name twice, and both constants have to survive.
#[test]
fn the_constants_module_declares_each_name_once() {
    let text = codegen::constants(registry());
    let module = parse("Constants.aev", &text);
    let mut names: Vec<String> = Vec::new();
    for decl in &module.decls {
        if let aevia_syntax::DeclKind::Let { pattern, .. } = &decl.kind {
            if let aevia_syntax::PatKind::Var(name) = &pattern.kind {
                names.push(name.clone());
            }
        }
    }
    let mut sorted = names.clone();
    sorted.sort();
    let duplicates: Vec<&String> = sorted
        .windows(2)
        .filter(|pair| pair[0] == pair[1])
        .map(|pair| &pair[0])
        .collect();
    assert!(
        duplicates.is_empty(),
        "a module cannot define one name twice: {duplicates:?}"
    );
    for expected in ["m_u", "m_u_2"] {
        assert!(
            names.iter().any(|name| name == expected),
            "`{expected}` is missing; bindings are {names:?}"
        );
    }
}

/// `hbar` is `h/(2*pi)`, which no decimal equals. The module writes the exact
/// rational and the power of `pi` apart; multiplying them out produced 47
/// digits with about 17 meaningful ones.
#[test]
fn hbar_is_written_exactly() {
    let text = codegen::constants(registry());
    let line = text
        .lines()
        .find(|line| line.starts_with("let hbar"))
        .expect("the module binds hbar");
    let (_, expression) = line.split_once(" = ").expect("a value");
    let coefficient = expression
        .trim_start_matches('(')
        .split_once(' ')
        .map_or(expression, |(coefficient, _)| coefficient);
    let exact = aevia_dim::parse_decimal(coefficient)
        .or_else(|| {
            let (numer, denom) = coefficient.split_once(")/(")?;
            let numer: num_bigint::BigInt = numer.parse().ok()?;
            let denom: num_bigint::BigInt = denom.trim_end_matches(')').parse().ok()?;
            Some(num_rational::BigRational::new(numer, denom))
        })
        .unwrap_or_else(|| panic!("`{coefficient}` is not an exact rational"));

    let hbar = registry().constant("hbar").expect("hbar is in the manifest");
    assert_eq!(
        exact, hbar.value.rat,
        "the coefficient in the file is not the manifest's exact value"
    );
    assert!(
        expression.contains("pi"),
        "the factor of pi must stay symbolic, not be folded into the number: {expression}"
    );
    // A folded 47-digit expansion is what this replaced; no run of significant
    // digits that long can come out of the exact rational.
    let longest = expression
        .split(|ch: char| !ch.is_ascii_digit())
        .map(|run| run.trim_start_matches('0').len())
        .max()
        .unwrap_or(0);
    assert!(
        longest <= 20,
        "a {longest}-digit expansion means the value was folded through f64"
    );
}
