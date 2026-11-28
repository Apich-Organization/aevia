//! Read-only help built from accepted checked source, never from a speculative draft.
use aevia_core::driver::LoadedModule;
use aevia_core::driver::ReplCompilation;
use aevia_syntax::Decl;
use aevia_syntax::DeclKind;
use aevia_syntax::ExprKind;
use aevia_syntax::Pat;
use aevia_syntax::PatKind;
use aevia_syntax::Span;
use aevia_types::CheckedModule;
use aevia_types::Export;
use std::borrow::Cow;
use std::collections::BTreeMap;
use std::collections::BTreeSet;
use std::fmt::Write as _;
use std::sync::OnceLock;

#[derive(Clone)]
struct Item {
    kind: &'static str,
    signature: String,
    ty: Option<String>,
    origin: String,
    doc: Option<String>,
    alias: Option<String>,
    builtin: Option<String>,
    exported: bool,
}

/// Compact metadata for the last accepted declaration submission. It retains
/// no compiler graph or interpreter and does not reopen source files.
#[derive(Default)]
pub struct Catalog {
    items: BTreeMap<String, Item>,
    units: BTreeMap<String, aevia_dim::UnitDef>,
    dimensions: BTreeMap<String, String>,
}

fn builtin_units() -> &'static aevia_dim::UnitTable {
    static UNITS: OnceLock<aevia_dim::UnitTable> = OnceLock::new();
    UNITS.get_or_init(aevia_rssn::unit_table)
}

