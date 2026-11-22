use std::fs::{self, File, OpenOptions};

use tempfile::TempDir;

use super::*;
use crate::account::{self, AccountKind, NewAccount};
use crate::backup::{self, Reason, restore_lock_path};
use crate::money::Currency;

fn db_path(dir: &TempDir) -> PathBuf {
    dir.path().join("data").join("orre.db")
}

fn topics(topics: &[Topic]) -> Check {
    Check::Changed(topics.iter().copied().collect())
}

/// Runs `sql` on a connection of its own, which is closed again.
fn write(path: &Path, sql: &str) {
    let db = Database::open(path).unwrap();
    db.conn().execute_batch(sql).unwrap();
}

/// A database, and a watcher that has taken its baseline.
fn watched(dir: &TempDir) -> (PathBuf, Watcher) {
    let path = db_path(dir);
    drop(Database::open(&path).unwrap());
    let mut watcher = Watcher::new(&path);
    assert_eq!(watcher.check().unwrap(), topics(&[]));
    (path, watcher)
}

/// Holds the restore lock for `db_path` as a running restore in another
/// process would, until dropped.
fn hold_restore_lock(db_path: &Path) -> File {
    let file = OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(false)
        .open(restore_lock_path(db_path))
        .unwrap();
    file.lock().unwrap();
    file
}

#[test]
fn no_database_file_is_no_change_and_no_error() {
    let dir = TempDir::new().unwrap();
    let path = db_path(&dir);
    let mut watcher = Watcher::new(&path);
    assert_eq!(watcher.check().unwrap(), Check::Unchanged);
    assert_eq!(watcher.opens, 0);

    // The first check that finds the file is the baseline.
    drop(Database::open(&path).unwrap());
    assert_eq!(watcher.check().unwrap(), topics(&[]));
    assert_eq!(watcher.check().unwrap(), Check::Unchanged);
}

#[test]
fn an_unchanged_stamp_opens_nothing() {
    let dir = TempDir::new().unwrap();
    let (_, mut watcher) = watched(&dir);
    assert_eq!(watcher.opens, 1);

    for _ in 0..3 {
        assert_eq!(watcher.check().unwrap(), Check::Unchanged);
    }
    assert_eq!(watcher.opens, 1);
}

#[test]
fn a_suggestion_is_the_inbox_only() {
    let dir = TempDir::new().unwrap();
    let (path, mut watcher) = watched(&dir);

    write(
        &path,
        "INSERT INTO suggestion (kind, payload_json, source_client) \
         VALUES ('merchant_alias', '{}', 'claude-code')",
    );
    assert_eq!(watcher.check().unwrap(), topics(&[Topic::Inbox]));
    assert_eq!(watcher.check().unwrap(), Check::Unchanged);

    // A decision made elsewhere shows too.
    write(
        &path,
        "UPDATE suggestion SET status = 'rejected', \
         decided_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')",
    );
    assert_eq!(watcher.check().unwrap(), topics(&[Topic::Inbox]));
}

#[test]
fn an_account_and_an_undone_batch_are_the_ledger() {
    let dir = TempDir::new().unwrap();
    let (path, mut watcher) = watched(&dir);

    {
        let mut db = Database::open(&path).unwrap();
        account::create(
            &mut db,
            &NewAccount::new("Current", AccountKind::Current, Currency::GBP),
        )
        .unwrap();
    }
    assert_eq!(watcher.check().unwrap(), topics(&[Topic::Ledger]));

    write(
        &path,
        "INSERT INTO import_batch (account_id, source_file, format) \
         VALUES (1, 'a.csv', 'csv')",
    );
    assert_eq!(watcher.check().unwrap(), topics(&[Topic::Ledger]));

    // Undoing keeps the row and its ID: only the status changes.
    write(&path, "UPDATE import_batch SET status = 'undone'");
    assert_eq!(watcher.check().unwrap(), topics(&[Topic::Ledger]));

    // An edit of an account in place.
    write(&path, "UPDATE account SET name = 'Renamed'");
    assert_eq!(watcher.check().unwrap(), topics(&[Topic::Ledger]));
}

