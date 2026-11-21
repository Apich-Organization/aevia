//! Parallel evaluation and concurrent tasks (§6.10, §9.7).
//!
//! # The design: shared-nothing replicas
//!
//! The interpreter is built on `Rc`: values, closures, scopes and the IR all
//! share by reference count, which keeps the single-threaded path fast and is
//! why a value cannot be handed to another thread. Converting the whole
//! runtime to `Arc` would tax every program to speed up a few. Instead each
//! worker thread owns a **replica** — an interpreter built from the same
//! program by the host's [`Replica`] factory, once per thread — and threads
//! exchange [`Portable`] values: a `Send` mirror of the data a pure function
//! consumes and produces (numbers with their dimension and display unit,
//! strings, tensors, records, variants, constructors and IR closures). A
//! homogeneous Float prefix of a map batch carries its values in a contiguous
//! buffer with shared scalar metadata.
//!
//! A closure crosses as *code identity plus captured data*: the Core IR is
//! closure-converted, so a lambda is its function's source span and the
//! values of its captures. The replica was built from the same sources, so
//! the span names the same function there.
//!
//! # The guarantee: parallelism never changes a value
//!
//! The language is pure, so a parallel evaluation can always be replaced by
//! the sequential one. Every primitive here returns `None` — "run it
//! sequentially" — when a value cannot cross (a symbolic term, a kernel
//! handle, an IO action, a closure with generic evidence), when no replica
//! factory is installed, or when a worker fails for any reason; the
//! sequential rerun then produces exactly the value or the error (with its
//! source span and call stack) the program would have produced anyway.
//!
//! Results never depend on the number of threads: `par_map` keeps the input
//! order, and `par_reduce` combines fixed blocks whose boundaries depend only
//! on the input length, so even floating-point reductions are reproducible
//! across machines with different core counts — and identical to the
//! sequential fallback, which uses the same blocks.
//!
//! # Concurrent tasks
//!
//! `spawn(f)` starts the IO action `f()` on a worker and returns a `Task`;
//! `await(t)` waits for it. A task's console output is buffered on its worker
//! and written when the task is awaited, in await order, so the program's
//! output is deterministic however the tasks interleave. A task whose thunk
//! cannot cross runs on the spawning thread when it is spawned (same
//! buffering), so `spawn` is always available.

#![cfg_attr(
    not(test),
    deny(
        missing_docs,
        clippy::arithmetic_side_effects,
        clippy::expect_used,
        clippy::indexing_slicing,
        clippy::nursery,
        clippy::panic,
        clippy::pedantic,
        clippy::single_call_fn,
        clippy::unwrap_used
    )
)]

use crate::Interp;
use crate::eval::EvalError;
use crate::evidence::ModuleContext;
use crate::generics::BindingsMemo;
use crate::generics::ClosedMemo;
use crate::generics::FunctionProof;
use crate::value::Closure;
use crate::value::ClosureBody;
use crate::value::ClosureEnv;
use crate::value::CtorValue;
use crate::value::ExactRep;
use crate::value::IrEnv;
use crate::value::NumKind;
use crate::value::NumVal;
use crate::value::Scope;
use crate::value::SourceCtorToken;
use crate::value::Storage;
use crate::value::TensorData;
use crate::value::Value;
use crate::value::VariantValue;
use aevia_dim::Atom;
use aevia_dim::Dim;
use aevia_dim::UnitDef;
use aevia_syntax::FileId;
use aevia_syntax::Span;
use aevia_types::Type;
use aevia_types::TypeSubstitution;
use aevia_types::callable::CallableId;
use aevia_types::infer::Infer;
use num_bigint::BigInt;
use num_rational::BigRational;
use std::cell::OnceCell;
use std::cell::RefCell;
use std::collections::HashMap;
use std::collections::VecDeque;
use std::rc::Rc;
use std::sync::Arc;
use std::sync::Mutex;
use std::sync::Weak;
use std::sync::atomic::AtomicUsize;
use std::sync::atomic::Ordering;

/// Builds a fresh interpreter for the running program on a worker thread.
///
/// The host that loaded the program installs one ([`Interp::set_replica`]).
/// The CLI and [`crate::run`] share an immutable checked revision with workers.
/// The factory must initialize the same modules in the same order as the main
/// interpreter because closures cross by source span.
pub type Replica = Arc<ReplicaFactory>;

type ReplicaFactory = dyn Fn() -> Result<Interp, String> + Send + Sync;

/// A numeric payload.
#[derive(Clone, Debug)]
pub enum PNum {
    /// An `Int`.
    Int(BigInt),
    /// An exact `Real`.
    Exact(BigRational),
    /// A `Float`.
    Float(f64),
    /// A `Complex`.
    Complex(f64, f64),
}

/// The `Send` mirror of a data value.
#[derive(Clone, Debug)]
pub enum Portable {
    /// `()`.
    Unit,
    /// A `Bool`.
    Bool(bool),
    /// A scalar with its dimension and display unit.
    Num(PNum, Dim, Option<UnitDef>),
    /// A string.
    Str(String),
    /// A dense `f64` tensor.
    Dense {
        /// Row-major shape.
        shape: Vec<usize>,
        /// Elements in row-major order.
        data: Vec<f64>,
        /// The element dimension.
        dim: Dim,
        /// The display unit.
        display: Option<UnitDef>,
    },
    /// Any other tensor, element by element.
    Boxed {
        /// Row-major shape.
        shape: Vec<usize>,
        /// Elements in row-major order.
        elements: Vec<Self>,
        /// The element dimension.
        dim: Dim,
    },
    /// A record, in field order.
    Record(Vec<(String, Self)>),
    /// A constructed variant.
    Variant {
        /// Its type.
        adt: u32,
        /// The constructor index.
        index: usize,
        /// The constructor name.
        name: String,
        /// The payload.
        fields: Vec<Self>,
    },
    /// A constructor or a builtin, possibly partially applied.
    Ctor {
        /// Its type (`u32::MAX` for a builtin).
        adt: u32,
        /// The constructor index.
        index: usize,
        /// The name.
        name: String,
        /// Fields it takes.
        arity: usize,
        /// Fields collected so far.
        args: Vec<Self>,
    },
    /// A Core IR closure: the function's source position and its captured
    /// values. A `FileId` is allocated per load, so the position names its
    /// file by the module's identity, which is the same in every replica.
    Closure {
        /// The module's identity (`Interp::file_keys`).
        module: Arc<str>,
        /// Byte offsets of the function in that file.
        start: u32,
        /// End offset.
        end: u32,
        /// The captured values, in capture order.
        captures: Vec<Self>,
    },
}

const MAX_DEPTH: usize = 256;

#[derive(Clone)]
struct Transfer {
    value: Portable,
    metadata: TransferMetadata,
}

impl Transfer {
    const fn unit() -> Self {
        Self {
            value: Portable::Unit,
            metadata: TransferMetadata {
                next: 0,
                entries: VecDeque::new(),
            },
        }
    }
}

/// Constructor, variant, tensor and closure metadata is keyed by preorder
/// position among those nodes, including those with no metadata. Source
/// evidence stays private and is copied only from interpreter-issued values.
#[derive(Clone, Default)]
struct TransferMetadata {
    next: usize,
    entries: VecDeque<(usize, ValueMetadata)>,
}

#[derive(Clone)]
enum ValueMetadata {
    Constructor(SourceCtorToken),
    Variant {
        type_args: Option<Vec<Type>>,
        verified_type: Option<Type>,
        constructor_origin: bool,
    },
    Tensor(TensorMetadata),
    Closure(Arc<ClosureSelection>),
}

/// A checked selection without the interpreter-local module and memo caches.
#[derive(Clone)]
struct ClosureSelection {
    identity: CallableId,
    infer: Infer,
    projection: TypeSubstitution,
    signature: Type,
    verified: bool,
}

impl From<&FunctionProof> for ClosureSelection {
    fn from(proof: &FunctionProof) -> Self {
        Self {
            identity: proof.definition.identity.clone(),
            infer: proof.infer.clone(),
            projection: proof.projection.clone(),
            signature: proof.signature.clone(),
            verified: proof.verified,
        }
    }
}

impl ClosureSelection {
    fn restore(
        self: Arc<Self>,
        context: &Rc<ModuleContext>,
        span: Span,
    ) -> Option<Rc<FunctionProof>> {
        let definition = context.evidence.functions.get(&span)?;
        if definition.identity != self.identity {
            return None;
        }
        let selection = Arc::unwrap_or_clone(self);
        Some(Rc::new(FunctionProof {
            context: context.clone(),
            definition: definition.clone(),
            infer: selection.infer,
            projection: selection.projection,
            signature: selection.signature,
            verified: selection.verified,
            bindings_memo: BindingsMemo::default(),
            closed_memo: ClosedMemo::default(),
        }))
    }
}

#[derive(Clone, Default)]
struct TensorMetadata {
    element: Option<Type>,
    display: Option<UnitDef>,
    storage: NonDenseStorage,
}

