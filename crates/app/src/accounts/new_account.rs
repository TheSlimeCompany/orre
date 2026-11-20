//! The New account dialog (`docs/DESIGN.md` → "Accounts panel" → **New
//! account dialog**; Paper `8IR-0`), opened by the `+` on the Accounts
//! section.
//!
//! Name, Bank, Kind (select), Currency (select), and an optional Balance
//! with its date: the account's first entered balance, the starting point
//! its checks count from. Pots aren't added here; imports create them.
//! Cancel, or "Add account" (neutral primary), which adds it and selects it.
//!
//! With Kind set to Asset or Liability (D6.9, Paper `EPI-0`; M3.17) Bank
//! and Preset go, Balance becomes Value (Owed for a liability, unsigned,
//! stored below zero) with its date, a Note row is added, and the caption
//! says nothing is imported. The first value is the account's first
//! valuation. The Assets and liabilities `+` opens it set to Asset
//! ([`NewAccountDialog::with_kind`]).
//!
//! Composed for M3.2 (not drawn): Bank shows only for the kinds a bank
//! exports (D6.9 drops it for kept kinds); a field that can't be read or a
//! refusal from core is said in `danger` under the caption, and nothing is
//! added.

use chrono::NaiveDate;
use gpui_kit::component::input::{InputEvent, InputState};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::{
    AppContext as _, Context, Entity, EventEmitter, FocusHandle, Focusable as _, IntoElement,
    ParentElement as _, Render, SharedString, Styled as _, Subscription, Window, div,
};
use orre_core::account::{AccountKind, EnteredBalance, EnteredSource, NewAccount};
use orre_core::id::AccountId;
use orre_core::money::Currency;

use crate::format::{currency_prefix, field_date, parse_amount, parse_date};
use crate::theme::tokens::{size, spacing};
use crate::ui::button::{Button, ButtonVariant};
use crate::ui::dialog::Dialog;
use crate::ui::field::{
    FieldRow, SelectField, amount_input, caption, date_input, error_caption, text_input,
};
use crate::ui::menu::{MenuGroup, Select, SelectState};

use super::inspector::{currency_groups, kind_groups, kind_label};
use super::{Accounts, DialogClosed};

/// The dialog's title.
const TITLE: &str = "New account";

/// Its primary button.
const ADD: &str = "Add account";

/// The dialog's caption (Paper `8JL-0`).
const CAPTION: &str = "Owed money on a credit card is a negative balance. Skip the balance if your bank’s export includes one: the first import fills it in. Pots are added from imports, not here.";

/// The caption with Kind set to Asset or Liability (Paper `EPO-0`).
const VALUED_CAPTION: &str = "Nothing is imported for an asset: add a value whenever you get a new figure, and Orre holds it until the next. For a liability, enter what you owe; Orre stores it as a negative balance.";

/// `message` as a sentence: a capital first letter and a full stop.
pub(crate) fn sentence(message: &str) -> SharedString {
    let mut chars = message.chars();
    let mut text: String = match chars.next() {
        Some(first) => first.to_uppercase().chain(chars).collect(),
        None => String::new(),
    };
    if !text.ends_with(['.', '?', '!']) {
        text.push('.');
    }
    text.into()
}

/// Reads an optional balance and its date. `Ok(None)` when no balance was
/// typed.
pub(super) fn read_balance(
    amount: &str,
    date: &str,
    currency: Currency,
    today: NaiveDate,
) -> Result<Option<(NaiveDate, orre_core::money::Money)>, SharedString> {
    if amount.trim().is_empty() {
        return Ok(None);
    }
    let balance = parse_amount(amount, currency).map_err(|error| {
        SharedString::from(format!(
            "The balance isn't an amount Orre can read ({error}). Type it as 1,284.10, with a minus if it's below zero."
        ))
    })?;
    let date = parse_date(date, today).ok_or_else(|| {
        SharedString::from("The date isn't one Orre can read. Type it as 9 Nov 2026.")
    })?;
    Ok(Some((date, balance)))
}

/// Which of the dialog's selects.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Which {
    Kind,
    Currency,
}

/// Emitted when the dialog has added an account, just before
/// [`DialogClosed`]: the Import dialog (which hosts this one for "New
/// account…" in its Account menu) chooses it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AccountCreated(pub AccountId);

