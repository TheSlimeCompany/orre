//! Tests that the full-text index (`transaction_text`, V24) follows every
//! write to a transaction's text: an import, its undo, an edit of the
//! display merchant, a bank correction and a restore of an older backup.
//!
//! "Follows" means a match through the index finds the new text and no
//! longer finds the removed or old text, and FTS5's own `integrity-check`
//! (against the external content) passes.

use chrono::NaiveDate;
use rusqlite::Connection;
use tempfile::TempDir;

use super::phrase;
use crate::account::{self, AccountKind, NewAccount};
use crate::backup;
use crate::db::{self, Database};
use crate::edit;
use crate::id::{AccountId, TransactionId};
use crate::import::{self, Committed};
use crate::money::{Currency, Money};
use crate::statement::{SourceFormat, SourceLocation, Statement, StatementLine};

/// The rows the index finds for `text`, by ID.
fn found(conn: &Connection, text: &str) -> Vec<i64> {
    let query = phrase(text).unwrap_or_else(|| panic!("{text:?} has no phrase"));
    let mut statement = conn
        .prepare("SELECT rowid FROM transaction_text WHERE transaction_text MATCH ?1 ORDER BY 1")
        .unwrap();
    statement
        .query_map([query], |row| row.get(0))
        .unwrap()
        .collect::<Result<_, _>>()
        .unwrap()
}

/// Asserts the index agrees with the `"transaction"` rows it indexes: FTS5's
/// `integrity-check` with `rank = 1` compares it with the external content.
#[track_caller]
fn assert_consistent(conn: &Connection) {
    conn.execute(
        "INSERT INTO transaction_text (transaction_text, rank) VALUES ('integrity-check', 1)",
        [],
    )
    .unwrap_or_else(|error| panic!("the index disagrees with the transactions: {error}"));
}

fn day(d: u32) -> NaiveDate {
    NaiveDate::from_ymd_opt(2026, 11, d).unwrap()
}

struct Fixture {
    db: Database,
    _dir: TempDir,
    account: AccountId,
}

impl Fixture {
    fn new() -> Self {
        let dir = TempDir::new().unwrap();
        let mut db = Database::open(&dir.path().join("orre.db")).unwrap();
        let account = account::create(
            &mut db,
            &NewAccount::new("Current", AccountKind::Current, Currency::GBP),
        )
        .unwrap();
        Fixture {
            db,
            _dir: dir,
            account,
        }
    }

    /// Imports `rows` (day of November 2026, amount, source ID, merchant,
    /// description) from an OFX-like file, accepting any confirmation.
    fn import(&mut self, rows: &[(u32, i64, &str, &str, &str)]) -> import::CommitSummary {
        let mut statement = Statement::new(SourceFormat::Ofx, Currency::GBP);
        for (index, &(d, minor, id, merchant, description)) in rows.iter().enumerate() {
            let mut line = StatementLine::new(
                day(d),
                Money::new(minor, Currency::GBP),
                description.to_owned(),
            );
            line.external_id = Some(id.to_owned());
            line.merchant = Some(merchant.to_owned());
            statement.push(SourceLocation::Entry(index as u64 + 1), Ok(line));
        }
        let preview = import::preview(&self.db, self.account, "statement.ofx", statement).unwrap();
        let confirm = preview.needs_confirmation();
        match import::commit(&mut self.db, &preview, confirm).unwrap() {
            Committed::Batch(summary) => summary,
            Committed::NothingNew => panic!("expected a batch"),
        }
    }

    fn conn(&self) -> &Connection {
        self.db.conn()
    }

    /// The live row with source ID `id`.
    fn live(&self, id: &str) -> TransactionId {
        self.conn()
            .query_row(
                "SELECT id FROM \"transaction\" WHERE external_id = ?1 AND superseded_by IS NULL",
                [id],
                |row| row.get(0),
            )
            .unwrap()
    }
}

