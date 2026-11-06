//! The operator registry, built from the Kernel Manifest.
//!
//! Aevia keeps no operator list of its own. Everything the checker and the
//! runtime know about rssn — names, arities, overloads, units, constants,
//! capabilities — is read here from `Session::manifest()`, so an operator
//! added to the kernel appears on the next run without a line changing in
//! this repository.

use crate::error::KernelError;
use aevia_dim::Dim;
use aevia_dim::Scale;
use num_traits::ToPrimitive;
use rssn::kernel::manifest::KernelManifest;
use rssn::kernel::signature::DimExpr;
use rssn::kernel::signature::Exp;
use rssn::kernel::signature::NatExpr;
use rssn::kernel::signature::Rational;
use rssn::kernel::signature::Signature;
use rssn::kernel::signature::Stability;
use rssn::kernel::signature::TypeExpr;
use rssn::kernel::signature::Variadic;
use std::collections::BTreeMap;
use std::sync::OnceLock;

/// Where an overload's signature came from.
///
/// The distinction matters: `Declared` and `Overlay` are exact, `Derived` only
/// says "numeric overload takes `Real`, symbolic overload takes `Term`". A
/// tool that shows a `derived` signature as if it were exact is lying to the
/// user.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum Source {
    /// The operator declared its own signature.
    Declared,
    /// A hand-written overlay supplied it.
    Overlay,
    /// The kernel derived it from the registration.
    Derived,
    /// There is no signature; only a dynamic call reaches it.
    None,
}

impl Source {
    /// Whether the signature is exact rather than form-level.
    #[must_use]
    pub fn is_exact(self) -> bool {
        matches!(self, Self::Declared | Self::Overlay)
    }

    /// The spelling used in generated stubs and the IDE.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            | Self::Declared => "declared",
            | Self::Overlay => "overlay",
            | Self::Derived => "derived",
            | Self::None => "none",
        }
    }
}

/// A type as a signature states it.
///
/// This mirrors rssn's `TypeExpr` but is Aevia's own type: the checker maps it
/// to `aevia_types::Type` without ever importing the kernel's types.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SigType {
    /// An exact integer.
    Int,
    /// An exact rational.
    Real,
    /// A double.
    Float,
    /// A complex number.
    Complex,
    /// A truth value.
    Bool,
    /// Text.
    Str,
    /// A free symbol of the expression graph.
    Symbol,
    /// A unit definition.
    Unit(Dim),
    /// An affine point.
    Point(Dim),
    /// A scalar with a dimension.
    Dim(Box<Self>, Dim),
    /// A dense tensor.
    Tensor {
        /// Rank, when the signature fixes it.
        rank: Option<usize>,
        /// Fully concrete extents, when the signature proves every extent.
        /// Partially unknown shapes remain rank-only rather than fabricating
        /// an extent or a type-level variable identity.
        shape: Option<Vec<usize>>,
        /// Element type.
        elem: Box<Self>,
    },
    /// A sparse tensor.
    Sparse {
        /// Rank, when the signature fixes it.
        rank: Option<usize>,
        /// Element type.
        elem: Box<Self>,
    },
    /// A record.
    Record(Vec<(String, Self)>),
    /// A tagged union.
    Variant(Vec<(String, Vec<Self>)>),
    /// A function.
    Func(Vec<Self>, Box<Self>),
    /// A table.
    Table,
    /// A device handle.
    Device,
    /// A host type.
    Opaque(String),
    /// A type variable.
    Var(String),
    /// A value whose representation is only known at run time.
    Dyn,
    /// The type of an expression that never returns.
    Never,
}

/// One overload.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Sig {
    /// Parameter types.
    pub params: Vec<SigType>,
    /// Result type.
    pub ret: SigType,
    /// Trailing arguments, for variadic operators.
    pub variadic: Option<(usize, Option<usize>, SigType)>,
}

impl Sig {
    /// The signature as it is shown to a user.
    #[must_use]
    pub fn render(&self) -> String {
        let params: Vec<String> = self.params.iter().map(render_type).collect();
        let mut text = format!("({}) -> {}", params.join(", "), render_type(&self.ret));
        if let Some((min, max, elem)) = &self.variadic {
            let bound = max.map_or_else(|| "...".to_string(), |max| format!("..{max}"));
            text.push_str(&format!(" with {min}{bound} x {}", render_type(elem)));
        }
        text
    }
}

/// An operator as the manifest describes it.
#[derive(Clone, Debug)]
pub struct OpEntry {
    /// The operator's name.
    pub name: String,
    /// The rule set that registered it.
    pub rule_set: String,
    /// Stable keys, one per overload.
    pub keys: Vec<rssn::kernel::signature::OpKey>,
    /// The overloads, as Aevia sees them.
    pub signatures: Vec<Sig>,
    /// The overloads as the kernel stated them.
    ///
    /// Resolution uses these and not [`Sig`]: the lattice of
    /// `kernel::overload` is defined over the kernel's own `TypeExpr`, and a
    /// round trip through Aevia's mirror would be a second implementation of
    /// the same decision. `signatures` and `raw` are index-aligned; both are
    /// built from `OpInfo.overloads`, so `signatures.len() == raw.len()`.
    pub raw: Vec<Signature>,
    /// Where the signatures came from.
    pub source: Source,
    /// Kernel dimensional rule, when declared independently of signatures.
    pub dim_rule: Option<rssn::kernel::signature::DimRuleInfo>,
    /// Why the kernel omitted a signature, if it supplied a reason.
    pub dynamic_reason: Option<String>,
    /// Kernel evaluation route, distinguishing value-native and graph calls.
    pub eval_kind: rssn::kernel::signature::EvalKind,
    /// Backend availability stated by the kernel.
    pub backend_support: rssn::kernel::signature::BackendSupport,
    /// How settled the operator is.
    pub stability: Stability,
    /// Documentation.
    pub doc: String,
    /// Accepted argument counts.
    pub arity: (usize, Option<usize>),
    /// Whether it takes a function argument.
    pub higher_order: bool,
}

