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
//! strings, tensors, records, variants, constructors and IR closures).
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

use crate::Interp;
use crate::eval::EvalError;
use crate::value::Closure;
use crate::value::ClosureBody;
use crate::value::ClosureEnv;
use crate::value::IrEnv;
use crate::value::NumKind;
use crate::value::NumVal;
use crate::value::Scope;
use crate::value::Storage;
use crate::value::Value;
use aevia_dim::Dim;
use aevia_dim::UnitDef;
use aevia_syntax::FileId;
use aevia_syntax::Span;
use num_bigint::BigInt;
use num_rational::BigRational;
use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;
use std::sync::Arc;
use std::sync::atomic::AtomicUsize;
use std::sync::atomic::Ordering;

/// Builds a fresh interpreter for the running program on a worker thread.
///
/// The host that loaded the program installs one ([`Interp::set_replica`]):
/// the CLI reloads the entry file, `aevia_core::run` recompiles its source.
/// It must produce an interpreter whose modules were loaded exactly as the
/// main one's were (same files, same order), because closures cross by
/// source span.
pub type Replica = Arc<dyn Fn() -> Result<Interp, String> + Send + Sync>;

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
        elements: Vec<Portable>,
        /// The element dimension.
        dim: Dim,
    },
    /// A record, in field order.
    Record(Vec<(String, Portable)>),
    /// A constructed variant.
    Variant {
        /// Its type.
        adt: u32,
        /// The constructor index.
        index: usize,
        /// The constructor name.
        name: String,
        /// The payload.
        fields: Vec<Portable>,
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
        args: Vec<Portable>,
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
        captures: Vec<Portable>,
    },
}

const MAX_DEPTH: usize = 256;

fn unit_def(display: Option<&Rc<UnitDef>>) -> Option<UnitDef> {
    display.map(|unit| (**unit).clone())
}

/// The portable form of `value`, or `None` when it cannot cross a thread.
#[must_use]
pub(crate) fn to_portable(
    keys: &FileKeys,
    value: &Value,
) -> Option<Portable> {
    to_portable_at(keys, value, 0)
}

