//! The review queues (ROADMAP M3.12; `docs/DESIGN.md` → "Review queues";
//! `docs/ARCHITECTURE.md` → "Decision record: review queues" and "Decision
//! record: review queues (built)"): Orre's own checks, under CHECKS in
//! the Inbox's left panel.
//!
//! - **Possible duplicates**: each pending pair, the selected one open in
//!   place side by side, with why it was flagged and what merging would do.
//!   Merge excludes one row as a duplicate (never deletes it) as an
//!   undoable change batch; "Not duplicates" dismisses the pair for good,
//!   undoable from the bar right after.
//! - **Balance checks**: every account's balances checked against its
//!   transactions, a failure's window and the pair that explains it, and
//!   locking an account's checked history (and unlocking it).
//! - **Owed to you** (M3.11): outstanding expense claims, with the payment
//!   Orre matched to some of them ("Link as reimbursement", "Not this"),
//!   and the people who owe the user for shared costs, a person's costs,
//!   payments and name in the inspector.
//!
//! [`Checks`] holds what the queues show (from [`store::ChecksStore`],
//! which opens the database per call), which queue is open, the
//! selection and the last outcome. Every change is core's
//! ([`orre_core::duplicate`], [`orre_core::reconciliation`],
//! [`orre_core::change`]); this only picks the IDs. Merchant names and
//! bank text are untrusted and shown plain ([`text`]).

pub mod balances;
#[cfg(any(test, feature = "dev-driver"))]
pub(crate) mod describe;
pub mod duplicates;
pub mod fixture;
pub mod lock;
pub mod owed;
pub mod store;
pub mod text;
pub mod view;

use gpui_kit::{AppContext as _, Context, EventEmitter, SharedString, Task};
use orre_core::checks::{AccountReview, CheckRow, Pair};
use orre_core::id::{AccountId, ChangeBatchId, DuplicateCandidateId, LinkId, TransactionId};
use orre_core::link::ShareLabel;
use orre_core::link::owed::Person;
use orre_core::money::Currency;
use orre_core::reconciliation::BalanceRef;

use crate::accounts::store::StoreError;
use crate::shell::live::{Reloads, Wrote};
use crate::ui::list::{self, Move};
use store::{ChecksData, ChecksStore};

/// Binds the queues' keys ([`view::KEYS`]): M merge, N not duplicates,
/// ←→ keep the other row, ↑↓ select, Escape clears the selection. Call
/// once, before the first [`view::ChecksView`].
pub fn init(cx: &mut gpui_kit::App) {
    view::KEYS.bind(cx);
}

/// The queues under CHECKS.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Queue {
    Duplicates,
    Balances,
    /// Owed to you (M3.11).
    Owed,
}

impl Queue {
    /// Every queue, in the panel's order.
    pub const ALL: [Queue; 3] = [Queue::Duplicates, Queue::Balances, Queue::Owed];

    /// What the queue is called: its toolbar's title.
    pub const fn title(self) -> &'static str {
        match self {
            Queue::Duplicates => "Possible duplicates",
            Queue::Balances => "Balance checks",
            Queue::Owed => "Owed to you",
        }
    }
}

/// What the queues ask their owner (the shell) to do.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ChecksEvent {
    /// A queue was opened (`Some`) or closed for the suggestions (`None`):
    /// the shell shows and focuses the right canvas.
    Shown(Option<Queue>),
    /// "Lock through …": open the lock dialog.
    LockRequested(AccountId),
    /// "Unlock…": open the unlock dialog.
    UnlockRequested(AccountId),
    /// "Add balance…": the accounts' Add balance dialog.
    AddBalanceRequested(AccountId),
    /// "Link a payment…": the link dialog for a person (M3.11).
    LinkPaymentRequested(ShareLabel, Currency),
    /// Transactions or locks changed: what reads them should read again.
    Changed,
}

/// What a change can be undone with, from the bar.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Undo {
    /// A merge's `duplicate_merge` batch, and the pair it merged.
    Merge(ChangeBatchId, DuplicateCandidateId),
    /// A dismissed pair, reopened.
    Dismiss(DuplicateCandidateId),
    /// A link edit's change batch (Owed to you, M3.11).
    Batch(ChangeBatchId),
}

