//! The Inbox (ROADMAP M3.6, M3.14; `docs/DESIGN.md` → "Inbox", "Inbox
//! impact preview"; `docs/ARCHITECTURE.md` → "Decision record: Inbox"):
//! the suggestions AI clients (and Orre itself) filed, grouped by kind,
//! each with what approving it would change, row by row, before it can be
//! approved.
//!
//! - [`Inbox`] holds what the screen shows (from [`store::InboxStore`],
//!   which opens the database per call), the filters, the selected
//!   suggestion and the outcome of the last decision. Every decision is
//!   core's ([`orre_core::inbox`]): this only picks the IDs. The
//!   suggestions are read on a background thread ([`Inbox::reload`]): each
//!   pending one is previewed against the whole ledger, which at 100,000
//!   transactions takes far longer than a frame (decision record
//!   "performance at 100,000 transactions", `docs/architecture/app.md`).
//!   Until the first read is back the table says nothing, as the
//!   Transactions table does; later reads keep showing the last one.
//! - [`panel`]: the left panel (INBOX status, CHECKS, KIND, SOURCE), with
//!   its keyboard cursor.
//! - [`view::InboxView`]: the toolbar, the grouped table with the selected
//!   suggestion open in place, and the inspector (`inspector`).
//!
//! Approval happens only here and in the CLI, never over MCP (`AGENTS.md`
//! → Invariants). A rule approved here is applied to past transactions
//! too, never over a category the user set (decision record "Inbox impact
//! preview", point 1), and "Approve all" leaves wide rules out (point 3).
//!
//! Rationales, patterns, names and bank text are untrusted text: shown as
//! plain text, stripped ([`text::plain`]), never acted on.

pub mod arrival;
#[cfg(any(test, feature = "dev-driver"))]
pub(crate) mod describe;
pub mod fixture;
mod inspector;
pub mod panel;
pub mod store;
pub mod text;
pub mod view;

use std::collections::{BTreeMap, BTreeSet};

use gpui_kit::{AppContext as _, Context, EventEmitter, SharedString, Task};
use orre_core::inbox::preview::Covered;
use orre_core::inbox::{ORRE_SOURCE, SuggestionId, SuggestionKind, SuggestionStatus};
use orre_core::watch::Topic;

use crate::shell::live::{Reloads, Wrote};
use crate::ui::list::{self, Move};
use arrival::{Arrived, arrived};
use store::{Entry, InboxData, InboxStore};

/// The order the kinds are listed in, widest reach first (`DESIGN.md` →
/// "Inbox impact preview" → "Kinds and order").
pub const KIND_ORDER: [SuggestionKind; 5] = [
    SuggestionKind::Rule,
    SuggestionKind::MerchantAlias,
    SuggestionKind::TransferPair,
    SuggestionKind::RefundLink,
    SuggestionKind::Categorization,
];

/// Binds the Inbox's keys ([`view::KEYS`]): A approve, R reject, ↑↓
/// select, Escape clears the selection; and the panel's ([`panel::KEYS`]):
/// ↑↓ move its cursor, Enter and Space act on its row. Call once, before
/// the first [`view::InboxView`].
pub fn init(cx: &mut gpui_kit::App) {
    view::KEYS.bind(cx);
    panel::KEYS.bind(cx);
}

/// Where the suggestions come from.
enum Source {
    Database(InboxStore),
    /// Nothing to read (no data folder, or a test without a database).
    None,
}

/// A group of the table: one kind from one source.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct GroupKey {
    pub kind: SuggestionKind,
    pub source: String,
}

/// What the last decision did, shown until the next one.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Outcome {
    /// It went through: what changed, for the confirmation bar.
    Done(SharedString),
    /// Core refused it; nothing changed. Shown where it was asked: under
    /// the inspector's card or the group's buttons.
    Refused { scope: Scope, message: SharedString },
}

/// Where a decision was asked for.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Scope {
    Suggestion(SuggestionId),
    Group(GroupKey),
}

/// The rows marked as new in the suggestion list (ROADMAP LU.5, `DESIGN.md`
/// → "Live updates: showing what arrived" → "Row mark"), as the shell last
/// set them ([`Inbox::set_marks`]).
#[derive(Clone, Debug, Default)]
struct Marks {
    /// Each marked suggestion, with the generation it was first marked in:
    /// the key of its fade-in, so marks already shown don't fade in again
    /// when more are added.
    ids: BTreeMap<SuggestionId, u64>,
    /// The generation the marks started fading out in, if they have: the
    /// key of the fade-out.
    fading: Option<u64>,
    generation: u64,
}

