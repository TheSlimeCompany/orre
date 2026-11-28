//! The New-transaction dialog (ROADMAP M3.10; `DESIGN.md` → "Transactions
//! additions" → Manual (cash account), states board `529-0`),
//! `--container-dialog` wide, opened by ⌘N on Transactions and by a cash
//! account's "New transaction" button, with the selected account preset.
//!
//! Field rows: Account (select: every account that holds transactions),
//! Date, Amount, Merchant, Category (the category menu with its filter
//! field) and Note. Amount is an Out/In segmented control (Out first, the
//! common case) beside an unsigned field with the currency's symbol before
//! it and its code after, so the sign is never typed. Cancel (secondary)
//! and "Add transaction" (neutral primary). It saves through core's
//! `manual::create` ([`TransactionsView::create_manual`]); the table reads
//! the database again and selects the new row, first clearing the filters
//! that would hide it (the panel's account, the category, dates,
//! Uncategorised or the search; the others stay).
//!
//! Composed for M3.10, not drawn: Merchant is required, since it's also the
//! row's description (`manual` needs one) and what the table shows; a field
//! that can't be read, or a refusal from core (a closed account, a locked
//! period), is said in `danger` under the form and nothing is added.

use chrono::NaiveDate;
use gpui_kit::component::input::{InputEvent, InputState};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::{
    AppContext as _, Context, Entity, EventEmitter, FocusHandle, Focusable as _, IntoElement,
    ParentElement as _, Render, SharedString, Styled as _, Subscription, Window, div,
};
use orre_core::category::CategoryKey;
use orre_core::id::{AccountId, TransactionId};
use orre_core::manual::ManualTransaction;
use orre_core::money::{Currency, Money};

use crate::accounts::DialogClosed;
use crate::format::{beyond_the_cap, currency_prefix, field_date, parse_amount, parse_date};
use crate::theme::tokens::{color, size, spacing, typography};
use crate::ui::button::{Button, ButtonVariant};
use crate::ui::dialog::Dialog;
use crate::ui::field::{
    FieldRow, SelectField, amount_input, date_input, error_caption, text_input,
};
use crate::ui::marker::Marker;
use crate::ui::menu::{Select, SelectState, filter_groups};
use crate::ui::segmented::Segmented;
use crate::ui::text::TextRoleExt as _;

use super::{TransactionsView, manual_edit, present};

/// Which way the money went.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Direction {
    /// Money out: the common case, first.
    #[default]
    Out,
    In,
}

impl Direction {
    fn key(self) -> &'static str {
        match self {
            Direction::Out => "out",
            Direction::In => "in",
        }
    }
}

/// What the dialog's fields hold.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Typed {
    pub account: Option<(AccountId, Currency)>,
    pub date: String,
    pub direction: Direction,
    /// Unsigned.
    pub amount: String,
    pub merchant: String,
    pub category: Option<CategoryKey>,
}

/// The transaction the fields describe, or why they don't yet.
pub fn read_entry(typed: &Typed, today: NaiveDate) -> Result<ManualTransaction, SharedString> {
    let (account, currency) = typed
        .account
        .ok_or_else(|| SharedString::from("Choose the account it's on."))?;
    let date = parse_date(&typed.date, today).ok_or_else(|| {
        SharedString::from("The date isn't one Orre can read. Type it as 9 Nov 2026.")
    })?;
    let unreadable = || {
        SharedString::from(
            "The amount isn't one Orre can read. Type it as 12.80, and choose Out or In.",
        )
    };
    let amount = parse_amount(&typed.amount, currency)
        .map_err(|error| beyond_the_cap(&error).map_or_else(unreadable, SharedString::from))?;
    if amount.is_negative() {
        return Err("Type the amount without a sign, and choose Out or In.".into());
    }
    if amount.is_zero() {
        return Err("The amount can't be zero.".into());
    }
    let amount = match typed.direction {
        Direction::Out => Money::new(-amount.minor(), currency),
        Direction::In => amount,
    };
    let merchant = typed.merchant.trim();
    if merchant.is_empty() {
        return Err("Say who was paid, or who paid you, under Merchant.".into());
    }
    let mut entry = ManualTransaction::new(account, date, amount, merchant);
    entry.merchant = Some(merchant.to_owned());
    entry.category = typed.category.clone();
    Ok(entry)
}

