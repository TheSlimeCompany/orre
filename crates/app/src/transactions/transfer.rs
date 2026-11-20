//! The inspector's Transfer section (M3.8): a row in a suggested or
//! confirmed transfer pair, and confirming, rejecting or unpairing it.
//!
//! Composed from existing specs, not drawn (user, 2026-11-15;
//! `ARCHITECTURE.md` → "Decision record: transfer-pair confirmation
//! (built)"): a suggested pair is a card like Orre's refund suggestion
//! (D6.5, `APQ-0`) with the other side as a `field` strip and the Inbox
//! transfer pair's wording (D6.6, `EWZ-0`); a confirmed pair is one row like
//! a link row, its ✕ unpairing. Every decision shows the undo bar.
//!
//! What the section shows and allows is core's `reporting::pairing`, over
//! the register; the write ([`inbox::decide_transfer`]) checks again and
//! also settles an agent's pending suggestion for the same pair.

use gpui_kit::component::Disableable as _;
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::{AnyElement, Context, IntoElement, ParentElement as _, Styled as _, Window, div};
use orre_core::id::TransferGroupId;
use orre_core::inbox::{self, RefundDecided, RefundLinked, TransferDecided};
use orre_core::reporting::pairing::{self, Pair};
use orre_core::reporting::register::RegisterRow;
use orre_core::transfer::{self, Action, Blocked, Decision, TransferStatus};

use crate::format;
use crate::inbox::text::unsigned;
use crate::theme::tokens::{color, radius, size, spacing, typography};
use crate::ui::button::{Button, ButtonVariant, IconButton, IconButtonSize};
use crate::ui::field::{self as fields, error_caption};
use crate::ui::icon::Glyph;
use crate::ui::inspector::{section, section_title_row};
use crate::ui::text::TextRoleExt as _;

use super::editor::EditPlace;
use super::write::{Offer, Policy, Said};
use super::{TransactionsEvent, TransactionsView, UndoAction};
use super::{linking, links};

/// What confirming does, from the Inbox transfer pair's preview (D6.6).
pub const CONFIRM_EFFECT: &str =
    "Confirming leaves both out of income and spending. Balances don't change.";
/// Under a confirmed pair.
pub const CONFIRMED_CAPTION: &str = "Not counted as income or spending.";

/// Whether `row` has the Transfer section: it's in a suggested or confirmed
/// pair.
pub fn has_transfer(row: &RegisterRow) -> bool {
    row.pair.is_some()
}

/// Why an action isn't allowed, in the inspector's words.
pub fn blocked_text(blocked: Blocked) -> &'static str {
    match blocked {
        Blocked::Excluded(_) => "One side is excluded. Include it again to confirm the transfer.",
        Blocked::Split(_) => "One side is split. Remove the split to confirm the transfer.",
        Blocked::Linked(_) => "One side is linked. Unlink it to confirm the transfer.",
        Blocked::PotPair => "The import paired this pot transfer. Undo the import to remove it.",
        Blocked::WrongStatus { .. } | Blocked::NotTwoSided => {
            "This pair has changed. Select the row again."
        }
    }
}

/// Why Orre suggested the pair: "Same amount out and in, 2 days apart".
pub fn why(row: &RegisterRow, partner: &RegisterRow) -> String {
    let days = (row.date - partner.date).num_days().unsigned_abs();
    match days {
        0 => "Same amount out and in, the same day".to_owned(),
        1 => "Same amount out and in, 1 day apart".to_owned(),
        n => format!("Same amount out and in, {n} days apart"),
    }
}

/// Why Orre suggested the pair, on the card that also offers a refund:
/// "Same amount out of Current account, 2 days earlier." Money in names the
/// account the money left; money out, the account it went into. Days are
/// the other row's, relative to `row`.
pub fn refund_why(row: &RegisterRow, partner: &RegisterRow, account: &str) -> String {
    let direction = if row.amount.is_positive() {
        "out of"
    } else {
        "into"
    };
    let days = (row.date - partner.date).num_days();
    let when = match days {
        0 => "the same day".to_owned(),
        1 => "1 day earlier".to_owned(),
        -1 => "1 day later".to_owned(),
        n if n > 0 => format!("{n} days earlier"),
        n => format!("{} days later", -n),
    };
    format!("Same amount {direction} {account}, {when}.")
}

