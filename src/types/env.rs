//! Type environment: scoped symbol table mapping names to `PhysicalType`s.
//!
//! Variables and function parameters live here, with full shape tracking.
//! Scopes are pushed/popped for blocks.

use crate::types::dim::{DimVector, PhysicalType};
use std::collections::HashMap;

/// A scoped symbol table for physical type bindings.
#[derive(Debug, Clone)]
pub struct TypeEnv {
    /// Stack of scopes: last = innermost (current block).
    scopes: Vec<HashMap<String, PhysicalType>>,
    /// Global type aliases (e.g. `type Velocity = m/s`) — always scalar.
    aliases: HashMap<String, DimVector>,
}

impl TypeEnv {
    #[must_use]
    pub fn new() -> Self {
        let mut env = Self {
            scopes: vec![HashMap::new()],
            aliases: HashMap::new(),
        };
        env.register_alias("Newton",       DimVector::NEWTON);
        env.register_alias("N",            DimVector::NEWTON);
        env.register_alias("Joule",        DimVector::JOULE);
        env.register_alias("J",            DimVector::JOULE);
        env.register_alias("Pascal",       DimVector::PASCAL);
        env.register_alias("Pa",           DimVector::PASCAL);
        env.register_alias("Watt",         DimVector::WATT);
        env.register_alias("W",            DimVector::WATT);
        env.register_alias("Hertz",        DimVector::HERTZ);
        env.register_alias("Hz",           DimVector::HERTZ);
        env.register_alias("Velocity",     DimVector::VELOCITY);
        env.register_alias("Acceleration", DimVector::ACCELERATION);
        env
    }

    // ── Scope management ───────────────────────────────────────────────────

    pub fn push_scope(&mut self) {
        self.scopes.push(HashMap::new());
    }

    pub fn pop_scope(&mut self) {
        debug_assert!(self.scopes.len() > 1, "attempted to pop global scope");
        self.scopes.pop();
    }

    // ── Variable bindings ──────────────────────────────────────────────────

    /// Bind a variable to a full `PhysicalType` (dimension + optional shape).
    pub fn define(&mut self, name: impl Into<String>, pt: PhysicalType) {
        if let Some(scope) = self.scopes.last_mut() {
            scope.insert(name.into(), pt);
        }
    }

    /// Look up a variable, returning its full `PhysicalType`.
    #[must_use]
    pub fn lookup(&self, name: &str) -> Option<PhysicalType> {
        for scope in self.scopes.iter().rev() {
            if let Some(pt) = scope.get(name) {
                return Some(pt.clone());
            }
        }
        self.aliases.get(name).map(|&d| PhysicalType::scalar(d))
    }

    // ── Type aliases ───────────────────────────────────────────────────────

    pub fn register_alias(&mut self, name: impl Into<String>, dim: DimVector) {
        self.aliases.insert(name.into(), dim);
    }

    #[must_use]
    pub fn lookup_alias(&self, name: &str) -> Option<DimVector> {
        self.aliases.get(name).copied()
    }

    // ── Function signatures ────────────────────────────────────────────────

    pub fn register_fn(&mut self, name: impl Into<String>, return_dim: DimVector) {
        if let Some(scope) = self.scopes.first_mut() {
            scope.insert(format!("fn::{}", name.into()), PhysicalType::scalar(return_dim));
        }
    }

    #[must_use]
    pub fn lookup_fn(&self, name: &str) -> Option<DimVector> {
        self.scopes
            .first()
            .and_then(|s| s.get(&format!("fn::{name}")))
            .map(|pt| pt.dim)
    }
}

impl Default for TypeEnv {
    fn default() -> Self {
        Self::new()
    }
}

// ── Tests ──────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_define_lookup() {
        let mut env = TypeEnv::new();
        env.define("mass", PhysicalType::scalar(DimVector::KG));
        assert_eq!(env.lookup("mass"), Some(PhysicalType::scalar(DimVector::KG)));
        assert_eq!(env.lookup("nope"), None);
    }

    #[test]
    fn test_define_tensor_lookup() {
        let mut env = TypeEnv::new();
        env.define("buf", PhysicalType::tensor(DimVector::KG, vec![1024]));
        let pt = env.lookup("buf").unwrap();
        assert_eq!(pt.dim, DimVector::KG);
        assert_eq!(pt.shape, Some(vec![1024]));
    }

    #[test]
    fn test_scope_shadowing() {
        let mut env = TypeEnv::new();
        env.define("x", PhysicalType::scalar(DimVector::M));
        env.push_scope();
        env.define("x", PhysicalType::scalar(DimVector::S));
        assert_eq!(env.lookup("x").unwrap().dim, DimVector::S);
        env.pop_scope();
        assert_eq!(env.lookup("x").unwrap().dim, DimVector::M);
    }

    #[test]
    fn test_alias_lookup() {
        let env = TypeEnv::new();
        assert_eq!(env.lookup("Newton").unwrap().dim, DimVector::NEWTON);
        assert_eq!(env.lookup("J").unwrap().dim,      DimVector::JOULE);
    }

    #[test]
    fn test_register_alias() {
        let mut env = TypeEnv::new();
        env.register_alias("MyUnit", DimVector::M.div(DimVector::S));
        assert_eq!(env.lookup_alias("MyUnit"), Some(DimVector::VELOCITY));
    }

    #[test]
    fn test_fn_registration() {
        let mut env = TypeEnv::new();
        env.register_fn("force", DimVector::NEWTON);
        assert_eq!(env.lookup_fn("force"), Some(DimVector::NEWTON));
    }
}
