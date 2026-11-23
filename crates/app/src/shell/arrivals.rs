//! What arrived from another process, and how long the shell shows it
//! (LU.5, `docs/DESIGN.md` → "Live updates: showing what arrived").
//!
//! A plain model, no GPUI: the shell diffs what a view held before a reload
//! against what it holds after, hands the difference to [`Arrivals::arrive`]
//! as an [`Arrival`], and draws what the queries return. Time is passed in
//! on every call, so tests and snapshots need no clock; the shell keeps one
//! timer, set to [`Arrivals::next_deadline`], and calls [`Arrivals::tick`].
//!
//! Three things follow one rule, each *seen* through its own view
//! ([`Channel`]): the arrival line, the badge's unseen fill and the row
//! marks.
//!
//! 1. *Unseen*: no time limit.
//! 2. *Seen* (the view is on show in a focused window, or the line was
//!    clicked) starts one [`HOLD`], once.
//! 3. *Fading*: [`FADE_OUT`] long, all together.
//! 4. *Gone*.
//!
//! The line is one item for the newest arrival. The marks and the badge are
//! a union per channel: a different arrival replaces the line but never
//! drops the earlier marks or the unseen fill.

use std::collections::{BTreeMap, BTreeSet};
use std::time::{Duration, Instant};

use orre_core::envelope;
use orre_core::id::TransactionId;
use orre_core::inbox::SuggestionId;

/// How long a seen arrival stays before it fades (a delay, not motion: it
/// keeps its length under reduced motion).
pub const HOLD: Duration = Duration::from_secs(10);

/// How long the fade takes: `--motion-base`'s exit. The shell draws it as
/// zero under reduced motion and may then drop the item at once.
pub const FADE_OUT: Duration = Duration::from_millis(100);

/// The longest client name shown, in characters, before "…".
pub const CLIENT_NAME_SHOWN: usize = 24;

/// The view through which an arrival is seen. A click on the line opens
/// the same view.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Channel {
    /// Suggestions: the Inbox.
    Inbox,
    /// Ledger changes, the removed-rows line included: Transactions.
    Transactions,
}

/// The suggestions another process filed, from one reload.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct SuggestionArrival {
    /// How many more suggestions are pending, by the client's display
    /// name. Clients whose count didn't go up are left out.
    pub per_client: BTreeMap<String, usize>,
    /// The new suggestions' ids, as far as the Inbox could tell: a subset
    /// of what `per_client` counts, or empty when its Pending list wasn't
    /// loaded.
    pub new_ids: BTreeSet<SuggestionId>,
}

/// The ledger changes another process made, from one reload.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct LedgerArrival {
    /// Transactions that weren't there before.
    pub new: BTreeSet<TransactionId>,
    /// Transactions whose user-facing fields differ.
    pub changed: BTreeSet<TransactionId>,
    /// How many transactions are gone.
    pub removed: usize,
    /// Whether an import batch went to `undone`.
    pub import_undone: bool,
}

/// What one outside reload brought for one channel. A reload that brought
/// both is two calls to [`Arrivals::arrive`]; the line shows the later.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Arrival {
    Suggestions(SuggestionArrival),
    Ledger(LedgerArrival),
}

impl Arrival {
    /// The view this arrival is seen through.
    pub fn channel(&self) -> Channel {
        match self {
            Arrival::Suggestions(_) => Channel::Inbox,
            Arrival::Ledger(_) => Channel::Transactions,
        }
    }
}

/// What leads the arrival line.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Lead {
    /// A 10px sparkle in `moldavite`: an AI client.
    Sparkle,
    /// A 6px `molten` dot: new or changed rows.
    Dot,
    /// Nothing: rows that are gone.
    None,
}

/// A run of the line's copy.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Span {
    pub text: String,
    /// Drawn in `moldavite-text`: a client's name.
    pub emphasis: bool,
}

/// How far along an item is. Unseen and holding items are both `Shown`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Phase {
    Shown,
    /// Fading out since this instant, for [`FADE_OUT`].
    Fading {
        since: Instant,
    },
}

