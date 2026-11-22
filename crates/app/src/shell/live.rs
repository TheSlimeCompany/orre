//! Live updates in the shell (ROADMAP LU.3; `docs/architecture/app.md` →
//! "Decision record: live updates (2026-11-21)" and "Decision record: live
//! updates in the shell (2026-11-22)").
//!
//! The shell keeps an [`orre_core::watch::Watcher`] on the database and
//! reloads the views whose [`Topic`] changed, whoever changed it: another
//! process (an MCP client, `orre import` in a terminal) or the app itself.
//!
//! - **The baseline** is taken before the window opens, off the UI thread
//!   ([`baseline`], in [`super::open`]), so it comes before any view's first
//!   read and a write landing between the two is reported. If it couldn't
//!   be taken, the first check that compares reloads every view.
//! - **Checks** run on a background thread, once a second while the window
//!   is active and every five seconds while it isn't, with one at once when
//!   it becomes active ([`poll_interval`]). Only [`super::open`] starts the
//!   timer: tests and the snapshot example never poll. A check's topics are
//!   reloaded with no rebase: the watcher has already taken them.
//! - **The app's own writes** reach the shell as [`Wrote`]: a view inside a
//!   shell ([`Reloads::ByShell`]) drops its read in flight and names the
//!   topics it changed, rather than reloading itself. The shell's `refresh`
//!   rebases those topics on a background thread, then reloads every view
//!   of them ([`views`]). That is the one
//!   place [`Watcher::rebase`] is called, so every rebase of a topic is
//!   followed by a read of every view of it started after the rebase, and a
//!   write another process made before the rebase is in those reads. Rounds
//!   are coalesced: topics named while one is in flight wait for the next,
//!   and a round is never cancelled once its rebase has begun. A view
//!   outside a shell (its own tests) reloads itself ([`Reloads::Itself`]).
//! - **The app's own restore** pauses the checks (the restored file would
//!   report every topic); when it's done, every topic is refreshed, which
//!   also takes the restored file's identity, and the checks resume when
//!   that round's rebase has finished. A failed restore resumes at once.
//!
//! Errors from a check or a rebase are logged at debug, once per run of
//! failures, and never with the database's contents: the next tick tries
//! again.

#[cfg(test)]
mod tests;

use std::collections::BTreeSet;
use std::path::Path;
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::Duration;

use gpui_kit::{AppContext as _, Context, Subscription, Task, Window};
use orre_core::watch::{Check, Topic, Watcher};

use super::{ShellView, View};

/// How often the database is checked while the window is active.
const ACTIVE_TICK: Duration = Duration::from_secs(1);

/// How often it's checked while the window isn't.
const INACTIVE_TICK: Duration = Duration::from_secs(5);

/// A view's own write changed these topics: the shell rebases them and
/// reloads every view of them (module docs).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Wrote(pub BTreeSet<Topic>);

impl Wrote {
    pub fn of(topics: impl IntoIterator<Item = Topic>) -> Self {
        Self(topics.into_iter().collect())
    }
}

/// Who reloads a view after its own write.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Reloads {
    /// The view, at once: outside a shell.
    #[default]
    Itself,
    /// The shell, after it rebased the topics the view names ([`Wrote`]).
    ByShell,
}

/// The views a change of some topics reloads.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Views {
    /// The Inbox, and with it the tab's badge.
    pub inbox: bool,
    pub transactions: bool,
    /// The accounts panel, and the status bar's balance and import line.
    pub accounts: bool,
    /// The review queues, and the status bar's failing checks.
    pub checks: bool,
    pub rules: bool,
    /// Settings' AI clients, and the status bar's agent count.
    pub settings: bool,
}

/// Which views reload when `topics` changed (decision record "live
/// updates", point 2): the Inbox on Inbox; Transactions on Ledger, and on
/// Rules for its merchant names; Accounts and Checks on Ledger; Rules on
/// Rules, and on Ledger (its counts) only while it's the view on show
/// (`rules_on_show`), since showing it reloads it; Settings' AI clients on
/// Settings.
pub fn views(topics: &BTreeSet<Topic>, rules_on_show: bool) -> Views {
    let ledger = topics.contains(&Topic::Ledger);
    let rules = topics.contains(&Topic::Rules);
    Views {
        inbox: topics.contains(&Topic::Inbox),
        transactions: ledger || rules,
        accounts: ledger,
        checks: ledger,
        rules: rules || (ledger && rules_on_show),
        settings: topics.contains(&Topic::Settings),
    }
}

/// How long the timer waits between checks.
pub fn poll_interval(active: bool) -> Duration {
    if active { ACTIVE_TICK } else { INACTIVE_TICK }
}

