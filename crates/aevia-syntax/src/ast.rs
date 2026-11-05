//! The abstract syntax tree.
//!
//! The tree keeps source spans on every expression, pattern and type: the type
//! checker and the interpreter both report positions, and the formatter needs
//! them to preserve layout. Brackets are *not* represented — the parser applies
//! precedence — which is what distinguishes the AST from the CST of §9.2.1.

use crate::span::Span;
use aevia_dim::{Scale, UnitExpr};
use num_bigint::BigInt;
use num_traits::One;

/// A literal number in a unit literal.
#[derive(Clone, Debug, PartialEq)]
pub enum NumberLit {
    /// An integer coefficient: `3 m` is `Real[L]` by default.
    Int(BigInt),
    /// A decimal coefficient, kept as source text so it can be read either as
    /// an `f64` (`3.0 m` defaults to `Float[L]`) or exactly (`Real` contexts
    /// turn `0.1` into `1/10`, never into a binary approximation).
    Decimal(String),
}

impl NumberLit {
    /// The coefficient as a float.
    #[must_use]
    pub fn as_f64(&self) -> f64 {
        match self {
            Self::Int(value) => {
                use num_traits::ToPrimitive;
                value.to_f64().unwrap_or(f64::INFINITY)
            }
            Self::Decimal(text) => text.replace('_', "").parse().unwrap_or(f64::NAN),
        }
    }

    /// The coefficient as an exact rational, when it has one.
    #[must_use]
    pub fn as_rational(&self) -> Option<num_rational::BigRational> {
        match self {
            Self::Int(value) => Some(num_rational::BigRational::from_integer(value.clone())),
            Self::Decimal(text) => aevia_dim::parse_decimal(text),
        }
    }

    /// True for the decimal form.
    #[must_use]
    pub fn is_decimal(&self) -> bool {
        matches!(self, Self::Decimal(_))
    }
}

/// A piece of an interpolated string.
#[derive(Clone, Debug)]
pub enum StrPiece {
    /// Literal text with escapes decoded.
    Text(String),
    /// `{expr}`.
    Expr(Box<Expr>),
}

/// Unary operators.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum UnOp {
    /// `-x`
    Neg,
    /// `+x`
    Pos,
    /// `!x`
    Not,
    /// `√x`
    Sqrt,
}

/// Binary operators, in the surface syntax of §2.6.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum BinOp {
    /// `+`
    Add,
    /// `-`
    Sub,
    /// `*` and `·`/`×`
    Mul,
    /// `/` and `÷`
    Div,
    /// `@`
    MatMul,
    /// `^`
    Pow,
    /// `==`
    Eq,
    /// `!=`
    Ne,
    /// `<`
    Lt,
    /// `<=`
    Le,
    /// `>`
    Gt,
    /// `>=`
    Ge,
    /// `&&`
    And,
    /// `||`
    Or,
    /// `::`
    Cons,
    /// `|>`
    Pipe,
    /// `<|`
    PipeBack,
}

impl BinOp {
    /// The source spelling, for diagnostics.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Add => "+",
            Self::Sub => "-",
            Self::Mul => "*",
            Self::Div => "/",
            Self::MatMul => "@",
            Self::Pow => "^",
            Self::Eq => "==",
            Self::Ne => "!=",
            Self::Lt => "<",
            Self::Le => "<=",
            Self::Gt => ">",
            Self::Ge => ">=",
            Self::And => "&&",
            Self::Or => "||",
            Self::Cons => "::",
            Self::Pipe => "|>",
            Self::PipeBack => "<|",
        }
    }
}

/// An expression.
#[derive(Clone, Debug)]
pub struct Expr {
    /// What the expression is.
    pub kind: ExprKind,
    /// Where it came from.
    pub span: Span,
}

impl Expr {
    /// Creates an expression node.
    #[must_use]
    pub fn new(kind: ExprKind, span: Span) -> Self {
        Self { kind, span }
    }
}

