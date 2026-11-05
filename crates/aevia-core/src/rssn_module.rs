//! The `Rssn` module: the kernel's operators as ordinary Aevia functions.
//!
//! The member list is not written here. `Rssn.sin` resolves through the
//! Kernel Manifest at check time ([`aevia_types`] maps the signature to a
//! type) and at run time this module executes the overload the checker
//! pinned. Adding an operator to rssn makes `Rssn.<name>` work without a
//! change in this file.
//!
//! # The fixed members of §8.19
//!
//! Besides the operators, §8.19 declares a handful of *fixed* entry points:
//! `call`/`dynamic`, `typed`, `call_ho`, `call_key`, `register`/
//! `register_plugin`, `resolve_overload`, `overloads`, and the data members
//! (`Version`, `ManifestHash`, `Capabilities`, `Units`, `Constants`,
//! `ValueKinds`). They are dispatched below. Two of them need a value this
//! build cannot construct, and say so with `E1001` naming the capability
//! instead of failing deep inside the conversion:
//!
//! * `call_ho` takes `Tensor<1, Callback>`; the kernel's host-callback ABI is
//!   `Send + Sync` and this evaluator is single-threaded, so `core.callback` is
//!   off. The empty list *is* the dynamic call and runs.
//! * `register`/`register_plugin` take a `Plugin`; a kernel `Plugin` is a Rust
//!   host object and source-level `@op` packaging (§13.5.2) is not
//!   implemented, so `core.plugin.source` is off.
//!
//! # Two operator paths, one decision
//!
//! [`call_operator`] takes an explicit [`OpKey`] and never resolves anything:
//! that is the path PLAN D1 requires, and the one the checker pins. [`call`]'s
//! operator arm is unreachable from a checked call site — the evaluator uses
//! [`call_operator`] — so an operator that reaches it is `E1005` rather than a
//! re-resolution.

use crate::eval::EvalError;
use crate::kernel as bridge;
use crate::value::NativeFn;
use crate::value::Value;
use crate::value::show;
use aevia_dim::Dim;
use aevia_rssn::OpEntry;
use aevia_rssn::OpKey;
use aevia_rssn::SigType;
use aevia_rssn::Val;
use aevia_rssn::registry;
use aevia_syntax::Span;
use std::rc::Rc;

/// Calls `Rssn.<name>`.
///
/// # Errors
///
/// `E1003` when the member is neither a fixed entry point nor a registered
/// operator, plus whatever the kernel reports.
pub fn call(
    name: &str,
    args: Vec<Value>,
    span: Span,
) -> Result<Value, EvalError> {
    match name {
        | "Version" => Ok(Value::string(registry().version.clone())),
        | "ManifestHash" => Ok(Value::string(registry().manifest_hash.clone())),
        | "Capabilities" => Ok(string_tensor(registry().capabilities.names())),
        | "Units" => {
            Ok(string_tensor(
                registry()
                    .units
                    .iter()
                    .map(|unit| unit.name.clone())
                    .collect(),
            ))
        },
        | "Constants" => {
            Ok(string_tensor(
                registry()
                    .constants
                    .iter()
                    .map(|constant| constant.symbol.clone())
                    .collect(),
            ))
        },
        | "ValueKinds" => {
            Ok(string_tensor(
                registry()
                    .value_kinds
                    .iter()
                    .map(|(kind, _, _)| kind.clone())
                    .collect(),
            ))
        },
        | "call" | "dynamic" => dynamic_call(&args, span),
        | "typed" => typed_call(&args, span),
        | "call_ho" => call_ho(&args, span),
        | "register" | "register_plugin" => register(&args, span),
        | "call_key" => call_key(&args, span),
        | "invoke" => invoke(&args, span),
        | "resolve_overload" => resolve(&args, span),
        | "overloads" => overloads(&args, span),
        // An operator call reaches the evaluator through
        // `Interp::rssn_calls`, which carries the key the checker pinned
        // (PLAN D1). Reaching this arm means the call did not come from a
        // checked call site, and there is nothing to resolve *with*.
        | _ => {
            Err(EvalError::new(
                "E1005",
                format!("`Rssn.{name}` has no pinned overload here"),
            )
            .at(span)
            .with_note("an operator is not a first-class value (PLAN D1)"))
        },
    }
}

