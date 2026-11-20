//! The Add balance dialog for a bank or cash account (Paper `I7C-0` on the
//! Accounts panel · states board `8D8-0`; drawn for M3.2 and approved by
//! the user, 2026-11-14), opened by "Add balance…" in the account inspector.
//!
//! Adapted from D6.9's "Add a balance for Mortgage": Balance (the currency's
//! symbol, mono, signed: a minus when overdrawn) with its date in a 96px
//! field, Note, a caption, then Cancel and "Add balance" (neutral primary).
//! It records a `manual` entered balance ([`orre_core::account::add_balance`]):
//! one per account and day, after the account's lock date, checked against
//! the transactions like any other. A refusal is said in `danger` under the
//! caption.
//!
//! On an asset or liability account it records a valuation (M3.17). For a
//! liability it is D6.9's drawn "Add a balance for Mortgage" (Paper
//! `FS0-0`): Owed, unsigned, stored below zero, the caption saying what
//! Orre will store as it's typed. For an asset it is composed from that
//! one (decided with the user, 2026-11-14): "Add a valuation for House",
//! Value, unsigned, and "Add valuation". Neither takes a minus.
//!
//! On a cash account it is the Cash count dialog (D6.9, `EPF-0`; M3.10):
//! "Count cash for Cash", Counted (the currency's symbol and the date),
//! then, from the second count, Orre's expectation and the Difference
//! (see [`Counting`]). The footer is Cancel, "Save count only" and, when
//! the count is off, "Add −£5.00 and save", which also adds the "Cash count
//! difference" transaction. Nothing is added unless that is chosen, and
//! Enter saves the count only. The first count is the starting point and
//! has only a plain save. It opens through the same
//! [`super::AccountsEvent::AddBalanceRequested`] as the other kinds.

use chrono::NaiveDate;
use gpui_kit::component::Disableable as _;
use gpui_kit::component::input::{InputEvent, InputState};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::{
    AppContext as _, Context, Entity, EventEmitter, FocusHandle, Focusable as _, IntoElement,
    ParentElement as _, Render, SharedString, Styled as _, Subscription, Window, div,
};
use orre_core::account::count::Expectation;
use orre_core::account::{AccountKind, EnteredBalance, EnteredSource};
use orre_core::id::AccountId;
use orre_core::money::Currency;
use orre_core::money::Money;
use orre_core::reconciliation::BalanceSource;

use crate::format::{
    Sign, currency_prefix, field_date, list_date, money, parse_amount, parse_date,
};
use crate::theme::tokens::{color, size, spacing, typography};
use crate::ui::button::{Button, ButtonVariant};
use crate::ui::dialog::Dialog;
use crate::ui::field::{FieldRow, amount_input, caption, date_input, error_caption, text_input};
use crate::ui::icon::Glyph;
use crate::ui::text::TextRoleExt as _;

use super::new_account::{read_balance, sentence};
use super::{Accounts, DialogClosed};

/// The dialog's caption (Paper `I7I-0`).
const CAPTION: &str = "Your balance at the end of that day, as your bank shows it. Type a minus if the account is overdrawn. Orre checks it against the transactions since the last balance.";

/// The caption for an asset (composed from `FS6-0`).
const ASSET_CAPTION: &str = "What it's worth on that day, from a survey, an estimate or a statement. Orre holds it until the next one; it isn't checked against transactions.";

/// The caption of a cash count after the first (composed from `EPF-0`).
/// It doesn't name the difference's category: core files it in Other only
/// while that category exists, under whatever the user calls it now.
const COUNT_CAPTION: &str = "Orre checks your count against the last one and what you've entered since. Adding the difference puts a \u{201c}Cash count difference\u{201d} entry on that day, so the check passes; saving the count only leaves it as a failed check to look into.";

/// The caption of the first cash count (composed from `EPF-0`).
const FIRST_COUNT_CAPTION: &str = "This is your first count, so it's the starting point: Orre counts forward from it and checks every later count against what you enter.";

