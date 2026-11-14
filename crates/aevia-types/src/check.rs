//! Checking a module: name resolution, inference, and the exported interface.
//!
//! The checker walks the AST in four passes:
//!
//! 1. dimensions, units and type declarations, in source order;
//! 2. top-level signatures, so bodies may refer forward (§7.1.2);
//! 3. bodies;
//! 4. defaulting and generalisation of the top-level bindings.
//!
//! The output is a [`CheckedModule`] whose export table the runtime consumes:
//! it records which names are values, constructors, types or units, and their
//! types after defaulting.

use crate::CallableId;
use crate::CallableProjection;
use crate::adt::Adt;
use crate::adt::Ctor;
use crate::adt::{
    self,
};
use crate::builtins::dynamic_value_type;
use crate::builtins::op_error_type;
use crate::builtins::{
    self,
};
use crate::class::ClassDef;
use crate::class::ClassError;
use crate::class::ClassTable;
use crate::class::DeriveError;
use crate::class::Instance;
use crate::class::InstanceError;
use crate::class::Origin;
use crate::class::Resolution;
use crate::class::is_structural_show_head;
use crate::class::method_scheme;
use crate::diagnostic_bridge::code;
use crate::infer::Infer;
use crate::infer::UnifyError;
use crate::keys::ClassKey;
use crate::keys::InstanceKey;
use crate::keys::ModulePath;
use crate::ty::Constraint;
use crate::ty::LiteralKind;
use crate::ty::Nat;
use crate::ty::Rep;
use crate::ty::Row;
use crate::ty::Scheme;
use crate::ty::Shape;
use crate::ty::Type;
use aevia_dim::Dim;
use aevia_dim::DimEnv;
use aevia_dim::UnitDef;
use aevia_dim::UnitError;
use aevia_dim::UnitExpr;
use aevia_dim::UnitTable;
use aevia_syntax::Diagnostic;
use aevia_syntax::Diagnostics;
use aevia_syntax::FileId;
use aevia_syntax::Span;
use aevia_syntax::ast::*;
use indexmap::IndexMap;
use num_bigint::BigInt;
use num_rational::BigRational;
use num_traits::One;
use num_traits::Signed;
use num_traits::ToPrimitive;
use num_traits::Zero;
use std::collections::HashMap;
use std::collections::HashSet;

mod sparse;

/// What a name exported by a module denotes.
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub enum Export {
    /// A function or value binding.
    Value {
        /// Its generalised type.
        scheme: Scheme,
        /// Index into the module's declarations.
        decl: usize,
        /// Number of parameters for functions.
        arity: usize,
    },
    /// A variant constructor.
    Ctor {
        /// The type it belongs to.
        #[serde(with = "crate::persist::adt_ref")]
        adt: adt::AdtId,
        /// Its position in the type.
        index: usize,
        /// Its type as a function or value.
        scheme: Scheme,
    },
    /// A nominal type whose single constructor has the same name: `type P2 =
    /// | P2(Float, Float)`. Both `A.P2` (in an annotation) and `A.P2(1, 2)`
    /// (as a value) must work, so the export carries both.
    TypeCtor {
        /// The type.
        #[serde(with = "crate::persist::adt_ref")]
        adt: adt::AdtId,
        /// The constructor's position.
        index: usize,
        /// The constructor's type.
        scheme: Scheme,
    },
    /// A nominal type.
    Type {
        /// The registered type.
        #[serde(with = "crate::persist::adt_ref")]
        adt: adt::AdtId,
    },
    /// A transparent type alias.
    Alias {
        /// Number of parameters.
        params: usize,
    },
    /// A unit value such as `m`.
    Unit {
        /// The definition, with dimension and scale.
        def: UnitDef,
    },
    /// A base dimension or alias.
    Dimension {
        /// The resolved dimension, including its defining owner for bases.
        dim: Dim,
        /// Whether this declaration introduces a base rather than an alias.
        is_base: bool,
    },
}

/// One statically resolved `Rssn.<op>` call.
///
/// The checker picks the overload; the runtime must execute this key and never
/// resolve again, or a program would run a different overload than it was
/// typed against.
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct RssnCall {
    /// Where the call is written.
    pub span: Span,
    /// The operator name.
    pub name: String,
    /// The resolved overload key.
    pub key: aevia_rssn::OpKey,
    /// The index of the overload in the manifest, for diagnostics.
    pub overload: usize,
}

/// What a class-method call site should execute (round-2 decision 1c).
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum DictTarget {
    /// The exact instance the checker proved, identified symbolically.
    Instance(InstanceKey),
    /// A builtin structural operation. `Show` is a *guarantee* for the heads
    /// §3.8.4 lists, so a record (or `Dyn`, etc.) is printed by the builtin
    /// printer when no user instance matches (round-2 decision 1e).
    Builtin(BuiltinDispatch),
    /// A quantified call site: the runtime dispatches among these instances by
    /// value witness. The checker proved their witnesses pairwise disjoint.
    Witness {
        /// The candidate instances, in registration order.
        candidates: Vec<InstanceKey>,
        /// The heads of the candidates that are `derive`d. A derived instance
        /// has no method bodies — the runtime's structural implementation is
        /// the instance (§3.8.5) — so a witness that matches one of these
        /// heads runs the builtin.
        derived: Vec<(InstanceKey, Type)>,
    },
}

/// A builtin operation the runtime already implements structurally.
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum BuiltinDispatch {
    /// The structural `Show` printer (records, `Dyn`, unit values, …).
    ShowStructural,
    /// A `derive`d instance (§3.8.5): the structural builtin of the method's
    /// name is its implementation.
    Derived,
}

/// One statically resolved class-method call.
///
/// `aevia-core` must dispatch `method` on `head` to the body of the instance
/// this record names (`target`), instead of to a global of the same name.
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct DictionaryUse {
    /// Where the call is written.
    pub span: Span,
    /// The class name as written.
    pub class: String,
    /// The instance head the checker proved.
    pub head: Type,
    /// The method that was called.
    pub method: String,
    /// What to execute at this site.
    pub target: DictTarget,
}

/// A method used as a value, resolved at its defining source reference.
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct ClassMethodRef {
    /// Full nominal class identity, independent of its visible spelling.
    pub class: ClassKey,
    /// The method declared by that class.
    pub method: String,
}

/// One explicitly ordered generic binder on a source function.
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum GenericBinder {
    Type(Type),
    Representation(Rep),
    Dimension(Dim),
    Natural(Nat),
    Row(Row),
}

impl GenericBinder {
    pub fn as_type(&self) -> Type {
        match self {
            | Self::Type(ty) => ty.clone(),
            | Self::Representation(rep) => Type::Scalar(*rep, Dim::one()),
            | Self::Dimension(dim) => Type::UnitOf(dim.clone()),
            | Self::Natural(nat) => {
                Type::tensor(Shape::Refined(vec![Some(nat.clone())]), Type::Unit)
            },
            | Self::Row(row) => Type::Record(row.clone()),
        }
    }

    pub fn substituted(
        &self,
        bindings: &crate::TypeSubstitution,
    ) -> Self {
        match self {
            | Self::Type(ty) => Self::Type(bindings.apply(ty)),
            | Self::Representation(rep) => Self::Representation(bindings.rep(*rep)),
            | Self::Dimension(dim) => Self::Dimension(bindings.dim(dim)),
            | Self::Natural(nat) => Self::Natural(nat.substitute(&bindings.naturals)),
            | Self::Row(row) => Self::Row(bindings.row(row)),
        }
    }

    pub fn resolved(
        &self,
        infer: &Infer,
    ) -> Self {
        match self {
            | Self::Type(ty) => Self::Type(infer.resolve(ty)),
            | Self::Representation(rep) => Self::Representation(infer.resolve_rep(*rep)),
            | Self::Dimension(dim) => Self::Dimension(infer.resolve_dim(dim)),
            | Self::Natural(nat) => Self::Natural(infer.resolve_nat(nat)),
            | Self::Row(row) => Self::Row(infer.resolve_row(row)),
        }
    }
}

/// The lexical boundary exited by a source `?` operation.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub enum TryBoundary {
    /// A real source function/lambda (never an implementation thunk).
    Function(Span),
    /// An effectful do program whose result is Option/Result.
    Do(Span),
}
/// The checked destination and result container of one source `?`.
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct TryTarget {
    pub boundary: TryBoundary,
    pub result: Type,
}

/// The checked signature that belongs to the actual source function body.
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct FunctionEvidence {
    /// Source return boundary, retained independently of callable identity.
    pub site: Span,
    /// Stable declaration syntax plus its resolved creation contract.
    pub identity: CallableId,
    pub scheme: Scheme,
    pub binders: Vec<GenericBinder>,
    pub reifications: Vec<Type>,
    /// Creation-site numeric constraints that must hold for a concrete activation.
    pub numeric_literals: Vec<(Span, LiteralKind, Type)>,
}

/// Reduces `vector` against an echelon `basis` of exponent vectors; `None`
/// when it is a rational combination of the basis (so not independent).
fn reduce_against(
    basis: &[(Vec<(u32, BigRational)>, String)],
    mut vector: Vec<(u32, BigRational)>,
) -> Option<Vec<(u32, BigRational)>> {
    use num_traits::Zero;
    for (row, _) in basis {
        let Some((pivot, pivot_value)) = row.first() else {
            continue;
        };
        let Some(own) = vector
            .iter()
            .find(|(id, _)| id == pivot)
            .map(|(_, value)| value.clone())
        else {
            continue;
        };
        let factor = own / pivot_value;
        for (id, value) in row {
            match vector.iter_mut().find(|(other, _)| other == id) {
                | Some((_, existing)) => *existing -= &factor * value,
                | None => vector.push((*id, -(&factor * value))),
            }
        }
        vector.retain(|(_, value)| !value.is_zero());
    }
    // Each row was reduced against every earlier row when it was inserted, so
    // it contains none of their pivots; eliminating in insertion order never
    // reintroduces a pivot already removed. A nonzero remainder is therefore
    // independent of the span, and its smallest variable is its pivot.
    vector.sort_by_key(|(id, _)| *id);
    (!vector.is_empty()).then_some(vector)
}

fn quantify_binders(
    scheme: &mut Scheme,
    binders: &[GenericBinder],
    infer: &Infer,
) {
    let mut variables = crate::TypeSubstitution::default();
    let mut fresh = Infer::new();
    for binder in binders {
        variables.freshen(&binder.as_type(), &mut fresh);
    }
    fn append(
        target: &mut Vec<u32>,
        keys: impl Iterator<Item = u32>,
    ) {
        let mut keys: Vec<_> = keys.collect();
        keys.sort_unstable();
        keys.dedup();
        for id in keys {
            if !target.contains(&id) {
                target.push(id);
            }
        }
    }
    append(
        &mut scheme.vars,
        variables
            .types
            .keys()
            .chain(variables.representations.keys())
            .copied(),
    );
    append(&mut scheme.dims, variables.dimensions.keys().copied());
    append(&mut scheme.rows, variables.rows.keys().copied());
    append(&mut scheme.nats, variables.naturals.keys().copied());
    infer.attach_numeric_scheme(scheme);
}

fn inferred_binders(scheme: &Scheme) -> Vec<GenericBinder> {
    let mut variables = crate::TypeSubstitution::default();
    variables.freshen(&scheme.ty, &mut Infer::new());
    let mut inputs = crate::TypeSubstitution::default();
    if let Type::Fn(args, _) = &scheme.ty {
        for arg in args {
            inputs.freshen(arg, &mut Infer::new());
        }
    }
    let computed: HashSet<u32> = scheme
        .numeric
        .iter()
        .filter_map(|constraint| {
            match constraint {
                | crate::NumericConstraint::Relation(
                    crate::RepRelation::Join {
                        result: Rep::Var(id), ..
                    }
                    | crate::RepRelation::Divide {
                        result: Rep::Var(id), ..
                    },
                ) => Some(*id),
                | crate::NumericConstraint::DefaultLiteral {
                    ty: Type::Scalar(Rep::Var(id), _),
                    ..
                } => Some(*id),
                | _ => None,
            }
        })
        .collect();
    scheme
        .vars
        .iter()
        .filter(|id| {
            (variables.types.contains_key(id) || variables.representations.contains_key(id))
                && (!computed.contains(id) || inputs.representations.contains_key(id))
        })
        .map(|id| {
            if variables.representations.contains_key(id) {
                GenericBinder::Representation(Rep::Var(*id))
            } else {
                GenericBinder::Type(Type::Var(*id))
            }
        })
        .chain(
            scheme
                .dims
                .iter()
                .map(|id| GenericBinder::Dimension(Dim::var(*id))),
        )
        .chain(
            scheme
                .nats
                .iter()
                .map(|id| GenericBinder::Natural(Nat::Var(*id))),
        )
        .chain(
            scheme
                .rows
                .iter()
                .map(|id| GenericBinder::Row(Row::Var(*id))),
        )
        .collect()
}

fn constructor_binders(scheme: &Scheme) -> Vec<GenericBinder> {
    let result = match &scheme.ty {
        | Type::Fn(_, result) => result.as_ref(),
        | ty => ty,
    };
    let Type::Adt(id, args) = result else {
        return inferred_binders(scheme);
    };
    adt::param_kinds_of(*id)
        .iter()
        .zip(args)
        .filter_map(|(kind, arg)| {
            match (kind, arg) {
                | (adt::ParamKind::Type, _) => Some(GenericBinder::Type(arg.clone())),
                | (adt::ParamKind::Dim(_), Type::UnitOf(dim)) => {
                    Some(GenericBinder::Dimension(dim.clone()))
                },
                | _ => None,
            }
        })
        .collect()
}

/// One class exported with a module's interface (§7.1.3/§7.4).
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct ClassExport {
    /// The symbolic identity.
    pub key: ClassKey,
    /// The number of parameters (always one in this language).
    pub params: usize,
    /// The method signatures, in [`Type::Param`] form.
    pub methods: Vec<(String, Type)>,
}

/// One instance exported with a module's interface.
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct InstanceExport {
    /// The symbolic identity: module + class key + canonical head.
    pub key: InstanceKey,
    /// The head type, for re-registration and diagnostics.
    pub head: Type,
    /// The specialised method types (`a` substituted by the head).
    #[serde(with = "crate::persist::sorted_map")]
    pub methods: HashMap<String, Type>,
    /// True when it came from `derive`; the runtime needs this to know the
    /// instance body is the builtin structural operation, not a user body.
    pub derived: bool,
}

/// The kind of one parameter of a generic declaration (§2: the annotation may
/// be omitted and the kind then inferred from the body).
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum AliasParamKind {
    /// A type parameter (`type Pair<a, b>`).
    Type,
    /// A dimension parameter (`type Normal<d>` with `Float[d]` fields).
    Dim,
    /// A type-level natural-number parameter used as a shape extent.
    Nat,
    /// A row parameter used as a record tail (`{ x: Int | r }`).
    Row,
}

/// A transparent type alias, with its definition, for the interface.
///
/// `Export::Alias` carries only the arity today; an importer cannot expand an
/// alias without the body, which is why `type Vec2` was `E0301` from another
/// module (§7.4). This is the definition the interface carries.
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct AliasExport {
    /// The number of parameters.
    pub params: usize,
    /// The expansion, with [`Type::Param`]/dimension/nat/row placeholders.
    pub body: Type,
    /// The kind of each parameter, in declaration order.
    pub kinds: Vec<AliasParamKind>,
}

/// Everything a consumer of a checked module needs, including the class and
/// instance data §7.1.3/§7.4 require the interface to carry.
///
/// The flat [`CheckedModule::exports`] table remains for the current driver and
/// editor; this is the complete interface and the thing an importer merges.
#[derive(Clone, Debug, Default, serde::Serialize, serde::Deserialize)]
pub struct ModuleInterface {
    /// The module path.
    pub name: ModulePath,
    /// The flat name table.
    pub exports: IndexMap<String, Export>,
    /// Classes declared (and exported) by the module.
    pub classes: IndexMap<ClassKey, ClassExport>,
    /// Instances declared by the module.
    pub instances: Vec<InstanceExport>,
    /// Aliases declared by the module, with their definitions.
    pub aliases: IndexMap<String, AliasExport>,
    /// Ordered function binders, in the corresponding exported scheme's universe.
    pub function_headers: IndexMap<String, Vec<GenericBinder>>,
    /// Exact source origins and binder projections for visible callable names.
    pub callable_exports: IndexMap<String, CallableProjection>,
}

/// The two type-directed `Dyn` primitives of §3.11/§4.9.4.
#[derive(Clone, Copy, PartialEq, Eq, Debug, serde::Serialize, serde::Deserialize)]
pub enum DynPrimitive {
    /// `dyn(x)`: the static dimension `d` is materialised into the value.
    Dyn,
    /// `check(x)`: the static dimension `d` is materialised and compared with
    /// the value's runtime dimension; a mismatch is `E0412`.
    Check,
}

/// One `dyn`/`check` call site with the concrete dimension the checker
/// materialised for it (§4.9.4).
///
/// `dyn`/`check` are type-directed: the static `d` never reaches the runtime in
/// the argument list, so the checker records it here and the evaluator looks it
/// up by span. For `dyn` the payload already carries the dimension (the
/// materialisation is redundant but harmless); for `check` it is the answer the
/// runtime must compare against — without it `check` cannot fail and `E0412`
/// is unreachable.
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct DynCall {
    /// Where the call is written.
    pub span: Span,
    /// Which primitive it is.
    pub primitive: DynPrimitive,
    /// The concrete dimension `d` the checker materialised.
    pub dim: Dim,
}

/// The stability marker of a declaration (§13.8.1).
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum Stability {
    /// Stable within a major version.
    Stable,
    /// May change; the old name survives at least one minor.
    Experimental,
    /// Has a deprecation warning with this message.
    Deprecated(String),
}

/// Which host extension a [`HostRegistration`] is (§13.5.2).
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum HostRegistrationKind {
    /// `@op("name")`: an Aevia function exposed as an RSSN operator.
    Op,
    /// `@rule("name")`: an Aevia function exposed as a rewrite rule.
    Rule,
}

/// One accepted `@op`/`@rule` registration.
///
/// The checker proves the declaration and names the operator; publishing it to
/// the kernel is a runtime/bridge step (the Aevia function is a host callback).
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct HostRegistration {
    /// Where the attribute is written.
    pub span: Span,
    /// Operator or rule.
    pub kind: HostRegistrationKind,
    /// The name written in the attribute.
    pub name: String,
    /// The Aevia function that implements it.
    pub function: String,
    /// The function's parameters: their names and, once checked, the
    /// dimension of each (`Dim::one()` for a non-scalar or unresolved one).
    /// The runtime traces the function on symbols of these dimensions.
    pub params: Vec<(String, Dim)>,
    /// The dimension of the function's result once checked (`Dim::one()`
    /// for a non-scalar or unresolved one). With [`Self::params`] it is the
    /// operator's dimension signature: dimension variables of a polymorphic
    /// function stay variables, which the kernel solves at each use.
    pub result: Dim,
}

/// The interface of a checked module.
#[derive(Clone, Debug, Default, serde::Serialize, serde::Deserialize)]
pub struct CheckedModule {
    /// The module path, when declared.
    pub name: Option<Vec<String>>,
    /// Nominal identity supplied by the module loader; visible names remain in
    /// `name`, while source origin distinguishes same-named package modules.
    pub identity: Option<ModulePath>,
    /// Every locally declared constructor, including private types. Runtime
    /// initialization pins the exact registry witness from this compilation.
    #[serde(with = "crate::persist::adt_pair_map")]
    pub constructors: IndexMap<String, (adt::AdtId, usize)>,
    /// Checked source constructor expressions and patterns, excluding value aliases.
    #[serde(with = "crate::persist::sorted_adt_pair_map")]
    pub constructor_refs: HashMap<Span, (adt::AdtId, usize)>,
    /// Function signatures by declaration/lambda site, retaining body variables.
    #[serde(with = "crate::persist::sorted_map")]
    pub functions: HashMap<Span, FunctionEvidence>,
    pub function_headers: IndexMap<String, Vec<GenericBinder>>,
    /// Exact source origins and binder projections for visible callable names.
    pub callable_exports: IndexMap<String, CallableProjection>,
    #[serde(with = "crate::persist::sorted_map")]
    pub generic_refs: HashMap<Span, Vec<GenericBinder>>,
    /// Source-authenticated selections in the actual declaration's order.
    #[serde(with = "crate::persist::sorted_map")]
    pub callable_refs: HashMap<Span, CallableProjection>,
    /// Exact lexical destinations for `?`, including deferred IO boundaries.
    #[serde(with = "crate::persist::sorted_map")]
    pub try_targets: HashMap<Span, TryTarget>,
    /// The file this module came from, used to build a deterministic module
    /// path when there is no `module` declaration (`<fileN>`).
    pub anonymous_file: FileId,
    /// Exported names.
    pub exports: IndexMap<String, Export>,
    /// The unit table after user declarations.
    #[serde(with = "crate::persist::default_units")]
    pub units: UnitTable,
    /// The dimension environment after user declarations.
    #[serde(with = "crate::persist::default_dims")]
    pub dims: DimEnv,
    /// Names of `test` declarations, in order.
    pub tests: Vec<String>,
    /// `Rssn.<op>` calls with the overload the checker resolved.
    pub rssn_calls: Vec<RssnCall>,
    /// `dyn`/`check` call sites with the concrete dimension the checker
    /// materialised for them (§4.9.4).
    pub dyn_calls: Vec<DynCall>,
    /// `to_system`/`from_system` call sites (§5.11.5): the system and the SI
    /// dimension of the quantity on the SI side of the conversion.
    #[serde(with = "crate::persist::sorted_map")]
    pub system_calls: HashMap<Span, (String, Dim)>,
    /// The stability marker of each declared name (§13.8.1).
    pub stability: IndexMap<String, Stability>,
    /// `@op`/`@rule` registrations the checker accepted (§13.5.2).
    pub registrations: Vec<HostRegistration>,
    /// Class-method calls with the instance the checker proved.
    pub dictionaries: Vec<DictionaryUse>,
    /// Classes this module declares and exports (§7.1.3/§7.4).
    pub classes: IndexMap<ClassKey, ClassExport>,
    /// All classes declared locally, including private runtime declarations.
    pub local_classes: IndexMap<ClassKey, ClassExport>,
    /// Exact runtime instance declarations keyed by their source site.
    #[serde(with = "crate::persist::sorted_map")]
    pub instance_sites: HashMap<Span, InstanceExport>,
    /// First-class method references with their resolved nominal identity.
    #[serde(with = "crate::persist::sorted_map")]
    pub method_refs: HashMap<Span, ClassMethodRef>,
    /// Instances this module declares, with the registry identity the runtime
    /// needs (round-2 decision 1b).
    pub instances: Vec<InstanceExport>,
    /// Transparent type aliases with their definitions, for importers (§7.4).
    pub alias_defs: IndexMap<String, AliasExport>,
    /// Transparent type aliases, by name.
    ///
    /// A record alias has no nominal tag at run time, so an instance head
    /// spelled `Vec2` and the checker's record `{ x: Float, y: Float }` are
    /// the same head written two ways. The runtime needs the definition to
    /// match them; without it an instance on an alias is unreachable.
    pub aliases: IndexMap<String, Type>,
    /// The instantiated representation of every numeric literal, keyed by the
    /// literal's span.
    ///
    /// A literal is representation-polymorphic (§3.10): `1` is `Int` by default
    /// but `Float` where a `Float` is expected, and `1.5` is `Float` or exact
    /// `Real`. That choice is static, so before this table existed a literal in
    /// a `Float` context still evaluated to an exact `Value`, and a statically
    /// `Float` expression could hold an exact `Real` — the static type and the
    /// runtime representation disagreed. The IR lowering and the evaluator must
    /// materialise each literal at the representation recorded here, which is
    /// the explicit coercion at the boundary the type says is there.
    #[serde(with = "crate::persist::sorted_map")]
    pub literal_reps: HashMap<Span, Rep>,
    /// Constant regions (§3.10.4): the root of every closed literal
    /// arithmetic tree whose static representation is inexact (`Float`,
    /// `Complex`, or a representation variable resolved per activation).
    ///
    /// The runtime evaluates such a tree with exact leaves and rounds the
    /// result once into the recorded representation, so `0.1 * 3 : Float` is
    /// the nearest `f64` to `3/10` and the static type still agrees with the
    /// runtime value. A tree whose exact evaluation fails (a division by
    /// zero) is evaluated step by step in its static representation instead.
    #[serde(with = "crate::persist::sorted_map")]
    pub constant_regions: HashMap<Span, Rep>,
    /// The module's unit system (§5.11), when it declares one.
    pub unit_system: Option<String>,
    /// The resolved type of each expression, keyed by its span.
    ///
    /// Every expression the checker visits has an entry: the type `synth`
    /// produced for it, or the type `check` checked it against. A `let` or
    /// parameter binder has an entry too, at the binder's own span, because
    /// that is the same type its uses get and a hover on the binder is the
    /// same question. The entries are recorded with unification variables
    /// intact and resolved once, at the end of the module, after defaulting —
    /// so an entry never names a variable that a later step bound to
    /// something else.
    ///
    /// This is what lets a language server answer "what is the type of *this*
    /// expression". [`Self::exports`] answers the same question for a name.
    #[serde(with = "crate::persist::sorted_map")]
    pub expr_types: HashMap<Span, Type>,
    /// Number of errors.
    pub errors: usize,
}

impl CheckedModule {
    /// The complete interface of §7.1.3/§7.4: names **and** the classes,
    /// instances and alias definitions an importer needs.
    ///
    /// A driver that only needs the flat name table keeps using `exports`; the
    /// interface is the unit of cross-module reuse (round-2 decision 1b).
    #[must_use]
    pub fn interface(&self) -> ModuleInterface {
        ModuleInterface {
            name: self.module_path(),
            exports: self.exports.clone(),
            classes: self.classes.clone(),
            instances: self.instances.clone(),
            aliases: self.alias_defs.clone(),
            function_headers: self.function_headers.clone(),
            callable_exports: self.callable_exports.clone(),
        }
    }

    /// The module path, or a deterministic anonymous path for a file with no
    /// `module` declaration.
    #[must_use]
    pub fn module_path(&self) -> ModulePath {
        if let Some(identity) = &self.identity {
            return identity.clone();
        }
        match &self.name {
            | Some(path) => ModulePath::new(path.iter().cloned()),
            | None => ModulePath::new([format!("<file{}>", self.anonymous_file)]),
        }
    }
}

/// The capability names a build provides (§13.8.2).
///
/// The checker needs them for the diagnostics that are *static capability
/// gaps*: `@jit` without `core.jit` is `E1001` at check time (§9.6.1), and the
/// host-extension attributes need the plugin ABI. The set is a plain list of
/// names so `aevia-types` stays independent of the manifest's shape; a caller
/// fills it from the kernel's own table ([`CheckOptions::default`] does).
#[derive(Clone, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Capabilities {
    names: std::collections::BTreeSet<String>,
}

impl Capabilities {
    /// A set from dotted capability names such as `core.jit`.
    #[must_use]
    pub fn from_names<I, S>(names: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        Self {
            names: names.into_iter().map(Into::into).collect(),
        }
    }

    /// Every capability, for a caller that does not want the gates to fire.
    ///
    /// This is not the default: a build's real set is what
    /// [`CheckOptions::default`] reads, so a missing feature cannot be silently
    /// assumed present.
    #[must_use]
    pub fn all() -> Self {
        const KNOWN: &[&str] = &[
            "core.dynamic_call",
            "core.jit",
            "core.jit_branch",
            "core.jit_index",
            "core.parallel_symbolic",
            "core.higher_order",
            "core.structured_results",
            "core.plugin",
            "core.plugin.source",
            "core.callback",
            "dim.exact_units",
            "dim.affine_units",
            "dim.user_base_dimensions",
        ];
        Self::from_names(KNOWN.iter().copied())
    }

    /// Adds one capability name.
    pub fn insert(
        &mut self,
        name: impl Into<String>,
    ) {
        self.names.insert(name.into());
    }

    /// Whether this build provides the named capability.
    ///
    /// An unknown name is *not* provided: a diagnostic that asks about a
    /// capability must say "missing" rather than assume it exists.
    #[must_use]
    pub fn has(
        &self,
        name: &str,
    ) -> bool {
        self.names.contains(name)
    }

    /// The names this build provides, sorted for determinism.
    #[must_use]
    pub fn names(&self) -> Vec<String> {
        let mut names: Vec<String> = self.names.iter().cloned().collect();
        names.sort();
        names
    }
}

/// Options for one check.
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct CheckOptions {
    /// The backend capabilities to check against (§13.8.2).
    pub capabilities: Capabilities,
}

impl Default for CheckOptions {
    fn default() -> Self {
        // The kernel's manifest is the single capability table (§13.8.2), so
        // the default reads it rather than keeping a second list. Existing
        // callers (`check_module`, `check_module_with_imports`) therefore gate
        // `@jit` on the build they are actually linked against.
        Self {
            capabilities: Capabilities::from_names(aevia_rssn::registry().capabilities.names()),
        }
    }
}

/// The result of checking a file.
pub struct CheckResult {
    /// Diagnostics, in source order.
    pub diagnostics: Diagnostics,
    /// The interface, even when there were errors (partial).
    pub module: CheckedModule,
}

/// Checks one parsed module.
#[must_use]
pub fn check_module(
    file: FileId,
    module: &Module,
) -> CheckResult {
    check_module_with_imports(file, module, &[])
}

/// An imported module: the prefix its names are reachable under, and its
/// exports.
pub type Import = (String, IndexMap<String, Export>);

/// Lexes nothing, checks one module with the names of its imports in scope.
///
/// `imports` holds one entry per `import`/`open` declaration that resolved. A
/// qualified import contributes `prefix.name`; an `open` contributes the bare
/// name as well, which is why the caller decides the prefix.
pub fn check_module_with_imports(
    file: FileId,
    module: &Module,
    imports: &[Import],
) -> CheckResult {
    check_module_with_options(file, module, imports, &CheckOptions::default())
}

/// Checks one module with the names of its imports **and** the full interfaces
/// of those modules in scope (§7.1.3/§7.4).
///
/// `imports` remains the flat name table the current driver builds; the
/// interfaces add the classes, instances and alias definitions a module needs
/// to be reused. `check_module_with_imports` delegates here with no
/// interfaces, so a driver that has not adopted the interface keeps working.
pub fn check_module_with_interfaces(
    file: FileId,
    module: &Module,
    imports: &[Import],
    interfaces: &[ModuleInterface],
    options: &CheckOptions,
) -> CheckResult {
    check_module_with_identity(file, module, imports, interfaces, options, None, &[])
}

/// The visible binding of one direct imported interface. Nominal identities
/// remain on its types and keys; `prefix` is only a source lookup name.
#[derive(Debug, serde::Serialize)]
pub struct InterfaceBinding {
    pub module: ModulePath,
    pub prefix: String,
    pub names: Option<Vec<String>>,
}

/// Checks a module under a loader-supplied nominal source identity. A package
/// alias changes visible names without changing ADT, class or instance keys.
pub fn check_module_with_identity(
    file: FileId,
    module: &Module,
    imports: &[Import],
    interfaces: &[ModuleInterface],
    options: &CheckOptions,
    identity: Option<ModulePath>,
    bindings: &[InterfaceBinding],
) -> CheckResult {
    let mut checker = Checker::new(file, options.clone());
    checker.checked.identity = identity;
    checker.add_imports(imports);
    for interface in interfaces {
        let explicit_bindings = !bindings.is_empty();
        let direct = bindings
            .iter()
            .any(|binding| binding.module == interface.name);
        checker.merge_interface(interface, !explicit_bindings, !explicit_bindings || direct);
    }
    for binding in bindings {
        if let Some(interface) = interfaces
            .iter()
            .find(|interface| interface.name == binding.module)
        {
            checker.bind_interface(interface, binding);
        }
    }
    checker.run(module);
    checker.checked.aliases = checker
        .aliases
        .iter()
        .map(|(name, def)| (name.clone(), def.ty.clone()))
        .collect();
    CheckResult {
        diagnostics: checker.diagnostics,
        module: checker.checked,
    }
}

// Import qualification is display metadata only. Keep every binder and every
// numeric requirement in the original universe while giving concrete owned
// generators readable local names in diagnostics.
fn import_dimension_display(
    scheme: &Scheme,
    names: &HashMap<aevia_dim::NominalBase, String>,
) -> Scheme {
    fn row(
        value: &Row,
        names: &HashMap<aevia_dim::NominalBase, String>,
    ) -> Row {
        match value {
            | Row::Extend { label, ty, tail } => {
                Row::Extend {
                    label: label.clone(),
                    ty: Box::new(ty_map(ty, names)),
                    tail: Box::new(row(tail, names)),
                }
            },
            | _ => value.clone(),
        }
    }
    fn ty_map(
        value: &Type,
        names: &HashMap<aevia_dim::NominalBase, String>,
    ) -> Type {
        match value {
            | Type::Scalar(rep, dim) => Type::Scalar(*rep, dim.with_display_names(names)),
            | Type::Point(dim) => Type::Point(dim.with_display_names(names)),
            | Type::UnitOf(dim) => Type::UnitOf(dim.with_display_names(names)),
            | Type::Tensor { shape, elem } => Type::tensor(shape.clone(), ty_map(elem, names)),
            | Type::Fn(args, ret) => {
                Type::func(
                    args.iter().map(|arg| ty_map(arg, names)).collect(),
                    ty_map(ret, names),
                )
            },
            | Type::Record(value) => Type::Record(row(value, names)),
            | Type::Adt(id, args) => {
                Type::Adt(*id, args.iter().map(|arg| ty_map(arg, names)).collect())
            },
            | Type::Io(value) => Type::Io(Box::new(ty_map(value, names))),
            | Type::TypeRepIntrinsic(value) => {
                Type::TypeRepIntrinsic(value.as_ref().map(|ty| Box::new(ty_map(ty, names))))
            },
            | Type::FromValueIntrinsic(value) => {
                Type::FromValueIntrinsic(value.as_ref().map(|ty| Box::new(ty_map(ty, names))))
            },
            | _ => value.clone(),
        }
    }
    let mut mapped = scheme.clone();
    mapped.ty = ty_map(&scheme.ty, names);
    mapped.materializations = scheme
        .materializations
        .iter()
        .map(|ty| ty_map(ty, names))
        .collect();
    for constraint in &mut mapped.constraints {
        for argument in &mut constraint.args {
            *argument = ty_map(argument, names);
        }
    }
    for constraint in &mut mapped.numeric {
        match constraint {
            | crate::NumericConstraint::Literal { ty, .. }
            | crate::NumericConstraint::DefaultLiteral { ty, .. } => *ty = ty_map(ty, names),
            | crate::NumericConstraint::Relation(_) => {},
        }
    }
    mapped
}

// Prove identity forwarding from lexical binding structure, never a function
// name or Fn signature. Unsupported computations deliberately have no summary.
fn forwarded_parameter(
    args: &[Param],
    body: &Expr,
) -> Option<usize> {
    fn hide(
        pattern: &Pat,
        names: &mut HashMap<String, usize>,
    ) {
        let mut pending = vec![pattern];
        while let Some(pattern) = pending.pop() {
            match &pattern.kind {
                | PatKind::Var(name) => {
                    names.remove(name);
                },
                | PatKind::Ctor { args, .. } | PatKind::Tuple(args) => pending.extend(args),
                | PatKind::Record(fields) => {
                    for (name, pattern) in fields {
                        if let Some(pattern) = pattern {
                            pending.push(pattern);
                        } else {
                            names.remove(name);
                        }
                    }
                },
                | PatKind::Tensor { items, rest } => {
                    pending.extend(items);
                    if let Some(name) = rest {
                        names.remove(name);
                    }
                },
                | PatKind::As { pattern, name } => {
                    names.remove(name);
                    pending.push(pattern);
                },
                | PatKind::Annot { pattern, .. } => pending.push(pattern),
                | _ => {},
            }
        }
    }
    fn bind(
        pattern: &Pat,
        value: Option<usize>,
        names: &mut HashMap<String, usize>,
    ) {
        hide(pattern, names);
        if let (PatKind::Var(name), Some(index)) = (&pattern.kind, value) {
            names.insert(name.clone(), index);
        }
    }
    fn walk(
        expr: &Expr,
        names: &HashMap<String, usize>,
        depth: usize,
    ) -> Option<usize> {
        if depth > 128 {
            return None;
        }
        match &expr.kind {
            | ExprKind::Var(name) => names.get(name).copied(),
            | ExprKind::Annot { expr, .. } => walk(expr, names, depth + 1),
            | ExprKind::LetIn {
                pattern, value, body, ..
            } => {
                let value = walk(value, names, depth + 1);
                let mut inner = names.clone();
                bind(pattern, value, &mut inner);
                walk(body, &inner, depth + 1)
            },
            | ExprKind::Block { stmts, tail } => {
                let mut inner = names.clone();
                for statement in stmts {
                    match statement {
                        | BlockStmt::Let {
                            pattern, value, rec, ..
                        } => {
                            let value = if *rec {
                                None
                            } else {
                                walk(value, &inner, depth + 1)
                            };
                            bind(pattern, value, &mut inner);
                        },
                        | BlockStmt::Fn(decl) => {
                            if let DeclKind::Fn { name, .. } = &decl.kind {
                                inner.remove(name);
                            }
                        },
                    }
                }
                walk(tail.as_deref()?, &inner, depth + 1)
            },
            | ExprKind::If {
                then_branch,
                else_branch,
                ..
            } => {
                let first = walk(then_branch, names, depth + 1)?;
                (walk(else_branch, names, depth + 1) == Some(first)).then_some(first)
            },
            | ExprKind::Match { arms, .. } => {
                let mut result = None;
                for arm in arms {
                    let mut inner = names.clone();
                    hide(&arm.pattern, &mut inner);
                    let current = walk(&arm.body, &inner, depth + 1)?;
                    if result.is_some_and(|previous| previous != current) {
                        return None;
                    }
                    result = Some(current);
                }
                result
            },
            | _ => None,
        }
    }
    let names = args
        .iter()
        .enumerate()
        .filter_map(|(index, arg)| {
            if let PatKind::Var(name) = &arg.pattern.kind {
                Some((name.clone(), index))
            } else {
                None
            }
        })
        .collect();
    walk(body, &names, 0)
}

/// Checks one module against an explicit capability set.
///
/// This is the entry point a consumer that owns the manifest registry should
/// use: it can pass the capabilities of the exact backend it will run on,
/// instead of the ones the compiler was linked against.
pub fn check_module_with_options(
    file: FileId,
    module: &Module,
    imports: &[Import],
    options: &CheckOptions,
) -> CheckResult {
    let mut checker = Checker::new(file, options.clone());
    checker.add_imports(imports);
    checker.run(module);
    checker.checked.aliases = checker
        .aliases
        .iter()
        .map(|(name, def)| (name.clone(), def.ty.clone()))
        .collect();
    CheckResult {
        diagnostics: checker.diagnostics,
        module: checker.checked,
    }
}

/// The `<...>` binders of one top-level function, captured in pass B so a type
/// annotation in the body can name them (§3.5.1).
#[derive(Clone, Default)]
struct FnGenerics {
    types: Vec<(String, Type)>,
    dims: Vec<(String, Dim)>,
    nats: Vec<(String, Nat)>,
    rows: Vec<(String, Row)>,
    binders: Vec<GenericBinder>,
}

/// The names a declaration body uses in each kind position (§2 kind inference).
#[derive(Default)]
struct ParamUsage {
    dims: HashSet<String>,
    nats: HashSet<String>,
    rows: HashSet<String>,
    types: HashSet<String>,
}

impl ParamUsage {
    fn scan(
        &mut self,
        ty: &Ty,
    ) {
        match &ty.kind {
            | TyKind::Real(dim) | TyKind::Float(dim) | TyKind::Complex(dim) => self.scan_dim(dim),
            | TyKind::Point(dim) | TyKind::UnitOf(dim) => self.scan_dim(dim),
            | TyKind::Tensor { shape, elem } | TyKind::Sparse { shape, elem, .. } => {
                self.scan_shape(shape);
                self.scan(elem);
            },
            | TyKind::Matrix { rows, cols, elem } => {
                self.scan_nat(rows);
                self.scan_nat(cols);
                self.scan(elem);
            },
            | TyKind::ShapeArg(shape) => self.scan_shape(shape),
            | TyKind::NatArg(nat) => self.scan_nat(nat),
            | TyKind::DimArg(dim) => self.scan_dim(dim),
            | TyKind::SymFun(domain, codomain) => {
                self.scan_dim(domain);
                self.scan_dim(codomain);
            },
            | TyKind::Record { fields, rest } => {
                for (_, field) in fields {
                    self.scan(field);
                }
                if let Some(rest) = rest {
                    self.rows.insert(rest.clone());
                }
            },
            | TyKind::Tuple(items) | TyKind::Row(items) => {
                for item in items {
                    self.scan(item);
                }
            },
            | TyKind::Named { path, args } => {
                if args.is_empty()
                    && let Some(name) = path.last()
                {
                    self.types.insert(name.clone());
                }
                for arg in args {
                    self.scan(arg);
                }
            },
            | TyKind::Option(inner)
            | TyKind::Set(inner)
            | TyKind::Table(inner)
            | TyKind::RateOf(inner)
            | TyKind::DiffusionOf(inner)
            | TyKind::Io(inner) => self.scan(inner),
            | TyKind::Result(ok, error) => {
                self.scan(ok);
                self.scan(error);
            },
            | TyKind::Fn(params, ret) => {
                for param in params {
                    self.scan(param);
                }
                self.scan(ret);
            },
            | TyKind::Forall(_, body) => self.scan(body),
            | TyKind::Var(name) => {
                // A bare type-variable spelling (`a`) names the parameter.
                self.types.insert(name.clone());
            },
            | TyKind::Int
            | TyKind::Bool
            | TyKind::Prop
            | TyKind::Str
            | TyKind::Unit
            | TyKind::Dyn(_) => {},
        }
    }

    fn scan_dim(
        &mut self,
        dim: &DimAst,
    ) {
        match dim {
            | DimAst::One => {},
            | DimAst::Name(name) => {
                self.dims.insert(name.clone());
            },
            | DimAst::Mul(left, right) | DimAst::Div(left, right) => {
                self.scan_dim(left);
                self.scan_dim(right);
            },
            | DimAst::Pow(base, _) => self.scan_dim(base),
            | DimAst::Paren(inner) => self.scan_dim(inner),
        }
    }

    fn scan_nat(
        &mut self,
        nat: &NatAst,
    ) {
        match nat {
            | NatAst::Lit(_) => {},
            | NatAst::Var(name) => {
                self.nats.insert(name.clone());
            },
            | NatAst::Add(left, right) | NatAst::Mul(left, right) => {
                self.scan_nat(left);
                self.scan_nat(right);
            },
        }
    }

    fn scan_shape(
        &mut self,
        shape: &ShapeAst,
    ) {
        match shape {
            | ShapeAst::Rank(nat) => self.scan_nat(nat),
            | ShapeAst::Refined(items) => {
                for item in items {
                    if let ShapeItem::Known(nat) = item {
                        self.scan_nat(nat);
                    }
                }
            },
        }
    }
}

fn tail_row_id(row: &Row) -> Option<u32> {
    match row {
        | Row::Var(id) => Some(*id),
        | _ => None,
    }
}

/// A bare written name inside a type argument (`L`, `n`, `a`), if any.
///
/// A lowercase name is parsed as a type *variable* (`TyKind::Var`), an
/// uppercase one as a named type; both are just names to kind inference.
fn bare_type_name(ty: &Ty) -> Option<&String> {
    match &ty.kind {
        | TyKind::Named { path, args } if args.is_empty() => path.last(),
        | TyKind::Var(name) => Some(name),
        | _ => None,
    }
}

#[derive(Clone, Copy, Debug, Default)]
struct LiteralClass {
    tree: bool,
    homogeneous: Option<LiteralKind>,
    atomic: bool,
}

struct Checker {
    file: FileId,
    /// The module path, used to qualify nominal keys (§A decision 4).
    module_path: ModulePath,
    /// The capabilities this check runs against (§13.8.2).
    options: CheckOptions,
    /// Declared names with a deprecation message (§13.8.1).
    deprecated: HashMap<String, String>,
    /// Declared names with a stability marker (§13.8.1).
    stability: IndexMap<String, Stability>,
    /// Accepted `@op`/`@rule` declarations (§13.5.2).
    registrations: Vec<HostRegistration>,
    /// Classes declared `private` in this module (§7.1.3): usable locally,
    /// absent from the exported interface.
    private_classes: HashSet<String>,
    instance_declarations: HashMap<Span, InstanceKey>,
    method_bindings: HashMap<String, ClassMethodRef>,
    constructor_bindings: HashMap<String, (adt::AdtId, usize)>,
    infer: Infer,
    globals: HashMap<String, Scheme>,
    fn_signatures: HashMap<String, Type>,
    /// The `<...>` binders of each top-level function, kept so a type
    /// annotation in the body can name them (`fn f<n>(...) = zeros_of<[n]>()`,
    /// §3.5.1). The signature's params are restored at the end of pass B, so
    /// the body pass needs them from here.
    fn_generics: HashMap<String, FnGenerics>,
    function_stack: Vec<Span>,
    function_inputs: HashMap<Span, Vec<Type>>,
    function_reifications: HashMap<Span, Vec<Type>>,
    call_materializations: HashMap<Span, Vec<Type>>,
    generic_bindings: HashMap<String, Vec<GenericBinder>>,
    callable_ids: HashMap<Span, CallableId>,
    global_callables: HashMap<String, CallableProjection>,
    local_callables: HashMap<(usize, String), CallableProjection>,
    local_headers: HashMap<(usize, String), Vec<GenericBinder>>,
    scopes: Vec<HashMap<String, Scheme>>,
    units: UnitTable,
    dims: DimEnv,
    aliases: HashMap<String, AliasDef>,
    adts: HashMap<String, adt::AdtId>,
    type_params: Vec<(String, Type)>,
    dim_params: Vec<(String, Dim)>,
    /// Type-level natural-number parameters (`<n: Nat>` and unkinded
    /// parameters used in a shape, §3.5.1).
    nat_params: Vec<(String, Nat)>,
    /// Row parameters of the declaration being checked (`<r: Row>`, or an
    /// unkinded parameter used as a record tail, §2/§3.6).
    row_params: Vec<(String, Row)>,
    /// The export-name sets of the prefix-less `import`/`open` statements of
    /// this module. Used to tell which interface is the source of an
    /// unqualified name when the flat import table has no module identity.
    unqualified_import_names: Vec<HashSet<String>>,
    /// Every name imported unqualified (§7.2.1/§7.2.2).
    requested_unqualified: HashSet<String>,
    /// The type a top-level `let` was given in pass B, so pass C checks
    /// against the same variable a forward reference sees (§7.1.2).
    top_lets: HashMap<String, Type>,
    /// Names declared by this compilation unit, for the orphan rule.
    local_adts: HashSet<adt::AdtId>,
    classes: ClassTable,
    /// Constraints whose type was still unknown when they were generated.
    deferred_constraints: Vec<(String, Type, Span)>,
    /// True while converting an `instance` head, where an unknown type or
    /// dimension name is an implicitly quantified variable (§3.8.1).
    implicit_params: bool,
    /// The alias being converted, so a self-reference is `E0312` rather than
    /// "unknown type".
    declaring_alias: Option<String>,
    /// Set when a `Real`/`Float` mix forced the exact path (`W0302`).
    mixed_rep: bool,
    /// Concrete expectation for a literal-only arithmetic source tree. It is
    /// scoped to that expression and never crosses a function call boundary.
    literal_context: Option<Rep>,
    literal_classes: HashMap<Span, LiteralClass>,
    literal_region: Option<(LiteralKind, Rep)>,
    /// Roots of closed literal arithmetic trees (§3.10.4 constant regions).
    constant_region_roots: Vec<Span>,
    /// Set once an unsatisfiable numeric relation has been reported.
    numeric_conflict_reported: bool,
    /// While set, dimensions are read in SI even in a module with a unit
    /// system (unit declarations and `from_system<D>` targets, §5.11).
    si_dimensions: bool,
    /// Top-level names that some body actually read, for `W0303`.
    referenced: HashSet<String>,
    /// `check` call sites whose `d` is only known after the whole module has
    /// been checked, because the surrounding expression fixes it.
    pending_checks: Vec<(Span, Dim)>,
    /// Local binders (parameters, `let`/match patterns) and their spans, for
    /// `W0101`; a name in `used_locals` was read somewhere (§B.12).
    local_binders: Vec<(String, Span)>,
    /// Whether the pattern being bound may legitimately fail to match: a
    /// `match` arm falls through to the next arm, while a `let` or parameter
    /// pattern must always match.
    refutable_pattern: bool,
    /// Local names that a body actually read.
    used_locals: HashSet<String>,
    /// Spans of numeric literals, so their instantiated representation can be
    /// exported in [`CheckedModule::literal_reps`].
    literal_spans: Vec<Span>,
    literal_owners: HashMap<Span, (Option<Span>, LiteralKind)>,
    function_literal_sites: HashMap<Span, Vec<(Span, LiteralKind)>>,
    expression_indices: HashMap<Span, Vec<usize>>,
    #[cfg(test)]
    literal_owner_visits: std::cell::Cell<usize>,
    /// `(span, type)` for every expression and binder, in the order the
    /// checker visited them.
    ///
    /// Kept as a vector while checking — a push is cheaper than a hash insert
    /// and a speculative branch (`overloaded_call`, `det`) can truncate it —
    /// and collapsed into [`CheckedModule::expr_types`] at the end, resolved.
    /// A span may be recorded twice (a call argument is both synthesised and
    /// checked); the last record is the authoritative one.
    expr_types: Vec<(Span, Type)>,
    diagnostics: Diagnostics,
    checked: CheckedModule,
    return_type: Option<Type>,
    do_returns: Vec<(usize, Span, Type)>,
}

#[derive(Clone)]
struct AliasDef {
    ty: Type,
    /// Number of type parameters, carried so the interface can re-declare it.
    params: usize,
    /// The kind of each parameter (§2 kind annotations/inference).
    kinds: Vec<AliasParamKind>,
    /// Placeholder variable ids, one per parameter (only the matching kind is
    /// meaningful): dimension ids for `Dim`, nat ids for `Nat`, row ids for
    /// `Row`; type parameters use `Type::Param(index)` in `ty` instead.
    dim_vars: Vec<u32>,
    nat_vars: Vec<u32>,
    row_vars: Vec<u32>,
}

impl Checker {
    fn new(
        file: FileId,
        options: CheckOptions,
    ) -> Self {
        let mut checker = Self {
            file,
            module_path: ModulePath::new([format!("<file{file}>")]),
            options,
            deprecated: HashMap::new(),
            stability: IndexMap::new(),
            registrations: Vec::new(),
            private_classes: HashSet::new(),
            instance_declarations: HashMap::new(),
            method_bindings: HashMap::new(),
            constructor_bindings: HashMap::new(),
            infer: Infer::new(),
            globals: HashMap::new(),
            fn_signatures: HashMap::new(),
            fn_generics: HashMap::new(),
            function_stack: Vec::new(),
            function_inputs: HashMap::new(),
            function_reifications: HashMap::new(),
            call_materializations: HashMap::new(),
            generic_bindings: HashMap::new(),
            callable_ids: HashMap::new(),
            global_callables: HashMap::new(),
            local_callables: HashMap::new(),
            local_headers: HashMap::new(),
            scopes: Vec::new(),
            units: aevia_rssn::unit_table(),
            dims: DimEnv::si(),
            aliases: HashMap::new(),
            adts: HashMap::new(),
            type_params: Vec::new(),
            dim_params: Vec::new(),
            nat_params: Vec::new(),
            row_params: Vec::new(),
            unqualified_import_names: Vec::new(),
            requested_unqualified: HashSet::new(),
            top_lets: HashMap::new(),
            local_adts: HashSet::new(),
            classes: ClassTable::prelude(),
            deferred_constraints: Vec::new(),
            implicit_params: false,
            declaring_alias: None,
            mixed_rep: false,
            literal_context: None,
            literal_classes: HashMap::new(),
            literal_region: None,
            constant_region_roots: Vec::new(),
            numeric_conflict_reported: false,
            si_dimensions: false,
            referenced: HashSet::new(),
            pending_checks: Vec::new(),
            local_binders: Vec::new(),
            refutable_pattern: false,
            used_locals: HashSet::new(),
            literal_spans: Vec::new(),
            literal_owners: HashMap::new(),
            function_literal_sites: HashMap::new(),
            expression_indices: HashMap::new(),
            #[cfg(test)]
            literal_owner_visits: std::cell::Cell::new(0),
            expr_types: Vec::with_capacity(256),
            diagnostics: Diagnostics::new(),
            checked: CheckedModule::default(),
            return_type: None,
            do_returns: Vec::new(),
        };
        checker.register_prelude();
        checker
    }

    /// Records that an unqualified import brought `key` in, and reports a
    /// clash when a *different* module already brought a different type.
    ///
    /// Opening the same module twice is idempotent (§7.2.1), so an identical
    /// scheme is not a clash.
    fn note_opened(
        &mut self,
        opened: &mut Vec<(String, Type)>,
        prefix: &str,
        key: &str,
        scheme: &Scheme,
    ) {
        if !prefix.is_empty() {
            return;
        }
        if let Some((_, previous)) = opened.iter().find(|(name, _)| name == key) {
            if previous != &scheme.ty {
                self.error_help(
                    "E0603",
                    format!("`{key}` is opened by two modules"),
                    Span::default(),
                    "qualify the name, or import the modules without `open`",
                );
            }
            return;
        }
        opened.push((key.to_string(), scheme.ty.clone()));
    }

    /// Puts imported names in scope.
    ///
    /// An imported value is a global with a dotted name, which is exactly how
    /// `A.B.f` resolves: the qualified form is one key, not a nested lookup.
    fn add_imports(
        &mut self,
        imports: &[Import],
    ) {
        // Names an earlier import in this call already brought in, with the
        // type it gave them: a second, different one is ambiguous (§7.2.2).
        let mut opened: Vec<(String, Type)> = Vec::new();
        for (prefix, exports) in imports {
            if prefix.is_empty() {
                self.unqualified_import_names
                    .push(exports.keys().cloned().collect());
                self.requested_unqualified.extend(exports.keys().cloned());
            }
            let mut dimension_names = HashMap::new();
            for (name, export) in exports {
                if let Export::Dimension { dim, .. } = export
                    && let Some((aevia_dim::Atom::Nominal(base), power)) = dim.factors().next()
                    && dim.factors().count() == 1
                    && power.is_one()
                {
                    let display = if prefix.is_empty() {
                        name.clone()
                    } else {
                        format!("{prefix}.{name}")
                    };
                    dimension_names.entry(base.clone()).or_insert(display);
                }
            }
            for (name, export) in exports {
                let key = if prefix.is_empty() {
                    name.clone()
                } else {
                    format!("{prefix}.{name}")
                };
                self.method_bindings.remove(&key);
                self.constructor_bindings.remove(&key);
                if let Export::Ctor { adt, index, .. } | Export::TypeCtor { adt, index, .. } =
                    export
                {
                    self.constructor_bindings
                        .insert(key.clone(), (*adt, *index));
                }
                match export {
                    | Export::Value { scheme, .. } | Export::Ctor { scheme, .. } => {
                        self.note_opened(&mut opened, prefix, &key, scheme);
                        self.globals.insert(
                            key.clone(),
                            import_dimension_display(scheme, &dimension_names),
                        );
                    },
                    | Export::TypeCtor { adt, scheme, .. } => {
                        self.note_opened(&mut opened, prefix, &key, scheme);
                        self.globals.insert(
                            key.clone(),
                            import_dimension_display(scheme, &dimension_names),
                        );
                        self.adts.insert(key.clone(), *adt);
                    },
                    | Export::Unit { def } => {
                        // The unit is usable both qualified and, after
                        // `import { km }`, unqualified. A qualified spelling
                        // (`V.Imp2.smoot`) is a unit *name*, so it has to be
                        // in the table, not just in the value namespace.
                        let mut definition = def.clone();
                        definition.dim = definition.dim.with_display_names(&dimension_names);
                        if prefix.is_empty() {
                            if self.units.lookup(&definition.name).is_none() {
                                let _ = self.units.insert(definition.clone());
                            }
                        } else if self.units.lookup(&key).is_none() {
                            definition.name = key.clone();
                            definition.aliases.clear();
                            let _ = self.units.insert(definition);
                        }
                        self.globals.insert(
                            key.clone(),
                            Scheme::mono(Type::UnitOf(
                                def.dim.with_display_names(&dimension_names),
                            )),
                        );
                    },
                    | Export::Type { adt } => {
                        self.adts.insert(key.clone(), *adt);
                    },
                    | Export::Alias { .. } => {},
                    | Export::Dimension { dim, .. } => {
                        self.dims.define_alias(&key, dim.with_display_name(&key));
                    },
                }
            }
        }
    }

    /// Merges an imported module's classes, instances and alias definitions
    /// into this checker, before any body is checked (§7.1.3/§7.4).
    ///
    /// Imported instances skip the orphan rule (they were legal where they were
    /// declared) but participate in the global coherence check, so an overlap
    /// is `E0311` naming both modules. Classes are registered with their
    /// symbolic key; their methods become callable globals.
    fn merge_interface(
        &mut self,
        interface: &ModuleInterface,
        visible_names: bool,
        publish_methods: bool,
    ) {
        let module = interface.name.clone();
        // Qualified alias definitions: a source reference `A.Vec2` expands.
        if visible_names {
            for (name, alias) in &interface.aliases {
                let qualified = if module.is_prelude() {
                    name.clone()
                } else {
                    format!("{}.{}", module.dotted(), name)
                };
                let def = self.imported_alias_def(alias);
                self.aliases.entry(qualified).or_insert(def);
            }
        }
        // Classes and instances.
        let classes: Vec<(ClassKey, ClassDef)> = interface
            .classes
            .iter()
            .map(|(key, class)| {
                (
                    key.clone(),
                    ClassDef {
                        params: class.params,
                        methods: class.methods.clone(),
                        builtin: false,
                        key: key.clone(),
                    },
                )
            })
            .collect();
        let instances: Vec<Instance> = interface
            .instances
            .iter()
            .map(|instance| {
                Instance {
                    class: instance.key.class.name.clone(),
                    head: instance.head.clone(),
                    methods: instance.methods.clone(),
                    origin: Origin::Imported,
                    key: instance.key.clone(),
                }
            })
            .collect();
        if let Err(error) = self.classes.merge_imported(&module, &classes, &instances) {
            let message = match error {
                | InstanceError::Overlapping { incoming, existing } => {
                    format!("imported instance `{incoming}` overlaps `{existing}`")
                },
                | InstanceError::WitnessCollision { incoming, existing } => {
                    format!(
                        "imported instances `{incoming}` and `{existing}` cannot be told apart at run time"
                    )
                },
                | _ => format!("cannot merge the interface of `{module}`"),
            };
            self.error_help(
                "E0311",
                message,
                Span::default(),
                "two modules provide instances that match the same type; remove one",
            );
        }
        // Imported class methods are callable by their plain name.
        if publish_methods {
            for class in interface.classes.values() {
                for (method, ty) in &class.methods {
                    if !self.globals.contains_key(method) {
                        self.globals.insert(
                            method.clone(),
                            method_scheme(&class.key.name, 1, ty.clone()),
                        );
                        self.method_bindings.insert(
                            method.clone(),
                            ClassMethodRef {
                                class: class.key.clone(),
                                method: method.clone(),
                            },
                        );
                    }
                }
            }
        }
        if !visible_names {
            return;
        }
        for (name, projection) in &interface.callable_exports {
            let qualified = if module.is_prelude() {
                name.clone()
            } else {
                format!("{}.{}", module.dotted(), name)
            };
            self.global_callables
                .entry(qualified)
                .or_insert_with(|| projection.clone());
            if self.requested_unqualified.contains(name) {
                self.global_callables
                    .entry(name.clone())
                    .or_insert_with(|| projection.clone());
            }
        }
        for (name, headers) in &interface.function_headers {
            let qualified = if module.is_prelude() {
                name.clone()
            } else {
                format!("{}.{}", module.dotted(), name)
            };
            self.generic_bindings
                .entry(qualified)
                .or_insert_with(|| headers.clone());
            if self.requested_unqualified.contains(name) {
                self.generic_bindings
                    .entry(name.clone())
                    .or_insert_with(|| headers.clone());
            }
        }
        // §7.2.1/§7.2.2: the module's types and their constructors are
        // reachable under the module prefix (`A.T`, `A.Ctor`), and the names a
        // prefix-less `import`/`open` selected are reachable unqualified. The
        // interface is the only source of an alias *body* and of an ADT
        // identity, so this is where imported type names become visible.
        for (name, export) in &interface.exports {
            let qualified = if module.is_prelude() {
                name.clone()
            } else {
                format!("{}.{}", module.dotted(), name)
            };
            if !self.globals.contains_key(&qualified)
                && let Export::Ctor { adt, index, .. } | Export::TypeCtor { adt, index, .. } =
                    export
            {
                self.constructor_bindings
                    .insert(qualified.clone(), (*adt, *index));
            }
            match export {
                | Export::Type { adt } => {
                    self.adts.entry(qualified).or_insert(*adt);
                },
                | Export::TypeCtor { adt, scheme, .. } => {
                    self.adts.entry(qualified.clone()).or_insert(*adt);
                    self.globals
                        .entry(qualified)
                        .or_insert_with(|| scheme.clone());
                },
                | Export::Ctor { scheme, .. } | Export::Value { scheme, .. } => {
                    self.globals
                        .entry(qualified)
                        .or_insert_with(|| scheme.clone());
                },
                | _ => {},
            }
        }
        // Which interface is the source of the prefix-less import? The flat
        // import table the driver builds contains exactly the names the module
        // selected (or all names for `open`), so the interface whose exports
        // cover that set is the direct one. Requiring uniqueness avoids leaking
        // a transitively loaded module's names.
        let matching = self
            .unqualified_import_names
            .iter()
            .filter(|names| {
                !names.is_empty()
                    && names
                        .iter()
                        .all(|name| interface.exports.contains_key(name))
            })
            .count();
        if matching == 1 {
            for (name, export) in &interface.exports {
                if !self.requested_unqualified.contains(name) {
                    continue;
                }
                match export {
                    | Export::Alias { .. } => {
                        if let Some(alias) = interface.aliases.get(name) {
                            let def = self.imported_alias_def(alias);
                            self.aliases.entry(name.clone()).or_insert(def);
                        }
                    },
                    | Export::Type { adt } => {
                        self.adts.entry(name.clone()).or_insert(*adt);
                        // §7.1: the constructors travel with the type, so
                        // `import M { T }` also names every constructor of `T`.
                        for (ctor_name, ctor) in &interface.exports {
                            match ctor {
                                | Export::Ctor { adt: id, scheme, .. } if id == adt => {
                                    self.globals
                                        .entry(ctor_name.clone())
                                        .or_insert_with(|| scheme.clone());
                                },
                                | Export::TypeCtor { adt: id, scheme, .. } if id == adt => {
                                    self.adts.entry(ctor_name.clone()).or_insert(*id);
                                    self.globals
                                        .entry(ctor_name.clone())
                                        .or_insert_with(|| scheme.clone());
                                },
                                | _ => {},
                            }
                        }
                    },
                    | Export::TypeCtor { adt, scheme, .. } => {
                        self.adts.entry(name.clone()).or_insert(*adt);
                        self.globals
                            .entry(name.clone())
                            .or_insert_with(|| scheme.clone());
                    },
                    | Export::Ctor { scheme, .. } | Export::Value { scheme, .. } => {
                        self.globals
                            .entry(name.clone())
                            .or_insert_with(|| scheme.clone());
                    },
                    | _ => {},
                }
            }
        }
    }

    fn bind_interface(
        &mut self,
        interface: &ModuleInterface,
        binding: &InterfaceBinding,
    ) {
        for (name, projection) in &interface.callable_exports {
            if binding
                .names
                .as_ref()
                .is_some_and(|names| !names.contains(name))
            {
                continue;
            }
            let key = if binding.prefix.is_empty() {
                name.clone()
            } else {
                format!("{}.{name}", binding.prefix)
            };
            self.global_callables.insert(key, projection.clone());
        }
        for (name, headers) in &interface.function_headers {
            if binding
                .names
                .as_ref()
                .is_some_and(|names| !names.contains(name))
            {
                continue;
            }
            let key = if binding.prefix.is_empty() {
                name.clone()
            } else {
                format!("{}.{name}", binding.prefix)
            };
            self.generic_bindings.insert(key, headers.clone());
        }
        for (name, alias) in &interface.aliases {
            if binding
                .names
                .as_ref()
                .is_some_and(|names| !names.contains(name))
            {
                continue;
            }
            let key = if binding.prefix.is_empty() {
                name.clone()
            } else {
                format!("{}.{name}", binding.prefix)
            };
            let def = self.imported_alias_def(alias);
            self.aliases.insert(key, def);
        }
        // Selecting a type imports its public constructors as specified by
        // §7.1. The same expansion is used by the runtime import binding.
        if let Some(names) = &binding.names {
            let selected: Vec<_> = names
                .iter()
                .filter_map(|name| {
                    match interface.exports.get(name) {
                        | Some(Export::Type { adt } | Export::TypeCtor { adt, .. }) => Some(*adt),
                        | _ => None,
                    }
                })
                .collect();
            for (name, export) in &interface.exports {
                match export {
                    | Export::Ctor {
                        adt, index, scheme, ..
                    }
                    | Export::TypeCtor {
                        adt, index, scheme, ..
                    } if selected.contains(adt) => {
                        self.globals.insert(name.clone(), scheme.clone());
                        self.constructor_bindings
                            .insert(name.clone(), (*adt, *index));
                    },
                    | _ => {},
                }
            }
        }
    }

    fn register_prelude(&mut self) {
        // Unit values are visible unqualified, mirroring `open Units.SI`.
        for unit in self.units.list().to_vec() {
            let ty = Type::UnitOf(unit.dim.clone());
            self.globals
                .insert(unit.name.clone(), Scheme::mono(ty.clone()));
            for alias in &unit.aliases {
                self.globals.insert(alias.clone(), Scheme::mono(ty.clone()));
            }
        }
        // `None`, `Ok`, ... are in the prelude, so every module can see them
        // without an import. They are *not* part of any module's interface:
        // exporting them would make two `open`s look like a name clash.
        for name in ["None", "Some", "Ok", "Err", "Less", "Equal", "Greater"] {
            if let Some((adt, index, _)) = adt::find_prelude_ctor(name) {
                self.constructor_bindings
                    .insert(name.to_string(), (adt, index));
                self.globals
                    .insert(name.to_string(), ctor_scheme(adt, index));
            }
        }
        // §3.8.4: `show`, `compare` and friends are class methods, not
        // unconstrained builtins. `show(f)` for a function is `E0310`.
        for (class, method, ty) in prelude_class_methods() {
            let scheme = crate::class::method_scheme(class, 1, ty);
            self.globals.insert(method.to_string(), scheme);
            self.method_bindings.insert(
                method.to_string(),
                ClassMethodRef {
                    class: ClassKey::prelude(class),
                    method: method.to_string(),
                },
            );
        }
    }

    /// Checks the attributes of every declaration (§2.2.7, §13.8.1, §9.6.1).
    ///
    /// Attributes are not decoration: `@jit` without `core.jit` is `E1001`
    /// (§9.6.1), `@op`/`@rule` need the plugin ABI and are recorded for the
    /// runtime (§13.5.2), and a malformed argument is `E0210` (appendix B).
    /// `@stable`/`@experimental`/`@deprecated` are recorded so a consumer (the
    /// language server) can report them, and a use of a deprecated name warns.
    fn check_attributes(
        &mut self,
        module: &Module,
    ) {
        for decl in &module.decls {
            let owner = declaration_name(decl);
            for attr in &decl.attrs {
                // §2.2.7: attribute arguments must be compile-time constants.
                // Literals, quantity literals, constant arithmetic, constant
                // aggregates and identifiers/qualified names (which may name a
                // constant) are accepted; a call, lambda, `if`, field access or
                // interpolated string is `E0210`.
                for arg in &attr.args {
                    if !is_attribute_constant(&arg.value) {
                        self.error_help(
                            "E0210",
                            format!(
                                "the argument of `@{}` must be a compile-time constant",
                                attr.name
                            ),
                            arg.value.span,
                            "write a literal, a constant name, or constant arithmetic over them",
                        );
                    }
                }
                match attr.name.as_str() {
                    | "jit" => {
                        if !matches!(decl.kind, DeclKind::Fn { .. }) {
                            self.error(
                                "E0210",
                                "`@jit` may only be applied to a `fn` declaration",
                                attr.span,
                            );
                            continue;
                        }
                        // §13.9.4: `@jit(target = Cpu)` is the default target;
                        // `Gpu(n)` and `Distributed(..)` are accepted by the
                        // language and need a device backend, which is a
                        // capability gap, not a syntax error.
                        let mut bad_argument = false;
                        for arg in &attr.args {
                            let device = match (&arg.name, &arg.value.kind) {
                                | (Some(name), ExprKind::Var(device)) if name == "target" => {
                                    Some(device.as_str())
                                },
                                | (Some(name), ExprKind::Call { callee, .. }) if name == "target" => {
                                    match &callee.kind {
                                        | ExprKind::Var(device) => Some(device.as_str()),
                                        | _ => None,
                                    }
                                },
                                | _ => None,
                            };
                            match device {
                                | Some("Cpu") => {},
                                | Some(device @ ("Gpu" | "Distributed")) => {
                                    let capability = if device == "Gpu" {
                                        "core.gpu"
                                    } else {
                                        "core.distributed"
                                    };
                                    if !self.options.capabilities.has(capability) {
                                        self.error_help(
                                            "E1001",
                                            format!("backend capability `{capability}` is missing"),
                                            arg.value.span,
                                            "this build compiles `@jit` functions for the CPU only; use `@jit` or `@jit(target = Cpu)`",
                                        );
                                        bad_argument = true;
                                    }
                                },
                                | _ => {
                                    self.error_help(
                                        "E0210",
                                        "`@jit` takes only `target = Cpu | Gpu(n) | Distributed(..)`",
                                        arg.value.span,
                                        "write `@jit` or `@jit(target = Cpu)`",
                                    );
                                    bad_argument = true;
                                },
                            }
                        }
                        if bad_argument {
                            continue;
                        }
                        if !self.options.capabilities.has("core.jit") {
                            // §9.6.1: a build without a JIT gate can only
                            // interpret, and saying so is the whole point of
                            // the capability model (§13.1 rule 1).
                            self.error_help(
                                "E1001",
                                "backend capability `core.jit` is missing",
                                attr.span,
                                "this build always interprets the function; remove `@jit` (§9.6.1)",
                            );
                        }
                    },
                    | "stable" | "experimental" => {
                        if let Some(arg) = attr.args.first() {
                            self.error(
                                "E0210",
                                format!("`@{}` takes no arguments", attr.name),
                                arg.value.span,
                            );
                            continue;
                        }
                        if let Some(name) = owner.clone() {
                            let marker = if attr.name == "stable" {
                                Stability::Stable
                            } else {
                                Stability::Experimental
                            };
                            self.stability.insert(name, marker);
                        }
                    },
                    | "deprecated" => {
                        let message = match attr.args.as_slice() {
                            | [] => "this name is deprecated".to_string(),
                            | [arg] => {
                                match string_constant(&arg.value) {
                                    | Some(text) => text,
                                    | None => {
                                        self.error(
                                            "E0210",
                                            "`@deprecated` takes a string message",
                                            arg.value.span,
                                        );
                                        continue;
                                    },
                                }
                            },
                            | [_, extra, ..] => {
                                self.error(
                                    "E0210",
                                    "`@deprecated` takes at most one message",
                                    extra.value.span,
                                );
                                continue;
                            },
                        };
                        if let Some(name) = owner.clone() {
                            self.deprecated.insert(name.clone(), message.clone());
                            self.stability.insert(name, Stability::Deprecated(message));
                        }
                    },
                    | "op" | "rule" => {
                        if !matches!(decl.kind, DeclKind::Fn { .. }) {
                            self.error(
                                "E0210",
                                format!(
                                    "`@{}` may only be applied to a `fn` declaration",
                                    attr.name
                                ),
                                attr.span,
                            );
                            continue;
                        }
                        let Some(function) = owner.clone() else {
                            continue;
                        };
                        let name_arg = match attr.args.as_slice() {
                            | [arg] => {
                                match string_constant(&arg.value) {
                                    | Some(text) => text,
                                    | None => {
                                        self.error(
                                            "E0210",
                                            format!("`@{}` takes a string name", attr.name),
                                            arg.value.span,
                                        );
                                        continue;
                                    },
                                }
                            },
                            | _ => {
                                self.error(
                                    "E0210",
                                    format!("`@{}` takes exactly one string name", attr.name),
                                    attr.span,
                                );
                                continue;
                            },
                        };
                        // §13.5.2: `@op` wraps an Aevia function as a kernel
                        // host callback and `@rule` publishes a rewrite rule —
                        // source-level extension packaging. The manifest's
                        // `core.plugin.source` row says whether the build can
                        // accept it. Without it the honest answer is a
                        // capability gap, not silence.
                        if !self.options.capabilities.has("core.plugin.source") {
                            self.error_help(
                                "E1001",
                                "backend capability `core.plugin.source` is missing",
                                attr.span,
                                "this build has no in-process kernel plugins, which `@op`/`@rule` publish into; call the function normally (§13.5.2)",
                            );
                            continue;
                        }
                        let params = match &decl.kind {
                            | DeclKind::Fn { args, .. } => {
                                args.iter()
                                    .enumerate()
                                    .map(|(index, arg)| {
                                        let name = match &arg.pattern.kind {
                                            | PatKind::Var(name) => name.clone(),
                                            | _ => format!("arg{index}"),
                                        };
                                        (name, Dim::one())
                                    })
                                    .collect()
                            },
                            | _ => Vec::new(),
                        };
                        self.registrations.push(HostRegistration {
                            span: attr.span,
                            kind: if attr.name == "op" {
                                HostRegistrationKind::Op
                            } else {
                                HostRegistrationKind::Rule
                            },
                            name: name_arg,
                            function,
                            params,
                            result: Dim::one(),
                        });
                    },
                    // `@bench` (§9.12) and `@compile_fail` (§9.12) are read by
                    // the tooling, not by the checker.
                    | _ => {},
                }
            }
        }
    }

    fn run(
        &mut self,
        module: &Module,
    ) {
        self.checked.name = module.name.clone();
        self.checked.anonymous_file = self.file;
        self.module_path = self.checked.module_path();
        self.check_attributes(module);
        // §5.11: `module M with units natural`. The module's unit table
        // resolves through the system from here on, which is what reduces
        // every unit literal (dimension and scale) and every dimension
        // written in a type; imported bindings keep their SI types.
        if let Some((system, span)) = &module.unit_system {
            match aevia_rssn::registry::registry().unit_system(system) {
                | Some(found) => {
                    self.units = std::mem::take(&mut self.units).with_system(found);
                    self.checked.unit_system = Some(system.clone());
                },
                | None => {
                    let known: Vec<String> = aevia_rssn::registry::registry()
                        .unit_systems
                        .iter()
                        .map(|system| system.name.clone())
                        .collect();
                    self.error_help(
                        "E0512",
                        format!("unknown unit system `{system}`"),
                        *span,
                        &format!("the kernel provides: {}", known.join(", ")),
                    );
                },
            }
        }

        // Pass A: types, dimensions and units.
        //
        // Units are collected before anything else resolves a unit literal, so
        // a declaration may be written after its first use (§7.1.2).
        for decl in &module.decls {
            match &decl.kind {
                | DeclKind::Dimension { name, value } => {
                    self.declare_dimension(name, value.as_ref(), decl.span, decl.private);
                },
                | DeclKind::Unit { .. } => self.declare_unit(decl),
                | DeclKind::Type { name, params, body } => {
                    self.declare_type(name, params, body, decl.private);
                },
                | DeclKind::Class {
                    name,
                    params,
                    methods,
                } => self.declare_class(name, params, methods, decl.span, decl.private),
                | _ => {},
            }
        }

        // Pass B: signatures at level 1, so they can be generalised at level 0.
        self.infer.enter_level();
        let mut pending: Vec<(String, Span)> = Vec::new();
        let mut seen_fns: HashSet<String> = HashSet::new();
        for (index, decl) in module.decls.iter().enumerate() {
            match &decl.kind {
                | DeclKind::Fn {
                    name,
                    params,
                    args,
                    ret,
                    body,
                    ..
                } => {
                    if !seen_fns.insert(name.clone()) {
                        self.error_help(
                            "E0316",
                            format!("`{name}` is defined more than once"),
                            decl.span,
                            "rename one of the two functions",
                        );
                    }
                    self.callable_ids.insert(
                        decl.span,
                        CallableId::new(
                            &self.module_path,
                            decl.span,
                            &aevia_syntax::fingerprint::declaration(decl),
                        ),
                    );
                    let (ty, generics) = self.fn_signature(params, args, ret.as_ref());
                    self.global_callables.insert(
                        name.clone(),
                        CallableProjection {
                            origin: self
                                .callable_ids
                                .get(&decl.span)
                                .expect("seeded callable identity")
                                .clone(),
                            arguments: generics.binders.clone(),
                            selected: false,
                            forwarded_parameter: forwarded_parameter(args, body),
                        },
                    );
                    self.generic_bindings
                        .insert(name.clone(), generics.binders.clone());
                    self.fn_generics.insert(name.clone(), generics);
                    self.fn_signatures.insert(name.clone(), ty.clone());
                    let scheme = Scheme::mono(ty);
                    self.globals.insert(name.clone(), scheme.clone());
                    // §7.1.3: `private` keeps a name out of the interface.
                    if !decl.private {
                        self.checked.exports.insert(
                            name.clone(),
                            Export::Value {
                                scheme,
                                decl: index,
                                arity: args.len(),
                            },
                        );
                    }
                    pending.push((name.clone(), decl.span));
                },
                | DeclKind::Let { pattern, ty, .. } => {
                    let PatKind::Var(name) = &pattern.kind else {
                        continue;
                    };
                    // One variable for both passes: a forward reference and the
                    // definition must see the same type (§7.1.2).
                    let value_ty = match ty {
                        | Some(ty) => self.convert_type(ty),
                        | None => self.infer.fresh_type(),
                    };
                    self.top_lets.insert(name.clone(), value_ty.clone());
                    let scheme = Scheme::mono(value_ty);
                    self.globals.insert(name.clone(), scheme.clone());
                    if !decl.private {
                        self.checked.exports.insert(
                            name.clone(),
                            Export::Value {
                                scheme,
                                decl: index,
                                arity: 0,
                            },
                        );
                    }
                    pending.push((name.clone(), decl.span));
                },
                | _ => {},
            }
        }

        // Pass C: bodies.
        for decl in &module.decls {
            match &decl.kind {
                | DeclKind::Fn { name, args, body, .. } => {
                    self.check_fn_body(name, args, body, decl.span)
                },
                | DeclKind::Let {
                    pattern,
                    ty,
                    value,
                    rec,
                } => self.check_top_let_body(pattern, ty.as_ref(), value, *rec, decl.span),
                | DeclKind::Test { name, body } => {
                    let ty = self.synth(body);
                    self.expect_bool(&ty, body.span);
                    self.checked.tests.push(name.clone());
                },
                | DeclKind::Instance { .. } => self.check_instance(decl),
                | DeclKind::Derive { .. } => self.check_derive(decl),
                | _ => {},
            }
            // §3.10.2 rule 6 / §3.5.1: a type-level equation that still has
            // several unknowns after the declaration cannot be expressed in its
            // scheme; it is genuinely ambiguous and reported as `E0304`.
            for unsolved in self.infer.take_unsolved_nat_equations() {
                if unsolved.linear {
                    self.error_help(
                        "E0304",
                        format!(
                            "cannot infer the type-level extents; the equation is ambiguous: {}",
                            unsolved.text
                        ),
                        decl.span,
                        "the equation involves several unknown extents; make the shapes agree, or add an annotation that determines them (for example a concrete `Tensor<[3], Float>`)",
                    );
                } else {
                    self.error_help(
                        "E0304",
                        format!(
                            "cannot solve the type-level equation {} (a product of unknown extents)",
                            unsolved.text
                        ),
                        decl.span,
                        "a product of unknowns is kept symbolic and only cancels against the same product (`n * m` and `m * n` agree); it cannot be solved for an unknown, so bind one factor",
                    );
                }
            }
        }

        // Pass D: generalise every top-level binding, then default what is
        // left. Defaulting *after* generalisation is what keeps `<d>`
        // polymorphic functions polymorphic (§3.10.2).
        self.infer.leave_level();
        for (name, _) in &pending {
            let Some(scheme) = self.globals.get(name).cloned() else {
                continue;
            };
            let generalised = self.generalize_scheme(&scheme);
            self.globals.insert(name.clone(), generalised.clone());
            if let Some(Export::Value { scheme, .. }) = self.checked.exports.get_mut(name) {
                *scheme = generalised;
            }
        }
        // A constraint whose type is one of this binding's quantified
        // variables belongs to the binding's scheme (§3.2.5): that is how
        // `fn f(x) = show(x)` becomes `Show a => a -> String`.
        for (name, _) in &pending {
            let Some(scheme) = self.globals.get(name).cloned() else {
                continue;
            };
            if scheme.vars.is_empty() {
                continue;
            }
            let mut attached: Vec<Constraint> = Vec::new();
            self.deferred_constraints.retain(|(class, ty, _)| {
                let resolved = self.infer.resolve(ty);
                if mentions_any(&resolved, &scheme.vars) {
                    attached.push(Constraint::new(class.clone(), vec![resolved]));
                    false
                } else {
                    true
                }
            });
            if attached.is_empty() {
                continue;
            }
            let mut combined = scheme.constraints.clone();
            for constraint in attached {
                if !combined.contains(&constraint) {
                    combined.push(constraint);
                }
            }
            let constrained = scheme.constrained(combined);
            self.globals.insert(name.clone(), constrained.clone());
            if let Some(Export::Value { scheme, .. }) = self.checked.exports.get_mut(name) {
                *scheme = constrained;
            }
        }
        for (name, span) in &pending {
            let Some(scheme) = self.globals.get(name).cloned() else {
                continue;
            };
            let (defaulted, warnings) = self.infer.default_scheme(&scheme);
            for _ in warnings {
                self.warn(
                    "W0301",
                    format!("the dimension of `{name}` is unresolved; it defaults to 1"),
                    *span,
                );
            }
            self.globals.insert(name.clone(), defaulted.clone());
            if let Some(Export::Value { scheme, .. }) = self.checked.exports.get_mut(name) {
                *scheme = defaulted;
            }
        }
        // A constraint that only ever appeared in a discarded expression still
        // has a literal type to default: `show(1)` is `Show Int`.
        for index in 0..self.deferred_constraints.len() {
            let ty = self.deferred_constraints[index].1.clone();
            let (defaulted, _) = self.infer.default_type(&ty);
            self.deferred_constraints[index].1 = defaulted;
        }

        // §B.12: a symbol nobody reads is dead weight.
        for decl in &module.decls {
            if let DeclKind::Let { pattern, value, .. } = &decl.kind {
                let PatKind::Var(name) = &pattern.kind else {
                    continue;
                };
                if self.referenced.contains(name) || decl.private {
                    continue;
                }
                if is_symbol_factory(value) {
                    self.warn(
                        "W0303",
                        format!("the symbol `{name}` is never used"),
                        decl.span,
                    );
                }
            }
        }

        // §B.12 `W0101`: a local binding (parameter, `let`, pattern) that no
        // body read. A name starting with `_` is the language's own "I know"
        // marker and is already skipped when it is recorded.
        let used_locals = std::mem::take(&mut self.used_locals);
        for (name, span) in std::mem::take(&mut self.local_binders) {
            if !used_locals.contains(&name) {
                self.warn("W0101", format!("unused variable `{name}`"), span);
            }
        }
        // §B.12 `W0601`: a `private` *value* declaration is this module's own,
        // so one no body reads is dead. Types, units and classes are used in
        // annotations (which do not go through `lookup_value`), so they are not
        // reported here rather than risking a false positive.
        for decl in &module.decls {
            if !decl.private {
                continue;
            }
            if !matches!(decl.kind, DeclKind::Fn { .. } | DeclKind::Let { .. }) {
                continue;
            }
            let Some(name) = declaration_name(decl) else {
                continue;
            };
            if !self.referenced.contains(&name) {
                self.warn(
                    "W0601",
                    format!("private declaration `{name}` is never used"),
                    decl.span,
                );
            }
        }

        // Every constraint whose type became known is solved; the rest is
        // `E0310` (§3.8.2 rule 3).
        self.flush_deferred_constraints();
        // A bound that the final substitution violates is a type error: the
        // annotation, not the literal, is what is wrong.
        for (got, bound) in self.infer.rep_conflicts() {
            self.diagnostics.push(
                Diagnostic::error("E0302", "type mismatch", Span::new(self.file, 0, 0))
                    .with_primary_label(format!(
                        "this value is {}, but the context needs at least {}",
                        got.name(),
                        bound.name()
                    ))
                    .with_help("annotate the binding with the wider representation"),
            );
        }

        if self.mixed_rep || self.infer.has_real_float_join() {
            self.warn(
                "W0302",
                "mixing `Real` and `Float` uses the exact `Real` path",
                Span::new(self.file, 0, 0),
            );
        }

        // §4.9.4: `dyn`/`check` materialise a *concrete* `d` at the call site.
        // `dyn` is decided while checking its argument; `check`'s `d` may be
        // fixed by the surrounding expression, so it is resolved here, after
        // generalisation and defaulting. A `check` whose `d` is still a variable
        // is generic code, which the spec says must use `check_with`.
        let pending_checks = std::mem::take(&mut self.pending_checks);
        for (span, dim) in pending_checks {
            let resolved = self.infer.resolve_dim(&dim);
            if resolved.is_concrete() {
                self.checked.dyn_calls.push(DynCall {
                    span,
                    primitive: DynPrimitive::Check,
                    dim: resolved,
                });
            } else {
                self.error_help(
                    "E0304",
                    "the dimension of `check` is not concrete",
                    span,
                    "generic code passes the dimension at run time: `check_with(dim, x)`",
                );
            }
        }

        // The runtime resolves unit literals through this table, so it has to
        // travel with the checked module — *after* the user's declarations.
        self.checked.units = self.units.clone();
        self.checked.dims = self.dims.clone();
        let mut dictionaries = std::mem::take(&mut self.checked.dictionaries);
        for dictionary in &mut dictionaries {
            // The head is recorded when the call is checked; a structural
            // match may already have found the instance, so its literal
            // arguments are only defaulted here.
            let protected = self.infer.generalize(&dictionary.head);
            let (defaulted, _) = self.infer.default_scheme(&protected);
            dictionary.head = self.infer.resolve(&defaulted.ty);
        }
        // §9.2.3/round-2 decision 1c: the *resolved* instance is only known
        // now — at check time the head may still have been a literal variable
        // (`show(1)`). Pin the key once the head is concrete; a quantified head
        // keeps its witness candidate list.
        for dictionary in &mut dictionaries {
            dictionary.target =
                self.dict_target(&dictionary.class, &dictionary.method, &dictionary.head);
        }
        self.checked.dictionaries = dictionaries;
        // Finalize each actual body independently of the module's current
        // name binding: a later declaration may shadow that name. Protect
        // generalizable lexical free variables as well as the function's own
        // binders, so a nested closure never defaults its parent's parameters.
        let lexical_types: HashSet<u32> = self
            .checked
            .functions
            .values()
            .flat_map(|function| function.scheme.vars.iter().copied())
            .collect();
        let lexical_dims: HashSet<u32> = self
            .checked
            .functions
            .values()
            .flat_map(|function| function.scheme.dims.iter().copied())
            .collect();
        let lexical_rows: HashSet<u32> = self
            .checked
            .functions
            .values()
            .flat_map(|function| function.scheme.rows.iter().copied())
            .collect();
        let lexical_nats: HashSet<u32> = self
            .checked
            .functions
            .values()
            .flat_map(|function| function.scheme.nats.iter().copied())
            .collect();
        // Call instances outside a definition have no activation dictionary.
        // Default their transported literal domains too (including clauses
        // whose original literal lives in another compilation unit).
        if let Err(error) = self.infer.default_unprotected_numeric(&lexical_types) {
            self.report_unify(error, Span::new(self.file, 0, 0));
        }
        fn protect_lexical(
            target: &mut Vec<u32>,
            relevant: impl Iterator<Item = u32>,
            quantified: &HashSet<u32>,
        ) {
            for id in relevant {
                if quantified.contains(&id) && !target.contains(&id) {
                    target.push(id);
                }
            }
        }
        for function in self.checked.functions.values_mut() {
            let protected = self.infer.generalize(&function.scheme.ty);
            let mut defaulting = function.scheme.clone();
            for (target, free) in [
                (&mut defaulting.vars, &protected.vars),
                (&mut defaulting.dims, &protected.dims),
                (&mut defaulting.rows, &protected.rows),
                (&mut defaulting.nats, &protected.nats),
            ] {
                for id in free {
                    if !target.contains(id) {
                        target.push(*id);
                    }
                }
            }
            // Protect only lexical variables this temporary defaulting walk
            // can reach; ordered/vacuous binders and body requirements remain
            // unchanged on the actual FunctionEvidence.
            let mut relevant = crate::TypeSubstitution::default();
            let mut names = Infer::new();
            relevant.freshen(&function.scheme.ty, &mut names);
            for constraint in &function.scheme.constraints {
                for ty in &constraint.args {
                    relevant.freshen(ty, &mut names);
                }
            }
            protect_lexical(
                &mut defaulting.vars,
                relevant
                    .types
                    .keys()
                    .chain(relevant.representations.keys())
                    .copied(),
                &lexical_types,
            );
            protect_lexical(
                &mut defaulting.dims,
                relevant.dimensions.keys().copied(),
                &lexical_dims,
            );
            protect_lexical(
                &mut defaulting.rows,
                relevant.rows.keys().copied(),
                &lexical_rows,
            );
            protect_lexical(
                &mut defaulting.nats,
                relevant.naturals.keys().copied(),
                &lexical_nats,
            );
            let (defaulted, _) = self.infer.default_scheme(&defaulting);
            function.scheme.ty = defaulted.ty;
            function.scheme.constraints = defaulted.constraints;
            function.scheme.numeric = defaulted.numeric;
            function.scheme.materializations = defaulted.materializations;
        }
        // The per-expression table is resolved *last*, after every unification
        // and every defaulting step: an entry recorded while checking holds
        // unification variables, and this is the only point at which their
        // final bindings are known. Resolving earlier would freeze the
        // instantiation a call happened to have at the moment it was checked.
        let recorded: HashMap<Span, Type> =
            std::mem::take(&mut self.expr_types).into_iter().collect();
        self.expression_indices.clear();
        // A body's internal literal variable is not a universal parameter.
        // Resolve its normal literal reading before freezing expression types;
        // only actual lexical quantifiers need activation-time materialization.
        let mut literal_default_errors = Vec::new();
        for span in &self.literal_spans {
            let Some(ty) = recorded.get(span).map(|ty| self.infer.resolve(ty)) else {
                continue;
            };
            let mut free = crate::TypeSubstitution::default();
            free.freshen(&ty, &mut Infer::new());
            let generic = free
                .types
                .keys()
                .chain(free.representations.keys())
                .any(|id| lexical_types.contains(id))
                || free.dimensions.keys().any(|id| lexical_dims.contains(id))
                || free.rows.keys().any(|id| lexical_rows.contains(id))
                || free.naturals.keys().any(|id| lexical_nats.contains(id));
            let owner = self.literal_owners.get(span).and_then(|(owner, _)| *owner);
            if generic && owner.is_some() {
                continue;
            }
            let Some((_, kind)) = self.literal_owners.get(span) else {
                continue;
            };
            let reading = match &ty {
                | Type::Var(_) => {
                    Some(Type::Scalar(
                        Rep::default_for_literal(self.infer.literal_kind(&ty).unwrap_or(*kind)),
                        Dim::one(),
                    ))
                },
                | Type::Scalar(rep @ Rep::Var(_), dim) => {
                    let bounds = self.infer.rep_bounds(*rep);
                    let preferred = self
                        .infer
                        .literal_representation(*rep)
                        .unwrap_or_else(|| Rep::default_for_literal(*kind));
                    let constrained_kind = self.infer.rep_literal_kind(*rep);
                    let chosen = [preferred, Rep::Int, Rep::Float, Rep::Real, Rep::Complex]
                        .into_iter()
                        .find(|candidate| {
                            kind.accepts_rep(*candidate)
                                && constrained_kind.is_none_or(|kind| kind.accepts_rep(*candidate))
                                && bounds.iter().all(|bound| {
                                    let bound = self.infer.resolve_rep(*bound);
                                    matches!(bound, Rep::Var(_)) || candidate.at_least(bound)
                                })
                        });
                    chosen.map(|rep| Type::Scalar(rep, dim.clone()))
                },
                | _ => None,
            };
            if let Some(reading) = reading
                && let Err(error) = self.infer.unify(&ty, &reading)
            {
                literal_default_errors.push((error, *span));
            }
        }
        for (error, span) in literal_default_errors {
            self.report_unify(error, span);
        }
        let resolved_expr_types: HashMap<Span, Type> = recorded
            .into_iter()
            .map(|(span, ty)| (span, self.infer.resolve(&ty)))
            .collect();
        // §3.10/§6.3.2: a literal's representation is the one the static type
        // chose. Export it so the runtime can materialise the literal at that
        // representation instead of its default one — without this a value the
        // checker typed `Float` can hold an exact `Real` at run time.
        for span in std::mem::take(&mut self.literal_spans) {
            if let Some(ty) = resolved_expr_types
                .get(&span)
                .filter(|ty| crate::class::has_variables(ty))
            {
                if let Some((Some(owner), kind)) = self.literal_owners.get(&span)
                    && let Some(function) = self.checked.functions.get_mut(owner)
                {
                    function.numeric_literals.push((span, *kind, ty.clone()));
                }
                if let Type::Scalar(rep, _) = ty {
                    self.checked.literal_reps.insert(span, *rep);
                }
                continue;
            }
            if let Some(Type::Scalar(rep, _)) = resolved_expr_types.get(&span)
                && let Some(rep) = self.infer.literal_representation(*rep)
            {
                self.checked.literal_reps.insert(span, rep);
            }
        }
        // §3.10.4: a closed literal arithmetic tree is a constant expression.
        // Its value is computed exactly and materialised once in the tree's
        // static representation, so only inexact representations need the
        // table; an exact one already evaluates exactly leaf by leaf.
        for span in std::mem::take(&mut self.constant_region_roots) {
            if let Some(Type::Scalar(rep, _)) = resolved_expr_types.get(&span)
                && !matches!(rep, Rep::Int | Rep::Real)
            {
                self.checked.constant_regions.insert(span, *rep);
            }
        }
        self.checked.expr_types = resolved_expr_types;
        for function in self.checked.functions.values_mut() {
            function.scheme = self.infer.resolve_scheme(&function.scheme);
            for constraint in &mut function.scheme.constraints {
                for arg in &mut constraint.args {
                    *arg = self.infer.resolve(arg);
                }
            }
            if function.binders.is_empty() {
                function.binders = inferred_binders(&function.scheme);
            }
            function.binders = function
                .binders
                .iter()
                .map(|binder| binder.resolved(&self.infer))
                .collect();
            function.reifications = function
                .reifications
                .iter()
                .map(|ty| self.infer.resolve(ty))
                .collect();
        }
        for (name, _) in &pending {
            if let Some(scheme) = self.globals.get(name).cloned() {
                let normalized = self.infer.resolve_scheme(&scheme);
                self.globals.insert(name.clone(), normalized.clone());
                if let Some(Export::Value { scheme, .. }) = self.checked.exports.get_mut(name) {
                    *scheme = normalized;
                }
            }
        }
        for message in self.infer.take_numeric_errors() {
            self.report_unify(UnifyError::Numeric(message), Span::new(self.file, 0, 0));
        }
        let mut completed_origins = HashMap::new();
        for function in self.checked.functions.values_mut() {
            let completed = function
                .identity
                .with_contract(&crate::callable::contract(function));
            completed_origins.insert(function.identity.clone(), completed.clone());
            function.identity = completed;
        }
        for projection in self
            .checked
            .callable_refs
            .values_mut()
            .chain(self.global_callables.values_mut())
        {
            if let Some(completed) = completed_origins.get(&projection.origin) {
                projection.origin = completed.clone();
            }
        }
        for target in self.checked.try_targets.values_mut() {
            target.result = self.infer.resolve(&target.result);
        }
        for projection in self.checked.callable_refs.values_mut() {
            *projection = projection.resolved(&self.infer);
        }
        for headers in self.checked.generic_refs.values_mut() {
            *headers = headers
                .iter()
                .map(|header| header.resolved(&self.infer))
                .collect();
        }
        for (name, export) in &self.checked.exports {
            if let Export::Value { scheme, .. } = export
                && matches!(scheme.ty, Type::Fn(..))
            {
                let headers = self
                    .generic_bindings
                    .get(name)
                    .cloned()
                    .unwrap_or_else(|| inferred_binders(scheme));
                if let Some(projection) = self.global_callables.get(name) {
                    self.checked
                        .callable_exports
                        .insert(name.clone(), projection.resolved(&self.infer));
                }
                self.checked.function_headers.insert(
                    name.clone(),
                    headers
                        .iter()
                        .map(|header| header.resolved(&self.infer))
                        .collect(),
                );
            }
        }
        // §7.1.3/§7.4, round-2 decision 1b: classes, instances and alias
        // definitions travel with the interface. Only what *this* module
        // declares is exported — a prelude instance or an imported one is not
        // re-exported (imports are not transitive).
        for class in self.classes.classes_declared_in(&self.module_path) {
            let exported = ClassExport {
                key: class.key.clone(),
                params: class.params,
                methods: class.methods.clone(),
            };
            self.checked
                .local_classes
                .insert(class.key.clone(), exported.clone());
            if !self.private_classes.contains(&class.key.name) {
                self.checked.classes.insert(class.key.clone(), exported);
            }
        }
        for instance in self.classes.instances_declared_in(&self.module_path) {
            let exported = InstanceExport {
                key: instance.key.clone(),
                head: instance.head.clone(),
                methods: instance.methods.clone(),
                derived: instance.origin == Origin::Derived,
            };
            for (span, key) in &self.instance_declarations {
                if key == &instance.key {
                    self.checked.instance_sites.insert(*span, exported.clone());
                }
            }
            if !self.private_classes.contains(&instance.class) {
                self.checked.instances.push(exported);
            }
        }
        // Alias bodies are already resolved templates: public aliases may
        // refer to private helpers without exporting those helper names.
        for (name, alias) in &self.aliases {
            if matches!(self.checked.exports.get(name), Some(Export::Alias { .. })) {
                self.checked
                    .alias_defs
                    .insert(name.clone(), self.canonical_alias_export(alias));
            }
        }
        // §13.8.1: stability markers travel with the interface, so the editor
        // can show them and a consumer can avoid a deprecated name.
        self.checked.stability = self.stability.clone();
        let mut registrations = self.registrations.clone();
        for registration in &mut registrations {
            if let Some(scheme) = self.globals.get(&registration.function)
                && let Type::Fn(params, result) = self.infer.resolve(&scheme.ty)
            {
                for ((_, dim), ty) in registration.params.iter_mut().zip(params.iter()) {
                    if let Type::Scalar(_, found) = self.infer.resolve(ty) {
                        *dim = self.infer.resolve_dim(&found);
                    }
                }
                if let Type::Scalar(_, found) = self.infer.resolve(&result) {
                    registration.result = self.infer.resolve_dim(&found);
                }
            }
        }
        self.checked.registrations = registrations;
        self.checked.errors = self.diagnostics.error_count();
    }

    /// Records the type of the expression or binder at `span`.
    ///
    /// The type is stored unresolved; [`Self::run`] resolves the whole table
    /// once checking is over. A zero-width span is a synthetic node the parser
    /// invented, and `Error` is the checker's "inference already failed" — an
    /// entry for either would look like information without being any.
    ///
    /// Records are append-only until a speculative suffix is rolled back.
    /// Even adjacent visits to the same span keep separate versions, so a
    /// failed overload cannot overwrite evidence preceding its checkpoint.
    /// The per-span index gives O(1) access to the latest surviving version;
    /// module finalization collapses those records to one public entry.
    fn note_type(
        &mut self,
        span: Span,
        ty: &Type,
    ) {
        if span.start >= span.end || matches!(ty, Type::Error) {
            return;
        }
        self.expression_indices
            .entry(span)
            .or_default()
            .push(self.expr_types.len());
        self.expr_types.push((span, ty.clone()));
    }

    fn truncate_expr_types(
        &mut self,
        len: usize,
    ) {
        for (span, _) in self.expr_types.drain(len..) {
            if let Some(indices) = self.expression_indices.get_mut(&span) {
                indices.pop();
                if indices.is_empty() {
                    self.expression_indices.remove(&span);
                }
            }
        }
    }

    fn latest_expr_type(
        &self,
        span: Span,
    ) -> Option<&Type> {
        let index = *self.expression_indices.get(&span)?.last()?;
        self.expr_types.get(index).map(|(_, ty)| ty)
    }

    fn record_literal(
        &mut self,
        span: Span,
        kind: LiteralKind,
    ) {
        self.literal_spans.push(span);
        let owner = self.function_stack.last().copied();
        let previous = self.literal_owners.insert(span, (owner, kind));
        if previous != Some((owner, kind))
            && let Some(owner) = owner
        {
            self.function_literal_sites
                .entry(owner)
                .or_default()
                .push((span, kind));
        }
    }

    fn owned_literal_sites(
        &self,
        site: Span,
    ) -> Vec<(Span, LiteralKind)> {
        let mut sites = self
            .function_literal_sites
            .get(&site)
            .cloned()
            .unwrap_or_default();
        #[cfg(test)]
        self.literal_owner_visits
            .set(self.literal_owner_visits.get() + sites.len());
        sites.retain(|(span, _)| {
            self.literal_owners
                .get(span)
                .is_some_and(|(owner, _)| *owner == Some(site))
        });
        sites.sort_by_key(|(span, _)| (span.file, span.start, span.end));
        sites.dedup_by_key(|(span, _)| *span);
        sites
    }

    /// Generalises a scheme, keeping the class constraints of the declaration
    /// it came from.
    fn generalize_scheme(
        &mut self,
        scheme: &Scheme,
    ) -> Scheme {
        let mut generalised = self
            .infer
            .generalize_with(&scheme.ty, scheme.constraints.clone());
        for (target, original) in [
            (&mut generalised.vars, &scheme.vars),
            (&mut generalised.dims, &scheme.dims),
            (&mut generalised.rows, &scheme.rows),
            (&mut generalised.nats, &scheme.nats),
        ] {
            for id in original {
                if !target.contains(id) {
                    target.push(*id);
                }
            }
        }
        generalised.materializations = scheme.materializations.clone();
        self.infer.attach_numeric_scheme(&mut generalised);
        generalised
    }

    // ----- declarations --------------------------------------------------

    fn declare_dimension(
        &mut self,
        name: &str,
        value: Option<&DimAst>,
        span: Span,
        private: bool,
    ) {
        let (dim, is_base) = match value {
            | Some(ast) => {
                let dim = self.convert_dim(ast, span);
                self.dims.define_alias(name, dim.clone());
                (dim, false)
            },
            | None => {
                if !self
                    .dims
                    .define_nominal_base(name, &self.module_path.identity_key())
                {
                    self.error(
                        "E0316",
                        format!("dimension `{name}` is already defined"),
                        span,
                    );
                    return;
                }
                (
                    self.dims.lookup(name).expect("just declared dimension"),
                    true,
                )
            },
        };
        if !private {
            self.checked
                .exports
                .insert(name.to_string(), Export::Dimension { dim, is_base });
        }
    }

    fn declare_unit(
        &mut self,
        decl: &Decl,
    ) {
        let DeclKind::Unit {
            name,
            dim,
            scale,
            base,
            offset,
            prefixable,
        } = &decl.kind
        else {
            return;
        };
        // §5.11.4: a unit is a definition in SI, wherever it is declared; a
        // module with a unit system sees it through the system like any other
        // unit, and an SI importer sees the unit it wrote.
        let si_dimensions = std::mem::replace(&mut self.si_dimensions, true);
        let dimension = self.convert_dim(dim, decl.span);
        self.si_dimensions = si_dimensions;
        let mut scale_value = scale.clone();
        if let Some(base) = base {
            match self.units.resolve_si(base) {
                | Ok(resolved) => scale_value = scale_value.mul(&resolved.scale),
                | Err(error) => {
                    self.unit_error(error, decl.span);
                    return;
                },
            }
        }
        let offset_value = match offset {
            | Some((offset_scale, offset_unit)) => {
                let mut value = offset_scale.clone();
                if let Some(unit) = offset_unit {
                    match self.units.resolve_si(unit) {
                        | Ok(resolved) => {
                            // §5.1.1: the offset must measure the same
                            // dimension as the unit it is attached to.
                            if resolved.dim.is_concrete()
                                && dimension.is_concrete()
                                && resolved.dim != dimension
                            {
                                let offset_dim = self.dims.format(&resolved.dim);
                                let unit_dim = self.dims.format(&dimension);
                                self.error_help(
                                    "E0504",
                                    format!(
                                        "the offset measures {offset_dim} but the unit measures {unit_dim}"
                                    ),
                                    decl.span,
                                    "an affine offset must have the unit's own dimension",
                                );
                                return;
                            }
                            value = value.mul(&resolved.scale);
                        },
                        | Err(error) => {
                            self.unit_error(error, decl.span);
                            return;
                        },
                    }
                }
                Some(value)
            },
            | None => None,
        };
        let def = UnitDef {
            name: name.clone(),
            aliases: Vec::new(),
            dim: dimension.clone(),
            scale: scale_value,
            offset: offset_value.clone(),
            prefixable: *prefixable,
            affine: offset_value.is_some(),
            display: name.clone(),
            category: "user".to_string(),
        };
        if let Err(error) = self.units.insert(def.clone()) {
            self.unit_error(error, decl.span);
            return;
        }
        let local = self.units.reduce_dim(&dimension);
        self.globals
            .insert(name.clone(), Scheme::mono(Type::UnitOf(local)));
        if !decl.private {
            self.checked
                .exports
                .insert(name.clone(), Export::Unit { def });
        }
    }

    /// The names a declaration body uses in each kind position, for the
    /// §2 "kind may be omitted and inferred from the use" rule.
    /// The declared parameter kinds of a generic alias or variant type, when it
    /// is already in scope. Used to propagate a kind through a nested
    /// application (`type Opts<d> = | Fixed(FixedOpts<d>) | …`).
    fn declared_param_kinds(
        &self,
        name: &str,
    ) -> Option<Vec<AliasParamKind>> {
        if let Some(alias) = self.aliases.get(name) {
            return Some(alias.kinds.clone());
        }
        self.adts.get(name).map(|id| {
            adt::param_kinds_of(*id)
                .into_iter()
                .map(|kind| {
                    match kind {
                        | adt::ParamKind::Type => AliasParamKind::Type,
                        | adt::ParamKind::Dim(_) => AliasParamKind::Dim,
                    }
                })
                .collect()
        })
    }

    /// Propagates a written argument's kind from the callee's declaration into
    /// the usage sets, so an unannotated parameter used only as a *dimension
    /// argument* (`FixedOpts<d>`) is inferred as a dimension.
    fn scan_kind_args(
        &self,
        ty: &Ty,
        usage: &mut ParamUsage,
    ) {
        match &ty.kind {
            | TyKind::Named { path, args } => {
                let kinds = self.declared_param_kinds(&path.join("."));
                for (index, arg) in args.iter().enumerate() {
                    let declared = kinds.as_ref().and_then(|kinds| kinds.get(index)).copied();
                    match declared {
                        | Some(AliasParamKind::Dim) => {
                            match bare_type_name(arg) {
                                | Some(name) => {
                                    usage.dims.insert(name.clone());
                                },
                                | None => self.scan_kind_args(arg, usage),
                            }
                        },
                        | Some(AliasParamKind::Nat) => {
                            match bare_type_name(arg) {
                                | Some(name) => {
                                    usage.nats.insert(name.clone());
                                },
                                | None => self.scan_kind_args(arg, usage),
                            }
                        },
                        | Some(AliasParamKind::Row) => {
                            match bare_type_name(arg) {
                                | Some(name) => {
                                    usage.rows.insert(name.clone());
                                },
                                | None => self.scan_kind_args(arg, usage),
                            }
                        },
                        | Some(AliasParamKind::Type) | None => self.scan_kind_args(arg, usage),
                    }
                }
            },
            | TyKind::Tensor { elem, .. }
            | TyKind::Sparse { elem, .. }
            | TyKind::Matrix { elem, .. }
            | TyKind::Option(elem)
            | TyKind::Set(elem)
            | TyKind::Table(elem)
            | TyKind::RateOf(elem)
            | TyKind::DiffusionOf(elem)
            | TyKind::Io(elem) => self.scan_kind_args(elem, usage),
            | TyKind::Result(ok, error) => {
                self.scan_kind_args(ok, usage);
                self.scan_kind_args(error, usage);
            },
            | TyKind::Fn(params, ret) => {
                for param in params {
                    self.scan_kind_args(param, usage);
                }
                self.scan_kind_args(ret, usage);
            },
            | TyKind::Record { fields, .. } => {
                for (_, field) in fields {
                    self.scan_kind_args(field, usage);
                }
            },
            | TyKind::Tuple(items) | TyKind::Row(items) => {
                for item in items {
                    self.scan_kind_args(item, usage);
                }
            },
            | TyKind::Forall(_, body) => self.scan_kind_args(body, usage),
            | _ => {},
        }
    }

    fn declaration_param_kinds(
        &mut self,
        params: &[TypeParam],
        body: &TypeBody,
    ) -> (Vec<AliasParamKind>, Vec<(usize, AliasParamKind)>) {
        let mut usage = ParamUsage::default();
        match body {
            | TypeBody::Alias(ty) => usage.scan(ty),
            | TypeBody::Variants(ctors) => {
                for ctor in ctors {
                    for field in &ctor.args {
                        usage.scan(field);
                    }
                }
            },
            | TypeBody::Abstract => {},
        }
        // A name used only as a *dimension argument* of an already-declared
        // generic type (`FixedOpts<d>`) is a dimension too, even though the
        // written argument looks like a type.
        match body {
            | TypeBody::Alias(ty) => self.scan_kind_args(ty, &mut usage),
            | TypeBody::Variants(ctors) => {
                for ctor in ctors {
                    for field in &ctor.args {
                        self.scan_kind_args(field, &mut usage);
                    }
                }
            },
            | TypeBody::Abstract => {},
        }
        let mut kinds = Vec::with_capacity(params.len());
        let mut extra: Vec<(usize, AliasParamKind)> = Vec::new();
        for (index, param) in params.iter().enumerate() {
            let name = &param.name;
            let explicit = match param.kind {
                | Some(Kind::Type) => Some(AliasParamKind::Type),
                | Some(Kind::Dim) => Some(AliasParamKind::Dim),
                | Some(Kind::Nat) => Some(AliasParamKind::Nat),
                | Some(Kind::Row) => Some(AliasParamKind::Row),
                | Some(other) => {
                    self.error_help(
                        "E0317",
                        format!(
                            "the parameter `{name}` has a kind that a type declaration does not support: {other:?}"
                        ),
                        Span::new(self.file, 0, 0),
                        "use `Type`, `Dim`, `Nat` or `Row`, or omit the kind so it is inferred from the body",
                    );
                    Some(AliasParamKind::Type)
                },
                | None => None,
            };
            let dim_use = usage.dims.contains(name);
            let nat_use = usage.nats.contains(name);
            let row_use = usage.rows.contains(name);
            let candidates: Vec<AliasParamKind> = [
                (row_use, AliasParamKind::Row),
                (dim_use, AliasParamKind::Dim),
                (nat_use, AliasParamKind::Nat),
            ]
            .into_iter()
            .filter_map(|(used, kind)| used.then_some(kind))
            .collect();
            if explicit.is_none() && candidates.len() > 1 {
                self.error_help(
                    "E0317",
                    format!("cannot infer the kind of `{name}`: the body uses it in incompatible positions"),
                    Span::new(self.file, 0, 0),
                    "annotate it, for example `type Foo<d: Dim>`",
                );
                // Give the non-primary readings a placeholder too, so the
                // declaration does not cascade into bogus E0407/E0304 errors.
                for kind in candidates.iter().skip(1) {
                    extra.push((index, *kind));
                }
            }
            kinds.push(
                explicit
                    .or_else(|| candidates.first().copied())
                    .unwrap_or(AliasParamKind::Type),
            );
        }
        (kinds, extra)
    }

    /// Allocates the placeholder of every parameter of the declaration being
    /// checked and pushes it into the matching conversion environment.
    ///
    /// Returns the dimension/nat/row placeholder ids (type parameters use
    /// `Type::Param(position)` and need no id).
    fn allocate_declaration_params(
        &mut self,
        params: &[TypeParam],
        kinds: &[AliasParamKind],
        extra: &[(usize, AliasParamKind)],
    ) -> (Vec<u32>, Vec<u32>, Vec<u32>) {
        let mut dim_vars = vec![0; params.len()];
        let mut nat_vars = vec![0; params.len()];
        let mut row_vars = vec![0; params.len()];
        for (index, (param, kind)) in params.iter().zip(kinds.iter()).enumerate() {
            match kind {
                | AliasParamKind::Type => {
                    self.type_params.push((
                        param.name.clone(),
                        Type::Param(u32::try_from(index).unwrap_or(0)),
                    ));
                },
                | AliasParamKind::Dim => {
                    let dim = self.infer.fresh_dim();
                    dim_vars[index] = dim.vars().first().copied().unwrap_or(0);
                    self.dim_params.push((param.name.clone(), dim));
                },
                | AliasParamKind::Nat => {
                    let nat = self.infer.fresh_nat();
                    nat_vars[index] = nat.vars().first().copied().unwrap_or(0);
                    self.nat_params.push((param.name.clone(), nat));
                },
                | AliasParamKind::Row => {
                    let row = self.infer.fresh_row();
                    row_vars[index] = tail_row_id(&row).unwrap_or(0);
                    self.row_params.push((param.name.clone(), row));
                },
            }
        }
        // A parameter whose kind could not be inferred (E0317 was reported)
        // gets a placeholder for every reading, so the rest of the body still
        // resolves and only the kind error is reported.
        for (index, kind) in extra {
            let Some(param) = params.get(*index) else {
                continue;
            };
            match kind {
                | AliasParamKind::Type => {
                    self.type_params.push((
                        param.name.clone(),
                        Type::Param(u32::try_from(*index).unwrap_or(0)),
                    ));
                },
                | AliasParamKind::Dim => {
                    let dim = self.infer.fresh_dim();
                    dim_vars[*index] = dim.vars().first().copied().unwrap_or(0);
                    self.dim_params.push((param.name.clone(), dim));
                },
                | AliasParamKind::Nat => {
                    let nat = self.infer.fresh_nat();
                    nat_vars[*index] = nat.vars().first().copied().unwrap_or(0);
                    self.nat_params.push((param.name.clone(), nat));
                },
                | AliasParamKind::Row => {
                    let row = self.infer.fresh_row();
                    row_vars[*index] = tail_row_id(&row).unwrap_or(0);
                    self.row_params.push((param.name.clone(), row));
                },
            }
        }
        (dim_vars, nat_vars, row_vars)
    }

    /// The interface form of an alias body: placeholders become canonical ids
    /// derived from the parameter position, so an interface never carries
    /// another process's inference ids.
    fn canonical_alias_export(
        &self,
        def: &AliasDef,
    ) -> AliasExport {
        let mut dims = std::collections::HashMap::new();
        let mut nats = std::collections::HashMap::new();
        let mut rows = std::collections::HashMap::new();
        for (index, kind) in def.kinds.iter().enumerate() {
            let canonical = u32::try_from(index).unwrap_or(0);
            match kind {
                | AliasParamKind::Dim => {
                    dims.insert(def.dim_vars[index], Dim::var(canonical));
                },
                | AliasParamKind::Nat => {
                    nats.insert(def.nat_vars[index], Nat::Var(canonical));
                },
                | AliasParamKind::Row => {
                    rows.insert(def.row_vars[index], Row::Var(canonical));
                },
                | AliasParamKind::Type => {},
            }
        }
        let types = vec![None; def.params];
        AliasExport {
            params: def.params,
            body: def.ty.substitute_kinded(&types, &dims, &nats, &rows),
            kinds: def.kinds.clone(),
        }
    }

    /// Rebuilds an imported alias with this checker's own fresh placeholders.
    fn imported_alias_def(
        &mut self,
        alias: &AliasExport,
    ) -> AliasDef {
        let mut dims = std::collections::HashMap::new();
        let mut nats = std::collections::HashMap::new();
        let mut rows = std::collections::HashMap::new();
        let mut dim_vars = vec![0; alias.params];
        let mut nat_vars = vec![0; alias.params];
        let mut row_vars = vec![0; alias.params];
        for (index, kind) in alias.kinds.iter().enumerate() {
            let canonical = u32::try_from(index).unwrap_or(0);
            match kind {
                | AliasParamKind::Dim => {
                    let dim = self.infer.fresh_dim();
                    dim_vars[index] = dim.vars().first().copied().unwrap_or(0);
                    dims.insert(canonical, dim);
                },
                | AliasParamKind::Nat => {
                    let nat = self.infer.fresh_nat();
                    nat_vars[index] = nat.vars().first().copied().unwrap_or(0);
                    nats.insert(canonical, nat);
                },
                | AliasParamKind::Row => {
                    let row = self.infer.fresh_row();
                    row_vars[index] = tail_row_id(&row).unwrap_or(0);
                    rows.insert(canonical, row);
                },
                | AliasParamKind::Type => {},
            }
        }
        let types = vec![None; alias.params];
        AliasDef {
            ty: alias.body.substitute_kinded(&types, &dims, &nats, &rows),
            params: alias.params,
            kinds: alias.kinds.clone(),
            dim_vars,
            nat_vars,
            row_vars,
        }
    }

    /// Expands a use of a generic alias (§2 kind-aware parameters).
    ///
    /// A type-kinded argument is converted as a type, a dimension-kinded one as
    /// a dimension, a nat-kinded one as a type-level `Nat` and a row-kinded one
    /// as a record row. The alias body stores placeholders, so the expansion is
    /// a substitution rather than a re-conversion.
    fn instantiate_alias(
        &mut self,
        alias: &AliasDef,
        args: &[Ty],
        span: Span,
    ) -> Type {
        if args.len() > alias.params {
            self.error(
                "E0315",
                format!(
                    "the alias takes {} type argument(s), found {}",
                    alias.params,
                    args.len()
                ),
                span,
            );
        }
        let mut types: Vec<Option<Type>> = vec![None; alias.params];
        let mut dims = std::collections::HashMap::new();
        let mut nats = std::collections::HashMap::new();
        let mut rows = std::collections::HashMap::new();
        for (index, kind) in alias.kinds.iter().enumerate() {
            let Some(arg) = args.get(index) else {
                // A missing argument stays a fresh variable of its kind, the
                // way an unapplied generic alias behaves.
                match kind {
                    | AliasParamKind::Type => {},
                    | AliasParamKind::Dim => {
                        let dim = self.infer.fresh_dim();
                        dims.insert(alias.dim_vars[index], dim);
                    },
                    | AliasParamKind::Nat => {
                        let nat = self.infer.fresh_nat();
                        nats.insert(alias.nat_vars[index], nat);
                    },
                    | AliasParamKind::Row => {
                        let row = self.infer.fresh_row();
                        rows.insert(alias.row_vars[index], row);
                    },
                }
                continue;
            };
            match kind {
                | AliasParamKind::Type => types[index] = Some(self.convert_type(arg)),
                | AliasParamKind::Dim => {
                    let dim = self.convert_type_as_dim(arg);
                    dims.insert(alias.dim_vars[index], dim);
                },
                | AliasParamKind::Nat => {
                    let nat = self.convert_type_as_nat(arg);
                    nats.insert(alias.nat_vars[index], nat);
                },
                | AliasParamKind::Row => {
                    let row = self.convert_type_as_row(arg);
                    rows.insert(alias.row_vars[index], row);
                },
            }
        }
        alias.ty.substitute_kinded(&types, &dims, &nats, &rows)
    }

    /// Converts a type argument written for a dimension-kinded parameter.
    fn convert_type_as_dim(
        &mut self,
        ty: &Ty,
    ) -> Dim {
        let name = match &ty.kind {
            | TyKind::Named { path, args } if args.is_empty() => Some(path.join(".")),
            | TyKind::Var(name) => Some(name.clone()),
            | _ => None,
        };
        if let Some(name) = name {
            if let Some((_, dim)) = self
                .dim_params
                .iter()
                .rev()
                .find(|(param, _)| *param == name)
            {
                return dim.clone();
            }
            if let Some(dim) = self.dims.lookup(&name) {
                return dim;
            }
            if self.implicit_params {
                return self.infer.fresh_dim();
            }
            self.error_help(
                "E0407",
                format!("unknown dimension `{name}` in a type argument"),
                ty.span,
                "the parameter is dimension-kinded; pass a dimension such as `L`",
            );
            return Dim::one();
        }
        match &ty.kind {
            | TyKind::DimArg(dim) => self.convert_dim(dim, ty.span),
            | _ => {
                self.error_help(
                    "E0407",
                    "this type argument must be a dimension",
                    ty.span,
                    "declare the parameter as `<d: Dim>` or pass a dimension such as `L`",
                );
                Dim::one()
            },
        }
    }

    /// Converts a type argument written for a natural-number-kinded parameter.
    fn convert_type_as_nat(
        &mut self,
        ty: &Ty,
    ) -> Nat {
        match bare_type_name(ty) {
            | Some(name) => {
                if let Some((_, nat)) = self
                    .nat_params
                    .iter()
                    .rev()
                    .find(|(param, _)| param == name)
                {
                    return nat.clone();
                }
                self.error_help(
                    "E0304",
                    format!("cannot infer the shape extent `{name}`"),
                    ty.span,
                    "the parameter is a type-level natural; pass a number such as `3`",
                );
                Nat::Lit(0)
            },
            | None => {
                match &ty.kind {
                    | TyKind::NatArg(nat) => self.convert_nat(nat, ty.span),
                    | _ => {
                        self.error_help(
                            "E0304",
                            "this type argument must be a type-level natural number",
                            ty.span,
                            "declare the parameter as `<n: Nat>` or pass a number such as `3`",
                        );
                        Nat::Lit(0)
                    },
                }
            },
        }
    }

    /// Converts a type argument written for a row-kinded parameter.
    fn convert_type_as_row(
        &mut self,
        ty: &Ty,
    ) -> Row {
        let converted = self.convert_type(ty);
        match self.infer.resolve(&converted) {
            | Type::Record(row) => row,
            | other => {
                self.error_help(
                    "E0302",
                    format!("a row argument must be a record type, found {other}"),
                    ty.span,
                    "declare the parameter as `<r: Row>` and pass a record such as `{ x: Int }`",
                );
                Row::Empty
            },
        }
    }

    fn declare_type(
        &mut self,
        name: &str,
        params: &[TypeParam],
        body: &TypeBody,
        private: bool,
    ) {
        let saved_types = std::mem::take(&mut self.type_params);
        let saved_dims = std::mem::take(&mut self.dim_params);
        let saved_nats = std::mem::take(&mut self.nat_params);
        let saved_rows = std::mem::take(&mut self.row_params);
        let (kinds, extra_kinds) = self.declaration_param_kinds(params, body);
        let (dim_vars, nat_vars, row_vars) =
            self.allocate_declaration_params(params, &kinds, &extra_kinds);
        if self.adts.contains_key(name) || self.aliases.contains_key(name) {
            self.error(
                "E0316",
                format!("type `{name}` is already defined in this module"),
                Span::new(self.file, 0, 0),
            );
        }
        match body {
            | TypeBody::Alias(ty) => {
                // An alias is transparent, so referring to itself is an
                // infinite type, not a recursive one (§3.6.4).
                let saved = self.declaring_alias.replace(name.to_string());
                let converted = self.convert_type(ty);
                self.declaring_alias = saved;
                self.aliases.insert(
                    name.to_string(),
                    AliasDef {
                        ty: converted,
                        params: params.len(),
                        kinds,
                        dim_vars,
                        nat_vars,
                        row_vars,
                    },
                );
                if !private {
                    self.checked
                        .exports
                        .insert(name.to_string(), Export::Alias { params: params.len() });
                }
            },
            | TypeBody::Variants(ctors) => {
                // Allocate the id *before* converting the fields, so a field
                // that mentions the type being declared resolves (§3.6.4).
                // The defining module is recorded with it, so the symbolic key
                // `(module, name)` is the identity the interface carries.
                let id = adt::allocate_in(
                    self.module_path.clone(),
                    name,
                    params.iter().map(|p| p.name.clone()).collect(),
                );
                // §2/§8.9.1: a variant type may be parameterised by a
                // dimension (`type SdeOpts<T> = | Fixed(SdeFixedOpts<T>) | …`).
                // A dimension argument is encoded as `UnitOf(d)`, so the ADT's
                // argument vector stays a vector of types; constructor schemes
                // quantify the dimension.
                let mut adt_kinds = Vec::with_capacity(params.len());
                for (index, kind) in kinds.iter().enumerate() {
                    match kind {
                        | AliasParamKind::Type => adt_kinds.push(adt::ParamKind::Type),
                        | AliasParamKind::Dim => {
                            adt_kinds.push(adt::ParamKind::Dim(dim_vars[index]));
                        },
                        | AliasParamKind::Nat | AliasParamKind::Row => {
                            self.error_help(
                                "E0317",
                                format!(
                                    "the variant type `{name}` cannot take a {} parameter",
                                    if matches!(kind, AliasParamKind::Nat) {
                                        "natural-number"
                                    } else {
                                        "row"
                                    }
                                ),
                                Span::new(self.file, 0, 0),
                                "a variant type may be parameterised by types and dimensions; use a record alias for a row/nat parameter",
                            );
                            adt_kinds.push(adt::ParamKind::Type);
                        },
                    }
                }
                adt::set_param_kinds(id, adt_kinds);
                self.adts.insert(name.to_string(), id);
                self.local_adts.insert(id);
                let mut converted = Vec::new();
                for ctor in ctors {
                    let fields: Vec<Type> = ctor
                        .args
                        .iter()
                        .map(|field| self.convert_type(field))
                        .collect();
                    for (field_ty, written) in fields.iter().zip(ctor.args.iter()) {
                        self.check_positivity(id, field_ty, true, written.span);
                    }
                    converted.push(Ctor {
                        name: ctor.name.clone(),
                        fields,
                    });
                }
                adt::set_ctors(id, converted);
                if !private {
                    self.checked
                        .exports
                        .insert(name.to_string(), Export::Type { adt: id });
                }
                if let Some(registered) = adt::get(id) {
                    for (index, ctor) in registered.ctors.iter().enumerate() {
                        let scheme = ctor_scheme(id, index);
                        self.checked
                            .constructors
                            .insert(ctor.name.clone(), (id, index));
                        self.constructor_bindings
                            .insert(ctor.name.clone(), (id, index));
                        self.globals.insert(ctor.name.clone(), scheme.clone());
                        if private {
                            continue;
                        }
                        // `type P2 = | P2(...)` names the type and the
                        // constructor the same; keep both, or one of the two
                        // meanings disappears for importers.
                        let export = if ctor.name == *name {
                            Export::TypeCtor {
                                adt: id,
                                index,
                                scheme,
                            }
                        } else {
                            Export::Ctor {
                                adt: id,
                                index,
                                scheme,
                            }
                        };
                        self.checked.exports.insert(ctor.name.clone(), export);
                    }
                }
            },
            | TypeBody::Abstract => {
                for kind in &kinds {
                    if !matches!(kind, AliasParamKind::Type) {
                        self.error_help(
                            "E0317",
                            format!("the abstract type `{name}` can only take type parameters"),
                            Span::new(self.file, 0, 0),
                            "an opaque runtime type is parameterised by types, not by dimensions or rows",
                        );
                        break;
                    }
                }
                let id = adt::allocate_in(
                    self.module_path.clone(),
                    name,
                    params.iter().map(|p| p.name.clone()).collect(),
                );
                adt::set_param_kinds(id, vec![adt::ParamKind::Type; params.len()]);
                adt::set_ctors(id, Vec::new());
                self.adts.insert(name.to_string(), id);
                self.local_adts.insert(id);
                if !private {
                    self.checked
                        .exports
                        .insert(name.to_string(), Export::Type { adt: id });
                }
            },
        }
        self.type_params = saved_types;
        self.dim_params = saved_dims;
        self.nat_params = saved_nats;
        self.row_params = saved_rows;
    }

    /// Rejects a recursive occurrence on the left of an arrow (§3.6.4).
    ///
    /// `type Bad = Bad -> Int` has no well-founded value: checking it is
    /// `E0312`. `type Tree<a> = | Node(Tree<a>, a, Tree<a>)` is positive and
    /// therefore legal.
    fn check_positivity(
        &mut self,
        id: adt::AdtId,
        ty: &Type,
        positive: bool,
        span: Span,
    ) {
        match ty {
            | Type::Adt(other, args) => {
                if *other == id && !positive {
                    self.error_help(
                        "E0312",
                        "non-positive recursive type",
                        span,
                        "the type may only appear to the right of an arrow",
                    );
                }
                for arg in args {
                    self.check_positivity(id, arg, positive, span);
                }
            },
            | Type::Tensor { elem, .. } => self.check_positivity(id, elem, positive, span),
            | Type::Fn(params, ret) => {
                for param in params {
                    self.check_positivity(id, param, false, span);
                }
                self.check_positivity(id, ret, positive, span);
            },
            | Type::Record(row) => {
                for (_, field) in row.fields() {
                    self.check_positivity(id, &field, positive, span);
                }
            },
            | Type::Io(inner) => self.check_positivity(id, inner, positive, span),
            | _ => {},
        }
    }

    fn declare_class(
        &mut self,
        name: &str,
        params: &[TypeParam],
        methods: &[(String, Ty)],
        span: Span,
        private: bool,
    ) {
        // §3.8.1, round-2 decision 1f: the language has exactly one class
        // parameter. Multi-parameter classes are removed from the standard, so
        // declaring one is rejected rather than half-supported (the second
        // parameter was never solved or checked — probe P7).
        if params.len() > 1 {
            self.error_help(
                "E0317",
                format!(
                    "class `{name}` declares {} parameters; a class takes exactly one",
                    params.len()
                ),
                span,
                "remove the extra parameters; generic classes are not part of the language (§3.8.1)",
            );
            return;
        }
        let saved_types = std::mem::take(&mut self.type_params);
        let saved_dims = std::mem::take(&mut self.dim_params);
        let saved_nats = std::mem::take(&mut self.nat_params);
        for (index, param) in params.iter().enumerate() {
            // `Param`, not `Var`: a class parameter is a binder, and an
            // instance substitutes the head for it with `substitute_params`.
            // `method_scheme` converts it to a quantified `Var` when the
            // method becomes a scheme.
            self.type_params.push((
                param.name.clone(),
                Type::Param(u32::try_from(index).unwrap_or(0)),
            ));
        }
        let mut converted = Vec::new();
        for (method, ty) in methods {
            let method_ty = self.convert_type(ty);
            converted.push((method.clone(), method_ty.clone()));
            let scheme = crate::class::method_scheme(name, params.len().max(1), method_ty);
            self.globals.insert(method.clone(), scheme);
            self.method_bindings.insert(
                method.clone(),
                ClassMethodRef {
                    class: ClassKey::new(self.module_path.clone(), name),
                    method: method.clone(),
                },
            );
        }
        self.type_params = saved_types;
        self.dim_params = saved_dims;
        self.nat_params = saved_nats;
        match self.classes.declare_class(
            self.module_path.clone(),
            name,
            params.len().max(1),
            converted,
        ) {
            | Ok(()) => {
                if private {
                    // §7.1.3: a `private class` is usable inside the module and
                    // is not part of its exported interface.
                    self.private_classes.insert(name.to_string());
                }
            },
            | Err(ClassError::Duplicate) => {
                self.error("E0316", format!("class `{name}` is already declared"), span);
            },
            | Err(ClassError::MultiParameter) => {
                // Unreachable: the arity is checked above. Kept total.
                self.error(
                    "E0317",
                    format!("class `{name}` declares more than one parameter"),
                    span,
                );
            },
        }
    }

    /// Checks an `instance` and registers it (§3.8.1).
    ///
    /// The methods are *not* installed as globals of their own name: that is
    /// how the old checker let one instance shadow `show` for the whole module.
    /// Each method gets a mangled global, and the call site records which
    /// instance the checker proved.
    fn check_instance(
        &mut self,
        decl: &Decl,
    ) {
        let DeclKind::Instance {
            class,
            ty: head_ast,
            methods,
        } = &decl.kind
        else {
            return;
        };
        let head = self.convert_instance_head(head_ast);
        // §3.5.1: an instance head must be a canonical type, and there is no
        // syntax to quantify a Nat variable per instance (`instance C for
        // Tensor<[n], Float]` has no binder for `n`), so a variable extent
        // cannot name a head. It would also make the witness/overlap keys
        // ambiguous at run time.
        if let Some(variable) = self.nat_variable_in(&head) {
            self.error_help(
                "E0304",
                format!("the instance head cannot contain the type-level variable `{variable}`"),
                head_ast.span,
                "write a concrete extent, or use a rank-only head such as `Tensor<1, Float>`",
            );
            return;
        }
        // A head must be a type constructor (or a record): `instance C for a`
        // would match every type in the class, which the grammar (§3.8.1) does
        // not express and the resolver cannot honour.
        if matches!(head, Type::Var(_) | Type::Param(_)) {
            self.error_help(
                "E0317",
                "an instance head must be a type constructor, not a type variable",
                decl.span,
                "write the concrete type, e.g. `instance Show for Vec2`",
            );
            return;
        }
        let Some(definition) = self.classes.class(class).cloned() else {
            self.error_help(
                "E0310",
                format!("no class `{class}` is in scope"),
                decl.span,
                "declare it with `class ... where`, or import the module that does",
            );
            return;
        };
        // A method the class does not declare cannot be part of the instance:
        // the call site could never resolve it.
        for instance_method in methods {
            if !definition
                .methods
                .iter()
                .any(|(name, _)| *name == instance_method.name)
            {
                self.error_help(
                    "E0317",
                    format!(
                        "`{}` is not a method of class `{class}`",
                        instance_method.name
                    ),
                    instance_method.span,
                    "remove it, or declare it in the class",
                );
            }
        }
        let mut specialised: HashMap<String, Type> = HashMap::new();
        let mut missing: Vec<String> = Vec::new();
        for (method, declared) in &definition.methods {
            let Some(instance_method) = methods.iter().find(|m| m.name == *method) else {
                missing.push(method.clone());
                continue;
            };
            // The instance's method must have the class method's type with the
            // head substituted for the class parameter.
            let expected = declared.substitute_params(std::slice::from_ref(&head));
            self.scopes.push(HashMap::new());
            let mut params = Vec::new();
            let Type::Fn(expected_params, expected_ret) = self.infer.resolve(&expected) else {
                self.scopes.pop();
                continue;
            };
            for (index, param) in instance_method.params.iter().enumerate() {
                let ty = match (&param.ty, expected_params.get(index)) {
                    | (Some(annotation), _) => {
                        let annotated = self.convert_type(annotation);
                        if let Some(want) = expected_params.get(index)
                            && let Err(error) = self.infer.unify(&annotated, want)
                        {
                            self.report_unify(error, param.span);
                        }
                        annotated
                    },
                    | (None, Some(want)) => want.clone(),
                    | (None, None) => self.infer.fresh_type(),
                };
                self.bind_pattern(&param.pattern, &ty);
                params.push(ty);
            }
            self.callable_ids.insert(
                instance_method.span,
                CallableId::new(
                    &self.module_path,
                    instance_method.span,
                    &aevia_syntax::fingerprint::method(instance_method),
                ),
            );
            self.function_inputs
                .insert(instance_method.span, params.clone());
            self.function_stack.push(instance_method.span);
            let body_ty = self.synth(&instance_method.body);
            if let Err(error) = self.infer.unify(&body_ty, &expected_ret) {
                self.report_unify(error, instance_method.span);
            }
            self.commit_function_literals(instance_method.span);
            self.function_stack.pop();
            self.scopes.pop();
            let method_ty = Type::func(params.clone(), (*expected_ret).clone());
            let mut scheme = self.infer.generalize_at(0, &method_ty, Vec::new());
            scheme.materializations = self.function_materializations(instance_method.span);
            self.infer.attach_numeric_scheme(&mut scheme);
            self.infer.quantify_numeric_inputs(&mut scheme, &params, 0);
            self.infer.quantify_numeric_existentials(&mut scheme, 0);
            self.checked.functions.insert(
                instance_method.span,
                FunctionEvidence {
                    site: instance_method.span,
                    identity: self
                        .callable_ids
                        .get(&instance_method.span)
                        .expect("seeded callable identity")
                        .clone(),
                    numeric_literals: Vec::new(),
                    scheme,
                    binders: Vec::new(),
                    reifications: self
                        .function_reifications
                        .remove(&instance_method.span)
                        .unwrap_or_default(),
                },
            );
            specialised.insert(method.clone(), method_ty);
        }
        // §3.8.1: a user class requires every method. A prelude class is a
        // *hook*: a partial override is allowed and the builtin implementation
        // covers the rest (round-2 decision 1e).
        if !missing.is_empty() && !definition.builtin {
            self.error_help(
                "E0317",
                format!(
                    "this `{class}` instance does not define {}",
                    missing.join(", ")
                ),
                decl.span,
                "every method of the class must be defined",
            );
        }
        let head_is_local = self.head_is_local_ast(head_ast);
        let class_is_local = self
            .classes
            .class_key(class)
            .is_some_and(|key| key.module == self.module_path);
        match self.classes.add_instance(
            &self.module_path,
            class,
            head,
            specialised.clone(),
            Origin::User,
            head_is_local,
            class_is_local,
        ) {
            Ok(key) => {
                self.instance_declarations.insert(decl.span, key.clone());
                // The body is reachable under a name no program can write, so
                // it cannot shadow the class method. The name is derived from
                // the *symbolic* instance key, so two primitive-head instances
                // (`Int` and `String`) get distinct globals — the old runtime
                // mangling used `Class@?` for every non-nominal head and let
                // them overwrite each other (round-2 decision 1a).
                for (method, method_ty) in &specialised {
                    self.globals.insert(
                        format!("{}::{}", key.mangle(), method),
                        Scheme::mono(method_ty.clone()),
                    );
                }
            }
            Err(InstanceError::Overlapping { incoming, existing }) => self.error_help(
                "E0311",
                format!(
                    "overlapping `{class}` instances: `{incoming}` and `{existing}` both match this type"
                ),
                decl.span,
                "two instances match the same type; merge or delete one",
            ),
            Err(InstanceError::WitnessCollision { incoming, existing }) => self.error_help(
                "E0311",
                format!(
                    "`{incoming}` and `{existing}` cannot be told apart at run time"
                ),
                decl.span,
                "their runtime witnesses are identical (`Tensor<1, Int>` and `Tensor<1, Real>` are both exact storage); use one instance",
            ),
            Err(InstanceError::Orphan) => self.error_help(
                "E0604",
                format!("orphan instance: neither `{class}` nor this type is local"),
                decl.span,
                "define the instance in the module that declares the type or the class",
            ),
            Err(InstanceError::UnknownClass | InstanceError::MultiParameter) => {}
        }
    }

    /// Whether an instance head names a type this module declares.
    ///
    /// Aliases are transparent, so the written name is what decides: `Vec2`
    /// declared here is local even though its expansion is a record type.
    /// The first free type-level Nat variable in a type, rendered for a
    /// diagnostic (used to reject an instance head such as
    /// `Tensor<[n], Float]`, §3.5.1/§3.8.1).
    fn nat_variable_in(
        &self,
        ty: &Type,
    ) -> Option<String> {
        match ty {
            | Type::Tensor { shape, elem } => {
                if let Shape::Refined(items) = shape {
                    for item in items.iter().flatten() {
                        let resolved = self.infer.resolve_nat(item);
                        if let Some(id) = resolved.vars().into_iter().next() {
                            return Some(format!("n{id}"));
                        }
                    }
                }
                self.nat_variable_in(elem)
            },
            | Type::Fn(params, ret) => {
                params
                    .iter()
                    .find_map(|param| self.nat_variable_in(param))
                    .or_else(|| self.nat_variable_in(ret))
            },
            | Type::Record(row) => {
                row.fields()
                    .into_iter()
                    .find_map(|(_, field)| self.nat_variable_in(&field))
            },
            | Type::Adt(_, args) => args.iter().find_map(|arg| self.nat_variable_in(arg)),
            | Type::Io(inner) => self.nat_variable_in(inner),
            | _ => None,
        }
    }

    fn head_is_local_ast(
        &self,
        ty: &Ty,
    ) -> bool {
        match &ty.kind {
            | TyKind::Named { path, .. } => {
                let name = path.join(".");
                self.adts
                    .get(&name)
                    .is_some_and(|id| self.local_adts.contains(id))
                    || self.aliases.contains_key(&name)
            },
            | TyKind::Record { .. } => true,
            | _ => false,
        }
    }

    /// Converts an instance head, treating unknown type and dimension names as
    /// implicitly quantified variables (§3.8.1).
    fn convert_instance_head(
        &mut self,
        ty: &Ty,
    ) -> Type {
        let saved_types = std::mem::take(&mut self.type_params);
        let saved_dims = std::mem::take(&mut self.dim_params);
        let saved_nats = std::mem::take(&mut self.nat_params);
        let saved_rows = std::mem::take(&mut self.row_params);
        let saved_flag = self.implicit_params;
        self.implicit_params = true;
        let converted = self.convert_type(ty);
        self.implicit_params = saved_flag;
        self.type_params = saved_types;
        self.dim_params = saved_dims;
        self.nat_params = saved_nats;
        self.row_params = saved_rows;
        converted
    }

    /// Checks a `derive` declaration (§3.8.5).
    fn check_derive(
        &mut self,
        decl: &Decl,
    ) {
        let DeclKind::Derive { classes, ty } = &decl.kind else {
            return;
        };
        let head = self.convert_type(ty);
        // A `derive` works on a nominal type or on a record alias: both are
        // structurally known here.
        let fields: Vec<Type> = match &head {
            | Type::Adt(id, _) => {
                match adt::get(*id) {
                    | Some(registered) => {
                        registered
                            .ctors
                            .iter()
                            .flat_map(|ctor| ctor.fields.clone())
                            .collect()
                    },
                    | None => Vec::new(),
                }
            },
            | Type::Record(row) => row.fields().into_iter().map(|(_, ty)| ty).collect(),
            | _ => {
                self.error_help(
                    "E0317",
                    "`derive` needs a nominal type or a record type",
                    decl.span,
                    "deriving works on `type` declarations",
                );
                return;
            },
        };
        let head_is_local = self.head_is_local_ast(ty);
        for class in classes {
            let class_is_local = self
                .classes
                .class_key(class)
                .is_some_and(|key| key.module == self.module_path);
            let result = self.classes.add_derived(
                &self.module_path,
                class,
                head.clone(),
                &fields,
                head_is_local,
                class_is_local,
            );
            match result {
                | Ok(_) => {},
                | Err(DeriveError::UnknownClass(_)) => {
                    self.error_help(
                        "E0310",
                        format!("no class `{class}` is in scope"),
                        decl.span,
                        "declare it with `class ... where`, or import the module that does",
                    )
                },
                | Err(DeriveError::UnsupportedClass(unsupported)) => {
                    self.error_help(
                        "E0317",
                        format!("`derive` cannot instantiate `{unsupported}`"),
                        decl.span,
                        "`derive` is closed to Eq, Ord, TotalOrd, Show and Hash (§3.8.5)",
                    )
                },
                | Err(DeriveError::Field { class: failed, field }) => self.error_help(
                    "E0317",
                    format!(
                        "cannot derive `{failed}`: the field `{field}` has no `{failed}` instance"
                    ),
                    decl.span,
                    "write the instance by hand, or derive over a field type that supports it",
                ),
                | Err(DeriveError::FunctionField { class: failed, field }) => {
                    self.error_help(
                        "E0317",
                        format!(
                            "cannot derive `{failed}`: the field `{field}` contains a function"
                        ),
                        decl.span,
                        "functions have no Eq/Ord/Show/Hash instance",
                    )
                },
                | Err(DeriveError::Overlapping(key)) => {
                    self.error_help(
                        "E0311",
                        format!("an instance `{key}` already exists"),
                        decl.span,
                        "remove the derive or the hand-written instance",
                    )
                },
                | Err(DeriveError::Orphan) => {
                    self.error_help(
                        "E0604",
                        "orphan instance: `derive` can only instantiate a class for a local type",
                        decl.span,
                        "derive in the module that declares the type",
                    )
                },
            }
        }
    }

    /// Solves a class constraint, or defers it until the type is known.
    fn solve_constraint(
        &mut self,
        constraint: &Constraint,
        span: Span,
    ) {
        let Some(arg) = constraint.args.first() else {
            return;
        };
        let resolved = self.infer.resolve(arg);
        match self.classes.resolve(&constraint.class, &resolved) {
            | Resolution::Found { .. } => {},
            | Resolution::Deferred => {
                self.deferred_constraints
                    .push((constraint.class.clone(), resolved, span))
            },
            | Resolution::Missing if is_structural_show(&constraint.class, &resolved) => {
                // §3.8.4/round-2 decision 1e: `Show` is a *guarantee* for the
                // structural heads; the builtin printer is the instance.
            },
            | Resolution::Missing => {
                let heads = self.classes.instance_heads(&constraint.class);
                let known = heads
                    .iter()
                    .map(ToString::to_string)
                    .collect::<Vec<_>>()
                    .join(", ");
                self.diagnostics.push(
                    Diagnostic::error(
                        "E0310",
                        format!("no instance of `{}` for `{resolved}`", constraint.class),
                        span,
                    )
                    .with_primary_label("this type has no instance")
                    .with_note(if known.is_empty() {
                        "the class has no instances".to_string()
                    } else {
                        format!("instances: {known}")
                    })
                    .with_help("add an `instance` declaration, or use a type that has one"),
                );
            },
        }
    }

    /// `to` and `value_in` with a literal unit argument.
    ///
    /// Returns `None` when the call does not have the shape this rule covers,
    /// so the builtin signature still applies.
    fn unit_display_call(
        &mut self,
        name: &str,
        args: &[Expr],
        span: Span,
    ) -> Option<Type> {
        let [unit_expr, value] = args else {
            return None;
        };
        let unit_name = unit_name_of(unit_expr)?;
        let unit = self.units.lookup(&unit_name)?;
        let synth_value = self.synth(value);
        let value_ty = self.infer.resolve(&synth_value);
        match value_ty {
            | Type::Point(dim) => {
                if dim.is_concrete() && unit.dim.is_concrete() && dim != unit.dim {
                    self.error_help(
                        "E0509",
                        format!("`{unit_name}` measures a different dimension"),
                        span,
                        "convert the value to the unit's dimension first",
                    );
                    return Some(Type::Error);
                }
                Some(if name == "to" {
                    Type::Point(dim)
                } else {
                    Type::float(Dim::one())
                })
            },
            | Type::Scalar(rep, dim) => {
                if unit.affine {
                    self.error_help(
                        "E0510",
                        format!("`{unit_name}` is an affine unit, but this is a temperature difference"),
                        value.span,
                        "use `delta_to` to display a difference, or a `Point` for an absolute temperature",
                    );
                    return Some(Type::Error);
                }
                if dim.is_concrete() && unit.dim.is_concrete() && dim != unit.dim {
                    self.error_help(
                        "E0509",
                        format!("`{unit_name}` measures a different dimension"),
                        span,
                        "convert the value to the unit's dimension first",
                    );
                    return Some(Type::Error);
                }
                Some(if name == "to" {
                    Type::Scalar(rep, dim)
                } else {
                    Type::Scalar(rep, Dim::one())
                })
            },
            | _ => None,
        }
    }

    /// Reports `E0310` when a concrete type has no instance of `class`.
    ///
    /// A type that is still unknown is re-checked at the end of the module,
    /// once defaulting has made it concrete: `sort([1.0, 2.0])` is
    /// `sort : Tensor<1, Float>` and `Float` has no `TotalOrd`.
    fn require_instance(
        &mut self,
        class: &str,
        ty: &Type,
        span: Span,
    ) {
        let resolved = self.infer.resolve(ty);
        match self.classes.resolve(class, &resolved) {
            | Resolution::Missing if is_structural_show(class, &resolved) => {},
            | Resolution::Missing => {
                self.error_help(
                    "E0310",
                    format!("no instance of `{class}` for `{resolved}`"),
                    span,
                    "this type has no total order; use a type that has one",
                );
            },
            | Resolution::Deferred => {
                self.deferred_constraints
                    .push((class.to_string(), resolved, span))
            },
            | Resolution::Found { .. } => {},
        }
    }

    /// Re-solves the constraints that were deferred, reporting `E0310` for the
    /// ones that never became concrete (§3.8.2 rule 3).
    fn flush_deferred_constraints(&mut self) {
        let deferred = std::mem::take(&mut self.deferred_constraints);
        for (class, ty, span) in deferred {
            let resolved = self.infer.resolve(&ty);
            match self.classes.resolve(&class, &resolved) {
                | Resolution::Found { .. } => {},
                | Resolution::Missing if is_structural_show(&class, &resolved) => {},
                | Resolution::Deferred | Resolution::Missing => {
                    self.error_help(
                        "E0310",
                        format!("no instance of `{class}` for `{resolved}`"),
                        span,
                        "add a type annotation that fixes the type, or an `instance` declaration",
                    );
                },
            }
        }
    }

    fn lexical_numeric_variables(&self) -> crate::TypeSubstitution {
        let mut free = crate::TypeSubstitution::default();
        let mut names = Infer::new();
        for (_, ty) in &self.type_params {
            free.freshen(&self.infer.resolve(ty), &mut names);
        }
        for (_, dim) in &self.dim_params {
            free.freshen(&Type::UnitOf(self.infer.resolve_dim(dim)), &mut names);
        }
        for (_, nat) in &self.nat_params {
            free.freshen(
                &Type::tensor(
                    Shape::Refined(vec![Some(self.infer.resolve_nat(nat))]),
                    Type::Unit,
                ),
                &mut names,
            );
        }
        for (_, row) in &self.row_params {
            free.freshen(&Type::Record(self.infer.resolve_row(row)), &mut names);
        }
        for site in &self.function_stack {
            if let Some(inputs) = self.function_inputs.get(site) {
                for ty in inputs {
                    free.freshen(&self.infer.resolve(ty), &mut names);
                }
            }
        }
        free
    }

    /// Commit a body's private literal readings before its scheme is
    /// published. A candidate may not silently specialize an input or an
    /// enclosing universal parameter; such literals stay dependent instead.
    fn commit_function_literals(
        &mut self,
        site: Span,
    ) {
        let protected = self.lexical_numeric_variables();
        let protected_domains: Vec<_> = protected
            .representations
            .keys()
            .filter_map(|id| {
                self.infer
                    .representation_domain(Rep::Var(*id))
                    .ok()
                    .map(|domain| (*id, domain))
            })
            .collect();
        let sites = self.owned_literal_sites(site);
        for (span, kind) in sites {
            let Some(ty) = self.latest_expr_type(span).map(|ty| self.infer.resolve(ty)) else {
                continue;
            };
            let mut free = crate::TypeSubstitution::default();
            free.freshen(&ty, &mut Infer::new());
            if free.types.keys().any(|id| protected.types.contains_key(id))
                || free
                    .representations
                    .keys()
                    .any(|id| protected.representations.contains_key(id))
            {
                continue;
            }
            let mut contract = Scheme::mono(ty.clone());
            self.infer.attach_numeric_scheme(&mut contract);
            let mut connected = crate::TypeSubstitution::default();
            for constraint in &contract.numeric {
                for ty in constraint.types() {
                    connected.freshen(&ty, &mut Infer::new());
                }
            }
            let mut inputs: Vec<_> = connected
                .representations
                .keys()
                .filter(|id| protected.representations.contains_key(id))
                .copied()
                .collect();
            inputs.sort_unstable();
            if !inputs.is_empty() {
                if let Err(error) =
                    self.infer
                        .add_numeric_constraint(crate::NumericConstraint::DefaultLiteral {
                            ty: ty.clone(),
                            kind,
                            inputs: inputs.into_iter().map(Rep::Var).collect(),
                        })
                {
                    self.report_unify(error, span);
                }
                continue;
            }
            let preferred = match &ty {
                | Type::Scalar(rep, _) => self.infer.rep_literal_kind(*rep).unwrap_or(kind),
                | _ => self.infer.literal_kind(&ty).unwrap_or(kind),
            };
            for candidate in [
                Rep::default_for_literal(preferred),
                Rep::Int,
                Rep::Float,
                Rep::Real,
                Rep::Complex,
            ] {
                if !kind.accepts_rep(candidate) || !preferred.accepts_rep(candidate) {
                    continue;
                }
                let reading = match &ty {
                    | Type::Var(_) => Type::Scalar(candidate, Dim::one()),
                    | Type::Scalar(Rep::Var(_), dim) => Type::Scalar(candidate, dim.clone()),
                    | _ => break,
                };
                // Probe only this connected numeric component. Cloning the
                // entire module's inference state for every `fn f_i() = 1`
                // made otherwise independent declarations quadratic.
                let mut trial = Infer::new();
                let mut projection = crate::TypeSubstitution::default();
                projection.freshen(&ty, &mut trial);
                for clause in &contract.numeric {
                    for ty in clause.types() {
                        projection.freshen(&ty, &mut trial);
                    }
                }
                let valid = contract.numeric.iter().all(|clause| {
                    trial
                        .add_numeric_constraint(clause.substitute(&projection))
                        .is_ok()
                });
                if !valid
                    || trial
                        .unify(&projection.apply(&ty), &projection.apply(&reading))
                        .is_err()
                {
                    continue;
                }
                if protected_domains.iter().any(|(id, old)| {
                    projection.representations.get(id).is_some_and(|rep| {
                        trial
                            .representation_domain(*rep)
                            .is_ok_and(|domain| domain != *old)
                    })
                }) {
                    continue;
                }
                if let Err(error) = self.infer.unify(&ty, &reading) {
                    self.report_unify(error, span);
                }
                break;
            }
        }
    }

    fn value_materializations(
        &self,
        site: Span,
    ) -> Vec<Type> {
        self.call_materializations
            .get(&site)
            .cloned()
            .or_else(|| {
                self.checked
                    .functions
                    .get(&site)
                    .map(|function| function.scheme.materializations.clone())
            })
            .unwrap_or_default()
    }

    fn function_materializations(
        &self,
        site: Span,
    ) -> Vec<Type> {
        let mut result = self
            .function_reifications
            .get(&site)
            .cloned()
            .unwrap_or_default();
        for (span, _) in self.owned_literal_sites(site) {
            if let Some(ty) = self.latest_expr_type(span) {
                result.push(self.infer.resolve(ty));
            }
        }
        result
    }

    fn weaken_data_argument(
        &mut self,
        ty: &Type,
    ) {
        let ty = self.infer.resolve(ty);
        // Callable values transport schemes; eagerly stored numeric data
        // transport a single representation. Recurse only through data.
        match &ty {
            | Type::Fn(..) => {},
            | Type::Tensor { elem, .. } | Type::Io(elem) => self.weaken_data_argument(elem),
            | Type::Record(row) => {
                for (_, field) in row.fields() {
                    self.weaken_data_argument(&field);
                }
            },
            | Type::Adt(_, args) => {
                for arg in args {
                    self.weaken_data_argument(arg);
                }
            },
            | Type::Scalar(..) | Type::Var(_)
                if self.infer.literal_kind(&ty).is_some() || matches!(ty, Type::Scalar(..)) =>
            {
                let protected = self.lexical_numeric_variables();
                self.infer.weaken_materialized(&ty, &protected);
            },
            | _ => {},
        }
    }

    fn fn_signature(
        &mut self,
        params: &[TypeParam],
        args: &[Param],
        ret: Option<&Ty>,
    ) -> (Type, FnGenerics) {
        let mut usage = ParamUsage::default();
        for arg in args {
            if let Some(ty) = &arg.ty {
                usage.scan(ty);
            }
        }
        if let Some(ty) = ret {
            usage.scan(ty);
        }
        let saved_types = self.type_params.clone();
        let saved_dims = self.dim_params.clone();
        let saved_nats = self.nat_params.clone();
        let saved_rows = self.row_params.clone();
        for param in params {
            match param.kind {
                | Some(Kind::Dim) => {
                    let dim = self.infer.fresh_dim();
                    self.dim_params.push((param.name.clone(), dim));
                },
                // §3.5.1: a type-level `Nat` parameter is a real variable; the
                // solver may bind it from a shape equation.
                | Some(Kind::Nat) => {
                    let nat = self.infer.fresh_nat();
                    self.infer.name_nat(&nat, &param.name, true);
                    self.nat_params.push((param.name.clone(), nat));
                },
                // §2/§3.6: a row parameter is usable as a record tail.
                | Some(Kind::Row) => {
                    let row = self.infer.fresh_row();
                    self.row_params.push((param.name.clone(), row));
                },
                | Some(Kind::Rep) => {
                    let rep = self.infer.fresh_rep();
                    self.type_params
                        .push((param.name.clone(), Type::Scalar(rep, Dim::one())));
                },
                | Some(Kind::Type) | None => {
                    let ty = self.infer.fresh_type();
                    self.type_params.push((param.name.clone(), ty));
                    // An unkinded parameter may also be used as a dimension
                    // (`fn mean<d>(xs: Real[d])`), as a shape extent
                    // (`fn zero<n>(xs: Tensor<[n], Float>)`) or as a record
                    // tail; give it every reading and let the use site choose.
                    if param.kind.is_none() {
                        let dim = self.infer.fresh_dim();
                        self.dim_params.push((param.name.clone(), dim));
                        let nat = self.infer.fresh_nat();
                        self.infer.name_nat(&nat, &param.name, true);
                        self.nat_params.push((param.name.clone(), nat));
                        let row = self.infer.fresh_row();
                        self.row_params.push((param.name.clone(), row));
                    }
                },
                | _ => {},
            }
        }
        let generics = FnGenerics {
            types: self.type_params.clone(),
            dims: self.dim_params.clone(),
            nats: self.nat_params.clone(),
            rows: self.row_params.clone(),
            binders: params
                .iter()
                .filter_map(|param| {
                    let kind = param.kind.unwrap_or_else(|| {
                        if usage.dims.contains(&param.name) {
                            Kind::Dim
                        } else if usage.nats.contains(&param.name) {
                            Kind::Nat
                        } else if usage.rows.contains(&param.name) {
                            Kind::Row
                        } else {
                            Kind::Type
                        }
                    });
                    match kind {
                        | Kind::Type => {
                            self.type_params
                                .iter()
                                .rev()
                                .find(|(name, _)| name == &param.name)
                                .map(|(_, ty)| GenericBinder::Type(ty.clone()))
                        },
                        | Kind::Rep => {
                            self.type_params
                                .iter()
                                .rev()
                                .find(|(name, _)| name == &param.name)
                                .and_then(|(_, ty)| {
                                    match ty {
                                        | Type::Scalar(rep, _) => {
                                            Some(GenericBinder::Representation(*rep))
                                        },
                                        | _ => None,
                                    }
                                })
                        },
                        | Kind::Dim => {
                            self.dim_params
                                .iter()
                                .rev()
                                .find(|(name, _)| name == &param.name)
                                .map(|(_, dim)| GenericBinder::Dimension(dim.clone()))
                        },
                        | Kind::Nat => {
                            self.nat_params
                                .iter()
                                .rev()
                                .find(|(name, _)| name == &param.name)
                                .map(|(_, nat)| GenericBinder::Natural(nat.clone()))
                        },
                        | Kind::Row => {
                            self.row_params
                                .iter()
                                .rev()
                                .find(|(name, _)| name == &param.name)
                                .map(|(_, row)| GenericBinder::Row(row.clone()))
                        },
                        | _ => None,
                    }
                })
                .collect(),
        };
        let mut arg_types = Vec::new();
        for arg in args {
            let ty = match &arg.ty {
                | Some(ty) => self.convert_type(ty),
                | None => self.infer.fresh_type(),
            };
            arg_types.push(ty);
        }
        let ret_ty = match ret {
            | Some(ty) => self.convert_type(ty),
            | None => self.infer.fresh_type(),
        };
        self.type_params = saved_types;
        self.dim_params = saved_dims;
        self.nat_params = saved_nats;
        self.row_params = saved_rows;
        (Type::func(arg_types, ret_ty), generics)
    }

    fn check_fn_body(
        &mut self,
        name: &str,
        args: &[Param],
        body: &Expr,
        site: Span,
    ) {
        let Some(signature) = self.fn_signatures.get(name).cloned() else {
            return;
        };
        let Type::Fn(params, ret) = signature else {
            return;
        };
        self.scopes.push(HashMap::new());
        for (arg, param) in args.iter().zip(params.iter()) {
            self.bind_pattern(&arg.pattern, param);
        }
        let saved_return = self.return_type.replace((*ret).clone());
        // §3.5.1: the body may name the function's `<...>` binders in an
        // annotation (`fn f<n>(...) = zeros_of<[n]>()`), so install them while
        // the body is checked. Pass B restored the outer environment.
        let saved_types = std::mem::take(&mut self.type_params);
        let saved_dims = std::mem::take(&mut self.dim_params);
        let saved_nats = std::mem::take(&mut self.nat_params);
        let saved_rows = std::mem::take(&mut self.row_params);
        if let Some(generics) = self.fn_generics.get(name).cloned() {
            self.type_params = generics.types;
            self.dim_params = generics.dims;
            self.nat_params = generics.nats;
            self.row_params = generics.rows;
        }
        self.function_inputs.insert(site, params.clone());
        self.function_stack.push(site);
        self.check(body, &ret);
        self.commit_function_literals(site);
        self.function_stack.pop();
        if let Some(generics) = self.fn_generics.get(name).cloned() {
            self.check_rigid_binders(name, &generics, site);
        }
        self.type_params = saved_types;
        self.dim_params = saved_dims;
        self.nat_params = saved_nats;
        self.row_params = saved_rows;
        self.return_type = saved_return;
        self.scopes.pop();
        // Generalise as soon as the body is checked, for the same reason the
        // `let` path does: a later declaration must see the polymorphic type.
        // Waiting for the end of the module is too late — a call site in
        // between has already unified the shared variable, so `fn ident(x) = x`
        // came out monomorphic and `ident("s")` after `ident(1)` was an error.
        let mut generalised = self.infer.generalize_at(
            self.infer.current_level().saturating_sub(1),
            &Type::func(params.clone(), (*ret).clone()),
            Vec::new(),
        );
        generalised.materializations = self.function_materializations(site);
        self.infer.attach_numeric_scheme(&mut generalised);
        self.infer.quantify_numeric_inputs(
            &mut generalised,
            &params,
            self.infer.current_level().saturating_sub(1),
        );
        self.infer.quantify_numeric_existentials(
            &mut generalised,
            self.infer.current_level().saturating_sub(1),
        );
        let mut binders = self
            .fn_generics
            .get(name)
            .map_or_else(Vec::new, |generics| {
                generics
                    .binders
                    .iter()
                    .map(|binder| binder.resolved(&self.infer))
                    .collect()
            });
        if binders.is_empty() {
            binders = inferred_binders(&generalised);
        }
        quantify_binders(&mut generalised, &binders, &self.infer);
        let forwarded = self
            .global_callables
            .get(name)
            .and_then(|value| value.forwarded_parameter);
        self.global_callables.insert(
            name.to_string(),
            CallableProjection {
                origin: self
                    .callable_ids
                    .get(&site)
                    .expect("seeded callable identity")
                    .clone(),
                arguments: binders.clone(),
                selected: false,
                forwarded_parameter: forwarded,
            },
        );
        self.generic_bindings
            .insert(name.to_string(), binders.clone());
        self.globals.insert(name.to_string(), generalised.clone());
        self.checked.functions.insert(
            site,
            FunctionEvidence {
                site,
                identity: self
                    .callable_ids
                    .get(&site)
                    .expect("seeded callable identity")
                    .clone(),
                numeric_literals: Vec::new(),
                scheme: generalised.clone(),
                binders,
                reifications: self.function_reifications.remove(&site).unwrap_or_default(),
            },
        );
        if let Some(Export::Value { scheme, .. }) = self.checked.exports.get_mut(name) {
            *scheme = generalised;
        }
    }

    /// A declared binder is universally quantified (§3.5, §4.4): the body
    /// must work for every choice of it. Inference may not solve it.
    ///
    /// The checker has no rigid variables, so the property is verified after
    /// the body. A declared dimension binder is rigid: it must still be a bare
    /// variable, distinct from the other declared dimension binders, because
    /// dimension polymorphism is the guarantee the annotation states (§4.4).
    /// A declared type binder may be refined (representation polymorphism)
    /// but must not become one fixed type. Extent binders are solvable by
    /// design (§3.5.1). Before
    /// this check `fn h<d>(x: Real[d]): Real[d] = x + 1 m` was accepted as
    /// `Real[L] -> Real[L]` — the annotation said generic, the program was not.
    fn check_rigid_binders(
        &mut self,
        name: &str,
        generics: &FnGenerics,
        site: Span,
    ) {
        let mut problems: Vec<String> = Vec::new();
        // Abelian unification solves a variable as a product of others
        // (`d = d26*d28/d25`), so a generic binder need not stay a bare
        // variable. It is generic when it is a product of free variables only
        // and the declared binders' exponent vectors are linearly
        // independent: each can still be chosen freely.
        let mut basis: Vec<(Vec<(u32, BigRational)>, String)> = Vec::new();
        for (binder, dim) in &generics.dims {
            let resolved = self.infer.resolve_dim(dim);
            let fixed = resolved
                .factors()
                .any(|(atom, _)| !matches!(atom, aevia_dim::Atom::Var(_)));
            if fixed || resolved.is_one() {
                problems.push(format!(
                    "the body requires `{binder} = {}`",
                    self.dims.format(&resolved)
                ));
                continue;
            }
            let vector: Vec<(u32, BigRational)> = resolved
                .factors()
                .filter_map(|(atom, power)| {
                    match atom {
                        | aevia_dim::Atom::Var(id) => Some((*id, power.clone())),
                        | _ => None,
                    }
                })
                .collect();
            if let Some(reduced) = reduce_against(&basis, vector) {
                basis.push((reduced, binder.clone()));
            } else {
                let others: Vec<&str> = basis.iter().map(|(_, name)| name.as_str()).collect();
                problems.push(format!(
                    "the body determines `{binder}` from `{}`",
                    others.join("`, `")
                ));
            }
        }
        // Extent binders are *solved* by linear equations by design
        // (§3.5.1: `fn f<n>(a: Tensor<[n + 1], τ>): Tensor<[2], τ>` has
        // `n = 1`), so they are not checked here.
        let mut types_seen: HashMap<u32, String> = HashMap::new();
        for (binder, ty) in &generics.types {
            match self.infer.resolve(ty) {
                | Type::Var(id) => {
                    if let Some(other) = types_seen.insert(id, binder.clone()) {
                        problems.push(format!("the body requires `{other} = {binder}`"));
                    }
                },
                | Type::Error => {},
                // A type binder may be refined to a representation-polymorphic
                // numeric type (`fn number<a>(): a = 1` is generic over `Int`,
                // `Real`, `Float`, `Complex`); only a body that makes it one
                // fixed type is less generic than declared.
                | resolved if !crate::class::has_variables(&resolved) => {
                    problems.push(format!("the body requires `{binder} = {resolved}`"));
                },
                | _ => {},
            }
        }
        if problems.is_empty() {
            return;
        }
        let mut diagnostic = Diagnostic::error(
            "E0318",
            format!("`{name}` is less generic than its declared type parameters"),
            site,
        )
        .with_primary_label("declared generic here");
        for problem in problems {
            diagnostic = diagnostic.with_note(problem);
        }
        diagnostic = diagnostic.with_help(
            "make the body work for every choice of the parameter, or replace the parameter with the type the body needs",
        );
        self.diagnostics.push(diagnostic);
    }

    /// Checks the body of a top-level `let`.
    ///
    /// The type is the one pass B published, so a forward reference from
    /// another declaration constrains the *same* variable (§7.1.2). A
    /// non-`rec` binding is hidden while its own value is checked: a top-level
    /// `let` may refer to its neighbours, not to itself (§3.3.2 rule 5).
    fn check_top_let_body(
        &mut self,
        pattern: &Pat,
        annotation: Option<&Ty>,
        value: &Expr,
        rec: bool,
        span: Span,
    ) {
        let PatKind::Var(name) = &pattern.kind else {
            self.check_let_body(pattern, annotation, value, rec, span);
            return;
        };
        let target = match self.top_lets.get(name).cloned() {
            | Some(target) => target,
            | None => self.infer.fresh_type(),
        };
        let target = match annotation {
            | Some(ty) => {
                let annotated = self.convert_type(ty);
                if let Err(error) = self.infer.unify(&target, &annotated) {
                    self.report_unify(error, ty.span);
                }
                annotated
            },
            | None => target,
        };
        self.note_type(pattern.span, &target);
        if rec {
            self.check(value, &target);
        } else {
            let saved = self.globals.remove(name);
            self.check(value, &target);
            if let Some(scheme) = saved {
                self.globals.insert(name.clone(), scheme);
            }
        }
        // Generalise as soon as the body is checked, so a *later* declaration
        // that uses this one sees the polymorphic type. Defaulting still waits
        // for the end of the module.
        let mut scheme = self.infer.generalize_at(0, &target, Vec::new());
        scheme.materializations = self.value_materializations(value.span);
        self.infer.attach_numeric_scheme(&mut scheme);
        if let Some(binders) = self.checked.generic_refs.get(&value.span).cloned() {
            quantify_binders(&mut scheme, &binders, &self.infer);
            self.generic_bindings.insert(name.clone(), binders);
            if let Some(projection) = self.checked.callable_refs.get(&value.span).cloned() {
                self.global_callables.insert(name.clone(), projection);
            } else {
                self.global_callables.remove(name);
            }
        } else {
            self.generic_bindings.remove(name);
            self.global_callables.remove(name);
        }
        self.globals.insert(name.clone(), scheme.clone());
        if let Some(Export::Value {
            scheme: export_scheme,
            ..
        }) = self.checked.exports.get_mut(name)
        {
            *export_scheme = scheme;
        }
    }

    /// Checks a `let` whose name is not published by pass B.
    fn check_let_body(
        &mut self,
        pattern: &Pat,
        annotation: Option<&Ty>,
        value: &Expr,
        rec: bool,
        span: Span,
    ) {
        self.infer.enter_level();
        let target = self.infer.fresh_type();
        if let (PatKind::Var(name), true) = (&pattern.kind, rec) {
            self.globals
                .insert(name.clone(), Scheme::mono(target.clone()));
        }
        let target = match annotation {
            | Some(ty) => {
                let annotated = self.convert_type(ty);
                if let Err(error) = self.infer.unify(&target, &annotated) {
                    self.report_unify(error, ty.span);
                }
                annotated
            },
            | None => target,
        };
        self.check(value, &target);
        self.infer.leave_level();
        self.note_type(pattern.span, &target);
        let mut scheme = self.infer.generalize(&target);
        scheme.materializations = self.value_materializations(value.span);
        self.infer.attach_numeric_scheme(&mut scheme);
        let lexical = self.lexical_numeric_variables();
        let (scheme, warnings) = self.infer.default_scheme_preserving(&scheme, &lexical);
        for _ in warnings {
            self.warn("W0301", "unresolved dimension defaulted to 1", span);
        }
        if let PatKind::Var(name) = &pattern.kind {
            self.globals.insert(name.clone(), scheme.clone());
            if let Some(Export::Value {
                scheme: export_scheme,
                ..
            }) = self.checked.exports.get_mut(name)
            {
                *export_scheme = scheme;
            }
        } else {
            self.bind_pattern(pattern, &scheme.ty);
        }
    }

    /// Generalises and defaults a local `let` and binds its pattern.
    fn bind_local(
        &mut self,
        pattern: &Pat,
        value_ty: &Type,
        span: Span,
        value_span: Span,
    ) {
        self.note_type(pattern.span, value_ty);
        let mut scheme = self.infer.generalize(value_ty);
        scheme.materializations = self.value_materializations(value_span);
        if let Some(headers) = self.checked.generic_refs.get(&value_span) {
            quantify_binders(&mut scheme, headers, &self.infer);
        }
        self.infer.attach_numeric_scheme(&mut scheme);
        let lexical = self.lexical_numeric_variables();
        let (mut scheme, warnings) = self.infer.default_scheme_preserving(&scheme, &lexical);
        for _ in warnings {
            self.warn("W0301", "unresolved dimension defaulted to 1", span);
        }
        if let PatKind::Var(name) = &pattern.kind {
            if let Some(headers) = self.checked.generic_refs.get(&value_span).cloned() {
                quantify_binders(&mut scheme, &headers, &self.infer);
                self.local_headers
                    .insert((self.scopes.len(), name.clone()), headers);
                if let Some(projection) = self.checked.callable_refs.get(&value_span).cloned() {
                    self.local_callables
                        .insert((self.scopes.len(), name.clone()), projection);
                } else {
                    self.local_callables
                        .remove(&(self.scopes.len(), name.clone()));
                }
            } else {
                self.local_headers
                    .remove(&(self.scopes.len(), name.clone()));
                self.local_callables
                    .remove(&(self.scopes.len(), name.clone()));
            }
            self.scopes
                .last_mut()
                .expect("a local binding has a scope")
                .insert(name.clone(), scheme);
        } else {
            self.bind_pattern(pattern, &scheme.ty);
        }
    }

    // ----- expressions ---------------------------------------------------

    fn synth(
        &mut self,
        expr: &Expr,
    ) -> Type {
        if self.literal_region.is_none()
            && matches!(expr.kind, ExprKind::Unary { .. } | ExprKind::Binary { .. })
        {
            let class = self.literal_class(expr);
            if class.tree {
                if !class.atomic {
                    self.constant_region_roots.push(expr.span);
                }
                return self.synth_literal_region(expr, class.homogeneous);
            }
        }
        self.seed_expression_callable(expr);
        let ty = self.synth_inner(expr);
        self.note_type(expr.span, &ty);
        self.carry_callable(expr);
        ty
    }

    fn seed_expression_callable(
        &mut self,
        expr: &Expr,
    ) {
        if matches!(expr.kind, ExprKind::Lambda { .. })
            || matches!(&expr.kind, ExprKind::Call { args, .. } if args.iter().any(|arg| matches!(arg.kind, ExprKind::Placeholder)))
        {
            self.callable_ids.entry(expr.span).or_insert_with(|| {
                CallableId::new(
                    &self.module_path,
                    expr.span,
                    &aevia_syntax::fingerprint::expression(expr),
                )
            });
        }
    }

    fn carry_callable(
        &mut self,
        expr: &Expr,
    ) {
        let source = match &expr.kind {
            | ExprKind::Annot { expr, .. } => Some(expr.span),
            | ExprKind::TypeApp { .. } => return,
            | ExprKind::Block { tail: Some(tail), .. } => Some(tail.span),
            | ExprKind::LetIn { body, .. } => Some(body.span),
            | ExprKind::Call { callee, args } => {
                self.checked
                    .callable_refs
                    .get(&callee.span)
                    .and_then(|projection| projection.forwarded_parameter)
                    .and_then(|index| args.get(index))
                    .map(|arg| arg.span)
            },
            | ExprKind::If {
                then_branch,
                else_branch,
                ..
            } => {
                let first = self
                    .checked
                    .callable_refs
                    .get(&then_branch.span)
                    .map(|value| value.resolved(&self.infer));
                let second = self
                    .checked
                    .callable_refs
                    .get(&else_branch.span)
                    .map(|value| value.resolved(&self.infer));
                if first.is_some() && first == second {
                    Some(then_branch.span)
                } else {
                    None
                }
            },
            | ExprKind::Match { arms, .. } => {
                let first = arms
                    .first()
                    .and_then(|arm| self.checked.callable_refs.get(&arm.body.span))
                    .map(|value| value.resolved(&self.infer));
                if first.is_some()
                    && arms.iter().all(|arm| {
                        self.checked
                            .callable_refs
                            .get(&arm.body.span)
                            .map(|value| value.resolved(&self.infer))
                            == first
                    })
                {
                    arms.first().map(|arm| arm.body.span)
                } else {
                    None
                }
            },
            | _ => None,
        };
        if let Some(source) = source {
            if let Some(projection) = self.checked.callable_refs.get(&source).cloned() {
                self.checked.callable_refs.insert(expr.span, projection);
                if let Some(headers) = self.checked.generic_refs.get(&source).cloned() {
                    self.checked.generic_refs.insert(expr.span, headers);
                }
            }
        } else if let Some(function) = self.checked.functions.get(&expr.span) {
            let arguments = if function.binders.is_empty() {
                inferred_binders(&function.scheme)
            } else {
                function.binders.clone()
            };
            self.checked
                .callable_refs
                .entry(expr.span)
                .or_insert(CallableProjection {
                    origin: function.identity.clone(),
                    arguments,
                    selected: false,
                    forwarded_parameter: None,
                });
        }
    }

    fn literal_class(
        &mut self,
        root: &Expr,
    ) -> LiteralClass {
        let mut pending = vec![(root, false)];
        while let Some((expr, ready)) = pending.pop() {
            if self.literal_classes.contains_key(&expr.span) {
                continue;
            }
            let kind = match &expr.kind {
                | ExprKind::Int(_) => Some(LiteralKind::Int),
                | ExprKind::Float(_) => Some(LiteralKind::Decimal),
                | ExprKind::Quantity { value, .. } => {
                    Some(if value.is_decimal() {
                        LiteralKind::QuantityDecimal
                    } else {
                        LiteralKind::QuantityInt
                    })
                },
                | _ => None,
            };
            if let Some(kind) = kind {
                self.literal_classes.insert(
                    expr.span,
                    LiteralClass {
                        tree: true,
                        homogeneous: Some(kind),
                        atomic: true,
                    },
                );
                continue;
            }
            match &expr.kind {
                | ExprKind::Unary {
                    op: UnOp::Pos | UnOp::Neg,
                    operand,
                } => {
                    if ready {
                        let class = self.literal_classes[&operand.span];
                        self.literal_classes.insert(expr.span, class);
                    } else {
                        pending.push((expr, true));
                        pending.push((operand, false));
                    }
                },
                | ExprKind::Binary {
                    op: BinOp::Add | BinOp::Sub | BinOp::Mul | BinOp::Div,
                    left,
                    right,
                } => {
                    if ready {
                        let left = self.literal_classes[&left.span];
                        let right = self.literal_classes[&right.span];
                        self.literal_classes.insert(
                            expr.span,
                            LiteralClass {
                                tree: left.tree && right.tree,
                                atomic: false,
                                homogeneous: if left.tree
                                    && right.tree
                                    && left.homogeneous == right.homogeneous
                                {
                                    left.homogeneous
                                } else {
                                    None
                                },
                            },
                        );
                    } else {
                        pending.push((expr, true));
                        pending.push((right, false));
                        pending.push((left, false));
                    }
                },
                | _ => {
                    self.literal_classes
                        .insert(expr.span, LiteralClass::default());
                },
            }
        }
        self.literal_classes[&root.span]
    }

    /// A closed homogeneous literal component may use one reading variable:
    /// all leaves have the same domain and no independently supplied value can
    /// distinguish their choices. Keep individual dimensions and source spans.
    fn synth_literal_region(
        &mut self,
        root: &Expr,
        kind: Option<LiteralKind>,
    ) -> Type {
        let reading = kind.map(|kind| (kind, self.infer.fresh_rep()));
        let previous = std::mem::replace(&mut self.literal_region, reading);
        // A heterogeneous tree keeps separate domains. Repeated atomic
        // operands on one evaluation spine may share their kind's reading;
        // a right-hand arithmetic subtree starts an independent context.
        let mut contexts: Vec<Vec<(LiteralKind, Rep)>> = vec![Vec::new()];
        let mut pending = vec![(root, false, 0usize)];
        let mut types = HashMap::<Span, Type>::new();
        while let Some((expr, ready, context)) = pending.pop() {
            if !ready {
                let class = self.literal_class(expr);
                if kind.is_none()
                    && expr.span != root.span
                    && !class.atomic
                    && let Some(kind) = class.homogeneous
                {
                    let ty = self.synth_literal_region(expr, Some(kind));
                    types.insert(expr.span, ty);
                    continue;
                }
                match &expr.kind {
                    | ExprKind::Unary { operand, .. } => {
                        pending.push((expr, true, context));
                        pending.push((operand, false, context));
                        continue;
                    },
                    | ExprKind::Binary { left, right, .. } => {
                        let right_context = if kind.is_none() && !self.literal_class(right).atomic {
                            contexts.push(Vec::new());
                            contexts.len() - 1
                        } else {
                            context
                        };
                        pending.push((expr, true, context));
                        pending.push((right, false, right_context));
                        pending.push((left, false, context));
                        continue;
                    },
                    | _ => {},
                }
            }
            let ty = match &expr.kind {
                | ExprKind::Unary { op, operand } => {
                    self.unary_type(*op, &types[&operand.span], expr.span)
                },
                | ExprKind::Binary { op, left, right } => {
                    self.binary_type(
                        *op,
                        &types[&left.span],
                        &types[&right.span],
                        left,
                        right,
                        expr.span,
                    )
                },
                | _ => {
                    if kind.is_none()
                        && let Some(leaf_kind) = self.literal_class(expr).homogeneous
                    {
                        let rep = if let Some((_, rep)) = contexts[context]
                            .iter()
                            .find(|(kind, _)| *kind == leaf_kind)
                        {
                            *rep
                        } else {
                            let rep = self.infer.fresh_rep();
                            contexts[context].push((leaf_kind, rep));
                            rep
                        };
                        self.literal_region = Some((leaf_kind, rep));
                    }
                    let ty = self.synth_inner(expr);
                    if kind.is_none() {
                        self.literal_region = previous;
                    }
                    ty
                },
            };
            self.note_type(expr.span, &ty);
            types.insert(expr.span, ty);
        }
        self.literal_region = previous;
        if !self.literal_class(root).atomic
            && let Some((_, rep)) = reading
        {
            self.infer.finish_literal_expression(rep);
        }
        types.remove(&root.span).unwrap_or(Type::Error)
    }

    fn synth_inner(
        &mut self,
        expr: &Expr,
    ) -> Type {
        match &expr.kind {
            // A parser recovery node: the syntax error was already reported, so
            // synthesise an unconstrained type instead of a second diagnostic.
            | ExprKind::Error => self.infer.fresh_type(),
            | ExprKind::UnitLit => Type::Unit,
            | ExprKind::Bool(_) => Type::Bool,
            | ExprKind::Int(_) => {
                self.record_literal(expr.span, LiteralKind::Int);
                let reading = self
                    .literal_context
                    .filter(|rep| LiteralKind::Int.accepts_rep(*rep))
                    .or_else(|| {
                        self.literal_region
                            .filter(|(kind, _)| *kind == LiteralKind::Int)
                            .map(|(_, rep)| rep)
                    });
                let ty = reading.map_or_else(
                    || self.infer.fresh_type(),
                    |rep| Type::Scalar(rep, Dim::one()),
                );
                self.infer.mark_literal(&ty, LiteralKind::Int);
                ty
            },
            | ExprKind::Float(_) => {
                self.record_literal(expr.span, LiteralKind::Decimal);
                let reading = self
                    .literal_context
                    .filter(|rep| LiteralKind::Decimal.accepts_rep(*rep))
                    .or_else(|| {
                        self.literal_region
                            .filter(|(kind, _)| *kind == LiteralKind::Decimal)
                            .map(|(_, rep)| rep)
                    });
                let ty = reading.map_or_else(
                    || self.infer.fresh_type(),
                    |rep| Type::Scalar(rep, Dim::one()),
                );
                self.infer.mark_literal(&ty, LiteralKind::Decimal);
                ty
            },
            | ExprKind::Str(pieces) => {
                for piece in pieces {
                    if let StrPiece::Expr(inner) = piece {
                        self.synth(inner);
                    }
                }
                Type::string()
            },
            | ExprKind::Quantity { value, unit } => {
                self.record_literal(
                    expr.span,
                    if value.is_decimal() {
                        LiteralKind::QuantityDecimal
                    } else {
                        LiteralKind::QuantityInt
                    },
                );
                self.quantity_type(value, unit, expr.span)
            },
            | ExprKind::Var(name) => self.lookup_value(name, expr.span),
            | ExprKind::QName(path) => {
                // `Rssn.Version` and friends are kernel members, not names in
                // scope; the manifest decides what exists.
                if path.len() == 2 && path[0] == "Rssn" {
                    return self.rssn_member(&path[1], expr.span);
                }
                self.lookup_value(&path.join("."), expr.span)
            },
            | ExprKind::Unary { op, operand } => self.synth_unary(*op, operand, expr.span),
            | ExprKind::Binary { op, left, right } => {
                self.synth_binary(*op, left, right, expr.span)
            },
            | ExprKind::Call { callee, args } => self.synth_call(callee, args, expr.span),
            | ExprKind::TypeApp { callee, args } => self.synth_type_app(callee, args, expr.span),
            | ExprKind::Lambda { params, ret, body } => {
                self.infer.enter_level();
                self.scopes.push(HashMap::new());
                let mut arg_types = Vec::new();
                for param in params {
                    let ty = match &param.ty {
                        | Some(ty) => self.convert_type(ty),
                        | None => self.infer.fresh_type(),
                    };
                    self.bind_pattern(&param.pattern, &ty);
                    arg_types.push(ty);
                }
                let ret_ty = match ret {
                    | Some(ty) => self.convert_type(ty),
                    | None => self.infer.fresh_type(),
                };
                let saved_return = self.return_type.replace(ret_ty.clone());
                self.function_inputs.insert(expr.span, arg_types.clone());
                self.function_stack.push(expr.span);
                self.check(body, &ret_ty);
                self.commit_function_literals(expr.span);
                self.function_stack.pop();
                self.return_type = saved_return;
                self.scopes.pop();
                self.infer.leave_level();
                let ty = Type::func(arg_types.clone(), ret_ty);
                let mut scheme = self.infer.generalize(&ty);
                scheme.materializations = self.function_materializations(expr.span);
                self.infer.attach_numeric_scheme(&mut scheme);
                self.infer.quantify_numeric_inputs(
                    &mut scheme,
                    &arg_types,
                    self.infer.current_level(),
                );
                self.infer
                    .quantify_numeric_existentials(&mut scheme, self.infer.current_level());
                self.checked.functions.insert(
                    expr.span,
                    FunctionEvidence {
                        site: expr.span,
                        identity: self
                            .callable_ids
                            .get(&expr.span)
                            .expect("seeded callable identity")
                            .clone(),
                        numeric_literals: Vec::new(),
                        scheme,
                        binders: Vec::new(),
                        reifications: self
                            .function_reifications
                            .remove(&expr.span)
                            .unwrap_or_default(),
                    },
                );
                ty
            },
            | ExprKind::If {
                cond,
                then_branch,
                else_branch,
            } => {
                let cond_ty = self.synth(cond);
                self.expect_bool(&cond_ty, cond.span);
                let branch = self.infer.fresh_type();
                self.check(then_branch, &branch);
                self.check(else_branch, &branch);
                branch
            },
            | ExprKind::Match { scrutinee, arms } => {
                let subject = self.synth(scrutinee);
                let result = self.infer.fresh_type();
                // §6.6 usefulness: a *guarded* arm never contributes coverage,
                // so only unguarded arms enter the exhaustiveness matrix.
                let mut unguarded: Vec<Pat> = Vec::new();
                for arm in arms {
                    self.scopes.push(HashMap::new());
                    let outer = std::mem::replace(&mut self.refutable_pattern, true);
                    self.bind_pattern(&arm.pattern, &subject);
                    self.refutable_pattern = outer;
                    // An arm is unreachable when the unguarded arms before it
                    // already cover every value it could match.
                    let subject_resolved = self.infer.resolve(&subject);
                    if self.pattern_is_unreachable(&arm.pattern, &unguarded, &subject_resolved) {
                        self.warn(
                            "W0201",
                            "this match arm is redundant: an earlier arm covers it",
                            arm.span,
                        );
                    }
                    if arm.guard.is_none() {
                        unguarded.push(arm.pattern.clone());
                    }
                    if let Some(guard) = &arm.guard {
                        let guard_ty = self.synth(guard);
                        self.expect_bool(&guard_ty, guard.span);
                    }
                    self.check(&arm.body, &result);
                    self.scopes.pop();
                }
                let subject_resolved = self.infer.resolve(&subject);
                self.check_exhaustive(&subject_resolved, &unguarded, expr.span);
                result
            },
            | ExprKind::LetIn {
                pattern,
                ty,
                value,
                body,
            } => {
                self.infer.enter_level();
                let value_ty = match ty {
                    | Some(ty) => {
                        let ty = self.convert_type(ty);
                        self.check(value, &ty);
                        ty
                    },
                    | None => self.synth(value),
                };
                self.infer.leave_level();
                if let PatKind::Var(name) = &pattern.kind {
                    self.warn_if_shadowing(name, pattern.span);
                }
                self.scopes.push(HashMap::new());
                self.bind_local(pattern, &value_ty, expr.span, value.span);
                let body_ty = self.synth(body);
                self.scopes.pop();
                body_ty
            },
            | ExprKind::Block { stmts, tail } => {
                self.scopes.push(HashMap::new());
                self.check_block_statements(stmts);
                let tail_ty = match tail {
                    | Some(tail) => self.synth(tail),
                    | None => {
                        self.error_help(
                            "E0201",
                            "block has no final expression",
                            expr.span,
                            "add a value after the `let` statements",
                        );
                        Type::Unit
                    },
                };
                self.scopes.pop();
                tail_ty
            },
            | ExprKind::Do { stmts } => self.synth_do(stmts, expr.span),
            | ExprKind::Record { base, fields } => self.synth_record(base.as_deref(), fields),
            | ExprKind::Field { base, name } => {
                if is_rssn_root(base) {
                    return self.rssn_member(name, expr.span);
                }
                // `A.B.f` parses as a field access on a qualified name; the
                // imported module puts `A.B.f` in scope as one key.
                if let Some(qualified) = qualified_name(base, name) {
                    if self.globals.contains_key(&qualified) {
                        return self.lookup_value(&qualified, expr.span);
                    }
                    // The module exists but does not export that name: say so
                    // rather than calling the whole path unbound.
                    if let Some(prefix) = qualified.rsplit_once('.').map(|(prefix, _)| prefix) {
                        let marker = format!("{prefix}.");
                        if self.globals.keys().any(|key| key.starts_with(&marker)) {
                            self.error_help(
                                "E0301",
                                format!("module `{prefix}` has no exported name `{name}`"),
                                expr.span,
                                "a `private` declaration is not part of the interface (§7.1.3)",
                            );
                            return Type::Error;
                        }
                    }
                }
                let base_ty = self.synth(base);
                self.field_type(&base_ty, name, expr.span)
            },
            | ExprKind::Tuple(items) => {
                let types: Vec<Type> = items.iter().map(|item| self.synth(item)).collect();
                Type::record(
                    types
                        .into_iter()
                        .enumerate()
                        .map(|(index, ty)| (index.to_string(), ty)),
                )
            },
            | ExprKind::Tensor(items) => self.synth_tensor(items, expr.span),
            | ExprKind::Index { base, indices } => self.synth_index(base, indices, expr.span),
            | ExprKind::Annot { expr: inner, ty } => {
                let ty = self.convert_type(ty);
                self.check(inner, &ty);
                ty
            },
            | ExprKind::Placeholder => {
                self.error("E0202", "`_` is only allowed as a call argument", expr.span);
                Type::Error
            },
            | ExprKind::Try(inner) => self.synth_try(inner, expr.span),
            | ExprKind::Factorial { expr: inner, .. } => {
                let ty = self.synth(inner);
                let _ = self.infer.unify(&ty, &Type::int());
                Type::int()
            },
        }
    }

    fn check(
        &mut self,
        expr: &Expr,
        expected: &Type,
    ) {
        self.seed_expression_callable(expr);
        self.check_inner(expr, expected);
        // Whatever path `check_inner` took — a direct comparison, or a
        // bidirectional rule that never synthesised the node — the type of the
        // expression is the type it was checked against. Recorded raw and
        // resolved at the end, like every other entry.
        self.note_type(expr.span, expected);
        self.carry_callable(expr);
    }

    fn check_block_statements(
        &mut self,
        stmts: &[BlockStmt],
    ) {
        for stmt in stmts {
            match stmt {
                | BlockStmt::Let {
                    pattern,
                    ty,
                    value,
                    rec,
                    span,
                } => {
                    if let PatKind::Var(name) = &pattern.kind {
                        self.warn_if_shadowing(name, pattern.span);
                    }
                    self.infer.enter_level();
                    let target = self.infer.fresh_type();
                    if let (PatKind::Var(name), true) = (&pattern.kind, *rec) {
                        self.scopes
                            .last_mut()
                            .expect("scope")
                            .insert(name.clone(), Scheme::mono(target.clone()));
                    }
                    let value_ty = match ty {
                        | Some(annotation) => {
                            let annotated = self.convert_type(annotation);
                            if let Err(error) = self.infer.unify(&target, &annotated) {
                                self.report_unify(error, annotation.span);
                            }
                            self.check(value, &annotated);
                            annotated
                        },
                        | None => {
                            self.check(value, &target);
                            target
                        },
                    };
                    self.infer.leave_level();
                    self.bind_local(pattern, &value_ty, *span, value.span);
                },
                | BlockStmt::Fn(decl) => {
                    if let DeclKind::Fn {
                        name,
                        params,
                        args,
                        ret,
                        body,
                        ..
                    } = &decl.kind
                    {
                        self.infer.enter_level();
                        self.callable_ids.insert(
                            decl.span,
                            CallableId::new(
                                &self.module_path,
                                decl.span,
                                &aevia_syntax::fingerprint::declaration(decl),
                            ),
                        );
                        let (ty, generics) = self.fn_signature(params, args, ret.as_ref());
                        self.local_headers
                            .insert((self.scopes.len(), name.clone()), generics.binders.clone());
                        let projection = CallableProjection {
                            origin: self
                                .callable_ids
                                .get(&decl.span)
                                .expect("seeded callable identity")
                                .clone(),
                            arguments: generics.binders.clone(),
                            selected: false,
                            forwarded_parameter: forwarded_parameter(args, body),
                        };
                        self.local_callables
                            .insert((self.scopes.len(), name.clone()), projection.clone());
                        let saved_callable = self.global_callables.insert(name.clone(), projection);
                        let saved_generics = self.fn_generics.insert(name.clone(), generics);
                        let saved_signature = self.fn_signatures.insert(name.clone(), ty.clone());
                        let saved_global = self.globals.get(name).cloned();
                        let saved_header = self.generic_bindings.get(name).cloned();
                        self.scopes
                            .last_mut()
                            .expect("scope")
                            .insert(name.clone(), Scheme::mono(ty));
                        self.check_fn_body(name, args, body, decl.span);
                        let scheme = self
                            .globals
                            .get(name)
                            .cloned()
                            .unwrap_or_else(|| Scheme::mono(Type::Error));
                        let headers = self.generic_bindings.get(name).cloned().unwrap_or_default();
                        if let Some(projection) = self.global_callables.get(name).cloned() {
                            self.local_callables
                                .insert((self.scopes.len(), name.clone()), projection);
                        }
                        match saved_callable {
                            | Some(value) => {
                                self.global_callables.insert(name.clone(), value);
                            },
                            | None => {
                                self.global_callables.remove(name);
                            },
                        }
                        self.infer.leave_level();
                        self.scopes
                            .last_mut()
                            .expect("scope")
                            .insert(name.clone(), scheme);
                        self.local_headers
                            .insert((self.scopes.len(), name.clone()), headers);
                        match saved_generics {
                            | Some(value) => {
                                self.fn_generics.insert(name.clone(), value);
                            },
                            | None => {
                                self.fn_generics.remove(name);
                            },
                        }
                        match saved_signature {
                            | Some(value) => {
                                self.fn_signatures.insert(name.clone(), value);
                            },
                            | None => {
                                self.fn_signatures.remove(name);
                            },
                        }
                        match saved_global {
                            | Some(value) => {
                                self.globals.insert(name.clone(), value);
                            },
                            | None => {
                                self.globals.remove(name);
                            },
                        }
                        match saved_header {
                            | Some(value) => {
                                self.generic_bindings.insert(name.clone(), value);
                            },
                            | None => {
                                self.generic_bindings.remove(name);
                            },
                        }
                    }
                },
            }
        }
    }

    fn check_inner(
        &mut self,
        expr: &Expr,
        expected: &Type,
    ) {
        let resolved_expected = self.infer.resolve(expected);
        // A literal expression may be read directly in its concrete source
        // context, including exact decimals inside arithmetic. Published
        // callable signatures and expressions containing variables/calls never
        // enter this branch: their numeric transfers remain authoritative.
        if self.literal_class(expr).tree
            && let Type::Scalar(rep, _) = &resolved_expected
            && !matches!(rep, Rep::Var(_))
        {
            let previous = self.literal_context.replace(*rep);
            let actual = self.synth(expr);
            self.literal_context = previous;
            if let Err(error) = self.infer.unify(&actual, expected) {
                self.report_unify(error, expr.span);
            }
            return;
        }
        // An empty list has no element to learn its type from; the context
        // supplies it, which is what `let e: Tensor<1, Float> = []` means.
        if matches!(&expr.kind, ExprKind::Tensor(items) if items.is_empty())
            && matches!(resolved_expected, Type::Tensor { .. })
        {
            return;
        }
        match (&expr.kind, &resolved_expected) {
            | (ExprKind::Lambda { params, ret, body }, Type::Fn(expected_params, expected_ret))
                if params.len() == expected_params.len() =>
            {
                self.infer.enter_level();
                self.scopes.push(HashMap::new());
                for (param, expected_ty) in params.iter().zip(expected_params.iter()) {
                    if let Some(annotation) = &param.ty {
                        let annotated = self.convert_type(annotation);
                        if let Err(error) = self.infer.unify(&annotated, expected_ty) {
                            self.report_unify(error, param.span);
                        }
                    }
                    self.bind_pattern(&param.pattern, expected_ty);
                }
                if let Some(annotation) = ret {
                    let annotated = self.convert_type(annotation);
                    if let Err(error) = self.infer.unify(&annotated, expected_ret) {
                        self.report_unify(error, annotation_span(annotation));
                    }
                }
                let saved_return = self.return_type.replace((**expected_ret).clone());
                self.function_inputs
                    .insert(expr.span, expected_params.clone());
                self.function_stack.push(expr.span);
                self.check(body, expected_ret);
                self.commit_function_literals(expr.span);
                self.function_stack.pop();
                self.return_type = saved_return;
                self.scopes.pop();
                self.infer.leave_level();
                let mut scheme = self.infer.generalize(&resolved_expected);
                scheme.materializations = self.function_materializations(expr.span);
                self.infer.attach_numeric_scheme(&mut scheme);
                self.infer.quantify_numeric_inputs(
                    &mut scheme,
                    expected_params,
                    self.infer.current_level(),
                );
                self.infer
                    .quantify_numeric_existentials(&mut scheme, self.infer.current_level());
                self.checked.functions.insert(
                    expr.span,
                    FunctionEvidence {
                        site: expr.span,
                        identity: self
                            .callable_ids
                            .get(&expr.span)
                            .expect("seeded callable identity")
                            .clone(),
                        numeric_literals: Vec::new(),
                        scheme,
                        binders: Vec::new(),
                        reifications: self
                            .function_reifications
                            .remove(&expr.span)
                            .unwrap_or_default(),
                    },
                );
                return;
            },
            | (
                ExprKind::If {
                    cond,
                    then_branch,
                    else_branch,
                },
                _,
            ) => {
                let cond_ty = self.synth(cond);
                self.expect_bool(&cond_ty, cond.span);
                self.check(then_branch, expected);
                self.check(else_branch, expected);
                return;
            },
            | (
                ExprKind::Block {
                    stmts,
                    tail: Some(tail),
                },
                _,
            ) => {
                self.scopes.push(HashMap::new());
                self.check_block_statements(stmts);
                self.check(tail, expected);
                self.scopes.pop();
                return;
            },
            | _ => {},
        }
        let actual = self.synth(expr);
        let expected_resolved = self.infer.resolve(expected);
        if matches!(
            self.infer.resolve(&actual),
            Type::ReadNpyIntrinsic(_) | Type::WriteNpyIntrinsic
        ) && matches!(expected_resolved, Type::Fn(..))
        {
            self.error_help(
                "E0302",
                "an NPY intrinsic cannot yet satisfy an explicit function type",
                expr.span,
                "wrap the rank-selected call in a source function or lambda",
            );
            return;
        }
        // §3.9.4: a pure context cannot run an action. Saying `E0313` instead
        // of a plain mismatch is what tells the user to return `IO`.
        if matches!(actual, Type::Io(_))
            && !matches!(expected_resolved, Type::Io(_) | Type::Var(_) | Type::Error)
        {
            self.error_help(
                "E0313",
                "this is an `IO` action in a pure context",
                expr.span,
                "return `IO` from the enclosing function, or `run_io` at the entry point",
            );
            return;
        }
        // §2.10/§3.5.1: a rank-only tensor does not *satisfy* a refined
        // annotation — that needs a runtime check (`refine_shape`). Shape
        // unification itself is symmetric for operands (§3.5.3: `v * i` may
        // mix `Tensor<[2], V>` and `Tensor<1, A>`), but an annotation is a
        // static guarantee, so this direction stays rejected.
        if let Some(message) = self.rank_only_to_refined(&actual, &expected_resolved) {
            self.error_help(
                "E0302",
                message,
                expr.span,
                "use `refine_shape` when the extent is checked at run time, or annotate only the rank",
            );
            return;
        }
        if let Err(error) = self.infer.unify(&actual, expected) {
            self.report_unify(error, expr.span);
        }
    }

    /// The rank-only → refined-annotation message, when it applies.
    ///
    /// `zeros(4) : Tensor<1, Float>` has no extent information, so writing
    /// `let a: Tensor<[3], Float> = zeros(4)` would claim a guarantee the
    /// checker cannot make. The reverse (refined value, rank-only annotation)
    /// is the safe implicit conversion and is allowed.
    fn rank_only_to_refined(
        &self,
        actual: &Type,
        expected: &Type,
    ) -> Option<String> {
        let actual = self.infer.resolve(actual);
        let expected = self.infer.resolve(expected);
        let (
            Type::Tensor {
                shape: Shape::Rank(actual_rank),
                ..
            },
            Type::Tensor {
                shape: Shape::Refined(items),
                ..
            },
        ) = (&actual, &expected)
        else {
            return None;
        };
        let known_extent = items
            .iter()
            .any(|item| item.as_ref().and_then(Nat::as_const).is_some());
        if known_extent && usize::try_from(*actual_rank).unwrap_or(usize::MAX) == items.len() {
            Some("a rank-only tensor cannot satisfy a refined annotation".to_string())
        } else {
            None
        }
    }

    fn synth_unary(
        &mut self,
        op: UnOp,
        operand: &Expr,
        span: Span,
    ) -> Type {
        let ty = self.synth(operand);
        self.unary_type(op, &ty, span)
    }

    fn unary_type(
        &mut self,
        op: UnOp,
        ty: &Type,
        span: Span,
    ) -> Type {
        let resolved = match op {
            | UnOp::Neg | UnOp::Pos => self.force_scalar(ty),
            | _ => self.infer.resolve(ty),
        };
        match op {
            | UnOp::Neg | UnOp::Pos => {
                match resolved {
                    // §4.9.3: unary `-` and `+` preserve `Dyn<ρ>`.
                    | Type::Scalar(..) | Type::Tensor { .. } | Type::Dyn(_) => resolved,
                    | Type::Adt(..) if builtins::sparse_parts(&resolved).is_some() => resolved,
                    | Type::Error => Type::Error,
                    | other => {
                        let symbol = if op == UnOp::Neg { "-" } else { "+" };
                        let message = format!("unary `{symbol}` is not defined for {other}");
                        if matches!(other, Type::Str) {
                            self.error_help(
                                "E0302",
                                message,
                                span,
 "strings are joined with `concat(a, b)` or interpolation `\"{a}{b}\"`; there is no `++` operator",
                            );
                        } else {
                            self.error("E0302", message, span);
                        }
                        Type::Error
                    },
                }
            },
            | UnOp::Not => {
                match resolved {
                    | Type::Bool => Type::Bool,
                    | Type::Prop => Type::Prop,
                    | other => {
                        self.error("E0302", format!("`!` expects Bool, found {other}"), span);
                        Type::Error
                    },
                }
            },
            | UnOp::Sqrt => {
                match resolved {
                    | Type::Scalar(rep, dim) => {
                        // §3.4.2: a root divides, so an integer becomes `Real`.
                        Type::Scalar(
                            self.div_rep(rep),
                            dim.pow(&BigRational::new(1.into(), 2.into())),
                        )
                    },
                    // §4.9.3: `sqrt : Dyn<ρ> -> Dyn<ρ>`, halving the runtime
                    // exponent.
                    | Type::Dyn(rep) => Type::Dyn(self.infer.resolve_rep(rep)),
                    | other => {
                        self.error(
                            "E0302",
                            format!("`√` expects a scalar, found {other}"),
                            span,
                        );
                        Type::Error
                    },
                }
            },
        }
    }

    /// The `(rep, dim)` of a §8.14 measurement record, when `ty` is one:
    /// `{ value: Real[d], uncertainty: Real[d] }`.
    fn measurement_parts(
        &self,
        ty: &Type,
    ) -> Option<(Rep, Dim)> {
        let Type::Record(row) = ty else {
            return None;
        };
        if row.tail_var().is_some() {
            return None;
        }
        let fields = row.fields();
        if fields.len() != 2 {
            return None;
        }
        let value = fields.iter().find(|(name, _)| name == "value")?;
        let uncertainty = fields.iter().find(|(name, _)| name == "uncertainty")?;
        let (Type::Scalar(value_rep, value_dim), Type::Scalar(_uncertainty_rep, uncertainty_dim)) = (
            self.infer.resolve(&value.1),
            self.infer.resolve(&uncertainty.1),
        ) else {
            return None;
        };
        if value_dim != uncertainty_dim {
            return None;
        }
        Some((self.infer.resolve_rep(value_rep), value_dim.clone()))
    }

    /// The `Measurement<d>` record type itself.
    fn measurement_type(
        rep: Rep,
        dim: Dim,
    ) -> Type {
        Type::record(vec![
            ("value".to_string(), Type::Scalar(rep, dim.clone())),
            ("uncertainty".to_string(), Type::Scalar(rep, dim)),
        ])
    }

    #[allow(clippy::too_many_arguments)]
    fn measurement_binary(
        &mut self,
        op: BinOp,
        left: Option<(Rep, Dim)>,
        right: Option<(Rep, Dim)>,
        left_ty: &Type,
        right_ty: &Type,
        span: Span,
    ) -> Type {
        let plain_scalar = |ty: &Type| matches!(ty, Type::Scalar(_, _));
        match (op, left, right) {
            | (BinOp::Add | BinOp::Sub, Some((rep, left_dim)), Some((_, right_dim))) => {
                if left_dim != right_dim {
                    self.error_help(
                        "E0401",
                        format!(
                            "dimension mismatch in measurement {}: {left_dim} and {right_dim}",
                            op.as_str()
                        ),
                        span,
                        "`+`/`-` require measurements of the same dimension",
                    );
                    return Type::Error;
                }
                Self::measurement_type(rep, left_dim)
            },
            | (BinOp::Add | BinOp::Sub, _, _) => {
                self.error_help(
                    "E0302",
                    format!(
                        "`{}` expects two measurements, found {left_ty} and {right_ty}",
                        op.as_str()
                    ),
                    span,
                    "wrap a plain number with `measure(x, u)` to give it an uncertainty",
                );
                Type::Error
            },
            | (BinOp::Mul, Some((rep, left_dim)), Some((_, right_dim))) => {
                Self::measurement_type(rep, left_dim.mul(&right_dim))
            },
            | (BinOp::Mul, Some((rep, left_dim)), None) if plain_scalar(right_ty) => {
                Self::measurement_type(rep, left_dim)
            },
            | (BinOp::Mul, None, Some((rep, right_dim))) if plain_scalar(left_ty) => {
                Self::measurement_type(rep, right_dim)
            },
            | (BinOp::Div, Some((rep, left_dim)), Some((_, right_dim))) => {
                Self::measurement_type(self.div_rep(rep), left_dim.div(&right_dim))
            },
            | (BinOp::Div, Some((rep, left_dim)), None) if plain_scalar(right_ty) => {
                Self::measurement_type(rep, left_dim)
            },
            | (BinOp::Div, None, Some(_)) => {
                self.error_help(
                    "E0302",
                    "dividing a plain number by a measurement is not supported",
                    span,
                    "divide by the measurement's value if that is what you mean",
                );
                Type::Error
            },
            | _ => {
                self.error_help(
                    "E0302",
                    format!(
                        "`{}` is not defined for {} and {}",
                        op.as_str(),
                        left_ty,
                        right_ty
                    ),
                    span,
                    "measurement operators take measurement records or a measurement and a number",
                );
                Type::Error
            },
        }
    }

    /// The name of an operand that is a local value spelled like a unit,
    /// when the other operand is a unit: `m/s` with a `let s = …` in scope.
    fn shadowed_unit_operand(
        &self,
        left: &Type,
        right: &Type,
        left_expr: &Expr,
        right_expr: &Expr,
    ) -> Option<String> {
        let candidate = |ty: &Type, expr: &Expr, other: &Type| {
            let ExprKind::Var(name) = &expr.kind else {
                return None;
            };
            // Only a value no unit can scale is a mistake: `t / s` with a
            // numeric `t` (the tonne's spelling) is ordinary arithmetic.
            (matches!(other, Type::UnitOf(_))
                && !matches!(
                    ty,
                    Type::UnitOf(_)
                        | Type::Scalar(..)
                        | Type::Tensor { .. }
                        | Type::Var(_)
                        | Type::Dyn(_)
                        | Type::Error
                )
                && self.units.lookup(name).is_some())
            .then(|| name.clone())
        };
        candidate(right, right_expr, left).or_else(|| candidate(left, left_expr, right))
    }

    fn synth_binary(
        &mut self,
        op: BinOp,
        left: &Expr,
        right: &Expr,
        span: Span,
    ) -> Type {
        let left_ty = self.synth(left);
        let right_ty = self.synth(right);
        self.binary_type(op, &left_ty, &right_ty, left, right, span)
    }

    /// Commits an unresolved operand to being a scalar.
    ///
    /// Arithmetic is the only place where a bare variable has an obvious
    /// reading, and leaving it unconstrained would make `2 * h / g` fail before
    /// `g`'s declaration is checked. Literals keep their dimension rule: a
    /// bare `2` may only become a dimensionless scalar.
    fn force_scalar(
        &mut self,
        ty: &Type,
    ) -> Type {
        let resolved = self.infer.resolve(ty);
        if !matches!(resolved, Type::Var(_)) {
            return resolved;
        }
        let dim = match self.infer.literal_kind(&resolved) {
            | Some(kind) if !kind.is_quantity() => Dim::one(),
            | _ => self.infer.fresh_dim(),
        };
        let scalar = Type::Scalar(self.infer.fresh_rep(), dim);
        let _ = self.infer.unify(&resolved, &scalar);
        self.infer.resolve(&scalar)
    }

    #[allow(clippy::too_many_arguments)]
    fn binary_type(
        &mut self,
        op: BinOp,
        left_ty: &Type,
        right_ty: &Type,
        left: &Expr,
        right: &Expr,
        span: Span,
    ) -> Type {
        let mut left_resolved = self.infer.resolve(left_ty);
        let mut right_resolved = self.infer.resolve(right_ty);
        // §8.14: measurement arithmetic. The `Measurement<d>` record is a plain
        // record type, so the operators are typed here from its shape rather
        // than from a nominal ADT. `+`/`-` need equal dimensions (E0401),
        // `*`/`/` combine them, and scaling by a plain number keeps `d`.
        if matches!(op, BinOp::Add | BinOp::Sub | BinOp::Mul | BinOp::Div) {
            let left_measurement = self.measurement_parts(&left_resolved);
            let right_measurement = self.measurement_parts(&right_resolved);
            if left_measurement.is_some() || right_measurement.is_some() {
                // The plain-number side may still be an unconstrained literal
                // (`a * 2.0`); commit it to a scalar before deciding.
                if left_measurement.is_some() && matches!(right_resolved, Type::Var(_)) {
                    right_resolved = self.force_scalar(&right_resolved);
                }
                if right_measurement.is_some() && matches!(left_resolved, Type::Var(_)) {
                    left_resolved = self.force_scalar(&left_resolved);
                }
                return self.measurement_binary(
                    op,
                    left_measurement,
                    right_measurement,
                    &left_resolved,
                    &right_resolved,
                    span,
                );
            }
        }
        if matches!(
            op,
            BinOp::Add | BinOp::Sub | BinOp::Mul | BinOp::Div | BinOp::Pow
        ) {
            left_resolved = self.force_scalar(&left_resolved);
            if op != BinOp::Pow {
                right_resolved = self.force_scalar(&right_resolved);
            }
        }
        if let Some(ty) = self.sparse_binary(op, &left_resolved, &right_resolved, left, right, span)
        {
            return ty;
        }
        match op {
            | BinOp::Add | BinOp::Sub => {
                self.add_type(op, &left_resolved, &right_resolved, left, right, span)
            },
            | BinOp::Mul | BinOp::Div
                if matches!(left_resolved, Type::Error)
                    || matches!(right_resolved, Type::Error) =>
            {
                // An operand that failed was reported where it failed.
                Type::Error
            },
            | BinOp::Mul | BinOp::Div
                if self
                    .shadowed_unit_operand(&left_resolved, &right_resolved, left, right)
                    .is_some() =>
            {
                let name = self
                    .shadowed_unit_operand(&left_resolved, &right_resolved, left, right)
                    .unwrap_or_default();
                self.error_help(
                    "E0302",
                    format!(
                        "`{}` is not defined for {left_resolved} and {right_resolved}",
                        op.as_str()
                    ),
                    span,
                    &shadowed_unit_help(&name),
                );
                Type::Error
            },
            | BinOp::Mul => self.mul_type(&left_resolved, &right_resolved, span),
            | BinOp::Div => self.div_type(&left_resolved, &right_resolved, span),
            | BinOp::MatMul => self.matmul_type(&left_resolved, &right_resolved, span),
            | BinOp::Pow => self.pow_type(&left_resolved, right, span),
            | BinOp::Eq | BinOp::Ne | BinOp::Lt | BinOp::Le | BinOp::Gt | BinOp::Ge => {
                if let Err(error) = self.infer.unify(&left_resolved, &right_resolved) {
                    self.report_unify(error, span);
                    return Type::Error;
                }
                let compared = self.infer.resolve(&left_resolved);
                // §3.8.4/§3.7.1: a function has no `Eq` instance — equality on
                // functions is undecidable.
                if contains_function(&compared) {
                    self.error_help(
                        "E0310",
                        format!("functions have no `Eq` instance, found {compared}"),
                        span,
                        "compare the values a function returns, not the function",
                    );
                    return Type::Error;
                }
                match compared {
                    | Type::Scalar(rep, _) if matches!(self.infer.resolve_rep(rep), Rep::Real) => {
                        // §3.8.6: comparing exact reals yields a proposition.
                        Type::Prop
                    },
                    | Type::Prop => Type::Prop,
                    | _ => Type::Bool,
                }
            },
            | BinOp::And | BinOp::Or => {
                match (&left_resolved, &right_resolved) {
                    | (Type::Bool, Type::Bool) => Type::Bool,
                    | (Type::Prop, Type::Prop) => Type::Prop,
                    | _ => {
                        self.error(
                        "E0302",
                        format!(
                            "`{}` expects Bool or Prop operands, found {left_resolved} and {right_resolved}",
                            op.as_str()
                        ),
                        span,
                    );
                        Type::Error
                    },
                }
            },
            | BinOp::Cons => {
                match &right_resolved {
                    | Type::Tensor { elem, .. } => {
                        let elem = (**elem).clone();
                        let _ = self.infer.unify(&left_resolved, &elem);
                        Type::tensor(Shape::Rank(1), elem)
                    },
                    | _ => {
                        self.error("E0302", "`::` expects a tensor on the right", span);
                        Type::Error
                    },
                }
            },
            | BinOp::Pipe | BinOp::PipeBack => {
                self.error("E0202", "pipes are desugared by the parser", span);
                Type::Error
            },
        }
    }

    fn add_type(
        &mut self,
        op: BinOp,
        left: &Type,
        right: &Type,
        left_expr: &Expr,
        right_expr: &Expr,
        span: Span,
    ) -> Type {
        match (left, right) {
            | (Type::Scalar(a_rep, a_dim), Type::Scalar(b_rep, b_dim)) => {
                if let Err(error) = self.infer.unify_dim(a_dim, b_dim) {
                    // §4.7: a bare `0` is not dimension-polymorphic, and the
                    // spec gives it its own code so the fix (`0 m`) is obvious.
                    let left_zero = is_bare_zero(left_expr);
                    let right_zero = is_bare_zero(right_expr);
                    let left_dim = self.infer.resolve_dim(a_dim);
                    let right_dim = self.infer.resolve_dim(b_dim);
                    if (left_zero && !right_dim.is_one()) || (right_zero && !left_dim.is_one()) {
                        self.error_help(
                            "E0406",
                            "a bare `0` is `Int`, not a dimensioned zero",
                            if left_zero {
                                left_expr.span
                            } else {
                                right_expr.span
                            },
                            "write `0 m` (or `0.0 m`) if you meant a zero quantity",
                        );
                        return Type::Error;
                    }
                    self.report_dim_error(error, span, left_expr.span, right_expr.span);
                    return Type::Error;
                }
                let rep = self.join_rep(*a_rep, *b_rep);
                Type::Scalar(rep, self.infer.resolve_dim(a_dim))
            },
            // §4.9.3: `Dyn + Dyn` and `Dyn - Dyn` keep `Dyn<ρ>`; the
            // dimensions must be equal, which the runtime checks (`E0412`).
            | (Type::Dyn(a), Type::Dyn(b)) => {
                if let Err(error) = self.infer.unify_rep(*a, *b) {
                    self.report_unify(error, span);
                    return Type::Error;
                }
                Type::Dyn(self.infer.resolve_rep(*a))
            },
            | (
                Type::Tensor { shape, elem: a },
                Type::Tensor {
                    shape: b_shape,
                    elem: b,
                },
            ) => {
                let shape = match self.infer.broadcast_shape(shape, b_shape) {
                    | Ok(shape) => shape,
                    | Err(error) => {
                        self.report_unify(error, span);
                        shape.clone()
                    },
                };
                let a = self.force_scalar(&self.infer.resolve(a));
                let b = self.force_scalar(&self.infer.resolve(b));
                let elem = self.add_type(op, &a, &b, left_expr, right_expr, span);
                Type::Tensor {
                    shape,
                    elem: Box::new(elem),
                }
            },
            | (Type::Tensor { shape, elem }, Type::Scalar(rep, dim))
            | (Type::Scalar(rep, dim), Type::Tensor { shape, elem }) => {
                let elem = self.force_scalar(&self.infer.resolve(elem));
                if let Type::Scalar(elem_rep, elem_dim) = elem {
                    if let Err(error) = self.infer.unify_dim(&elem_dim, dim) {
                        self.report_dim_error(error, span, left_expr.span, right_expr.span);
                        return Type::Error;
                    }
                    // §3.5.3: `Scalar<ρ1,d> + Tensor<shape, Scalar<ρ2,d>>` is
                    // `Tensor<shape, Scalar<join(ρ1,ρ2), d>>`.
                    let joined = self.join_rep(elem_rep, *rep);
                    Type::Tensor {
                        shape: shape.clone(),
                        elem: Box::new(Type::Scalar(joined, self.infer.resolve_dim(&elem_dim))),
                    }
                } else {
                    Type::Tensor {
                        shape: shape.clone(),
                        elem: Box::new(elem),
                    }
                }
            },
            | (Type::Point(a), Type::Point(b)) => {
                if op == BinOp::Sub {
                    if let Err(error) = self.infer.unify_dim(a, b) {
                        self.report_unify(error, span);
                        return Type::Error;
                    }
                    Type::float(self.infer.resolve_dim(a))
                } else {
                    self.error_help(
                        "E0405",
                        "two affine points cannot be added",
                        span,
                        "subtract them to get a difference, or use `lerp`",
                    );
                    Type::Error
                }
            },
            | (Type::Point(a), Type::Scalar(rep, b)) if op == BinOp::Add => {
                if let Err(error) = self.infer.unify_dim(a, b) {
                    self.report_dim_error(error, span, left_expr.span, right_expr.span);
                    return Type::Error;
                }
                if let Err(error) = self.infer.unify_rep(*rep, Rep::Float) {
                    self.report_unify(error, right_expr.span);
                    return Type::Error;
                }
                Type::Point(self.infer.resolve_dim(a))
            },
            | (Type::Scalar(rep, b), Type::Point(a)) if op == BinOp::Add => {
                if let Err(error) = self.infer.unify_dim(a, b) {
                    self.report_dim_error(error, span, left_expr.span, right_expr.span);
                    return Type::Error;
                }
                if let Err(error) = self.infer.unify_rep(*rep, Rep::Float) {
                    self.report_unify(error, left_expr.span);
                    return Type::Error;
                }
                Type::Point(self.infer.resolve_dim(a))
            },
            | _ => {
                if matches!(left, Type::Dyn(_)) || matches!(right, Type::Dyn(_)) {
                    // §4.9.4: `Dyn<ρ>` is not a `Scalar<ρ,d>` and does not
                    // convert implicitly — the escape layer is explicit.
                    self.error_help(
                        "E0302",
                        format!("`{}` cannot mix `Dyn` with {left} and {right}", op.as_str()),
                        span,
                        "`Dyn` does not convert implicitly; `check` it first",
                    );
                } else if matches!(left, Type::Error) || matches!(right, Type::Error) {
                    // An operand that failed was reported where it failed.
                } else if let Some(name) =
                    self.shadowed_unit_operand(left, right, left_expr, right_expr)
                {
                    self.error_help(
                        "E0302",
                        format!("`{}` is not defined for {left} and {right}", op.as_str()),
                        span,
                        &shadowed_unit_help(&name),
                    );
                } else if op == BinOp::Add
                    && (matches!(left, Type::Str) || matches!(right, Type::Str))
                {
                    self.error_help(
                        "E0302",
                        format!("`+` is not defined for {left} and {right}"),
                        span,
                        "strings are joined with `concat(a, b)` or interpolation `\"{a}{b}\"`",
                    );
                } else {
                    self.error(
                        "E0302",
                        format!("`{}` is not defined for {left} and {right}", op.as_str()),
                        span,
                    );
                }
                Type::Error
            },
        }
    }

    fn mul_type(
        &mut self,
        left: &Type,
        right: &Type,
        span: Span,
    ) -> Type {
        match (left, right) {
            | (Type::Scalar(a_rep, a_dim), Type::Scalar(b_rep, b_dim)) => {
                Type::Scalar(self.join_rep(*a_rep, *b_rep), a_dim.mul(b_dim))
            },
            | (Type::Tensor { shape, elem }, Type::Scalar(rep, dim))
            | (Type::Scalar(rep, dim), Type::Tensor { shape, elem }) => {
                let elem = self.scale_elem(elem, Some(*rep), dim, false, span);
                Type::Tensor {
                    shape: shape.clone(),
                    elem: Box::new(elem),
                }
            },
            // §3.5.3: elementwise multiplication of two tensors of the same
            // rank; shapes must be equal or broadcast (a 1-extent axis).
            | (
                Type::Tensor { shape, elem: a },
                Type::Tensor {
                    shape: b_shape,
                    elem: b,
                },
            ) => {
                let shape = match self.infer.broadcast_shape(shape, b_shape) {
                    | Ok(shape) => shape,
                    | Err(error) => {
                        self.report_unify(error, span);
                        shape.clone()
                    },
                };
                let a = self.force_scalar(&self.infer.resolve(a));
                let b = self.force_scalar(&self.infer.resolve(b));
                let elem = self.mul_type(&a, &b, span);
                Type::Tensor {
                    shape,
                    elem: Box::new(elem),
                }
            },
            // §4.9.3: `Dyn * Dyn` adds the runtime dimensions.
            | (Type::Dyn(a), Type::Dyn(b)) => {
                if let Err(error) = self.infer.unify_rep(*a, *b) {
                    self.report_unify(error, span);
                    return Type::Error;
                }
                Type::Dyn(self.infer.resolve_rep(*a))
            },
            | (Type::UnitOf(a), Type::UnitOf(b)) => Type::UnitOf(a.mul(b)),
            | (Type::UnitOf(dim), Type::Scalar(rep, other))
            | (Type::Scalar(rep, other), Type::UnitOf(dim)) => Type::Scalar(*rep, other.mul(dim)),
            | (Type::UnitOf(dim), Type::Tensor { shape, elem })
            | (Type::Tensor { shape, elem }, Type::UnitOf(dim)) => {
                // §3.5.3: `Unit[e] * Tensor<shape, Scalar<ρ,d>>` keeps ρ.
                let elem = self.scale_elem(elem, None, dim, false, span);
                Type::Tensor {
                    shape: shape.clone(),
                    elem: Box::new(elem),
                }
            },
            | (Type::Point(_), _) | (_, Type::Point(_)) => {
                self.error("E0405", "affine points cannot be multiplied", span);
                Type::Error
            },
            | _ => {
                if matches!(left, Type::Dyn(_)) || matches!(right, Type::Dyn(_)) {
                    self.error_help(
                        "E0302",
                        format!("`*` cannot mix `Dyn` with {left} and {right}"),
                        span,
                        "`Dyn` does not convert implicitly; `check` it first",
                    );
                } else {
                    self.error(
                        "E0302",
                        format!("`*` is not defined for {left} and {right}"),
                        span,
                    );
                }
                Type::Error
            },
        }
    }

    fn div_type(
        &mut self,
        left: &Type,
        right: &Type,
        span: Span,
    ) -> Type {
        match (left, right) {
            | (Type::Scalar(a_rep, a_dim), Type::Scalar(b_rep, b_dim)) => {
                let joined = self.join_rep(*a_rep, *b_rep);
                Type::Scalar(self.div_rep(joined), a_dim.div(b_dim))
            },
            | (Type::Tensor { shape, elem }, Type::Scalar(rep, dim)) => {
                let elem = self.scale_elem(elem, Some(*rep), dim, true, span);
                Type::Tensor {
                    shape: shape.clone(),
                    elem: Box::new(elem),
                }
            },
            // §3.5.3: elementwise division of two equal-rank tensors.
            | (
                Type::Tensor { shape, elem: a },
                Type::Tensor {
                    shape: b_shape,
                    elem: b,
                },
            ) => {
                let shape = match self.infer.broadcast_shape(shape, b_shape) {
                    | Ok(shape) => shape,
                    | Err(error) => {
                        self.report_unify(error, span);
                        shape.clone()
                    },
                };
                let a = self.force_scalar(&self.infer.resolve(a));
                let b = self.force_scalar(&self.infer.resolve(b));
                let elem = self.div_type(&a, &b, span);
                Type::Tensor {
                    shape,
                    elem: Box::new(elem),
                }
            },
            // §4.9.3: `Dyn / Dyn` subtracts the runtime dimensions.
            | (Type::Dyn(a), Type::Dyn(b)) => {
                if let Err(error) = self.infer.unify_rep(*a, *b) {
                    self.report_unify(error, span);
                    return Type::Error;
                }
                Type::Dyn(self.infer.resolve_rep(*a))
            },
            | (Type::UnitOf(a), Type::UnitOf(b)) => Type::UnitOf(a.div(b)),
            | (Type::Scalar(rep, other), Type::UnitOf(dim)) => Type::Scalar(*rep, other.div(dim)),
            | (Type::Tensor { shape, elem }, Type::UnitOf(dim)) => {
                // §3.5.3: `Tensor<shape, Scalar<ρ,d>> / Unit[e]` keeps ρ and
                // divides the dimension; it is not a scalar division, so `ρ` is
                // not `div_rep`'d.
                let elem = self.scale_elem(elem, None, &dim.inverse(), false, span);
                Type::Tensor {
                    shape: shape.clone(),
                    elem: Box::new(elem),
                }
            },
            // §5.6.3: `p / k` and `p / K` are `E0405`, not a generic mismatch.
            | (Type::Point(_), _) | (_, Type::Point(_)) => {
                self.error_help(
                    "E0405",
                    "an affine point cannot be divided",
                    span,
                    "divide a temperature *difference*, or use `lerp`",
                );
                Type::Error
            },
            | _ => {
                if matches!(left, Type::Dyn(_)) || matches!(right, Type::Dyn(_)) {
                    self.error_help(
                        "E0302",
                        format!("`/` cannot mix `Dyn` with {left} and {right}"),
                        span,
                        "`Dyn` does not convert implicitly; `check` it first",
                    );
                } else {
                    self.error(
                        "E0302",
                        format!("`/` is not defined for {left} and {right}"),
                        span,
                    );
                }
                Type::Error
            },
        }
    }

    /// Scales or divides a tensor's element type.
    ///
    /// Division promotes an integer element to `Real` (`div_rep`, §3.4.2) but
    /// keeps a `Float` element on the IEEE path: `Tensor<Float> / 2` stays
    /// `Tensor<Float>`.
    ///
    /// `rep` is the representation the surrounding expression contributes, or
    /// `None` when the element's own representation is kept: §3.5.3 says
    /// `Tensor<shape, Scalar<ρ,d>> * Unit[e]` is `Tensor<shape, Scalar<ρ,d*e>>`,
    /// so a unit never changes ρ. A tensor literal's element is still a fresh
    /// variable at this point, so it is committed to a scalar first — otherwise
    /// `[[1.0]] * ohm` reported "cannot scale a tensor of t3 by a unit".
    fn scale_elem(
        &mut self,
        elem: &Type,
        rep: Option<Rep>,
        dim: &Dim,
        divide: bool,
        span: Span,
    ) -> Type {
        let elem = self.force_scalar(&self.infer.resolve(elem));
        match elem {
            | Type::Scalar(elem_rep, elem_dim) => {
                let joined = match rep {
                    | Some(rep) => self.join_rep(elem_rep, rep),
                    | None => elem_rep,
                };
                if divide {
                    Type::Scalar(self.div_rep(joined), elem_dim.div(dim))
                } else {
                    Type::Scalar(joined, elem_dim.mul(dim))
                }
            },
            | other => {
                self.error(
                    "E0302",
                    format!("cannot scale a tensor of {other} by a unit"),
                    span,
                );
                Type::Error
            },
        }
    }

    /// `div_rep` of §3.4.2: `Int / Int` and `2^-1` are exact rationals.
    ///
    /// A literal that could still be an `Int` is committed to `Real` here,
    /// because the operation itself decides the representation.
    fn div_rep(
        &mut self,
        rep: Rep,
    ) -> Rep {
        match self.infer.divide_representation(rep) {
            | Ok(rep) => rep,
            | Err(error) => {
                self.report_unify(error, Span::new(self.file, 0, 0));
                Rep::Real
            },
        }
    }

    fn matmul_type(
        &mut self,
        left: &Type,
        right: &Type,
        span: Span,
    ) -> Type {
        let (
            Type::Tensor {
                shape: left_shape,
                elem: le,
            },
            Type::Tensor {
                shape: right_shape,
                elem: re,
            },
        ) = (left, right)
        else {
            self.error(
                "E0302",
                format!("`@` expects two tensors, found {left} and {right}"),
                span,
            );
            return Type::Error;
        };
        let elem = self.multiply_elems(le, re, span);
        // §3.5.3.1: the shared extent is unified with the Nat solver, so
        // `Tensor<[m, k]> @ Tensor<[k, n]>` works for any of the three being a
        // type-level variable (§3.5.1).
        let ls = self.infer.resolve_shape(left_shape);
        let rs = self.infer.resolve_shape(right_shape);
        if let (Shape::Refined(left_items), Shape::Refined(right_items)) = (&ls, &rs)
            && let (Some(k), Some(k2)) = (
                left_items.get(1).cloned().flatten(),
                right_items.first().cloned().flatten(),
            )
            && let Err(error) = self.infer.unify_nat(&k, &k2)
        {
            // A concrete mismatch is a shape error (`E0802`); a
            // non-linear Nat equation is `E0304`.
            match error {
                | crate::infer::UnifyError::NatMismatch { .. } => {
                    let left_extent = self.infer.resolve_nat(&k);
                    let right_extent = self.infer.resolve_nat(&k2);
                    self.error(
                                "E0802",
                                format!(
                                    "matmul requires left columns ({left_extent}) = right rows ({right_extent})"
                                ),
                                span,
                            );
                },
                | other => self.report_unify(other, span),
            }
            return Type::Error;
        }
        match (ls.rank(), rs.rank()) {
            | (2, 2) => {
                Type::Tensor {
                    shape: match (&ls, &rs) {
                        | (Shape::Refined(a), Shape::Refined(b)) => {
                            Shape::Refined(vec![
                                a.first().cloned().flatten(),
                                b.get(1).cloned().flatten(),
                            ])
                        },
                        | _ => Shape::Rank(2),
                    },
                    elem: Box::new(elem),
                }
            },
            | (2, 1) | (1, 2) => {
                // Collapse the shared axis and keep the surviving extent when
                // it is statically known: `Tensor<[2, 3]> @ Tensor<[3]>` is
                // `Tensor<[2]>`, and `Tensor<[3]> @ Tensor<[3, 4]>` is
                // `Tensor<[4]>`. Unknown extents fall back to rank-only.
                let surviving = if ls.rank() == 2 {
                    match &ls {
                        | Shape::Refined(items) => items.first().cloned().flatten(),
                        | Shape::Rank(_) => None,
                    }
                } else {
                    match &rs {
                        | Shape::Refined(items) => items.get(1).cloned().flatten(),
                        | Shape::Rank(_) => None,
                    }
                };
                let shape = surviving.map_or(Shape::Rank(1), |extent| {
                    Shape::Refined(vec![Some(self.infer.resolve_nat(&extent))])
                });
                Type::Tensor {
                    shape,
                    elem: Box::new(elem),
                }
            },
            | (1, 1) => elem,
            | _ => {
                self.error(
                    "E0302",
                    format!(
                        "`@` is not defined for tensors of rank {} and {}",
                        ls.rank(),
                        rs.rank()
                    ),
                    span,
                );
                Type::Error
            },
        }
    }

    fn multiply_elems(
        &mut self,
        left: &Type,
        right: &Type,
        span: Span,
    ) -> Type {
        let left = self.force_scalar(&self.infer.resolve(left));
        let right = self.force_scalar(&self.infer.resolve(right));
        match (&left, &right) {
            | (Type::Scalar(a_rep, a_dim), Type::Scalar(b_rep, b_dim)) => {
                Type::Scalar(self.join_rep(*a_rep, *b_rep), a_dim.mul(b_dim))
            },
            | _ => {
                self.error(
                    "E0411",
                    format!("incompatible tensor element types {left} and {right}"),
                    span,
                );
                Type::Error
            },
        }
    }

    fn pow_type(
        &mut self,
        base: &Type,
        exponent: &Expr,
        span: Span,
    ) -> Type {
        // §4.11: the exponent is a number, never a quantity — `2^(3 m)` is
        // `E0402`, not a silent mismatch.
        let exponent_synth = self.synth(exponent);
        let exponent_ty = self.infer.resolve(&exponent_synth);
        match &exponent_ty {
            | Type::Scalar(_, dim) => {
                let resolved = self.infer.resolve_dim(dim);
                if !resolved.is_one()
                    && (resolved.is_concrete()
                        || self.infer.unify_dim(&resolved, &Dim::one()).is_err())
                {
                    let formatted = self.dims.format(&resolved);
                    self.error_help(
                        "E0402",
                        format!("the exponent of `^` must be dimensionless, found {formatted}"),
                        exponent.span,
                        "divide the exponent by its unit first",
                    );
                    return Type::Error;
                }
            },
            | Type::Var(_) | Type::Error => {},
            | other => {
                self.error(
                    "E0402",
                    format!("the exponent of `^` must be a number, found {other}"),
                    exponent.span,
                );
                return Type::Error;
            },
        }
        // §4.9.3: `(^) : Dyn<ρ> × Int -> Dyn<ρ>` — the runtime dimension
        // exponent is multiplied. Unlike `Scalar`, the exponent may be a
        // runtime `Int`: that is the whole point of `Dyn`.
        if let Type::Dyn(rep) = base {
            if let Err(error) = self.infer.unify(&exponent_ty, &Type::int()) {
                self.report_unify(error, exponent.span);
                return Type::Error;
            }
            return Type::Dyn(self.infer.resolve_rep(*rep));
        }
        let exponent_value = literal_exponent(exponent);
        let Some(exponent_value) = exponent_value else {
            // A runtime exponent of a non-`Int` representation makes the
            // power non-integral in general: `2^x` with `x : Real` is the
            // `Real` term `2^x`, and with `x : Float` a `Float`. An `Int`
            // exponent keeps the base's representation.
            let exponent_rep = match &exponent_ty {
                | Type::Scalar(rep, _) => Some(self.infer.resolve_rep(*rep)),
                | _ => None,
            };
            // §3.4.3: a runtime exponent needs a dimensionless base. When the
            // base's dimension is still an unsolved variable (a lambda
            // parameter such as `t` in `diff(fn t => t^t)`), that requirement
            // is a constraint, not an error: the variable is solved to `1`.
            // A dimension that is already known not to be `1` stays `E0404`.
            let base = match base {
                | Type::Scalar(rep, dim) if !dim.is_concrete() => {
                    // Resolve again: checking the exponent may already have
                    // solved the variable (`t^t` makes `t` dimensionless).
                    let resolved = self.infer.resolve_dim(dim);
                    let declared = self.dim_params.iter().any(|(_, binder)| {
                        let binder = self.infer.resolve_dim(binder);
                        binder.vars().iter().any(|id| resolved.contains_var(*id))
                    });
                    if resolved.is_one()
                        || (!declared
                            && !resolved.is_concrete()
                            && self.infer.unify_dim(&resolved, &Dim::one()).is_ok())
                    {
                        Type::Scalar(*rep, Dim::one())
                    } else {
                        base.clone()
                    }
                },
                | other => other.clone(),
            };
            let base = &base;
            return match base {
                | Type::Scalar(rep, dim) if dim.is_one() => {
                    match exponent_rep {
                        | Some(exponent_rep @ (Rep::Real | Rep::Float | Rep::Complex)) => {
                            let base_rep = self.div_rep(*rep);
                            Type::Scalar(self.join_rep(base_rep, exponent_rep), Dim::one())
                        },
                        | _ => Type::Scalar(*rep, Dim::one()),
                    }
                },
                | Type::Scalar(_, _) => {
                    self.error_help(
                        "E0404",
                        "a dimensioned value cannot be raised to a runtime exponent",
                        span,
                        "use a literal exponent, or `pow_dyn` for a checked dynamic power",
                    );
                    Type::Error
                },
                | other => other.clone(),
            };
        };
        match base {
            | Type::Scalar(rep, dim) => {
                // §3.4.2: a negative or fractional exponent divides, so an
                // integer base becomes an exact rational: `2^-1` is `Real`.
                let rep = if exponent_value.is_integer() && !exponent_value.is_negative() {
                    *rep
                } else {
                    self.div_rep(*rep)
                };
                Type::Scalar(rep, dim.pow(&exponent_value))
            },
            | Type::Tensor { shape, elem } => {
                Type::Tensor {
                    shape: shape.clone(),
                    elem: Box::new(self.pow_type(elem, exponent, span)),
                }
            },
            | other => {
                self.error("E0302", format!("`^` is not defined for {other}"), span);
                Type::Error
            },
        }
    }

    /// `join(ρ1, ρ2)` of §3.4.1.
    ///
    /// Two concrete representations take their least upper bound — `join(Int,
    /// Float) = Float`, so `fn f(a: Int): Int = a + 2.0` is a type error
    /// instead of silently typing as `Int`. When one side is a variable the
    /// other side becomes a *lower bound*: the variable stays free,
    /// so `fn f(x) = x + 1` is polymorphic rather than `Int -> Int`.
    fn join_rep(
        &mut self,
        a: Rep,
        b: Rep,
    ) -> Rep {
        let mut a = self.infer.resolve_rep(a);
        let mut b = self.infer.resolve_rep(b);
        let protected = self.lexical_numeric_variables();
        let a_kind = self.infer.direct_literal_kind(a);
        let b_kind = self.infer.direct_literal_kind(b);
        let unprotected =
            |rep: Rep| !matches!(rep,Rep::Var(id) if protected.representations.contains_key(&id));
        let decimal =
            |kind: LiteralKind| matches!(kind, LiteralKind::Decimal | LiteralKind::QuantityDecimal);
        // §3.10.2: a mixed integer/decimal literal pair keeps two independent
        // domains; each leaf defaults by its own rule and the result is their
        // `join` (`1 + 1.0 : Float`). Forcing both to `Real` here was the
        // recorded "exact decimal folding" deviation. Exactness of a closed
        // literal tree is now a *value* property — the constant region is
        // folded exactly and rounded once (see `constant_regions`) — so it no
        // longer changes the static type.
        let _ = decimal;
        for (variable, concrete, kind) in [(a, b, a_kind), (b, a, b_kind)] {
            if matches!(variable, Rep::Var(_))
                && !matches!(concrete, Rep::Var(_))
                && unprotected(variable)
                && let Some(kind) = kind
                && let Some(reading) = literal_reading_above(kind, concrete)
                && let Err(error) = self.infer.unify_rep(variable, reading)
            {
                self.report_unify(error, Span::new(self.file, 0, 0));
            }
        }
        a = self.infer.resolve_rep(a);
        b = self.infer.resolve_rep(b);
        if matches!((a, b), (Rep::Real, Rep::Float) | (Rep::Float, Rep::Real)) {
            self.mixed_rep = true;
        }
        match self.infer.join_representation(a, b) {
            | Ok(rep) => rep,
            | Err(error) => {
                self.report_unify(error, Span::new(self.file, 0, 0));
                Rep::Real
            },
        }
    }

    /// Whether `name` is a value binding that shadows a builtin in call
    /// position.
    ///
    /// A unit name is *not* a value binding: §1.8.2 keeps the unit namespace
    /// separate from the value namespace, so `at` is both the technical
    /// atmosphere (a unit, `3 at`) and the §8.4 indexing builtin (`at(t, 0)`).
    /// Only a real value binding shadows a builtin; the unit table's entries in
    /// `globals` merely make the unit usable as a bare value.
    /// Whether `name` is declared by *this* module (a top-level `let`/`fn` or a
    /// local binding) rather than imported.
    ///
    /// Imported values also live in `globals`, but they must not disable the
    /// type-function typing of the built-in entry points: `Ode.aev` exports
    /// `let odeint = odeint`, whose re-exported HM scheme types the right-hand
    /// side as returning the *state* instead of `RateOf<state>` (§8.9). A local
    /// declaration is a genuine shadow and keeps the ordinary path.
    fn is_locally_defined(
        &self,
        name: &str,
    ) -> bool {
        if self.top_lets.contains_key(name) || self.fn_signatures.contains_key(name) {
            return true;
        }
        self.scopes
            .iter()
            .rev()
            .any(|scope| scope.contains_key(name))
    }

    fn synth_call(
        &mut self,
        callee: &Expr,
        args: &[Expr],
        span: Span,
    ) -> Type {
        let ty = self.synth_call_inner(callee, args, span);
        if !args
            .iter()
            .any(|arg| matches!(arg.kind, ExprKind::Placeholder))
        {
            let materializations = self
                .call_materializations
                .get(&callee.span)
                .cloned()
                .unwrap_or_default();
            let protected = self.lexical_numeric_variables();
            for ty in materializations {
                self.infer.weaken_materialized(&ty, &protected);
            }
        }
        ty
    }

    fn synth_call_inner(
        &mut self,
        callee: &Expr,
        args: &[Expr],
        span: Span,
    ) -> Type {
        if args
            .iter()
            .any(|arg| matches!(arg.kind, ExprKind::Placeholder))
        {
            return self.partial_application(callee, args, span);
        }
        // `Rssn.<op>(...)`: resolve the overload *here* and pin it, so the
        // runtime cannot execute a different one.
        if let Some(op) = rssn_op_name(callee) {
            return self.rssn_call(&op, args, span);
        }
        // `Rssn.invoke` names an operator by *string*, so its result type is
        // not known statically. Typing it as a fresh variable would be
        // `∀a. a` — every call site could claim a different type. It returns
        // the dynamic value type instead, which the caller must `check`.
        if is_rssn_member(callee, "invoke") {
            let expected = [Type::Str, Type::Str, value_list_type()];
            for (arg, ty) in args.iter().zip(expected.iter()) {
                self.check(arg, ty);
            }
            if args.len() != expected.len() {
                self.error_help(
                    "E0315",
                    format!("`Rssn.invoke` takes 3 arguments, found {}", args.len()),
                    span,
                    "the generated declaration always passes a name, a signature hash and a list",
                );
            }
            // §13.2.2: when both keys are string literals, they name exactly
            // one manifest overload by its pinned `OpKey` digest, so the call
            // has that overload's declared result type. Anything else keeps
            // the dynamic value type and lets the runtime report E1005.
            if args.len() == expected.len()
                && let (Some(op_name), Some(hash)) =
                    (string_constant(&args[0]), string_constant(&args[1]))
                && let Some(ty) =
                    crate::rssn_resolve::invoke_result(&op_name, &hash, &mut self.infer)
            {
                return ty;
            }
            return dynamic_value_type();
        }
        // A class method is solved against the instance table after the
        // arguments are checked, so the constraint sees the argument's type.
        if let ExprKind::Var(name) = &callee.kind
            && let Some(scheme) = self.globals.get(name).cloned()
            && self.method_bindings.contains_key(name)
            && !self.is_locally_defined(name)
        {
            return self.class_method_call(name, &scheme, args, span);
        }
        // `zeros<Float>(3, 4)`: the type argument fixes the element type.
        if let ExprKind::TypeApp {
            callee: inner,
            args: type_args,
        } = &callee.kind
        {
            if let ExprKind::Var(name) = &inner.kind {
                // §3.5.1: `reshape<[3, 2]>(a)`, `zeros_of<[3, 4]>()` and
                // `ones_of<[3, 4]>()` name their target in a *shape* type
                // argument, so the result has the refined shape.
                if matches!(
                    name.as_str(),
                    "reshape" | "zeros_of" | "ones_of" | "refine_shape"
                ) {
                    return self.shape_constructor(name, type_args, args, span);
                }
                // §4.9.4: `check<d>(x)` names the dimension to materialise. A
                // `dyn` takes no type argument: its `d` comes from the argument.
                if name == "check" {
                    return self.check_call(args, type_args.first(), span);
                }
                if name == "from_system" && !self.is_locally_defined(name) {
                    return self.system_call(false, args, type_args.first(), span);
                }
                if builtins::takes_format_argument(name) && !self.is_locally_defined(name) {
                    return self.sparse_format_call(name, type_args, args, span);
                }
            }
            let inner_ty = self.synth(inner);
            if let Type::TensorIntrinsic(intrinsic) = self.infer.resolve(&inner_ty) {
                return self.tensor_constructor(intrinsic, type_args, args, span);
            }
            if let Type::FromValueIntrinsic(specialization) = self.infer.resolve(&inner_ty) {
                if specialization.is_some() || type_args.len() != 1 {
                    self.error(
                        "E0315",
                        "`from_value` requires one type argument when specialized",
                        span,
                    );
                    return Type::Error;
                }
                let Some(ty) = self.reify_type_argument(&type_args[0]) else {
                    return Type::Error;
                };
                return self.from_value_call(Some(ty), args, span);
            }
            if let Type::TypeRepIntrinsic(specialization) = self.infer.resolve(&inner_ty) {
                if specialization.is_some() || type_args.len() != 1 {
                    self.error(
                        "E0315",
                        "`type_rep` requires exactly one type argument",
                        span,
                    );
                    return Type::Error;
                }
                if self.reify_type_argument(&type_args[0]).is_none() {
                    return Type::Error;
                }
                return self.type_rep_call(args, span);
            }
            // User functions must apply their ordered binders here as well as
            // in a standalone `f<T>` expression. Otherwise calls silently
            // ignore explicit arguments and omit their runtime evidence.
            let applied = self.apply_type_arguments(inner, type_args, callee.span, inner_ty);
            self.note_type(callee.span, &applied);
            if let Type::ReadNpyIntrinsic(rank) = self.infer.resolve(&applied) {
                return self.read_npy_call(rank, args, span);
            }
            return self.call_with_type(applied, args, span);
        }
        if let ExprKind::Var(name) = &callee.kind {
            // Resolve a lexical binding before falling back to an ambient intrinsic.
            let bound = self
                .scopes
                .iter()
                .rev()
                .find_map(|scope| scope.get(name))
                .or_else(|| self.globals.get(name))
                .map(|scheme| self.infer.resolve(&scheme.ty));
            if let Some(Type::FromValueIntrinsic(specialization)) = &bound {
                self.synth(callee);
                return self.from_value_call(specialization.as_deref().cloned(), args, span);
            }
            if bound.is_none() && name == "from_value" {
                self.synth(callee);
                return self.from_value_call(None, args, span);
            }
            if matches!(&bound, Some(Type::ToCallbackIntrinsic))
                || (bound.is_none() && name == "to_callback")
            {
                self.synth(callee);
                return self.to_callback_call(args, span);
            }
            if let Some(Type::TypeRepIntrinsic(specialization)) = &bound {
                self.synth(callee);
                if specialization.is_none() {
                    self.error(
                        "E0315",
                        "`type_rep` requires an explicit type argument",
                        span,
                    );
                    return Type::Error;
                }
                return self.type_rep_call(args, span);
            }
            if bound.is_none() && name == "type_rep" {
                self.error(
                    "E0315",
                    "`type_rep` requires an explicit type argument",
                    span,
                );
                return Type::Error;
            }
            let intrinsic = match bound {
                | Some(Type::TensorIntrinsic(intrinsic)) => Some(intrinsic),
                | Some(_) => None,
                | None => {
                    crate::ty::TensorIntrinsic::from_name(name)
                        .map(|kind| crate::ty::TensorIntrinsicType { kind, element: None })
                },
            };
            if let Some(intrinsic) = intrinsic {
                self.synth(callee);
                return self.tensor_constructor(intrinsic, &[], args, span);
            }
            // A *local* declaration shadows a built-in entry point (so
            // `let odeint = …` wins); an import does not — the module tree
            // re-exports `odeint`, `sdeint_diag`, … with their loose HM
            // schemes, while the type-function typing lives here (§8.9).
            let shadowed = self.is_locally_defined(name);
            if shadowed {
                let callee_ty = self.synth(callee);
                return self.call_with_type(callee_ty, args, span);
            }
            // A prelude unit is also a value (`min` is a minute, `dyn` is the
            // dyne), but in *call* position the builtin function of that name
            // wins for the ordinary data functions. The checker's
            // type-directed primitives are excluded: they need their own
            // special typing below (`dyn`, `check`, `rank`, …), not the generic
            // builtin scheme.
            let has_special_typing = matches!(
                name.as_str(),
                "dyn"
                    | "check"
                    | "odeint"
                    | "odeint_with"
                    | "solve_ivp"
                    | "sdeint_diag"
                    | "sdeint_general"
                    | "sde_ensemble_diag"
                    | "sde_ensemble_general"
                    | "ode_solve"
                    | "ode_bvp"
                    | "odeint_order"
                    | "odeint_order_with"
                    | "par_map"
                    | "rank"
                    | "shape"
                    | "write_npy"
                    | "min_all"
                    | "max_all"
                    | "transpose"
            );
            if !has_special_typing
                && let Some(scheme) = self.globals.get(name)
                && matches!(scheme.ty, Type::UnitOf(_))
                && let Some(candidates) = builtins::schemes(name)
            {
                if candidates.len() > 1 {
                    return self.overloaded_call(name, &candidates, args, span);
                }
                let ty = self.infer.instantiate(&candidates[0]);
                return self.call_with_type(ty, args, span);
            }
            // §8.9: the ODE entry points are typed through the built-in type
            // function `RateOf<s>`, which unification cannot express.
            if matches!(name.as_str(), "odeint" | "odeint_with" | "solve_ivp") {
                return self.ode_call(name, args, span);
            }
            // §8.9.1: the SDE entry points use two built-in type functions —
            // `RateOf` for the drift and `DiffusionOf` for the diffusion.
            if matches!(
                name.as_str(),
                "sdeint_diag" | "sdeint_general" | "sde_ensemble_diag" | "sde_ensemble_general"
            ) {
                return self.sde_call(name, args, span);
            }
            if matches!(
                name.as_str(),
                "ode_solve" | "ode_bvp" | "odeint_order" | "odeint_order_with"
            ) {
                return self.ode_ext_call(name, args, span);
            }
            // §3.11 / §4.9.4: the type-directed `Dyn` primitives.
            if name == "dyn" {
                return self.dyn_call(args, span);
            }
            if name == "check" {
                return self.check_call(args, None, span);
            }
            if matches!(name.as_str(), "to_system" | "from_system")
                && !self.is_locally_defined(name)
            {
                return self.system_call(name == "to_system", args, None, span);
            }
            // §8.1: `min_all`/`max_all` reduce a whole tensor of any rank to
            // its scalar element (`min`/`max` stay the binary scalar
            // functions). A scheme cannot quantify the shape's rank, so the
            // element/dimension relationship is enforced here instead of by a
            // rank-1-only scheme.
            if matches!(name.as_str(), "sparse_coo" | "contract") {
                return self.sparse_special_call(name, args, span);
            }
            if matches!(name.as_str(), "min_all" | "max_all") {
                if args.len() != 1 {
                    self.error(
                        "E0315",
                        format!("`{name}` takes 1 argument, found {}", args.len()),
                        span,
                    );
                    for arg in args {
                        self.synth(arg);
                    }
                    return Type::Error;
                }
                let arg_ty = self.synth(&args[0]);
                return match self.infer.resolve(&arg_ty) {
                    | Type::Tensor { elem, .. } => {
                        // A tensor literal's element can still be a literal
                        // type variable; commit it to a scalar first.
                        let elem = self.force_scalar(&elem);
                        match self.infer.resolve(&elem) {
                            | Type::Scalar(rep, dim) => Type::Scalar(rep, dim),
                            | other => {
                                self.error_help(
                                    "E0302",
                                    format!(
                                        "`{name}` reduces a tensor of scalars, found elements of type {other}"
                                    ),
                                    args[0].span,
                                    "reduce a `Tensor<n, Scalar<ρ, d>>`; it returns its element type",
                                );
                                Type::Error
                            },
                        }
                    },
                    | other => {
                        self.error_help(
                            "E0302",
                            format!("`{name}` expects a tensor, found {other}"),
                            args[0].span,
                            "`min_all`/`max_all` reduce a whole tensor of any rank",
                        );
                        Type::Error
                    },
                };
            }
            // §8.1/§8.4: `rank` and `shape` require a tensor; the scheme used
            // to take a fresh type variable, so `rank(3)` type-checked.
            if matches!(name.as_str(), "rank" | "shape") {
                if args.len() != 1 {
                    self.error(
                        "E0315",
                        format!("`{name}` takes 1 argument, found {}", args.len()),
                        span,
                    );
                    for arg in args {
                        self.synth(arg);
                    }
                    return Type::Error;
                }
                let arg_ty = self.synth(&args[0]);
                let arg_ty = match builtins::sparse_parts(&self.infer.resolve(&arg_ty)) {
                    | Some((_, dense)) => dense.clone(),
                    | None => arg_ty,
                };
                return match self.infer.resolve(&arg_ty) {
                    | Type::Tensor { shape, .. } => {
                        if name == "rank" {
                            Type::int()
                        } else {
                            // `shape : Tensor<n, τ> -> Tensor<1, Int>`; the
                            // length is the rank, which is known.
                            Type::tensor(
                                Shape::Refined(vec![Some(Nat::lit(
                                    usize::try_from(shape.rank()).unwrap_or(0),
                                ))]),
                                Type::int(),
                            )
                        }
                    },
                    | other => {
                        self.error_help(
                            "E0302",
                            format!("`{name}` expects a tensor, found {other}"),
                            args[0].span,
                            "`rank`/`shape` describe a `Tensor<n, τ>` value",
                        );
                        Type::Error
                    },
                };
            }
            // §8.4: `transpose` swaps the two extents of a rank-2 tensor; the
            // refined shape is preserved (`[m, n]` → `[n, m]`), and anything
            // that is not a rank-2 tensor is an error here rather than a
            // runtime surprise.
            if name == "transpose" {
                if args.len() != 1 {
                    self.error(
                        "E0315",
                        format!("`transpose` takes 1 argument, found {}", args.len()),
                        span,
                    );
                    for arg in args {
                        self.synth(arg);
                    }
                    return Type::Error;
                }
                let arg_ty = self.synth(&args[0]);
                let arg_resolved = self.infer.resolve(&arg_ty);
                if let Some((format, dense)) = builtins::sparse_parts(&arg_resolved) {
                    let format = format.clone();
                    return match dense {
                        | Type::Tensor { shape, elem } if shape.rank() == 2 => {
                            let swapped = match shape {
                                | Shape::Refined(items) if items.len() == 2 => {
                                    Shape::Refined(vec![items[1].clone(), items[0].clone()])
                                },
                                | _ => Shape::Rank(2),
                            };
                            builtins::sparse_type(
                                format,
                                Type::Tensor {
                                    shape: swapped,
                                    elem: elem.clone(),
                                },
                            )
                        },
                        | other => {
                            self.error_help(
                                "E0302",
                                format!(
                                    "`transpose` expects a rank-2 sparse matrix, found {other}"
                                ),
                                args[0].span,
                                "only a matrix can be transposed",
                            );
                            Type::Error
                        },
                    };
                }
                return match self.infer.resolve(&arg_ty) {
                    | Type::Tensor { shape, elem } if shape.rank() >= 2 => {
                        // §8.4: `transpose` reverses the axis order, so a
                        // refined shape is reversed whole (`[m, n]` → `[n, m]`,
                        // `[a, b, c]` → `[c, b, a]`).
                        let rank = shape.rank();
                        let swapped = match &shape {
                            | Shape::Refined(items) if items.len() as u32 == rank => {
                                Shape::Refined(items.iter().rev().cloned().collect())
                            },
                            | _ => Shape::Rank(rank),
                        };
                        Type::Tensor { shape: swapped, elem }
                    },
                    | other => {
                        self.error_help(
                            "E0302",
                            format!("`transpose` expects a tensor of rank 2 or more, found {other}"),
                            args[0].span,
                            "transpose a matrix (`Tensor<2, τ>`) or any higher-rank tensor, which reverses its axes; a vector is its own transpose",
                        );
                        Type::Error
                    },
                };
            }
            // §6.10: explicit parallelism needs `Parallel a` (§3.8.4).
            if name == "par_map" {
                return self.par_map_call(args, span);
            }
            if is_transcendental(name)
                && let Some(arg) = args.first()
            {
                let ty = self.synth(arg);
                if let Type::Scalar(rep, dim) = self.infer.resolve(&ty) {
                    // An unresolved dimension is *committed* to 1: `exp(t)`
                    // in an unannotated lambda is dimensionless until
                    // something else says otherwise. A concrete dimension
                    // other than 1 is the error §3.4.4 describes.
                    let resolved = self.infer.resolve_dim(&dim);
                    if !resolved.is_one()
                        && (resolved.is_concrete()
                            || self.infer.unify_dim(&resolved, &Dim::one()).is_err())
                    {
                        let formatted = self.dims.format(&resolved);
                        self.error_help(
                            "E0403",
                            format!(
                                "`{name}` requires a dimensionless argument, found {formatted}"
                            ),
                            arg.span,
                            "divide by a unit first, for example `x / 1 m`",
                        );
                        return Type::Error;
                    }
                    return Type::Scalar(rep, Dim::one());
                }
            }
            if builtins::is_special(name) && !matches!(name.as_str(), "zeros" | "ones" | "fill") {
                return self.special_call(name, args, span);
            }
            // §5.4.1: an affine unit displays a `Point`, never a temperature
            // difference — `value_in(°C, 10 K)` is `E0510`, not a conversion.
            if matches!(name.as_str(), "value_in" | "to")
                && let Some(ty) = self.unit_display_call(name, args, span)
            {
                return ty;
            }
            // §4.11: `det`/`prod` of a dimensioned tensor cannot be expressed
            // statically — the exponent is the runtime order.
            if matches!(name.as_str(), "det" | "prod") {
                let saved = self.infer.clone();
                let before = self.diagnostics.items.len();
                let types_before = self.expr_types.len();
                if let Some(arg) = args.first() {
                    let ty = self.synth(arg);
                    if let Type::Tensor { elem, .. } = self.infer.resolve(&ty)
                        && let Type::Scalar(_, dim) = self.infer.resolve(&elem)
                    {
                        let resolved = self.infer.resolve_dim(&dim);
                        if !resolved.is_one() && resolved.is_concrete() {
                            let formatted = self.dims.format(&resolved);
                            self.error_help(
                                "E0410",
                                format!(
                                    "`{name}` of a {formatted} tensor needs a runtime dimension"
                                ),
                                arg.span,
                                "divide by a unit first, or use `det_unit`",
                            );
                            return Type::Error;
                        }
                    }
                }
                self.infer.restore(&saved);
                self.diagnostics.items.truncate(before);
                // The probe was speculative: its substitutions were thrown
                // away, so the types it recorded would name variables nobody
                // can resolve. Drop them with the substitutions.
                self.truncate_expr_types(types_before);
            }
            // §8.1.9: building a set orders its elements, so the element
            // type needs a total order, exactly as `sort` does.
            if name == "set_of"
                && let Some(arg) = args.first()
            {
                let ty = self.synth(arg);
                let elem = match self.infer.resolve(&ty) {
                    | Type::Tensor { elem, .. } => Some(*elem),
                    | _ => None,
                };
                if let Some(elem) = elem {
                    self.require_instance("TotalOrd", &elem, span);
                    return Type::Adt(adt::set_id(), vec![self.infer.resolve(&elem)]);
                }
                return Type::Adt(adt::set_id(), vec![self.infer.fresh_type()]);
            }
            // §3.8.4: `sort` needs a total order.
            // The ordering family shares that rule: `sort_axis` and `argsort`
            // order the elements, `sort_by_key` the keys the function gives.
            // At rank >= 2 the sequence is the first-axis rows, and a row is
            // ordered lexicographically by its elements.
            let ordered_arity = match name.as_str() {
                | "sort" | "argsort" => Some(1),
                | "sort_axis" => Some(2),
                | _ => None,
            };
            if let Some(arity) = ordered_arity
                && args.len() == arity
            {
                let ty = self.synth(&args[0]);
                if let Type::Tensor { elem, .. } = self.infer.resolve(&ty) {
                    self.require_instance("TotalOrd", &elem, span);
                }
                if let Some(axis) = args.get(1) {
                    let axis_ty = self.synth(axis);
                    if let Err(error) = self.infer.unify(&axis_ty, &Type::int()) {
                        self.report_unify(error, axis.span);
                    }
                }
                return if name.as_str() == "argsort" {
                    Type::tensor(Shape::Rank(1), Type::int())
                } else {
                    ty
                };
            }
            if name == "sort_by_key" && args.len() == 2 {
                let ty = self.synth(&args[1]);
                if let Type::Tensor { shape, elem } = self.infer.resolve(&ty) {
                    let rank = shape.rank();
                    let item = if rank <= 1 {
                        *elem
                    } else {
                        Type::tensor(Shape::Rank(rank - 1), *elem)
                    };
                    let key = self.infer.fresh_type();
                    self.check(&args[0], &Type::func(vec![item], key.clone()));
                    self.require_instance("TotalOrd", &key, span);
                    return ty;
                }
            }
            if let Some(candidates) = builtins::schemes(name) {
                // A call no overload can take by arity may be to a user
                // function that an import binds under the built-in's name
                // (`first(p, q)`); it then goes the ordinary way.
                let arity_fits = candidates
                    .iter()
                    .any(|c| matches!(&c.ty, Type::Fn(params, _) if params.len() == args.len()));
                if candidates.len() > 1 && (arity_fits || !self.globals.contains_key(name)) {
                    return self.overloaded_call(name, &candidates, args, span);
                }
                // A single-scheme builtin whose name is also a prelude unit (`cg` is
                // both the conjugate-gradient solver and the centigram): in call
                // position the function wins, as it does for `min`.
                if self.units.lookup(name).is_some()
                    && matches!(
                        self.globals.get(name).map(|scheme| &scheme.ty),
                        Some(Type::UnitOf(_)) | None
                    )
                {
                    let ty = self.infer.instantiate(&candidates[0]);
                    return self.call_with_type(ty, args, span);
                }
            }
        }
        let callee_ty = self.synth(callee);
        match self.infer.resolve(&callee_ty) {
            | Type::TensorIntrinsic(intrinsic) => {
                return self.tensor_constructor(intrinsic, &[], args, span);
            },
            | Type::FromValueIntrinsic(specialization) => {
                return self.from_value_call(specialization.map(|ty| *ty), args, span);
            },
            | Type::ReadNpyIntrinsic(rank) => return self.read_npy_call(rank, args, span),
            | Type::WriteNpyIntrinsic => return self.write_npy_call(args, span),
            | Type::ToCallbackIntrinsic => return self.to_callback_call(args, span),
            | Type::TypeRepIntrinsic(Some(_)) => return self.type_rep_call(args, span),
            | Type::TypeRepIntrinsic(None) => {
                self.error(
                    "E0315",
                    "`type_rep` requires an explicit type argument",
                    span,
                );
                return Type::Error;
            },
            | _ => {},
        }
        let ty = self.call_with_type(callee_ty, args, span);
        // §8.1/§8.4/§8.7: `mean`/`var`/`std`/`cov`/`corr`/`inv`/`linsolve`/
        // `lstsq` divide, so the spec's result representation is `div_rep(ρ)`
        // (`Int → Real`). A scheme cannot express that conditional, so the
        // refinement is applied at the call site with the argument's rep known.
        match &callee.kind {
            | ExprKind::Var(name) => self.refine_div_rep_result(name, ty),
            | _ => ty,
        }
    }

    /// Applies the `div_rep` result refinement of the division-shaped
    /// builtins (§3.4.2, §8.1/§8.4/§8.7).
    fn refine_div_rep_result(
        &mut self,
        name: &str,
        ty: Type,
    ) -> Type {
        const DIVIDED: [&str; 8] = [
            "mean", "var", "std", "cov", "corr", "inv", "linsolve", "lstsq",
        ];
        if !DIVIDED.contains(&name) {
            return ty;
        }
        let resolved = self.infer.resolve(&ty);
        self.div_rep_in_type(&resolved)
    }

    /// Rewrites every scalar leaf of a result to its `div_rep`.
    fn div_rep_in_type(
        &mut self,
        ty: &Type,
    ) -> Type {
        match ty {
            | Type::Scalar(rep, dim) => Type::Scalar(self.div_rep(*rep), dim.clone()),
            | Type::Tensor { shape, elem } => {
                Type::Tensor {
                    shape: shape.clone(),
                    elem: Box::new(self.div_rep_in_type(elem)),
                }
            },
            | Type::Adt(id, args)
                if (*id == adt::RESULT_ID || *id == adt::OPTION_ID) && !args.is_empty() =>
            {
                let mut values = args.clone();
                values[0] = self.div_rep_in_type(&args[0]);
                Type::Adt(*id, values)
            },
            | other => other.clone(),
        }
    }

    fn call_with_type(
        &mut self,
        callee_ty: Type,
        args: &[Expr],
        span: Span,
    ) -> Type {
        let callee_ty = self.infer.resolve(&callee_ty);
        match callee_ty {
            | Type::ReadNpyIntrinsic(rank) => self.read_npy_call(rank, args, span),
            | Type::WriteNpyIntrinsic => self.write_npy_call(args, span),
            | Type::Fn(params, ret) => {
                // `() -> τ` and `f()`: a function taking unit can be written
                // with the unit argument or without it, so `random()`,
                // `time_now()` and `read_line()` type-check (`()` is one
                // value; §2.3.6's call syntax has no separate zero-arg form).
                if params.len() == 1
                    && args.is_empty()
                    && matches!(self.infer.resolve(&params[0]), Type::Unit)
                {
                    return *ret;
                }
                if params.len() != args.len() {
                    self.error(
                        "E0315",
                        format!(
                            "this function takes {} argument(s) but {} were given",
                            params.len(),
                            args.len()
                        ),
                        span,
                    );
                    for arg in args {
                        self.synth(arg);
                    }
                    return *ret;
                }
                for (arg, param) in args.iter().zip(params.iter()) {
                    self.check(arg, param);
                    self.weaken_data_argument(param);
                }
                *ret
            },
            | Type::Var(_) => {
                let params: Vec<Type> = args.iter().map(|arg| self.synth(arg)).collect();
                for param in &params {
                    self.weaken_data_argument(param);
                }
                let ret = self.infer.fresh_type();
                let function = Type::func(params, ret.clone());
                // §3.12 E0303: `x(x)` gives the callee a type containing
                // itself. Discarding this error used to make an occurs-check
                // hole silent.
                if let Err(error) = self.infer.unify(&callee_ty, &function) {
                    self.report_unify(error, span);
                    return Type::Error;
                }
                ret
            },
            | other => {
                // A callee that already failed to check was reported where it
                // failed; "<error> is not a function" only repeats that.
                if !matches!(other, Type::Error) {
                    self.error("E0305", format!("{other} is not a function"), span);
                }
                for arg in args {
                    self.synth(arg);
                }
                Type::Error
            },
        }
    }

    fn partial_application(
        &mut self,
        callee: &Expr,
        args: &[Expr],
        span: Span,
    ) -> Type {
        let mut params = Vec::new();
        let mut call_args = Vec::new();
        let mut scope = HashMap::new();
        let mut param_types = Vec::new();
        for (index, arg) in args.iter().enumerate() {
            if matches!(arg.kind, ExprKind::Placeholder) {
                let name = aevia_syntax::ast::placeholder_param_name(index);
                let ty = self.infer.fresh_type();
                scope.insert(name.clone(), Scheme::mono(ty.clone()));
                param_types.push(ty);
                params.push(Param {
                    pattern: Pat::new(PatKind::Var(name.clone()), arg.span),
                    ty: None,
                    span: arg.span,
                });
                call_args.push(Expr::new(ExprKind::Var(name), arg.span));
            } else {
                call_args.push(arg.clone());
            }
        }
        self.scopes.push(scope);
        let result_type = self.infer.fresh_type();
        let saved_return = self.return_type.replace(result_type.clone());
        self.function_stack.push(span);
        let body_ty = self.synth_call(callee, &call_args, span);
        if let Err(error) = self.infer.unify(&result_type, &body_ty) {
            self.report_unify(error, span);
        }
        self.function_stack.pop();
        self.return_type = saved_return;
        self.scopes.pop();
        let _ = params;
        let ty = Type::func(param_types, body_ty);
        self.checked.functions.insert(
            span,
            FunctionEvidence {
                site: span,
                identity: self
                    .callable_ids
                    .get(&span)
                    .expect("seeded callable identity")
                    .clone(),
                numeric_literals: Vec::new(),
                scheme: self.infer.generalize(&ty),
                binders: Vec::new(),
                reifications: Vec::new(),
            },
        );
        ty
    }

    /// Resolve and record a concrete type argument for runtime reification.
    /// A generic argument cannot be reified without a runtime type dictionary.
    fn reify_type_argument(
        &mut self,
        argument: &Ty,
    ) -> Option<Type> {
        let ty = self.convert_type(argument);
        let resolved = self.infer.resolve(&ty);
        let generic = crate::class::has_variables(&resolved);
        if matches!(resolved, Type::Error) || (generic && !self.declared_generic_type(&resolved)) {
            self.error(
                "E0304",
                "cannot reify an unresolved type; supply a concrete type argument",
                argument.span,
            );
            return None;
        }
        if generic && let Some(site) = self.function_stack.last() {
            self.function_reifications
                .entry(*site)
                .or_default()
                .push(resolved.clone());
        }
        self.note_type(argument.span, &resolved);
        Some(resolved)
    }

    fn declared_generic_type(
        &self,
        ty: &Type,
    ) -> bool {
        if self.function_stack.is_empty() {
            return false;
        }
        let mut allowed = crate::TypeSubstitution::default();
        let mut infer = Infer::new();
        for (_, ty) in &self.type_params {
            allowed.freshen(&self.infer.resolve(ty), &mut infer);
        }
        for (_, dim) in &self.dim_params {
            allowed.freshen(&Type::UnitOf(self.infer.resolve_dim(dim)), &mut infer);
        }
        for (_, nat) in &self.nat_params {
            allowed.freshen(
                &Type::tensor(
                    Shape::Refined(vec![Some(self.infer.resolve_nat(nat))]),
                    Type::Unit,
                ),
                &mut infer,
            );
        }
        for (_, row) in &self.row_params {
            allowed.freshen(&Type::Record(self.infer.resolve_row(row)), &mut infer);
        }
        let mut used = crate::TypeSubstitution::default();
        used.freshen(ty, &mut infer);
        used.types.keys().all(|id| allowed.types.contains_key(id))
            && used
                .representations
                .keys()
                .all(|id| allowed.representations.contains_key(id))
            && used
                .dimensions
                .keys()
                .all(|id| allowed.dimensions.contains_key(id))
            && used.rows.keys().all(|id| allowed.rows.contains_key(id))
            && used
                .naturals
                .keys()
                .all(|id| allowed.naturals.contains_key(id))
    }

    #[allow(clippy::wrong_self_convention)] // lowers a call; not a conversion of `self`
    fn to_callback_call(
        &mut self,
        args: &[Expr],
        span: Span,
    ) -> Type {
        if args.len() != 1 {
            self.error(
                "E0315",
                format!("`to_callback` takes one function, found {}", args.len()),
                span,
            );
            for arg in args {
                self.synth(arg);
            }
            return Type::Error;
        }
        let argument = self.synth(&args[0]);
        let function = self.infer.resolve(&argument);
        if !matches!(function, Type::Fn(..)) {
            self.error("E0302", "`to_callback` requires a function", args[0].span);
            return Type::Error;
        }
        opaque_type("Callback")
    }

    fn write_npy_call(
        &mut self,
        args: &[Expr],
        span: Span,
    ) -> Type {
        if args.len() != 2 {
            self.error(
                "E0315",
                format!("`write_npy` takes two arguments, found {}", args.len()),
                span,
            );
            for arg in args {
                self.synth(arg);
            }
            return Type::Error;
        }
        self.check(&args[0], &Type::Str);
        let value = self.synth(&args[1]);
        let Type::Tensor { elem, .. } = self.infer.resolve(&value) else {
            self.error(
                "E0302",
                format!("`write_npy` needs a Float tensor, found {value}"),
                args[1].span,
            );
            return Type::Error;
        };
        if let Err(error) = self.infer.unify(&elem, &Type::float(Dim::one())) {
            self.report_unify(error, args[1].span);
            return Type::Error;
        }
        Type::Io(Box::new(Type::result(
            Type::Unit,
            builtin_type("DataError").unwrap_or(Type::Error),
        )))
    }

    fn read_npy_call(
        &mut self,
        rank: Option<u32>,
        args: &[Expr],
        span: Span,
    ) -> Type {
        if args.len() != 1 {
            self.error(
                "E0315",
                format!("`read_npy` takes one path argument, found {}", args.len()),
                span,
            );
            for arg in args {
                self.synth(arg);
            }
            return Type::Error;
        }
        self.check(&args[0], &Type::Str);
        let Some(rank) = rank else {
            self.error_help(
                "E0314",
                "`read_npy` requires an explicit rank",
                span,
                "write `read_npy<2>(path)`",
            );
            return Type::Error;
        };
        Type::Io(Box::new(Type::result(
            Type::tensor(Shape::Rank(rank), Type::float(Dim::one())),
            builtin_type("DataError").unwrap_or(Type::Error),
        )))
    }

    #[allow(clippy::wrong_self_convention)] // checks a call; not a constructor
    fn from_value_call(
        &mut self,
        specialization: Option<Type>,
        args: &[Expr],
        span: Span,
    ) -> Type {
        if args.len() != 1 {
            self.error(
                "E0315",
                format!(
                    "`from_value` takes one value argument, found {}",
                    args.len()
                ),
                span,
            );
            for arg in args {
                self.synth(arg);
            }
            return Type::Error;
        }
        self.check(&args[0], &dynamic_value_type());
        let ty = specialization.unwrap_or_else(|| self.infer.fresh_type());
        Type::result(ty, crate::builtins::op_error_type())
    }

    fn type_rep_call(
        &mut self,
        args: &[Expr],
        span: Span,
    ) -> Type {
        if !args.is_empty() {
            self.error(
                "E0315",
                format!("`type_rep` takes no value arguments, found {}", args.len()),
                span,
            );
            for arg in args {
                self.synth(arg);
            }
            return Type::Error;
        }
        opaque_type("TypeRep")
    }

    fn tensor_constructor(
        &mut self,
        intrinsic: crate::ty::TensorIntrinsicType,
        type_args: &[Ty],
        args: &[Expr],
        span: Span,
    ) -> Type {
        let name = intrinsic.kind.name();
        if type_args.len() > 1
            || (name == "fill" && !type_args.is_empty())
            || (intrinsic.element.is_some() && !type_args.is_empty())
        {
            self.error(
                "E0315",
                format!(
                    "`{name}` accepts {} type argument(s)",
                    if name == "fill" {
                        "no"
                    } else {
                        "at most one"
                    }
                ),
                span,
            );
            return Type::Error;
        }
        if name == "fill" && args.is_empty() {
            self.error("E0315", "`fill` needs a value", span);
            return Type::Error;
        }
        let elem = if name == "fill" {
            let value = self.synth(&args[0]);
            let scalar = Type::Scalar(self.infer.fresh_rep(), self.infer.fresh_dim());
            if let Err(error) = self.infer.unify(&value, &scalar) {
                self.report_unify(error, args[0].span);
            }
            value
        } else if let Some(rep) = intrinsic.element {
            Type::Scalar(rep, Dim::one())
        } else if let Some(ty) = type_args.first() {
            let Some(rep) = self.tensor_element_rep(name, ty) else {
                return Type::Error;
            };
            Type::Scalar(rep, Dim::one())
        } else {
            Type::float(Dim::one())
        };
        let dimensions = if name == "fill" {
            &args[1..]
        } else {
            args
        };
        for arg in dimensions {
            self.check(arg, &Type::int());
        }
        Type::tensor(
            Shape::Rank(u32::try_from(dimensions.len()).unwrap_or(0)),
            elem,
        )
    }

    fn tensor_element_rep(
        &mut self,
        name: &str,
        ty: &Ty,
    ) -> Option<crate::ty::Rep> {
        let elem = self.convert_type(ty);
        match self.infer.resolve(&elem) {
            | Type::Scalar(
                rep @ (crate::ty::Rep::Int | crate::ty::Rep::Real | crate::ty::Rep::Float),
                dim,
            ) if dim.is_one() => Some(rep),
            | _ => {
                self.error(
                    "E0302",
                    format!("`{name}` requires Int, Real or dimensionless Float elements"),
                    ty.span,
                );
                None
            },
        }
    }

    /// §3.5.1: the shape-naming constructors, whose target is a type-level
    /// shape argument rather than an element type.
    ///
    /// `reshape<[3, 2]>(a)` returns `Result<Tensor<[3, 2], τ>, ShapeError>`
    /// (always a `Result`), and a statically-known element count is checked
    /// here: reshaping 6 elements into `[4, 2]` is `E0802` before the program
    /// runs. `zeros_of<[3, 4]>()`/`ones_of<[3, 4]>()` build the refined
    /// tensor directly (§13.9.1).
    fn shape_constructor(
        &mut self,
        name: &str,
        type_args: &[Ty],
        args: &[Expr],
        span: Span,
    ) -> Type {
        let target = type_args
            .first()
            .and_then(|ty| self.shape_type_argument(ty));
        // `refine_shape<[3, 2]>(a) : Option<Tensor<[3, 2], τ>>` (§3.5.1): the
        // run-time check that turns a rank-only (or differently refined) tensor
        // into a refined one. The branch that matches `Some(b)` knows `b`'s
        // extents statically; the `None` branch is the "any other shape" case.
        if name == "refine_shape" {
            if args.len() != 1 {
                self.error(
                    "E0315",
                    format!("`refine_shape` takes 1 argument, found {}", args.len()),
                    span,
                );
                for arg in args {
                    self.synth(arg);
                }
                return Type::Error;
            }
            let arg_ty = self.synth(&args[0]);
            let (source_shape, elem) = match self.infer.resolve(&arg_ty) {
                | Type::Tensor { shape, elem } => (shape, (*elem).clone()),
                | other => {
                    self.error_help(
                        "E0302",
                        format!("`refine_shape` expects a tensor, found {other}"),
                        args[0].span,
                        "refine a `Tensor<n, τ>`",
                    );
                    return Type::Error;
                },
            };
            let Some(target) = target else {
                self.error_help(
                    "E0315",
                    "`refine_shape` needs its target shape as a type argument",
                    span,
                    "write `refine_shape<[3, 2]>(a)`",
                );
                return Type::Error;
            };
            if usize::try_from(source_shape.rank()).unwrap_or(usize::MAX) != target.len() {
                self.error_help(
                    "E0302",
                    format!(
                        "cannot refine a rank-{} tensor to a shape of rank {}",
                        source_shape.rank(),
                        target.len()
                    ),
                    span,
                    "`refine_shape` checks extents, never the rank",
                );
                return Type::Error;
            }
            // The check runs at run time, where a type-level variable has no
            // value: only literal (or `?`) extents can be tested.
            for extent in target.iter().flatten() {
                if self.infer.resolve_nat(extent).as_const().is_none() {
                    self.error_help(
                        "E0304",
                        format!(
                            "`refine_shape` tests extents at run time, but `{}` has no run-time value",
                            self.infer.render_nat(extent)
                        ),
                        span,
                        "name literal extents, for example `refine_shape<[3, ?]>(a)`",
                    );
                    return Type::Error;
                }
            }
            let target = self.infer.resolve_shape(&Shape::Refined(target));
            return Type::option(Type::tensor(target, elem));
        }
        if name == "reshape" {
            if args.len() != 1 {
                self.error(
                    "E0315",
                    format!("`reshape` takes 1 argument, found {}", args.len()),
                    span,
                );
                for arg in args {
                    self.synth(arg);
                }
                return Type::Error;
            }
            let arg_ty = self.synth(&args[0]);
            let (source_shape, elem) = match self.infer.resolve(&arg_ty) {
                | Type::Tensor { shape, elem } => (shape, (*elem).clone()),
                | other => {
                    self.error_help(
                        "E0302",
                        format!("`reshape` expects a tensor, found {other}"),
                        args[0].span,
                        "reshape a `Tensor<n, τ>`",
                    );
                    return Type::Error;
                },
            };
            let Some(target) = target else {
                // No shape argument: the runtime reports that it needs one.
                return Type::result(arg_ty, builtins::shape_error_type());
            };
            // §3.5.1: the element count is preserved. When both shapes are
            // known (or only differ by Nat variables) the count is a
            // type-level equation, so `reshape<[2 * n]>(a)` with `a:
            // Tensor<[n], Float>` solves `n = 2n` ⇒ `n = 0`; a determined
            // mismatch is `E0802`. A product of two variable extents is not
            // linear and is `E0304`.
            let source_items = match &source_shape {
                | Shape::Refined(items) => Some(items.clone()),
                | Shape::Rank(_) => None,
            };
            if let Some(source_items) = source_items
                && let (Some(source_product), Some(target_product)) = (
                    self.shape_product(&source_items),
                    self.shape_product(&target),
                )
                && let Err(error) = self.infer.unify_nat(&source_product, &target_product)
            {
                match error {
                    | crate::infer::UnifyError::NatMismatch { .. } => {
                        let source_count = self.infer.resolve_nat(&source_product);
                        let target_count = self.infer.resolve_nat(&target_product);
                        self.error_help(
                            "E0802",
                            format!("cannot reshape {source_count} element(s) into {target_count}"),
                            span,
                            "the element count must be preserved",
                        );
                    },
                    | other => self.report_unify(other, span),
                }
                return Type::Error;
            }
            let target = self.infer.resolve_shape(&Shape::Refined(target));
            return Type::result(Type::tensor(target, elem), builtins::shape_error_type());
        }
        // `zeros_of<[3, 4]>()` / `ones_of<[3, 4]>()`.
        if !args.is_empty() {
            self.error(
                "E0315",
                format!("`{name}` takes no arguments, found {}", args.len()),
                span,
            );
            for arg in args {
                self.synth(arg);
            }
        }
        Type::tensor(
            target.map_or(Shape::Rank(1), |items| {
                self.infer.resolve_shape(&Shape::Refined(items))
            }),
            Type::float(Dim::one()),
        )
    }

    /// The product of a shape's known extents, as a Nat term (§3.5.1).
    ///
    /// `None` when an extent is `?`: the count is not statically known.
    fn shape_product(
        &self,
        items: &[Option<Nat>],
    ) -> Option<Nat> {
        let mut product = Nat::Lit(1);
        for item in items {
            let extent = self.infer.resolve_nat(item.as_ref()?);
            product = match (product.as_const(), extent.as_const()) {
                | (Some(left), Some(right)) => Nat::Lit(left.saturating_mul(right)),
                | _ => Nat::Mul(Box::new(product), Box::new(extent)),
            };
        }
        Some(product)
    }

    /// The concrete extents a shape type argument names (§A.5 `shape`).
    ///
    /// `reshape<[3, 2]>` and the spelled-out `reshape<Tensor<[3, 2], Float>>`
    /// (the spelling the runtime tests use) both reach here; `?` is `None`.
    fn shape_type_argument(
        &mut self,
        ty: &Ty,
    ) -> Option<Vec<Option<Nat>>> {
        match &ty.kind {
            | TyKind::ShapeArg(shape) | TyKind::Tensor { shape, .. } => {
                match self.convert_shape(shape) {
                    | Shape::Refined(items) => Some(items),
                    | Shape::Rank(_) => None,
                }
            },
            | _ => {
                self.error_help(
                    "E0302",
                    "the type argument of `reshape` must be a shape",
                    ty.span,
                    "write `reshape<[3, 2]>(a)`",
                );
                None
            },
        }
    }

    /// Types an ODE entry point through the `RateOf` type function (§8.9).
    ///
    /// `odeint : ((Float[T], s) -> RateOf<s>) × s × Tensor<1, Float[T]> ->
    /// Tensor<1, s>`; `odeint_with` adds `OdeOpts<s>` and answers with
    /// `{ t: Tensor<1, Float[T]>, y: Tensor<1, s> }`, and `solve_ivp` takes the
    /// interval `(Float[T], Float[T])` instead of a time series.
    ///
    /// `RateOf` recurses through tensors and records and divides a scalar's
    /// dimension by `T`, so the right-hand side is *not* typed as returning the
    /// state type: a state `Float[L]` returns `Float[L/T]`.
    fn ode_call(
        &mut self,
        name: &str,
        args: &[Expr],
        span: Span,
    ) -> Type {
        let expected = if name == "odeint" { 3 } else { 4 };
        if args.len() != expected {
            self.error(
                "E0315",
                format!("`{name}` takes {expected} arguments, found {}", args.len()),
                span,
            );
            for arg in args {
                self.synth(arg);
            }
            return Type::Error;
        }
        let time = Dim::base("Time");
        // The right-hand side: `(Float[T], s) -> RateOf<s>`.
        let rhs_ty = self.synth(&args[0]);
        let state = match self.infer.resolve(&rhs_ty) {
            | Type::Fn(params, ret) if params.len() == 2 => {
                if let Err(error) = self.infer.unify(&params[0], &Type::float(time.clone())) {
                    self.report_unify(error, args[0].span);
                }
                let state = self.infer.resolve(&params[1]);
                let rate = self.rate_of(&state, span);
                if !rate.is_error()
                    && let Err(error) = self.infer.unify(&ret, &rate)
                {
                    self.report_unify(error, args[0].span);
                }
                state
            },
            | other => {
                self.error_help(
                    "E0302",
                    format!(
                        "the right-hand side of `{name}` must be a function `(Float[T], s) -> RateOf<s>`, found {other}"
                    ),
                    args[0].span,
                    "write `fn rhs(_t: Float[T], s: State): Rate = …`",
                );
                for arg in &args[1..] {
                    self.synth(arg);
                }
                return Type::Error;
            },
        };
        self.check(&args[1], &state);
        let time_series = Type::tensor(Shape::Rank(1), Type::float(time.clone()));
        match name {
            | "odeint" | "odeint_with" => self.check(&args[2], &time_series),
            // `solve_ivp` takes an interval, `(Float[T], Float[T])`.
            | _ => {
                let interval = Type::record(vec![
                    ("0".to_string(), Type::float(time.clone())),
                    ("1".to_string(), Type::float(time.clone())),
                ]);
                self.check(&args[2], &interval);
            },
        }
        if name != "odeint" {
            // `OdeOpts<s>`: the options record is checked field-wise by its
            // literal; its `events` field mentions `s`, so a closed record type
            // would need the whole shape. It is synthesised and left to the
            // record literal's own checking.
            self.synth(&args[3]);
        }
        match name {
            | "odeint" => Type::tensor(Shape::Rank(1), state),
            | _ => {
                Type::record(vec![
                    ("t".to_string(), time_series),
                    ("y".to_string(), Type::tensor(Shape::Rank(1), state)),
                ])
            },
        }
    }

    /// The solution record of `ode_solve` / `ode_bvp` (§8.9).
    fn ode_solution_type(
        state: &Type,
        time: &Dim,
    ) -> Type {
        let hit = Type::record(vec![
            ("index".to_string(), Type::int()),
            ("t".to_string(), Type::float(time.clone())),
            ("y".to_string(), state.clone()),
        ]);
        Type::record(vec![
            (
                "t".to_string(),
                Type::tensor(Shape::Rank(1), Type::float(time.clone())),
            ),
            ("y".to_string(), Type::tensor(Shape::Rank(1), state.clone())),
            (
                "at".to_string(),
                Type::func(vec![Type::float(time.clone())], state.clone()),
            ),
            ("events".to_string(), Type::tensor(Shape::Rank(1), hit)),
            ("nfev".to_string(), Type::int()),
            ("rejected".to_string(), Type::int()),
            ("method".to_string(), Type::string()),
        ])
    }

    /// The event record of `ode_solve` and `sde_events`:
    /// `{ g: (Float[T], s) -> Float[1], terminal: Bool, direction: Int }`.
    fn event_record_type(
        state: &Type,
        time: &Dim,
    ) -> Type {
        Type::record(vec![
            (
                "g".to_string(),
                Type::func(
                    vec![Type::float(time.clone()), state.clone()],
                    Type::float(Dim::one()),
                ),
            ),
            ("terminal".to_string(), Type::bool()),
            ("direction".to_string(), Type::int()),
        ])
    }

    /// §8.9: `ode_solve`, `ode_bvp`, `odeint_order`, `odeint_order_with`.
    ///
    /// The right-hand side of an order-`n` equation takes `n + 1` arguments
    /// `(t, y, y', …, y⁽ⁿ⁻¹⁾)` and answers `y⁽ⁿ⁾`; the type of the `k`-th
    /// derivative is `RateOf` applied `k` times to the state type, so the
    /// dimension of every derivative is tracked (`y : Float[L]` makes `y'`
    /// `Float[L/T]` and `y''` `Float[L/T^2]`).
    fn ode_ext_call(
        &mut self,
        name: &str,
        args: &[Expr],
        span: Span,
    ) -> Type {
        let expected = match name {
            | "ode_solve" | "ode_bvp" => 5,
            | "odeint_order" => 3,
            | _ => 4,
        };
        if args.len() != expected {
            self.error(
                "E0315",
                format!("`{name}` takes {expected} arguments, found {}", args.len()),
                span,
            );
            for arg in args {
                self.synth(arg);
            }
            return Type::Error;
        }
        let time = Dim::base("Time");
        let higher = name.starts_with("odeint_order");
        let rhs_ty = self.synth(&args[0]);
        let (state, order) = match self.infer.resolve(&rhs_ty) {
            | Type::Fn(params, ret) if params.len() >= 2 && (higher || params.len() == 2) => {
                if let Err(error) = self.infer.unify(&params[0], &Type::float(time.clone())) {
                    self.report_unify(error, args[0].span);
                }
                let state = self.infer.resolve(&params[1]);
                let order = params.len() - 1;
                let mut derivative = state.clone();
                for parameter in &params[2..] {
                    derivative = self.rate_of(&derivative, span);
                    if derivative.is_error() {
                        break;
                    }
                    if let Err(error) = self.infer.unify(parameter, &derivative) {
                        self.report_unify(error, args[0].span);
                    }
                }
                if !derivative.is_error() {
                    let top = self.rate_of(&derivative, span);
                    if !top.is_error()
                        && let Err(error) = self.infer.unify(&ret, &top)
                    {
                        self.report_unify(error, args[0].span);
                    }
                }
                (state, order)
            },
            | other => {
                let shape = if higher {
                    "(Float[T], y, y', …, y⁽ⁿ⁻¹⁾) -> y⁽ⁿ⁾"
                } else {
                    "(Float[T], s) -> RateOf<s>"
                };
                self.error_help(
                    "E0302",
                    format!(
                        "the right-hand side of `{name}` must be a function `{shape}`, found {other}"
                    ),
                    args[0].span,
                    "write `fn rhs(_t: Float[T], s: State): Rate = …`",
                );
                for arg in &args[1..] {
                    self.synth(arg);
                }
                return Type::Error;
            },
        };
        let time_series = Type::tensor(Shape::Rank(1), Type::float(time.clone()));
        let interval = Type::record(vec![
            ("0".to_string(), Type::float(time.clone())),
            ("1".to_string(), Type::float(time.clone())),
        ]);
        match name {
            | "ode_solve" => {
                self.check(&args[1], &state);
                self.check(&args[2], &interval);
                self.synth(&args[3]);
                let events = Type::tensor(Shape::Rank(1), Self::event_record_type(&state, &time));
                self.check(&args[4], &events);
                Self::ode_solution_type(&state, &time)
            },
            | "ode_bvp" => {
                self.check(&args[1], &state);
                self.check(&args[2], &interval);
                // `bc(y(t0), y(t1))` answers one residual per state component;
                // the residuals may carry any one dimension.
                let residual_dim = self.infer.fresh_dim();
                let boundary = Type::func(
                    vec![state.clone(), state.clone()],
                    Type::tensor(Shape::Rank(1), Type::float(residual_dim)),
                );
                self.check(&args[3], &boundary);
                self.synth(&args[4]);
                Self::ode_solution_type(&state, &time)
            },
            | _ => {
                // The initial value of every derivative, as a tuple.
                let mut fields = Vec::with_capacity(order);
                let mut derivative = state.clone();
                for index in 0..order {
                    if index > 0 {
                        derivative = self.rate_of(&derivative, span);
                    }
                    fields.push((index.to_string(), derivative.clone()));
                }
                let initial = Type::record(fields.clone());
                self.check(&args[1], &initial);
                self.check(&args[2], &time_series);
                if args.len() == 4 {
                    self.synth(&args[3]);
                }
                Type::record(vec![
                    ("t".to_string(), time_series),
                    ("y".to_string(), Type::tensor(Shape::Rank(1), state)),
                    ("d".to_string(), Type::tensor(Shape::Rank(1), initial)),
                ])
            },
        }
    }

    /// §8.9.1: the SDE entry points.
    ///
    /// `sdeint_diag : (((Float[T], s) -> RateOf<s>), ((Float[T], s) ->
    /// DiffusionOf<s>), s, SdePolicy) -> Result<SdePath<s>, SdeError>`.
    /// The drift reuses [`Self::rate_of`]; the diffusion uses
    /// [`Self::diffusion_of`]. `sdeint_general` integrates scalar or rank-1
    /// states and its diffusion returns a rank-2 tensor whose elements are
    /// `DiffusionOf<element>` — the noise column count is a runtime check
    /// because it depends on the value, not the type.
    fn sde_call(
        &mut self,
        full_name: &str,
        args: &[Expr],
        span: Span,
    ) -> Type {
        // `sde_ensemble_diag(drift, diffusion, y0, opts, n)` is `sdeint_diag`
        // run `n` times with member seeds: same typing, one more argument,
        // and a tensor of results.
        let ensemble = full_name.starts_with("sde_ensemble_");
        let name = if ensemble {
            if full_name.ends_with("general") {
                "sdeint_general"
            } else {
                "sdeint_diag"
            }
        } else {
            full_name
        };
        if ensemble && args.len() != 5 {
            self.error(
                "E0315",
                format!("`{full_name}` takes 5 arguments, found {}", args.len()),
                span,
            );
            for arg in args {
                self.synth(arg);
            }
            return Type::Error;
        }
        let args = if ensemble {
            self.check(&args[4], &Type::int());
            &args[..4]
        } else {
            args
        };
        let result = self.sde_call_single(name, args, span);
        if ensemble && !result.is_error() {
            Type::tensor(Shape::Rank(1), result)
        } else {
            result
        }
    }

    fn sde_call_single(
        &mut self,
        name: &str,
        args: &[Expr],
        span: Span,
    ) -> Type {
        if args.len() != 4 {
            self.error(
                "E0315",
                format!("`{name}` takes 4 arguments, found {}", args.len()),
                span,
            );
            for arg in args {
                self.synth(arg);
            }
            return Type::Error;
        }
        let time = Dim::base("Time");
        // 1. Drift: `(Float[T], s) -> RateOf<s>`; the state `s` comes from it.
        let drift_ty = self.synth(&args[0]);
        let state = match self.infer.resolve(&drift_ty) {
            | Type::Fn(params, ret) if params.len() == 2 => {
                if let Err(error) = self.infer.unify(&params[0], &Type::float(time.clone())) {
                    self.report_unify(error, args[0].span);
                }
                let state = self.infer.resolve(&params[1]);
                let rate = self.rate_of(&state, span);
                if !rate.is_error()
                    && let Err(error) = self.infer.unify(&ret, &rate)
                {
                    self.report_unify(error, args[0].span);
                }
                state
            },
            | other => {
                self.error_help(
                    "E0302",
                    format!(
                        "the drift of `{name}` must be a function `(Float[T], s) -> RateOf<s>`, found {other}"
                    ),
                    args[0].span,
                    "write `fn drift(_t: Float[T], s: State): RateOf<State> = …`",
                );
                for arg in &args[1..] {
                    self.synth(arg);
                }
                return Type::Error;
            },
        };
        // 2. Diffusion. Diagonal noise: `(Float[T], s) -> DiffusionOf<s>`.
        //    General noise: a rank-2 tensor of `DiffusionOf<element>`.
        let expected_diffusion = if name == "sdeint_diag" {
            self.diffusion_of(&state, span)
        } else {
            let mut record_diffusion = None;
            let elem = match self.infer.resolve(&state) {
                | Type::Scalar(_, _) => state.clone(),
                | Type::Tensor { shape, elem } if shape.rank() == 1 => (*elem).clone(),
                // A record state: the diffusion is a record of the same shape whose
                // fields carry a trailing noise axis (`GeneralDiffusionOf<s>`).
                | Type::Record(_) => {
                    record_diffusion = Some(self.general_diffusion_of(&state, span));
                    state.clone()
                },
                | other => {
                    self.error_help(
                        "E0302",
                        format!(
                            "`sdeint_general` needs a scalar, rank-1 or record state, found {other}"
                        ),
                        span,
                        "pass a `Float`, a `Tensor<1, Float>` or a closed record of them as the state",
                    );
                    for arg in &args[1..] {
                        self.synth(arg);
                    }
                    return Type::Error;
                },
            };
            if let Some(record) = record_diffusion {
                record
            } else if elem.is_error() {
                Type::Error
            } else {
                Type::tensor(Shape::Rank(2), self.diffusion_of(&elem, span))
            }
        };
        let diffusion_ty = self.synth(&args[1]);
        match self.infer.resolve(&diffusion_ty) {
            | Type::Fn(params, ret) if params.len() == 2 => {
                if let Err(error) = self.infer.unify(&params[0], &Type::float(time.clone())) {
                    self.report_unify(error, args[1].span);
                }
                if let Err(error) = self.infer.unify(&params[1], &state) {
                    self.report_unify(error, args[1].span);
                }
                if !expected_diffusion.is_error()
                    && let Err(error) = self.infer.unify(&ret, &expected_diffusion)
                {
                    self.report_unify(error, args[1].span);
                }
            },
            | other => {
                self.error_help(
                    "E0302",
                    format!(
                        "the diffusion of `{name}` must be a function `(Float[T], s) -> DiffusionOf<s>`, found {other}"
                    ),
                    args[1].span,
                    "write `fn noise(_t: Float[T], s: State): DiffusionOf<State> = …`",
                );
            },
        }
        // 3. Initial state and 4. policy record (the record literal checks its
        // own field types; the policy schema is closed in the standard library).
        self.check(&args[2], &state);
        self.synth(&args[3]);
        let sde_error = builtin_type("SdeError").unwrap_or(Type::Error);
        let method = builtin_type("SdeMethod").unwrap_or(Type::Error);
        let path = Type::record(vec![
            (
                "t".to_string(),
                Type::tensor(Shape::Rank(1), Type::float(time)),
            ),
            ("y".to_string(), Type::tensor(Shape::Rank(1), state)),
            ("steps".to_string(), Type::int()),
            ("rejected".to_string(), Type::int()),
            ("nfev".to_string(), Type::int()),
            ("seed".to_string(), Type::int()),
            ("noise".to_string(), Type::int()),
            ("method".to_string(), method),
        ]);
        Type::result(path, sde_error)
    }

    /// §6.10: `par_map(f, xs)` — the result order is the input order, and the
    /// element type must have a `Parallel` instance (§3.8.4: only `Int`,
    /// `Float` and tensors of them; symbolic `Real` is not parallel).
    fn par_map_call(
        &mut self,
        args: &[Expr],
        span: Span,
    ) -> Type {
        if args.len() != 2 {
            self.error(
                "E0315",
                format!("`par_map` takes 2 arguments, found {}", args.len()),
                span,
            );
            for arg in args {
                self.synth(arg);
            }
            return Type::Error;
        }
        let f_ty = self.synth(&args[0]);
        let xs_ty = self.synth(&args[1]);
        let element = match self.infer.resolve(&xs_ty) {
            | Type::Tensor { elem, .. } => (*elem).clone(),
            | other => {
                self.error_help(
                    "E0302",
                    format!("`par_map` expects a tensor, found {other}"),
                    args[1].span,
                    "build a tensor first, or use `map` on other containers",
                );
                return Type::Error;
            },
        };
        self.require_instance("Parallel", &element, span);
        let result = self.infer.fresh_type();
        if let Err(error) = self
            .infer
            .unify(&f_ty, &Type::func(vec![element], result.clone()))
        {
            self.report_unify(error, args[0].span);
            return Type::Error;
        }
        Type::tensor(Shape::Rank(1), result)
    }

    /// §3.11/§4.9.2: `dyn : Scalar<ρ,d> -> Dyn<ρ>`, where `d` must be a
    /// concrete dimension (§4.9.4).
    ///
    /// "Concrete" means it contains no free dimension variable. A literal's
    /// dimension is the dimensionless `1` (a literal cannot be dimensioned
    /// unless it carries a unit), so `dyn(3)` is fine; a `<d>`-polymorphic
    /// parameter is not, and the spec's answer is `dyn_with(x, dim)`.
    fn dyn_call(
        &mut self,
        args: &[Expr],
        span: Span,
    ) -> Type {
        if args.len() != 1 {
            self.error(
                "E0315",
                format!("`dyn` takes 1 argument, found {}", args.len()),
                span,
            );
            for arg in args {
                self.synth(arg);
            }
            return Type::Error;
        }
        let arg_ty = self.synth(&args[0]);
        let scalar = self.force_scalar(&arg_ty);
        let Type::Scalar(rep, dim) = self.infer.resolve(&scalar) else {
            let resolved = self.infer.resolve(&scalar);
            self.error_help(
                "E0302",
                format!("`dyn` expects a scalar, found {resolved}"),
                args[0].span,
                "wrap a `Scalar<ρ,d>`; a tensor's elements are extracted with `map`",
            );
            return Type::Error;
        };
        let dim = self.infer.resolve_dim(&dim);
        if !dim.is_concrete() {
            self.error_help(
                "E0304",
                "the dimension of `dyn`'s argument must be concrete",
                args[0].span,
                "generic code passes it at run time: `dyn_with(x, dim)`",
            );
            return Type::Error;
        }
        self.checked.dyn_calls.push(DynCall {
            span,
            primitive: DynPrimitive::Dyn,
            dim,
        });
        Type::Dyn(self.infer.resolve_rep(rep))
    }

    /// §3.11/§4.9.2: `check : Dyn<ρ> -> Result<Scalar<ρ,d>, DimError>`, with
    /// `d` concrete. `check<d>(x)` names it explicitly (§4.9.4).
    fn check_call(
        &mut self,
        args: &[Expr],
        type_arg: Option<&Ty>,
        span: Span,
    ) -> Type {
        if args.len() != 1 {
            self.error(
                "E0315",
                format!("`check` takes 1 argument, found {}", args.len()),
                span,
            );
            for arg in args {
                self.synth(arg);
            }
            return Type::Error;
        }
        let arg_ty = self.synth(&args[0]);
        let rep = match self.infer.resolve(&arg_ty) {
            | Type::Dyn(rep) => self.infer.resolve_rep(rep),
            | other => {
                self.error_help(
                    "E0302",
                    format!("`check` expects a `Dyn<ρ>`, found {other}"),
                    args[0].span,
                    "`dyn(x)` wraps a scalar; `check` is the only way back (§4.9.4)",
                );
                return Type::Error;
            },
        };
        let dim = match type_arg {
            | Some(ty) => {
                let Some(dim) = self.dimension_type_argument(ty) else {
                    return Type::Error;
                };
                self.infer.resolve_dim(&dim)
            },
            // The expected type of the enclosing expression fixes `d` — but it
            // may arrive after this call is synthesised, so it is resolved at
            // the end of the module (`pending_checks`).
            | None => self.infer.fresh_dim(),
        };
        self.pending_checks.push((span, dim.clone()));
        Type::result(Type::Scalar(rep, dim), builtins::dim_error_type())
    }

    /// §5.11.5: `to_system("natural", x)` and `from_system<D>("natural", x)`.
    ///
    /// `to_system` takes an SI quantity of a concrete dimension `d` to the
    /// system's representative `R(d)`; `from_system<D>` takes a value of
    /// `R(D)` back to the SI dimension `D`, which must be written because the
    /// reduction forgets it (`L` and `T` are both `Energy^-1`). `D` is read in
    /// SI even inside a module with a unit system. The runtime needs the
    /// system and the SI dimension, recorded in `system_calls`.
    fn system_call(
        &mut self,
        to: bool,
        args: &[Expr],
        type_arg: Option<&Ty>,
        span: Span,
    ) -> Type {
        let name = if to {
            "to_system"
        } else {
            "from_system"
        };
        if args.len() != 2 {
            self.error(
                "E0315",
                format!(
                    "`{name}` takes 2 arguments (a system name and a value), found {}",
                    args.len()
                ),
                span,
            );
            for arg in args {
                self.synth(arg);
            }
            return Type::Error;
        }
        self.check(&args[0], &Type::string());
        let Some(system_name) = string_constant(&args[0]) else {
            self.error_help(
                "E0302",
                format!("`{name}` needs the unit system as a string literal"),
                args[0].span,
                "write the name, for example `\"natural\"`",
            );
            self.synth(&args[1]);
            return Type::Error;
        };
        let Some(system) = aevia_rssn::registry::registry().unit_system(&system_name) else {
            self.error(
                "E0512",
                format!("unknown unit system `{system_name}`"),
                args[0].span,
            );
            self.synth(&args[1]);
            return Type::Error;
        };
        let value = self.synth(&args[1]);
        let value = self.force_scalar(&value);
        let Type::Scalar(rep, dim) = self.infer.resolve(&value) else {
            if !matches!(self.infer.resolve(&value), Type::Error) {
                self.error(
                    "E0302",
                    format!("`{name}` converts a scalar quantity"),
                    args[1].span,
                );
            }
            return Type::Error;
        };
        let rep = self.infer.resolve_rep(rep).div_rep();
        let si = if to {
            let dim = self.infer.resolve_dim(&dim);
            if !dim.is_concrete() {
                self.error_help(
                    "E0304",
                    format!(
                        "`to_system` needs the argument's dimension to be known here, found {}",
                        self.dims.format(&dim)
                    ),
                    args[1].span,
                    "annotate the value's dimension",
                );
                return Type::Error;
            }
            dim
        } else {
            let Some(ty) = type_arg else {
                self.error_help(
                    "E0304",
                    "`from_system` needs the SI dimension it converts to",
                    span,
                    "write it as a type argument, for example `from_system<L>(\"natural\", x)`",
                );
                return Type::Error;
            };
            let saved = std::mem::replace(&mut self.si_dimensions, true);
            let target = self.dimension_type_argument(ty);
            self.si_dimensions = saved;
            let Some(target) = target else {
                return Type::Error;
            };
            let target = self.infer.resolve_dim(&target);
            let reduced = system.reduce_dim(&target);
            if let Err(error) = self
                .infer
                .unify(&Type::Scalar(rep, dim.clone()), &Type::Scalar(rep, reduced))
            {
                self.report_unify(error, args[1].span);
            }
            target
        };
        self.checked
            .system_calls
            .insert(span, (system_name, si.clone()));
        let result = if to {
            system.reduce_dim(&si)
        } else {
            si
        };
        Type::Scalar(rep, result)
    }

    /// The dimension a `check<d>` type argument names (§4.9.4).
    ///
    /// `d` is written in the type language: `L`/`Length` is a named dimension,
    /// `d` is a quantified dimension variable of the enclosing declaration.
    fn dimension_type_argument(
        &mut self,
        ty: &Ty,
    ) -> Option<Dim> {
        match &ty.kind {
            | TyKind::Var(name) => {
                if let Some((_, dim)) = self
                    .dim_params
                    .iter()
                    .rev()
                    .find(|(param, _)| param == name)
                {
                    return Some(dim.clone());
                }
                let si_dimensions = self.si_dimensions;
                let reduce = |dim: Dim| {
                    if si_dimensions {
                        dim
                    } else {
                        self.units.reduce_dim(&dim)
                    }
                };
                self.dims.lookup(name).map(reduce).or_else(|| {
                    self.error(
                        "E0407",
                        format!("unknown dimension `{name}` in `check<{name}>`"),
                        ty.span,
                    );
                    None
                })
            },
            | TyKind::Named { path, args } if args.is_empty() => {
                let name = path.join(".");
                let si_dimensions = self.si_dimensions;
                let reduce = |dim: Dim| {
                    if si_dimensions {
                        dim
                    } else {
                        self.units.reduce_dim(&dim)
                    }
                };
                self.dims.lookup(&name).map(reduce).or_else(|| {
                    self.error(
                        "E0407",
                        format!("unknown dimension `{name}` in `check<{name}>`"),
                        ty.span,
                    );
                    None
                })
            },
            | _ => {
                self.error_help(
                    "E0407",
                    "the type argument of `check` must be a dimension",
                    ty.span,
                    "write `check<L>(x)` or `check<Length>(x)`",
                );
                None
            },
        }
    }

    fn overloaded_call(
        &mut self,
        name: &str,
        candidates: &[Scheme],
        args: &[Expr],
        span: Span,
    ) -> Type {
        self.overloaded_call_over(name, candidates, args, span, true)
    }

    /// The ranks of the tensor-typed, non-lambda arguments of a call, found by
    /// synthesising them speculatively (everything is rolled back).
    fn probe_argument_ranks(
        &mut self,
        args: &[Expr],
    ) -> Vec<u32> {
        let saved = self.infer.clone();
        let diagnostics_before = self.diagnostics.items.len();
        let types_before = self.expr_types.len();
        let mut ranks = Vec::new();
        for arg in args {
            if matches!(arg.kind, ExprKind::Lambda { .. }) {
                continue;
            }
            let ty = self.synth(arg);
            if let Type::Tensor { shape, .. } = self.infer.resolve(&ty) {
                let rank = shape.rank();
                if !ranks.contains(&rank) {
                    ranks.push(rank);
                }
            }
        }
        self.infer.restore(&saved);
        self.diagnostics.items.truncate(diagnostics_before);
        self.truncate_expr_types(types_before);
        ranks
    }

    /// Overload resolution over `candidates`. `probe` lets a failed call to a
    /// rank-generic built-in retry with the signature template instantiated
    /// at the (above [`builtins::STATIC_RANK`]) rank of its arguments.
    fn overloaded_call_over(
        &mut self,
        name: &str,
        candidates: &[Scheme],
        args: &[Expr],
        span: Span,
        probe: bool,
    ) -> Type {
        let saved = self.infer.clone();
        let diagnostics_before = self.diagnostics.items.len();
        let types_before = self.expr_types.len();
        let mut first_error: Option<UnifyError> = None;
        let mut speculative_failure: Option<&Scheme> = None;
        for candidate in candidates {
            self.infer.restore(&saved);
            self.diagnostics.items.truncate(diagnostics_before);
            // Each candidate is a guess that was rolled back; its recorded
            // types were rolled back with it.
            self.truncate_expr_types(types_before);
            let callee = self.infer.instantiate(candidate);
            let Type::Fn(params, ret) = self.infer.resolve(&callee) else {
                continue;
            };
            if params.len() != args.len() {
                continue;
            }
            let mut ok = true;
            // A lambda is checked *against* the parameter type, after the
            // other arguments have fixed it: synthesising it first leaves its
            // parameter unknown, and an overloaded operation in the body
            // (`sum(row)`) would then commit to its first overload and make a
            // rank-3 `filter(fn (slab) => sum(slab) > 0, cube)` fail.
            let is_lambda = |arg: &Expr| matches!(arg.kind, ExprKind::Lambda { .. });
            for (arg, param) in args.iter().zip(params.iter()) {
                if is_lambda(arg) {
                    continue;
                }
                let arg_ty = self.synth(arg);
                if let Err(error) = self.infer.unify(&arg_ty, param) {
                    first_error.get_or_insert(error);
                    ok = false;
                    break;
                }
            }
            if ok {
                for (arg, param) in args.iter().zip(params.iter()) {
                    if !is_lambda(arg) {
                        continue;
                    }
                    let before = self.diagnostics.items.len();
                    self.check(arg, param);
                    if self.diagnostics.items[before..]
                        .iter()
                        .any(|item| item.severity == aevia_syntax::Severity::Error)
                    {
                        speculative_failure.get_or_insert(candidate);
                        ok = false;
                        break;
                    }
                }
            }
            if ok {
                // The division-shaped built-ins (`mean`, `var`, ...) promote
                // `Int` to `Real` at the call site; an overloaded call must
                // get the same refinement as a single-scheme one.
                return self.refine_div_rep_result(name, *ret);
            }
        }
        // The rank-generic built-ins are templates over the rank; the table
        // enumerates the first few, and a tensor of a higher rank gets the
        // instantiation at its own rank.
        if probe {
            self.infer.restore(&saved);
            self.diagnostics.items.truncate(diagnostics_before);
            self.truncate_expr_types(types_before);
            let ranks = self.probe_argument_ranks(args);
            let extra = builtins::rank_schemes_for(name, &ranks);
            if !extra.is_empty() {
                return self.overloaded_call_over(name, &extra, args, span, false);
            }
        }
        // Every candidate failed. When one failed only inside a lambda body,
        // that candidate is the one the author meant: replay it so its own
        // diagnostics (not "no overload") reach the reader.
        if first_error.is_none()
            && let Some(candidate) = speculative_failure
        {
            self.infer.restore(&saved);
            self.diagnostics.items.truncate(diagnostics_before);
            self.truncate_expr_types(types_before);
            let callee = self.infer.instantiate(candidate);
            if let Type::Fn(params, ret) = self.infer.resolve(&callee) {
                for (arg, param) in args.iter().zip(params.iter()) {
                    if !matches!(arg.kind, ExprKind::Lambda { .. }) {
                        let arg_ty = self.synth(arg);
                        if let Err(error) = self.infer.unify(&arg_ty, param) {
                            self.report_unify(error, span);
                        }
                    }
                }
                for (arg, param) in args.iter().zip(params.iter()) {
                    if matches!(arg.kind, ExprKind::Lambda { .. }) {
                        self.check(arg, param);
                    }
                }
                return *ret;
            }
        }
        self.infer.restore(&saved);
        self.truncate_expr_types(types_before);
        for arg in args {
            self.synth(arg);
        }
        if let Some(error) = first_error {
            self.report_unify(error, span);
        } else {
            self.error(
                "E1003",
                format!("no overload of `{name}` accepts {} argument(s)", args.len()),
                span,
            );
        }
        Type::Error
    }

    fn special_call(
        &mut self,
        name: &str,
        args: &[Expr],
        span: Span,
    ) -> Type {
        match name {
            | "zeros" | "ones" => {
                let rank = u32::try_from(args.len()).unwrap_or(0);
                for arg in args {
                    let ty = self.synth(arg);
                    let _ = self.infer.unify(&ty, &Type::int());
                }
                Type::tensor(Shape::Rank(rank), Type::float(Dim::one()))
            },
            | "fill" => {
                if args.is_empty() {
                    self.error("E0315", "`fill` needs a value", span);
                    return Type::Error;
                }
                let elem = self.synth(&args[0]);
                let rank = u32::try_from(args.len().saturating_sub(1)).unwrap_or(0);
                for arg in &args[1..] {
                    let ty = self.synth(arg);
                    let _ = self.infer.unify(&ty, &Type::int());
                }
                Type::tensor(Shape::Rank(rank), elem)
            },
            | "symbol" => {
                if let Some(arg) = args.first() {
                    let ty = self.synth(arg);
                    let _ = self.infer.unify(&ty, &Type::string());
                }
                // §3.3.3: a symbol keeps one dimension for the whole program,
                // so its dimension variable is weak and never generalised.
                Type::real(self.infer.fresh_weak_dim())
            },
            | "symfun" => {
                if let Some(arg) = args.first() {
                    let ty = self.synth(arg);
                    let _ = self.infer.unify(&ty, &Type::string());
                }
                let domain = self.infer.fresh_weak_dim();
                let codomain = self.infer.fresh_weak_dim();
                Type::func(vec![Type::real(domain)], Type::real(codomain))
            },
            | _ => Type::Error,
        }
    }

    fn synth_type_app(
        &mut self,
        callee: &Expr,
        args: &[Ty],
        span: Span,
    ) -> Type {
        let ty = self.synth(callee);
        self.apply_type_arguments(callee, args, span, ty)
    }

    fn apply_type_arguments(
        &mut self,
        callee: &Expr,
        args: &[Ty],
        span: Span,
        ty: Type,
    ) -> Type {
        if let Some(mut projection) = self.checked.callable_refs.get(&callee.span).cloned() {
            projection.selected = true;
            self.checked.callable_refs.insert(span, projection);
        }
        if let Some(materializations) = self.call_materializations.get(&callee.span).cloned() {
            self.call_materializations.insert(span, materializations);
        }
        if matches!(self.infer.resolve(&ty), Type::Fn(..) | Type::Adt(..))
            && self.checked.generic_refs.contains_key(&callee.span)
        {
            if let Some(headers) = self
                .checked
                .generic_refs
                .get(&callee.span)
                .cloned()
                .filter(|headers| !headers.is_empty())
            {
                if headers.len() != args.len() {
                    self.error(
                        "E0315",
                        format!(
                            "this function takes {} generic argument(s), found {}",
                            headers.len(),
                            args.len()
                        ),
                        span,
                    );
                    return Type::Error;
                }
                for (header, arg) in headers.iter().zip(args) {
                    let argument = match header {
                        | GenericBinder::Type(_) | GenericBinder::Representation(_) => {
                            self.convert_type(arg)
                        },
                        | GenericBinder::Dimension(_) => {
                            Type::UnitOf(self.convert_type_as_dim(arg))
                        },
                        | GenericBinder::Natural(_) => {
                            Type::tensor(
                                Shape::Refined(vec![Some(self.convert_type_as_nat(arg))]),
                                Type::Unit,
                            )
                        },
                        | GenericBinder::Row(_) => Type::Record(self.convert_type_as_row(arg)),
                    };
                    self.note_type(arg.span, &argument);
                    if crate::class::has_variables(&argument)
                        && let Some(site) = self.function_stack.last()
                    {
                        self.function_reifications
                            .entry(*site)
                            .or_default()
                            .push(argument.clone());
                    }
                    if let Err(error) = self.infer.unify(&header.as_type(), &argument) {
                        self.report_unify(error, arg.span);
                    }
                }
                self.checked.generic_refs.insert(span, headers);
            }
            return self.infer.resolve(&ty);
        }
        if matches!(self.infer.resolve(&ty), Type::WriteNpyIntrinsic) {
            self.error("E0315", "`write_npy` does not accept type arguments", span);
            return Type::Error;
        }
        if let Type::ReadNpyIntrinsic(specialization) = self.infer.resolve(&ty) {
            if specialization.is_some() || args.len() != 1 {
                self.error(
                    "E0315",
                    "`read_npy` requires exactly one rank argument",
                    span,
                );
                return Type::Error;
            }
            let nat = self.convert_type_as_nat(&args[0]);
            let Some(rank) = nat.as_const().and_then(|n| u32::try_from(n).ok()) else {
                self.error_help(
                    "E0304",
                    "`read_npy` needs a concrete representable rank",
                    args[0].span,
                    "supply a literal rank such as `read_npy<2>`",
                );
                return Type::Error;
            };
            self.note_type(args[0].span, &Type::tensor(Shape::Rank(rank), Type::Unit));
            return Type::ReadNpyIntrinsic(Some(rank));
        }
        if let Type::FromValueIntrinsic(specialization) = self.infer.resolve(&ty) {
            if specialization.is_some() || args.len() != 1 {
                self.error(
                    "E0315",
                    "`from_value` requires one type argument when specialized",
                    span,
                );
                return Type::Error;
            }
            return self
                .reify_type_argument(&args[0])
                .map_or(Type::Error, |ty| {
                    Type::FromValueIntrinsic(Some(Box::new(ty)))
                });
        }
        if let Type::TypeRepIntrinsic(specialization) = self.infer.resolve(&ty) {
            if specialization.is_some() || args.len() != 1 {
                self.error(
                    "E0315",
                    "`type_rep` requires exactly one type argument",
                    span,
                );
                return Type::Error;
            }
            return self
                .reify_type_argument(&args[0])
                .map_or(Type::Error, |ty| Type::TypeRepIntrinsic(Some(Box::new(ty))));
        }
        let Type::TensorIntrinsic(mut intrinsic) = self.infer.resolve(&ty) else {
            return ty;
        };
        if args.len() != 1
            || intrinsic.kind == crate::ty::TensorIntrinsic::Fill
            || intrinsic.element.is_some()
        {
            self.error(
                "E0315",
                "this tensor constructor cannot take those type arguments",
                span,
            );
            return Type::Error;
        }
        let Some(rep) = self.tensor_element_rep(intrinsic.kind.name(), &args[0]) else {
            return Type::Error;
        };
        intrinsic.element = Some(rep);
        Type::TensorIntrinsic(intrinsic)
    }

    fn synth_record(
        &mut self,
        base: Option<&Expr>,
        fields: &[(String, Expr)],
    ) -> Type {
        // §3.6.1/§2: two fields with the same name in one literal is `E0208`,
        // not a silent last-write-wins.
        let mut seen: HashSet<&str> = HashSet::new();
        for (name, _) in fields {
            if !seen.insert(name.as_str()) {
                self.error_help(
                    "E0208",
                    format!("duplicate field `{name}`"),
                    Span::new(self.file, 0, 0),
                    "delete one of the two fields",
                );
            }
        }
        let mut row = match base {
            | Some(base) => {
                let base_ty = self.synth(base);
                match self.infer.resolve(&base_ty) {
                    // The base row — including its open tail — is the result
                    // row, so `{ p with z = … }` keeps `p`'s unknown fields.
                    | Type::Record(row) => row,
                    | other => {
                        self.error(
                            "E0302",
                            format!("record update expects a record, found {other}"),
                            base.span,
                        );
                        Row::Empty
                    },
                }
            },
            | None => Row::Empty,
        };
        for (name, value) in fields {
            let ty = self.synth(value);
            if base.is_some() {
                // §3.6.2: adding a field requires `row(p) lacks z`.
                if row.field(name).is_none()
                    && let Some(tail) = row.tail_var()
                    && let Err(error) = self.infer.require_lacks(tail, name)
                {
                    self.report_unify(error, value.span);
                }
                row = row.update(name, ty);
            } else {
                row = Row::Extend {
                    label: name.clone(),
                    ty: Box::new(ty),
                    tail: Box::new(row),
                };
            }
        }
        Type::Record(if base.is_some() {
            row
        } else {
            Row::closed(row.fields())
        })
    }

    fn field_type(
        &mut self,
        base: &Type,
        name: &str,
        span: Span,
    ) -> Type {
        let base = self.infer.resolve(base);
        if matches!(base, Type::Var(_)) {
            // An unannotated parameter used as `p.x` is an open record with at
            // least that field (§3.6.1).
            let field = self.infer.fresh_type();
            let tail = self.infer.fresh_row();
            let record = Type::Record(Row::Extend {
                label: name.to_string(),
                ty: Box::new(field.clone()),
                tail: Box::new(tail),
            });
            let _ = self.infer.unify(&base, &record);
            return field;
        }
        match self.infer.resolve(&base) {
            | Type::Record(row) => {
                match row.field(name) {
                    | Some(ty) => ty.clone(),
                    | None => {
                        match row.tail_var() {
                            | Some(tail) => {
                                // The row is still open: absorb the field (§3.6.1).
                                let field = self.infer.fresh_type();
                                let _ = self.infer.extend_row(tail, name, field.clone());
                                field
                            },
                            | None => {
                                let fields = row.fields();
                                let close = closest_name(
                                    name,
                                    fields.iter().map(|(field, _)| field.as_str()),
                                )
                                .map(str::to_string);
                                let message =
                                    format!("the record {{ {row} }} has no field `{name}`");
                                match close {
                                    | Some(close) => {
                                        self.error_help(
                                            "E0306",
                                            message,
                                            span,
                                            &format!("did you mean `{close}`?"),
                                        )
                                    },
                                    | None if !fields.is_empty() => {
                                        let names: Vec<&str> = fields
                                            .iter()
                                            .map(|(field, _)| field.as_str())
                                            .collect();
                                        self.error_help(
                                            "E0306",
                                            message,
                                            span,
                                            &format!("the fields are {}", names.join(", ")),
                                        )
                                    },
                                    | None => self.error("E0306", message, span),
                                }
                                Type::Error
                            },
                        }
                    },
                }
            },
            | other => {
                self.error(
                    "E0302",
                    format!("field access expects a record, found {other}"),
                    span,
                );
                Type::Error
            },
        }
    }

    fn synth_tensor(
        &mut self,
        items: &[Expr],
        span: Span,
    ) -> Type {
        if items.is_empty() {
            self.error_help(
                "E0314",
                "the element type of an empty tensor cannot be inferred",
                span,
                "annotate it, for example `let e: Tensor<1, Float> = []`",
            );
            return Type::Error;
        }
        let mut elem: Option<Type> = None;
        for item in items {
            let ty = self.synth(item);
            match &elem {
                | None => elem = Some(ty),
                | Some(existing) => {
                    let existing_resolved = self.infer.resolve(existing);
                    let this_resolved = self.infer.resolve(&ty);
                    if let Err(error) = self.infer.unify(existing, &ty) {
                        // §3.5.2: same-level sub-sequences must have equal
                        // length, and that has its own code (E0203) instead of
                        // a generic mismatch — `[[1.0, 2.0], [3.0]]` is ragged.
                        if shapes_are_ragged(&existing_resolved, &this_resolved) {
                            self.error(
                                "E0203",
                                "ragged tensor literal: every row must have the same length",
                                item.span,
                            );
                        } else {
                            self.report_unify(error, item.span);
                        }
                    }
                },
            }
        }
        let elem = elem.unwrap_or(Type::Error);
        let resolved = self.infer.resolve(&elem);
        match resolved {
            | Type::Tensor {
                shape: inner,
                elem: inner_elem,
            } => {
                let mut shape = vec![Some(Nat::lit(items.len()))];
                shape.extend(shape_items(&inner));
                Type::Tensor {
                    shape: Shape::Refined(shape),
                    elem: inner_elem,
                }
            },
            | other => {
                Type::Tensor {
                    shape: Shape::Refined(vec![Some(Nat::lit(items.len()))]),
                    elem: Box::new(other),
                }
            },
        }
    }

    fn synth_index(
        &mut self,
        base: &Expr,
        indices: &[SliceAst],
        span: Span,
    ) -> Type {
        let base_ty = self.synth(base);
        let mut resolved = self.infer.resolve(&base_ty);
        if matches!(resolved, Type::Var(_)) {
            let elem = self.infer.fresh_type();
            let tensor = Type::tensor(Shape::Rank(1), elem);
            let _ = self.infer.unify(&resolved, &tensor);
            resolved = self.infer.resolve(&tensor);
        }
        // §8.1.4: a string is indexed by grapheme cluster. `s[i]` is the
        // cluster at `i` and `s[i..j]` (or `s[i:j]`) a substring; both are
        // `String`, and a string has exactly one axis.
        if matches!(resolved, Type::Str) {
            if indices.len() != 1 {
                self.error(
                    "E0302",
                    format!("a string has one axis, found {} indices", indices.len()),
                    span,
                );
                return Type::Error;
            }
            let parts: Vec<&Expr> = match &indices[0] {
                | SliceAst::Index(expr) => vec![expr],
                | SliceAst::Range { start, end, step, .. } => {
                    [start, end, step].into_iter().flatten().collect()
                },
            };
            for part in parts {
                let ty = self.synth(part);
                if let Err(error) = self.infer.unify(&ty, &Type::int()) {
                    self.report_unify(error, part.span);
                }
            }
            return Type::Str;
        }
        if let Some((format, dense)) = builtins::sparse_parts(&resolved) {
            let (format, dense) = (format.clone(), dense.clone());
            return self.sparse_index(&format, &dense, indices, span);
        }
        let Type::Tensor { shape, elem } = resolved else {
            self.error(
                "E0302",
                format!("indexing expects a tensor or a string, found {resolved}"),
                span,
            );
            return Type::Error;
        };
        let mut sliced = false;
        let mut consumed: Vec<usize> = Vec::new();
        // Sliced axes keep their rank but get a new extent when the range and
        // step are literals and the source extent is known; otherwise the
        // extent becomes unknown (`?`) rather than keeping a stale value.
        let mut slice_extents: Vec<(usize, Option<Nat>)> = Vec::new();
        for (position, index) in indices.iter().enumerate() {
            match index {
                | SliceAst::Index(expr) => {
                    // §3.5.4: the index is an `Int`. A `String` index used to
                    // be silently accepted because the error was discarded.
                    let ty = self.synth(expr);
                    if let Err(error) = self.infer.unify(&ty, &Type::int()) {
                        self.report_unify(error, expr.span);
                    }
                    // A literal index into a refined shape is checked here;
                    // a dynamic one is checked at run time (§3.5.4).
                    if let Some(value) = literal_index(expr)
                        && let Shape::Refined(items) = &shape
                        && let Some(Some(extent)) = items.get(position)
                    {
                        // §3.5.1: an extent that is still a Nat
                        // variable cannot be bounds-checked here; the
                        // runtime check covers it.
                        let extent_nat = self.infer.resolve_nat(extent);
                        if let Some(extent) = extent_nat.as_const() {
                            let extent = i64::try_from(extent).unwrap_or(i64::MAX);
                            if value < -extent || value >= extent {
                                self.error_help(
                                            "E0802",
                                            format!(
                                                "index {value} is out of bounds for a dimension of extent {extent}"
                                            ),
                                            expr.span,
                                            "a literal index is checked at compile time",
                                        );
                            }
                        }
                    }
                    consumed.push(position);
                },
                | SliceAst::Range { start, end, step, .. } => {
                    sliced = true;
                    for part in [start, end].into_iter().flatten() {
                        let ty = self.synth(part);
                        if let Err(error) = self.infer.unify(&ty, &Type::int()) {
                            self.report_unify(error, part.span);
                        }
                    }
                    // §6.6.4: the step is an `Int`, not a `Float` and not a
                    // quantity — a step is a number of positions, so it has no
                    // dimension and no fractional part. A known zero is
                    // rejected here; a computed one is `E0802` at run time.
                    if let Some(step) = step {
                        let ty = self.synth(step);
                        if self.infer.unify(&ty, &Type::int()).is_err() {
                            self.error(
                                "E0302",
                                "a slice step must be an Int".to_string(),
                                step.span,
                            );
                        } else if literal_index(step) == Some(0) {
                            self.error_help(
                                "E0802",
                                "a slice step of zero never advances".to_string(),
                                step.span,
                                "omit the step to walk by one",
                            );
                        }
                    }
                    // §3.5.3: a literal range gives a statically known extent
                    // when the source extent is known; anything else (negative
                    // indices, non-unit steps, dynamic bounds) leaves it `?`.
                    let source_extent = match &shape {
                        | Shape::Refined(items) => items.get(position).cloned().flatten(),
                        | Shape::Rank(_) => None,
                    };
                    let extent = self.literal_slice_extent(
                        source_extent.as_ref(),
                        start.as_ref(),
                        end.as_ref(),
                        step.as_ref(),
                    );
                    slice_extents.push((position, extent));
                },
            }
        }
        let rank = shape.rank();
        let removed = u32::try_from(consumed.len()).unwrap_or(0);
        let result_rank = if sliced {
            rank.saturating_sub(removed).max(1)
        } else {
            rank.saturating_sub(removed)
        };
        if result_rank == 0 {
            return *elem;
        }
        // The refinement survives indexing: `[3]` indexed by `0` is a scalar,
        // and `[3, 4]` indexed by `i` is `[4]`.
        let result_shape = match &shape {
            | Shape::Refined(items) => {
                let mut kept = items.clone();
                for position in consumed.iter().rev() {
                    if *position < kept.len() {
                        kept.remove(*position);
                    }
                }
                // Sliced axes keep their rank; the extent is recomputed when
                // literal, and becomes `?` when it cannot be known.
                for (position, extent) in &slice_extents {
                    if *position < kept.len() {
                        kept[*position] = extent.clone();
                    }
                }
                Shape::Refined(kept)
            },
            | Shape::Rank(_) => Shape::Rank(result_rank),
        };
        Type::Tensor {
            shape: result_shape,
            elem,
        }
    }

    /// The extent a literal slice produces, when it is statically known
    /// (§3.5.3). Deliberately conservative: only a non-negative, ascending,
    /// unit-step range over a known extent is computed; everything else is `?`.
    fn literal_slice_extent(
        &self,
        source_extent: Option<&Nat>,
        start: Option<&Expr>,
        end: Option<&Expr>,
        step: Option<&Expr>,
    ) -> Option<Nat> {
        let extent = self
            .infer
            .resolve_nat(source_extent?)
            .as_const()
            .and_then(|value| i64::try_from(value).ok())?;
        let step = match step {
            | None => 1,
            | Some(step) => literal_index(step)?,
        };
        if step != 1 {
            return None;
        }
        let start = match start {
            | None => 0,
            | Some(start) => literal_index(start)?,
        };
        let end = match end {
            | None => extent,
            | Some(end) => literal_index(end)?,
        };
        if start < 0 || end < 0 || end > extent || end < start {
            return None;
        }
        Some(Nat::Lit(usize::try_from(end - start).unwrap_or(0)))
    }

    fn synth_try(
        &mut self,
        inner: &Expr,
        span: Span,
    ) -> Type {
        let inner_ty = self.synth(inner);
        let inner_ty = self.infer.resolve(&inner_ty);
        let result_arity = match &inner_ty {
            | Type::Adt(id, args) if *id == adt::RESULT_ID && args.len() == 2 => Some(2),
            | Type::Adt(id, args) if *id == adt::OPTION_ID && !args.is_empty() => Some(1),
            | _ => None,
        };
        let Some(arity) = result_arity else {
            self.error("E0702", "`?` requires a `Result` or `Option` value", span);
            return Type::Error;
        };
        let destination = self
            .do_returns
            .last()
            .filter(|(depth, _, _)| *depth == self.function_stack.len())
            .map(|(_, site, ty)| (TryBoundary::Do(*site), ty.clone()))
            .or_else(|| {
                self.function_stack
                    .last()
                    .zip(self.return_type.as_ref())
                    .map(|(site, ty)| (TryBoundary::Function(*site), ty.clone()))
            });
        let Some((boundary, target)) = destination else {
            self.error(
                "E0702",
                "`?` requires an enclosing function or IO do boundary",
                span,
            );
            return Type::Error;
        };
        let Type::Adt(container, input_args) = &inner_ty else {
            unreachable!()
        };
        let success = self.infer.fresh_type();
        let output_args = if arity == 2 {
            vec![success, input_args[1].clone()]
        } else {
            vec![success]
        };
        let output = Type::Adt(*container, output_args);
        // An inferred return is constrained immediately. Error conversion is
        // never fabricated: concrete error types must agree as well.
        if let Err(error) = self.infer.unify(&target, &output) {
            self.error_help(
                "E0702",
                format!("`?` does not match its enclosing return container: {error}"),
                span,
                "return the same Option/Result family and error type, or handle it with `match`",
            );
            return Type::Error;
        }
        self.checked.try_targets.insert(
            span,
            TryTarget {
                boundary,
                result: target,
            },
        );
        match inner_ty {
            | Type::Adt(_, args) => args[0].clone(),
            | other => other,
        }
    }

    fn synth_do(
        &mut self,
        stmts: &[DoStmt],
        span: Span,
    ) -> Type {
        let destination = self.infer.fresh_type();
        self.do_returns
            .push((self.function_stack.len(), span, destination.clone()));
        self.scopes.push(HashMap::new());
        let mut last_ty = Type::Unit;
        for (position, stmt) in stmts.iter().enumerate() {
            let is_last = position + 1 == stmts.len();
            match stmt {
                | DoStmt::Let {
                    pattern, ty, value, ..
                } => {
                    let value_ty = match ty {
                        | Some(ty) => {
                            let ty = self.convert_type(ty);
                            self.check(value, &ty);
                            ty
                        },
                        | None => self.synth(value),
                    };
                    self.bind_pattern(pattern, &value_ty);
                    last_ty = Type::Unit;
                },
                | DoStmt::Bind { pattern, value, .. } => {
                    let value_ty = self.synth(value);
                    let inner = match self.infer.resolve(&value_ty) {
                        | Type::Io(inner) => *inner,
                        | other => {
                            self.error(
                                "E0701",
                                format!("`<-` expects an IO action, found {other}"),
                                value.span,
                            );
                            Type::Error
                        },
                    };
                    self.bind_pattern(pattern, &inner);
                    last_ty = Type::Unit;
                },
                | DoStmt::Expr(expr) => {
                    let ty = self.synth(expr);
                    match self.infer.resolve(&ty) {
                        | Type::Io(inner) => {
                            // The last statement's result is the block's.
                            if !inner.is_unit() && !is_last {
                                self.warn(
                                    "W0202",
                                    "this IO action's result is discarded",
                                    expr.span,
                                );
                            }
                            last_ty = *inner;
                        },
                        | other => {
                            self.error(
                                "E0701",
                                format!("a `do` statement must be an IO action, found {other}"),
                                expr.span,
                            );
                        },
                    }
                },
            }
        }
        self.scopes.pop();
        self.do_returns.pop();
        if let Err(error) = self.infer.unify(&destination, &last_ty) {
            // A `?` in this block returns from it with a `Result`/`Option`
            // (§6.8.2): when the block's own value is not that container the
            // `?` cannot escape the `IO` block, which is `E0702`.
            let escapes = self
                .checked
                .try_targets
                .values()
                .any(|target| matches!(target.boundary, TryBoundary::Do(site) if site == span));
            if escapes {
                self.error_help(
                    "E0702",
                    format!("`?` does not match its enclosing `IO` block: {error}"),
                    span,
                    "end the block with a value of the same Option/Result family, or handle it with `match`",
                );
            } else {
                self.report_unify(error, span);
            }
        }
        Type::io(last_ty)
    }

    fn quantity_type(
        &mut self,
        value: &NumberLit,
        unit: &UnitExpr,
        span: Span,
    ) -> Type {
        let resolved = match self.units.resolve(unit) {
            | Ok(resolved) => resolved,
            | Err(error) => {
                self.unit_error(error, span);
                return Type::Error;
            },
        };
        if resolved.affine {
            return Type::Point(resolved.dim);
        }
        let kind = if value.is_decimal() {
            LiteralKind::QuantityDecimal
        } else {
            LiteralKind::QuantityInt
        };
        let rep = self
            .literal_context
            .filter(|rep| kind.accepts_rep(*rep))
            .or_else(|| {
                self.literal_region
                    .filter(|(other, _)| *other == kind)
                    .map(|(_, rep)| rep)
            })
            .unwrap_or_else(|| self.infer.fresh_rep());
        self.infer
            .mark_literal(&Type::Scalar(rep, resolved.dim.clone()), kind);
        Type::Scalar(rep, resolved.dim)
    }

    /// A pattern that matches every value of its type.
    fn pattern_catch_all(pattern: &Pat) -> bool {
        match &pattern.kind {
            | PatKind::Wildcard | PatKind::Var(_) | PatKind::Error => true,
            | PatKind::As { pattern, .. } | PatKind::Annot { pattern, .. } => {
                Self::pattern_catch_all(pattern)
            },
            | _ => false,
        }
    }

    /// Strips `as`/annotation wrappers before analysing a pattern.
    fn pattern_inner(pattern: &Pat) -> &Pat {
        match &pattern.kind {
            | PatKind::As { pattern, .. } | PatKind::Annot { pattern, .. } => {
                Self::pattern_inner(pattern)
            },
            | _ => pattern,
        }
    }

    fn wildcard_pattern() -> Pat {
        Pat {
            kind: PatKind::Wildcard,
            span: Span::default(),
        }
    }

    /// The field patterns a constructor pattern leaves for its arguments.
    fn specialise_ctor(
        patterns: &[Pat],
        name: &str,
        arity: usize,
    ) -> Option<Vec<Vec<Pat>>> {
        let mut columns: Vec<Vec<Pat>> = vec![Vec::new(); arity];
        let mut any = false;
        for pattern in patterns {
            if Self::pattern_catch_all(pattern) {
                any = true;
                for column in &mut columns {
                    column.push(Self::wildcard_pattern());
                }
                continue;
            }
            if let PatKind::Ctor { name: written, args } = &Self::pattern_inner(pattern).kind
                && written == name
            {
                any = true;
                for (index, column) in columns.iter_mut().enumerate() {
                    column.push(
                        args.get(index)
                            .cloned()
                            .unwrap_or_else(Self::wildcard_pattern),
                    );
                }
            }
        }
        any.then_some(columns)
    }

    /// The patterns a record/tuple pattern leaves for one field.
    fn specialise_field(
        patterns: &[Pat],
        label: &str,
        index: usize,
    ) -> Vec<Pat> {
        let mut column = Vec::new();
        for pattern in patterns {
            if Self::pattern_catch_all(pattern) {
                column.push(Self::wildcard_pattern());
                continue;
            }
            match &Self::pattern_inner(pattern).kind {
                | PatKind::Record(fields) => {
                    column.push(
                        fields
                            .iter()
                            .find(|(name, _)| name == label)
                            .and_then(|(_, value)| value.clone())
                            .unwrap_or_else(Self::wildcard_pattern),
                    )
                },
                | PatKind::Tuple(items) => {
                    column.push(
                        items
                            .get(index)
                            .cloned()
                            .unwrap_or_else(Self::wildcard_pattern),
                    )
                },
                | _ => {},
            }
        }
        column
    }

    /// Bounded matrix coverage preserves correlations between all fields.
    fn patterns_cover(
        &self,
        patterns: &[Pat],
        ty: &Type,
        depth: u32,
    ) -> bool {
        crate::pattern_matrix::covers(patterns, ty, &self.infer, depth)
    }

    /// Whether an arm can never be reached because earlier arms cover it.
    fn pattern_is_unreachable(
        &self,
        pattern: &Pat,
        previous: &[Pat],
        ty: &Type,
    ) -> bool {
        if previous.iter().any(Self::pattern_catch_all) {
            return true;
        }
        if Self::pattern_catch_all(pattern) {
            return self.patterns_cover(previous, ty, 0);
        }
        if let Type::Adt(id, _) = self.infer.resolve(ty)
            && let PatKind::Ctor { name, .. } = &Self::pattern_inner(pattern).kind
            && let Some(registered) = adt::get(id)
            && let Some(ctor) = registered.ctors.iter().find(|ctor| ctor.name == *name)
            && ctor.fields.is_empty()
        {
            return previous.iter().any(|prior| {
                matches!(
                    &Self::pattern_inner(prior).kind,
                    PatKind::Ctor { name: written, .. } if written == name
                )
            });
        }
        // A duplicate literal is redundant; a new literal never is (the domain
        // is infinite).
        match &Self::pattern_inner(pattern).kind {
            PatKind::Lit(literal) => previous.iter().any(|prior| {
                matches!(&Self::pattern_inner(prior).kind, PatKind::Lit(seen) if seen == literal)
            }),
            _ => false,
        }
    }

    /// A short pattern showing one value the arms miss (§6.6 E0309 message).
    fn missing_pattern(
        &self,
        patterns: &[Pat],
        ty: &Type,
        depth: u32,
    ) -> String {
        if depth > 8 {
            return "_".to_string();
        }
        let resolved = self.infer.resolve(ty);
        match &resolved {
            | Type::Adt(id, args) => {
                let Some(registered) = adt::get(*id) else {
                    return "_".to_string();
                };
                for ctor in &registered.ctors {
                    let Some(columns) =
                        Self::specialise_ctor(patterns, &ctor.name, ctor.fields.len())
                    else {
                        return ctor.name.clone();
                    };
                    for (index, (column, field_ty)) in
                        columns.iter().zip(ctor.fields.iter()).enumerate()
                    {
                        let field_ty = field_ty.substitute_params(args);
                        if !self.patterns_cover(column, &field_ty, depth + 1) {
                            let inner = self.missing_pattern(column, &field_ty, depth + 1);
                            let mut args_out = vec!["_".to_string(); ctor.fields.len()];
                            args_out[index] = inner;
                            return format!("{}({})", ctor.name, args_out.join(", "));
                        }
                    }
                }
                "_".to_string()
            },
            | Type::Bool => {
                let mut yes = false;
                let mut no = false;
                for pattern in patterns {
                    if let PatKind::Lit(PatLit::Bool(value)) = &Self::pattern_inner(pattern).kind {
                        if *value {
                            yes = true;
                        } else {
                            no = true;
                        }
                    }
                }
                match (yes, no) {
                    | (true, false) => "false".to_string(),
                    | (false, true) => "true".to_string(),
                    | _ => "true or false".to_string(),
                }
            },
            | Type::Unit => "()".to_string(),
            | Type::Record(row) => {
                let fields = row.fields();
                for (index, (label, field_ty)) in fields.iter().enumerate() {
                    let column = Self::specialise_field(patterns, label, index);
                    if !self.patterns_cover(&column, field_ty, depth + 1) {
                        let inner = self.missing_pattern(&column, field_ty, depth + 1);
                        return format!("{{ {label} = {inner} }}");
                    }
                }
                "_".to_string()
            },
            | _ => "_".to_string(),
        }
    }

    fn check_exhaustive(
        &mut self,
        subject: &Type,
        unguarded: &[Pat],
        span: Span,
    ) {
        if self.patterns_cover(unguarded, subject, 0) {
            return;
        }
        let missing = self.missing_pattern(unguarded, subject, 0);
        self.error_help(
            "E0309",
            format!("non-exhaustive match: missing {missing}"),
            span,
            "add the missing arm or a `_` arm",
        );
    }

    // ----- patterns ------------------------------------------------------

    fn bind_pattern(
        &mut self,
        pattern: &Pat,
        ty: &Type,
    ) {
        self.validate_pattern_names(std::iter::once(pattern));
        self.bind_pattern_inner(pattern, ty);
    }

    fn validate_pattern_names<'a>(
        &mut self,
        patterns: impl IntoIterator<Item = &'a Pat>,
    ) {
        let mut pending = patterns.into_iter().collect::<Vec<_>>();
        let mut names = HashSet::new();
        while let Some(pattern) = pending.pop() {
            let site = pattern.span;
            let mut bind = |name: &str| {
                if !names.insert(name.to_string()) {
                    self.error(
                        "E0308",
                        format!("pattern binds `{name}` more than once"),
                        pattern.span,
                    );
                }
            };
            match &pattern.kind {
                | PatKind::Var(name) => bind(name),
                | PatKind::Ctor { args, .. } | PatKind::Tuple(args) => pending.extend(args),
                | PatKind::Record(fields) => {
                    let mut labels = HashSet::new();
                    for (name, pattern) in fields {
                        if !labels.insert(name) {
                            self.error(
                                "E0308",
                                format!("record pattern repeats field `{name}`"),
                                pattern.as_ref().map_or(site, |p| p.span),
                            );
                        }
                        if let Some(pattern) = pattern {
                            pending.push(pattern);
                        } else if !names.insert(name.clone()) {
                            self.error(
                                "E0308",
                                format!("pattern binds `{name}` more than once"),
                                site,
                            );
                        }
                    }
                },
                | PatKind::Tensor { items, rest } => {
                    if let Some(name) = rest {
                        bind(name);
                    }
                    pending.extend(items);
                },
                | PatKind::As { pattern, name } => {
                    bind(name);
                    pending.push(pattern);
                },
                | PatKind::Annot { pattern, .. } => pending.push(pattern),
                | _ => {},
            }
        }
    }

    /// §B.12 `W0103`: a local binder that rebinds a name already visible in an
    /// enclosing local scope shadows it. A unit or builtin name is *not*
    /// shadowed: §1.8.2 keeps those namespaces separate, and every `let x`
    /// would warn.
    fn warn_if_shadowing(
        &mut self,
        name: &str,
        span: Span,
    ) {
        let shadows = !name.starts_with('_')
            && self
                .scopes
                .iter()
                .rev()
                .any(|enclosing| enclosing.contains_key(name));
        if shadows {
            self.warn(
                "W0103",
                format!("`{name}` shadows an enclosing binding"),
                span,
            );
        }
    }

    fn bind_pattern_inner(
        &mut self,
        pattern: &Pat,
        ty: &Type,
    ) {
        match &pattern.kind {
            // A parser recovery node: the syntax error was already reported.
            | PatKind::Error => {},
            | PatKind::Wildcard => {},
            | PatKind::Var(name) => {
                self.local_headers
                    .remove(&(self.scopes.len(), name.clone()));
                self.local_callables
                    .remove(&(self.scopes.len(), name.clone()));
                // The binder is a place the editor asks about, and this is the
                // type its uses get. Recorded at the pattern's own span, so it
                // never competes with an expression that happens to share it.
                self.note_type(pattern.span, ty);
                if self.scopes.is_empty() {
                    self.globals.insert(name.clone(), Scheme::mono(ty.clone()));
                    return;
                }
                self.warn_if_shadowing(name, pattern.span);
                // §B.12 `W0101`: remembered until the module ends so an unread
                // local can be reported once.
                if !name.starts_with('_') {
                    self.local_binders.push((name.clone(), pattern.span));
                }
                if let Some(scope) = self.scopes.last_mut() {
                    scope.insert(name.clone(), Scheme::mono(ty.clone()));
                }
            },
            | PatKind::Ctor { name, args } => {
                if matches!(self.infer.resolve(ty), Type::Var(_))
                    && let Some((id, index)) = self.constructor_bindings.get(name).copied()
                {
                    let constructor = self.infer.instantiate(&ctor_scheme(id, index));
                    let result = match constructor {
                        | Type::Fn(_, result) => *result,
                        | value => value,
                    };
                    if let Err(error) = self.infer.unify(ty, &result) {
                        self.report_unify(error, pattern.span);
                    }
                }
                let resolved = self.infer.resolve(ty);
                let (adt_id, adt_args) = match &resolved {
                    | Type::Adt(id, args) => (*id, args.clone()),
                    | _ => {
                        self.error(
                            "E0308",
                            format!("pattern `{name}` does not match {resolved}"),
                            pattern.span,
                        );
                        return;
                    },
                };
                let Some(registered) = adt::get(adt_id) else {
                    return;
                };
                let Some(index) = registered.ctors.iter().position(|ctor| ctor.name == *name)
                else {
                    self.error(
                        "E0307",
                        format!("`{}` has no constructor `{name}`", registered.name),
                        pattern.span,
                    );
                    return;
                };
                if adt::module_of(adt_id) != self.module_path
                    && !self
                        .constructor_bindings
                        .values()
                        .any(|(id, ctor)| adt::same_type(*id, adt_id) && *ctor == index)
                {
                    self.error(
                        "E0307",
                        format!("constructor `{name}` is not visible in this module"),
                        pattern.span,
                    );
                    return;
                }
                self.checked
                    .constructor_refs
                    .insert(pattern.span, (adt_id, index));
                let ctor = &registered.ctors[index];
                if ctor.fields.len() != args.len() {
                    self.error(
                        "E0307",
                        format!(
                            "constructor `{name}` takes {} argument(s), found {}",
                            ctor.fields.len(),
                            args.len()
                        ),
                        pattern.span,
                    );
                    return;
                }
                for (sub, field) in args.iter().zip(ctor.fields.iter()) {
                    let field_ty = instantiate_pattern_field(adt_id, field, &adt_args);
                    self.bind_pattern_inner(sub, &field_ty);
                }
            },
            | PatKind::Record(fields) => {
                let resolved = self.infer.resolve(ty);
                let Type::Record(row) = resolved else {
                    self.error(
                        "E0308",
                        format!("a record pattern does not match {resolved}"),
                        pattern.span,
                    );
                    return;
                };
                for (name, sub) in fields {
                    let Some(field_ty) = row.field(name).cloned() else {
                        self.error(
                            "E0306",
                            format!("the record {{ {row} }} has no field `{name}`"),
                            pattern.span,
                        );
                        continue;
                    };
                    match sub {
                        | Some(sub) => self.bind_pattern_inner(sub, &field_ty),
                        | None => {
                            if !name.starts_with('_') {
                                self.local_binders.push((name.clone(), pattern.span));
                            }
                            if let Some(scope) = self.scopes.last_mut() {
                                scope.insert(name.clone(), Scheme::mono(field_ty));
                            }
                        },
                    }
                }
            },
            | PatKind::Tuple(items) => {
                let resolved = self.infer.resolve(ty);
                let Type::Record(row) = resolved else {
                    self.error(
                        "E0308",
                        format!("a tuple pattern does not match {resolved}"),
                        pattern.span,
                    );
                    return;
                };
                if row.tail_var().is_some()
                    || row.fields().len() != items.len()
                    || (0..items.len()).any(|index| row.field(&index.to_string()).is_none())
                {
                    self.error(
                        "E0308",
                        "tuple pattern arity does not match its value",
                        pattern.span,
                    );
                    return;
                }
                for (index, item) in items.iter().enumerate() {
                    let field_ty = row
                        .field(&index.to_string())
                        .cloned()
                        .unwrap_or(Type::Error);
                    self.bind_pattern_inner(item, &field_ty);
                }
            },
            | PatKind::Tensor { items, rest } => {
                if matches!(self.infer.resolve(ty), Type::Var(_)) {
                    let elem = self.infer.fresh_type();
                    let tensor = Type::tensor(Shape::Rank(1), elem);
                    if let Err(error) = self.infer.unify(ty, &tensor) {
                        self.report_unify(error, pattern.span);
                    }
                }
                let resolved = self.infer.resolve(ty);
                let Type::Tensor { shape, elem } = resolved else {
                    self.error(
                        "E0308",
                        format!("a tensor pattern does not match {resolved}"),
                        pattern.span,
                    );
                    return;
                };
                // §6.5: a tensor pattern matches only rank 1. In a `match` arm
                // a scrutinee of another rank is not an error: the arm simply
                // never matches and evaluation falls through to the next arm,
                // so its binders are still typed (rank-1 slices of the element
                // type). A `let` or parameter pattern must match, so there the
                // mismatch is rejected.
                let matches_rank = shape.rank() == 1;
                if !matches_rank && !self.refutable_pattern {
                    self.error("E0308", "a tensor pattern requires rank one", pattern.span);
                    return;
                }
                for item in items {
                    self.bind_pattern_inner(item, &elem);
                }
                if let Some(name) = rest
                    && let Some(scope) = self.scopes.last_mut()
                {
                    scope.insert(
                        name.clone(),
                        Scheme::mono(Type::tensor(
                            if matches_rank {
                                shape
                                    .extent(0)
                                    .and_then(|n| n.checked_sub(items.len()))
                                    .map_or(Shape::Rank(1), |n| Shape::refined(vec![Some(n)]))
                            } else {
                                Shape::Rank(1)
                            },
                            (*elem).clone(),
                        )),
                    );
                }
            },
            | PatKind::Lit(lit) => {
                let lit_ty = match lit {
                    | PatLit::Int(_) => Type::int(),
                    | PatLit::Str(_) => Type::string(),
                    | PatLit::Bool(_) => Type::bool(),
                    | PatLit::Unit => Type::Unit,
                };
                if let Err(error) = self.infer.unify(ty, &lit_ty) {
                    self.report_unify(error, pattern.span);
                }
            },
            | PatKind::As { pattern: inner, name } => {
                self.bind_pattern_inner(inner, ty);
                if let Some(scope) = self.scopes.last_mut() {
                    scope.insert(name.clone(), Scheme::mono(ty.clone()));
                }
            },
            | PatKind::Annot {
                pattern: inner,
                ty: annotation,
            } => {
                let annotated = self.convert_type(annotation);
                if let Err(error) = self.infer.unify(&annotated, ty) {
                    self.report_unify(error, pattern.span);
                }
                self.bind_pattern_inner(inner, &annotated);
            },
        }
    }

    // ----- types ---------------------------------------------------------

    fn convert_type(
        &mut self,
        ty: &Ty,
    ) -> Type {
        let params = self.type_params.clone();
        self.convert_type_with_params(ty, &params)
    }

    fn convert_type_with_params(
        &mut self,
        ty: &Ty,
        params: &[(String, Type)],
    ) -> Type {
        match &ty.kind {
            | TyKind::Int => Type::int(),
            | TyKind::Bool => Type::Bool,
            | TyKind::Prop => Type::Prop,
            | TyKind::Str => Type::string(),
            | TyKind::Unit => Type::Unit,
            | TyKind::Real(dim) => Type::real(self.convert_dim(dim, ty.span)),
            | TyKind::Float(dim) => Type::float(self.convert_dim(dim, ty.span)),
            | TyKind::Complex(dim) => Type::complex(self.convert_dim(dim, ty.span)),
            | TyKind::Point(dim) => Type::Point(self.convert_dim(dim, ty.span)),
            | TyKind::UnitOf(dim) => Type::UnitOf(self.convert_dim(dim, ty.span)),
            | TyKind::Tensor { shape, elem } => {
                Type::Tensor {
                    shape: self.convert_shape(shape),
                    elem: Box::new(self.convert_type_with_params(elem, params)),
                }
            },
            // `Matrix<n, m, τ>` is a rank-2 tensor with a static shape.
            | TyKind::Matrix { rows, cols, elem } => {
                Type::Tensor {
                    shape: Shape::Refined(vec![
                        Some(self.convert_nat(rows, ty.span)),
                        Some(self.convert_nat(cols, ty.span)),
                    ]),
                    elem: Box::new(self.convert_type_with_params(elem, params)),
                }
            },
            // `Sparse<fmt, shape, τ>` is its own type (§13.9.5): it carries the
            // format and the dense counterpart (same shape and element), and it
            // does not unify with `Tensor<shape, τ>`.
            | TyKind::Sparse { format, shape, elem } => {
                let dense = Type::Tensor {
                    shape: self.convert_shape(shape),
                    elem: Box::new(self.convert_type_with_params(elem, params)),
                };
                let canonical = match format.to_ascii_lowercase().as_str() {
                    | "csr" => "Csr",
                    | "csc" => "Csc",
                    | "coo" => "Coo",
                    | "block" => "Block",
                    | "bcsr" => "Bcsr",
                    | _ => "",
                };
                match builtins::format_type(canonical) {
                    | Some(marker) => builtins::sparse_type(marker, dense),
                    | None => {
                        self.error_help(
                            "E0310",
                            format!("`{format}` is not a sparse format"),
                            ty.span,
                            "the formats are Csr, Csc, Coo, Block and Bcsr",
                        );
                        Type::Error
                    },
                }
            },
            // A row sum is the union of its summands' fields.
            | TyKind::Row(items) => {
                let mut merged: Vec<(String, Type)> = Vec::new();
                for item in items {
                    let converted = self.convert_type_with_params(item, params);
                    match converted {
                        | Type::Record(row) => merged.extend(row.fields()),
                        | other => return other,
                    }
                }
                Type::record(merged)
            },
            // A dimension argument is carried as `Real[d]`, the encoding this
            // checker already uses for a dimension in argument position.
            | TyKind::DimArg(dim) => Type::real(self.convert_dim(dim, ty.span)),
            // A natural number or a shape is a value-level argument: it only
            // appears where a shape is expected, so it contributes no type.
            | TyKind::NatArg(_) | TyKind::ShapeArg(_) => Type::int(),
            | TyKind::Option(inner) => Type::option(self.convert_type_with_params(inner, params)),
            | TyKind::Result(ok, err) => {
                Type::result(
                    self.convert_type_with_params(ok, params),
                    self.convert_type_with_params(err, params),
                )
            },
            | TyKind::Set(inner) => {
                let inner = self.convert_type_with_params(inner, params);
                // §3.8.4: `Set<a>` needs `TotalOrd a`; `Set<Float>` is not
                // legal because `Float` has no total order (NaN).
                self.require_instance("TotalOrd", &inner, ty.span);
                Type::Adt(adt::set_id(), vec![inner])
            },
            | TyKind::Table(inner) => {
                Type::Adt(
                    adt::intern(Adt {
                        name: "Table".to_string(),
                        params: vec!["r".to_string()],
                        ctors: Vec::new(),
                    }),
                    vec![self.convert_type_with_params(inner, params)],
                )
            },
            | TyKind::Dyn(rep) => {
                Type::Dyn(match rep.as_str() {
                    | "Int" => Rep::Int,
                    | "Float" => Rep::Float,
                    | "Complex" => Rep::Complex,
                    | _ => Rep::Real,
                })
            },
            | TyKind::RateOf(inner) => {
                let inner = self.convert_type_with_params(inner, params);
                self.rate_of(&inner, ty.span)
            },
            | TyKind::DiffusionOf(inner) => {
                let inner = self.convert_type_with_params(inner, params);
                self.diffusion_of(&inner, ty.span)
            },
            | TyKind::SymFun(domain, codomain) => {
                let domain = self.convert_dim(domain, ty.span);
                let codomain = self.convert_dim(codomain, ty.span);
                Type::func(vec![Type::real(domain)], Type::real(codomain))
            },
            | TyKind::Io(inner) => Type::io(self.convert_type_with_params(inner, params)),
            | TyKind::Fn(args, ret) => {
                Type::func(
                    args.iter()
                        .map(|arg| self.convert_type_with_params(arg, params))
                        .collect(),
                    self.convert_type_with_params(ret, params),
                )
            },
            | TyKind::Record { fields, rest } => {
                let mut collected: Vec<(String, Type)> = fields
                    .iter()
                    .map(|(name, ty)| (name.clone(), self.convert_type_with_params(ty, params)))
                    .collect();
                if let Some(rest_name) = rest {
                    // An open row: the fields are known and the tail is either
                    // a declared row parameter (`type With<r> = { x: Int | r }`)
                    // or a fresh variable that absorbs whatever the caller
                    // passes.
                    collected.sort_by(|a, b| a.0.cmp(&b.0));
                    let mut row = match self
                        .row_params
                        .iter()
                        .rev()
                        .find(|(param, _)| param == rest_name)
                    {
                        | Some((_, row)) => row.clone(),
                        | None => self.infer.fresh_row(),
                    };
                    for (name, ty) in collected.into_iter().rev() {
                        row = Row::Extend {
                            label: name,
                            ty: Box::new(ty),
                            tail: Box::new(row),
                        };
                    }
                    Type::Record(row)
                } else {
                    Type::record(collected)
                }
            },
            | TyKind::Tuple(items) => {
                Type::record(
                    items
                        .iter()
                        .enumerate()
                        .map(|(index, ty)| {
                            (index.to_string(), self.convert_type_with_params(ty, params))
                        })
                        .collect::<Vec<_>>(),
                )
            },
            | TyKind::Named { path, args } => {
                let name = path.join(".");
                if args.is_empty()
                    && let Some((_, parameter)) = params
                        .iter()
                        .rev()
                        .find(|(parameter, _)| parameter == &name)
                {
                    return parameter.clone();
                }
                if self.declaring_alias.as_deref() == Some(name.as_str()) {
                    self.error_help(
                        "E0312",
                        "non-positive recursive type",
                        ty.span,
                        "a type alias cannot refer to itself; declare a variant type instead",
                    );
                    return Type::Error;
                }
                if let Some(alias) = self.aliases.get(&name).cloned() {
                    return self.instantiate_alias(&alias, args, ty.span);
                }
                if let Some(id) = self.adts.get(&name).copied() {
                    // §2/§8.9.1: an ADT parameter may be dimension-kinded; a
                    // dimension argument is encoded as `UnitOf(d)`, so the
                    // argument vector stays a vector of types.
                    let kinds = adt::param_kinds_of(id);
                    if args.len() > kinds.len() {
                        self.error(
                            "E0315",
                            format!(
                                "`{name}` takes {} type argument(s), found {}",
                                kinds.len(),
                                args.len()
                            ),
                            ty.span,
                        );
                    }
                    let mut converted = Vec::with_capacity(kinds.len());
                    for (index, kind) in kinds.iter().enumerate() {
                        match kind {
                            | adt::ParamKind::Type => {
                                let value = match args.get(index) {
                                    | Some(arg) => self.convert_type_with_params(arg, params),
                                    | None => self.infer.fresh_type(),
                                };
                                converted.push(value);
                            },
                            | adt::ParamKind::Dim(_) => {
                                let dim = match args.get(index) {
                                    | Some(arg) => self.convert_type_as_dim(arg),
                                    | None => self.infer.fresh_dim(),
                                };
                                converted.push(Type::UnitOf(dim));
                            },
                        }
                    }
                    return Type::Adt(id, converted);
                }
                if let Some(builtin) = builtin_type(&name) {
                    return builtin;
                }
                self.error("E0301", format!("unknown type `{name}`"), ty.span);
                Type::Error
            },
            | TyKind::Var(name) => {
                if let Some((_, ty)) = params.iter().rev().find(|(param, _)| param == name) {
                    return ty.clone();
                }
                if self.implicit_params {
                    // The parameter snapshot predates this traversal. Reuse a
                    // variable first seen in an earlier part of the same head.
                    if let Some((_, ty)) = self
                        .type_params
                        .iter()
                        .rev()
                        .find(|(param, _)| param == name)
                    {
                        return ty.clone();
                    }
                    // §3.8.1: a variable in an instance head is implicitly
                    // universally quantified.
                    let fresh = self.infer.fresh_type();
                    self.type_params.push((name.clone(), fresh.clone()));
                    return fresh;
                }
                self.error("E0301", format!("unknown type variable `{name}`"), ty.span);
                Type::Error
            },
            | TyKind::Forall(_, body) => self.convert_type_with_params(body, params),
        }
    }

    /// §8.9: `RateOf<s>` is a built-in type function — the derivative of a
    /// state has its dimension divided by `T`, recursively through tensors and
    /// records.
    ///
    /// A state field that cannot be flattened into the numeric kernel
    /// (`Int`, `Bool`, a function, a variant) is `E0302`: the ODE runtime
    /// flattens `Float` scalars and tensors into `Vec<f64>`, so anything else
    /// has no representation on that path. An open record row cannot be
    /// flattened either.
    fn rate_of(
        &mut self,
        ty: &Type,
        span: Span,
    ) -> Type {
        let resolved = self.infer.resolve(ty);
        match resolved {
            | Type::Scalar(rep, dim) => {
                let rep = self.infer.resolve_rep(rep);
                let is_int = rep == Rep::Int
                    || matches!(self.infer.rep_literal_kind(rep), Some(LiteralKind::Int));
                if is_int {
                    self.error_help(
                        "E0302",
                        "an ODE state cannot contain an `Int` field",
                        span,
                        "make the state field a `Float` (the solver works on `f64`)",
                    );
                    return Type::Error;
                }
                Type::Scalar(rep, dim.div(&Dim::base("Time")))
            },
            | Type::Tensor { shape, elem } => {
                let elem = self.rate_of(&elem, span);
                Type::Tensor {
                    shape: shape.clone(),
                    elem: Box::new(elem),
                }
            },
            | Type::Record(row) => {
                if row.tail_var().is_some() {
                    self.error_help(
                        "E0302",
                        "an ODE state cannot be an open record",
                        span,
                        "write the state as a closed record type",
                    );
                    return Type::Error;
                }
                let fields = row
                    .fields()
                    .into_iter()
                    .map(|(name, ty)| (name, self.rate_of(&ty, span)))
                    .collect::<Vec<_>>();
                Type::Record(Row::closed(fields))
            },
            | Type::Var(_) => {
                // An unannotated parameter of the right-hand side: commit it to
                // a scalar and use its literal kind.
                let scalar = self.force_scalar(&resolved);
                if matches!(self.infer.resolve(&scalar), Type::Error) {
                    Type::Error
                } else {
                    self.rate_of(&scalar, span)
                }
            },
            | other => {
                self.error_help(
                    "E0302",
                    format!("an ODE state cannot contain a value of type {other}"),
                    span,
                    "an `odeint` state is `Float` scalars, tensors and closed records of them",
                );
                Type::Error
            },
        }
    }

    /// §8.9.1: `DiffusionOf<s>` is the built-in type function for the diffusion
    /// of an SDE: a scalar's dimension is divided by `T^(1/2)` (the Brownian
    /// scaling `dW ~ √dt`), recursively through tensors and closed records.
    ///
    /// ```text
    /// DiffusionOf<Scalar<ρ,d>>        = Scalar<ρ, d / T^(1/2)>
    /// DiffusionOf<Tensor<n, τ>>       = Tensor<n, DiffusionOf<τ>>
    /// DiffusionOf<{ l1: τ1, … }>      = { l1: DiffusionOf<τ1>, … }
    /// ```
    ///
    /// `Int`/`Bool`/function fields and open rows are `E0302`, exactly as for
    /// `RateOf`: the SDE path flattens the state into `Vec<f64>`.
    fn diffusion_of(
        &mut self,
        ty: &Type,
        span: Span,
    ) -> Type {
        let resolved = self.infer.resolve(ty);
        match resolved {
            | Type::Scalar(rep, dim) => {
                let rep = self.infer.resolve_rep(rep);
                let is_int = rep == Rep::Int
                    || matches!(self.infer.rep_literal_kind(rep), Some(LiteralKind::Int));
                if is_int {
                    self.error_help(
                        "E0302",
                        "an SDE state cannot contain an `Int` field",
                        span,
                        "make the state field a `Float` (the integrator works on `f64`)",
                    );
                    return Type::Error;
                }
                let half = num_rational::BigRational::new(1.into(), 2.into());
                let sqrt_time = Dim::base("Time").pow(&half);
                // `dW ~ √dt`, so the diffusion's dimension is the state's
                // *divided* by `T^(1/2)` (Brownian scaling).
                Type::Scalar(rep, dim.div(&sqrt_time))
            },
            | Type::Tensor { shape, elem } => {
                let elem = self.diffusion_of(&elem, span);
                Type::Tensor {
                    shape: shape.clone(),
                    elem: Box::new(elem),
                }
            },
            | Type::Record(row) => {
                if row.tail_var().is_some() {
                    self.error_help(
                        "E0302",
                        "an SDE state cannot be an open record",
                        span,
                        "write the state as a closed record type",
                    );
                    return Type::Error;
                }
                let fields = row
                    .fields()
                    .into_iter()
                    .map(|(name, ty)| (name, self.diffusion_of(&ty, span)))
                    .collect::<Vec<_>>();
                Type::Record(Row::closed(fields))
            },
            | Type::Var(_) => {
                let scalar = self.force_scalar(&resolved);
                if matches!(self.infer.resolve(&scalar), Type::Error) {
                    Type::Error
                } else {
                    self.diffusion_of(&scalar, span)
                }
            },
            | other => {
                self.error_help(
                    "E0302",
                    format!("an SDE state cannot contain a value of type {other}"),
                    span,
                    "an SDE state is `Float` scalars, tensors and closed records of them",
                );
                Type::Error
            },
        }
    }

    /// §8.9.1: the diffusion of `sdeint_general` for a state `s` with `m` noise
    /// channels (the channel count is a run-time check, like the matrix width of
    /// a rank-1 state): `GeneralDiffusionOf<s>` is `DiffusionOf<s>` with a
    /// trailing noise axis on every leaf.
    ///
    /// ```text
    /// GeneralDiffusionOf<Scalar<ρ,d>>   = Tensor<1, Scalar<ρ, d / T^(1/2)>>
    /// GeneralDiffusionOf<Tensor<n, τ>>  = Tensor<n + 1, DiffusionOf<τ>>
    /// GeneralDiffusionOf<{ l: τ, … }>   = { l: GeneralDiffusionOf<τ>, … }
    /// ```
    ///
    /// Flattening it field by field, row-major, is the kernel's `n x m` diffusion
    /// matrix (a scalar field is one row of `m` coefficients).
    fn general_diffusion_of(
        &mut self,
        ty: &Type,
        span: Span,
    ) -> Type {
        let resolved = self.infer.resolve(ty);
        match resolved {
            | Type::Scalar(..) => {
                let leaf = self.diffusion_of(&resolved, span);
                if leaf.is_error() {
                    Type::Error
                } else {
                    Type::tensor(Shape::Rank(1), leaf)
                }
            },
            | Type::Tensor { shape, elem } => {
                let leaf = self.diffusion_of(&elem, span);
                if leaf.is_error() {
                    return Type::Error;
                }
                let shape = match shape {
                    | Shape::Rank(rank) => Shape::Rank(rank + 1),
                    | Shape::Refined(mut items) => {
                        items.push(None);
                        Shape::Refined(items)
                    },
                };
                Type::Tensor {
                    shape,
                    elem: Box::new(leaf),
                }
            },
            | Type::Record(row) => {
                if row.tail_var().is_some() {
                    self.error_help(
                        "E0302",
                        "an SDE state cannot be an open record",
                        span,
                        "write the state as a closed record type",
                    );
                    return Type::Error;
                }
                let fields = row
                    .fields()
                    .into_iter()
                    .map(|(name, ty)| (name, self.general_diffusion_of(&ty, span)))
                    .collect::<Vec<_>>();
                Type::Record(Row::closed(fields))
            },
            | Type::Var(_) => {
                let scalar = self.force_scalar(&resolved);
                if matches!(self.infer.resolve(&scalar), Type::Error) {
                    Type::Error
                } else {
                    self.general_diffusion_of(&scalar, span)
                }
            },
            // `Int`/`Bool`/function fields: the same `E0302` as `DiffusionOf`.
            | other => self.diffusion_of(&other, span),
        }
    }

    fn convert_shape(
        &mut self,
        shape: &ShapeAst,
    ) -> Shape {
        match shape {
            | ShapeAst::Rank(nat) => {
                let converted = self.convert_nat(nat, Span::new(self.file, 0, 0));
                match converted.as_const() {
                    | Some(rank) => Shape::Rank(u32::try_from(rank).unwrap_or(0)),
                    | None => {
                        // §3.5.4: indexing needs a known rank, so a rank that
                        // is still a variable is `E0304` (write a concrete
                        // rank). Refined extents may stay variables.
                        self.error_help(
                            "E0304",
                            "the rank of a tensor must be a concrete natural number",
                            Span::new(self.file, 0, 0),
                            "write a concrete rank, for example `Tensor<1, Float>`, or use a refined shape `Tensor<[n], Float>`",
                        );
                        Shape::Rank(0)
                    },
                }
            },
            | ShapeAst::Refined(items) => {
                Shape::Refined(
                    items
                        .iter()
                        .map(|item| {
                            match item {
                                | ShapeItem::Known(nat) => {
                                    Some(self.convert_nat(nat, Span::new(self.file, 0, 0)))
                                },
                                | ShapeItem::Unknown => None,
                            }
                        })
                        .collect(),
                )
            },
        }
    }

    /// Converts a type-level natural number (§3.5.1).
    ///
    /// A variable resolves through the enclosing declaration's `<n: Nat>` (or
    /// an unkinded parameter, which gets a Nat reading too). An unknown name is
    /// `E0304`: unlike an unkinded type parameter, an undeclared Nat variable
    /// has no binder to quantify over. Inside an instance head the name is an
    /// implicit parameter, like a dimension there.
    fn convert_nat(
        &mut self,
        nat: &NatAst,
        span: Span,
    ) -> Nat {
        match nat {
            | NatAst::Lit(value) => Nat::Lit(*value),
            | NatAst::Var(name) => {
                if let Some((_, nat)) = self
                    .nat_params
                    .iter()
                    .rev()
                    .find(|(param, _)| param == name)
                {
                    return nat.clone();
                }
                if self.implicit_params {
                    return self.infer.fresh_nat();
                }
                self.error_help(
                    "E0304",
                    format!("cannot infer the shape extent `{name}`"),
                    span,
                    "declare it, for example `fn f<n>(xs: Tensor<[n], Float>)`; only literals, `+` and `*` by a literal are supported in a shape",
                );
                Nat::Lit(0)
            },
            | NatAst::Add(left, right) => {
                let left = self.convert_nat(left, span);
                let right = self.convert_nat(right, span);
                match (left.as_const(), right.as_const()) {
                    | (Some(left), Some(right)) => Nat::Lit(left.saturating_add(right)),
                    | _ => Nat::Add(Box::new(left), Box::new(right)),
                }
            },
            | NatAst::Mul(left, right) => {
                let left = self.convert_nat(left, span);
                let right = self.convert_nat(right, span);
                match (left.as_const(), right.as_const()) {
                    | (Some(left), Some(right)) => Nat::Lit(left.saturating_mul(right)),
                    | _ => Nat::Mul(Box::new(left), Box::new(right)),
                }
            },
        }
    }

    fn convert_dim(
        &mut self,
        dim: &DimAst,
        span: Span,
    ) -> Dim {
        match dim {
            | DimAst::One => Dim::one(),
            | DimAst::Name(name) => {
                if let Some((_, dim)) = self
                    .dim_params
                    .iter()
                    .rev()
                    .find(|(param, _)| param == name)
                {
                    return dim.clone();
                }
                match self.dims.lookup(name) {
                    // §5.11.2: a dimension written in a module with a unit
                    // system denotes its representative (`L` is `Energy^-1`
                    // in natural units). The reduction is a homomorphism, so
                    // reducing each named factor reduces the whole product.
                    | Some(dim) if !self.si_dimensions => self.units.reduce_dim(&dim),
                    | Some(dim) => dim,
                    | None => {
                        if self.implicit_params {
                            // §3.8.1: `instance Show for Real[d]` quantifies
                            // `d` implicitly.
                            let fresh = self.infer.fresh_dim();
                            self.dim_params.push((name.clone(), fresh.clone()));
                            return fresh;
                        }
                        let help = self.unknown_dimension_help(name);
                        let mut diagnostic =
                            Diagnostic::error("E0407", format!("unknown dimension `{name}`"), span);
                        if let Some(help) = help {
                            diagnostic = diagnostic.with_help(help);
                        }
                        self.diagnostics.push(diagnostic);
                        Dim::one()
                    },
                }
            },
            | DimAst::Mul(left, right) => {
                self.convert_dim(left, span)
                    .mul(&self.convert_dim(right, span))
            },
            | DimAst::Div(left, right) => {
                self.convert_dim(left, span)
                    .div(&self.convert_dim(right, span))
            },
            | DimAst::Pow(base, exponent) => self.convert_dim(base, span).pow(exponent),
            | DimAst::Paren(inner) => self.convert_dim(inner, span),
        }
    }

    // ----- helpers -------------------------------------------------------

    fn lookup_value(
        &mut self,
        name: &str,
        span: Span,
    ) -> Type {
        self.referenced.insert(name.to_string());
        // §13.8.1: a deprecated name still works, but using it warns — that is
        // the "warning period" the stability model promises.
        if let Some(message) = self.deprecated.get(name).cloned() {
            self.warn("W0602", format!("`{name}` is deprecated: {message}"), span);
        }
        for (index, scope) in self.scopes.iter().enumerate().rev() {
            if let Some(scheme) = scope.get(name) {
                // §B.12: this local is read, so `W0101` stays quiet for it.
                self.used_locals.insert(name.to_string());
                let scheme = scheme.clone();
                let headers = self
                    .local_headers
                    .get(&(index + 1, name.to_string()))
                    .cloned()
                    .unwrap_or_else(|| inferred_binders(&scheme));
                let origin = self
                    .local_callables
                    .get(&(index + 1, name.to_string()))
                    .cloned();
                return self.instantiate_with_headers(&scheme, &headers, origin.as_ref(), span);
            }
        }
        if let Some(scheme) = self.globals.get(name).cloned() {
            if !self.is_locally_defined(name) {
                if let Some(reference) = self.constructor_bindings.get(name) {
                    self.checked.constructor_refs.insert(span, *reference);
                }
                if let Some(reference) = self.method_bindings.get(name) {
                    self.checked.method_refs.insert(span, reference.clone());
                }
            }
            let headers = self.generic_bindings.get(name).cloned().unwrap_or_else(|| {
                if self.constructor_bindings.contains_key(name) {
                    constructor_binders(&scheme)
                } else {
                    inferred_binders(&scheme)
                }
            });
            let origin = self.global_callables.get(name).cloned();
            return self.instantiate_with_headers(&scheme, &headers, origin.as_ref(), span);
        }
        if let Some(unit) = self.units.lookup(name) {
            return Type::UnitOf(unit.dim);
        }
        if name == "from_value" {
            return Type::FromValueIntrinsic(None);
        }
        if name == "read_npy" {
            return Type::ReadNpyIntrinsic(None);
        }
        if name == "write_npy" {
            return Type::WriteNpyIntrinsic;
        }
        if name == "to_callback" {
            return Type::ToCallbackIntrinsic;
        }
        if name == "type_rep" {
            return Type::TypeRepIntrinsic(None);
        }
        if let Some(intrinsic) = crate::ty::TensorIntrinsic::from_name(name) {
            return Type::TensorIntrinsic(crate::ty::TensorIntrinsicType {
                kind: intrinsic,
                element: None,
            });
        }
        if let Some(candidates) = builtins::schemes(name) {
            return self.infer.instantiate(&candidates[0]);
        }
        if let Some((adt, index, _)) = adt::find_prelude_ctor(name) {
            let scheme = ctor_scheme(adt, index);
            return self.infer.instantiate(&scheme);
        }
        let suggestion = {
            let locals = self.scopes.iter().flat_map(|scope| scope.keys());
            let candidates = locals
                .chain(self.globals.keys())
                .map(String::as_str)
                .chain(builtins::PRELUDE_NAMES.iter().copied());
            closest_name(name, candidates).map(str::to_string)
        };
        match suggestion {
            | Some(close) => {
                self.error_help(
                    "E0301",
                    format!("unbound name `{name}`"),
                    span,
                    &format!("did you mean `{close}`?"),
                );
            },
            | None => self.error("E0301", format!("unbound name `{name}`"), span),
        }
        Type::Error
    }

    fn instantiate_with_headers(
        &mut self,
        scheme: &Scheme,
        headers: &[GenericBinder],
        origin: Option<&CallableProjection>,
        span: Span,
    ) -> Type {
        let (ty, constraints, bindings) = self.infer.instantiate_scheme_with_bindings(scheme);
        if let Some(origin) = origin {
            self.checked
                .callable_refs
                .insert(span, origin.substituted(&bindings));
        }
        self.call_materializations.insert(
            span,
            scheme
                .materializations
                .iter()
                .map(|ty| bindings.apply(ty))
                .collect(),
        );
        if !headers.is_empty() {
            self.checked.generic_refs.insert(
                span,
                headers
                    .iter()
                    .map(|header| header.substituted(&bindings))
                    .collect(),
            );
        }
        for constraint in &constraints {
            self.solve_constraint(constraint, span);
        }
        ty
    }

    fn expect_bool(
        &mut self,
        ty: &Type,
        span: Span,
    ) {
        if matches!(self.infer.resolve(ty), Type::Var(_)) {
            let _ = self.infer.unify(ty, &Type::Bool);
            return;
        }
        match self.infer.resolve(ty) {
            | Type::Bool | Type::Error => {},
            | Type::Prop => {
                self.error_help(
                    "E0302",
                    "a symbolic proposition is not a `Bool`",
                    span,
                    "use `decide(p)` or `assuming(p, ...)`",
                );
            },
            | other => self.error("E0302", format!("expected Bool, found {other}"), span),
        }
    }

    fn report_unify(
        &mut self,
        error: UnifyError,
        span: Span,
    ) {
        match error {
            | UnifyError::Dim(mismatch) => {
                let left = self.dims.format(&mismatch.left);
                let right = self.dims.format(&mismatch.right);
                let difference = self.dims.format(&mismatch.difference);
                self.diagnostics.push(
                    Diagnostic::error("E0401", "dimension mismatch", span)
                        .with_primary_label(format!("expected {right}, found {left}"))
                        .with_note(format!("left:  {left}"))
                        .with_note(format!("right: {right}"))
                        .with_note(format!("difference: {difference}")),
                );
            },
            | UnifyError::Mismatch { left, right } => {
                let mut diagnostic = Diagnostic::error("E0302", "type mismatch", span)
                    .with_primary_label(format!("expected {right}, found {left}"));
                if matches!(
                    (&left, &right),
                    (Type::TensorIntrinsic(_), Type::Fn(..))
                        | (Type::Fn(..), Type::TensorIntrinsic(_))
                ) {
                    diagnostic = diagnostic.with_help("a variadic tensor constructor cannot satisfy a fixed function type; wrap it in a lambda with the required arguments, for example `fn (n) => zeros(n)`");
                }
                self.diagnostics.push(diagnostic);
            },
            | UnifyError::LiteralMismatch { kind, requested } => {
                let label = match kind {
                    | LiteralKind::Int | LiteralKind::Decimal => {
                        format!("a bare numeric literal cannot have type {requested}")
                    },
                    | _ => format!("this literal cannot have type {requested}"),
                };
                // The help depends on what was asked for: a dimension wants
                // a unit, a `Complex` wants a constructor, and anything else
                // (a `Bool`, a `String`) has no literal-level fix.
                let requested_ty = self.infer.resolve(&requested);
                let help = match &requested_ty {
                    | Type::Scalar(Rep::Complex, _) => {
                        Some(
                            "a real literal is not a `Complex`; build one with `complex(re, im)` \
                         (for real data, `rfft` takes a real tensor)",
                        )
                    },
                    | Type::Scalar(_, dim) if !self.infer.resolve_dim(dim).is_one() => {
                        Some("write a unit, for example `1 m`")
                    },
                    | _ => None,
                };
                let mut diagnostic =
                    Diagnostic::error("E0302", "type mismatch", span).with_primary_label(label);
                if let Some(help) = help {
                    diagnostic = diagnostic.with_help(help);
                }
                self.diagnostics.push(diagnostic);
            },
            | UnifyError::ShapeMismatch { left, right } => {
                let error = UnifyError::ShapeMismatch {
                    left: left.clone(),
                    right: right.clone(),
                };
                let mut diagnostic = Diagnostic::error(
                    code(&error),
                    format!(
                        "shape mismatch: expected {}, found {}",
                        self.infer.render_shape(&left),
                        self.infer.render_shape(&right)
                    ),
                    span,
                );
                if left.rank() != right.rank() {
                    diagnostic = diagnostic.with_help(
                        "tensors of different rank do not broadcast (§3.5.3); \
                         build the larger shape explicitly, for example with `map` or `outer`",
                    );
                }
                self.diagnostics.push(diagnostic);
            },
            // One unsatisfiable representation system is one error. The
            // relation stays in the constraint store, so every later
            // propagation through it would otherwise report the same
            // conflict again at each span it touches.
            | UnifyError::Numeric(message) => {
                if std::mem::replace(&mut self.numeric_conflict_reported, true) {
                    return;
                }
                self.diagnostics.push(
                    Diagnostic::error("E0302", "type mismatch", span)
                        .with_primary_label(
                            "no numeric representation satisfies the arithmetic here",
                        )
                        .with_note(message)
                        .with_help(
                            "a decimal literal is never `Int`; annotate the binding with \
                             `Float` or `Real`, or convert explicitly",
                        ),
                );
            },
            | other => {
                self.diagnostics
                    .push(Diagnostic::error(code(&other), other.to_string(), span));
            },
        }
    }

    fn report_dim_error(
        &mut self,
        error: UnifyError,
        span: Span,
        left_span: Span,
        right_span: Span,
    ) {
        let UnifyError::Dim(mismatch) = error else {
            self.report_unify(error, span);
            return;
        };
        let left = self.dims.format(&mismatch.left);
        let right = self.dims.format(&mismatch.right);
        let difference = self.dims.format(&mismatch.difference);
        self.diagnostics.push(
            Diagnostic::error("E0401", "dimension mismatch", span)
                .with_primary_label(format!("expected {right}, found {left}"))
                .with_label(left_span, format!("this is {left}"))
                .with_label(right_span, format!("this is {right}"))
                .with_note(format!("difference: {difference}")),
        );
        if mismatch.right.is_one() {
            self.diagnostics
                .items
                .last_mut()
                .expect("diagnostic was just pushed")
                .helps
                .push("a bare number has no unit; write `2 m` if you meant a length".to_string());
        }
    }

    fn unit_error(
        &mut self,
        error: UnitError,
        span: Span,
    ) {
        let mut diagnostic = Diagnostic::error(error.code(), error.to_string(), span);
        if let UnitError::Unknown(name) = &error
            && let Some(close) = self.close_unit(name)
        {
            diagnostic = diagnostic.with_help(format!("did you mean `{close}`?"));
        }
        self.diagnostics.push(diagnostic);
    }

    /// The unit closest to a misspelt `name`, counting the common prefixed
    /// spellings (`km`, `ms`, …) as candidates. The error path is the only
    /// caller, so building the candidate list on demand is fine.
    fn close_unit(
        &self,
        name: &str,
    ) -> Option<String> {
        const COMMON_PREFIXES: &[&str] = &["k", "M", "G", "T", "m", "c", "µ", "n", "p"];
        // A prefix followed by a misspelt unit (`kmm` for `km`) reads best as
        // the same prefix on the nearest unit name.
        for prefix in COMMON_PREFIXES {
            if let Some(rest) = name.strip_prefix(prefix)
                && rest.len() > 1
            {
                let bases: Vec<&String> = self
                    .units
                    .list()
                    .iter()
                    .filter(|def| def.prefixable)
                    .flat_map(|def| std::iter::once(&def.name).chain(def.aliases.iter()))
                    .collect();
                if let Some(close) = closest_unit(rest, bases.iter().map(|s| s.as_str())) {
                    return Some(format!("{prefix}{close}"));
                }
            }
        }
        let mut candidates: Vec<String> = Vec::new();
        for def in self.units.list() {
            let spellings = std::iter::once(&def.name).chain(def.aliases.iter());
            for spelling in spellings {
                candidates.push(spelling.clone());
                if def.prefixable {
                    for prefix in COMMON_PREFIXES {
                        candidates.push(format!("{prefix}{spelling}"));
                    }
                }
            }
        }
        closest_unit(name, candidates.iter().map(String::as_str)).map(str::to_string)
    }

    /// The help for an unknown dimension name: a unit written where a
    /// dimension belongs is the common slip (`Float[m]` for `Float[L]`), then a
    /// misspelt dimension.
    fn unknown_dimension_help(
        &self,
        name: &str,
    ) -> Option<String> {
        if let Some(unit) = self.units.lookup_si(name) {
            let dim = self.dims.format(&unit.dim);
            return Some(format!(
                "`{name}` is a unit, not a dimension: it measures `{dim}`, so write `Float[{dim}]` (units appear in values, as in `2 {name}`)"
            ));
        }
        let mut candidates: Vec<&str> = self.dims.base_names().iter().map(String::as_str).collect();
        candidates.extend(self.dims.aliases().keys().map(String::as_str));
        closest_name(name, candidates).map(|close| format!("did you mean `{close}`?"))
    }

    fn error(
        &mut self,
        code: &str,
        message: impl Into<String>,
        span: Span,
    ) {
        self.diagnostics
            .push(Diagnostic::error(code, message, span));
    }

    fn error_help(
        &mut self,
        code: &str,
        message: impl Into<String>,
        span: Span,
        help: &str,
    ) {
        self.diagnostics
            .push(Diagnostic::error(code, message, span).with_help(help));
    }

    fn warn(
        &mut self,
        code: &str,
        message: impl Into<String>,
        span: Span,
    ) {
        self.diagnostics
            .push(Diagnostic::warning(code, message, span));
    }
}

/// The name of a unit when an expression is a bare unit value.
fn unit_name_of(expr: &Expr) -> Option<String> {
    match &expr.kind {
        | ExprKind::Var(name) => Some(name.clone()),
        | ExprKind::QName(path) => Some(path.join(".")),
        | _ => None,
    }
}

/// Whether a type mentions any of `ids`.
fn mentions_any(
    ty: &Type,
    ids: &[u32],
) -> bool {
    match ty {
        | Type::Var(id) => ids.contains(id),
        | Type::Scalar(rep, _) => {
            match rep {
                | Rep::Var(id) => ids.contains(id),
                | _ => false,
            }
        },
        | Type::Tensor { elem, .. } => mentions_any(elem, ids),
        | Type::Fn(params, ret) => {
            params.iter().any(|param| mentions_any(param, ids)) || mentions_any(ret, ids)
        },
        | Type::Record(row) => {
            row.fields()
                .iter()
                .any(|(_, field)| mentions_any(field, ids))
        },
        | Type::Adt(_, args) => args.iter().any(|arg| mentions_any(arg, ids)),
        | Type::Io(inner) => mentions_any(inner, ids),
        | _ => false,
    }
}

/// Whether an expression calls `symbol` or `symfun`.
fn is_symbol_factory(expr: &Expr) -> bool {
    match &expr.kind {
        | ExprKind::Call { callee, .. } => {
            match &callee.kind {
                | ExprKind::Var(name) => matches!(name.as_str(), "symbol" | "symfun"),
                | _ => false,
            }
        },
        | _ => false,
    }
}

/// Whether `class ty` is satisfied by the builtin structural fallback.
///
/// §3.8.4 lists `Show` for records, `Tensor`, `Point`, `Solutions`, the scalar
/// representations and `Dyn`; the printer already implements them, so an
/// explicit instance is not required (round-2 decision 1e). Only `Show` has
/// this guarantee.
fn is_structural_show(
    class: &str,
    ty: &Type,
) -> bool {
    class == "Show" && is_structural_show_head(ty)
}

/// Whether an expression is the bare literal `0` (§4.7).
fn is_bare_zero(expr: &Expr) -> bool {
    matches!(&expr.kind, ExprKind::Int(value) if value.is_zero())
}

/// The value of a literal index, when it is one (§3.5.4).
fn literal_index(expr: &Expr) -> Option<i64> {
    match &expr.kind {
        | ExprKind::Int(value) => value.to_i64(),
        | ExprKind::Unary {
            op: UnOp::Neg,
            operand,
        } => literal_index(operand).map(|value| -value),
        | _ => None,
    }
}

/// Whether a type mentions a function anywhere.
fn contains_function(ty: &Type) -> bool {
    match ty {
        | Type::Fn(..) => true,
        | Type::Tensor { elem, .. } => contains_function(elem),
        | Type::Record(row) => {
            row.fields()
                .iter()
                .any(|(_, field)| contains_function(field))
        },
        | Type::Adt(_, args) => args.iter().any(contains_function),
        | Type::Io(inner) => contains_function(inner),
        | _ => false,
    }
}

/// The least reading of a literal that is above `concrete` (§3.4.1).
///
/// A literal has a finite set of representations; the default is preferred
/// when it is high enough, otherwise the first accepted reading above the
/// bound.
fn literal_reading_above(
    kind: LiteralKind,
    concrete: Rep,
) -> Option<Rep> {
    let default = Rep::default_for_literal(kind);
    if kind.accepts_rep(default) && default.at_least(concrete) {
        return Some(default);
    }
    Rep::upper_candidates(concrete)
        .iter()
        .copied()
        .find(|candidate| kind.accepts_rep(*candidate))
}

fn annotation_span(ty: &Ty) -> Span {
    ty.span
}

fn literal_exponent(expr: &Expr) -> Option<BigRational> {
    match &expr.kind {
        | ExprKind::Int(value) => {
            value
                .to_i64()
                .map(|value| BigRational::from_integer(BigInt::from(value)))
        },
        | ExprKind::Unary {
            op: UnOp::Neg,
            operand,
        } => literal_exponent(operand).map(|value| -value),
        | ExprKind::Binary {
            op: BinOp::Div,
            left,
            right,
        } => {
            match (&left.kind, &right.kind) {
                | (ExprKind::Int(a), ExprKind::Int(b)) if !b.is_zero() => {
                    let a = BigRational::from_integer(a.clone());
                    let b = BigRational::from_integer(b.clone());
                    Some(a / b)
                },
                | _ => None,
            }
        },
        | _ => None,
    }
}

fn ctor_scheme(
    adt: adt::AdtId,
    index: usize,
) -> Scheme {
    let Some(registered) = adt::get(adt) else {
        return Scheme::mono(Type::Error);
    };
    let Some(ctor) = registered.ctors.get(index) else {
        return Scheme::mono(Type::Error);
    };
    // §2/§8.9.1: type parameters become quantified type variables; a
    // dimension-kinded parameter becomes `UnitOf(d)` with `d` quantified as a
    // dimension variable. That keeps `Type::Adt`'s argument vector a vector of
    // types while `SdeOpts<L>` and `SdeOpts<T>` still unify through the
    // dimension (`UnitOf(L)` unifies with `UnitOf(T)` when `L ~ T`).
    let kinds = adt::param_kinds_of(adt);
    let mut vars = Vec::new();
    let mut dims = Vec::new();
    let mut params: Vec<Type> = Vec::with_capacity(kinds.len());
    for (position, kind) in kinds.iter().enumerate() {
        match kind {
            | adt::ParamKind::Type => {
                let id = u32::try_from(position).unwrap_or(0);
                vars.push(id);
                params.push(Type::Var(id));
            },
            | adt::ParamKind::Dim(id) => {
                dims.push(*id);
                params.push(Type::UnitOf(Dim::var(*id)));
            },
        }
    }
    let result = Type::Adt(adt, params.clone());
    let fields: Vec<Type> = ctor
        .fields
        .iter()
        .map(|field| field.substitute_params(&params))
        .collect();
    let ty = if fields.is_empty() {
        result
    } else {
        Type::func(fields, result)
    };
    Scheme {
        vars,
        dims,
        rows: Vec::new(),
        nats: Vec::new(),
        constraints: Vec::new(),
        numeric: Vec::new(),
        materializations: Vec::new(),
        ty,
    }
}

#[must_use]
fn builtin_type(name: &str) -> Option<Type> {
    // ABI names must resolve to the shared prelude types, rather than fresh
    // abstract declarations in each generated kernel facade.
    if name == "Value" {
        return Some(dynamic_value_type());
    }
    if name == "Symbol" {
        return Some(Type::Dyn(Rep::Real));
    }
    const OPAQUE: [&str; 20] = [
        "EvalError",
        "MathError",
        "DataError",
        "IOError",
        "LinAlgError",
        "ShapeError",
        "ParseError",
        "OpError",
        "DimError",
        "SimError",
        "PluginError",
        "Dynamic",
        "TypeRep",
        "Callback",
        "Plugin",
        "PluginId",
        "DimValue",
        // §8.17: the simulation boundary type, so a wrapper can name it.
        "Json",
        // §8.9.1: the SDE boundary types (opaque until the standard-library
        // module declares the method constructors).
        "SdeError",
        "SdeMethod",
    ];
    if OPAQUE.contains(&name) {
        let id = adt::intern(Adt {
            name: name.to_string(),
            params: Vec::new(),
            ctors: Vec::new(),
        });
        return Some(Type::Adt(id, Vec::new()));
    }
    None
}

/// Whether two element types are both tensors of the same rank but with a
/// different known extent — the `E0203` ragged-literal case (§3.5.2).
fn shapes_are_ragged(
    left: &Type,
    right: &Type,
) -> bool {
    let (
        Type::Tensor {
            shape: Shape::Refined(a),
            ..
        },
        Type::Tensor {
            shape: Shape::Refined(b),
            ..
        },
    ) = (left, right)
    else {
        return false;
    };
    if a.len() != b.len() {
        return false;
    }
    a.iter().zip(b.iter()).any(|(left, right)| {
        match (left, right) {
            | (Some(left), Some(right)) => left != right,
            | _ => false,
        }
    })
}

/// The name a declaration introduces, when it introduces one.
fn declaration_name(decl: &Decl) -> Option<String> {
    match &decl.kind {
        | DeclKind::Fn { name, .. }
        | DeclKind::Type { name, .. }
        | DeclKind::Class { name, .. }
        | DeclKind::Unit { name, .. }
        | DeclKind::Dimension { name, .. }
        | DeclKind::Test { name, .. } => Some(name.clone()),
        | DeclKind::Let { pattern, .. } => {
            match &pattern.kind {
                | PatKind::Var(name) => Some(name.clone()),
                | _ => None,
            }
        },
        | DeclKind::Import { .. } | DeclKind::Instance { .. } | DeclKind::Derive { .. } => None,
    }
}

/// Whether an attribute argument is a compile-time constant (§2.2.7).
///
/// A name is accepted because it may denote a `let` constant (the constant
/// itself is checked where it is declared); a call or any other value-producing
/// expression is not a compile-time constant.
fn is_attribute_constant(expr: &Expr) -> bool {
    match &expr.kind {
        | ExprKind::Int(_)
        | ExprKind::Float(_)
        | ExprKind::Bool(_)
        | ExprKind::UnitLit
        | ExprKind::Quantity { .. } => true,
        // A string is constant only without interpolation.
        | ExprKind::Str(pieces) => {
            pieces
                .iter()
                .all(|piece| matches!(piece, StrPiece::Text(_)))
        },
        | ExprKind::Var(_) | ExprKind::QName(_) => true,
        // A constructor applied to constants (`Gpu(0)`, §13.9.4) is a
        // constant: constructors are capitalised, functions are not.
        | ExprKind::Call { callee, args } => {
            matches!(&callee.kind, ExprKind::Var(name) if name.starts_with(char::is_uppercase))
                && args.iter().all(is_attribute_constant)
        },
        | ExprKind::Unary { op, operand } => {
            matches!(op, UnOp::Neg | UnOp::Pos | UnOp::Not) && is_attribute_constant(operand)
        },
        | ExprKind::Binary { left, right, .. } => {
            is_attribute_constant(left) && is_attribute_constant(right)
        },
        | ExprKind::Tuple(items) => items.iter().all(is_attribute_constant),
        | ExprKind::Tensor(items) => items.iter().all(is_attribute_constant),
        | ExprKind::Record { base: None, fields } => {
            fields.iter().all(|(_, value)| is_attribute_constant(value))
        },
        | _ => false,
    }
}

/// The text of a compile-time constant string expression (§2.2.7: attribute
/// arguments must be constant).
fn string_constant(expr: &Expr) -> Option<String> {
    let ExprKind::Str(pieces) = &expr.kind else {
        return None;
    };
    let mut text = String::new();
    for piece in pieces {
        match piece {
            | StrPiece::Text(part) => text.push_str(part),
            | StrPiece::Expr(_) => return None,
        }
    }
    Some(text)
}

/// The class methods of §3.8.4 that the prelude declares.
///
/// Their type is `∀a. C a => σ`, which is what makes `show(f)` and `f == f`
/// errors: a function has no `Show`/`Eq` instance.
fn prelude_class_methods() -> Vec<(&'static str, &'static str, Type)> {
    let var = Type::Var(0);
    let ordering = Type::Adt(adt::ORDERING_ID, Vec::new());
    vec![
        ("Show", "show", Type::func(vec![var.clone()], Type::Str)),
        ("Show", "repr", Type::func(vec![var.clone()], Type::Str)),
        ("Show", "to_latex", Type::func(vec![var.clone()], Type::Str)),
        ("Show", "to_typst", Type::func(vec![var.clone()], Type::Str)),
        (
            "Ord",
            "compare",
            Type::func(
                vec![var.clone(), var.clone()],
                Type::option(ordering.clone()),
            ),
        ),
        (
            "TotalOrd",
            "total_compare",
            Type::func(vec![var.clone(), var.clone()], ordering),
        ),
        ("Hash", "hash", Type::func(vec![var], Type::int())),
    ]
}

/// Functions of §3.4.4 that only accept dimensionless arguments.
fn is_transcendental(name: &str) -> bool {
    matches!(
        name,
        "exp"
            | "ln"
            | "log"
            | "log2"
            | "log10"
            | "sin"
            | "cos"
            | "tan"
            | "asin"
            | "acos"
            | "atan"
            | "sinh"
            | "cosh"
            | "tanh"
            | "asinh"
            | "acosh"
            | "atanh"
            | "gamma"
            | "lgamma"
            | "erf"
            | "erfc"
            | "zeta"
    )
}

/// The flat key a qualified field access names, when the base is a path or an
/// import alias.
fn qualified_name(
    base: &Expr,
    field: &str,
) -> Option<String> {
    match &base.kind {
        | ExprKind::QName(path) => Some(format!("{}.{field}", path.join("."))),
        | ExprKind::Var(name) if name.chars().next().is_some_and(char::is_uppercase) => {
            Some(format!("{name}.{field}"))
        },
        | _ => None,
    }
}

/// Whether an expression is `Rssn.<member>`.
fn is_rssn_member(
    expr: &Expr,
    member: &str,
) -> bool {
    match &expr.kind {
        | ExprKind::Field { base, name } => name == member && is_rssn_root(base),
        | ExprKind::QName(path) => path.len() == 2 && path[0] == "Rssn" && path[1] == member,
        | _ => false,
    }
}

/// The operator name when an expression names `Rssn.<op>`.
///
/// The manifest's non-operator members (`Rssn.call`, `Rssn.Version`, …) are
/// excluded: they have a fixed type and are not resolved against overloads.
fn rssn_op_name(expr: &Expr) -> Option<String> {
    const MEMBERS: [&str; 16] = [
        "Version",
        "ManifestHash",
        "Capabilities",
        "Units",
        "Constants",
        "ValueKinds",
        "call",
        "dynamic",
        "call_key",
        "invoke",
        "resolve_overload",
        "typed",
        "call_ho",
        "call_ho_key",
        "register",
        "register_plugin",
    ];
    let name = match &expr.kind {
        | ExprKind::Field { base, name } if is_rssn_root(base) => name.clone(),
        | ExprKind::QName(path) if path.len() == 2 && path[0] == "Rssn" => path[1].clone(),
        | _ => return None,
    };
    if MEMBERS.contains(&name.as_str()) || name == "overloads" {
        return None;
    }
    Some(name)
}

/// Whether an expression names the `Rssn` module itself.
fn is_rssn_root(expr: &Expr) -> bool {
    match &expr.kind {
        | ExprKind::Var(name) => name == "Rssn",
        | ExprKind::QName(path) => path.len() == 1 && path[0] == "Rssn",
        | _ => false,
    }
}

/// `Tensor<1, Value>`: the argument list of a dynamic call.
fn value_list_type() -> Type {
    Type::tensor(Shape::Rank(1), dynamic_value_type())
}

/// An opaque host type of the dynamic ABI (`TypeRep`, `Plugin`, …).
///
/// It is interned by name, so this and `builtin_type`'s `OPAQUE` table resolve
/// to the same `AdtId` and a source annotation such as `fn f(p: Plugin)` names
/// the type the checker's signatures use.
fn opaque_type(name: &str) -> Type {
    let id = adt::intern(Adt {
        name: name.to_string(),
        params: Vec::new(),
        ctors: Vec::new(),
    });
    Type::Adt(id, Vec::new())
}

/// Maps a manifest signature type to the checker's type.
fn rssn_type(
    sig: &aevia_rssn::SigType,
    infer: &mut Infer,
) -> Type {
    use aevia_rssn::SigType as S;
    match sig {
        | S::Int => Type::int(),
        | S::Real => Type::real(Dim::one()),
        | S::Float => Type::float(Dim::one()),
        | S::Complex => Type::complex(Dim::one()),
        | S::Bool => Type::Bool,
        | S::Str => Type::Str,
        | S::Symbol => Type::Dyn(Rep::Real),
        | S::Unit(dim) => Type::UnitOf(dim.clone()),
        | S::Point(dim) => Type::Point(dim.clone()),
        | S::Dim(inner, dim) => {
            match **inner {
                | S::Int => Type::Scalar(Rep::Int, dim.clone()),
                | S::Float => Type::Scalar(Rep::Float, dim.clone()),
                | S::Complex => Type::Scalar(Rep::Complex, dim.clone()),
                | _ => Type::Scalar(Rep::Real, dim.clone()),
            }
        },
        | S::Tensor { rank, shape, elem } => {
            if shape
                .as_ref()
                .is_some_and(|shape| rank.is_some_and(|rank| rank != shape.len()))
            {
                return Type::Error;
            }
            let shape = shape.as_ref().map_or_else(
                || Shape::Rank(rank.map_or(1, |rank| u32::try_from(rank).unwrap_or(1))),
                |shape| Shape::refined(shape.iter().copied().map(Some)),
            );
            Type::tensor(shape, rssn_type(elem, infer))
        },
        // A kernel sparse value is an Aevia `Sparse<F, shape, τ>` of the same
        // rank. The format is a runtime property of the kernel value, so it
        // stays an unsolved variable (§13.9.5).
        | S::Sparse { rank, elem } => {
            let shape = Shape::Rank(rank.map_or(1, |rank| u32::try_from(rank).unwrap_or(1)));
            let format = infer.fresh_type();
            builtins::sparse_type(format, Type::tensor(shape, rssn_type(elem, infer)))
        },
        | S::Record(fields) => {
            Type::record(
                fields
                    .iter()
                    .map(|(name, ty)| (name.clone(), rssn_type(ty, infer))),
            )
        },
        | S::Func(params, ret) => {
            Type::Fn(
                params.iter().map(|param| rssn_type(param, infer)).collect(),
                Box::new(rssn_type(ret, infer)),
            )
        },
        | S::Table | S::Device | S::Opaque(_) | S::Var(_) | S::Variant(_) => dynamic_value_type(),
        | S::Dyn | S::Never => Type::Dyn(Rep::Real),
    }
}

impl Checker {
    /// Types `Rssn.<name>` in *value* position.
    ///
    /// An overloaded name is not a first-class value: unless every
    /// overload has the same type, using it without a call is `E0303`.
    fn rssn_member(
        &mut self,
        name: &str,
        span: Span,
    ) -> Type {
        match name {
            | "Version" | "ManifestHash" => return Type::Str,
            | "Capabilities" | "Units" | "Constants" | "ValueKinds" => {
                return Type::tensor(Shape::Rank(1), Type::Str);
            },
            | "call" | "dynamic" => {
                return Type::Fn(
                    vec![Type::Str, value_list_type()],
                    Box::new(Type::Adt(
                        adt::RESULT_ID,
                        vec![dynamic_value_type(), op_error_type()],
                    )),
                );
            },
            | "call_key" => {
                return Type::Fn(
                    vec![Type::Str, Type::Str, value_list_type()],
                    Box::new(Type::Adt(
                        adt::RESULT_ID,
                        vec![dynamic_value_type(), op_error_type()],
                    )),
                );
            },
            | "invoke" => {
                // As a value `invoke` is a dynamic call: its result type is
                // only known per call site, never once for the module.
                return Type::Fn(
                    vec![Type::Str, Type::Str, value_list_type()],
                    Box::new(dynamic_value_type()),
                );
            },
            | "resolve_overload" => {
                return Type::Fn(
                    vec![Type::Str, value_list_type()],
                    Box::new(Type::Adt(adt::RESULT_ID, vec![Type::Str, op_error_type()])),
                );
            },
            | "overloads" => {
                return Type::Fn(
                    vec![Type::Str],
                    Box::new(Type::tensor(Shape::Rank(1), Type::Str)),
                );
            },
            // §8.19: the host-extension entry points. Their arguments and
            // results are opaque host values (`TypeRep`, `Callback`, `Plugin`),
            // so the honest static type names those types and leaves the rest
            // to the runtime, which returns `Err(OpError)` when the capability
            // is missing.
            | "typed" => {
                return Type::Fn(
                    vec![Type::Str, value_list_type(), opaque_type("TypeRep")],
                    Box::new(Type::Adt(
                        adt::RESULT_ID,
                        vec![dynamic_value_type(), op_error_type()],
                    )),
                );
            },
            | "call_ho_key" => {
                return Type::func(
                    vec![
                        Type::Str,
                        Type::Str,
                        value_list_type(),
                        Type::tensor(Shape::Rank(1), opaque_type("Callback")),
                    ],
                    Type::Adt(adt::RESULT_ID, vec![dynamic_value_type(), op_error_type()]),
                );
            },
            | "call_ho" => {
                return Type::Fn(
                    vec![
                        Type::Str,
                        value_list_type(),
                        Type::tensor(Shape::Rank(1), opaque_type("Callback")),
                    ],
                    Box::new(Type::Adt(
                        adt::RESULT_ID,
                        vec![dynamic_value_type(), op_error_type()],
                    )),
                );
            },
            | "register" | "register_plugin" => {
                return Type::Fn(
                    vec![opaque_type("Plugin")],
                    Box::new(Type::Adt(
                        adt::RESULT_ID,
                        vec![opaque_type("PluginId"), opaque_type("PluginError")],
                    )),
                );
            },
            | _ => {},
        }
        let Some(entry) = aevia_rssn::registry().op(name) else {
            self.error_help(
                "E0301",
                format!("rssn has no operator `{name}`"),
                span,
                "run `aevia rssn --audit` for the operators this build provides",
            );
            return Type::Error;
        };
        if entry.signatures.is_empty() {
            return Type::Fn(vec![value_list_type()], Box::new(dynamic_value_type()));
        }
        let types: Vec<Type> = entry
            .signatures
            .iter()
            .map(|signature| self.rssn_signature_type(signature))
            .collect();
        let first = types.first().cloned().unwrap_or(Type::Error);
        if types.iter().all(|ty| *ty == first) {
            return first;
        }
        self.error_help(
            "E0303",
            format!("`Rssn.{name}` is overloaded and cannot be used as a value"),
            span,
            "call it, or bind the result of a call to a name",
        );
        Type::Error
    }

    /// Types a call to `Rssn.<op>` by resolving the overload at the call site.
    fn rssn_call(
        &mut self,
        name: &str,
        args: &[Expr],
        span: Span,
    ) -> Type {
        let Some(entry) = aevia_rssn::registry().op(name) else {
            for arg in args {
                self.synth(arg);
            }
            self.error_help(
                "E0301",
                format!("rssn has no operator `{name}`"),
                span,
                "run `aevia rssn --audit` for the operators this build provides",
            );
            return Type::Error;
        };
        let mut arg_types = Vec::new();
        let mut classified = Vec::new();
        for arg in args {
            let ty = self.synth(arg);
            let resolved = self.infer.resolve(&ty);
            classified.push(self.classify_arg(&resolved));
            arg_types.push(resolved);
        }
        match crate::rssn_resolve::resolve(entry, &classified) {
            | crate::rssn_resolve::Resolution::Unique { index, .. } => {
                let Some(signature) = entry.signatures.get(index) else {
                    return Type::Error;
                };
                let ret = if entry.source.is_exact() {
                    if entry
                        .raw
                        .get(index)
                        .is_some_and(aevia_rssn::registry::signature_has_unrepresentable_dimensions)
                    {
                        self.error_help(
                            "E1001",
                            format!("`Rssn.{name}` has dimensions this checker cannot represent"),
                            span,
                            "use `Rssn.dynamic` and inspect the kernel result",
                        );
                        return Type::Error;
                    }
                    match crate::rssn_resolve::instantiate_dimensions(
                        signature,
                        &arg_types,
                        &mut self.infer,
                    ) {
                        | Ok(Some(ret)) => ret,
                        | Ok(None) => Type::Dyn(Rep::Real),
                        | Err(error) => {
                            self.report_unify(error, span);
                            return Type::Error;
                        },
                    }
                } else {
                    // A derived signature identifies a dispatch form, not the
                    // dimensional relation of the actual kernel result.
                    Type::Dyn(Rep::Real)
                };
                if let Some(key) = entry.key(index) {
                    self.checked.rssn_calls.push(RssnCall {
                        span,
                        name: name.to_string(),
                        key: key.clone(),
                        overload: index,
                    });
                }
                ret
            },
            | crate::rssn_resolve::Resolution::Ambiguous { candidates } => {
                let rendered: Vec<String> = candidates
                    .iter()
                    .filter_map(|index| entry.signatures.get(*index))
                    .map(aevia_rssn::Sig::render)
                    .collect();
                self.diagnostics.push(
                    Diagnostic::error(
                        "E1002",
                        format!("`Rssn.{name}` is ambiguous for these arguments"),
                        span,
                    )
                    .with_primary_label("several overloads match equally well")
                    .with_note(format!("candidates: {}", rendered.join("; ")))
                    .with_help("annotate an argument so one overload wins"),
                );
                Type::Error
            },
            | crate::rssn_resolve::Resolution::None => {
                let rendered: Vec<String> = entry
                    .signatures
                    .iter()
                    .map(aevia_rssn::Sig::render)
                    .collect();
                self.diagnostics.push(
                    Diagnostic::error(
                        "E1003",
                        format!("no overload of `Rssn.{name}` accepts these arguments"),
                        span,
                    )
                    .with_primary_label("no candidate accepts this argument list")
                    .with_note(format!("overloads: {}", rendered.join("; ")))
                    .with_help("use `Rssn.dynamic` for a signature-free call"),
                );
                Type::Error
            },
            | crate::rssn_resolve::Resolution::Unknown => {
                self.error("E0301", format!("rssn has no operator `{name}`"), span);
                Type::Error
            },
        }
    }

    /// Classifies an argument for overload resolution.
    ///
    /// A literal that has not been committed yet is classified by the
    /// representation it defaults to: `sin(0.5)` must pick the numeric
    /// overload, not tie with the symbolic one.
    fn classify_arg(
        &self,
        ty: &Type,
    ) -> aevia_rssn::SigType {
        match ty {
            | Type::Var(_) => {
                match self.infer.literal_kind(ty) {
                    | Some(kind) => {
                        crate::rssn_resolve::classify(&Type::Scalar(
                            Rep::default_for_literal(kind),
                            Dim::one(),
                        ))
                    },
                    | None => aevia_rssn::SigType::Dyn,
                }
            },
            | Type::Scalar(Rep::Var(id), dim) => {
                match self.infer.rep_literal_kind(Rep::Var(*id)) {
                    | Some(kind) => {
                        crate::rssn_resolve::classify(&Type::Scalar(
                            Rep::default_for_literal(kind),
                            dim.clone(),
                        ))
                    },
                    | None => aevia_rssn::SigType::Dyn,
                }
            },
            | other => crate::rssn_resolve::classify(other),
        }
    }

    /// The Aevia type of one manifest signature.
    fn rssn_signature_type(
        &mut self,
        signature: &aevia_rssn::Sig,
    ) -> Type {
        let mut params: Vec<Type> = Vec::with_capacity(signature.params.len() + 1);
        for param in &signature.params {
            params.push(rssn_type(param, &mut self.infer));
        }
        if signature.variadic.is_some() {
            params.push(Type::tensor(Shape::Rank(1), dynamic_value_type()));
        }
        Type::Fn(params, Box::new(rssn_type(&signature.ret, &mut self.infer)))
    }

    /// Types a call to a class method and solves its constraints.
    fn class_method_call(
        &mut self,
        name: &str,
        scheme: &Scheme,
        args: &[Expr],
        span: Span,
    ) -> Type {
        let (ty, constraints) = self.infer.instantiate_scheme(scheme);
        let ret = self.call_with_type(ty, args, span);
        for constraint in &constraints {
            self.solve_constraint(constraint, span);
            if let Some(arg) = constraint.args.first() {
                let head = self.infer.resolve(arg);
                let target = self.dict_target(&constraint.class, name, &head);
                self.checked.dictionaries.push(DictionaryUse {
                    span,
                    class: constraint.class.clone(),
                    head,
                    method: name.to_string(),
                    target,
                });
            }
        }
        ret
    }

    /// What a class-method call site should execute (round-2 decision 1c).
    ///
    /// * a concrete head with a matching instance → that instance's key;
    /// * a structural `Show` head → the builtin printer (the guarantee);
    /// * a quantified head → the witnessable candidate instances, in
    ///   registration order. Instances without a runtime witness cannot be
    ///   dispatched this way and are left to the runtime's existing shape
    ///   dispatch until Phase B pins them; the checker has already rejected
    ///   colliding witnesses at registration (`E0311`).
    fn dict_target(
        &self,
        class: &str,
        method: &str,
        head: &Type,
    ) -> DictTarget {
        let _ = method;
        if let Some(instance) = self.classes.resolve_instance(class, head) {
            if instance.origin == Origin::Derived {
                return DictTarget::Builtin(BuiltinDispatch::Derived);
            }
            return DictTarget::Instance(instance.key.clone());
        }
        if is_structural_show(class, head) {
            return DictTarget::Builtin(BuiltinDispatch::ShowStructural);
        }
        let instances = self.classes.instances_of(class);
        let candidates = instances
            .iter()
            .map(|instance| instance.key.clone())
            .collect();
        let derived = instances
            .iter()
            .filter(|instance| instance.origin == Origin::Derived)
            .map(|instance| (instance.key.clone(), instance.head.clone()))
            .collect();
        DictTarget::Witness { candidates, derived }
    }
}

fn shape_items(shape: &Shape) -> Vec<Option<Nat>> {
    match shape {
        | Shape::Rank(rank) => vec![None; usize::try_from(*rank).unwrap_or(0)],
        | Shape::Refined(items) => items.clone(),
    }
}

#[cfg(test)]
mod literal_region_tests {
    use super::*;

    #[test]
    fn a_speculative_record_at_the_same_span_restores_previous_evidence() {
        let mut checker = Checker::new(0, CheckOptions::default());
        let span = Span::new(0, 1, 2);
        checker.note_type(span, &Type::Bool);
        let checkpoint = checker.expr_types.len();
        checker.note_type(span, &Type::int());
        assert_eq!(checker.latest_expr_type(span), Some(&Type::int()));
        checker.truncate_expr_types(checkpoint);
        assert_eq!(checker.latest_expr_type(span), Some(&Type::Bool));
    }

    #[test]
    fn composite_literal_domains_do_not_generalize_constructed_data() {
        let source = "let value = 1 + 2\nlet first: Int = value\nlet invalid: Float = value\n";
        let lexed = aevia_syntax::lex(0, source).unwrap();
        let (module, diagnostics) = aevia_syntax::parse_module(0, source, &lexed);
        assert!(!diagnostics.has_errors());
        let result = check_module(0, &module);
        assert!(
            result
                .diagnostics
                .items
                .iter()
                .any(|diagnostic| diagnostic.code == "E0302")
        );
    }

    #[test]
    fn literal_chains_keep_bounded_constraints_and_check_on_a_small_stack() {
        std::thread::Builder::new()
            .stack_size(16 * 1024 * 1024)
            .spawn(|| {
                for count in [64, 512, 4096] {
                    for (first, expected) in [("1", Type::int()), ("1 / 3", Type::real(Dim::one()))]
                    {
                        let source = format!("let result = {first}{}\n", " + 1".repeat(count));
                        let lexed = aevia_syntax::lex(0, &source).unwrap();
                        let (module, diagnostics) = aevia_syntax::parse_module(0, &source, &lexed);
                        assert!(!diagnostics.has_errors(), "{:?}", diagnostics.items);
                        let mut checker = Checker::new(0, CheckOptions::default());
                        checker.run(&module);
                        assert!(
                            !checker.diagnostics.has_errors(),
                            "{:?}",
                            checker.diagnostics.items
                        );
                        assert!(
                            checker.infer.numeric_constraint_count() <= 4,
                            "{count} literals generated {} numeric constraints",
                            checker.infer.numeric_constraint_count()
                        );
                        assert!(checker.infer.numeric_constraint_peak() <= 4);
                        assert!(checker.literal_classes.len() <= 2 * count + 6);
                        let Some(Export::Value { scheme, .. }) =
                            checker.checked.exports.get("result")
                        else {
                            panic!("missing result")
                        };
                        assert_eq!(scheme.ty, expected);
                        assert!(checker.checked.expr_types.len() >= count);
                    }
                }
            })
            .unwrap()
            .join()
            .unwrap();
    }
    #[test]
    fn mixed_literal_spines_are_iterative_and_keep_separate_domains() {
        std::thread::Builder::new()
            .stack_size(16 * 1024 * 1024)
            .spawn(|| {
                for count in [64, 512, 4096] {
                    for first in ["1 + 1.0", "(1.0 + 2.0)"] {
                        let source = format!("let result = {first}{}\n", " + 1".repeat(count));
                        let lexed = aevia_syntax::lex(0, &source).unwrap();
                        let (module, diagnostics) = aevia_syntax::parse_module(0, &source, &lexed);
                        assert!(!diagnostics.has_errors());
                        let mut checker = Checker::new(0, CheckOptions::default());
                        checker.run(&module);
                        assert!(
                            !checker.diagnostics.has_errors(),
                            "{:?}",
                            checker.diagnostics.items
                        );
                        assert!(
                            checker.infer.numeric_constraint_peak() <= 6,
                            "mixed {count}: peak {}",
                            checker.infer.numeric_constraint_peak()
                        );
                        let Some(Export::Value { scheme, .. }) =
                            checker.checked.exports.get("result")
                        else {
                            panic!("missing result")
                        };
                        // §3.10.2: `join(Int, Float) = Float` for both
                        // spellings; exactness of the closed tree is a value
                        // property (constant-region folding), not a type.
                        assert_eq!(scheme.ty, Type::float(Dim::one()));
                    }
                }
            })
            .unwrap()
            .join()
            .unwrap();
    }

    #[test]
    fn independent_declarations_visit_only_their_own_literal_records() {
        for count in [64, 512, 2048] {
            let source = (0..count)
                .map(|index| format!("fn f{index}() = 1\n"))
                .collect::<String>();
            let lexed = aevia_syntax::lex(0, &source).unwrap();
            let (module, diagnostics) = aevia_syntax::parse_module(0, &source, &lexed);
            assert!(!diagnostics.has_errors());
            let mut checker = Checker::new(0, CheckOptions::default());
            checker.run(&module);
            assert!(
                !checker.diagnostics.has_errors(),
                "{:?}",
                checker.diagnostics.items
            );
            assert_eq!(checker.checked.functions.len(), count);
            assert!(checker.literal_owner_visits.get() <= 4 * count);
            assert!(checker.infer.numeric_constraint_peak() <= 2);
        }
    }
    #[test]
    fn independent_generic_declarations_project_only_their_numeric_components() {
        for body in ["x + 1", "x + 1.0"] {
            for count in [16, 32, 64, 128] {
                let source = (0..count)
                    .map(|index| format!("fn f{index}(x) = {body}\n"))
                    .collect::<String>();
                let tokens = aevia_syntax::lex(0, &source).unwrap();
                let (module, diagnostics) = aevia_syntax::parse_module(0, &source, &tokens);
                assert!(!diagnostics.has_errors());
                let mut checker = Checker::new(0, CheckOptions::default());
                let started = std::time::Instant::now();
                checker.run(&module);
                assert!(
                    !checker.diagnostics.has_errors(),
                    "{:?}",
                    checker.diagnostics.items
                );
                let work = checker.infer.numeric_constraint_work();
                eprintln!(
                    "generic scaling body={body:?} count={count} facts={} component_visits={work} elapsed={:?}",
                    checker.infer.numeric_constraint_count(),
                    started.elapsed()
                );
                assert_eq!(checker.checked.functions.len(), count);
                assert!(
                    work <= 256 * count,
                    "{count} declarations revisited {work} facts"
                );
                for function in checker.checked.functions.values() {
                    assert_eq!(function.scheme.numeric.len(), 3);
                }
            }
        }
    }
}

pub(crate) fn instantiate_pattern_field(
    id: adt::AdtId,
    field: &Type,
    arguments: &[Type],
) -> Type {
    let mut types = Vec::new();
    let mut dims = HashMap::new();
    for (kind, argument) in adt::param_kinds_of(id).iter().zip(arguments) {
        match (kind, argument) {
            | (adt::ParamKind::Type, value) => types.push(Some(value.clone())),
            | (adt::ParamKind::Dim(id), Type::UnitOf(dim)) => {
                types.push(None);
                dims.insert(*id, dim.clone());
            },
            | _ => types.push(None),
        }
    }
    field.substitute_kinded(&types, &dims, &HashMap::new(), &HashMap::new())
}

/// Help for an operand that is a local value spelled like a unit (§5.1.3).
fn shadowed_unit_help(name: &str) -> String {
    format!(
        "`{name}` here is the local binding in scope, not the unit: in an expression a value shadows a unit name (§5.1.3). Rename the binding, or write the unit in a literal (`1.0 {name}`), where unit names are never shadowed"
    )
}

/// The candidate closest to a misspelt `name`, for a "did you mean" help.
///
/// Distance is optimal string alignment over characters (a transposition
/// costs one), and the accepted distance grows with the name: one edit for
/// names of up to three characters, two up to six, three beyond. Internal
/// names (mangled instance globals, generated helpers) are never offered.
/// Ties go to the lexicographically smallest candidate, so the help text is
/// deterministic.
#[must_use]
pub fn closest_name<'a>(
    name: &str,
    candidates: impl IntoIterator<Item = &'a str>,
) -> Option<&'a str> {
    let target: Vec<char> = name.chars().collect();
    let limit = match target.len() {
        | 0 => return None,
        | 1..=3 => 1,
        | 4..=6 => 2,
        | _ => 3,
    };
    let mut best: Option<(usize, &'a str)> = None;
    for candidate in candidates {
        if candidate == name
            || candidate.is_empty()
            || candidate.contains([':', '@', '$', '#'])
            || candidate.starts_with("__")
        {
            continue;
        }
        let other: Vec<char> = candidate.chars().collect();
        if other.len().abs_diff(target.len()) > limit {
            continue;
        }
        let distance = osa_distance(&target, &other);
        if distance > limit || distance >= other.len() {
            continue;
        }
        // Beyond one edit the first letter must agree (ignoring case): a
        // reader misspells the middle of a name, and `Normal` -> `format`
        // is two edits away but no help at all.
        if distance > 1 && !target[0].to_lowercase().eq(other[0].to_lowercase()) {
            continue;
        }
        let better = match best {
            | None => true,
            | Some((d, current)) => distance < d || (distance == d && candidate < current),
        };
        if better {
            best = Some((distance, candidate));
        }
    }
    best.map(|(_, candidate)| candidate)
}

/// Whether `short` can be obtained from `long` by deleting characters.
fn is_subsequence(
    short: &[char],
    long: &[char],
) -> bool {
    let mut rest = long.iter();
    short
        .iter()
        .all(|wanted| rest.by_ref().any(|have| have == wanted))
}

/// The unit spellings a reader is most likely to have meant; they win ties
/// between equally distant candidates.
const COMMON_UNITS: &[&str] = &[
    "m", "s", "g", "kg", "A", "K", "mol", "cd", "Hz", "N", "Pa", "J", "W", "C", "V", "F", "Ω", "S",
    "T", "L", "min", "h", "d", "rad", "deg", "eV", "bar", "atm", "ft", "in", "mi", "lb", "°C",
];

/// The unit spelling closest to a misspelt `name`.
///
/// Like [`closest_name`] (same distance and first-letter rules), but
/// equally distant candidates are ranked by what a reader most plausibly
/// meant instead of alphabetically: first the *containment-minimal* edit (the
/// candidate is `name` with characters deleted, or `name` with characters
/// added — `kmm` is `km` plus a stray letter, not `klm` with a wrong one),
/// then a common unit, then alphabetical order. A pure deletion is accepted
/// even when it leaves a single character (`mm` -> `m`).
#[must_use]
pub fn closest_unit<'a>(
    name: &str,
    candidates: impl IntoIterator<Item = &'a str>,
) -> Option<&'a str> {
    let target: Vec<char> = name.chars().collect();
    let limit = match target.len() {
        | 0 => return None,
        | 1..=3 => 1,
        | 4..=6 => 2,
        | _ => 3,
    };
    let mut best: Option<((usize, bool, bool), &'a str)> = None;
    for candidate in candidates {
        if candidate == name || candidate.is_empty() || candidate.contains([':', '@', '$', '#']) {
            continue;
        }
        let other: Vec<char> = candidate.chars().collect();
        if other.len().abs_diff(target.len()) > limit {
            continue;
        }
        let contained = is_subsequence(&other, &target) || is_subsequence(&target, &other);
        let distance = osa_distance(&target, &other);
        if distance > limit || (distance >= other.len() && !contained) {
            continue;
        }
        if distance > 1 && !target[0].to_lowercase().eq(other[0].to_lowercase()) {
            continue;
        }
        let key = (distance, !contained, !COMMON_UNITS.contains(&candidate));
        let better = match best {
            | None => true,
            | Some((current_key, current)) => {
                key < current_key || (key == current_key && candidate < current)
            },
        };
        if better {
            best = Some((key, candidate));
        }
    }
    best.map(|(_, candidate)| candidate)
}

fn osa_distance(
    a: &[char],
    b: &[char],
) -> usize {
    let width = b.len() + 1;
    let mut table = vec![0usize; (a.len() + 1) * width];
    for (i, cell) in table.iter_mut().step_by(width).enumerate() {
        *cell = i;
    }
    for (j, cell) in table.iter_mut().take(width).enumerate() {
        *cell = j;
    }
    for i in 1..=a.len() {
        for j in 1..=b.len() {
            let cost = usize::from(a[i - 1] != b[j - 1]);
            let mut value = (table[(i - 1) * width + j] + 1)
                .min(table[i * width + j - 1] + 1)
                .min(table[(i - 1) * width + j - 1] + cost);
            if i > 1 && j > 1 && a[i - 1] == b[j - 2] && a[i - 2] == b[j - 1] {
                value = value.min(table[(i - 2) * width + j - 2] + 1);
            }
            table[i * width + j] = value;
        }
    }
    table[a.len() * width + b.len()]
}

#[cfg(test)]
mod suggestion_tests {
    use super::closest_name;
    use super::closest_unit;

    #[test]
    fn a_stray_letter_is_deleted_rather_than_substituted() {
        // `kmm` is `km` with an extra `m`; `klm` (kilolumen) is a wrong
        // letter, and used to win alphabetically.
        let bases = ["m", "lm", "mi", "s"];
        assert_eq!(closest_unit("mm", bases), Some("m"));
        assert_eq!(closest_unit("mm", ["lm", "m"]), Some("m"));
        assert_eq!(closest_unit("kmm", ["klm", "km", "kg"]), Some("km"));
        // Equally good and equally contained: a common unit beats an
        // alphabetically earlier odd one.
        assert_eq!(closest_unit("hh", ["ah", "h"]), Some("h"));
        assert_eq!(closest_unit("kgg", ["kg", "kgg2"]), Some("kg"));
        assert_eq!(closest_unit("secs", ["sec", "s", "secs2"]), Some("sec"));
        // Nothing near stays nothing.
        assert_eq!(closest_unit("qqqq", ["m", "s"]), None);
    }

    #[test]
    fn close_names_are_suggested_and_far_ones_are_not() {
        let names = ["println", "print", "length", "linspace", "sqrt"];
        assert_eq!(closest_name("pritnln", names), Some("println"));
        assert_eq!(closest_name("lenght", names), Some("length"));
        assert_eq!(closest_name("sqtr", names), Some("sqrt"));
        assert_eq!(closest_name("zzz", names), None);
        assert_eq!(closest_name("x", ["y"]), None);
        assert_eq!(
            closest_name("value", ["valeu", "Show@Int::show"]),
            Some("valeu")
        );
    }
}
