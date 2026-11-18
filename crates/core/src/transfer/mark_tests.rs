//! Tests for marking two transactions as a transfer by hand, and undoing it.

use super::tests::{Fixture, money};
use super::*;
use crate::account::AccountKind;
use crate::exclusion::{self, ExclusionReason};
use crate::link::{self, LinkKind, LinkTarget, NewLink};
use crate::split::{self, SplitLine};

// The rule ------------------------------------------------------------------

fn side(id: i64, account: i64, minor: i64) -> MarkSide {
    MarkSide {
        id: TransactionId::new(id),
        account: AccountId::new(account),
        amount: money(minor),
        group: None,
        excluded: false,
        split: false,
        linked: false,
    }
}

#[test]
fn opposite_amounts_on_two_accounts_are_markable_either_way_round() {
    assert_eq!(markable(&side(1, 1, -5000), &side(2, 2, 5000)), Ok(()));
    assert_eq!(markable(&side(2, 2, 5000), &side(1, 1, -5000)), Ok(()));
}

#[test]
fn a_rejected_pair_is_markable() {
    let mut a = side(1, 1, -5000);
    a.group = Some(TransferStatus::Rejected);
    assert_eq!(markable(&a, &side(2, 2, 5000)), Ok(()));
}

#[test]
fn the_rule_refuses_what_isnt_one_transfer() {
    let (a, b) = (side(1, 1, -5000), side(2, 2, 5000));
    assert_eq!(
        markable(&a, &side(2, 1, 5000)),
        Err(NotMarkable::SameAccount)
    );
    let mut euros = b;
    euros.amount = Money::new(5000, Currency::EUR);
    assert_eq!(markable(&a, &euros), Err(NotMarkable::Currencies));
    assert_eq!(
        markable(&a, &side(2, 2, 4999)),
        Err(NotMarkable::NotOpposite)
    );
    assert_eq!(
        markable(&a, &side(2, 2, -5000)),
        Err(NotMarkable::NotOpposite)
    );
    assert_eq!(
        markable(&side(1, 1, 0), &side(2, 2, 0)),
        Err(NotMarkable::NotOpposite)
    );
    assert_eq!(
        markable(&side(1, 1, i64::MIN), &side(2, 2, i64::MAX)),
        Err(NotMarkable::NotOpposite)
    );
}

#[test]
fn the_rule_refuses_a_row_in_a_pair_excluded_split_or_linked() {
    let a = side(1, 1, -5000);
    for status in [TransferStatus::Suggested, TransferStatus::Confirmed] {
        let mut b = side(2, 2, 5000);
        b.group = Some(status);
        assert_eq!(
            markable(&a, &b),
            Err(NotMarkable::InTransfer(TransactionId::new(2)))
        );
    }
    let mut b = side(2, 2, 5000);
    b.excluded = true;
    assert_eq!(
        markable(&a, &b),
        Err(NotMarkable::Excluded(TransactionId::new(2)))
    );
    let mut b = side(2, 2, 5000);
    b.split = true;
    assert_eq!(
        markable(&a, &b),
        Err(NotMarkable::Split(TransactionId::new(2)))
    );
    let mut b = side(2, 2, 5000);
    b.linked = true;
    assert_eq!(
        markable(&a, &b),
        Err(NotMarkable::Linked(TransactionId::new(2)))
    );
}

// Marking -------------------------------------------------------------------

/// £50 out of a current account on day 1 and into savings on day 20: too
/// far apart for detection.
fn far_apart(fx: &mut Fixture) -> (TransactionId, TransactionId) {
    let current = fx.account(AccountKind::Current);
    let savings = fx.account(AccountKind::Savings);
    let out = fx.manual(current, 1, -5000);
    let into = fx.manual(savings, 20, 5000);
    (out, into)
}

fn status(fx: &Fixture, group: TransferGroupId) -> TransferStatus {
    super::group(&fx.db, group).unwrap().status
}

