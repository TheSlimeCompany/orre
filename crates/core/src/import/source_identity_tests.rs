//! Tests for the account-number check (R.13): `source_identity.rs` and
//! `migrations/V23__account_source_identity.sql`.

use chrono::NaiveDate;
use tempfile::TempDir;

use super::*;
use crate::account::{AccountKind, NewAccount};
use crate::setting;
use crate::statement::SourceAccountNumber;

use super::source_identity::hash;

const NUMBER: &str = "ACCT-4455-6677-8899";

struct Fixture {
    db: Database,
    dir: TempDir,
}

impl Fixture {
    fn new() -> Self {
        let dir = TempDir::new().unwrap();
        let db = Database::open(&dir.path().join("coffer.db")).unwrap();
        Fixture { db, dir }
    }

    fn account(&mut self, name: &str) -> AccountId {
        account::create(
            &mut self.db,
            &NewAccount::new(name, AccountKind::Current, Currency::GBP),
        )
        .unwrap()
    }

    fn preview(&self, account: AccountId, statement: Statement) -> ImportPreview {
        preview(&self.db, account, "statement.ofx", statement).unwrap()
    }

    fn import(&mut self, account: AccountId, statement: Statement) -> BatchId {
        let preview = self.preview(account, statement);
        match commit(&mut self.db, &preview, preview.needs_confirmation()).unwrap() {
            Committed::Batch(summary) => summary.batch,
            Committed::NothingNew => panic!("expected a batch"),
        }
    }

    fn count(&self, sql: &str) -> i64 {
        self.db.conn().query_row(sql, [], |row| row.get(0)).unwrap()
    }
}

/// An OFX statement with one row, dated and described by `n`, so that
/// every statement made with a different `n` has something new.
fn file(number: Option<&str>, n: u32) -> Statement {
    let mut statement = Statement::new(SourceFormat::Ofx, Currency::GBP);
    if let Some(number) = number {
        statement = statement.with_source_account(SourceAccountNumber::new(number).unwrap());
    }
    let mut line = StatementLine::new(
        NaiveDate::from_ymd_opt(2026, 11, n).unwrap(),
        Money::new(-100 * i64::from(n), Currency::GBP),
        format!("Shop {n}"),
    );
    line.external_id = Some(format!("FIT-{n}"));
    statement.push(SourceLocation::Entry(1), Ok(line));
    statement
}

#[test]
fn the_number_is_normalised_before_it_is_hashed() {
    let plain = SourceAccountNumber::new("ACCT44556677").unwrap();
    for same in [" acct-4455 6677 ", "Acct-44-55-66-77", "\tACCT 4455-6677\n"] {
        assert_eq!(SourceAccountNumber::new(same).unwrap(), plain, "{same:?}");
    }
    assert_ne!(SourceAccountNumber::new("ACCT44556678").unwrap(), plain);
    // A masked number is an identity like any other.
    assert!(SourceAccountNumber::new("XXXX-XXXX-XXXX-9876").is_some());
    assert!(SourceAccountNumber::new(" - \t").is_none());
}

#[test]
fn debug_output_never_shows_the_number() {
    let number = SourceAccountNumber::new(NUMBER).unwrap();
    let statement = file(Some(NUMBER), 1);
    for shown in [format!("{number:?}"), format!("{statement:?}")] {
        assert!(!shown.contains("4455"), "{shown}");
        assert!(!shown.contains("ACCT"), "{shown}");
    }
    assert_eq!(format!("{number:?}"), "SourceAccountNumber(..)");
}