/// The Inbox's state (module docs).
pub struct Inbox {
    source: Source,
    data: InboxData,
    status: SuggestionStatus,
    kind: Option<SuggestionKind>,
    from: Option<String>,
    selected: Option<SuggestionId>,
    /// The pending rule that would settle the selected categorisation,
    /// looked up when it's selected.
    covered: Option<(SuggestionId, Option<Covered>)>,
    outcome: Option<Outcome>,
    load_error: Option<SharedString>,
    /// Whether a read has come back, or there is nothing to read.
    loaded: bool,
    /// The background read in flight; a newer one replaces it.
    loading: Option<Task<()>>,
    /// Who reads the suggestions again after a decision.
    reloads: Reloads,
    /// The rows marked as new.
    marks: Marks,
    /// The highest suggestion id any read has listed: ids only grow, so a
    /// pending suggestion above it arrived since ([`arrival::arrived`]).
    newest_seen: Option<SuggestionId>,
    /// The panel row the keyboard cursor was last drawn on, for
    /// `describe()` (test-only).
    #[cfg(any(test, feature = "dev-driver"))]
    panel_cursor: std::cell::RefCell<Option<panel::InboxPanelRow>>,
}

impl EventEmitter<Wrote> for Inbox {}

/// After a read that replaced loaded data and brought pending suggestions
/// from an AI client: what arrived. The shell decides whether the read was
/// unasked, and so news.
impl EventEmitter<Arrived> for Inbox {}

impl Inbox {
    /// The suggestions in `store`, read now, on this thread: for tests and
    /// the snapshot. The app uses [`Inbox::unloaded`].
    pub fn from_store(store: InboxStore) -> Self {
        let mut inbox = Self::with_source(Source::Database(store));
        inbox.read_now();
        inbox
    }

    /// The suggestions in `store`, not read yet: [`InboxView::new`] starts
    /// the first read, on a background thread.
    ///
    /// [`InboxView::new`]: view::InboxView::new
    pub fn unloaded(store: InboxStore) -> Self {
        Self {
            loaded: false,
            ..Self::with_source(Source::Database(store))
        }
    }

    /// No suggestions, because they can't be read: `reason` says why.
    pub fn unavailable(reason: impl Into<SharedString>) -> Self {
        Self {
            load_error: Some(reason.into()),
            ..Self::with_source(Source::None)
        }
    }

    /// An empty Inbox that reads nothing (tests).
    pub fn empty() -> Self {
        Self::with_source(Source::None)
    }

    fn with_source(source: Source) -> Self {
        Self {
            source,
            data: InboxData::default(),
            status: SuggestionStatus::Pending,
            kind: None,
            from: None,
            selected: None,
            covered: None,
            outcome: None,
            load_error: None,
            loaded: true,
            loading: None,
            reloads: Reloads::Itself,
            marks: Marks::default(),
            newest_seen: None,
            #[cfg(any(test, feature = "dev-driver"))]
            panel_cursor: Default::default(),
        }
    }

    /// Whether the suggestions have been read (or there are none to read):
    /// until then an empty table means "not yet", not "nothing to review".
    pub fn loaded(&self) -> bool {
        self.loaded
    }

    pub fn data(&self) -> &InboxData {
        &self.data
    }

    pub fn status(&self) -> SuggestionStatus {
        self.status
    }

    pub fn kind_filter(&self) -> Option<SuggestionKind> {
        self.kind
    }

    pub fn source_filter(&self) -> Option<&str> {
        self.from.as_deref()
    }

    /// The suggestions marked as new ([`Inbox::set_marks`]), listed or not.
    pub fn marked(&self) -> BTreeSet<SuggestionId> {
        self.marks.ids.keys().copied().collect()
    }

    /// Whether the marks are fading out.
    pub fn marks_fading(&self) -> bool {
        self.marks.fading.is_some()
    }

    /// Marks `marks` as new in the suggestion list, from the shell: a 6px
    /// molten dot beside each listed row. With `fading` the dots fade out
    /// (100ms, `motion::exit(motion::BASE)`) and the shell sets none when
    /// it's done; otherwise a mark fades in (150ms) when it first appears.
    /// Fades are 0ms under `App::reduce_motion`. Only listed rows show a
    /// mark. Marks touch no selection, open suggestion or scroll.
    pub fn set_marks(
        &mut self,
        marks: BTreeSet<SuggestionId>,
        fading: bool,
        cx: &mut Context<Self>,
    ) {
        if self.marked() == marks && self.marks_fading() == fading {
            return;
        }
        self.marks.generation += 1;
        let generation = self.marks.generation;
        let mut kept = std::mem::take(&mut self.marks.ids);
        self.marks.ids = marks
            .into_iter()
            .map(|id| (id, kept.remove(&id).unwrap_or(generation)))
            .collect();
        self.marks.fading = fading.then_some(generation);
        cx.notify();
    }

