//! Thread-affine, scoped transport for host callbacks. No interpreter or
//! closure pointer is sent to another thread; handles contain only tokens.
use crate::eval::Interp;
use crate::value::CallbackValue;
use crate::value::KernelValue;
use crate::value::Value;
use aevia_rssn::KernelError;
use aevia_rssn::Sig;
use aevia_rssn::Val;
use aevia_rssn::kernel::BridgeCallback;
use aevia_rssn::witness::DimensionWitness;
use aevia_syntax::Span;
use aevia_types::Rep;
use aevia_types::Type;
use std::cell::RefCell;
use std::collections::HashMap;
use std::panic::AssertUnwindSafe;
use std::panic::catch_unwind;
use std::panic::resume_unwind;
use std::rc::Rc;
use std::sync::Arc;
use std::sync::atomic::AtomicU64;
use std::sync::atomic::Ordering;
use std::thread::ThreadId;
use std::thread::{
    self,
};

struct Scope {
    interp: Option<Interp>,
    callbacks: Vec<Rc<CallbackValue>>,
}
thread_local! { static SCOPES: RefCell<HashMap<u64, Rc<RefCell<Scope>>>> = RefCell::new(HashMap::new()); }
static NEXT: AtomicU64 = AtomicU64::new(1);

struct Handle {
    token: u64,
    index: usize,
    thread: ThreadId,
    signature: Sig,
}
impl BridgeCallback for Handle {
    fn signature(&self) -> Sig {
        self.signature.clone()
    }

    fn call(
        &self,
        args: &[Val],
        witnesses: &[DimensionWitness],
    ) -> Result<(Val, DimensionWitness), KernelError> {
        if thread::current().id() != self.thread {
            return Err(KernelError::new(
                "E1001",
                "callback called on a different thread",
            ));
        }
        let scope = SCOPES
            .with(|scopes| scopes.borrow().get(&self.token).cloned())
            .ok_or_else(|| KernelError::new("E1001", "callback scope expired"))?;
        let mut scope = scope
            .try_borrow_mut()
            .map_err(|_| KernelError::new("E1001", "callback scope is already active"))?;
        let callback = scope
            .callbacks
            .get(self.index)
            .cloned()
            .ok_or_else(|| KernelError::new("E1001", "callback handle is invalid"))?;
        let Type::Fn(parameters, result) = &callback.schema else {
            return Err(KernelError::new(
                "E1001",
                "callback has no concrete function schema",
            ));
        };
        if args.len() != parameters.len() || args.len() != witnesses.len() {
            return Err(KernelError::new("E1003", "callback arity mismatch"));
        }
        let mut converted = Vec::with_capacity(args.len());
        for ((payload, witness), expected) in args.iter().zip(witnesses).zip(parameters) {
            let opaque = Value::KernelValue(Rc::new(KernelValue {
                payload: payload.clone(),
                witness: witness.clone(),
                host_witness: None,
            }));
            converted.push(
                crate::rssn_module::from_kernel_value(&opaque, expected)
                    .map_err(|reason| KernelError::new("E1003", reason))?,
            );
        }
        let interp = scope
            .interp
            .as_mut()
            .ok_or_else(|| KernelError::new("E1001", "callback interpreter is unavailable"))?;
        let result_value = interp
            .apply(callback.function.clone(), converted, Span::default())
            .map_err(|error| KernelError::new(error.code, error.message))?;
        let payload = crate::kernel::to_val(&result_value)
            .map_err(|error| KernelError::new(error.code, error.message))?;
        let witness = crate::rssn_module::host_dimension_witness(&result_value);
        let opaque = Value::KernelValue(Rc::new(KernelValue {
            payload: payload.clone(),
            witness: witness.clone(),
            host_witness: None,
        }));
        crate::rssn_module::validate_kernel_value(&opaque, result)
            .map_err(|reason| KernelError::new("E1003", reason))?;
        Ok((payload, witness))
    }
}

