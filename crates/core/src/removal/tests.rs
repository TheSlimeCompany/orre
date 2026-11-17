//! Tests for removing transactions against a real database.

use chrono::NaiveDate;
use rusqlite::params;
use tempfile::TempDir;

use super::*;
use crate::account::{self, AccountKind, NewAccount};
use crate::db::Database;
use crate::id::AccountId;
use crate::import::{self, Committed};
use crate::money::{Currency, Money};
use crate::statement::{SourceFormat, SourceLocation, Statement, StatementLine};
use crate::transfer;

fn day(d: u32) -> NaiveDate {
    NaiveDate::from_ymd_opt(2026, 11, d).unwrap()
}

fn gbp(minor: i64) -> Money {
    Money::new(minor, Currency::GBP)
}

/// Imports `rows` (day, minor units) into `account` as one batch and returns
/// the batch with its transactions in row order.
fn import_rows(
    db: &mut Database,
    account: AccountId,
    rows: &[(u32, i64)],
) -> (BatchId, Vec<TransactionId>) {
    let mut statement = Statement::new(SourceFormat::Csv, Currency::GBP);
    for (index, (d, minor)) in rows.iter().enumerate() {
        let text = format!("Payment {index}");
        statement.push(
            SourceLocation::Line(index as u64 + 2),
            Ok(StatementLine::new(day(*d), gbp(*minor), text)),
        );
    }
    let preview = import::preview(db, account, "statement.csv", statement).unwrap();
    let confirm = preview.needs_confirmation();
    let Committed::Batch(summary) = import::commit(db, &preview, confirm).unwrap() else {
        panic!("expected a batch");
    };
    let mut statement = db
        .conn()
        .prepare("SELECT id FROM \"transaction\" WHERE batch_id = ?1 ORDER BY date, id")
        .unwrap();
    let ids = statement
        .query_map([summary.batch], |row| row.get(0))
        .unwrap()
        .collect::<Result<_, _>>()
        .unwrap();
    (summary.batch, ids)
}

/// A database where every table that references a transaction holds a row
/// that references one of the transactions of `batch`.
struct Populated {
    db: Database,
    _dir: TempDir,
    /// The import whose rows are removed.
    batch: BatchId,
    /// Its rows.
    removed: Vec<TransactionId>,
    /// Rows of another import that those rows are tied to.
    kept: Vec<TransactionId>,
    /// The transfer group of the removed row and a kept partner.
    group: TransferGroupId,
    /// The partner of the removed transfer row.
    partner: TransactionId,
}

