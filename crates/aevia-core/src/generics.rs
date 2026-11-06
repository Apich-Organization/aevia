//! Actual callable signatures and per-activation generic substitutions.
//! A span supplies an expectation only at a checked source call. The authority
//! is the immutable signature belonging to the evaluated source closure.
use crate::eval::EvalError;
use crate::eval::Interp;
use crate::evidence::ModuleContext;
use crate::value::Closure;
use crate::value::Value;
use aevia_syntax::Span;
use aevia_types::Infer;
use aevia_types::Type;
use aevia_types::TypeSubstitution;
use aevia_types::check::FunctionEvidence;
use aevia_types::check::GenericBinder;
use std::rc::Rc;
use std::rc::Weak;

pub(crate) fn specialize_closure(
    closure: &Rc<Closure>,
    proof: Rc<FunctionProof>,
) -> Rc<Closure> {
    let mut selected = (**closure).clone();
    selected.canonical_origin = Closure::origin(closure);
    selected.proof = Some(proof);
    Rc::new(selected)
}

#[derive(Clone, Debug)]
pub(crate) struct CallBindings {
    pub(crate) context: Weak<ModuleContext>,
    pub(crate) values: TypeSubstitution,
    pub(crate) verified: bool,
}
#[derive(Clone, Debug)]
pub(crate) struct FunctionProof {
    pub(crate) context: Rc<ModuleContext>,
    pub(crate) definition: FunctionEvidence,
    pub(crate) infer: Infer,
    pub(crate) projection: TypeSubstitution,
    pub(crate) signature: Type,
    pub(crate) verified: bool,
}
impl FunctionProof {
    pub(crate) fn at(
        context: &Rc<ModuleContext>,
        site: Span,
        captured: Option<&Rc<CallBindings>>,
    ) -> Option<Rc<Self>> {
        let definition = context.evidence.functions.get(&site)?.clone();
        let mut infer = Infer::new();
        let mut projection = TypeSubstitution::default();
        projection.freshen(&definition.scheme.ty, &mut infer);
        for binder in &definition.binders {
            projection.freshen(&binder_type(binder), &mut infer);
        }
        for ty in &definition.scheme.materializations {
            projection.freshen(ty, &mut infer);
        }
        for ty in &definition.reifications {
            projection.freshen(ty, &mut infer);
        }
        for (_, _, ty) in &definition.numeric_literals {
            projection.freshen(ty, &mut infer);
        }
        for constraint in &definition.scheme.numeric {
            for ty in constraint.types() {
                projection.freshen(&ty, &mut infer);
            }
        }
        for constraint in &definition.scheme.numeric {
            infer
                .add_numeric_constraint(constraint.substitute(&projection))
                .ok()?;
        }
        let mut verified = true;
        if let Some(captured) = captured.filter(|captured| {
            captured
                .context
                .upgrade()
                .is_some_and(|owner| Rc::ptr_eq(&owner, context))
        }) {
            verified = captured.verified;
            let mut capture_names = TypeSubstitution::default();
            for (original, target) in &captured.values.types {
                if let Some(own) = projection.types.get(original).cloned() {
                    let target = capture_names.freshen(target, &mut infer);
                    infer.unify(&own, &target).ok()?;
                }
            }
            for (original, target) in &captured.values.representations {
                if let Some(own) = projection.representations.get(original) {
                    let target = capture_names
                        .freshen(&Type::Scalar(*target, aevia_dim::Dim::one()), &mut infer);
                    infer
                        .unify(&Type::Scalar(*own, aevia_dim::Dim::one()), &target)
                        .ok()?;
                }
            }
            for (original, target) in &captured.values.dimensions {
                if let Some(own) = projection.dimensions.get(original) {
                    let target = capture_names.freshen(&Type::UnitOf(target.clone()), &mut infer);
                    infer.unify(&Type::UnitOf(own.clone()), &target).ok()?;
                }
            }
            for (original, target) in &captured.values.rows {
                if let Some(own) = projection.rows.get(original) {
                    let target = capture_names.freshen(&Type::Record(target.clone()), &mut infer);
                    infer.unify(&Type::Record(own.clone()), &target).ok()?;
                }
            }
            for (original, target) in &captured.values.naturals {
                if let Some(own) = projection.naturals.get(original) {
                    let target = capture_names.freshen(&nat_type(target.clone()), &mut infer);
                    infer.unify(&nat_type(own.clone()), &target).ok()?;
                }
            }
        }
        let signature = infer.resolve(&projection.apply(&definition.scheme.ty));
        Some(Rc::new(Self {
            context: context.clone(),
            definition,
            infer,
            projection,
            signature,
            verified,
        }))
    }

