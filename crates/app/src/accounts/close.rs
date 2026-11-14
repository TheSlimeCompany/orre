//! The Close account dialog (`docs/DESIGN.md` → "Accounts you keep
//! yourself" → **Close House?**; Paper `FT1-0` on the states board
//! `EPF-0`), opened by "Close account…" in the account inspector, on every
//! kind of account (M3.17).
//!
//! The body first (what closing does: the account leaves the panel and net
//! worth from the day, its history stays, it can be reopened), then the day
//! in a 96px date field, then Cancel and "Close account" (neutral primary:
//! it can be undone). Core decides the rest
//! ([`coffer_core::account::close`]): never before what the account holds,
//! nor before its lock date. A refusal is said in `danger` under the form.
//!
//! Drawn for an asset ("Sold on"). Composed for M3.17 (decided with the
//! user, 2026-11-14): a liability's day is "Paid off on" and net worth rises
//! by what was owed; a bank, cash or pot account's is "Closed on", its
//! balance leaves net worth after the day, and its open pots close with it.
//! The day starts at today, or at the earliest day it can close if that's
//! later. The body follows the day as it's typed.

use chrono::NaiveDate;
use coffer_core::account::AccountKind;
use coffer_core::id::AccountId;
use coffer_core::money::Money;
use gpui_kit::component::input::{InputEvent, InputState};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::{
    AppContext as _, Context, Entity, EventEmitter, FocusHandle, Focusable as _, IntoElement,
    ParentElement as _, Render, SharedString, Styled as _, Subscription, Window, div,
};

use crate::format::{Sign, currency_prefix, field_date, list_date, money, parse_date};
use crate::theme::tokens::{color, spacing};
use crate::ui::button::{Button, ButtonVariant};
use crate::ui::dialog::Dialog;
use crate::ui::field::{FieldRow, caption, date_input, error_caption};

use super::data::AccountsData;
use super::new_account::sentence;
use super::{Accounts, DialogClosed};

/// What the dialog says about one account, read when it opens.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CloseFacts {
    pub name: String,
    pub kind: AccountKind,
    /// Its latest balance or value, if it has one.
    pub balance: Option<Money>,
    /// Its open pots' names, which close with it.
    pub pots: Vec<String>,
    /// The earliest day it can close.
    pub earliest: Option<NaiveDate>,
}

impl CloseFacts {
    /// The facts for `account`, if it's open.
    pub fn of(data: &AccountsData, account: AccountId) -> Option<CloseFacts> {
        let node = data.node(account)?;
        Some(CloseFacts {
            name: node.account.name.clone(),
            kind: node.account.kind,
            balance: node.latest.map(|latest| latest.balance),
            pots: node
                .pots
                .iter()
                .map(|pot| pot.account.name.clone())
                .collect(),
            earliest: data.detail(account).and_then(|detail| detail.close_from),
        })
    }

    /// The day field's label.
    pub fn day_label(&self) -> &'static str {
        match self.kind {
            AccountKind::Asset => "Sold on",
            AccountKind::Liability => "Paid off on",
            _ => "Closed on",
        }
    }

    /// The body for closing on `on` (`None`: a day that doesn't read).
    pub fn body(&self, on: Option<NaiveDate>, today: NaiveDate) -> String {
        let name = &self.name;
        let day = on.map_or_else(|| "that day".to_owned(), |on| list_date(on, today));
        let amount =
            |balance: Money| money(balance.checked_abs().unwrap_or(balance), Sign::Negative);
        match (self.kind, self.balance) {
            (AccountKind::Asset, Some(value)) if !value.is_zero() => format!(
                "{name} leaves the Accounts panel and counts as {zero} from {day}, so net worth drops by {value} from then (the sale money arrives in your bank account). Its valuations stay, so earlier net worth doesn't change. You can reopen it.",
                zero = zero(value),
                value = amount(value),
            ),
            (AccountKind::Liability, Some(owed)) if !owed.is_zero() => format!(
                "{name} leaves the Accounts panel and counts as {zero} from {day}, so net worth rises by {owed} from then. Its balances stay, so earlier net worth doesn't change. You can reopen it.",
                zero = zero(owed),
                owed = amount(owed),
            ),
            (AccountKind::Asset | AccountKind::Liability, _) => format!(
                "{name} leaves the Accounts panel and counts as nothing from {day}. Its values stay, so earlier net worth doesn't change. You can reopen it."
            ),
            (_, balance) => {
                let mut body = format!(
                    "{name} leaves the Accounts panel and net worth after {day}. Its transactions, balances and imports stay, so earlier net worth doesn't change. You can reopen it."
                );
                if let Some(balance) = balance.filter(|balance| !balance.is_zero()) {
                    body.push_str(&format!(
                        " Its balance, {}, leaves net worth with it.",
                        money(balance, Sign::Negative)
                    ));
                }
                match self.pots.as_slice() {
                    [] => {}
                    [pot] => body.push_str(&format!(" Its pot, {pot}, closes with it.")),
                    pots => body.push_str(&format!(" Its pots, {}, close with it.", list(pots))),
                }
                body
            }
        }
    }
}

/// Nothing in `amount`'s currency, as the body writes it: `£0`.
pub(super) fn zero(amount: Money) -> String {
    let prefix = currency_prefix(amount.currency());
    if prefix.chars().count() > 1 {
        format!("{prefix} 0")
    } else {
        format!("{prefix}0")
    }
}

/// `A, B and C`.
fn list(names: &[String]) -> String {
    match names {
        [] => String::new(),
        [one] => one.clone(),
        [rest @ .., last] => format!("{} and {last}", rest.join(", ")),
    }
}

