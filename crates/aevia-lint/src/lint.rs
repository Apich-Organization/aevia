//! Lint definitions, groups, levels, and the central [`LintStore`].

use std::collections::HashMap;
use std::fmt;
use std::fmt::Write as _;

/// Enforcement level of a lint.
#[derive(
    Clone,
    Copy,
    Debug,
    PartialEq,
    Eq,
    PartialOrd,
    Ord,
    Hash,
    serde::Serialize,
    serde::Deserialize,
    bincode_next::Encode,
    bincode_next::Decode,
    bincode_next::Fingerprint,
)]
pub enum Level {
    /// Suppress the lint.
    Allow,
    /// Emit as a warning (exit 0 unless promoted).
    Warn,
    /// Emit as an error (exit 1).
    Deny,
    /// Emit as an error that cannot be lowered by inner attributes or flags.
    Forbid,
}

impl Level {
    /// Parses `"allow"`, `"warn"`, `"deny"`, or `"forbid"`.
    #[must_use]
    pub fn parse(text: &str) -> Option<Self> {
        match text {
            | "allow" => Some(Self::Allow),
            | "warn" => Some(Self::Warn),
            | "deny" => Some(Self::Deny),
            | "forbid" => Some(Self::Forbid),
            | _ => None,
        }
    }

    /// Lowercase canonical spelling.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            | Self::Allow => "allow",
            | Self::Warn => "warn",
            | Self::Deny => "deny",
            | Self::Forbid => "forbid",
        }
    }

    /// Converts from manifest's [`aevia_core::manifest::LintLevel`].
    #[must_use]
    pub const fn from_manifest(level: aevia_core::manifest::LintLevel) -> Self {
        match level {
            | aevia_core::manifest::LintLevel::Allow => Self::Allow,
            | aevia_core::manifest::LintLevel::Warn => Self::Warn,
            | aevia_core::manifest::LintLevel::Deny => Self::Deny,
            | aevia_core::manifest::LintLevel::Forbid => Self::Forbid,
        }
    }
}

impl fmt::Display for Level {
    fn fmt(
        &self,
        f: &mut fmt::Formatter<'_>,
    ) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Lint category / group.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum LintGroup {
    /// Likely bugs or unintended physical/numeric semantics.
    Correctness,
    /// Naming and idiomatic style conventions.
    Style,
    /// Extra strictness, off by default (`allow`).
    Pedantic,
}

impl LintGroup {
    /// Canonical group name.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            | Self::Correctness => "correctness",
            | Self::Style => "style",
            | Self::Pedantic => "pedantic",
        }
    }
}

/// Dense identifier into [`LintStore`].
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct LintId(pub usize);

/// Static descriptor for one lint.
#[derive(Clone, Copy, Debug)]
pub struct Lint {
    /// Stable diagnostic code (`W0101`, `W0701`, ...).
    pub code: &'static str,
    /// Kebab-case name (`unused-variable`).
    pub name: &'static str,
    /// Group membership.
    pub group: LintGroup,
    /// Default level when unconfigured.
    pub default_level: Level,
    /// `true` for compiler warnings emitted by `aevia-types`; `false` for `aevia lint`-only passes.
    pub compiler_lint: bool,
    /// One-line description.
    pub summary: &'static str,
    /// Detailed `--explain` text.
    pub explanation: &'static str,
}

/// Registry of all compiler and extra lints.
#[derive(Debug)]
pub struct LintStore {
    lints: Vec<&'static Lint>,
    by_name: HashMap<String, LintId>,
    by_code: HashMap<&'static str, LintId>,
}

impl Default for LintStore {
    fn default() -> Self {
        Self::with_builtins()
    }
}

impl LintStore {
    /// Creates an empty store.
    #[must_use]
    pub fn empty() -> Self {
        Self {
            lints: Vec::new(),
            by_name: HashMap::new(),
            by_code: HashMap::new(),
        }
    }

    /// Creates the default store with all compiler and built-in `aevia lint` lints.
    #[must_use]
    pub fn with_builtins() -> Self {
        let mut store = Self::empty();
        for lint in BUILTIN_LINTS {
            store.register(lint);
        }
        store
    }

