//! Unknown manifest information must never become a concrete static claim.
use aevia_rssn::SigType;
use aevia_rssn::registry;
use rssn::kernel::signature::DimExpr;
use rssn::kernel::signature::Exp;
use rssn::kernel::signature::NatExpr;
use rssn::kernel::signature::Signature;
use rssn::kernel::signature::TypeExpr;

#[test]
fn unknown_tensor_ranks_remain_unknown() {
    for ty in [
        SigType::Tensor {
            rank: None,
            shape: None,
            elem: Box::new(SigType::Float),
        },
        SigType::Sparse {
            rank: None,
            elem: Box::new(SigType::Float),
        },
    ] {
        match registry::to_type_expr(&ty) {
            | TypeExpr::Tensor { rank, .. } | TypeExpr::Sparse { rank, .. } => {
                assert!(matches!(rank, NatExpr::Var(_)))
            },
            | other => panic!("unexpected type: {other:?}"),
        }
    }
}

#[test]
fn unrepresentable_dimension_expressions_are_not_dimensionless() {
    for dim in [
        DimExpr::NonMetric,
        DimExpr::Container,
        DimExpr::Pow(
            Box::new(DimExpr::Base("Length".into())),
            Exp::Symbol("n".into()),
        ),
    ] {
        assert!(!registry::convert_dim(&dim).is_concrete(), "lost {dim:?}");
        let sig = Signature::new(
            vec![TypeExpr::Tensor {
                rank: NatExpr::Lit(1),
                shape: None,
                elem: Box::new(TypeExpr::Dim(Box::new(TypeExpr::Real), dim)),
            }],
            TypeExpr::Real,
        );
        assert!(registry::signature_has_unrepresentable_dimensions(&sig));
    }
    assert!(!registry::signature_has_unrepresentable_dimensions(
        &Signature::new(
            vec![TypeExpr::Dim(
                Box::new(TypeExpr::Float),
                DimExpr::Var("D".into())
            )],
            TypeExpr::Float
        )
    ));
}

#[test]
fn empty_tensor_witness_comes_from_schema_not_first_element() {
    use aevia_rssn::witness::DimensionWitness;
    use aevia_rssn::witness::extract;
    use rssn::kernel::value::TensorValue;
    use rssn::kernel::value::Value;
    let empty = Value::Tensor(TensorValue {
        shape: vec![0],
        data: vec![],
        elem: TypeExpr::Dim(Box::new(TypeExpr::Float), DimExpr::Base("Length".into())),
    });
    assert!(
        matches!(extract(&empty, true), DimensionWitness::Tensor { dimension:Some(d), .. } if d == aevia_dim::Dim::base("Length"))
    );
    assert!(matches!(
        extract(&Value::Float(1.0), false),
        DimensionWitness::Unknown
    ));
}

#[test]
fn uniform_tensor_witnesses_do_not_allocate_per_scalar_but_keep_conflicts() {
    use aevia_rssn::witness::DimensionWitness;
    use aevia_rssn::witness::extract;
    use rssn::kernel::value::DynValue;
    use rssn::kernel::value::TensorValue;
    use rssn::kernel::value::Value;
    let length = DimExpr::Base("Length".into());
    let make = |data| {
        Value::Tensor(TensorValue {
            shape: vec![2],
            data,
            elem: TypeExpr::Dim(Box::new(TypeExpr::Float), length.clone()),
        })
    };
    let value = |dim| {
        Value::Dyn(DynValue {
            value: Box::new(Value::Float(1.0)),
            dim,
        })
    };
    let correct = make(vec![value(length.clone()), value(length.clone())]);
    assert!(
        matches!(extract(&correct,true), DimensionWitness::Tensor {elements,..} if elements.is_empty())
    );
    let wrong = make(vec![
        value(length.clone()),
        value(DimExpr::Base("Time".into())),
    ]);
    assert!(
        matches!(extract(&wrong,true), DimensionWitness::Tensor {elements,..} if elements.len()==1)
    );
}
