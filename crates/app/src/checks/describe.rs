//! What the review queues show, as plain data, for tests (ROADMAP U.3):
//! the CHECKS rows of the panel, the toolbar, the open queue's table, the
//! selection, the inspector, the undo bar or refusal, a lock dialog, and
//! the actions available with their keys.
//!
//! Compiled for tests only (`#[cfg(test)]` on the module): it isn't in a
//! release build and nothing outside the tests can reach it, `coffer mcp`
//! included (`docs/architecture/app.md` → "Decision record: views that
//! describe themselves stay out of the release binary").
//!
//! Every string comes from the code the queues draw with ([`super::view`],
//! [`super::duplicates`], [`super::balances`], [`super::owed`],
//! [`super::text`]): a row's lanes from its `…RowText`, labels from the
//! constants the buttons are made with, so names, bank text, bank IDs and
//! account names are described exactly as shown: through
//! [`super::text::plain`]. Styling (tones, markers, icons) is left out, but
//! for the ✓ or ≠ of a criterion and whether a check failed; mixed runs are
//! joined into one string, and an inspector row's name and amount into its
//! value.
//!
//! What it leaves out: the lanes' headings, the inspector's fixed
//! explanatory captions (sentences with figures in them are kept, as rows
//! labelled ""), Owed to you's Name field while it's typed in and why a rename
//! was refused (the view holds those, not [`Checks`]), and the pointer's
//! hover.

use coffer_core::checks::{AccountReview, CheckRow, Pair};
use coffer_core::reconciliation::Outcome as CheckOutcome;
use gpui_kit::{AnyView, App};

pub use crate::ui::describe::{Action, Dialog, Header, Inspector, PanelRow, Section};

use super::balances::{self, Caption, CheckRowText, Foot, LockText, ThisCheck};
use super::duplicates::{self, Compared, Criterion, Field, MergeRow, PairRowText};
use super::lock::{LockDialog, UnlockDialog};
use super::owed::{ClaimRowText, ListRowText, PersonRowText};
use super::text::{self, Part};
use super::view::{self, KEYS};
use super::{Checks, Outcome, Queue, owed};
use crate::theme::tokens::color;

/// What the review queues show ([`Checks::describe`]).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ChecksDescription {
    /// The CHECKS rows of the Inbox's panel, the open queue selected.
    pub queues: Vec<PanelRow>,
    /// The toolbar's title: the queue's name.
    pub title: &'static str,
    pub caption: String,
    /// The keys the toolbar hints, with what they do ([`KEYS`]).
    pub toolbar_keys: Vec<(&'static str, &'static str)>,
    /// Why core refused the last change, under the toolbar.
    pub refused: Option<String>,
    pub body: Body,
    pub inspector: InspectorDescription,
    /// The undo bar after a change: its message, and Undo when it has one.
    pub undo_bar: Option<(String, Vec<Action>)>,
    /// The lock or unlock dialog, when one is open over the window
    /// ([`ChecksDescription::with_dialog`]).
    pub dialog: Option<Dialog>,
    /// The canvas's keys ([`KEYS`]: its own, then ↑↓), and whether each
    /// does anything now.
    pub actions: Vec<Action>,
}

/// The open queue's canvas under the toolbar.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Body {
    /// No rows: the empty state's title and body (or why the queue
    /// couldn't be read).
    Empty {
        title: &'static str,
        body: String,
    },
    Duplicates(DuplicatesDescription),
    Balances(BalancesDescription),
    Owed(OwedDescription),
}

/// Possible duplicates' table.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DuplicatesDescription {
    pub pairs: Vec<PairDescription>,
    /// The selected pair, open under its row.
    pub open: Option<ComparisonDescription>,
    /// The caption under the pairs.
    pub footnote: String,
}

/// A pair's row, lane by lane.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PairDescription {
    /// "OCADO RETAIL LTD and Ocado".
    pub names: String,
    pub account: String,
    pub why: String,
    pub dates: String,
    pub amount: String,
    pub selected: bool,
}