#[test]
fn a_rule_and_its_reorder_are_the_rules() {
    let dir = TempDir::new().unwrap();
    let (path, mut watcher) = watched(&dir);

    write(
        &path,
        "INSERT INTO rule (priority, match_field, match_kind, pattern) \
         VALUES (1, 'merchant', 'contains', 'shop')",
    );
    assert_eq!(watcher.check().unwrap(), topics(&[Topic::Rules]));
    write(&path, "UPDATE rule SET priority = 2");
    assert_eq!(watcher.check().unwrap(), topics(&[Topic::Rules]));
}

#[test]
fn categories_and_merchant_aliases_are_the_rules() {
    let dir = TempDir::new().unwrap();
    let (path, mut watcher) = watched(&dir);

    write(&path, "UPDATE category SET label = 'Renamed' WHERE id = 1");
    assert_eq!(watcher.check().unwrap(), topics(&[Topic::Rules]));
    write(
        &path,
        "INSERT INTO merchant_alias (match_kind, pattern, display_name) \
         VALUES ('prefix', 'SHOP', 'Shop')",
    );
    assert_eq!(watcher.check().unwrap(), topics(&[Topic::Rules]));
}

#[test]
fn what_changes_without_the_user_is_not_a_change() {
    let dir = TempDir::new().unwrap();
    let (path, mut watcher) = watched(&dir);
    write(
        &path,
        "INSERT INTO mcp_client (name) VALUES ('claude-code')",
    );
    assert_eq!(watcher.check().unwrap(), topics(&[Topic::Settings]));

    // A client's call and the app saving its window.
    write(
        &path,
        "UPDATE mcp_client SET last_seen_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now'); \
         INSERT INTO setting (key, value) VALUES ('app.window', '{}')",
    );
    assert_eq!(watcher.check().unwrap(), topics(&[]));
    write(
        &path,
        "UPDATE setting SET value = '{\"x\": 1}' WHERE key = 'app.window'",
    );
    assert_eq!(watcher.check().unwrap(), topics(&[]));
    assert_eq!(crate::setting::WINDOW_STATE, "app.window");
}

#[test]
fn rebase_absorbs_only_the_topics_given() {
    let dir = TempDir::new().unwrap();
    let (path, mut watcher) = watched(&dir);

    write(
        &path,
        "INSERT INTO suggestion (kind, payload_json, source_client) \
         VALUES ('merchant_alias', '{}', 'claude-code'); \
         INSERT INTO mcp_client (name) VALUES ('claude-code')",
    );
    watcher.rebase(&[Topic::Inbox]).unwrap();
    assert_eq!(watcher.check().unwrap(), topics(&[Topic::Settings]));
    assert_eq!(watcher.check().unwrap(), Check::Unchanged);
}

#[test]
fn rebase_before_the_baseline_or_during_a_restore_does_nothing() {
    let dir = TempDir::new().unwrap();
    let path = db_path(&dir);
    drop(Database::open(&path).unwrap());
    let mut watcher = Watcher::new(&path);
    watcher.rebase(&[Topic::Inbox]).unwrap();
    assert_eq!(watcher.opens, 0);
    assert_eq!(watcher.check().unwrap(), topics(&[]));

    write(
        &path,
        "INSERT INTO mcp_client (name) VALUES ('claude-code')",
    );
    let lock = hold_restore_lock(&path);
    watcher.rebase(&[Topic::Settings]).unwrap();
    drop(lock);
    assert_eq!(watcher.check().unwrap(), topics(&[Topic::Settings]));
}

#[test]
fn the_checks_own_open_is_not_a_change() {
    let dir = TempDir::new().unwrap();
    let (_, mut watcher) = watched(&dir);
    // The baseline's open created and removed the `-wal`; the next check
    // finds the stamp as the baseline left it.
    assert_eq!(watcher.check().unwrap(), Check::Unchanged);
    assert_eq!(watcher.opens, 1);
}

#[test]
fn a_balance_snapshot_is_the_ledger_and_so_is_deleting_it() {
    let dir = TempDir::new().unwrap();
    let (path, mut watcher) = watched(&dir);
    write(
        &path,
        "INSERT INTO account (name, kind, currency) VALUES ('Current', 'current', 'GBP')",
    );
    assert_eq!(watcher.check().unwrap(), topics(&[Topic::Ledger]));

    write(
        &path,
        "INSERT INTO balance_snapshot (account_id, date, balance_minor, source) \
         VALUES (1, '2026-11-01', 100, 'manual')",
    );
    assert_eq!(watcher.check().unwrap(), topics(&[Topic::Ledger]));
    write(&path, "DELETE FROM balance_snapshot");
    assert_eq!(watcher.check().unwrap(), topics(&[Topic::Ledger]));
}

