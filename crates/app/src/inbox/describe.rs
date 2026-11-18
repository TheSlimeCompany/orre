//! What the Inbox shows, as plain data, for tests (ROADMAP U.1): the left
//! panel with its keyboard cursor (U.9), the toolbar, the groups and their
//! rows, the selection, the suggestion open
//! under its row, the inspector, the confirmation bar, and the actions
//! available with their keys.
//!
//! Compiled for tests only (`#[cfg(test)]` on the module): it isn't in a
//! release build and nothing outside the tests can reach it, `coffer mcp`
//! included. A path that reads or drives the view's actions would get round
//! "approval happens only in Coffer's own UI" (`AGENTS.md` → Invariants).
//!
//! Every string comes from the code the view draws with
//! ([`super::view`]'s `…Text` types, [`super::inspector`]'s, [`super::text`]),
//! so untrusted text (patterns, names, bank text, rationales) is described
//! exactly as it's shown: through [`super::text::plain`], which is
//! [`coffer_core::envelope::shown`]. Styling (colours, fonts, markers,
//! icons) is left out; mixed runs are joined into one string.
//!
//! The panel's cursor is described as last drawn: the panel notes which
//! row it drew the cursor on, since where it is lives with the panel's
//! element ([`crate::ui::panel_cursor::PanelCursor::keyed`]) and whether
//! it shows depends on the window's focus.
//!
//! What it leaves out: the panel's icons, the pointer's hover (✓ and ✕ show
//! on a hovered row too; the description lists them only through the
//! selection's keys), the inspector's "If you approve" and definition
//! sections, and the lane headings.

use coffer_core::inbox::SuggestionStatus;

pub use crate::ui::describe::Action;

use crate::checks::Checks;

use super::inspector::{self, CardText, HeaderText, NOTHING_SELECTED, header_text};
use super::panel::{self, InboxPanelRow};
use super::store::Entry;
use super::text::{Part, kind_name};
use super::view::{
    self, AmountText, FooterText, ItemText, LineText, MerchantCell, MerchantText, NowText,
    OpenLine, OpenText, RowText, SEVERAL, after_label, category_now_words, now_words,
};
use super::{Inbox, Outcome};

/// What the Inbox shows ([`Inbox::describe`]).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct InboxDescription {
    /// The left panel.
    pub panel: PanelDescription,
    /// The toolbar's title: the status shown ("Pending").
    pub title: &'static str,
    /// The toolbar's caption: "15 suggestions · nothing changes until you
    /// approve".
    pub caption: String,
    /// The keys the toolbar lists, with what they do ([`view::KEYS`]).
    pub toolbar_keys: Vec<(&'static str, &'static str)>,
    /// The empty state's title and body, when no group is listed.
    pub empty: Option<(&'static str, String)>,
    pub groups: Vec<GroupDescription>,
    /// The selected row's label.
    pub selected: Option<String>,
    /// The selected suggestion, open under its row.
    pub open: Option<OpenDescription>,
    pub inspector: InspectorDescription,
    /// The bar after an approval: what changed.
    pub confirmation: Option<String>,
    /// The canvas's keys ([`view::KEYS`]: its own, then ↑↓), and whether
    /// each does anything now.
    pub actions: Vec<Action>,
}

/// The left panel ([`super::panel`]).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PanelDescription {
    /// INBOX, CHECKS, KIND, SOURCE.
    pub sections: Vec<PanelSection>,
    /// The open row's label: the open queue, else the open status. The
    /// cursor starts there.
    pub open: String,
    /// The panel's keys: ↑↓ move the cursor, Enter and Space act on its
    /// row ([`panel::KEYS`]).
    pub keys: Vec<Action>,
    /// Why the suggestions couldn't be read, under the sections.
    pub error: Option<String>,
}

/// A section of the panel: its label and rows.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PanelSection {
    pub title: &'static str,
    pub rows: Vec<PanelRowDescription>,
}

/// A row of the panel.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PanelRowDescription {
    /// Its label, with its suffix: "Coffer from your edits".
    pub label: String,
    pub count: String,
    /// The `danger` dot: a balance check fails.
    pub flagged: bool,
    /// The moldavite dot: an agent with work waiting.
    pub connected: bool,
    /// The open status or queue, or a filter that's on.
    pub selected: bool,
    /// The keyboard cursor is drawn on it (the panel has keyboard focus).
    pub cursor: bool,
}

/// One group of the table: one kind from one source.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GroupDescription {
    /// The kind's name: "Rules", "Merchant names".
    pub kind: &'static str,
    /// The count beside the name.
    pub count: usize,
    pub rows: Vec<RowDescription>,
    /// The footer, shown while suggestions are pending.
    pub footer: Option<FooterDescription>,
}

/// A suggestion's row, lane by lane ("" where a lane is empty).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RowDescription {
    /// The first lane: "Merchant contains UBER *EATS".
    pub label: String,
    /// NOW: the category, name or rule that decides it now.
    pub now: String,
    /// SUGGESTED: what approving sets.
    pub suggested: String,
    pub date: String,
    /// The amount, or the count of rows it changes.
    pub amount: String,
    /// When it was decided, for an approved or rejected one.
    pub decided: Option<String>,
    pub selected: bool,
}