/// A watcher whose baseline was taken, or tried: what [`super::open`]
/// hands the shell.
pub struct Watched {
    watcher: Watcher,
    /// Whether the baseline compared. If not (no database file yet, a
    /// restore held the lock, an error), the views may have read before it,
    /// so the first check that compares reloads every view.
    baselined: bool,
}

/// Takes the baseline of the database at `path`: blocking, so off the UI
/// thread, and before any view reads.
pub fn baseline(path: &Path) -> Watched {
    let mut watcher = Watcher::new(path);
    let baselined = match watcher.check() {
        Ok(check) => matches!(check, Check::Changed(_)),
        Err(error) => {
            tracing::debug!(%error, "couldn't take the live-updates baseline");
            false
        }
    };
    Watched { watcher, baselined }
}

/// The shell's live updates (module docs).
pub(super) struct Live {
    /// Shared with the background check or rebase in flight; they take
    /// turns.
    watcher: Arc<Mutex<Watcher>>,
    baselined: bool,
    /// Whether the window is active: the timer's interval.
    active: bool,
    /// While the app's own restore runs, nothing is checked.
    paused: bool,
    /// The next round that starts resumes the checks when its rebase is
    /// done: set by a restore, whose every topic that round holds.
    resume_with_next_round: bool,
    /// Topics named since the round in flight started.
    pending: BTreeSet<Topic>,
    /// The round in flight: a rebase, then the reloads. Never replaced
    /// while it runs.
    round: Option<Task<()>>,
    /// The check in flight.
    checking: Option<Task<()>>,
    timer: Option<Task<()>>,
    /// Whether the last check or rebase failed: a run of failures logs once.
    failing: bool,
    _activation: Option<Subscription>,
    /// What the last check reported, after the baseline rule (tests).
    #[cfg(test)]
    last_reported: Option<BTreeSet<Topic>>,
}

fn lock(watcher: &Mutex<Watcher>) -> MutexGuard<'_, Watcher> {
    // A panic in a check leaves the watcher as it was: its state is only
    // replaced at the end of a check or rebase that worked.
    watcher
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

impl ShellView {
    /// Keeps `watched` from now on: checks and the app's own writes reload
    /// the views (module docs). Starts no timer ([`Self::keep_watching`]).
    pub fn watch(&mut self, watched: Watched, cx: &mut Context<Self>) {
        self.live = Some(Live {
            watcher: Arc::new(Mutex::new(watched.watcher)),
            baselined: watched.baselined,
            active: true,
            paused: false,
            resume_with_next_round: false,
            pending: BTreeSet::new(),
            round: None,
            checking: None,
            timer: None,
            failing: false,
            _activation: None,
            #[cfg(test)]
            last_reported: None,
        });
        cx.notify();
    }

    /// Checks the database on a timer from now on, faster while the window
    /// is active, and at once when it becomes active (only [`super::open`]).
    pub fn keep_watching(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let activation = cx.observe_window_activation(window, |this, window, cx| {
            this.activation_changed(window.is_window_active(), cx)
        });
        let Some(live) = self.live.as_mut() else {
            return;
        };
        live.active = window.is_window_active();
        live._activation = Some(activation);
        self.restart_timer(cx);
    }

    /// The window became active or stopped being: the timer follows, and
    /// becoming active checks at once.
    pub(super) fn activation_changed(&mut self, active: bool, cx: &mut Context<Self>) {
        let Some(live) = self.live.as_mut() else {
            return;
        };
        if live.active == active {
            return;
        }
        live.active = active;
        if live.timer.is_some() {
            self.restart_timer(cx);
        }
        if active {
            self.check_changes(cx);
        }
    }