impl Catalog {
    #[allow(
        clippy::single_call_fn,
        clippy::too_many_lines,
        reason = "assembles a REPL documentation index with source and builtin metadata"
    )]
    pub(crate) fn with_origins(
        compilation: &ReplCompilation,
        location: &dyn Fn(Span) -> String,
    ) -> Self {
        let Some(entry) = compilation.program.entry() else {
            return Self::default();
        };
        let mut catalog = Self::default();
        let sources = &compilation.program.sources;
        // Only direct entry edges establish visible names. A dependency's
        // private helpers and transitive aliases do not enter the REPL scope.
        let mut modules: BTreeMap<_, BTreeMap<String, Item>> = BTreeMap::new();
        for module in &compilation.program.modules {
            let mut imports = BTreeMap::new();
            for edge in &module.imports {
                if let Some(target) = modules.get(&edge.file) {
                    for (name, item) in target.iter().filter(|(_, item)| item.exported) {
                        if edge
                            .names
                            .as_ref()
                            .is_some_and(|names| !names.contains(name))
                        {
                            continue;
                        }
                        let key = if edge.prefix.is_empty() {
                            name.clone()
                        } else {
                            format!("{}.{name}", edge.prefix)
                        };
                        let mut imported = item.clone();
                        imported.exported = false;
                        imports.insert(key, imported);
                    }
                }
            }
            modules.insert(module.file, local_items(module, sources, imports, location));
        }
        for edge in &entry.imports {
            let Some(module) = compilation
                .program
                .modules
                .iter()
                .find(|module| module.file == edge.file)
            else {
                continue;
            };
            let Some(entries) = modules.get(&module.file) else {
                continue;
            };
            let mut visible = Vec::new();
            for (name, item) in entries.iter().filter(|(_, item)| item.exported) {
                if edge
                    .names
                    .as_ref()
                    .is_some_and(|names| !names.contains(name))
                {
                    continue;
                }
                let qualified = if edge.prefix.is_empty() {
                    name.clone()
                } else {
                    format!("{}.{name}", edge.prefix)
                };
                visible.push(qualified.clone());
                catalog.items.insert(qualified, item.clone());
            }
            let module_name = if edge.prefix.is_empty() {
                module.path.join(".")
            } else {
                edge.prefix.clone()
            };
            if !module_name.is_empty() {
                let mut doc =
                    leading_module_doc(sources.file(module.file).map_or("", |file| &file.text))
                        .unwrap_or_default();
                if !doc.is_empty() {
                    doc.push_str("\n\n");
                }
                let _ = writeln!(doc, "Visible bindings ({}):", visible.len());
                for name in visible.iter().take(40) {
                    let _ = writeln!(doc, "  {name}");
                }
                if visible.len() > 40 {
                    let _ = writeln!(doc, "  … use :apropos {module_name} to search");
                }
                catalog.items.insert(
                    module_name.clone(),
                    Item {
                        kind: "module",
                        signature: format!("module {}", module.path.join(".")),
                        ty: None,
                        origin: module.source.path.display().to_string(),
                        doc: Some(doc),
                        alias: None,
                        builtin: None,
                        exported: false,
                    },
                );
            }
        }
        if let Some(accepted) = modules.remove(&entry.file) {
            catalog.items.extend(accepted);
        }
        for unit in entry.checked.units.list() {
            catalog.units.insert(unit.name.clone(), unit.clone());
            for alias in &unit.aliases {
                catalog.units.insert(alias.clone(), unit.clone());
            }
        }
        for name in entry.checked.dims.base_names() {
            if let Some(dim) = entry.checked.dims.lookup(name) {
                catalog
                    .dimensions
                    .insert(name.clone(), dim.to_plain_string());
            }
        }
        for (name, dim) in entry.checked.dims.aliases() {
            catalog
                .dimensions
                .insert(name.clone(), dim.to_plain_string());
        }
        catalog
    }

    pub(crate) fn render(
        &self,
        requested: &str,
    ) -> Result<String, String> {
        let name = requested.trim().trim_matches('`');
        if name.is_empty() {
            return Err("usage: :doc ITEM (for example :doc println, :doc sqrt, or :doc M.function); use :apropos TEXT to search".into());
        }
        if let Some(text) = operator_help(name) {
            return Ok(text);
        }
        if !name
            .chars()
            .all(|c| c.is_alphanumeric() || matches!(c, '_' | '.' | '°' | '%' | 'µ' | 'μ'))
        {
            return Err("item help expects a name, such as :doc println or :doc M.function; use :type EXPR to inspect an expression".into());
        }
        if let Some(item) = self.items.get(name) {
            return Ok(render_item(name, item));
        }
        if let Some(item) = builtin_item(name) {
            return Ok(render_item(name, &item));
        }
        if let Some(item) = type_item(name) {
            return Ok(render_item(name, &item));
        }
        if let Some(unit) = self
            .units
            .get(name)
            .cloned()
            .or_else(|| builtin_units().lookup(name))
        {
            return Ok(unit_help(name, &unit));
        }
        if let Some(dim) = self.dimensions.get(name).cloned().or_else(|| {
            static DIMENSIONS: OnceLock<aevia_dim::DimEnv> = OnceLock::new();
            DIMENSIONS
                .get_or_init(aevia_dim::DimEnv::si)
                .lookup(name)
                .map(|dim| dim.to_plain_string())
        }) {
            return Ok(format!(
                "{name} — dimension\nDimension: {dim}\n\nDimensions appear in scalar types, for example Real[L/T].\nUse :dims for base dimensions and :units {name} for matching units."
            ));
        }
        if let Some(text) = kernel_help(name) {
            return Ok(text);
        }
        let lower = name.to_lowercase();
        let limit = (name.chars().count() / 3).clamp(1, 3);
        let mut suggestions: Vec<_> = self
            .names()
            .into_iter()
            .filter_map(|candidate| {
                let distance = edit_distance(&lower, &candidate.to_lowercase());
                (distance <= limit).then_some((distance, candidate))
            })
            .collect();
        suggestions.sort_unstable();
        let hint = suggestions
            .iter()
            .take(5)
            .map(|&(_, name)| name)
            .collect::<Vec<_>>()
            .join(", ");
        Err(if hint.is_empty() {
            format!(
                "no accepted item named `{name}`; use :apropos {name} to search, or import its module first"
            )
        } else {
            format!(
                "no accepted item named `{name}`; did you mean {hint}? Use :apropos {name} to search"
            )
        })
    }

    pub(crate) fn search(
        &self,
        query: &str,
    ) -> Result<String, String> {
        const SHOWN: usize = 30;
        let words: Vec<_> = query.split_whitespace().map(str::to_lowercase).collect();
        if words.is_empty() {
            return Err(
                "usage: :apropos TEXT (for example :apropos print or :apropos matrix)".into(),
            );
        }
        let mut count = 0_usize;
        let mut names = Vec::new();
        let mut descriptions = Vec::new();
        for name in self.names() {
            let lower_name = name.to_lowercase();
            let mut missing = words
                .iter()
                .filter(|word| !lower_name.contains(word.as_str()));
            let group = if let Some(first) = missing.next() {
                // Query words cannot cross the space separating a name from
                // its description. Inspect descriptions only for missing words.
                let description = self
                    .items
                    .get(name)
                    .and_then(|item| item.doc.as_deref())
                    .or_else(|| builtin_guide(name).map(|(description, _)| description))
                    .or_else(|| type_help(name).map(|(_, description)| description))
                    .unwrap_or("")
                    .to_lowercase();
                if !description.contains(first.as_str())
                    || !missing.all(|word| description.contains(word.as_str()))
                {
                    continue;
                }
                &mut descriptions
            } else {
                &mut names
            };
            // The name set already visits both result groups in name order.
            count = count.saturating_add(1);
            if group.len() < SHOWN {
                group.push(name);
            }
        }
        if count == 0 {
            return Err(format!(
                "no item help matches `{}`; try a shorter name or keyword",
                query.trim()
            ));
        }
        let mut out = format!("Item help matching `{}` ({count}):\n", query.trim());
        for name in names.into_iter().chain(descriptions).take(SHOWN) {
            let summary = self
                .items
                .get(name)
                .map(|item| Cow::Borrowed(item.ty.as_deref().unwrap_or(&item.signature)))
                .or_else(|| builtin_item(name).map(|item| Cow::Owned(item.signature)))
                .or_else(|| type_help(name).map(|(signature, _)| Cow::Borrowed(signature)))
                .unwrap_or(Cow::Borrowed("unit or dimension"));
            let _ = writeln!(out, "  {name} : {}", summary.replace('\n', " "));
        }
        if count > SHOWN {
            let _ = writeln!(out, "  … narrow the query to see more");
        }
        out.push_str("Use :doc NAME to read an item.");
        Ok(out)
    }

    fn names(&self) -> BTreeSet<&str> {
        self.items
            .keys()
            .map(String::as_str)
            .chain(aevia_types::builtins::PRELUDE_NAMES.iter().copied())
            .chain(TYPE_NAMES.iter().copied())
            .chain(self.units.keys().map(String::as_str))
            .chain(self.dimensions.keys().map(String::as_str))
            .chain(builtin_units().list().iter().map(|unit| unit.name.as_str()))
            .collect()
    }
}