/// A count's plain save, when it agrees or is the first.
const SAVE: &str = "Save count";

/// A count's save that adds no difference.
const SAVE_ONLY: &str = "Save count only";

/// The button that adds the difference and saves: "Add −£5.00 and save".
fn add_difference_label(difference: Money) -> String {
    format!("Add {} and save", money(difference, Sign::Negative))
}

/// What a cash count shows below Counted, once what was typed can be read.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Counting {
    pub expectation: Expectation,
}

impl Counting {
    /// "£23.00 from transactions since the 1 Nov count": what Orre
    /// expects. `None` for the first count.
    pub fn expected_text(&self, today: NaiveDate) -> Option<String> {
        let expected = self.expectation.expected?;
        let since = self.expectation.since?;
        Some(format!(
            "{} from transactions since the {} count",
            money(expected, Sign::Negative),
            list_date(since, today)
        ))
    }

    /// The difference when the count is off ("−£5.00"), `None` when
    /// it agrees or there's nothing to compare it with.
    pub fn difference(&self) -> Option<Money> {
        self.expectation
            .difference
            .filter(|difference| !difference.is_zero())
    }
}

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
                "Type what you owe without a minus: Orre stores it below zero.".into()
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

/// A liability's caption (Paper `FS6-0`), with what Orre will store for
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
        "Type the amount the statement says you owe. Orre stores it {stored} and holds it until the next balance. A valuation sets the balance, so it's never a failed check."
    )
}