impl TensorMetadata {
    fn into_entry(self) -> Option<ValueMetadata> {
        (self.element.is_some()
            || self.display.is_some()
            || !matches!(self.storage, NonDenseStorage::Boxed))
        .then_some(ValueMetadata::Tensor(self))
    }
}

#[derive(Clone, Copy, Default)]
enum NonDenseStorage {
    #[default]
    Boxed,
    Bool,
    Exact,
    TypedExact(ExactRep),
}

impl NonDenseStorage {
    #[cfg_attr(
        not(test),
        expect(
            clippy::single_call_fn,
            reason = "keep buffer classification next to reconstruction"
        )
    )]
    fn encoded_as_elements(storage: &Storage) -> Option<Self> {
        match storage.backing() {
            | Storage::Boxed(_) => Some(Self::Boxed),
            | Storage::Bool(_) => Some(Self::Bool),
            | Storage::Exact(_) => Some(Self::Exact),
            | Storage::TypedExact(_, rep) => Some(Self::TypedExact(*rep)),
            | _ => None,
        }
    }

    fn restore(
        self,
        values: Vec<Value>,
        checkpoint: &mut TransferCheckpoint<'_>,
    ) -> Option<Storage> {
        let mut rationals = |values: Vec<Value>| {
            values
                .into_iter()
                .map(|value| {
                    if checkpoint.cancelled() {
                        return None;
                    }
                    match value {
                        | Value::Num(NumVal {
                            kind: NumKind::Int(value),
                            ..
                        }) => Some(value.to_rational()),
                        | Value::Num(NumVal {
                            kind: NumKind::Exact(value),
                            ..
                        }) => Some(Rc::unwrap_or_clone(value)),
                        | _ => None,
                    }
                })
                .collect::<Option<Vec<_>>>()
        };
        Some(match self {
            | Self::Boxed => Storage::Boxed(values),
            | Self::Bool => {
                Storage::Bool(
                    values
                        .into_iter()
                        .map(|value| {
                            if checkpoint.cancelled() {
                                return None;
                            }
                            if let Value::Bool(value) = value {
                                Some(value)
                            } else {
                                None
                            }
                        })
                        .collect::<Option<Vec<_>>>()?,
                )
            },
            | Self::Exact => Storage::Exact(rationals(values)?),
            | Self::TypedExact(rep) => Storage::TypedExact(rationals(values)?, rep),
        })
    }
}

struct TransferMismatch;

impl TransferMetadata {
    fn position(&mut self) -> Option<usize> {
        let position = self.next;
        self.next = position.checked_add(1)?;
        Some(position)
    }

    fn record(
        &mut self,
        entry: Option<ValueMetadata>,
    ) -> Option<()> {
        let position = self.position()?;
        if let Some(entry) = entry {
            self.entries.push_back((position, entry));
        }
        Some(())
    }

    fn take(&mut self) -> Result<Option<ValueMetadata>, TransferMismatch> {
        let position = self.position().ok_or(TransferMismatch)?;
        if let Some((recorded, _)) = self.entries.front()
            && *recorded <= position
        {
            return (*recorded == position)
                .then(|| self.entries.pop_front().map(|(_, entry)| entry))
                .ok_or(TransferMismatch);
        }
        Ok(None)
    }

    fn take_tensor(&mut self) -> Result<TensorMetadata, TransferMismatch> {
        match self.take()? {
            | Some(ValueMetadata::Tensor(tensor)) => Ok(tensor),
            | None => Ok(TensorMetadata::default()),
            | _ => Err(TransferMismatch),
        }
    }
}

// A boxed tensor or record can contain many nodes inside one transfer.
struct TransferCheckpoint<'a> {
    cancellation: Option<&'a crate::CancellationToken>,
    remaining: u8,
}

impl<'a> TransferCheckpoint<'a> {
    const fn new(interp: &'a Interp) -> Self {
        Self {
            cancellation: interp.cancellation.as_ref(),
            remaining: 0,
        }
    }

    fn cancelled(&mut self) -> bool {
        let Some(token) = self.cancellation else {
            return false;
        };
        // Check the root and then every 256 values without an atomic read per leaf.
        let checkpoint = self.remaining == 0;
        self.remaining = self.remaining.wrapping_sub(1);
        checkpoint && token.is_cancelled()
    }
}

/// The portable form of `value`, or `None` when it cannot cross a thread.
#[must_use]
fn to_portable(
    interp: &Interp,
    value: &Value,
) -> Option<Transfer> {
    let mut metadata = TransferMetadata::default();
    let mut checkpoint = TransferCheckpoint::new(interp);
    let value = to_portable_at(&interp.file_keys, value, 0, &mut metadata, &mut checkpoint)?;
    metadata.next = 0;
    Some(Transfer { value, metadata })
}

#[cfg_attr(
    not(test),
    expect(
        clippy::too_many_lines,
        reason = "keep the thread transfer rules exhaustive in one match"
    )
)]
fn to_portable_at(
    keys: &FileKeys,
    value: &Value,
    depth: usize,
    metadata: &mut TransferMetadata,
    checkpoint: &mut TransferCheckpoint<'_>,
) -> Option<Portable> {
    if depth > MAX_DEPTH || checkpoint.cancelled() {
        return None;
    }
    let next = depth.checked_add(1)?;
    Some(match value {
        | Value::Unit => Portable::Unit,
        | Value::Bool(value) => Portable::Bool(*value),
        | Value::Num(num) => {
            let payload = match &num.kind {
                | NumKind::Int(value) => PNum::Int(value.to_big()),
                | NumKind::Exact(value) => PNum::Exact(value.as_ref().clone()),
                | NumKind::Float(value) => PNum::Float(*value),
                | NumKind::Complex(re, im) => PNum::Complex(*re, *im),
                // A symbolic term may hold a kernel handle owned by this
                // thread's session.
                | NumKind::Sym(_) => return None,
            };
            Portable::Num(payload, num.dim.clone(), num.display.as_deref().cloned())
        },
        | Value::Str(text) => Portable::Str(text.to_string()),
        | Value::Tensor(tensor) => {
            let element = tensor.verified_element.as_deref().cloned();
            if let Some(data) = tensor.dense_flat() {
                metadata.record(
                    TensorMetadata {
                        element,
                        ..TensorMetadata::default()
                    }
                    .into_entry(),
                )?;
                Portable::Dense {
                    shape: tensor.shape.clone(),
                    data: data.into_owned(),
                    dim: tensor.dim.clone(),
                    display: tensor.display.as_deref().cloned(),
                }
            } else {
                let storage = NonDenseStorage::encoded_as_elements(&tensor.storage)?;
                metadata.record(
                    TensorMetadata {
                        element,
                        display: tensor.display.as_deref().cloned(),
                        storage,
                    }
                    .into_entry(),
                )?;
                let mut elements = Vec::with_capacity(tensor.storage.len());
                for element in tensor.iter_elements() {
                    elements.push(to_portable_at(keys, &element, next, metadata, checkpoint)?);
                }
                if elements.len() != tensor.storage.len() {
                    return None;
                }
                Portable::Boxed {
                    shape: tensor.shape.clone(),
                    elements,
                    dim: tensor.dim.clone(),
                }
            }
        },
        | Value::Record(record) => {
            let mut fields = Vec::with_capacity(record.fields.len());
            for (name, field) in &record.fields {
                fields.push((
                    name.clone(),
                    to_portable_at(keys, field, next, metadata, checkpoint)?,
                ));
            }
            Portable::Record(fields)
        },
        | Value::Variant(variant) => {
            let has_proof = variant.type_args.is_some()
                || variant.verified_type.is_some()
                || variant.constructor_origin;
            metadata.record(has_proof.then(|| {
                ValueMetadata::Variant {
                    type_args: variant.type_args.clone(),
                    verified_type: variant.verified_type.clone(),
                    constructor_origin: variant.constructor_origin,
                }
            }))?;
            let mut fields = Vec::with_capacity(variant.fields.len());
            for field in &variant.fields {
                fields.push(to_portable_at(keys, field, next, metadata, checkpoint)?);
            }
            Portable::Variant {
                adt: variant.adt,
                index: variant.index,
                name: variant.name.clone(),
                fields,
            }
        },
        | Value::Ctor(ctor) => {
            let CtorValue {
                adt,
                index,
                name,
                arity,
                args,
                origin,
            } = &**ctor;
            metadata.record(origin.clone().map(ValueMetadata::Constructor))?;
            let mut collected = Vec::with_capacity(args.len());
            for arg in args {
                collected.push(to_portable_at(keys, arg, next, metadata, checkpoint)?);
            }
            Portable::Ctor {
                adt: *adt,
                index: *index,
                name: name.clone(),
                arity: *arity,
                args: collected,
            }
        },
        | Value::Closure(closure) => {
            let (ClosureBody::Ir(function), ClosureEnv::Ir(env)) = (&closure.body, &closure.env)
            else {
                return None;
            };
            // Generic evidence from an activation (a specialised proof's call
            // bindings) is interpreter state the replica cannot reconstruct;
            // the module scope's own bindings are rebuilt there.
            let module_bindings = env.scope.as_ref().and_then(|scope| scope.types.as_ref());
            match (&closure.captured_bindings, module_bindings) {
                | (None, _) => {},
                | (Some(captured), Some(module)) if Rc::ptr_eq(captured, module) => {},
                | _ => return None,
            }
            let selection =
                if std::rc::Weak::ptr_eq(&closure.canonical_origin, &std::rc::Weak::new()) {
                    None
                } else {
                    let proof = closure.proof.as_deref()?;
                    let context = env.scope.as_ref()?.context.as_ref()?;
                    if !Rc::ptr_eq(&proof.context, context)
                        || !context.evidence.functions.get(&function.span).is_some_and(
                            |definition| definition.identity == proof.definition.identity,
                        )
                    {
                        return None;
                    }
                    Some(ValueMetadata::Closure(Arc::new(ClosureSelection::from(
                        proof,
                    ))))
                };
            metadata.record(selection)?;
            let mut captures = Vec::new();
            for slot in env.slots.borrow().iter() {
                captures.push(to_portable_at(keys, slot, next, metadata, checkpoint)?);
            }
            Portable::Closure {
                module: keys.get(&function.span.file)?.clone(),
                start: function.span.start,
                end: function.span.end,
                captures,
            }
        },
        | _ => return None,
    })
}

