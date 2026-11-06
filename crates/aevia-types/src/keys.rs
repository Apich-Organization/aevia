//! Stable, symbolic identity for dispatch and for the module interface.
//!
//! Round-2 decision 4 (`.workbuddy-ai/design/round2-design-register.md`): the
//! checker, the interface file and the runtime share one nominal key scheme.
//!
//! * [`ClassKey`] is `(defining module, class name)`.
//! * [`AdtKey`] is `(defining module, type name)` — **never** an `AdtId`, which
//!   is a process-global witness that changes between runs, and never rendered
//!   text.
//! * [`InstanceKey`] is `(defining module, class key, canonical head key)`.
//! * [`HeadKey`] is the structural canonical form of a head (or any type used as
//!   one). Aliases are transparent because the checker expands them before
//!   asking for a key.
//! * [`Witness`] is what the runtime can actually observe about a value. A
//!   quantified call site may dispatch by witness only when the candidate
//!   instances are pairwise disjoint; otherwise the program is rejected
//!   (`E0311`/`E0310`) rather than guessed at.
//!
//! The canonical key is also what an interface file may persist; `AdtId` never
//! crosses a module boundary or a process boundary.

use crate::adt;
use crate::ty::Nat;
use crate::ty::Rep;
use crate::ty::Row;
use crate::ty::Shape;
use crate::ty::Type;
use std::fmt;

/// A module path, `[]` for the prelude/builtin scope, `["A", "B"]` for `A.B`.
#[derive(Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Debug, Default)]
pub struct ModulePath(pub Vec<String>, Option<ModuleOrigin>);

/// The source identity of a module in a path package. Lookup aliases and
/// declared names are deliberately absent: neither may merge nominal types.
#[derive(Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
pub struct ModuleOrigin {
    /// Canonical package directory, including its declared name and version.
    pub package: String,
    /// Canonical source file relative to the package directory.
    pub source: String,
}

impl ModulePath {
    /// The prelude/builtin module.
    #[must_use]
    pub fn prelude() -> Self {
        Self(Vec::new(), None)
    }

    /// A module from its dotted segments.
    #[must_use]
    pub fn new(segments: impl IntoIterator<Item = impl Into<String>>) -> Self {
        Self(segments.into_iter().map(Into::into).collect(), None)
    }

    /// A visible module path with an independent nominal source identity.
    #[must_use]
    pub fn with_origin(
        mut self,
        origin: ModuleOrigin,
    ) -> Self {
        self.1 = Some(origin);
        self
    }

    /// The package source identity, when supplied by a package-aware driver.
    #[must_use]
    pub fn origin(&self) -> Option<&ModuleOrigin> {
        self.1.as_ref()
    }

    /// Canonical owner text for source-owned dimension generators. Every
    /// component is length-framed; display/import aliases never participate.
    #[must_use]
    pub fn identity_key(&self) -> String {
        fn part(
            text: &mut String,
            value: &str,
        ) {
            text.push_str(&value.len().to_string());
            text.push(':');
            text.push_str(value);
        }
        let mut text = "aevia-module-v1:".to_string();
        part(&mut text, &self.0.len().to_string());
        for component in &self.0 {
            part(&mut text, component);
        }
        if let Some(origin) = self.origin() {
            part(&mut text, "origin");
            part(&mut text, &origin.package);
            part(&mut text, &origin.source);
        } else {
            part(&mut text, "no-origin");
        }
        text
    }

    /// Whether this is the prelude scope.
    #[must_use]
    pub fn is_prelude(&self) -> bool {
        self.0.is_empty() && self.1.is_none()
    }

    /// The dotted spelling, `"<prelude>"` for the prelude.
    #[must_use]
    pub fn dotted(&self) -> String {
        if self.0.is_empty() {
            "<prelude>".to_string()
        } else {
            self.0.join(".")
        }
    }
}

impl fmt::Display for ModulePath {
    fn fmt(
        &self,
        f: &mut fmt::Formatter<'_>,
    ) -> fmt::Result {
        f.write_str(&self.dotted())
    }
}