/// True when `name` is one of the fixed entry points rather than an operator.
#[must_use]
pub fn is_entry_point(name: &str) -> bool {
    matches!(
        name,
        "Version"
            | "ManifestHash"
            | "Capabilities"
            | "Units"
            | "Constants"
            | "ValueKinds"
            | "call"
            | "dynamic"
            | "typed"
            | "call_ho"
            | "register"
            | "register_plugin"
            | "call_key"
            | "invoke"
            | "resolve_overload"
            | "overloads"
    )
}

/// Calls exactly the overload `key` names, without resolving anything.
///
/// This is the dispatch entry point of PLAN D1: the key was chosen
/// statically, so the runtime must execute it and never look for a better
/// match.
///
/// # Errors
///
/// `E1003` when the key names no registered operator, `E0315` on a wrong
/// argument count, plus whatever the kernel reports.
pub fn call_operator(
    key: &OpKey,
    args: Vec<Value>,
    span: Span,
) -> Result<Value, EvalError> {
    let Some(entry) = registry().op(&key.name) else {
        return Err(
            EvalError::new("E1003", format!("rssn has no operator `{}`", key.name)).at(span),
        );
    };
    let converted = arguments(entry, &key.name, &args, span)?;
    execute(key, entry, converted, span)
}

/// Converts the arguments of a call, honouring the variadic ABI.
///
/// A variadic operator is typed `[α] -> R` and generated as
/// `concat([fixed…], rest)`, so the list the call site passes is *one*
/// argument statically and N positional arguments at the kernel. Passing the
/// list through as a single argument is what made `Rssn.and([true, false])`
/// answer a term where the checker promised a `Bool`.
fn arguments(
    entry: &OpEntry,
    name: &str,
    args: &[Value],
    span: Span,
) -> Result<Vec<Val>, EvalError> {
    if !entry.accepts(args.len()) {
        return Err(EvalError::new(
            "E0315",
            format!(
                "`{name}` takes {} argument(s), found {}",
                entry.arity.0,
                args.len()
            ),
        )
        .at(span));
    }
    let variadic = entry.is_variadic();
    let last = args.len().checked_sub(1);
    let mut converted = Vec::with_capacity(args.len());
    for (index, arg) in args.iter().enumerate() {
        // Only the trailing argument spreads, and only when it is a list: a
        // rank-1 tensor is the static `[α]`, anything else is a single value.
        if variadic && Some(index) == last {
            if let Some(items) = list_items(arg) {
                for item in &items {
                    converted.push(bridge::to_val(item)?);
                }
                continue;
            }
        }
        converted.push(bridge::to_val(arg)?);
    }
    Ok(converted)
}

/// The elements of a rank-1 tensor, when the value is one.
fn list_items(value: &Value) -> Option<Vec<Value>> {
    let Value::Tensor(tensor) = value else {
        return None;
    };
    if tensor.shape.len() != 1 {
        return None;
    }
    Some(
        (0..tensor.storage.len())
            .filter_map(|index| tensor.get_flat(index))
            .collect(),
    )
}

/// Executes a resolved overload on converted arguments.
fn execute(
    key: &OpKey,
    entry: &OpEntry,
    converted: Vec<Val>,
    span: Span,
) -> Result<Value, EvalError> {
    let value = aevia_rssn::kernel()
        .call_key(key, &converted)
        .map_err(|failure| bridge::error(&failure).at(span))?;
    let dim = result_dim(entry, key);
    Ok(bridge::from_val(&value, dim))
}

