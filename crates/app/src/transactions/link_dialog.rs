//! The link dialog (ROADMAP M3.11; `docs/DESIGN.md` → "Refunds,
//! reimbursements and shared costs" → Link dialog; Paper `EUY-0`),
//! `--container-dialog` wide.
//!
//! "Link ASOS.com +£34.00": the Kind control (Refund / Reimbursement /
//! Settlement), a search field (prefilled with the merchant for a refund),
//! the candidates in 28px rows (mono date, merchant · account, mono
//! amount), same merchant first, then newest, from core's
//! `reporting::linking`; a row that can't take more is at
//! `opacity-disabled` with the reason. Amount defaults to what both sides
//! can take, so one refund can be split across purchases. Cancel / "Link
//! refund" (neutral primary: linking is undoable).
//!
//! Composed for M3.11 (user, 2026-11-14) where D6.5 doesn't draw it:
//!
//! - from a purchase, the same dialog lists money in (Refund, and
//!   Reimbursement when it's claimed back);
//! - a settlement from money in lists people as 28px rows: the lock, the
//!   name, "owes" and a mono amount;
//! - "Link a payment…" on Owed to you opens it for that person, Kind fixed
//!   to Settlement, listing money in.
//!
//! The rows, candidates or people, show at most eight 28px rows and the
//! 4px gaps between them, and scroll inside the dialog past that (the
//! Transactions screen's `MENU_ROWS`: menus "scroll after 8 rows"), so a
//! long list doesn't grow the dialog.
//!
//! Keyboard (`DESIGN.md` → "Keyboard paths"): from the search field, ↑↓
//! move the chosen row (its `molten-soft` look), skipping the rows at
//! `opacity-disabled` and stopping at the ends, and scroll it into view;
//! Enter is the primary button.
//!
//! The link is made by the Transactions screen ([`TransactionsView::link`]),
//! through core's `edit::link`, which checks both sides and the caps; a
//! refusal is said under the form in caption type, `danger`.

use gpui_kit::base::TestSupportExt as _;
use gpui_kit::component::Disableable as _;
use gpui_kit::component::input::{self, InputEvent, InputState};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::{
    AnyElement, App, AppContext as _, Context, Entity, EventEmitter, FocusHandle,
    InteractiveElement as _, IntoElement, ParentElement as _, Pixels, Render, ScrollHandle,
    SharedString, StatefulInteractiveElement as _, Styled as _, Subscription, Window, div,
};
use orre_core::id::TransactionId;
use orre_core::link::{LinkKind, LinkTarget, NewLink, ShareLabel};
use orre_core::money::{Currency, Money};
use orre_core::reporting::linking::{self as core_linking, Blocked, Candidate};
use orre_core::reporting::register::{Register, RegisterRow};

use crate::accounts::DialogClosed;
use crate::format::{self, Sign};
use crate::inbox::text::unsigned;
use crate::theme::tokens::{color, font, opacity, radius, size, spacing, typography};
use crate::ui::button::{Button, ButtonVariant};
use crate::ui::dialog::Dialog;
use crate::ui::field::{self as fields, FieldRow, error_caption, search_input, text_input};
use crate::ui::icon::Glyph;
use crate::ui::list::{self, Move, Reveal as _};
use crate::ui::segmented::Segmented;
use crate::ui::text::TextRoleExt as _;

use super::TransactionsView;
use super::arrived::DIALOG_CAPTION;
use super::linking;
use super::links::LinkFrom;

/// What the user picked to link to.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Choice {
    Transaction(TransactionId),
    Person(ShareLabel),
}

/// The link dialog.
pub struct LinkDialog {
    transactions: Entity<TransactionsView>,
    from: LinkFrom,
    kinds: Vec<LinkKind>,
    kind: LinkKind,
    search: Entity<InputState>,
    amount: Entity<InputState>,
    chosen: Option<Choice>,
    /// The rows' scrolling, so ↑↓ can bring the chosen row into view.
    scroll: ScrollHandle,
    focus: FocusHandle,
    error: Option<SharedString>,
    _subscriptions: Vec<Subscription>,
}

impl EventEmitter<DialogClosed> for LinkDialog {}