/// `message` with each `2026-11-17` in it written as the date field shows
/// it (`17 Nov 2026`): core's errors name their dates in ISO form. Only a
/// date standing as a word is rewritten, not one inside a path, a file name
/// or a timestamp (`/tmp/orre-2026-11-17/`, `2026-11-17T09:00`).
fn field_dates(message: &str) -> String {
    // Characters that make a date part of something larger.
    let joins_before = |c: char| c.is_alphanumeric() || matches!(c, '/' | '\\' | '-' | '_' | '.');
    let joins_after = |c: char| c.is_alphanumeric() || matches!(c, '/' | '\\' | '-' | '_' | ':');
    let mut shown = String::with_capacity(message.len());
    let mut before: Option<char> = None;
    let mut rest = message;
    while let Some(next) = rest.chars().next() {
        let date = rest
            .get(..10)
            .filter(|text| {
                text.bytes().enumerate().all(|(at, byte)| {
                    matches!(at, 4 | 7) == (byte == b'-') && (byte == b'-' || byte.is_ascii_digit())
                })
            })
            .filter(|_| !before.is_some_and(joins_before))
            .filter(|_| !rest[10..].chars().next().is_some_and(joins_after))
            .and_then(|text| NaiveDate::parse_from_str(text, "%Y-%m-%d").ok());
        if let Some(date) = date {
            shown.push_str(&field_date(date));
            before = Some('7');
            rest = &rest[10..];
        } else {
            shown.push(next);
            before = Some(next);
            rest = &rest[next.len_utf8()..];
        }
    }
    shown
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
    /// For a cash account: what Orre expects of what's typed, when it
    /// can be read.
    counting: Option<Counting>,
    /// Whether the account has no count yet: this one is the starting point.
    first_count: bool,
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
        let first_count = data
            .detail(account)
            .is_none_or(|detail| detail.balances.is_empty());
        let (title, kind, currency) = data.node(account).map_or_else(
            || ("Add a balance".into(), AccountKind::Current, Currency::GBP),
            |node| {
                let wording = Wording::of(node.account.kind);
                let title = if node.account.kind == AccountKind::Cash {
                    format!("Count cash for {}", super::name(&node.account.name))
                } else {
                    format!("{} {}", wording.title, super::name(&node.account.name))
                };
                (title.into(), node.account.kind, node.account.currency)
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
                // A liability's caption says what will be stored, and a
                // cash count says what it differs by.
                // A refusal is about what was typed: it goes when that changes.
                InputEvent::Change => {
                    this.error = None;
                    this.refresh_counting(cx);
                    cx.notify();
                }
                _ => {}
            }
        };
        let subscriptions = [&amount, &date, &note]
            .into_iter()
            .map(|input| cx.subscribe_in(input, window, submit_on_enter))
            .collect();
        amount.read(cx).focus_handle(cx).focus(window, cx);
        let mut dialog = Self {
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
            counting: None,
            first_count,
            _subscriptions: subscriptions,
        };
        dialog.refresh_counting(cx);
        dialog
    }

    /// Whether this counts cash rather than adding a balance.
    pub fn is_count(&self) -> bool {
        self.kind == AccountKind::Cash
    }

    /// What a cash count shows: Orre's expectation and the difference.
    pub fn counting(&self) -> Option<&Counting> {
        self.counting.as_ref()
    }

    /// Types `amount` into the Counted field (snapshots and tests).
    pub fn type_amount(&mut self, amount: &str, window: &mut Window, cx: &mut Context<Self>) {
        self.amount.update(cx, |input, cx| {
            input.set_value(SharedString::from(amount.to_owned()), window, cx)
        });
        self.refresh_counting(cx);
        cx.notify();
    }

    /// Types `date` into the Date field (snapshots).
    pub fn type_date(&mut self, date: &str, window: &mut Window, cx: &mut Context<Self>) {
        self.date.update(cx, |input, cx| {
            input.set_value(SharedString::from(date.to_owned()), window, cx)
        });
        self.refresh_counting(cx);
        cx.notify();
    }

    /// Reads what Orre expects of the count in the fields, if they can
    /// be read.
    fn refresh_counting(&mut self, cx: &mut Context<Self>) {
        self.counting = None;
        if !self.is_count() {
            return;
        }
        // A negative count is refused when saved, so it offers no difference.
        let counted = parse_amount(&self.amount.read(cx).value(), self.currency)
            .ok()
            .filter(|counted| !counted.is_negative());
        let date = parse_date(&self.date.read(cx).value(), self.today);
        if let (Some(counted), Some(date)) = (counted, date) {
            self.counting = self
                .accounts
                .read(cx)
                .cash_expectation(self.account, date, counted)
                .ok()
                .map(|expectation| Counting { expectation });
        }
    }

    pub fn error(&self) -> Option<&SharedString> {
        self.error.as_ref()
    }

    /// "This account already has a balance on 17 Nov 2026. Choose another
    /// day.", while the Date field holds a day with an entered balance:
    /// core allows one per account and day, and refuses the second. Read
    /// from the account's balances as core reads them: an opening balance
    /// on the account row doesn't count, and a file's balance isn't listed.
    /// `None` when the date can't be read or the day is free. Adding is
    /// held back while it shows, so it's the only thing said.
    pub fn day_taken(&self, cx: &gpui_kit::App) -> Option<SharedString> {
        let date = parse_date(&self.date.read(cx).value(), self.today)?;
        let accounts = self.accounts.read(cx);
        let taken = accounts
            .data()
            .detail(self.account)?
            .balances
            .iter()
            .any(|balance| balance.date == date && balance.source != BalanceSource::Opening);
        taken.then(|| {
            let noun = match self.kind {
                AccountKind::Cash => "count",
                AccountKind::Asset => "valuation",
                _ => "balance",
            };
            format!(
                "This account already has a {noun} on {}. Choose another day.",
                field_date(date)
            )
            .into()
        })
    }

    /// The Date field: in a 1px `danger` border while its day already has
    /// a balance (`IRD-0`; no alert icon, the 96px field has no room).
    fn date_field(&self, cx: &gpui_kit::App) -> gpui_kit::component::input::Input {
        date_input(&self.date).when(self.day_taken(cx).is_some(), |this| {
            this.border(size::HAIRLINE).border_color(color::DANGER)
        })
    }

    /// The dialog's caption, then what's wrong in `danger` caption type
    /// (the day taken, or a refusal), 8px under it as one group (`IRD-0`).
    fn captions(
        &self,
        text: impl Into<SharedString>,
        taken: Option<SharedString>,
    ) -> gpui_kit::Div {
        div()
            .flex()
            .flex_col()
            .gap(spacing::S2)
            .child(caption(text))
            .when_some(taken, |this, taken| this.child(error_caption(taken)))
            .when_some(self.error.clone(), |this, error| {
                this.child(error_caption(error))
            })
    }

    /// Records the balance from the fields, or says why it can't. A cash
    /// count is saved on its own: the difference is added only on request
    /// ([`Self::save_with_difference`]).
    pub fn submit(&mut self, _: &mut Window, cx: &mut Context<Self>) {
        if self.is_count() {
            self.save_count(false, cx);
            return;
        }
        if self.day_taken(cx).is_some() {
            return;
        }
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
                self.error = Some(sentence(&field_dates(&error)));
                cx.notify();
            }
        }
    }

    /// "Add −£5.00 and save": the count and the difference, together.
    pub fn save_with_difference(&mut self, cx: &mut Context<Self>) {
        self.save_count(true, cx);
    }

    /// Stores the cash count in the fields, with its difference when asked.
    fn save_count(&mut self, add_difference: bool, cx: &mut Context<Self>) {
        if self.day_taken(cx).is_some() {
            return;
        }
        let value =
            |input: &Entity<InputState>, cx: &Context<Self>| input.read(cx).value().to_string();
        let amount = value(&self.amount, cx);
        let read = if amount.trim().is_empty() {
            Err("Type what you counted, as 23.00.".into())
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
        if balance.is_negative() {
            self.error = Some("Type what you counted without a minus.".into());
            cx.notify();
            return;
        }
        let count = EnteredBalance {
            date,
            balance,
            source: EnteredSource::Manual,
            note: None,
        };
        let account = self.account;
        let result = self.accounts.update(cx, |accounts, cx| {
            accounts.count_cash(account, &count, add_difference, cx)
        });
        match result {
            Ok(()) => cx.emit(DialogClosed),
            Err(error) => {
                self.error = Some(sentence(&field_dates(&error)));
                cx.notify();
            }
        }
    }

    pub fn cancel(&mut self, cx: &mut Context<Self>) {
        cx.emit(DialogClosed);
    }

    /// The Cash count dialog: Counted, Orre's expectation, the
    /// Difference and the footer.
    fn render_count(&mut self, cx: &mut Context<Self>) -> Dialog {
        let counting = self.counting;
        let expected = counting.and_then(|counting| counting.expected_text(self.today));
        let difference = counting.and_then(|counting| counting.difference());
        let first = self.first_count;
        // A count dated before the starting point has nothing to differ from.
        let has_base = expected.is_some();
        let fields = div()
            .flex()
            .flex_col()
            .gap(spacing::S2)
            .child(FieldRow::new(
                "Counted",
                div()
                    .flex()
                    .flex_1()
                    .min_w_0()
                    .gap(spacing::S2)
                    .child(
                        amount_input(&self.amount, currency_prefix(self.currency), cx)
                            .id("count-amount")
                            .flex_1(),
                    )
                    .child(self.date_field(cx).id("count-date")),
            ))
            .when_some(expected, |this, expected| {
                this.child(FieldRow::new(
                    "Orre",
                    div()
                        .flex_1()
                        .min_w_0()
                        .px(spacing::S2)
                        .text_role(typography::BODY)
                        .text_color(color::TEXT)
                        .child(expected),
                ))
            })
            .when(has_base && !first, |this| {
                this.child(FieldRow::new(
                    "Difference",
                    self.difference_value(difference),
                ))
            });
        let taken = self.day_taken(cx);
        let cancel = cx.listener(|this, _, _, cx| this.cancel(cx));
        let on_cancel = cx.entity().downgrade();
        Dialog::new(self.title.clone(), &self.focus)
            .body(fields)
            .body(self.captions(
                if first {
                    FIRST_COUNT_CAPTION
                } else {
                    COUNT_CAPTION
                },
                taken.clone(),
            ))
            .action(
                Button::new("count-cancel", "Cancel")
                    .with_variant(ButtonVariant::Secondary)
                    .with_surface(color::FLOAT)
                    .on_click(cancel),
            )
            .when_some(difference, |this, difference| {
                let label = add_difference_label(difference);
                this.action(
                    Button::new("count-save-only", SAVE_ONLY)
                        .with_variant(ButtonVariant::Secondary)
                        .with_surface(color::FLOAT)
                        .disabled(taken.is_some())
                        .on_click(cx.listener(|this, _, _, cx| this.save_count(false, cx))),
                )
                .action(
                    Button::new("count-add", label)
                        .with_variant(ButtonVariant::Primary)
                        .with_surface(color::FLOAT)
                        .disabled(taken.is_some())
                        .on_click(cx.listener(|this, _, _, cx| this.save_with_difference(cx))),
                )
            })
            .when(difference.is_none(), |this| {
                this.action(
                    Button::new("count-save", SAVE)
                        .with_variant(ButtonVariant::Primary)
                        .with_surface(color::FLOAT)
                        .disabled(taken.is_some())
                        .on_click(cx.listener(|this, _, _, cx| this.save_count(false, cx))),
                )
            })
            .on_cancel(move |_, cx| {
                let _ = on_cancel.update(cx, |this, cx| this.cancel(cx));
            })
    }

    /// The Difference row's value: `warning` with the alert icon when the
    /// count is off, unresolved rather than failed (as an unbalanced split);
    /// plain when it agrees.
    fn difference_value(&self, difference: Option<Money>) -> gpui_kit::AnyElement {
        let shown = self.difference_text(difference);
        let tone = if difference.is_some() {
            color::WARNING
        } else {
            color::TEXT_MUTED
        };
        div()
            .flex()
            .flex_1()
            .min_w_0()
            .items_center()
            .gap(spacing::S1_5)
            .px(spacing::S2)
            .when(difference.is_some(), |this| {
                this.child(Glyph::Warning.render().text_color(color::WARNING))
            })
            .child(
                div()
                    .text_role(typography::MONO_AMOUNT)
                    .text_color(tone)
                    .child(shown),
            )
            .into_any_element()
    }
}