impl OpEntry {
    /// Whether calls use the value-native ABI rather than the dimensionless graph.
    #[must_use]
    pub fn is_value_native(&self) -> bool {
        matches!(
            self.eval_kind,
            rssn::kernel::signature::EvalKind::Dynamic
                | rssn::kernel::signature::EvalKind::HostCallback
        )
    }

    /// The primary signature, when there is one.
    #[must_use]
    pub fn primary(&self) -> Option<&Sig> {
        self.signatures.first()
    }

    /// The key of the overload `index`.
    #[must_use]
    pub fn key(
        &self,
        index: usize,
    ) -> Option<&rssn::kernel::signature::OpKey> {
        self.keys.get(index)
    }

    /// Whether the operator accepts `count` arguments.
    #[must_use]
    pub fn accepts(
        &self,
        count: usize,
    ) -> bool {
        count >= self.arity.0 && self.arity.1.is_none_or(|max| count <= max)
    }

    /// Whether the registration takes any number of arguments.
    ///
    /// This is the flag the ABI has to honour: a variadic operator's static
    /// type is `[α] -> R`, so one list argument at the call site means N
    /// positional arguments at the kernel.
    #[must_use]
    pub fn is_variadic(&self) -> bool {
        self.arity.1.is_none()
    }

    /// The index of the overload `key` names, when this entry has it.
    #[must_use]
    pub fn key_index(
        &self,
        key: &rssn::kernel::signature::OpKey,
    ) -> Option<usize> {
        self.keys
            .iter()
            .position(|candidate| candidate.signature_hash == key.signature_hash)
    }

    /// The key of the overload `arg_types` belong to.
    ///
    /// This is `kernel::overload`'s lattice, applied to types instead of
    /// values: arity first, then the weakest per-argument match, then the
    /// unique maximum. A tie is `E1002`; nothing accepted is `E1003`. The
    /// checker calls this so that what it types is what the runtime executes
    /// (PLAN D1).
    ///
    /// # Errors
    ///
    /// `E1002` for a tie, `E1003` when no overload accepts the arguments.
    pub fn resolve_by_type(
        &self,
        arg_types: &[rssn::kernel::signature::TypeExpr],
    ) -> Result<rssn::kernel::signature::OpKey, KernelError> {
        use rssn::kernel::overload::Match;
        let scored: Vec<(usize, Match)> =
            self.raw
                .iter()
                .enumerate()
                .filter_map(|(index, signature)| {
                    if !signature.accepts_arity(arg_types.len()) {
                        return None;
                    }
                    let mut worst = Match::Exact;
                    for (position, got) in arg_types.iter().enumerate() {
                        let param = signature.params.get(position).or_else(|| {
                            signature.variadic.as_ref().map(|variadic| &variadic.elem)
                        })?;
                        let matched = rssn::kernel::overload::matches_type(param, got);
                        if !matched.accepted() {
                            return None;
                        }
                        // `Match` is ordered `No < Generic < Shaped < Widened <
                        // Exact`, so the minimum is the weakest parameter — the
                        // same rule `overload::score` applies.
                        worst = worst.min(matched);
                    }
                    Some((index, worst))
                })
                .collect();
        let Some(top) = scored.iter().map(|(_, matched)| *matched).max() else {
            return Err(KernelError::of(
                "E1003",
                crate::error::Failure::Type,
                format!(
                    "no overload of `{}` accepts these argument types",
                    self.name
                ),
            )
            .at_operator(&self.name));
        };
        let tied: Vec<usize> = scored
            .iter()
            .filter(|(_, matched)| *matched == top)
            .map(|(index, _)| *index)
            .collect();
        if let [only] = tied.as_slice() {
            return match self.keys.get(*only) {
                | Some(key) => Ok(key.clone()),
                // `keys` and `overloads` are index-aligned by contract
                // (`OpInfo.keys` has one entry per overload). If they are not,
                // inventing a zero-hash key would silently execute the wrong
                // registration, so the inconsistency is the error.
                | None => {
                    Err(KernelError::of(
                        "E0899",
                        crate::error::Failure::Kernel,
                        format!(
                            "`{}` has {} overloads but {} keys: the manifest is inconsistent",
                            self.name,
                            self.raw.len(),
                            self.keys.len()
                        ),
                    )
                    .at_operator(&self.name))
                },
            };
        }
        Err(KernelError::of(
            "E1002",
            crate::error::Failure::Ambiguous,
            format!(
                "{} overloads of `{}` match equally well",
                tied.len(),
                self.name
            ),
        )
        .with_help(format!(
            "candidates: {}",
            tied.iter()
                .filter_map(|index| self.signatures.get(*index))
                .map(Sig::render)
                .collect::<Vec<_>>()
                .join("; ")
        ))
        .at_operator(&self.name))
    }
}

/// One row of the capability table.
///
/// There is exactly one table, derived from `Features`. The three consumers —
/// `Capabilities::has`, `Rssn.Capabilities` and the flags `Rssn.aev` declares
/// — read this, so a new `Features` field can no longer be invisible to two of
/// them.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Capability {
    /// The dotted name a diagnostic and `Rssn.Capabilities` use.
    pub name: String,
    /// The binding `Rssn.aev` declares for it.
    pub binding: String,
    /// Whether this build provides it.
    pub enabled: bool,
}