/// `(defining module, type name)` — the nominal identity of a type.
#[derive(Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
pub struct AdtKey {
    /// The complete declaration schema, when this is a finalized source type.
    pub schema: Option<crate::DeclarationSchemaKey>,
    /// The module that declares the type.
    pub module: ModulePath,
    /// The declared name.
    pub name: String,
}

impl AdtKey {
    /// A key from its parts.
    #[must_use]
    pub fn new(
        module: ModulePath,
        name: impl Into<String>,
    ) -> Self {
        Self {
            schema: None,
            module,
            name: name.into(),
        }
    }
}

impl fmt::Display for AdtKey {
    fn fmt(
        &self,
        f: &mut fmt::Formatter<'_>,
    ) -> fmt::Result {
        if self.module.is_prelude() {
            f.write_str(&self.name)
        } else {
            write!(f, "{}.{}", self.module, self.name)
        }
    }
}

/// `(defining module, class name)`.
#[derive(Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
pub struct ClassKey {
    /// The module that declares the class.
    pub module: ModulePath,
    /// The declared name.
    pub name: String,
}

impl ClassKey {
    /// A key from its parts.
    #[must_use]
    pub fn new(
        module: ModulePath,
        name: impl Into<String>,
    ) -> Self {
        Self {
            module,
            name: name.into(),
        }
    }

    /// A prelude class key, for the built-in classes of §3.8.4.
    #[must_use]
    pub fn prelude(name: impl Into<String>) -> Self {
        Self {
            module: ModulePath::prelude(),
            name: name.into(),
        }
    }
}

impl fmt::Display for ClassKey {
    fn fmt(
        &self,
        f: &mut fmt::Formatter<'_>,
    ) -> fmt::Result {
        if self.module.is_prelude() {
            f.write_str(&self.name)
        } else {
            write!(f, "{}.{}", self.module, self.name)
        }
    }
}

/// A representation, spelled out so the key does not depend on `Rep`'s internal
/// numbering.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
pub enum RepKind {
    /// Arbitrary-precision integer.
    Int,
    /// Exact rational / symbolic.
    Real,
    /// IEEE-754 binary64.
    Float,
    /// Complex.
    Complex,
}

impl RepKind {
    /// The kind of a concrete representation; `None` for a variable.
    #[must_use]
    pub fn of(rep: Rep) -> Option<Self> {
        match rep {
            | Rep::Int => Some(Self::Int),
            | Rep::Real => Some(Self::Real),
            | Rep::Float => Some(Self::Float),
            | Rep::Complex => Some(Self::Complex),
            | Rep::Var(_) => None,
        }
    }
}

impl fmt::Display for RepKind {
    fn fmt(
        &self,
        f: &mut fmt::Formatter<'_>,
    ) -> fmt::Result {
        f.write_str(match self {
            | Self::Int => "Int",
            | Self::Real => "Real",
            | Self::Float => "Float",
            | Self::Complex => "Complex",
        })
    }
}

/// The storage class a tensor value reports at run time.
///
/// This is deliberately coarser than the element representation: an exact `Int`
/// tensor and an exact `Real` tensor are *both* exact storage, so a runtime
/// witness cannot tell `Tensor<1, Int>` from `Tensor<1, Real>`. Two instances
/// whose witnesses collide are rejected (`E0311`) instead of guessed at.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
pub enum StorageClass {
    /// Exact rational elements (`Int` and `Real`).
    Exact,
    /// IEEE-754 binary64 elements.
    Float,
    /// Complex elements.
    Complex,
}

impl StorageClass {
    /// The storage class of an element representation.
    #[must_use]
    pub fn of(rep: Rep) -> Option<Self> {
        match rep {
            | Rep::Int | Rep::Real => Some(Self::Exact),
            | Rep::Float => Some(Self::Float),
            | Rep::Complex => Some(Self::Complex),
            | Rep::Var(_) => None,
        }
    }
}