/// Which of the dialog's selects.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Which {
    Account,
    Category,
}

/// The New-transaction dialog.
pub struct NewTransactionDialog {
    transactions: Entity<TransactionsView>,
    account: Option<AccountId>,
    direction: Direction,
    category: Option<i64>,
    date: Entity<InputState>,
    amount: Entity<InputState>,
    merchant: Entity<InputState>,
    note: Entity<InputState>,
    category_filter: Entity<InputState>,
    account_select: SelectState,
    category_select: SelectState,
    focus: FocusHandle,
    today: NaiveDate,
    error: Option<SharedString>,
    _subscriptions: Vec<Subscription>,
}

impl EventEmitter<DialogClosed> for NewTransactionDialog {}

impl NewTransactionDialog {
    /// The dialog with `account` chosen when it holds transactions.
    pub fn new(
        transactions: Entity<TransactionsView>,
        account: Option<AccountId>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let view = transactions.read(cx);
        let today = view.today();
        let holds = |id: AccountId| {
            view.register()
                .account(id)
                .is_some_and(|account| account.kind.holds_transactions())
        };
        let account = account.filter(|id| holds(*id)).or_else(|| {
            view.register()
                .accounts()
                .iter()
                .find(|account| account.kind.holds_transactions())
                .map(|account| account.id)
        });
        let date = cx.new(|cx| InputState::new(window, cx).default_value(field_date(today)));
        let amount = cx.new(|cx| InputState::new(window, cx).placeholder("0.00"));
        let merchant = cx.new(|cx| InputState::new(window, cx).placeholder("Who was paid"));
        let note = cx.new(|cx| InputState::new(window, cx).placeholder("Optional"));
        let category_filter =
            cx.new(|cx| InputState::new(window, cx).placeholder("Find a category"));
        let submit_on_enter = |this: &mut Self,
                               _: &Entity<InputState>,
                               event: &InputEvent,
                               _: &mut Window,
                               cx: &mut Context<Self>| {
            match event {
                InputEvent::PressEnter { .. } => this.submit(cx),
                // The amount field's font follows what's typed.
                InputEvent::Change => cx.notify(),
                _ => {}
            }
        };
        let mut subscriptions: Vec<Subscription> = [&date, &amount, &merchant, &note]
            .into_iter()
            .map(|input| cx.subscribe_in(input, window, submit_on_enter))
            .collect();
        subscriptions.push(cx.subscribe_in(
            &category_filter,
            window,
            |this, _, event: &InputEvent, _, cx| {
                if matches!(event, InputEvent::Change) {
                    let groups = this.category_groups(cx);
                    this.category_select.highlight_first(&groups);
                    cx.notify();
                }
            },
        ));
        // Typing starts the amount, the one field always typed.
        amount.read(cx).focus_handle(cx).focus(window, cx);
        Self {
            transactions,
            account,
            direction: Direction::Out,
            category: None,
            date,
            amount,
            merchant,
            note,
            category_filter,
            account_select: SelectState::new(cx),
            category_select: SelectState::new(cx),
            focus: cx.focus_handle(),
            today,
            error: None,
            _subscriptions: subscriptions,
        }
    }

    pub fn account(&self) -> Option<AccountId> {
        self.account
    }

    pub fn error(&self) -> Option<&SharedString> {
        self.error.as_ref()
    }

    /// Picks the direction, as Out / In does.
    pub fn set_direction(&mut self, direction: Direction, cx: &mut Context<Self>) {
        self.direction = direction;
        cx.notify();
    }

