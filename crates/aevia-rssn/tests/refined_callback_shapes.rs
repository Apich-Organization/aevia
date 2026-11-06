use aevia_rssn::SigType;
use aevia_rssn::codegen;
use rssn::kernel::signature::NatExpr;
use rssn::kernel::signature::TypeExpr;
use std::sync::Arc;
use std::sync::atomic::AtomicUsize;
use std::sync::atomic::Ordering;

#[test]
fn concrete_extents_reach_kernel_signature_and_generated_declaration() {
    let ty = SigType::Tensor {
        rank: Some(2),
        shape: Some(vec![2, 3]),
        elem: Box::new(SigType::Float),
    };
    assert_eq!(codegen::declaration_type(&ty), "Tensor<[2, 3], Float>");
    let TypeExpr::Tensor { rank, shape, elem } = aevia_rssn::registry::to_type_expr(&ty) else {
        panic!("expected tensor")
    };
    assert_eq!(rank, NatExpr::Lit(2));
    assert_eq!(shape, Some(vec![NatExpr::Lit(2), NatExpr::Lit(3)]));
    assert!(matches!(*elem, TypeExpr::Float));
}

struct InvalidCallback {
    called: Arc<AtomicUsize>,
    ty: SigType,
}
impl aevia_rssn::kernel::BridgeCallback for InvalidCallback {
    fn signature(&self) -> aevia_rssn::Sig {
        aevia_rssn::Sig {
            params: vec![self.ty.clone()],
            ret: SigType::Float,
            variadic: None,
        }
    }

    fn call(
        &self,
        _: &[aevia_rssn::Val],
        _: &[aevia_rssn::witness::DimensionWitness],
    ) -> Result<(aevia_rssn::Val, aevia_rssn::witness::DimensionWitness), aevia_rssn::KernelError>
    {
        self.called.fetch_add(1, Ordering::SeqCst);
        unreachable!("malformed callback must be rejected before invocation")
    }
}

#[test]
fn public_callback_rejects_malformed_or_untransportable_nested_schemas() {
    use aevia_rssn::Kernel;
    use aevia_rssn::Num;
    use aevia_rssn::Val;
    use aevia_rssn::witness::DimensionWitness as W;
    let arguments = [
        Val::Num(Num::Float(0.0)),
        Val::Num(Num::Float(1.0)),
        Val::Num(Num::Float(0.0)),
        Val::Num(Num::Float(0.00001)),
        Val::Num(Num::Int(100.into())),
    ];
    let witnesses = vec![W::Scalar(aevia_dim::Dim::one()); arguments.len()];
    let called = Arc::new(AtomicUsize::new(0));
    for ty in [
        SigType::Tensor {
            rank: Some(1),
            shape: Some(vec![1, 2]),
            elem: Box::new(SigType::Float),
        },
        SigType::Tensor {
            rank: None,
            shape: Some(vec![2]),
            elem: Box::new(SigType::Float),
        },
        SigType::Record(vec![
            ("x".into(), SigType::Float),
            ("x".into(), SigType::Float),
        ]),
        SigType::Tensor {
            rank: Some(1),
            shape: None,
            elem: Box::new(SigType::Opaque("forged".into())),
        },
    ] {
        let callback: Arc<dyn aevia_rssn::kernel::BridgeCallback> = Arc::new(InvalidCallback {
            called: called.clone(),
            ty,
        });
        let error = Kernel::new()
            .call_ho_scoped("num_integrate", &arguments, &witnesses, &[callback])
            .expect_err("malformed signature accepted");
        assert!(
            error.message.contains("core.callback.signature"),
            "{error:?}"
        );
    }
    assert_eq!(called.load(Ordering::SeqCst), 0);
}