    pub(crate) fn specialize(
        &self,
        expected: &Type,
    ) -> Result<Rc<Self>, EvalError> {
        let mut next = self.clone();
        let mut names = TypeSubstitution::default();
        let expected = names.freshen(expected, &mut next.infer);
        next.infer.unify(&next.signature,&expected).map_err(|error| EvalError::new("E0302",format!("actual callable signature does not match the requested signature: {error:?}")))?;
        next.signature = next.infer.resolve(&next.signature);
        Ok(Rc::new(next))
    }

    pub(crate) fn explicit(
        &self,
        arguments: &[Type],
    ) -> Result<Rc<Self>, EvalError> {
        if self.definition.binders.is_empty() {
            return Ok(Rc::new(self.clone()));
        }
        if arguments.len() != self.definition.binders.len() {
            return Err(EvalError::new(
                "E0315",
                format!(
                    "this function takes {} generic argument(s), found {}",
                    self.definition.binders.len(),
                    arguments.len()
                ),
            ));
        }
        let mut next = self.clone();
        for (binder, argument) in self.definition.binders.iter().zip(arguments) {
            if aevia_types::class::has_variables(argument) {
                return Err(EvalError::new(
                    "E0304",
                    "a function type application needs concrete runtime type arguments",
                ));
            }
            let variable = self.projection.apply(&binder.as_type());
            next.infer.unify(&variable,argument).map_err(|error| EvalError::new("E0302",format!("function specialization conflicts with its selected type arguments: {error:?}")))?;
        }
        next.signature = next.infer.resolve(&next.signature);
        Ok(Rc::new(next))
    }

    pub(crate) fn bindings(&self) -> Rc<CallBindings> {
        Rc::new(CallBindings {
            context: Rc::downgrade(&self.context),
            values: self.projection.resolved(&self.infer),
            verified: self.verified,
        })
    }

    pub(crate) fn check_reifications(&self) -> Result<(), EvalError> {
        let bindings = self.projection.resolved(&self.infer);
        if self
            .definition
            .reifications
            .iter()
            .any(|ty| !self.verified || aevia_types::class::has_variables(&bindings.apply(ty)))
        {
            return Err(EvalError::new(
                "E0304",
                "this function needs concrete, verified generic arguments for type reification",
            ));
        }
        for (span, kind, ty) in &self.definition.numeric_literals {
            let ty = bindings.apply(ty);
            if !self.verified || aevia_types::class::has_variables(&ty) {
                return Err(EvalError::new(
                    "E0304",
                    "a numeric literal requires concrete runtime type arguments",
                )
                .at(*span));
            }
            if !matches!(&ty,Type::Scalar(rep,dim) if kind.accepts_rep(*rep) && (kind.is_quantity() || dim.is_one()))
            {
                return Err(EvalError::new(
                    "E0302",
                    format!("the selected type {ty} cannot represent this numeric literal"),
                )
                .at(*span));
            }
        }
        Ok(())
    }
}
fn nat_type(nat: aevia_types::Nat) -> Type {
    Type::tensor(aevia_types::Shape::Refined(vec![Some(nat)]), Type::Unit)
}
fn binder_type(binder: &GenericBinder) -> Type {
    match binder {
        | GenericBinder::Type(ty) => ty.clone(),
        | GenericBinder::Representation(rep) => Type::Scalar(*rep, aevia_dim::Dim::one()),
        | GenericBinder::Dimension(dim) => Type::UnitOf(dim.clone()),
        | GenericBinder::Natural(nat) => nat_type(nat.clone()),
        | GenericBinder::Row(row) => Type::Record(row.clone()),
    }
}
impl Interp {
    pub(crate) fn checked_ast_closure(
        &self,
        params: Vec<aevia_syntax::Pat>,
        body: aevia_syntax::Expr,
        scope: Rc<crate::Scope>,
        name: Option<String>,
        site: Span,
    ) -> Value {
        let mut closure = Closure::ast(params, body, scope, name);
        closure.proof = self.function_proof(site);
        closure.captured_bindings = self.bindings.clone();
        Value::Closure(Rc::new(closure))
    }