    /// Registers a lint descriptor.
    pub fn register(
        &mut self,
        lint: &'static Lint,
    ) -> LintId {
        let id = LintId(self.lints.len());
        self.lints.push(lint);
        self.by_name.insert(lint.name.to_string(), id);
        // Accept snake_case alias (`unused_variable`) as well as kebab-case (`unused-variable`).
        self.by_name.insert(lint.name.replace('-', "_"), id);
        self.by_code.insert(lint.code, id);
        id
    }

    /// All registered lints in registration order.
    #[must_use]
    pub fn all(&self) -> &[&'static Lint] {
        &self.lints
    }

    /// Looks up a lint by id.
    #[must_use]
    pub fn get(
        &self,
        id: LintId,
    ) -> Option<&'static Lint> {
        self.lints.get(id.0).copied()
    }

    /// Looks up a lint by kebab-case name, `snake_case` alias, or `W0xxx` code.
    #[must_use]
    pub fn find(
        &self,
        name_or_code: &str,
    ) -> Option<(LintId, &'static Lint)> {
        let id = self
            .by_name
            .get(name_or_code)
            .or_else(|| self.by_code.get(name_or_code))
            .copied()?;
        self.get(id).map(|lint| (id, lint))
    }

    /// Looks up a lint by exact diagnostic code (e.g. `"W0101"`).
    #[must_use]
    pub fn find_by_code(
        &self,
        code: &str,
    ) -> Option<(LintId, &'static Lint)> {
        let id = *self.by_code.get(code)?;
        self.get(id).map(|lint| (id, lint))
    }

    /// Expands a lint or group name (`correctness`, `style`, `pedantic`, `warnings`, `all`)
    /// into matching [`LintId`]s. Returns `None` for unknown names.
    #[must_use]
    pub fn expand_target(
        &self,
        target: &str,
    ) -> Option<Vec<LintId>> {
        let ids: Vec<LintId> = match target {
            | "correctness" => {
                self.lints
                    .iter()
                    .enumerate()
                    .filter(|(_, l)| l.group == LintGroup::Correctness)
                    .map(|(i, _)| LintId(i))
                    .collect()
            },
            | "style" => {
                self.lints
                    .iter()
                    .enumerate()
                    .filter(|(_, l)| l.group == LintGroup::Style)
                    .map(|(i, _)| LintId(i))
                    .collect()
            },
            | "pedantic" => {
                self.lints
                    .iter()
                    .enumerate()
                    .filter(|(_, l)| l.group == LintGroup::Pedantic)
                    .map(|(i, _)| LintId(i))
                    .collect()
            },
            | "warnings" => {
                // Every lint whose default level is not `Allow` (plus compiler warnings).
                self.lints
                    .iter()
                    .enumerate()
                    .filter(|(_, l)| l.default_level >= Level::Warn || l.compiler_lint)
                    .map(|(i, _)| LintId(i))
                    .collect()
            },
            | "all" => (0..self.lints.len()).map(LintId).collect(),
            | other => {
                let (id, _) = self.find(other)?;
                vec![id]
            },
        };
        Some(ids)
    }

    /// Formats `aevia lint --list`.
    #[must_use]
    pub fn format_list(&self) -> String {
        let mut out =
            String::from("CODE   NAME                       DEFAULT  GROUP        SUMMARY\n");
        for lint in &self.lints {
            let tier = if lint.compiler_lint {
                ""
            } else {
                " (lint)"
            };
            let _ = writeln!(
                out,
                "{:<6} {:<26} {:<8} {:<12} {}{tier}",
                lint.code,
                lint.name,
                lint.default_level.as_str(),
                lint.group.as_str(),
                lint.summary,
            );
        }
        out.push_str("\nGroups: correctness, style, pedantic, warnings, all\n");
        out
    }

    /// Formats `aevia lint --explain <code|name>`.
    #[must_use]
    pub fn format_explain(
        &self,
        target: &str,
    ) -> Option<String> {
        let (_, lint) = self.find(target)?;
        Some(format!(
            "{} ({})\n  group:   {}\n  default: {}\n  tier:    {}\n\n{}\n",
            lint.code,
            lint.name,
            lint.group.as_str(),
            lint.default_level.as_str(),
            if lint.compiler_lint {
                "compiler (check & lint)"
            } else {
                "extra (aevia lint)"
            },
            lint.explanation
        ))
    }
}