/// The dimension to attach to a result.
///
/// The dimension comes from the overload the key names, not from the first
/// one: when the two differ, reading `primary()` gives the result a dimension
/// the executed overload never promised.
fn result_dim(
    entry: &OpEntry,
    key: &OpKey,
) -> Dim {
    signature_dim(entry, entry.key_index(key))
}

/// The dimension a signature states, `1` when it states none.
fn signature_dim(
    entry: &OpEntry,
    index: Option<usize>,
) -> Dim {
    let signature = index
        .and_then(|index| entry.signatures.get(index))
        .or_else(|| entry.primary());
    match signature.map(|sig| &sig.ret) {
        | Some(SigType::Dim(_, dim) | SigType::Point(dim)) => dim.clone(),
        | _ => Dim::one(),
    }
}

/// The `OpKey` a `Rssn.<name>` call with these argument types denotes.
///
/// The checker calls this at a call site (PLAN D1, `W2.8`): the returned key
/// is pinned into the Core IR and handed to [`call_operator`], so the overload
/// that was typed is the overload that runs. Resolution is `kernel::overload`'s
/// lattice, not a second implementation of it.
///
/// # Errors
///
/// `E1002` when several overloads tie, `E1003` when none accepts the
/// arguments.
pub fn resolve_key(
    name: &str,
    arg_types: &[SigType],
) -> Result<OpKey, aevia_rssn::KernelError> {
    aevia_rssn::resolve_key(name, arg_types)
}

/// `Rssn.typed(name, values, rep)`.
///
/// §8.19 types it as `(String, Tensor<1, Value>, TypeRep) -> Result<a, OpError>`:
/// the dynamic call plus a declared result type. `type_rep` in this build
/// answers `{ name = "TypeRep", kind = "opaque" }` — it carries no structure a
/// runtime could check a value against — so the only thing `typed` can verify
/// is that the third argument is a `TypeRep` at all. The call itself is the
/// dynamic one, so a kernel failure still arrives as `Err` rather than as a
/// diagnostic (§8.19 makes the dynamic boundary value-level).
fn typed_call(
    args: &[Value],
    span: Span,
) -> Result<Value, EvalError> {
    let representation = args.get(2).ok_or_else(|| {
        EvalError::new(
            "E0315",
            "`Rssn.typed` needs a `TypeRep` as its third argument",
        )
        .at(span)
    })?;
    if !is_type_rep(representation) {
        return Err(EvalError::new(
            "E0302",
            format!(
                "`Rssn.typed` expects a `TypeRep`, found {}",
                representation.type_name()
            ),
        )
        .at(span));
    }
    dynamic_call(args, span)
}

/// Whether a value is the `TypeRep` that `type_rep()` produces.
fn is_type_rep(value: &Value) -> bool {
    let Value::Record(record) = value else {
        return false;
    };
    matches!(record.fields.get("name"), Some(Value::Str(name)) if name.as_ref() == "TypeRep")
}

/// `Rssn.dynamic(name, values)`.
///
/// The result is always a `Result`: §8.19 makes the dynamic entry point the one
/// place where a kernel failure is an ordinary value rather than a diagnostic,
/// so a caller can match on it. A *capability* gap is not such a failure: a
/// build without `core.dynamic_call` cannot run the entry point at all, and
/// says so as `E1001`.
fn dynamic_call(
    args: &[Value],
    span: Span,
) -> Result<Value, EvalError> {
    let name = args
        .first()
        .map(show)
        .ok_or_else(|| EvalError::new("E0315", "`Rssn.dynamic` needs a name").at(span))?;
    registry()
        .capabilities
        .require("core.dynamic_call")
        .map_err(|failure| bridge::error(&failure).at(span))?;
    let values = dynamic_arguments(args.get(1), span)?;
    forward(&name, &values, span, false)
}