/// What the build can do.
#[derive(Clone, Debug, Default)]
pub struct Capabilities {
    /// Dynamic calls by name.
    pub dynamic_call: bool,
    /// A JIT backend is compiled in.
    pub jit: bool,
    /// The JIT lowers branches.
    pub jit_branch: bool,
    /// The JIT lowers indexing.
    pub jit_index: bool,
    /// Symbolic requests can run in parallel.
    pub parallel_symbolic: bool,
    /// Unit scales are exact.
    pub exact_units: bool,
    /// Affine units are supported.
    pub affine_units: bool,
    /// Hosts may define base dimensions.
    pub user_base_dimensions: bool,
    /// Structured results are available.
    pub structured_results: bool,
    /// Higher-order operators accept callbacks.
    pub higher_order: bool,
    /// The kernel's SDE noise primitives are available.
    pub sde: bool,
    /// How a plugin may reach this build.
    pub plugin_kinds: Vec<String>,
}

impl Capabilities {
    /// The table, in the order diagnostics and the generated module use.
    #[must_use]
    pub fn table(&self) -> Vec<Capability> {
        let row = |name: &str, binding: &str, enabled: bool| {
            Capability {
                name: name.to_string(),
                binding: binding.to_string(),
                enabled,
            }
        };
        let mut rows = vec![
            row("core.dynamic_call", "dynamic_call", self.dynamic_call),
            row("core.jit", "jit", self.jit),
            row("core.jit_branch", "jit_branch", self.jit_branch),
            row("core.jit_index", "jit_index", self.jit_index),
            row(
                "core.parallel_symbolic",
                "parallel_symbolic",
                self.parallel_symbolic,
            ),
            row("core.higher_order", "higher_order", self.higher_order),
            // The SDE noise primitives are the kernel's (`sde_wiener`,
            // `sde_levy_areas`); the solver loops live in `aevia-std` because
            // they need Aevia closures. A host gates its SDE surface on this
            // row, exactly as it gates ODE on `core.higher_order`-style flags.
            row("core.sde", "sde", self.sde),
            // Callback support is scoped to synchronous, thread-affine
            // interpreter calls. Individual signatures remain subject to the
            // binding catalog's Float-only, trailing callback restrictions.
            row(
                "core.callback",
                "callback",
                self.dynamic_call && self.higher_order,
            ),
            // Source-level plugin construction is not implemented.
            row("core.plugin.source", "plugin_source", false),
            row(
                "core.structured_results",
                "structured_results",
                self.structured_results,
            ),
            row("dim.exact_units", "exact_units", self.exact_units),
            row("dim.affine_units", "affine_units", self.affine_units),
            row(
                "dim.user_base_dimensions",
                "user_base_dimensions",
                self.user_base_dimensions,
            ),
            row("core.plugin", "plugin", !self.plugin_kinds.is_empty()),
        ];
        for kind in &self.plugin_kinds {
            rows.push(row(
                &format!("core.plugin.{kind}"),
                &format!("plugin_{}", kind.replace('-', "_")),
                true,
            ));
        }
        rows
    }

    /// Whether the named capability is present.
    ///
    /// An unknown name is not a capability this build has: a diagnostic that
    /// asks about one must say "missing", never "present by default".
    #[must_use]
    pub fn has(
        &self,
        name: &str,
    ) -> bool {
        self.table()
            .iter()
            .any(|row| row.name == name && row.enabled)
    }

    /// The names of the capabilities this build provides.
    #[must_use]
    pub fn names(&self) -> Vec<String> {
        self.table()
            .into_iter()
            .filter(|row| row.enabled)
            .map(|row| row.name)
            .collect()
    }

    /// Fails with `E1001` naming `name` when the build lacks it.
    ///
    /// This is what a genuine backend gap must report (PLAN §3.1): the code
    /// names the capability, so a user can tell "this build cannot" from
    /// "this program is wrong".
    ///
    /// # Errors
    ///
    /// `E1001("core.<capability>")`.
    pub fn require(
        &self,
        name: &str,
    ) -> Result<(), KernelError> {
        if self.has(name) {
            Ok(())
        } else {
            Err(KernelError::capability(name))
        }
    }

    /// The capability `entry` needs and this build does not provide.
    ///
    /// The requirement is read from the operator's own description — its
    /// `higher_order` flag and the shape of its signatures — never from a list
    /// of operator names. That is what keeps a *genuine* gap from turning into
    /// a type error or a panic deep inside the conversion: an operator that
    /// takes a callback, returns a structured record, or speaks in affine
    /// points cannot run in a build that says it has none of those.
    ///
    /// `None` means the operator needs nothing this build is missing.
    #[must_use]
    pub fn missing_for(
        &self,
        entry: &OpEntry,
    ) -> Option<&'static str> {
        if entry.higher_order && !self.higher_order {
            return Some("core.higher_order");
        }
        if !self.structured_results
            && entry
                .signatures
                .iter()
                .any(|signature| matches!(signature.ret, SigType::Record(_)))
        {
            return Some("core.structured_results");
        }
        if !self.affine_units && entry.signatures.iter().any(signature_is_affine) {
            return Some("dim.affine_units");
        }
        None
    }

    /// `E1001` when the build cannot run `entry` at all.
    ///
    /// # Errors
    ///
    /// `E1001` naming the missing capability, with the operator attached.
    pub fn require_op(
        &self,
        entry: &OpEntry,
    ) -> Result<(), KernelError> {
        match self.missing_for(entry) {
            | Some(name) => Err(KernelError::capability(name).at_operator(&entry.name)),
            | None => Ok(()),
        }
    }
}

