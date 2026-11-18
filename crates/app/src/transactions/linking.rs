//! What the Transactions view says about links and shares (ROADMAP M3.11;
//! `docs/DESIGN.md` → "Refunds, reimbursements and shared costs"; Paper
//! `9WS-0`, `BAT-0`, `EB0-0`): the category cell's suffix, a link row's
//! words, the captions and the suggestion card, the undo bar's messages.
//! Pure, so it's tested without a window.
//!
//! Core decides how a row counts and why a link was suggested
//! (`coffer_core::reporting::linking`); this only words it. Merchant names
//! and people are untrusted or the user's own text: made [`plain`] before
//! they're shown, never acted on.

use chrono::NaiveDate;
use coffer_core::link::LinkKind;
use coffer_core::money::Money;
use coffer_core::reporting::linking::{self, Counts, RefundWhy};
use coffer_core::reporting::register::{LinkOther, LinkSide, Register, RegisterRow, RowLink};

use crate::format::{self, Sign};
use crate::inbox::text::{NAME_SHOWN, Part, plain, unsigned};
use crate::theme::tokens::color;

use super::present::{self, CellText};

/// A name as it's shown (untrusted text made plain).
pub fn name(raw: &str) -> String {
    plain(raw, NAME_SHOWN)
}

/// The row's category cell, with how it counts (Paper `FOD-0`): "Shopping
/// · refund", "Entertainment · shared", "Repays 2 claims".
pub fn cell(register: &Register, row: &RegisterRow) -> CellText {
    let base = present::cell(register, row);
    if row.transfer.is_some() {
        return base;
    }
    match linking::counts_as(register, row) {
        Counts::Plain => base,
        Counts::Refund { .. } => {
            let purchase = register.refund_purchase(row);
            purchase
                .map_or(base, |purchase| present::cell(register, purchase))
                .suffix("refund")
        }
        Counts::Refunded => base.suffix("refunded"),
        Counts::Shared => base.suffix("shared"),
        Counts::ToClaim => base.suffix("to claim"),
        Counts::Claimed => base.suffix("claimed"),
        Counts::RepaysClaims(1) => CellText::repays("Repays 1 claim".to_owned()),
        Counts::RepaysClaims(count) => CellText::repays(format!("Repays {count} claims")),
        Counts::RepaysPerson(person) => CellText::repays(format!("Repays {}", name(&person))),
        Counts::RepaysPeople(count) => CellText::repays(format!("Repays {count} people")),
    }
}

/// One row of the Links section: what the link is, its date, its amount.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LinkLine {
    /// "Refund", "Refund of ASOS.com", "Reimbursed", "Repays Sam".
    pub what: String,
    /// The other side's date, when it is a transaction.
    pub date: Option<NaiveDate>,
    /// "+£34.00" (the money in) or "−£68.00" (the purchase).
    pub amount: String,
}

/// The Links row for `link` on `row` (Paper `FEC-0`, `FH4-0`, `FK4-0`).
/// From the purchase: the money in and the amount linked; from the money
/// in: the purchase and its amount, or the person and the amount linked.
pub fn link_line(register: &Register, link: &RowLink) -> LinkLine {
    let other = match &link.other {
        LinkOther::Transaction(id) => register.row(*id),
        LinkOther::Person(_) => None,
    };
    let date = other.map(|other| other.date);
    let linked = format::money(link.amount, Sign::Always);
    match (link.side, link.kind, &link.other) {
        (LinkSide::Purchase, LinkKind::Refund, _) => LinkLine {
            what: "Refund".into(),
            date,
            amount: linked,
        },
        (LinkSide::Purchase, _, _) => LinkLine {
            what: "Reimbursed".into(),
            date,
            amount: linked,
        },
        (LinkSide::Inflow, _, LinkOther::Person(person)) => LinkLine {
            what: format!("Repays {}", name(person)),
            date: None,
            amount: linked,
        },
        (LinkSide::Inflow, kind, LinkOther::Transaction(_)) => {
            let merchant = other.map(|other| name(&other.merchant)).unwrap_or_default();
            LinkLine {
                what: match kind {
                    LinkKind::Refund => format!("Refund of {merchant}"),
                    _ => format!("Repays {merchant}"),
                },
                date,
                amount: other.map_or(linked, |other| format::money(other.amount, Sign::Always)),
            }
        }
    }
}

/// The confirmed links the Links section lists, oldest first.
pub fn confirmed(row: &RegisterRow) -> impl Iterator<Item = &RowLink> {
    row.links
        .iter()
        .filter(|link| link.status == coffer_core::link::LinkStatus::Confirmed)
}

/// Coffer's suggestions on `row`'s money in, for the suggestion card: all
/// of one kind (a refund, or a payment's set of claims).
pub fn suggestions(row: &RegisterRow) -> Vec<&RowLink> {
    row.links
        .iter()
        .filter(|link| {
            link.status == coffer_core::link::LinkStatus::Suggested && link.side == LinkSide::Inflow
        })
        .collect()
}

