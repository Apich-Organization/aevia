//! Source-level plugins (§13.5.2): `@op` and `@rule` functions published to
//! the kernel.
//!
//! # How a function becomes an operator
//!
//! The kernel's extension language is its term language: an operator is a
//! definition `name(a, b) := body`, a rule is a rewrite `name: lhs => rhs`.
//! An Aevia function is turned into one by **tracing**: it is applied to fresh
//! symbols, each with its parameter's checked dimension, and the term it
//! builds is the body. This needs no second compiler and inherits the
//! kernel's whole machinery — once `mydomain/softplus` is a definition the
//! kernel can call it, differentiate it, simplify and integrate through it,
//! and evaluate it numerically, which an opaque host callback could not offer.
//!
//! The price is checked rather than hidden: a function whose result depends
//! on its arguments through control flow (`if x > 0`, a `match` on a value),
//! or that does IO, does not trace to a term, and registering it is `E1006`
//! with the reason. A rule function returns the pair `(lhs, rhs)`; its
//! parameters are the rule's pattern variables.
//!
//! One plugin is registered per loaded module, named `aevia/<module>#n`, in
//! the current thread's kernel session.

use crate::Interp;
use crate::value::NumVal;
use crate::value::Scope;
use crate::value::Value;
use aevia_syntax::Diagnostic;
use aevia_syntax::Diagnostics;
use aevia_types::check::HostRegistration;
use aevia_types::check::HostRegistrationKind;
use std::rc::Rc;

/// The kernel text of a traced value.
fn kernel_text(value: &Value) -> Result<String, String> {
    let term = crate::builtins::value_to_term(value).map_err(|error| error.message)?;
    match crate::kernel::term_val(&term).map_err(|error| error.message)? {
        | aevia_rssn::Val::Term(handle) => {
            aevia_rssn::kernel()
                .try_text(handle)
                .map_err(|error| error.to_string())
        },
        | other => Ok(format!("{other}")),
    }
}

fn parameter(index: usize) -> String {
    format!("aevia_arg{index}")
}

/// Traces one registration to its kernel text.
fn trace(
    interp: &mut Interp,
    registration: &HostRegistration,
    function: Value,
) -> Result<String, String> {
    let args: Vec<Value> = registration
        .params
        .iter()
        .enumerate()
        .map(|(index, (_, dim))| {
            Value::Num(NumVal::symbolic(
                crate::sym::Term::Sym(parameter(index)),
                dim.clone(),
            ))
        })
        .collect();
    let result = interp
        .apply(function, args, registration.span)
        .map_err(|error| {
            format!(
                "the function does not trace to a term ({}: {}); an `@{}` body must build an \
                 expression from its parameters without branching on them or doing IO",
                error.code,
                error.message,
                kind_name(registration.kind)
            )
        })?;
    let names: Vec<String> = (0..registration.params.len()).map(parameter).collect();
    match registration.kind {
        | HostRegistrationKind::Op => {
            let body = kernel_text(&result)?;
            Ok(format!(
                "{}({}) := {body}",
                registration.name,
                names.join(", ")
            ))
        },
        | HostRegistrationKind::Rule => {
            let Value::Record(pair) = &result else {
                return Err(format!(
                    "a rule function returns the pair `(lhs, rhs)`, found {}",
                    result.type_name()
                ));
            };
            let (Some(lhs), Some(rhs), 2) = (
                pair.fields.get("0"),
                pair.fields.get("1"),
                pair.fields.len(),
            ) else {
                return Err("a rule function returns the pair `(lhs, rhs)`".to_string());
            };
            let mark = |text: String| {
                let mut out = text;
                // Longest names first: `aevia_arg10` before `aevia_arg1`.
                for name in names.iter().rev() {
                    out = out.replace(name.as_str(), &format!("?{name}"));
                }
                out
            };
            let lhs = mark(kernel_text(lhs)?);
            let rhs = mark(kernel_text(rhs)?);
            Ok(format!("{}: {lhs} => {rhs}", registration.name))
        },
    }
}

const fn kind_name(kind: HostRegistrationKind) -> &'static str {
    match kind {
        | HostRegistrationKind::Op => "op",
        | HostRegistrationKind::Rule => "rule",
    }
}

thread_local! {
    static PLUGINS: std::cell::Cell<u32> = const { std::cell::Cell::new(0) };
}

impl Interp {
    /// Publishes the `@op`/`@rule` registrations of the module being loaded
    /// (`Interp::registrations`), whose functions `root` binds.
    pub(crate) fn publish_module_registrations(
        &mut self,
        module: &aevia_syntax::ast::Module,
        root: &Rc<Scope>,
    ) -> Diagnostics {
        let registrations: Vec<HostRegistration> = self
            .registrations
            .iter()
            .filter(|registration| registration.span.file == module.file)
            .cloned()
            .collect();
        let name = module
            .name
            .as_ref()
            .map(|path| path.join("."))
            .unwrap_or_default();
        self.publish_registrations_in(&registrations, root, &name)
    }