/// What the last change did, shown until the next one.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Outcome {
    /// It went through: the bar's message, and its undo.
    Done {
        message: SharedString,
        undo: Option<Undo>,
    },
    /// Core refused it; nothing changed.
    Refused(SharedString),
}

/// Where the queues are read from.
enum Source {
    Database(ChecksStore),
    /// Nothing to read (no data folder, or a test without a database).
    None,
}

/// The review queues' state (module docs).
pub struct Checks {
    source: Source,
    data: ChecksData,
    queue: Option<Queue>,
    pair: Option<DuplicateCandidateId>,
    /// The selected pair planned keeping its other row ("Keep this
    /// instead", ←→).
    swapped: Option<Pair>,
    check: Option<(AccountId, BalanceRef)>,
    /// The person selected on Owed to you, in a currency.
    person: Option<(ShareLabel, Currency)>,
    outcome: Option<Outcome>,
    load_error: Option<SharedString>,
    /// The background read in flight; a newer one replaces it.
    loading: Option<Task<()>>,
    /// Who reads the queues again after their own write.
    reloads: Reloads,
    /// A pair to select when the read lands (Undo reopening it).
    pending_pair: Option<DuplicateCandidateId>,
    /// An account whose check to select when the read lands (`show_account`).
    pending_account: Option<AccountId>,
    /// Select the open queue's first entry when the read lands, if nothing
    /// in it is selected (`show`).
    pending_default: bool,
}

impl EventEmitter<ChecksEvent> for Checks {}
impl EventEmitter<Wrote> for Checks {}

impl Checks {
    /// The queues in `store`, read now, on this thread: for tests and the
    /// snapshot. The app uses [`Checks::unloaded`].
    pub fn from_store(store: ChecksStore) -> Self {
        let mut checks = Self::with_source(Source::Database(store));
        checks.read_now();
        checks
    }

    /// The queues in `store`, not read yet: the shell starts the first read
    /// (`start_loading`), on a background thread.
    pub fn unloaded(store: ChecksStore) -> Self {
        Self::with_source(Source::Database(store))
    }

    /// No queues, because they can't be read: `reason` says why.
    pub fn unavailable(reason: impl Into<SharedString>) -> Self {
        Self {
            load_error: Some(reason.into()),
            ..Self::with_source(Source::None)
        }
    }

    /// Empty queues that read nothing (tests).
    pub fn empty() -> Self {
        Self::with_source(Source::None)
    }

    fn with_source(source: Source) -> Self {
        Self {
            source,
            data: ChecksData::default(),
            queue: None,
            pair: None,
            swapped: None,
            check: None,
            person: None,
            outcome: None,
            load_error: None,
            loading: None,
            reloads: Reloads::Itself,
            pending_pair: None,
            pending_account: None,
            pending_default: false,
        }
    }

    pub fn data(&self) -> &ChecksData {
        &self.data
    }

    pub fn today(&self) -> chrono::NaiveDate {
        self.data
            .today
            .unwrap_or_else(crate::accounts::fixture::today)
    }

    /// The open queue; `None` while the Inbox shows suggestions.
    pub fn queue(&self) -> Option<Queue> {
        self.queue
    }

    pub fn outcome(&self) -> Option<&Outcome> {
        self.outcome.as_ref()
    }

    pub fn load_error(&self) -> Option<&SharedString> {
        self.load_error.as_ref()
    }

    /// Pending possible duplicates: the panel's count.
    pub fn pending_pairs(&self) -> usize {
        self.data.review.pairs.len()
    }

    /// Open items on Owed to you: claims and people (the panel's count).
    pub fn owed_open(&self) -> usize {
        self.data.owed.open()
    }

    /// Failing balance checks: the panel's count and the status bar's.
    pub fn failing(&self) -> usize {
        self.data.review.failing()
    }

    /// `queue`'s count on its row in the Inbox panel's CHECKS.
    pub fn queue_count(&self, queue: Queue) -> usize {
        match queue {
            Queue::Duplicates => self.pending_pairs(),
            Queue::Balances => self.failing(),
            Queue::Owed => self.owed_open(),
        }
    }