impl fmt::Display for StorageClass {
    fn fmt(
        &self,
        f: &mut fmt::Formatter<'_>,
    ) -> fmt::Result {
        f.write_str(match self {
            | Self::Exact => "Exact",
            | Self::Float => "Float",
            | Self::Complex => "Complex",
        })
    }
}

/// The canonical key of a type used as an instance head.
///
/// `Wildcard` marks a position the instance leaves open (`instance C for
/// Option<a>`): a wildcard head cannot be dispatched by exact key, only by
/// witness.
#[derive(Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
pub enum HeadKey {
    /// A concrete scalar representation.
    Scalar(RepKind),
    /// A scalar whose representation is a variable.
    ScalarVar,
    /// `Bool`.
    Bool,
    /// `String`.
    String,
    /// `()`.
    Unit,
    /// `Prop`.
    Prop,
    /// `Point[d]` — the affine point constructor.
    Point,
    /// `Unit[d]` — the unit value constructor.
    UnitOf,
    /// `Tensor<shape, τ>`; the shape keeps its rank and known extents.
    Tensor {
        /// The rank, or the refined shape.
        shape: Vec<Option<usize>>,
        /// The element key.
        elem: Box<HeadKey>,
    },
    /// A nominal type, with its structural arguments.
    Adt(AdtKey, Vec<HeadKey>),
    /// A record's canonical field names and types.
    Record(Vec<(String, HeadKey)>),
    /// A function type: no instance head may dispatch it.
    Fn,
    /// `Dyn<ρ>`.
    Dyn,
    /// `IO τ`.
    Io(Box<HeadKey>),
    /// An open position (`a` in `instance C for Option<a>`).
    Wildcard,
}

impl fmt::Display for HeadKey {
    fn fmt(
        &self,
        f: &mut fmt::Formatter<'_>,
    ) -> fmt::Result {
        match self {
            | Self::Scalar(rep) => write!(f, "{rep}"),
            | Self::ScalarVar => f.write_str("<scalar>"),
            | Self::Bool => f.write_str("Bool"),
            | Self::String => f.write_str("String"),
            | Self::Unit => f.write_str("()"),
            | Self::Prop => f.write_str("Prop"),
            | Self::Point => f.write_str("Point"),
            | Self::UnitOf => f.write_str("Unit"),
            | Self::Tensor { shape, elem } => {
                f.write_str("Tensor<")?;
                f.write_str("[")?;
                for (index, extent) in shape.iter().enumerate() {
                    if index > 0 {
                        f.write_str(", ")?;
                    }
                    match extent {
                        | Some(extent) => write!(f, "{extent}")?,
                        | None => f.write_str("?")?,
                    }
                }
                f.write_str("], ")?;
                write!(f, "{elem}>")
            },
            | Self::Adt(key, args) => {
                write!(f, "{key}")?;
                if !args.is_empty() {
                    f.write_str("<")?;
                    for (index, arg) in args.iter().enumerate() {
                        if index > 0 {
                            f.write_str(", ")?;
                        }
                        write!(f, "{arg}")?;
                    }
                    f.write_str(">")?;
                }
                Ok(())
            },
            | Self::Record(labels) => {
                f.write_str("{")?;
                for (index, (label, ty)) in labels.iter().enumerate() {
                    if index > 0 {
                        f.write_str(", ")?;
                    }
                    write!(f, "{label}: {ty}")?;
                }
                f.write_str("}")
            },
            | Self::Fn => f.write_str("->"),
            | Self::Dyn => f.write_str("Dyn"),
            | Self::Io(inner) => write!(f, "IO {inner}"),
            | Self::Wildcard => f.write_str("_"),
        }
    }
}

