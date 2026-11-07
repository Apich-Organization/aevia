//! Signatures of the built-in functions.
//!
//! This table is the single place where the *names* of built-ins are decided;
//! `aevia-core` implements the same names. Keeping the signatures here means
//! the type checker never needs the runtime and the runtime never needs the
//! checker.
//!
//! Schemes are written with small variable ids (`Type::Var(0)`,
//! `Rep::Var(0)`, `Dim::var(0)`) and quantified explicitly. A name may have
//! several schemes when it is overloaded (`length` on `String` and on tensors,
//! `sum` with and without an axis); the checker tries them in order and keeps
//! the first that type-checks.

use crate::adt::Adt;
use crate::adt::{
    self,
};
use crate::ty::Constraint;
use crate::ty::Rep;
use crate::ty::Row;
use crate::ty::Scheme;
use crate::ty::Shape;
use crate::ty::Type;
use aevia_dim::Dim;
use num_rational::BigRational;

/// Builds a scheme quantifying `vars` type/representation variables, `dims`
/// dimension variables and `rows` row variables.
///
/// Every free variable a signature mentions MUST be quantified: an
/// unquantified one is shared between instantiations of the scheme, so the
/// second declaration that instantiates it unifies the shared variable and can
/// report a spurious `E0303` (occurrence). That is what happened to `on` and
/// `odeint_with`, whose bodies used `var(1)` while only `var(0)` was
/// quantified. In test builds the helper checks this directly.
fn scheme(
    vars: u32,
    dims: u32,
    rows: u32,
    ty: Type,
) -> Scheme {
    let scheme = Scheme {
        vars: (0..vars).collect(),
        dims: (0..dims).collect(),
        rows: (0..rows).collect(),
        nats: Vec::new(),
        constraints: Vec::new(),
        numeric: Vec::new(),
        materializations: Vec::new(),
        ty,
    };
    #[cfg(test)]
    assert_fully_quantified(&scheme);
    scheme
}

/// Test-only: every free type/representation, dimension and row variable of a
/// scheme's body must be in its quantified lists.
#[cfg(test)]
fn assert_fully_quantified(scheme: &Scheme) {
    use std::collections::BTreeSet;

    fn walk(
        ty: &Type,
        vars: &mut BTreeSet<u32>,
        dims: &mut BTreeSet<u32>,
        rows: &mut BTreeSet<u32>,
    ) {
        match ty {
            | Type::Scalar(rep, dim) => {
                if let Rep::Var(id) = rep {
                    vars.insert(*id);
                }
                dims.extend(dim.vars());
            },
            | Type::Tensor { elem, .. } => walk(elem, vars, dims, rows),
            | Type::Fn(params, ret) => {
                for param in params {
                    walk(param, vars, dims, rows);
                }
                walk(ret, vars, dims, rows);
            },
            | Type::Record(row) => walk_row(row, vars, dims, rows),
            | Type::Adt(_, args) => {
                for arg in args {
                    walk(arg, vars, dims, rows);
                }
            },
            | Type::Io(inner) => walk(inner, vars, dims, rows),
            | Type::Var(id) => {
                vars.insert(*id);
            },
            | Type::Point(dim) | Type::UnitOf(dim) => dims.extend(dim.vars()),
            | Type::Dyn(Rep::Var(id)) => {
                vars.insert(*id);
            },
            | Type::Bool
            | Type::Prop
            | Type::Str
            | Type::Unit
            | Type::TensorIntrinsic(_)
            | Type::TypeRepIntrinsic(_)
            | Type::FromValueIntrinsic(_)
            | Type::WriteNpyIntrinsic
            | Type::ReadNpyIntrinsic(_)
            | Type::ToCallbackIntrinsic
            | Type::Dyn(_)
            | Type::Param(_)
            | Type::Error => {},
        }
    }

    fn walk_row(
        row: &Row,
        vars: &mut BTreeSet<u32>,
        dims: &mut BTreeSet<u32>,
        rows: &mut BTreeSet<u32>,
    ) {
        match row {
            | Row::Empty => {},
            | Row::Var(id) => {
                rows.insert(*id);
            },
            | Row::Extend { ty, tail, .. } => {
                walk(ty, vars, dims, rows);
                walk_row(tail, vars, dims, rows);
            },
        }
    }

    let (mut vars, mut dims, mut rows) = (BTreeSet::new(), BTreeSet::new(), BTreeSet::new());
    walk(&scheme.ty, &mut vars, &mut dims, &mut rows);
    let extra_vars: Vec<u32> = vars
        .iter()
        .copied()
        .filter(|id| !scheme.vars.contains(id))
        .collect();
    let extra_dims: Vec<u32> = dims
        .iter()
        .copied()
        .filter(|id| !scheme.dims.contains(id))
        .collect();
    let extra_rows: Vec<u32> = rows
        .iter()
        .copied()
        .filter(|id| !scheme.rows.contains(id))
        .collect();
    assert!(
        extra_vars.is_empty() && extra_dims.is_empty() && extra_rows.is_empty(),
        "unquantified scheme variables: vars {extra_vars:?}, dims {extra_dims:?}, rows {extra_rows:?} in {scheme}"
    );
}

fn var(id: u32) -> Type {
    Type::Var(id)
}

fn dim_var(id: u32) -> Dim {
    Dim::var(id)
}

fn scalar(
    rep: Rep,
    dim: Dim,
) -> Type {
    Type::Scalar(rep, dim)
}

fn rep_var(id: u32) -> Rep {
    Rep::Var(id)
}

fn one() -> Dim {
    Dim::one()
}

fn time() -> Dim {
    Dim::base("Time")
}

fn func(
    args: Vec<Type>,
    ret: Type,
) -> Type {
    Type::func(args, ret)
}

fn tensor(
    rank: u32,
    elem: Type,
) -> Type {
    Type::tensor(Shape::Rank(rank), elem)
}

fn set(elem: Type) -> Type {
    Type::Adt(adt::set_id(), vec![elem])
}

fn list(elem: Type) -> Type {
    tensor(1, elem)
}

fn unit_ty() -> Type {
    Type::Unit
}

fn io(inner: Type) -> Type {
    Type::io(inner)
}

/// The opaque `Value` type of the dynamic ABI, interned on first use.
#[must_use]
pub fn dynamic_value_type() -> Type {
    let id = adt::intern(Adt {
        name: "Value".to_string(),
        params: Vec::new(),
        ctors: Vec::new(),
    });
    Type::Adt(id, Vec::new())
}

/// The opaque `OpError` type of the dynamic ABI.
#[must_use]
pub fn op_error_type() -> Type {
    let id = adt::intern(Adt {
        name: "OpError".to_string(),
        params: Vec::new(),
        ctors: Vec::new(),
    });
    Type::Adt(id, Vec::new())
}

/// An opaque error type such as `MathError`, interned on first use.
fn error_type(name: &str) -> Type {
    let id = adt::intern(Adt {
        name: name.to_string(),
        params: Vec::new(),
        ctors: Vec::new(),
    });
    Type::Adt(id, Vec::new())
}

/// The opaque `DimValue` of §4.9.2: a runtime dimension (base dimensions with
/// rational exponents), not to be confused with the kind `Dim`.
///
/// `dim_of` produces one and `dyn_with`/`check_with` consume one; the checker
/// does not look inside it.
#[must_use]
pub fn dim_value_type() -> Type {
    let id = adt::intern(Adt {
        name: "DimValue".to_string(),
        params: Vec::new(),
        ctors: Vec::new(),
    });
    Type::Adt(id, Vec::new())
}

/// The `ShapeError` a failed `reshape` returns (§8.4).
#[must_use]
pub fn shape_error_type() -> Type {
    error_type("ShapeError")
}

/// The `DimError` returned by a failed `check`/`check_with` (§4.9.2).
#[must_use]
pub fn dim_error_type() -> Type {
    error_type("DimError")
}

/// `Solutions<ρ,d>` of §8.6: the roots, their multiplicities, the conditions
/// the answer is subject to, the variable that was solved for, and whether the
/// solver claimed to be exhaustive.
///
/// It is a record rather than an opaque type so that `solve(f).roots` works.
fn solutions(element: Type) -> Type {
    Type::record(vec![
        ("variable".to_string(), Type::option(Type::string())),
        ("roots".to_string(), Type::tensor(Shape::Rank(1), element)),
        (
            "multiplicities".to_string(),
            Type::tensor(Shape::Rank(1), Type::int()),
        ),
        (
            "conditions".to_string(),
            Type::tensor(Shape::Rank(1), error_type("Condition")),
        ),
        ("complete".to_string(), Type::bool()),
    ])
}

fn half() -> BigRational {
    BigRational::new(1.into(), 2.into())
}

fn third() -> BigRational {
    BigRational::new(1.into(), 3.into())
}

fn two() -> BigRational {
    BigRational::from_integer(2.into())
}

/// `Ordering`, the result of `compare`/`total_compare` (§8.1.2).
fn ordering() -> Type {
    Type::Adt(adt::ORDERING_ID, Vec::new())
}

/// A tuple type, which is a record with the fields `0`, `1`, … (§3.5).
fn tuple(fields: Vec<Type>) -> Type {
    Type::record(
        fields
            .into_iter()
            .enumerate()
            .map(|(index, ty)| (index.to_string(), ty))
            .collect::<Vec<_>>(),
    )
}

/// The opaque `Table<ρ>` of §8.11.
fn table(row: Type) -> Type {
    Type::Adt(
        adt::intern(Adt {
            name: "Table".to_string(),
            params: vec!["r".to_string()],
            ctors: Vec::new(),
        }),
        vec![row],
    )
}

/// The opaque `Json` of §8.11.
fn json() -> Type {
    error_type("Json")
}

/// The opaque `Sparse<F, shape, τ>` of §8.4.1.
fn sparse(element: Type) -> Type {
    Type::Adt(
        adt::intern(Adt {
            name: "Sparse".to_string(),
            params: vec!["f".to_string(), "shape".to_string(), "t".to_string()],
            ctors: Vec::new(),
        }),
        vec![error_type("Format"), element],
    )
}

/// The opaque `Series` of §8.12.
fn series() -> Type {
    Type::Adt(
        adt::intern(Adt {
            name: "Series".to_string(),
            params: Vec::new(),
            ctors: Vec::new(),
        }),
        Vec::new(),
    )
}

/// The `Field` of §8.10: a grid and the values on it.
fn field() -> Type {
    Type::record(vec![
        ("grid".to_string(), error_type("Grid")),
        (
            "values".to_string(),
            Type::tensor(Shape::Rank(2), Type::float(Dim::var(0))),
        ),
    ])
}

/// `Measurement<d> = { value, uncertainty }` (§8.14).
fn measurement(dim: Dim) -> Type {
    Type::record(vec![
        ("value".to_string(), Type::real(dim.clone())),
        ("uncertainty".to_string(), Type::real(dim)),
    ])
}

