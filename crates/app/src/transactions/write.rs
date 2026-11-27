//! The Transactions view's edit pipeline: every change the view makes to
//! the database goes through [`TransactionsView::write`] (`ARCHITECTURE.md`
//! → "Decision record: the Transactions view", point 3, and "Decision
//! record: splits, tags and notes in the inspector", point 1).
//!
//! A write is a [`Policy`] and one call to core, an `op` on the database.
//! The pipeline opens the database for it ([`TransactionStore::open`],
//! refused while a restore holds its lock) and owns the sequence around
//! the call, so each place that changes something only names core's
//! function and says what differs:
//!
//! - **Sample data** can't change: nothing is opened or called and the
//!   [`SAMPLE_DATA`] refusal is said where the policy says errors.
//! - **Where an error is said** ([`Said`]): under the inspector's Category
//!   select, under an inspector section, only to the dialog that asked, or
//!   nowhere (logged). The error (opening's or core's) is said as it is,
//!   or as a sentence (the M3.10 dialogs, the exclusion card and the
//!   Transfer section).
//! - **The undo bar** ([`Offer`]): left as it is, taken away, or offering to
//!   revert what was saved. A single-row edit shows none (the M3.3 rule);
//!   links, a change to several rows, excluding and transfer decisions do.
//!   An undo the bar offered that fails is offered again with the reason.
//! - **The register** is read again after a save, and the shell is told
//!   through a [`TransactionsEvent`] when the policy names one. Inside a
//!   shell the shell reads it again, with every other view of the ledger
//!   ([`crate::shell::live`]).
//!
//! Reads that only fill a dialog or a prompt (what undoing an import
//! removes, the rule prompt's proposal, a manual row's fields) don't change
//! anything and stay with their callers: [`TransactionsView::query`] opens
//! the database for them with the same refusal, or they call
//! [`TransactionStore::open`] themselves.

use gpui_kit::{Context, SharedString};
use orre_core::db::Database;
use orre_core::id::TransactionId;

use crate::accounts::new_account::sentence;
use crate::accounts::store::StoreError;

use super::editor::EditPlace;
use super::store::TransactionStore;
use super::{EditFailure, Source, TransactionsEvent, TransactionsView, Undo, UndoAction};

/// Why sample data (the Paper fixture) can't change.
#[cfg(any(test, feature = "dev-samples"))]
pub(super) const SAMPLE_DATA: &str = "This is sample data, which can't be changed.";

/// Where a write's error is said.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Said {
    /// Under the inspector's Category select (the view's `edit_error`).
    Category,
    /// Under an inspector section.
    Inspector(EditPlace),
    /// Only returned: the dialog that asked says it.
    Dialog,
    /// Nowhere: logged, if the policy logs.
    Logged,
}

/// Which error a write that saves takes away.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Clears {
    /// Neither.
    Nothing,
    /// The one under the Category select.
    Category,
    /// The one under an inspector section, wherever it is.
    Inspector,
}

/// What the undo bar does after a write that saved.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) enum Offer {
    /// Stays as it is.
    Keep,
    /// Goes away.
    Clear,
    /// Goes away if it offers this.
    ClearIf(UndoAction),
    /// Offers to revert `action`, saying the message.
    Show(UndoAction, SharedString),
}

/// What a write does around its call to core. [`Policy::new`] says errors at
/// `said` as they are, logs nothing, leaves the undo bar, reads the register
/// again after a save and tells the shell nothing; the rest are changes to
/// that.
pub(super) struct Policy<'a, T> {
    said: Said,
    sentence: bool,
    log: Option<&'static str>,
    clears: Clears,
    offer: Box<dyn FnOnce(&T) -> Offer + 'a>,
    retry: Option<UndoAction>,
    event: Option<TransactionsEvent>,
    reload: bool,
    always: bool,
    wrote: fn(&T) -> bool,
}

impl<'a, T> Policy<'a, T> {
    /// Errors said at `said`; a save takes away the error said there before.
    pub(super) fn new(said: Said) -> Self {
        let clears = match said {
            Said::Category => Clears::Category,
            Said::Inspector(_) => Clears::Inspector,
            Said::Dialog | Said::Logged => Clears::Nothing,
        };
        Self {
            said,
            sentence: false,
            log: None,
            clears,
            offer: Box::new(|_| Offer::Keep),
            retry: None,
            event: None,
            reload: true,
            always: false,
            wrote: |_| true,
        }
    }

    /// One of the inspector's own edits (M3.9: split, tags, note, merchant
    /// name, a manual row's fields): said at `place`, no undo bar.
    pub(super) fn edit(place: EditPlace) -> Self {
        Self::new(Said::Inspector(place)).log("couldn't save an edit")
    }

    /// A link or share edit in the inspector (M3.11): said at `place`; Owed
    /// to you reads them again.
    pub(super) fn link(place: EditPlace) -> Self {
        Self::new(Said::Inspector(place))
            .log("couldn't save a link or share")
            .emit(TransactionsEvent::Changed)
    }

    /// A change from one of the M3.10 dialogs (excluding, undoing an import,
    /// adding or deleting a manual row): said to the dialog as a sentence;
    /// the accounts panel and the checks read again.
    pub(super) fn ledger() -> Self {
        Self::new(Said::Dialog)
            .sentence()
            .emit(TransactionsEvent::LedgerChanged)
    }

    /// The store's error is said as a sentence: capitalised, with a full
    /// stop.
    pub(super) fn sentence(mut self) -> Self {
        self.sentence = true;
        self
    }

    /// A failed write is logged with `message` (never the row's
    /// contents).
    pub(super) fn log(mut self, message: &'static str) -> Self {
        self.log = Some(message);
        self
    }