/// The undo bar's message after linking a pair as a refund of `purchase`.
pub fn refund_undo_message(purchase: &str) -> String {
    format!("Linked as refund: {purchase}")
}

/// The undo bar's message after `action`.
pub fn undo_message(action: Action, from: &str, to: &str) -> String {
    match action {
        Action::Confirm => format!("Confirmed transfer: {from} → {to}"),
        Action::Reject => "Marked as not a transfer".to_owned(),
        Action::Unpair => "Unpaired transfer".to_owned(),
    }
}

/// A transfer decision or its undo: said under the Transfer section as a
/// sentence; the checks read again.
fn transfer_policy<'a, T>() -> Policy<'a, T> {
    Policy::new(Said::Inspector(EditPlace::Transfer))
        .sentence()
        .emit(TransactionsEvent::Changed)
}

impl TransactionsView {
    /// Confirms, rejects or unpairs the pair `group` that `row` is in; the
    /// undo bar offers to reverse it.
    pub(super) fn decide_transfer(
        &mut self,
        row: &RegisterRow,
        group: TransferGroupId,
        action: Action,
        cx: &mut Context<Self>,
    ) {
        let (from, to) = self.pair_accounts(row);
        let message = undo_message(action, &from, &to);
        let policy = transfer_policy()
            .log("couldn't decide a transfer pair")
            .undo(move |decided: &TransferDecided| {
                Offer::Show(UndoAction::Transfer(decided.decision), message.into())
            });
        self.write(policy, cx, |db| inbox::decide_transfer(db, group, action))
            .ok();
    }

    /// The undo bar's Undo after a transfer decision. If it fails, the bar
    /// offers it again with the reason; the table and the checks read again
    /// either way.
    pub(super) fn undo_transfer(&mut self, decision: Decision, cx: &mut Context<Self>) {
        let policy = transfer_policy()
            .log("couldn't undo a transfer decision")
            .retry(UndoAction::Transfer(decision))
            .always();
        self.write(policy, cx, |db| transfer::undo(db, &decision))
            .ok();
    }

    /// Rejects the suggested pair `group` that `row` is in and links it as a
    /// refund, in one write; the undo bar offers to reverse both.
    pub(super) fn link_pair_as_refund(
        &mut self,
        row: &RegisterRow,
        partner: &RegisterRow,
        group: TransferGroupId,
        cx: &mut Context<Self>,
    ) {
        let purchase = if row.amount.is_negative() {
            row
        } else {
            partner
        };
        let message = refund_undo_message(&linking::name(&purchase.merchant));
        let policy = transfer_policy()
            .log("couldn't link a pair as a refund")
            .undo(move |decided: &RefundDecided| {
                Offer::Show(UndoAction::RefundLinked(decided.linked), message.into())
            });
        self.write(policy, cx, |db| inbox::link_as_refund(db, group))
            .ok();
    }

    /// The undo bar's Undo after linking a pair as a refund; failures are
    /// offered again as [`Self::undo_transfer`]'s are.
    pub(super) fn undo_refund_linked(&mut self, linked: RefundLinked, cx: &mut Context<Self>) {
        let policy = transfer_policy()
            .log("couldn't undo a refund link")
            .retry(UndoAction::RefundLinked(linked))
            .always();
        self.write(policy, cx, |db| inbox::undo_refund_linked(db, &linked))
            .ok();
    }

    /// The pair's accounts, money out first: "Current account", "Savings".
    fn pair_accounts(&self, row: &RegisterRow) -> (String, String) {
        let name = |row: &RegisterRow| {
            self.register
                .account(row.account)
                .map_or_else(String::new, |account| account.name.clone())
        };
        let partner = pairing::pair(&self.register, row).and_then(|pair| pair.partner);
        let (mine, theirs) = (name(row), partner.map(name).unwrap_or_default());
        if Pair::outgoing(row) {
            (mine, theirs)
        } else {
            (theirs, mine)
        }
    }
}

/// Whether `row` has the Transfer section: it's in a pair.
pub(super) fn has_section(view: &TransactionsView, row: &RegisterRow) -> bool {
    pairing::pair(&view.register, row).is_some()
}