/// Compiler lints (emitted by `aevia-types`).
/// Reports a variable binding with no uses.
pub static W0101_UNUSED_VARIABLE: Lint = Lint {
    code: "W0101",
    name: "unused-variable",
    group: LintGroup::Correctness,
    default_level: Level::Warn,
    compiler_lint: true,
    summary: "unused local variable or parameter",
    explanation: "A local binding or parameter is never read. Prefix it with `_` if intentional, or remove it.",
};

/// Reports a binding that shadows another binding in scope.
pub static W0103_SHADOWED_BINDING: Lint = Lint {
    code: "W0103",
    name: "shadowed-binding",
    group: LintGroup::Style,
    default_level: Level::Warn,
    compiler_lint: true,
    summary: "local binding shadows an outer binding",
    explanation: "A local `let` or parameter rebinds a name already visible in an enclosing scope.",
};

/// Reports an unreachable match arm.
pub static W0201_REDUNDANT_MATCH_ARM: Lint = Lint {
    code: "W0201",
    name: "redundant-match-arm",
    group: LintGroup::Correctness,
    default_level: Level::Warn,
    compiler_lint: true,
    summary: "match arm is already covered by earlier arms",
    explanation: "Earlier patterns already cover every value matched by this arm, so it can never execute.",
};

/// Reports an `IO` result discarded without choosing how to handle it.
pub static W0202_DISCARDED_IO_RESULT: Lint = Lint {
    code: "W0202",
    name: "discarded-io-result",
    group: LintGroup::Correctness,
    default_level: Level::Warn,
    compiler_lint: true,
    summary: "non-unit value discarded in a `do` statement",
    explanation: "An `IO` action or expression inside `do` returns a non-`()` value that is ignored. Bind it with `<-` or `let _ = ...`.",
};

/// Reports a discarded `Result` accepted in relaxed mode.
pub static W0204_RELAXED_DISCARDED_RESULT: Lint = Lint {
    code: "W0204",
    name: "relaxed-discarded-result",
    group: LintGroup::Correctness,
    default_level: Level::Warn,
    compiler_lint: true,
    summary: "`Result` value discarded under `@relax_result`",
    explanation: "A `Result` is bound to `_` in a declaration marked `@relax_result`. Handle the error or match on it.",
};

/// Reports an implicit unwrap accepted in relaxed mode.
pub static W0205_RELAXED_IMPLICIT_UNWRAP: Lint = Lint {
    code: "W0205",
    name: "relaxed-implicit-unwrap",
    group: LintGroup::Correctness,
    default_level: Level::Warn,
    compiler_lint: true,
    summary: "implicit `Result`/`Option` unwrap under `@relax_result`",
    explanation: "A `?` that cannot be converted to the enclosing return type is treated as an unwrap in a declaration marked `@relax_result`.",
};

/// Reports a non-exhaustive match accepted in relaxed mode.
pub static W0206_RELAXED_NON_EXHAUSTIVE_MATCH: Lint = Lint {
    code: "W0206",
    name: "relaxed-non-exhaustive-match",
    group: LintGroup::Correctness,
    default_level: Level::Warn,
    compiler_lint: true,
    summary: "non-exhaustive `Result`/`Option` match under `@relax_result`",
    explanation: "A `match` on a `Result` or `Option` misses cases; the error is relaxed to a warning in a declaration marked `@relax_result`.",
};

/// Reports an unresolved dimension that defaults to dimensionless.
pub static W0301_UNRESOLVED_DIMENSION_DEFAULT: Lint = Lint {
    code: "W0301",
    name: "unresolved-dimension-default",
    group: LintGroup::Correctness,
    default_level: Level::Warn,
    compiler_lint: true,
    summary: "unresolved dimension variable defaulted to 1",
    explanation: "A dimension variable remained unconstrained and defaulted to dimensionless (`1`). Add a unit or dimension annotation if a physical dimension was intended.",
};

/// Reports mixed exact and approximate real arithmetic.
pub static W0302_REAL_FLOAT_MIX: Lint = Lint {
    code: "W0302",
    name: "real-float-mix",
    group: LintGroup::Correctness,
    default_level: Level::Warn,
    compiler_lint: true,
    summary: "`Real`/`Float` mix uses the exact `Real` path",
    explanation: "Mixing exact `Real` and IEEE-754 `Float` promotes to `Real` at runtime; annotate explicitly if `Float` performance was intended.",
};

