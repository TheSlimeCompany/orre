//! Tests for `migrations/V21__superseded.sql`: the rebuild of
//! `"transaction"` keeps every row, reference, index, trigger and view, and
//! the new `superseded` reason and `superseded_by` link hold their rules.

use std::collections::BTreeMap;

use rusqlite::types::Value;
use rusqlite::{Connection, ErrorCode, params};
use tempfile::TempDir;

use super::Database;

/// The tables whose rows V21 must keep: `"transaction"` and every table
/// that references it.
const KEPT_TABLES: [&str; 7] = [
    "transaction_split",
    "transaction_tag",
    "change_item",
    "transaction_link",
    "duplicate_candidate",
    "schedule_match",
    "transfer_group",
];

/// The schema objects V21 changes; every other object's SQL is unchanged.
const CHANGED: [&str; 4] = [
    "transaction",
    "transaction_external_id",
    "transaction_insert_new",
    "transaction_source_fields_read_only",
];

/// The objects V21 adds.
const ADDED: [&str; 3] = [
    "transaction_superseded_by",
    "transaction_superseded_by_update",
    "transaction_superseded_by_insert",
];

/// The objects migrations after V21 add: the test opens the database at the
/// latest version, so they appear too, and aren't V21's.
const ADDED_LATER: &[&str] = &[
    // V22, R.11.
    "category_key_retired",
    "category_key_retire",
    // V23, R.13.
    "account_source_identity",
    "account_source_identity_account",
    "account_source_identity_hash",
];

#[track_caller]
fn assert_refused<T: std::fmt::Debug>(result: rusqlite::Result<T>) {
    match result {
        Err(err) if err.sqlite_error_code() == Some(ErrorCode::ConstraintViolation) => {}
        other => panic!("expected a constraint violation, got {other:?}"),
    }
}

fn rows(conn: &Connection, sql: &str) -> Vec<Vec<Value>> {
    let mut statement = conn.prepare(sql).unwrap();
    let columns = statement.column_count();
    statement
        .query_map([], |row| {
            (0..columns)
                .map(|index| row.get::<_, Value>(index))
                .collect::<rusqlite::Result<Vec<_>>>()
        })
        .unwrap()
        .collect::<Result<_, _>>()
        .unwrap()
}

/// Every schema object's SQL, by name.
fn schema(conn: &Connection) -> BTreeMap<String, String> {
    rows(
        conn,
        "SELECT name, sql FROM sqlite_schema WHERE sql IS NOT NULL \
         AND name <> 'refinery_schema_history'",
    )
    .into_iter()
    .map(|row| match (&row[0], &row[1]) {
        (Value::Text(name), Value::Text(sql)) => (name.clone(), sql.clone()),
        other => panic!("unexpected schema row {other:?}"),
    })
    .collect()
}

/// The version just before `V21__superseded.sql`, found by name so the
/// tests don't depend on its number.
fn before_superseded() -> super::SchemaVersion {
    let superseded = super::migrations::embedded()
        .into_iter()
        .find(|migration| migration.name() == "superseded")
        .expect("the superseded migration");
    super::SchemaVersion::from(superseded.version()) - 1
}

/// The columns `"transaction"` had before V21 (since V18), in order.
const V18_COLUMNS: &str = "id, account_id, batch_id, date, time, amount_minor, currency, \
     merchant, description_raw, description_normalized, external_id, fingerprint, \
     source_category, source_type, original_amount_minor, original_currency, note, origin, \
     created_at, merchant_display, category_id, category_source, rule_id, transfer_group_id, \
     user_note, my_share_minor, share_label, reimbursable, excluded_reason, excluded_note, \
     excluded_at, merchant_display_source";