    /// Publishes a loaded module's `@op`/`@rule` functions to the kernel.
    ///
    /// Runs after the module's declarations are bound, so every function is a
    /// value. Each failure is an `E1006` at its attribute; the rest of the
    /// module's registrations still go through.
    ///
    /// The order is what makes source operators first-class:
    ///
    /// 1. every `@op` function is traced into a kernel definition, with its
    ///    checked parameter and result dimensions as the operator's
    ///    dimension signature;
    /// 2. each traced function's binding is replaced by a wrapper
    ///    ([`operator_wrapper`]): on numeric arguments it runs the Aevia body,
    ///    on symbolic ones it builds the operator node `name(args…)`, so the
    ///    symbolic layer and the kernel see `demo/softplus(x)` rather than its
    ///    expansion;
    /// 3. the `@rule` functions are traced *after* that, so a rule can be
    ///    stated about a source operator (`softplus(-x) => softplus(x) - x`);
    /// 4. one plugin carries the definitions, signatures and rewrites.
    ///
    /// If the kernel refuses the plugin, the original bindings are restored.
    pub fn publish_registrations_in(
        &mut self,
        registrations: &[HostRegistration],
        scope: &Rc<Scope>,
        module: &str,
    ) -> Diagnostics {
        let mut diagnostics = Diagnostics::new();
        if registrations.is_empty() {
            return diagnostics;
        }
        let mut definitions = Vec::new();
        let mut signatures = Vec::new();
        let mut rewrites = Vec::new();
        let mut published = Vec::new();
        let mut replaced: Vec<(String, Value)> = Vec::new();
        let fail =
            |diagnostics: &mut Diagnostics, registration: &HostRegistration, reason: String| {
                diagnostics.push(Diagnostic::error(
                    "E1006",
                    format!(
                        "`@{}(\"{}\")` cannot be published: {reason}",
                        kind_name(registration.kind),
                        registration.name
                    ),
                    registration.span,
                ));
            };
        let ops = registrations
            .iter()
            .filter(|registration| registration.kind == HostRegistrationKind::Op);
        let rules = registrations
            .iter()
            .filter(|registration| registration.kind == HostRegistrationKind::Rule);
        for registration in ops {
            let Some(function) = scope.lookup(&registration.function) else {
                fail(
                    &mut diagnostics,
                    registration,
                    format!("`{}` is not bound", registration.function),
                );
                continue;
            };
            match trace(self, registration, function.clone()) {
                | Ok(text) => {
                    definitions.push(text);
                    signatures.push(aevia_rssn::SourceOpSignature {
                        name: registration.name.clone(),
                        params: registration
                            .params
                            .iter()
                            .map(|(_, dim)| aevia_rssn::registry::to_dim_expr(dim))
                            .collect(),
                        result: aevia_rssn::registry::to_dim_expr(&registration.result),
                    });
                    let wrapper = operator_wrapper(registration, function.clone());
                    scope
                        .vars
                        .borrow_mut()
                        .insert(registration.function.clone(), wrapper);
                    replaced.push((registration.function.clone(), function));
                    published.push(registration);
                },
                | Err(reason) => fail(&mut diagnostics, registration, reason),
            }
        }
        // The operators go to the kernel before any rule is traced: a rule
        // about `softplus` builds the node `demo/softplus(..)`, which the
        // kernel must already know.
        let serial = PLUGINS.with(|count| {
            let next = count.get() + 1;
            count.set(next);
            next
        });
        let name = if module.is_empty() {
            format!("aevia/main#{serial}")
        } else {
            format!("aevia/{module}#{serial}")
        };
        let refuse =
            |diagnostics: &mut Diagnostics, published: &[&HostRegistration], reason: &str| {
                for registration in published {
                    diagnostics.push(Diagnostic::error(
                        "E1006",
                        format!(
                            "the kernel refused `@{}(\"{}\")`: {reason}",
                            kind_name(registration.kind),
                            registration.name
                        ),
                        registration.span,
                    ));
                }
            };
        if !definitions.is_empty()
            && let Err(reason) = aevia_rssn::kernel().register_source_plugin_with(
                &name,
                "0.1.0",
                definitions,
                Vec::new(),
                signatures,
            )
        {
            // The kernel has no such operators: calls must keep running the
            // Aevia bodies rather than build nodes nobody can reduce.
            for (function, original) in replaced {
                scope.vars.borrow_mut().insert(function, original);
            }
            refuse(&mut diagnostics, &published, &reason);
            published.clear();
        }
        let mut rule_registrations = Vec::new();
        for registration in rules {
            let Some(function) = scope.lookup(&registration.function) else {
                fail(
                    &mut diagnostics,
                    registration,
                    format!("`{}` is not bound", registration.function),
                );
                continue;
            };
            match trace(self, registration, function) {
                | Ok(text) => {
                    rewrites.push(text);
                    rule_registrations.push(registration);
                },
                | Err(reason) => fail(&mut diagnostics, registration, reason),
            }
        }
        if !rewrites.is_empty()
            && let Err(reason) = aevia_rssn::kernel().register_source_plugin_with(
                &format!("{name}/rules"),
                "0.1.0",
                Vec::new(),
                rewrites,
                Vec::new(),
            )
        {
            refuse(&mut diagnostics, &rule_registrations, &reason);
        }
        diagnostics
    }
}

