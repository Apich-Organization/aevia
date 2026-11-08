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
        let mut rewrites = Vec::new();
        let mut published = Vec::new();
        for registration in registrations {
            let Some(function) = scope.lookup(&registration.function) else {
                diagnostics.push(Diagnostic::error(
                    "E1006",
                    format!("`{}` is not bound", registration.function),
                    registration.span,
                ));
                continue;
            };
            match trace(self, registration, function) {
                | Ok(text) => {
                    match registration.kind {
                        | HostRegistrationKind::Op => definitions.push(text),
                        | HostRegistrationKind::Rule => rewrites.push(text),
                    }
                    published.push(registration);
                },
                | Err(reason) => {
                    diagnostics.push(Diagnostic::error(
                        "E1006",
                        format!(
                            "`@{}(\"{}\")` cannot be published: {reason}",
                            kind_name(registration.kind),
                            registration.name
                        ),
                        registration.span,
                    ));
                },
            }
        }
        if definitions.is_empty() && rewrites.is_empty() {
            return diagnostics;
        }
        // A REPL reloads modules; each load is its own plugin.
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
        if let Err(reason) =
            aevia_rssn::kernel().register_source_plugin(&name, "0.1.0", definitions, rewrites)
        {
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
        }
        diagnostics
    }
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