/// The shape of an expression.
#[derive(Clone, Debug)]
pub enum ExprKind {
    /// `()`
    UnitLit,
    /// A recovery node: the parser reported a syntax error here.
    ///
    /// Later phases must not type-check or evaluate it — it exists so that one
    /// syntax error does not fabricate a cascade of type errors.
    Error,
    /// `true` / `false`
    Bool(bool),
    /// An integer literal.
    Int(BigInt),
    /// A floating-point literal, kept as source text for exact re-reading.
    Float(String),
    /// A string literal, possibly with interpolations.
    Str(Vec<StrPiece>),
    /// `3 m`, `9.80665 m/s^2`, `25 °C`, `50 %`.
    Quantity {
        /// The numeric coefficient.
        value: NumberLit,
        /// The unit expression; resolved during name resolution.
        unit: UnitExpr,
    },
    /// A lowercase name.
    Var(String),
    /// A dotted name such as `Units.SI.km` or `Physics.Ising.run`.
    QName(Vec<String>),
    /// A prefix operator.
    Unary {
        /// The operator.
        op: UnOp,
        /// Its operand.
        operand: Box<Expr>,
    },
    /// An infix operator.
    Binary {
        /// The operator.
        op: BinOp,
        /// Left operand.
        left: Box<Expr>,
        /// Right operand.
        right: Box<Expr>,
    },
    /// `f(x, y)`
    Call {
        /// The callee.
        callee: Box<Expr>,
        /// The arguments.
        args: Vec<Expr>,
    },
    /// `zeros<Float>(3, 4)`
    TypeApp {
        /// The callee.
        callee: Box<Expr>,
        /// Explicit type arguments.
        args: Vec<Ty>,
    },
    /// `fn (x: Real[L]) => ...`
    Lambda {
        /// The parameters.
        params: Vec<Param>,
        /// An optional return annotation.
        ret: Option<Ty>,
        /// The body.
        body: Box<Expr>,
    },
    /// `if c then a else b`
    If {
        /// The condition.
        cond: Box<Expr>,
        /// The true branch.
        then_branch: Box<Expr>,
        /// The false branch.
        else_branch: Box<Expr>,
    },
    /// `match e with | p -> ...`
    Match {
        /// The scrutinee.
        scrutinee: Box<Expr>,
        /// The arms, in order.
        arms: Vec<MatchArm>,
    },
    /// `let p = e in body`
    LetIn {
        /// The bound pattern.
        pattern: Pat,
        /// An optional annotation.
        ty: Option<Ty>,
        /// The right-hand side.
        value: Box<Expr>,
        /// The body.
        body: Box<Expr>,
    },
    /// An indented block: statements followed by a result expression.
    Block {
        /// The `let`/`fn` statements.
        stmts: Vec<BlockStmt>,
        /// The final expression, if the block has one.
        tail: Option<Box<Expr>>,
    },
    /// `do ...`
    Do {
        /// The statements.
        stmts: Vec<DoStmt>,
    },
    /// `{ x = 1, y = 2 }` or `{ p with x = 3 }`.
    Record {
        /// The record being updated, for `with`.
        base: Option<Box<Expr>>,
        /// The fields, in source order.
        fields: Vec<(String, Expr)>,
    },
    /// `p.x`
    Field {
        /// The record.
        base: Box<Expr>,
        /// The field name.
        name: String,
    },
    /// `(a, b)`
    Tuple(Vec<Expr>),
    /// `[a, b, c]`
    Tensor(Vec<Expr>),
    /// `a[i, j]`
    Index {
        /// The tensor.
        base: Box<Expr>,
        /// The indices and slices.
        indices: Vec<SliceAst>,
    },
    /// `e : T`
    Annot {
        /// The expression.
        expr: Box<Expr>,
        /// The annotation.
        ty: Ty,
    },
    /// `_` as an argument, meaning "make this a lambda".
    Placeholder,
    /// `e?`
    Try(Box<Expr>),
    /// `e!` or `e!!`
    Factorial {
        /// The operand.
        expr: Box<Expr>,
        /// True for `!!`.
        double: bool,
    },
}

/// One `match` arm.
#[derive(Clone, Debug)]
pub struct MatchArm {
    /// The pattern.
    pub pattern: Pat,
    /// The optional guard.
    pub guard: Option<Expr>,
    /// The body.
    pub body: Expr,
    /// Where the arm starts.
    pub span: Span,
}