    /// Whether `queue`'s row in the panel is flagged: Balance checks, while
    /// a check fails.
    pub fn queue_flagged(&self, queue: Queue) -> bool {
        queue == Queue::Balances && self.failing() > 0
    }

    /// Whether any account has a balance check yet: the status bar's
    /// "Reconciled" needs one.
    pub fn has_checks(&self) -> bool {
        self.data.review.has_checks()
    }

    // Opening a queue ----------------------------------------------------------

    /// Opens `queue` (CHECKS in the panel), selecting its first pair or
    /// its first failing check when nothing in it is selected (user,
    /// 2026-11-14).
    pub fn show(&mut self, queue: Queue, cx: &mut Context<Self>) {
        let opened = self.queue != Some(queue);
        self.queue = Some(queue);
        if opened {
            self.pending_default = true;
            self.reload(cx);
        }
        self.select_default();
        cx.emit(ChecksEvent::Shown(Some(queue)));
        cx.notify();
    }

    /// Closes the queues: the Inbox shows its suggestions.
    pub fn hide(&mut self, cx: &mut Context<Self>) {
        if self.queue.take().is_some() {
            self.outcome = None;
            cx.emit(ChecksEvent::Shown(None));
            cx.notify();
        }
    }

    /// Opens Balance checks on `account`'s first failing check (the
    /// Accounts panel's flagged dot), else its newest check.
    pub fn show_account(&mut self, account: AccountId, cx: &mut Context<Self>) {
        self.queue = Some(Queue::Balances);
        self.pending_account = Some(account);
        self.reload(cx);
        self.select_account_check(account);
        cx.emit(ChecksEvent::Shown(Some(Queue::Balances)));
        cx.notify();
    }

    /// Selects `account`'s first failing check, else its newest.
    fn select_account_check(&mut self, account: AccountId) {
        let row = self.data.review.account(account).and_then(|review| {
            review
                .rows
                .iter()
                .find(|row| row.check.outcome.is_failed())
                .or_else(|| review.rows.last())
                .map(|row| row.check.balance)
        });
        self.check = row.map(|balance| (account, balance));
    }

    fn select_default(&mut self) {
        match self.queue {
            Some(Queue::Duplicates) if self.selected_pair().is_none() => {
                self.pair = self.data.review.pairs.first().map(|pair| pair.candidate);
                self.swapped = None;
            }
            Some(Queue::Owed) if self.selected_person().is_none() => {
                self.person = self
                    .data
                    .owed
                    .people
                    .first()
                    .map(|person| (person.label.clone(), person.owed.currency()));
            }
            Some(Queue::Balances) if self.selected_check().is_none() => {
                self.check = self.data.review.accounts.iter().find_map(|account| {
                    account
                        .rows
                        .iter()
                        .rev()
                        .find(|row| row.check.outcome.is_failed())
                        .map(|row| (account.account, row.check.balance))
                });
            }
            _ => {}
        }
    }

    // Reading ------------------------------------------------------------------

    /// Reads the queues again on a background thread, keeping the
    /// selection where it still is. A newer read replaces one in flight.
    /// When it lands, a selection that has gone is let go and the pending
    /// selections (`show`, `show_account`, Undo) are made from the new data.
    pub fn reload(&mut self, cx: &mut Context<Self>) {
        let Source::Database(store) = &self.source else {
            self.pending_pair = None;
            self.pending_account = None;
            self.pending_default = false;
            cx.notify();
            return;
        };
        let store = store.clone();
        self.loading = Some(cx.spawn(async move |this, cx| {
            let result = cx
                .background_spawn(async move { store.load() })
                .await
                .map_err(|error| SharedString::from(error.to_string()));
            this.update(cx, |this, cx| {
                this.loading = None;
                this.land(result);
                cx.notify();
            })
            .ok();
        }));
    }

    /// Starts the first read, if it hasn't been asked for.
    pub(crate) fn start_loading(&mut self, cx: &mut Context<Self>) {
        if self.loading.is_none() && matches!(self.source, Source::Database(_)) {
            self.reload(cx);
        }
    }

