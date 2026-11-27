//! The Overview screen (ROADMAP M4.1; `docs/DESIGN.md` → "Overview" and
//! "Data visualisation", Paper `22V-0`): how this month is going, as the
//! month's Spent, Income and Net against last month, the last 12 months'
//! income and spending, where the month's money went and what recurs in the
//! next 30 days.
//!
//! [`OverviewView`] holds the selected month, the account filter and the
//! [`Overview`] core read for them ([`store::OverviewStore`] opens the
//! database per call; every number is core's, this formats). The read runs on
//! a background thread ([`OverviewView::reload`]): it reads the chart's
//! months, every older line once (for the earliest month with data) and the
//! recurring series, which at 100,000 transactions takes longer than a frame. Until the
//! first read lands the canvas says nothing; a later one replaces what is
//! shown when it arrives, and a month picked meanwhile drops the read in
//! flight. The screen writes nothing, so it never asks the shell to reload
//! anything; the shell reloads it ([`crate::shell::live`]) when the ledger or
//! the categories change.
//!
//! The account filter is the accounts panel's selection, as on Transactions
//! ([`OverviewView::show_accounts`]); the compact agent card counts the
//! Inbox's pending suggestions ([`OverviewView::show_inbox`]) and its Review
//! asks the shell to open the Inbox ([`OverviewEvent`]).
//!
//! Months are core's [`Period`]s: the switcher steps with `previous` and
//! `next`, and stops at the earliest month with data and at the current
//! month. The screen opens on the current month, or on the latest month with
//! data when the current one has none yet (the statements lag), and keeps
//! doing so on each read until the user picks a month. Without a data folder
//! the screen says so ([`OverviewView::unavailable`]); tests and snapshots
//! use `fixture`'s sample data (`dev-samples`).

#[cfg(any(test, feature = "dev-driver"))]
pub(crate) mod describe;
#[cfg(any(test, feature = "dev-samples"))]
pub mod fixture;
pub mod store;
pub mod text;
pub mod view;

use chrono::NaiveDate;
use gpui_kit::{AppContext as _, Context, Entity, EventEmitter, SharedString, Subscription, Task};
use orre_core::id::AccountId;
use orre_core::reporting::Period;
use orre_core::reporting::overview::{Overview, OverviewQuery};
use orre_core::reporting::period::MonthStart;

use crate::accounts::{Accounts, AccountsEvent};
use crate::inbox::Inbox;
use store::OverviewStore;

/// Where the screen's data comes from.
enum Source {
    /// The database.
    Database(OverviewStore),
    /// Sample data: `fixture`'s functions answer the query as core would
    /// (tests, `dev-samples`).
    #[cfg(any(test, feature = "dev-samples"))]
    Sample(fn(&OverviewQuery) -> Overview),
    /// Nothing to read, and why (no data folder).
    Unavailable(SharedString),
}

/// What the screen asks its owner (the shell) to do.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OverviewEvent {
    /// "Review" on the agent card: open the Inbox.
    ReviewRequested,
}

impl EventEmitter<OverviewEvent> for OverviewView {}

/// The Overview screen's state (module docs).
pub struct OverviewView {
    source: Source,
    /// The current day: ends the chart's window and starts the recurring
    /// list.
    today: NaiveDate,
    /// Whether `today` is read from the clock on each read
    /// ([`OverviewView::unloaded`], the app), so a window left open across
    /// midnight moves on; tests and the sample data keep their day.
    follows_clock: bool,
    /// The month the switcher shows.
    month: Period,
    /// Whether the user picked `month`. Until they do, a read of the
    /// database opens on the latest month with data
    /// ([`landing_month`](orre_core::reporting::overview::landing_month)),
    /// which is today's unless the statements lag.
    browsed: bool,
    account: Option<AccountId>,
    /// What the last read brought; `None` until the first one lands.
    data: Option<Overview>,
    load_error: Option<SharedString>,
    /// The read in flight; replacing it drops a stale one.
    loading: Option<Task<()>>,
    /// The Inbox, whose pending counts the agent card shows.
    inbox: Option<Entity<Inbox>>,
    _subscriptions: Vec<Subscription>,
}