impl LinkDialog {
    pub fn new(
        transactions: Entity<TransactionsView>,
        from: LinkFrom,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let focus = cx.focus_handle();
        let search = cx.new(|cx| InputState::new(window, cx).placeholder("Search"));
        let amount = cx.new(|cx| InputState::new(window, cx));
        let kinds = match &from {
            LinkFrom::Transaction(id) => transactions
                .read(cx)
                .register()
                .row(*id)
                .map(core_linking::kinds_from)
                .unwrap_or_default(),
            LinkFrom::Person(..) => vec![LinkKind::Settlement],
        };
        let subscriptions = vec![
            cx.subscribe_in(&search, window, |this, _, event, window, cx| {
                if matches!(event, InputEvent::Change) {
                    this.choose_first(window, cx);
                    cx.notify();
                }
            }),
            cx.subscribe_in(&amount, window, |this, _, event, _, cx| {
                if matches!(event, InputEvent::PressEnter { .. }) {
                    this.submit(cx);
                } else if matches!(event, InputEvent::Change) {
                    cx.notify();
                }
            }),
            // The row may go in a read from another process: the primary
            // and the caption follow the view.
            cx.observe(&transactions, |_, _, cx| cx.notify()),
        ];
        let mut dialog = Self {
            transactions,
            from,
            kind: kinds.first().copied().unwrap_or(LinkKind::Refund),
            kinds,
            search,
            amount,
            chosen: None,
            scroll: ScrollHandle::new(),
            focus,
            error: None,
            _subscriptions: subscriptions,
        };
        dialog.reset_search(window, cx);
        dialog
            .search
            .update(cx, |search, cx| search.focus(window, cx));
        dialog
    }