/// Returns every scheme attached to a built-in name, if it is one.
#[must_use]
pub fn schemes(name: &str) -> Option<Vec<Scheme>> {
    Some(match name {
        // ----- constants -------------------------------------------------
        | "pi" | "e" | "oo" => vec![Scheme::mono(Type::real(one()))],
        | "nan" => vec![Scheme::mono(Type::float(one()))],
        | "i" => vec![Scheme::mono(Type::complex(one()))],

        // ----- elementary functions --------------------------------------
        | "sqrt" => {
            vec![
                scheme(
                    1,
                    1,
                    0,
                    func(
                        vec![scalar(rep_var(0), dim_var(0))],
                        scalar(rep_var(0), dim_var(0).pow(&half())),
                    ),
                ),
                // §4.9.3: `sqrt : Dyn<ρ> -> Dyn<ρ>` halves the runtime exponent.
                scheme(
                    1,
                    0,
                    0,
                    func(vec![Type::Dyn(rep_var(0))], Type::Dyn(rep_var(0))),
                ),
            ]
        },
        // ----- §3.11 / §4.9 dynamic dimension -----------------------------
        // `dyn` and `check` are type-directed primitives: the checker reads
        // the concrete `d` at the call site and materialises it (§4.9.4), so
        // their schemes quantify it too. The rest of the escape layer is
        // ordinary HM.
        | "dyn" => {
            vec![scheme(
                1,
                1,
                0,
                func(vec![scalar(rep_var(0), dim_var(0))], Type::Dyn(rep_var(0))),
            )]
        },
        | "dyn_with" => {
            vec![scheme(
                1,
                1,
                0,
                func(
                    vec![scalar(rep_var(0), dim_var(0)), dim_value_type()],
                    Type::Dyn(rep_var(0)),
                ),
            )]
        },
        | "check" => {
            vec![scheme(
                1,
                1,
                0,
                func(
                    vec![Type::Dyn(rep_var(0))],
                    Type::result(scalar(rep_var(0), dim_var(0)), dim_error_type()),
                ),
            )]
        },
        | "check_with" => {
            vec![scheme(
                1,
                0,
                0,
                func(
                    vec![dim_value_type(), Type::Dyn(rep_var(0))],
                    Type::result(scalar(rep_var(0), one()), dim_error_type()),
                ),
            )]
        },
        | "dim_of" => {
            vec![scheme(
                1,
                0,
                0,
                func(vec![Type::Dyn(rep_var(0))], dim_value_type()),
            )]
        },
        | "value_of" => {
            vec![scheme(
                1,
                0,
                0,
                func(vec![Type::Dyn(rep_var(0))], scalar(rep_var(0), one())),
            )]
        },
        | "pow_dyn" => {
            vec![scheme(
                1,
                0,
                0,
                func(
                    vec![Type::Dyn(rep_var(0)), Type::int()],
                    Type::Dyn(rep_var(0)),
                ),
            )]
        },
        | "cbrt" => {
            vec![scheme(
                1,
                1,
                0,
                func(
                    vec![scalar(rep_var(0), dim_var(0))],
                    scalar(rep_var(0), dim_var(0).pow(&third())),
                ),
            )]
        },
        | "exp" | "ln" | "log" | "log2" | "log10" | "sin" | "cos" | "tan" | "asin" | "acos"
        | "atan" | "sinh" | "cosh" | "tanh" | "asinh" | "acosh" | "atanh" | "gamma"
        | "lgamma" | "erf" | "erfc" | "zeta" => {
            vec![scheme(
                1,
                0,
                0,
                func(vec![scalar(rep_var(0), one())], scalar(rep_var(0), one())),
            )]
        },
        | "atan2" => {
            vec![scheme(
                1,
                1,
                0,
                func(
                    vec![
                        scalar(rep_var(0), dim_var(0)),
                        scalar(rep_var(0), dim_var(0)),
                    ],
                    scalar(rep_var(0), one()),
                ),
            )]
        },
        | "hypot" => {
            vec![scheme(
                1,
                1,
                0,
                func(
                    vec![
                        scalar(rep_var(0), dim_var(0)),
                        scalar(rep_var(0), dim_var(0)),
                    ],
                    scalar(rep_var(0), dim_var(0)),
                ),
            )]
        },
        | "abs" => {
            vec![scheme(
                1,
                1,
                0,
                func(
                    vec![scalar(rep_var(0), dim_var(0))],
                    scalar(rep_var(0), dim_var(0)),
                ),
            )]
        },
        | "sign" => {
            vec![scheme(
                1,
                1,
                0,
                func(vec![scalar(rep_var(0), dim_var(0))], Type::int()),
            )]
        },
        | "min" | "max" => {
            vec![scheme(
                1,
                1,
                0,
                func(
                    vec![
                        scalar(rep_var(0), dim_var(0)),
                        scalar(rep_var(0), dim_var(0)),
                    ],
                    scalar(rep_var(0), dim_var(0)),
                ),
            )]
        },
        | "clamp" => {
            vec![scheme(
                1,
                1,
                0,
                func(
                    vec![
                        scalar(rep_var(0), dim_var(0)),
                        scalar(rep_var(0), dim_var(0)),
                        scalar(rep_var(0), dim_var(0)),
                    ],
                    scalar(rep_var(0), dim_var(0)),
                ),
            )]
        },
        | "floor" | "ceil" | "round" | "trunc" => {
            vec![scheme(
                1,
                1,
                0,
                func(
                    vec![scalar(rep_var(0), dim_var(0))],
                    scalar(rep_var(0), dim_var(0)),
                ),
            )]
        },
        | "mod" | "rem" => {
            vec![scheme(
                1,
                1,
                0,
                func(
                    vec![
                        scalar(rep_var(0), dim_var(0)),
                        scalar(rep_var(0), dim_var(0)),
                    ],
                    scalar(rep_var(0), dim_var(0)),
                ),
            )]
        },
        | "div" | "quot" | "gcd" | "lcm" => {
            vec![Scheme::mono(func(
                vec![Type::int(), Type::int()],
                Type::int(),
            ))]
        },
        | "factorial" | "double_factorial" => {
            vec![Scheme::mono(func(vec![Type::int()], Type::int()))]
        },
        | "to_float" => {
            vec![scheme(
                1,
                1,
                0,
                func(
                    vec![scalar(rep_var(0), dim_var(0))],
                    Type::result(Type::float(dim_var(0)), error_type("EvalError")),
                ),
            )]
        },
        | "to_real" => {
            vec![scheme(
                1,
                1,
                0,
                func(vec![scalar(rep_var(0), dim_var(0))], Type::real(dim_var(0))),
            )]
        },
        | "to_int" => {
            vec![scheme(
                1,
                0,
                0,
                func(vec![scalar(rep_var(0), one())], Type::int()),
            )]
        },

        // ----- unit conversion -------------------------------------------
        | "to" | "delta_to" => {
            vec![
                scheme(
                    1,
                    1,
                    0,
                    func(
                        vec![Type::UnitOf(dim_var(0)), scalar(rep_var(0), dim_var(0))],
                        scalar(rep_var(0), dim_var(0)),
                    ),
                ),
                scheme(
                    0,
                    1,
                    0,
                    func(
                        vec![Type::UnitOf(dim_var(0)), Type::Point(dim_var(0))],
                        Type::Point(dim_var(0)),
                    ),
                ),
            ]
        },
        | "value_in" => {
            vec![
                scheme(
                    1,
                    1,
                    0,
                    func(
                        vec![Type::UnitOf(dim_var(0)), scalar(rep_var(0), dim_var(0))],
                        scalar(rep_var(0), one()),
                    ),
                ),
                scheme(
                    0,
                    1,
                    0,
                    func(
                        vec![Type::UnitOf(dim_var(0)), Type::Point(dim_var(0))],
                        Type::float(one()),
                    ),
                ),
            ]
        },
        | "base" => {
            vec![scheme(
                1,
                1,
                0,
                func(
                    vec![scalar(rep_var(0), dim_var(0))],
                    scalar(rep_var(0), dim_var(0)),
                ),
            )]
        },
        | "point" => {
            vec![scheme(
                0,
                1,
                0,
                func(
                    vec![Type::float(one()), Type::UnitOf(dim_var(0))],
                    Type::Point(dim_var(0)),
                ),
            )]
        },

        // ----- tensors ----------------------------------------------------
        | "linspace" => {
            vec![scheme(
                1,
                1,
                0,
                func(
                    vec![
                        scalar(rep_var(0), dim_var(0)),
                        scalar(rep_var(0), dim_var(0)),
                        Type::int(),
                    ],
                    list(scalar(rep_var(0), dim_var(0))),
                ),
            )]
        },
        | "range" => {
            vec![
                Scheme::mono(func(vec![Type::int(), Type::int()], list(Type::int()))),
                Scheme::mono(func(
                    vec![Type::int(), Type::int(), Type::int()],
                    list(Type::int()),
                )),
            ]
        },
        | "length" => {
            vec![
                Scheme::mono(func(vec![Type::string()], Type::int())),
                scheme(1, 0, 0, func(vec![tensor(1, var(0))], Type::int())),
                scheme(1, 0, 0, func(vec![set(var(0))], Type::int())),
            ]
        },
        // ----- §8.1.9 sets -------------------------------------------------
        | "set_of" => vec![scheme(1, 0, 0, func(vec![list(var(0))], set(var(0))))],
        | "elements" => vec![scheme(1, 0, 0, func(vec![set(var(0))], list(var(0))))],
        | "member" => {
            vec![scheme(
                1,
                0,
                0,
                func(vec![var(0), set(var(0))], Type::bool()),
            )]
        },
        | "set_insert" | "set_remove" => {
            vec![scheme(
                1,
                0,
                0,
                func(vec![set(var(0)), var(0)], set(var(0))),
            )]
        },
        | "union" | "intersection" | "difference" => {
            vec![scheme(
                1,
                0,
                0,
                func(vec![set(var(0)), set(var(0))], set(var(0))),
            )]
        },
        | "is_subset" => {
            vec![scheme(
                1,
                0,
                0,
                func(vec![set(var(0)), set(var(0))], Type::bool()),
            )]
        },
        | "shape" => vec![scheme(1, 0, 0, func(vec![var(0)], list(Type::int())))],
        | "rank" => vec![scheme(1, 0, 0, func(vec![var(0)], Type::int()))],
        | "sum" => {
            vec![
                scheme(
                    1,
                    1,
                    0,
                    func(
                        vec![tensor(1, scalar(rep_var(0), dim_var(0)))],
                        scalar(rep_var(0), dim_var(0)),
                    ),
                ),
                scheme(
                    1,
                    1,
                    0,
                    func(
                        vec![tensor(1, scalar(rep_var(0), dim_var(0))), Type::int()],
                        tensor(1, scalar(rep_var(0), dim_var(0))),
                    ),
                ),
                // §3.5.5: the axis form is *keepdims*, so a rank-2 reduction keeps
                // rank 2 (`sum(A, 1)` is a column vector). Rank is not first-class,
                // so each supported rank gets an overload.
                scheme(
                    1,
                    1,
                    0,
                    func(
                        vec![tensor(2, scalar(rep_var(0), dim_var(0))), Type::int()],
                        tensor(2, scalar(rep_var(0), dim_var(0))),
                    ),
                ),
                // §8.5: `sum(f, a, b)` is the calculus overload, and the index is
                // a dimensionless `Real` so `k` may be symbolic.
                scheme(
                    0,
                    1,
                    0,
                    func(
                        vec![
                            func(vec![Type::real(one())], Type::real(dim_var(0))),
                            Type::real(one()),
                            Type::real(one()),
                        ],
                        Type::real(dim_var(0)),
                    ),
                ),
            ]
        },
        | "prod" => {
            vec![scheme(
                1,
                0,
                0,
                func(
                    vec![tensor(1, scalar(rep_var(0), one()))],
                    scalar(rep_var(0), one()),
                ),
            )]
        },
        | "mean" => {
            vec![scheme(
                1,
                1,
                0,
                func(
                    vec![tensor(1, scalar(rep_var(0), dim_var(0)))],
                    scalar(rep_var(0).div_rep(), dim_var(0)),
                ),
            )]
        },
        | "var" => {
            vec![
                scheme(
                    1,
                    1,
                    0,
                    func(
                        vec![tensor(1, scalar(rep_var(0), dim_var(0)))],
                        scalar(
                            rep_var(0).div_rep(),
                            dim_var(0).pow(&BigRational::from_integer(2.into())),
                        ),
                    ),
                ),
                // §8.7: `var(xs, true)` is the population variance.
                scheme(
                    1,
                    1,
                    0,
                    func(
                        vec![tensor(1, scalar(rep_var(0), dim_var(0))), Type::bool()],
                        scalar(
                            rep_var(0).div_rep(),
                            dim_var(0).pow(&BigRational::from_integer(2.into())),
                        ),
                    ),
                ),
            ]
        },
        | "std" => {
            vec![scheme(
                1,
                1,
                0,
                func(
                    vec![tensor(1, scalar(rep_var(0), dim_var(0)))],
                    scalar(rep_var(0).div_rep(), dim_var(0)),
                ),
            )]
        },
        | "min_all" | "max_all" => {
            vec![scheme(
                1,
                1,
                0,
                func(
                    vec![tensor(1, scalar(rep_var(0), dim_var(0)))],
                    scalar(rep_var(0), dim_var(0)),
                ),
            )]
        },
        | "norm" => {
            vec![
                scheme(
                    1,
                    1,
                    0,
                    func(
                        vec![tensor(1, scalar(rep_var(0), dim_var(0)))],
                        scalar(rep_var(0), dim_var(0)),
                    ),
                ),
                scheme(
                    1,
                    1,
                    0,
                    func(
                        vec![tensor(1, scalar(rep_var(0), dim_var(0))), Type::int()],
                        scalar(rep_var(0), dim_var(0)),
                    ),
                ),
            ]
        },
        | "normalize" => {
            vec![scheme(
                1,
                1,
                0,
                func(
                    vec![tensor(1, scalar(rep_var(0), dim_var(0)))],
                    tensor(1, scalar(rep_var(0), one())),
                ),
            )]
        },
        | "dot" => {
            vec![scheme(
                1,
                2,
                0,
                func(
                    vec![
                        tensor(1, scalar(rep_var(0), dim_var(0))),
                        tensor(1, scalar(rep_var(0), dim_var(1))),
                    ],
                    scalar(rep_var(0), dim_var(0).mul(&dim_var(1))),
                ),
            )]
        },
        | "matmul" => {
            vec![scheme(
                1,
                2,
                0,
                func(
                    vec![
                        tensor(2, scalar(rep_var(0), dim_var(0))),
                        tensor(2, scalar(rep_var(0), dim_var(1))),
                    ],
                    tensor(2, scalar(rep_var(0), dim_var(0).mul(&dim_var(1)))),
                ),
            )]
        },
        | "matvec" => {
            vec![scheme(
                1,
                2,
                0,
                func(
                    vec![
                        tensor(2, scalar(rep_var(0), dim_var(0))),
                        tensor(1, scalar(rep_var(0), dim_var(1))),
                    ],
                    tensor(1, scalar(rep_var(0), dim_var(0).mul(&dim_var(1)))),
                ),
            )]
        },
        | "vecmat" => {
            vec![scheme(
                1,
                2,
                0,
                func(
                    vec![
                        tensor(1, scalar(rep_var(0), dim_var(0))),
                        tensor(2, scalar(rep_var(0), dim_var(1))),
                    ],
                    tensor(1, scalar(rep_var(0), dim_var(0).mul(&dim_var(1)))),
                ),
            )]
        },
        | "outer" => {
            vec![scheme(
                1,
                2,
                0,
                func(
                    vec![
                        tensor(1, scalar(rep_var(0), dim_var(0))),
                        tensor(1, scalar(rep_var(0), dim_var(1))),
                    ],
                    tensor(2, scalar(rep_var(0), dim_var(0).mul(&dim_var(1)))),
                ),
            )]
        },
        | "transpose" => {
            vec![scheme(
                1,
                0,
                0,
                func(vec![tensor(2, var(0))], tensor(2, var(0))),
            )]
        },
        | "trace" => {
            vec![scheme(
                1,
                1,
                0,
                func(
                    vec![tensor(2, scalar(rep_var(0), dim_var(0)))],
                    scalar(rep_var(0), dim_var(0)),
                ),
            )]
        },
        | "det" => {
            vec![scheme(
                1,
                0,
                0,
                func(
                    vec![tensor(2, scalar(rep_var(0), one()))],
                    scalar(rep_var(0), one()),
                ),
            )]
        },
        | "inv" => {
            vec![scheme(
                1,
                1,
                0,
                func(
                    vec![tensor(2, scalar(rep_var(0), dim_var(0)))],
                    Type::result(
                        tensor(2, scalar(rep_var(0).div_rep(), dim_var(0).inverse())),
                        error_type("LinAlgError"),
                    ),
                ),
            )]
        },
        | "linsolve" => {
            vec![scheme(
                1,
                2,
                0,
                func(
                    vec![
                        tensor(2, scalar(rep_var(0), dim_var(0))),
                        tensor(1, scalar(rep_var(0), dim_var(1))),
                    ],
                    Type::result(
                        tensor(1, scalar(rep_var(0).div_rep(), dim_var(1).div(&dim_var(0)))),
                        error_type("LinAlgError"),
                    ),
                ),
            )]
        },
        | "eye" => {
            vec![Scheme::mono(func(
                vec![Type::int()],
                tensor(2, Type::float(one())),
            ))]
        },
        | "diag" => {
            vec![
                scheme(1, 0, 0, func(vec![list(var(0))], tensor(2, var(0)))),
                scheme(1, 0, 0, func(vec![tensor(2, var(0))], list(var(0)))),
            ]
        },
        | "flatten" => vec![scheme(1, 0, 0, func(vec![tensor(2, var(0))], list(var(0))))],
        // §3.5.5/§8.4: `at : (Tensor<n, τ>, Int) -> Tensor<n-1, τ>`. Rank is
        // not first-class here, so the ranks the runtime supports have their
        // own overload; the checker picks by the argument's rank. The *unit*
        // `at` (technical atmosphere) is a separate namespace (§1.8.2), so
        // `at(t, 0)` reaches this function.
        | "at" => {
            vec![
                scheme(1, 0, 0, func(vec![list(var(0)), Type::int()], var(0))),
                scheme(
                    1,
                    0,
                    0,
                    func(vec![tensor(2, var(0)), Type::int()], list(var(0))),
                ),
                scheme(
                    1,
                    0,
                    0,
                    func(vec![tensor(3, var(0)), Type::int()], tensor(2, var(0))),
                ),
            ]
        },
        | "map" => {
            vec![
                scheme(
                    2,
                    0,
                    0,
                    func(vec![func(vec![var(0)], var(1)), list(var(0))], list(var(1))),
                ),
                scheme(
                    2,
                    0,
                    0,
                    func(
                        vec![func(vec![var(0)], var(1)), Type::option(var(0))],
                        Type::option(var(1)),
                    ),
                ),
            ]
        },
        | "map2" => {
            vec![scheme(
                3,
                0,
                0,
                func(
                    vec![
                        func(vec![var(0), var(1)], var(2)),
                        list(var(0)),
                        list(var(1)),
                    ],
                    list(var(2)),
                ),
            )]
        },
        | "filter" => {
            vec![
                scheme(
                    1,
                    0,
                    0,
                    func(
                        vec![func(vec![var(0)], Type::bool()), list(var(0))],
                        list(var(0)),
                    ),
                ),
                // §8.11: `filter` on a table keeps the row type.
                scheme(
                    1,
                    0,
                    0,
                    func(
                        vec![func(vec![var(0)], Type::bool()), table(var(0))],
                        table(var(0)),
                    ),
                ),
            ]
        },
        | "fold" | "fold_left" => {
            vec![scheme(
                2,
                0,
                0,
                func(
                    vec![func(vec![var(0), var(1)], var(0)), var(0), list(var(1))],
                    var(0),
                ),
            )]
        },
        | "sort_by" => {
            vec![scheme(
                1,
                0,
                0,
                func(
                    vec![func(vec![var(0), var(0)], ordering()), list(var(0))],
                    list(var(0)),
                ),
            )]
        },
        | "sort_float" => {
            vec![Scheme::mono(func(
                vec![list(Type::float(one())), Type::bool()],
                list(Type::float(one())),
            ))]
        },
        // §3.8.4/§6.10: explicit parallelism requires `Parallel a` — `Int`,
        // `Float` and tensors of them; symbolic `Real` is not parallel. The
        // constraint is enforced at the call site, where the element type is
        // known (`Checker::par_map_call`).
        | "par_map" => {
            vec![Scheme {
                vars: vec![0, 1],
                dims: Vec::new(),
                rows: Vec::new(),
                nats: Vec::new(),
                constraints: vec![Constraint::new("Parallel", vec![var(0)])],
                numeric: Vec::new(),
                materializations: Vec::new(),
                ty: func(vec![func(vec![var(0)], var(1)), list(var(0))], list(var(1))),
            }]
        },
        // §6.10: `rng(seed)` is the pure, reproducible generator; `random()` is
        // the `IO` one. The value is a next-number function.
        | "rng" => {
            vec![scheme(
                0,
                0,
                0,
                func(
                    vec![Type::int()],
                    func(vec![Type::Unit], Type::float(one())),
                ),
            )]
        },
        | "zip" => {
            vec![scheme(
                2,
                0,
                0,
                func(
                    vec![list(var(0)), list(var(1))],
                    list(Type::Record(Row::closed([
                        ("0".to_string(), var(0)),
                        ("1".to_string(), var(1)),
                    ]))),
                ),
            )]
        },
        | "zip_with" => {
            vec![scheme(
                3,
                0,
                0,
                func(
                    vec![
                        func(vec![var(0), var(1)], var(2)),
                        list(var(0)),
                        list(var(1)),
                    ],
                    list(var(2)),
                ),
            )]
        },
        | "enumerate" => {
            vec![scheme(
                1,
                0,
                0,
                func(
                    vec![list(var(0))],
                    list(Type::Record(Row::closed([
                        ("index".to_string(), Type::int()),
                        ("value".to_string(), var(0)),
                    ]))),
                ),
            )]
        },
        | "take" | "drop" => {
            vec![scheme(
                1,
                0,
                0,
                func(vec![Type::int(), list(var(0))], list(var(0))),
            )]
        },
        | "reverse" => vec![scheme(1, 0, 0, func(vec![list(var(0))], list(var(0))))],
        | "sort" => vec![scheme(1, 0, 0, func(vec![list(var(0))], list(var(0))))],
        | "concat" => {
            vec![
                Scheme::mono(func(vec![Type::string(), Type::string()], Type::string())),
                scheme(
                    1,
                    0,
                    0,
                    func(vec![list(var(0)), list(var(0))], list(var(0))),
                ),
            ]
        },
        | "any" | "all" => {
            vec![scheme(
                1,
                0,
                0,
                func(
                    vec![func(vec![var(0)], Type::bool()), list(var(0))],
                    Type::bool(),
                ),
            )]
        },
        | "find" => {
            vec![scheme(
                1,
                0,
                0,
                func(
                    vec![func(vec![var(0)], Type::bool()), list(var(0))],
                    Type::option(var(0)),
                ),
            )]
        },
        | "centroid" => {
            vec![scheme(
                0,
                1,
                0,
                func(vec![list(Type::Point(dim_var(0)))], Type::Point(dim_var(0))),
            )]
        },

        // ----- strings ----------------------------------------------------
        | "join" => {
            vec![
                Scheme::mono(func(
                    vec![Type::string(), list(Type::string())],
                    Type::string(),
                )),
                // §8.11: `join` on two tables, keyed by a column name.
                scheme(
                    3,
                    0,
                    0,
                    func(
                        vec![table(var(0)), table(var(1)), Type::string()],
                        table(var(2)),
                    ),
                ),
            ]
        },
        | "split" => {
            vec![Scheme::mono(func(
                vec![Type::string(), Type::string()],
                list(Type::string()),
            ))]
        },
        | "replace" => {
            vec![Scheme::mono(func(
                vec![Type::string(), Type::string(), Type::string()],
                Type::string(),
            ))]
        },
        | "trim" | "upper" | "lower" => {
            vec![Scheme::mono(func(vec![Type::string()], Type::string()))]
        },
        | "contains" | "starts_with" | "ends_with" => {
            vec![Scheme::mono(func(
                vec![Type::string(), Type::string()],
                Type::bool(),
            ))]
        },
        | "parse_int" => {
            vec![Scheme::mono(func(
                vec![Type::string()],
                Type::result(Type::int(), error_type("ParseError")),
            ))]
        },
        | "parse_float" => {
            vec![Scheme::mono(func(
                vec![Type::string()],
                Type::result(Type::float(one()), error_type("ParseError")),
            ))]
        },
        | "format" => {
            vec![Scheme::mono(func(
                vec![Type::string(), list(Type::string())],
                Type::string(),
            ))]
        },
        | "chars" => {
            vec![Scheme::mono(func(
                vec![Type::string()],
                list(Type::string()),
            ))]
        },

        // ----- Option / Result -------------------------------------------
        | "and_then" => {
            vec![scheme(
                2,
                0,
                0,
                func(
                    vec![
                        func(vec![var(0)], Type::option(var(1))),
                        Type::option(var(0)),
                    ],
                    Type::option(var(1)),
                ),
            )]
        },
        | "unwrap_or" => {
            vec![scheme(
                1,
                0,
                0,
                func(vec![Type::option(var(0)), var(0)], var(0)),
            )]
        },
        | "is_some" | "is_none" => {
            vec![scheme(
                1,
                0,
                0,
                func(vec![Type::option(var(0))], Type::bool()),
            )]
        },
        | "map_err" => {
            vec![scheme(
                3,
                0,
                0,
                func(
                    vec![func(vec![var(1)], var(2)), Type::result(var(0), var(1))],
                    Type::result(var(0), var(2)),
                ),
            )]
        },

        // ----- display and IO --------------------------------------------
        | "show" | "repr" | "to_latex" | "to_typst" => {
            vec![scheme(1, 0, 0, func(vec![var(0)], Type::string()))]
        },
        | "print" | "println" | "eprintln" => {
            vec![scheme(1, 0, 0, func(vec![var(0)], io(unit_ty())))]
        },
        | "read_file" => {
            vec![Scheme::mono(func(
                vec![Type::string()],
                io(Type::result(Type::string(), error_type("IOError"))),
            ))]
        },
        | "write_file" => {
            vec![Scheme::mono(func(
                vec![Type::string(), Type::string()],
                io(Type::result(unit_ty(), error_type("IOError"))),
            ))]
        },
        | "read_line" => vec![Scheme::mono(func(vec![unit_ty()], io(Type::string())))],
        // §9.14 / §13.9.6: `with_cap(caps, thunk)` runs the action the thunk
        // builds with only the named capabilities (a comma-separated list of
        // `fs`, `net`, `env`, `rand`, `gpu`) still granted.
        | "with_cap" => {
            vec![scheme(
                1,
                0,
                0,
                func(
                    vec![Type::string(), func(Vec::new(), io(var(0)))],
                    io(var(0)),
                ),
            )]
        },
        // `args : () -> Tensor<1, String>` — the program's arguments, an empty
        // tensor when the program was started without any (§8.1; the runtime
        // supplies the value).
        | "args" => vec![Scheme::mono(func(vec![unit_ty()], list(Type::string())))],
        | "time_now" => vec![Scheme::mono(func(vec![unit_ty()], io(Type::float(time()))))],
        | "random" => vec![Scheme::mono(func(vec![unit_ty()], io(Type::float(one()))))],
        | "read_csv_dyn" => {
            vec![Scheme::mono(func(
                vec![Type::string()],
                io(Type::result(
                    Type::Adt(
                        adt::intern(Adt {
                            name: "Table".to_string(),
                            params: vec!["r".to_string()],
                            ctors: Vec::new(),
                        }),
                        vec![error_type("Dynamic")],
                    ),
                    error_type("DataError"),
                )),
            ))]
        },

        // ----- symbolic ---------------------------------------------------
        | "diff" => {
            vec![scheme(
                1,
                2,
                0,
                func(
                    vec![func(vec![Type::real(dim_var(0))], Type::real(dim_var(1)))],
                    func(
                        vec![Type::real(dim_var(0))],
                        Type::real(dim_var(1).div(&dim_var(0))),
                    ),
                ),
            )]
        },
        | "diff_at" => {
            vec![scheme(
                1,
                2,
                0,
                func(
                    vec![
                        func(vec![Type::real(dim_var(0))], Type::real(dim_var(1))),
                        Type::real(dim_var(0)),
                    ],
                    Type::real(dim_var(1).div(&dim_var(0))),
                ),
            )]
        },
        | "antiderivative" => {
            vec![scheme(
                1,
                2,
                0,
                func(
                    vec![func(vec![Type::real(dim_var(0))], Type::real(dim_var(1)))],
                    func(
                        vec![Type::real(dim_var(0))],
                        Type::real(dim_var(1).mul(&dim_var(0))),
                    ),
                ),
            )]
        },
        | "defint" => {
            vec![scheme(
                1,
                2,
                0,
                func(
                    vec![
                        func(vec![Type::real(dim_var(0))], Type::real(dim_var(1))),
                        Type::real(dim_var(0)),
                        Type::real(dim_var(0)),
                    ],
                    Type::result(
                        Type::real(dim_var(1).mul(&dim_var(0))),
                        error_type("MathError"),
                    ),
                ),
            )]
        },
        | "limit" => {
            vec![scheme(
                1,
                2,
                0,
                func(
                    vec![
                        func(vec![Type::real(dim_var(0))], Type::real(dim_var(1))),
                        Type::real(dim_var(0)),
                    ],
                    Type::result(Type::real(dim_var(1)), error_type("MathError")),
                ),
            )]
        },
        | "series" => {
            vec![scheme(
                1,
                2,
                0,
                func(
                    vec![
                        func(vec![Type::real(dim_var(0))], Type::real(dim_var(1))),
                        Type::real(dim_var(0)),
                        Type::int(),
                    ],
                    Type::real(dim_var(1)),
                ),
            )]
        },
        | "solve" => {
            vec![scheme(
                1,
                2,
                0,
                func(
                    vec![func(vec![Type::real(dim_var(0))], Type::real(dim_var(1)))],
                    Type::result(solutions(Type::real(dim_var(0))), error_type("MathError")),
                ),
            )]
        },
        | "nsolve" => {
            vec![scheme(
                1,
                2,
                0,
                func(
                    vec![
                        func(vec![Type::real(dim_var(0))], Type::real(dim_var(1))),
                        Type::real(dim_var(0)),
                    ],
                    Type::result(Type::real(dim_var(0)), error_type("MathError")),
                ),
            )]
        },
        | "eval" | "simplify" => {
            vec![scheme(
                1,
                1,
                0,
                func(vec![Type::real(dim_var(0))], Type::real(dim_var(0))),
            )]
        },
        // ----- §8.5.1 algebraic normal forms -------------------------------
        // Rewrites that change the *form* of an exact expression and keep its
        // value, so the dimension is unchanged.
        | "expand" | "cancel" | "together" => {
            vec![scheme(
                0,
                1,
                0,
                func(vec![Type::real(dim_var(0))], Type::real(dim_var(0))),
            )]
        },
        | "apart" | "collect" => {
            vec![scheme(
                0,
                2,
                0,
                func(
                    vec![Type::real(dim_var(0)), Type::real(dim_var(1))],
                    Type::real(dim_var(0)),
                ),
            )]
        },
        | "degree" => {
            vec![scheme(
                0,
                2,
                0,
                func(
                    vec![Type::real(dim_var(0)), Type::real(dim_var(1))],
                    Type::int(),
                ),
            )]
        },
        | "approx" => {
            vec![scheme(
                1,
                1,
                0,
                func(
                    vec![Type::real(dim_var(0))],
                    Type::result(Type::float(dim_var(0)), error_type("EvalError")),
                ),
            )]
        },
        | "subst" => {
            vec![scheme(
                1,
                3,
                0,
                func(
                    vec![
                        Type::real(dim_var(0)),
                        Type::real(dim_var(1)),
                        Type::real(dim_var(2)),
                    ],
                    Type::real(dim_var(2)),
                ),
            )]
        },
        | "to_value" => {
            vec![scheme(
                1,
                0,
                0,
                func(vec![Type::Var(0)], dynamic_value_type()),
            )]
        },
        | "from_value" => {
            vec![scheme(
                1,
                0,
                0,
                func(
                    vec![dynamic_value_type()],
                    Type::Adt(adt::RESULT_ID, vec![Type::Var(0), op_error_type()]),
                ),
            )]
        },
        | "decide" => {
            vec![
                // A symbolic (exact `Real`) comparison is a `Prop`.
                Scheme::mono(func(vec![Type::Prop], Type::option(Type::bool()))),
                // A comparison on machine numbers is already a `Bool`.
                Scheme::mono(func(vec![Type::Bool], Type::option(Type::bool()))),
            ]
        },
        | "assuming" => {
            vec![scheme(
                1,
                0,
                0,
                func(vec![Type::Prop, func(Vec::new(), var(0))], var(0)),
            )]
        },
        | "by_cases" => {
            vec![scheme(
                0,
                1,
                0,
                func(
                    vec![
                        Type::Prop,
                        func(Vec::new(), Type::real(dim_var(0))),
                        func(Vec::new(), Type::real(dim_var(0))),
                    ],
                    Type::real(dim_var(0)),
                ),
            )]
        },

        // ----- testing ----------------------------------------------------
        | "assert" => vec![Scheme::mono(func(vec![Type::bool()], io(unit_ty())))],
        | "assert_eq" => vec![scheme(1, 0, 0, func(vec![var(0), var(0)], io(unit_ty())))],
        | "approx_eq" => {
            vec![scheme(
                1,
                1,
                0,
                func(
                    vec![
                        scalar(rep_var(0), dim_var(0)),
                        scalar(rep_var(0), dim_var(0)),
                        scalar(rep_var(0), dim_var(0)),
                    ],
                    Type::bool(),
                ),
            )]
        },
        | "forall" => {
            vec![scheme(
                1,
                0,
                0,
                func(vec![func(vec![var(0)], Type::Prop)], Type::bool()),
            )]
        },

        // ----- plotting ---------------------------------------------------
        | "line" | "scatter" => {
            vec![scheme(
                1,
                2,
                0,
                func(
                    vec![
                        tensor(1, scalar(rep_var(0), dim_var(0))),
                        tensor(1, scalar(rep_var(0), dim_var(1))),
                    ],
                    Type::Adt(
                        adt::intern(Adt {
                            name: "Series".to_string(),
                            params: Vec::new(),
                            ctors: Vec::new(),
                        }),
                        Vec::new(),
                    ),
                ),
            )]
        },
        | "label" => {
            vec![Scheme::mono(func(
                vec![
                    Type::Adt(
                        adt::intern(Adt {
                            name: "Series".to_string(),
                            params: Vec::new(),
                            ctors: Vec::new(),
                        }),
                        Vec::new(),
                    ),
                    Type::string(),
                ],
                Type::Adt(
                    adt::intern(Adt {
                        name: "Series".to_string(),
                        params: Vec::new(),
                        ctors: Vec::new(),
                    }),
                    Vec::new(),
                ),
            ))]
        },
        | "combine" => {
            vec![Scheme::mono(func(
                vec![list(Type::Adt(
                    adt::intern(Adt {
                        name: "Series".to_string(),
                        params: Vec::new(),
                        ctors: Vec::new(),
                    }),
                    Vec::new(),
                ))],
                Type::Adt(
                    adt::intern(Adt {
                        name: "PlotSpec".to_string(),
                        params: Vec::new(),
                        ctors: Vec::new(),
                    }),
                    Vec::new(),
                ),
            ))]
        },
        | "plot" => {
            vec![Scheme::mono(func(
                vec![Type::Adt(
                    adt::intern(Adt {
                        name: "PlotSpec".to_string(),
                        params: Vec::new(),
                        ctors: Vec::new(),
                    }),
                    Vec::new(),
                )],
                io(unit_ty()),
            ))]
        },
        | "save" => {
            vec![Scheme::mono(func(
                vec![
                    Type::string(),
                    Type::Adt(
                        adt::intern(Adt {
                            name: "PlotSpec".to_string(),
                            params: Vec::new(),
                            ctors: Vec::new(),
                        }),
                        Vec::new(),
                    ),
                ],
                io(unit_ty()),
            ))]
        },

        // ----- §8.1.1 function combinators --------------------------------
        | "id" => vec![scheme(1, 0, 0, func(vec![var(0)], var(0)))],
        | "const" => vec![scheme(2, 0, 0, func(vec![var(0), var(1)], var(0)))],
        | "compose" => {
            vec![scheme(
                3,
                0,
                0,
                func(
                    vec![func(vec![var(1)], var(2)), func(vec![var(0)], var(1))],
                    func(vec![var(0)], var(2)),
                ),
            )]
        },
        | "flip" => {
            vec![scheme(
                3,
                0,
                0,
                func(
                    vec![func(vec![var(0), var(1)], var(2))],
                    func(vec![var(1), var(0)], var(2)),
                ),
            )]
        },
        | "curry" => {
            vec![scheme(
                3,
                0,
                0,
                func(
                    vec![func(vec![var(0), var(1)], var(2))],
                    func(vec![var(0)], func(vec![var(1)], var(2))),
                ),
            )]
        },
        | "uncurry" => {
            vec![scheme(
                3,
                0,
                0,
                func(
                    vec![func(vec![var(0)], func(vec![var(1)], var(2)))],
                    func(vec![var(0), var(1)], var(2)),
                ),
            )]
        },
        | "apply" => {
            vec![scheme(
                2,
                0,
                0,
                func(vec![func(vec![var(0)], var(1)), var(0)], var(1)),
            )]
        },
        // §8.3: the function version of `f(a, _)`.
        | "partial" => {
            vec![scheme(
                3,
                0,
                0,
                func(
                    vec![func(vec![var(0), var(1)], var(2)), var(0)],
                    func(vec![var(1)], var(2)),
                ),
            )]
        },

        // ----- §8.1.2 booleans and comparisons ----------------------------
        | "not" => vec![Scheme::mono(func(vec![Type::bool()], Type::bool()))],
        | "xor" => {
            vec![Scheme::mono(func(
                vec![Type::bool(), Type::bool()],
                Type::bool(),
            ))]
        },
        | "compare" => {
            vec![scheme(
                1,
                0,
                0,
                func(vec![var(0), var(0)], Type::option(ordering())),
            )]
        },
        | "partial_compare" => {
            vec![Scheme::mono(func(
                vec![Type::float(one()), Type::float(one())],
                Type::option(ordering()),
            ))]
        },
        | "total_compare" => vec![scheme(1, 0, 0, func(vec![var(0), var(0)], ordering()))],
        | "sym_eq" | "sym_ne" | "sym_lt" => {
            vec![scheme(
                0,
                1,
                0,
                func(
                    vec![Type::real(dim_var(0)), Type::real(dim_var(0))],
                    Type::Prop,
                ),
            )]
        },

        // ----- §8.1.3 `zero` ----------------------------------------------
        | "zero" => vec![scheme(1, 1, 0, scalar(rep_var(0), dim_var(0)))],

        // ----- §8.2.2 special functions -----------------------------------
        | "beta" => {
            vec![scheme(
                1,
                0,
                0,
                func(
                    vec![scalar(rep_var(0), one()), scalar(rep_var(0), one())],
                    scalar(rep_var(0), one()),
                ),
            )]
        },
        | "bessel_j0" | "bessel_j1" | "bessel_y0" | "bessel_y1" | "j1" | "y1" => {
            vec![scheme(
                1,
                0,
                0,
                func(vec![scalar(rep_var(0), one())], scalar(rep_var(0), one())),
            )]
        },
        | "bessel_jn" | "bessel_yn" | "legendre_p" | "hermite_h" | "laguerre_l" | "chebyshev_t"
        | "chebyshev_u" => {
            vec![scheme(
                1,
                0,
                0,
                func(
                    vec![Type::int(), scalar(rep_var(0), one())],
                    scalar(rep_var(0), one()),
                ),
            )]
        },

        // ----- §8.4 decompositions and shape operations -------------------
        | "lstsq" => {
            vec![scheme(
                1,
                2,
                0,
                func(
                    vec![
                        tensor(2, scalar(rep_var(0), dim_var(0))),
                        tensor(1, scalar(rep_var(0), dim_var(1))),
                    ],
                    Type::result(
                        tensor(1, scalar(rep_var(0).div_rep(), dim_var(1).div(&dim_var(0)))),
                        error_type("LinAlgError"),
                    ),
                ),
            )]
        },
        | "eig" => {
            vec![scheme(
                1,
                1,
                0,
                func(
                    vec![tensor(2, scalar(rep_var(0), dim_var(0)))],
                    tuple(vec![
                        tensor(1, Type::complex(dim_var(0))),
                        tensor(2, Type::complex(one())),
                    ]),
                ),
            )]
        },
        | "eigvals" => {
            vec![scheme(
                1,
                1,
                0,
                func(
                    vec![tensor(2, scalar(rep_var(0), dim_var(0)))],
                    tensor(1, Type::complex(dim_var(0))),
                ),
            )]
        },
        | "svd" => {
            vec![scheme(
                1,
                1,
                0,
                func(
                    vec![tensor(2, scalar(rep_var(0), dim_var(0)))],
                    tuple(vec![
                        tensor(2, scalar(rep_var(0), one())),
                        tensor(1, scalar(rep_var(0), dim_var(0))),
                        tensor(2, scalar(rep_var(0), one())),
                    ]),
                ),
            )]
        },
        | "qr" => {
            vec![scheme(
                1,
                1,
                0,
                func(
                    vec![tensor(2, scalar(rep_var(0), dim_var(0)))],
                    tuple(vec![
                        tensor(2, scalar(rep_var(0), one())),
                        tensor(2, scalar(rep_var(0), dim_var(0))),
                    ]),
                ),
            )]
        },
        | "lu" => {
            vec![scheme(
                1,
                1,
                0,
                func(
                    vec![tensor(2, scalar(rep_var(0), dim_var(0)))],
                    tuple(vec![
                        tensor(2, scalar(rep_var(0), one())),
                        tensor(2, scalar(rep_var(0), one())),
                        tensor(2, scalar(rep_var(0), dim_var(0))),
                    ]),
                ),
            )]
        },
        | "cholesky" => {
            vec![scheme(
                1,
                1,
                0,
                func(
                    vec![tensor(2, scalar(rep_var(0), dim_var(0)))],
                    Type::result(
                        tensor(2, scalar(rep_var(0), dim_var(0).pow(&half()))),
                        error_type("LinAlgError"),
                    ),
                ),
            )]
        },
        | "tril" | "triu" => {
            vec![scheme(
                1,
                0,
                0,
                func(vec![tensor(2, var(0))], tensor(2, var(0))),
            )]
        },
        // `reshape<shape>` names its target in a *shape* type argument, which
        // `Checker::shape_constructor` reads (§3.5.1). This scheme is the
        // fallback for a call without one: the runtime reports that it needs a
        // shape, so the static type stays the argument's own type.
        | "reshape" => {
            vec![scheme(
                1,
                0,
                0,
                func(vec![var(0)], Type::result(var(0), error_type("ShapeError"))),
            )]
        },
        | "reshape_dyn" => {
            vec![scheme(
                1,
                0,
                0,
                func(
                    vec![tensor(2, var(0)), list(Type::int())],
                    Type::result(tensor(2, var(0)), error_type("ShapeError")),
                ),
            )]
        },
        // §3.5.5/§8.4: `set : (Tensor<n, τ>, Tensor<1,Int>, τ) -> Tensor<n, τ>`.
        | "set" => {
            vec![
                scheme(
                    2,
                    0,
                    0,
                    func(vec![list(var(0)), list(Type::int()), var(0)], list(var(0))),
                ),
                scheme(
                    2,
                    0,
                    0,
                    func(
                        vec![tensor(2, var(0)), list(Type::int()), var(0)],
                        tensor(2, var(0)),
                    ),
                ),
                scheme(
                    2,
                    0,
                    0,
                    func(
                        vec![tensor(3, var(0)), list(Type::int()), var(0)],
                        tensor(3, var(0)),
                    ),
                ),
            ]
        },
        | "map_indexed" => {
            vec![scheme(
                2,
                0,
                0,
                func(
                    vec![func(vec![Type::int(), var(0)], var(1)), list(var(0))],
                    list(var(1)),
                ),
            )]
        },
        | "approx_map" => {
            vec![scheme(
                1,
                2,
                0,
                func(
                    vec![
                        func(vec![Type::real(dim_var(0))], Type::real(dim_var(1))),
                        list(Type::real(dim_var(0))),
                    ],
                    Type::result(list(Type::float(dim_var(1))), error_type("EvalError")),
                ),
            )]
        },
        | "det_unit" => {
            vec![scheme(
                1,
                1,
                0,
                func(
                    vec![
                        tensor(2, scalar(rep_var(0), dim_var(0))),
                        Type::UnitOf(dim_var(0)),
                    ],
                    Type::Dyn(rep_var(0)),
                ),
            )]
        },
        // §8.4.1: the sparse layout family.
        | "from_dense" => {
            vec![scheme(
                1,
                0,
                0,
                func(vec![tensor(2, var(0))], sparse(var(0))),
            )]
        },
        | "to_dense" => {
            vec![scheme(
                1,
                0,
                0,
                func(vec![sparse(var(0))], tensor(2, var(0))),
            )]
        },
        | "nnz" => vec![scheme(1, 0, 0, func(vec![sparse(var(0))], Type::int()))],

        // §8.4: refined-shape constructors. The shape lives in a type argument
        // the checker does not read yet, so the static rank is the common
        // two-dimensional one; the runtime builds whatever the argument says.
        | "zeros_of" | "ones_of" => {
            vec![Scheme::mono(func(
                Vec::new(),
                tensor(2, Type::float(one())),
            ))]
        },

        // ----- §8.5 numeric calculus --------------------------------------
        | "numeric_deriv" => {
            vec![scheme(
                0,
                2,
                0,
                func(
                    vec![func(vec![Type::float(dim_var(0))], Type::float(dim_var(1)))],
                    func(
                        vec![Type::float(dim_var(0))],
                        Type::float(dim_var(1).div(&dim_var(0))),
                    ),
                ),
            )]
        },
        | "quadrature" => {
            vec![scheme(
                0,
                2,
                0,
                func(
                    vec![
                        func(vec![Type::float(dim_var(0))], Type::float(dim_var(1))),
                        Type::float(dim_var(0)),
                        Type::float(dim_var(0)),
                    ],
                    Type::result(
                        Type::float(dim_var(1).mul(&dim_var(0))),
                        error_type("MathError"),
                    ),
                ),
            )]
        },
        | "product" => {
            vec![scheme(
                0,
                0,
                0,
                func(
                    vec![
                        func(vec![Type::real(one())], Type::real(one())),
                        Type::real(one()),
                        Type::real(one()),
                    ],
                    Type::real(one()),
                ),
            )]
        },
        | "product_dyn" => {
            vec![scheme(
                0,
                1,
                0,
                func(
                    vec![
                        func(vec![Type::real(one())], Type::real(dim_var(0))),
                        Type::real(one()),
                        Type::real(one()),
                    ],
                    Type::Dyn(Rep::Real),
                ),
            )]
        },
        | "sum_f" => {
            vec![scheme(
                0,
                1,
                0,
                func(
                    vec![
                        func(vec![Type::float(one())], Type::float(dim_var(0))),
                        Type::float(one()),
                        Type::float(one()),
                    ],
                    Type::float(dim_var(0)),
                ),
            )]
        },
        | "grad" => {
            vec![scheme(
                0,
                2,
                0,
                func(
                    vec![
                        func(vec![list(Type::real(dim_var(0)))], Type::real(dim_var(1))),
                        list(Type::real(dim_var(0))),
                    ],
                    list(Type::real(dim_var(1).div(&dim_var(0)))),
                ),
            )]
        },
        | "grad_f" => {
            vec![scheme(
                0,
                2,
                0,
                func(
                    vec![
                        func(vec![list(Type::float(dim_var(0)))], Type::float(dim_var(1))),
                        list(Type::float(dim_var(0))),
                    ],
                    list(Type::float(dim_var(1).div(&dim_var(0)))),
                ),
            )]
        },
        | "jacobian" => {
            vec![scheme(
                0,
                2,
                0,
                func(
                    vec![
                        func(
                            vec![list(Type::real(dim_var(0)))],
                            list(Type::real(dim_var(1))),
                        ),
                        list(Type::real(dim_var(0))),
                    ],
                    tensor(2, Type::real(dim_var(1).div(&dim_var(0)))),
                ),
            )]
        },
        | "hessian" => {
            vec![scheme(
                0,
                2,
                0,
                func(
                    vec![
                        func(vec![list(Type::real(dim_var(0)))], Type::real(dim_var(1))),
                        list(Type::real(dim_var(0))),
                    ],
                    tensor(2, Type::real(dim_var(1).div(&dim_var(0).pow(&two())))),
                ),
            )]
        },
        | "divergence" => {
            vec![scheme(
                0,
                2,
                0,
                func(
                    vec![
                        func(
                            vec![list(Type::real(dim_var(0)))],
                            list(Type::real(dim_var(1))),
                        ),
                        list(Type::real(dim_var(0))),
                    ],
                    Type::real(dim_var(1).div(&dim_var(0))),
                ),
            )]
        },
        | "curl" => {
            vec![scheme(
                0,
                2,
                0,
                func(
                    vec![
                        func(
                            vec![list(Type::real(dim_var(0)))],
                            list(Type::real(dim_var(1))),
                        ),
                        list(Type::real(dim_var(0))),
                    ],
                    list(Type::real(dim_var(1).div(&dim_var(0)))),
                ),
            )]
        },
        | "laplacian" => {
            vec![scheme(
                0,
                2,
                0,
                func(
                    vec![
                        func(vec![list(Type::real(dim_var(0)))], Type::real(dim_var(1))),
                        list(Type::real(dim_var(0))),
                    ],
                    Type::real(dim_var(1).div(&dim_var(0).pow(&two()))),
                ),
            )]
        },

        // ----- §8.6 solving and optimisation ------------------------------
        | "solve_complex" => {
            vec![scheme(
                0,
                2,
                0,
                func(
                    vec![func(
                        vec![Type::real(dim_var(0))],
                        Type::complex(dim_var(1)),
                    )],
                    Type::result(
                        solutions(Type::complex(dim_var(0))),
                        error_type("MathError"),
                    ),
                ),
            )]
        },
        | "nsolve_f" | "root" => {
            vec![scheme(
                0,
                2,
                0,
                func(
                    vec![
                        func(vec![Type::float(dim_var(0))], Type::float(dim_var(1))),
                        Type::float(dim_var(0)),
                    ],
                    Type::result(Type::float(dim_var(0)), error_type("MathError")),
                ),
            )]
        },
        | "solve_system" => {
            vec![scheme(
                0,
                2,
                0,
                func(
                    vec![
                        func(
                            vec![list(Type::real(dim_var(0)))],
                            list(Type::real(dim_var(1))),
                        ),
                        list(Type::real(dim_var(0))),
                    ],
                    Type::result(list(Type::real(dim_var(0))), error_type("MathError")),
                ),
            )]
        },
        | "minimize" | "maximize" => {
            vec![scheme(
                0,
                2,
                0,
                func(
                    vec![
                        func(vec![list(Type::real(dim_var(0)))], Type::real(dim_var(1))),
                        list(Type::real(dim_var(0))),
                    ],
                    Type::result(list(Type::real(dim_var(0))), error_type("MathError")),
                ),
            )]
        },
        | "minimize_f" => {
            vec![scheme(
                0,
                2,
                0,
                func(
                    vec![
                        func(vec![list(Type::float(dim_var(0)))], Type::float(dim_var(1))),
                        list(Type::float(dim_var(0))),
                    ],
                    Type::result(list(Type::float(dim_var(0))), error_type("MathError")),
                ),
            )]
        },
        | "least_squares" => {
            vec![scheme(
                0,
                2,
                0,
                func(
                    vec![
                        func(
                            vec![list(Type::real(dim_var(0)))],
                            list(Type::real(dim_var(1))),
                        ),
                        list(Type::real(dim_var(0))),
                    ],
                    Type::result(list(Type::real(dim_var(0))), error_type("MathError")),
                ),
            )]
        },

        // ----- §8.7 statistics --------------------------------------------
        | "median" => {
            vec![scheme(
                1,
                1,
                0,
                func(
                    vec![tensor(1, scalar(rep_var(0), dim_var(0)))],
                    scalar(rep_var(0), dim_var(0)),
                ),
            )]
        },
        | "quantile" => {
            vec![scheme(
                1,
                1,
                0,
                func(
                    vec![
                        tensor(1, scalar(rep_var(0), dim_var(0))),
                        scalar(rep_var(0), one()),
                    ],
                    scalar(rep_var(0), dim_var(0)),
                ),
            )]
        },
        | "cov" => {
            vec![scheme(
                1,
                2,
                0,
                func(
                    vec![
                        tensor(1, scalar(rep_var(0), dim_var(0))),
                        tensor(1, scalar(rep_var(0), dim_var(1))),
                    ],
                    scalar(rep_var(0).div_rep(), dim_var(0).mul(&dim_var(1))),
                ),
            )]
        },
        | "corr" => {
            vec![scheme(
                1,
                2,
                0,
                func(
                    vec![
                        tensor(1, scalar(rep_var(0), dim_var(0))),
                        tensor(1, scalar(rep_var(0), dim_var(1))),
                    ],
                    scalar(rep_var(0).div_rep(), one()),
                ),
            )]
        },
        | "skewness" | "kurtosis" => {
            vec![scheme(
                1,
                1,
                0,
                func(
                    vec![tensor(1, scalar(rep_var(0), dim_var(0)))],
                    scalar(rep_var(0), one()),
                ),
            )]
        },
        | "histogram" => {
            vec![scheme(
                1,
                1,
                0,
                func(
                    vec![tensor(1, scalar(rep_var(0), dim_var(0))), Type::int()],
                    Type::record(vec![
                        ("counts".to_string(), tensor(1, Type::int())),
                        (
                            "edges".to_string(),
                            tensor(1, scalar(rep_var(0), dim_var(0))),
                        ),
                    ]),
                ),
            )]
        },
        | "mode" => {
            vec![scheme(
                1,
                1,
                0,
                func(
                    vec![tensor(1, scalar(rep_var(0), dim_var(0)))],
                    scalar(rep_var(0), dim_var(0)),
                ),
            )]
        },
        // §8.7.1: the distribution interface. Without type classes (PLAN D3)
        // the distribution argument is an unconstrained variable; the runtime
        // reads its fields.
        | "pdf" => {
            vec![scheme(
                1,
                1,
                0,
                func(
                    vec![var(0), Type::real(dim_var(0))],
                    Type::real(dim_var(0).inverse()),
                ),
            )]
        },
        | "cdf" => {
            vec![scheme(
                1,
                1,
                0,
                func(vec![var(0), Type::real(dim_var(0))], Type::real(one())),
            )]
        },
        | "sample" => {
            vec![scheme(
                1,
                1,
                0,
                func(vec![var(0)], io(Type::real(dim_var(0)))),
            )]
        },
        | "sample_n" => {
            vec![scheme(
                1,
                1,
                0,
                func(
                    vec![var(0), Type::int(), Type::option(Type::int())],
                    list(Type::real(dim_var(0))),
                ),
            )]
        },

        // ----- §8.8 FFT and interpolation ---------------------------------
        | "fft" | "ifft" => {
            vec![scheme(
                0,
                1,
                0,
                func(
                    vec![list(Type::complex(dim_var(0)))],
                    list(Type::complex(dim_var(0))),
                ),
            )]
        },
        | "fft_norm" => {
            vec![scheme(
                1,
                1,
                0,
                func(
                    vec![list(Type::complex(dim_var(0))), var(0)],
                    list(Type::complex(dim_var(0))),
                ),
            )]
        },
        | "rfft" => {
            vec![scheme(
                0,
                1,
                0,
                func(
                    vec![list(Type::float(dim_var(0)))],
                    list(Type::complex(dim_var(0))),
                ),
            )]
        },
        | "irfft" => {
            vec![scheme(
                0,
                1,
                0,
                func(
                    vec![list(Type::complex(dim_var(0)))],
                    list(Type::float(dim_var(0))),
                ),
            )]
        },
        | "fftfreq" => {
            vec![scheme(
                0,
                1,
                0,
                func(
                    vec![Type::int(), Type::float(dim_var(0))],
                    list(Type::float(dim_var(0).inverse())),
                ),
            )]
        },
        | "interp1" | "spline" => {
            vec![scheme(
                1,
                2,
                0,
                func(
                    vec![
                        tensor(1, scalar(rep_var(0), dim_var(0))),
                        tensor(1, scalar(rep_var(0), dim_var(1))),
                        scalar(rep_var(0), dim_var(0)),
                    ],
                    scalar(rep_var(0), dim_var(1)),
                ),
            )]
        },
        | "interp2" => {
            vec![scheme(
                1,
                3,
                0,
                func(
                    vec![
                        tensor(1, scalar(rep_var(0), dim_var(0))),
                        tensor(1, scalar(rep_var(0), dim_var(1))),
                        tensor(2, scalar(rep_var(0), dim_var(2))),
                        scalar(rep_var(0), dim_var(0)),
                        scalar(rep_var(0), dim_var(1)),
                    ],
                    scalar(rep_var(0), dim_var(2)),
                ),
            )]
        },
        | "polyfit" | "chebyshev_fit" => {
            vec![scheme(
                1,
                1,
                0,
                func(
                    vec![
                        tensor(1, scalar(rep_var(0), one())),
                        tensor(1, scalar(rep_var(0), dim_var(0))),
                        Type::int(),
                    ],
                    tensor(1, scalar(rep_var(0), dim_var(0))),
                ),
            )]
        },
        | "polyval" => {
            vec![scheme(
                1,
                1,
                0,
                func(
                    vec![
                        tensor(1, scalar(rep_var(0), dim_var(0))),
                        scalar(rep_var(0), one()),
                    ],
                    scalar(rep_var(0), dim_var(0)),
                ),
            )]
        },

        // ----- §8.9 ODE ---------------------------------------------------
        // `RateOf<s>` is a built-in type function, so no HM scheme can state
        // these signatures exactly. The entries below keep the names in the
        // built-in table (the runtime dispatches on it); the checker types a
        // call through `ode_call`, which applies §8.9's `RateOf` recursively
        // and rejects a non-`Float` state field with `E0302`.
        | "odeint" => {
            vec![scheme(
                1,
                1,
                0,
                func(
                    vec![
                        func(vec![Type::float(time()), var(0)], var(0)),
                        var(0),
                        list(Type::float(time())),
                    ],
                    list(var(0)),
                ),
            )]
        },
        | "odeint_with" => {
            vec![scheme(
                2,
                1,
                0,
                func(
                    vec![
                        func(vec![Type::float(time()), var(0)], var(0)),
                        var(0),
                        list(Type::float(time())),
                        var(1),
                    ],
                    Type::record(vec![
                        ("t".to_string(), list(Type::float(time()))),
                        ("y".to_string(), list(var(0))),
                    ]),
                ),
            )]
        },
        | "solve_ivp" => {
            vec![scheme(
                2,
                1,
                0,
                func(
                    vec![
                        func(vec![Type::float(time()), var(0)], var(0)),
                        var(0),
                        tuple(vec![Type::float(time()), Type::float(time())]),
                        var(1),
                    ],
                    Type::record(vec![
                        ("t".to_string(), list(Type::float(time()))),
                        ("y".to_string(), list(var(0))),
                    ]),
                ),
            )]
        },

        // ----- §8.9.1 SDE --------------------------------------------------
        // The two entry points are typed by `Checker::sde_call` through the
        // built-in type functions `RateOf`/`DiffusionOf`; this loose scheme
        // exists so the names resolve and an unimported call still reports
        // the arity/type errors. `sde_order`'s argument is loose because the
        // `SdeMethod` constructors are not values (the runtime selects the
        // method by name, as for `OdeMethod`/`Norm`).
        | "sdeint_diag" | "sdeint_general" => {
            vec![scheme(
                2,
                0,
                0,
                func(vec![var(0), var(0), var(0), var(0)], var(1)),
            )]
        },
        // `wiener(seed, m, steps, dt) -> Tensor<2, Float[T^(1/2)]]`: the
        // increments are Brownian (`dW ~ √dt`), matching the kernel manifest
        // `sde_wiener(seed, m, steps, h)` argument order (§8.9.1).
        | "wiener" => {
            vec![scheme(
                0,
                1,
                0,
                func(
                    vec![
                        Type::int(),
                        Type::int(),
                        Type::int(),
                        Type::float(dim_var(0)),
                    ],
                    tensor(
                        2,
                        Type::float(Dim::base("Time").pow(&BigRational::new(1.into(), 2.into()))),
                    ),
                ),
            )]
        },
        | "sde_order" => {
            vec![scheme(
                1,
                0,
                0,
                func(
                    vec![var(0)],
                    Type::record(vec![
                        ("strong".to_string(), Type::float(one())),
                        ("weak".to_string(), Type::float(one())),
                    ]),
                ),
            )]
        },
        | "sde_ensemble_seed" => {
            vec![scheme(
                0,
                0,
                0,
                func(vec![Type::int(), Type::int()], Type::int()),
            )]
        },

        // ----- §8.10 PDE --------------------------------------------------
        | "solve_poisson" => {
            vec![scheme(
                1,
                1,
                0,
                func(vec![var(0)], Type::result(field(), error_type("PdeError"))),
            )]
        },
        | "solve_heat" | "solve_wave" | "solve_advection" => {
            vec![scheme(
                1,
                1,
                0,
                func(
                    vec![var(0)],
                    Type::result(list(field()), error_type("PdeError")),
                ),
            )]
        },

        // ----- §8.11 data -------------------------------------------------
        | "read_csv" => {
            vec![scheme(
                1,
                0,
                0,
                func(
                    vec![Type::string()],
                    io(Type::result(table(var(0)), error_type("DataError"))),
                ),
            )]
        },
        | "write_csv" => {
            vec![scheme(
                1,
                0,
                0,
                func(
                    vec![Type::string(), table(var(0))],
                    io(Type::result(unit_ty(), error_type("DataError"))),
                ),
            )]
        },
        | "read_npy" => vec![Scheme::mono(Type::ReadNpyIntrinsic(None))],
        | "write_npy" => vec![Scheme::mono(Type::WriteNpyIntrinsic)],
        // §8.17: the simulation boundary is `Json`. A typed wrapper encodes its
        // parameters with `to_json`, runs `simulate`, and decodes the answer
        // with `from_json` — a typed façade over a dynamic boundary rather than
        // a polymorphic `∀a.a` lie. The `a` in `from_json` is fixed by the
        // caller's declared result type and erased at run time.
        | "to_json" => vec![scheme(1, 0, 0, func(vec![var(0)], json()))],
        | "from_json" => {
            vec![scheme(
                1,
                0,
                0,
                func(vec![json()], Type::result(var(0), error_type("SimError"))),
            )]
        },
        | "read_json" => {
            vec![Scheme::mono(func(
                vec![Type::string()],
                io(Type::result(json(), error_type("DataError"))),
            ))]
        },
        | "write_json" => {
            vec![Scheme::mono(func(
                vec![Type::string(), json()],
                io(Type::result(unit_ty(), error_type("DataError"))),
            ))]
        },
        | "column" => {
            vec![scheme(
                2,
                0,
                0,
                func(
                    vec![table(var(0)), Type::string()],
                    Type::result(list(var(1)), error_type("DataError")),
                ),
            )]
        },
        | "column_dyn" => {
            vec![scheme(
                1,
                0,
                0,
                func(
                    vec![table(var(0)), Type::string()],
                    Type::result(list(dynamic_value_type()), error_type("DataError")),
                ),
            )]
        },
        | "row" => {
            vec![scheme(
                1,
                0,
                0,
                func(
                    vec![table(var(0)), Type::int()],
                    Type::result(var(0), error_type("DataError")),
                ),
            )]
        },
        | "select" => {
            vec![scheme(
                2,
                0,
                0,
                func(vec![table(var(0)), list(Type::string())], table(var(1))),
            )]
        },
        | "schema" => {
            vec![scheme(
                1,
                0,
                0,
                func(
                    vec![table(var(0))],
                    list(Type::record(vec![
                        ("name".to_string(), Type::string()),
                        ("dtype".to_string(), Type::string()),
                        ("unit".to_string(), Type::option(Type::string())),
                    ])),
                ),
            )]
        },
        | "to_table" => {
            vec![scheme(
                1,
                0,
                0,
                func(
                    vec![list(var(0))],
                    Type::result(table(var(0)), error_type("DataError")),
                ),
            )]
        },
        | "to_dynamic" => {
            vec![scheme(
                1,
                0,
                0,
                func(vec![table(var(0))], table(error_type("Dynamic"))),
            )]
        },

        // ----- §8.12 plotting ---------------------------------------------
        | "line_with" | "scatter_with" => {
            vec![scheme(
                1,
                2,
                0,
                func(
                    vec![
                        tensor(1, scalar(rep_var(0), dim_var(0))),
                        tensor(1, scalar(rep_var(0), dim_var(1))),
                        var(0),
                    ],
                    series(),
                ),
            )]
        },
        | "errorbar" => {
            vec![scheme(
                1,
                2,
                0,
                func(
                    vec![
                        tensor(1, scalar(rep_var(0), dim_var(0))),
                        tensor(1, scalar(rep_var(0), dim_var(1))),
                        tensor(1, scalar(rep_var(0), dim_var(1))),
                    ],
                    series(),
                ),
            )]
        },
        | "errorbar_with" => {
            vec![scheme(
                1,
                2,
                0,
                func(
                    vec![
                        tensor(1, scalar(rep_var(0), dim_var(0))),
                        tensor(1, scalar(rep_var(0), dim_var(1))),
                        tensor(1, scalar(rep_var(0), dim_var(1))),
                        var(0),
                    ],
                    series(),
                ),
            )]
        },
        | "hist" => {
            vec![scheme(
                1,
                1,
                0,
                func(
                    vec![tensor(1, scalar(rep_var(0), dim_var(0))), Type::int()],
                    series(),
                ),
            )]
        },
        | "hist_with" => {
            vec![scheme(
                1,
                1,
                0,
                func(
                    vec![
                        tensor(1, scalar(rep_var(0), dim_var(0))),
                        Type::int(),
                        var(0),
                    ],
                    series(),
                ),
            )]
        },
        | "heatmap" => {
            vec![scheme(
                1,
                1,
                0,
                func(vec![tensor(2, scalar(rep_var(0), dim_var(0)))], series()),
            )]
        },
        | "heatmap_with" => {
            vec![scheme(
                1,
                1,
                0,
                func(
                    vec![tensor(2, scalar(rep_var(0), dim_var(0))), var(0)],
                    series(),
                ),
            )]
        },
        | "default_plot_opts" => vec![Scheme::mono(error_type("PlotOpts"))],

        // ----- §8.13 testing ----------------------------------------------
        | "compile_fail" => vec![Scheme::mono(func(vec![Type::string()], Type::bool()))],

        // ----- §8.14 measurement ------------------------------------------
        | "measure" => {
            vec![scheme(
                0,
                1,
                0,
                func(
                    vec![Type::real(dim_var(0)), Type::real(dim_var(0))],
                    measurement(dim_var(0)),
                ),
            )]
        },
        | "value" | "uncertainty" => {
            vec![scheme(
                0,
                1,
                0,
                func(vec![measurement(dim_var(0))], Type::real(dim_var(0))),
            )]
        },

        // ----- §8.15 complex ----------------------------------------------
        | "complex" => {
            vec![Scheme::mono(func(
                vec![Type::real(one()), Type::real(one())],
                Type::complex(one()),
            ))]
        },
        | "re" | "im" => {
            vec![scheme(
                0,
                1,
                0,
                func(vec![Type::complex(dim_var(0))], Type::real(dim_var(0))),
            )]
        },
        | "conj" => {
            vec![scheme(
                0,
                1,
                0,
                func(vec![Type::complex(dim_var(0))], Type::complex(dim_var(0))),
            )]
        },
        | "magnitude" => {
            vec![scheme(
                0,
                1,
                0,
                func(vec![Type::complex(dim_var(0))], Type::real(dim_var(0))),
            )]
        },
        | "arg" => {
            vec![scheme(
                0,
                1,
                0,
                func(vec![Type::complex(dim_var(0))], Type::real(one())),
            )]
        },
        | "complex_sqrt" => {
            vec![scheme(
                0,
                1,
                0,
                func(
                    vec![Type::complex(dim_var(0))],
                    Type::complex(dim_var(0).pow(&half())),
                ),
            )]
        },
        | "complex_exp" | "complex_ln" | "complex_sin" | "complex_cos" => {
            vec![Scheme::mono(func(
                vec![Type::complex(one())],
                Type::complex(one()),
            ))]
        },

        // ----- §8.16 number theory, combinatorics, geometry ----------------
        | "factor" => {
            vec![
                Scheme::mono(func(
                    vec![Type::int()],
                    list(tuple(vec![Type::int(), Type::int()])),
                )),
                // §8.5.1: an exact expression factors over the rationals.
                scheme(
                    0,
                    1,
                    0,
                    func(vec![Type::real(dim_var(0))], Type::real(dim_var(0))),
                ),
            ]
        },
        | "is_prime" => vec![Scheme::mono(func(vec![Type::int()], Type::bool()))],
        | "primes" => vec![Scheme::mono(func(vec![Type::int()], list(Type::int())))],
        | "binomial" => {
            vec![Scheme::mono(func(
                vec![Type::int(), Type::int()],
                Type::int(),
            ))]
        },
        | "permutations" => vec![Scheme::mono(func(vec![Type::int()], Type::int()))],
        | "polygon_area" => {
            vec![scheme(
                0,
                1,
                0,
                func(
                    vec![tensor(2, Type::real(dim_var(0)))],
                    Type::real(dim_var(0).pow(&two())),
                ),
            )]
        },
        | "distance" => {
            vec![scheme(
                0,
                1,
                0,
                func(
                    vec![
                        tensor(1, Type::real(dim_var(0))),
                        tensor(1, Type::real(dim_var(0))),
                    ],
                    Type::real(dim_var(0)),
                ),
            )]
        },

        // ----- §8.17 simulation -------------------------------------------
        | "simulate" => {
            vec![Scheme::mono(func(
                vec![Type::string(), json()],
                Type::result(json(), error_type("SimError")),
            ))]
        },

        // ----- §8.18 device -----------------------------------------------
        | "to_device" => {
            vec![scheme(
                1,
                0,
                0,
                func(vec![tensor(2, var(0)), var(1)], io(tensor(2, var(0)))),
            )]
        },
        // `on(device, thunk)`: `a` is the device and `b` the thunk result, so
        // both type variables are quantified (a missing `var(1)` made the
        // second instantiation in a module an E0303).
        | "on" => {
            vec![scheme(
                2,
                0,
                0,
                func(vec![var(0), func(Vec::new(), var(1))], io(var(1))),
            )]
        },
        | "device_of" => {
            vec![scheme(
                1,
                0,
                0,
                func(vec![tensor(2, var(0))], error_type("Device")),
            )]
        },

        // ----- §8.19 dynamic boundary -------------------------------------
        | "type_rep" => vec![scheme(1, 0, 0, func(Vec::new(), error_type("TypeRep")))],

        | _ => return None,
    })
}

