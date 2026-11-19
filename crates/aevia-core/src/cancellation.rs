//! Cooperative interruption of an interpreter and its dispatched workers.
#![deny(
    missing_docs,
    clippy::arithmetic_side_effects,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::nursery,
    clippy::panic,
    clippy::pedantic,
    clippy::single_call_fn,
    clippy::unwrap_used
)]

use crate::EvalError;
use crate::Interp;
use std::sync::Arc;
use std::sync::Mutex;
use std::sync::Weak;
use std::sync::atomic::AtomicBool;
use std::sync::atomic::Ordering;

/// The stable runtime diagnostic for an interrupted evaluation.
pub const INTERRUPTED: &str = "E0812";

#[derive(Default, Debug)]
struct State {
    requested: Arc<AtomicBool>,
    children: Mutex<Vec<Weak<Self>>>,
}

thread_local! {
    static CURRENT: std::cell::RefCell<Option<CancellationToken>> = const { std::cell::RefCell::new(None) };
}

/// Restores both evaluator and kernel interruption context after unwinding.
pub(crate) struct CancellationScope {
    previous: Option<CancellationToken>,
    _kernel: aevia_rssn::cancellation::Guard,
}

impl Drop for CancellationScope {
    fn drop(&mut self) {
        CURRENT.with(|current| *current.borrow_mut() = self.previous.take());
    }
}

#[allow(
    clippy::single_call_fn,
    reason = "keep cancellation context private to this module"
)]
pub(crate) fn inherited_token() -> Option<CancellationToken> {
    CURRENT.with(|current| current.borrow().clone())
}

pub(crate) fn enter_optional(token: Option<CancellationToken>) -> CancellationScope {
    let flag = token.as_ref().map(|token| token.0.requested.clone());
    let previous = CURRENT.with(|current| current.replace(token));
    CancellationScope {
        previous,
        _kernel: aevia_rssn::cancellation::install_optional(flag),
    }
}

/// A thread-safe interruption request. Cancelling does not unwind another
/// thread; the interpreter stops at its next cooperative checkpoint.
///
/// Clones address the same request. Hosts reset it before a new evaluation,
/// never during one. Dispatched workers receive child requests that remain
/// cancelled even if a later foreground evaluation resets its parent.
#[derive(Clone, Default, Debug)]
pub struct CancellationToken(Arc<State>);

impl CancellationToken {
    /// Creates a token with no pending interruption.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Requests interruption of this evaluation and all dispatched children.
    pub fn cancel(&self) {
        self.0.requested.store(true, Ordering::Release);
        let mut children = self
            .0
            .children
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        children.retain(|child| {
            child.upgrade().is_some_and(|child| {
                Self(child).cancel();
                true
            })
        });
    }

    /// Clears the foreground request before the host starts new work.
    pub fn reset(&self) {
        // New foreground work owns a new set of dispatched children. Earlier
        // successful background tasks keep their own request; awaiting one
        // explicitly connects interruption to that task again.
        self.0
            .children
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clear();
        self.0.requested.store(false, Ordering::Release);
    }

    /// Whether this evaluation has been interrupted.
    #[must_use]
    pub fn is_cancelled(&self) -> bool {
        self.0.requested.load(Ordering::Acquire)
    }

    pub(crate) fn child(&self) -> Self {
        let child = Self::new();
        let mut children = self
            .0
            .children
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        children.retain(|child| child.strong_count() > 0);
        if self.is_cancelled() {
            child.cancel();
        }
        children.push(Arc::downgrade(&child.0));
        child
    }

    #[allow(
        clippy::single_call_fn,
        reason = "give token holders an explicit scope-entry operation"
    )]
    pub(crate) fn enter(&self) -> CancellationScope {
        enter_optional(Some(self.clone()))
    }
}

impl Interp {
    /// Installs the request shared with the host's signal handler.
    pub fn set_cancellation_token(
        &mut self,
        token: CancellationToken,
    ) {
        self.cancellation = Some(token);
    }

    /// Returns this interpreter's request, if the host installed one.
    #[must_use]
    pub fn cancellation_token(&self) -> Option<CancellationToken> {
        self.cancellation.clone()
    }

    /// Stops before executing the next evaluator step if interruption was requested.
    ///
    /// # Errors
    ///
    /// Returns `E0812` when the interruption token is set.
    pub fn check_cancelled(&self) -> Result<(), EvalError> {
        if self
            .cancellation
            .as_ref()
            .is_some_and(CancellationToken::is_cancelled)
        {
            Err(EvalError::new(INTERRUPTED, "evaluation interrupted"))
        } else {
            Ok(())
        }
    }

    pub(crate) fn cancellation_scope(&self) -> Option<CancellationScope> {
        self.cancellation.as_ref().map(CancellationToken::enter)
    }

    /// Supplies an interactive line reader, for example one whose terminal
    /// editor reports Ctrl-C as an interruption. Without it, IO reads stdin.
    pub fn set_line_reader(
        &mut self,
        reader: std::rc::Rc<dyn Fn() -> Result<String, EvalError>>,
    ) {
        self.line_reader = Some(reader);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use aevia_types::check::CheckedModule;

    #[test]
    fn resetting_the_foreground_does_not_revive_dispatched_children() {
        let parent = CancellationToken::new();
        let child = parent.child();
        let grandchild = child.child();
        parent.cancel();
        parent.reset();
        assert!(!parent.is_cancelled());
        assert!(child.is_cancelled());
        assert!(grandchild.is_cancelled());
        assert!(!parent.child().is_cancelled());
    }

    #[test]
    fn a_later_foreground_request_does_not_cancel_an_earlier_background_task() {
        let foreground = CancellationToken::new();
        let previous_task = foreground.child();
        foreground.reset();
        foreground.cancel();
        assert!(!previous_task.is_cancelled());
    }

    #[test]
    fn replicas_inherit_the_scoped_request_and_unrelated_jobs_clear_it() {
        let request = CancellationToken::new();
        let checked = CheckedModule::default();
        {
            let _active = request.enter();
            let replica = Interp::new(&checked);
            request.cancel();
            let outcome = replica.check_cancelled();
            assert!(
                matches!(&outcome, Err(failure) if failure.code == INTERRUPTED),
                "{outcome:?}"
            );
            {
                let _unrelated = enter_optional(None);
                assert!(Interp::new(&checked).check_cancelled().is_ok());
                assert!(!aevia_rssn::cancellation::is_cancelled());
            }
            assert!(aevia_rssn::cancellation::is_cancelled());
        }
        assert!(inherited_token().is_none());
        assert!(!aevia_rssn::cancellation::is_cancelled());
    }
}