fn populated() -> Populated {
    let dir = TempDir::new().unwrap();
    let mut db = Database::open(&dir.path().join("coffer.db")).unwrap();
    let current = account::create(
        &mut db,
        &NewAccount::new("Current", AccountKind::Current, Currency::GBP),
    )
    .unwrap();
    let savings = account::create(
        &mut db,
        &NewAccount::new("Savings", AccountKind::Savings, Currency::GBP),
    )
    .unwrap();

    // Rows of the removed batch: two purchases, a transfer out, and money in.
    let (batch, removed) = import_rows(
        &mut db,
        current,
        &[(1, -1_000), (2, -200), (3, -2_000), (4, 300)],
    );
    // Rows of another import: the transfer's other half, money in that pays
    // back a removed purchase, and a purchase that money in of the removed
    // batch pays back.
    let (_, mut kept) = import_rows(&mut db, savings, &[(3, 2_000), (5, 500), (6, -350)]);
    let [purchase, other, transfer_out, inflow] = removed[..] else {
        panic!("four rows")
    };
    // A row on the removed rows' account that a candidate pairs one with.
    let (_, extra_rows) = import_rows(&mut db, current, &[(7, -50)]);
    kept.extend(extra_rows);
    let [partner, refund, bought, extra] = kept[..] else {
        panic!("four rows")
    };

    let conn = db.conn();
    // The second import paired the transfer with its other half itself.
    let group: TransferGroupId = conn
        .query_row(
            "SELECT transfer_group_id FROM \"transaction\" WHERE id = ?1",
            [transfer_out],
            |row| row.get(0),
        )
        .unwrap();

    conn.execute_batch(
        "INSERT INTO tag (name) VALUES ('holiday');
         INSERT INTO change_batch (kind) VALUES ('bulk_edit');
         INSERT INTO schedule (account_id, merchant_display, cadence, start_date, amount_minor, \
             currency, tolerance_days) \
             VALUES (1, 'Payment', 'monthly', '2026-11-01', -200, 'GBP', 3);",
    )
    .unwrap();
    conn.execute(
        "INSERT INTO transaction_tag (transaction_id, tag_id) VALUES (?1, 1)",
        [purchase],
    )
    .unwrap();
    conn.execute(
        "INSERT INTO change_item (batch_id, transaction_id, field, old_value, new_value) \
         VALUES (1, ?1, 'user_note', '\"a\"', '\"b\"')",
        [purchase],
    )
    .unwrap();
    conn.execute(
        "INSERT INTO schedule_match (schedule_id, occurrence_date, transaction_id, matched_by) \
         VALUES (1, '2026-11-02', ?1, 'auto')",
        [other],
    )
    .unwrap();
    // Two split lines on the purchase.
    conn.execute(
        "INSERT INTO transaction_split (transaction_id, line_no, amount_minor) \
         VALUES (?1, 1, -600), (?1, 2, -400)",
        [purchase],
    )
    .unwrap();
    // A kept refund pays back a removed purchase; a removed inflow pays back
    // a kept one.
    conn.execute(
        "INSERT INTO transaction_link (kind, inflow_id, target_id, amount_minor, status) \
         VALUES ('refund', ?1, ?2, 100, 'confirmed'), ('refund', ?3, ?4, 100, 'confirmed')",
        params![refund, other, inflow, bought],
    )
    .unwrap();
    // A candidate between a removed row and a kept one on the same account, and one between two
    // removed rows.
    conn.execute(
        "INSERT INTO duplicate_candidate (transaction_a, transaction_b, reason) \
         VALUES (?1, ?2, 'overlapping_import'), (?3, ?4, 'overlapping_import')",
        params![purchase, extra, other, inflow],
    )
    .unwrap();

    Populated {
        db,
        _dir: dir,
        batch,
        removed,
        kept,
        group,
        partner,
    }
}

/// How many rows of each referencing table point at a transaction in
/// `ids`.
fn referencing(db: &Database, ids: &[TransactionId]) -> Vec<(String, i64)> {
    let list = ids
        .iter()
        .map(|id| id.to_string())
        .collect::<Vec<_>>()
        .join(", ");
    REFERENCES
        .iter()
        .map(|(table, column)| {
            let count = db
                .conn()
                .query_row(
                    &format!("SELECT COUNT(*) FROM {table} WHERE {column} IN ({list})"),
                    [],
                    |row| row.get(0),
                )
                .unwrap();
            (format!("{table}.{column}"), count)
        })
        .collect()
}

fn count(db: &Database, sql: &str) -> i64 {
    db.conn().query_row(sql, [], |row| row.get(0)).unwrap()
}