/// Whether a signature speaks in affine points (`degC`, `degF`).
fn signature_is_affine(signature: &Sig) -> bool {
    signature
        .params
        .iter()
        .chain(std::iter::once(&signature.ret))
        .any(|ty| matches!(ty, SigType::Point(_)))
}

/// A unit, as the manifest describes it.
#[derive(Clone, Debug)]
pub struct UnitInfo {
    /// Canonical spelling.
    pub name: String,
    /// How it is printed.
    pub display: String,
    /// The dimension it measures.
    pub dim: Dim,
    /// Exact scale to coherent SI.
    pub scale: Scale,
    /// Offset for affine units.
    pub offset: Option<Scale>,
    /// Whether it is an affine point scale.
    pub affine: bool,
    /// Other spellings.
    pub aliases: Vec<String>,
    /// Whether SI prefixes may be attached.
    pub prefixable: bool,
    /// Coarse grouping.
    pub category: String,
}

/// A physical constant, as the manifest describes it.
#[derive(Clone, Debug)]
pub struct ConstantInfo {
    /// What it is called.
    pub name: String,
    /// The symbol it is written with.
    pub symbol: String,
    /// Exact value in coherent SI units.
    pub value: Scale,
    /// What it measures.
    pub dim: Dim,
    /// Coherent SI unit.
    pub unit: String,
    /// Whether the value is exact by definition.
    pub exact: bool,
    /// Standard uncertainty, when measured.
    pub uncertainty: Option<Scale>,
    /// Where the value comes from.
    pub source: String,
}

/// The manifest, indexed for lookup.
#[derive(Debug)]
pub struct Registry {
    /// Every spelling (name and aliases) mapped to the operator it denotes.
    ops: BTreeMap<String, OpEntry>,
    /// One entry per operator in `manifest.ops`, in name order.
    ///
    /// Iteration is over this and not over `ops`: the map is keyed by spelling,
    /// so it also holds one entry per alias, and counting or generating from it
    /// would over-report operators and emit a second declaration for one.
    canonical: Vec<OpEntry>,
    /// Every base dimension the kernel knows.
    pub base_dimensions: Vec<String>,
    /// Units, sorted by name.
    pub units: Vec<UnitInfo>,
    /// Physical constants, sorted by symbol.
    pub constants: Vec<ConstantInfo>,
    /// What the build can do.
    pub capabilities: Capabilities,
    /// The kernel version.
    pub version: String,
    /// The dynamic ABI version.
    pub api_version: u32,
    /// The manifest hash, in hex.
    pub manifest_hash: String,
    /// How each value variant may be used.
    pub value_kinds: Vec<(String, bool, String)>,
}

impl Registry {
    /// Indexes a manifest.
    #[must_use]
    pub fn from_manifest(manifest: &KernelManifest) -> Self {
        let mut ops = BTreeMap::new();
        let mut canonical = Vec::with_capacity(manifest.ops.len());
        for op in &manifest.ops {
            let entry = OpEntry {
                name: op.name.clone(),
                rule_set: op.rule_set.clone(),
                keys: op.keys.clone(),
                signatures: op.overloads.iter().map(convert_signature).collect(),
                raw: op.overloads.clone(),
                source: match op.signature_source {
                    | rssn::kernel::manifest::SignatureSource::Declared => Source::Declared,
                    | rssn::kernel::manifest::SignatureSource::Overlay => Source::Overlay,
                    | rssn::kernel::manifest::SignatureSource::Derived => Source::Derived,
                    | rssn::kernel::manifest::SignatureSource::None => Source::None,
                },
                dim_rule: op.dim_rule.clone(),
                dynamic_reason: op.dynamic_reason.clone(),
                eval_kind: op.eval,
                backend_support: op.backend_support,
                stability: op.stability,
                doc: op.doc.clone(),
                arity: (usize::from(op.arity.min), op.arity.max.map(usize::from)),
                higher_order: op.higher_order,
            };
            debug_assert_eq!(
                entry.signatures.len(),
                entry.raw.len(),
                "`{}` must keep its two overload views index-aligned",
                op.name
            );
            // An alias resolves to the same entry; a name that two rule sets
            // both register keeps the first, which is what the dynamic ABI
            // does as well.
            for alias in &op.aliases {
                ops.entry(alias.clone()).or_insert_with(|| entry.clone());
            }
            ops.insert(op.name.clone(), entry.clone());
            canonical.push(entry);
        }
        canonical.sort_by(|left, right| left.name.cmp(&right.name));
        let features = &manifest.features;
        Self {
            ops,
            canonical,
            base_dimensions: manifest
                .base_dimensions
                .iter()
                .map(|base| base.name.clone())
                .collect(),
            units: manifest.units.iter().map(convert_unit).collect(),
            constants: manifest.constants.iter().map(convert_constant).collect(),
            capabilities: Capabilities {
                dynamic_call: features.dynamic_call,
                jit: features.jit,
                jit_branch: features.jit_branch,
                jit_index: features.jit_index,
                parallel_symbolic: features.parallel_symbolic,
                exact_units: features.exact_units,
                affine_units: features.affine_units,
                user_base_dimensions: features.user_base_dimensions,
                structured_results: features.structured_results,
                higher_order: features.higher_order,
                sde: features.sde,
                plugin_kinds: features.plugin_kinds.clone(),
            },
            version: manifest.rssn_version.clone(),
            api_version: manifest.api_version,
            manifest_hash: hex(&manifest.manifest_hash),
            value_kinds: manifest
                .value_kinds
                .iter()
                .map(|info| {
                    (
                        format!("{:?}", info.kind).to_lowercase(),
                        info.in_term_graph,
                        info.note.clone(),
                    )
                })
                .collect(),
        }
    }

