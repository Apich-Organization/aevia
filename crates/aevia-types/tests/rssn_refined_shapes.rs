use aevia_rssn::SigType;
use aevia_types::Shape;
use aevia_types::Type;
use aevia_types::rssn_resolve::Score;
use aevia_types::rssn_resolve::classify;
use aevia_types::rssn_resolve::score_type;

#[test]
fn concrete_extents_participate_in_overload_scoring() {
    let tensor = |shape| {
        SigType::Tensor {
            rank: Some(1),
            shape,
            elem: Box::new(SigType::Float),
        }
    };
    let exact = tensor(Some(vec![3]));
    assert_eq!(score_type(&exact, &tensor(Some(vec![3]))), Score::Exact);
    assert_eq!(score_type(&exact, &tensor(Some(vec![4]))), Score::No);
    assert_eq!(score_type(&exact, &tensor(None)), Score::Shaped);
    assert_eq!(score_type(&tensor(None), &exact), Score::Shaped);
    assert_eq!(
        classify(&Type::tensor(
            Shape::refined([Some(3)]),
            Type::float(aevia_dim::Dim::one())
        )),
        exact
    );
    assert_eq!(
        classify(&Type::tensor(
            Shape::refined([Some(3), None]),
            Type::float(aevia_dim::Dim::one())
        )),
        SigType::Tensor {
            rank: Some(2),
            shape: None,
            elem: Box::new(SigType::Float)
        }
    );
}

#[test]
fn callable_shapes_are_contravariant_in_arguments_and_covariant_in_results() {
    let tensor = |shape| {
        SigType::Tensor {
            rank: Some(1),
            shape,
            elem: Box::new(SigType::Float),
        }
    };
    let function = |input, output| SigType::Func(vec![tensor(input)], Box::new(tensor(output)));
    let expected = function(Some(vec![2]), None);
    let safe = function(None, Some(vec![2]));
    assert!(score_type(&expected, &safe).accepted());
    assert_eq!(score_type(&safe, &expected), Score::No);
    let fixed_input = function(Some(vec![2]), None);
    let unknown_input = function(None, None);
    assert_eq!(score_type(&unknown_input, &fixed_input), Score::No);
    let fixed_result = function(None, Some(vec![2]));
    assert_eq!(score_type(&fixed_result, &unknown_input), Score::No);
    assert_eq!(
        score_type(&fixed_result, &function(None, Some(vec![3]))),
        Score::No
    );
    let time = SigType::Dim(Box::new(SigType::Float), aevia_dim::Dim::base("Time"));
    let dimensionless = SigType::Func(vec![SigType::Float], Box::new(SigType::Float));
    let wrong_result = SigType::Func(vec![SigType::Float], Box::new(time));
    assert_eq!(score_type(&dimensionless, &wrong_result), Score::No);
    let real_callback = SigType::Func(vec![SigType::Real], Box::new(SigType::Real));
    let float_callback = SigType::Func(vec![SigType::Float], Box::new(SigType::Float));
    assert_eq!(score_type(&real_callback, &float_callback), Score::No);
    assert_eq!(score_type(&float_callback, &real_callback), Score::No);
}