/// The caption under the Links rows: what the purchase counts after its
/// refunds, or what can be linked when nothing is.
pub fn links_caption(register: &Register, row: &RegisterRow) -> Option<Vec<Part>> {
    let refunded = confirmed(row)
        .filter(|link| link.side == LinkSide::Purchase && link.kind == LinkKind::Refund)
        .count();
    if refunded > 0 && !row.share.claim {
        let spent = absolute(row.amount);
        let taken = Money::new(spent.minor() - row.share.left.minor(), spent.currency());
        let category = present::category_label(register, row);
        return Some(vec![
            Part::mono(unsigned(taken)),
            Part::text(" of "),
            Part::mono(unsigned(spent)),
            Part::text(format!(" refunded, so {category} counts ")),
            Part::mono(unsigned(row.share.left)),
            Part::text(". ✕ unlinks; Undo brings it back."),
        ]);
    }
    if row.links.is_empty() {
        let words = if row.amount.is_negative() {
            "Link a refund of this, or a payment that repays it."
        } else {
            "Link this to a purchase it refunds, a claim it repays or someone who paid you back."
        };
        return Some(vec![Part::text(words)]);
    }
    None
}

fn absolute(amount: Money) -> Money {
    Money::new(amount.minor().saturating_abs(), amount.currency())
}

/// Why Coffer suggests the refund (Paper `APT-0`): "Same merchant and
/// account, 11 days earlier, for more than this amount."
pub fn refund_why(why: &RefundWhy) -> String {
    let same = match (why.same_merchant, why.same_account) {
        (true, true) => "Same merchant and account",
        (true, false) => "Same merchant",
        (false, true) => "Same account",
        (false, false) => "A purchase",
    };
    let when = match why.days_earlier {
        0 => "the same day".to_owned(),
        1 => "1 day earlier".to_owned(),
        days => format!("{days} days earlier"),
    };
    let amount = if why.exact {
        "for exactly this amount"
    } else {
        "for more than this amount"
    };
    format!("{same}, {when}, {amount}.")
}

/// What linking the suggested refund changes (Paper `AQ0-0`): "Linked, it
/// takes the purchase's category, so it's £34.00 less Shopping and never
/// income. The purchase shows £34.00 of £68.00 refunded."
pub fn refund_effect(register: &Register, purchase: &RegisterRow, amount: Money) -> Vec<Part> {
    let spent = absolute(purchase.amount);
    let refunded = Money::new(
        spent.minor() - purchase.share.left.minor() + amount.minor(),
        spent.currency(),
    );
    let mut parts = vec![Part::text(
        "Linked, it takes the purchase's category, so it's ",
    )];
    if register.row_categories(purchase).is_empty() {
        parts.push(Part::text("never income."));
    } else {
        let category = present::category_label(register, purchase);
        parts.push(Part::mono(unsigned(amount)).color(color::TEXT_STRONG));
        parts.push(Part::text(format!(" less {category}")).color(color::TEXT_STRONG));
        parts.push(Part::text(" and never income."));
    }
    parts.extend([
        Part::text(" The purchase shows "),
        Part::mono(unsigned(refunded)),
        Part::text(" of "),
        Part::mono(unsigned(spent)),
        Part::text(" refunded."),
    ]);
    parts
}

/// The suggestion card's title and why, for a payment's set of claims
/// (composed for M3.11 from the refund card and Owed to you's strip).
pub fn reimbursement_why(claims: usize) -> (&'static str, String) {
    let what = if claims == 1 {
        "the marked claim".to_owned()
    } else {
        format!("{claims} marked claims")
    };
    (
        "Does this repay your claims?",
        format!("It matches {what} to the penny."),
    )
}

/// What linking a payment's claims changes.
pub fn reimbursement_effect(claims: usize) -> String {
    if claims == 1 {
        "Linked, the claim clears and the payment isn't income.".into()
    } else {
        "Linked, the claims clear and the payment isn't income.".into()
    }
}

/// The Share section's caption for a shared purchase (Paper `CBP-0`).
pub fn shared_caption(mine: Money, person: Option<&str>) -> Vec<Part> {
    match person {
        Some(person) => {
            let person = name(person);
            vec![
                Part::text("Budgets and reports count your "),
                Part::mono(unsigned(mine)),
                Part::text(format!(
                    ". {person}'s part waits under Owed to you until a payment from {person} \
                     is linked. Names stay on this computer."
                )),
            ]
        }
        None => vec![
            Part::text("Budgets and reports count your "),
            Part::mono(unsigned(mine)),
            Part::text(
                " once you choose who owes the rest. It waits under Owed to you until they \
                 pay. Names stay on this computer.",
            ),
        ],
    }
}

/// The Share section's caption for a claim (Paper `FJY-0`).
pub const CLAIM_CAPTION: &str = "Budgets and reports leave it out while it's claimed. It stays on \
                                 Owed to you until repayments cover it.";

