//! The rule in both places it is read: from the database and from the
//! Transactions table's rows. Every case runs through both, and they must
//! agree.

use chrono::NaiveDate;

use super::*;
use crate::link::{self, LinkKind, LinkTarget, NewLink, ShareLabel};
use crate::money::{Currency, Money};
use crate::reporting::register::Register;
use crate::reporting::test_ledger::{SHARE_LABEL, TestLedger};

#[test]
fn the_rule_checks_a_transfer_then_a_split_then_a_refund() {
    let none = CategoryFacts::default();
    assert_eq!(none.no_own_category(), None);
    let split = CategoryFacts {
        split: true,
        ..none
    };
    assert_eq!(split.no_own_category(), Some(NoOwnCategory::Split));
    let transfer = CategoryFacts {
        confirmed_transfer: true,
        ..none
    };
    assert_eq!(transfer.no_own_category(), Some(NoOwnCategory::Transfer));
    let refund = CategoryFacts {
        confirmed_refund_in: true,
        ..none
    };
    assert_eq!(refund.no_own_category(), Some(NoOwnCategory::Refund));
    let all = CategoryFacts {
        split: true,
        confirmed_transfer: true,
        confirmed_refund_in: true,
    };
    assert_eq!(all.no_own_category(), Some(NoOwnCategory::Transfer));
    assert_eq!(
        CategoryFacts {
            confirmed_transfer: false,
            ..all
        }
        .no_own_category(),
        Some(NoOwnCategory::Split)
    );
}

#[test]
fn its_words_name_no_transaction_text() {
    assert_eq!(
        NoOwnCategory::Split.to_string(),
        "it is split, so its lines take categories"
    );
    assert_eq!(
        NoOwnCategory::Transfer.to_string(),
        "it is in a confirmed transfer, which takes no category"
    );
    assert_eq!(
        NoOwnCategory::Refund.to_string(),
        "it is a linked refund, which counts in its purchase's category"
    );
}

#[test]
fn the_database_and_the_register_agree_on_every_case() {
    let mut ledger = TestLedger::new();
    let current = ledger.account("Current", Currency::GBP);
    let savings = ledger.account("Savings", Currency::GBP);

    let plain = ledger.row(current, "2026-11-02", -1200, Some("shopping"));
    let uncategorised = ledger.row(current, "2026-11-02", -800, None);
    let split = ledger.row(current, "2026-11-03", -5000, None);
    ledger.split(split, &[(-3000, Some("food.groceries")), (-2000, None)]);

    let out = ledger.row(current, "2026-11-04", -25_000, None);
    let into = ledger.row(savings, "2026-11-04", 25_000, None);
    ledger.transfer(out, into);
    let maybe_out = ledger.row(current, "2026-11-05", -7_000, None);
    let maybe_in = ledger.row(savings, "2026-11-05", 7_000, None);
    ledger.suggested_transfer(maybe_out, maybe_in);

    let purchase = ledger.row(current, "2026-11-06", -6800, Some("shopping"));
    let refund = ledger.row(current, "2026-11-08", 3400, None);
    ledger.refund(refund, purchase, 3400);
    let other_purchase = ledger.row(current, "2026-11-06", -2000, Some("shopping"));
    let maybe_refund = ledger.row(current, "2026-11-09", 2000, None);
    ledger.suggested_refund(maybe_refund, other_purchase, 2000);

    let claimed = ledger.row(current, "2026-11-06", -4000, Some("travel"));
    ledger.claim_back(claimed);
    let repaid = ledger.row(current, "2026-11-10", 4000, None);
    ledger.reimburse(repaid, claimed, 4000);
    let shared = ledger.row(current, "2026-11-07", -2500, Some("food.eating_out"));
    ledger.share(shared, -1000);
    let settled = ledger.row(current, "2026-11-10", 1500, None);
    link::create(
        &mut ledger.db,
        &NewLink {
            kind: LinkKind::Settlement,
            inflow: settled,
            target: LinkTarget::Person(ShareLabel::new(SHARE_LABEL).unwrap()),
            amount: Money::new(1500, Currency::GBP),
        },
    )
    .unwrap();

    let cases = [
        ("plain", plain, None),
        ("uncategorised", uncategorised, None),
        ("split", split, Some(NoOwnCategory::Split)),
        ("transfer out", out, Some(NoOwnCategory::Transfer)),
        ("transfer in", into, Some(NoOwnCategory::Transfer)),
        ("suggested transfer out", maybe_out, None),
        ("suggested transfer in", maybe_in, None),
        ("refund's money in", refund, Some(NoOwnCategory::Refund)),
        ("refund's purchase", purchase, None),
        ("suggested refund's money in", maybe_refund, None),
        ("suggested refund's purchase", other_purchase, None),
        ("reimbursement's money in", repaid, None),
        ("claimed purchase", claimed, None),
        ("shared purchase", shared, None),
        ("settlement's money in", settled, None),
    ];

    let today = NaiveDate::from_ymd_opt(2026, 11, 15).unwrap();
    let register = Register::load(&ledger.reader(), today).unwrap();
    for (name, id, expected) in cases {
        let row = register.row(id).unwrap();
        let stored = facts_in(ledger.db.conn(), id).unwrap().unwrap();
        assert_eq!(row.category_facts(), stored, "{name}: the facts");
        assert_eq!(
            no_own_category_in(ledger.db.conn(), id).unwrap(),
            expected,
            "{name}: the database"
        );
        assert_eq!(row.no_own_category(), expected, "{name}: the register");
        assert_eq!(row.takes_category(), expected.is_none(), "{name}");
        assert_eq!(
            row.category_follows_purchase(),
            expected == Some(NoOwnCategory::Refund),
            "{name}"
        );
    }
    assert_eq!(
        register
            .refund_purchase(register.row(refund).unwrap())
            .map(|row| row.id),
        Some(purchase)
    );
    assert_eq!(
        register.refund_purchase(register.row(maybe_refund).unwrap()),
        None
    );

    // No such transaction: nothing to say; the write reports it.
    let gone = crate::id::TransactionId::new(9_999);
    assert_eq!(facts_in(ledger.db.conn(), gone).unwrap(), None);
    assert_eq!(no_own_category_in(ledger.db.conn(), gone).unwrap(), None);
}