    /// After their own write, the shell reads the queues again, with every
    /// other view of the ledger ([`crate::shell::live`]).
    pub fn reload_through_shell(&mut self) {
        self.reloads = Reloads::ByShell;
    }

    /// Their own write changed the ledger: the queues read again
    /// themselves, or inside a shell drop the read in flight (it started
    /// before the write) and leave the read to the shell. The pending
    /// selections are made when that read lands.
    fn wrote_ledger(&mut self, cx: &mut Context<Self>) {
        match self.reloads {
            Reloads::Itself => self.reload(cx),
            Reloads::ByShell => {
                self.loading = None;
                cx.emit(Wrote::of([orre_core::watch::Topic::Ledger]));
                cx.notify();
            }
        }
    }

    /// Reads on this thread (tests and the snapshot).
    fn read_now(&mut self) {
        let Source::Database(store) = &self.source else {
            return;
        };
        let result = store
            .load()
            .map_err(|error| SharedString::from(error.to_string()));
        self.land(result);
    }

    /// Takes a read, then fixes the selection against it.
    fn land(&mut self, result: Result<ChecksData, SharedString>) {
        match result {
            Ok(data) => {
                self.data = data;
                self.load_error = None;
            }
            Err(error) => {
                tracing::warn!(%error, "couldn't read the review queues");
                self.load_error = Some(error);
            }
        }
        if self
            .pair
            .is_some_and(|id| self.data.review.pair(id).is_none())
        {
            self.pair = None;
        }
        // A swap is planned against the data it was read with.
        if let Some(swapped) = self.swapped.take()
            && Some(swapped.candidate) == self.pair
        {
            self.swap_to(swapped.default_keep());
        }
        if self.selected_check().is_none() {
            self.check = None;
        }
        if self.selected_person().is_none() {
            self.person = None;
        }
        if let Some(candidate) = self.pending_pair.take()
            && self.data.review.pair(candidate).is_some()
        {
            self.pair = Some(candidate);
            self.swapped = None;
        }
        if let Some(account) = self.pending_account.take() {
            self.select_account_check(account);
        }
        if std::mem::take(&mut self.pending_default) {
            self.select_default();
        }
    }

    fn store(&self) -> Result<&ChecksStore, SharedString> {
        match &self.source {
            Source::Database(store) => Ok(store),
            Source::None => Err("There is no database to change.".into()),
        }
    }

    // Possible duplicates ------------------------------------------------------

    /// The pairs in queue order.
    pub fn pairs(&self) -> &[Pair] {
        &self.data.review.pairs
    }

    /// The selected pair as it would merge now: with its rows swapped when
    /// the user chose to keep the other one.
    pub fn selected_pair(&self) -> Option<&Pair> {
        let id = self.pair?;
        match &self.swapped {
            Some(swapped) if swapped.candidate == id => Some(swapped),
            _ => self.data.review.pair(id),
        }
    }

    /// Selects pair `id` (`None`: nothing), or nothing if it's selected.
    pub fn toggle_pair(&mut self, id: DuplicateCandidateId, cx: &mut Context<Self>) {
        let next = (self.pair != Some(id)).then_some(id);
        self.select_pair(next, cx);
    }

    pub fn select_pair(&mut self, id: Option<DuplicateCandidateId>, cx: &mut Context<Self>) {
        if self.pair != id {
            self.pair = id;
            self.swapped = None;
            cx.notify();
        }
    }

    /// Moves the selected pair (↑↓, [`list::step`]).
    pub fn select_next_pair(&mut self, to: Move, cx: &mut Context<Self>) {
        let order: Vec<DuplicateCandidateId> =
            self.pairs().iter().map(|pair| pair.candidate).collect();
        let next = list::step(&order, self.pair.as_ref(), to);
        if next.is_some() {
            self.select_pair(next, cx);
        }
    }

    /// "Keep this instead" (←→): plans the selected pair keeping the row
    /// it would exclude; again, back to the default.
    pub fn keep_other(&mut self, cx: &mut Context<Self>) {
        let Some(pair) = self.selected_pair() else {
            return;
        };
        let other = pair.rows[1].transaction;
        self.swap_to(other);
        cx.notify();
    }