    /// Which error a save takes away, if not the one at `said`.
    pub(super) fn clears(mut self, clears: Clears) -> Self {
        self.clears = clears;
        self
    }

    /// What the undo bar does after a save, from what core returned.
    pub(super) fn undo(mut self, offer: impl FnOnce(&T) -> Offer + 'a) -> Self {
        self.offer = Box::new(offer);
        self
    }

    /// When it fails, the undo bar offers `action` again with the reason:
    /// for the bar's own Undo, whose row may be filtered out or no longer
    /// show the section that says the error.
    pub(super) fn retry(mut self, action: UndoAction) -> Self {
        self.retry = Some(action);
        self
    }

    /// After a save, tells the shell `event`.
    pub(super) fn emit(mut self, event: TransactionsEvent) -> Self {
        self.event = Some(event);
        self
    }

    /// Nothing to read again: the write changes no row.
    pub(super) fn no_reload(mut self) -> Self {
        self.reload = false;
        self
    }

    /// Reads the register again and tells the shell even when it fails:
    /// the undo bar's Undo, after which the table shows what's stored.
    pub(super) fn always(mut self) -> Self {
        self.always = true;
        self
    }

    /// Whether the call wrote anything, from what it returned: when it
    /// didn't (a rename to the same name), nothing else happens.
    pub(super) fn wrote(mut self, wrote: fn(&T) -> bool) -> Self {
        self.wrote = wrote;
        self
    }
}

impl TransactionsView {
    /// Opens the database and runs `op` on it as `policy` says: refused on
    /// sample data; an error said where the policy says, the undo bar and
    /// the register as it says, and the shell told. Returns what `op`
    /// returned, or why it didn't save (already said, unless the policy
    /// leaves it to the dialog).
    pub(super) fn write<T, E: Into<StoreError>>(
        &mut self,
        policy: Policy<'_, T>,
        cx: &mut Context<Self>,
        op: impl FnOnce(&mut Database) -> Result<T, E>,
    ) -> Result<T, SharedString> {
        let result = match &self.source {
            Source::Database(store) => run(store, op).map_err(|error| {
                if let Some(message) = policy.log {
                    tracing::warn!(%error, "{message}");
                }
                let text = error.to_string();
                if policy.sentence {
                    sentence(&text)
                } else {
                    text.into()
                }
            }),
            #[cfg(any(test, feature = "dev-samples"))]
            Source::Fixture => Err(SAMPLE_DATA.into()),
            Source::Unavailable(reason) => Err(reason.clone()),
        };
        match result {
            Ok(value) => {
                if !(policy.wrote)(&value) {
                    return Ok(value);
                }
                match policy.clears {
                    Clears::Nothing => {}
                    Clears::Category => self.edit_error = None,
                    Clears::Inspector => self.editor.error = None,
                }
                match (policy.offer)(&value) {
                    Offer::Keep => {}
                    Offer::Clear => self.undo = None,
                    Offer::ClearIf(action) => {
                        if self.undo.as_ref().map(|undo| &undo.action) == Some(&action) {
                            self.undo = None;
                        }
                    }
                    Offer::Show(action, message) => self.undo = Some(Undo { action, message }),
                }
                self.after_write(&policy.event, policy.reload, cx);
                Ok(value)
            }
            Err(error) => {
                match policy.said {
                    Said::Category => self.edit_error = Some(error.clone()),
                    Said::Inspector(place) => self.editor.error = Some((place, error.clone())),
                    Said::Dialog | Said::Logged => {}
                }
                if let Some(action) = policy.retry {
                    self.undo = Some(Undo {
                        action,
                        message: format!("Couldn't undo: {error}").into(),
                    });
                }
                if policy.always {
                    self.after_write(&policy.event, policy.reload, cx);
                } else {
                    cx.notify();
                }
                Err(error)
            }
        }
    }

    /// Reads the register again if `reload` (inside a shell, the shell
    /// does, with every view of the ledger), tells the shell `event`.
    fn after_write(
        &mut self,
        event: &Option<TransactionsEvent>,
        reload: bool,
        cx: &mut Context<Self>,
    ) {
        if reload {
            self.wrote_ledger(cx);
        }
        if let Some(event) = event {
            cx.emit(event.clone());
        }
        cx.notify();
    }

    /// Opens the database and runs a read a dialog needs on it (what
    /// undoing an import removes): refused on sample data, the error as a
    /// sentence.
    pub(super) fn query<T, E: Into<StoreError>>(
        &self,
        op: impl FnOnce(&Database) -> Result<T, E>,
    ) -> Result<T, SharedString> {
        match &self.source {
            Source::Database(store) => {
                run(store, |db| op(db)).map_err(|error| sentence(&error.to_string()))
            }
            #[cfg(any(test, feature = "dev-samples"))]
            Source::Fixture => Err(SAMPLE_DATA.into()),
            Source::Unavailable(reason) => Err(reason.clone()),
        }
    }

    /// After saving what was typed on row `left` as the user left it: an
    /// error said in the inspector can't show there (it shows the next row
    /// by then), so the failure bar says it instead, in place of the undo
    /// bar, and its Show goes back to the row with the reason.
    pub(super) fn say_on_failure_bar(&mut self, left: TransactionId) {
        if let Some((place, reason)) = self.editor.error.take() {
            self.undo = None;
            self.edit_failure = Some(EditFailure {
                row: left,
                place,
                reason,
            });
        }
    }
}

/// Opens the database through `store` and runs `op` on it; the connection
/// closes when `op` returns.
fn run<T, E: Into<StoreError>>(
    store: &TransactionStore,
    op: impl FnOnce(&mut Database) -> Result<T, E>,
) -> Result<T, StoreError> {
    let mut db = store.open()?;
    op(&mut db).map_err(Into::into)
}
