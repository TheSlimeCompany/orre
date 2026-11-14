//! The Add balance dialog for a bank or cash account (Paper `I7C-0` on the
//! Accounts panel · states board `8D8-0`; drawn for M3.2 and approved by
//! the user, 2026-11-14), opened by "Add balance…" in the account inspector.
//!
//! Adapted from D6.9's "Add a balance for Mortgage": Balance (the currency's
//! symbol, mono, signed: a minus when overdrawn) with its date in a 96px
//! field, Note, a caption, then Cancel and "Add balance" (neutral primary).
//! It records a `manual` entered balance ([`coffer_core::account::add_balance`]):
//! one per account and day, after the account's lock date, checked against
//! the transactions like any other. A refusal is said in `danger` under the
//! caption.
//!
//! On an asset or liability account it records a valuation (M3.17). For a
//! liability it is D6.9's drawn "Add a balance for Mortgage" (Paper
//! `FS0-0`): Owed, unsigned, stored below zero, the caption saying what
//! Coffer will store as it's typed. For an asset it is composed from that
//! one (decided with the user, 2026-11-14): "Add a valuation for House",
//! Value, unsigned, and "Add valuation". Neither takes a minus.

use chrono::NaiveDate;
use coffer_core::account::{AccountKind, EnteredBalance, EnteredSource};
use coffer_core::id::AccountId;
use coffer_core::money::Currency;
use coffer_core::money::Money;
use gpui_kit::component::input::{InputEvent, InputState};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::{
    AppContext as _, Context, Entity, EventEmitter, FocusHandle, Focusable as _, IntoElement,
    ParentElement as _, Render, SharedString, Styled as _, Subscription, Window, div,
};

use crate::format::{Sign, currency_prefix, field_date, money, parse_amount};
use crate::theme::tokens::{color, spacing};
use crate::ui::button::{Button, ButtonVariant};
use crate::ui::dialog::Dialog;
use crate::ui::field::{FieldRow, amount_input, caption, date_input, error_caption, text_input};

use super::new_account::{read_balance, sentence};
use super::{Accounts, DialogClosed};

/// The dialog's caption (Paper `I7I-0`).
const CAPTION: &str = "Your balance at the end of that day, as your bank shows it. Type a minus if the account is overdrawn. Coffer checks it against the transactions since the last balance.";

/// The caption for an asset (composed from `FS6-0`).
const ASSET_CAPTION: &str = "What it's worth on that day, from a survey, an estimate or a statement. Coffer holds it until the next one; it isn't checked against transactions.";

/// What the dialog says, by the account's kind.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Wording {
    /// "Add a balance for" / "Add a valuation for".
    pub title: &'static str,
    /// "Balance" / "Value" / "Owed".
    pub label: &'static str,
    /// "Add balance" / "Add valuation".
    pub button: &'static str,
}

impl Wording {
    pub const fn of(kind: AccountKind) -> Wording {
        match kind {
            AccountKind::Asset => Wording {
                title: "Add a valuation for",
                label: "Value",
                button: "Add valuation",
            },
            AccountKind::Liability => Wording {
                title: "Add a balance for",
                label: "Owed",
                button: "Add balance",
            },
            _ => Wording {
                title: "Add a balance for",
                label: "Balance",
                button: "Add balance",
            },
        }
    }
}

/// The value an asset or liability stores for `typed`, which must not be
/// below zero: a liability's is what's owed, stored below zero (D6.9).
pub fn stored_value(kind: AccountKind, typed: Money) -> Result<Money, SharedString> {
    if typed.is_negative() {
        return Err(match kind {
            AccountKind::Liability => {
                "Type what you owe without a minus: Coffer stores it below zero.".into()
            }
            _ => "Type the value without a minus.".into(),
        });
    }
    if kind == AccountKind::Liability {
        typed
            .checked_neg()
            .map_err(|error| SharedString::from(error.to_string()))
    } else {
        Ok(typed)
    }
}