impl HeadKey {
    /// Whether the head is fully concrete, so an instance can be selected by
    /// exact key rather than by runtime witness.
    #[must_use]
    pub fn is_concrete(&self) -> bool {
        match self {
            | Self::ScalarVar | Self::Wildcard => false,
            | Self::Tensor { shape, elem } => {
                shape.iter().all(Option::is_some) && elem.is_concrete()
            },
            | Self::Adt(_, args) => args.iter().all(Self::is_concrete),
            | Self::Record(fields) => fields.iter().all(|(_, ty)| ty.is_concrete()),
            | Self::Io(inner) => inner.is_concrete(),
            | Self::Scalar(_)
            | Self::Bool
            | Self::String
            | Self::Unit
            | Self::Prop
            | Self::Point
            | Self::UnitOf
            | Self::Fn
            | Self::Dyn => true,
        }
    }
}

/// The canonical head key of a type (§A decision 4).
///
/// Aliases are transparent: the checker stores expanded types, so the key of
/// `Vec2` and of the record it names is the same. A type variable is
/// [`HeadKey::Wildcard`].
#[must_use]
pub fn head_key(ty: &Type) -> HeadKey {
    match ty {
        | Type::Scalar(rep, _) => RepKind::of(*rep).map_or(HeadKey::ScalarVar, HeadKey::Scalar),
        | Type::Bool => HeadKey::Bool,
        | Type::Prop => HeadKey::Prop,
        | Type::Str => HeadKey::String,
        | Type::Unit => HeadKey::Unit,
        | Type::Point(_) => HeadKey::Point,
        | Type::UnitOf(_) => HeadKey::UnitOf,
        | Type::Tensor { shape, elem } => {
            HeadKey::Tensor {
                shape: shape_items(shape),
                elem: Box::new(head_key(elem)),
            }
        },
        | Type::Record(row) => {
            let mut fields: Vec<_> = row
                .fields()
                .into_iter()
                .map(|(label, ty)| (label, head_key(&ty)))
                .collect();
            fields.sort_by(|a, b| a.0.cmp(&b.0));
            if row.tail_var().is_some() {
                fields.push(("...".to_string(), HeadKey::Wildcard));
            }
            HeadKey::Record(fields)
        },
        | Type::Adt(id, args) => {
            HeadKey::Adt(adt::key_of(*id), args.iter().map(head_key).collect())
        },
        | Type::Io(inner) => HeadKey::Io(Box::new(head_key(inner))),
        | Type::Dyn(rep) => {
            match RepKind::of(*rep) {
                | Some(_) => HeadKey::Dyn,
                | None => HeadKey::Wildcard,
            }
        },
        | Type::Fn(..)
        | Type::TensorIntrinsic(_)
        | Type::TypeRepIntrinsic(_)
        | Type::FromValueIntrinsic(_)
        | Type::ToCallbackIntrinsic => HeadKey::Fn,
        | Type::Var(_) | Type::Param(_) | Type::Error => HeadKey::Wildcard,
    }
}

fn shape_items(shape: &Shape) -> Vec<Option<usize>> {
    match shape {
        | Shape::Rank(rank) => vec![None; usize::try_from(*rank).unwrap_or(0)],
        // Only concrete extents can name a head: a shape that still has a
        // variable is `?` in the key (witness dispatch, not exact key).
        | Shape::Refined(items) => {
            items
                .iter()
                .map(|item| item.as_ref().and_then(Nat::as_const))
                .collect()
        },
    }
}

fn row_labels(row: &Row) -> Vec<String> {
    let mut labels: Vec<String> = row
        .fields()
        .into_iter()
        .map(|(label, _)| label)
        .collect::<std::collections::BTreeSet<_>>()
        .into_iter()
        .collect();
    if row.tail_var().is_some() {
        // An open record's labels are a lower bound; represent that explicitly
        // so it cannot be confused with the closed head of the same labels.
        labels.push("...".to_string());
    }
    labels
}

/// `(defining module, class key, canonical head)` — the identity of an
/// instance.
#[derive(Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
pub struct InstanceKey {
    /// The module that declares the instance.
    pub module: ModulePath,
    /// The class it instantiates.
    pub class: ClassKey,
    /// The canonical head.
    pub head: HeadKey,
}