/// The selected pair, side by side.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ComparisonDescription {
    /// Under "Keep": where the kept row came from.
    pub keep: String,
    /// Under "Exclude as duplicate": where the other came from.
    pub exclude: String,
    /// Each field's label, then the kept row's value and the other's.
    pub fields: Vec<(&'static str, String, String)>,
    /// The locked-period block: its title and what it says.
    pub locked: Option<(&'static str, String)>,
    /// The caption beside the buttons.
    pub caption: String,
    /// "Keep this instead", "Unlock…" while locked, Merge and Not
    /// duplicates.
    pub actions: Vec<Action>,
}

/// Balance checks' table.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BalancesDescription {
    pub accounts: Vec<AccountDescription>,
    /// The caption naming the accounts that aren't checked.
    pub not_checked: Option<String>,
}

/// One account's section.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AccountDescription {
    pub name: String,
    /// "Locked through 31 Oct".
    pub locked: Option<String>,
    /// Newest first.
    pub rows: Vec<CheckDescription>,
    /// The lock prompt or "nothing to check yet", with its button.
    pub foot: Option<(String, Action)>,
}

/// A balance's row, lane by lane, and the window under it when it failed.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CheckDescription {
    /// Whether it failed (an alert), passed (a ✓), or isn't a check.
    pub status: Option<CheckStatus>,
    pub source: String,
    pub date: String,
    pub balance: String,
    pub coffer: String,
    pub difference: String,
    pub selected: bool,
    /// Under a failed row: what it says, and "Review pair" when a pair
    /// explains the gap.
    pub window: Option<(String, Vec<Action>)>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CheckStatus {
    Passed,
    Failed,
}

/// Owed to you's table.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OwedDescription {
    pub claims: Vec<ClaimDescription>,
    /// One strip per matched payment: what it says, and its buttons.
    pub matches: Vec<(String, Vec<Action>)>,
    pub people: Vec<PersonDescription>,
}

/// An expense claim's row.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ClaimDescription {
    pub merchant: String,
    pub account: String,
    pub date: String,
    pub owed: String,
    /// A payment matches it (the `link` icon).
    pub matched: bool,
}

/// A person's row under Shared costs.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PersonDescription {
    pub name: String,
    pub costs: String,
    pub last_paid: String,
    pub owed: String,
    pub selected: bool,
}

/// The inspector.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum InspectorDescription {
    /// Nothing is selected: what it says instead.
    Nothing(&'static str),
    Shown(Box<Inspector>),
}

/// Runs joined into the line they're drawn as.
fn joined(parts: &[Part]) -> String {
    parts.iter().map(|part| part.text.as_str()).collect()
}

/// An inspector row of Costs or Payments: its date, then its name and
/// amount as one value.
fn list_row(ListRowText { date, name, amount }: ListRowText) -> (String, String) {
    (date, format!("{name} {amount}"))
}

/// A [`Section`] with sentences that carry figures, as `rows` labelled "".
fn with_lines(mut section: Section, lines: impl IntoIterator<Item = String>) -> Section {
    section
        .rows
        .extend(lines.into_iter().map(|line| (String::new(), line)));
    section
}

// Possible duplicates ------------------------------------------------------------

fn pair_row(pair: &Pair, selected: bool, today: chrono::NaiveDate) -> PairDescription {
    let PairRowText {
        names,
        account,
        why,
        dates,
        amount,
    } = duplicates::pair_row_text(pair, color::TEXT_MUTED, today);
    PairDescription {
        names: joined(&names),
        account,
        why,
        dates,
        amount,
        selected,
    }
}

fn comparison(pair: &Pair, today: chrono::NaiveDate) -> ComparisonDescription {
    let fields = duplicates::fields(&pair.rows)
        .into_iter()
        .map(|field| match field {
            Field::Compared(Compared {
                label,
                values: [kept, other],
                missing: [kept_missing, other_missing],
                ..
            }) => (
                label,
                kept.unwrap_or_else(|| kept_missing.to_owned()),
                other.unwrap_or_else(|| other_missing.to_owned()),
            ),
            Field::Category => (
                duplicates::CATEGORY,
                duplicates::category_label(&pair.rows[0]),
                duplicates::category_label(&pair.rows[1]),
            ),
        })
        .collect();
    let locked = pair.effect.as_ref().ok().and_then(|effect| {
        duplicates::locked_words(pair, effect, today)
            .map(|words| (duplicates::LOCKED_TITLE, joined(&words)))
    });
    let mut actions = vec![Action::button(duplicates::KEEP_OTHER, true)];
    if locked.is_some() {
        actions.push(Action::button(duplicates::UNLOCK, true));
    }
    actions.push(Action::button(
        duplicates::MERGE,
        duplicates::can_merge(pair),
    ));
    actions.push(Action::button(duplicates::NOT_DUPLICATES, true));
    ComparisonDescription {
        keep: text::added(&pair.rows[0], today),
        exclude: text::added(&pair.rows[1], today),
        fields,
        locked,
        caption: duplicates::footer_caption(pair).0,
        actions,
    }
}