/// The arrival line as the status bar draws it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ArrivalLine {
    pub lead: Lead,
    pub spans: Vec<Span>,
    /// The view a click opens; `None` when there is nothing to look at.
    pub opens: Option<Channel>,
    pub phase: Phase,
}

impl ArrivalLine {
    /// The copy as one string.
    pub fn text(&self) -> String {
        self.spans.iter().map(|span| span.text.as_str()).collect()
    }
}

/// The ids to mark in a view, and how far along the marks are.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Marked<'a, Id> {
    pub ids: &'a BTreeSet<Id>,
    pub phase: Phase,
}

/// Where an item is in its life.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Stage {
    Unseen,
    /// Seen at this instant.
    Holding(Instant),
    /// Fading since this instant.
    Fading(Instant),
}

impl Stage {
    /// Starts the hold if the item is unseen.
    fn see(&mut self, now: Instant) {
        if *self == Stage::Unseen {
            *self = Stage::Holding(now);
        }
    }

    fn deadline(self) -> Option<Instant> {
        match self {
            Stage::Unseen => None,
            Stage::Holding(since) => Some(since + HOLD),
            Stage::Fading(since) => Some(since + FADE_OUT),
        }
    }

    /// The stage at `now`, or `None` once gone.
    fn at(mut self, now: Instant) -> Option<Stage> {
        loop {
            match self {
                Stage::Holding(since) if now >= since + HOLD => {
                    self = Stage::Fading(since + HOLD);
                }
                Stage::Fading(since) if now >= since + FADE_OUT => return None,
                _ => return Some(self),
            }
        }
    }

    fn phase(self) -> Phase {
        match self {
            Stage::Unseen | Stage::Holding(_) => Phase::Shown,
            Stage::Fading(since) => Phase::Fading { since },
        }
    }
}

/// What the line says, and what it adds up.
#[derive(Clone, Debug, PartialEq, Eq)]
enum LineKind {
    Suggestions(BTreeMap<String, usize>),
    New(usize),
    Changed(usize),
    Removed { count: usize, import_undone: bool },
}

impl LineKind {
    fn channel(&self) -> Channel {
        match self {
            LineKind::Suggestions(_) => Channel::Inbox,
            _ => Channel::Transactions,
        }
    }

    /// This line with `more` of the same kind from the same source added,
    /// or `None` if `more` is a different arrival.
    fn merged(&self, more: &LineKind) -> Option<LineKind> {
        match (self, more) {
            (LineKind::Suggestions(a), LineKind::Suggestions(b)) if a.keys().eq(b.keys()) => {
                Some(LineKind::Suggestions(
                    a.iter()
                        .map(|(name, n)| (name.clone(), n + b[name]))
                        .collect(),
                ))
            }
            (LineKind::New(a), LineKind::New(b)) => Some(LineKind::New(a + b)),
            (LineKind::Changed(a), LineKind::Changed(b)) => Some(LineKind::Changed(a + b)),
            (
                LineKind::Removed {
                    count: a,
                    import_undone: x,
                },
                LineKind::Removed {
                    count: b,
                    import_undone: y,
                },
            ) => Some(LineKind::Removed {
                count: a + b,
                import_undone: *x || *y,
            }),
            _ => None,
        }
    }

    fn view(&self, phase: Phase) -> ArrivalLine {
        let plain = |text: String| Span {
            text,
            emphasis: false,
        };
        let noun = |n: usize| {
            if n == 1 {
                "transaction"
            } else {
                "transactions"
            }
        };
        let (lead, spans, opens) = match self {
            LineKind::Suggestions(clients) => {
                let total: usize = clients.values().sum();
                let noun = if total == 1 {
                    "suggestion"
                } else {
                    "suggestions"
                };
                let mut spans = vec![plain(format!("{total} {noun} from "))];
                if clients.len() == 1 {
                    let name = clients.keys().next().map_or("", String::as_str);
                    spans.push(Span {
                        text: client_name(name),
                        emphasis: true,
                    });
                } else {
                    spans.push(plain(format!("{} AI clients", clients.len())));
                }
                (Lead::Sparkle, spans, Some(Channel::Inbox))
            }
            LineKind::New(n) => (
                Lead::Dot,
                vec![plain(format!("{n} new {}", noun(*n)))],
                Some(Channel::Transactions),
            ),
            LineKind::Changed(n) => (
                Lead::Dot,
                vec![plain(format!("{n} {} changed", noun(*n)))],
                Some(Channel::Transactions),
            ),
            LineKind::Removed {
                count,
                import_undone,
            } => {
                let tail = if *import_undone {
                    " · import undone"
                } else {
                    ""
                };
                (
                    Lead::None,
                    vec![plain(format!("{count} {} removed{tail}", noun(*count)))],
                    None,
                )
            }
        };
        ArrivalLine {
            lead,
            spans,
            opens,
            phase,
        }
    }
}