    /// The generation `id`'s mark was first set in, and the one the marks
    /// started fading out in: the keys of its animations.
    pub(crate) fn mark(&self, id: SuggestionId) -> Option<(u64, Option<u64>)> {
        self.marks
            .ids
            .get(&id)
            .map(|since| (*since, self.marks.fading))
    }

    pub fn selected(&self) -> Option<SuggestionId> {
        self.selected
    }

    pub fn outcome(&self) -> Option<&Outcome> {
        self.outcome.as_ref()
    }

    pub fn load_error(&self) -> Option<&SharedString> {
        self.load_error.as_ref()
    }

    /// The count on the Inbox tab: pending suggestions from AI clients.
    /// Orre's own aren't counted (`DESIGN.md` → "Inbox impact preview").
    pub fn badge(&self) -> usize {
        let own = self
            .data
            .counts
            .by_source
            .get(ORRE_SOURCE)
            .copied()
            .unwrap_or(0);
        self.data.counts.pending.saturating_sub(own)
    }

    /// The covering rule for the selected categorisation, if it's known.
    pub fn covered(&self) -> Option<&Covered> {
        match (&self.covered, self.selected) {
            (Some((id, covered)), Some(selected)) if *id == selected => covered.as_ref(),
            _ => None,
        }
    }

    /// The suggestions the filters show, as table groups: kinds in
    /// [`KIND_ORDER`], then sources by name with Orre's last.
    pub fn groups(&self) -> Vec<(GroupKey, Vec<&Entry>)> {
        let mut groups: Vec<(GroupKey, Vec<&Entry>)> = Vec::new();
        for kind in KIND_ORDER {
            if self.kind.is_some_and(|filter| filter != kind) {
                continue;
            }
            let mut sources: Vec<&str> = self
                .data
                .entries
                .iter()
                .filter(|entry| entry.suggestion.kind == kind)
                .map(|entry| entry.suggestion.source_client.as_str())
                .filter(|source| self.from.as_deref().is_none_or(|from| from == *source))
                .collect();
            sources.sort_by_key(|source| (*source == ORRE_SOURCE, *source));
            sources.dedup();
            for source in sources {
                let entries = self
                    .data
                    .entries
                    .iter()
                    .filter(|entry| {
                        entry.suggestion.kind == kind && entry.suggestion.source_client == source
                    })
                    .collect();
                groups.push((
                    GroupKey {
                        kind,
                        source: source.to_owned(),
                    },
                    entries,
                ));
            }
        }
        groups
    }

    /// The shown suggestions in table order.
    pub fn order(&self) -> Vec<SuggestionId> {
        self.groups()
            .into_iter()
            .flat_map(|(_, entries)| entries.into_iter().map(Entry::id))
            .collect()
    }

    pub fn entry(&self, id: SuggestionId) -> Option<&Entry> {
        self.data.entries.iter().find(|entry| entry.id() == id)
    }

    /// The selected suggestion, if it's shown.
    pub fn selected_entry(&self) -> Option<&Entry> {
        self.selected.and_then(|id| self.entry(id))
    }

    /// Shows the suggestions with `status`.
    pub fn show_status(&mut self, status: SuggestionStatus, cx: &mut Context<Self>) {
        if self.status == status {
            return;
        }
        self.status = status;
        self.selected = None;
        self.outcome = None;
        self.reload(cx);
    }

    /// Shows only `kind`, or every kind if it's shown alone already.
    pub fn toggle_kind(&mut self, kind: SuggestionKind, cx: &mut Context<Self>) {
        self.kind = (self.kind != Some(kind)).then_some(kind);
        self.keep_selection_visible();
        cx.notify();
    }

    /// Shows only `source`, or every source if it's shown alone already.
    pub fn toggle_source(&mut self, source: &str, cx: &mut Context<Self>) {
        self.from = (self.from.as_deref() != Some(source)).then(|| source.to_owned());
        self.keep_selection_visible();
        cx.notify();
    }

    fn keep_selection_visible(&mut self) {
        if self.selected.is_some_and(|id| !self.order().contains(&id)) {
            self.selected = None;
        }
    }