fn duplicates_body(checks: &Checks, today: chrono::NaiveDate) -> Body {
    let selected = checks.selected_pair().map(|pair| pair.candidate);
    DuplicatesDescription {
        pairs: checks
            .pairs()
            .iter()
            .map(|pair| pair_row(pair, selected == Some(pair.candidate), today))
            .collect(),
        open: checks.selected_pair().map(|pair| comparison(pair, today)),
        footnote: duplicates::footnote(),
    }
    .into()
}

fn pair_inspector(checks: &Checks, pair: &Pair, today: chrono::NaiveDate) -> Inspector {
    let (title, figure, caption) = duplicates::header_text(pair);
    let why = duplicates::criteria(pair).into_iter().fold(
        Section::titled(duplicates::WHY_TITLE),
        |section, Criterion { met, label, detail }| {
            let mark = if met { "✓" } else { "≠" };
            section.row(
                format!("{mark} {label}"),
                detail.map(|(words, _)| words).unwrap_or_default(),
            )
        },
    );
    let merge = match &pair.effect {
        Ok(effect) => {
            let rows = duplicates::merge_rows(pair, effect, &checks.data().palette, today);
            let section = rows.into_iter().fold(
                Section::titled(duplicates::MERGE_TITLE),
                |section, MergeRow { label, parts, .. }| section.row(label, joined(&parts)),
            );
            with_lines(
                section,
                duplicates::merge_captions(pair, effect, today)
                    .iter()
                    .map(|parts| joined(parts)),
            )
        }
        Err(message) => with_lines(
            Section::titled(duplicates::MERGE_TITLE),
            [duplicates::cant_merge(message)],
        ),
    };
    Inspector {
        header: Header {
            title,
            figure: Some(figure),
            caption: Some(caption),
        },
        sections: vec![why, merge],
    }
}

// Balance checks -----------------------------------------------------------------

fn check_row(
    checks: &Checks,
    account: &AccountReview,
    row: &CheckRow,
    today: chrono::NaiveDate,
) -> CheckDescription {
    let check = &row.check;
    let CheckRowText {
        source,
        date,
        balance,
        coffer,
        difference,
    } = balances::check_row_text(row, today);
    CheckDescription {
        status: match check.outcome {
            CheckOutcome::Failed { .. } => Some(CheckStatus::Failed),
            CheckOutcome::Passed { .. } => Some(CheckStatus::Passed),
            _ => None,
        },
        source,
        date,
        balance,
        coffer,
        difference,
        selected: checks.is_check_selected(account.account, check.balance),
        window: check
            .outcome
            .is_failed()
            .then(|| balances::window_words(checks, account, row, today))
            .flatten()
            .map(|(words, pair)| {
                let review = pair
                    .map(|_| vec![Action::button(balances::REVIEW_PAIR, true)])
                    .unwrap_or_default();
                (joined(&words), review)
            }),
    }
}

fn balances_body(checks: &Checks, today: chrono::NaiveDate) -> Body {
    let accounts = checks
        .accounts()
        .iter()
        .map(|account| AccountDescription {
            name: text::account(&account.name),
            locked: account
                .locked_through
                .map(|through| joined(&balances::locked_through(through, today))),
            rows: account
                .rows
                .iter()
                .rev()
                .map(|row| check_row(checks, account, row, today))
                .collect(),
            foot: balances::foot(account, today)
                .map(|Foot { words, button, .. }| (joined(&words), Action::button(button, true))),
        })
        .collect();
    BalancesDescription {
        accounts,
        not_checked: balances::not_checked(checks),
    }
    .into()
}