/// Reports a symbolic declaration with no uses.
pub static W0303_UNUSED_SYMBOL: Lint = Lint {
    code: "W0303",
    name: "unused-symbol",
    group: LintGroup::Correctness,
    default_level: Level::Warn,
    compiler_lint: true,
    summary: "unused top-level symbol",
    explanation: "A top-level declaration is never referenced.",
};

/// Reports an unused private declaration.
pub static W0601_UNUSED_PRIVATE_DECL: Lint = Lint {
    code: "W0601",
    name: "unused-private-decl",
    group: LintGroup::Correctness,
    default_level: Level::Warn,
    compiler_lint: true,
    summary: "unused private declaration",
    explanation: "A `private` declaration is never used inside its module.",
};

/// Reports a reference to a deprecated declaration.
pub static W0602_DEPRECATED_USE: Lint = Lint {
    code: "W0602",
    name: "deprecated-use",
    group: LintGroup::Correctness,
    default_level: Level::Warn,
    compiler_lint: true,
    summary: "use of a `@deprecated` declaration",
    explanation: "Calls or references a declaration marked `@deprecated`. Migrate to the replacement noted in its deprecation message.",
};

/// Extra lints (`aevia lint` only).
/// Reports an unused imported module or name.
pub static W0102_UNUSED_IMPORT: Lint = Lint {
    code: "W0102",
    name: "unused-import",
    group: LintGroup::Correctness,
    default_level: Level::Warn,
    compiler_lint: false,
    summary: "imported module or name is never used",
    explanation: "An `import` statement brings a module or selective names into scope that are never referenced in this module.",
};

/// Reports a value name that uses uppercase characters.
pub static W0701_NON_SNAKE_CASE: Lint = Lint {
    code: "W0701",
    name: "non-snake-case",
    group: LintGroup::Style,
    default_level: Level::Warn,
    compiler_lint: false,
    summary: "function or variable name should be `snake_case`",
    explanation: "Aevia uses `snake_case` for functions and value bindings. Rename the identifier or suppress with `@allow(non-snake-case)`.",
};

/// Reports a type name that differs from `UpperCamelCase`.
pub static W0702_NON_CAMEL_CASE_TYPE: Lint = Lint {
    code: "W0702",
    name: "non-camel-case-type",
    group: LintGroup::Style,
    default_level: Level::Warn,
    compiler_lint: false,
    summary: "type, class, or constructor name should be `UpperCamelCase`",
    explanation: "Aevia uses `UpperCamelCase` for nominal types, type classes, and constructors.",
};

/// Reports a public declaration without a documentation comment.
pub static W0703_MISSING_DOC_PUBLIC: Lint = Lint {
    code: "W0703",
    name: "missing-doc-public",
    group: LintGroup::Pedantic,
    default_level: Level::Allow,
    compiler_lint: false,
    summary: "public declaration is missing a `##` doc comment",
    explanation: "Public declarations appear in generated registry docs (`aevia doc`). Add a `##` doc comment above the declaration.",
};

/// Reports an annotation that repeats the inferred type.
pub static W0704_REDUNDANT_ANNOTATION: Lint = Lint {
    code: "W0704",
    name: "redundant-annotation",
    group: LintGroup::Pedantic,
    default_level: Level::Allow,
    compiler_lint: false,
    summary: "type annotation duplicates an already-known type",
    explanation: "An expression carries an explicit `: T` annotation that repeats the literal or inner annotation type.",
};

/// Reports a declaration that shadows a prelude name.
pub static W0705_SHADOWED_PRELUDE_NAME: Lint = Lint {
    code: "W0705",
    name: "shadowed-prelude-name",
    group: LintGroup::Style,
    default_level: Level::Warn,
    compiler_lint: false,
    summary: "declaration shadows a prelude builtin",
    explanation: "Binding a name such as `sqrt`, `sin`, `abs`, or `map` shadows the prelude builtin in this scope.",
};