    /// Selects `id` (`None`: nothing), or clears it if it's selected.
    pub fn select(&mut self, id: Option<SuggestionId>, cx: &mut Context<Self>) {
        if self.selected == id {
            return;
        }
        self.selected = id;
        self.lookup_covered();
        cx.notify();
    }

    /// A click on a row: selects it, or clears the selection if it's
    /// selected already.
    pub fn toggle(&mut self, id: SuggestionId, cx: &mut Context<Self>) {
        let next = (self.selected != Some(id)).then_some(id);
        self.select(next, cx);
    }

    /// Moves the selection (↑↓, [`list::step`]): stopping at the ends;
    /// with nothing selected, down selects the first and up the last.
    pub fn select_next(&mut self, to: Move, cx: &mut Context<Self>) {
        if let Some(next) = list::step(&self.order(), self.selected.as_ref(), to) {
            self.select(Some(next), cx);
        }
    }

    /// Looks up the rule that would cover the selected categorisation.
    fn lookup_covered(&mut self) {
        let Some(entry) = self.selected_entry() else {
            return;
        };
        if entry.suggestion.kind != SuggestionKind::Categorization
            || entry.suggestion.status != SuggestionStatus::Pending
        {
            return;
        }
        let id = entry.id();
        if matches!(&self.covered, Some((covered, _)) if *covered == id) {
            return;
        }
        let Source::Database(store) = &self.source else {
            return;
        };
        let covered = store.covered_by(id).unwrap_or_else(|error| {
            tracing::warn!(%error, "couldn't look up a covering rule");
            None
        });
        self.covered = Some((id, covered));
    }

    /// Reads the suggestions again, on a background thread. What is shown
    /// stays until the read is back; a read asked for meanwhile replaces
    /// this one.
    pub fn reload(&mut self, cx: &mut Context<Self>) {
        let Source::Database(store) = &self.source else {
            cx.notify();
            return;
        };
        let store = store.clone();
        let status = self.status;
        self.loading = Some(cx.spawn(async move |this, cx| {
            let result = cx
                .background_spawn(async move { store.load(status) })
                .await
                .map_err(|error| SharedString::from(error.to_string()));
            this.update(cx, |this, cx| {
                this.loading = None;
                this.apply(status, result, cx);
            })
            .ok();
        }));
    }

    /// Starts the first read, if it hasn't been asked for.
    pub(crate) fn start_loading(&mut self, cx: &mut Context<Self>) {
        if !self.loaded && self.loading.is_none() {
            self.reload(cx);
        }
    }

    /// After a decision, the shell reads the suggestions again, with every
    /// other view of what it changed ([`crate::shell::live`]).
    pub fn reload_through_shell(&mut self) {
        self.reloads = Reloads::ByShell;
    }

    /// A decision changed `topics`: the suggestions read again, or inside a
    /// shell the read in flight (started before the decision) is dropped
    /// and the shell reads them.
    fn wrote(&mut self, topics: &[Topic], cx: &mut Context<Self>) {
        match self.reloads {
            Reloads::Itself => self.reload(cx),
            Reloads::ByShell => {
                self.loading = None;
                cx.emit(Wrote::of(topics.iter().copied()));
            }
        }
    }

    /// Reads on this thread (tests and the snapshot).
    fn read_now(&mut self) {
        let Source::Database(store) = &self.source else {
            return;
        };
        let result = store
            .load(self.status)
            .map_err(|error| SharedString::from(error.to_string()));
        self.take(result);
    }

    fn apply(
        &mut self,
        status: SuggestionStatus,
        result: Result<InboxData, SharedString>,
        cx: &mut Context<Self>,
    ) {
        // Showing another status has asked for its own read.
        let arrival = (status == self.status).then(|| self.take(result)).flatten();
        cx.notify();
        if let Some(arrival) = arrival {
            cx.emit(Arrived(arrival));
        }
    }

    /// Shows what a read brought, and returns what it brought that's new
    /// and pending: nothing for the first read or an error.
    fn take(&mut self, result: Result<InboxData, SharedString>) -> Option<arrival::Arrival> {
        self.loaded = true;
        let mut arrival = None;
        match result {
            Ok(data) => {
                let brought = arrived(&self.data, &data, self.newest_seen);
                arrival = (!brought.is_empty()).then_some(brought);
                self.newest_seen = self
                    .newest_seen
                    .into_iter()
                    .chain(data.entries.iter().map(Entry::id))
                    .max();
                self.data = data;
                self.load_error = None;
            }
            Err(error) => {
                tracing::warn!(%error, "couldn't read the inbox");
                self.load_error = Some(error);
            }
        }
        self.covered = None;
        // A suggestion still listed stays selected and open, wherever it
        // moved to, and new ones don't take the selection. One that has
        // gone (decided elsewhere, say `orre inbox` in a terminal) leaves
        // nothing selected, never a neighbour: `A` must only ever decide a
        // suggestion the user picked (ROADMAP LU.4). The user's own decision
        // has already moved the selection on before this read (`decide`).
        self.keep_selection_visible();
        if self.selected.is_some_and(|id| self.entry(id).is_none()) {
            self.selected = None;
        }
        self.lookup_covered();
        arrival
    }