fn check_inspector(
    checks: &Checks,
    account: &AccountReview,
    row: &CheckRow,
    today: chrono::NaiveDate,
) -> Inspector {
    let (title, figure, caption) = balances::header_text(account, row, today);
    let ThisCheck {
        title: this_title,
        rows,
        caption: words,
    } = balances::this_check_text(row, today);
    let this = rows
        .into_iter()
        .fold(Section::titled(this_title), |section, (label, parts)| {
            section.row(label, joined(&parts))
        });
    let this = match words {
        Caption::Fixed(_) => this,
        Caption::Rich(words) => with_lines(this, [joined(&words)]),
    };
    let mut sections = vec![this];
    if row.check.outcome.is_failed() {
        let explained = balances::explained_by(checks, row, today);
        let lead = joined(&balances::explain_lead(explained.is_some(), row, today));
        let section = match explained {
            Some((amount, words, _)) => with_lines(
                Section::titled(balances::EXPLAIN_TITLE)
                    .row(balances::POSSIBLE_DUPLICATE, amount)
                    .action(Action::button(balances::REVIEW_PAIR, true)),
                [joined(&words), lead],
            ),
            None => with_lines(Section::titled(balances::EXPLAIN_TITLE), [lead]),
        };
        sections.push(section);
    }
    let LockText {
        title: lock_title,
        button,
        words,
    } = balances::lock_text(account, Some(row), today);
    let mut lock = with_lines(Section::titled(joined(&lock_title)), [joined(&words)]);
    if let Some((label, _)) = button {
        lock = lock.action(Action::button(label, true));
    }
    sections.push(lock);
    Inspector {
        header: Header {
            title,
            figure,
            caption: Some(caption),
        },
        sections,
    }
}

// Owed to you --------------------------------------------------------------------

fn owed_body(checks: &Checks, today: chrono::NaiveDate) -> Body {
    let queue = &checks.data().owed;
    OwedDescription {
        claims: queue
            .claims
            .iter()
            .map(|claim| {
                let ClaimRowText {
                    merchant,
                    account,
                    date,
                    owed,
                } = owed::claim_row_text(claim, today);
                ClaimDescription {
                    merchant,
                    account,
                    date,
                    owed,
                    matched: claim.matched,
                }
            })
            .collect(),
        matches: queue
            .matches
            .iter()
            .map(|found| {
                let (words, _) = owed::match_text(found, today);
                (
                    joined(&words),
                    vec![
                        Action::button(owed::LINK, true),
                        Action::button(owed::NOT_THIS, true),
                    ],
                )
            })
            .collect(),
        people: queue
            .people
            .iter()
            .map(|person| {
                let PersonRowText {
                    name,
                    costs,
                    last_paid,
                    owed,
                } = owed::person_row_text(person, today);
                PersonDescription {
                    name,
                    costs: joined(&costs),
                    last_paid,
                    owed,
                    selected: checks.is_person_selected(person),
                }
            })
            .collect(),
    }
    .into()
}

fn person_inspector(
    person: &coffer_core::link::owed::Person,
    today: chrono::NaiveDate,
) -> Inspector {
    let (title, figure, caption) = owed::header_text(person);
    let costs = person
        .costs
        .iter()
        .fold(Section::titled(owed::COSTS), |section, cost| {
            let (date, value) = list_row(owed::cost_row_text(cost, today));
            section.row(date, value)
        });
    let payments = person.payments.iter().fold(
        Section::titled(owed::payments_title(person))
            .action(Action::button(owed::LINK_PAYMENT, true)),
        |section, payment| {
            let (date, value) = list_row(owed::payment_row_text(payment, today));
            section.row(date, value)
        },
    );
    let name = Section::default().row(owed::NAME, owed::name_value(person));
    Inspector {
        header: Header {
            title,
            figure: Some(figure),
            caption: Some(caption),
        },
        sections: vec![costs, payments, name],
    }
}

// The whole -------------------------------------------------------------------------

impl From<DuplicatesDescription> for Body {
    fn from(table: DuplicatesDescription) -> Self {
        Body::Duplicates(table)
    }
}

impl From<BalancesDescription> for Body {
    fn from(table: BalancesDescription) -> Self {
        Body::Balances(table)
    }
}

impl From<OwedDescription> for Body {
    fn from(table: OwedDescription) -> Self {
        Body::Owed(table)
    }
}