/// What a worker knows about its replica: the functions by span and the
/// module scopes by file.
struct ReplicaIndex {
    functions: HashMap<Span, Rc<aevia_ir::Function>>,
    files: HashMap<Arc<str>, FileId>,
}

impl ReplicaIndex {
    #[cfg_attr(
        not(test),
        expect(
            clippy::single_call_fn,
            reason = "build the closure index lazily at the transport boundary"
        )
    )]
    fn build(interp: &Interp) -> Self {
        let mut functions = HashMap::new();
        let mut pending: Vec<Rc<aevia_ir::Function>> = Vec::new();
        for value in interp.globals.values() {
            collect_value_functions(value, &mut pending, 0);
        }
        for scope in interp.file_scopes.values() {
            for value in scope.vars.borrow().values() {
                collect_value_functions(value, &mut pending, 0);
            }
        }
        for template in interp.context.instances.borrow().iter() {
            if let crate::evidence::MethodCode::Ir(function) = &*template.code {
                pending.push(function.clone());
            }
        }
        while let Some(function) = pending.pop() {
            if functions.contains_key(&function.span) {
                continue;
            }
            collect_block(&function.body, &mut pending);
            functions.insert(function.span, function);
        }
        let files = interp
            .file_keys
            .iter()
            .map(|(file, key)| (key.clone(), *file))
            .collect();
        Self { functions, files }
    }
}

fn collect_value_functions(
    value: &Value,
    out: &mut Vec<Rc<aevia_ir::Function>>,
    depth: usize,
) {
    if depth > MAX_DEPTH {
        return;
    }
    let next = depth.saturating_add(1);
    match value {
        | Value::Closure(closure) => {
            // Closure environments can be recursive. Their code already
            // identifies nested functions without walking the captures.
            if let ClosureBody::Ir(function) = &closure.body {
                out.push(function.clone());
            }
        },
        | Value::Record(record) => {
            for field in record.fields.values() {
                collect_value_functions(field, out, next);
            }
        },
        | Value::Variant(variant) => {
            for field in &variant.fields {
                collect_value_functions(field, out, next);
            }
        },
        | Value::Ctor(ctor) => {
            for arg in &ctor.args {
                collect_value_functions(arg, out, next);
            }
        },
        | Value::Tensor(tensor) => {
            if let Storage::Boxed(items) = &tensor.storage {
                for item in items {
                    collect_value_functions(item, out, next);
                }
            }
        },
        | _ => {},
    }
}

fn collect_block(
    block: &aevia_ir::Block,
    out: &mut Vec<Rc<aevia_ir::Function>>,
) {
    use aevia_ir::Rhs;
    use aevia_ir::Stmt;
    use aevia_ir::Tail;
    for stmt in &block.stmts {
        let Stmt::Let(item) = stmt else {
            continue;
        };
        match &item.rhs {
            | Rhs::Closure(function) | Rhs::RecClosure { function, .. } => {
                out.push(function.clone());
            },
            | Rhs::Block(inner) | Rhs::AndOr { right: inner, .. } => collect_block(inner, out),
            // A `do` program's statements are lowered with the function, so
            // lambdas written inside `main` are indexed even though the
            // replica never runs `main`.
            | Rhs::Do(plan) => {
                for step in &plan.steps {
                    match step {
                        | aevia_ir::DoStep::Let { value, .. }
                        | aevia_ir::DoStep::Bind { value, .. }
                        | aevia_ir::DoStep::Action { value, .. } => collect_block(value, out),
                    }
                }
            },
            | _ => {},
        }
    }
    match &block.tail {
        | Tail::If {
            then_block,
            else_block,
            ..
        } => {
            collect_block(then_block, out);
            collect_block(else_block, out);
        },
        | Tail::Match { arms, .. } => {
            for arm in arms {
                if let Some(guard) = &arm.guard {
                    collect_block(guard, out);
                }
                collect_block(&arm.body, out);
            }
        },
        | _ => {},
    }
}

/// Rebuilds a value on an interpreter with the same loaded program.
fn from_portable(
    interp: &Interp,
    index: &OnceCell<ReplicaIndex>,
    mut transfer: Transfer,
) -> Option<Value> {
    let mut checkpoint = TransferCheckpoint::new(interp);
    let value = from_portable_at(
        interp,
        index,
        transfer.value,
        &mut transfer.metadata,
        &mut checkpoint,
    )?;
    transfer.metadata.entries.is_empty().then_some(value)
}

#[cfg_attr(
    not(test),
    expect(
        clippy::too_many_lines,
        reason = "keep the reconstruction rules exhaustive in one match"
    )
)]
fn from_portable_at(
    interp: &Interp,
    index: &OnceCell<ReplicaIndex>,
    value: Portable,
    metadata: &mut TransferMetadata,
    checkpoint: &mut TransferCheckpoint<'_>,
) -> Option<Value> {
    if checkpoint.cancelled() {
        return None;
    }
    Some(match value {
        | Portable::Unit => Value::Unit,
        | Portable::Bool(value) => Value::Bool(value),
        | Portable::Num(payload, dim, display) => {
            Value::Num(NumVal {
                kind: match payload {
                    | PNum::Int(value) => NumKind::Int(value.into()),
                    | PNum::Exact(value) => NumKind::Exact(Rc::new(value)),
                    | PNum::Float(value) => NumKind::Float(value),
                    | PNum::Complex(re, im) => NumKind::Complex(re, im),
                },
                dim,
                display: display.map(Rc::new),
            })
        },
        | Portable::Str(text) => Value::string(text),
        | Portable::Dense {
            shape,
            data,
            dim,
            display,
        } => {
            let mut tensor = TensorData::new(
                shape,
                Storage::Dense(Rc::new(data)),
                dim,
                display.map(Rc::new),
            );
            tensor.verified_element = metadata.take_tensor().ok()?.element.map(Rc::new);
            Value::Tensor(Rc::new(tensor))
        },
        | Portable::Boxed { shape, elements, dim } => {
            let tensor_metadata = metadata.take_tensor().ok()?;
            let mut values = Vec::with_capacity(elements.len());
            for element in elements {
                values.push(from_portable_at(
                    interp, index, element, metadata, checkpoint,
                )?);
            }
            let storage = tensor_metadata.storage.restore(values, checkpoint)?;
            let mut tensor =
                TensorData::new(shape, storage, dim, tensor_metadata.display.map(Rc::new));
            tensor.verified_element = tensor_metadata.element.map(Rc::new);
            Value::Tensor(Rc::new(tensor))
        },
        | Portable::Record(fields) => {
            let mut collected = indexmap::IndexMap::new();
            for (name, field) in fields {
                collected.insert(
                    name,
                    from_portable_at(interp, index, field, metadata, checkpoint)?,
                );
            }
            Value::Record(Rc::new(crate::value::RecordValue { fields: collected }))
        },
        | Portable::Variant {
            adt,
            index: ctor,
            name,
            fields,
        } => {
            let (type_args, verified_type, constructor_origin) = match metadata.take().ok()? {
                | Some(ValueMetadata::Variant {
                    type_args,
                    verified_type,
                    constructor_origin,
                }) => (type_args, verified_type, constructor_origin),
                | None => (None, None, false),
                | _ => return None,
            };
            let mut payload = Vec::with_capacity(fields.len());
            for field in fields {
                payload.push(from_portable_at(
                    interp, index, field, metadata, checkpoint,
                )?);
            }
            Value::Variant(Rc::new(VariantValue {
                adt,
                index: ctor,
                name,
                fields: payload,
                type_args,
                verified_type,
                constructor_origin,
            }))
        },
        | Portable::Ctor {
            adt,
            index: ctor,
            name,
            arity,
            args,
        } => {
            let origin = match metadata.take().ok()? {
                | Some(ValueMetadata::Constructor(token)) => Some(token),
                | None => None,
                | _ => return None,
            };
            let mut values = Vec::with_capacity(args.len());
            for arg in args {
                values.push(from_portable_at(interp, index, arg, metadata, checkpoint)?);
            }
            Value::ctor(CtorValue {
                adt,
                index: ctor,
                name,
                arity,
                args: values,
                origin,
            })
        },
        | Portable::Closure {
            module,
            start,
            end,
            captures,
        } => {
            let selection = match metadata.take().ok()? {
                | Some(ValueMetadata::Closure(selection)) => Some(selection),
                | None => None,
                | _ => return None,
            };
            let lookup = index.get_or_init(|| ReplicaIndex::build(interp));
            let Some(file) = lookup.files.get(&*module).copied() else {
                return fallback(&format!(
                    "the replica has no module `{module}` (it has {:?})",
                    lookup.files.keys().collect::<Vec<_>>()
                ));
            };
            let span = Span { file, start, end };
            let Some(function) = lookup.functions.get(&span).cloned() else {
                return fallback(&format!(
                    "the replica has no function at {start}..{end} of `{module}` ({} indexed)",
                    lookup.functions.len()
                ));
            };
            let scope = interp.file_scopes.get(&span.file)?.clone();
            let mut slots = Vec::with_capacity(captures.len());
            for capture in captures {
                slots.push(from_portable_at(
                    interp, index, capture, metadata, checkpoint,
                )?);
            }
            if slots.len() != function.captures.len() {
                return None;
            }
            let selected = selection.is_some();
            let proof = if let Some(selection) = selection {
                Some(selection.restore(scope.context.as_ref()?, span)?)
            } else {
                scope
                    .context
                    .as_ref()
                    .and_then(|context| FunctionProof::at(context, span, scope.types.as_ref()))
            };
            Value::Closure(Rc::new_cyclic(|origin| {
                Closure {
                    // The original Rc belongs to another thread. A selected
                    // closure starts its own local cache identity and can cross again.
                    canonical_origin: if selected {
                        origin.clone()
                    } else {
                        std::rc::Weak::new()
                    },
                    params: Vec::new(),
                    body: ClosureBody::Ir(function.clone()),
                    env: ClosureEnv::Ir(Rc::new(IrEnv {
                        slots: RefCell::new(slots),
                        scope: Some(scope.clone()),
                    })),
                    name: Some(function.name.clone()),
                    proof,
                    captured_bindings: scope.types.clone(),
                }
            }))
        },
    })
}