    /// Picks the category with ID `category`, as the Category select does.
    pub fn set_category(&mut self, category: Option<i64>, cx: &mut Context<Self>) {
        self.category = category;
        cx.notify();
    }

    /// Fills the text fields as the user would (snapshots, tests).
    pub fn fill(
        &mut self,
        amount: &str,
        merchant: &str,
        note: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        for (input, text) in [
            (&self.amount, amount),
            (&self.merchant, merchant),
            (&self.note, note),
        ] {
            let text = SharedString::from(text.to_owned());
            input.update(cx, |input, cx| input.set_value(text, window, cx));
        }
        cx.notify();
    }

    /// Types `text` into Date (tests).
    pub fn set_date(&mut self, text: &str, window: &mut Window, cx: &mut Context<Self>) {
        let text = SharedString::from(text.to_owned());
        self.date
            .update(cx, |input, cx| input.set_value(text, window, cx));
    }

    fn currency(&self, cx: &gpui_kit::App) -> Currency {
        self.account
            .and_then(|id| self.transactions.read(cx).register().account(id))
            .map_or(Currency::GBP, |account| account.currency)
    }

    fn typed(&self, cx: &gpui_kit::App) -> Typed {
        let value = |input: &Entity<InputState>| input.read(cx).value().to_string();
        let register = self.transactions.read(cx).register();
        Typed {
            account: self.account.map(|id| (id, self.currency(cx))),
            date: value(&self.date),
            direction: self.direction,
            amount: value(&self.amount),
            merchant: value(&self.merchant),
            category: self
                .category
                .and_then(|id| register.category(id))
                .map(|category| category.key.clone()),
        }
    }

    /// Adds the transaction, or says why it can't.
    pub fn submit(&mut self, cx: &mut Context<Self>) {
        let entry = match read_entry(&self.typed(cx), self.today) {
            Ok(entry) => entry,
            Err(error) => {
                self.error = Some(error);
                cx.notify();
                return;
            }
        };
        let note = self.note.read(cx).value().to_string();
        let result: Result<TransactionId, SharedString> = self
            .transactions
            .update(cx, |view, cx| view.create_manual(&entry, &note, cx));
        match result {
            Ok(_) => cx.emit(DialogClosed),
            Err(error) => {
                self.error = Some(error);
                cx.notify();
            }
        }
    }

    pub fn cancel(&mut self, cx: &mut Context<Self>) {
        cx.emit(DialogClosed);
    }

    fn category_groups(&self, cx: &gpui_kit::App) -> Vec<crate::ui::menu::MenuGroup> {
        let register = self.transactions.read(cx).register();
        filter_groups(
            &present::category_groups(register),
            &self.category_filter.read(cx).value(),
        )
    }

    fn state(&mut self, which: Which) -> &mut SelectState {
        match which {
            Which::Account => &mut self.account_select,
            Which::Category => &mut self.category_select,
        }
    }

    fn chosen(&self, which: Which) -> Option<SharedString> {
        match which {
            Which::Account => self.account.map(|id| id.to_string().into()),
            Which::Category => self.category.map(|id| id.to_string().into()),
        }
    }

    fn toggle(&mut self, which: Which, open: bool, window: &mut Window, cx: &mut Context<Self>) {
        let chosen = self.chosen(which);
        self.state(which)
            .set_open(open, chosen.as_ref(), window, cx);
        if which == Which::Category && open {
            self.category_filter.update(cx, |filter, cx| {
                filter.set_value("", window, cx);
                filter.focus(window, cx);
            });
        }
        cx.notify();
    }

    fn choose(
        &mut self,
        which: Which,
        key: &SharedString,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match which {
            Which::Account => {
                if let Ok(id) = key.parse::<i64>() {
                    self.account = Some(AccountId::new(id));
                }
            }
            Which::Category => self.category = key.parse::<i64>().ok(),
        }
        self.state(which).set_open(false, None, window, cx);
        cx.notify();
    }