/// A pending group's footer.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FooterDescription {
    /// "Approve all (n)" and "Reject all" ("Approve", "Reject" for one).
    pub actions: Vec<Action>,
    /// Which ones "Approve all" leaves out.
    pub note: Option<String>,
    /// "claude-code · 2h ago", "Coffer · from your edits".
    pub source: String,
    /// Why core refused the last decision on the group.
    pub refused: Option<String>,
}

/// The selected suggestion, open under its row.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OpenDescription {
    /// A wide rule's warning: its title, then its body.
    pub warning: Option<(String, String)>,
    /// The first lane's heading: "Bank text · 6 matches".
    pub header: Option<String>,
    pub lines: Vec<LineDescription>,
    pub caption: String,
}

/// A listed line of an open suggestion, lane by lane.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LineDescription {
    /// Bank text, or a wide rule's merchant label.
    pub text: String,
    pub now: String,
    pub after: String,
    /// The date, or a wide rule's merchant's count.
    pub date: String,
    pub amount: String,
}

/// The inspector.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum InspectorDescription {
    /// Nothing is selected: what it says instead.
    Nothing(&'static str),
    Suggestion(Box<SuggestionDescription>),
}

/// The inspector for the selected suggestion: its header and card.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SuggestionDescription {
    pub title: String,
    /// The display figure, with its tail: "5 of 6", "£250.00".
    pub figure: Option<String>,
    pub caption: Option<String>,
    /// The card's source line: "claude-code · just now".
    pub source: String,
    /// What it proposes: "File UBER *EATS under Eating out".
    pub proposal: String,
    /// The client's reason.
    pub reason: Option<String>,
    /// The card's first red line, while it's pending: "Can't be approved
    /// now: …." (Approve is disabled).
    pub blocked: Option<String>,
    /// Its second: why core refused the last decision on it.
    pub refused: Option<String>,
    /// The card's Approve and Reject, while it's pending.
    pub actions: Vec<Action>,
}

/// Runs joined into the line they're drawn as.
fn joined(parts: &[Part]) -> String {
    parts.iter().map(|part| part.text.as_str()).collect()
}

fn row(entry: &Entry, today: chrono::NaiveDate, selected: bool) -> RowDescription {
    let RowText {
        item,
        now,
        suggested,
        date,
        amount,
    } = RowText::of(entry, today);
    let pending = entry.suggestion.status == SuggestionStatus::Pending;
    RowDescription {
        label: match item {
            ItemText::Rich(parts) => joined(&parts),
            ItemText::Plain(words) | ItemText::Muted(words) => words,
        },
        now: match now {
            NowText::Empty => String::new(),
            NowText::Muted(words) => words,
            NowText::Rich(parts) => joined(&parts),
            NowText::Category(now) => now_words(now),
        },
        suggested: suggested.unwrap_or_default(),
        date: date.unwrap_or_default(),
        amount: match amount {
            Some(AmountText::Count { value, .. }) => value.to_string(),
            Some(AmountText::Money(amount)) => amount,
            None => String::new(),
        },
        decided: (!pending).then(|| super::text::decided_on(entry, today)),
        selected,
    }
}

fn open(entry: &Entry, today: chrono::NaiveDate) -> Option<OpenDescription> {
    let OpenText {
        warning,
        header,
        lines,
        caption,
    } = OpenText::of(entry)?;
    let bank = |(before, matched, after): (String, String, String)| before + &matched + &after;
    let lines = lines
        .into_iter()
        .map(|line| match line {
            OpenLine::Row { line, names } => {
                let text = LineText::of(line, names, today);
                LineDescription {
                    text: bank(text.bank_text),
                    now: text
                        .name
                        .unwrap_or_else(|| now_words(&line.transaction.now)),
                    after: after_label(&line.after, names),
                    date: text.date,
                    amount: text.amount,
                }
            }
            OpenLine::Merchant { line, label } => {
                let text = MerchantText::of(line, label);
                LineDescription {
                    text: match text.merchant {
                        MerchantCell::Label(label) => label,
                        MerchantCell::BankText(parts) => bank(parts),
                    },
                    now: category_now_words(&line.now),
                    after: line
                        .after
                        .as_ref()
                        .map_or_else(|| SEVERAL.to_owned(), |after| after_label(after, false)),
                    date: text.count,
                    amount: text.total.unwrap_or_default(),
                }
            }
        })
        .collect();
    Some(OpenDescription {
        warning: warning.map(|warning| (warning.title, joined(&warning.body))),
        header: header.map(|header| header.first),
        lines,
        caption,
    })
}