/// True when the name is a built-in value or function.
#[must_use]
pub fn is_builtin(name: &str) -> bool {
    // Building the schemes only to test for their existence allocated every
    // type in every overload; the evaluator asks this on each reference to a
    // builtin (`map`, `println`, `mod`) inside a loop. The answer is a fixed
    // property of the name, so it is computed once per name.
    thread_local! {
        static KNOWN: std::cell::RefCell<std::collections::HashMap<String, bool>> =
            std::cell::RefCell::new(std::collections::HashMap::new());
    }
    if let Some(known) = KNOWN.with(|known| known.borrow().get(name).copied()) {
        return known;
    }
    let answer = schemes(name).is_some();
    KNOWN.with(|known| known.borrow_mut().insert(name.to_string(), answer));
    answer
}

/// Built-ins whose type depends on the number of arguments (`zeros`, `ones`,
/// `fill`) or on a fresh dimension (`symbol`). The checker handles these
/// directly instead of instantiating a scheme.
#[must_use]
pub fn is_special(name: &str) -> bool {
    matches!(
        name,
        "zeros" | "ones" | "fill" | "symbol" | "symfun" | "to_callback"
    )
}

/// Built-ins that read an explicit type argument written before the call.
///
/// `zeros<Int>(3)` and `zeros<Float>(3)` have the same runtime shape but not
/// the same element type, and `reshape<[3, 2]>` names a target shape, so the
/// argument is part of the call rather than decoration (§8.4). The runtime
/// keeps the list here so the IR lowering and `aevia-core` cannot drift apart.
#[must_use]
pub fn takes_type_argument(name: &str) -> bool {
    matches!(name, "zeros" | "ones" | "reshape" | "zeros_of" | "ones_of")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sqrt_halves_the_exponent() {
        let scheme = schemes("sqrt").expect("sqrt exists").remove(0);
        assert_eq!(scheme.dims.len(), 1);
        let Type::Fn(_, ret) = &scheme.ty else {
            panic!("expected function");
        };
        let Type::Scalar(_, dim) = &**ret else {
            panic!("expected scalar");
        };
        assert_eq!(*dim, Dim::var(0).pow(&half()));
    }

    #[test]
    fn unknown_names_are_not_builtins() {
        assert!(!is_builtin("not_a_function"));
        assert!(is_builtin("sqrt"));
        assert!(is_builtin("println"));
    }

    #[test]
    fn overloading_is_visible() {
        // `length` of a string, a rank-1 tensor and a set (§8.1.5.1).
        assert_eq!(schemes("length").expect("length").len(), 3);
        // §8.4 and §8.5 give `sum` the reduction, the keepdims axis form for
        // ranks 1 and 2, and the calculus summation `sum(f, a, b)`.
        assert_eq!(schemes("sum").expect("sum").len(), 4);
    }
}