/// Checks that the removed rows are gone with everything that referred to
/// them, and the kept rows are still there, now unpaired.
fn assert_removed(p: &Populated) {
    assert_eq!(
        referencing(&p.db, &p.removed)
            .into_iter()
            .filter(|(_, n)| *n > 0)
            .collect::<Vec<_>>(),
        vec![],
    );
    assert_eq!(
        p.db.conn()
            .query_row(
                "SELECT COUNT(*) FROM \"transaction\" WHERE batch_id = ?1",
                [p.batch],
                |row| row.get::<_, i64>(0),
            )
            .unwrap(),
        0
    );
    assert_eq!(
        count(&p.db, "SELECT COUNT(*) FROM \"transaction\""),
        p.kept.len() as i64
    );
    for id in &p.kept {
        let transfer_group: Option<TransferGroupId> =
            p.db.conn()
                .query_row(
                    "SELECT transfer_group_id FROM \"transaction\" WHERE id = ?1",
                    [id],
                    |row| row.get(0),
                )
                .unwrap();
        assert_eq!(transfer_group, None);
    }
    assert_eq!(count(&p.db, "SELECT COUNT(*) FROM transfer_group"), 0);
    assert_eq!(count(&p.db, "SELECT COUNT(*) FROM transaction_link"), 0);
    // What isn't tied to a transaction stays.
    assert_eq!(count(&p.db, "SELECT COUNT(*) FROM tag"), 1);
    assert_eq!(count(&p.db, "SELECT COUNT(*) FROM change_batch"), 1);
    assert_eq!(count(&p.db, "SELECT COUNT(*) FROM schedule"), 1);
}

#[test]
fn the_list_matches_every_foreign_key_to_a_transaction_in_the_schema() {
    let dir = TempDir::new().unwrap();
    let db = Database::open(&dir.path().join("coffer.db")).unwrap();
    let mut statement = db
        .conn()
        .prepare(
            "SELECT m.name, f.\"from\" \
             FROM sqlite_master AS m, pragma_foreign_key_list(m.name) AS f \
             WHERE m.type = 'table' AND f.\"table\" = 'transaction'",
        )
        .unwrap();
    let mut in_schema: Vec<(String, String)> = statement
        .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))
        .unwrap()
        .collect::<Result<_, _>>()
        .unwrap();
    in_schema.sort();
    let mut listed: Vec<(String, String)> = REFERENCES
        .iter()
        .map(|(table, column)| (table.to_string(), column.to_string()))
        .collect();
    listed.sort();
    assert_eq!(listed, in_schema);
}

#[test]
fn the_fixture_references_the_rows_to_remove_from_every_table() {
    let p = populated();
    for (table, n) in referencing(&p.db, &p.removed) {
        assert!(n > 0, "{table} holds nothing for the rows to remove");
    }
}

#[test]
fn removing_an_import_clears_every_reference_and_leaves_partners_unpaired() {
    let mut p = populated();
    let tx = p.db.conn_mut().transaction().unwrap();
    let removed = remove(&tx, Scope::Import(p.batch)).unwrap();
    tx.commit().unwrap();
    assert_eq!(removed.dissolved_groups, vec![p.group]);
    assert_removed(&p);
    assert!(transfer::group_of(&p.db, p.partner).unwrap().is_none());
}

#[test]
fn removing_the_rows_one_by_one_clears_every_reference_too() {
    let mut p = populated();
    let mut dissolved = Vec::new();
    for id in p.removed.clone() {
        let tx = p.db.conn_mut().transaction().unwrap();
        dissolved.extend(remove(&tx, Scope::Row(id)).unwrap().dissolved_groups);
        tx.commit().unwrap();
    }
    assert_eq!(dissolved, vec![p.group]);
    assert_removed(&p);
}

#[test]
fn removing_one_row_leaves_the_others_and_their_references() {
    let mut p = populated();
    let [purchase, ..] = p.removed[..] else {
        panic!("rows")
    };
    let tx = p.db.conn_mut().transaction().unwrap();
    let removed = remove(&tx, Scope::Row(purchase)).unwrap();
    tx.commit().unwrap();
    assert_eq!(removed.dissolved_groups, vec![]);
    assert_eq!(count(&p.db, "SELECT COUNT(*) FROM \"transaction\""), 7);
    assert_eq!(count(&p.db, "SELECT COUNT(*) FROM transfer_group"), 1);
    assert_eq!(count(&p.db, "SELECT COUNT(*) FROM schedule_match"), 1);
    assert_eq!(count(&p.db, "SELECT COUNT(*) FROM transaction_link"), 2);
    assert_eq!(count(&p.db, "SELECT COUNT(*) FROM duplicate_candidate"), 1);
}