#[test]
fn an_import_is_indexed_by_its_display_merchant_merchant_and_description() {
    let mut fixture = Fixture::new();
    fixture.import(&[
        (
            2,
            -350,
            "a1",
            "Bean There",
            "CARD PAYMENT TO BEAN THERE LONDON",
        ),
        (3, -900, "a2", "Noodle Bar", "POS NOODLEBAR 0042"),
    ]);
    let (first, second) = (fixture.live("a1"), fixture.live("a2"));
    let conn = fixture.conn();

    assert_eq!(found(conn, "bean there"), [first.get()]);
    assert_eq!(found(conn, "LONDON"), [first.get()]);
    assert_eq!(found(conn, "noodlebar 00"), [second.get()]);
    assert_eq!(found(conn, "nonexistent"), Vec::<i64>::new());
    let display: String = conn
        .query_row(
            "SELECT merchant_display FROM \"transaction\" WHERE id = ?1",
            [first],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(found(conn, &display), [first.get()]);
    assert_consistent(conn);
}

#[test]
fn an_undone_import_leaves_the_index() {
    let mut fixture = Fixture::new();
    fixture.import(&[(2, -350, "k1", "Keeper Cafe", "KEEPER CAFE")]);
    let kept = fixture.live("k1");
    let batch = fixture
        .import(&[(3, -900, "g1", "Goner Grill", "GONER GRILL 77")])
        .batch;
    assert_eq!(found(fixture.conn(), "goner grill").len(), 1);

    import::undo(&mut fixture.db, batch).unwrap();

    let conn = fixture.conn();
    assert_eq!(found(conn, "goner grill"), Vec::<i64>::new());
    assert_eq!(found(conn, "GONER"), Vec::<i64>::new());
    assert_eq!(found(conn, "keeper cafe"), [kept.get()]);
    assert_consistent(conn);
}

#[test]
fn editing_the_display_merchant_moves_the_index_with_it() {
    let mut fixture = Fixture::new();
    fixture.import(&[(2, -350, "m1", "", "CARD PAYMENT TO ACME 123")]);
    let row = fixture.live("m1");

    edit::set_merchant(&mut fixture.db, row, "Renamed Roasters").unwrap();
    let conn = fixture.conn();
    assert_eq!(found(conn, "renamed roasters"), [row.get()]);
    // The bank's text is still there.
    assert_eq!(found(conn, "ACME 123"), [row.get()]);
    assert_consistent(conn);

    edit::set_merchant(&mut fixture.db, row, "Second Name").unwrap();
    let conn = fixture.conn();
    assert_eq!(found(conn, "renamed roasters"), Vec::<i64>::new());
    assert_eq!(found(conn, "second name"), [row.get()]);
    assert_consistent(conn);

    // Blank goes back to the derived name, so the typed one is gone again.
    edit::set_merchant(&mut fixture.db, row, "").unwrap();
    let conn = fixture.conn();
    assert_eq!(found(conn, "second name"), Vec::<i64>::new());
    assert_eq!(found(conn, "ACME 123"), [row.get()]);
    assert_consistent(conn);
}

/// Undoing the edit (a change batch) writes the old display merchant back.
#[test]
fn undoing_a_merchant_edit_restores_the_old_index_entry() {
    let mut fixture = Fixture::new();
    fixture.import(&[(2, -350, "u1", "", "CARD PAYMENT TO ACME 123")]);
    let row = fixture.live("u1");
    let batch = edit::set_merchant(&mut fixture.db, row, "Temporary Name")
        .unwrap()
        .unwrap();
    assert_eq!(found(fixture.conn(), "temporary name"), [row.get()]);

    crate::change::undo(&mut fixture.db, batch).unwrap();

    let conn = fixture.conn();
    assert_eq!(found(conn, "temporary name"), Vec::<i64>::new());
    assert_eq!(found(conn, "ACME 123"), [row.get()]);
    assert_consistent(conn);
}

/// A bank correction (the same source ID on another date or amount) keeps
/// the old row, superseded, and adds the new one; both are indexed.
#[test]
fn a_bank_correction_keeps_both_rows_indexed() {
    let mut fixture = Fixture::new();
    fixture.import(&[(2, -350, "c1", "Corrected Cafe", "ORIGINAL TEXT")]);
    let original = fixture.live("c1");

    fixture.import(&[(3, -360, "c1", "Corrected Cafe", "REVISED TEXT")]);
    let revised = fixture.live("c1");

    assert_ne!(original, revised, "the correction superseded the row");
    let conn = fixture.conn();
    assert_eq!(found(conn, "original text"), [original.get()]);
    assert_eq!(found(conn, "revised text"), [revised.get()]);
    assert_eq!(
        found(conn, "corrected cafe"),
        [original.get(), revised.get()]
    );
    assert_consistent(conn);
}

/// Opens a database at the version before V24, as an older Orre left it,
/// and puts two rows in it (SQL, since the old schema has no index).
fn older_database(path: &std::path::Path) {
    let db = db::open_at_version(path, Database::supported_schema_version() - 1).unwrap();
    db.conn()
        .execute_batch(
            "INSERT INTO account (name, kind, currency) VALUES ('Old', 'current', 'GBP');
             INSERT INTO \"transaction\" (account_id, date, amount_minor, currency, merchant,
                 description_raw, description_normalized, origin, merchant_display)
             VALUES (1, '2026-11-02', -100, 'GBP', 'Old Merchant', 'OLD DESCRIPTION', 'OLD',
                 'manual', 'Old Display');
             INSERT INTO \"transaction\" (account_id, date, amount_minor, currency,
                 description_raw, description_normalized, origin)
             VALUES (1, '2026-11-03', -200, 'GBP', 'SECOND ROW', 'SECOND', 'manual');",
        )
        .unwrap();
    assert!(
        db.conn().prepare("SELECT 1 FROM transaction_text").is_err(),
        "the index doesn't exist before its migration"
    );
}

