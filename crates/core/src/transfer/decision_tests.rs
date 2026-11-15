//! Tests for confirming, rejecting, unpairing and undoing transfer pairs
//! against a real database.

use chrono::NaiveTime;
use rusqlite::params;

use super::tests::{Fixture, day, money};
use super::*;
use crate::account::AccountKind;
use crate::category::CategoryKey;
use crate::exclusion::{self, ExclusionReason};
use crate::import::{self, Committed};
use crate::link::{self, LinkKind, LinkTarget, NewLink};
use crate::split::{self, SplitLine};
use crate::statement::{
    SourceFormat, SourceLocation, Statement, StatementLine, SubAccountKind, SubAccountRef,
};

/// A suggested pair: £50 out of a current account, £50 into savings, the
/// same day. Returns the pair and the two accounts.
struct Pair {
    pair: SuggestedPair,
    current: AccountId,
    savings: AccountId,
}

fn suggested(fx: &mut Fixture) -> Pair {
    let current = fx.account(AccountKind::Current);
    let savings = fx.account(AccountKind::Savings);
    fx.import(savings, &[(10, 5000)]);
    let batch = fx.import(current, &[(10, -5000)]);
    let pairs = fx.detect(batch, TransferWindow::DEFAULT);
    assert_eq!(pairs.len(), 1);
    Pair {
        pair: pairs[0],
        current,
        savings,
    }
}

fn status(fx: &Fixture, group: TransferGroupId) -> TransferStatus {
    super::group(&fx.db, group).unwrap().status
}

fn blocked(result: Result<Decision, Error>) -> Blocked {
    match result {
        Err(Error::Blocked(blocked)) => blocked,
        other => panic!("expected a refusal, got {other:?}"),
    }
}

// Confirming ----------------------------------------------------------------

#[test]
fn confirming_a_suggested_pair_changes_only_its_status() {
    let mut fx = Fixture::new();
    let p = suggested(&mut fx);
    let before = fx.count("SELECT COUNT(*) FROM \"transaction\" WHERE category_id IS NOT NULL");
    let total = fx.count("SELECT SUM(ABS(amount_minor)) FROM \"transaction\"");

    let decision = confirm(&mut fx.db, p.pair.group).unwrap();

    assert_eq!(
        decision,
        Decision {
            group: p.pair.group,
            action: Action::Confirm,
            transactions: [
                p.pair.outflow.min(p.pair.inflow),
                p.pair.outflow.max(p.pair.inflow)
            ],
        }
    );
    assert_eq!(status(&fx, p.pair.group), TransferStatus::Confirmed);
    assert_eq!(
        fx.count("SELECT COUNT(*) FROM \"transaction\" WHERE category_id IS NOT NULL"),
        before
    );
    assert_eq!(
        fx.count("SELECT SUM(ABS(amount_minor)) FROM \"transaction\""),
        total
    );
    // Both rows leave income and spending.
    assert_eq!(
        fx.count(
            "SELECT COUNT(*) FROM ledger_line WHERE transfer_status = 'confirmed' \
             AND transfer_group_id = (SELECT transfer_group_id FROM \"transaction\" LIMIT 1)"
        ),
        2
    );
}

#[test]
fn confirming_a_pair_with_a_manual_row_works() {
    let mut fx = Fixture::new();
    let current = fx.account(AccountKind::Current);
    let savings = fx.account(AccountKind::Savings);
    let batch = fx.import(current, &[(10, -5000)]);
    let arrived = fx.manual(savings, 11, 5000);
    let pair = fx.detect(batch, TransferWindow::DEFAULT)[0];
    assert_eq!(pair.inflow, arrived);

    confirm(&mut fx.db, pair.group).unwrap();

    assert_eq!(status(&fx, pair.group), TransferStatus::Confirmed);
}

#[test]
fn confirming_refuses_an_unknown_group() {
    let mut fx = Fixture::new();
    assert!(matches!(
        confirm(&mut fx.db, TransferGroupId::new(99)),
        Err(Error::GroupNotFound(_))
    ));
}

#[test]
fn confirming_refuses_an_excluded_row() {
    let mut fx = Fixture::new();
    let p = suggested(&mut fx);
    exclusion::exclude(&mut fx.db, p.pair.inflow, ExclusionReason::Other, Some("n")).unwrap();

    assert_eq!(
        blocked(confirm(&mut fx.db, p.pair.group)),
        Blocked::Excluded(p.pair.inflow)
    );
    assert_eq!(status(&fx, p.pair.group), TransferStatus::Suggested);
    // Rejecting it is still allowed.
    reject(&mut fx.db, p.pair.group).unwrap();
}