impl AddBalanceDialog {
    /// The Difference row's amount: £0.00 when the count agrees.
    fn difference_text(&self, difference: Option<Money>) -> String {
        money(
            difference.unwrap_or(Money::zero(self.currency)),
            Sign::Negative,
        )
    }

    /// The caption under the fields of a balance or valuation: a
    /// liability's says what will be stored for what's typed.
    fn caption_text(&self, cx: &gpui_kit::App) -> String {
        match self.kind {
            AccountKind::Liability => {
                owed_caption(parse_amount(&self.amount.read(cx).value(), self.currency).ok())
            }
            AccountKind::Asset => ASSET_CAPTION.to_owned(),
            _ => CAPTION.to_owned(),
        }
    }

    /// What the dialog shows, as plain data (test-only, ROADMAP U.3): each
    /// field's label and what's in it (the amount's date a row labelled ""
    /// after it), Orre's expectation and the Difference on a count, and
    /// a liability's caption (it names what will be stored) as a row
    /// labelled "". The fixed captions are left out.
    #[cfg(test)]
    pub fn describe(&self, cx: &gpui_kit::App) -> crate::ui::describe::Dialog {
        use crate::ui::describe::{Action, Dialog};
        let value = |input: &Entity<InputState>| input.read(cx).value().to_string();
        let cancel = Action::button("Cancel", true);
        // Adding waits while the date has a balance (`IRD-0`).
        let free = self.day_taken(cx).is_none();
        if self.is_count() {
            let expected = self
                .counting
                .and_then(|counting| counting.expected_text(self.today));
            let difference = self.counting.and_then(|counting| counting.difference());
            let mut fields = vec![
                ("Counted".to_owned(), value(&self.amount)),
                (String::new(), value(&self.date)),
            ];
            let has_base = expected.is_some();
            fields.extend(expected.map(|expected| ("Orre".to_owned(), expected)));
            if has_base && !self.first_count {
                fields.push(("Difference".to_owned(), self.difference_text(difference)));
            }
            let actions = match difference {
                Some(difference) => vec![
                    cancel,
                    Action::button(SAVE_ONLY, free),
                    Action::button(add_difference_label(difference), free),
                ],
                None => vec![cancel, Action::button(SAVE, free)],
            };
            return Dialog {
                title: self.title.to_string(),
                fields,
                error: self
                    .day_taken(cx)
                    .or_else(|| self.error.clone())
                    .map(|error| error.to_string()),
                actions,
            };
        }
        let wording = Wording::of(self.kind);
        let mut fields = vec![
            (wording.label.to_owned(), value(&self.amount)),
            (String::new(), value(&self.date)),
            ("Note".to_owned(), value(&self.note)),
        ];
        if self.kind == AccountKind::Liability {
            fields.push((String::new(), self.caption_text(cx)));
        }
        Dialog {
            title: self.title.to_string(),
            fields,
            error: self
                .day_taken(cx)
                .or_else(|| self.error.clone())
                .map(|error| error.to_string()),
            actions: vec![cancel, Action::button(wording.button, free)],
        }
    }
}