#[allow(
    clippy::single_call_fn,
    reason = "builds documentation records from local declarations"
)]
fn local_items(
    module: &LoadedModule,
    sources: &aevia_syntax::SourceMap,
    mut items: BTreeMap<String, Item>,
    location: &dyn Fn(Span) -> String,
) -> BTreeMap<String, Item> {
    for (index, declaration) in module.module.decls.iter().enumerate() {
        for (name, span) in declaration_names(declaration) {
            let kind = match &declaration.kind {
                | DeclKind::Fn { .. } => "function",
                | DeclKind::Let { .. } => "value",
                | DeclKind::Type { name: type_name, .. } if &name != type_name => "constructor",
                | DeclKind::Type { .. } => "type",
                | DeclKind::Dimension { .. } => "dimension",
                | DeclKind::Unit { .. } => "unit",
                | DeclKind::Class { name: class, .. } if &name != class => "class method",
                | DeclKind::Class { .. } => "class",
                | _ => continue,
            };
            let ty = declaration_type(&module.checked, declaration, index, &name, span);
            let signature = if matches!(declaration.kind, DeclKind::Let { .. }) {
                format!("let {name}")
            } else {
                aevia_doc::module_doc(sources.text(declaration.span), "<accepted>", "Repl")
                    .ok()
                    .and_then(|doc| doc.decls.into_iter().next())
                    .map_or_else(
                        || {
                            aevia_fmt::print::describe_decl(declaration)
                                .trim_matches('`')
                                .to_string()
                        },
                        |doc| doc.signature,
                    )
            };
            let mut signature = signature;
            let declared_unit = match module.checked.exports.get(&name) {
                | Some(Export::Unit { def }) => Some(def.clone()),
                | _ if matches!(declaration.kind, DeclKind::Unit { .. }) => {
                    module.checked.units.lookup(&name)
                },
                | _ => None,
            };
            if let Some(unit) = declared_unit {
                let _ = write!(
                    signature,
                    "\nDimension: {}\nExact coherent scale: {}",
                    unit.dim.to_plain_string(),
                    unit.scale
                );
                if let Some(offset) = unit.offset {
                    let _ = write!(signature, "\nAffine offset: {offset}");
                }
            }
            let mut item = Item {
                kind,
                signature,
                ty,
                origin: location(declaration.span),
                doc: declaration.doc.clone(),
                alias: None,
                builtin: None,
                exported: !declaration.private
                    && (module.checked.exports.contains_key(&name)
                        || module.checked.classes.keys().any(|key| key.name == name)),
            };
            if let DeclKind::Let { value, .. } = &declaration.kind {
                let target = referenced_name(value);
                if let Some(target) = target {
                    if let Some(original) = items.get(&target) {
                        if item.doc.is_none() {
                            item.doc.clone_from(&original.doc);
                        }
                        item.builtin.clone_from(&original.builtin);
                        item.alias = Some(format!("{target} ({})", original.origin));
                    } else {
                        let another_source_binding =
                            module
                                .module
                                .decls
                                .iter()
                                .enumerate()
                                .any(|(other_index, decl)| {
                                    other_index != index
                                        && declaration_names(decl)
                                            .iter()
                                            .any(|(name, _)| name == &target)
                                });
                        if !another_source_binding
                            && aevia_types::builtins::schemes(&target).is_some()
                        {
                            item.builtin = Some(target.clone());
                        }
                        item.alias = Some(target);
                    }
                }
            }
            items.insert(name, item);
        }
    }
    items
}