/// Reports exact equality tests between approximate numbers.
pub static W0706_FLOAT_EQUALITY: Lint = Lint {
    code: "W0706",
    name: "float-equality",
    group: LintGroup::Correctness,
    default_level: Level::Warn,
    compiler_lint: false,
    summary: "strict equality (`==`/`!=`) on `Float` values",
    explanation: "IEEE-754 floating-point equality (`==` / `!=`) is sensitive to rounding error. Prefer comparing `abs(a - b) < eps`.",
};

/// Reports a bare zero coerced to a dimensionful scalar or point.
pub static W0707_DIMENSION_LITERAL_DEFAULT: Lint = Lint {
    code: "W0707",
    name: "dimension-literal-default",
    group: LintGroup::Correctness,
    default_level: Level::Warn,
    compiler_lint: false,
    summary: "bare zero literal coerced to a dimensioned quantity",
    explanation: "A bare `0` or `0.0` acquires a physical dimension from context without an explicit unit. Write `0 m` (or the intended unit) for clarity.",
};

/// Reports a comparison with identical operands.
pub static W0708_SELF_COMPARISON: Lint = Lint {
    code: "W0708",
    name: "self-comparison",
    group: LintGroup::Correctness,
    default_level: Level::Warn,
    compiler_lint: false,
    summary: "comparing a value to itself",
    explanation: "Comparing an expression to itself with `==`, `!=`, `<`, `<=`, `>`, or `>=` is usually a bug (such as a typo in one operand).",
};

/// Reports an `if` expression that can be replaced with its condition or negation.
pub static W0709_REDUNDANT_IF: Lint = Lint {
    code: "W0709",
    name: "redundant-if",
    group: LintGroup::Style,
    default_level: Level::Warn,
    compiler_lint: false,
    summary: "redundant `if` expression",
    explanation: "An `if` expression that returns `true`/`false` directly can use the condition or its negation. Before replacing identical branches with their shared expression, check whether skipping evaluation of the condition could hide an error.",
};

/// Reports repeated imports of the same module.
pub static W0710_DUPLICATE_IMPORT: Lint = Lint {
    code: "W0710",
    name: "duplicate-import",
    group: LintGroup::Style,
    default_level: Level::Warn,
    compiler_lint: false,
    summary: "module is imported more than once",
    explanation: "Importing the same module multiple times is redundant; combine the imports into a single declaration.",
};

/// Reports an `if` whose condition is a boolean literal.
pub static W0711_CONSTANT_CONDITION: Lint = Lint {
    code: "W0711",
    name: "constant-condition",
    group: LintGroup::Pedantic,
    default_level: Level::Allow,
    compiler_lint: false,
    summary: "an `if` condition is always `true` or `false`",
    explanation: "A literal `if` condition always selects the same branch. Check whether a variable or test was intended. Enable with `-W constant-condition` or `@warn(constant-condition)`.",
};

static BUILTIN_LINTS: &[&Lint] = &[
    &W0101_UNUSED_VARIABLE,
    &W0102_UNUSED_IMPORT,
    &W0103_SHADOWED_BINDING,
    &W0201_REDUNDANT_MATCH_ARM,
    &W0202_DISCARDED_IO_RESULT,
    &W0204_RELAXED_DISCARDED_RESULT,
    &W0205_RELAXED_IMPLICIT_UNWRAP,
    &W0206_RELAXED_NON_EXHAUSTIVE_MATCH,
    &W0301_UNRESOLVED_DIMENSION_DEFAULT,
    &W0302_REAL_FLOAT_MIX,
    &W0303_UNUSED_SYMBOL,
    &W0601_UNUSED_PRIVATE_DECL,
    &W0602_DEPRECATED_USE,
    &W0701_NON_SNAKE_CASE,
    &W0702_NON_CAMEL_CASE_TYPE,
    &W0703_MISSING_DOC_PUBLIC,
    &W0704_REDUNDANT_ANNOTATION,
    &W0705_SHADOWED_PRELUDE_NAME,
    &W0706_FLOAT_EQUALITY,
    &W0707_DIMENSION_LITERAL_DEFAULT,
    &W0708_SELF_COMPARISON,
    &W0709_REDUNDANT_IF,
    &W0710_DUPLICATE_IMPORT,
    &W0711_CONSTANT_CONDITION,
];