#[test]
fn confirming_refuses_a_linked_row() {
    let mut fx = Fixture::new();
    let p = suggested(&mut fx);
    // A link can't be made while the pair is suggested, so link a rejected
    // pair's rows and make it suggested again behind core's back.
    reject(&mut fx.db, p.pair.group).unwrap();
    link_refund(&mut fx, p.pair.inflow, p.pair.outflow);
    fx.db
        .conn()
        .execute(
            "UPDATE transfer_group SET status = 'suggested' WHERE id = ?1",
            [p.pair.group],
        )
        .unwrap();

    let refused = blocked(confirm(&mut fx.db, p.pair.group));

    assert!(matches!(refused, Blocked::Linked(id) if id == p.pair.outflow || id == p.pair.inflow));
    assert_eq!(status(&fx, p.pair.group), TransferStatus::Suggested);
}

#[test]
fn confirming_refuses_a_split_row() {
    let mut fx = Fixture::new();
    let p = suggested(&mut fx);
    reject(&mut fx.db, p.pair.group).unwrap();
    split_in_two(&mut fx, p.pair.outflow);
    // The schema refuses to suggest a pair holding a split row; take the
    // trigger away to reach core's own check.
    fx.db
        .conn()
        .execute_batch(
            "DROP TRIGGER transfer_group_split; \
             UPDATE transfer_group SET status = 'suggested';",
        )
        .unwrap();

    assert_eq!(
        blocked(confirm(&mut fx.db, p.pair.group)),
        Blocked::Split(p.pair.outflow)
    );
}

#[test]
fn a_group_of_three_is_not_a_pair() {
    let mut fx = Fixture::new();
    let p = suggested(&mut fx);
    let extra = fx.manual(p.savings, 10, 1);
    fx.db
        .conn()
        .execute(
            "UPDATE \"transaction\" SET transfer_group_id = ?1 WHERE id = ?2",
            params![p.pair.group, extra],
        )
        .unwrap();

    assert_eq!(
        blocked(confirm(&mut fx.db, p.pair.group)),
        Blocked::NotTwoSided
    );
    assert_eq!(
        blocked(reject(&mut fx.db, p.pair.group)),
        Blocked::NotTwoSided
    );
}

// Rejecting and unpairing ---------------------------------------------------

#[test]
fn a_rejected_pair_keeps_its_rows_and_is_not_suggested_again() {
    let mut fx = Fixture::new();
    let p = suggested(&mut fx);

    let decision = reject(&mut fx.db, p.pair.group).unwrap();

    assert_eq!(decision.action, Action::Reject);
    let group = super::group(&fx.db, p.pair.group).unwrap();
    assert_eq!(group.status, TransferStatus::Rejected);
    assert_eq!(group.transactions.len(), 2);
    assert!(
        detect(
            fx.db.conn(),
            &[p.pair.outflow, p.pair.inflow],
            TransferWindow::DEFAULT
        )
        .unwrap()
        .is_empty()
    );
    // A later import in the window doesn't pair the rows either.
    let again = fx.import(p.current, &[(11, -5000)]);
    assert!(fx.detect_ids(again).is_empty());
}

#[test]
fn unpairing_a_confirmed_pair_rejects_it() {
    let mut fx = Fixture::new();
    let p = suggested(&mut fx);
    confirm(&mut fx.db, p.pair.group).unwrap();

    let decision = unpair(&mut fx.db, p.pair.group).unwrap();

    assert_eq!(decision.action, Action::Unpair);
    assert_eq!(status(&fx, p.pair.group), TransferStatus::Rejected);
    assert!(
        detect(
            fx.db.conn(),
            &[p.pair.outflow, p.pair.inflow],
            TransferWindow::DEFAULT
        )
        .unwrap()
        .is_empty()
    );
    // The rows are free to be split and linked again.
    split_in_two(&mut fx, p.pair.outflow);
}

#[test]
fn each_action_refuses_the_wrong_status() {
    let mut fx = Fixture::new();
    let p = suggested(&mut fx);
    let group = p.pair.group;
    let wrong = |found| Blocked::WrongStatus { found };

    assert_eq!(
        blocked(unpair(&mut fx.db, group)),
        wrong(TransferStatus::Suggested)
    );

    confirm(&mut fx.db, group).unwrap();
    assert_eq!(
        blocked(confirm(&mut fx.db, group)),
        wrong(TransferStatus::Confirmed)
    );
    assert_eq!(
        blocked(reject(&mut fx.db, group)),
        wrong(TransferStatus::Confirmed)
    );

    unpair(&mut fx.db, group).unwrap();
    assert_eq!(
        blocked(confirm(&mut fx.db, group)),
        wrong(TransferStatus::Rejected)
    );
    assert_eq!(
        blocked(reject(&mut fx.db, group)),
        wrong(TransferStatus::Rejected)
    );
    assert_eq!(
        blocked(unpair(&mut fx.db, group)),
        wrong(TransferStatus::Rejected)
    );
}