fn referenced_name(expression: &aevia_syntax::Expr) -> Option<String> {
    match &expression.kind {
        | ExprKind::Var(name) => Some(name.clone()),
        | ExprKind::QName(path) => Some(path.join(".")),
        | ExprKind::Field { base, name } => {
            referenced_name(base).map(|prefix| format!("{prefix}.{name}"))
        },
        | ExprKind::Annot { expr, .. } => referenced_name(expr),
        | _ => None,
    }
}

#[allow(
    clippy::single_call_fn,
    reason = "formats declaration types in the interactive documentation index"
)]
fn declaration_type(
    checked: &CheckedModule,
    declaration: &Decl,
    index: usize,
    name: &str,
    span: Span,
) -> Option<String> {
    if let Some(function) = checked
        .functions
        .get(&declaration.span)
        .filter(|_| matches!(declaration.kind, DeclKind::Fn { .. }))
    {
        return Some(function.scheme.to_string());
    }
    if let Some(export) = checked.exports.get(name) {
        match export {
            | Export::Value { scheme, decl, .. } if *decl == index => {
                return Some(scheme.to_string());
            },
            | Export::Ctor { scheme, .. } | Export::TypeCtor { scheme, .. } => {
                return Some(scheme.to_string());
            },
            | Export::Unit { def } => return Some(format!("Unit[{}]", def.dim)),
            | Export::Dimension { dim, .. } => return Some(dim.to_plain_string()),
            | _ => {},
        }
    }
    if let DeclKind::Let { pattern, .. } = &declaration.kind
        && matches!(pattern.kind, PatKind::Record(_))
        && span == pattern.span
    {
        return None;
    }
    checked.expr_types.get(&span).map(ToString::to_string)
}

fn declaration_names(declaration: &Decl) -> Vec<(String, Span)> {
    match &declaration.kind {
        | DeclKind::Fn { name, .. }
        | DeclKind::Dimension { name, .. }
        | DeclKind::Unit { name, .. } => vec![(name.clone(), declaration.span)],
        | DeclKind::Let { pattern, .. } => pattern_names(pattern),
        | DeclKind::Type { name, body, .. } => {
            let mut names = vec![(name.clone(), declaration.span)];
            if let aevia_syntax::ast::TypeBody::Variants(constructors) = body {
                names.extend(
                    constructors
                        .iter()
                        .filter(|ctor| &ctor.name != name)
                        .map(|ctor| (ctor.name.clone(), ctor.span)),
                );
            }
            names
        },
        | DeclKind::Class { name, methods, .. } => {
            std::iter::once((name.clone(), declaration.span))
                .chain(
                    methods
                        .iter()
                        .map(|method| (method.name.clone(), method.ty.span)),
                )
                .collect()
        },
        | _ => Vec::new(),
    }
}

fn pattern_names(pattern: &Pat) -> Vec<(String, Span)> {
    match &pattern.kind {
        | PatKind::Var(name) => vec![(name.clone(), pattern.span)],
        | PatKind::Tuple(items) | PatKind::Ctor { args: items, .. } => {
            items.iter().flat_map(pattern_names).collect()
        },
        | PatKind::Record(fields) => {
            fields
                .iter()
                .flat_map(|(name, field)| {
                    field
                        .as_ref()
                        .map_or_else(|| vec![(name.clone(), pattern.span)], pattern_names)
                })
                .collect()
        },
        | PatKind::Tensor { items, rest } => {
            items
                .iter()
                .flat_map(pattern_names)
                .chain(rest.iter().map(|name| (name.clone(), pattern.span)))
                .collect()
        },
        | PatKind::As { pattern: inner, name } => {
            pattern_names(inner)
                .into_iter()
                .chain(std::iter::once((name.clone(), pattern.span)))
                .collect()
        },
        | PatKind::Annot { pattern, .. } => pattern_names(pattern),
        | _ => Vec::new(),
    }
}

fn render_item(
    name: &str,
    item: &Item,
) -> String {
    let mut out = format!("{name} — {}\n{}\n", item.kind, item.signature);
    if let Some(ty) = &item.ty {
        let _ = writeln!(out, "Type: {ty}");
    }
    let _ = writeln!(out, "Defined at: {}", item.origin);
    if let Some(alias) = &item.alias {
        let _ = writeln!(out, "Alias of: {alias}");
    }
    if let Some(doc) = item.doc.as_deref().filter(|doc| !doc.trim().is_empty()) {
        let _ = write!(out, "\n{}\n", doc.trim());
    }
    if let Some(builtin) = item.builtin.as_deref() {
        if let Some((description, example)) = builtin_guide(builtin) {
            let _ = write!(
                out,
                "\n{description}\n\nExample:\n  {}\n",
                example.replace('\n', "\n  ")
            );
        } else if item.doc.is_none() {
            out.push_str("\nNo additional prose is bundled for this built-in. The signatures above come from the checker; use :type NAME or :apropos KEYWORD to explore related operations.\n");
        }
    } else if item.doc.as_deref().is_none_or(|doc| doc.trim().is_empty()) && item.kind != "module" {
        out.push_str("\nNo source documentation was attached to this declaration.\n");
    }
    out.trim_end().to_string()
}