    fn swap_to(&mut self, keep: TransactionId) {
        let Some(id) = self.pair else {
            return;
        };
        let Some(listed) = self.data.review.pair(id) else {
            return;
        };
        if listed.default_keep() == keep {
            self.swapped = None;
            return;
        }
        let planned = self.store().and_then(|store| {
            store
                .pair(id, keep)
                .map_err(|error| SharedString::from(error.to_string()))
        });
        match planned {
            Ok(pair) => self.swapped = Some(pair),
            Err(message) => self.outcome = Some(Outcome::Refused(message)),
        }
    }

    /// Whether Merge can go ahead for the selected pair: it has a plan and
    /// the row it excludes isn't locked.
    pub fn can_merge(&self) -> bool {
        self.selected_pair().is_some_and(duplicates::can_merge)
    }

    /// Merges the selected pair (Merge, M), keeping the row shown as kept.
    pub fn merge(&mut self, cx: &mut Context<Self>) {
        if !self.can_merge() {
            return;
        }
        let Some(pair) = self.selected_pair() else {
            return;
        };
        let (id, keep) = (pair.candidate, pair.default_keep());
        let message = format!(
            "Merged {}: {} excluded as a duplicate",
            text::name(&pair.rows[0]),
            text::name(&pair.rows[1])
        );
        let next = self.next_pair_after(id);
        let result = self.store().and_then(|store| {
            store
                .merge(id, keep)
                .map_err(|error| SharedString::from(error.to_string()))
        });
        self.outcome = Some(match result {
            Ok(merged) => {
                self.pair = next;
                self.swapped = None;
                Outcome::Done {
                    message: message.into(),
                    undo: Some(Undo::Merge(merged.batch, id)),
                }
            }
            Err(message) => Outcome::Refused(message),
        });
        self.wrote_ledger(cx);
        cx.emit(ChecksEvent::Changed);
    }

    /// "Not duplicates" (N): dismisses the selected pair. It changes no
    /// transaction, so a lock doesn't stop it.
    pub fn dismiss(&mut self, cx: &mut Context<Self>) {
        let Some(pair) = self.selected_pair() else {
            return;
        };
        let id = pair.candidate;
        let message = format!(
            "{} and {} marked as not duplicates",
            text::name(&pair.rows[0]),
            text::name(&pair.rows[1])
        );
        let next = self.next_pair_after(id);
        let result = self.store().and_then(|store| {
            store
                .dismiss(id)
                .map_err(|error| SharedString::from(error.to_string()))
        });
        self.outcome = Some(match result {
            Ok(()) => {
                self.pair = next;
                self.swapped = None;
                Outcome::Done {
                    message: message.into(),
                    undo: Some(Undo::Dismiss(id)),
                }
            }
            Err(message) => Outcome::Refused(message),
        });
        self.wrote_ledger(cx);
    }

    /// The bar's Undo: undoes the last merge or dismissal, and opens the
    /// pair again.
    pub fn undo(&mut self, cx: &mut Context<Self>) {
        let Some(Outcome::Done {
            undo: Some(undo), ..
        }) = self.outcome.clone()
        else {
            return;
        };
        let result = self.store().and_then(|store| {
            match undo {
                Undo::Merge(batch, _) | Undo::Batch(batch) => store.undo(batch).map(drop),
                Undo::Dismiss(id) => store.reopen(id),
            }
            .map_err(|error| SharedString::from(error.to_string()))
        });
        let candidate = match undo {
            Undo::Merge(_, id) | Undo::Dismiss(id) => Some(id),
            Undo::Batch(_) => None,
        };
        match result {
            Ok(()) => {
                self.outcome = None;
                // The pair is selected when the read lands.
                self.pending_pair = candidate;
                self.wrote_ledger(cx);
                if matches!(undo, Undo::Batch(_) | Undo::Merge(..)) {
                    cx.emit(ChecksEvent::Changed);
                }
            }
            Err(message) => {
                self.outcome = Some(Outcome::Refused(message));
                cx.notify();
            }
        }
    }