    pub(crate) fn resolved_expr_type(
        &self,
        span: Span,
    ) -> Option<Type> {
        let ty = self.expr_type(span)?;
        if let Some(bindings) = self.bindings.as_ref().filter(|bindings| {
            bindings
                .context
                .upgrade()
                .is_some_and(|owner| Rc::ptr_eq(&owner, &self.context))
        }) {
            if !bindings.verified && aevia_types::class::has_variables(ty) {
                return None;
            }
            Some(bindings.values.apply(ty))
        } else {
            Some(ty.clone())
        }
    }

    pub(crate) fn resolved_generic_arguments(
        &self,
        span: Span,
    ) -> Option<Vec<Type>> {
        let headers = self.context.evidence.generic_refs.get(&span)?;
        let values = self.bindings.as_ref().filter(|bindings| {
            bindings.verified
                && bindings
                    .context
                    .upgrade()
                    .is_some_and(|owner| Rc::ptr_eq(&owner, &self.context))
        });
        let args: Vec<_> = headers
            .iter()
            .map(|header| {
                let ty = header.as_type();
                values.map_or(ty.clone(), |values| values.values.apply(&ty))
            })
            .collect();
        (!args.iter().any(aevia_types::class::has_variables)).then_some(args)
    }

    /// Authenticate and specialize the actual function, never a claimed host type.
    pub(crate) fn reify_callable(
        &self,
        value: &Value,
        expected: &Type,
    ) -> Result<Value, EvalError> {
        if !matches!(expected, Type::Fn(..)) || aevia_types::class::has_variables(expected) {
            return Err(EvalError::new(
                "E0304",
                "a callback needs a concrete function signature",
            ));
        }
        let Value::Closure(closure) = value else {
            return Err(EvalError::new(
                "E0302",
                "this callable has no checked source function signature",
            ));
        };
        let proof = closure.proof.as_ref().ok_or_else(|| {
            EvalError::new(
                "E0302",
                "this callable has no checked source function signature",
            )
        })?;
        if !proof
            .context
            .session
            .upgrade()
            .is_some_and(|session| Rc::ptr_eq(&session, &self.session))
        {
            return Err(EvalError::new(
                "E0809",
                "the callable belongs to a different interpreter session",
            ));
        }
        let proof = proof.specialize(expected)?;
        if !proof.verified || aevia_types::class::has_variables(&proof.signature) {
            return Err(EvalError::new(
                "E0304",
                "the callable's captured type arguments are not concrete and verified",
            ));
        }
        proof.check_reifications()?;
        Ok(Value::Closure(specialize_closure(closure, proof)))
    }

    pub(crate) fn concrete_callable_type(
        &self,
        value: &Value,
    ) -> Option<Type> {
        let Value::Closure(closure) = value else {
            return None;
        };
        let proof = closure.proof.as_ref()?;
        (proof.verified && !aevia_types::class::has_variables(&proof.signature))
            .then(|| proof.signature.clone())
    }

    pub(crate) fn function_proof(
        &self,
        site: Span,
    ) -> Option<Rc<FunctionProof>> {
        FunctionProof::at(&self.context, site, self.bindings.as_ref())
    }
}

