//! The typed signatures of the sparse layout family (§8.4.1, §13.9.5).
//!
//! `Sparse<F, shape, τ>` is a core type constructor: it does **not** unify with
//! `Tensor<shape, τ>`. It is encoded as the nominal type `Sparse` applied to
//! the format marker `F` (`Csr`, `Csc`, `Coo`, `Block`, `Bcsr`) and the dense
//! type with the same shape and element, so unification, substitution and the
//! occurs check come from the ordinary `Adt` rules while the shape and the
//! element dimension stay visible to the checker.
//!
//! Rank is not first-class in a scheme, so each of the ranks 1 to 4 gets its
//! own overload (`Csr`/`Csc` are rank 2; `Coo` is a sparse *tensor* of any
//! rank). Element dimensions propagate as for dense tensors: a sum needs equal
//! dimensions, a product multiplies them, `spsolve` divides them.

use super::Adt;
use super::Scheme;
use super::adt;
use super::dim_var;
use super::error_type;
use super::func;
use super::one;
use super::rep_var;
use super::scalar;
use super::scheme;
use super::tensor;
use super::tuple;
use super::var;
use crate::ty::Type;

/// The format marker types the type language names (§3, `Format`).
pub const FORMATS: [&str; 5] = ["Csr", "Csc", "Coo", "Block", "Bcsr"];

fn sparse_adt() -> adt::AdtId {
    adt::intern(Adt {
        name: "Sparse".to_string(),
        params: vec!["f".to_string(), "t".to_string()],
        ctors: Vec::new(),
    })
}

/// The marker type of a storage format, if `name` is one.
#[must_use]
pub fn format_type(name: &str) -> Option<Type> {
    FORMATS.contains(&name).then(|| error_type(name))
}

/// `Sparse<format, dense>`, where `dense` is the `Tensor<shape, τ>` with the
/// same shape and element type.
#[must_use]
pub fn sparse_type(
    format: Type,
    dense: Type,
) -> Type {
    Type::Adt(sparse_adt(), vec![format, dense])
}

/// The format and the dense counterpart of a sparse type.
#[must_use]
pub fn sparse_parts(ty: &Type) -> Option<(&Type, &Type)> {
    match ty {
        | Type::Adt(id, args) if args.len() == 2 && *id == sparse_adt() => {
            Some((&args[0], &args[1]))
        },
        | _ => None,
    }
}

/// Names whose explicit format type argument (`from_dense<Coo>`) the checker
/// reads; every other sparse builtin takes none.
#[must_use]
pub fn takes_format_argument(name: &str) -> bool {
    matches!(name, "from_dense" | "convert" | "speye" | "spdiag")
}

const RANKS: std::ops::RangeInclusive<u32> = 1..=4;

// Variable numbering inside these schemes (type and representation variables
// share one id space): 0 is the representation, 1 and 2 are free formats.
fn rep() -> crate::ty::Rep {
    rep_var(0)
}

fn elem(dim: u32) -> Type {
    scalar(rep(), dim_var(dim))
}

fn sp(
    format: Type,
    rank: u32,
    element: Type,
) -> Type {
    sparse_type(format, tensor(rank, element))
}

fn csr() -> Type {
    error_type("Csr")
}