fn inspector(inbox: &Inbox) -> InspectorDescription {
    let Some(entry) = inbox.selected_entry() else {
        return InspectorDescription::Nothing(NOTHING_SELECTED);
    };
    let HeaderText {
        title,
        figure,
        caption,
    } = header_text(entry, inspector::today(inbox));
    let CardText {
        name,
        when,
        proposal,
        reason,
        pending,
        blocked,
        refused,
    } = CardText::of(inbox, entry, crate::theme::tokens::color::TEXT_STRONG);
    let actions = if pending {
        vec![
            Action::button("Approve", blocked.is_none()),
            Action::button("Reject", true),
        ]
    } else {
        Vec::new()
    };
    InspectorDescription::Suggestion(Box::new(SuggestionDescription {
        title: title.into_iter().map(|(part, _)| part).collect(),
        figure: figure.map(|(figure, tail)| figure + tail.as_deref().unwrap_or_default()),
        caption,
        source: format!("{name} · {when}"),
        proposal: joined(&proposal),
        reason,
        blocked: blocked.filter(|_| pending),
        refused: refused.map(|message| message.to_string()),
        actions,
    }))
}

/// The canvas's keys, from [`view::KEYS`]: A and R work on a pending
/// suggestion that's selected, Escape while one is selected, ↑↓ while
/// there are rows.
fn actions(pending: bool, selected: bool, has_rows: bool) -> Vec<Action> {
    Action::view_keys(
        &view::KEYS,
        |key| {
            if key.label() == view::CLEAR.label() {
                selected
            } else {
                assert!(
                    [view::APPROVE.label(), view::REJECT.label()].contains(&key.label()),
                    "say when the Inbox's new key {:?} works",
                    key.label()
                );
                pending && selected
            }
        },
        has_rows,
    )
}

/// The left panel, section by section, with the cursor as last drawn.
fn describe_panel(inbox: &Inbox, checks: &Checks) -> PanelDescription {
    let cursor = inbox.panel_cursor.borrow().clone();
    let home = panel::home(inbox, checks);
    let label = |row: &InboxPanelRow| {
        let text = row.text(inbox, checks);
        match text.suffix {
            Some(suffix) => format!("{} {suffix}", text.label),
            None => text.label,
        }
    };
    let sections: Vec<PanelSection> = panel::sections(inbox)
        .into_iter()
        .map(|(title, rows)| PanelSection {
            title,
            rows: rows
                .iter()
                .map(|row| {
                    let text = row.text(inbox, checks);
                    PanelRowDescription {
                        label: label(row),
                        count: text.count.to_string(),
                        flagged: text.flagged,
                        connected: text.connected,
                        selected: text.selected,
                        cursor: cursor.as_ref() == Some(row),
                    }
                })
                .collect(),
        })
        .collect();
    PanelDescription {
        open: label(&home),
        sections,
        keys: Action::panel_keys(&panel::KEYS, true),
        error: inbox.load_error().map(ToString::to_string),
    }
}

impl Inbox {
    /// Notes the panel row the keyboard cursor is drawn on (`None`: no
    /// row), as the panel draws it.
    pub(super) fn record_panel_cursor(&self, row: Option<InboxPanelRow>) {
        *self.panel_cursor.borrow_mut() = row;
    }

    /// What the Inbox shows now, as plain data (module docs), with the
    /// panel's CHECKS as `checks` has them.
    pub fn describe(&self, checks: &Checks) -> InboxDescription {
        let today = view::today(self.data().read_at);
        let pending = self.status() == SuggestionStatus::Pending;
        let (title, caption) = view::toolbar_text(self);
        let groups = self.groups();
        let empty = groups.is_empty().then(|| {
            let (title, body) = view::empty_text(self);
            (title, body.to_string())
        });
        let groups: Vec<GroupDescription> = groups
            .iter()
            .map(|(key, entries)| GroupDescription {
                kind: kind_name(key.kind),
                count: entries.len(),
                rows: entries
                    .iter()
                    .map(|entry| row(entry, today, self.selected() == Some(entry.id())))
                    .collect(),
                footer: pending.then(|| {
                    let footer = FooterText::of(self, key, entries);
                    FooterDescription {
                        actions: vec![
                            Action::button(footer.approve_label, footer.can_approve),
                            Action::button(footer.reject_label, true),
                        ],
                        note: footer.note,
                        source: format!("{} · {}", footer.source.name, footer.source.tail),
                        refused: footer.refused.map(|message| message.to_string()),
                    }
                }),
            })
            .collect();
        let selected = self.selected_entry();
        let has_rows = !self.order().is_empty();
        InboxDescription {
            panel: describe_panel(self, checks),
            title,
            caption,
            toolbar_keys: if pending {
                view::KEYS
                    .hints()
                    .into_iter()
                    .map(|hint| (hint.cap, hint.label))
                    .collect()
            } else {
                Vec::new()
            },
            empty,
            selected: groups
                .iter()
                .flat_map(|group| &group.rows)
                .find(|row| row.selected)
                .map(|row| row.label.clone()),
            open: selected
                .filter(|_| pending)
                .and_then(|entry| open(entry, today)),
            inspector: inspector(self),
            confirmation: match self.outcome() {
                Some(Outcome::Done(message)) => Some(message.to_string()),
                _ => None,
            },
            actions: actions(pending, selected.is_some(), has_rows),
            groups,
        }
    }
}