/// A function or lambda parameter.
#[derive(Clone, Debug)]
pub struct Param {
    /// The pattern.
    pub pattern: Pat,
    /// An optional type annotation.
    pub ty: Option<Ty>,
    /// Where the parameter is written.
    pub span: Span,
}

/// A statement inside a block.
#[derive(Clone, Debug)]
pub enum BlockStmt {
    /// `let p = e`
    Let {
        /// The pattern.
        pattern: Pat,
        /// An optional annotation.
        ty: Option<Ty>,
        /// The value.
        value: Expr,
        /// Whether `rec` was written.
        rec: bool,
        /// The statement span.
        span: Span,
    },
    /// A nested function declaration.
    Fn(Box<Decl>),
}

/// A statement inside a `do` block.
#[derive(Clone, Debug)]
pub enum DoStmt {
    /// `let p = e` — a pure binding.
    Let {
        /// The pattern.
        pattern: Pat,
        /// An optional annotation.
        ty: Option<Ty>,
        /// The value.
        value: Expr,
        /// The statement span.
        span: Span,
    },
    /// `p <- action`
    Bind {
        /// The pattern.
        pattern: Pat,
        /// The action.
        value: Expr,
        /// The statement span.
        span: Span,
    },
    /// A bare action.
    Expr(Expr),
}

/// An index or slice inside `[...]`.
#[derive(Clone, Debug)]
pub enum SliceAst {
    /// `i`
    Index(Expr),
    /// `a..b`, `a..`, `..b`, `..`, `a:b:step`
    ///
    /// The `..`/`..=` forms never carry a step; the `:` form carries one when
    /// it is written, and its absence means `1`.
    Range {
        /// The start, when written.
        start: Option<Expr>,
        /// The end, when written.
        end: Option<Expr>,
        /// Whether the range is inclusive (`..=`).
        inclusive: bool,
        /// The step, when written (`a:b:s`). `None` means `1`.
        step: Option<Expr>,
    },
}

/// A pattern.
#[derive(Clone, Debug)]
pub struct Pat {
    /// What the pattern is.
    pub kind: PatKind,
    /// Where it came from.
    pub span: Span,
}

impl Pat {
    /// Creates a pattern node.
    #[must_use]
    pub fn new(kind: PatKind, span: Span) -> Self {
        Self { kind, span }
    }
}

/// The shape of a pattern.
#[derive(Clone, Debug)]
pub enum PatKind {
    /// `_`
    Wildcard,
    /// A recovery node: the parser reported a syntax error here.
    ///
    /// It binds nothing and matches nothing, so a pattern error is reported
    /// once, by the parser.
    Error,
    /// A binding.
    Var(String),
    /// `Circle(r)` or a bare `None`.
    Ctor {
        /// The constructor name.
        name: String,
        /// Its arguments.
        args: Vec<Pat>,
    },
    /// `{ x, y = p }`
    Record(Vec<(String, Option<Pat>)>),
    /// `(a, b)`
    Tuple(Vec<Pat>),
    /// `[a, b, ..rest]`
    Tensor {
        /// The leading element patterns.
        items: Vec<Pat>,
        /// The `..rest` tail, when written.
        rest: Option<String>,
    },
    /// A literal pattern.
    Lit(PatLit),
    /// `p as x`
    As {
        /// The inner pattern.
        pattern: Box<Pat>,
        /// The binding name.
        name: String,
    },
    /// `(p : T)`
    Annot {
        /// The inner pattern.
        pattern: Box<Pat>,
        /// The type.
        ty: Ty,
    },
}

/// A literal allowed in a pattern.
#[derive(Clone, Debug, PartialEq)]
pub enum PatLit {
    /// An integer.
    Int(BigInt),
    /// A string.
    Str(String),
    /// A boolean.
    Bool(bool),
    /// `()`.
    Unit,
}

/// A type expression.
#[derive(Clone, Debug)]
pub struct Ty {
    /// What the type is.
    pub kind: TyKind,
    /// Where it came from.
    pub span: Span,
}

impl Ty {
    /// Creates a type node.
    #[must_use]
    pub fn new(kind: TyKind, span: Span) -> Self {
        Self { kind, span }
    }
}