    /// The operator called `name` (or one of its aliases).
    #[must_use]
    pub fn op(
        &self,
        name: &str,
    ) -> Option<&OpEntry> {
        self.ops.get(name)
    }

    /// Every operator, in name order, once per operator.
    pub fn ops(&self) -> impl Iterator<Item = &OpEntry> {
        self.canonical.iter()
    }

    /// How many operators the manifest registers.
    ///
    /// Aliases are spellings of an operator, not operators: this is
    /// `manifest.ops.len()`.
    #[must_use]
    pub fn op_count(&self) -> usize {
        self.canonical.len()
    }

    /// The unit called `name`.
    #[must_use]
    pub fn unit(
        &self,
        name: &str,
    ) -> Option<&UnitInfo> {
        self.units
            .iter()
            .find(|unit| unit.name == name || unit.aliases.iter().any(|alias| alias == name))
    }

    /// The constant with that symbol.
    #[must_use]
    pub fn constant(
        &self,
        symbol: &str,
    ) -> Option<&ConstantInfo> {
        self.constants
            .iter()
            .find(|constant| constant.symbol == symbol)
    }

    /// The overload of `name` that `arg_types` belong to.
    ///
    /// This is the function the checker calls at a `Rssn.<op>` call site: it
    /// returns the `OpKey` to pin, and it is the *only* place the choice is
    /// made. The runtime executes that key without resolving again (PLAN D1).
    ///
    /// # Errors
    ///
    /// `E1003` when no operator is registered under `name`, `E1002` when
    /// several overloads tie, `E1003` when none accepts the arguments.
    pub fn resolve(
        &self,
        name: &str,
        arg_types: &[SigType],
    ) -> Result<rssn::kernel::signature::OpKey, KernelError> {
        let Some(entry) = self.op(name) else {
            return Err(KernelError::of(
                "E1003",
                crate::error::Failure::Unknown,
                format!("rssn has no operator `{name}`"),
            )
            .with_help("run `aevia rssn` for the operator count, or use `Rssn.dynamic`"));
        };
        if entry.raw.is_empty() {
            // A registration that declares no overload: the all-zero hash key
            // is the one `call_key` accepts.
            return Ok(rssn::kernel::signature::OpKey::new(
                &entry.rule_set,
                &entry.name,
                None,
            ));
        }
        let types: Vec<TypeExpr> = arg_types.iter().map(to_type_expr).collect();
        entry.resolve_by_type(&types)
    }
}

/// The overload of `name` that `arg_types` belong to.
///
/// The checker's entry point; see [`Registry::resolve`].
///
/// # Errors
///
/// `E1002` for an ambiguity, `E1003` when nothing matches.
pub fn resolve_key(
    name: &str,
    arg_types: &[SigType],
) -> Result<rssn::kernel::signature::OpKey, KernelError> {
    registry().resolve(name, arg_types)
}

static REGISTRY: OnceLock<Registry> = OnceLock::new();

/// The registry of the current build.
#[must_use]
pub fn registry() -> &'static Registry {
    REGISTRY.get_or_init(|| {
        let manifest = crate::kernel::kernel().manifest();
        Registry::from_manifest(&manifest)
    })
}

fn hex(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        out.push_str(&format!("{byte:02x}"));
    }
    out
}

/// The dimension variables of one signature.
///
/// A kernel signature names its dimension parameters (`T`), and the same name
/// means the same Aevia dimension variable in every parameter and in the
/// result (`sde_wiener` puts `T` under a square root: `Real[T]` in, `T^(1/2)`
/// out). The conversion therefore shares one mapping per signature; without it
/// every occurrence would become an unrelated variable and the emitted source
/// could not express the relationship. `Any` is *not* shared: each occurrence
/// is an independently unpinned dimension.
#[derive(Default)]
struct SignatureDims {
    named: std::collections::HashMap<String, u32>,
}

impl SignatureDims {
    /// The variable for a signature dimension parameter named `name`.
    fn var(
        &mut self,
        name: &str,
    ) -> Dim {
        let id = *self.named.entry(name.to_string()).or_insert_with(fresh_dim);
        Dim::var(id)
    }
}

fn convert_signature(signature: &Signature) -> Sig {
    let mut dims = SignatureDims::default();
    Sig {
        params: signature
            .params
            .iter()
            .map(|ty| convert_type_with(ty, &mut dims))
            .collect(),
        ret: convert_type_with(&signature.ret, &mut dims),
        variadic: signature
            .variadic
            .as_ref()
            .map(|variadic| variadic_bounds_with(variadic, &mut dims)),
    }
}

fn variadic_bounds_with(
    variadic: &Variadic,
    dims: &mut SignatureDims,
) -> (usize, Option<usize>, SigType) {
    (
        usize::from(variadic.min),
        variadic.max.map(usize::from),
        convert_type_with(&variadic.elem, dims),
    )
}