/// Whether an argument is symbolic: a term with free symbols. A closed term
/// (`pi`, `sqrt(2)`) is a number for this purpose; the body evaluates it.
fn is_symbolic(value: &Value) -> bool {
    match value {
        | Value::Num(num) => {
            match &num.kind {
                | crate::value::NumKind::Sym(term) => !term.free_symbols().is_empty(),
                | _ => false,
            }
        },
        | _ => false,
    }
}

/// The binding a published `@op` function is replaced with (§13.5.2).
///
/// * All arguments numeric (or closed terms): the original Aevia function
///   runs. It is equal to what the kernel would compute from the definition
///   and avoids a kernel round trip per call.
/// * Some argument symbolic: the result is the operator node
///   `name(args…)` (a kernel term, carried by handle), with the dimension
///   the checked signature gives for the arguments' dimensions. Kernel
///   requests on it — `diff`, `simplify`, `integrate`, numeric evaluation —
///   see the operator, its definition and every `@rule` about it.
fn operator_wrapper(
    registration: &HostRegistration,
    original: Value,
) -> Value {
    let name = registration.name.clone();
    let params: Vec<aevia_dim::Dim> = registration
        .params
        .iter()
        .map(|(_, dim)| dim.clone())
        .collect();
    let result = registration.result.clone();
    Value::Native(Rc::new(crate::value::NativeFn(Box::new(
        move |interp: &mut Interp, args: Vec<Value>, span| {
            if !args.iter().any(is_symbolic) {
                return interp.apply(original.clone(), args, span);
            }
            let mut unifier = aevia_dim::DimUnifier::new();
            let mut kernel_args = Vec::with_capacity(args.len());
            for (index, arg) in args.iter().enumerate() {
                if let (Value::Num(num), Some(expected)) = (arg, params.get(index))
                    && unifier.unify(expected, &num.dim).is_err()
                {
                    return Err(crate::eval::EvalError::new(
                        "E0401",
                        format!(
                            "argument {} of `{name}` has dimension {}, the operator expects \
                             {expected}",
                            index + 1,
                            num.dim
                        ),
                    )
                    .at(span));
                }
                let term = crate::builtins::value_to_term(arg)?;
                kernel_args.push(crate::kernel::term_val(&term)?);
            }
            let node = aevia_rssn::kernel()
                .apply_node(&name, &kernel_args)
                .map_err(|failure| crate::kernel::error(&failure).at(span))?;
            let term = crate::kernel::term_of(&node).ok_or_else(|| {
                crate::eval::EvalError::new(
                    "E1003",
                    format!("the kernel built no term for `{name}`"),
                )
                .at(span)
            })?;
            Ok(Value::Num(NumVal::symbolic(term, unifier.resolve(&result))))
        },
    ))))
}

/// `load_plugin(path)`: loads, checks and evaluates the module at `path`
/// (with its imports) in a fresh interpreter on this thread, which publishes
/// its registrations to this thread's kernel session. `Ok(n)` counts the
/// published `@op`/`@rule` functions; `Err` carries the diagnostics.
pub(crate) fn load_plugin(path: &std::path::Path) -> Value {
    let failure = |message: String| {
        crate::value::make_variant(
            aevia_types::adt::RESULT_ID,
            1,
            "Err",
            vec![Value::string(message)],
        )
    };
    let program = crate::driver::load(path);
    if program.diagnostics.has_errors() {
        let messages: Vec<String> = program
            .diagnostics
            .items
            .iter()
            .filter(|item| item.code.starts_with('E'))
            .map(|item| format!("{}: {}", item.code, item.message))
            .collect();
        return failure(messages.join("; "));
    }
    let Some(entry) = program.entry() else {
        return failure(format!("`{}` produced no module", path.display()));
    };
    let mut plugin = Interp::new(&entry.checked).with_buffer();
    let diagnostics = crate::driver::run_modules(&mut plugin, &program);
    if diagnostics.has_errors() {
        let messages: Vec<String> = diagnostics
            .items
            .iter()
            .map(|item| format!("{}: {}", item.code, item.message))
            .collect();
        return failure(messages.join("; "));
    }
    let count: usize = program
        .modules
        .iter()
        .map(|module| module.checked.registrations.len())
        .sum();
    crate::value::make_variant(
        aevia_types::adt::RESULT_ID,
        0,
        "Ok",
        vec![Value::Num(NumVal::int(num_bigint::BigInt::from(count)))],
    )
}