#[test]
fn a_stale_decision_changes_nothing() {
    let mut fx = Fixture::new();
    let p = suggested(&mut fx);
    // Two screens show the same suggestion; one rejects it.
    reject(&mut fx.db, p.pair.group).unwrap();

    assert!(matches!(
        confirm(&mut fx.db, p.pair.group),
        Err(Error::Blocked(Blocked::WrongStatus { .. }))
    ));
    assert_eq!(status(&fx, p.pair.group), TransferStatus::Rejected);
}

#[test]
fn a_decision_inside_a_callers_transaction_rolls_back_with_it() {
    let mut fx = Fixture::new();
    let p = suggested(&mut fx);
    {
        let tx = fx.db.conn_mut().transaction().unwrap();
        confirm_in(&tx, p.pair.group).unwrap();
        drop(tx);
    }
    assert_eq!(status(&fx, p.pair.group), TransferStatus::Suggested);
}

// Pot pairs -----------------------------------------------------------------

/// Imports a pot transfer into `parent`; returns its confirmed group.
fn pot_transfer(fx: &mut Fixture, parent: AccountId) -> TransferGroupId {
    let mut line = StatementLine::new(day(10), money(-2500), "pot_FAKE00000000001");
    line.time = Some(NaiveTime::from_hms_opt(9, 0, 1).unwrap());
    line.external_id = Some("tx_0001".into());
    line.source_type = Some("Pot transfer".into());
    line.category = Some(CategoryKey::new("transfer.savings").unwrap());
    line.sub_account = Some(SubAccountRef {
        kind: SubAccountKind::Pot,
        key: "pot_FAKE00000000001".into(),
        name: Some("Rainy day".into()),
    });
    let mut statement = Statement::new(SourceFormat::Csv, Currency::GBP).with_preset("uk-monzo");
    statement.push(SourceLocation::Line(2), Ok(line));
    let preview = import::preview(&fx.db, parent, "monzo.csv", statement).unwrap();
    let confirm = preview.needs_confirmation();
    let Committed::Batch(_) = import::commit(&mut fx.db, &preview, confirm).unwrap() else {
        panic!("expected a new batch");
    };
    fx.db
        .conn()
        .query_row(
            "SELECT transfer_group_id FROM \"transaction\" WHERE origin = 'mirrored'",
            [],
            |row| row.get(0),
        )
        .unwrap()
}