/// `Rssn.call_ho(name, values, callbacks)`.
///
/// §8.19 types the last argument as `Tensor<1, Callback>`. No Aevia expression
/// can produce a `Callback`: the kernel's host-callback ABI is a
/// `Send + Sync` trait object and this evaluator is a single-threaded `Rc`
/// graph, so an Aevia closure cannot be converted into one. That is exactly the
/// missing `core.callback` capability, and a non-empty list says so as
/// `E1001`.
///
/// An *empty* list is the one case with a faithful meaning: the kernel's
/// `call_ho` with no callbacks is its dynamic call. It is forwarded there so
/// the entry point is not a dead name — and the callback-free result must equal
/// `Rssn.dynamic`'s, which the tests assert.
fn call_ho(
    args: &[Value],
    span: Span,
) -> Result<Value, EvalError> {
    let name = args
        .first()
        .map(show)
        .ok_or_else(|| EvalError::new("E0315", "`Rssn.call_ho` needs a name").at(span))?;
    if args.len() != 3 {
        return Err(EvalError::new(
            "E0315",
            format!("`Rssn.call_ho` takes 3 arguments, found {}", args.len()),
        )
        .at(span));
    }
    let callbacks = dynamic_arguments(args.get(2), span)?;
    if !callbacks.is_empty() {
        return Err(capability_gap(
            "core.callback",
            "an Aevia closure cannot cross to the kernel: its host-callback ABI is \
             `Send + Sync` and this evaluator is single-threaded, so no `Callback` \
             value is constructible from source",
            span,
        ));
    }
    registry()
        .capabilities
        .require("core.dynamic_call")
        .map_err(|failure| bridge::error(&failure).at(span))?;
    let values = dynamic_arguments(args.get(1), span)?;
    forward(&name, &values, span, true)
}

/// `Rssn.register(plugin)` and its alias `Rssn.register_plugin(plugin)`.
///
/// §8.19 types the argument as `Plugin`, the handle §13.5.1 registers with
/// `Session::register(Box<dyn Plugin>)`. No Aevia expression can produce one:
/// a kernel `Plugin` is a Rust host object registered through the embedding
/// API, and the source-level form (§13.5.2, `@op` packaging) is not
/// implemented. The runtime reports `E1001("core.plugin.source")` rather than
/// pretending to register something; `core.plugin` itself is *not* claimed
/// missing, because in-process host plugins exist in this build.
fn register(
    args: &[Value],
    span: Span,
) -> Result<Value, EvalError> {
    if args.len() != 1 {
        return Err(EvalError::new(
            "E0315",
            format!("`Rssn.register` takes 1 argument, found {}", args.len()),
        )
        .at(span));
    }
    Err(capability_gap(
        "core.plugin.source",
        "no Aevia source expression can construct a `Plugin` value: Rust host \
         plugins are registered through the embedding API (§13.5.1), and \
         source-level `@op` packaging (§13.5.2) is the missing feature",
        span,
    ))
}

/// The dynamic call behind `Rssn.dynamic` and a callback-free `Rssn.call_ho`.
///
/// The two differ only in which kernel entry point runs: with no callbacks,
/// `Session::call_ho` *is* `Session::call_value`. The conversion and the
/// `Result`-as-value channel are shared so the two cannot drift.
fn forward(
    name: &str,
    values: &[Value],
    _span: Span,
    higher_order: bool,
) -> Result<Value, EvalError> {
    let mut converted = Vec::with_capacity(values.len());
    for value in values {
        match bridge::to_val(value) {
            | Ok(value) => converted.push(value),
            | Err(error) => return Ok(err(Value::string(error.message))),
        }
    }
    let kernel = aevia_rssn::kernel();
    let dim = registry()
        .op(name)
        .map_or_else(Dim::one, |entry| signature_dim(entry, None));
    let result = if higher_order {
        kernel.call_ho(name, &converted)
    } else {
        kernel.call(name, &converted)
    };
    match result {
        | Ok(value) => Ok(ok(bridge::from_val(&value, dim))),
        // The error payload keeps its code: `Err(e)` with the whole structure
        // of `OpError` is not representable in Aevia, but a bare message would
        // make `Ambiguous` and `BudgetExceeded` read alike.
        | Err(failure) => {
            Ok(err(Value::string(format!(
                "{}: {}",
                failure.code, failure.message
            ))))
        },
    }
}

