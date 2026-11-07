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
    /// The activation bindings this proof induces, computed once.
    ///
    /// Every call of a proved function installs `bindings()` as the active
    /// substitution. Building it resolves the whole projection, and a fresh
    /// `Rc` per call also defeated [`CallProofCache`] (whose key includes the
    /// active bindings' identity), so a recursive loop never hit the cache.
    pub(crate) bindings_memo: BindingsMemo,
}

/// A per-proof memo that a clone does not inherit.
///
/// A cloned proof is about to be refined (`specialize`, call-site selection),
/// so the original's bindings would be stale for it.
#[derive(Debug, Default)]
pub(crate) struct BindingsMemo(std::cell::OnceCell<Rc<CallBindings>>);

impl Clone for BindingsMemo {
    fn clone(&self) -> Self {
        Self::default()
    }
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
            bindings_memo: BindingsMemo::default(),
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

    fn checked_selection(
        &self,
        projection: &aevia_types::CallableProjection,
        values: &TypeSubstitution,
    ) -> Result<Rc<Self>, EvalError> {
        if projection.origin != self.definition.identity {
            if projection.selected {
                return Err(EvalError::new(
                    "E0302",
                    "generic selection belongs to a different checked callable",
                ));
            }
            // A host may replace a non-selected ordinary binding. Its own
            // function proof stays authoritative; no foreign phantom choices
            // are copied merely because its Fn shape is compatible.
            return Ok(Rc::new(self.clone()));
        }
        if projection.arguments.len() != self.definition.binders.len() {
            if projection.selected {
                return Err(EvalError::new(
                    "E0304",
                    "incomplete checked generic binder projection",
                ));
            }
            return Ok(Rc::new(self.clone()));
        }
        let mut next = self.clone();
        let mut names = TypeSubstitution::default();
        for (binder, argument) in self.definition.binders.iter().zip(&projection.arguments) {
            if std::mem::discriminant(binder) != std::mem::discriminant(argument) {
                return Err(EvalError::new(
                    "E0302",
                    "checked generic binder kind does not match the actual declaration",
                ));
            }
            let requested = values.apply(&argument.as_type());
            let requested = names.freshen(&requested, &mut next.infer);
            let own = self.projection.apply(&binder.as_type());
            next.infer.unify(&own, &requested).map_err(|error| {
                EvalError::new(
                    "E0302",
                    format!(
                        "checked generic selection conflicts with the actual callable: {error}"
                    ),
                )
            })?;
        }
        next.signature = next.infer.resolve(&next.signature);
        Ok(Rc::new(next))
    }