#[test]
fn committing_learns_a_hash_and_never_stores_the_number() {
    let mut fixture = Fixture::new();
    let account = fixture.account("Current");
    // The preview only reads: no key yet.
    let first = fixture.preview(account, file(Some(NUMBER), 1));
    assert_eq!(
        fixture.count("SELECT COUNT(*) FROM setting WHERE key = 'import.account_key'"),
        0
    );
    assert!(first.source_account_mismatch().is_none());

    let batch = fixture.import(account, file(Some(NUMBER), 1));

    assert_eq!(
        fixture.count("SELECT COUNT(*) FROM setting WHERE key = 'import.account_key'"),
        1
    );
    let (stored_batch, stored_account, hash): (i64, i64, String) = fixture
        .db
        .conn()
        .query_row(
            "SELECT batch_id, account_id, identity_hash FROM account_source_identity",
            [],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .unwrap();
    assert_eq!((stored_batch, stored_account), (batch.get(), account.get()));
    assert_eq!(hash.len(), 64);

    // Nowhere in the database file, or its journal, is the number, in
    // either spelling.
    fixture
        .db
        .conn()
        .execute_batch("PRAGMA wal_checkpoint(TRUNCATE)")
        .unwrap();
    let mut bytes = std::fs::read(fixture.db.path()).unwrap();
    let wal = fixture.db.path().with_extension("db-wal");
    if let Ok(more) = std::fs::read(wal) {
        bytes.extend(more);
    }
    for needle in [NUMBER, "ACCT445566778899", "445566778899"] {
        assert!(
            !bytes
                .windows(needle.len())
                .any(|window| window == needle.as_bytes()),
            "{needle} is stored"
        );
    }
    drop(fixture.dir);
}

#[test]
fn the_hash_depends_on_the_database_key() {
    let number = SourceAccountNumber::new(NUMBER).unwrap();
    let mut hashes = Vec::new();
    for _ in 0..2 {
        let mut fixture = Fixture::new();
        let account = fixture.account("Current");
        fixture.import(account, file(Some(NUMBER), 1));
        let key = setting::source_identity_key_on(fixture.db.conn())
            .unwrap()
            .unwrap();
        let stored: String = fixture
            .db
            .conn()
            .query_row(
                "SELECT identity_hash FROM account_source_identity",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(stored, hash(&key, &number));
        hashes.push(stored);
    }
    assert_ne!(hashes[0], hashes[1]);
}

#[test]
fn a_file_for_another_number_warns_once_the_account_has_a_hash() {
    let mut fixture = Fixture::new();
    let account = fixture.account("Current");
    fixture.import(account, file(Some(NUMBER), 1));

    let same = fixture.preview(account, file(Some(" acct 4455-6677-8899"), 2));
    assert_eq!(same.source_account_mismatch(), None);

    let other = fixture.preview(account, file(Some("ACCT-0000-1111-2222"), 2));
    assert_eq!(
        other.source_account_mismatch(),
        Some(&SourceAccountMismatch {
            other_account: None,
            account_has_others: true,
        })
    );
    // A warning is not a confirmation.
    assert_eq!(other.needs_confirmation(), Confirmations::default());
}

#[test]
fn no_warning_without_a_number_a_hash_or_a_key() {
    let mut fixture = Fixture::new();
    let account = fixture.account("Current");
    let spare = fixture.account("Spare");
    // No key, no hash.
    assert_eq!(
        fixture
            .preview(account, file(Some(NUMBER), 1))
            .source_account_mismatch(),
        None
    );

    fixture.import(account, file(Some(NUMBER), 1));
    // A file with no number (a CSV has none), and an account that has
    // never had a hash getting a number no account has.
    assert_eq!(
        fixture
            .preview(account, file(None, 2))
            .source_account_mismatch(),
        None
    );
    assert_eq!(
        fixture
            .preview(spare, file(Some("ACCT-0000-1111-2222"), 2))
            .source_account_mismatch(),
        None
    );
}

#[test]
fn the_warning_names_the_account_the_number_belongs_to() {
    let mut fixture = Fixture::new();
    let current = fixture.account("Current");
    let savings = fixture.account("Savings");
    fixture.import(current, file(Some(NUMBER), 1));
    fixture.import(savings, file(Some("ACCT-0000-1111-2222"), 2));

    let wrong = fixture.preview(savings, file(Some(NUMBER), 3));

    assert_eq!(
        wrong.source_account_mismatch(),
        Some(&SourceAccountMismatch {
            other_account: Some(OtherAccount {
                id: current,
                name: "Current".to_owned(),
            }),
            account_has_others: true,
        })
    );
}

#[test]
fn a_new_account_is_warned_about_a_number_another_account_has() {
    let mut fixture = Fixture::new();
    let current = fixture.account("Current");
    let spare = fixture.account("Spare");
    fixture.import(current, file(Some(NUMBER), 1));

    // Spare has had no file yet, but this number is Current's.
    let wrong = fixture.preview(spare, file(Some(NUMBER), 2));
    assert_eq!(
        wrong.source_account_mismatch(),
        Some(&SourceAccountMismatch {
            other_account: Some(OtherAccount {
                id: current,
                name: "Current".to_owned(),
            }),
            account_has_others: false,
        })
    );
    assert_eq!(wrong.needs_confirmation(), Confirmations::default());
}

#[test]
fn importing_anyway_learns_the_number_so_a_replacement_card_warns_once() {
    let mut fixture = Fixture::new();
    let account = fixture.account("Card");
    fixture.import(account, file(Some(NUMBER), 1));
    let replacement = "ACCT-9999-8888-7777";

    assert!(
        fixture
            .preview(account, file(Some(replacement), 2))
            .source_account_mismatch()
            .is_some()
    );
    fixture.import(account, file(Some(replacement), 2));

    // Both numbers are the account's now.
    assert_eq!(
        fixture
            .preview(account, file(Some(replacement), 3))
            .source_account_mismatch(),
        None
    );
    assert_eq!(
        fixture
            .preview(account, file(Some(NUMBER), 3))
            .source_account_mismatch(),
        None
    );
}

#[test]
fn undoing_a_batch_forgets_its_hash_unless_another_batch_has_it() {
    let mut fixture = Fixture::new();
    let account = fixture.account("Card");
    let first = fixture.import(account, file(Some(NUMBER), 1));
    let second = fixture.import(account, file(Some(NUMBER), 2));
    let replacement = fixture.import(account, file(Some("ACCT-9999-8888-7777"), 3));

    // Another batch still has the number.
    undo(&mut fixture.db, first).unwrap();
    assert_eq!(
        fixture.count("SELECT COUNT(*) FROM account_source_identity"),
        2
    );
    assert_eq!(
        fixture
            .preview(account, file(Some(NUMBER), 4))
            .source_account_mismatch(),
        None
    );

    undo(&mut fixture.db, second).unwrap();
    assert_eq!(
        fixture.count("SELECT COUNT(*) FROM account_source_identity"),
        1
    );
    assert!(
        fixture
            .preview(account, file(Some(NUMBER), 4))
            .source_account_mismatch()
            .is_some()
    );

    // With the last batch gone the account has no hash: no warning.
    undo(&mut fixture.db, replacement).unwrap();
    assert_eq!(
        fixture.count("SELECT COUNT(*) FROM account_source_identity"),
        0
    );
    assert_eq!(
        fixture
            .preview(account, file(Some(NUMBER), 4))
            .source_account_mismatch(),
        None
    );
}

#[test]
fn rehearsing_writes_no_key_and_no_hash() {
    let mut fixture = Fixture::new();
    let account = fixture.account("Card");
    let preview = fixture.preview(account, file(Some(NUMBER), 1));

    rehearse(&fixture.db, &preview).unwrap().unwrap();

    assert_eq!(
        fixture.count("SELECT COUNT(*) FROM setting WHERE key = 'import.account_key'"),
        0
    );
    assert_eq!(
        fixture.count("SELECT COUNT(*) FROM account_source_identity"),
        0
    );
}

#[test]
fn a_file_without_a_number_stores_nothing() {
    let mut fixture = Fixture::new();
    let account = fixture.account("Card");
    fixture.import(account, file(None, 1));

    assert_eq!(
        fixture.count("SELECT COUNT(*) FROM account_source_identity"),
        0
    );
    assert_eq!(
        fixture.count("SELECT COUNT(*) FROM setting WHERE key = 'import.account_key'"),
        0
    );
}

/// Account deletion is refused while a batch exists, so the cascade is
/// only reachable once the batch goes too, in one deferred transaction.
#[test]
fn deleting_an_account_takes_its_hashes_with_it() {
    let mut fixture = Fixture::new();
    let account = fixture.account("Card");
    let kept = fixture.account("Kept");
    let batch = fixture.import(account, file(Some(NUMBER), 1));
    fixture.import(kept, file(Some("ACCT-9999-8888-7777"), 2));
    undo(&mut fixture.db, batch).unwrap();
    // Undo already forgot it; learn it again for the cascade to have work.
    fixture
        .db
        .conn()
        .execute(
            "INSERT INTO account_source_identity (batch_id, account_id, identity_hash) \
             VALUES (?1, ?2, ?3)",
            params![batch, account, "0".repeat(64)],
        )
        .unwrap();

    let tx = fixture.db.conn_mut().transaction().unwrap();
    tx.pragma_update(None, "defer_foreign_keys", true).unwrap();
    tx.execute("DELETE FROM account WHERE id = ?1", [account])
        .unwrap();
    let remaining: i64 = tx
        .query_row(
            "SELECT COUNT(*) FROM account_source_identity WHERE account_id = ?1",
            [account],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(remaining, 0);
    // The batch row would block the commit; remove it as a real delete
    // would have to.
    tx.execute("DELETE FROM import_batch WHERE id = ?1", [batch])
        .unwrap();
    tx.commit().unwrap();

    assert_eq!(
        fixture.count("SELECT COUNT(*) FROM account_source_identity"),
        1
    );
}
