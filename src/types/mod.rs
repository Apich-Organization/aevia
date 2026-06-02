//! Aevia dimensional type system — Phase 2.
//!
//! Exposes the `DimVector` algebra, the scoped `TypeEnv`, rich `TypeError`
//! diagnostics, and the top-level `check()` entry point.

pub mod checker;
pub use checker::check_with_imports;
pub mod dim;
pub mod env;
pub mod error;

pub use checker::{check, CheckResult};
pub use dim::DimVector;
pub use env::TypeEnv;
pub use error::{TypeError, TypeErrorCode};