fn signature(callback: &CallbackValue) -> Result<Sig, KernelError> {
    let Type::Fn(parameters, result) = &callback.schema else {
        return Err(KernelError::capability("core.callback.signature"));
    };
    if !parameters.iter().all(supported_schema) || !supported_schema(result) {
        return Err(KernelError::capability("core.callback.signature"));
    }
    Ok(Sig {
        params: parameters
            .iter()
            .map(aevia_types::rssn_resolve::classify)
            .collect(),
        ret: aevia_types::rssn_resolve::classify(result),
        variadic: None,
    })
}

/// Only schemas faithfully checked by the shared kernel-value validator may
/// cross the callback boundary. Fully concrete tensor extents are preserved in
/// the kernel signature and the sealed Type checks every actual payload.
fn supported_schema(ty: &Type) -> bool {
    if aevia_types::class::has_variables(ty) {
        return false;
    }
    match ty {
        | Type::Scalar(Rep::Int | Rep::Real | Rep::Float | Rep::Complex, dim) => dim.is_concrete(),
        | Type::Bool | Type::Str => true,
        | Type::Tensor { elem, .. } => supported_schema(elem),
        | Type::Record(row) => {
            row.tail_var().is_none()
                && row
                    .fields()
                    .iter()
                    .all(|(_, field)| supported_schema(field))
        },
        // A kernel-origin tag does not prove an Aevia nominal identity; opaque,
        // function, IO and other unrepresentable values remain fail-closed.
        | _ => false,
    }
}

