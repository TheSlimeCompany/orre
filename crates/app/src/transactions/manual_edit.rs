//! A manual row's own fields in the inspector (ROADMAP M3.10; `DESIGN.md` →
//! "Transactions additions" → Manual (cash account), Paper `4D7-0`).
//!
//! A row the user entered has no imported source fields, so Details shows
//! its Date and Amount as fields (mono, the amount signed with the
//! currency as its unit) and its Account as a select, where an imported
//! row shows them locked. Each saves through core's [`manual::update`],
//! after [`manual::get`] reads what is stored: Date and Amount
//! when the field is left or on Enter, like Merchant and Note; the Account
//! on a choice. Core refuses what it must (a locked period, a date after
//! the account closed, a split or linked row's amount, another currency,
//! the amount or account of a row in a suggested or confirmed transfer
//! pair until it's unpaired), and the reason is said under Details. A Date
//! or Amount saved as the user leaves the row can't be said there, as
//! Details shows the next row by then: the failure bar says it instead
//! (`TransactionsView::say_on_failure_bar`), and its Show goes back to the row
//! with the reason under Details.

use gpui_kit::{Context, Focusable as _, SharedString, Window};
use orre_core::id::{AccountId, TransactionId};
use orre_core::manual;
use orre_core::reporting::register::{Register, RegisterRow};

use crate::accounts::store::StoreError;
use crate::format;
use crate::ui::menu::{MenuGroup, MenuOption};

use super::editor::EditPlace;
use super::write::Policy;
use super::{TransactionsEvent, TransactionsView};

/// The accounts a manual row can be on, or a new one added to: every
/// account that holds transactions, in the register's order.
pub fn account_groups(register: &Register) -> Vec<MenuGroup> {
    let options = register
        .accounts()
        .iter()
        .filter(|account| account.kind.holds_transactions())
        .map(|account| MenuOption::new(account.id.to_string(), account.name.clone()))
        .collect();
    vec![MenuGroup::new(None, options)]
}

impl TransactionsView {
    /// Puts a manual `row`'s date and amount in their fields; unless
    /// `force`, only into a field that isn't being typed in.
    pub(super) fn fill_manual(
        &mut self,
        row: &RegisterRow,
        force: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !row.source.is_manual() {
            return;
        }
        for (slot, input, value) in [
            (2, self.editor.date.clone(), format::field_date(row.date)),
            (
                3,
                self.editor.amount.clone(),
                format::field_amount(row.amount),
            ),
        ] {
            let typing = input.read(cx).focus_handle(cx).is_focused(window);
            if !force && typing {
                continue;
            }
            let filled = input.update(cx, |input, cx| {
                if input.value().as_ref() != value {
                    input.set_value(SharedString::from(value), window, cx);
                }
                input.value()
            });
            self.editor.filled[slot] = filled;
        }
    }

    /// The manual row the editor shows, if it is one.
    fn shown_manual(&self) -> Option<RegisterRow> {
        self.editor
            .shown
            .and_then(|id| self.register.row(id))
            .filter(|row| row.source.is_manual())
            .cloned()
    }

    /// Saves the Date field of a manual row when it changed.
    pub(super) fn commit_manual_date(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(row) = self.shown_manual() else {
            return;
        };
        let typed = self.editor.date.read(cx).value();
        if typed == self.editor.filled[2] {
            return;
        }
        let Some(date) = format::parse_date(&typed, self.today) else {
            self.editor.error = Some((
                EditPlace::Details,
                "The date isn't one Orre can read. Type it as 9 Nov 2026.".into(),
            ));
            cx.notify();
            return;
        };
        if date == row.date {
            // Typed differently, the same day: show it as stored.
            self.fill_manual(&row, true, window, cx);
            return;
        }
        self.update_manual(row.id, cx, move |entry| entry.date = date);
    }

    /// Saves the Amount field of a manual row when it changed.
    pub(super) fn commit_manual_amount(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(row) = self.shown_manual() else {
            return;
        };
        let typed = self.editor.amount.read(cx).value();
        if typed == self.editor.filled[3] {
            return;
        }
        let amount = match format::parse_amount(&typed, row.amount.currency()) {
            Ok(amount) if !amount.is_zero() => amount,
            _ => {
                self.editor.error = Some((
                    EditPlace::Details,
                    "The amount isn't one Orre can read. Type it as −23.00, with a minus for \
                     money out."
                        .into(),
                ));
                cx.notify();
                return;
            }
        };
        if amount == row.amount {
            self.fill_manual(&row, true, window, cx);
            return;
        }
        self.update_manual(row.id, cx, move |entry| entry.amount = amount);
    }

    /// Moves the selected manual row to `account`, keeping its amount.
    pub(super) fn set_manual_account(&mut self, account: AccountId, cx: &mut Context<Self>) {
        let Some(row) = self.single_row().filter(|row| row.source.is_manual()) else {
            return;
        };
        if row.account == account {
            return;
        }
        let id = row.id;
        self.update_manual(id, cx, move |entry| entry.account = account);
    }

    /// One change to manual row `id` through core, then the register is
    /// read again; the accounts panel's balances follow.
    fn update_manual(
        &mut self,
        id: TransactionId,
        cx: &mut Context<Self>,
        change: impl FnOnce(&mut orre_core::manual::ManualTransaction),
    ) {
        let policy = Policy::edit(EditPlace::Details).emit(TransactionsEvent::LedgerChanged);
        self.write(policy, cx, |db| -> Result<(), StoreError> {
            let mut entry = manual::get(db, id)?;
            change(&mut entry);
            manual::update(db, id, &entry)?;
            Ok(())
        })
        .ok();
    }
}
