//! Immutable lexical checking evidence and revision-local implementation code.
//!
//! A source identity survives editing; this snapshot belongs to one successful
//! check. Closures retain it through their scope, rather than consulting mutable
//! span tables belonging to a later check of the same file.
use crate::eval::InstanceEntry;
use crate::value::Closure;
use crate::value::ClosureBody;
use crate::value::ClosureEnv;
use crate::value::IrEnv;
use crate::value::Scope;
use crate::value::Value;
use aevia_syntax::Expr;
use aevia_syntax::Pat;
use aevia_syntax::Span;
use aevia_types::CheckedModule;
use aevia_types::DictionaryUse;
use aevia_types::Type;
use std::cell::RefCell;
use std::rc::Rc;
use std::rc::Weak;

/// A span-keyed table on the per-call path (see [`crate::fxhash`]).
pub(crate) type SpanMap<V> = crate::fxhash::FxHashMap<Span, V>;

#[derive(Debug)]
pub(crate) struct ModuleEvidence {
    pub(crate) expr_types: SpanMap<Type>,
    pub(crate) literal_reps: SpanMap<aevia_types::Rep>,
    pub(crate) constant_regions: SpanMap<aevia_types::Rep>,
    pub(crate) dyn_calls: SpanMap<aevia_dim::Dim>,
    pub(crate) system_calls: SpanMap<(String, aevia_dim::Dim)>,
    pub(crate) rssn_calls: SpanMap<aevia_rssn::OpKey>,
    pub(crate) dictionaries: SpanMap<Vec<DictionaryUse>>,
    pub(crate) method_refs: SpanMap<aevia_types::check::ClassMethodRef>,
    pub(crate) constructor_refs: SpanMap<(aevia_types::adt::AdtId, usize)>,
    pub(crate) functions: SpanMap<aevia_types::check::FunctionEvidence>,
    pub(crate) callable_refs: SpanMap<aevia_types::CallableProjection>,
    pub(crate) try_targets: SpanMap<aevia_types::check::TryTarget>,
    pub(crate) units: aevia_dim::UnitTable,
}
#[derive(Debug)]
pub(crate) struct ModuleContext {
    pub(crate) evidence: ModuleEvidence,
    pub(crate) session: Weak<()>,
    pub(crate) instances: RefCell<Vec<InstanceTemplate>>,
}
fn respan<V: Clone>(table: &std::collections::HashMap<Span, V>) -> SpanMap<V> {
    table
        .iter()
        .map(|(span, value)| (*span, value.clone()))
        .collect()
}

impl ModuleContext {
    pub(crate) fn new(
        checked: &CheckedModule,
        session: &Rc<()>,
    ) -> Rc<Self> {
        let mut dictionaries: SpanMap<Vec<DictionaryUse>> = SpanMap::default();
        for site in &checked.dictionaries {
            dictionaries
                .entry(site.span)
                .or_default()
                .push(site.clone());
        }
        Rc::new(Self {
            evidence: ModuleEvidence {
                expr_types: respan(&checked.expr_types),
                literal_reps: respan(&checked.literal_reps),
                constant_regions: respan(&checked.constant_regions),
                system_calls: respan(&checked.system_calls),
                dyn_calls: checked
                    .dyn_calls
                    .iter()
                    .map(|site| (site.span, site.dim.clone()))
                    .collect(),
                rssn_calls: checked
                    .rssn_calls
                    .iter()
                    .map(|site| (site.span, site.key.clone()))
                    .collect(),
                dictionaries,
                method_refs: respan(&checked.method_refs),
                constructor_refs: respan(&checked.constructor_refs),
                functions: respan(&checked.functions),
                callable_refs: respan(&checked.callable_refs),
                try_targets: respan(&checked.try_targets),
                units: checked.units.clone(),
            },
            session: Rc::downgrade(session),
            instances: RefCell::new(Vec::new()),
        })
    }
}
#[derive(Clone, Debug)]
pub(crate) enum MethodCode {
    Ast {
        site: Span,
        params: Vec<Pat>,
        body: Expr,
        name: String,
    },
    Ir(Rc<aevia_ir::Function>),
}
/// The code owns no lexical environment; that is retained by closures and the
/// acyclic imported-scope graph. Reconstructing a method cannot create a new
/// environment/reference cycle.
#[derive(Clone, Debug)]
pub(crate) struct InstanceTemplate {
    pub(crate) entry: InstanceEntry,
    pub(crate) method: String,
    pub(crate) code: Rc<MethodCode>,
    pub(crate) scope: Weak<Scope>,
    pub(crate) cached: Rc<RefCell<Weak<Closure>>>,
}
impl InstanceTemplate {
    pub(crate) fn instantiate(&self) -> Option<Value> {
        if let Some(closure) = self.cached.borrow().upgrade() {
            return Some(Value::Closure(closure));
        }
        let scope = self.scope.upgrade()?;
        let closure = match &*self.code {
            | MethodCode::Ast {
                params,
                body,
                name,
                site,
            } => {
                let mut closure = Closure::ast(
                    params.clone(),
                    body.clone(),
                    scope.clone(),
                    Some(name.clone()),
                );
                closure.proof = scope.context.as_ref().and_then(|context| {
                    crate::generics::FunctionProof::at(context, *site, scope.types.as_ref())
                });
                closure
            },
            | MethodCode::Ir(function) => {
                Closure {
                    canonical_origin: Weak::new(),
                    params: Vec::new(),
                    body: ClosureBody::Ir(function.clone()),
                    env: ClosureEnv::Ir(Rc::new(IrEnv {
                        slots: RefCell::new(Vec::new()),
                        scope: Some(scope.clone()),
                    })),
                    name: Some(function.name.clone()),
                    captured_bindings: scope.types.clone(),
                    proof: scope.context.as_ref().and_then(|context| {
                        crate::generics::FunctionProof::at(
                            context,
                            function.span,
                            scope.types.as_ref(),
                        )
                    }),
                }
            },
        };
        let closure = Rc::new(closure);
        *self.cached.borrow_mut() = Rc::downgrade(&closure);
        Some(Value::Closure(closure))
    }
}