#[allow(
    clippy::single_call_fn,
    reason = "recovers module-level docs for the interactive index"
)]
fn leading_module_doc(text: &str) -> Option<String> {
    let mut lines = Vec::new();
    for line in text.lines() {
        if let Some(doc) = line.trim_start().strip_prefix("##") {
            lines.push(doc.strip_prefix(' ').unwrap_or(doc));
        } else if line.trim().starts_with("module ") {
            return (!lines.is_empty()).then(|| lines.join("\n"));
        } else if !line.trim().is_empty() {
            break;
        }
    }
    None
}

fn builtin_item(name: &str) -> Option<Item> {
    let schemes = aevia_types::builtins::schemes(name)?;
    let mut signatures = Vec::new();
    for scheme in schemes {
        let text = scheme.to_string();
        if !signatures.contains(&text) {
            signatures.push(text);
        }
    }
    let mut signature = signatures
        .iter()
        .take(8)
        .map(|ty| format!("{name}: {ty}"))
        .collect::<Vec<_>>()
        .join("\n");
    if signatures.len() > 8 {
        let _ = write!(
            signature,
            "\n… {} additional rank/shape overloads; the checker selects one at each call",
            signatures.len().saturating_sub(8)
        );
    }
    Some(Item {
        kind: "built-in function or value",
        signature,
        ty: None,
        origin: "Aevia Prelude".into(),
        doc: None,
        alias: None,
        builtin: Some(name.to_string()),
        exported: true,
    })
}

const TYPE_NAMES: &[&str] = &[
    "Int", "Real", "Float", "Complex", "Bool", "String", "Unit", "Tensor", "Prop", "Option",
    "Result", "IO", "Dyn", "Point", "UnitOf", "Task", "Show",
];
#[allow(
    clippy::too_many_lines,
    reason = "keeps the type reference descriptions and signatures together"
)]
fn type_help(name: &str) -> Option<(&'static str, &'static str)> {
    let (signature, doc) = match name {
        | "Int" => {
            (
                "Int[d]",
                "Arbitrary-precision exact integers. Dimensions are optional; Int is dimensionless. Integer division with / produces an exact Real; quot performs integer division.",
            )
        },
        | "Real" => {
            (
                "Real[d]",
                "Exact rational or symbolic real quantities. Unlike Float, decimal literals in a Real context retain exact values. Use approx when a numerical approximation is needed.",
            )
        },
        | "Float" => {
            (
                "Float[d]",
                "IEEE 754 floating-point quantities. Decimal literals normally infer Float; to_float returns a checked conversion result for exact or symbolic values.",
            )
        },
        | "Complex" => {
            (
                "Complex[d]",
                "Complex numerical quantities with a carried physical dimension.",
            )
        },
        | "Bool" => {
            (
                "Bool",
                "Boolean values true and false. Conditions in if expressions must be Bool.",
            )
        },
        | "String" => {
            (
                "String",
                "Unicode text with interpolation: println(\"answer: {6 * 7}\"). Use concat(a, b) to concatenate strings; length counts grapheme clusters.",
            )
        },
        | "Unit" => {
            (
                "Unit",
                "The type of (). IO actions such as println return IO Unit. This type is distinct from a measurement unit such as m or s.",
            )
        },
        | "Tensor" => {
            (
                "Tensor<[extents...], element>",
                "A rectangular, typed multidimensional array. Examples: [1, 2, 3] and [[1.0, 2.0], [3.0, 4.0]]. Shape and element dimensions are checked; use shape, reshape, map, and the linear-algebra functions to work with tensors.",
            )
        },
        | "Prop" => {
            (
                "Prop",
                "A symbolic proposition. This differs from Bool, the concrete type required by if conditions.",
            )
        },
        | "Option" => {
            (
                "Option<T> = None | Some(T)",
                "An optional value. Match Some(value) and None to handle presence explicitly.",
            )
        },
        | "Result" => {
            (
                "Result<T, E> = Ok(T) | Err(E)",
                "A successful value or a recoverable failure. Pattern-match Ok/Err, or use ? inside a compatible Result-returning function or IO action.",
            )
        },
        | "IO" => {
            (
                "IO<T>",
                "A description of an effectful computation. In the REPL, evaluating an IO expression executes it once. In a file, define fn main(): IO Unit = do ...; use <- to bind IO results inside do.",
            )
        },
        | "Dyn" => {
            (
                "Dyn<representation>",
                "A quantity with a runtime dimension. dyn(value) erases its static dimension; check<L>(value) attempts a checked recovery.",
            )
        },
        | "Point" => {
            (
                "Point[d]",
                "An affine quantity, such as an absolute temperature. Use point(value, unit), to, and value_in; subtracting two points gives a difference.",
            )
        },
        | "UnitOf" => {
            (
                "Unit[d]",
                "A unit value such as m or km. It describes a scale and dimension; use a quantity literal such as 3 km to create a measured value.",
            )
        },
        | "Task" => {
            (
                "Task<T>",
                "An IO task handle created with spawn. await reads its result once; task output is buffered until it is awaited.",
            )
        },
        | "Show" => {
            (
                "Show a",
                "The display capability used when formatting values. show(value) returns text; print/println create IO actions. Use repr for a representation that preserves numeric phase.",
            )
        },
        | _ => return None,
    };
    Some((signature, doc))
}