    fn register<'a>(&self, cx: &'a App) -> &'a Register {
        self.transactions.read(cx).register()
    }

    /// The row linked from, when it's a transaction.
    fn source_row<'a>(&self, cx: &'a App) -> Option<&'a RegisterRow> {
        match &self.from {
            LinkFrom::Transaction(id) => self.register(cx).row(*id),
            LinkFrom::Person(..) => None,
        }
    }

    /// Whether the row linked from is the money in.
    fn source_is_inflow(&self, cx: &App) -> bool {
        self.source_row(cx).is_some_and(core_linking::can_be_inflow)
    }

    fn currency(&self, cx: &App) -> Currency {
        match &self.from {
            LinkFrom::Person(_, currency) => *currency,
            LinkFrom::Transaction(_) => self
                .source_row(cx)
                .map_or(Currency::GBP, |row| row.amount.currency()),
        }
    }

    /// The candidate transactions for the kind and search.
    pub fn candidates(&self, cx: &App) -> Vec<Candidate> {
        let query = self.search.read(cx).value();
        let register = self.register(cx);
        match &self.from {
            LinkFrom::Person(_, currency) => core_linking::payments(register, *currency, &query),
            LinkFrom::Transaction(_) => self
                .source_row(cx)
                .map(|row| core_linking::candidates(register, row, self.kind, &query))
                .unwrap_or_default(),
        }
    }

    /// The people a settlement from money in can repay.
    fn people(&self, cx: &App) -> Vec<(ShareLabel, Money)> {
        let query = self.search.read(cx).value();
        let register = self.register(cx);
        self.source_row(cx)
            .map(|row| {
                core_linking::people(register, row, &query)
                    .into_iter()
                    .map(|person| (person.label.clone(), person.owed))
                    .collect()
            })
            .unwrap_or_default()
    }

    /// Whether the list is people (a settlement from money in).
    fn lists_people(&self) -> bool {
        self.kind == LinkKind::Settlement && matches!(self.from, LinkFrom::Transaction(_))
    }

    /// The search a kind starts with: a refund from money in looks for its
    /// merchant; the rest start empty.
    fn reset_search(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let text = match (&self.from, self.kind) {
            (LinkFrom::Transaction(_), LinkKind::Refund) if self.source_is_inflow(cx) => self
                .source_row(cx)
                .map(|row| linking::name(&row.merchant))
                .unwrap_or_default(),
            _ => String::new(),
        };
        self.search
            .update(cx, |search, cx| search.set_value(text, window, cx));
        self.choose_first(window, cx);
    }

    /// Chooses the first candidate that can take a link.
    fn choose_first(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let first = if self.lists_people() {
            self.people(cx)
                .into_iter()
                .next()
                .map(|(label, _)| Choice::Person(label))
        } else {
            self.candidates(cx)
                .into_iter()
                .find(|candidate| candidate.blocked.is_none())
                .map(|candidate| Choice::Transaction(candidate.transaction))
        };
        if self.chosen != first {
            self.chosen = first;
            self.fill_amount(window, cx);
        }
    }

    /// The rows listed, in order, each with whether it can be chosen: a
    /// candidate that can't take more is listed at `opacity-disabled`.
    fn rows(&self, cx: &App) -> Vec<(Choice, bool)> {
        if self.lists_people() {
            self.people(cx)
                .into_iter()
                .map(|(label, _)| (Choice::Person(label), true))
                .collect()
        } else {
            self.candidates(cx)
                .into_iter()
                .map(|candidate| {
                    (
                        Choice::Transaction(candidate.transaction),
                        candidate.blocked.is_none(),
                    )
                })
                .collect()
        }
    }

    /// ↑↓ from the search field: chooses the next row that can be chosen
    /// that way ([`list::step`]: stopping at the ends; with nothing chosen,
    /// ↓ starts at the top and ↑ at the bottom), and scrolls it into view.
    fn move_choice(&mut self, to: Move, window: &mut Window, cx: &mut Context<Self>) {
        let rows = self.rows(cx);
        let open: Vec<Choice> = rows
            .iter()
            .filter(|(_, enabled)| *enabled)
            .map(|(choice, _)| choice.clone())
            .collect();
        let Some(next) = list::step(&open, self.chosen.as_ref(), to) else {
            return;
        };
        if let Some(index) = rows.iter().position(|(choice, _)| *choice == next) {
            self.scroll.reveal(index);
        }
        if self.chosen.as_ref() == Some(&next) {
            // At an end: the amount typed stays.
            cx.notify();
        } else {
            self.choose(next, window, cx);
        }
    }

    /// What the side linked from can still take.
    fn source_left(&self, cx: &App) -> Option<Money> {
        match &self.from {
            LinkFrom::Transaction(_) => self.source_row(cx).map(|row| row.share.left),
            // A person: what the chosen payment has left.
            LinkFrom::Person(..) => match &self.chosen {
                Some(Choice::Transaction(id)) => {
                    self.register(cx).row(*id).map(|row| row.share.left)
                }
                _ => None,
            },
        }
    }

    /// What the chosen side can still take.
    fn chosen_left(&self, cx: &App) -> Option<Money> {
        match (&self.chosen, &self.from) {
            (Some(Choice::Person(label)), _) | (_, LinkFrom::Person(label, _)) => {
                let currency = self.currency(cx);
                self.register(cx)
                    .people()
                    .iter()
                    .find(|person| &person.label == label && person.owed.currency() == currency)
                    .map(|person| person.owed)
            }
            (Some(Choice::Transaction(id)), _) => {
                self.register(cx).row(*id).map(|row| row.share.left)
            }
            (None, _) => None,
        }
    }

    /// Puts the default amount in the field: what both sides can take.
    fn fill_amount(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let amount = match (self.source_left(cx), self.chosen_left(cx)) {
            (Some(from), Some(other)) => Some(core_linking::default_amount(from, other)),
            _ => None,
        };
        let text = amount.map_or_else(String::new, |amount| amount.to_decimal_string());
        self.amount
            .update(cx, |input, cx| input.set_value(text, window, cx));
    }

    pub fn choose(&mut self, choice: Choice, window: &mut Window, cx: &mut Context<Self>) {
        self.chosen = Some(choice);
        self.error = None;
        self.fill_amount(window, cx);
        cx.notify();
    }

    pub fn set_kind(&mut self, kind: LinkKind, window: &mut Window, cx: &mut Context<Self>) {
        if self.kind == kind || !self.kinds.contains(&kind) {
            return;
        }
        self.kind = kind;
        self.chosen = None;
        self.error = None;
        self.reset_search(window, cx);
        cx.notify();
    }

    pub fn kind(&self) -> LinkKind {
        self.kind
    }

    pub fn chosen(&self) -> Option<&Choice> {
        self.chosen.as_ref()
    }

    pub fn error(&self) -> Option<&SharedString> {
        self.error.as_ref()
    }

    /// The link the dialog would make now, or why it can't.
    fn new_link(&self, cx: &App) -> Result<(NewLink, Option<String>), SharedString> {
        let currency = self.currency(cx);
        let typed = self.amount.read(cx).value();
        let amount = format::parse_amount(&typed, currency)
            .map_err(|_| SharedString::from("Type the amount to link, like 34.00."))?;
        let amount = Money::new(amount.minor().saturating_abs(), currency);
        let nothing = || SharedString::from("Choose what to link it to.");
        let (inflow, target, person) = match (&self.from, &self.chosen) {
            (LinkFrom::Person(label, _), Some(Choice::Transaction(payment))) => (
                *payment,
                LinkTarget::Person(label.clone()),
                Some(label.as_str().to_owned()),
            ),
            (LinkFrom::Transaction(from), Some(Choice::Person(label))) => (
                *from,
                LinkTarget::Person(label.clone()),
                Some(label.as_str().to_owned()),
            ),
            (LinkFrom::Transaction(from), Some(Choice::Transaction(other))) => {
                if self.source_is_inflow(cx) {
                    (*from, LinkTarget::Purchase(*other), None)
                } else {
                    (*other, LinkTarget::Purchase(*from), None)
                }
            }
            _ => return Err(nothing()),
        };
        Ok((
            NewLink {
                kind: self.kind,
                inflow,
                target,
                amount,
            },
            person,
        ))
    }

    /// Whether the row linked from went in a read from another process: the
    /// primary is disabled and the caption says so; Cancel still closes.
    /// Linking from a person has no row to lose.
    pub fn removed(&self, cx: &App) -> bool {
        match &self.from {
            LinkFrom::Transaction(id) => self.transactions.read(cx).row_removed(*id),
            LinkFrom::Person(..) => false,
        }
    }

    /// Links, or says why it can't.
    pub fn submit(&mut self, cx: &mut Context<Self>) {
        if self.removed(cx) {
            return;
        }
        let (link, person) = match self.new_link(cx) {
            Ok(link) => link,
            Err(error) => {
                self.error = Some(error);
                cx.notify();
                return;
            }
        };
        match self
            .transactions
            .update(cx, |view, cx| view.link(&link, person.as_deref(), cx))
        {
            Ok(()) => cx.emit(DialogClosed),
            Err(error) => {
                self.error = Some(error);
                cx.notify();
            }
        }
    }

    pub fn cancel(&mut self, cx: &mut Context<Self>) {
        cx.emit(DialogClosed);
    }

    /// The dialog's title: "Link ASOS.com +£34.00", or "Link a payment from
    /// Sam".
    fn title(&self, cx: &App) -> AnyElement {
        let title = div()
            .flex()
            .whitespace_nowrap()
            .overflow_hidden()
            .text_role(typography::TITLE)
            .text_color(color::TEXT);
        match (&self.from, self.source_row(cx)) {
            (LinkFrom::Transaction(_), Some(row)) => title
                .child(
                    div()
                        .min_w_0()
                        .text_ellipsis()
                        .child(format!("Link {}\u{a0}", linking::name(&row.merchant))),
                )
                .child(
                    div()
                        .flex_none()
                        .font_family(font::MONO)
                        .child(format::money(row.amount, Sign::Always)),
                )
                .into_any_element(),
            (LinkFrom::Person(label, _), _) => title
                .child(format!(
                    "Link a payment from {}",
                    linking::name(label.as_str())
                ))
                .into_any_element(),
            _ => title.child("Link").into_any_element(),
        }
    }

    /// One candidate row (Paper `EXQ-0`, `EXU-0`, `EXY-0`).
    fn candidate_row(
        &self,
        candidate: &Candidate,
        today: chrono::NaiveDate,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let register = self.register(cx);
        let row = register.row(candidate.transaction)?;
        let chosen = self.chosen == Some(Choice::Transaction(candidate.transaction));
        let secondary = if chosen {
            color::TEXT_SECONDARY
        } else {
            color::TEXT_MUTED
        };
        let detail = match candidate.blocked {
            Some(Blocked::AlreadyRefunded) => "already refunded".to_owned(),
            Some(Blocked::AlreadyRepaid) => "already repaid".to_owned(),
            Some(Blocked::AllLinked) => "all linked".to_owned(),
            None => register
                .account(row.account)
                .map_or_else(String::new, |account| linking::name(&account.name)),
        };
        let id = candidate.transaction;
        let blocked = candidate.blocked.is_some();
        Some(
            div()
                .id(SharedString::from(format!("link-candidate-{id}")))
                .test_support()
                .flex()
                .flex_none()
                .items_center()
                .gap(spacing::S2)
                .h(size::CONTROL)
                .px(spacing::S2)
                .rounded(radius::MD)
                .when(chosen, |this| this.bg(color::MOLTEN_SOFT))
                .when(blocked, |this| this.opacity(opacity::DISABLED))
                .when(!blocked && !chosen, |this| {
                    this.hover(|style| style.bg(color::HOVER))
                })
                .when(!blocked, |this| {
                    this.on_click(cx.listener(move |this, _, window, cx| {
                        this.choose(Choice::Transaction(id), window, cx)
                    }))
                })
                .child(
                    div()
                        .flex_none()
                        .w(spacing::S6 * 2.)
                        .text_role(typography::MONO_VALUE)
                        .text_color(secondary)
                        .child(format::list_date(row.date, today)),
                )
                .child(
                    div()
                        .flex()
                        .flex_1()
                        .min_w_0()
                        .whitespace_nowrap()
                        .overflow_hidden()
                        .text_role(typography::BODY)
                        .text_color(color::TEXT)
                        .child(
                            div()
                                .min_w_0()
                                .text_ellipsis()
                                .child(linking::name(&row.merchant)),
                        )
                        .when(!detail.is_empty(), |this| {
                            this.child(
                                div()
                                    .flex_none()
                                    .text_color(secondary)
                                    .child(format!("\u{a0}· {detail}")),
                            )
                        }),
                )
                .child(
                    div()
                        .flex_none()
                        .text_role(typography::MONO_AMOUNT)
                        .text_color(color::TEXT)
                        .child(format::money(row.amount, Sign::Always)),
                )
                .into_any_element(),
        )
    }

    /// One person row (composed for M3.11): the lock, the name, "owes" and
    /// what they owe.
    fn person_row(&self, label: &ShareLabel, owed: Money, cx: &mut Context<Self>) -> AnyElement {
        let chosen = self.chosen == Some(Choice::Person(label.clone()));
        let choice = label.clone();
        div()
            .id(SharedString::from(format!(
                "link-person-{}",
                label.as_str()
            )))
            .test_support()
            .flex()
            .flex_none()
            .items_center()
            .gap(spacing::S2)
            .h(size::CONTROL)
            .px(spacing::S2)
            .rounded(radius::MD)
            .when(chosen, |this| this.bg(color::MOLTEN_SOFT))
            .when(!chosen, |this| this.hover(|style| style.bg(color::HOVER)))
            .on_click(cx.listener(move |this, _, window, cx| {
                this.choose(Choice::Person(choice.clone()), window, cx)
            }))
            .child(Glyph::Lock.render().text_color(color::TEXT_MUTED))
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .whitespace_nowrap()
                    .text_ellipsis()
                    .text_role(typography::BODY)
                    .text_color(color::TEXT)
                    .child(linking::name(label.as_str())),
            )
            .child(
                div()
                    .flex_none()
                    .text_role(typography::CAPTION)
                    .text_color(if chosen {
                        color::TEXT_SECONDARY
                    } else {
                        color::TEXT_MUTED
                    })
                    .child("owes"),
            )
            .child(
                div()
                    .flex_none()
                    .text_role(typography::MONO_AMOUNT)
                    .text_color(color::TEXT)
                    .child(unsigned(owed)),
            )
            .into_any_element()
    }

    /// What the list says when nothing matches (composed for M3.11).
    fn nothing(&self) -> &'static str {
        match (&self.from, self.kind) {
            (LinkFrom::Person(..), _) => "No money in matches.",
            (_, LinkKind::Settlement) => "No one owes you in this currency.",
            (_, LinkKind::Reimbursement) => {
                "No claim to repay. Mark a purchase to claim back in its Share section first."
            }
            (_, LinkKind::Refund) => "Nothing matches.",
        }
    }

    /// The purchase's category, for the caption.
    fn purchase_category(&self, cx: &App) -> Option<String> {
        let register = self.register(cx);
        let purchase = match (&self.chosen, self.source_is_inflow(cx)) {
            (Some(Choice::Transaction(id)), true) => register.row(*id),
            (_, false) => self.source_row(cx),
            _ => None,
        }?;
        (!register.row_categories(purchase).is_empty())
            .then(|| super::present::category_label(register, purchase))
    }
}

