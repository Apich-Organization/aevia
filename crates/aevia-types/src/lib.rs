//! Type and dimension checking for Aevia.
//!
//! The crate implements the type system of chapter 03 on top of the dimension
//! algebra of chapter 04:
//!
//! * [`ty`] defines the internal type language (`Scalar<ρ, δ>`, tensors,
//!   records with rows, nominal variants, `IO`);
//! * [`infer`] unifies types, dimensions and rows, and generalises bindings;
//! * [`builtins`] holds the signature of every built-in;
//! * [`check`] walks a module and produces the export table the runtime uses.
//!
//! ```
//! use aevia_syntax::lex;
//! use aevia_syntax::parse_module;
//! use aevia_types::check::check_module;
//!
//! let source =
//!     "fn fall_time(h: Float[L]): Float[T] =\n    sqrt(2 * h / g)\n\nlet g = 9.80665 m/s^2\n";
//! let lexed = lex(0, source).expect("lexes");
//! let (module, parse_diagnostics) = parse_module(0, source, &lexed);
//! assert!(!parse_diagnostics.has_errors());
//! let checked = check_module(0, &module);
//! assert!(
//!     !checked.diagnostics.has_errors(),
//!     "{}",
//!     checked.diagnostics.render(&aevia_syntax::SourceMap::new())
//! );
//! ```

#![forbid(unsafe_code)]

pub mod adt;
pub mod builtins;
pub mod check;
pub mod class;
pub mod diagnostic_bridge;
pub mod infer;
pub mod keys;
pub mod rssn_resolve;
pub mod ty;

pub use check::AliasExport;
pub use check::Capabilities;
pub use check::CheckOptions;
pub use check::CheckResult;
pub use check::CheckedModule;
pub use check::ClassExport;
pub use check::DictTarget;
pub use check::DictionaryUse;
pub use check::DynCall;
pub use check::DynPrimitive;
pub use check::Export;
pub use check::HostRegistration;
pub use check::HostRegistrationKind;
pub use check::InstanceExport;
pub use check::ModuleInterface;
pub use check::RssnCall;
pub use check::Stability;
pub use check::check_module;
pub use check::check_module_with_imports;
pub use check::check_module_with_interfaces;
pub use check::check_module_with_options;
pub use class::ClassTable;
pub use class::Origin;
pub use class::Resolution;
pub use infer::Infer;
pub use infer::UnifyError;
pub use keys::AdtKey;
pub use keys::ClassKey;
pub use keys::HeadKey;
pub use keys::InstanceKey;
pub use keys::ModulePath;
pub use keys::RepKind;
pub use keys::StorageClass;
pub use keys::Witness;
pub use keys::head_key;
pub use keys::witness_collision;
pub use keys::witness_of;
pub use keys::witnesses_are_disjoint;
pub use ty::Constraint;
pub use ty::LiteralKind;
pub use ty::Nat;
pub use ty::Rep;
pub use ty::Row;
pub use ty::Scheme;
pub use ty::Shape;
pub use ty::Type;