    fn restart_timer(&mut self, cx: &mut Context<Self>) {
        let Some(live) = self.live.as_mut() else {
            return;
        };
        let interval = poll_interval(live.active);
        live.timer = Some(cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor().timer(interval).await;
                if this.update(cx, |this, cx| this.check_changes(cx)).is_err() {
                    return;
                }
            }
        }));
    }

    /// Asks the watcher what changed, on a background thread, and reloads
    /// the views of what did. Nothing while paused or while a check is in
    /// flight.
    pub(super) fn check_changes(&mut self, cx: &mut Context<Self>) {
        let Some(live) = self.live.as_mut() else {
            return;
        };
        if live.paused || live.checking.is_some() {
            return;
        }
        let watcher = live.watcher.clone();
        live.checking = Some(cx.spawn(async move |this, cx| {
            let result = cx
                .background_spawn(async move { lock(&watcher).check() })
                .await;
            this.update(cx, |this, cx| this.checked(result, cx)).ok();
        }));
    }

    /// Takes a check's answer. Whatever it reports is reloaded, even if a
    /// restore started meanwhile: the watcher has taken those topics, so
    /// dropping the reload would lose them.
    fn checked(&mut self, result: Result<Check, orre_core::watch::Error>, cx: &mut Context<Self>) {
        let Some(live) = self.live.as_mut() else {
            return;
        };
        live.checking = None;
        let topics = match result {
            Ok(Check::Changed(_)) if !live.baselined => {
                live.baselined = true;
                Topic::ALL.into_iter().collect()
            }
            Ok(check) => check.topics(),
            Err(error) => {
                if !live.failing {
                    tracing::debug!(%error, "couldn't check the database for changes");
                }
                live.failing = true;
                return;
            }
        };
        live.failing = false;
        #[cfg(test)]
        {
            live.last_reported = Some(topics.clone());
        }
        if !topics.is_empty() {
            self.reload_views(&topics, cx);
        }
    }

    /// The app's own write changed `topics`: rebase them, then reload every
    /// view of them (module docs). Without a watcher (tests), the views
    /// reload at once.
    pub(super) fn refresh(&mut self, topics: BTreeSet<Topic>, cx: &mut Context<Self>) {
        let Some(live) = self.live.as_mut() else {
            self.reload_views(&topics, cx);
            return;
        };
        live.pending.extend(topics);
        self.start_round(cx);
    }

    /// Starts a round for the pending topics, unless one is in flight (it
    /// starts the next when it ends).
    fn start_round(&mut self, cx: &mut Context<Self>) {
        let Some(live) = self.live.as_mut() else {
            return;
        };
        if live.round.is_some() || live.pending.is_empty() {
            return;
        }
        let topics = std::mem::take(&mut live.pending);
        let resumes = std::mem::take(&mut live.resume_with_next_round);
        let watcher = live.watcher.clone();
        let rebased: Vec<Topic> = topics.iter().copied().collect();
        live.round = Some(cx.spawn(async move |this, cx| {
            let result = cx
                .background_spawn(async move { lock(&watcher).rebase(&rebased) })
                .await;
            this.update(cx, |this, cx| this.end_round(&topics, resumes, result, cx))
                .ok();
        }));
    }

    /// After a round's rebase: every view of its topics reads again, and
    /// the next round starts.
    fn end_round(
        &mut self,
        topics: &BTreeSet<Topic>,
        resumes: bool,
        result: Result<(), orre_core::watch::Error>,
        cx: &mut Context<Self>,
    ) {
        if let Some(live) = self.live.as_mut() {
            live.round = None;
            match result {
                Ok(()) => live.failing = false,
                Err(error) => {
                    // The watcher kept what it had: the next check reports
                    // the topics again, which is a reload too many, not one
                    // missed.
                    if !live.failing {
                        tracing::debug!(%error, "couldn't rebase the live-updates watcher");
                    }
                    live.failing = true;
                }
            }
            if resumes {
                live.paused = false;
            }
        }
        self.reload_views(topics, cx);
        self.start_round(cx);
    }

    /// Reloads the views of `topics` ([`views`]).
    fn reload_views(&mut self, topics: &BTreeSet<Topic>, cx: &mut Context<Self>) {
        let views = views(topics, self.view == View::Rules);
        if views.inbox {
            self.inbox.update(cx, |inbox, cx| inbox.reload(cx));
        }
        if views.transactions {
            self.transactions
                .update(cx, |transactions, cx| transactions.reload(cx));
        }
        if views.accounts {
            self.accounts.update(cx, |accounts, cx| accounts.reload(cx));
        }
        if views.checks {
            self.checks.update(cx, |checks, cx| checks.reload(cx));
        }
        if views.rules {
            self.rules.update(cx, |rules, cx| rules.reload(cx));
        }
        if views.settings {
            self.settings
                .update(cx, |settings, cx| settings.reload_clients(cx));
        }
        cx.notify();
    }

    /// The app's own restore started: no checks until it's done.
    pub(super) fn pause_watching(&mut self) {
        if let Some(live) = self.live.as_mut() {
            live.paused = true;
        }
    }

    /// The app's own restore is done: every topic is refreshed, and the
    /// checks resume when that round's rebase has finished.
    pub(super) fn restored(&mut self, cx: &mut Context<Self>) {
        if let Some(live) = self.live.as_mut() {
            live.resume_with_next_round = true;
        }
        self.refresh(Topic::ALL.into_iter().collect(), cx);
    }

    /// The app's own restore failed: the checks resume, with one at once.
    pub(super) fn restore_failed(&mut self, cx: &mut Context<Self>) {
        if let Some(live) = self.live.as_mut() {
            live.paused = false;
        }
        self.check_changes(cx);
    }
}