/// What the dialog starts with when opened from the Import dialog: the
/// fields a file settles, and the caption that says so.
pub struct NewAccountPrefill {
    /// The file's currency, when it has one.
    pub currency: Option<Currency>,
    /// The detected preset's bank name.
    pub bank: Option<String>,
    pub kind: AccountKind,
    /// Replaces the dialog's caption.
    pub caption: SharedString,
}

/// The New account dialog.
pub struct NewAccountDialog {
    accounts: Entity<Accounts>,
    focus: FocusHandle,
    name: Entity<InputState>,
    bank: Entity<InputState>,
    balance: Entity<InputState>,
    date: Entity<InputState>,
    note: Entity<InputState>,
    kind: AccountKind,
    currency: Currency,
    kind_select: SelectState,
    currency_select: SelectState,
    error: Option<SharedString>,
    caption: Option<SharedString>,
    today: NaiveDate,
    _subscriptions: Vec<Subscription>,
}

impl EventEmitter<DialogClosed> for NewAccountDialog {}
impl EventEmitter<AccountCreated> for NewAccountDialog {}

impl NewAccountDialog {
    pub fn new(accounts: Entity<Accounts>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        Self::with_kind(accounts, AccountKind::Current, window, cx)
    }

    /// The dialog with Kind set to `kind`: the Assets and liabilities `+`
    /// opens it set to Asset.
    pub fn with_kind(
        accounts: Entity<Accounts>,
        kind: AccountKind,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let today = accounts.read(cx).data().today;
        let name = cx.new(|cx| InputState::new(window, cx).placeholder("Name"));
        let bank = cx.new(|cx| InputState::new(window, cx).placeholder("Bank"));
        let balance = cx.new(|cx| InputState::new(window, cx).placeholder("Optional"));
        let date = cx.new(|cx| InputState::new(window, cx).default_value(field_date(today)));
        let note = cx.new(|cx| InputState::new(window, cx).placeholder("Optional"));
        let submit_on_enter = |this: &mut Self,
                               _: &Entity<InputState>,
                               event: &InputEvent,
                               window: &mut Window,
                               cx: &mut Context<Self>| {
            if let InputEvent::PressEnter { .. } = event {
                this.submit(window, cx);
            }
        };
        let subscriptions = [&name, &bank, &balance, &date, &note]
            .into_iter()
            .map(|input| cx.subscribe_in(input, window, submit_on_enter))
            .collect();
        // The first field takes focus, so typing starts the name.
        name.read(cx).focus_handle(cx).focus(window, cx);
        Self {
            accounts,
            focus: cx.focus_handle(),
            name,
            bank,
            balance,
            date,
            note,
            kind,
            currency: Currency::GBP,
            kind_select: SelectState::new(cx),
            currency_select: SelectState::new(cx),
            error: None,
            caption: None,
            today,
            _subscriptions: subscriptions,
        }
    }

    /// Starts the fields from what an import file settles (Import's "New
    /// account…"): the currency, the bank, the kind and a caption saying so.
    pub fn prefill(
        &mut self,
        prefill: NewAccountPrefill,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let Some(currency) = prefill.currency {
            self.currency = currency;
        }
        if let Some(bank) = prefill.bank {
            self.bank
                .update(cx, |input, cx| input.set_value(bank, window, cx));
        }
        self.kind = prefill.kind;
        self.caption = Some(prefill.caption);
        cx.notify();
    }

    /// Types `name` into the Name field.
    #[cfg(test)]
    pub fn type_name(&mut self, name: &str, window: &mut Window, cx: &mut Context<Self>) {
        self.name
            .update(cx, |input, cx| input.set_value(name.to_owned(), window, cx));
    }

    /// The kind chosen so far.
    pub fn kind(&self) -> AccountKind {
        self.kind
    }

    /// The currency chosen so far.
    pub fn currency(&self) -> Currency {
        self.currency
    }

    pub fn error(&self) -> Option<&SharedString> {
        self.error.as_ref()
    }