    fn store(&self) -> Result<&InboxStore, SharedString> {
        match &self.source {
            Source::Database(store) => Ok(store),
            Source::None => Err("There is no database to change.".into()),
        }
    }

    /// Approves `id` on its own (its row's ✓, the inspector, A).
    pub fn approve(&mut self, id: SuggestionId, cx: &mut Context<Self>) {
        self.decide(Scope::Suggestion(id), vec![id], true, cx);
    }

    /// Rejects `id` (its row's ✕, the inspector, R).
    pub fn reject(&mut self, id: SuggestionId, cx: &mut Context<Self>) {
        self.decide(Scope::Suggestion(id), vec![id], false, cx);
    }

    /// The suggestions "Approve all" approves in `group`: all but the ones
    /// that need their own approval (wide rules).
    pub fn approvable(&self, group: &GroupKey) -> Vec<SuggestionId> {
        self.groups()
            .into_iter()
            .filter(|(key, _)| key == group)
            .flat_map(|(_, entries)| entries)
            .filter(|entry| !entry.needs_own_approval())
            .map(Entry::id)
            .collect()
    }

    /// Every suggestion shown in `group`.
    pub fn in_group(&self, group: &GroupKey) -> Vec<SuggestionId> {
        self.groups()
            .into_iter()
            .filter(|(key, _)| key == group)
            .flat_map(|(_, entries)| entries.into_iter().map(Entry::id))
            .collect()
    }

    /// "Approve all (n)": every [`approvable`](Self::approvable) one, all
    /// or none.
    pub fn approve_group(&mut self, group: GroupKey, cx: &mut Context<Self>) {
        let ids = self.approvable(&group);
        if !ids.is_empty() {
            self.decide(Scope::Group(group), ids, true, cx);
        }
    }

    /// "Reject all": every suggestion in `group`, all or none.
    pub fn reject_group(&mut self, group: GroupKey, cx: &mut Context<Self>) {
        let ids = self.in_group(&group);
        if !ids.is_empty() {
            self.decide(Scope::Group(group), ids, false, cx);
        }
    }

    fn decide(
        &mut self,
        scope: Scope,
        ids: Vec<SuggestionId>,
        approve: bool,
        cx: &mut Context<Self>,
    ) {
        if self.status != SuggestionStatus::Pending {
            return;
        }
        let next = self.next_after(&ids);
        let message = approve
            .then(|| text::approved_message(&self.data, &ids))
            .flatten();
        let result = self.store().and_then(|store| {
            if approve {
                store.approve(&ids).map(drop)
            } else {
                store.reject(&ids)
            }
            .map_err(|error| SharedString::from(error.to_string()))
        });
        let decided = result.is_ok();
        self.outcome = match result {
            Ok(()) => {
                self.selected = next;
                message.map(|message| Outcome::Done(message.into()))
            }
            Err(message) => Some(Outcome::Refused { scope, message }),
        };
        if !decided {
            self.reload(cx);
        } else if approve {
            // An approval can change transactions (a categorisation, a
            // transfer pair) and rules or names.
            self.wrote(&[Topic::Inbox, Topic::Ledger, Topic::Rules], cx);
        } else {
            self.wrote(&[Topic::Inbox], cx);
        }
    }

    /// The row to select after `ids` leave the list: the next one below
    /// the last of them, else the one above the first.
    fn next_after(&self, ids: &[SuggestionId]) -> Option<SuggestionId> {
        let order = self.order();
        let selected = self.selected?;
        if !ids.contains(&selected) {
            return Some(selected);
        }
        let at = order.iter().position(|id| *id == selected)?;
        order[at..]
            .iter()
            .chain(order[..at].iter().rev())
            .find(|id| !ids.contains(id))
            .copied()
    }

    /// Hides the confirmation bar.
    pub fn dismiss_outcome(&mut self, cx: &mut Context<Self>) {
        if self.outcome.take().is_some() {
            cx.notify();
        }
    }
}

#[cfg(test)]
mod panel_tests;
#[cfg(test)]
mod tests;
