//! The public bridge must validate callback evidence independently of the
//! interpreter's own callback implementation.
use aevia_dim::Dim;
use aevia_rssn::kernel::BridgeCallback;
use aevia_rssn::witness::DimensionWitness as W;
use aevia_rssn::{Kernel, KernelError, Num, Sig, SigType, Val};
use std::sync::Arc;

struct Callback { witness: W, wrong_representation: bool }
impl BridgeCallback for Callback {
    fn signature(&self) -> Sig { Sig { params: vec![SigType::Float], ret: SigType::Float, variadic: None } }
    fn call(&self, args: &[Val], _: &[W]) -> Result<(Val, W), KernelError> {
        if self.wrong_representation { return Ok((Val::Num(Num::Int(1.into())), self.witness.clone())); }
        Ok((args[0].clone(), self.witness.clone()))
    }
}
fn call(callback: Callback) -> Result<(Val, W), KernelError> {
    let args = vec![Val::Num(Num::Float(0.0)), Val::Num(Num::Float(1.0)), Val::Num(Num::Float(1e-9)), Val::Num(Num::Float(1e-9)), Val::Num(Num::Int(100.into()))];
    let witnesses = vec![W::Scalar(Dim::one()); args.len()];
    Kernel::new().call_ho_scoped("num_integrate", &args, &witnesses, &[Arc::new(callback)])
}
#[test]
fn public_callback_requires_proven_result_dimensions_and_representation() {
    assert!(call(Callback { witness: W::Scalar(Dim::one()), wrong_representation: false }).is_ok());
    for callback in [
        Callback { witness: W::Unknown, wrong_representation: false },
        Callback { witness: W::Scalar(Dim::base("Length")), wrong_representation: false },
        Callback { witness: W::Scalar(Dim::one()), wrong_representation: true },
    ] {
        let error = call(callback).expect_err("unproven callback result must fail");
        assert_eq!(error.code, "E1003", "{error:?}");
    }
}
