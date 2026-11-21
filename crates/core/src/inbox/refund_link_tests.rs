//! Linking a suggested transfer pair as a refund ([`link_as_refund`]) and
//! undoing it: one write, all or nothing.

use rusqlite::params;
use serde_json::json;

use super::super::testing::{Fixture, day};
use super::super::{SuggestionKind, SuggestionStatus, get};
use super::*;
use crate::money::Currency;

/// Detection's suggested group for `out` and `inn`, as an import leaves it.
fn suggested_pair(f: &Fixture, out: TransactionId, inn: TransactionId) -> TransferGroupId {
    f.transfer(out, inn, false);
    transfer::group_of(&f.db, out).unwrap().unwrap().id
}

fn counts(f: &Fixture) -> Vec<i64> {
    ["transaction_link", "change_batch"]
        .iter()
        .map(|table| f.count(table))
        .collect()
}

fn status(f: &Fixture, group: TransferGroupId) -> TransferStatus {
    transfer::group(&f.db, group).unwrap().status
}

#[test]
fn linking_a_suggested_pair_as_a_refund_rejects_it_and_links_in_one_step() {
    let mut f = Fixture::new();
    let purchase = f.row(day(2), -1240, "Odeon", Some("shopping"));
    let refund = f.row_on(f.savings, day(4), 1240, "Cash", None);
    let group = suggested_pair(&f, purchase, refund);
    let pending = f.file(
        SuggestionKind::TransferPair,
        json!({"outflow_id": purchase.get(), "inflow_id": refund.get()}),
    );

    let decided = link_as_refund(&mut f.db, group).unwrap();
    let linked = decided.linked;

    assert_eq!(linked.inflow, refund);
    assert_eq!(linked.purchase, purchase);
    assert_eq!(linked.amount, Money::new(1240, Currency::GBP));
    assert_eq!(decided.settled, vec![pending]);
    assert_eq!(status(&f, group), TransferStatus::Rejected);
    let stored = link::get(&f.db, linked.link).unwrap();
    assert_eq!(stored.kind, LinkKind::Refund);
    assert_eq!(stored.status, LinkStatus::Confirmed);
    assert_eq!(stored.inflow, refund);
    assert_eq!(stored.target, LinkTarget::Purchase(purchase));
    assert_eq!(stored.amount, linked.amount);
    let settled = get(&f.db, pending).unwrap();
    assert_eq!(settled.status, SuggestionStatus::Rejected);
    assert_eq!(
        settled.decision_reason.as_deref(),
        Some(DECIDED_IN_TRANSACTIONS)
    );
}

#[test]
fn a_refund_the_link_rules_refuse_leaves_the_pair_suggested() {
    let mut f = Fixture::new();
    let purchase = f.row(day(2), -1240, "Odeon", None);
    let refund = f.row_on(f.savings, day(4), 1240, "Cash", None);
    let group = suggested_pair(&f, purchase, refund);
    // Part of the purchase is refunded already, so a whole refund doesn't fit.
    let earlier = f.row_on(f.savings, day(3), 500, "Other refund", None);
    f.conn()
        .execute(
            "INSERT INTO transaction_link (kind, inflow_id, target_id, amount_minor, status) \
             VALUES ('refund', ?1, ?2, 500, 'confirmed')",
            params![earlier, purchase],
        )
        .unwrap();
    let before = counts(&f);

    let error = link_as_refund(&mut f.db, group).unwrap_err();

    assert!(
        matches!(
            error,
            ApproveError::Link(link::Error::ExceedsPurchase { .. })
        ),
        "{error:?}"
    );
    assert_eq!(counts(&f), before);
    assert_eq!(
        status(&f, group),
        TransferStatus::Suggested,
        "the rejection was rolled back"
    );
}

#[test]
fn a_decided_pair_is_not_linked_as_a_refund() {
    let mut f = Fixture::new();
    let purchase = f.row(day(2), -1240, "Odeon", None);
    let refund = f.row_on(f.savings, day(4), 1240, "Cash", None);
    let group = suggested_pair(&f, purchase, refund);
    decide_transfer(&mut f.db, group, transfer::Action::Confirm).unwrap();

    let error = link_as_refund(&mut f.db, group).unwrap_err();

    assert!(
        matches!(
            error,
            ApproveError::Transfer(transfer::Error::Blocked(
                transfer::Blocked::WrongStatus { .. }
            ))
        ),
        "{error:?}"
    );
    assert_eq!(status(&f, group), TransferStatus::Confirmed);
    assert!(link::links_of(&f.db, refund).unwrap().is_empty());
}