/// The shape of a type.
#[derive(Clone, Debug)]
pub enum TyKind {
    /// `Int`
    Int,
    /// `Real[d]`
    Real(DimAst),
    /// `Float[d]`
    Float(DimAst),
    /// `Complex[d]`
    Complex(DimAst),
    /// `Bool`
    Bool,
    /// `Prop`
    Prop,
    /// `String`
    Str,
    /// `()`
    Unit,
    /// `Tensor<shape, τ>`
    Tensor {
        /// The rank or refined shape.
        shape: ShapeAst,
        /// The element type.
        elem: Box<Ty>,
    },
    /// `Matrix<rows, cols, τ>` (§A.5): a rank-2 tensor with a static shape.
    Matrix {
        /// The number of rows.
        rows: NatAst,
        /// The number of columns.
        cols: NatAst,
        /// The element type.
        elem: Box<Ty>,
    },
    /// `Sparse<fmt, shape, τ>` (§A.5): a tensor in a storage format.
    Sparse {
        /// The storage format, `CSR` or `csc`.
        format: String,
        /// The rank or refined shape.
        shape: ShapeAst,
        /// The element type.
        elem: Box<Ty>,
    },
    /// A row sum, `{ a: Int } + { b: Int }` (§A.5 `row_type`).
    ///
    /// Rows are structural, so the summands are kept in source order and the
    /// checker merges them; the node exists so a formatter can print what the
    /// user wrote.
    Row(Vec<Ty>),
    /// A type argument that is a dimension, `Vec<L>` or `Vec[L]` (§A.5
    /// `type_arg`).
    ///
    /// The kind has to be kept: `L` is a dimension in `Vec<L>` and a type in
    /// `Vec<L>`, and only the constructor's declared parameter kinds tell the
    /// two apart.
    DimArg(DimAst),
    /// A type argument that is a natural number, `Tensor<n, τ>` style.
    NatArg(NatAst),
    /// A type argument that is a refined shape, `[3, ?]`.
    ShapeArg(ShapeAst),
    /// `Point[d]`
    Point(DimAst),
    /// `Unit[d]`
    UnitOf(DimAst),
    /// `Option<τ>`
    Option(Box<Ty>),
    /// `Result<τ, ε>`
    Result(Box<Ty>, Box<Ty>),
    /// `Set<τ>`
    Set(Box<Ty>),
    /// `Table<ρ>`
    Table(Box<Ty>),
    /// `Dyn<ρ>`
    Dyn(String),
    /// `RateOf<τ>`
    RateOf(Box<Ty>),
    /// `DiffusionOf<τ>` (§8.9.1): the diffusion's dimension is the state's
    /// divided by `T^(1/2)`, so it is a built-in type function like `RateOf`.
    DiffusionOf(Box<Ty>),
    /// `SymFun<d, e>`
    SymFun(DimAst, DimAst),
    /// `IO τ`
    Io(Box<Ty>),
    /// `(τ1, ..., τn) -> τ`
    Fn(Vec<Ty>, Box<Ty>),
    /// `{ x: τ, ...r }`
    Record {
        /// The declared fields.
        fields: Vec<(String, Ty)>,
        /// The open-row tail.
        rest: Option<String>,
    },
    /// `(τ1, τ2)`
    Tuple(Vec<Ty>),
    /// A named type, possibly applied.
    Named {
        /// The dotted path.
        path: Vec<String>,
        /// Type arguments.
        args: Vec<Ty>,
    },
    /// A type variable.
    Var(String),
    /// `forall a. τ`
    Forall(Vec<TypeParam>, Box<Ty>),
}

/// A tensor shape annotation.
#[derive(Clone, Debug)]
pub enum ShapeAst {
    /// A bare rank, `Tensor<2, τ>`.
    Rank(NatAst),
    /// A refined shape, `Tensor<[3, 4], τ>`.
    Refined(Vec<ShapeItem>),
}

/// One dimension of a refined shape.
#[derive(Clone, Debug)]
pub enum ShapeItem {
    /// A known extent.
    Known(NatAst),
    /// `?`
    Unknown,
}