// ---------------------------------------------------------------------------
// Workers
// ---------------------------------------------------------------------------

struct Worker {
    recipe: Weak<ReplicaFactory>,
    interp: Interp,
    index: OnceCell<ReplicaIndex>,
}

thread_local! {
    /// Idle replicas on this thread. A job takes one out and returns it, so
    /// a nested `par_map` that rayon runs on the same thread (while the
    /// outer job's replica is mid-call) takes — or builds — another one
    /// instead of re-borrowing a busy interpreter.
    static WORKERS: RefCell<Vec<Worker>> = const { RefCell::new(Vec::new()) };
}

static DISPATCHED: AtomicUsize = AtomicUsize::new(0);
static REPLICAS: AtomicUsize = AtomicUsize::new(0);

/// How many work items ran on worker threads, and how many replicas were
/// built, since the process started (for tests and `--stats`).
#[must_use]
pub fn stats() -> (usize, usize) {
    (
        DISPATCHED.load(Ordering::Relaxed),
        REPLICAS.load(Ordering::Relaxed),
    )
}

/// Runs `job` on this thread's replica of `replica`, building it first if
/// the thread has none (or one of another program).
fn with_worker<T>(
    replica: &Replica,
    cancellation: Option<&crate::CancellationToken>,
    capabilities: u8,
    job: impl FnOnce(&mut Interp, &OnceCell<ReplicaIndex>) -> Option<T>,
) -> Option<T> {
    let _cancellation_scope = crate::cancellation::enter_optional(cancellation.cloned());
    if cancellation.is_some_and(crate::CancellationToken::is_cancelled) {
        return None;
    }
    let idle = WORKERS.with(|cell| {
        let mut workers = cell.borrow_mut();
        // The weak reference keeps its allocation from becoming another
        // program's address, while allowing the old factory to be dropped.
        workers.retain(|worker| std::ptr::addr_eq(worker.recipe.as_ptr(), Arc::as_ptr(replica)));
        workers.pop()
    });
    let mut worker = match idle {
        | Some(worker) => worker,
        | None => {
            let mut interp = match replica() {
                | Ok(interp) => interp,
                | Err(error) => return fallback(&format!("the replica failed to build: {error}")),
            };
            // A replica evaluates sequentially. Nested data parallelism would
            // be safe in a work-stealing pool, but rayon runs part of a nested
            // map on the calling thread, whose replica is busy, so each nested
            // level builds new replicas — measured slower than running the
            // inner map in place. A task on a worker likewise runs locally
            // (`await` would block a pool thread).
            interp.replica = None;
            interp.on_worker = true;
            interp.output = crate::eval::Output::Buffer(Rc::new(RefCell::new(String::new())));
            REPLICAS.fetch_add(1, Ordering::Relaxed);
            Worker {
                recipe: Arc::downgrade(replica),
                interp,
                index: OnceCell::new(),
            }
        },
    };
    worker.interp.cancellation = cancellation.cloned();
    worker.interp.check_cancelled().ok()?;
    DISPATCHED.fetch_add(1, Ordering::Relaxed);
    // A job can be launched from within a narrower capability scope than the
    // replica's factory. Keep the factory policy across reuse as a separate cap.
    let factory_capabilities = worker.interp.capabilities;
    worker.interp.capabilities &= capabilities;
    let result = job(&mut worker.interp, &worker.index);
    worker.interp.capabilities = factory_capabilities;
    worker.interp.cancellation = None;
    WORKERS.with(|cell| cell.borrow_mut().push(worker));
    result
}

struct PoolCache {
    pool: Option<Arc<rayon::ThreadPool>>,
}

impl PoolCache {
    #[allow(
        clippy::single_call_fn,
        reason = "initializes both the global pool and an isolated sizing test"
    )]
    const fn new() -> Self {
        Self { pool: None }
    }

    #[allow(
        clippy::single_call_fn,
        reason = "pool sizing is tested independently of the global cache"
    )]
    fn get(
        &mut self,
        jobs: usize,
        work_items: usize,
    ) -> Option<Arc<rayon::ThreadPool>> {
        let budget = jobs.clamp(1, rayon::max_num_threads());
        let needed = work_items.clamp(1, budget);
        if let Some(pool) = self.pool.as_ref()
            && (needed..=budget).contains(&pool.current_num_threads())
        {
            return Some(Arc::clone(pool));
        }
        // Grow in steps so slightly larger workloads can reuse the same workers.
        let threads = needed
            .checked_next_power_of_two()
            .unwrap_or(budget)
            .min(budget);
        let pool = Arc::new(
            rayon::ThreadPoolBuilder::new()
                .num_threads(threads)
                // Workers need enough stack for recursive interpretation.
                .stack_size(256 * 1024 * 1024)
                .thread_name(|index| format!("aevia-worker-{index}"))
                .build()
                .ok()?,
        );
        // Running callers retain their pool while unused configurations exit.
        self.pool = Some(Arc::clone(&pool));
        Some(pool)
    }
}

/// Reuses the runtime worker pool within the host's job budget.
pub(crate) fn pool(
    jobs: usize,
    work_items: usize,
) -> Option<Arc<rayon::ThreadPool>> {
    static POOL: Mutex<PoolCache> = Mutex::new(PoolCache::new());
    POOL.lock().ok()?.get(jobs, work_items)
}

/// The number of worker threads to use when the host does not say.
#[must_use]
pub fn default_jobs() -> usize {
    std::env::var("AEVIA_JOBS")
        .ok()
        .and_then(|text| text.parse().ok())
        .unwrap_or_else(aevia_rssn::execution::default_cpu_threads)
        .max(1)
}

/// Inputs below this size are mapped sequentially: the replica round trip
/// costs more than it saves.
const MIN_PARALLEL: usize = 32;

/// Give work stealing room to balance uneven inputs without rebuilding the
/// portable closure once per element on small maps.
const MAP_BATCHES_PER_WORKER: usize = 4;

const REDUCE_BLOCK_SIZE: usize = 1024;

struct FloatMapBatch {
    values: Vec<f64>,
    dim: Dim,
    display: Option<UnitDef>,
}