    /// The pair to open after `id` leaves the queue: the next one, else the
    /// one before.
    fn next_pair_after(&self, id: DuplicateCandidateId) -> Option<DuplicateCandidateId> {
        let order: Vec<DuplicateCandidateId> =
            self.pairs().iter().map(|pair| pair.candidate).collect();
        let at = order.iter().position(|row| *row == id)?;
        order
            .get(at + 1)
            .or_else(|| at.checked_sub(1).and_then(|before| order.get(before)))
            .copied()
    }

    /// Hides the bar.
    pub fn dismiss_outcome(&mut self, cx: &mut Context<Self>) {
        if self.outcome.take().is_some() {
            cx.notify();
        }
    }

    // Balance checks -----------------------------------------------------------

    pub fn accounts(&self) -> &[AccountReview] {
        &self.data.review.accounts
    }

    /// The selected balance and its account.
    pub fn selected_check(&self) -> Option<(&AccountReview, &CheckRow)> {
        let (account, balance) = self.check?;
        let review = self.data.review.account(account)?;
        let row = review
            .rows
            .iter()
            .find(|row| row.check.balance == balance)?;
        Some((review, row))
    }

    pub fn is_check_selected(&self, account: AccountId, balance: BalanceRef) -> bool {
        self.check == Some((account, balance))
    }

    /// Selects a balance, or nothing if it's selected.
    pub fn toggle_check(
        &mut self,
        account: AccountId,
        balance: BalanceRef,
        cx: &mut Context<Self>,
    ) {
        let next = (self.check != Some((account, balance))).then_some((account, balance));
        self.check = next;
        cx.notify();
    }

    /// The balances in table order: accounts as listed, each newest first.
    fn check_order(&self) -> Vec<(AccountId, BalanceRef)> {
        self.accounts()
            .iter()
            .flat_map(|account| {
                account
                    .rows
                    .iter()
                    .rev()
                    .map(move |row| (account.account, row.check.balance))
            })
            .collect()
    }

    /// Moves the selected balance (↑↓, [`list::step`]).
    pub fn select_next_check(&mut self, to: Move, cx: &mut Context<Self>) {
        let order = self.check_order();
        if let Some(next) = list::step(&order, self.check.as_ref(), to) {
            self.check = Some(next);
            cx.notify();
        }
    }

    /// "Review pair": opens the pair in Possible duplicates.
    pub fn review_pair(&mut self, id: DuplicateCandidateId, cx: &mut Context<Self>) {
        self.queue = Some(Queue::Duplicates);
        self.pair = Some(id);
        self.swapped = None;
        cx.emit(ChecksEvent::Shown(Some(Queue::Duplicates)));
        cx.notify();
    }

    /// Clears the selection in the open queue (Escape).
    pub fn clear_selection(&mut self, cx: &mut Context<Self>) {
        match self.queue {
            Some(Queue::Duplicates) => self.select_pair(None, cx),
            Some(Queue::Balances) => {
                self.check = None;
                cx.notify();
            }
            Some(Queue::Owed) => {
                self.person = None;
                cx.notify();
            }
            None => {}
        }
    }

    // Owed to you (M3.11) -------------------------------------------------------

    /// The selected person.
    pub fn selected_person(&self) -> Option<&Person> {
        let (label, currency) = self.person.as_ref()?;
        self.data
            .owed
            .people
            .iter()
            .find(|person| &person.label == label && person.owed.currency() == *currency)
    }

    pub fn is_person_selected(&self, person: &Person) -> bool {
        self.person.as_ref().is_some_and(|(label, currency)| {
            &person.label == label && person.owed.currency() == *currency
        })
    }

    /// Selects a person, or nothing if they're selected.
    pub fn toggle_person(&mut self, label: ShareLabel, currency: Currency, cx: &mut Context<Self>) {
        let next = (label, currency);
        self.person = (self.person.as_ref() != Some(&next)).then_some(next);
        cx.notify();
    }

