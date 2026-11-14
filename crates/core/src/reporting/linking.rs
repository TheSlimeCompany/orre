//! `linking`: what the Transactions view says about links and shares, read
//! from the [`Register`] (ROADMAP M3.11; `DESIGN.md` → "Refunds,
//! reimbursements and shared costs"; `ARCHITECTURE.md` → "Decision record:
//! refunds, reimbursements and settlements (built)").
//!
//! Pure functions over rows already in memory, so the table, the inspector
//! and the link dialog ask nothing of the database while the user types:
//!
//! - [`counts_as`]: how a row counts, for the category cell's suffix
//!   ("Shopping · refund", "Repays 2 claims");
//! - [`candidates`] and [`people`]: what the link dialog offers, same
//!   merchant first, then newest, with what each can still take and why a
//!   row can't take more;
//! - [`refund_why`]: why Coffer suggested a refund.
//!
//! Whatever the user picks is still checked by core's `link` when it is
//! written (its checks and caps decide). Merchant names and descriptions
//! are untrusted text: only ever compared and matched, never acted on.

use chrono::NaiveDate;

use crate::id::TransactionId;
use crate::link::{LinkKind, LinkStatus, MAX_LINKED_PURCHASE_MINOR, PersonOwed};
use crate::money::{Currency, Money};

use super::register::{LinkOther, LinkSide, Register, RegisterRow, RowCategory, RowLink};

/// The most candidates the link dialog lists: the search narrows the rest.
pub const CANDIDATE_LIMIT: usize = 50;

/// How a row counts, beyond its category (the category cell's suffix).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Counts {
    /// As the bank moved it, by its own category.
    Plain,
    /// Money in linked as a refund: counts in its purchase's category
    /// (`None` when that purchase is split or uncategorised).
    Refund { category: Option<i64> },
    /// A purchase with a refund linked.
    Refunded,
    /// A purchase (or a line of it) shared with someone.
    Shared,
    /// Claimed back, with something still to come back.
    ToClaim,
    /// Claimed back and fully repaid.
    Claimed,
    /// Money in that repays this many claims.
    RepaysClaims(usize),
    /// Money in that repays one person: their local name.
    RepaysPerson(String),
    /// Money in that repays this many people.
    RepaysPeople(usize),
}

fn confirmed(row: &RegisterRow) -> impl Iterator<Item = &RowLink> {
    row.links
        .iter()
        .filter(|link| link.status == LinkStatus::Confirmed)
}

/// How `row` counts (module docs). Money out: a claim first, then a refund,
/// then a share. Money in: a refund first, then reimbursements, then
/// settlements.
pub fn counts_as(register: &Register, row: &RegisterRow) -> Counts {
    if row.amount.is_negative() {
        if row.share.claim {
            return if row.share.left.is_positive() {
                Counts::ToClaim
            } else {
                Counts::Claimed
            };
        }
        if confirmed(row).any(|link| link.kind == LinkKind::Refund) {
            return Counts::Refunded;
        }
        let shared_line = match &row.category {
            RowCategory::Split(parts) => parts.iter().any(|part| part.share_label.is_some()),
            _ => false,
        };
        if row.share.label.is_some() || shared_line {
            return Counts::Shared;
        }
        return Counts::Plain;
    }
    let links: Vec<&RowLink> = confirmed(row)
        .filter(|link| link.side == LinkSide::Inflow)
        .collect();
    if let Some(refund) = links.iter().find(|link| link.kind == LinkKind::Refund) {
        let category =
            match &refund.other {
                LinkOther::Transaction(id) => register.row(*id).and_then(|purchase| match purchase
                    .category
                {
                    RowCategory::Category(category) => Some(category),
                    _ => None,
                }),
                LinkOther::Person(_) => None,
            };
        return Counts::Refund { category };
    }
    let claims = links
        .iter()
        .filter(|link| link.kind == LinkKind::Reimbursement)
        .count();
    if claims > 0 {
        return Counts::RepaysClaims(claims);
    }
    let mut people: Vec<&str> = links
        .iter()
        .filter_map(|link| match &link.other {
            LinkOther::Person(label) => Some(label.as_str()),
            LinkOther::Transaction(_) => None,
        })
        .collect();
    people.sort_unstable();
    people.dedup();
    match people.as_slice() {
        [] => Counts::Plain,
        [one] => Counts::RepaysPerson((*one).to_owned()),
        many => Counts::RepaysPeople(many.len()),
    }
}