/// Fills a V18 database with a row in every table that references a
/// transaction, and deletes one transaction so the sequence is ahead of
/// the highest ID.
fn populate_v18(conn: &Connection) {
    conn.execute_batch(
        "INSERT INTO account (name, kind, currency) VALUES ('Current', 'current', 'GBP');
         INSERT INTO account (name, kind, currency) VALUES ('Savings', 'savings', 'GBP');
         INSERT INTO import_batch (account_id, source_file, format) VALUES (1, 'a.ofx', 'ofx');
         INSERT INTO import_batch (account_id, source_file, format) VALUES (2, 'b.ofx', 'ofx');
         INSERT INTO \"transaction\" (account_id, batch_id, date, time, amount_minor, currency,
             merchant, description_raw, description_normalized, external_id, fingerprint,
             source_category, source_type, original_amount_minor, original_currency, note,
             origin, user_note, my_share_minor, share_label, reimbursable)
         VALUES (1, 1, '2026-11-02', '12:30:00', -4000, 'GBP', 'Shop', 'Shop  one', 'SHOP ONE',
             'tx_1', 'fp-1', 'groceries', 'POS', -4600, 'EUR', 'bank note', 'imported',
             'mine', -2000, 'Sam', 1);
         INSERT INTO \"transaction\" (account_id, batch_id, date, amount_minor, currency,
             description_raw, description_normalized, external_id, fingerprint, origin)
         VALUES (1, 1, '2026-11-03', 2500, 'GBP', 'Refund', 'REFUND', 'tx_2', 'fp-2', 'imported');
         INSERT INTO \"transaction\" (account_id, batch_id, date, amount_minor, currency,
             description_raw, description_normalized, external_id, fingerprint, origin)
         VALUES (1, 1, '2026-11-04', -10000, 'GBP', 'To savings', 'TO SAVINGS', 'tx_3', 'fp-3',
             'imported');
         INSERT INTO \"transaction\" (account_id, batch_id, date, amount_minor, currency,
             description_raw, description_normalized, external_id, fingerprint, origin)
         VALUES (2, 2, '2026-11-04', 10000, 'GBP', 'From current', 'FROM CURRENT', 'tx_4',
             'fp-4', 'imported');
         INSERT INTO \"transaction\" (account_id, date, amount_minor, currency, description_raw,
             description_normalized, origin, excluded_reason, excluded_at, merchant_display,
             merchant_display_source)
         VALUES (1, '2026-11-05', -4000, 'GBP', 'Shop', 'SHOP', 'manual', 'bank_error',
             '2026-11-06T09:00:00.000Z', 'Shop', 'user');
         INSERT INTO \"transaction\" (account_id, date, amount_minor, currency, description_raw,
             description_normalized, origin)
         VALUES (1, '2026-11-06', -1, 'GBP', 'Gone', 'GONE', 'manual');
         DELETE FROM \"transaction\" WHERE id = 6;
         INSERT INTO \"transaction\" (account_id, batch_id, date, amount_minor, currency,
             description_raw, description_normalized, external_id, fingerprint, origin)
         VALUES (1, 1, '2026-11-07', -3000, 'GBP', 'Split', 'SPLIT', 'tx_7', 'fp-7', 'imported');
         INSERT INTO transaction_split (transaction_id, line_no, amount_minor) VALUES (7, 1, -1000);
         INSERT INTO transaction_split (transaction_id, line_no, amount_minor) VALUES (7, 2, -2000);
         INSERT INTO transfer_group (status) VALUES ('confirmed');
         UPDATE \"transaction\" SET transfer_group_id = 1 WHERE id IN (3, 4);
         INSERT INTO tag (name) VALUES ('holiday');
         INSERT INTO transaction_tag (transaction_id, tag_id) VALUES (1, 1);
         INSERT INTO change_batch (kind) VALUES ('bulk_edit');
         INSERT INTO change_item (batch_id, transaction_id, field, old_value, new_value)
         VALUES (1, 1, 'user_note', 'null', '\"mine\"');
         INSERT INTO transaction_link (kind, inflow_id, target_id, amount_minor, status)
         VALUES ('refund', 2, 1, 2500, 'suggested');
         INSERT INTO duplicate_candidate (transaction_a, transaction_b, reason)
         VALUES (1, 5, 'manual_entry');
         INSERT INTO schedule (account_id, merchant_display, cadence, start_date, amount_minor,
             currency, tolerance_days)
         VALUES (1, 'Shop', 'monthly', '2026-11-02', -4000, 'GBP', 3);
         INSERT INTO schedule_match (schedule_id, occurrence_date, transaction_id, matched_by)
         VALUES (1, '2026-11-02', 1, 'user');",
    )
    .unwrap();
}