/// An `E1001` naming a capability this build does not provide.
///
/// The capability comes from the one table in [`aevia_rssn::Capabilities`], so
/// the message and `Rssn.Capabilities` cannot disagree; the debug assertion
/// catches the day a capability is turned on while the arm that reports it
/// missing is still there.
fn capability_gap(
    capability: &str,
    why: &str,
    span: Span,
) -> EvalError {
    debug_assert!(
        !registry().capabilities.has(capability),
        "`{capability}` is provided by this build, so an arm reporting it missing is stale"
    );
    let failure = aevia_rssn::KernelError::capability(capability);
    bridge::error(&failure).at(span).with_note(why)
}

/// `Rssn.call_key(name, hash, values)`.
fn call_key(
    args: &[Value],
    span: Span,
) -> Result<Value, EvalError> {
    let name = args
        .first()
        .map(show)
        .ok_or_else(|| EvalError::new("E0315", "`Rssn.call_key` needs a name").at(span))?;
    let hash = args.get(1).map(show).ok_or_else(|| {
        EvalError::new("E0315", "`Rssn.call_key` needs a signature hash").at(span)
    })?;
    let values = dynamic_arguments(args.get(2), span)?;
    let mut converted = Vec::with_capacity(values.len());
    for value in &values {
        converted.push(bridge::to_val(value)?);
    }
    let entry = registry().op(&name);
    // §10.0: `call_key` executes the key it is given. A key that no longer
    // names a live overload means the module was generated against a different
    // manifest, which is `E1005` — not a reason to pick another overload.
    let Some(key) = entry.and_then(|entry| pinned_key(entry, &hash)) else {
        return Ok(err(Value::string(format!(
            "E1005: no overload of `{name}` has signature {hash}; regenerate the Rssn module"
        ))));
    };
    let dim = entry.map_or_else(Dim::one, |entry| result_dim(entry, &key));
    let value = aevia_rssn::kernel()
        .call_key(&key, &converted)
        .map_err(|failure| bridge::error(&failure).at(span))?;
    Ok(ok(bridge::from_val(&value, dim)))
}

/// `Rssn.invoke(name, hash, values)`: the call behind a generated
/// declaration.
///
/// It differs from [`call_key`] only in the error channel: a generated `fn`
/// must return the declared type, so a kernel failure raises instead of
/// returning `Err`.
fn invoke(
    args: &[Value],
    span: Span,
) -> Result<Value, EvalError> {
    let name = args
        .first()
        .map(show)
        .ok_or_else(|| EvalError::new("E0315", "`Rssn.invoke` needs a name").at(span))?;
    let hash = args
        .get(1)
        .map(show)
        .ok_or_else(|| EvalError::new("E0315", "`Rssn.invoke` needs a signature hash").at(span))?;
    let values = dynamic_arguments(args.get(2), span)?;
    let mut converted = Vec::with_capacity(values.len());
    for value in &values {
        converted.push(bridge::to_val(value)?);
    }
    let entry = registry().op(&name);
    let Some(key) = entry.and_then(|entry| pinned_key(entry, &hash)) else {
        return Err(EvalError::new(
            "E1005",
            format!("no overload of `{name}` has signature {hash}"),
        )
        .at(span)
        .with_note("regenerate the Rssn module against the current manifest"));
    };
    let dim = entry.map_or_else(Dim::one, |entry| result_dim(entry, &key));
    let value = aevia_rssn::kernel()
        .call_key(&key, &converted)
        .map_err(|failure| bridge::error(&failure).at(span))?;
    Ok(bridge::from_val(&value, dim))
}

/// The key of the overload of `entry` whose signature hash is `hash`.
fn pinned_key(
    entry: &OpEntry,
    hash: &str,
) -> Option<OpKey> {
    entry
        .keys
        .iter()
        .find(|key| hex(&key.signature_hash) == hash)
        .cloned()
}

