//! Static RSSN dimensions are instantiated and constrained per call.
use aevia_dim::Dim;
use aevia_dim::ratio;
use aevia_rssn::Sig;
use aevia_rssn::SigType;
use aevia_types::infer::Infer;
use aevia_types::infer::UnifyError;
use aevia_types::rssn_resolve::instantiate_dimensions;
use aevia_types::ty::Shape;
use aevia_types::ty::Type;

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

#[test]
fn variadic_dimensions_bind_the_result_and_check_each_tail_argument() {
    let d = Dim::var(321);
    let half = ratio(1, 2);
    let sig = Sig {
        params: vec![SigType::Bool],
        ret: SigType::Dim(Box::new(SigType::Real), d.pow(&half)),
        variadic: Some((1, Some(2), param(d))),
    };
    let length = Dim::base("Length");
    let time = Dim::base("Time");
    let area = length.pow(&ratio(2, 1));
    let mut infer = Infer::new();
    for dimension in [area.clone(), time.clone()] {
        let result = instantiate_dimensions(
            &sig,
            &[
                Type::Bool,
                vector(dimension.clone()),
                vector(dimension.clone()),
            ],
            &mut infer,
        )
        .unwrap();
        assert_eq!(result, Some(Type::real(dimension.pow(&half))));
    }
    assert!(matches!(
        instantiate_dimensions(&sig, &[Type::Bool, vector(area), vector(time)], &mut infer),
        Err(UnifyError::Dim(_))
    ));
}