// Once an output cannot share the prefix's metadata, the remainder uses the
// canonical transfer representation. This retains exact metadata for a boxed
// result without copying a unit for every value of a homogeneous batch.
struct MappedBatch {
    floats: Option<FloatMapBatch>,
    rest: Vec<Transfer>,
}

// Nominal dimensions compare equal across import aliases. A mixed output may
// still need to restore the original spelling of every boxed value.
#[cfg_attr(
    not(test),
    expect(
        clippy::single_call_fn,
        reason = "keeps the alias-sensitive transfer check explicit"
    )
)]
fn same_map_dimension(
    first: &Dim,
    next: &Dim,
) -> bool {
    first == next
        && first
            .factors()
            .zip(next.factors())
            .all(|((left, _), (right, _))| {
                match (left, right) {
                    | (Atom::Nominal(left), Atom::Nominal(right)) => {
                        left.display_name() == right.display_name()
                    },
                    | _ => true,
                }
            })
}

fn map_worker_batch(
    interp: &mut Interp,
    index: &OnceCell<ReplicaIndex>,
    function: &Transfer,
    len: usize,
    mut next: impl FnMut(&Interp, &OnceCell<ReplicaIndex>) -> Option<Value>,
) -> Option<MappedBatch> {
    let function = from_portable(interp, index, function.clone())
        .or_else(|| fallback("the replica has no such function"))?;
    let mut numbers = Vec::new();
    let mut metadata: Option<(Dim, Option<Rc<UnitDef>>)> = None;
    let mut rest = Vec::new();
    for _ in 0..len {
        let value = next(interp, index)?;
        let result = match interp.apply(function.clone(), vec![value], Span::default()) {
            | Ok(result) => result,
            | Err(error) if error.code == crate::cancellation::INTERRUPTED => return None,
            | Err(error) => {
                return fallback(&format!(
                    "a worker failed ({}: {}); rerunning sequentially",
                    error.code, error.message
                ));
            },
        };
        if rest.is_empty()
            && let Value::Num(NumVal {
                kind: NumKind::Float(raw),
                dim,
                display,
            }) = &result
        {
            let (first_dim, first_display) = metadata.get_or_insert_with(|| {
                numbers.reserve_exact(len);
                (dim.clone(), display.clone())
            });
            let same_display = match (first_display.as_ref(), display.as_ref()) {
                | (Some(left), Some(right)) => Rc::ptr_eq(left, right),
                | (None, None) => true,
                | _ => false,
            };
            if same_display && same_map_dimension(first_dim, dim) {
                numbers.push(*raw);
                continue;
            }
        }
        if rest.capacity() == 0 {
            rest.reserve_exact(len.saturating_sub(numbers.len()));
        }
        rest.push(to_portable(interp, &result).or_else(|| fallback("a result cannot cross back"))?);
    }
    interp.check_cancelled().ok()?;
    Some(MappedBatch {
        floats: metadata.map(|(dim, display)| {
            FloatMapBatch {
                values: numbers,
                dim,
                display: display.as_deref().cloned(),
            }
        }),
        rest,
    })
}

#[derive(Clone, Copy)]
enum ReduceInput<'a> {
    Boxed(&'a [Value]),
    Dense {
        values: &'a [f64],
        dim: &'a Dim,
        display: Option<&'a UnitDef>,
    },
}

fn reduce_worker_batch(
    interp: &mut Interp,
    index: &OnceCell<ReplicaIndex>,
    function: &Transfer,
    zero: &Transfer,
    len: usize,
    mut next: impl FnMut(&Interp, &OnceCell<ReplicaIndex>) -> Option<Value>,
) -> Option<Vec<Transfer>> {
    let function = from_portable(interp, index, function.clone())?;
    let zero = from_portable(interp, index, zero.clone())?;
    let mut results = Vec::with_capacity(len.div_ceil(REDUCE_BLOCK_SIZE));
    for start in (0..len).step_by(REDUCE_BLOCK_SIZE) {
        let mut acc = zero.clone();
        for _ in 0..len.saturating_sub(start).min(REDUCE_BLOCK_SIZE) {
            let value = next(interp, index)?;
            acc = interp
                .apply(function.clone(), vec![acc, value], Span::default())
                .ok()?;
        }
        results.push(to_portable(interp, &acc)?);
    }
    Some(results)
}

impl Interp {
    /// Installs the factory workers build their replicas with (see
    /// [`Replica`]). Parallel evaluation of Aevia closures uses this factory.
    pub fn set_replica(
        &mut self,
        replica: Replica,
    ) {
        self.replica = Some(replica);
    }

    /// Sets how many worker threads parallel primitives may use; `1` makes
    /// every primitive sequential.
    pub fn set_jobs(
        &mut self,
        jobs: usize,
    ) {
        self.jobs = jobs.max(1);
    }

    fn parallel_replica(&self) -> Option<Replica> {
        if self.jobs <= 1 {
            return None;
        }
        self.replica.clone()
    }

    /// Maps values on worker threads and returns a tensor in input order.
    /// `None` requests sequential replay (see the module documentation).
    /// With no explicit dimension, the tensor uses the first result's dimension.
    pub(crate) fn par_map_values(
        &self,
        function: &Value,
        elements: &[Value],
        dim: Option<Dim>,
    ) -> Option<Value> {
        if elements.len() < MIN_PARALLEL {
            return None;
        }
        let replica = self
            .parallel_replica()
            .or_else(|| fallback("no replica factory, or --jobs 1"))?;
        let function = to_portable(self, function)
            .or_else(|| fallback("the function cannot cross threads"))?;
        let mut inputs = Vec::with_capacity(elements.len());
        for element in elements {
            inputs
                .push(to_portable(self, element).or_else(|| fallback("an element cannot cross"))?);
        }
        let pool =
            pool(self.jobs, inputs.len()).or_else(|| fallback("the worker pool did not start"))?;
        let batch_size = inputs.len().div_ceil(
            pool.current_num_threads()
                .saturating_mul(MAP_BATCHES_PER_WORKER),
        );
        let cancellation = self.cancellation_token().map(|token| token.child());
        let capabilities = self.capabilities;
        let results: Option<Vec<MappedBatch>> = pool.install(|| {
            use rayon::prelude::*;
            inputs
                .par_chunks_mut(batch_size)
                .map(|inputs| {
                    with_worker(
                        &replica,
                        cancellation.as_ref(),
                        capabilities,
                        |interp, index| {
                            let len = inputs.len();
                            let mut inputs = inputs.iter_mut();
                            map_worker_batch(interp, index, &function, len, |interp, index| {
                                from_portable(
                                    interp,
                                    index,
                                    std::mem::replace(inputs.next()?, Transfer::unit()),
                                )
                            })
                        },
                    )
                    .or_else(|| fallback("a worker could not run"))
                })
                .collect()
        });
        self.finish_parallel_map(results?, elements.len(), dim)
    }

    /// Maps a checked contiguous `Float` window through the normal worker dispatch.
    /// Each worker boxes its own inputs and shares their display unit within a batch.
    pub(crate) fn par_map_dense_values(
        &self,
        function: &Value,
        elements: &[f64],
        dim: &Dim,
        display: Option<&UnitDef>,
    ) -> Option<Value> {
        if elements.len() < MIN_PARALLEL {
            return None;
        }
        let replica = self
            .parallel_replica()
            .or_else(|| fallback("no replica factory, or --jobs 1"))?;
        let function = to_portable(self, function)
            .or_else(|| fallback("the function cannot cross threads"))?;
        let pool = pool(self.jobs, elements.len())
            .or_else(|| fallback("the worker pool did not start"))?;
        let batch_size = elements.len().div_ceil(
            pool.current_num_threads()
                .saturating_mul(MAP_BATCHES_PER_WORKER),
        );
        let cancellation = self.cancellation_token().map(|token| token.child());
        let capabilities = self.capabilities;
        // `install` joins the workers before the caller's immutable slice and
        // scalar metadata leave scope. No interpreter-owned Rc crosses threads.
        let results: Option<Vec<MappedBatch>> = pool.install(|| {
            use rayon::prelude::*;
            elements
                .par_chunks(batch_size)
                .map(|inputs| {
                    with_worker(
                        &replica,
                        cancellation.as_ref(),
                        capabilities,
                        |interp, index| {
                            let display = display.cloned().map(Rc::new);
                            let mut elements = inputs.iter().copied();
                            map_worker_batch(interp, index, &function, inputs.len(), |_, _| {
                                Some(Value::Num(NumVal {
                                    kind: NumKind::Float(elements.next()?),
                                    dim: dim.clone(),
                                    display: display.clone(),
                                }))
                            })
                        },
                    )
                    .or_else(|| fallback("a worker could not run"))
                })
                .collect()
        });
        self.finish_parallel_map(results?, elements.len(), None)
    }