/// The Transfer section, or `None` outside a pair.
pub(super) fn transfer_section(
    view: &TransactionsView,
    row: &RegisterRow,
    window: &Window,
    cx: &mut Context<TransactionsView>,
) -> Option<AnyElement> {
    let pair = pairing::pair(&view.register, row)?;
    let body = match (pair.status, pair.partner) {
        (_, None) => vec![fields::caption(blocked_text(Blocked::NotTwoSided)).into_any_element()],
        (TransferStatus::Confirmed, Some(partner)) => confirmed(view, row, &pair, partner, cx),
        (_, Some(partner)) => vec![suggestion_card(view, row, &pair, partner, window, cx)],
    };
    Some(
        section(true)
            .gap(spacing::S2)
            .child(section_title_row(
                super::inspector::Section::Transfer.title(),
                None,
            ))
            .children(body)
            .children(
                view.editor
                    .error_at(EditPlace::Transfer)
                    .map(|error| error_caption(error).into_any_element()),
            )
            .into_any_element(),
    )
}

/// A confirmed pair: one row like a link row (D6.5), its ✕ unpairing.
fn confirmed(
    view: &TransactionsView,
    row: &RegisterRow,
    pair: &Pair<'_>,
    partner: &RegisterRow,
    cx: &mut Context<TransactionsView>,
) -> Vec<AnyElement> {
    let what = direction(view, row, partner);
    let group = pair.group;
    let unpair = pair.unpair;
    let this = row.clone();
    let line = div()
        .flex()
        .flex_none()
        .items_center()
        .gap(spacing::S2)
        .h(size::CONTROL)
        .child(Glyph::Transfer.render().text_color(color::TEXT_MUTED))
        .child(
            div()
                .flex()
                .flex_1()
                .min_w_0()
                .items_baseline()
                .whitespace_nowrap()
                .overflow_hidden()
                .text_role(typography::BODY)
                .text_color(color::TEXT)
                // The space is in UI type, as the link rows set it.
                .child(
                    div()
                        .min_w_0()
                        .text_ellipsis()
                        .child(format!("{what}\u{a0}")),
                )
                .child(
                    div()
                        .flex_none()
                        .text_role(typography::MONO_VALUE)
                        .text_color(color::TEXT_MUTED)
                        .child(format::list_date(partner.date, view.today)),
                ),
        )
        .child(
            div()
                .flex_none()
                .text_role(typography::MONO_AMOUNT)
                .text_color(color::TEXT)
                .child(unsigned(partner.amount)),
        )
        .when(unpair.is_ok(), |line| {
            line.child(
                IconButton::new("unpair-transfer", Glyph::Close, "Unpair")
                    .with_size(IconButtonSize::Small)
                    .on_click(cx.listener(move |view, _, _, cx| {
                        view.decide_transfer(&this, group, Action::Unpair, cx)
                    })),
            )
        })
        .into_any_element();
    let caption = match unpair {
        Err(blocked @ Blocked::PotPair) => blocked_text(blocked),
        _ => CONFIRMED_CAPTION,
    };
    vec![line, fields::caption(caption).into_any_element()]
}

/// "To Savings" or "From Current account": the other side's account, from
/// `row`'s point of view.
fn direction(view: &TransactionsView, row: &RegisterRow, partner: &RegisterRow) -> String {
    let account = view
        .register
        .account(partner.account)
        .map_or_else(String::new, |account| account.name.clone());
    if Pair::outgoing(row) {
        format!("To {account}")
    } else {
        format!("From {account}")
    }
}

/// The other side, as the refund card's purchase strip (D6.5, `APV-0`):
/// mono date, "From Current account", mono amount without a sign (the
/// words give the direction, as on a confirmed pair's row).
fn partner_strip(view: &TransactionsView, row: &RegisterRow, partner: &RegisterRow) -> AnyElement {
    div()
        .flex()
        .flex_none()
        .items_center()
        .gap(spacing::S2)
        .h(size::CONTROL)
        .px(spacing::S2)
        .rounded(radius::MD)
        .bg(color::FIELD)
        .child(
            div()
                .flex_none()
                .text_role(typography::MONO_VALUE)
                .text_color(color::TEXT_MUTED)
                .child(format::list_date(partner.date, view.today)),
        )
        .child(
            div()
                .flex_1()
                .min_w_0()
                .whitespace_nowrap()
                .text_ellipsis()
                .text_role(typography::BODY)
                .text_color(color::TEXT)
                .child(direction(view, row, partner)),
        )
        .child(
            div()
                .flex_none()
                .text_role(typography::MONO_AMOUNT)
                .text_color(color::TEXT)
                .child(unsigned(partner.amount)),
        )
        .into_any_element()
}