    /// Adds the account from the fields, or says why it can't.
    pub fn submit(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let _ = window;
        let value =
            |input: &Entity<InputState>, cx: &Context<Self>| input.read(cx).value().to_string();
        let balance = match read_balance(
            &value(&self.balance, cx),
            &value(&self.date, cx),
            self.currency,
            self.today,
        ) {
            Ok(balance) => balance,
            Err(error) => {
                self.error = Some(error);
                cx.notify();
                return;
            }
        };
        let valued = !self.kind.holds_transactions();
        let balance = match balance {
            Some((date, typed)) if valued => {
                match super::add_balance::stored_value(self.kind, typed) {
                    Ok(stored) => Some((date, stored)),
                    Err(error) => {
                        self.error = Some(error);
                        cx.notify();
                        return;
                    }
                }
            }
            balance => balance,
        };
        let note = value(&self.note, cx).trim().to_owned();
        let note = (valued && !note.is_empty()).then_some(note);
        let bank = value(&self.bank, cx);
        let account = NewAccount {
            name: value(&self.name, cx),
            institution: (self.kind.is_imported() && !bank.trim().is_empty()).then_some(bank),
            kind: self.kind,
            currency: self.currency,
            parent: None,
            preset: None,
        };
        let balance = balance.map(|(date, balance)| EnteredBalance {
            date,
            balance,
            source: EnteredSource::for_kind(self.kind),
            note,
        });
        let result = self.accounts.update(cx, |accounts, cx| {
            accounts.create(&account, balance.as_ref(), cx)
        });
        match result {
            Ok(id) => {
                cx.emit(AccountCreated(id));
                cx.emit(DialogClosed);
            }
            Err(error) => {
                self.error = Some(sentence(&error));
                cx.notify();
            }
        }
    }

    pub fn cancel(&mut self, cx: &mut Context<Self>) {
        cx.emit(DialogClosed);
    }

    fn state(&mut self, which: Which) -> &mut SelectState {
        match which {
            Which::Kind => &mut self.kind_select,
            Which::Currency => &mut self.currency_select,
        }
    }

    fn chosen(&self, which: Which) -> SharedString {
        match which {
            Which::Kind => self.kind.as_str().into(),
            Which::Currency => self.currency.code().to_owned().into(),
        }
    }