/// A liability's caption (Paper `FS6-0`), with what Coffer will store for
/// `typed` when it reads.
pub fn owed_caption(typed: Option<Money>) -> String {
    let stored = typed
        .and_then(|typed| stored_value(AccountKind::Liability, typed).ok())
        .filter(|stored| !stored.is_zero())
        .map_or_else(
            || "below zero".to_owned(),
            |stored| format!("as {}", money(stored, Sign::Negative)),
        );
    format!(
        "Type the amount the statement says you owe. Coffer stores it {stored} and holds it until the next balance. A valuation sets the balance, so it's never a failed check."
    )
}

/// Add balance, for one account.
pub struct AddBalanceDialog {
    accounts: Entity<Accounts>,
    account: AccountId,
    title: SharedString,
    kind: AccountKind,
    currency: Currency,
    focus: FocusHandle,
    amount: Entity<InputState>,
    date: Entity<InputState>,
    note: Entity<InputState>,
    error: Option<SharedString>,
    today: NaiveDate,
    _subscriptions: Vec<Subscription>,
}

impl EventEmitter<DialogClosed> for AddBalanceDialog {}

impl AddBalanceDialog {
    /// The dialog for `account`, which must be open.
    pub fn new(
        accounts: Entity<Accounts>,
        account: AccountId,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let data = accounts.read(cx).data();
        let today = data.today;
        let (title, kind, currency) = data.node(account).map_or_else(
            || ("Add a balance".into(), AccountKind::Current, Currency::GBP),
            |node| {
                let wording = Wording::of(node.account.kind);
                (
                    format!("{} {}", wording.title, node.account.name).into(),
                    node.account.kind,
                    node.account.currency,
                )
            },
        );
        let amount = cx.new(|cx| InputState::new(window, cx));
        let date = cx.new(|cx| InputState::new(window, cx).default_value(field_date(today)));
        let note = cx.new(|cx| InputState::new(window, cx).placeholder("Optional"));
        let submit_on_enter = |this: &mut Self,
                               _: &Entity<InputState>,
                               event: &InputEvent,
                               window: &mut Window,
                               cx: &mut Context<Self>| {
            match event {
                InputEvent::PressEnter { .. } => this.submit(window, cx),
                // A liability's caption says what will be stored.
                InputEvent::Change => cx.notify(),
                _ => {}
            }
        };
        let subscriptions = [&amount, &date, &note]
            .into_iter()
            .map(|input| cx.subscribe_in(input, window, submit_on_enter))
            .collect();
        amount.read(cx).focus_handle(cx).focus(window, cx);
        Self {
            accounts,
            account,
            title,
            kind,
            currency,
            focus: cx.focus_handle(),
            amount,
            date,
            note,
            error: None,
            today,
            _subscriptions: subscriptions,
        }
    }

    pub fn error(&self) -> Option<&SharedString> {
        self.error.as_ref()
    }

    /// Records the balance from the fields, or says why it can't.
    pub fn submit(&mut self, _: &mut Window, cx: &mut Context<Self>) {
        let value =
            |input: &Entity<InputState>, cx: &Context<Self>| input.read(cx).value().to_string();
        let amount = value(&self.amount, cx);
        let read = if amount.trim().is_empty() {
            Err(format!(
                "Type the {}, as 1,284.10.",
                Wording::of(self.kind).label.to_lowercase()
            )
            .into())
        } else {
            read_balance(&amount, &value(&self.date, cx), self.currency, self.today)
        };
        let (date, balance) = match read {
            Ok(Some(read)) => read,
            Ok(None) => return,
            Err(error) => {
                self.error = Some(error);
                cx.notify();
                return;
            }
        };
        let valued = !self.kind.holds_transactions();
        let balance = if valued {
            match stored_value(self.kind, balance) {
                Ok(balance) => balance,
                Err(error) => {
                    self.error = Some(error);
                    cx.notify();
                    return;
                }
            }
        } else {
            balance
        };
        let note = value(&self.note, cx).trim().to_owned();
        let entered = EnteredBalance {
            date,
            balance,
            source: EnteredSource::for_kind(self.kind),
            note: (!note.is_empty()).then_some(note),
        };
        let account = self.account;
        let result = self.accounts.update(cx, |accounts, cx| {
            accounts.add_balance(account, &entered, cx)
        });
        match result {
            Ok(()) => cx.emit(DialogClosed),
            Err(error) => {
                self.error = Some(sentence(&error));
                cx.notify();
            }
        }
    }

