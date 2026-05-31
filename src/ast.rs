//! Aevia AST (Abstract Syntax Tree) definitions.

use miette::SourceSpan;
use rust_decimal::Decimal;

/// A node wrapped with its source span for diagnostic reporting.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Spanned<T> {
    pub node: T,
    pub span: SourceSpan,
}

impl<T> Spanned<T> {
    /// Create a new spanned node.
    pub fn new(node: T, span: SourceSpan) -> Self {
        Self { node, span }
    }
}

/// A complete Aevia source file.
#[derive(Debug, Clone, PartialEq)]
pub struct SourceFile {
    /// Optional module-level doc comments.
    pub doc: Option<String>,
    /// Top-level items in the file.
    pub items: Vec<Spanned<Item>>,
}

/// A top-level module item.
#[derive(Debug, Clone, PartialEq)]
pub enum Item {
    /// Module declaration, e.g., `pub mod kinematics;` or `mod mechanics { ... }`
    Module {
        name: String,
        body: Option<Vec<Spanned<Item>>>,
        visibility: Visibility,
    },
    /// Import statement, e.g., `use mechanics::kinematics::{Force as F};`
    Use {
        path: Vec<String>,
        alias: Option<String>,
    },
    /// Dimensional type alias, e.g., `type Acceleration = m / s^2;`
    TypeAlias {
        name: String,
        dimension_expr: Spanned<DimExpr>,
    },
    /// Structural physical type, e.g., `pub struct Particle { ... }`
    Struct {
        name: String,
        fields: Vec<Field>,
        visibility: Visibility,
    },
    /// Function declaration, supporting expression-bodied (`:=`) and block-bodied (`{}`) forms.
    Function {
        name: String,
        params: Vec<Param>,
        return_type: Option<Spanned<DimExpr>>,
        body: FunctionBody,
        visibility: Visibility,
        attributes: Vec<Attribute>,
    },
    /// Custom mathematical operator declaration, e.g., `pub op custom(x: m -> m/s) { ... }`
    CustomOp {
        name: String,
        param_name: String,
        input_dim: Spanned<DimExpr>,
        output_dim: Spanned<DimExpr>,
        properties: OpProperties,
    },
}

/// Visibility modifiers.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Visibility {
    /// Visible only in the defining module (default).
    Private,
    /// Visible within the defining crate boundaries (`pub(crate)`).
    Crate,
    /// Publicly visible everywhere (`pub`).
    Public,
}

/// Struct field declaration with its dimensional annotation.
#[derive(Debug, Clone, PartialEq)]
pub struct Field {
    pub name: String,
    pub dim: Spanned<DimExpr>,
    pub visibility: Visibility,
}

/// Function parameter.
#[derive(Debug, Clone, PartialEq)]
pub struct Param {
    pub name: String,
    pub dim: Spanned<DimExpr>,
}

/// Function body representation.
#[derive(Debug, Clone, PartialEq)]
pub enum FunctionBody {
    /// Expression-bodied syntax, e.g., `:= m * a;` (auto-inlined)
    Expression(Spanned<Expr>),
    /// Block-bodied syntax, e.g., `{ let scalar = 0.5; scalar * k * x^2 }` (runs in a fiber context)
    Block(Vec<Spanned<Stmt>>),
}

/// Compiler attribute/procedural macros, e.g., `#[jit_kernel]`
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Attribute {
    pub name: String,
    pub args: Vec<String>,
}

/// Properties and simplify/egraph saturation rules for custom ops.
#[derive(Debug, Clone, PartialEq)]
pub struct OpProperties {
    pub vectorizable: bool,
    pub commutative: bool,
    pub associative: bool,
    pub cost: Option<Decimal>,
    pub simplify_rules: Vec<SimplifyRule>,
    pub egraph_rules: Vec<EGraphRule>,
}

/// An AST-level simplify rule, e.g., `custom(0) => 0`
#[derive(Debug, Clone, PartialEq)]
pub struct SimplifyRule {
    pub pattern: Spanned<Expr>,
    pub replacement: Spanned<Expr>,
}

/// An AST-level e-graph saturation rule, e.g., `rewrite custom(y) => integrate(y, dy)`
#[derive(Debug, Clone, PartialEq)]
pub struct EGraphRule {
    pub pattern: Spanned<Expr>,
    pub replacement: Spanned<Expr>,
}

/// Computational statements.
#[derive(Debug, Clone, PartialEq)]
pub enum Stmt {
    /// Variable declaration, e.g., `let mut mass: kg = 10.0;`
    Let {
        name: String,
        declared_type: Option<Spanned<DimExpr>>,
        init: Spanned<Expr>,
        is_mut: bool,
    },
    /// Computational expression as a statement.
    Expr(Spanned<Expr>),
    /// Variable assignment, e.g., `mass = 20.0;`
    Assign {
        target: String,
        value: Spanned<Expr>,
    },
    /// Loop break instruction.
    Break,
}

/// Dimensional unit expressions (e.g. `m * kg / s^2`).
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum DimExpr {
    /// Base physical dimension (e.g., `m`, `s`, `kg`, `mol`, `K`, `A`, `cd`).
    Base(String),
    /// Product of two physical dimensions, e.g., `N * m`
    Mul(Box<Spanned<DimExpr>>, Box<Spanned<DimExpr>>),
    /// Division of physical dimensions, e.g., `m / s`
    Div(Box<Spanned<DimExpr>>, Box<Spanned<DimExpr>>),
    /// Dimensional exponentiation, e.g., `s^2`
    Power(Box<Spanned<DimExpr>>, i32),
}

/// Computational expressions.
#[derive(Debug, Clone, PartialEq)]
pub enum Expr {
    /// Literal number with an optional physical unit suffix (e.g., `9.8_m/s^2`, `10.0`).
    Literal {
        value: Decimal,
        suffix: Option<Spanned<DimExpr>>,
    },
    /// Variable reference.
    Variable(String),
    /// Binary operation, e.g., `a + b`
    BinaryOp {
        op: BinOp,
        lhs: Box<Spanned<Expr>>,
        rhs: Box<Spanned<Expr>>,
    },
    /// Unary operation, e.g., `-x`
    UnaryOp {
        op: UnOp,
        expr: Box<Spanned<Expr>>,
    },
    /// Function or custom operator invocation, e.g., `kinetic_energy(m, v)`
    Call {
        func: String,
        args: Vec<Spanned<Expr>>,
    },
    /// Computational block expression, e.g., `{ let a = 1.0; a }`
    Block(Vec<Spanned<Stmt>>),
    /// Conditional branching expression, e.g., `if a < b { a } else { b }`
    If {
        cond: Box<Spanned<Expr>>,
        then_branch: Box<Spanned<Expr>>,
        else_branch: Option<Box<Spanned<Expr>>>,
    },
    /// Iterative loop expression.
    Loop {
        body: Vec<Spanned<Stmt>>,
    },
    /// Unsafe transmute construct for tensors, e.g. `unsafe transmute(buffer) { assume_unit: kg/m^3, ... }`
    UnsafeTransmute {
        expr: Box<Spanned<Expr>>,
        assume_unit: Option<Spanned<DimExpr>>,
        assume_layout: Option<String>,
    },
}

/// Binary mathematical and comparison operators.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum BinOp {
    Add,
    Sub,
    Mul,
    Div,
    Pow,
    Mod,
    Lt,
    Gt,
    Eq,
}

/// Unary operators.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum UnOp {
    Neg,
}