#[allow(
    clippy::single_call_fn,
    reason = "renders a full document from shared type reference metadata"
)]
fn type_item(name: &str) -> Option<Item> {
    let (signature, doc) = type_help(name)?;
    Some(Item {
        kind: "built-in type or class",
        signature: signature.into(),
        ty: None,
        origin: "Aevia Prelude".into(),
        doc: Some(doc.into()),
        alias: None,
        builtin: None,
        exported: true,
    })
}

#[allow(
    clippy::single_call_fn,
    reason = "provides a dedicated formatter for unit definitions"
)]
fn unit_help(
    name: &str,
    unit: &aevia_dim::UnitDef,
) -> String {
    let mut out = format!(
        "{name} — unit\nCanonical unit: {}\nDimension: {}\nExact coherent scale: {}\n",
        unit.name,
        unit.dim.to_plain_string(),
        unit.scale
    );
    if let Some(offset) = &unit.offset {
        let _ = writeln!(out, "Affine offset: {offset}");
    }
    if !unit.aliases.is_empty() {
        let _ = writeln!(out, "Aliases: {}", unit.aliases.join(", "));
    }
    if unit.affine {
        out.push_str("\nThis is an affine unit. Use point(value, unit) for absolute values; use a linear difference unit for intervals.");
    } else {
        let _ = write!(
            out,
            "\nExample:\n  3 {}\nUse value_in to inspect a quantity in a chosen unit.",
            unit.display
        );
    }
    out
}

#[allow(
    clippy::single_call_fn,
    reason = "maps RSSN operator metadata to interactive help"
)]
fn kernel_help(name: &str) -> Option<String> {
    if name == "Rssn" {
        return Some("Rssn — scientific kernel module\n\nKernel functions are available under Rssn. Native signatures and documentation come from the bundled kernel manifest.\nUse :doc Rssn.NAME for a binding, or import Rssn as K and inspect K.NAME.\nThe dynamic escape hatch is Rssn.dynamic(\"operator\", [to_value(argument)]).".into());
    }
    let binding_name = name.strip_prefix("Rssn.")?;
    for binding in aevia_rssn::bindings::live_catalog() {
        if binding.name != binding_name
            && binding.typed_callback_name.as_deref() != Some(binding_name)
        {
            continue;
        }
        let mut out = format!(
            "{name} — kernel binding\nKernel operator: {}\nDefined at: bundled RSSN manifest ({})\n",
            binding.op.name, binding.op.rule_set
        );
        if let Some(signature) = binding
            .overload
            .and_then(|index| binding.op.signatures.get(index))
        {
            let _ = writeln!(out, "Kernel contract: {}", signature.render());
        }
        match &binding.coverage {
            | aevia_rssn::bindings::Coverage::Typed => {
                out.push_str("An exact typed Aevia binding is available.\n");
            },
            | aevia_rssn::bindings::Coverage::Dynamic(reasons) => {
                let _ = writeln!(out, "Dynamic bridge: {}", reasons.join("; "));
            },
            | aevia_rssn::bindings::Coverage::Unavailable(reasons) => {
                let _ = writeln!(out, "Unavailable in this build: {}", reasons.join("; "));
            },
        }
        if !binding.op.doc.trim().is_empty() {
            let _ = write!(out, "\n{}", binding.op.doc.trim());
        }
        return Some(out);
    }
    None
}

/// The description and example for a standard-library name, written for the
/// person calling it from Aevia. The prose lives in `aevia_stdlib_model::guides`
/// so the generated standard-library modules print the same text.
fn builtin_guide(name: &str) -> Option<(&'static str, &'static str)> {
    aevia_stdlib_model::guides::guide(name).map(|guide| (guide.description, guide.example))
}