    fn select(&self, which: Which, cx: &mut Context<Self>) -> Select {
        let register = self.transactions.read(cx).register();
        let (id, label, field, groups) = match which {
            Which::Account => {
                let name = self
                    .account
                    .and_then(|id| register.account(id))
                    .map(|account| account.name.clone());
                let field = match name {
                    Some(name) => SelectField::new("new-transaction-account-field", name),
                    None => SelectField::new("new-transaction-account-field", "Choose an account")
                        .placeholder(true),
                };
                (
                    "new-transaction-account",
                    "Account",
                    field,
                    manual_edit::account_groups(register),
                )
            }
            Which::Category => {
                let category = self.category.and_then(|id| register.category(id));
                let field = match category {
                    Some(category) => {
                        SelectField::new("new-transaction-category-field", category.label.clone())
                            .marker(present::marker(category.colour))
                    }
                    None => SelectField::new("new-transaction-category-field", "Choose a category")
                        .marker(Marker::None)
                        .placeholder(true),
                };
                (
                    "new-transaction-category",
                    "Category",
                    field,
                    self.category_groups(cx),
                )
            }
        };
        let moved = groups.clone();
        let state = match which {
            Which::Account => &self.account_select,
            Which::Category => &self.category_select,
        };
        let view = cx.entity().downgrade();
        let (choose, step) = (view.clone(), view.clone());
        Select::new(id, label, field, groups, self.chosen(which), state)
            .max_height(size::CONTROL * super::MENU_ROWS)
            .when(which == Which::Category, |this| {
                this.filter(&self.category_filter)
            })
            .on_toggle(move |open, window, cx| {
                let _ = view.update(cx, |this, cx| this.toggle(which, open, window, cx));
            })
            .on_choose(move |key, window, cx| {
                let _ = choose.update(cx, |this, cx| this.choose(which, key, window, cx));
            })
            .on_move(move |delta, _, cx| {
                let _ = step.update(cx, |this, cx| {
                    this.state(which).move_highlight(&moved, delta);
                    cx.notify();
                });
            })
    }
}

impl Render for NewTransactionDialog {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let currency = self.currency(cx);
        let view = cx.entity().downgrade();
        let direction = Segmented::new("new-transaction-direction", self.direction.key())
            .segment(Direction::Out.key(), "Out")
            .segment(Direction::In.key(), "In")
            .on_choose(move |key, _, cx| {
                let direction = if key.as_ref() == Direction::In.key() {
                    Direction::In
                } else {
                    Direction::Out
                };
                let _ = view.update(cx, |this, cx| this.set_direction(direction, cx));
            });
        let amount = div()
            .flex()
            .flex_1()
            .min_w_0()
            .gap(spacing::S2)
            .child(direction)
            .child(
                amount_input(&self.amount, currency_prefix(currency), cx)
                    .id("new-transaction-amount")
                    .flex_1()
                    .suffix(
                        div()
                            .text_role(typography::MONO_VALUE)
                            .text_color(color::TEXT_MUTED)
                            .child(currency.code().to_owned()),
                    ),
            );
        let fields = div()
            .flex()
            .flex_col()
            .gap(spacing::S2)
            .child(FieldRow::new("Account", self.select(Which::Account, cx)))
            .child(FieldRow::new(
                "Date",
                date_input(&self.date).id("new-transaction-date"),
            ))
            .child(FieldRow::new("Amount", amount))
            .child(FieldRow::new(
                "Merchant",
                text_input(&self.merchant).id("new-transaction-merchant"),
            ))
            .child(FieldRow::new("Category", self.select(Which::Category, cx)))
            .child(FieldRow::new(
                "Note",
                text_input(&self.note).id("new-transaction-note"),
            ));
        let on_cancel = cx.entity().downgrade();
        Dialog::new("New transaction", &self.focus)
            .body(fields)
            .when_some(self.error.clone(), |this, error| {
                this.body(error_caption(error))
            })
            .action(
                Button::new("new-transaction-cancel", "Cancel")
                    .with_variant(ButtonVariant::Secondary)
                    .with_surface(color::FLOAT)
                    .on_click(cx.listener(|this, _, _, cx| this.cancel(cx))),
            )
            .action(
                Button::new("new-transaction-add", "Add transaction")
                    .with_variant(ButtonVariant::Primary)
                    .with_surface(color::FLOAT)
                    .on_click(cx.listener(|this, _, _, cx| this.submit(cx))),
            )
            .on_cancel(move |_, cx| {
                let _ = on_cancel.update(cx, |this, cx| this.cancel(cx));
            })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn typed() -> Typed {
        Typed {
            account: Some((AccountId::new(8), Currency::GBP)),
            date: "7 Nov 2026".to_owned(),
            direction: Direction::Out,
            amount: "12.80".to_owned(),
            merchant: " Farmers' market ".to_owned(),
            category: None,
        }
    }