/// A type-level natural number (§3.5.1: literals, `+`, `*` and linear
/// equations — no recursion, no proof terms).
#[derive(Clone, Debug)]
pub enum NatAst {
    /// A literal.
    Lit(usize),
    /// A variable.
    Var(String),
    /// `a + b`.
    Add(Box<NatAst>, Box<NatAst>),
    /// `a * b` (`b` must be a literal for the equation to stay linear).
    Mul(Box<NatAst>, Box<NatAst>),
}

/// A dimension expression as written in a type.
#[derive(Clone, Debug, PartialEq)]
pub enum DimAst {
    /// `1`
    One,
    /// `L`, `Length`, `d`, `Currency`.
    Name(String),
    /// `a*b`
    Mul(Box<DimAst>, Box<DimAst>),
    /// `a/b`
    Div(Box<DimAst>, Box<DimAst>),
    /// `a^n`
    Pow(Box<DimAst>, num_rational::BigRational),
    /// `(a)`
    Paren(Box<DimAst>),
}

impl DimAst {
    /// Renders the dimension back to source form.
    #[must_use]
    pub fn to_source(&self) -> String {
        match self {
            Self::One => "1".to_string(),
            Self::Name(name) => name.clone(),
            Self::Mul(left, right) => format!("{}*{}", left.to_source(), right.to_source()),
            Self::Div(left, right) => format!("{}/{}", left.to_source(), right.to_source()),
            Self::Pow(base, exponent) => {
                if exponent.denom().is_one() {
                    format!("{}^{}", base.to_source(), exponent.numer())
                } else {
                    format!("{}^{}", base.to_source(), crate::ast::format_rat(exponent))
                }
            }
            Self::Paren(inner) => format!("({})", inner.to_source()),
        }
    }
}

fn format_rat(value: &num_rational::BigRational) -> String {
    aevia_dim::format_rational(value)
}

/// The kind of a type parameter.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Kind {
    /// `Type`
    Type,
    /// `Dim`
    Dim,
    /// `Nat`
    Nat,
    /// `Row`
    Row,
    /// `Rep`
    Rep,
    /// `Format`
    Format,
    /// `Shape`
    Shape,
}

impl Kind {
    /// Parses a kind name.
    #[must_use]
    pub fn from_name(name: &str) -> Option<Self> {
        Some(match name {
            "Type" => Self::Type,
            "Dim" => Self::Dim,
            "Nat" => Self::Nat,
            "Row" => Self::Row,
            "Rep" => Self::Rep,
            "Format" => Self::Format,
            "Shape" => Self::Shape,
            _ => return None,
        })
    }

    /// The source spelling.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Type => "Type",
            Self::Dim => "Dim",
            Self::Nat => "Nat",
            Self::Row => "Row",
            Self::Rep => "Rep",
            Self::Format => "Format",
            Self::Shape => "Shape",
        }
    }
}

/// A type parameter, `<d: Dim>` or `<a>`.
#[derive(Clone, Debug)]
pub struct TypeParam {
    /// The parameter name.
    pub name: String,
    /// The declared kind, when written.
    pub kind: Option<Kind>,
    /// Where it is written.
    pub span: Span,
}

/// An attribute such as `@stable` or `@op("...")`.
#[derive(Clone, Debug)]
pub struct Attribute {
    /// The attribute name.
    pub name: String,
    /// The arguments.
    pub args: Vec<AttrArg>,
    /// Where it is written.
    pub span: Span,
}

/// One attribute argument, `name = expr` or a positional expression.
#[derive(Clone, Debug)]
pub struct AttrArg {
    /// The argument name, when written.
    pub name: Option<String>,
    /// The value.
    pub value: Expr,
}

/// A top-level declaration.
#[derive(Clone, Debug)]
pub struct Decl {
    /// Attributes attached to the declaration.
    pub attrs: Vec<Attribute>,
    /// Whether `private` was written.
    pub private: bool,
    /// The `##` documentation attached to it.
    pub doc: Option<String>,
    /// What is declared.
    pub kind: DeclKind,
    /// Where it came from.
    pub span: Span,
}

impl Decl {
    /// The attribute called `name`, when it was written.
    ///
    /// Later phases read attributes through this: `@jit` is `E1001("core.jit")`
    /// when the backend has no JIT (§9.6.1), `@op` registers an RSSN operator
    /// (§13.5) and `@deprecated` drives the stability warnings (§13.8.1).
    #[must_use]
    pub fn attribute(&self, name: &str) -> Option<&Attribute> {
        self.attrs.iter().find(|attr| attr.name == name)
    }