pub(crate) type PreviousContext = (
    Rc<ModuleContext>,
    Option<Rc<Scope>>,
    Option<Rc<crate::generics::CallBindings>>,
);
impl crate::Interp {
    /// A span locates evidence within the current lexical revision. This getter
    /// does not authenticate the provenance of a host-origin call or argument.
    pub(crate) fn expr_type(
        &self,
        span: Span,
    ) -> Option<&Type> {
        self.context.evidence.expr_types.get(&span)
    }

    pub(crate) fn enter_scope(
        &mut self,
        scope: &Rc<Scope>,
    ) -> Result<Option<PreviousContext>, crate::eval::EvalError> {
        if self
            .active_scope
            .as_ref()
            .is_some_and(|active| Rc::ptr_eq(active, scope))
        {
            return Ok(None);
        }
        let context = scope.context.as_ref().unwrap_or(&self.context);
        if !context
            .session
            .upgrade()
            .is_some_and(|session| Rc::ptr_eq(&session, &self.session))
        {
            return Err(crate::eval::EvalError::new(
                "E0809",
                "a closure belongs to a different or reset interpreter session",
            ));
        }
        let old = (
            self.context.clone(),
            self.active_scope.clone(),
            self.bindings.clone(),
        );
        self.context = context.clone();
        self.active_scope = Some(scope.clone());
        self.bindings = scope.types.clone();
        Ok(Some(old))
    }

    pub(crate) fn restore_context(
        &mut self,
        previous: Option<PreviousContext>,
    ) {
        if let Some((context, scope, bindings)) = previous {
            self.context = context;
            self.active_scope = scope;
            self.bindings = bindings;
        }
    }

    pub(crate) fn remember_scope(
        &mut self,
        scope: &Rc<Scope>,
    ) {
        self.module_scopes
            .insert(self.current_module.clone(), scope.clone());
    }

    pub(crate) fn module_scope(
        &mut self,
        imports: Vec<Rc<Scope>>,
    ) -> Rc<Scope> {
        let scope = Scope::module(self.context.clone(), imports);
        self.remember_scope(&scope);
        scope
    }

    /// The instantiated body of the first visible template whose global is
    /// `global`, in the order [`Self::instance_templates`] lists them.
    ///
    /// `None` means no visible template has that global; `Some(None)` means one
    /// does but its defining scope is gone. That is the answer
    /// `instance_templates().find(..).and_then(instantiate)` gives, without
    /// building (and cloning every template of) the list.
    pub(crate) fn find_instance_template(
        &self,
        global: &str,
    ) -> Option<Option<Value>> {
        // The innermost context is searched first by the full walk below, so
        // a hit there needs none of the walk's bookkeeping.
        if let Some(context) = self
            .active_scope
            .as_ref()
            .and_then(|scope| scope.context.as_ref())
            && let Some(template) = context
                .instances
                .borrow()
                .iter()
                .rev()
                .find(|template| template.entry.global == global)
        {
            return Some(template.instantiate());
        }
        let mut pending = self.active_scope.iter().cloned().collect::<Vec<_>>();
        let mut scopes = std::collections::HashSet::new();
        let mut contexts = std::collections::HashSet::new();
        while let Some(scope) = pending.pop() {
            if !scopes.insert(Rc::as_ptr(&scope)) {
                continue;
            }
            if let Some(context) = &scope.context
                && contexts.insert(Rc::as_ptr(context))
                && let Some(template) = context
                    .instances
                    .borrow()
                    .iter()
                    .rev()
                    .find(|template| template.entry.global == global)
            {
                return Some(template.instantiate());
            }
            pending.extend(scope.imports.iter().rev().cloned());
            if let Some(parent) = &scope.parent {
                pending.push(parent.clone());
            }
        }
        None
    }

