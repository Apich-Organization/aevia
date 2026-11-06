//! Static RSSN dimensions are instantiated and constrained per call.
use aevia_dim::Dim;
use aevia_rssn::{Sig, SigType};
use aevia_types::infer::Infer;
use aevia_types::rssn_resolve::instantiate_dimensions;
use aevia_types::ty::{Shape, Type};

fn vector(dim: Dim) -> Type {
    Type::tensor(Shape::Rank(1), Type::float(dim))
}
fn param(dim: Dim) -> SigType {
    SigType::Tensor {
        rank: Some(1),
        shape: None,
        elem: Box::new(SigType::Dim(Box::new(SigType::Real), dim)),
    }
}
#[test]
fn return_dimension_follows_both_actual_arguments() {
    let d = Dim::var(100);
    let e = Dim::var(101);
    let sig = Sig {
        params: vec![param(d.clone()), param(e.clone())],
        ret: SigType::Dim(Box::new(SigType::Real), d.mul(&e)),
        variadic: None,
    };
    let mut infer = Infer::new();
    let first = instantiate_dimensions(
        &sig,
        &[vector(Dim::base("Length")), vector(Dim::base("Time"))],
        &mut infer,
    )
    .unwrap()
    .unwrap();
    assert_eq!(
        first,
        Type::real(Dim::base("Length").mul(&Dim::base("Time")))
    );
    let second = instantiate_dimensions(
        &sig,
        &[vector(Dim::base("Mass")), vector(Dim::base("Time"))],
        &mut infer,
    )
    .unwrap()
    .unwrap();
    assert_eq!(
        second,
        Type::real(Dim::base("Mass").mul(&Dim::base("Time")))
    );
}
#[test]
fn shared_dimensions_reject_incompatible_arguments() {
    let d = Dim::var(123);
    let sig = Sig {
        params: vec![param(d.clone()), param(d.clone())],
        ret: SigType::Dim(Box::new(SigType::Real), d),
        variadic: None,
    };
    let mut infer = Infer::new();
    assert!(
        instantiate_dimensions(
            &sig,
            &[vector(Dim::base("Length")), vector(Dim::base("Time"))],
            &mut infer
        )
        .is_err()
    );
}