/// V21 rebuilds `"transaction"` with foreign keys off: every row, ID and
/// reference stays, the sequence doesn't go back, foreign keys are on and
/// hold afterwards, and every other index, trigger and view is as it was.
#[test]
fn the_rebuild_keeps_every_row_reference_and_trigger() {
    let dir = TempDir::new().unwrap();
    let path = dir.path().join("coffer.db");
    let (transactions, others, before) = {
        let db = super::open_at_version(&path, before_superseded()).unwrap();
        let conn = db.conn();
        populate_v18(conn);
        let others: Vec<_> = KEPT_TABLES
            .iter()
            .map(|table| rows(conn, &format!("SELECT * FROM {table} ORDER BY 1, 2")))
            .collect();
        (
            rows(
                conn,
                &format!("SELECT {V18_COLUMNS} FROM \"transaction\" ORDER BY id"),
            ),
            others,
            schema(conn),
        )
    };

    let db = Database::open(&path).unwrap();
    let conn = db.conn();

    assert_eq!(
        rows(
            conn,
            &format!("SELECT {V18_COLUMNS} FROM \"transaction\" ORDER BY id")
        ),
        transactions
    );
    let superseded: i64 = conn
        .query_row(
            "SELECT count(*) FROM \"transaction\" WHERE superseded_by IS NOT NULL",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(superseded, 0);
    for (table, kept) in KEPT_TABLES.iter().zip(&others) {
        assert_eq!(
            &rows(conn, &format!("SELECT * FROM {table} ORDER BY 1, 2")),
            kept,
            "{table}"
        );
    }

    // Foreign keys are on again, and nothing points at nothing.
    let on: i64 = conn
        .query_row("PRAGMA foreign_keys", [], |row| row.get(0))
        .unwrap();
    assert_eq!(on, 1);
    assert!(rows(conn, "SELECT * FROM pragma_foreign_key_check").is_empty());
    assert_refused(conn.execute("DELETE FROM \"transaction\" WHERE id = 1", []));

    // The deleted row's ID isn't handed out again.
    conn.execute(
        "INSERT INTO \"transaction\" (account_id, date, amount_minor, currency, \
         description_raw, description_normalized, origin) \
         VALUES (1, '2026-11-08', -1, 'GBP', 'x', 'X', 'manual')",
        [],
    )
    .unwrap();
    assert_eq!(conn.last_insert_rowid(), 8);

    let after = schema(conn);
    for (name, sql) in &before {
        if CHANGED.contains(&name.as_str()) {
            assert!(after.contains_key(name), "{name} is gone");
        } else {
            assert_eq!(after.get(name), Some(sql), "{name} changed");
        }
    }
    let added: Vec<&str> = after
        .keys()
        .filter(|name| !before.contains_key(*name))
        .map(String::as_str)
        .filter(|name| !ADDED_LATER.contains(name))
        .collect();
    let mut expected = ADDED.to_vec();
    expected.sort_unstable();
    assert_eq!(added, expected);

    // The recreated triggers still work: the source fields stay read-only,
    // a split line keeps its parent's sign, a source ID is still unique.
    assert_refused(conn.execute(
        "UPDATE \"transaction\" SET amount_minor = 0 WHERE id = 1",
        [],
    ));
    assert_refused(conn.execute(
        "INSERT INTO transaction_split (transaction_id, line_no, amount_minor) VALUES (7, 3, 5)",
        [],
    ));
    assert_refused(conn.execute(
        "INSERT INTO \"transaction\" (account_id, batch_id, date, amount_minor, currency, \
         description_raw, description_normalized, external_id, fingerprint, origin) \
         VALUES (1, 1, '2026-11-09', -1, 'GBP', 'x', 'X', 'tx_1', 'fp-9', 'imported')",
        [],
    ));
}

// The superseded reason and link at V21 ---------------------------------------

struct Fixture {
    db: Database,
    _dir: TempDir,
}

impl Fixture {
    /// A database with account 1, its batch 1, and an imported row with
    /// source ID `tx_1` (ID 1).
    fn new() -> Fixture {
        let dir = TempDir::new().unwrap();
        let db = Database::open(&dir.path().join("coffer.db")).unwrap();
        db.conn()
            .execute_batch(
                "INSERT INTO account (name, kind, currency) VALUES ('Current', 'current', 'GBP');
                 INSERT INTO account (name, kind, currency) VALUES ('Other', 'current', 'GBP');
                 INSERT INTO import_batch (account_id, source_file, format)
                 VALUES (1, 'a.ofx', 'ofx');
                 INSERT INTO import_batch (account_id, source_file, format)
                 VALUES (2, 'b.ofx', 'ofx');",
            )
            .unwrap();
        let fixture = Fixture { db, _dir: dir };
        assert_eq!(fixture.insert(None, 1, "tx_1", "fp-1").unwrap(), 1);
        fixture
    }

    fn conn(&self) -> &Connection {
        self.db.conn()
    }

    /// Inserts an imported row on account `account` (its batch has the
    /// same ID), with ID `id` when given.
    fn insert(
        &self,
        id: Option<i64>,
        account: i64,
        external_id: &str,
        fingerprint: &str,
    ) -> rusqlite::Result<i64> {
        self.conn().execute(
            "INSERT INTO \"transaction\" (id, account_id, batch_id, date, amount_minor, \
             currency, description_raw, description_normalized, external_id, fingerprint, \
             origin) VALUES (?1, ?2, ?2, '2026-11-02', -1234, 'GBP', 'x', 'X', ?3, ?4, \
             'imported')",
            params![id, account, external_id, fingerprint],
        )?;
        Ok(self.conn().last_insert_rowid())
    }

    fn supersede(&self, id: i64, by: Option<i64>) -> rusqlite::Result<usize> {
        self.conn().execute(
            "UPDATE \"transaction\" SET excluded_reason = 'superseded', \
             excluded_at = '2026-11-10T09:00:00.000Z', superseded_by = ?1 WHERE id = ?2",
            params![by, id],
        )
    }
}

/// The import's order: point the stored row at the correction's ID, then
/// insert the correction with the same source ID. The foreign key waits for
/// the commit.
#[test]
fn a_superseded_row_steps_aside_for_its_correction() {
    let fixture = Fixture::new();
    let conn = fixture.conn();
    conn.execute_batch("BEGIN").unwrap();
    fixture.supersede(1, Some(2)).unwrap();
    assert_eq!(fixture.insert(Some(2), 1, "tx_1", "fp-2").unwrap(), 2);
    conn.execute_batch("COMMIT").unwrap();

    // A third version supersedes the second.
    conn.execute_batch("BEGIN").unwrap();
    fixture.supersede(2, Some(3)).unwrap();
    fixture.insert(Some(3), 1, "tx_1", "fp-3").unwrap();
    conn.execute_batch("COMMIT").unwrap();

    // Only the live row holds the source ID.
    assert_refused(fixture.insert(None, 1, "tx_1", "fp-4"));

    // Undo: the correction goes and the row it replaced comes back.
    conn.execute_batch("BEGIN").unwrap();
    conn.execute("DELETE FROM \"transaction\" WHERE id = 3", [])
        .unwrap();
    conn.execute(
        "UPDATE \"transaction\" SET excluded_reason = NULL, excluded_at = NULL, \
         superseded_by = NULL WHERE id = 2",
        [],
    )
    .unwrap();
    conn.execute_batch("COMMIT").unwrap();
}

#[test]
fn a_correction_that_never_comes_fails_the_commit() {
    let fixture = Fixture::new();
    let conn = fixture.conn();
    conn.execute_batch("BEGIN").unwrap();
    fixture.supersede(1, Some(99)).unwrap();
    assert_refused(conn.execute_batch("COMMIT"));
    conn.execute_batch("ROLLBACK").unwrap();
}

#[test]
fn the_reason_and_the_link_go_together() {
    let fixture = Fixture::new();
    let conn = fixture.conn();
    // The reason without the link, and the link without the reason.
    assert_refused(fixture.supersede(1, None));
    assert_refused(conn.execute(
        "UPDATE \"transaction\" SET superseded_by = 1 WHERE id = 1",
        [],
    ));
    // Another reason with the link.
    conn.execute_batch("BEGIN").unwrap();
    fixture.supersede(1, Some(2)).unwrap();
    fixture.insert(Some(2), 1, "tx_1", "fp-2").unwrap();
    assert_refused(conn.execute(
        "UPDATE \"transaction\" SET excluded_reason = 'other', excluded_note = 'x' WHERE id = 1",
        [],
    ));
    conn.execute_batch("COMMIT").unwrap();
    // A row isn't superseded by itself.
    let third = fixture.insert(None, 2, "tx_9", "fp-9").unwrap();
    assert_refused(fixture.supersede(third, Some(third)));
}

/// `superseded_by` changes only with the reason, so a superseded row is
/// never pointed at another row.
#[test]
fn a_superseded_row_is_not_pointed_elsewhere() {
    let fixture = Fixture::new();
    let conn = fixture.conn();
    conn.execute_batch("BEGIN").unwrap();
    fixture.supersede(1, Some(2)).unwrap();
    fixture.insert(Some(2), 1, "tx_1", "fp-2").unwrap();
    conn.execute_batch("COMMIT").unwrap();
    conn.execute_batch("BEGIN").unwrap();
    fixture.supersede(2, Some(3)).unwrap();
    fixture.insert(Some(3), 1, "tx_1", "fp-3").unwrap();
    conn.execute_batch("COMMIT").unwrap();

    assert_refused(conn.execute(
        "UPDATE \"transaction\" SET superseded_by = 3 WHERE id = 1",
        [],
    ));
}

/// The two rows share an account and a source ID, whichever comes first.
#[test]
fn a_correction_has_the_same_account_and_source_id() {
    let fixture = Fixture::new();
    let conn = fixture.conn();
    let other_id = fixture.insert(None, 1, "tx_2", "fp-2").unwrap();
    let other_account = fixture.insert(None, 2, "tx_1", "fp-3").unwrap();
    // Pointed at a row that exists.
    assert_refused(fixture.supersede(1, Some(other_id)));
    assert_refused(fixture.supersede(1, Some(other_account)));
    // Inserted under the ID the stored row points at.
    conn.execute_batch("BEGIN").unwrap();
    fixture.supersede(1, Some(10)).unwrap();
    assert_refused(fixture.insert(Some(10), 1, "tx_5", "fp-5"));
    assert_refused(fixture.insert(Some(10), 2, "tx_1", "fp-5"));
    conn.execute_batch("ROLLBACK").unwrap();
    // A new row isn't superseded.
    assert_refused(conn.execute(
        "INSERT INTO \"transaction\" (account_id, batch_id, date, amount_minor, currency, \
         description_raw, description_normalized, external_id, fingerprint, origin, \
         excluded_reason, excluded_at, superseded_by) VALUES (1, 1, '2026-11-02', -1, 'GBP', \
         'x', 'X', 'tx_8', 'fp-8', 'imported', 'superseded', '2026-11-10T09:00:00.000Z', 1)",
        [],
    ));
}