/// A suggested pair: Orre's suggestion card (D6.5, `APQ-0`), neutral. It
/// asks "Is this a transfer?" with "Confirm transfer" and "Not a transfer";
/// when the pair could be a refund too (P11), "Transfer or refund?" with
/// "Transfer", "Refund" and "Neither".
fn suggestion_card(
    view: &TransactionsView,
    row: &RegisterRow,
    pair: &Pair<'_>,
    partner: &RegisterRow,
    window: &Window,
    cx: &mut Context<TransactionsView>,
) -> AnyElement {
    let group = pair.group;
    let (confirm_row, reject_row, refund_row) = (row.clone(), row.clone(), row.clone());
    let refund_partner = partner.clone();
    let blocked = pair.confirm.err();
    let refundable = pair.refund.is_ok();
    let purchase = if row.amount.is_negative() {
        row
    } else {
        partner
    };
    let (title, why) = if refundable {
        let account = view
            .register
            .account(partner.account)
            .map_or("", |account| account.name.as_str());
        ("Transfer or refund?", refund_why(row, partner, account))
    } else {
        ("Is this a transfer?", why(row, partner))
    };
    let strip = if refundable {
        links::purchase_strip(view, partner)
    } else {
        partner_strip(view, row, partner)
    };
    let effect = match blocked {
        Some(blocked) => fields::caption(blocked_text(blocked)).into_any_element(),
        None if refundable => links::rich_caption(
            window,
            linking::transfer_or_refund_effect(&view.register, purchase),
        ),
        None => fields::caption(CONFIRM_EFFECT).into_any_element(),
    };
    div()
        .flex()
        .flex_col()
        .gap(spacing::S2_5)
        .p(spacing::S3)
        .rounded(radius::SURFACE)
        .border(size::HAIRLINE)
        .border_color(color::HAIRLINE)
        .bg(color::FLOAT)
        .child(
            div()
                .flex()
                .flex_col()
                .gap(spacing::S1)
                .child(
                    div()
                        .text_role(typography::SECTION_TITLE)
                        .text_color(color::TEXT)
                        .child(title),
                )
                .child(
                    div()
                        .text_role(typography::CAPTION)
                        .text_color(color::TEXT_MUTED)
                        .child(why),
                ),
        )
        .child(strip)
        .child(effect)
        .child(
            div()
                .flex()
                .gap(spacing::S1_5)
                .child(
                    Button::new(
                        "confirm-transfer",
                        if refundable {
                            "Transfer"
                        } else {
                            "Confirm transfer"
                        },
                    )
                    .with_variant(ButtonVariant::Outline)
                    .with_surface(color::FLOAT)
                    .disabled(blocked.is_some())
                    .on_click(cx.listener(move |view, _, _, cx| {
                        view.decide_transfer(&confirm_row, group, Action::Confirm, cx)
                    })),
                )
                .when(refundable, |buttons| {
                    buttons.child(
                        Button::new("link-pair-as-refund", "Refund")
                            .with_variant(ButtonVariant::Outline)
                            .with_surface(color::FLOAT)
                            .on_click(cx.listener(move |view, _, _, cx| {
                                view.link_pair_as_refund(&refund_row, &refund_partner, group, cx)
                            })),
                    )
                })
                .child(
                    Button::new(
                        "reject-transfer",
                        if refundable {
                            "Neither"
                        } else {
                            "Not a transfer"
                        },
                    )
                    .with_variant(ButtonVariant::Ghost)
                    .with_surface(color::FLOAT)
                    .disabled(pair.reject.is_err())
                    .on_click(cx.listener(move |view, _, _, cx| {
                        view.decide_transfer(&reject_row, group, Action::Reject, cx)
                    })),
                ),
        )
        .into_any_element()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_wording() {
        assert_eq!(
            undo_message(Action::Confirm, "Current account", "Savings"),
            "Confirmed transfer: Current account → Savings"
        );
        assert_eq!(
            undo_message(Action::Reject, "a", "b"),
            "Marked as not a transfer"
        );
        assert_eq!(undo_message(Action::Unpair, "a", "b"), "Unpaired transfer");
        assert_eq!(refund_undo_message("Odeon"), "Linked as refund: Odeon");
        assert!(blocked_text(Blocked::PotPair).contains("Undo the import"));
    }
}