#[test]
fn migrating_builds_the_index_for_existing_rows() {
    let dir = TempDir::new().unwrap();
    let path = dir.path().join("orre.db");
    older_database(&path);

    let db = Database::open(&path).unwrap();

    let conn = db.conn();
    assert_eq!(found(conn, "old merchant"), [1]);
    assert_eq!(found(conn, "old display"), [1]);
    assert_eq!(found(conn, "OLD DESCRIPTION"), [1]);
    assert_eq!(found(conn, "second row"), [2]);
    assert_consistent(conn);
}

#[test]
fn restoring_an_older_backup_rebuilds_the_index() {
    let dir = TempDir::new().unwrap();
    let old = dir.path().join("old.db");
    older_database(&old);
    let path = dir.path().join("data").join("orre.db");

    let restored = backup::restore(&path, &old).unwrap();

    assert_eq!(
        restored.migrated_to,
        Some(Database::supported_schema_version())
    );
    let db = Database::open(&path).unwrap();
    let conn = db.conn();
    assert_eq!(found(conn, "old merchant"), [1]);
    assert_eq!(found(conn, "second row"), [2]);
    assert_consistent(conn);
}

/// A backup taken at the current version carries its index, which keeps
/// working after a restore.
#[test]
fn restoring_a_current_backup_keeps_the_index() {
    let mut fixture = Fixture::new();
    fixture.import(&[(2, -350, "r1", "Restored Roastery", "RESTORED ROASTERY 9")]);
    let taken = backup::create(&fixture.db, backup::Reason::Manual).unwrap();
    let dir = TempDir::new().unwrap();
    let path = dir.path().join("data").join("orre.db");

    backup::restore(&path, &taken.path).unwrap();

    let db = Database::open(&path).unwrap();
    assert_eq!(found(db.conn(), "restored roastery").len(), 1);
    assert_consistent(db.conn());
}
