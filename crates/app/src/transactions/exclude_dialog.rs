//! The Exclude dialog (ROADMAP M3.10; `DESIGN.md` → "Transactions
//! additions" → Excluded, states board `51L-0`), `--container-dialog` wide,
//! opened by the Source section's "Exclude…" and the exclusion card's
//! "Edit reason…".
//!
//! "Exclude “Odeon Luxe Leicester Square”?", a body saying what changes and
//! that it can be undone, Reason (select: Duplicate, Bank error, Disputed,
//! Other) and Details (optional text, required for Other). With Duplicate
//! chosen the body also says the row leaves the account's balance. Cancel
//! (secondary) and Exclude (neutral primary, not destructive: excluding is
//! reversible and drops nothing). It goes through core's `exclusion`
//! ([`TransactionsView::exclude`]); a refusal (a lock, a missing note) is
//! said in `danger` under the form.
//!
//! Composed for M3.10, not drawn: the Reason select starts with no choice
//! ("Choose a reason"), so a reason, and with it whether the balance
//! changes, is always picked by the user; "Edit reason…" opens the same
//! dialog filled in, titled "Change why “…” is excluded" with "Save".

use coffer_core::exclusion::ExclusionReason;
use coffer_core::id::TransactionId;
use gpui_kit::component::input::{InputEvent, InputState};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::{
    AppContext as _, Context, Entity, EventEmitter, FocusHandle, IntoElement, ParentElement as _,
    Render, SharedString, Styled as _, Subscription, Window, div,
};

use crate::accounts::DialogClosed;
use crate::theme::tokens::{color, spacing, typography};
use crate::ui::button::{Button, ButtonVariant};
use crate::ui::dialog::Dialog;
use crate::ui::field::{FieldRow, SelectField, caption, error_caption, text_input};
use crate::ui::menu::{MenuGroup, MenuOption, Select, SelectState};
use crate::ui::text::TextRoleExt as _;

use super::TransactionsView;
use super::linking;
use super::source::reason_label;

/// The body: what excluding changes. A duplicate also leaves the balance.
pub fn body(reason: Option<ExclusionReason>) -> &'static str {
    if reason.is_some_and(ExclusionReason::leaves_balance) {
        "It leaves spending, income, budgets, reports and the account’s balance: a duplicate \
         never happened at the bank. It stays in the list under the Excluded filter, and you \
         can include it again at any time."
    } else {
        "It leaves spending, income, budgets and reports, and stays in the account’s balance. \
         It stays in the list under the Excluded filter, and you can include it again at any \
         time."
    }
}

/// What the form asks of core, or why it can't yet.
pub fn read(
    reason: Option<ExclusionReason>,
    details: &str,
) -> Result<(ExclusionReason, Option<String>), SharedString> {
    let reason = reason.ok_or_else(|| SharedString::from("Choose a reason."))?;
    let details = details.trim();
    if reason.needs_note() && details.is_empty() {
        return Err("Say why in Details: Other needs a few words.".into());
    }
    Ok((reason, (!details.is_empty()).then(|| details.to_owned())))
}

fn reason_groups() -> Vec<MenuGroup> {
    vec![MenuGroup::new(
        None,
        ExclusionReason::CHOOSABLE
            .into_iter()
            .map(|reason| MenuOption::new(reason.as_str(), reason_label(reason)))
            .collect(),
    )]
}

/// The Exclude dialog for one transaction.
pub struct ExcludeDialog {
    transactions: Entity<TransactionsView>,
    transaction: TransactionId,
    merchant: String,
    /// Whether the row is excluded already: "Edit reason…".
    editing: bool,
    reason: Option<ExclusionReason>,
    reason_select: SelectState,
    details: Entity<InputState>,
    focus: FocusHandle,
    error: Option<SharedString>,
    _subscriptions: Vec<Subscription>,
}

impl EventEmitter<DialogClosed> for ExcludeDialog {}

impl ExcludeDialog {
    pub fn new(
        transactions: Entity<TransactionsView>,
        transaction: TransactionId,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let row = transactions.read(cx).register().row(transaction).cloned();
        let (merchant, reason, note) = row.map_or_else(
            || (String::new(), None, None),
            |row| {
                (
                    linking::name(&row.merchant),
                    row.excluded,
                    row.excluded_note,
                )
            },
        );
        let details = cx.new(|cx| {
            let mut input = InputState::new(window, cx).placeholder("Optional");
            if let Some(note) = &note {
                input.set_value(SharedString::from(note.clone()), window, cx);
            }
            input
        });
        let subscriptions =
            vec![
                cx.subscribe_in(&details, window, |this, _, event: &InputEvent, _, cx| {
                    if matches!(event, InputEvent::PressEnter { .. }) {
                        this.submit(cx);
                    }
                }),
            ];
        let focus = cx.focus_handle();
        focus.focus(window, cx);
        Self {
            transactions,
            transaction,
            merchant,
            editing: reason.is_some(),
            reason,
            reason_select: SelectState::new(cx),
            details,
            focus,
            error: None,
            _subscriptions: subscriptions,
        }
    }

    pub fn reason(&self) -> Option<ExclusionReason> {
        self.reason
    }

    pub fn error(&self) -> Option<&SharedString> {
        self.error.as_ref()
    }

