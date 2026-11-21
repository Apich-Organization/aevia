//! The public bridge must validate callback evidence independently of the
//! interpreter's own callback implementation.
use aevia_dim::Dim;
use aevia_rssn::Kernel;
use aevia_rssn::KernelError;
use aevia_rssn::Num;
use aevia_rssn::Sig;
use aevia_rssn::SigType;
use aevia_rssn::Val;
use aevia_rssn::kernel::BridgeCallback;
use aevia_rssn::witness::DimensionWitness as W;
use std::sync::Arc;

struct Callback {
    witness: W,
    wrong_representation: bool,
}
impl BridgeCallback for Callback {
    fn signature(&self) -> Sig {
        Sig {
            params: vec![SigType::Float],
            ret: SigType::Float,
            variadic: None,
        }
    }

    fn call(
        &self,
        args: &[Val],
        _: &[W],
    ) -> Result<(Val, W), KernelError> {
        if self.wrong_representation {
            return Ok((Val::Num(Num::Int(1.into())), self.witness.clone()));
        }
        Ok((args[0].clone(), self.witness.clone()))
    }
}
fn call(callback: Callback) -> Result<(Val, W), KernelError> {
    let args = vec![
        Val::Num(Num::Float(0.0)),
        Val::Num(Num::Float(1.0)),
        Val::Num(Num::Float(1e-9)),
        Val::Num(Num::Float(1e-9)),
        Val::Num(Num::Int(100.into())),
    ];
    let witnesses = vec![W::Scalar(Dim::one()); args.len()];
    Kernel::new().call_ho_scoped("num_integrate", &args, &witnesses, &[Arc::new(callback)])
}
#[test]
fn public_callback_requires_proven_result_dimensions_and_representation() {
    assert!(
        call(Callback {
            witness: W::Scalar(Dim::one()),
            wrong_representation: false
        })
        .is_ok()
    );
    for callback in [
        Callback {
            witness: W::Unknown,
            wrong_representation: false,
        },
        Callback {
            witness: W::Scalar(Dim::base("Length")),
            wrong_representation: false,
        },
        Callback {
            witness: W::Scalar(Dim::one()),
            wrong_representation: true,
        },
    ] {
        let error = call(callback).expect_err("unproven callback result must fail");
        assert_eq!(error.code, "E1003", "{error:?}");
    }
}

fn tensor_witness(dimension: &Dim) -> W {
    W::Tensor {
        dimension: Some(dimension.clone()),
        element: Some(Box::new(W::Scalar(dimension.clone()))),
        element_repr: Some(SigType::Float),
        elements: vec![],
    }
}

struct FlatMetric;

impl BridgeCallback for FlatMetric {
    fn signature(&self) -> Sig {
        let measured = |rank, base| {
            SigType::Tensor {
                rank: Some(rank),
                shape: None,
                elem: Box::new(SigType::Dim(Box::new(SigType::Float), Dim::base(base))),
            }
        };
        Sig {
            params: vec![measured(1, "Length")],
            ret: measured(2, "Temperature"),
            variadic: None,
        }
    }

    fn call(
        &self,
        args: &[Val],
        witnesses: &[W],
    ) -> Result<(Val, W), KernelError> {
        assert!(matches!(args, [Val::Tensor { shape, .. }] if shape == &[2]));
        assert_eq!(witnesses, &[tensor_witness(&Dim::base("Length"))]);
        Ok((
            Val::Tensor {
                shape: vec![2, 2],
                data: [3.0, 0.0, 0.0, 3.0]
                    .map(|v| Val::Num(Num::Float(v)))
                    .to_vec(),
            },
            tensor_witness(&Dim::base("Temperature")),
        ))
    }
}

#[test]
fn scoped_calls_use_manifest_slots_for_leading_callbacks_and_keep_dynamic_units() {
    let kernel = Kernel::new();
    let name = "num_native_geometry_ricci_scalar";
    let args = [
        Val::Tensor {
            shape: vec![2],
            data: [1.0, 2.0].map(|v| Val::Num(Num::Float(v))).to_vec(),
        },
        Val::Num(Num::Float(2.0)),
        Val::Num(Num::Float(3.0)),
        Val::Num(Num::Int(128.into())),
    ];
    let length = Dim::base("Length");
    let mut witnesses = [
        tensor_witness(&length),
        W::Scalar(length),
        W::Scalar(Dim::base("Temperature")),
        W::Scalar(Dim::one()),
    ];
    let callbacks: [Arc<dyn BridgeCallback>; 1] = [Arc::new(FlatMetric)];
    let unpinned = kernel
        .call_ho_scoped(name, &args, &witnesses, &callbacks)
        .unwrap();
    let key = aevia_rssn::registry::registry()
        .op(name)
        .unwrap()
        .key(0)
        .unwrap();
    let pinned = kernel
        .call_ho_scoped_key(&key, &args, &witnesses, &callbacks)
        .unwrap();
    assert_eq!(pinned, unpinned);
    assert!(matches!(pinned.1, W::Scalar(dim) if !dim.is_one()));
    witnesses[2] = W::Scalar(Dim::one());
    assert!(
        kernel
            .call_ho_scoped(name, &args, &witnesses, &callbacks)
            .is_err()
    );
    assert!(kernel.call_ho_scoped(name, &args, &witnesses, &[]).is_err());
}