    /// Tabulates integer indices on workers without a caller-owned input buffer.
    pub(crate) fn par_tabulate_values(
        &self,
        function: &Value,
        count: usize,
    ) -> Option<Value> {
        if count < MIN_PARALLEL {
            return None;
        }
        let replica = self
            .parallel_replica()
            .or_else(|| fallback("no replica factory, or --jobs 1"))?;
        let function = to_portable(self, function)
            .or_else(|| fallback("the function cannot cross threads"))?;
        let pool = pool(self.jobs, count).or_else(|| fallback("the worker pool did not start"))?;
        let batch_size = count.div_ceil(
            pool.current_num_threads()
                .saturating_mul(MAP_BATCHES_PER_WORKER),
        );
        let cancellation = self.cancellation_token().map(|token| token.child());
        let capabilities = self.capabilities;
        let results: Option<Vec<MappedBatch>> = pool.install(|| {
            use rayon::prelude::*;
            (0..count.div_ceil(batch_size))
                .into_par_iter()
                .map(|batch| {
                    let start = batch.checked_mul(batch_size)?;
                    let end = start.saturating_add(batch_size).min(count);
                    let mut indices = start..end;
                    with_worker(
                        &replica,
                        cancellation.as_ref(),
                        capabilities,
                        |interp, index| {
                            map_worker_batch(interp, index, &function, indices.len(), |_, _| {
                                indices.next().map(|index| Value::Num(NumVal::int(index)))
                            })
                        },
                    )
                    .or_else(|| fallback("a worker could not run"))
                })
                .collect()
        });
        self.finish_parallel_map(results?, count, None)
    }

    fn finish_parallel_map(
        &self,
        results: Vec<MappedBatch>,
        len: usize,
        dim: Option<Dim>,
    ) -> Option<Value> {
        self.check_cancelled().ok()?;
        if results.iter().all(|batch| batch.rest.is_empty()) {
            let mut batches = results.into_iter();
            let first = batches.next()?.floats?;
            let mut values = first.values;
            values.reserve_exact(len.saturating_sub(values.len()));
            let mut display = first.display;
            for batch in batches {
                let batch = batch.floats?;
                if display.is_none() {
                    display = batch.display;
                }
                // Check between bounded copies even when a worker batch is large.
                for chunk in batch.values.chunks(8192) {
                    self.check_cancelled().ok()?;
                    values.extend_from_slice(chunk);
                }
            }
            return Some(Value::Tensor(Rc::new(TensorData::new(
                vec![len],
                Storage::Dense(Rc::new(values)),
                dim.unwrap_or(first.dim),
                display.map(Rc::new),
            ))));
        }

        let index = OnceCell::new();
        let mut checkpoint = TransferCheckpoint::new(self);
        let mut values = Vec::with_capacity(len);
        for batch in results {
            if let Some(floats) = batch.floats {
                let display = floats.display.map(Rc::new);
                for raw in floats.values {
                    if checkpoint.cancelled() {
                        return None;
                    }
                    values.push(Value::Num(NumVal {
                        kind: NumKind::Float(raw),
                        dim: floats.dim.clone(),
                        display: display.clone(),
                    }));
                }
            }
            for portable in batch.rest {
                values.push(from_portable(self, &index, portable)?);
            }
        }
        let dim = values
            .first()
            .map_or_else(Dim::one, crate::value::value_dim);
        Some(crate::eval::build_tensor(values, vec![len], dim))
    }

    fn restore_parallel_map(
        &self,
        results: Vec<Vec<Transfer>>,
        len: usize,
    ) -> Option<Vec<Value>> {
        let index = OnceCell::new();
        let mut values = Vec::with_capacity(len);
        for portable in results.into_iter().flatten() {
            values.push(from_portable(self, &index, portable)?);
        }
        Some(values)
    }

    /// Rebuilds a worker's result on this interpreter: a closure returned
    /// by a worker is rebuilt from this side's own function index.
    fn local(
        &self,
        value: Transfer,
    ) -> Option<Value> {
        let index = OnceCell::new();
        from_portable(self, &index, value)
    }

    /// `par_reduce(f, zero, xs)`: folds fixed blocks of `xs` in parallel and
    /// then the block results in order. `f` must be associative with `zero`
    /// as its identity (§8.3); the block structure is the same on every
    /// machine and in the sequential fallback, so the result is too.
    pub(crate) fn par_reduce_values(
        &mut self,
        function: &Value,
        zero: &Value,
        tensor: &TensorData,
        span: Span,
    ) -> Result<Value, EvalError> {
        let dense = tensor.dense();
        let boxed = dense.is_none().then(|| tensor.elements());
        let input = match dense {
            | Some(values) => {
                ReduceInput::Dense {
                    values,
                    dim: &tensor.dim,
                    display: tensor.display.as_deref(),
                }
            },
            | None => ReduceInput::Boxed(boxed.as_deref().unwrap_or_default()),
        };
        let parallel = self.parallel_blocks(function, zero, input);
        self.check_cancelled().map_err(|error| error.at(span))?;
        let partial = parallel.map_or_else(
            || {
                let elements = boxed.unwrap_or_else(|| tensor.elements());
                elements
                    .chunks(REDUCE_BLOCK_SIZE)
                    .map(|block| {
                        let mut acc = zero.clone();
                        for element in block {
                            acc = self.apply(function.clone(), vec![acc, element.clone()], span)?;
                        }
                        Ok(acc)
                    })
                    .collect::<Result<Vec<_>, EvalError>>()
            },
            Ok,
        )?;
        let mut acc = zero.clone();
        for value in partial {
            acc = self.apply(function.clone(), vec![acc, value], span)?;
        }
        Ok(acc)
    }

    fn parallel_blocks(
        &self,
        function: &Value,
        zero: &Value,
        input: ReduceInput<'_>,
    ) -> Option<Vec<Value>> {
        let len = match &input {
            | ReduceInput::Boxed(values) => values.len(),
            | ReduceInput::Dense { values, .. } => values.len(),
        };
        if len <= REDUCE_BLOCK_SIZE {
            return None;
        }
        let replica = self.parallel_replica()?;
        let function = to_portable(self, function)?;
        let zero = to_portable(self, zero)?;
        let (mut packed, dense) = match input {
            | ReduceInput::Boxed(values) => {
                let mut packed = Vec::with_capacity(values.len());
                for value in values {
                    packed.push(to_portable(self, value)?);
                }
                (packed, None)
            },
            | ReduceInput::Dense { values, dim, display } => {
                (Vec::new(), Some((values, dim, display)))
            },
        };
        let blocks = len.div_ceil(REDUCE_BLOCK_SIZE);
        let pool = pool(self.jobs, blocks)?;
        let blocks_per_batch = blocks.div_ceil(
            pool.current_num_threads()
                .saturating_mul(MAP_BATCHES_PER_WORKER),
        );
        let batch_size = blocks_per_batch.saturating_mul(REDUCE_BLOCK_SIZE);
        let cancellation = self.cancellation_token().map(|token| token.child());
        let capabilities = self.capabilities;
        let results: Option<Vec<Vec<Transfer>>> = pool.install(|| {
            use rayon::iter::Either;
            use rayon::prelude::*;

            // Both batch iterators yield whole, consecutively ordered 1024-element blocks.
            let batches = match dense {
                | Some((values, dim, display)) => {
                    Either::Left(
                        values
                            .par_chunks(batch_size)
                            .map(move |values| Either::Left((values, dim, display))),
                    )
                },
                | None => Either::Right(packed.par_chunks_mut(batch_size).map(Either::Right)),
            };
            batches
                .map(|inputs| {
                    with_worker(
                        &replica,
                        cancellation.as_ref(),
                        capabilities,
                        |interp, index| {
                            match inputs {
                                | Either::Left((values, dim, display)) => {
                                    let display = display.cloned().map(Rc::new);
                                    let len = values.len();
                                    let mut values = values.iter().copied();
                                    reduce_worker_batch(
                                        interp,
                                        index,
                                        &function,
                                        &zero,
                                        len,
                                        |_, _| {
                                            Some(Value::Num(NumVal {
                                                kind: NumKind::Float(values.next()?),
                                                dim: dim.clone(),
                                                display: display.clone(),
                                            }))
                                        },
                                    )
                                },
                                | Either::Right(inputs) => {
                                    let len = inputs.len();
                                    let mut inputs = inputs.iter_mut();
                                    reduce_worker_batch(
                                        interp,
                                        index,
                                        &function,
                                        &zero,
                                        len,
                                        |interp, index| {
                                            from_portable(
                                                interp,
                                                index,
                                                std::mem::replace(inputs.next()?, Transfer::unit()),
                                            )
                                        },
                                    )
                                },
                            }
                        },
                    )
                })
                .collect()
        });
        self.restore_parallel_map(results?, blocks)
    }

    // ----- tasks ------------------------------------------------------------