fn convert_type_with(
    ty: &TypeExpr,
    dims: &mut SignatureDims,
) -> SigType {
    match ty {
        | TypeExpr::Int => SigType::Int,
        | TypeExpr::Real => SigType::Real,
        | TypeExpr::Float => SigType::Float,
        | TypeExpr::Complex => SigType::Complex,
        | TypeExpr::Bool => SigType::Bool,
        | TypeExpr::String => SigType::Str,
        | TypeExpr::Symbol => SigType::Symbol,
        | TypeExpr::Unit(dim) => SigType::Unit(convert_dim_with(dim, dims)),
        | TypeExpr::Point(dim) => SigType::Point(convert_dim_with(dim, dims)),
        | TypeExpr::Dim(inner, dim) => {
            SigType::Dim(
                Box::new(convert_type_with(inner, dims)),
                convert_dim_with(dim, dims),
            )
        },
        | TypeExpr::Tensor { rank, shape, elem } => {
            SigType::Tensor {
                rank: convert_rank(rank),
                shape: shape
                    .as_ref()
                    .and_then(|extents| {
                        extents
                            .iter()
                            .map(|extent| {
                                match extent {
                                    | NatExpr::Lit(n) => usize::try_from(*n).ok(),
                                    | _ => None,
                                }
                            })
                            .collect::<Option<Vec<_>>>()
                    })
                    .filter(|extents| convert_rank(rank).is_none_or(|r| r == extents.len())),
                elem: Box::new(convert_type_with(elem, dims)),
            }
        },
        | TypeExpr::Sparse { rank, elem, .. } => {
            SigType::Sparse {
                rank: convert_rank(rank),
                elem: Box::new(convert_type_with(elem, dims)),
            }
        },
        | TypeExpr::Record(fields) => {
            SigType::Record(
                fields
                    .iter()
                    .map(|(name, ty)| (name.clone(), convert_type_with(ty, dims)))
                    .collect(),
            )
        },
        | TypeExpr::Variant(variants) => {
            SigType::Variant(
                variants
                    .iter()
                    .map(|(name, payload)| {
                        (
                            name.clone(),
                            payload
                                .iter()
                                .map(|ty| convert_type_with(ty, dims))
                                .collect(),
                        )
                    })
                    .collect(),
            )
        },
        | TypeExpr::Function(params, ret) => {
            SigType::Func(
                params
                    .iter()
                    .map(|ty| convert_type_with(ty, dims))
                    .collect(),
                Box::new(convert_type_with(ret, dims)),
            )
        },
        | TypeExpr::Table(_) => SigType::Table,
        | TypeExpr::Dyn(_) => SigType::Dyn,
        | TypeExpr::Device => SigType::Device,
        | TypeExpr::Opaque(name) => SigType::Opaque(name.clone()),
        | TypeExpr::Var(name) => SigType::Var(name.clone()),
        | TypeExpr::Never => SigType::Never,
    }
}

fn convert_rank(rank: &NatExpr) -> Option<usize> {
    match rank {
        | NatExpr::Lit(value) => usize::try_from(*value).ok(),
        | _ => None,
    }
}

/// The kernel's spelling of a type Aevia's mirror states.
///
/// [`convert_type`] is lossy in the ways a *display* may be lossy (a tensor's
/// extents, a `Dyn`'s representation). Resolution only compares constructors
/// and dimensions, and those survive, so the round trip is sound for the
/// lattice even though it is not an identity.
#[must_use]
pub fn to_type_expr(ty: &SigType) -> TypeExpr {
    match ty {
        | SigType::Int => TypeExpr::Int,
        | SigType::Real => TypeExpr::Real,
        | SigType::Float => TypeExpr::Float,
        | SigType::Complex => TypeExpr::Complex,
        | SigType::Bool => TypeExpr::Bool,
        | SigType::Str => TypeExpr::String,
        | SigType::Symbol => TypeExpr::Symbol,
        | SigType::Unit(dim) => TypeExpr::Unit(to_dim_expr(dim)),
        | SigType::Point(dim) => TypeExpr::Point(to_dim_expr(dim)),
        | SigType::Dim(inner, dim) => {
            TypeExpr::Dim(Box::new(to_type_expr(inner)), to_dim_expr(dim))
        },
        | SigType::Tensor { rank, shape, elem } => {
            if rank.is_some_and(|rank| u64::try_from(rank).is_err())
                || shape.as_ref().is_some_and(|extents| {
                    !rank.is_some_and(|rank| rank == extents.len())
                        || extents.iter().any(|extent| u64::try_from(*extent).is_err())
                })
            {
                return TypeExpr::Never;
            }
            TypeExpr::Tensor {
                rank: rank
                    .and_then(|rank| u64::try_from(rank).ok())
                    .map_or_else(|| NatExpr::Var("unknown_rank".to_string()), NatExpr::Lit),
                shape: shape
                    .as_ref()
                    .filter(|extents| rank.is_some_and(|rank| rank == extents.len()))
                    .and_then(|extents| {
                        extents
                            .iter()
                            .map(|extent| u64::try_from(*extent).ok().map(NatExpr::Lit))
                            .collect()
                    }),
                elem: Box::new(to_type_expr(elem)),
            }
        },
        | SigType::Sparse { rank, elem } => {
            TypeExpr::Sparse {
                format: String::new(),
                rank: rank
                    .and_then(|rank| u64::try_from(rank).ok())
                    .map_or_else(|| NatExpr::Var("unknown_rank".to_string()), NatExpr::Lit),
                elem: Box::new(to_type_expr(elem)),
            }
        },
        | SigType::Record(fields) => {
            TypeExpr::Record(
                fields
                    .iter()
                    .map(|(name, ty)| (name.clone(), to_type_expr(ty)))
                    .collect(),
            )
        },
        | SigType::Variant(variants) => {
            TypeExpr::Variant(
                variants
                    .iter()
                    .map(|(name, payload)| {
                        (name.clone(), payload.iter().map(to_type_expr).collect())
                    })
                    .collect(),
            )
        },
        | SigType::Func(params, ret) => {
            TypeExpr::Function(
                params.iter().map(to_type_expr).collect(),
                Box::new(to_type_expr(ret)),
            )
        },
        | SigType::Table => {
            TypeExpr::Table(rssn::kernel::signature::RowExpr {
                columns: Vec::new(),
                rows: NatExpr::Lit(0),
            })
        },
        | SigType::Device => TypeExpr::Device,
        | SigType::Opaque(name) => TypeExpr::Opaque(name.clone()),
        | SigType::Var(name) => TypeExpr::Var(name.clone()),
        | SigType::Dyn => {
            TypeExpr::Dyn(rssn::kernel::signature::RepExpr {
                repr: "term".to_string(),
                dim: None,
            })
        },
        | SigType::Never => TypeExpr::Never,
    }
}