/// The overloads of a name that is a sparse builtin (`format` replaces the
/// default `Csr` result format of the constructors, `from_dense<Coo>`), or
/// `None` when the name is not one.
#[must_use]
pub fn sparse_schemes(
    name: &str,
    format: Option<Type>,
) -> Option<Vec<Scheme>> {
    let int = Type::int;
    Some(match name {
        | "from_dense" => {
            let f = format.unwrap_or_else(csr);
            RANKS
                .map(|r| {
                    scheme(
                        1,
                        1,
                        0,
                        func(vec![tensor(r, elem(0))], sp(f.clone(), r, elem(0))),
                    )
                })
                .collect()
        },
        | "to_dense" => {
            RANKS
                .map(|r| {
                    scheme(
                        3,
                        1,
                        0,
                        func(vec![sp(var(1), r, elem(0))], tensor(r, elem(0))),
                    )
                })
                .collect()
        },
        | "nnz" => {
            vec![scheme(
                3,
                0,
                0,
                func(vec![sparse_type(var(1), var(2))], int()),
            )]
        },
        | "density" => {
            vec![scheme(
                3,
                0,
                0,
                func(vec![sparse_type(var(1), var(2))], Type::float(one())),
            )]
        },
        | "format_of" => {
            vec![scheme(
                3,
                0,
                0,
                func(vec![sparse_type(var(1), var(2))], Type::string()),
            )]
        },
        | "convert" => {
            let target = format.unwrap_or_else(|| var(3));
            vec![scheme(
                4,
                0,
                0,
                func(
                    vec![sparse_type(var(1), var(2))],
                    sparse_type(target, var(2)),
                ),
            )]
        },
        | "sparse_csr" => {
            vec![scheme(
                1,
                1,
                0,
                func(
                    vec![
                        int(),
                        int(),
                        tensor(1, int()),
                        tensor(1, int()),
                        tensor(1, elem(0)),
                    ],
                    sp(csr(), 2, elem(0)),
                ),
            )]
        },
        | "speye" => {
            let f = format.unwrap_or_else(csr);
            vec![scheme(
                0,
                0,
                0,
                func(vec![int()], sp(f, 2, Type::float(one()))),
            )]
        },
        | "spdiag" => {
            let f = format.unwrap_or_else(csr);
            vec![scheme(
                1,
                1,
                0,
                func(vec![tensor(1, elem(0))], sp(f, 2, elem(0))),
            )]
        },
        // The call forms are typed by the checker (the result rank depends on a
        // literal); these loose schemes only make the names first-class.
        | "sparse_coo" => {
            vec![scheme(
                3,
                1,
                0,
                func(
                    vec![tensor(1, int()), var(1), tensor(1, elem(0))],
                    sparse_type(error_type("Coo"), var(2)),
                ),
            )]
        },
        | "contract" => {
            vec![scheme(
                4,
                0,
                0,
                func(
                    vec![var(0), var(1), tensor(1, int()), tensor(1, int())],
                    var(2),
                ),
            )]
        },
        | "kron" => {
            vec![scheme(
                3,
                2,
                0,
                func(
                    vec![sp(var(1), 2, elem(0)), sp(var(2), 2, elem(1))],
                    sp(csr(), 2, scalar(rep(), dim_var(0).mul(&dim_var(1)))),
                ),
            )]
        },
        | "spsolve" => {
            vec![scheme(
                2,
                2,
                0,
                func(
                    vec![sp(var(1), 2, elem(0)), tensor(1, elem(1))],
                    tensor(1, scalar(rep().div_rep(), dim_var(1).div(&dim_var(0)))),
                ),
            )]
        },
        | "cg" => {
            let float = |dim: u32| Type::float(dim_var(dim));
            vec![scheme(
                2,
                2,
                0,
                func(
                    vec![
                        sp(var(1), 2, float(0)),
                        tensor(1, float(1)),
                        Type::float(one()),
                        int(),
                    ],
                    Type::record(vec![
                        (
                            "x".to_string(),
                            tensor(1, Type::float(dim_var(1).div(&dim_var(0)))),
                        ),
                        ("iterations".to_string(), int()),
                        ("residual".to_string(), Type::float(one())),
                        ("converged".to_string(), Type::bool()),
                    ]),
                ),
            )]
        },
        | "map_nonzero" => {
            RANKS
                .map(|r| {
                    scheme(
                        3,
                        2,
                        0,
                        func(
                            vec![
                                func(
                                    vec![scalar(rep_var(0), dim_var(0))],
                                    scalar(rep_var(2), dim_var(1)),
                                ),
                                sp(var(1), r, scalar(rep_var(0), dim_var(0))),
                            ],
                            sp(var(1), r, scalar(rep_var(2), dim_var(1))),
                        ),
                    )
                })
                .collect()
        },
        | "nonzeros" => {
            RANKS
                .map(|r| {
                    scheme(
                        2,
                        1,
                        0,
                        func(
                            vec![sp(var(1), r, elem(0))],
                            tensor(1, tuple(vec![tensor(1, int()), elem(0)])),
                        ),
                    )
                })
                .collect()
        },
        | "get" => {
            RANKS
                .map(|r| {
                    let mut params = vec![sp(var(1), r, elem(0))];
                    params.extend((0..r).map(|_| int()));
                    scheme(2, 1, 0, func(params, elem(0)))
                })
                .collect()
        },
        | _ => return None,
    })
}

/// The sparse overloads appended to an existing name (`sum`, `matmul`).
#[must_use]
pub fn sparse_overloads(name: &str) -> Vec<Scheme> {
    match name {
        // `sum(A)` is the element sum; `sum(A, axis)` keeps the axis, as the
        // dense form does (§3.5.5), and is dense.
        | "sum" => {
            RANKS
                .flat_map(|r| {
                    [
                        scheme(3, 1, 0, func(vec![sp(var(1), r, elem(0))], elem(0))),
                        scheme(
                            3,
                            1,
                            0,
                            func(
                                vec![sp(var(1), r, elem(0)), Type::int()],
                                tensor(r, elem(0)),
                            ),
                        ),
                    ]
                })
                .collect()
        },
        // The four layouts of `matmul` (§13.9.5): sparse×dense, dense×sparse
        // are dense; sparse×sparse is `Csr`.
        | "matmul" => {
            let product = || scalar(rep(), dim_var(0).mul(&dim_var(1)));
            let mut out = Vec::new();
            for r in [1, 2] {
                out.push(scheme(
                    3,
                    2,
                    0,
                    func(
                        vec![sp(var(1), 2, elem(0)), tensor(r, elem(1))],
                        tensor(r, product()),
                    ),
                ));
                out.push(scheme(
                    3,
                    2,
                    0,
                    func(
                        vec![tensor(r, elem(0)), sp(var(1), 2, elem(1))],
                        tensor(r, product()),
                    ),
                ));
            }
            out.push(scheme(
                3,
                2,
                0,
                func(
                    vec![sp(var(1), 2, elem(0)), sp(var(2), 2, elem(1))],
                    sp(csr(), 2, product()),
                ),
            ));
            out
        },
        | _ => Vec::new(),
    }
}