impl InstanceKey {
    /// A key from its parts.
    #[must_use]
    pub fn new(
        module: ModulePath,
        class: ClassKey,
        head: HeadKey,
    ) -> Self {
        Self { module, class, head }
    }

    /// A deterministic, process-local mangled name for an instance body.
    ///
    /// The runtime and the checker must derive the *same* name from the same
    /// data (round-2 decision 1a): the old runtime used `Class@?` for every head
    /// that was not a nominal type, so two primitive-head instances overwrote
    /// one global. This name is built from the symbolic key, so it is unique per
    /// `(module, class, head)` and stable within a process.
    #[must_use]
    pub fn mangle(&self) -> String {
        // Display deliberately omits nominal source origins. Runtime globals
        // must include them, including origins nested in the head/class key.
        format!("instance::{self:?}")
    }
}

impl fmt::Display for InstanceKey {
    fn fmt(
        &self,
        f: &mut fmt::Formatter<'_>,
    ) -> fmt::Result {
        write!(f, "{}@{}", self.class, self.head)?;
        if !self.module.is_prelude() {
            write!(f, " [{}]", self.module)?;
        }
        Ok(())
    }
}

/// What the runtime can observe about a value, for witness dispatch.
///
/// The table is the reviewed one: ADT module+name identity, `NumKind`, tensor
/// rank+storage, canonical record labels, `Bool`/`String`/`Unit`/`Prop`.
/// Functions and `Dyn` have **no** witness, so a quantified call site that would
/// need them is rejected at check time.
#[derive(Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
pub enum Witness {
    /// A nominal type, by module+name, with the witnesses of its arguments:
    /// `Option<Int>` and `Option<String>` are told apart by their payloads.
    Adt(AdtKey, Vec<Witness>),
    /// A scalar number kind.
    Num(RepKind),
    /// A tensor: the runtime knows its rank and its storage class.
    Tensor {
        /// The number of axes.
        rank: u32,
        /// What the buffer stores.
        storage: StorageClass,
    },
    /// A record, by its canonical label set.
    Record(Vec<String>),
    /// `Bool`.
    Bool,
    /// `String`.
    String,
    /// `()`.
    Unit,
    /// `Prop`.
    Prop,
}

impl fmt::Display for Witness {
    fn fmt(
        &self,
        f: &mut fmt::Formatter<'_>,
    ) -> fmt::Result {
        match self {
            | Self::Adt(key, args) => {
                write!(f, "adt {key}")?;
                if !args.is_empty() {
                    f.write_str("<")?;
                    for (index, arg) in args.iter().enumerate() {
                        if index > 0 {
                            f.write_str(", ")?;
                        }
                        write!(f, "{arg}")?;
                    }
                    f.write_str(">")?;
                }
                Ok(())
            },
            | Self::Num(rep) => write!(f, "num {rep}"),
            | Self::Tensor { rank, storage } => write!(f, "tensor rank {rank} {storage}"),
            | Self::Record(labels) => {
                f.write_str("record {")?;
                for (index, label) in labels.iter().enumerate() {
                    if index > 0 {
                        f.write_str(", ")?;
                    }
                    f.write_str(label)?;
                }
                f.write_str("}")
            },
            | Self::Bool => f.write_str("Bool"),
            | Self::String => f.write_str("String"),
            | Self::Unit => f.write_str("()"),
            | Self::Prop => f.write_str("Prop"),
        }
    }
}