/// Converts a dimension expression to Aevia's algebra with fresh variables.
///
/// A signature dimension variable (`TypeParamKind::Dim`, e.g. `T`) shares one
/// Aevia variable across a whole signature when converted through
/// [`convert_signature`]; this wrapper allocates fresh ids, for callers that
/// only convert one expression.
///
/// `Any` and a signature dimension variable mean "some dimension, not pinned";
/// they become a dimension variable rather than the dimensionless scalar.
/// Reading them as `1` turns the loosest possible signature into the strictest
/// possible type: `Real[Any] -> Real` would only accept a plain `Real`, which
/// is the one thing the kernel does *not* promise. Two `Any`s in one signature
/// are independent variables.
///
/// A symbolic exponent, non-metric value or container has no scalar dimension
/// mirror. They receive fresh unknown dimensions instead of the false claim
/// `1`; [`signature_has_unrepresentable_dimensions`] lets static callers
/// decline signatures whose relationships cannot be expressed.
#[must_use]
pub fn convert_dim(dim: &DimExpr) -> Dim {
    convert_dim_with(dim, &mut SignatureDims::default())
}

/// The shared form of [`convert_dim`]: named signature variables are interned.
fn convert_dim_with(
    dim: &DimExpr,
    dims: &mut SignatureDims,
) -> Dim {
    match dim {
        | DimExpr::Base(name) => Dim::base(name),
        | DimExpr::Mul(factors) => {
            factors.iter().fold(Dim::one(), |acc, factor| {
                acc.mul(&convert_dim_with(factor, dims))
            })
        },
        | DimExpr::Pow(base, Exp::Rat(exponent)) => {
            let Some(exponent) = convert_exponent(exponent) else {
                return Dim::one();
            };
            convert_dim_with(base, dims).pow(&exponent)
        },
        | DimExpr::Pow(_, Exp::Symbol(_)) => Dim::var(fresh_dim()),
        | DimExpr::Inverse(base) => {
            convert_dim_with(base, dims).pow(&num_rational::BigRational::from_integer(
                num_bigint::BigInt::from(-1),
            ))
        },
        | DimExpr::Dimless => Dim::one(),
        // Neither non-metric nor container is a dimension. Preserve the fact
        // that it is unknown; callers must consult the raw signature before
        // making a static promise about it.
        | DimExpr::NonMetric | DimExpr::Container => Dim::var(fresh_dim()),
        | DimExpr::Var(name) => dims.var(name),
        | DimExpr::Any => Dim::var(fresh_dim()),
    }
}

/// Ids for the dimension variables the signature conversion introduces.
///
/// The checker numbers its own variables from zero; starting far above that
/// range makes an accidental collision require 16 million variables in one
/// compilation unit.
const DIM_VAR_BASE: u32 = 1 << 24;

thread_local! {
    static NEXT_DIM_VAR: std::cell::Cell<u32> = const { std::cell::Cell::new(DIM_VAR_BASE) };
}

/// A dimension variable id that no other converted signature uses.
fn fresh_dim() -> u32 {
    NEXT_DIM_VAR.with(|next| {
        let id = next.get();
        next.set(id.wrapping_add(1));
        id
    })
}

fn convert_exponent(exponent: &Rational) -> Option<num_rational::BigRational> {
    Some(num_rational::BigRational::new(
        num_bigint::BigInt::from(exponent.numer),
        num_bigint::BigInt::from(exponent.denom),
    ))
}

/// Whether any part of a kernel signature uses dimensional semantics that
/// Aevia's scalar dimension algebra cannot express. Such signatures may still
/// be called dynamically, but must not be presented as exact static types.
#[must_use]
pub fn signature_has_unrepresentable_dimensions(signature: &Signature) -> bool {
    fn dim(d: &DimExpr) -> bool {
        match d {
            | DimExpr::NonMetric | DimExpr::Container | DimExpr::Pow(_, Exp::Symbol(_)) => true,
            | DimExpr::Pow(base, _) | DimExpr::Inverse(base) => dim(base),
            | DimExpr::Mul(parts) => parts.iter().any(dim),
            | _ => false,
        }
    }
    fn ty(t: &TypeExpr) -> bool {
        match t {
            | TypeExpr::Unit(d) | TypeExpr::Point(d) => dim(d),
            | TypeExpr::Dim(inner, d) => dim(d) || ty(inner),
            | TypeExpr::Tensor { elem, .. } | TypeExpr::Sparse { elem, .. } => ty(elem),
            | TypeExpr::Record(fields) => fields.iter().any(|(_, t)| ty(t)),
            | TypeExpr::Variant(cases) => cases.iter().any(|(_, fields)| fields.iter().any(ty)),
            | TypeExpr::Function(args, ret) => args.iter().any(ty) || ty(ret),
            | TypeExpr::Table(row) => row.columns.iter().any(|(_, t)| ty(t)),
            | TypeExpr::Dyn(rep) => rep.dim.as_ref().is_some_and(dim),
            | _ => false,
        }
    }
    signature
        .params
        .iter()
        .chain(std::iter::once(&signature.ret))
        .chain(signature.variadic.iter().map(|v| &v.elem))
        .any(ty)
}