/// Whether two merchant names are the same, ignoring case (as Coffer's
/// refund suggestions compare them).
pub fn same_merchant(a: &str, b: &str) -> bool {
    a.trim().to_lowercase() == b.trim().to_lowercase()
}

/// Whether `row` can be the money in of a link: money in, outside a
/// transfer, not split, no share of its own, not a duplicate.
pub fn can_be_inflow(row: &RegisterRow) -> bool {
    row.amount.is_positive()
        && !row.in_transfer
        && !matches!(row.category, RowCategory::Split(_))
        && row.share.my_share.is_none()
        && row.share.label.is_none()
        && row.excluded != Some(crate::exclusion::ExclusionReason::Duplicate)
}

/// Whether `row` can be the purchase of a link: money out, outside a
/// transfer, not a duplicate, no larger than a link can take.
pub fn can_be_purchase(row: &RegisterRow) -> bool {
    row.amount.is_negative()
        && !row.in_transfer
        && row.amount.minor() >= -MAX_LINKED_PURCHASE_MINOR
        && row.excluded != Some(crate::exclusion::ExclusionReason::Duplicate)
}

/// Which kinds of link `row` can start from in the dialog: money in can be
/// a refund, a reimbursement or a settlement; a purchase can take a refund,
/// and a reimbursement when it's claimed back.
pub fn kinds_from(row: &RegisterRow) -> Vec<LinkKind> {
    if can_be_inflow(row) {
        LinkKind::ALL.to_vec()
    } else if can_be_purchase(row) {
        if row.share.claim {
            vec![LinkKind::Refund, LinkKind::Reimbursement]
        } else {
            vec![LinkKind::Refund]
        }
    } else {
        Vec::new()
    }
}

/// Why a candidate can't take a link now.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Blocked {
    /// A purchase with nothing left to refund.
    AlreadyRefunded,
    /// A claim fully repaid.
    AlreadyRepaid,
    /// Money in with nothing left to link.
    AllLinked,
}

/// A transaction the link dialog offers.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Candidate {
    pub transaction: TransactionId,
    /// What it can still take (a purchase) or has left (money in).
    pub left: Money,
    /// The same merchant as the row linked from (listed first).
    pub same_merchant: bool,
    /// Why it can't be chosen; listed, dimmed, so nothing disappears.
    pub blocked: Option<Blocked>,
}

/// What the link dialog offers for a `kind` link from `from`, matching
/// `query` (the table's search; blank matches all): same merchant first,
/// then newest, at most [`CANDIDATE_LIMIT`]. Pairs already linked are left
/// out: they're in the Links section. A pair Coffer only suggested stays
/// (linking it confirms the suggestion).
///
/// - From money in: purchases in its currency on or before its date; for a
///   reimbursement, only purchases claimed back.
/// - From a purchase: money in, in its currency, on or after its date.
/// - Settlements link people, not transactions: see [`people`].
pub fn candidates(
    register: &Register,
    from: &RegisterRow,
    kind: LinkKind,
    query: &str,
) -> Vec<Candidate> {
    if kind == LinkKind::Settlement {
        return Vec::new();
    }
    let linked: Vec<TransactionId> = from
        .links
        .iter()
        .filter(|link| link.status == LinkStatus::Confirmed)
        .filter_map(|link| match link.other {
            LinkOther::Transaction(id) => Some(id),
            LinkOther::Person(_) => None,
        })
        .collect();
    let currency = from.amount.currency();
    let from_inflow = can_be_inflow(from);
    let from_purchase = !from_inflow && can_be_purchase(from);
    if !from_inflow && !from_purchase {
        return Vec::new();
    }
    let mut out: Vec<Candidate> = register
        .rows()
        .iter()
        .filter(|row| row.id != from.id && !linked.contains(&row.id))
        .filter(|row| row.amount.currency() == currency)
        .filter(|row| {
            if from_inflow {
                can_be_purchase(row)
                    && row.date <= from.date
                    && (kind != LinkKind::Reimbursement || row.share.claim)
            } else {
                can_be_inflow(row) && row.date >= from.date
            }
        })
        .filter(|row| register.row_matches(row.id, query))
        .map(|row| {
            let blocked = if row.share.left.is_positive() {
                None
            } else if from_inflow && row.share.claim {
                Some(Blocked::AlreadyRepaid)
            } else if from_inflow {
                Some(Blocked::AlreadyRefunded)
            } else {
                Some(Blocked::AllLinked)
            };
            Candidate {
                transaction: row.id,
                left: row.share.left,
                same_merchant: same_merchant(&row.merchant, &from.merchant),
                blocked,
            }
        })
        .collect();
    // Rows are newest first already; a stable sort keeps that within each.
    out.sort_by_key(|candidate| !candidate.same_merchant);
    out.truncate(CANDIDATE_LIMIT);
    out
}

