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
        ..none
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
fn a_transaction_needs_a_category_when_it_takes_one_and_has_none_or_a_split_line_has_none() {
    for bits in 0..32u8 {
        let has = |bit: u8| bits & (1 << bit) != 0;
        let facts = CategoryFacts {
            split: has(0),
            confirmed_transfer: has(1),
            confirmed_refund_in: has(2),
            has_category: has(3),
            split_line_uncategorised: has(4),
        };
        // Written out without `no_own_category`, so the rule is checked
        // rather than restated.
        let expected = !facts.confirmed_transfer
            && if facts.split {
                facts.split_line_uncategorised
            } else {
                !facts.confirmed_refund_in && !facts.has_category
            };
        assert_eq!(facts.needs_category(), expected, "{facts:?}");
    }
    let bare = CategoryFacts::default();
    assert!(bare.needs_category());
    assert!(
        !CategoryFacts {
            has_category: true,
            ..bare
        }
        .needs_category()
    );
    assert!(
        !CategoryFacts {
            confirmed_refund_in: true,
            ..bare
        }
        .needs_category()
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
    ledger.start_importing();

    let plain = ledger.row(current, "2026-11-02", -1200, Some("shopping"));
    let uncategorised = ledger.row(current, "2026-11-02", -800, None);
    let split = ledger.row(current, "2026-11-03", -5000, None);
    ledger.split(split, &[(-3000, Some("food.groceries")), (-2000, None)]);

    let split_done = ledger.row(current, "2026-11-03", -4000, None);
    ledger.split(
        split_done,
        &[(-3000, Some("food.groceries")), (-1000, Some("shopping"))],
    );

    let out = ledger.row(current, "2026-11-04", -25_000, None);
    let into = ledger.row(savings, "2026-11-04", 25_000, None);
    ledger.transfer(out, into);
    let maybe_out = ledger.row(current, "2026-11-05", -7_000, None);
    let maybe_in = ledger.row(savings, "2026-11-05", 7_000, None);
    ledger.suggested_transfer(maybe_out, maybe_in);

    let purchase = ledger.row(current, "2026-11-06", -6800, Some("shopping"));
    let refund = ledger.row(current, "2026-11-08", 3400, None);
    ledger.refund(refund, purchase, 3400);
    // Money in only partly linked: its uncategorised remainder still shows in
    // reports, but the row takes no category of its own, so it needs none.
    let big_purchase = ledger.row(current, "2026-11-06", -9000, Some("shopping"));
    let partly = ledger.row(current, "2026-11-08", 5000, None);
    ledger.refund(partly, big_purchase, 3000);
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

    // Rows that need a category, by the rule.
    let needs = [
        uncategorised,
        split,
        maybe_out,
        maybe_in,
        maybe_refund,
        repaid,
        settled,
    ];
    let cases = [
        ("plain", plain, None),
        ("uncategorised", uncategorised, None),
        ("split", split, Some(NoOwnCategory::Split)),
        (
            "fully categorised split",
            split_done,
            Some(NoOwnCategory::Split),
        ),
        ("transfer out", out, Some(NoOwnCategory::Transfer)),
        ("transfer in", into, Some(NoOwnCategory::Transfer)),
        ("suggested transfer out", maybe_out, None),
        ("suggested transfer in", maybe_in, None),
        ("refund's money in", refund, Some(NoOwnCategory::Refund)),
        ("refund's purchase", purchase, None),
        (
            "partly linked refund's money in",
            partly,
            Some(NoOwnCategory::Refund),
        ),
        ("partly refunded purchase", big_purchase, None),
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
            row.is_uncategorised(),
            needs.contains(&id),
            "{name}: the register needs a category"
        );
        assert_eq!(
            stored.needs_category(),
            needs.contains(&id),
            "{name}: the database needs a category"
        );
        assert_eq!(
            row.category_follows_purchase(),
            expected == Some(NoOwnCategory::Refund),
            "{name}"
        );
    }
    let in_batches = |ledger: &mut TestLedger| {
        let (current_batch, savings_batch) = (ledger.batch_of(current), ledger.batch_of(savings));
        needs_category_in_batch(ledger.db.conn(), current_batch).unwrap()
            + needs_category_in_batch(ledger.db.conn(), savings_batch).unwrap()
    };
    assert_eq!(in_batches(&mut ledger), needs.len());
    // A manual row isn't an imported row of the batch.
    ledger.stop_importing();
    ledger.row(current, "2026-11-11", -100, None);
    assert_eq!(in_batches(&mut ledger), needs.len());
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