/// Converts Aevia's dimension to a signature dimension.
///
/// A dimension variable becomes `DimExpr::Var("dN")`, so a unit or a signature
/// that mentions one survives the round trip; exponents that do not fit the
/// signature's `i64` pair become `Any`: the static mirror cannot state them,
/// and pretending to would be worse than saying "unknown".
#[must_use]
pub fn to_dim_expr(dim: &Dim) -> DimExpr {
    if dim.is_one() {
        return DimExpr::Dimless;
    }
    let mut factors = Vec::new();
    for (atom, exponent) in dim.factors() {
        let (Some(numer), Some(denom)) = (exponent.numer().to_i64(), exponent.denom().to_i64())
        else {
            return DimExpr::Any;
        };
        let base = match atom {
            | aevia_dim::Atom::Base(name) => DimExpr::Base(name.to_string()),
            | aevia_dim::Atom::Var(id) => DimExpr::Var(format!("d{id}")),
        };
        factors.push(base.pow(Exp::Rat(Rational::new(numer, denom))));
    }
    match factors.len() {
        | 0 => DimExpr::Dimless,
        | 1 => factors.remove(0),
        | _ => DimExpr::Mul(factors),
    }
}

fn convert_unit(unit: &rssn::kernel::value::UnitValue) -> UnitInfo {
    UnitInfo {
        name: unit.name.clone(),
        display: unit.display.clone(),
        dim: convert_dim(&unit.dimension),
        scale: convert_scale(&unit.scale),
        offset: unit.offset.as_ref().map(convert_scale),
        affine: unit.affine,
        aliases: unit.aliases.clone(),
        prefixable: unit.prefixable,
        category: unit.category.clone(),
    }
}

fn convert_constant(constant: &rssn::kernel::value::ConstantValue) -> ConstantInfo {
    ConstantInfo {
        name: constant.name.clone(),
        symbol: constant.symbol.clone(),
        value: convert_scale(&constant.value),
        dim: convert_dim(&constant.dimension),
        unit: constant.unit.clone(),
        exact: constant.exact,
        uncertainty: constant.uncertainty.as_ref().map(convert_scale),
        source: constant.source.clone(),
    }
}

/// The compiler's scale for a manifest scale.
#[must_use]
pub fn convert_scale(scale: &rssn::kernel::value::ScaleValue) -> Scale {
    Scale {
        rat: num_rational::BigRational::new(scale.numer.clone(), scale.denom.clone()),
        pi: scale.pi,
    }
}

/// The manifest form of a scale.
#[must_use]
pub fn to_scale_value(scale: &Scale) -> rssn::kernel::value::ScaleValue {
    rssn::kernel::value::ScaleValue {
        numer: scale.rat.numer().clone(),
        denom: scale.rat.denom().clone(),
        pi: scale.pi,
    }
}

/// Renders a signature type the way the surface syntax writes it.
///
/// A dimension the signature does not pin down is written `[_]`: the variable
/// the conversion allocated is an internal id (`d16777216`), and printing it
/// would suggest the language has a name for it.
#[must_use]
pub fn render_type(ty: &SigType) -> String {
    match ty {
        | SigType::Int => "Int".to_string(),
        | SigType::Real => "Real".to_string(),
        | SigType::Float => "Float".to_string(),
        | SigType::Complex => "Complex".to_string(),
        | SigType::Bool => "Bool".to_string(),
        | SigType::Str => "String".to_string(),
        | SigType::Symbol => "Symbol".to_string(),
        | SigType::Unit(dim) => format!("Unit[{}]", render_dim(dim)),
        | SigType::Point(dim) => format!("Point[{}]", render_dim(dim)),
        | SigType::Dim(inner, dim) => format!("{}[{}]", render_type(inner), render_dim(dim)),
        | SigType::Tensor { rank, shape, elem } => {
            if let Some(shape) = shape
                .as_ref()
                .filter(|shape| rank.is_some_and(|rank| rank == shape.len()))
            {
                return format!(
                    "Tensor<[{}], {}>",
                    shape
                        .iter()
                        .map(usize::to_string)
                        .collect::<Vec<_>>()
                        .join(", "),
                    render_type(elem)
                );
            }
            match rank {
                | Some(rank) => format!("Tensor<{rank}, {}>", render_type(elem)),
                | None => format!("Tensor<_, {}>", render_type(elem)),
            }
        },
        | SigType::Sparse { rank, elem } => {
            match rank {
                | Some(rank) => format!("Sparse<{rank}, {}>", render_type(elem)),
                | None => format!("Sparse<_, {}>", render_type(elem)),
            }
        },
        | SigType::Record(fields) => {
            let items: Vec<String> = fields
                .iter()
                .map(|(name, ty)| format!("{name}: {}", render_type(ty)))
                .collect();
            format!("{{ {} }}", items.join(", "))
        },
        | SigType::Variant(variants) => {
            let items: Vec<String> = variants
                .iter()
                .map(|(name, payload)| {
                    if payload.is_empty() {
                        name.clone()
                    } else {
                        let types: Vec<String> = payload.iter().map(render_type).collect();
                        format!("{name}({})", types.join(", "))
                    }
                })
                .collect();
            items.join(" | ")
        },
        | SigType::Func(params, ret) => {
            let types: Vec<String> = params.iter().map(render_type).collect();
            format!("({}) -> {}", types.join(", "), render_type(ret))
        },
        | SigType::Table => "Table".to_string(),
        | SigType::Device => "Device".to_string(),
        | SigType::Opaque(name) => name.clone(),
        | SigType::Var(name) => name.clone(),
        | SigType::Dyn => "Dyn<term>".to_string(),
        | SigType::Never => "Never".to_string(),
    }
}

/// A dimension as it may be written in a signature.
fn render_dim(dim: &Dim) -> String {
    if dim.is_concrete() {
        dim.to_plain_string()
    } else {
        "_".to_string()
    }
}
