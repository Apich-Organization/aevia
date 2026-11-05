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
//! use aevia_syntax::{lex, parse_module};
//! use aevia_types::check::check_module;
//!
//! let source = "fn fall_time(h: Float[L]): Float[T] =\n    sqrt(2 * h / g)\n\nlet g = 9.80665 m/s^2\n";
//! let lexed = lex(0, source).expect("lexes");
//! let (module, parse_diagnostics) = parse_module(0, source, &lexed);
//! assert!(!parse_diagnostics.has_errors());
//! let checked = check_module(0, &module);
//! assert!(!checked.diagnostics.has_errors(), "{}", checked.diagnostics.render(&aevia_syntax::SourceMap::new()));
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

pub use check::{
    check_module, check_module_with_imports, check_module_with_interfaces,
    check_module_with_options, AliasExport, Capabilities, CheckOptions, CheckResult, CheckedModule,
    ClassExport, DictTarget, DictionaryUse, DynCall, DynPrimitive, Export, HostRegistration,
    HostRegistrationKind, InstanceExport, ModuleInterface, RssnCall, Stability,
};
pub use class::{ClassTable, Origin, Resolution};
pub use infer::{Infer, UnifyError};
pub use keys::{
    head_key, witness_collision, witness_of, witnesses_are_disjoint, AdtKey, ClassKey, HeadKey,
    InstanceKey, ModulePath, RepKind, StorageClass, Witness,
};
pub use ty::{Constraint, LiteralKind, Nat, Rep, Row, Scheme, Shape, Type};