    pub(crate) fn bindings(&self) -> Rc<CallBindings> {
        self.bindings_memo
            .0
            .get_or_init(|| {
                Rc::new(CallBindings {
                    context: Rc::downgrade(&self.context),
                    values: self.projection.resolved(&self.infer),
                    verified: self.verified,
                })
            })
            .clone()
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

    pub(crate) fn checked_callable_selection(
        &self,
        proof: &Rc<FunctionProof>,
        span: Span,
        required: bool,
    ) -> Result<Rc<FunctionProof>, EvalError> {
        let Some(projection) = self.context.evidence.callable_refs.get(&span) else {
            return if required {
                Err(EvalError::new(
                    "E0304",
                    "function type application has no checked binder identity",
                )
                .at(span))
            } else {
                Ok(proof.clone())
            };
        };
        if required && projection.origin != proof.definition.identity {
            return Err(EvalError::new(
                "E0302",
                "function type application belongs to a different checked callable",
            )
            .at(span));
        }
        let values = self.bindings.as_ref().filter(|bindings| {
            bindings.verified
                && bindings
                    .context
                    .upgrade()
                    .is_some_and(|context| Rc::ptr_eq(&context, &self.context))
        });
        let empty = TypeSubstitution::default();
        proof.checked_selection(
            projection,
            values.map_or(&empty, |bindings| &bindings.values),
        )
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
        // The selection below is a pure function of the proof, the call site,
        // the active bindings and the arguments' runtime schemas; for scalar
        // arguments the schema is all `runtime_input_matches` looks at. A
        // recursive numeric loop asks the same question every iteration, and
        // answering it by re-unification was ~90% of the cost of a call.
        let key = CallProofKey::new(self, original, callee, args);
        if let Some(key) = &key {
            if let Some(hit) = self.call_proofs.get(key, args) {
                return Ok(Some(hit));
            }
        }
        let result = self.prepare_function_call_uncached(original, args, callee)?;
        let result = result.map(|proof| self.call_proofs.canonical(original, proof));
        if let (Some(key), Some(proof)) = (key, &result) {
            self.call_proofs.insert(
                key,
                args,
                proof.clone(),
                (
                    original.clone(),
                    self.bindings.clone(),
                    self.context.clone(),
                ),
            );
        }
        Ok(result)
    }

    fn prepare_function_call_uncached(
        &self,
        original: &Rc<FunctionProof>,
        args: &mut [Value],
        callee: Option<Span>,
    ) -> Result<Option<Rc<FunctionProof>>, EvalError> {
        let expected = callee.and_then(|span| self.resolved_expr_type(span));
        let selected = if let Some(span) = callee {
            self.checked_callable_selection(original, span, false)?
        } else {
            original.clone()
        };
        let selected = if let Some(expected @ Type::Fn(..)) = &expected {
            selected.specialize(expected)?
        } else {
            selected
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
        // Cheap rejections first: this runs on every `let` and return, and
        // resolving the checked type allocates.
        let Value::Closure(closure) = &value else {
            return value;
        };
        if closure.proof.is_none() || !matches!(self.expr_type(span), Some(Type::Fn(..))) {
            return value;
        }
        let Some(expected @ Type::Fn(..)) = self.resolved_expr_type(span) else {
            return value;
        };
        let Some(original) = &closure.proof else {
            return value;
        };
        let selected = self.checked_callable_selection(original, span, false);
        let proof = match selected.and_then(|proof| proof.specialize(&expected)) {
            | Ok(proof) => proof,
            | Err(error) => {
                return Value::Native(Rc::new(crate::value::NativeFn(Box::new(move |_, _, _| {
                    Err(error.clone())
                }))));
            },
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
                if let Type::Scalar(_, dim) = &**elem {
                    if &tensor.dim != dim {
                        return false;
                    }
                }
                if self.tensor_inputs.contains(tensor, elem) {
                    return true;
                }
                // A suffix can reuse only a complete proof of its immutable
                // root. If the root has not been seen, validate the whole root
                // once; a valid suffix never certifies an invalid prefix.
                if TensorInputCache::eligible(elem)
                    && matches!(tensor.storage, crate::value::Storage::SharedSlice(_))
                {
                    let root = TensorInputCache::root(tensor);
                    if tensor.dim == root.dim
                        && self.runtime_input_matches(
                            &Value::Tensor(root.clone()),
                            &Type::tensor(
                                aevia_types::Shape::Rank(root.rank() as u32),
                                (**elem).clone(),
                            ),
                        )
                    {
                        return true;
                    }
                }
                if tensor.storage.is_empty() {
                    return tensor.verified_element.as_ref().is_some_and(|actual| {
                        crate::host_witness::strict_schema_equal(actual, elem)
                    }) || crate::host_witness::capture(value, expected, false).is_some();
                }
                let valid = (0..tensor.storage.len()).all(|index| {
                    tensor
                        .get_flat(index)
                        .is_some_and(|value| self.runtime_input_matches(&value, elem))
                });
                if valid {
                    self.tensor_inputs.remember(tensor, elem);
                }
                valid
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

// Primitive input proofs retain only Weak identities. A live Weak prevents
// Rc::get_mut; Rc::make_mut either detaches it or clones to a different identity.
// No proof survives such a change, and no proof keeps the payload alive.
/// The identity of one call-site proof question; see
/// [`Interp::prepare_function_call`].
///
/// The arguments enter only as a digest of their scalar schemas (tag and
/// dimension), so a lookup allocates nothing: the tail-call allocation gate
/// counts every allocation a loop step makes. Entries under one key are told
/// apart by [`ArgumentSchema`]s, compared in place against the values.
#[derive(Clone, PartialEq, Eq, Hash, Debug)]
pub(crate) struct CallProofKey {
    proof: usize,
    callee: Option<Span>,
    bindings: usize,
    context: usize,
    digest: u64,
}

/// A scalar argument's schema: its representation tag and, for a number,
/// its dimension. Equal schemas make `runtime_input_matches` answer equally.
#[derive(Clone, PartialEq, Debug)]
struct ArgumentSchema {
    tag: u8,
    dim: Option<aevia_dim::Dim>,
}

fn argument_tag(value: &Value) -> Option<(u8, Option<&aevia_dim::Dim>)> {
    Some(match value {
        | Value::Num(number) => {
            let tag = match number.kind {
                | crate::value::NumKind::Int(_) => 0,
                | crate::value::NumKind::Exact(_) => 1,
                | crate::value::NumKind::Sym(_) => 2,
                | crate::value::NumKind::Float(_) => 3,
                | crate::value::NumKind::Complex(..) => 4,
            };
            (tag, Some(&number.dim))
        },
        | Value::Bool(_) => (5, None),
        | Value::Str(_) => (6, None),
        | Value::Unit => (7, None),
        // Anything else is validated by content or rewritten in place
        // (closures are specialized), so it is not memoizable here.
        | _ => return None,
    })
}

fn schemas_match(
    schemas: &[ArgumentSchema],
    args: &[Value],
) -> bool {
    schemas.len() == args.len()
        && schemas.iter().zip(args).all(|(schema, value)| {
            argument_tag(value)
                .is_some_and(|(tag, dim)| tag == schema.tag && dim == schema.dim.as_ref())
        })
}

impl CallProofKey {
    fn new(
        interp: &Interp,
        proof: &Rc<FunctionProof>,
        callee: Option<Span>,
        args: &[Value],
    ) -> Option<Self> {
        use std::hash::Hash;
        use std::hash::Hasher;
        let mut hasher = crate::fxhash::FxHasher::default();
        for value in args {
            let (tag, dim) = argument_tag(value)?;
            tag.hash(&mut hasher);
            dim.hash(&mut hasher);
        }
        Some(Self {
            proof: Rc::as_ptr(proof) as usize,
            callee,
            bindings: interp
                .bindings
                .as_ref()
                .map_or(0, |bindings| Rc::as_ptr(bindings) as usize),
            context: Rc::as_ptr(&interp.context) as usize,
            digest: hasher.finish(),
        })
    }
}

/// Memoized call-site proofs.
///
/// Each entry keeps the proof, bindings and context whose addresses its key
/// names alive, so an address in a key can never be reused by a different
/// object while the entry exists.
#[derive(Default)]
pub(crate) struct CallProofCache {
    entries: std::cell::RefCell<crate::fxhash::FxHashMap<CallProofKey, Vec<CallProofEntry>>>,
    /// Concrete results already handed out, per original proof.
    ///
    /// Each activation installs its proof's bindings, and the next call's key
    /// names those bindings by identity. Without this, a recursive call made
    /// a new (equal) proof every iteration, so its key never repeated.
    canonical: std::cell::RefCell<
        crate::fxhash::FxHashMap<usize, (Rc<FunctionProof>, Vec<Rc<FunctionProof>>)>,
    >,
}

struct CallProofEntry {
    arguments: Vec<ArgumentSchema>,
    result: Rc<FunctionProof>,
    _pins: (
        Rc<FunctionProof>,
        Option<Rc<CallBindings>>,
        Rc<ModuleContext>,
    ),
}

impl CallProofCache {
    const CAPACITY: usize = 4096;
    const PER_PROOF: usize = 8;

    /// An already-issued result equal to `proof`, or `proof` itself.
    ///
    /// Only fully concrete proofs are merged: for them, equal signatures,
    /// verification and resolved bindings mean every later use (selection,
    /// specialization, reification checks) answers identically.
    fn canonical(
        &self,
        original: &Rc<FunctionProof>,
        proof: Rc<FunctionProof>,
    ) -> Rc<FunctionProof> {
        if aevia_types::class::has_variables(&proof.signature) {
            return proof;
        }
        let bindings = proof.bindings();
        if bindings
            .values
            .types
            .values()
            .any(aevia_types::class::has_variables)
            || bindings
                .values
                .representations
                .values()
                .any(|rep| matches!(rep, aevia_types::Rep::Var(_)))
            || !bindings
                .values
                .dimensions
                .values()
                .all(aevia_dim::Dim::is_concrete)
            || bindings
                .values
                .rows
                .values()
                .any(|row| aevia_types::class::has_variables(&Type::Record(row.clone())))
            || bindings
                .values
                .naturals
                .values()
                .any(|nat| !nat.vars().is_empty())
        {
            return proof;
        }
        let mut canonical = self.canonical.borrow_mut();
        if canonical.len() >= Self::CAPACITY {
            canonical.clear();
        }
        let (_, issued) = canonical
            .entry(Rc::as_ptr(original) as usize)
            .or_insert_with(|| (original.clone(), Vec::new()));
        for existing in issued.iter() {
            let other = existing.bindings();
            if existing.verified == proof.verified
                && existing.signature == proof.signature
                && Rc::ptr_eq(&existing.context, &proof.context)
                && other.values.types == bindings.values.types
                && other.values.representations == bindings.values.representations
                && other.values.dimensions == bindings.values.dimensions
                && other.values.rows == bindings.values.rows
                && other.values.naturals == bindings.values.naturals
            {
                return existing.clone();
            }
        }
        if issued.len() < Self::PER_PROOF {
            issued.push(proof.clone());
        }
        proof
    }

    fn get(
        &self,
        key: &CallProofKey,
        args: &[Value],
    ) -> Option<Rc<FunctionProof>> {
        self.entries.borrow().get(key).and_then(|entries| {
            entries
                .iter()
                .find(|entry| schemas_match(&entry.arguments, args))
                .map(|entry| entry.result.clone())
        })
    }

    fn insert(
        &self,
        key: CallProofKey,
        args: &[Value],
        result: Rc<FunctionProof>,
        pins: (
            Rc<FunctionProof>,
            Option<Rc<CallBindings>>,
            Rc<ModuleContext>,
        ),
    ) {
        let Some(arguments) = args
            .iter()
            .map(|value| {
                argument_tag(value).map(|(tag, dim)| {
                    ArgumentSchema {
                        tag,
                        dim: dim.cloned(),
                    }
                })
            })
            .collect::<Option<Vec<_>>>()
        else {
            return;
        };
        let mut entries = self.entries.borrow_mut();
        if entries.len() >= Self::CAPACITY {
            entries.clear();
        }
        entries.entry(key).or_default().push(CallProofEntry {
            arguments,
            result,
            _pins: pins,
        });
    }
}

#[derive(Default)]
pub(crate) struct TensorInputCache {
    entries: std::cell::RefCell<
        std::collections::VecDeque<(std::rc::Weak<crate::value::TensorData>, Type)>,
    >,
}
impl TensorInputCache {
    const CAPACITY: usize = 256;

    fn eligible(schema: &Type) -> bool {
        crate::host_witness::is_leaf_schema(schema) && !aevia_types::class::has_variables(schema)
    }

    fn root(mut tensor: &Rc<crate::value::TensorData>) -> &Rc<crate::value::TensorData> {
        while let crate::value::Storage::SharedSlice(view) = &tensor.storage {
            tensor = &view.source;
        }
        tensor
    }

    fn contains(
        &self,
        tensor: &Rc<crate::value::TensorData>,
        schema: &Type,
    ) -> bool {
        if !Self::eligible(schema) {
            return false;
        }
        let root = Self::root(tensor);
        if tensor.dim != root.dim {
            return false;
        }
        self.entries
            .borrow()
            .iter()
            .rev()
            .any(|(identity, proven)| {
                identity
                    .upgrade()
                    .is_some_and(|source| Rc::ptr_eq(&source, root))
                    && crate::host_witness::strict_schema_equal(proven, schema)
            })
    }

    fn remember(
        &self,
        tensor: &Rc<crate::value::TensorData>,
        schema: &Type,
    ) {
        if !Self::eligible(schema)
            || matches!(tensor.storage, crate::value::Storage::SharedSlice(_))
        {
            return;
        }
        let mut entries = self.entries.borrow_mut();
        entries.retain(|(identity, _)| identity.strong_count() != 0);
        while entries.len() >= Self::CAPACITY {
            entries.pop_front();
        }
        entries.push_back((Rc::downgrade(tensor), schema.clone()));
    }
}

#[cfg(test)]
mod tensor_input_cache_tests {
    use super::*;
    use crate::value::Storage;
    use crate::value::TensorData;
    fn tensor() -> Rc<TensorData> {
        Rc::new(TensorData::new(
            vec![2],
            Storage::Dense(Rc::new(vec![1., 2.])),
            aevia_dim::Dim::one(),
            None,
        ))
    }
    fn matches(
        i: &Interp,
        tensor: &Rc<TensorData>,
        elem: Type,
    ) -> bool {
        i.runtime_input_matches(
            &Value::Tensor(tensor.clone()),
            &Type::tensor(aevia_types::Shape::Rank(1), elem),
        )
    }
    #[test]
    fn weak_identity_proofs_block_get_mut_and_make_mut_invalidates_them() {
        let i = Interp::new(&aevia_types::CheckedModule::default());
        let mut data = tensor();
        let schema = Type::float(aevia_dim::Dim::one());
        assert!(matches(&i, &data, schema.clone()));
        assert_eq!(i.tensor_inputs.entries.borrow().len(), 1);
        let old = Rc::downgrade(&data);
        assert!(Rc::get_mut(&mut data).is_none());
        Rc::make_mut(&mut data).storage =
            Storage::Boxed(vec![Value::string("bad"), Value::string("bad")]);
        assert!(old.upgrade().is_none());
        assert!(!matches(&i, &data, schema));
    }
    #[test]
    fn shared_root_proofs_do_not_bypass_dimension_shape_or_schema_validation() {
        let i = Interp::new(&aevia_types::CheckedModule::default());
        let data = tensor();
        let schema = Type::float(aevia_dim::Dim::one());
        assert!(matches(&i, &data, schema.clone()));
        let mut tail = data.tail_view(1).unwrap();
        assert!(matches(&i, &tail, schema.clone()));
        assert!(!matches(&i, &tail, Type::int()));
        assert!(!matches(&i, &tail, Type::Var(77)));
        Rc::make_mut(&mut tail).dim = aevia_dim::Dim::base("Length");
        assert!(!matches(&i, &tail, schema.clone()));
        Rc::make_mut(&mut tail).dim = aevia_dim::Dim::one();
        Rc::make_mut(&mut tail).shape = vec![2];
        assert!(!matches(&i, &tail, schema));
    }
    #[test]
    fn proofs_are_strictly_bounded_even_when_all_input_roots_remain_alive() {
        let i = Interp::new(&aevia_types::CheckedModule::default());
        let roots = (0..400).map(|_| tensor()).collect::<Vec<_>>();
        for root in &roots {
            assert!(matches(&i, root, Type::float(aevia_dim::Dim::one())));
        }
        assert_eq!(
            i.tensor_inputs.entries.borrow().len(),
            TensorInputCache::CAPACITY
        );
    }
    #[test]
    fn a_homogeneous_suffix_never_certifies_a_malformed_prefix_of_its_root() {
        let i = Interp::new(&aevia_types::CheckedModule::default());
        let root = Rc::new(TensorData::new(
            vec![2],
            Storage::Boxed(vec![Value::string("bad"), Value::float(1.)]),
            aevia_dim::Dim::one(),
            None,
        ));
        let tail = root.tail_view(1).unwrap();
        let schema = Type::float(aevia_dim::Dim::one());
        assert!(matches(&i, &tail, schema.clone()));
        assert!(!matches(&i, &root, schema));
        assert!(i.tensor_inputs.entries.borrow().is_empty());
    }
}