#[allow(
    clippy::single_call_fn,
    reason = "keeps the spelling-distance metric separate from help ranking"
)]
fn edit_distance(
    left: &str,
    right: &str,
) -> usize {
    let right: Vec<_> = right.chars().collect();
    let mut row: Vec<_> = (0..=right.len()).collect();
    for (index, character) in left.chars().enumerate() {
        let mut diagonal = index;
        let mut west = index.saturating_add(1);
        if let Some(first) = row.first_mut() {
            *first = west;
        }
        for (other, cell) in right.iter().zip(row.iter_mut().skip(1)) {
            let north = *cell;
            *cell = north
                .saturating_add(1)
                .min(west.saturating_add(1))
                .min(diagonal.saturating_add(usize::from(character != *other)));
            west = *cell;
            diagonal = north;
        }
    }
    row.last().copied().unwrap_or_default()
}


#[allow(
    clippy::single_call_fn,
    reason = "maps operator syntax to interactive help text"
)]
fn operator_help(name: &str) -> Option<String> {
    let (syntax, description, example) = match name {
        | "+" | "-" => {
            (
                "left + right / left - right",
                "Add or subtract compatible numeric quantities. Both operands need the same dimension; representations are promoted according to the numeric rules.",
                "3 m + 2 m",
            )
        },
        | "*" | "·" | "×" => {
            (
                "left * right",
                "Multiply numeric values. Physical dimensions multiply too; tensor multiplication is elementwise with supported broadcasting. Use @ for matrix products.",
                "3 m * 2 s",
            )
        },
        | "/" | "÷" => {
            (
                "left / right",
                "Divide numeric values and their physical dimensions. Exact integer division yields an exact Real; quot is integer division.",
                "3 m / 2 s",
            )
        },
        | "^" => {
            (
                "base ^ exponent",
                "Exponentiation. The exponent is dimensionless; dimensionful powers need an exponent the checker can establish statically.",
                "(3 m)^2",
            )
        },
        | "@" => {
            (
                "left @ right",
                "Dot or matrix product, according to operand ranks. The contracting extents must agree and element dimensions multiply.",
                "[1.0, 2.0] @ [3.0, 4.0]",
            )
        },
        | "==" | "!=" => {
            (
                "left == right / left != right",
                "Compare compatible values for equality or inequality. The result is Bool; dimensions must agree for quantities.",
                "3 m == 300 cm",
            )
        },
        | "<" | "<=" | ">" | ">=" => {
            (
                "left < right / left <= right / left > right / left >= right",
                "Compare ordered compatible values. Quantities require the same physical dimension; the result is Bool.",
                "2 m < 3 m",
            )
        },
        | "&&" | "||" => {
            (
                "left && right / left || right",
                "Short-circuit Boolean conjunction or disjunction. Both operands must be Bool.",
                "true && false",
            )
        },
        | "!" => {
            (
                "!condition / value!",
                "Prefix ! negates a Bool. Postfix ! computes an exact integer factorial.",
                "!false\n5!",
            )
        },
        | "|>" => {
            (
                "value |> function",
                "Pass a value to a function: x |> f means f(x).",
                "3 |> (fn (x) => x + 1)",
            )
        },
        | "<|" => {
            (
                "function <| value",
                "Apply a function to a value: f <| x means f(x).",
                "(fn (x) => x + 1) <| 3",
            )
        },
        | "::" => {
            (
                "value :: collection",
                "Prepend an element to a rank-1 collection; its type must agree with the collection's elements.",
                "1 :: [2, 3]",
            )
        },
        | _ => return None,
    };
    Some(format!(
        "{name} — operator\n{syntax}\n\n{description}\n\nExample:\n  {}",
        example.replace('\n', "\n  ")
    ))
}


#[cfg(test)]
mod tests {
    use super::*;

    fn catalog_item(
        signature: &str,
        doc: &str,
    ) -> Item {
        Item {
            kind: "value",
            signature: signature.into(),
            ty: None,
            origin: "<test REPL>".into(),
            doc: Some(doc.into()),
            alias: None,
            builtin: None,
            exported: false,
        }
    }

    #[test]
    fn apropos_counts_all_matches_and_orders_visible_bindings_before_descriptions() {
        let mut catalog = Catalog::default();
        for index in (0..36).rev() {
            catalog.items.insert(
                format!("find_probe_{index:02}"),
                catalog_item("a\nb", "marker in the description"),
            );
        }
        catalog.items.insert(
            "aa_doc_only".into(),
            catalog_item("c", "A find_probe note about the marker"),
        );
        let mut expected = String::from("Item help matching `find_probe` (37):\n");
        for index in 0..30 {
            let _ = writeln!(expected, "  find_probe_{index:02} : a b");
        }
        expected.push_str("  … narrow the query to see more\nUse :doc NAME to read an item.");
        assert_eq!(catalog.search("  find_probe  ").unwrap(), expected);

        let two_words = catalog.search("find_probe marker").unwrap();
        assert!(two_words.starts_with("Item help matching `find_probe marker` (37):\n  aa_doc_only : c\n  find_probe_00 : a b\n"));
        assert_eq!(
            catalog.search("\t \u{2003}\n").unwrap_err(),
            "usage: :apropos TEXT (for example :apropos print or :apropos matrix)"
        );
    }