    fn toggle(&mut self, which: Which, open: bool, window: &mut Window, cx: &mut Context<Self>) {
        let chosen = self.chosen(which);
        self.state(which).set_open(open, Some(&chosen), window, cx);
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
            Which::Kind => {
                if let Some(kind) = AccountKind::from_key(key) {
                    self.kind = kind;
                }
            }
            Which::Currency => {
                if let Ok(currency) = Currency::new(key) {
                    self.currency = currency;
                }
            }
        }
        self.state(which).set_open(false, None, window, cx);
        cx.notify();
    }

    fn groups(which: Which) -> Vec<MenuGroup> {
        match which {
            Which::Kind => kind_groups(|_| None),
            Which::Currency => currency_groups(),
        }
    }

    fn select(&self, which: Which, cx: &mut Context<Self>) -> Select {
        let (id, label, value) = match which {
            Which::Kind => ("new-account-kind", "Kind", kind_label(self.kind).to_owned()),
            Which::Currency => (
                "new-account-currency",
                "Currency",
                self.currency.code().to_owned(),
            ),
        };
        let state = match which {
            Which::Kind => &self.kind_select,
            Which::Currency => &self.currency_select,
        };
        let groups = Self::groups(which);
        let view = cx.entity().downgrade();
        let (choose, step) = (view.clone(), view.clone());
        let moved = groups.clone();
        Select::new(
            id,
            label,
            SelectField::new(format!("{id}-field"), value),
            groups,
            Some(self.chosen(which)),
            state,
        )
        .when(which == Which::Currency, |this| {
            this.max_height(size::CONTROL * 8.)
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

impl NewAccountDialog {
    /// What the dialog shows, as plain data (test-only, ROADMAP U.3): each
    /// field's label and what's in it; the amount's date is a row labelled
    /// "" after it.
    #[cfg(test)]
    pub fn describe(&self, cx: &gpui_kit::App) -> crate::ui::describe::Dialog {
        use crate::ui::describe::{Action, Dialog};
        let value = |input: &Entity<InputState>| input.read(cx).value().to_string();
        let mut fields = vec![("Name".to_owned(), value(&self.name))];
        if self.kind.is_imported() {
            fields.push(("Bank".to_owned(), value(&self.bank)));
        }
        fields.push(("Kind".to_owned(), kind_label(self.kind).to_owned()));
        fields.push(("Currency".to_owned(), self.currency.code().to_owned()));
        let label = super::add_balance::Wording::of(self.kind).label;
        fields.push((label.to_owned(), value(&self.balance)));
        fields.push((String::new(), value(&self.date)));
        if !self.kind.holds_transactions() {
            fields.push(("Note".to_owned(), value(&self.note)));
        }
        Dialog {
            title: TITLE.to_owned(),
            fields,
            error: self.error.as_ref().map(ToString::to_string),
            actions: vec![Action::button("Cancel", true), Action::button(ADD, true)],
        }
    }
}

impl Render for NewAccountDialog {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let has_bank = self.kind.is_imported();
        let valued = !self.kind.holds_transactions();
        let label = super::add_balance::Wording::of(self.kind).label;
        let fields = div()
            .flex()
            .flex_col()
            .gap(spacing::S2)
            .child(FieldRow::new(
                "Name",
                text_input(&self.name).id("new-account-name"),
            ))
            .when(has_bank, |this| {
                this.child(FieldRow::new(
                    "Bank",
                    text_input(&self.bank).id("new-account-bank"),
                ))
            })
            .child(FieldRow::new("Kind", self.select(Which::Kind, cx)))
            .child(FieldRow::new("Currency", self.select(Which::Currency, cx)))
            .child(FieldRow::new(
                label,
                div()
                    .flex()
                    .flex_1()
                    .min_w_0()
                    .gap(spacing::S2)
                    .child(
                        amount_input(&self.balance, currency_prefix(self.currency), cx)
                            .id("new-account-balance")
                            .flex_1(),
                    )
                    .child(date_input(&self.date).id("new-account-date")),
            ))
            .when(valued, |this| {
                this.child(FieldRow::new(
                    "Note",
                    text_input(&self.note).id("new-account-note"),
                ))
            });
        let cancel = cx.listener(|this, _, _, cx| this.cancel(cx));
        let add = cx.listener(|this, _, window, cx| this.submit(window, cx));
        let on_cancel = cx.entity().downgrade();
        Dialog::new(TITLE, &self.focus)
            .body(fields)
            .body(caption(self.caption.clone().unwrap_or_else(|| {
                if valued { VALUED_CAPTION } else { CAPTION }.into()
            })))
            .when_some(self.error.clone(), |this, error| {
                this.body(error_caption(error))
            })
            .action(
                Button::new("new-account-cancel", "Cancel")
                    .with_variant(ButtonVariant::Secondary)
                    .with_surface(crate::theme::tokens::color::FLOAT)
                    .on_click(cancel),
            )
            .action(
                Button::new("new-account-add", ADD)
                    .with_variant(ButtonVariant::Primary)
                    .with_surface(crate::theme::tokens::color::FLOAT)
                    .on_click(add),
            )
            .on_cancel(move |_, cx| {
                let _ = on_cancel.update(cx, |this, cx| this.cancel(cx));
            })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn errors_read_as_sentences() {
        assert_eq!(
            sentence("an account needs a name"),
            "An account needs a name."
        );
        assert_eq!(sentence("Already one."), "Already one.");
    }

    #[test]
    fn the_balance_is_optional_but_must_read_when_typed() {
        let today = NaiveDate::from_ymd_opt(2026, 11, 14).unwrap();
        assert_eq!(
            read_balance("  ", "nonsense", Currency::GBP, today),
            Ok(None)
        );
        let (date, balance) = read_balance("-24.10", "9 Nov 2026", Currency::GBP, today)
            .unwrap()
            .unwrap();
        assert_eq!(
            (date.to_string(), balance.minor()),
            ("2026-11-09".to_owned(), -2_410)
        );
        assert!(read_balance("12.345", "9 Nov 2026", Currency::GBP, today).is_err());
        assert!(read_balance("12", "31 Feb 2026", Currency::GBP, today).is_err());
    }
}