    /// Picks `reason`, as the Reason select does.
    pub fn choose_reason(
        &mut self,
        reason: ExclusionReason,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.reason = Some(reason);
        self.error = None;
        // Other needs details (`DESIGN.md`: "required for Other").
        let placeholder = if reason.needs_note() {
            "Required"
        } else {
            "Optional"
        };
        self.details.update(cx, |input, cx| {
            input.set_placeholder(placeholder, window, cx)
        });
        cx.notify();
    }

    /// Types `text` into Details (snapshots, tests).
    pub fn set_details(&mut self, text: &str, window: &mut Window, cx: &mut Context<Self>) {
        let text = SharedString::from(text.to_owned());
        self.details
            .update(cx, |input, cx| input.set_value(text, window, cx));
    }

    /// Excludes the row, or says why it can't.
    pub fn submit(&mut self, cx: &mut Context<Self>) {
        let typed = self.details.read(cx).value().to_string();
        let (reason, note) = match read(self.reason, &typed) {
            Ok(read) => read,
            Err(error) => {
                self.error = Some(error);
                cx.notify();
                return;
            }
        };
        let transaction = self.transaction;
        let result = self.transactions.update(cx, |view, cx| {
            view.exclude(transaction, reason, note.as_deref(), cx)
        });
        match result {
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

    fn reason_select(&self, cx: &mut Context<Self>) -> Select {
        let field = match self.reason {
            Some(reason) => SelectField::new("exclude-reason-field", reason_label(reason)),
            None => SelectField::new("exclude-reason-field", "Choose a reason").placeholder(true),
        };
        let groups = reason_groups();
        let moved = groups.clone();
        let (toggled, chosen, step) = (
            cx.entity().downgrade(),
            cx.entity().downgrade(),
            cx.entity().downgrade(),
        );
        Select::new(
            "exclude-reason",
            "Reason",
            field,
            groups,
            self.reason.map(|reason| reason.as_str().into()),
            &self.reason_select,
        )
        .on_toggle(move |open, window, cx| {
            let _ = toggled.update(cx, |this, cx| {
                let chosen = this
                    .reason
                    .map(|reason| SharedString::from(reason.as_str()));
                this.reason_select
                    .set_open(open, chosen.as_ref(), window, cx);
                cx.notify();
            });
        })
        .on_choose(move |key, window, cx| {
            let _ = chosen.update(cx, |this, cx| {
                if let Some(reason) = ExclusionReason::from_key(key) {
                    this.choose_reason(reason, window, cx);
                }
                this.reason_select.set_open(false, None, window, cx);
            });
        })
        .on_move(move |delta, _, cx| {
            let _ = step.update(cx, |this, cx| {
                this.reason_select.move_highlight(&moved, delta);
                cx.notify();
            });
        })
    }

    /// "Exclude “Odeon Luxe Leicester Square”?", truncating the name.
    fn title(&self) -> impl IntoElement {
        let (before, after) = if self.editing {
            ("Change why “", "” is excluded")
        } else {
            ("Exclude “", "”?")
        };
        div()
            .flex()
            .min_w_0()
            .whitespace_nowrap()
            .overflow_hidden()
            .text_role(typography::TITLE)
            .text_color(color::TEXT)
            .child(div().flex_none().child(before))
            .child(div().min_w_0().text_ellipsis().child(self.merchant.clone()))
            .child(div().flex_none().child(after))
    }
}

impl Render for ExcludeDialog {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let form = div()
            .flex()
            .flex_col()
            .gap(spacing::S2)
            .child(FieldRow::new("Reason", self.reason_select(cx)))
            .child(FieldRow::new(
                "Details",
                text_input(&self.details).id("exclude-details").w_full(),
            ));
        let on_cancel = cx.entity().downgrade();
        Dialog::with_title(self.title(), &self.focus)
            .body(caption(body(self.reason)))
            .body(form)
            .when_some(self.error.clone(), |this, error| {
                this.body(error_caption(error))
            })
            .action(
                Button::new("exclude-cancel", "Cancel")
                    .with_variant(ButtonVariant::Secondary)
                    .with_surface(color::FLOAT)
                    .on_click(cx.listener(|this, _, _, cx| this.cancel(cx))),
            )
            .action(
                Button::new(
                    "exclude-confirm",
                    if self.editing { "Save" } else { "Exclude" },
                )
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

    #[test]
    fn other_needs_details_and_a_reason_is_chosen() {
        assert!(read(None, "anything").is_err());
        assert!(read(Some(ExclusionReason::Other), "  ").is_err());
        assert_eq!(
            read(Some(ExclusionReason::Other), " Test charge "),
            Ok((ExclusionReason::Other, Some("Test charge".to_owned())))
        );
        assert_eq!(
            read(Some(ExclusionReason::BankError), ""),
            Ok((ExclusionReason::BankError, None))
        );
    }

    #[test]
    fn the_body_says_a_duplicate_leaves_the_balance() {
        assert!(body(Some(ExclusionReason::Duplicate)).contains("account’s balance: a duplicate"));
        assert!(body(Some(ExclusionReason::BankError)).contains("stays in the account’s balance"));
        assert!(body(None).contains("stays in the account’s balance"));
    }
}