/// The columns each hashed table is read with and leaves out are the
/// table's columns, no more and no fewer.
#[test]
fn the_hashed_columns_are_the_tables_columns() {
    let dir = TempDir::new().unwrap();
    let db = Database::open(&db_path(&dir)).unwrap();
    for table in ROWS {
        let mut statement = db
            .conn()
            .prepare(&format!(
                "SELECT name FROM pragma_table_info('{}')",
                table.table
            ))
            .unwrap();
        let live: BTreeSet<String> = statement
            .query_map([], |row| row.get(0))
            .unwrap()
            .collect::<Result<_, _>>()
            .unwrap();
        let listed: BTreeSet<String> = table
            .columns
            .iter()
            .chain(table.left_out)
            .map(|column| column.to_string())
            .collect();
        assert_eq!(live, listed, "{}", table.table);
        assert!(
            table
                .columns
                .iter()
                .all(|column| !table.left_out.contains(column)),
            "{}",
            table.table
        );
    }
}

#[test]
fn a_client_and_a_setting_are_the_settings() {
    let dir = TempDir::new().unwrap();
    let (path, mut watcher) = watched(&dir);

    write(
        &path,
        "INSERT INTO mcp_client (name) VALUES ('claude-code')",
    );
    assert_eq!(watcher.check().unwrap(), topics(&[Topic::Settings]));
    write(&path, "UPDATE mcp_client SET status = 'known', enabled = 1");
    assert_eq!(watcher.check().unwrap(), topics(&[Topic::Settings]));
    write(
        &path,
        "INSERT INTO setting (key, value) VALUES ('mcp.enabled', 'false')",
    );
    assert_eq!(watcher.check().unwrap(), topics(&[Topic::Settings]));
}

#[test]
fn a_checkpoint_changes_the_stamp_and_no_topic() {
    let dir = TempDir::new().unwrap();
    let (path, mut watcher) = watched(&dir);
    // Another process holds a connection, so the writes below stay in the
    // `-wal` until it checkpoints them into the database file.
    let held = Database::open(&path).unwrap();
    write(
        &path,
        "INSERT INTO mcp_client (name) VALUES ('claude-code')",
    );
    assert_eq!(watcher.check().unwrap(), topics(&[Topic::Settings]));
    // Settled within a check or two (the watcher's own opens).
    assert!((0..3).any(|_| watcher.check().unwrap() == Check::Unchanged));
    let opens = watcher.opens;

    // The checkpoint moves the frames from `-wal` into the database file:
    // the files change, the data doesn't.
    held.conn()
        .query_row("PRAGMA wal_checkpoint(TRUNCATE)", [], |_| Ok(()))
        .unwrap();
    assert_ne!(
        Stamp::read(&path).unwrap(),
        watcher.seen.as_ref().map(|seen| seen.stamp),
        "the checkpoint is a stamp change"
    );
    assert_eq!(watcher.check().unwrap(), topics(&[]));
    assert_eq!(watcher.opens, opens + 1);
    assert_eq!(watcher.check().unwrap(), Check::Unchanged);
}

#[test]
fn a_write_while_the_check_reads_is_reported_at_the_next_check() {
    let dir = TempDir::new().unwrap();
    let (path, mut watcher) = watched(&dir);
    write(
        &path,
        "INSERT INTO mcp_client (name) VALUES ('claude-code')",
    );
    // A write lands after the check's queries, before it reads the stamp
    // again: the check can't report it, and must not absorb it.
    watcher.between = Some(|path: &Path| {
        write(
            path,
            "INSERT INTO setting (key, value) VALUES ('test.key', '1')",
        );
    });
    assert_eq!(watcher.check().unwrap(), topics(&[Topic::Settings]));
    watcher.between = None;

    assert_eq!(watcher.check().unwrap(), topics(&[Topic::Settings]));
    // Then it settles: a quiet database costs nothing again.
    assert_eq!(watcher.check().unwrap(), Check::Unchanged);
    let opens = watcher.opens;
    assert_eq!(watcher.check().unwrap(), Check::Unchanged);
    assert_eq!(watcher.opens, opens);
}

