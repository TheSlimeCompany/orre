//! Cooperative cancellation of a long read (SLI-37).
//!
//! A [`Cancel`] is a shared flag: whoever started a read keeps one clone
//! and calls [`Cancel::cancel`] when the answer is no longer wanted; the
//! read checks [`Cancel::check`] between its steps and in its hot loops,
//! and stops with [`Cancelled`] instead of an answer. Nothing is
//! interrupted: a read only stops at a check, and does no more work after
//! it.
//!
//! The flag carries no data, so relaxed atomics are enough: a read that
//! misses a `cancel()` for a few checks does a little more work and is
//! thrown away all the same.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

/// A clone-able cancel flag (module docs). Clones share one flag.
#[derive(Clone, Debug)]
pub struct Cancel(Option<Arc<AtomicBool>>);

impl Default for Cancel {
    /// [`Cancel::new`].
    fn default() -> Self {
        Self::new()
    }
}

impl Cancel {
    /// A flag not cancelled yet.
    pub fn new() -> Self {
        Self(Some(Arc::new(AtomicBool::new(false))))
    }

    /// A flag that is never cancelled: what a caller that always wants the
    /// answer passes. [`Cancel::cancel`] on it (or a clone) does nothing.
    pub const fn never() -> Self {
        Self(None)
    }

    /// Asks every read holding a clone of this flag to stop.
    pub fn cancel(&self) {
        if let Some(flag) = &self.0 {
            flag.store(true, Ordering::Relaxed);
        }
    }

    /// Whether [`Cancel::cancel`] was called on this flag or a clone.
    pub fn is_cancelled(&self) -> bool {
        self.0
            .as_ref()
            .is_some_and(|flag| flag.load(Ordering::Relaxed))
    }

    /// [`Cancelled`] once the flag is cancelled: `cancel.check()?` between
    /// a read's steps.
    pub fn check(&self) -> Result<(), Cancelled> {
        if self.is_cancelled() {
            Err(Cancelled)
        } else {
            Ok(())
        }
    }
}

/// A read stopped because its [`Cancel`] was cancelled: neither an answer
/// nor a failure.
#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
#[error("the read was cancelled")]
pub struct Cancelled;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clones_share_the_flag() {
        let cancel = Cancel::new();
        let clone = cancel.clone();
        assert_eq!(clone.check(), Ok(()));
        cancel.cancel();
        assert!(clone.is_cancelled());
        assert_eq!(clone.check(), Err(Cancelled));
    }

    #[test]
    fn never_is_never_cancelled() {
        let never = Cancel::never();
        never.clone().cancel();
        assert!(!never.is_cancelled());
    }
}
