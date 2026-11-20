//! The Rules view's edit pipeline: every change the view makes to the
//! database goes through [`Rules::write`] (`ARCHITECTURE.md` → "Decision
//! record: the Rules view's edit pipeline and section state"), the way
//! Transactions' changes go through its own.
//!
//! A write is a [`Policy`] and one call to core, an `op` on the database.
//! The pipeline opens the database for it ([`RulesStore::open`], refused
//! while a restore holds its lock) and owns the sequence around the call, so
//! each place that changes something only names core's function and says
//! what differs:
//!
//! - **No database** (an unavailable or empty view): nothing is opened or
//!   called and [`NO_DATABASE`] is said where the policy says errors.
//! - **Where an error is said** ([`Said`]): under the toolbar
//!   ([`Outcome::Refused`]), only to the dialog that asked, or on the undo
//!   bar, which offers the same Undo again with the reason. The text is
//!   core's error as a sentence.
//! - **A save** takes the bar away, asks for everything to be read again
//!   ([`Rules::reload`], on a background thread) and tells the shell
//!   [`RulesEvent::Changed`] when the policy names it: when a transaction
//!   changed, or what Transactions shows of one (its category, its rule, its
//!   name). The write itself runs here, on the UI thread. Until the read
//!   lands the view shows the last one, so the caller does what must show at
//!   once: a moved rule takes its new place, a deleted row goes, a created
//!   rule or category is selected when the read lands, and what was saved is
//!   remembered so the same save isn't made twice. Choosing what is selected
//!   afterwards stays with the caller, from what `op` returned.
//!
//! Reads that fill the view (a preview, the suggestions over a rule, a
//! change's items, what undoing would do) change nothing:
//! [`Rules::query`] opens the database for them with the same refusal.

use gpui_kit::{Context, SharedString};
use orre_core::db::Database;
use orre_core::id::ChangeBatchId;

use crate::accounts::store::StoreError;

use super::store::RulesStore;
use super::{Outcome, Rules, RulesEvent, Source, message};

/// Why a view without a database can't change anything.
pub(super) const NO_DATABASE: &str = "There is no database to change.";

/// Where a write's error is said.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Said {
    /// Under the toolbar ([`Outcome::Refused`]).
    Toolbar,
    /// Only returned: the dialog that asked says it.
    Dialog,
    /// On the undo bar, which offers to undo `batch` again with the reason:
    /// for the bar's own Undo.
    UndoBar(ChangeBatchId),
}

/// What a write does around its call to core. [`Policy::new`] says errors at
/// `said` and tells the shell nothing; [`Policy::tells_shell`] changes that.
pub(super) struct Policy<T> {
    said: Said,
    tells_shell: fn(&T) -> bool,
}

impl<T> Policy<T> {
    /// Errors said at `said`.
    pub(super) fn new(said: Said) -> Self {
        Self {
            said,
            tells_shell: |_| false,
        }
    }

    /// After a save, tells the shell [`RulesEvent::Changed`].
    pub(super) fn tells_shell(mut self) -> Self {
        self.tells_shell = |_| true;
        self
    }

    /// After a save, tells the shell [`RulesEvent::Changed`] when `changed`
    /// says, from what core returned: an apply that changed no transaction
    /// tells it nothing.
    pub(super) fn tells_shell_when(mut self, changed: fn(&T) -> bool) -> Self {
        self.tells_shell = changed;
        self
    }
}

impl Rules {
    /// Opens the database and runs `op` on it as `policy` says: refused
    /// without a database; an error said where the policy says; after a save
    /// the bar goes, a read of everything is asked for (it lands later) and
    /// the shell is told as the policy says. Returns what `op` returned, or
    /// why it didn't save (already said, unless the policy leaves it to the
    /// dialog).
    pub(super) fn write<T, E: Into<StoreError>>(
        &mut self,
        policy: Policy<T>,
        cx: &mut Context<Self>,
        op: impl FnOnce(&mut Database) -> Result<T, E>,
    ) -> Result<T, SharedString> {
        let result = match &self.source {
            Source::Database(store) => run(store, op).map_err(message),
            Source::None => Err(NO_DATABASE.into()),
        };
        match result {
            Ok(value) => {
                self.outcome = None;
                self.reload(cx);
                if (policy.tells_shell)(&value) {
                    cx.emit(RulesEvent::Changed);
                }
                cx.notify();
                Ok(value)
            }
            Err(error) => {
                match policy.said {
                    Said::Toolbar => self.outcome = Some(Outcome::Refused(error.clone())),
                    Said::Dialog => {}
                    Said::UndoBar(batch) => {
                        self.outcome = Some(Outcome::Done {
                            message: format!("Couldn't undo: {error}").into(),
                            undo: Some(batch),
                        });
                    }
                }
                cx.notify();
                Err(error)
            }
        }
    }

    /// Opens the database and runs a read on it: refused without a database,
    /// the error as a sentence.
    pub(super) fn query<T, E: Into<StoreError>>(
        &self,
        op: impl FnOnce(&Database) -> Result<T, E>,
    ) -> Result<T, SharedString> {
        match &self.source {
            Source::Database(store) => {
                let db = store.open().map_err(message)?;
                op(&db).map_err(|error| message(error.into()))
            }
            Source::None => Err(NO_DATABASE.into()),
        }
    }
}

/// Opens the database through `store` and runs `op` on it; the connection
/// closes when `op` returns.
fn run<T, E: Into<StoreError>>(
    store: &RulesStore,
    op: impl FnOnce(&mut Database) -> Result<T, E>,
) -> Result<T, StoreError> {
    let mut db = store.open()?;
    op(&mut db).map_err(Into::into)
}