/// Move the interpreter into a thread-local scope for the duration of a
/// synchronous kernel call. The old interpreter is restored on success,
/// error, or unwind; callback handles cannot outlive their scope.
pub(crate) fn with_scoped<T>(
    interp: &mut Interp,
    callbacks: Vec<Rc<CallbackValue>>,
    run: impl FnOnce(&[Arc<dyn BridgeCallback>]) -> T,
) -> Result<T, KernelError> {
    let signatures = callbacks
        .iter()
        .map(|callback| signature(callback))
        .collect::<Result<Vec<_>, _>>()?;
    let token = NEXT
        .try_update(Ordering::Relaxed, Ordering::Relaxed, |n| n.checked_add(1))
        .map_err(|_| KernelError::new("E1001", "callback scope token space exhausted"))?;
    let moved = std::mem::replace(interp, Interp::new(&aevia_types::CheckedModule::default()));
    let scope = Rc::new(RefCell::new(Scope {
        interp: Some(moved),
        callbacks,
    }));
    SCOPES.with(|scopes| {
        scopes.borrow_mut().insert(token, Rc::clone(&scope));
    });
    let handles: Vec<Arc<dyn BridgeCallback>> = signatures
        .into_iter()
        .enumerate()
        .map(|(index, signature)| {
            Arc::new(Handle {
                token,
                index,
                thread: thread::current().id(),
                signature,
            }) as Arc<dyn BridgeCallback>
        })
        .collect();
    let result = catch_unwind(AssertUnwindSafe(|| run(&handles)));
    SCOPES.with(|scopes| {
        scopes.borrow_mut().remove(&token);
    });
    let restored = scope
        .borrow_mut()
        .interp
        .take()
        .expect("callback scope owns interpreter");
    *interp = restored;
    match result {
        | Ok(result) => Ok(result),
        | Err(panic) => resume_unwind(panic),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::value::NativeFn;
    use aevia_dim::Dim;
    fn callback(
        f: impl Fn(&mut Interp, Vec<Value>, Span) -> Result<Value, crate::eval::EvalError> + 'static
    ) -> Rc<CallbackValue> {
        Rc::new(CallbackValue {
            function: Value::Native(Rc::new(NativeFn(Box::new(f)))),
            schema: Type::Fn(
                vec![Type::float(Dim::one())],
                Box::new(Type::float(Dim::one())),
            ),
        })
    }
    fn invoke(handle: &dyn BridgeCallback) -> Result<(Val, DimensionWitness), KernelError> {
        handle.call(
            &[Val::Num(aevia_rssn::Num::Float(3.0))],
            &[DimensionWitness::Scalar(Dim::one())],
        )
    }
    #[test]
    fn handles_are_thread_affine_and_expire_after_scope() {
        let mut interp = Interp::new(&aevia_types::CheckedModule::default());
        let cb = callback(|_, values, _| Ok(values[0].clone()));
        let escaped = with_scoped(&mut interp, vec![cb], |handles| {
            assert!(invoke(handles[0].as_ref()).is_ok());
            let other = Arc::clone(&handles[0]);
            assert!(
                std::thread::spawn(move || invoke(other.as_ref()))
                    .join()
                    .unwrap()
                    .is_err()
            );
            Arc::clone(&handles[0])
        })
        .unwrap();
        assert!(invoke(escaped.as_ref()).is_err());
    }
    #[test]
    fn same_scope_reentry_fails_without_panicking_and_host_errors_keep_codes() {
        let mut interp = Interp::new(&aevia_types::CheckedModule::default());
        let slot: Rc<RefCell<Option<Arc<dyn BridgeCallback>>>> = Rc::new(RefCell::new(None));
        let captured = Rc::clone(&slot);
        let cb = callback(move |_, _, span| {
            let handle = captured.borrow().as_ref().unwrap().clone();
            let error = invoke(handle.as_ref()).expect_err("same scope is already borrowed");
            assert_eq!(error.code, "E1001");
            Err(crate::eval::EvalError::new("E0805", "callback fixture").at(span))
        });
        with_scoped(&mut interp, vec![cb], |handles| {
            *slot.borrow_mut() = Some(Arc::clone(&handles[0]));
            let error = invoke(handles[0].as_ref()).expect_err("host error propagates");
            assert_eq!(error.code, "E0805");
        })
        .unwrap();
    }
    #[test]
    fn nested_scopes_work_and_panics_restore_interpreter() {
        let mut interp = Interp::new(&aevia_types::CheckedModule::default());
        interp.seed = 123;
        let cb = callback(|interp, values, _| {
            let inner = callback(|_, values, _| Ok(values[0].clone()));
            with_scoped(interp, vec![inner], |handles| invoke(handles[0].as_ref()))
                .unwrap()
                .unwrap();
            Ok(values[0].clone())
        });
        with_scoped(&mut interp, vec![cb], |handles| {
            assert!(invoke(handles[0].as_ref()).is_ok())
        })
        .unwrap();
        let panicking = callback(|_, _, _| panic!("callback fixture"));
        let unwind = catch_unwind(AssertUnwindSafe(|| {
            let _ = with_scoped(&mut interp, vec![panicking], |handles| {
                invoke(handles[0].as_ref())
            });
        }));
        assert!(unwind.is_err());
        assert_eq!(interp.seed, 123);
    }

    #[test]
    fn recursive_tensor_callback_validates_shape_representation_and_dimensions() {
        use aevia_dim::Dim;
        use aevia_rssn::SigType;
        use aevia_types::Shape;
        let element = Type::float(Dim::base("Time"));
        let tensor = Type::tensor(Shape::refined([Some(2)]), element);
        let callback = Rc::new(CallbackValue {
            function: Value::Native(Rc::new(NativeFn(Box::new(
                |_, args, _| Ok(args[0].clone()),
            )))),
            schema: Type::Fn(vec![tensor.clone()], Box::new(tensor)),
        });
        let values = Val::Tensor {
            shape: vec![2],
            data: vec![
                Val::Num(aevia_rssn::Num::Float(1.0)),
                Val::Num(aevia_rssn::Num::Float(2.0)),
            ],
        };
        let witness = DimensionWitness::Tensor {
            dimension: Some(Dim::base("Time")),
            element: None,
            element_repr: Some(SigType::Float),
            elements: vec![],
        };
        let mut interp = Interp::new(&aevia_types::CheckedModule::default());
        with_scoped(&mut interp, vec![callback], |handles| {
            let call =
                |value: Val, witness: DimensionWitness| handles[0].call(&[value], &[witness]);
            assert!(call(values.clone(), witness.clone()).is_ok());
            let bad_shape = Val::Tensor {
                shape: vec![1, 2],
                data: vec![
                    Val::Num(aevia_rssn::Num::Float(1.0)),
                    Val::Num(aevia_rssn::Num::Float(2.0)),
                ],
            };
            assert!(call(bad_shape, witness.clone()).is_err());
            let bad_extent = Val::Tensor {
                shape: vec![1],
                data: vec![Val::Num(aevia_rssn::Num::Float(1.0))],
            };
            assert!(call(bad_extent, witness.clone()).is_err());
            let bad_rep = Val::Tensor {
                shape: vec![2],
                data: vec![
                    Val::Num(aevia_rssn::Num::Int(1.into())),
                    Val::Num(aevia_rssn::Num::Int(2.into())),
                ],
            };
            assert!(call(bad_rep, witness.clone()).is_err());
            let wrong_units = DimensionWitness::Tensor {
                dimension: Some(Dim::base("Length")),
                element: None,
                element_repr: Some(SigType::Float),
                elements: vec![],
            };
            assert!(call(values.clone(), wrong_units).is_err());
        })
        .unwrap();
    }

    #[test]
    fn recursive_callback_schema_rejects_open_or_nominal_types() {
        use aevia_types::Row;
        use aevia_types::Shape;
        let numeric = Type::float(Dim::one());
        let record = Type::Record(Row::closed([
            (
                "values".into(),
                Type::tensor(Shape::Rank(1), numeric.clone()),
            ),
            ("ready".into(), Type::Bool),
        ]));
        assert!(supported_schema(&record));
        assert!(!supported_schema(&Type::Record(Row::Var(12))));
        assert!(!supported_schema(&Type::tensor(
            Shape::Rank(1),
            Type::Var(4)
        )));
        assert!(!supported_schema(&Type::Fn(
            vec![numeric.clone()],
            Box::new(numeric)
        )));
    }

    #[test]
    fn empty_tensor_callback_needs_trusted_element_representation() {
        use aevia_rssn::SigType;
        use aevia_types::Shape;
        let tensor = Type::tensor(Shape::Rank(1), Type::float(Dim::one()));
        let callback = Rc::new(CallbackValue {
            function: Value::Native(Rc::new(NativeFn(Box::new(
                |_, args, _| Ok(args[0].clone()),
            )))),
            schema: Type::Fn(vec![tensor.clone()], Box::new(tensor)),
        });
        let value = Val::Tensor {
            shape: vec![0],
            data: vec![],
        };
        let witness = |element_repr| {
            DimensionWitness::Tensor {
                dimension: Some(Dim::one()),
                element: None,
                element_repr,
                elements: vec![],
            }
        };
        let mut interp = Interp::new(&aevia_types::CheckedModule::default());
        with_scoped(&mut interp, vec![callback], |handles| {
            assert!(
                handles[0]
                    .call(
                        std::slice::from_ref(&value),
                        &[witness(Some(SigType::Float))]
                    )
                    .is_ok()
            );
            assert!(
                handles[0]
                    .call(std::slice::from_ref(&value), &[witness(None)])
                    .is_err()
            );
            assert!(
                handles[0]
                    .call(&[value], &[witness(Some(SigType::Int))])
                    .is_err()
            );
        })
        .unwrap();
    }

    #[test]
    fn recursive_record_callback_preserves_nested_witnesses() {
        use aevia_rssn::SigType;
        use aevia_types::Row;
        use aevia_types::Shape;
        let tensor = Type::tensor(Shape::Rank(1), Type::float(Dim::one()));
        let record = Type::Record(Row::closed([
            ("values".to_string(), tensor),
            ("ready".to_string(), Type::Bool),
        ]));
        let callback = Rc::new(CallbackValue {
            function: Value::Native(Rc::new(NativeFn(Box::new(
                |_, args, _| Ok(args[0].clone()),
            )))),
            schema: Type::Fn(vec![record.clone()], Box::new(record)),
        });
        let value = Val::Record(vec![
            (
                "values".into(),
                Val::Tensor {
                    shape: vec![1],
                    data: vec![Val::Num(aevia_rssn::Num::Float(3.0))],
                },
            ),
            ("ready".into(), Val::Bool(true)),
        ]);
        let array = DimensionWitness::Tensor {
            dimension: Some(Dim::one()),
            element: None,
            element_repr: Some(SigType::Float),
            elements: vec![],
        };
        let witness = DimensionWitness::Record(vec![
            ("values".into(), array),
            ("ready".into(), DimensionWitness::Unknown),
        ]);
        let mut interp = Interp::new(&aevia_types::CheckedModule::default());
        with_scoped(&mut interp, vec![callback], |handles| {
            assert!(
                handles[0]
                    .call(std::slice::from_ref(&value), &[witness])
                    .is_ok()
            );
            assert!(
                handles[0]
                    .call(&[value], &[DimensionWitness::Unknown])
                    .is_err()
            );
        })
        .unwrap();
    }

    #[test]
    fn checked_source_tensor_callbacks_roundtrip_in_both_engines() {
        use aevia_rssn::SigType;
        let source = "let callback = to_callback(fn (x: Tensor<1, Float>) => x)\n";
        let compilation = crate::compile("vector_callback.aev", source);
        assert!(
            !compilation.diagnostics.has_errors(),
            "{:?}",
            compilation.diagnostics.items
        );
        let module = compilation.module.unwrap();
        let value = Val::Tensor {
            shape: vec![2],
            data: vec![
                Val::Num(aevia_rssn::Num::Float(3.0)),
                Val::Num(aevia_rssn::Num::Float(4.0)),
            ],
        };
        let witness = DimensionWitness::Tensor {
            dimension: Some(Dim::one()),
            element: None,
            element_repr: Some(SigType::Float),
            elements: vec![],
        };
        for mode in [crate::Mode::Ast, crate::Mode::Ir] {
            let mut interp = Interp::new(&compilation.checked).with_mode(mode);
            let diagnostics = interp.load(&module);
            assert!(
                !diagnostics.has_errors(),
                "{mode:?}: {:?}",
                diagnostics.items
            );
            let Value::Callback(callback) = interp.eval_toplevel("callback").unwrap() else {
                panic!("expected callback")
            };
            with_scoped(&mut interp, vec![callback], |handles| {
                let (returned, _) = handles[0]
                    .call(std::slice::from_ref(&value), std::slice::from_ref(&witness))
                    .unwrap();
                assert_eq!(returned, value, "{mode:?}");
            })
            .unwrap();
        }
    }

    #[test]
    fn callback_results_cannot_forge_tensor_shape_or_scalar_units() {
        use aevia_types::Shape;
        let expected = Type::tensor(Shape::refined([Some(2)]), Type::float(Dim::base("Time")));
        let wrong = Value::Tensor(Rc::new(crate::value::TensorData::new(
            vec![1],
            crate::value::Storage::Dense(Rc::new(vec![1.0])),
            Dim::base("Length"),
            None,
        )));
        let callback = Rc::new(CallbackValue {
            function: Value::Native(Rc::new(NativeFn(Box::new(
                move |_, _, _| Ok(wrong.clone()),
            )))),
            schema: Type::Fn(vec![], Box::new(expected)),
        });
        let mut interp = Interp::new(&aevia_types::CheckedModule::default());
        with_scoped(&mut interp, vec![callback], |handles| {
            assert!(handles[0].call(&[], &[]).is_err());
        })
        .unwrap();
    }

    #[test]
    fn checked_source_callbacks_roundtrip_empty_record_tensors() {
        use aevia_types::Row;
        use aevia_types::Type;
        let source = "let callback = to_callback(fn (x: Tensor<1, { t: Float[Time] }>) => x)\n";
        let compilation = crate::compile("empty_record_callback.aev", source);
        assert!(
            !compilation.diagnostics.has_errors(),
            "{:?}",
            compilation.diagnostics.items
        );
        let module = compilation.module.unwrap();
        let element = Type::Record(Row::closed([(
            "t".to_string(),
            Type::float(Dim::base("Time")),
        )]));
        let tensor = crate::value::TensorData::checked(
            vec![0],
            crate::value::Storage::Boxed(vec![]),
            Dim::one(),
            None,
            element,
        )
        .unwrap();
        let value = Value::Tensor(Rc::new(tensor));
        let payload = crate::kernel::to_val(&value).unwrap();
        let witness = crate::rssn_module::host_dimension_witness(&value);
        for mode in [crate::Mode::Ast, crate::Mode::Ir] {
            let mut interp = Interp::new(&compilation.checked).with_mode(mode);
            let diagnostics = interp.load(&module);
            assert!(
                !diagnostics.has_errors(),
                "{mode:?}: {:?}",
                diagnostics.items
            );
            let Value::Callback(callback) = interp.eval_toplevel("callback").unwrap() else {
                panic!("expected callback")
            };
            with_scoped(&mut interp, vec![callback], |handles| {
                let (returned, evidence) = handles[0]
                    .call(
                        std::slice::from_ref(&payload),
                        std::slice::from_ref(&witness),
                    )
                    .unwrap();
                assert_eq!(returned, payload, "{mode:?}");
                assert_eq!(evidence, witness, "{mode:?}");
            })
            .unwrap();
        }
    }

    #[test]
    fn checked_source_callback_roundtrips_empty_tensor_with_refined_inner_shape() {
        use aevia_types::Shape;
        use aevia_types::Type;
        let source = "let callback = to_callback(fn (x: Tensor<1, Tensor<[2], Float> >) => x)\n";
        let compilation = crate::compile("empty_nested_callback.aev", source);
        assert!(
            !compilation.diagnostics.has_errors(),
            "{:?}",
            compilation.diagnostics.items
        );
        let module = compilation.module.unwrap();
        let element = Type::tensor(Shape::refined([Some(2)]), Type::float(Dim::one()));
        let tensor = crate::value::TensorData::checked(
            vec![0],
            crate::value::Storage::Boxed(vec![]),
            Dim::one(),
            None,
            element,
        )
        .unwrap();
        let value = Value::Tensor(Rc::new(tensor));
        let payload = crate::kernel::to_val(&value).unwrap();
        let witness = crate::rssn_module::host_dimension_witness(&value);
        for mode in [crate::Mode::Ast, crate::Mode::Ir] {
            let mut interp = Interp::new(&compilation.checked).with_mode(mode);
            let diagnostics = interp.load(&module);
            assert!(
                !diagnostics.has_errors(),
                "{mode:?}: {:?}",
                diagnostics.items
            );
            let Value::Callback(callback) = interp.eval_toplevel("callback").unwrap() else {
                panic!("expected callback")
            };
            with_scoped(&mut interp, vec![callback], |handles| {
                let (returned, evidence) = handles[0]
                    .call(
                        std::slice::from_ref(&payload),
                        std::slice::from_ref(&witness),
                    )
                    .unwrap();
                assert_eq!(returned, payload, "{mode:?}");
                assert_eq!(evidence, witness, "{mode:?}");
            })
            .unwrap();
        }
    }
}