    /// Moves the selected person (↑↓, [`list::step`]).
    pub fn select_next_person(&mut self, to: Move, cx: &mut Context<Self>) {
        let order: Vec<(ShareLabel, Currency)> = self
            .data
            .owed
            .people
            .iter()
            .map(|person| (person.label.clone(), person.owed.currency()))
            .collect();
        if let Some(next) = list::step(&order, self.person.as_ref(), to) {
            self.person = Some(next);
            cx.notify();
        }
    }

    /// Runs a link edit and shows its undo bar with `message`.
    fn link_edit(
        &mut self,
        message: String,
        cx: &mut Context<Self>,
        edit: impl FnOnce(&ChecksStore) -> Result<Option<ChangeBatchId>, StoreError>,
    ) {
        let result = self
            .store()
            .and_then(|store| edit(store).map_err(|error| SharedString::from(error.to_string())));
        self.outcome = Some(match result {
            Ok(batch) => Outcome::Done {
                message: message.into(),
                undo: batch.map(Undo::Batch),
            },
            Err(message) => Outcome::Refused(message),
        });
        self.wrote_ledger(cx);
        cx.emit(ChecksEvent::Changed);
    }

    /// "Link as reimbursement": confirms a payment's matched claims.
    pub fn link_match(&mut self, links: Vec<LinkId>, message: String, cx: &mut Context<Self>) {
        self.link_edit(message, cx, |store| store.confirm_links(&links));
    }

    /// "Not this": the payment isn't suggested for those claims again.
    pub fn reject_match(&mut self, links: Vec<LinkId>, cx: &mut Context<Self>) {
        self.link_edit(
            "Orre won't suggest that payment again".into(),
            cx,
            |store| store.reject_links(&links),
        );
    }

    /// Renames the selected person (Name): everywhere their name is.
    pub fn rename_person(&mut self, to: &str, cx: &mut Context<Self>) -> Result<(), SharedString> {
        let Some((from, currency)) = self.person.clone() else {
            return Ok(());
        };
        let to = ShareLabel::new(to).map_err(|error| SharedString::from(error.to_string()))?;
        if to == from {
            return Ok(());
        }
        self.store()?
            .rename_person(&from, &to)
            .map_err(|error| SharedString::from(error.to_string()))?;
        self.person = Some((to, currency));
        self.wrote_ledger(cx);
        cx.emit(ChecksEvent::Changed);
        Ok(())
    }

    pub fn request_link_payment(&mut self, cx: &mut Context<Self>) {
        if let Some((label, currency)) = self.person.clone() {
            cx.emit(ChecksEvent::LinkPaymentRequested(label, currency));
        }
    }

    pub fn request_lock(&mut self, account: AccountId, cx: &mut Context<Self>) {
        cx.emit(ChecksEvent::LockRequested(account));
    }

    pub fn request_unlock(&mut self, account: AccountId, cx: &mut Context<Self>) {
        cx.emit(ChecksEvent::UnlockRequested(account));
    }

    pub fn request_add_balance(&mut self, account: AccountId, cx: &mut Context<Self>) {
        cx.emit(ChecksEvent::AddBalanceRequested(account));
    }

    /// Locks `account` through `through` (the lock dialog), or says why
    /// core refused.
    pub fn lock(
        &mut self,
        account: AccountId,
        through: chrono::NaiveDate,
        cx: &mut Context<Self>,
    ) -> Result<(), SharedString> {
        self.store()?
            .lock(account, through)
            .map_err(|error| SharedString::from(error.to_string()))?;
        self.wrote_ledger(cx);
        cx.emit(ChecksEvent::Changed);
        Ok(())
    }

    /// Unlocks `account` (the unlock dialog): the lock is cleared entirely.
    pub fn unlock(
        &mut self,
        account: AccountId,
        cx: &mut Context<Self>,
    ) -> Result<(), SharedString> {
        self.store()?
            .unlock(account)
            .map_err(|error| SharedString::from(error.to_string()))?;
        // Reading again plans the selected pair again, lock lifted.
        self.wrote_ledger(cx);
        cx.emit(ChecksEvent::Changed);
        Ok(())
    }

    /// The account named `account`, for dialogs.
    pub fn account(&self, account: AccountId) -> Option<&AccountReview> {
        self.data.review.account(account)
    }
}

#[cfg(test)]
mod tests;