    /// Starts the IO action `thunk()` as a task (§9.7.3).
    pub(crate) fn spawn_task(
        &mut self,
        thunk: &Value,
        span: Span,
    ) -> Value {
        let replica = self.parallel_replica().filter(|_| !self.on_worker);
        if let (Some(replica), Some(portable)) = (replica, to_portable(self, thunk))
            && let Some(pool) = pool(self.jobs, self.jobs)
        {
            let (sender, receiver) = std::sync::mpsc::channel();
            let cancellation = self.cancellation_token().map(|token| token.child());
            let task_cancellation = cancellation.clone();
            let capabilities = self.capabilities;
            pool.spawn(move || {
                let outcome = with_worker(
                    &replica,
                    cancellation.as_ref(),
                    capabilities,
                    |interp, index| {
                        let thunk = from_portable(interp, index, portable)?;
                        Some(run_task(interp, thunk))
                    },
                )
                .unwrap_or_else(|| {
                    let (code, message) = if cancellation
                        .as_ref()
                        .is_some_and(crate::CancellationToken::is_cancelled)
                    {
                        (crate::cancellation::INTERRUPTED, "evaluation interrupted")
                    } else {
                        ("E0811", "the task could not start on a worker")
                    };
                    TaskOutcome::Failed {
                        code,
                        message: message.into(),
                        output: String::new(),
                    }
                });
                let _ = sender.send(outcome);
            });
            return Value::Task(Rc::new(TaskHandle {
                state: RefCell::new(TaskState::Running(receiver)),
                cancellation: task_cancellation,
            }));
        }
        // The thunk cannot cross: run it here, now, with the same buffering.
        let saved = std::mem::replace(
            &mut self.output,
            crate::eval::Output::Buffer(Rc::new(RefCell::new(String::new()))),
        );
        let result = self
            .apply(thunk.clone(), Vec::new(), span)
            .and_then(|action| self.run_io_value(&action, span));
        let buffered = std::mem::replace(&mut self.output, saved);
        let output = match buffered {
            | crate::eval::Output::Buffer(buffer) => buffer.borrow().clone(),
            | crate::eval::Output::Console => String::new(),
        };
        Value::Task(Rc::new(TaskHandle {
            state: RefCell::new(TaskState::Local(result, output)),
            cancellation: None,
        }))
    }

    /// Waits for a task, writes its buffered output, and returns its value.
    pub(crate) fn await_task(
        &mut self,
        task: &TaskHandle,
        span: Span,
    ) -> Result<Value, EvalError> {
        if let Err(error) = self.check_cancelled() {
            if let Some(request) = &task.cancellation {
                request.cancel();
            }
            return Err(error.at(span));
        }
        let state = std::mem::replace(&mut *task.state.borrow_mut(), TaskState::Awaited);
        let (result, output) = match state {
            | TaskState::Local(result, output) => (result, output),
            | TaskState::Running(receiver) => {
                let outcome = loop {
                    if let Err(error) = self.check_cancelled() {
                        if let Some(request) = &task.cancellation {
                            request.cancel();
                        }
                        return Err(error.at(span));
                    }
                    match receiver.recv_timeout(std::time::Duration::from_millis(20)) {
                        | Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {},
                        | outcome => break outcome,
                    }
                };
                match outcome {
                    | Ok(TaskOutcome::Done { value, output }) => {
                        let value = self.local(value).ok_or_else(|| {
                            EvalError::new("E0811", "a task result could not be rebuilt").at(span)
                        });
                        (value, output)
                    },
                    | Ok(TaskOutcome::Failed {
                        code,
                        message,
                        output,
                    }) => (Err(EvalError::new(code, message).at(span)), output),
                    | Err(_) => {
                        (
                            Err(EvalError::new("E0811", "a task's worker thread stopped").at(span)),
                            String::new(),
                        )
                    },
                }
            },
            | TaskState::Awaited => {
                return Err(EvalError::new(
                    "E0811",
                    "this task was already awaited; a task's result is read once",
                )
                .at(span));
            },
        };
        if let Err(error) = self.check_cancelled() {
            if let Some(request) = &task.cancellation {
                request.cancel();
            }
            return Err(error.at(span));
        }
        if !output.is_empty() {
            self.write_out(&output, false, false);
        }
        result
    }

    /// Runs an IO value to completion (a plain value is its own result).
    fn run_io_value(
        &mut self,
        value: &Value,
        _span: Span,
    ) -> Result<Value, EvalError> {
        match value {
            | Value::Io(action) => self.run_io(action),
            | other => Ok(other.clone()),
        }
    }
}

/// Reports why a primitive runs sequentially when `AEVIA_PARALLEL_TRACE` is
/// set, and returns `None` so the caller falls back.
fn fallback<T>(reason: &str) -> Option<T> {
    if std::env::var_os("AEVIA_PARALLEL_TRACE").is_some() {
        eprintln!("aevia: parallel: sequential fallback: {reason}");
    }
    None
}

#[cfg_attr(
    not(test),
    expect(
        clippy::single_call_fn,
        reason = "separate task execution and result transfer from dispatch"
    )
)]
fn run_task(
    interp: &mut Interp,
    thunk: Value,
) -> TaskOutcome {
    if let crate::eval::Output::Buffer(buffer) = &interp.output {
        buffer.borrow_mut().clear();
    }
    let result = interp
        .apply(thunk, Vec::new(), Span::default())
        .and_then(|action| interp.run_io_value(&action, Span::default()))
        .and_then(|value| {
            let transferable = to_portable(interp, &value);
            interp.check_cancelled()?;
            Ok((value, transferable))
        });
    let output = match &interp.output {
        | crate::eval::Output::Buffer(buffer) => std::mem::take(&mut *buffer.borrow_mut()),
        | crate::eval::Output::Console => String::new(),
    };
    match result {
        | Ok((value, transferable)) => {
            match transferable {
                | Some(value) => TaskOutcome::Done { value, output },
                | None => {
                    TaskOutcome::Failed {
                        code: "E0811",
                        message: format!(
                            "a task's result must be data (numbers, strings, tensors, records, \
                             variants or IR closures); this one is a {}",
                            value.type_name()
                        ),
                        output,
                    }
                },
            }
        },
        | Err(error) => {
            TaskOutcome::Failed {
                code: error.code,
                message: error.message.clone(),
                output,
            }
        },
    }
}

enum TaskOutcome {
    Done {
        value: Transfer,
        output: String,
    },
    Failed {
        code: &'static str,
        message: String,
        output: String,
    },
}

/// A running or finished task (§9.7.3).
pub struct TaskHandle {
    state: RefCell<TaskState>,
    cancellation: Option<crate::CancellationToken>,
}

impl std::fmt::Debug for TaskHandle {
    fn fmt(
        &self,
        f: &mut std::fmt::Formatter<'_>,
    ) -> std::fmt::Result {
        f.write_str("<Task>")
    }
}

enum TaskState {
    Running(std::sync::mpsc::Receiver<TaskOutcome>),
    Local(Result<Value, EvalError>, String),
    Awaited,
}

/// The scope each module of a loaded program was evaluated in, by file.
pub(crate) type FileScopes = HashMap<FileId, Rc<Scope>>;

/// A stable identity per loaded file (logical source and owner), equal in
/// the main interpreter and every replica of it.
pub(crate) type FileKeys = HashMap<FileId, Arc<str>>;

#[cfg(test)]
mod tests {
    use super::*;
    use aevia_types::check::CheckedModule;

    #[test]
    fn a_later_caller_gets_its_requested_thread_count() {
        let wider = pool(3, 3).expect("three-thread pool");
        let narrower = pool(2, 2).expect("two-thread pool");
        assert_eq!(wider.current_num_threads(), 3);
        assert_eq!(narrower.current_num_threads(), 2);
    }

    #[test]
    fn a_worker_pool_grows_only_as_work_needs_it() {
        let mut cache = PoolCache::new();
        let small = cache.get(8, 2).unwrap();
        assert_eq!(small.current_num_threads(), 2);
        let larger = cache.get(8, 3).unwrap();
        assert_eq!(larger.current_num_threads(), 4);
        assert!(Arc::ptr_eq(&larger, &cache.get(8, 2).unwrap()));
        assert!(Arc::ptr_eq(&larger, &cache.get(4, 2).unwrap()));
        assert!(Arc::ptr_eq(&larger, &cache.get(6, 4).unwrap()));
        assert_eq!(small.current_num_threads(), 2);
        assert_eq!(cache.get(3, 10).unwrap().current_num_threads(), 3);
    }

    #[test]
    fn idle_replica_reserves_its_factory_identity_without_owning_it() {
        let factory: Replica = Arc::new(|| Ok(Interp::new(&CheckedModule::default())));
        assert_eq!(with_worker(&factory, None, 0, |_, _| Some(())), Some(()));
        assert_eq!(Arc::weak_count(&factory), 1);
        assert_eq!(Arc::strong_count(&factory), 1);
    }