#[test]
fn a_transfer_decided_and_a_report_deleted_are_seen() {
    let dir = TempDir::new().unwrap();
    let (path, mut watcher) = watched(&dir);
    write(
        &path,
        "INSERT INTO transfer_group (status) VALUES ('suggested')",
    );
    assert_eq!(watcher.check().unwrap(), topics(&[Topic::Ledger]));
    // An Inbox approval of a transfer pair records no change batch.
    write(&path, "UPDATE transfer_group SET status = 'confirmed'");
    assert_eq!(watcher.check().unwrap(), topics(&[Topic::Ledger]));

    write(
        &path,
        "INSERT INTO report (id, period, title, body_markdown, source_client) \
         VALUES (1, '2026-10', 'a', 'b', 'claude-code'), \
                (2, '2026-10', 'c', 'd', 'claude-code')",
    );
    assert_eq!(watcher.check().unwrap(), topics(&[Topic::Inbox]));
    write(&path, "DELETE FROM report WHERE id = 1");
    assert_eq!(watcher.check().unwrap(), topics(&[Topic::Inbox]));
}

#[test]
fn rebasing_every_topic_after_a_restore_takes_the_new_file() {
    let dir = TempDir::new().unwrap();
    let (path, mut watcher) = watched(&dir);
    let backup = {
        let db = Database::open(&path).unwrap();
        backup::create(&db, Reason::Manual).unwrap().path
    };
    backup::restore(&path, &backup).unwrap();
    // The app restored, and rebases before its views read again.
    watcher.rebase(&Topic::ALL).unwrap();
    assert_eq!(watcher.check().unwrap(), topics(&[]));

    // A later write is reported as usual.
    write(
        &path,
        "INSERT INTO mcp_client (name) VALUES ('claude-code')",
    );
    assert_eq!(watcher.check().unwrap(), topics(&[Topic::Settings]));
}

#[test]
fn a_restore_reports_every_topic() {
    let dir = TempDir::new().unwrap();
    let (path, mut watcher) = watched(&dir);
    let backup = {
        let db = Database::open(&path).unwrap();
        backup::create(&db, Reason::Manual).unwrap().path
    };
    write(
        &path,
        "INSERT INTO mcp_client (name) VALUES ('claude-code')",
    );
    assert_eq!(watcher.check().unwrap(), topics(&[Topic::Settings]));

    backup::restore(&path, &backup).unwrap();
    assert_eq!(watcher.check().unwrap(), topics(&Topic::ALL));
    assert_eq!(watcher.check().unwrap(), Check::Unchanged);
}

#[test]
fn a_file_swapped_in_has_a_new_identity() {
    let dir = TempDir::new().unwrap();
    let (path, mut watcher) = watched(&dir);
    let copy = dir.path().join("copy.db");
    fs::copy(&path, &copy).unwrap();
    for suffix in ["-wal", "-shm"] {
        let _ = fs::remove_file(db::sidecar(&path, suffix));
    }
    fs::rename(&copy, &path).unwrap();

    assert_eq!(watcher.check().unwrap(), topics(&Topic::ALL));
}

#[test]
fn a_held_restore_lock_skips_the_check_and_the_next_one_catches_up() {
    let dir = TempDir::new().unwrap();
    let (path, mut watcher) = watched(&dir);

    write(
        &path,
        "INSERT INTO mcp_client (name) VALUES ('claude-code')",
    );
    let lock = hold_restore_lock(&path);
    assert_eq!(watcher.check().unwrap(), Check::Skipped);
    assert_eq!(watcher.check().unwrap(), Check::Skipped);
    assert!(Check::Skipped.topics().is_empty());

    drop(lock);
    assert_eq!(watcher.check().unwrap(), topics(&[Topic::Settings]));
    assert_eq!(watcher.check().unwrap(), Check::Unchanged);
}

#[test]
fn a_database_that_cannot_be_opened_is_an_error_and_is_retried() {
    let dir = TempDir::new().unwrap();
    let (path, mut watcher) = watched(&dir);
    let good = fs::read(&path).unwrap();

    fs::write(&path, b"").unwrap();
    assert!(matches!(watcher.check(), Err(Error::Open(_))));

    // Put the database back: the check that failed stored nothing.
    fs::write(&path, good).unwrap();
    assert!(matches!(watcher.check(), Ok(Check::Changed(_))));
}