impl OverviewView {
    fn with_source(source: Source, today: NaiveDate) -> Self {
        Self {
            source,
            today,
            follows_clock: false,
            month: Period::containing(today, MonthStart::CALENDAR),
            browsed: false,
            account: None,
            data: None,
            load_error: None,
            loading: None,
            inbox: None,
            _subscriptions: Vec::new(),
        }
    }

    /// The sample data on the sample day, read now.
    #[cfg(any(test, feature = "dev-samples"))]
    pub fn fixture() -> Self {
        Self::sample(fixture::sample)
    }

    /// `sample`'s data on the sample day, read now.
    #[cfg(any(test, feature = "dev-samples"))]
    pub fn sample(sample: fn(&OverviewQuery) -> Overview) -> Self {
        let mut view = Self::with_source(Source::Sample(sample), fixture::today());
        view.data = Some(sample(&view.query()));
        view
    }

    /// Nothing to show, and `reason` instead: no data folder. Never sample
    /// data (W1-3).
    pub fn unavailable(reason: impl Into<SharedString>) -> Self {
        let reason = reason.into();
        let mut view = Self::with_source(Source::Unavailable(reason.clone()), crate::today());
        view.load_error = Some(reason);
        view
    }

    /// The database in `store` as of `today`, read now, on this thread: for
    /// tests and the snapshot. The app uses [`OverviewView::unloaded`].
    pub fn from_store(store: OverviewStore, today: NaiveDate) -> Self {
        let mut view = Self::with_source(Source::Database(store), today);
        let result = view.read().map_err(SharedString::from);
        view.apply(result);
        view
    }

    /// The database in `store` as of `today`, not read yet: the shell starts
    /// the first read ([`OverviewView::reload`]).
    pub fn unloaded(store: OverviewStore, today: NaiveDate) -> Self {
        let mut view = Self::with_source(Source::Database(store), today);
        view.follows_clock = true;
        view
    }

    /// Moves `today` to the clock's day when the view follows the clock.
    /// The current month stays selected if it was: on 1 December a window
    /// left on November moves to December.
    fn follow_clock(&mut self) {
        if !self.follows_clock {
            return;
        }
        if let Ok(now) = orre_core::schedule::local_today() {
            self.follow(now);
        }
    }

    /// Pins the view to `today` and stops it following the clock, so a test
    /// reads as of a fixed day whatever the real date is.
    #[cfg(test)]
    pub(crate) fn hold_day(&mut self, today: NaiveDate) {
        self.follows_clock = false;
        self.follow(today);
    }

    /// Moves `today` to `now`, keeping the current month selected if it was.
    fn follow(&mut self, now: NaiveDate) {
        if now != self.today {
            let start = self.month_start();
            if self.month == Period::containing(self.today, start) {
                self.month = Period::containing(now, start);
            }
            self.today = now;
        }
    }

    /// Filters by `accounts`' selection, as Transactions does.
    pub fn show_accounts(&mut self, accounts: Entity<Accounts>, cx: &mut Context<Self>) {
        self.account = accounts.read(cx).selected();
        self._subscriptions
            .push(cx.subscribe(&accounts, |this, _, event, cx| {
                let AccountsEvent::Selected(account) = event else {
                    return;
                };
                if this.account != *account {
                    this.account = *account;
                    this.reload(cx);
                }
            }));
    }

    /// Counts `inbox`'s pending suggestions on the agent card.
    pub fn show_inbox(&mut self, inbox: Entity<Inbox>, cx: &mut Context<Self>) {
        self._subscriptions
            .push(cx.observe(&inbox, |_, _, cx| cx.notify()));
        self.inbox = Some(inbox);
    }

    fn query(&self) -> OverviewQuery {
        OverviewQuery {
            month: self.month,
            account: self.account,
            today: self.today,
        }
    }

    /// The overview for the current query, on this thread.
    fn read(&self) -> Result<Overview, String> {
        let query = self.query();
        match &self.source {
            Source::Database(store) => {
                load(store, &query, self.browsed).map_err(|error| error.to_string())
            }
            #[cfg(any(test, feature = "dev-samples"))]
            Source::Sample(sample) => Ok(sample(&query)),
            Source::Unavailable(reason) => Err(reason.to_string()),
        }
    }