    pub fn cancel(&mut self, cx: &mut Context<Self>) {
        cx.emit(DialogClosed);
    }
}

impl Render for AddBalanceDialog {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let wording = Wording::of(self.kind);
        let caption_text = match self.kind {
            AccountKind::Liability => {
                owed_caption(parse_amount(&self.amount.read(cx).value(), self.currency).ok())
            }
            AccountKind::Asset => ASSET_CAPTION.to_owned(),
            _ => CAPTION.to_owned(),
        };
        let fields = div()
            .flex()
            .flex_col()
            .gap(spacing::S2)
            .child(FieldRow::new(
                wording.label,
                div()
                    .flex()
                    .flex_1()
                    .min_w_0()
                    .gap(spacing::S2)
                    .child(
                        amount_input(&self.amount, currency_prefix(self.currency), cx)
                            .id("add-balance-amount")
                            .flex_1(),
                    )
                    .child(date_input(&self.date).id("add-balance-date")),
            ))
            .child(FieldRow::new(
                "Note",
                text_input(&self.note).id("add-balance-note"),
            ));
        let cancel = cx.listener(|this, _, _, cx| this.cancel(cx));
        let add = cx.listener(|this, _, window, cx| this.submit(window, cx));
        let on_cancel = cx.entity().downgrade();
        Dialog::new(self.title.clone(), &self.focus)
            .body(fields)
            .body(caption(caption_text))
            .when_some(self.error.clone(), |this, error| {
                this.body(error_caption(error))
            })
            .action(
                Button::new("add-balance-cancel", "Cancel")
                    .with_variant(ButtonVariant::Secondary)
                    .with_surface(color::FLOAT)
                    .on_click(cancel),
            )
            .action(
                Button::new("add-balance-add", wording.button)
                    .with_variant(ButtonVariant::Primary)
                    .with_surface(color::FLOAT)
                    .on_click(add),
            )
            .on_cancel(move |_, cx| {
                let _ = on_cancel.update(cx, |this, cx| this.cancel(cx));
            })
    }
}

#[cfg(test)]
mod tests {
    use coffer_core::money::Currency;

    use super::*;

    #[test]
    fn a_liability_stores_what_is_owed_below_zero() {
        let gbp = |minor| Money::new(minor, Currency::GBP);
        assert_eq!(
            stored_value(AccountKind::Liability, gbp(21_264_000)),
            Ok(gbp(-21_264_000))
        );
        assert_eq!(stored_value(AccountKind::Asset, gbp(100)), Ok(gbp(100)));
        assert!(stored_value(AccountKind::Liability, gbp(-1)).is_err());
        assert!(stored_value(AccountKind::Asset, gbp(-1)).is_err());
        assert_eq!(
            owed_caption(Some(gbp(21_264_000))),
            "Type the amount the statement says you owe. Coffer stores it as \u{2212}£212,640.00 and holds it until the next balance. A valuation sets the balance, so it's never a failed check."
        );
        assert!(owed_caption(None).contains("stores it below zero"));
        assert_eq!(Wording::of(AccountKind::Asset).button, "Add valuation");
        assert_eq!(Wording::of(AccountKind::Cash).label, "Balance");
    }
}