/// A client's display name as the line shows it: plain text on one line,
/// cut at [`CLIENT_NAME_SHOWN`] characters with "…".
fn client_name(name: &str) -> String {
    let shown = envelope::shown(name, CLIENT_NAME_SHOWN);
    if shown.cut > 0 {
        format!("{}…", shown.text)
    } else {
        shown.text
    }
}

#[derive(Debug)]
struct Line {
    kind: LineKind,
    stage: Stage,
}

/// The marks of one channel and, for the Inbox, the badge's unseen fill.
/// One stage for all of them.
#[derive(Debug)]
struct Marks<Id> {
    ids: BTreeSet<Id>,
    /// Only the Inbox's: the count went up while it wasn't on show.
    badge_unseen: bool,
    stage: Stage,
}

impl<Id: Ord> Marks<Id> {
    fn new() -> Self {
        Marks {
            ids: BTreeSet::new(),
            badge_unseen: false,
            stage: Stage::Unseen,
        }
    }

    /// Whether anything is left to show.
    fn is_live(&self) -> bool {
        self.badge_unseen || !self.ids.is_empty()
    }

    /// Restarts the stage for an arrival: a hold from `now` if it was seen,
    /// else unseen (the earlier marks wait with it).
    fn arrived(&mut self, seen_now: bool, now: Instant) {
        self.stage = if seen_now {
            Stage::Holding(now)
        } else {
            Stage::Unseen
        };
    }

    /// Moves the stage to `now`; clears everything once faded. Whether
    /// anything changed.
    fn advance(&mut self, now: Instant) -> bool {
        match self.stage.at(now) {
            Some(stage) => {
                let changed = stage != self.stage;
                self.stage = stage;
                changed
            }
            None => {
                let was_live = self.is_live();
                *self = Marks::new();
                was_live
            }
        }
    }

    fn deadline(&self) -> Option<Instant> {
        self.stage.deadline().filter(|_| self.is_live())
    }
}

/// What arrived from other processes and how long each part stays. See the
/// module docs.
#[derive(Debug)]
pub struct Arrivals {
    line: Option<Line>,
    inbox: Marks<SuggestionId>,
    transactions: Marks<TransactionId>,
}

impl Default for Arrivals {
    fn default() -> Self {
        Self::new()
    }
}

impl Arrivals {
    pub fn new() -> Self {
        Arrivals {
            line: None,
            inbox: Marks::new(),
            transactions: Marks::new(),
        }
    }