impl Render for AddBalanceDialog {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if self.is_count() {
            return self.render_count(cx).into_any_element();
        }
        let wording = Wording::of(self.kind);
        let caption_text = self.caption_text(cx);
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
                    .child(self.date_field(cx).id("add-balance-date")),
            ))
            .child(FieldRow::new(
                "Note",
                text_input(&self.note).id("add-balance-note"),
            ));
        let taken = self.day_taken(cx);
        let cancel = cx.listener(|this, _, _, cx| this.cancel(cx));
        let add = cx.listener(|this, _, window, cx| this.submit(window, cx));
        let on_cancel = cx.entity().downgrade();
        Dialog::new(self.title.clone(), &self.focus)
            .body(fields)
            .body(self.captions(caption_text, taken.clone()))
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
                    .disabled(taken.is_some())
                    .on_click(add),
            )
            .on_cancel(move |_, cx| {
                let _ = on_cancel.update(cx, |this, cx| this.cancel(cx));
            })
            .into_any_element()
    }
}

#[cfg(test)]
mod tests {
    use orre_core::money::Currency;

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
            "Type the amount the statement says you owe. Orre stores it as \u{2212}£212,640.00 and holds it until the next balance. A valuation sets the balance, so it's never a failed check."
        );
        assert!(owed_caption(None).contains("stores it below zero"));
        assert_eq!(Wording::of(AccountKind::Asset).button, "Add valuation");
        assert_eq!(Wording::of(AccountKind::Cash).label, "Balance");
    }

    #[test]
    fn core_dates_are_written_as_the_field_shows_them() {
        assert_eq!(
            field_dates("it has a balance on 2026-11-17; edit that one, closed 2026-01-05"),
            "it has a balance on 17 Nov 2026; edit that one, closed 5 Jan 2026"
        );
        assert_eq!(
            field_dates("no dates, just ünïcode"),
            "no dates, just ünïcode"
        );
        // Part of a path, a file name or a timestamp: left as it is.
        for kept in [
            "couldn't open /tmp/orre-2026-11-17/orre.db",
            "backup-2026-11-17.db",
            "at 2026-11-17T09:00",
            "2026-11-17_orre.db",
        ] {
            assert_eq!(field_dates(kept), kept);
        }
        assert_eq!(field_dates("2026-11-17."), "17 Nov 2026.");
    }

    #[test]
    fn the_count_caption_holds_without_other() {
        // Core files the difference in Other only while it exists
        // (`account::count`), and the user may have renamed it: the
        // caption names no category.
        assert!(COUNT_CAPTION.contains("Cash count difference"));
        assert!(!COUNT_CAPTION.contains("Other"), "{COUNT_CAPTION}");
    }
}