    /// The mangled global of `key`'s `method`, formatted once per call site.
    ///
    /// Mangling renders the whole key through `Debug`, which was a large share
    /// of every instance method call. The cache is keyed by the call's span and
    /// verified against the key itself, so a re-checked module that reuses a
    /// span for a different instance cannot be answered from a stale entry.
    pub(crate) fn cached_instance_global(
        &self,
        span: Span,
        key: &aevia_types::InstanceKey,
        method: &str,
    ) -> Rc<str> {
        let mut cache = self.instance_globals.borrow_mut();
        if cache.len() >= 4096 {
            cache.clear();
        }
        let entries = cache.entry(span).or_default();
        if let Some((_, _, global)) = entries
            .iter()
            .find(|(cached, name, _)| cached == key && name == method)
        {
            return global.clone();
        }
        let global: Rc<str> = Self::instance_global(key, method).into();
        entries.push((key.clone(), method.to_string(), global.clone()));
        global
    }

    /// An empty buffer with room for `capacity` values, reused when one is free.
    ///
    /// Every call builds an argument list and a frame; both are dropped when
    /// the call ends, so a one-slot-per-call allocation pair is recycled
    /// instead.
    pub(crate) fn take_buffer(
        &self,
        capacity: usize,
    ) -> Vec<Value> {
        let mut buffer = self.buffer_pool.borrow_mut().pop().unwrap_or_default();
        buffer.reserve(capacity);
        buffer
    }

    /// Returns a buffer taken with [`Self::take_buffer`] (or any `Vec`) to the pool.
    pub(crate) fn recycle_buffer(
        &self,
        mut buffer: Vec<Value>,
    ) {
        // Dropping the values may run arbitrary destructors, so it happens
        // before the pool is borrowed.
        buffer.clear();
        if buffer.capacity() != 0 && buffer.capacity() <= 256 {
            let mut pool = self.buffer_pool.borrow_mut();
            if pool.len() < 64 {
                pool.push(buffer);
            }
        }
    }

    pub(crate) fn instance_templates(&self) -> Vec<InstanceTemplate> {
        let mut pending = self.active_scope.iter().cloned().collect::<Vec<_>>();
        let mut scopes = std::collections::HashSet::new();
        let mut contexts = std::collections::HashSet::new();
        let mut seen = std::collections::HashSet::new();
        let mut result = Vec::new();
        while let Some(scope) = pending.pop() {
            if !scopes.insert(Rc::as_ptr(&scope)) {
                continue;
            }
            if let Some(context) = &scope.context
                && contexts.insert(Rc::as_ptr(context))
            {
                for template in context.instances.borrow().iter().rev() {
                    // Newest lexical implementation wins only within this
                    // revision's scope graph, never via mutable globals.
                    if seen.insert(template.entry.global.clone()) {
                        result.push(template.clone());
                    }
                }
            }
            pending.extend(scope.imports.iter().rev().cloned());
            if let Some(parent) = &scope.parent {
                pending.push(parent.clone());
            }
        }
        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn implementation_templates_do_not_keep_their_defining_scope_alive() {
        let session = Rc::new(());
        let context = ModuleContext::new(&CheckedModule::default(), &session);
        let scope = Scope::module(context.clone(), Vec::new());
        let weak = Rc::downgrade(&scope);
        context.instances.borrow_mut().push(InstanceTemplate {
            entry: InstanceEntry {
                key: None,
                checked_head: None,
                shape: crate::eval::HeadShape::Scalar("Int"),
                global: "test".into(),
            },
            method: "test".into(),
            scope: weak.clone(),
            cached: Rc::new(RefCell::new(Weak::new())),
            code: Rc::new(MethodCode::Ast {
                site: Span::default(),
                params: Vec::new(),
                body: Expr {
                    kind: aevia_syntax::ExprKind::UnitLit,
                    span: Span::default(),
                },
                name: "test".into(),
            }),
        });
        assert!(context.instances.borrow()[0].instantiate().is_some());
        drop(scope);
        assert!(weak.upgrade().is_none());
        assert!(context.instances.borrow()[0].instantiate().is_none());
    }
}