/// The payments a person's settlement can come from ("Link a payment…" on
/// Owed to you): money in, in `currency`, matching `query`, newest first,
/// at most [`CANDIDATE_LIMIT`]; one with nothing left to link is listed,
/// blocked.
pub fn payments(register: &Register, currency: Currency, query: &str) -> Vec<Candidate> {
    register
        .rows()
        .iter()
        .filter(|row| row.amount.currency() == currency && can_be_inflow(row))
        .filter(|row| register.row_matches(row.id, query))
        .map(|row| Candidate {
            transaction: row.id,
            left: row.share.left,
            same_merchant: false,
            blocked: (!row.share.left.is_positive()).then_some(Blocked::AllLinked),
        })
        .take(CANDIDATE_LIMIT)
        .collect()
}

/// The people a settlement from `from` (money in) can repay: everyone who
/// owes the user in its currency and matches `query` (ignoring case), by
/// name.
pub fn people<'a>(register: &'a Register, from: &RegisterRow, query: &str) -> Vec<&'a PersonOwed> {
    let query = query.trim().to_lowercase();
    if !can_be_inflow(from) {
        return Vec::new();
    }
    register
        .people()
        .iter()
        .filter(|person| {
            person.owed.currency() == from.amount.currency()
                && person.owed.is_positive()
                && person.label.as_str().to_lowercase().contains(&query)
        })
        .collect()
}

/// What a new link defaults to: as much as both sides can take, so one
/// refund can be split across purchases.
pub fn default_amount(from_left: Money, other_left: Money) -> Money {
    if other_left.minor() < from_left.minor() {
        other_left
    } else {
        from_left
    }
}

/// Why Coffer suggested a refund (decision record "refunds, reimbursements
/// and shared costs", point 2).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RefundWhy {
    pub same_merchant: bool,
    pub same_account: bool,
    /// How many days before the refund the purchase was.
    pub days_earlier: i64,
    /// The purchase has exactly this much left to refund (otherwise more).
    pub exact: bool,
}

/// Why `link`, a suggested refund on `refund`'s side, was suggested;
/// `None` for anything else or when the purchase isn't in the register.
pub fn refund_why(register: &Register, refund: &RegisterRow, link: &RowLink) -> Option<RefundWhy> {
    if link.kind != LinkKind::Refund || link.side != LinkSide::Inflow {
        return None;
    }
    let LinkOther::Transaction(id) = link.other else {
        return None;
    };
    let purchase = register.row(id)?;
    Some(RefundWhy {
        same_merchant: same_merchant(&purchase.merchant, &refund.merchant),
        same_account: purchase.account == refund.account,
        days_earlier: days(purchase.date, refund.date),
        exact: purchase.share.left == link.amount,
    })
}

fn days(from: NaiveDate, to: NaiveDate) -> i64 {
    (to - from).num_days()
}

#[cfg(test)]
mod tests;
