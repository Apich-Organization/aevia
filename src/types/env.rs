//! Type environment: scoped symbol table mapping names to `DimVector`s.
//!
//! Variables, function parameters, type aliases, and struct fields
//! all live in this environment. Scopes are pushed/popped for blocks.

use crate::types::dim::DimVector;
use std::collections::HashMap;

/// A scoped symbol table for dimensional type bindings.
///
/// Scopes form a stack: the innermost scope (current block) is checked first,
/// then parent scopes in order.
#[derive(Debug, Clone)]
pub struct TypeEnv {
    /// Stack of scopes. The last entry is the innermost (current) scope.
    scopes: Vec<HashMap<String, DimVector>>,
    /// Global type aliases (e.g. `type Velocity = m/s`).
    aliases: HashMap<String, DimVector>,
}

impl TypeEnv {
    /// Create a new environment with a single global scope and built-in aliases.
    #[must_use]
    pub fn new() -> Self {
        let mut env = Self {
            scopes: vec![HashMap::new()],
            aliases: HashMap::new(),
        };
        // Pre-register the canonical SI derived unit aliases so that
        // `type Newton = kg * m / s^2;` isn't required in every file.
        env.register_alias("Newton", DimVector::NEWTON);
        env.register_alias("N",      DimVector::NEWTON);
        env.register_alias("Joule",  DimVector::JOULE);
        env.register_alias("J",      DimVector::JOULE);
        env.register_alias("Pascal", DimVector::PASCAL);
        env.register_alias("Pa",     DimVector::PASCAL);
        env.register_alias("Watt",   DimVector::WATT);
        env.register_alias("W",      DimVector::WATT);
        env.register_alias("Hertz",  DimVector::HERTZ);
        env.register_alias("Hz",     DimVector::HERTZ);
        env.register_alias("Velocity",     DimVector::VELOCITY);
        env.register_alias("Acceleration", DimVector::ACCELERATION);
        env
    }

    // ── Scope management ───────────────────────────────────────────────────

    /// Push a new nested scope (e.g. entering a block body).
    pub fn push_scope(&mut self) {
        self.scopes.push(HashMap::new());
    }

    /// Pop the innermost scope (e.g. leaving a block body).
    ///
    /// Panics in debug builds if called with only the global scope remaining.
    pub fn pop_scope(&mut self) {
        debug_assert!(self.scopes.len() > 1, "attempted to pop global scope");
        self.scopes.pop();
    }

    // ── Variable bindings ──────────────────────────────────────────────────

    /// Bind a variable name to a `DimVector` in the **current** scope.
    pub fn define(&mut self, name: impl Into<String>, dim: DimVector) {
        if let Some(scope) = self.scopes.last_mut() {
            scope.insert(name.into(), dim);
        }
    }

    /// Look up a variable name, searching from innermost to outermost scope.
    /// Also checks the alias table.
    #[must_use]
    pub fn lookup(&self, name: &str) -> Option<DimVector> {
        // Search scopes innermost-first.
        for scope in self.scopes.iter().rev() {
            if let Some(&dim) = scope.get(name) {
                return Some(dim);
            }
        }
        // Fall back to global alias table.
        self.aliases.get(name).copied()
    }

    // ── Type aliases ───────────────────────────────────────────────────────

    /// Register a global type alias (from `type X = ...;` declarations).
    pub fn register_alias(&mut self, name: impl Into<String>, dim: DimVector) {
        self.aliases.insert(name.into(), dim);
    }

    /// Look up a type alias by name.
    #[must_use]
    pub fn lookup_alias(&self, name: &str) -> Option<DimVector> {
        self.aliases.get(name).copied()
    }

    // ── Function signatures ────────────────────────────────────────────────

    /// Register a named function's return type so call-site checking works.
    pub fn register_fn(&mut self, name: impl Into<String>, return_dim: DimVector) {
        // Functions live in the global scope under a mangled key.
        if let Some(scope) = self.scopes.first_mut() {
            scope.insert(format!("fn::{}", name.into()), return_dim);
        }
    }

    /// Look up a function's return dimension.
    #[must_use]
    pub fn lookup_fn(&self, name: &str) -> Option<DimVector> {
        self.scopes
            .first()
            .and_then(|s| s.get(&format!("fn::{name}")).copied())
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
        env.define("mass", DimVector::KG);
        assert_eq!(env.lookup("mass"), Some(DimVector::KG));
        assert_eq!(env.lookup("nope"), None);
    }

    #[test]
    fn test_scope_shadowing() {
        let mut env = TypeEnv::new();
        env.define("x", DimVector::M);
        env.push_scope();
        env.define("x", DimVector::S);
        assert_eq!(env.lookup("x"), Some(DimVector::S)); // inner shadows outer
        env.pop_scope();
        assert_eq!(env.lookup("x"), Some(DimVector::M)); // outer restored
    }

    #[test]
    fn test_alias_lookup() {
        let env = TypeEnv::new();
        assert_eq!(env.lookup("Newton"), Some(DimVector::NEWTON));
        assert_eq!(env.lookup("J"),      Some(DimVector::JOULE));
    }

    #[test]
    fn test_register_alias() {
        let mut env = TypeEnv::new();
        env.register_alias("MyUnit", DimVector::M.div(DimVector::S));
        assert_eq!(env.lookup("MyUnit"), Some(DimVector::VELOCITY));
    }

    #[test]
    fn test_fn_registration() {
        let mut env = TypeEnv::new();
        env.register_fn("force", DimVector::NEWTON);
        assert_eq!(env.lookup_fn("force"), Some(DimVector::NEWTON));
    }
}