/// The most the rows grow to: eight rows and the gaps between them, so
/// eight show whole and a ninth scrolls.
pub(super) fn rows_height() -> Pixels {
    size::CONTROL * super::MENU_ROWS + spacing::S1 * (super::MENU_ROWS - 1.)
}

impl Render for LinkDialog {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let today = self.transactions.read(cx).today();
        let currency = self.currency(cx);
        let kinds = (self.kinds.len() > 1).then(|| {
            let view = cx.entity().downgrade();
            let mut control = Segmented::new("link-kind", self.kind.as_str()).fill();
            for kind in &self.kinds {
                control = control.segment(kind.as_str(), linking::kind_label(*kind));
            }
            control.on_choose(move |key, window, cx| {
                if let Some(kind) = LinkKind::from_key(key) {
                    view.update(cx, |this, cx| this.set_kind(kind, window, cx))
                        .ok();
                }
            })
        });
        let rows: Vec<AnyElement> = if self.lists_people() {
            self.people(cx)
                .iter()
                .map(|(label, owed)| self.person_row(label, *owed, cx))
                .collect()
        } else {
            self.candidates(cx)
                .iter()
                .filter_map(|candidate| self.candidate_row(candidate, today, cx))
                .collect()
        };
        let empty = rows.is_empty().then(|| {
            div()
                .flex()
                .items_center()
                .h(size::CONTROL)
                .px(spacing::S2)
                .text_role(typography::CAPTION)
                .text_color(color::TEXT_MUTED)
                .child(self.nothing())
        });
        // The rows scroll as one after eight, so ↑↓ can bring the chosen
        // one into view.
        let rows = (!rows.is_empty()).then(|| {
            div()
                .id("link-rows")
                .test_support()
                .flex()
                .flex_col()
                .gap(spacing::S1)
                .max_h(rows_height())
                .overflow_y_scroll()
                .track_scroll(&self.scroll)
                .children(rows)
        });
        let list = div()
            .flex()
            .flex_col()
            .gap(spacing::S1)
            // The search field's own keys, caught on the way down to it, so
            // typing searches while ↑↓ choose a row and Enter links.
            .capture_action(cx.listener(|this, _: &input::MoveUp, window, cx| {
                cx.stop_propagation();
                this.move_choice(Move::UP, window, cx);
            }))
            .capture_action(cx.listener(|this, _: &input::MoveDown, window, cx| {
                cx.stop_propagation();
                this.move_choice(Move::DOWN, window, cx);
            }))
            .capture_action(cx.listener(|this, _: &input::Enter, _, cx| {
                cx.stop_propagation();
                this.submit(cx);
            }))
            .child(search_input(&self.search).w_full())
            .children(rows)
            .children(empty);
        let source_left = self.source_left(cx);
        let typed = format::parse_amount(&self.amount.read(cx).value(), currency).ok();
        let hint = match (typed, source_left) {
            (Some(typed), Some(left)) => linking::amount_hint(self.kind, typed, left),
            _ => None,
        };
        let amount = text_input(&self.amount)
            .w_full()
            .gap(spacing::S1)
            .font_family(font::MONO)
            .prefix(
                div()
                    .text_role(typography::MONO_AMOUNT)
                    .text_color(color::TEXT_MUTED)
                    .child(format::currency_prefix(currency)),
            )
            .when_some(hint, |this, hint| {
                this.suffix(
                    div()
                        .text_role(typography::CAPTION)
                        .text_color(color::TEXT_MUTED)
                        .child(hint),
                )
            });
        let category = self.purchase_category(cx);
        let form = div()
            .flex()
            .flex_col()
            .gap(spacing::S2)
            .child(FieldRow::new("Amount", amount))
            .child(fields::caption(linking::dialog_caption(
                self.kind,
                category.as_deref(),
            )))
            .when_some(self.error.clone(), |this, error| {
                this.child(error_caption(error))
            });
        let on_cancel = cx.entity().downgrade();
        let removed = self.removed(cx);
        Dialog::with_title(self.title(cx), &self.focus)
            .when_some(kinds, |this, kinds| this.body(kinds))
            .body(list)
            .body(form)
            .when(removed, |this| this.body(fields::caption(DIALOG_CAPTION)))
            .action(
                Button::new("link-cancel", "Cancel")
                    .with_variant(ButtonVariant::Secondary)
                    .with_surface(color::FLOAT)
                    .on_click(cx.listener(|this, _, _, cx| this.cancel(cx))),
            )
            .action(
                Button::new("link-confirm", linking::link_button(self.kind))
                    .with_variant(ButtonVariant::Primary)
                    .with_surface(color::FLOAT)
                    .disabled(removed)
                    .on_click(cx.listener(|this, _, _, cx| this.submit(cx))),
            )
            .on_cancel(move |_, cx| {
                let _ = on_cancel.update(cx, |this, cx| this.cancel(cx));
            })
    }
}