#[test]
fn a_pots_pair_is_confirmed_and_cannot_be_unpaired() {
    let mut fx = Fixture::new();
    let current = fx.account(AccountKind::Current);
    let group = pot_transfer(&mut fx, current);
    assert_eq!(status(&fx, group), TransferStatus::Confirmed);

    assert_eq!(blocked(unpair(&mut fx.db, group)), Blocked::PotPair);
    assert_eq!(
        blocked(reject(&mut fx.db, group)),
        Blocked::WrongStatus {
            found: TransferStatus::Confirmed
        }
    );
    assert_eq!(status(&fx, group), TransferStatus::Confirmed);

    let mirrored: TransactionId = fx
        .db
        .conn()
        .query_row(
            "SELECT id FROM \"transaction\" WHERE origin = 'mirrored'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    let view = pair_of(&fx.db, mirrored).unwrap().unwrap();
    assert_eq!(view.unpair, Err(Blocked::PotPair));
    // The parent's row shows the same.
    let parent_row = view.partners[0].transaction;
    let parent_view = pair_of(&fx.db, parent_row).unwrap().unwrap();
    assert_eq!(parent_view.unpair, Err(Blocked::PotPair));
}

#[test]
fn a_pot_and_its_parent_paired_by_detection_can_be_decided_like_any_pair() {
    let mut fx = Fixture::new();
    let current = fx.account(AccountKind::Current);
    let pot = fx.pot(current);
    fx.import(pot, &[(10, 5000)]);
    let batch = fx.import(current, &[(10, -5000)]);
    let pair = fx.detect(batch, TransferWindow::DEFAULT)[0];

    confirm(&mut fx.db, pair.group).unwrap();
    unpair(&mut fx.db, pair.group).unwrap();

    assert_eq!(status(&fx, pair.group), TransferStatus::Rejected);
}

// Undo ----------------------------------------------------------------------

#[test]
fn undo_restores_the_status_each_decision_started_from() {
    let mut fx = Fixture::new();
    let p = suggested(&mut fx);
    let group = p.pair.group;

    let confirmed = confirm(&mut fx.db, group).unwrap();
    undo(&mut fx.db, &confirmed).unwrap();
    assert_eq!(status(&fx, group), TransferStatus::Suggested);

    let rejected = reject(&mut fx.db, group).unwrap();
    undo(&mut fx.db, &rejected).unwrap();
    assert_eq!(status(&fx, group), TransferStatus::Suggested);

    confirm(&mut fx.db, group).unwrap();
    let unpaired = unpair(&mut fx.db, group).unwrap();
    undo(&mut fx.db, &unpaired).unwrap();
    assert_eq!(status(&fx, group), TransferStatus::Confirmed);
}

#[test]
fn undo_refuses_when_the_pair_has_moved_on() {
    let mut fx = Fixture::new();
    let p = suggested(&mut fx);
    let group = p.pair.group;
    let confirmed = confirm(&mut fx.db, group).unwrap();
    unpair(&mut fx.db, group).unwrap();

    assert!(matches!(
        undo(&mut fx.db, &confirmed),
        Err(Error::Changed(g)) if g == group
    ));
    assert_eq!(status(&fx, group), TransferStatus::Rejected);

    // Undoing twice is refused the second time.
    let mut fx = Fixture::new();
    let p = suggested(&mut fx);
    let decision = reject(&mut fx.db, p.pair.group).unwrap();
    undo(&mut fx.db, &decision).unwrap();
    assert!(matches!(
        undo(&mut fx.db, &decision),
        Err(Error::Changed(_))
    ));
}

#[test]
fn undo_refuses_when_the_pair_was_dissolved_or_its_rows_changed() {
    let mut fx = Fixture::new();
    let p = suggested(&mut fx);
    let decision = confirm(&mut fx.db, p.pair.group).unwrap();
    // Undoing the import that made one side dissolves the group.
    let batch: BatchId = fx
        .db
        .conn()
        .query_row(
            "SELECT batch_id FROM \"transaction\" WHERE id = ?1",
            [p.pair.outflow],
            |row| row.get(0),
        )
        .unwrap();
    import::undo(&mut fx.db, batch).unwrap();
    assert!(matches!(
        undo(&mut fx.db, &decision),
        Err(Error::Changed(_))
    ));

    // A third row in the group isn't the pair that was decided.
    let mut fx = Fixture::new();
    let p = suggested(&mut fx);
    let decision = confirm(&mut fx.db, p.pair.group).unwrap();
    let extra = fx.manual(p.savings, 10, 1);
    fx.db
        .conn()
        .execute(
            "UPDATE \"transaction\" SET transfer_group_id = ?1 WHERE id = ?2",
            params![p.pair.group, extra],
        )
        .unwrap();
    assert!(matches!(
        undo(&mut fx.db, &decision),
        Err(Error::Changed(_))
    ));
}

#[test]
fn undoing_a_rejection_refuses_a_row_split_or_linked_since() {
    let mut fx = Fixture::new();
    let p = suggested(&mut fx);
    let decision = reject(&mut fx.db, p.pair.group).unwrap();
    split_in_two(&mut fx, p.pair.outflow);
    assert!(matches!(
        undo(&mut fx.db, &decision),
        Err(Error::Blocked(Blocked::Split(id))) if id == p.pair.outflow
    ));
    assert_eq!(status(&fx, p.pair.group), TransferStatus::Rejected);

    let mut fx = Fixture::new();
    let p = suggested(&mut fx);
    let decision = reject(&mut fx.db, p.pair.group).unwrap();
    link_refund(&mut fx, p.pair.inflow, p.pair.outflow);
    assert!(matches!(
        undo(&mut fx.db, &decision),
        Err(Error::Blocked(Blocked::Linked(_)))
    ));
    assert_eq!(status(&fx, p.pair.group), TransferStatus::Rejected);
}

#[test]
fn undoing_an_unpair_refuses_a_row_excluded_since_but_undoing_a_confirm_does_not() {
    let mut fx = Fixture::new();
    let p = suggested(&mut fx);
    confirm(&mut fx.db, p.pair.group).unwrap();
    let unpaired = unpair(&mut fx.db, p.pair.group).unwrap();
    exclusion::exclude(
        &mut fx.db,
        p.pair.outflow,
        ExclusionReason::Other,
        Some("n"),
    )
    .unwrap();
    assert!(matches!(
        undo(&mut fx.db, &unpaired),
        Err(Error::Blocked(Blocked::Excluded(id))) if id == p.pair.outflow
    ));

    let mut fx = Fixture::new();
    let p = suggested(&mut fx);
    let confirmed = confirm(&mut fx.db, p.pair.group).unwrap();
    exclusion::exclude(
        &mut fx.db,
        p.pair.outflow,
        ExclusionReason::Other,
        Some("n"),
    )
    .unwrap();
    undo(&mut fx.db, &confirmed).unwrap();
    assert_eq!(status(&fx, p.pair.group), TransferStatus::Suggested);
}

// The read model ------------------------------------------------------------

#[test]
fn a_transaction_outside_a_group_has_no_pair() {
    let mut fx = Fixture::new();
    let current = fx.account(AccountKind::Current);
    let row = fx.manual(current, 10, -100);
    assert_eq!(pair_of(&fx.db, row).unwrap(), None);
    assert!(matches!(
        pair_of(&fx.db, TransactionId::new(99)),
        Err(Error::TransactionNotFound(_))
    ));
}

#[test]
fn the_pair_view_names_the_other_side_and_what_is_allowed() {
    let mut fx = Fixture::new();
    let p = suggested(&mut fx);

    let from_savings = pair_of(&fx.db, p.pair.inflow).unwrap().unwrap();
    assert_eq!(from_savings.group, p.pair.group);
    assert_eq!(from_savings.status, TransferStatus::Suggested);
    assert_eq!(
        from_savings.partners,
        vec![Partner {
            transaction: p.pair.outflow,
            account: p.current,
            account_name: "Account".into(),
            date: day(10),
            amount: money(-5000),
        }]
    );
    assert_eq!(from_savings.allows(Action::Confirm), Ok(()));
    assert_eq!(from_savings.allows(Action::Reject), Ok(()));
    assert_eq!(
        from_savings.allows(Action::Unpair),
        Err(Blocked::WrongStatus {
            found: TransferStatus::Suggested
        })
    );
    let from_current = pair_of(&fx.db, p.pair.outflow).unwrap().unwrap();
    assert_eq!(from_current.partners[0].account, p.savings);
    assert_eq!(from_current.partners[0].amount, money(5000));

    confirm(&mut fx.db, p.pair.group).unwrap();
    let confirmed = pair_of(&fx.db, p.pair.inflow).unwrap().unwrap();
    assert_eq!(confirmed.status, TransferStatus::Confirmed);
    assert_eq!(confirmed.allows(Action::Unpair), Ok(()));
    assert!(confirmed.allows(Action::Confirm).is_err());

    unpair(&mut fx.db, p.pair.group).unwrap();
    let rejected = pair_of(&fx.db, p.pair.inflow).unwrap().unwrap();
    assert_eq!(rejected.status, TransferStatus::Rejected);
    for action in [Action::Confirm, Action::Reject, Action::Unpair] {
        assert!(rejected.allows(action).is_err());
    }
}

#[test]
fn the_pair_view_says_why_confirming_is_blocked() {
    let mut fx = Fixture::new();
    let p = suggested(&mut fx);
    exclusion::exclude(
        &mut fx.db,
        p.pair.outflow,
        ExclusionReason::Other,
        Some("n"),
    )
    .unwrap();

    let view = pair_of(&fx.db, p.pair.inflow).unwrap().unwrap();

    assert_eq!(view.confirm, Err(Blocked::Excluded(p.pair.outflow)));
    assert_eq!(view.reject, Ok(()));
}

// Helpers -------------------------------------------------------------------

/// Splits `transaction` (money out) into two lines.
fn split_in_two(fx: &mut Fixture, transaction: TransactionId) {
    let amount: i64 = fx
        .db
        .conn()
        .query_row(
            "SELECT amount_minor FROM \"transaction\" WHERE id = ?1",
            [transaction],
            |row| row.get(0),
        )
        .unwrap();
    let line = |minor| SplitLine {
        amount: money(minor),
        category: None,
        my_share: None,
        share_label: None,
        user_note: None,
    };
    split::set(
        &mut fx.db,
        transaction,
        &[line(amount / 2), line(amount - amount / 2)],
    )
    .unwrap();
}

/// A refund link of part of `purchase` by `inflow`.
fn link_refund(fx: &mut Fixture, inflow: TransactionId, purchase: TransactionId) {
    link::create(
        &mut fx.db,
        &NewLink {
            kind: LinkKind::Refund,
            inflow,
            target: LinkTarget::Purchase(purchase),
            amount: money(1000),
        },
    )
    .unwrap();
}