    /// Takes in what a reload nobody asked for brought. `seen_now` is true
    /// when the arrival's channel is on show in a focused window: it is
    /// seen at once, starting its hold, and the badge doesn't turn unseen
    /// for it.
    ///
    /// A ledger arrival's line is the first of: new transactions, removed
    /// transactions, changed transactions; its marks are the new and the
    /// changed ones all the same. An arrival that brought nothing is
    /// ignored.
    ///
    /// The line: while it is unseen, an arrival of the same kind from the
    /// same source (the same clients) adds to its count; any other arrival
    /// replaces it, as does any arrival after the line was seen.
    ///
    /// The marks and the badge: the new ids join the earlier ones. If the
    /// arrival isn't seen the channel is unseen again; if it is, a hold
    /// starts, or restarts if one was running.
    pub fn arrive(&mut self, arrival: Arrival, seen_now: bool, now: Instant) {
        let kind = match arrival {
            Arrival::Suggestions(arrival) => {
                let per_client: BTreeMap<String, usize> = arrival
                    .per_client
                    .into_iter()
                    .filter(|(_, count)| *count > 0)
                    .collect();
                if per_client.is_empty() {
                    return;
                }
                self.inbox.ids.extend(arrival.new_ids);
                self.inbox.badge_unseen |= !seen_now;
                self.inbox.arrived(seen_now, now);
                LineKind::Suggestions(per_client)
            }
            Arrival::Ledger(arrival) => {
                let kind = if !arrival.new.is_empty() {
                    LineKind::New(arrival.new.len())
                } else if arrival.removed > 0 {
                    LineKind::Removed {
                        count: arrival.removed,
                        import_undone: arrival.import_undone,
                    }
                } else if !arrival.changed.is_empty() {
                    LineKind::Changed(arrival.changed.len())
                } else {
                    return;
                };
                self.transactions.ids.extend(arrival.new);
                self.transactions.ids.extend(arrival.changed);
                self.transactions.arrived(seen_now, now);
                kind
            }
        };
        let merged = self
            .line
            .as_ref()
            .filter(|line| line.stage == Stage::Unseen)
            .and_then(|line| line.kind.merged(&kind));
        self.line = Some(Line {
            kind: merged.unwrap_or(kind),
            stage: if seen_now {
                Stage::Holding(now)
            } else {
                Stage::Unseen
            },
        });
    }

    /// `channel`'s view came on show in a focused window. Starts the hold
    /// of its marks, its badge and, if the line is its, the line. Items
    /// already seen keep their hold: calling this again restarts nothing.
    pub fn seen(&mut self, channel: Channel, now: Instant) {
        match channel {
            Channel::Inbox => self.inbox.stage.see(now),
            Channel::Transactions => self.transactions.stage.see(now),
        }
        if let Some(line) = &mut self.line
            && line.kind.channel() == channel
        {
            line.stage.see(now);
        }
    }

    /// The line was clicked: seen through the line's channel.
    pub fn clicked(&mut self, now: Instant) {
        if let Some(channel) = self.line.as_ref().map(|line| line.kind.channel()) {
            self.seen(channel, now);
        }
    }

    /// Moves every item along to `now`, dropping what has faded out.
    /// Whether anything changed (a phase, or an item gone).
    pub fn tick(&mut self, now: Instant) -> bool {
        let mut changed = false;
        if let Some(line) = &mut self.line {
            match line.stage.at(now) {
                Some(stage) => {
                    changed |= stage != line.stage;
                    line.stage = stage;
                }
                None => {
                    self.line = None;
                    changed = true;
                }
            }
        }
        changed |= self.inbox.advance(now);
        changed |= self.transactions.advance(now);
        changed
    }

    /// When the next phase changes, for one timer; `None` while everything
    /// is unseen or gone.
    pub fn next_deadline(&self) -> Option<Instant> {
        [
            self.line.as_ref().and_then(|line| line.stage.deadline()),
            self.inbox.deadline(),
            self.transactions.deadline(),
        ]
        .into_iter()
        .flatten()
        .min()
    }

    /// The arrival line, if there is one.
    pub fn line(&self) -> Option<ArrivalLine> {
        self.line
            .as_ref()
            .map(|line| line.kind.view(line.stage.phase()))
    }

    /// The Inbox badge's unseen fill and its phase, if it is unseen.
    pub fn inbox_badge_unseen(&self) -> Option<Phase> {
        self.inbox.badge_unseen.then(|| self.inbox.stage.phase())
    }

    /// The new suggestions to mark, if any.
    pub fn suggestion_marks(&self) -> Option<Marked<'_, SuggestionId>> {
        (!self.inbox.ids.is_empty()).then(|| Marked {
            ids: &self.inbox.ids,
            phase: self.inbox.stage.phase(),
        })
    }

    /// The new and changed transactions to mark, if any.
    pub fn transaction_marks(&self) -> Option<Marked<'_, TransactionId>> {
        (!self.transactions.ids.is_empty()).then(|| Marked {
            ids: &self.transactions.ids,
            phase: self.transactions.stage.phase(),
        })
    }
}

#[cfg(test)]
mod tests;