impl Interp {
    /// Prepare a real user-function activation. No span is consulted unless the
    /// caller explicitly supplies the callee span of a checked source call.
    pub(crate) fn prepare_function_call(
        &self,
        function: &Value,
        args: &mut [Value],
        callee: Option<Span>,
    ) -> Result<Option<Rc<FunctionProof>>, EvalError> {
        let Value::Closure(closure) = function else {
            return Ok(None);
        };
        let Some(original) = &closure.proof else {
            return Ok(None);
        };
        let expected = callee.and_then(|span| self.resolved_expr_type(span));
        let requested = callee.and_then(|span| self.resolved_generic_arguments(span));
        // An alias may expose a subset or a different order of the actual
        // definition's binders. Prefer its requested signature whenever the
        // ordinal header mapping does not agree with that signature.
        let ordinal = requested
            .filter(|args| args.len() == original.definition.binders.len())
            .and_then(|args| original.explicit(&args).ok());
        let selected = if let Some(expected @ Type::Fn(..)) = &expected {
            ordinal
                .as_ref()
                .and_then(|proof| proof.specialize(expected).ok())
                .map_or_else(|| original.specialize(expected), Ok)?
        } else {
            ordinal.unwrap_or_else(|| original.clone())
        };
        let mut selected = (*selected).clone();
        let Type::Fn(params, _) = selected.signature.clone() else {
            return Ok(None);
        };
        if params.len() != args.len() {
            return Ok(Some(Rc::new(selected)));
        }
        // Runtime data can solve dimensions, extents and representations that
        // the source's unrefined argument annotation intentionally omitted.
        for (parameter, value) in params.iter().zip(args.iter()) {
            let actual = self.call_argument_schema(value);
            if let Some(actual) = actual {
                let mut names = TypeSubstitution::default();
                let actual = names.freshen(&actual, &mut selected.infer);
                let mut trial = selected.infer.clone();
                if trial.unify(parameter, &actual).is_ok() {
                    selected.infer = trial;
                } else {
                    selected.verified = false;
                }
            }
        }
        // A higher-order source function carries relations that its plain Fn
        // shape cannot express. Once data inputs select its arguments, feed
        // its authenticated derived result back into the enclosing proof.
        // At most one new argument dependency is solved per pass.
        for _ in 0..=args.len() {
            let before = selected.infer.resolve(&selected.signature);
            let Type::Fn(params, _) = &before else {
                unreachable!()
            };
            for (parameter, value) in params.iter().zip(args.iter_mut()) {
                let Value::Closure(closure) = value else {
                    continue;
                };
                if !matches!(parameter, Type::Fn(..)) {
                    continue;
                }
                let Some(proof) = &closure.proof else {
                    continue;
                };
                let Ok(proof) = proof.specialize(parameter) else {
                    continue;
                };
                let mut names = TypeSubstitution::default();
                let derived = names.freshen(&proof.signature, &mut selected.infer);
                let mut trial = selected.infer.clone();
                if trial.unify(parameter, &derived).is_ok() {
                    selected.infer = trial;
                    *value = Value::Closure(specialize_closure(closure, proof));
                }
            }
            if selected.infer.resolve(&selected.signature) == before {
                break;
            }
        }
        selected.signature = selected.infer.resolve(&selected.signature);
        let Type::Fn(params, _) = &selected.signature else {
            unreachable!()
        };
        for (parameter, value) in params.iter().zip(args.iter_mut()) {
            let valid = match value {
                | Value::Closure(closure) if matches!(parameter, Type::Fn(..)) => {
                    if let Some(proof) = &closure.proof {
                        match proof.specialize(parameter) {
                            | Ok(proof) => {
                                let valid = proof.verified;
                                *value = Value::Closure(specialize_closure(closure, proof));
                                valid
                            },
                            | Err(_) => false,
                        }
                    } else {
                        false
                    }
                },
                | _ => self.runtime_input_matches(value, parameter),
            };
            if !valid {
                selected.verified = false;
            }
        }
        selected.check_reifications()?;
        Ok(Some(Rc::new(selected)))
    }

    #[cfg(feature = "jit")]
    pub(crate) fn self_specialization_matches(
        &self,
        current: &Rc<Closure>,
        instance: Option<&Rc<FunctionProof>>,
        target: &Value,
        callee_span: Span,
        args: &[Value],
    ) -> bool {
        let Value::Closure(target_closure) = target else {
            return false;
        };
        // The scope must still contain the actual canonical base closure.
        // A rebound selected clone of that base remains an interpreter call,
        // even if its currently visible signature happens to be identical.
        if !Closure::same_origin(current, target_closure)
            || !Weak::ptr_eq(&Closure::origin(current), &Rc::downgrade(target_closure))
        {
            return false;
        }
        let Some(current_proof) = instance else {
            return Rc::ptr_eq(current, target_closure);
        };
        if !current_proof.verified {
            return false;
        }
        let mut schemas = args.to_vec();
        let Ok(Some(next)) = self.prepare_function_call(target, &mut schemas, Some(callee_span))
        else {
            return false;
        };
        if !next.verified
            || !Rc::ptr_eq(&current_proof.context, &next.context)
            || aevia_types::class::has_variables(&current_proof.signature)
            || current_proof.signature != next.signature
        {
            return false;
        }
        let left = current_proof.projection.resolved(&current_proof.infer);
        let right = next.projection.resolved(&next.infer);
        let closed = |values: &TypeSubstitution| {
            !values.types.values().any(aevia_types::class::has_variables)
                && !values
                    .representations
                    .values()
                    .any(|rep| matches!(rep, aevia_types::Rep::Var(_)))
                && values.dimensions.values().all(aevia_dim::Dim::is_concrete)
                && values
                    .rows
                    .values()
                    .all(|row| !aevia_types::class::has_variables(&Type::Record(row.clone())))
                && values.naturals.values().all(|nat| nat.vars().is_empty())
        };
        closed(&left)
            && closed(&right)
            && left.types == right.types
            && left.representations == right.representations
            && left.dimensions == right.dimensions
            && left.rows == right.rows
            && left.naturals == right.naturals
    }

