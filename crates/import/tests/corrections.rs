//! The bank's corrections end to end (ROADMAP R.4, L.21): the next export
//! of an account repeats source IDs with a tip added, a fuel hold settled
//! and a pending row posted on a later day. The import supersedes the
//! stored rows, carries the user's fields across, reconciles its balance
//! against the corrected rows, and undoing it restores the stored rows
//! exactly.
//!
//! `fixtures/ofx/corrections-first.ofx` and `corrections-next.ofx` are
//! synthetic.

mod common;

use chrono::NaiveDate;
use common::{Scratch, outcome_kinds, read_ofx};
use orre_core::account::{self, EnteredBalance, EnteredSource};
use orre_core::exclusion::{self, ExclusionReason};
use orre_core::id::TransactionId;
use orre_core::import::{
    self, CheckedBalance, Committed, Confirmations, Error, ReconciliationStatus, RowOutcome,
};
use orre_core::money::{Currency, Money};
use orre_core::note;

const FIRST: &str = "corrections-first.ofx";
const NEXT: &str = "corrections-next.ofx";

fn kinds(rows: &[(u64, &str)]) -> Vec<(u64, String)> {
    rows.iter()
        .map(|(n, kind)| (*n, (*kind).to_owned()))
        .collect()
}

fn gbp(minor: i64) -> Money {
    Money::new(minor, Currency::GBP)
}

fn live(scratch: &Scratch, account: orre_core::id::AccountId, id: &str) -> TransactionId {
    TransactionId::new(scratch.by_external_id(account, id).unwrap().id)
}

#[test]
fn the_next_export_corrects_a_tip_a_fuel_hold_and_a_posting_date() {
    let mut scratch = Scratch::new();
    let account = scratch.current("Current", Currency::GBP);
    // 1000.00 at the end of 4 November, before either file's rows.
    account::add_balance(
        &mut scratch.db,
        account,
        &EnteredBalance {
            date: NaiveDate::from_ymd_opt(2026, 11, 4).unwrap(),
            balance: gbp(100_000),
            source: EnteredSource::Manual,
            note: None,
        },
    )
    .unwrap();
    let first = scratch.import_batch(account, FIRST, read_ofx(FIRST));
    assert_eq!(first.transactions, 4);
    let bistro = live(&scratch, account, "CORR-0001");
    note::set(&mut scratch.db, bistro, "Team lunch").unwrap();

    let preview = scratch.preview(account, NEXT, read_ofx(NEXT));
    assert_eq!(
        outcome_kinds(&preview),
        kinds(&[
            (1, "corrects"),
            (2, "corrects"),
            (3, "corrects"),
            (4, "already:source_id"),
            (5, "new"),
        ])
    );
    let corrections: Vec<_> = preview
        .corrections()
        .map(|(_, c)| (c.was_date.to_string(), c.was_amount))
        .collect();
    assert_eq!(
        corrections,
        [
            ("2026-11-05".to_owned(), gbp(-4_000)),
            ("2026-11-06".to_owned(), gbp(-100)),
            ("2026-11-07".to_owned(), gbp(-1_250)),
        ]
    );
    assert_eq!(preview.counts().corrections, 3);
    // The file's balance checks against the corrected rows: counting the
    // stored versions instead would leave it 54.67 out.
    assert_eq!(
        preview
            .reconciliation()
            .checks
            .iter()
            .map(|check| (check.balance, check.after.is_failed()))
            .collect::<Vec<_>>(),
        [(CheckedBalance::FromFile(0), false)]
    );
    assert_eq!(
        preview.reconciliation().status(),
        ReconciliationStatus::Reconciled
    );
    assert_eq!(
        preview.needs_confirmation(),
        Confirmations {
            flagged_rows: false,
            balance_mismatch: false,
            corrections: true,
        }
    );
    assert!(matches!(
        import::commit(&mut scratch.db, &preview, Confirmations::default()),
        Err(Error::NotConfirmed(needed)) if needed.corrections
    ));

    let Committed::Batch(next) =
        import::commit(&mut scratch.db, &preview, preview.needs_confirmation()).unwrap()
    else {
        panic!("expected a batch");
    };
    assert_eq!((next.transactions, next.flags), (4, 0));
    assert_eq!(next.corrections.len(), 3);
    assert!(next.corrections.iter().all(|c| c.not_carried.is_empty()));

    // The stored rows keep their source fields, excluded as superseded.
    let replaced = next.corrections[0].replaced;
    assert_eq!(replaced, bistro);
    let exclusion = exclusion::get(&scratch.db, bistro).unwrap().unwrap();
    assert_eq!(exclusion.reason, ExclusionReason::Superseded);
    assert_eq!(exclusion.superseded_by, Some(next.corrections[0].by));
    let stored: (i64, Option<String>) = scratch
        .reader()
        .query_row(
            "SELECT amount_minor, user_note FROM \"transaction\" WHERE id = ?1",
            [bistro.get()],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .unwrap();
    assert_eq!(stored, (-4_000, Some("Team lunch".to_owned())));
    // The new rows: the bank's values, the user's note.
    let tip = scratch.by_external_id(account, "CORR-0001").unwrap();
    assert_eq!(tip.amount_minor, -4_600);
    let note: Option<String> = scratch
        .reader()
        .query_row(
            "SELECT user_note FROM \"transaction\" WHERE id = ?1",
            [tip.id],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(note.as_deref(), Some("Team lunch"));
    let posted = scratch.by_external_id(account, "CORR-0003").unwrap();
    assert_eq!(posted.date, "2026-11-09");
    // Every version is known: both files are already imported.
    assert!(
        scratch
            .preview(account, NEXT, read_ofx(NEXT))
            .is_nothing_new()
    );
    let first_again = scratch.preview(account, FIRST, read_ofx(FIRST));
    assert!(first_again.is_nothing_new());
    assert!(
        first_again
            .rows()
            .all(|row| matches!(row.outcome, RowOutcome::AlreadyImported { .. }))
    );

    // Undo restores the stored rows exactly as they were.
    let undo = import::undo_preview(&scratch.db, next.batch).unwrap();
    assert_eq!(undo.restored.len(), 3);
    assert!(undo.restored.contains(&bistro));
    import::undo(&mut scratch.db, next.batch).unwrap();
    assert_eq!(exclusion::get(&scratch.db, bistro).unwrap(), None);
    assert_eq!(live(&scratch, account, "CORR-0001"), bistro);
    assert_eq!(scratch.stored(account).len(), 4);
}