fn to_portable_at(
    keys: &FileKeys,
    value: &Value,
    depth: usize,
) -> Option<Portable> {
    if depth > MAX_DEPTH {
        return None;
    }
    let next = depth + 1;
    Some(match value {
        | Value::Unit => Portable::Unit,
        | Value::Bool(value) => Portable::Bool(*value),
        | Value::Num(num) => {
            let payload = match &num.kind {
                | NumKind::Int(value) => PNum::Int(value.clone()),
                | NumKind::Exact(value) => PNum::Exact(value.clone()),
                | NumKind::Float(value) => PNum::Float(*value),
                | NumKind::Complex(re, im) => PNum::Complex(*re, *im),
                // A symbolic term may hold a kernel handle owned by this
                // thread's session.
                | NumKind::Sym(_) => return None,
            };
            Portable::Num(payload, num.dim.clone(), unit_def(num.display.as_ref()))
        },
        | Value::Str(text) => Portable::Str(text.to_string()),
        | Value::Tensor(tensor) => {
            if tensor.verified_element.is_some() {
                // A schema witness (a tensor of records with a checked
                // element type) is evidence this side cannot recreate.
                return None;
            }
            if let (Storage::Dense { .. }, Some(data)) = (&tensor.storage, tensor.dense_flat()) {
                Portable::Dense {
                    shape: tensor.shape.clone(),
                    data: data.into_owned(),
                    dim: tensor.dim.clone(),
                    display: unit_def(tensor.display.as_ref()),
                }
            } else {
                if tensor.display.is_some() {
                    return None;
                }
                let mut elements = Vec::with_capacity(tensor.storage.len());
                for element in tensor.elements() {
                    elements.push(to_portable_at(keys, &element, next)?);
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
                fields.push((name.clone(), to_portable_at(keys, field, next)?));
            }
            Portable::Record(fields)
        },
        | Value::Variant(variant) => {
            if variant.type_args.is_some() || variant.verified_type.is_some() {
                return None;
            }
            let mut fields = Vec::with_capacity(variant.fields.len());
            for field in &variant.fields {
                fields.push(to_portable_at(keys, field, next)?);
            }
            Portable::Variant {
                adt: variant.adt,
                index: variant.index,
                name: variant.name.clone(),
                fields,
            }
        },
        | Value::Ctor {
            adt,
            index,
            name,
            arity,
            args,
            ..
        } => {
            let mut collected = Vec::with_capacity(args.len());
            for arg in args {
                collected.push(to_portable_at(keys, arg, next)?);
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
            let mut captures = Vec::new();
            for slot in env.slots.borrow().iter() {
                captures.push(to_portable_at(keys, slot, next)?);
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
pub(crate) struct ReplicaIndex {
    functions: HashMap<Span, Rc<aevia_ir::Function>>,
    files: HashMap<Arc<str>, FileId>,
}

impl ReplicaIndex {
    fn build(interp: &Interp) -> Self {
        let mut functions = HashMap::new();
        let mut pending: Vec<Rc<aevia_ir::Function>> = Vec::new();
        let mut seen_values = 0usize;
        let visit = |value: &Value, pending: &mut Vec<Rc<aevia_ir::Function>>| {
            if let Value::Closure(closure) = value {
                if let ClosureBody::Ir(function) = &closure.body {
                    pending.push(function.clone());
                }
            }
        };
        for value in interp.globals.values() {
            visit(value, &mut pending);
            seen_values += 1;
        }
        for scope in interp.file_scopes.values() {
            for value in scope.vars.borrow().values() {
                visit(value, &mut pending);
            }
        }
        for template in interp.context.instances.borrow().iter() {
            if let crate::evidence::MethodCode::Ir(function) = &*template.code {
                pending.push(function.clone());
            }
        }
        let _ = seen_values;
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
            | Rhs::Closure(function) => out.push(function.clone()),
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

/// Rebuilds a value on the replica.
fn from_portable(
    interp: &Interp,
    index: &ReplicaIndex,
    value: Portable,
) -> Option<Value> {
    Some(match value {
        | Portable::Unit => Value::Unit,
        | Portable::Bool(value) => Value::Bool(value),
        | Portable::Num(payload, dim, display) => {
            Value::Num(NumVal {
                kind: match payload {
                    | PNum::Int(value) => NumKind::Int(value),
                    | PNum::Exact(value) => NumKind::Exact(value),
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
            let elements = data
                .into_iter()
                .map(|value| Value::Num(NumVal::float(value, dim.clone())))
                .collect();
            let tensor = crate::eval::build_tensor(elements, shape, dim);
            match (tensor, display) {
                | (Value::Tensor(tensor), Some(display)) => {
                    let mut tensor = (*tensor).clone();
                    tensor.display = Some(Rc::new(display));
                    Value::Tensor(Rc::new(tensor))
                },
                | (tensor, _) => tensor,
            }
        },
        | Portable::Boxed { shape, elements, dim } => {
            let mut values = Vec::with_capacity(elements.len());
            for element in elements {
                values.push(from_portable(interp, index, element)?);
            }
            crate::eval::build_tensor(values, shape, dim)
        },
        | Portable::Record(fields) => {
            let mut collected = indexmap::IndexMap::new();
            for (name, field) in fields {
                collected.insert(name, from_portable(interp, index, field)?);
            }
            Value::Record(Rc::new(crate::value::RecordValue { fields: collected }))
        },
        | Portable::Variant {
            adt,
            index: ctor,
            name,
            fields,
        } => {
            let mut values = Vec::with_capacity(fields.len());
            for field in fields {
                values.push(from_portable(interp, index, field)?);
            }
            crate::value::make_variant(adt, ctor, &name, values)
        },
        | Portable::Ctor {
            adt,
            index: ctor,
            name,
            arity,
            args,
        } => {
            let mut values = Vec::with_capacity(args.len());
            for arg in args {
                values.push(from_portable(interp, index, arg)?);
            }
            Value::Ctor {
                adt,
                index: ctor,
                name,
                arity,
                args: values,
                origin: None,
            }
        },
        | Portable::Closure {
            module,
            start,
            end,
            captures,
        } => {
            let Some(file) = index.files.get(&*module).copied() else {
                return fallback(&format!(
                    "the replica has no module `{module}` (it has {:?})",
                    index.files.keys().collect::<Vec<_>>()
                ));
            };
            let span = Span { file, start, end };
            let Some(function) = index.functions.get(&span).cloned() else {
                return fallback(&format!(
                    "the replica has no function at {start}..{end} of `{module}` ({} indexed)",
                    index.functions.len()
                ));
            };
            let scope = interp.file_scopes.get(&span.file)?.clone();
            let mut slots = Vec::with_capacity(captures.len());
            for capture in captures {
                slots.push(from_portable(interp, index, capture)?);
            }
            if slots.len() != function.captures.len() {
                return None;
            }
            Value::Closure(Rc::new(Closure {
                canonical_origin: std::rc::Weak::new(),
                params: Vec::new(),
                body: ClosureBody::Ir(function.clone()),
                env: ClosureEnv::Ir(Rc::new(IrEnv {
                    slots: RefCell::new(slots),
                    scope: Some(scope.clone()),
                })),
                name: Some(function.name.clone()),
                proof: scope.context.as_ref().and_then(|context| {
                    crate::generics::FunctionProof::at(context, function.span, scope.types.as_ref())
                }),
                captured_bindings: scope.types.clone(),
            }))
        },
    })
}

// ---------------------------------------------------------------------------
// Workers
// ---------------------------------------------------------------------------

struct Worker {
    recipe: usize,
    interp: Interp,
    index: ReplicaIndex,
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

fn recipe_id(replica: &Replica) -> usize {
    Arc::as_ptr(replica).cast::<()>() as usize
}

/// Runs `job` on this thread's replica of `replica`, building it first if
/// the thread has none (or one of another program).
fn with_worker<T>(
    replica: &Replica,
    job: impl FnOnce(&mut Interp, &ReplicaIndex) -> Option<T>,
) -> Option<T> {
    let id = recipe_id(replica);
    let idle = WORKERS.with(|cell| {
        let mut workers = cell.borrow_mut();
        // A replica of another program is useless now; drop it.
        workers.retain(|worker| worker.recipe == id);
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
            let index = ReplicaIndex::build(&interp);
            REPLICAS.fetch_add(1, Ordering::Relaxed);
            Worker {
                recipe: id,
                interp,
                index,
            }
        },
    };
    DISPATCHED.fetch_add(1, Ordering::Relaxed);
    let result = job(&mut worker.interp, &worker.index);
    WORKERS.with(|cell| cell.borrow_mut().push(worker));
    result
}

/// The process-wide pool, sized by the first caller's `jobs`.
fn pool(jobs: usize) -> Option<&'static rayon::ThreadPool> {
    static POOL: std::sync::OnceLock<Option<rayon::ThreadPool>> = std::sync::OnceLock::new();
    POOL.get_or_init(|| {
        rayon::ThreadPoolBuilder::new()
            .num_threads(jobs.max(1))
            // The interpreter is recursive; a worker needs the same headroom
            // the CLI gives the main interpreter thread.
            .stack_size(256 * 1024 * 1024)
            .thread_name(|index| format!("aevia-worker-{index}"))
            .build()
            .ok()
    })
    .as_ref()
}

/// The number of worker threads to use when the host does not say.
#[must_use]
pub fn default_jobs() -> usize {
    std::env::var("AEVIA_JOBS")
        .ok()
        .and_then(|text| text.parse().ok())
        .unwrap_or_else(|| std::thread::available_parallelism().map_or(1, usize::from))
}

/// Inputs below this size are mapped sequentially: the replica round trip
/// costs more than it saves.
const MIN_PARALLEL: usize = 32;

fn chunks(len: usize) -> Vec<std::ops::Range<usize>> {
    // Chunk boundaries depend only on the length, never on the thread count.
    let size = len.div_ceil(64).max(1);
    (0..len)
        .step_by(size)
        .map(|start| start..(start + size).min(len))
        .collect()
}

impl Interp {
    /// Installs the factory workers build their replicas with (see
    /// [`Replica`]). Without one every parallel primitive is sequential.
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

    /// `par_map` on worker threads: `Some` with the mapped elements in input
    /// order, or `None` to map sequentially (see the module documentation).
    pub(crate) fn par_map_values(
        &mut self,
        function: &Value,
        elements: &[Value],
    ) -> Option<Vec<Value>> {
        if elements.len() < MIN_PARALLEL {
            return None;
        }
        let replica = self
            .parallel_replica()
            .or_else(|| fallback("no replica factory, or --jobs 1"))?;
        let function = to_portable(&self.file_keys, function)
            .or_else(|| fallback("the function cannot cross threads"))?;
        let mut inputs = Vec::with_capacity(elements.len());
        for element in elements {
            inputs.push(
                to_portable(&self.file_keys, element)
                    .or_else(|| fallback("an element cannot cross"))?,
            );
        }
        let pool = pool(self.jobs).or_else(|| fallback("the worker pool did not start"))?;
        let ranges = chunks(inputs.len());
        let results: Option<Vec<Vec<Portable>>> = pool.install(|| {
            use rayon::prelude::*;
            ranges
                .par_iter()
                .map(|range| {
                    with_worker(&replica, |interp, index| {
                        let function = from_portable(interp, index, function.clone())
                            .or_else(|| fallback("the replica has no such function"))?;
                        let mut out = Vec::with_capacity(range.len());
                        for input in &inputs[range.clone()] {
                            let value = from_portable(interp, index, input.clone())?;
                            let result = match interp.apply(
                                function.clone(),
                                vec![value],
                                Span::default(),
                            ) {
                                | Ok(result) => result,
                                | Err(error) => {
                                    return fallback(&format!(
                                        "a worker failed ({}: {}); rerunning sequentially",
                                        error.code, error.message
                                    ));
                                },
                            };
                            out.push(
                                to_portable(&interp.file_keys, &result)
                                    .or_else(|| fallback("a result cannot cross back"))?,
                            );
                        }
                        Some(out)
                    })
                    .or_else(|| fallback("a worker could not run"))
                })
                .collect()
        });
        let results = results?;
        let index = ReplicaIndex::build(self);
        let mut values = Vec::with_capacity(elements.len());
        for portable in results.into_iter().flatten() {
            values.push(from_portable(self, &index, portable)?);
        }
        Some(values)
    }

    /// Rebuilds a worker's result on this interpreter: a closure returned
    /// by a worker is rebuilt from this side's own function index.
    fn local(
        &self,
        value: Portable,
    ) -> Option<Value> {
        let index = ReplicaIndex::build(self);
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
        elements: &[Value],
        span: Span,
    ) -> Result<Value, EvalError> {
        let blocks = reduce_blocks(elements.len());
        let partial = self
            .parallel_blocks(function, zero, elements, &blocks)
            .map_or_else(
                || {
                    blocks
                        .iter()
                        .map(|block| {
                            let mut acc = zero.clone();
                            for element in &elements[block.clone()] {
                                acc =
                                    self.apply(function.clone(), vec![acc, element.clone()], span)?;
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
        &mut self,
        function: &Value,
        zero: &Value,
        elements: &[Value],
        blocks: &[std::ops::Range<usize>],
    ) -> Option<Vec<Value>> {
        if blocks.len() < 2 {
            return None;
        }
        let replica = self.parallel_replica()?;
        let function = to_portable(&self.file_keys, function)?;
        let zero = to_portable(&self.file_keys, zero)?;
        let mut inputs = Vec::with_capacity(elements.len());
        for element in elements {
            inputs.push(to_portable(&self.file_keys, element)?);
        }
        let pool = pool(self.jobs)?;
        let results: Option<Vec<Portable>> = pool.install(|| {
            use rayon::prelude::*;
            blocks
                .par_iter()
                .map(|block| {
                    with_worker(&replica, |interp, index| {
                        let function = from_portable(interp, index, function.clone())?;
                        let mut acc = from_portable(interp, index, zero.clone())?;
                        for input in &inputs[block.clone()] {
                            let value = from_portable(interp, index, input.clone())?;
                            acc = interp
                                .apply(function.clone(), vec![acc, value], Span::default())
                                .ok()?;
                        }
                        to_portable(&interp.file_keys, &acc)
                    })
                })
                .collect()
        });
        let index = ReplicaIndex::build(self);
        results?
            .into_iter()
            .map(|value| from_portable(self, &index, value))
            .collect()
    }

    // ----- tasks ------------------------------------------------------------

    /// Starts the IO action `thunk()` as a task (§9.7.3).
    pub(crate) fn spawn_task(
        &mut self,
        thunk: &Value,
        span: Span,
    ) -> Result<Value, EvalError> {
        let replica = self.parallel_replica().filter(|_| !self.on_worker);
        if let (Some(replica), Some(portable)) = (replica, to_portable(&self.file_keys, thunk)) {
            if let Some(pool) = pool(self.jobs) {
                let (sender, receiver) = std::sync::mpsc::channel();
                pool.spawn(move || {
                    let outcome = with_worker(&replica, |interp, index| {
                        let thunk = from_portable(interp, index, portable)?;
                        Some(run_task(interp, thunk))
                    })
                    .unwrap_or_else(|| {
                        TaskOutcome::Failed {
                            code: "E0811",
                            message: "the task could not start on a worker".into(),
                            output: String::new(),
                        }
                    });
                    let _ = sender.send(outcome);
                });
                return Ok(Value::Task(Rc::new(TaskHandle {
                    state: RefCell::new(TaskState::Running(receiver)),
                })));
            }
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
        Ok(Value::Task(Rc::new(TaskHandle {
            state: RefCell::new(TaskState::Local(result, output)),
        })))
    }

    /// Waits for a task, writes its buffered output, and returns its value.
    pub(crate) fn await_task(
        &mut self,
        task: &TaskHandle,
        span: Span,
    ) -> Result<Value, EvalError> {
        let state = std::mem::replace(&mut *task.state.borrow_mut(), TaskState::Awaited);
        let (result, output) = match state {
            | TaskState::Local(result, output) => (result, output),
            | TaskState::Running(receiver) => {
                match receiver.recv() {
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

/// Block boundaries for `par_reduce`, a function of the length only.
fn reduce_blocks(len: usize) -> Vec<std::ops::Range<usize>> {
    const BLOCK: usize = 1024;
    (0..len)
        .step_by(BLOCK)
        .map(|start| start..(start + BLOCK).min(len))
        .collect()
}

fn run_task(
    interp: &mut Interp,
    thunk: Value,
) -> TaskOutcome {
    if let crate::eval::Output::Buffer(buffer) = &interp.output {
        buffer.borrow_mut().clear();
    }
    let result = interp
        .apply(thunk, Vec::new(), Span::default())
        .and_then(|action| interp.run_io_value(&action, Span::default()));
    let output = match &interp.output {
        | crate::eval::Output::Buffer(buffer) => std::mem::take(&mut *buffer.borrow_mut()),
        | crate::eval::Output::Console => String::new(),
    };
    match result {
        | Ok(value) => {
            match to_portable(&interp.file_keys, &value) {
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
        value: Portable,
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
