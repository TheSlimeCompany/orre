//! What the whole window shows, as plain data, for the dev driver
//! (ROADMAP U.5) and tests: the open view's description, the dialog open
//! over it, and what has keyboard focus.
//!
//! Compiled only for tests and the `dev-driver` feature, like every view's
//! `describe()`: it isn't in a release build and `orre mcp` can't reach it
//! (`docs/architecture/app.md` → "Decision record: views that describe
//! themselves stay out of the release binary", and "the dev driver").
//!
//! Each view's description is the one its tests read, built the way those
//! tests build it. A dialog with a `describe()` of its own is described by
//! it. Any other dialog lists its observed controls (role and accessible
//! label, never a field's value, which can hold raw bank text).

use gpui_kit::test::{ElementSnapshot, TestQueryExt as _};
use gpui_kit::{AnyView, App, ElementId, EntityId, Window};

use crate::accounts::describe::AccountsDescription;
use crate::checks::describe::ChecksDescription;
use crate::inbox::describe::InboxDescription;
use crate::rules::describe::{DialogDescription, RulesDescription};
use crate::settings::describe::SettingsDescription;
use crate::transactions::describe::TransactionsDescription;
use crate::ui::describe::Dialog;

use super::{ShellView, View};

/// What the window shows ([`ShellView::describe`]).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ShellDescription {
    /// The open view's tab label ("Settings" for Settings).
    pub view: &'static str,
    pub screen: Screen,
    /// The dialog open over the window, if any.
    pub dialog: Option<DialogShown>,
    /// What has keyboard focus: an observed control's label (or id), else
    /// "the window" when focus is on something unobserved, else `None`.
    pub focus: Option<String>,
    /// The status bar's arrival line (LU.5): "3 suggestions from Claude
    /// Code".
    pub arrival: Option<String>,
    /// The Inbox tab's badge: the count, and whether it's unseen (LU.5).
    pub inbox_badge: Option<(usize, bool)>,
}

/// The open view's own description.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Screen {
    /// Transactions, with the accounts panel and the account inspector.
    Transactions {
        transactions: Box<TransactionsDescription>,
        accounts: Box<AccountsDescription>,
    },
    Inbox(Box<InboxDescription>),
    /// The Inbox with one of its review queues open (CHECKS).
    Checks(Box<ChecksDescription>),
    Rules(Box<RulesDescription>),
    Settings(Box<SettingsDescription>),
    /// A view not built yet: its tab shows a placeholder.
    NotBuilt,
}

/// A dialog open over the window.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DialogShown {
    /// Described by its own `describe()`.
    Described(Dialog),
    /// A Rules dialog, described by its own `describe()`.
    Rules(DialogDescription),
    /// A dialog with no `describe()`: its observed controls, as
    /// (role, accessible label), top to bottom.
    Controls(Vec<(String, String)>),
}

impl ShellView {
    /// What the window shows now (module docs). `window` must be this
    /// shell's, with a frame drawn since the last change.
    pub fn describe(&self, window: &Window, cx: &App) -> ShellDescription {
        let screen = match self.view {
            View::Transactions => {
                let transactions = self.transactions.read(cx);
                let inspector = transactions
                    .selected()
                    .is_empty()
                    .then(|| transactions.account_inspector(cx))
                    .flatten();
                Screen::Transactions {
                    transactions: Box::new(transactions.describe(cx)),
                    accounts: Box::new(
                        self.accounts
                            .read(cx)
                            .describe(inspector.as_ref().map(|inspector| inspector.read(cx)), cx),
                    ),
                }
            }
            View::Inbox if self.checks.read(cx).queue().is_some() => {
                Screen::Checks(Box::new(self.checks.read(cx).describe()))
            }
            View::Inbox => {
                Screen::Inbox(Box::new(self.inbox.read(cx).describe(self.checks.read(cx))))
            }
            View::Rules => Screen::Rules(Box::new(self.rules.read(cx).describe())),
            View::Settings => Screen::Settings(Box::new(self.settings.read(cx).describe())),
            _ => Screen::NotBuilt,
        };
        ShellDescription {
            view: self.view.label(),
            screen,
            dialog: self.describe_dialog(window, cx),
            focus: focus(window, cx),
            arrival: self.arrived.arrivals().line().map(|line| line.text()),
            inbox_badge: {
                let pending = self.inbox.read(cx).badge();
                (pending > 0).then(|| {
                    let unseen = self.arrived.arrivals().inbox_badge_unseen().is_some();
                    (pending, unseen)
                })
            },
        }
    }

    /// The open dialog's entity: the shell's, or the Rules view's own
    /// (deleting a category). The driver clicks inside it only.
    pub fn open_dialog_id(&self, cx: &App) -> Option<EntityId> {
        self.dialog
            .as_ref()
            .map(|(dialog, _)| dialog.entity_id())
            .or_else(|| {
                (self.view == View::Rules)
                    .then(|| self.rules_view.read(cx).delete_category_dialog())
                    .flatten()
                    .map(|dialog| dialog.entity_id())
            })
    }

    fn describe_dialog(&self, window: &Window, cx: &App) -> Option<DialogShown> {
        if let Some(rules) = crate::rules::describe::dialog(self, cx) {
            return Some(DialogShown::Rules(rules));
        }
        let (dialog, _) = self.dialog.as_ref()?;
        Some(
            described(dialog, cx)
                .map(DialogShown::Described)
                .unwrap_or_else(|| DialogShown::Controls(controls(dialog.entity_id(), window))),
        )
    }
}

/// `dialog` described by the view that opened it, when it's one with a
/// `describe()`.
fn described(dialog: &AnyView, cx: &App) -> Option<Dialog> {
    let open = Some(dialog);
    AccountsDescription::dialog_of(open, cx)
        .or_else(|| SettingsDescription::dialog_of(open, cx))
        .or_else(|| ChecksDescription::dialog_of(open, cx))
}

/// The observed controls inside the view `dialog`, top to bottom.
fn controls(dialog: EntityId, window: &Window) -> Vec<(String, String)> {
    let scope = ElementId::View(dialog);
    window
        .elements()
        .into_iter()
        .filter(|element| element.visible() && element.path().contains(&scope))
        .filter_map(|element| {
            let label = element.label()?.to_owned();
            let role = element
                .role()
                .map_or_else(|| "control".to_owned(), |role| format!("{role:?}"));
            Some((role, label))
        })
        .collect()
}

/// The observed control with keyboard focus, by its label or id: the
/// innermost one whose focus scope holds the focus.
fn focus(window: &Window, cx: &App) -> Option<String> {
    window.focused(cx)?;
    let focused = window
        .elements()
        .into_iter()
        .filter(holds_focus)
        .max_by_key(|element| element.path().len());
    Some(match focused {
        Some(element) => element.label().map_or_else(
            || {
                element
                    .path()
                    .last()
                    .map_or_else(String::new, |id| format!("{id}"))
            },
            str::to_owned,
        ),
        None => "the window".to_owned(),
    })
}

/// Whether `element`'s focus scope holds the focus. Read from its `Debug`
/// form: `ElementSnapshot::focused` panics on a control whose focus binding
/// wasn't observed, and the driver mustn't panic on what the app draws.
fn holds_focus(element: &ElementSnapshot) -> bool {
    format!("{element:?}").contains("focused: Some(true)")
}