/// The refund's read-only Category caption (Paper `FNM-0`).
pub const FOLLOWS_CAPTION: &str =
    "Set by the linked purchase, so the refund nets against it. Unlink to set it yourself.";

/// What the undo bar says after a link: "Linked the ASOS.com refund to the
/// 28 Oct purchase" (Paper `EC0-0`).
pub fn linked_message(
    kind: LinkKind,
    inflow: &RegisterRow,
    purchases: &[&RegisterRow],
    person: Option<&str>,
    today: NaiveDate,
) -> String {
    let merchant = name(&inflow.merchant);
    match (kind, purchases, person) {
        (LinkKind::Refund, [purchase], _) => format!(
            "Linked the {merchant} refund to the {} purchase",
            format::list_date(purchase.date, today)
        ),
        (LinkKind::Reimbursement, [_], _) => format!("Linked {merchant} to the claim"),
        (LinkKind::Reimbursement, many, _) if many.len() > 1 => {
            format!("Linked {merchant} to {} claims", many.len())
        }
        (LinkKind::Settlement, _, Some(person)) => {
            format!("Linked {merchant} as a payment from {}", name(person))
        }
        _ => format!("Linked {merchant}"),
    }
}

/// What the undo bar says after an unlink or a rejected suggestion.
pub fn unlinked_message(kind: LinkKind, suggestion: bool) -> &'static str {
    match (kind, suggestion) {
        (LinkKind::Refund, true) => "Coffer won't suggest that refund again",
        (_, true) => "Coffer won't suggest that payment again",
        (LinkKind::Refund, false) => "Unlinked the refund",
        (LinkKind::Reimbursement, false) => "Unlinked the reimbursement",
        (LinkKind::Settlement, false) => "Unlinked the payment",
    }
}

/// A kind's label in the dialog's Kind control.
pub fn kind_label(kind: LinkKind) -> &'static str {
    match kind {
        LinkKind::Refund => "Refund",
        LinkKind::Reimbursement => "Reimbursement",
        LinkKind::Settlement => "Settlement",
    }
}

/// The dialog's commit button: "Link refund".
pub fn link_button(kind: LinkKind) -> &'static str {
    match kind {
        LinkKind::Refund => "Link refund",
        LinkKind::Reimbursement => "Link reimbursement",
        LinkKind::Settlement => "Link payment",
    }
}

/// The dialog's caption under Amount (Paper `EYF-0`).
pub fn dialog_caption(kind: LinkKind, category: Option<&str>) -> String {
    match (kind, category) {
        (LinkKind::Refund, Some(category)) => format!(
            "The refund takes the purchase's category, {category}. Part of a refund can go to \
             one purchase and the rest to another."
        ),
        (LinkKind::Refund, None) => "The refund takes the purchase's category. Part of a refund \
                                     can go to one purchase and the rest to another."
            .into(),
        (LinkKind::Reimbursement, _) => "The payment clears the claim. Part of a payment can \
                                         repay one claim and the rest another."
            .into(),
        (LinkKind::Settlement, _) => "The payment comes off what they owe. A part payment \
                                      leaves the rest owed."
            .into(),
    }
}

/// What the Amount field's hint says: "all of this refund".
pub fn amount_hint(kind: LinkKind, amount: Money, from_left: Money) -> Option<&'static str> {
    (amount == from_left).then_some(match kind {
        LinkKind::Refund => "all of this refund",
        _ => "all of this payment",
    })
}

#[cfg(test)]
mod tests {
    use coffer_core::money::Currency;

    use super::*;

    fn gbp(minor: i64) -> Money {
        Money::new(minor, Currency::GBP)
    }

    #[test]
    fn why_a_refund_is_suggested() {
        let why = RefundWhy {
            same_merchant: true,
            same_account: true,
            days_earlier: 11,
            exact: false,
        };
        assert_eq!(
            refund_why(&why),
            "Same merchant and account, 11 days earlier, for more than this amount."
        );
        let why = RefundWhy {
            same_account: false,
            days_earlier: 1,
            exact: true,
            ..why
        };
        assert_eq!(
            refund_why(&why),
            "Same merchant, 1 day earlier, for exactly this amount."
        );
    }

    #[test]
    fn captions_name_the_person_plainly() {
        let parts = shared_caption(gbp(-1590), Some("Sam\u{202e}"));
        let text: String = parts.iter().map(|part| part.text.as_str()).collect();
        assert_eq!(
            text,
            "Budgets and reports count your £15.90. Sam's part waits under Owed to you until \
             a payment from Sam is linked. Names stay on this computer."
        );
        assert_eq!(
            unlinked_message(LinkKind::Refund, false),
            "Unlinked the refund"
        );
        assert_eq!(link_button(LinkKind::Settlement), "Link payment");
        assert_eq!(
            amount_hint(LinkKind::Refund, gbp(3400), gbp(3400)),
            Some("all of this refund")
        );
        assert_eq!(amount_hint(LinkKind::Refund, gbp(3000), gbp(3400)), None);
    }
}