    /// True when the attribute called `name` was written.
    #[must_use]
    pub fn has_attribute(&self, name: &str) -> bool {
        self.attribute(name).is_some()
    }
}

/// The shape of a declaration.
#[derive(Clone, Debug)]
pub enum DeclKind {
    /// `import` or `open`
    Import {
        /// The module path.
        path: Vec<String>,
        /// `as N`
        alias: Option<String>,
        /// `{ a, b }`
        names: Option<Vec<String>>,
        /// True for `open`.
        open: bool,
    },
    /// `dimension Currency` or `dimension Area = L^2`
    Dimension {
        /// The name.
        name: String,
        /// The definition, when it is an alias.
        value: Option<DimAst>,
    },
    /// `unit km : L = 1000 m`
    Unit {
        /// The unit name.
        name: String,
        /// The dimension.
        dim: DimAst,
        /// The exact scale.
        scale: Scale,
        /// The unit the scale multiplies, when written.
        base: Option<UnitExpr>,
        /// The affine offset, when written.
        offset: Option<(Scale, Option<UnitExpr>)>,
        /// Whether `prefixable` was written.
        prefixable: bool,
    },
    /// `type Name<...> = ...`
    Type {
        /// The type name.
        name: String,
        /// Its parameters.
        params: Vec<TypeParam>,
        /// The body.
        body: TypeBody,
    },
    /// `class Show a where ...`
    Class {
        /// The class name.
        name: String,
        /// Its parameters.
        params: Vec<TypeParam>,
        /// The method signatures.
        methods: Vec<(String, Ty)>,
    },
    /// `instance Show for Vec2 where ...`
    Instance {
        /// The class name.
        class: String,
        /// The instance head.
        ty: Ty,
        /// The method definitions.
        methods: Vec<InstanceMethod>,
    },
    /// `derive Eq, Show for Vec2`
    Derive {
        /// The class names.
        classes: Vec<String>,
        /// The type.
        ty: Ty,
    },
    /// `fn f(...) = ...`
    Fn {
        /// The function name.
        name: String,
        /// Its type parameters.
        params: Vec<TypeParam>,
        /// Its arguments.
        args: Vec<Param>,
        /// The return annotation.
        ret: Option<Ty>,
        /// The body.
        body: Expr,
    },
    /// `let p = e`
    Let {
        /// The pattern.
        pattern: Pat,
        /// An optional annotation.
        ty: Option<Ty>,
        /// The value.
        value: Expr,
        /// Whether `rec` was written.
        rec: bool,
    },
    /// `test "name" = expr`
    Test {
        /// The test name.
        name: String,
        /// The body, of type `Bool`.
        body: Expr,
    },
}

/// A method definition inside an instance.
#[derive(Clone, Debug)]
pub struct InstanceMethod {
    /// The method name.
    pub name: String,
    /// Its parameters.
    pub params: Vec<Param>,
    /// The return annotation.
    pub ret: Option<Ty>,
    /// The body.
    pub body: Expr,
    /// Where it is written.
    pub span: Span,
}

/// A type declaration body.
#[derive(Clone, Debug)]
pub enum TypeBody {
    /// `type Vec2 = { x: Real[L], y: Real[L] }`
    Alias(Ty),
    /// `type Shape = | Circle(Real[L]) | Rect(Real[L], Real[L])`
    Variants(Vec<Constructor>),
    /// `type Table<ρ>` — abstract, provided by the runtime.
    Abstract,
}

/// One variant constructor.
#[derive(Clone, Debug)]
pub struct Constructor {
    /// The constructor name.
    pub name: String,
    /// Its argument types.
    pub args: Vec<Ty>,
    /// Where it is written.
    pub span: Span,
}

/// A parsed module: the contents of one `.aev` file.
#[derive(Clone, Debug)]
pub struct Module {
    /// The declared module name, if any.
    pub name: Option<Vec<String>>,
    /// The declarations, in source order.
    pub decls: Vec<Decl>,
    /// The file the module came from.
    pub file: crate::span::FileId,
}
