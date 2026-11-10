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

use aevia_rssn::codegen;
use aevia_rssn::kernel;
use aevia_rssn::registry;
use aevia_syntax::SourceMap;
use aevia_syntax::lex;
use aevia_syntax::parse_module;

/// Parses a generated module, returning it or panicking with the first
/// diagnostic.
fn parse(
    name: &str,
    text: &str,
) -> aevia_syntax::Module {
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

/// A pinned literal key types the call from the manifest overload, and only a
/// pinned key does: this is the property that makes the emitted facade
/// checkable without weakening `Rssn.invoke` for non-literal callers.
///
/// The whole-module acceptance test lives in its own test target
/// (`tests/emit_typecheck.rs`): checking a generated module leaves global
/// checker state that a second `check_module` call in the same process sees
/// (the module's opaque `type Value`/`Symbol` registrations), and the
/// acceptance must be a clean-process measurement.
#[test]
fn a_literal_key_types_the_invoke_result() {
    let entry = registry().op("E").expect("`E` is in the manifest");
    let hash: String = entry
        .keys
        .first()
        .expect("`E` pins a key")
        .signature_hash
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect();
    let source =
        format!("module InvokeTyping\n\nlet e: Real = Rssn.invoke(\"E\", \"{hash}\", [])\n");
    let module = parse("InvokeTyping.aev", &source);
    let checked = aevia_types::check_module(0, &module);
    assert!(
        !checked.diagnostics.has_errors(),
        "a pinned literal key must type the result: {}",
        checked.diagnostics.render(&SourceMap::new())
    );

    // The pinned key really is the static claim: the wrong annotation is
    // rejected instead of being accepted as `Value`.
    let wrong = "module InvokeTypingWrong\n\nlet e: Bool = Rssn.invoke(\"E\", \"";
    let source = format!("{wrong}{hash}\", [])\n");
    let module = parse("InvokeTypingWrong.aev", &source);
    let checked = aevia_types::check_module(0, &module);
    assert!(
        checked.diagnostics.has_errors(),
        "`E` is a `Real`, not a `Bool`"
    );

    // An unknown hash is not a static claim: it keeps the dynamic `Value`
    // type, so annotating it `Real` is the ordinary `E0302` and the runtime
    // `E1005` remains the diagnostic of record for the call itself.
    let source = "module InvokeTypingUnknown\n\nlet e: Real = Rssn.invoke(\"E\", \"00000000000000000000000000000000\", [])\n";
    let module = parse("InvokeTypingUnknown.aev", source);
    let checked = aevia_types::check_module(0, &module);
    assert!(
        checked.diagnostics.has_errors(),
        "an unknown pinned hash stays dynamic and cannot satisfy `Real`"
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

    let rows = aevia_rssn::bindings::catalog(registry());
    let typed = rows
        .iter()
        .filter(|row| matches!(row.coverage, aevia_rssn::bindings::Coverage::Typed))
        .count();
    let pinned = text
        .lines()
        .filter(|line| {
            (line.starts_with("fn ") || line.starts_with("let "))
                && line.contains(" = Rssn.invoke(")
        })
        .count();
    let dynamic = text
        .lines()
        .filter(|line| {
            line.starts_with("fn ")
                && (line.contains(" = Rssn.dynamic(") || line.contains(" = Rssn.call_ho("))
        })
        .count();
    assert_eq!(pinned, typed);
    assert_eq!(
        pinned + dynamic,
        rows.len(),
        "each overload or unsigned operator needs one honest binding"
    );
}

/// The overlay set the kernel carries: the original precision levers plus the
/// logic/signal and SDE-noise ones. They are listed by name so a registration
/// that silently lost its overlay (falling back to `Derived`) is caught — the
/// count itself is the manifest's, not a constant typed here.
const OVERLAY_OPERATORS: [&str; 24] = [
    "diff",
    "solve",
    "taylor",
    "quantity",
    "convert",
    "det",
    "trace",
    "norm",
    "transpose",
    "rank",
    "dot",
    "cross",
    "identity",
    "and",
    "or",
    "xor",
    "not",
    "implies",
    "iff",
    "fftconv",
    "interpolate",
    "sum",
    "sde_levy_areas",
    "sde_wiener",
];

/// Every operator in [`OVERLAY_OPERATORS`] really has an overlay signature and
/// a generated declaration that pins a key. A name whose operator fell back to
/// `Derived` (`det : (Term) -> Term`) would still have a declaration, so the
/// source distribution is asserted too.
#[test]
fn the_known_overlay_operators_are_typed() {
    let text = codegen::operators(registry());
    for name in OVERLAY_OPERATORS {
        let op = registry()
            .op(name)
            .unwrap_or_else(|| panic!("the manifest has no operator `{name}`"));
        assert_eq!(
            op.source,
            aevia_rssn::Source::Overlay,
            "`{name}` must carry an overlay signature, not a derived one"
        );
        assert!(
            !op.signatures.is_empty(),
            "`{name}` has an overlay but no signature"
        );
        assert!(
            text.contains(&format!("Rssn.invoke(\"{name}\", "))
                || text.contains(&format!("Rssn.dynamic(\"{name}\", args)"))
                || text.contains(&format!("Rssn.call_ho(\"{name}\", args, callbacks)")),
            "`{name}` has neither an exact binding nor a dynamic route"
        );
    }
}

/// The generated module records the manifest it came from, and the registry's
/// copy of the version, ABI and hash is the live manifest's. This is the
/// assertion that would catch a stale constant: the kernel's numbers move
/// between releases and the bridge must follow them, not remember them.
#[test]
fn the_generated_module_records_the_live_manifest() {
    let manifest = kernel::kernel().manifest();
    let registry = registry();
    assert_eq!(registry.version, manifest.rssn_version, "the rssn version");
    assert_eq!(
        registry.api_version, manifest.api_version,
        "the ABI version"
    );
    let hash: String = manifest
        .manifest_hash
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect();
    assert_eq!(registry.manifest_hash, hash, "the manifest hash");
    assert_eq!(
        aevia_rssn::SUPPORTED_API_VERSION,
        manifest.api_version,
        "this build understands the manifest's ABI"
    );
    let text = codegen::operators(registry);
    assert!(
        text.contains(&format!(
            "## rssn {}, api {}, manifest {}",
            manifest.rssn_version, manifest.api_version, hash
        )),
        "the module must state the manifest it was generated from"
    );
}

/// The distribution of signature sources in the registry is the manifest's:
/// `Declared`/`Overlay`/`Derived`/`None` are counted from `OpInfo`, not from a
/// list kept here, and every `None` operator is documented as reachable through
/// `Rssn.dynamic` instead of silently having no declaration.
#[test]
fn the_source_distribution_is_the_manifests() {
    use rssn::kernel::manifest::SignatureSource;
    let manifest = kernel::kernel().manifest();
    let mut expected = [0usize; 4];
    for op in &manifest.ops {
        let slot = match op.signature_source {
            | SignatureSource::Declared => 0,
            | SignatureSource::Overlay => 1,
            | SignatureSource::Derived => 2,
            | SignatureSource::None => 3,
        };
        expected[slot] += 1;
    }
    let observed = [
        registry()
            .ops()
            .filter(|op| op.source == aevia_rssn::Source::Declared)
            .count(),
        registry()
            .ops()
            .filter(|op| op.source == aevia_rssn::Source::Overlay)
            .count(),
        registry()
            .ops()
            .filter(|op| op.source == aevia_rssn::Source::Derived)
            .count(),
        registry()
            .ops()
            .filter(|op| op.source == aevia_rssn::Source::None)
            .count(),
    ];
    assert_eq!(
        observed, expected,
        "the registry's source distribution is not the manifest's"
    );

    let text = codegen::operators(registry());
    for op in registry()
        .ops()
        .filter(|op| op.source == aevia_rssn::Source::None)
    {
        assert!(
            text.contains(&format!("Rssn.dynamic(\"{}\", args)", op.name))
                || text.contains(&format!("Rssn.call_ho(\"{}\", args, callbacks)", op.name)),
            "`{}` has no signature and must have a dynamic route",
            op.name
        );
    }
}

/// Whether every type in a signature has a source spelling. The generator falls
/// back to `Dyn<term>` for the ones that do not (unknown rank, a type variable,
/// a host type, an unpinned dimension), so exact-signature assertions apply
/// only where the manifest states a writable type.
fn spellable(ty: &aevia_rssn::SigType) -> bool {
    use aevia_rssn::SigType;
    match ty {
        | SigType::Tensor { rank, elem, .. } | SigType::Sparse { rank, elem } => {
            rank.is_some() && spellable(elem)
        },
        | SigType::Unit(dim) | SigType::Point(dim) => dim.is_concrete(),
        | SigType::Dim(inner, dim) => dim.is_concrete() && spellable(inner),
        | SigType::Record(fields) => fields.iter().all(|(_, ty)| spellable(ty)),
        | SigType::Func(params, ret) => params.iter().all(spellable) && spellable(ret),
        | SigType::Variant(_)
        | SigType::Table
        | SigType::Device
        | SigType::Opaque(_)
        | SigType::Var(_)
        | SigType::Never => false,
        | _ => true,
    }
}

/// Every `Overlay` signature is generated *exactly*: the declaration's return
/// type is the manifest's, not the `Dyn<term>` the conservative `Derived`
/// source would produce. The overlay set grew between kernel releases (it is
/// the precision lever, not a fixed list), so the count comes from the live
/// manifest and every overlay operator is checked — not a chosen few.
#[test]
fn every_overlay_signature_is_generated_exactly() {
    use aevia_rssn::registry::render_type;
    let text = codegen::operators(registry());
    let overlay: Vec<_> = registry()
        .ops()
        .filter(|op| op.source == aevia_rssn::Source::Overlay)
        .collect();
    assert!(
        !overlay.is_empty(),
        "the manifest states overlay signatures, so the module must use them"
    );
    let mut checked = 0usize;
    for op in overlay {
        assert!(
            !op.signatures.is_empty(),
            "overlay operator `{}` has no signature to generate",
            op.name
        );
        assert_eq!(
            op.keys.len(),
            op.signatures.len(),
            "overlay operator `{}` must pin one key per overload",
            op.name
        );
        for (index, signature) in op.signatures.iter().enumerate() {
            let row = aevia_rssn::bindings::catalog(registry())
                .into_iter()
                .find(|row| row.op.name == op.name && row.overload == Some(index))
                .expect("catalog row");
            if !matches!(row.coverage, aevia_rssn::bindings::Coverage::Typed)
                || !spellable(&signature.ret)
                || !signature.params.iter().all(spellable)
            {
                continue;
            }
            checked += 1;
            let key = &op.keys[index];
            let hash: String = key
                .signature_hash
                .iter()
                .map(|byte| format!("{byte:02x}"))
                .collect();
            let marker = format!("Rssn.invoke(\"{}\", \"{hash}\", ", op.name);
            let declaration = text
                .lines()
                .find(|line| line.contains(&marker))
                .unwrap_or_else(|| {
                    panic!(
                        "overload {index} of `{}` has no declaration pinning its key",
                        op.name
                    )
                });
            // `fn name(params): Ret = Rssn.invoke(…)` or a nullary
            // `let name: Ret = Rssn.invoke(…)`: the declared type is the last
            // `: …` before the call.
            let (signature_part, _) = declaration
                .rsplit_once(" = Rssn.invoke(")
                .unwrap_or_else(|| panic!("malformed declaration: {declaration}"));
            let (_, declared_return) = signature_part
                .rsplit_once(": ")
                .unwrap_or_else(|| panic!("no declared return type: {declaration}"));
            let expected_return = render_type(&signature.ret);
            assert_eq!(
                declared_return, expected_return,
                "overload {index} of `{}` does not state its overlay return type: {declaration}",
                op.name
            );
        }
    }
    assert!(
        checked > 0,
        "no overlay signature had a source-writable type to assert"
    );
}

/// One `let` in `Units.aev` per manifest unit, or a comment saying the unit's
/// every spelling is a keyword. A unit silently dropped here would not resolve
/// from source even though the kernel knows it.
#[test]
fn the_units_module_covers_every_manifest_unit() {
    let manifest = kernel::kernel().manifest();
    let text = codegen::units(registry());
    let bindings = text.lines().filter(|line| line.starts_with("let ")).count();
    let unwritable = text
        .lines()
        .filter(|line| line.contains("cannot be written in Aevia source"))
        .count();
    assert_eq!(
        bindings + unwritable,
        manifest.units.len(),
        "Units.aev covers {} of {} manifest units",
        bindings + unwritable,
        manifest.units.len()
    );
    // A module cannot define one name twice.
    let mut names: Vec<&str> = text
        .lines()
        .filter_map(|line| line.strip_prefix("let "))
        .filter_map(|line| line.split(':').next())
        .collect();
    let total = names.len();
    names.sort_unstable();
    names.dedup();
    assert_eq!(names.len(), total, "a unit binding is declared twice");
}

/// One binding per manifest constant, and the generated capability flags are
/// the bridge's one capability table — not a second list.
#[test]
fn the_constants_and_capabilities_are_the_manifests() {
    let manifest = kernel::kernel().manifest();
    let registry = registry();
    let text = codegen::constants(registry);
    let bindings = text.lines().filter(|line| line.starts_with("let ")).count();
    assert_eq!(
        bindings,
        manifest.constants.len(),
        "Constants.aev does not bind every manifest constant"
    );

    let operators = codegen::operators(registry);
    for row in registry.capabilities.table() {
        assert!(
            operators.contains(&format!("let {}: Bool = {}", row.binding, row.enabled)),
            "the generated module does not state capability `{}` as {}",
            row.name,
            row.enabled
        );
    }
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
    let expected: std::collections::BTreeSet<String> = aevia_rssn::bindings::catalog(registry())
        .into_iter()
        .filter(|row| matches!(row.coverage, aevia_rssn::bindings::Coverage::Typed))
        .filter_map(|row| row.overload.and_then(|index| row.op.key(index)))
        .map(|key| {
            key.signature_hash
                .iter()
                .map(|byte| format!("{byte:02x}"))
                .collect()
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
        if let aevia_syntax::DeclKind::Let { pattern, .. } = &decl.kind
            && let aevia_syntax::PatKind::Var(name) = &pattern.kind
        {
            names.push(name.clone());
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

    let hbar = registry()
        .constant("hbar")
        .expect("hbar is in the manifest");
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

/// A dimension parameter in an overlay signature is **spelled**, not hidden
/// behind `Dyn<term>`: the manifest's `Real[T]` / `T^(1/2)` reaches the emitted
/// source. This closes the codegen limitation documented in §10.0/§13.2.2.
#[test]
fn dimension_parameters_are_spelled_not_dyn() {
    let text = codegen::operators(registry());
    let first_line = |prefix: &str| -> String {
        text.lines()
            .find(|line| line.starts_with(prefix))
            .unwrap_or_else(|| panic!("no declaration starts with `{prefix}`"))
            .to_string()
    };
    let wiener = first_line("fn sde_wiener<");
    assert!(
        wiener.starts_with("fn sde_wiener<d>("),
        "dimension must have an explicit binder: {wiener}"
    );
    assert!(
        wiener.contains("a3: Real[d]") && wiener.contains("Tensor<2, Real[d^(1/2)]>"),
        "`sde_wiener` must keep its dimension parameter: {wiener}"
    );
    assert!(
        !wiener.contains("Dyn<term>"),
        "a dimension-parametric signature must not fall back to `Dyn`: {wiener}"
    );
    let areas = first_line("fn sde_levy_areas<");
    assert!(
        areas.contains("a1: Real[d]") && areas.contains("Tensor<1, Real[d]>"),
        "`sde_levy_areas` must keep its dimension parameter: {areas}"
    );
    assert!(!areas.contains("Dyn<term>"), "{areas}");
    // `quantity` and `convert` have genuinely dynamic value representations,
    // so the strict facade offers the explicit Result-returning dynamic route.
    for name in ["fn quantity(", "fn convert("] {
        let line = first_line(name);
        assert!(
            line.contains("Rssn.dynamic(") && line.contains("Result<Value, OpError>"),
            "{line}"
        );
    }
    // Count them live rather than pinning: the kernel may add dimension-
    // parametric operators, and that is exactly what this test should accept.
    let dimensioned = text
        .lines()
        .filter(|line| {
            line.contains(" = Rssn.invoke(")
                && ["Real[d", "Unit[d", "Point[d", "Float[d"]
                    .iter()
                    .any(|needle| line.contains(needle))
        })
        .count();
    assert!(
        dimensioned >= 2,
        "only {dimensioned} declaration(s) carry a dimension variable"
    );
}

/// The `Dyn<term>` fallback becomes *documented and narrow*: it stays only
/// where Aevia source cannot state the type (unknown tensor rank, table row,
/// host type, type parameter, or a kernel signature whose own type is `Dyn`).
#[test]
fn genuinely_open_types_still_say_dyn() {
    use aevia_dim::Dim;
    use aevia_dim::ratio;
    use aevia_rssn::SigType;

    let var = Dim::var(1);
    assert_eq!(
        codegen::declaration_type(&SigType::Dim(Box::new(SigType::Real), var.clone())),
        "Real[d]"
    );
    let half = ratio(1, 2);
    assert_eq!(
        codegen::declaration_type(&SigType::Dim(
            Box::new(SigType::Real),
            var.clone().pow(&half)
        )),
        "Real[d^(1/2)]"
    );
    assert_eq!(
        codegen::declaration_type(&SigType::Tensor {
            rank: Some(2),
            shape: None,
            elem: Box::new(SigType::Dim(Box::new(SigType::Real), var.pow(&half))),
        }),
        "Tensor<2, Real[d^(1/2)]>"
    );
    // Genuinely unrepresentable in a declaration.
    assert_eq!(
        codegen::declaration_type(&SigType::Tensor {
            rank: None,
            shape: None,
            elem: Box::new(SigType::Real),
        }),
        "Dyn<term>",
        "an unknown tensor rank has no declaration spelling"
    );
    assert_eq!(codegen::declaration_type(&SigType::Table), "Dyn<term>");
    assert_eq!(
        codegen::declaration_type(&SigType::Opaque("HostType".to_string())),
        "Dyn<term>"
    );
    assert_eq!(
        codegen::declaration_type(&SigType::Var("a".to_string())),
        "Dyn<term>"
    );
    // A manifest signature that itself says `Dyn` keeps saying it: the kernel
    // deliberately leaves the representation unknown (`quantity`'s value arg).
    let quantity = registry().op("quantity").expect("quantity is registered");
    assert_eq!(
        codegen::declaration_type(&quantity.signatures[0].params[0]),
        "Dyn<term>",
        "the manifest states `Dyn<term, any>` here; a declaration cannot be more precise than the kernel"
    );
}

/// The precise spelling is not just text: a declaration using `Real[T]`,
/// `Real[T^(1/2)]` and a dimensioned tensor parses and type-checks. This is the
/// smallest independent proof that `--emit`'s new spelling is legal Aevia.
#[test]
fn a_dimension_parametric_declaration_type_checks() {
    let source = "module DimSpelling\n\n\
        fn f<d>(a: Real[d]): Real[d^(1/2)] = sqrt(a)\n\
        \n\
        fn g<d>(a: Tensor<2, Real[d]>): Tensor<2, Real[d]> = a\n\
        fn product<d, e>(a: Float[d], b: Float[e]): Float[d*e] = a * b\n";
    let module = parse("DimSpelling.aev", source);
    let checked = aevia_types::check_module(0, &module);
    assert!(
        !checked.diagnostics.has_errors(),
        "the dimension-parametric spelling does not type-check: {}",
        checked.diagnostics.render(&SourceMap::new())
    );
}