    #[test]
    fn a_reused_worker_respects_both_the_caller_and_factory_mask() {
        use crate::value::caps;
        let factory: Replica = Arc::new(|| {
            let mut interp = Interp::new(&CheckedModule::default());
            interp.capabilities = caps::ENV | caps::FS;
            Ok(interp)
        });
        let observe = |worker: &mut Interp, _: &OnceCell<ReplicaIndex>| Some(worker.capabilities);
        assert_eq!(
            with_worker(&factory, None, caps::FS | caps::RAND, observe),
            Some(caps::FS)
        );
        assert_eq!(
            with_worker(&factory, None, caps::ALL, observe),
            Some(caps::ENV | caps::FS)
        );
    }

    #[test]
    fn a_task_cancelled_before_worker_start_reports_interruption_after_reset() {
        let mut interp = Interp::new(&CheckedModule::default());
        interp.set_jobs(2);
        interp.set_replica(Arc::new(|| Err("unexpected replica".to_owned())));
        let request = crate::CancellationToken::new();
        interp.set_cancellation_token(request.clone());
        request.cancel();
        let Value::Task(task) = interp.spawn_task(&Value::Unit, Span::default()) else {
            panic!("expected a task handle");
        };
        request.reset();
        let failure = interp
            .await_task(&task, Span::default())
            .expect_err("cancelled task");
        assert_eq!(failure.code, crate::cancellation::INTERRUPTED);
    }

    fn map_identity(
        interp: &mut Interp,
        inputs: Vec<Value>,
    ) -> MappedBatch {
        let function = Value::ctor(CtorValue {
            adt: u32::MAX,
            index: usize::MAX,
            name: "id".to_owned(),
            arity: usize::MAX,
            args: Vec::new(),
            origin: None,
        });
        let transfer = to_portable(interp, &function).unwrap();
        let mut inputs = inputs.into_iter();
        map_worker_batch(interp, &OnceCell::new(), &transfer, inputs.len(), |_, _| {
            inputs.next()
        })
        .unwrap()
    }

    #[test]
    fn float_map_reconstruction_preserves_raw_bits_and_first_explicit_display() {
        let mut interp = Interp::new(&CheckedModule::default());
        let nan = f64::from_bits(0x7ff8_0012_3456_789a);
        let raw = [-0.0_f64, nan, 1.25, -2.5];
        let metre = Dim::base("Metre");
        let cm = Rc::new(UnitDef::linear(
            "cm",
            metre.clone(),
            aevia_dim::Scale::one(),
        ));
        let inch = Rc::new(UnitDef::linear(
            "inch",
            metre.clone(),
            aevia_dim::Scale::one(),
        ));
        let batch = map_identity(
            &mut interp,
            vec![Value::float(raw[0]), Value::float(raw[1])],
        );
        assert!(batch.rest.is_empty());
        assert_eq!(batch.floats.as_ref().unwrap().values.len(), 2);
        let explicit = map_identity(
            &mut interp,
            vec![Value::Num(
                NumVal::float(raw[2], metre.clone()).with_display(cm),
            )],
        );
        let later = map_identity(
            &mut interp,
            vec![Value::Num(NumVal::float(raw[3], metre).with_display(inch))],
        );
        let Some(Value::Tensor(out)) =
            interp.finish_parallel_map(vec![batch, explicit, later], raw.len(), None)
        else {
            panic!("expected dense Float tensor")
        };
        assert!(out.dim.is_one());
        assert_eq!(out.display.as_ref().unwrap().name, "cm");
        let bits = out
            .dense()
            .unwrap()
            .iter()
            .map(|raw| raw.to_bits())
            .collect::<Vec<_>>();
        assert_eq!(bits, raw.map(f64::to_bits));
    }

    #[test]
    fn mixed_map_reconstruction_preserves_nominal_aliases_and_promotion() {
        let mut interp = Interp::new(&CheckedModule::default());
        let definition = Dim::nominal_base("markets", "Credit");
        let alias = definition.with_display_name("CustomerCredit");
        assert_eq!(definition, alias);
        let first = Value::Num(NumVal::float(8.0, definition.clone()));
        let second = Value::Num(NumVal::float(13.0, alias.clone()));
        let batch = map_identity(
            &mut interp,
            vec![first.clone(), second.clone(), Value::Bool(false)],
        );
        assert_eq!(batch.floats.as_ref().unwrap().values.len(), 1);
        assert_eq!(batch.rest.len(), 2);
        let later = map_identity(&mut interp, vec![Value::string("fallback")]);
        let Some(Value::Tensor(out)) = interp.finish_parallel_map(vec![batch, later], 4, None)
        else {
            panic!("expected mixed tensor")
        };
        let Storage::Boxed(values) = &out.storage else {
            panic!("expected boxed tensor")
        };
        assert_eq!(
            out.dim.to_display_si_string(),
            definition.to_display_si_string()
        );
        assert_eq!(
            values[0].as_num().unwrap().dim.to_display_si_string(),
            definition.to_display_si_string()
        );
        assert_eq!(
            values[1].as_num().unwrap().dim.to_display_si_string(),
            alias.to_display_si_string()
        );
        assert!(matches!(values[2], Value::Bool(false)));
        assert!(matches!(&values[3], Value::Str(text) if text.as_ref() == "fallback"));

        let numbers = map_identity(&mut interp, vec![first, Value::int(17), second]);
        let Some(Value::Tensor(out)) = interp.finish_parallel_map(vec![numbers], 3, None) else {
            panic!("expected promoted tensor")
        };
        assert_eq!(out.dense().unwrap(), &[8.0, 17.0, 13.0]);
    }

    #[test]
    fn cancelled_map_reconstruction_drops_the_packed_worker_result() {
        let mut interp = Interp::new(&CheckedModule::default());
        let token = crate::CancellationToken::new();
        interp.set_cancellation_token(token.clone());
        let dense = map_identity(&mut interp, vec![Value::float(2.0); 32]);
        assert!(dense.rest.is_empty());
        token.cancel();
        assert!(interp.finish_parallel_map(vec![dense], 32, None).is_none());
        assert_eq!(
            interp.check_cancelled().unwrap_err().code,
            crate::cancellation::INTERRUPTED,
        );
    }

    #[test]
    fn moving_a_reversed_tensor_view_preserves_its_dense_logical_values() {
        let tensor = TensorData::new(
            vec![3],
            Storage::Dense(Rc::new(vec![1.0, 2.0, 3.0])),
            Dim::one(),
            None,
        );
        let view = tensor
            .slice_step_view(&[(2, 2, -1)])
            .expect("reversed view");
        let source = Value::Tensor(Rc::new(view));
        let interp = Interp::new(&CheckedModule::default());
        let index = OnceCell::new();
        let portable = to_portable(&interp, &source).expect("portable view");
        let Some(Value::Tensor(result)) = from_portable(&interp, &index, portable) else {
            panic!("expected the reconstructed tensor");
        };
        assert_eq!(result.shape, [2]);
        assert!(matches!(result.storage, Storage::Dense(_)));
        assert_eq!(result.dense_flat().as_deref(), Some([3.0, 2.0].as_slice()));
        assert!(index.get().is_none());
    }
    #[test]
    fn interrupted_recursive_transfer_leaves_no_partially_rebuilt_value() {
        let mut elements = vec![Value::string("portable text"); 600];
        elements[0] = Value::int(17);
        elements[599] = Value::string("last");
        let value = Value::Tensor(Rc::new(TensorData::new(
            vec![600],
            Storage::Boxed(elements),
            Dim::one(),
            None,
        )));
        let mut interp = Interp::new(&CheckedModule::default());
        let token = crate::CancellationToken::new();
        interp.set_cancellation_token(token.clone());
        let index = OnceCell::new();
        let complete = to_portable(&interp, &value).unwrap();

        // Starting the recursive helpers after their root checkpoint models
        // cancellation arriving in the middle of a single large value.
        let mut copying = TransferCheckpoint::new(&interp);
        let mut restoring = TransferCheckpoint::new(&interp);
        assert!(!copying.cancelled());
        assert!(!restoring.cancelled());
        token.cancel();

        assert!(to_portable(&interp, &value).is_none());
        assert!(from_portable(&interp, &index, complete.clone()).is_none());
        assert!(
            to_portable_at(
                &interp.file_keys,
                &value,
                0,
                &mut TransferMetadata::default(),
                &mut copying,
            )
            .is_none()
        );
        let mut complete = complete;
        assert!(
            from_portable_at(
                &interp,
                &index,
                complete.value,
                &mut complete.metadata,
                &mut restoring,
            )
            .is_none()
        );
        assert_eq!(
            interp.check_cancelled().unwrap_err().code,
            crate::cancellation::INTERRUPTED
        );

        token.reset();
        let restored = from_portable(&interp, &index, to_portable(&interp, &value).unwrap());
        let Some(Value::Tensor(restored)) = restored else {
            panic!("expected a complete tensor after reset");
        };
        assert_eq!(restored.shape, [600]);
        assert!(matches!(restored.storage, Storage::Boxed(_)));
        assert_eq!(
            crate::value::show(&Value::Tensor(restored)),
            crate::value::show(&value)
        );
    }
}