    #[test]
    fn the_sign_comes_from_out_or_in() {
        let today = NaiveDate::from_ymd_opt(2026, 11, 14).unwrap();
        let entry = read_entry(&typed(), today).unwrap();
        assert_eq!(entry.amount, Money::new(-1280, Currency::GBP));
        assert_eq!(entry.description, "Farmers' market");
        assert_eq!(entry.merchant.as_deref(), Some("Farmers' market"));
        assert_eq!(entry.date, NaiveDate::from_ymd_opt(2026, 11, 7).unwrap());

        let money_in = Typed {
            direction: Direction::In,
            ..typed()
        };
        assert_eq!(
            read_entry(&money_in, today).unwrap().amount,
            Money::new(1280, Currency::GBP)
        );
    }

    #[test]
    fn every_required_field_is_checked() {
        let today = NaiveDate::from_ymd_opt(2026, 11, 14).unwrap();
        let wrong = [
            Typed {
                account: None,
                ..typed()
            },
            Typed {
                date: "31 Feb".to_owned(),
                ..typed()
            },
            Typed {
                amount: "-12.80".to_owned(),
                ..typed()
            },
            Typed {
                amount: "0".to_owned(),
                ..typed()
            },
            Typed {
                amount: "12.805".to_owned(),
                ..typed()
            },
            Typed {
                merchant: "  ".to_owned(),
                ..typed()
            },
        ];
        for typed in wrong {
            assert!(read_entry(&typed, today).is_err(), "{typed:?}");
        }
    }

    #[test]
    fn an_amount_beyond_the_cap_says_what_the_cap_is() {
        let today = NaiveDate::from_ymd_opt(2026, 11, 14).unwrap();
        let read = |amount: &str, direction| {
            let typed = Typed {
                amount: amount.to_owned(),
                direction,
                ..typed()
            };
            read_entry(&typed, today).map(|entry| entry.amount.minor())
        };
        let cap = orre_core::money::MAX_MINOR;
        assert_eq!(read("10,000,000,000.00", Direction::Out), Ok(-cap));
        assert_eq!(read("£10000000000.00", Direction::In), Ok(cap));

        let beyond = Err(SharedString::from(
            "Amounts can't exceed 10,000,000,000.00 GBP.",
        ));
        assert_eq!(read("10000000000.01", Direction::Out), beyond);
        assert_eq!(read("-10000000000.01", Direction::Out), beyond);
        assert_eq!(read("92233720368547758.07", Direction::In), beyond);
        assert_eq!(
            read("-10000000000.00", Direction::Out),
            Err("Type the amount without a sign, and choose Out or In.".into())
        );
        assert_eq!(
            read("12.8x", Direction::Out),
            Err(
                "The amount isn't one Orre can read. Type it as 12.80, and choose Out or In."
                    .into()
            )
        );
    }
}