    #[test]
    fn cached_prelude_units_do_not_shadow_new_catalog_units_or_operators() {
        let mut catalog = Catalog::default();
        let original = catalog.render("m").unwrap();
        let original_add = catalog.render("+").unwrap();
        let dim = aevia_dim::Dim::base("Length");
        let replacement =
            aevia_dim::UnitDef::linear("m", dim.clone(), aevia_dim::Scale::parse("2").unwrap());
        catalog.units.insert("m".into(), replacement.clone());
        catalog
            .units
            .insert("custom_probe_unit".into(), replacement.clone());
        catalog.units.insert("+".into(), replacement);
        let updated = catalog.render("m").unwrap();
        assert_ne!(original, updated);
        assert!(updated.contains("Exact coherent scale: 2"));
        assert_eq!(catalog.render("+").unwrap(), original_add);
        assert_eq!(
            catalog.search("CUSTOM_PROBE_UNIT").unwrap(),
            "Item help matching `CUSTOM_PROBE_UNIT` (1):\n  custom_probe_unit : unit or dimension\nUse :doc NAME to read an item."
        );
    }

    #[test]
    fn accepted_repl_unit_joins_the_search_after_the_prelude_is_read() {
        let name = "repl_doc_probe_unit";
        assert!(Catalog::default().render(name).is_err());
        let identity = aevia_core::source::SourceIdentity::from_name("repl-doc-unit-test.aev");
        let mut compiler = aevia_core::driver::ReplCompiler::new(&std::env::temp_dir(), identity);
        let compilation = compiler.compile("unit repl_doc_probe_unit : Length = 2 m\n", None);
        assert!(
            !compilation.diagnostics.has_errors(),
            "{:?}",
            compilation.diagnostics.items
        );
        let catalog = Catalog::with_origins(&compilation, &|_| "<test REPL>".into());
        assert!(catalog.render(name).unwrap().contains(name));
        let help = catalog.search(name).unwrap();
        assert!(help.starts_with("Item help matching `repl_doc_probe_unit` (1):\n"));
    }

    #[test]
    fn bundled_examples_typecheck_without_running_any_effects() {
        // Every guide, not only the Prelude's: the generated standard-library
        // modules print the same examples.
        for guide in aevia_stdlib_model::guides::GUIDES {
            let (name, example) = (guide.name, guide.example);
            if example.is_empty() {
                continue;
            }
            let mut source = String::new();
            for (index, line) in example.lines().enumerate() {
                if line.starts_with(':') {
                    continue;
                }
                if line.starts_with(char::is_whitespace)
                    || ["let ", "fn ", "import ", "open "]
                        .iter()
                        .any(|keyword| line.starts_with(keyword))
                {
                    let _ = writeln!(source, "{line}");
                } else {
                    let _ = writeln!(source, "let __guide_{index} = {line}");
                }
            }
            // Examples may import standard-library modules, so they are loaded
            // as a module rather than compiled as a bare source.
            let entry = std::env::temp_dir().join("aevia-item-help-example.aev");
            let compilation = aevia_core::driver::load_with_roots_and_source(
                &entry,
                aevia_core::manifest::Roots::adjacent(&entry),
                &source,
                &aevia_types::check::CheckOptions::default(),
            );
            assert!(
                !compilation.diagnostics.has_errors(),
                "{name}: {source}\n{:?}",
                compilation.diagnostics.items
            );
        }
    }

    #[test]
    fn every_prelude_name_has_a_guide() {
        let missing: Vec<&str> = aevia_types::builtins::PRELUDE_NAMES
            .iter()
            .copied()
            .filter(|name| builtin_guide(name).is_none())
            .collect();
        assert!(
            missing.is_empty(),
            "these Prelude names have no description in aevia-stdlib-model/src/guides.rs: {missing:?}"
        );
    }

    #[test]
    fn operator_examples_typecheck_without_evaluation() {
        for operator in [
            "+", "-", "*", "/", "^", "@", "==", "<", "&&", "||", "!", "|>", "<|", "::",
        ] {
            let help = operator_help(operator).unwrap();
            let example = help.split("Example:\n").nth(1).unwrap();
            let source = example
                .lines()
                .enumerate()
                .map(|(index, line)| format!("let __operator_{index} = {}\n", line.trim()))
                .collect::<String>();
            let compilation = aevia_core::compile("operator-help.aev", &source);
            assert!(
                !compilation.diagnostics.has_errors(),
                "{operator}: {source}\n{:?}",
                compilation.diagnostics.items
            );
        }
    }
}