/// The canvas's keys: M, N and ←→ on Possible duplicates with a pair
/// selected (M only when it can merge), Escape while something is
/// selected, ↑↓ while the open queue has rows.
fn actions(checks: &Checks) -> Vec<Action> {
    let queue = checks.queue();
    let duplicates = queue == Some(Queue::Duplicates);
    let pair = duplicates && checks.selected_pair().is_some();
    let selected = match queue {
        Some(Queue::Duplicates) => checks.selected_pair().is_some(),
        Some(Queue::Balances) => checks.selected_check().is_some(),
        Some(Queue::Owed) => checks.selected_person().is_some(),
        None => false,
    };
    let rows = match queue {
        Some(Queue::Duplicates) => !checks.pairs().is_empty(),
        Some(Queue::Balances) => checks.accounts().iter().any(|a| !a.rows.is_empty()),
        Some(Queue::Owed) => !checks.data().owed.people.is_empty(),
        None => false,
    };
    let [merge, dismiss, keep, clear] = KEYS.keys() else {
        panic!("say when the queues' new key works: {:?}", KEYS.keys());
    };
    Action::view_keys(
        &KEYS,
        |key| {
            if key.label() == merge.label() {
                pair && checks.can_merge()
            } else if [dismiss.label(), keep.label()].contains(&key.label()) {
                pair
            } else {
                assert_eq!(key.label(), clear.label());
                selected
            }
        },
        rows,
    )
}

impl Checks {
    /// What the review queues show now, as plain data (module docs). A
    /// dialog is the shell's: add it with
    /// [`ChecksDescription::with_dialog`].
    pub fn describe(&self) -> ChecksDescription {
        let today = self.today();
        let queue = self.queue().unwrap_or(Queue::Duplicates);
        let queues = Queue::ALL
            .into_iter()
            .map(|row| PanelRow {
                label: row.title(),
                count: Some(self.queue_count(row).to_string()),
                flagged: self.queue_flagged(row),
                selected: self.queue() == Some(row),
            })
            .collect();
        let caption = match queue {
            Queue::Owed => joined(&owed::toolbar_caption(&self.data().owed)),
            _ => view::toolbar_caption(self),
        };
        let empty = match queue {
            Queue::Duplicates => duplicates::empty(self),
            Queue::Balances => balances::empty(self),
            Queue::Owed => owed::empty(self),
        };
        let body = match (empty, queue) {
            (Some((title, body)), _) => Body::Empty {
                title,
                body: body.to_string(),
            },
            (None, Queue::Duplicates) => duplicates_body(self, today),
            (None, Queue::Balances) => balances_body(self, today),
            (None, Queue::Owed) => owed_body(self, today),
        };
        let inspector = match queue {
            Queue::Duplicates => self
                .selected_pair()
                .map(|pair| pair_inspector(self, pair, today))
                .ok_or(duplicates::NOTHING_SELECTED),
            Queue::Balances => self
                .selected_check()
                .map(|(account, row)| check_inspector(self, account, row, today))
                .ok_or(balances::NOTHING_SELECTED),
            Queue::Owed => self
                .selected_person()
                .map(|person| person_inspector(person, today))
                .ok_or(owed::NOTHING_SELECTED),
        };
        ChecksDescription {
            queues,
            title: queue.title(),
            caption,
            toolbar_keys: if view::hinted(self) {
                KEYS.hints()
                    .into_iter()
                    .map(|hint| (hint.cap, hint.label))
                    .collect()
            } else {
                Vec::new()
            },
            refused: match self.outcome() {
                Some(Outcome::Refused(message)) => Some(message.to_string()),
                _ => None,
            },
            body,
            inspector: match inspector {
                Ok(shown) => InspectorDescription::Shown(Box::new(shown)),
                Err(words) => InspectorDescription::Nothing(words),
            },
            undo_bar: match self.outcome() {
                Some(Outcome::Done { message, undo }) => Some((
                    message.to_string(),
                    undo.map(|_| Action::button(crate::ui::undo_bar::UNDO, true))
                        .into_iter()
                        .collect(),
                )),
                _ => None,
            },
            dialog: None,
            actions: actions(self),
        }
    }
}

impl ChecksDescription {
    /// With `dialog`, the shell's open dialog, when it's the lock or the
    /// unlock dialog.
    pub fn with_dialog(mut self, dialog: Option<&AnyView>, cx: &App) -> Self {
        self.dialog = dialog.and_then(|dialog| {
            if let Ok(lock) = dialog.clone().downcast::<LockDialog>() {
                return Some(lock.read(cx).describe());
            }
            dialog
                .clone()
                .downcast::<UnlockDialog>()
                .ok()
                .map(|unlock| unlock.read(cx).describe())
        });
        self
    }
}