fn resolve(
    args: &[Value],
    span: Span,
) -> Result<Value, EvalError> {
    let name = args
        .first()
        .map(show)
        .ok_or_else(|| EvalError::new("E0315", "`Rssn.resolve_overload` needs a name").at(span))?;
    let values = dynamic_arguments(args.get(1), span)?;
    let mut converted = Vec::with_capacity(values.len());
    for value in &values {
        converted.push(bridge::to_val(value)?);
    }
    let kernel = aevia_rssn::kernel();
    match kernel.resolve(&name, &converted) {
        | Ok(key) => {
            Ok(ok(Value::string(format!(
                "{}.{}@{}",
                key.rule_set,
                key.name,
                hex(&key.signature_hash)
            ))))
        },
        | Err(failure) => Ok(err(Value::string(failure.message))),
    }
}

fn overloads(
    args: &[Value],
    span: Span,
) -> Result<Value, EvalError> {
    let name = args
        .first()
        .map(show)
        .ok_or_else(|| EvalError::new("E0315", "`Rssn.overloads` needs a name").at(span))?;
    let Some(entry) = registry().op(&name) else {
        return Ok(Value::Tensor(Rc::new(crate::value::TensorData {
            shape: vec![0],
            storage: crate::value::Storage::Boxed(Vec::new()),
            dim: Dim::one(),
            display: None,
        })));
    };
    let items: Vec<Value> = entry
        .signatures
        .iter()
        .map(|signature| Value::string(format!("{} :: {}", name, signature.render())))
        .collect();
    let len = items.len();
    Ok(Value::Tensor(Rc::new(crate::value::TensorData {
        shape: vec![len],
        storage: crate::value::Storage::Boxed(items),
        dim: Dim::one(),
        display: None,
    })))
}

/// The argument list of a dynamic call.
fn dynamic_arguments(
    value: Option<&Value>,
    span: Span,
) -> Result<Vec<Value>, EvalError> {
    match value {
        | Some(Value::Tensor(tensor)) => {
            Ok((0..tensor.storage.len())
                .filter_map(|index| tensor.get_flat(index))
                .collect())
        },
        | Some(Value::Unit) | None => Ok(Vec::new()),
        | Some(other) => {
            Err(EvalError::new(
                "E0302",
                format!("expected a list of values, found {}", other.type_name()),
            )
            .at(span))
        },
    }
}

fn string_tensor(items: Vec<String>) -> Value {
    let len = items.len();
    Value::Tensor(Rc::new(crate::value::TensorData {
        shape: vec![len],
        storage: crate::value::Storage::Boxed(items.into_iter().map(Value::string).collect()),
        dim: Dim::one(),
        display: None,
    }))
}

fn hex(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        out.push_str(&format!("{byte:02x}"));
    }
    out
}

fn ok(value: Value) -> Value {
    crate::value::make_variant(aevia_types::adt::RESULT_ID, 0, "Ok", vec![value])
}

fn err(value: Value) -> Value {
    crate::value::make_variant(aevia_types::adt::RESULT_ID, 1, "Err", vec![value])
}

/// The members that are data rather than operators.
const DATA_MEMBERS: [&str; 6] = [
    "Version",
    "ManifestHash",
    "Capabilities",
    "Units",
    "Constants",
    "ValueKinds",
];

/// Whether `name` is data rather than a callable.
#[must_use]
pub fn is_data_member(name: &str) -> bool {
    DATA_MEMBERS.contains(&name)
}

/// A `Rssn.<name>` value.
///
/// Data members evaluate now; everything else becomes a callable, so
/// `Rssn.sin` can be passed to `map` without being applied.
#[must_use]
pub fn member(name: &str) -> Value {
    if is_data_member(name) {
        return call(name, Vec::new(), Span::default()).unwrap_or(Value::Error);
    }
    let owned = name.to_string();
    Value::Native(Rc::new(NativeFn(Box::new(move |_interp, args, span| {
        call(&owned, args, span)
    }))))
}