/// Close account, for one account.
pub struct CloseAccountDialog {
    accounts: Entity<Accounts>,
    account: AccountId,
    facts: Option<CloseFacts>,
    focus: FocusHandle,
    date: Entity<InputState>,
    error: Option<SharedString>,
    today: NaiveDate,
    _subscriptions: Vec<Subscription>,
}

impl EventEmitter<DialogClosed> for CloseAccountDialog {}

impl CloseAccountDialog {
    /// The dialog for `account`, which must be open.
    pub fn new(
        accounts: Entity<Accounts>,
        account: AccountId,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let data = accounts.read(cx).data();
        let today = data.today;
        let facts = CloseFacts::of(data, account);
        let start = facts
            .as_ref()
            .and_then(|facts| facts.earliest)
            .map_or(today, |earliest| earliest.max(today));
        let date = cx.new(|cx| InputState::new(window, cx).default_value(field_date(start)));
        let subscriptions = vec![cx.subscribe_in(
            &date,
            window,
            |this: &mut Self, _, event: &InputEvent, window, cx| match event {
                InputEvent::PressEnter { .. } => this.submit(window, cx),
                InputEvent::Change => cx.notify(),
                _ => {}
            },
        )];
        date.read(cx).focus_handle(cx).focus(window, cx);
        Self {
            accounts,
            account,
            facts,
            focus: cx.focus_handle(),
            date,
            error: None,
            today,
            _subscriptions: subscriptions,
        }
    }

    pub fn error(&self) -> Option<&SharedString> {
        self.error.as_ref()
    }

    fn typed(&self, cx: &Context<Self>) -> Option<NaiveDate> {
        parse_date(&self.date.read(cx).value(), self.today)
    }

    /// Closes the account on the typed day, or says why it can't.
    pub fn submit(&mut self, _: &mut Window, cx: &mut Context<Self>) {
        let Some(on) = self.typed(cx) else {
            self.error = Some("The date isn't one Coffer can read. Type it as 6 Nov 2026.".into());
            cx.notify();
            return;
        };
        let account = self.account;
        let result = self
            .accounts
            .update(cx, |accounts, cx| accounts.close(account, on, cx));
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

impl Render for CloseAccountDialog {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let (title, body, label) = match &self.facts {
            Some(facts) => (
                format!("Close {}?", facts.name),
                facts.body(self.typed(cx), self.today),
                facts.day_label(),
            ),
            None => ("Close account?".to_owned(), String::new(), "Closed on"),
        };
        let cancel = cx.listener(|this, _, _, cx| this.cancel(cx));
        let close = cx.listener(|this, _, window, cx| this.submit(window, cx));
        let on_cancel = cx.entity().downgrade();
        Dialog::new(title, &self.focus)
            .body(caption(body))
            .body(
                div()
                    .flex()
                    .flex_col()
                    .gap(spacing::S2)
                    .child(FieldRow::new(
                        label,
                        date_input(&self.date).id("close-account-date"),
                    )),
            )
            .when_some(self.error.clone(), |this, error| {
                this.body(error_caption(error))
            })
            .action(
                Button::new("close-account-cancel", "Cancel")
                    .with_variant(ButtonVariant::Secondary)
                    .with_surface(color::FLOAT)
                    .on_click(cancel),
            )
            .action(
                Button::new("close-account-close", "Close account")
                    .with_variant(ButtonVariant::Primary)
                    .with_surface(color::FLOAT)
                    .on_click(close),
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

    fn day(m: u32, d: u32) -> NaiveDate {
        NaiveDate::from_ymd_opt(2026, m, d).unwrap()
    }

    fn facts(kind: AccountKind, minor: i64, pots: &[&str]) -> CloseFacts {
        CloseFacts {
            name: "House".into(),
            kind,
            balance: Some(Money::new(minor, Currency::GBP)),
            pots: pots.iter().map(|pot| (*pot).to_owned()).collect(),
            earliest: None,
        }
    }

    #[test]
    fn the_asset_body_is_paper_s() {
        assert_eq!(
            facts(AccountKind::Asset, 38_500_000, &[]).body(Some(day(11, 6)), day(11, 14)),
            "House leaves the Accounts panel and counts as £0 from 6 Nov, so net worth drops by £385,000.00 from then (the sale money arrives in your bank account). Its valuations stay, so earlier net worth doesn't change. You can reopen it."
        );
        assert_eq!(facts(AccountKind::Asset, 1, &[]).day_label(), "Sold on");
        assert_eq!(
            facts(AccountKind::Liability, -1, &[]).day_label(),
            "Paid off on"
        );
    }

    #[test]
    fn a_bank_account_names_its_balance_and_pots() {
        let mut savings = facts(AccountKind::Current, 128_410, &["Holiday", "Rainy day"]);
        savings.name = "Current account".into();
        let body = savings.body(Some(day(11, 9)), day(11, 14));
        assert!(
            body.starts_with(
                "Current account leaves the Accounts panel and net worth after 9 Nov."
            )
        );
        assert!(body.contains("Its balance, £1,284.10, leaves net worth with it."));
        assert!(body.ends_with("Its pots, Holiday and Rainy day, close with it."));
        assert_eq!(savings.day_label(), "Closed on");
        assert_eq!(list(&["A".into(), "B".into(), "C".into()]), "A, B and C");
        let mortgage = facts(AccountKind::Liability, -21_264_000, &[]);
        assert!(
            mortgage
                .body(None, day(11, 14))
                .contains("net worth rises by £212,640.00 from then")
        );
    }
}