#[test]
fn undoing_a_refund_link_deletes_it_and_suggests_the_pair_again() {
    let mut f = Fixture::new();
    let purchase = f.row(day(2), -1240, "Odeon", None);
    let refund = f.row_on(f.savings, day(4), 1240, "Cash", None);
    let group = suggested_pair(&f, purchase, refund);
    let decided = link_as_refund(&mut f.db, group).unwrap();
    let linked = decided.linked;

    undo_refund_linked(&mut f.db, &linked).unwrap();

    assert_eq!(status(&f, group), TransferStatus::Suggested);
    assert!(link::links_of(&f.db, refund).unwrap().is_empty());
    assert!(link::links_of(&f.db, purchase).unwrap().is_empty());
}

#[test]
fn a_refund_link_that_changed_since_is_not_undone() {
    let mut f = Fixture::new();
    let purchase = f.row(day(2), -1240, "Odeon", None);
    let refund = f.row_on(f.savings, day(4), 1240, "Cash", None);
    let group = suggested_pair(&f, purchase, refund);
    let decided = link_as_refund(&mut f.db, group).unwrap();
    let linked = decided.linked;
    link::reject(&mut f.db, linked.link).unwrap();

    let error = undo_refund_linked(&mut f.db, &linked).unwrap_err();

    assert!(
        matches!(error, ApproveError::Transfer(transfer::Error::Changed(g)) if g == group),
        "{error:?}"
    );
    assert_eq!(status(&f, group), TransferStatus::Rejected);
}

#[test]
fn undoing_a_refund_link_puts_back_the_links_it_replaced_or_dropped() {
    let mut f = Fixture::new();
    let purchase = f.row(day(2), -1240, "Odeon", None);
    let other = f.row(day(1), -1240, "Cinema", None);
    let refund = f.row_on(f.savings, day(4), 1240, "Cash", None);
    let group = suggested_pair(&f, purchase, refund);
    // A suggestion for the pair itself (the refund confirms it in place) and
    // one to another purchase (the refund leaves no room for it, so it is
    // dropped).
    for (target, amount) in [(purchase, 600), (other, 1240)] {
        f.conn()
            .execute(
                "INSERT INTO transaction_link (kind, inflow_id, target_id, amount_minor, status) \
                 VALUES ('refund', ?1, ?2, ?3, 'suggested')",
                params![refund, target, amount],
            )
            .unwrap();
    }
    let before = link::links_of(&f.db, refund).unwrap();
    assert_eq!(before.len(), 2);

    let linked = link_as_refund(&mut f.db, group).unwrap().linked;
    let after = link::links_of(&f.db, refund).unwrap();
    assert_eq!(after.len(), 1, "the suggestion to the other purchase went");
    assert_eq!(after[0].status, LinkStatus::Confirmed);

    undo_refund_linked(&mut f.db, &linked).unwrap();

    assert_eq!(link::links_of(&f.db, refund).unwrap(), before);
    assert_eq!(status(&f, group), TransferStatus::Suggested);
}

#[test]
fn a_dropped_suggestion_that_no_longer_fits_stays_dropped_and_undo_still_works() {
    let mut f = Fixture::new();
    let purchase = f.row(day(2), -1240, "Odeon", None);
    let refund = f.row_on(f.savings, day(4), 1240, "Cash", None);
    let elsewhere = f.row(day(3), 1240, "Odeon refund", None);
    let group = suggested_pair(&f, purchase, refund);
    // Another payment suggested as the purchase's refund: the refund leaves
    // the purchase no room for it, so it is dropped.
    f.conn()
        .execute(
            "INSERT INTO transaction_link (kind, inflow_id, target_id, amount_minor, status) \
             VALUES ('refund', ?1, ?2, 1240, 'suggested')",
            params![elsewhere, purchase],
        )
        .unwrap();
    let linked = link_as_refund(&mut f.db, group).unwrap().linked;
    assert!(link::links_of(&f.db, elsewhere).unwrap().is_empty());
    // Since then, that payment got a share of its own: a link can't name it.
    f.conn()
        .execute(
            "UPDATE \"transaction\" SET my_share_minor = 600 WHERE id = ?1",
            [elsewhere],
        )
        .unwrap();

    undo_refund_linked(&mut f.db, &linked).unwrap();

    assert!(link::links_of(&f.db, elsewhere).unwrap().is_empty());
    assert!(link::links_of(&f.db, refund).unwrap().is_empty());
    assert_eq!(status(&f, group), TransferStatus::Suggested);
}