/// The witness a value of this type reports, when it has one (§A decision 1c).
#[must_use]
pub fn witness_of(ty: &Type) -> Option<Witness> {
    match ty {
        | Type::Scalar(rep, _) => RepKind::of(*rep).map(Witness::Num),
        | Type::Bool => Some(Witness::Bool),
        | Type::Prop => Some(Witness::Prop),
        | Type::Str => Some(Witness::String),
        | Type::Unit => Some(Witness::Unit),
        | Type::Tensor { shape, elem } => {
            let Type::Scalar(rep, _) = **elem else {
                return None;
            };
            Some(Witness::Tensor {
                rank: shape.rank(),
                storage: StorageClass::of(rep)?,
            })
        },
        | Type::Record(row) => Some(Witness::Record(row_labels(row))),
        | Type::Adt(id, args) => {
            let args = args
                .iter()
                .map(witness_of)
                .collect::<Option<Vec<Witness>>>()?;
            Some(Witness::Adt(adt::key_of(*id), args))
        },
        // Functions, `Dyn`, `Point`, `Unit` values, `IO` and open variables have
        // no witness the dispatcher can read.
        | Type::Fn(..)
        | Type::TensorIntrinsic(_)
        | Type::TypeRepIntrinsic(_)
        | Type::FromValueIntrinsic(_)
        | Type::ToCallbackIntrinsic
        | Type::Dyn(_)
        | Type::Point(_)
        | Type::UnitOf(_)
        | Type::Io(_)
        | Type::Var(_)
        | Type::Param(_)
        | Type::Error => None,
    }
}

/// Whether two witnesses are distinguishable at run time.
#[must_use]
pub fn witnesses_are_disjoint(
    left: &Witness,
    right: &Witness,
) -> bool {
    left != right
}