fn not_markable(result: Result<Marked, Error>) -> NotMarkable {
    match result {
        Err(Error::NotMarkable(why)) => why,
        other => panic!("expected a refusal, got {other:?}"),
    }
}

#[test]
fn marking_records_a_confirmed_pair_and_unmarking_removes_it() {
    let mut fx = Fixture::new();
    let (out, into) = far_apart(&mut fx);
    let total = fx.count("SELECT SUM(amount_minor) FROM \"transaction\"");

    let marked = mark(&mut fx.db, into, out).unwrap();

    assert_eq!(marked.transactions, [out, into]);
    assert_eq!(marked.previous, Previous::New([None, None]));
    assert_eq!(status(&fx, marked.group), TransferStatus::Confirmed);
    assert_eq!(fx.group_id(out), Some(marked.group));
    assert_eq!(fx.group_id(into), Some(marked.group));
    assert_eq!(
        fx.count("SELECT SUM(amount_minor) FROM \"transaction\""),
        total
    );

    unmark(&mut fx.db, &marked).unwrap();

    assert_eq!(fx.group_id(out), None);
    assert_eq!(fx.group_id(into), None);
    assert!(matches!(
        super::group(&fx.db, marked.group),
        Err(Error::GroupNotFound(_))
    ));
    assert_eq!(fx.count("SELECT COUNT(*) FROM transfer_group"), 0);
}

#[test]
fn marking_a_rejected_pair_confirms_its_group_again() {
    let mut fx = Fixture::new();
    let current = fx.account(AccountKind::Current);
    let savings = fx.account(AccountKind::Savings);
    fx.import(savings, &[(10, 5000)]);
    let batch = fx.import(current, &[(10, -5000)]);
    let pair = fx.detect(batch, TransferWindow::DEFAULT)[0];
    reject(&mut fx.db, pair.group).unwrap();

    let marked = mark(&mut fx.db, pair.outflow, pair.inflow).unwrap();

    assert_eq!(marked.group, pair.group);
    assert_eq!(marked.previous, Previous::Rejected);
    assert_eq!(status(&fx, pair.group), TransferStatus::Confirmed);

    unmark(&mut fx.db, &marked).unwrap();

    assert_eq!(status(&fx, pair.group), TransferStatus::Rejected);
    assert_eq!(fx.group_id(pair.outflow), Some(pair.group));
    assert_eq!(fx.group_id(pair.inflow), Some(pair.group));
}

#[test]
fn a_row_leaves_its_rejected_pair_and_goes_back_on_unmark() {
    let mut fx = Fixture::new();
    let current = fx.account(AccountKind::Current);
    let savings = fx.account(AccountKind::Savings);
    let isa = fx.account(AccountKind::Savings);
    fx.import(savings, &[(10, 5000)]);
    let batch = fx.import(current, &[(10, -5000)]);
    let pair = fx.detect(batch, TransferWindow::DEFAULT)[0];
    reject(&mut fx.db, pair.group).unwrap();
    let into_isa = fx.manual(isa, 12, 5000);

    let marked = mark(&mut fx.db, pair.outflow, into_isa).unwrap();

    assert_ne!(marked.group, pair.group);
    assert_eq!(marked.previous, Previous::New([Some(pair.group), None]));
    assert_eq!(fx.group_id(pair.outflow), Some(marked.group));
    // The old partner stays in the rejected group, so it isn't suggested.
    assert_eq!(fx.group_id(pair.inflow), Some(pair.group));
    assert_eq!(status(&fx, pair.group), TransferStatus::Rejected);

    unmark(&mut fx.db, &marked).unwrap();

    assert_eq!(fx.group_id(pair.outflow), Some(pair.group));
    assert_eq!(fx.group_id(into_isa), None);
    assert_eq!(
        super::group(&fx.db, pair.group).unwrap().transactions,
        [pair.outflow.min(pair.inflow), pair.outflow.max(pair.inflow)]
    );
}