/// Every name [`schemes`] answers for, transcribed from it.
///
/// `schemes` answers one name at a time and cannot be enumerated; this list is
/// what editors complete and what the checker's "did you mean" suggestions
/// search. `aevia-analysis` tests that every entry is still a builtin.
pub const PRELUDE_NAMES: &[&str] = &[
    "pi",
    "e",
    "oo",
    "nan",
    "i",
    "sqrt",
    "cbrt",
    "exp",
    "ln",
    "log",
    "log2",
    "log10",
    "sin",
    "cos",
    "tan",
    "asin",
    "acos",
    "atan",
    "sinh",
    "cosh",
    "tanh",
    "asinh",
    "acosh",
    "atanh",
    "gamma",
    "lgamma",
    "erf",
    "erfc",
    "zeta",
    "atan2",
    "hypot",
    "abs",
    "sign",
    "min",
    "max",
    "clamp",
    "floor",
    "ceil",
    "round",
    "trunc",
    "mod",
    "rem",
    "div",
    "quot",
    "gcd",
    "lcm",
    "factorial",
    "double_factorial",
    "to_float",
    "to_real",
    "to_int",
    "to",
    "delta_to",
    "value_in",
    "base",
    "point",
    "linspace",
    "range",
    "length",
    "shape",
    "rank",
    "sum",
    "prod",
    "mean",
    "var",
    "std",
    "min_all",
    "max_all",
    "norm",
    "normalize",
    "dot",
    "matmul",
    "matvec",
    "vecmat",
    "outer",
    "transpose",
    "trace",
    "det",
    "inv",
    "linsolve",
    "eye",
    "diag",
    "flatten",
    "at",
    "map",
    "map2",
    "filter",
    "fold",
    "fold_left",
    "sort_by",
    "sort_float",
    "zip",
    "zip_with",
    "enumerate",
    "take",
    "drop",
    "reverse",
    "sort",
    "concat",
    "any",
    "all",
    "find",
    "centroid",
    "join",
    "split",
    "replace",
    "trim",
    "upper",
    "lower",
    "contains",
    "starts_with",
    "ends_with",
    "parse_int",
    "parse_float",
    "format",
    "chars",
    "and_then",
    "unwrap_or",
    "is_some",
    "is_none",
    "map_err",
    "show",
    "repr",
    "to_latex",
    "to_typst",
    "print",
    "println",
    "eprintln",
    "read_file",
    "write_file",
    "read_line",
    "time_now",
    "random",
    "read_csv_dyn",
    "diff",
    "diff_at",
    "antiderivative",
    "defint",
    "limit",
    "series",
    "solve",
    "nsolve",
    "eval",
    "simplify",
    "set_of",
    "elements",
    "member",
    "set_insert",
    "set_remove",
    "union",
    "intersection",
    "difference",
    "is_subset",
    "expand",
    "cancel",
    "together",
    "apart",
    "collect",
    "degree",
    "approx",
    "subst",
    "to_value",
    "from_value",
    "to_callback",
    "decide",
    "assuming",
    "by_cases",
    "assert",
    "assert_eq",
    "approx_eq",
    "forall",
    "line",
    "scatter",
    "label",
    "combine",
    "plot",
    "save",
    "id",
    "const",
    "compose",
    "flip",
    "curry",
    "uncurry",
    "apply",
    "partial",
    "not",
    "xor",
    "compare",
    "partial_compare",
    "total_compare",
    "sym_eq",
    "sym_ne",
    "sym_lt",
    "zero",
    "beta",
    "bessel_j0",
    "bessel_j1",
    "bessel_y0",
    "bessel_y1",
    "j1",
    "y1",
    "bessel_jn",
    "bessel_yn",
    "legendre_p",
    "hermite_h",
    "laguerre_l",
    "chebyshev_t",
    "chebyshev_u",
    "lstsq",
    "eig",
    "eigvals",
    "svd",
    "qr",
    "lu",
    "cholesky",
    "tril",
    "triu",
    "reshape",
    "reshape_dyn",
    "set",
    "map_indexed",
    "approx_map",
    "det_unit",
    "from_dense",
    "to_dense",
    "nnz",
    "zeros_of",
    "ones_of",
    "numeric_deriv",
    "quadrature",
    "product",
    "product_dyn",
    "sum_f",
    "grad",
    "grad_f",
    "jacobian",
    "hessian",
    "divergence",
    "curl",
    "laplacian",
    "solve_complex",
    "nsolve_f",
    "root",
    "solve_system",
    "minimize",
    "maximize",
    "minimize_f",
    "least_squares",
    "median",
    "quantile",
    "cov",
    "corr",
    "skewness",
    "kurtosis",
    "histogram",
    "mode",
    "pdf",
    "cdf",
    "sample",
    "sample_n",
    "fft",
    "ifft",
    "fft_norm",
    "rfft",
    "irfft",
    "fftfreq",
    "interp1",
    "spline",
    "interp2",
    "polyfit",
    "chebyshev_fit",
    "polyval",
    "odeint",
    "odeint_with",
    "solve_ivp",
    "solve_poisson",
    "solve_heat",
    "solve_wave",
    "solve_advection",
    "read_csv",
    "write_csv",
    "read_npy",
    "write_npy",
    "read_json",
    "write_json",
    "column",
    "column_dyn",
    "row",
    "select",
    "schema",
    "to_table",
    "to_dynamic",
    "line_with",
    "scatter_with",
    "errorbar",
    "errorbar_with",
    "hist",
    "hist_with",
    "heatmap",
    "heatmap_with",
    "default_plot_opts",
    "compile_fail",
    "measure",
    "value",
    "uncertainty",
    "complex",
    "re",
    "im",
    "conj",
    "magnitude",
    "arg",
    "complex_sqrt",
    "complex_exp",
    "complex_ln",
    "complex_sin",
    "complex_cos",
    "factor",
    "is_prime",
    "primes",
    "binomial",
    "permutations",
    "polygon_area",
    "distance",
    "simulate",
    "to_device",
    "on",
    "device_of",
    "type_rep",
    "with_cap",
];