    /// Reads the overview again for the selected month and account, on a
    /// background thread. What is shown stays until the read is back; a read
    /// asked for meanwhile replaces this one.
    pub fn reload(&mut self, cx: &mut Context<Self>) {
        self.follow_clock();
        let query = self.query();
        match &self.source {
            #[cfg(any(test, feature = "dev-samples"))]
            Source::Sample(sample) => {
                let result = Ok(sample(&query));
                self.apply(result);
                cx.notify();
            }
            Source::Unavailable(_) => {}
            Source::Database(store) => {
                let store = store.clone();
                let browsed = self.browsed;
                self.loading = Some(cx.spawn(async move |this, cx| {
                    let result = cx
                        .background_spawn(async move { load(&store, &query, browsed) })
                        .await
                        .map_err(|error| SharedString::from(error.to_string()));
                    this.update(cx, |this, cx| {
                        this.apply(result);
                        cx.notify();
                    })
                    .ok();
                }));
            }
        }
    }

    fn apply(&mut self, result: Result<Overview, SharedString>) {
        self.loading = None;
        match result {
            Ok(overview) => {
                if !self.browsed {
                    // The month the read landed on.
                    self.month = overview.selected.period;
                }
                self.data = Some(overview);
                self.load_error = None;
            }
            Err(error) => {
                tracing::warn!(%error, "couldn't read the overview");
                self.load_error = Some(error);
            }
        }
    }

    /// The month the switcher shows.
    pub fn month(&self) -> Period {
        self.month
    }

    /// The account the accounts panel filters by, if any.
    pub fn account(&self) -> Option<AccountId> {
        self.account
    }

    /// What the last read brought, if one has landed.
    pub fn data(&self) -> Option<&Overview> {
        self.data.as_ref()
    }

    pub fn load_error(&self) -> Option<&SharedString> {
        self.load_error.as_ref()
    }

    /// The user's month start as of the last read; calendar months until
    /// the first one lands.
    pub fn month_start(&self) -> MonthStart {
        self.data
            .as_ref()
            .map_or(MonthStart::CALENDAR, |overview| overview.month_start)
    }

    /// The day the screen is as of.
    pub fn today(&self) -> NaiveDate {
        self.today
    }

    /// Shows `month`, reading it. Months after the current one aren't shown.
    pub fn select_month(&mut self, month: Period, cx: &mut Context<Self>) {
        // The day first, so a pick of last month's "current" month after
        // midnight on the 1st isn't moved on by the read's own follow.
        self.follow_clock();
        // Until a month is picked each read lands on the latest month with
        // data, which may not be the one shown. A pick of the shown month
        // while such a read is in flight still replaces it, else it would
        // land on another month under a switcher that has stopped following.
        let landing = !self.browsed && self.loading.is_some();
        self.browsed = true;
        let month = month.min(Period::containing(self.today, self.month_start()));
        if month == self.month && !landing {
            return;
        }
        self.month = month;
        self.reload(cx);
        cx.notify();
    }

    /// The month before the shown one, unless it's before the earliest
    /// month with data (or there is none).
    pub fn previous_month(&self) -> Option<Period> {
        let earliest = self.data.as_ref()?.earliest_month?;
        self.month.previous().filter(|&month| month >= earliest)
    }

    /// The month after the shown one, unless it's after the current month.
    pub fn next_month(&self) -> Option<Period> {
        self.month
            .next()
            .filter(|&month| month <= Period::containing(self.today, self.month_start()))
    }

    pub fn show_previous(&mut self, cx: &mut Context<Self>) {
        if let Some(month) = self.previous_month() {
            self.select_month(month, cx);
        }
    }

    pub fn show_next(&mut self, cx: &mut Context<Self>) {
        if let Some(month) = self.next_month() {
            self.select_month(month, cx);
        }
    }

    /// "Review" on the agent card.
    pub fn request_review(&mut self, cx: &mut Context<Self>) {
        cx.emit(OverviewEvent::ReviewRequested);
    }
}

/// `query` read from `store`: its month as asked once the user has
/// `browsed`, else the month the screen opens on.
fn load(
    store: &OverviewStore,
    query: &OverviewQuery,
    browsed: bool,
) -> Result<Overview, crate::accounts::store::StoreError> {
    if browsed {
        store.load(query)
    } else {
        store.land(query)
    }
}

#[cfg(test)]
mod tests;