/// The first colliding pair in a candidate list, for a diagnostic.
#[must_use]
pub fn witness_collision(witnesses: &[Option<Witness>]) -> Option<(usize, usize)> {
    for (index, left) in witnesses.iter().enumerate() {
        for (other_index, right) in witnesses.iter().enumerate().skip(index + 1) {
            match (left, right) {
                | (Some(left), Some(right)) if !witnesses_are_disjoint(left, right) => {
                    return Some((index, other_index));
                },
                | _ => {},
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::adt::Ctor;
    use crate::adt::{
        self,
    };
    use aevia_dim::Dim;

    fn record(labels: &[&str]) -> Type {
        Type::record(
            labels
                .iter()
                .map(|label| ((*label).to_string(), Type::int())),
        )
    }

    fn user_adt(
        module: &str,
        name: &str,
    ) -> Type {
        let id = adt::allocate_in(ModulePath::new([module]), name, Vec::new());
        adt::set_ctors(
            id,
            vec![Ctor {
                name: format!("{name}Ctor"),
                fields: Vec::new(),
            }],
        );
        Type::Adt(id, Vec::new())
    }

    #[test]
    fn keys_are_symbolic_and_stable_for_every_head_form() {
        // No `AdtId` or rendered text enters a key: the same declaration asked
        // twice yields the same key, and two modules with the same type name
        // yield different ones.
        let first = user_adt("A", "Shape");
        let second = user_adt("A", "Shape");
        assert_eq!(head_key(&first), head_key(&second));
        let other_module = user_adt("B", "Shape");
        assert_ne!(head_key(&first), head_key(&other_module));

        assert_eq!(head_key(&Type::int()), HeadKey::Scalar(RepKind::Int));
        assert_eq!(
            head_key(&Type::real(Dim::base("Length"))),
            HeadKey::Scalar(RepKind::Real)
        );
        assert_eq!(
            head_key(&Type::float(Dim::one())),
            HeadKey::Scalar(RepKind::Float)
        );
        assert_eq!(head_key(&Type::Bool), HeadKey::Bool);
        assert_eq!(head_key(&Type::Str), HeadKey::String);
        assert_eq!(head_key(&Type::Unit), HeadKey::Unit);
        assert_eq!(head_key(&Type::Prop), HeadKey::Prop);
        assert_eq!(
            head_key(&Type::option(Type::int())),
            HeadKey::Adt(
                AdtKey::new(ModulePath::prelude(), "Option"),
                vec![HeadKey::Scalar(RepKind::Int)]
            )
        );
        // Alias heads are transparent: the checker expands them, so the record
        // key is what the instance carries.
        assert_eq!(
            head_key(&record(&["x", "y"])),
            HeadKey::Record(vec![
                ("x".to_string(), HeadKey::Scalar(RepKind::Int)),
                ("y".to_string(), HeadKey::Scalar(RepKind::Int)),
            ])
        );
        assert_eq!(
            head_key(&Type::tensor(
                Shape::refined([Some(3), Some(2)]),
                Type::int()
            )),
            HeadKey::Tensor {
                shape: vec![Some(3), Some(2)],
                elem: Box::new(HeadKey::Scalar(RepKind::Int)),
            }
        );
    }

    #[test]
    fn a_head_with_a_variable_is_not_concrete() {
        let generic = Type::option(Type::Param(0));
        let key = head_key(&generic);
        assert!(!key.is_concrete());
        assert_eq!(
            key,
            HeadKey::Adt(
                AdtKey::new(ModulePath::prelude(), "Option"),
                vec![HeadKey::Wildcard]
            )
        );
    }

    #[test]
    fn the_witness_table_covers_the_reviewed_heads() {
        assert_eq!(witness_of(&Type::int()), Some(Witness::Num(RepKind::Int)));
        // Int and Real are different NumKinds, so they can be witnesses.
        assert_ne!(
            witness_of(&Type::int()),
            witness_of(&Type::real(Dim::one()))
        );
        // ... but a tensor's storage does not distinguish them: both are exact.
        assert_eq!(
            witness_of(&Type::tensor(Shape::Rank(1), Type::int())),
            witness_of(&Type::tensor(Shape::Rank(1), Type::real(Dim::one())))
        );
        assert_ne!(
            witness_of(&Type::tensor(Shape::Rank(1), Type::int())),
            witness_of(&Type::tensor(Shape::Rank(2), Type::int()))
        );
        assert_ne!(
            witness_of(&Type::tensor(Shape::Rank(1), Type::float(Dim::one()))),
            witness_of(&Type::tensor(Shape::Rank(1), Type::int()))
        );
        assert_ne!(
            witness_of(&record(&["x", "y"])),
            witness_of(&record(&["x", "z"]))
        );
        // Functions and `Dyn` have no witness at all.
        assert_eq!(
            witness_of(&Type::func(vec![Type::int()], Type::int())),
            None
        );
        assert_eq!(witness_of(&Type::Dyn(Rep::Float)), None);
    }

    #[test]
    fn a_collision_is_found_between_instances_that_cannot_be_told_apart() {
        // `Tensor<1, Int>` and `Tensor<1, Real>` are distinct heads but one
        // storage class: dispatching between them by witness is impossible.
        let left = witness_of(&Type::tensor(Shape::Rank(1), Type::int()));
        let right = witness_of(&Type::tensor(Shape::Rank(1), Type::real(Dim::one())));
        assert_eq!(witness_collision(&[left, right]), Some((0, 1)));
        // Different ADT modules never collide.
        let a = witness_of(&user_adt("A", "Shape"));
        let b = witness_of(&user_adt("B", "Shape"));
        assert_eq!(witness_collision(&[a, b]), None);
        // Two num kinds do not collide.
        let int = witness_of(&Type::int());
        let float = witness_of(&Type::float(Dim::one()));
        assert_eq!(witness_collision(&[int, float]), None);
        // Two equally shaped records do; a missing witness can never be
        // dispatched.
        let x = witness_of(&record(&["x"]));
        let y = witness_of(&record(&["x"]));
        assert_eq!(witness_collision(&[x, y]), Some((0, 1)));
        assert_eq!(witness_collision(&[None]), None);
    }

    #[test]
    fn builtin_heads_print_with_their_module() {
        assert_eq!(
            AdtKey::new(ModulePath::prelude(), "Option").to_string(),
            "Option"
        );
        assert_eq!(
            AdtKey::new(ModulePath::new(["A", "B"]), "Shape").to_string(),
            "A.B.Shape"
        );
        assert_eq!(ClassKey::prelude("Show").to_string(), "Show");
        assert_eq!(
            ClassKey::new(ModulePath::new(["Pretty"]), "Show").to_string(),
            "Pretty.Show"
        );
        assert_eq!(ModulePath::prelude().to_string(), "<prelude>");
    }
}