    /// Bind a specialization to an escaping value at an actual source
    /// expression. Ordinary host apply/atom copying never calls this hook.
    pub(crate) fn bind_source_function(
        &self,
        value: Value,
        span: Span,
    ) -> Value {
        let Some(expected @ Type::Fn(..)) = self.resolved_expr_type(span) else {
            return value;
        };
        let Value::Closure(closure) = &value else {
            return value;
        };
        let Some(original) = &closure.proof else {
            return value;
        };
        if original.signature == expected {
            return value;
        }
        let Ok(proof) = original.specialize(&expected) else {
            return value;
        };
        Value::Closure(specialize_closure(closure, proof))
    }
}

impl Interp {
    fn opaque_named(
        ty: &Type,
        name: &str,
    ) -> bool {
        matches!(ty,Type::Adt(id,args) if args.is_empty() && aevia_types::adt::module_of(*id).is_prelude() && aevia_types::adt::name_of(*id)==name)
    }

    pub(crate) fn runtime_input_matches(
        &self,
        value: &Value,
        expected: &Type,
    ) -> bool {
        match (value, expected) {
            | (Value::KernelValue(_), ty) if *ty == aevia_types::builtins::dynamic_value_type() => {
                true
            },
            | (Value::KernelError(_), ty) if *ty == aevia_types::builtins::op_error_type() => true,
            | (Value::Callback(callback), ty) if Self::opaque_named(ty, "Callback") => {
                self.reify_callable(&callback.function, &callback.schema)
                    .is_ok()
            },
            | (Value::TypeRep(actual), Type::Adt(id, args))
                if aevia_types::adt::module_of(*id).is_prelude()
                    && aevia_types::adt::name_of(*id) == "TypeRep"
                    && args.len() == 1 =>
            {
                crate::host_witness::strict_schema_equal(actual, &args[0])
            },
            | (Value::Closure(closure), ty @ Type::Fn(..)) => {
                closure
                    .proof
                    .as_ref()
                    .is_some_and(|proof| proof.verified && proof.specialize(ty).is_ok())
            },
            | (Value::Tensor(tensor), Type::Tensor { shape, elem }) => {
                if tensor.shape.len() != shape.rank() as usize
                    || tensor.shape.iter().enumerate().any(|(axis, extent)| {
                        shape
                            .extent(axis)
                            .is_some_and(|expected| expected != *extent)
                    })
                    || tensor
                        .shape
                        .iter()
                        .try_fold(1usize, |total, extent| total.checked_mul(*extent))
                        != Some(tensor.storage.len())
                {
                    return false;
                }
                if tensor.storage.is_empty() {
                    return tensor.verified_element.as_ref().is_some_and(|actual| {
                        crate::host_witness::strict_schema_equal(actual, elem)
                    }) || crate::host_witness::capture(value, expected, false).is_some();
                }
                (0..tensor.storage.len()).all(|index| {
                    tensor
                        .get_flat(index)
                        .is_some_and(|value| self.runtime_input_matches(&value, elem))
                })
            },
            | (Value::Record(record), Type::Record(row)) => {
                row.fields().iter().all(|(name, ty)| {
                    record
                        .fields
                        .get(name)
                        .is_some_and(|value| self.runtime_input_matches(value, ty))
                }) && (row.tail_var().is_some() || record.fields.len() == row.fields().len())
            },
            | _ => crate::host_witness::capture(value, expected, false).is_some(),
        }
    }

    fn call_argument_schema(
        &self,
        value: &Value,
    ) -> Option<Type> {
        match value {
            | Value::Closure(closure) => {
                closure.proof.as_ref().map(|proof| proof.signature.clone())
            },
            | Value::KernelValue(_) => Some(aevia_types::builtins::dynamic_value_type()),
            | Value::KernelError(_) => Some(aevia_types::builtins::op_error_type()),
            | Value::Callback(_) => {
                Some(Type::Adt(
                    aevia_types::adt::intern(aevia_types::adt::Adt {
                        name: "Callback".into(),
                        params: Vec::new(),
                        ctors: Vec::new(),
                    }),
                    Vec::new(),
                ))
            },
            | Value::Tensor(tensor) => {
                let Type::Tensor { elem, .. } = crate::host_witness::runtime_schema(value)? else {
                    return None;
                };
                Some(Type::tensor(
                    aevia_types::Shape::refined(tensor.shape.iter().copied().map(Some)),
                    *elem,
                ))
            },
            | _ => crate::host_witness::runtime_schema(value),
        }
    }
}