#[test]
fn marking_refuses_what_the_rule_refuses_and_writes_nothing() {
    let mut fx = Fixture::new();
    let current = fx.account(AccountKind::Current);
    let savings = fx.account(AccountKind::Savings);
    let out = fx.manual(current, 1, -5000);
    let also_current = fx.manual(current, 2, 5000);
    let less = fx.manual(savings, 2, 4000);

    assert_eq!(
        not_markable(mark(&mut fx.db, out, also_current)),
        NotMarkable::SameAccount
    );
    assert_eq!(
        not_markable(mark(&mut fx.db, out, less)),
        NotMarkable::NotOpposite
    );
    assert_eq!(fx.count("SELECT COUNT(*) FROM transfer_group"), 0);
    assert!(matches!(
        mark(&mut fx.db, out, TransactionId::new(999)),
        Err(Error::TransactionNotFound(_))
    ));
}

#[test]
fn marking_refuses_a_row_in_a_pair_excluded_split_or_linked() {
    let mut fx = Fixture::new();
    let (out, into) = far_apart(&mut fx);
    let current = fx.account(AccountKind::Current);

    fx.put_in_group(into, TransferStatus::Suggested);
    assert_eq!(
        not_markable(mark(&mut fx.db, out, into)),
        NotMarkable::InTransfer(into)
    );

    let (out, into) = far_apart(&mut fx);
    exclusion::exclude(&mut fx.db, into, ExclusionReason::Other, Some("n")).unwrap();
    assert_eq!(
        not_markable(mark(&mut fx.db, out, into)),
        NotMarkable::Excluded(into)
    );

    let (out, into) = far_apart(&mut fx);
    split::set(&mut fx.db, out, &[split_line(-2500), split_line(-2500)]).unwrap();
    assert_eq!(
        not_markable(mark(&mut fx.db, out, into)),
        NotMarkable::Split(out)
    );

    let (out, into) = far_apart(&mut fx);
    let purchase = fx.manual(current, 1, -2000);
    link::create(
        &mut fx.db,
        &NewLink {
            kind: LinkKind::Refund,
            inflow: into,
            target: LinkTarget::Purchase(purchase),
            amount: money(1000),
        },
    )
    .unwrap();
    assert_eq!(
        not_markable(mark(&mut fx.db, out, into)),
        NotMarkable::Linked(into)
    );
}

#[test]
fn unmarking_refuses_a_pair_that_changed_since() {
    let mut fx = Fixture::new();
    let (out, into) = far_apart(&mut fx);
    let marked = mark(&mut fx.db, out, into).unwrap();
    let group = marked.group;
    unpair(&mut fx.db, group).unwrap();

    assert!(matches!(
        unmark(&mut fx.db, &marked),
        Err(Error::Changed(g)) if g == group
    ));
    assert_eq!(status(&fx, group), TransferStatus::Rejected);
    assert_eq!(fx.group_id(out), Some(group));

    // Unpaired, the two can be marked again: the rejected pair's group.
    let again = mark(&mut fx.db, out, into).unwrap();
    assert_eq!(again.group, group);
    assert_eq!(again.previous, Previous::Rejected);
}

#[test]
fn a_marked_pair_is_left_out_of_detection() {
    let mut fx = Fixture::new();
    let current = fx.account(AccountKind::Current);
    let savings = fx.account(AccountKind::Savings);
    let out = fx.manual(current, 1, -5000);
    let into = fx.manual(savings, 1, 5000);
    let marked = mark(&mut fx.db, out, into).unwrap();
    let other = fx.manual(savings, 2, 5000);

    let found = detect(fx.db.conn(), &[out, into, other], TransferWindow::DEFAULT).unwrap();

    assert!(found.is_empty());
    assert_eq!(status(&fx, marked.group), TransferStatus::Confirmed);
}

fn split_line(minor: i64) -> SplitLine {
    SplitLine {
        amount: money(minor),
        category: None,
        my_share: None,
        share_label: None,
        user_note: None,
    }
}
