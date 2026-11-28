//! A restore compares the backup's schema with what this build's
//! migrations create at the backup's version, and refuses any difference
//! (SLI-50): a trigger or view a backup brings along would otherwise run in
//! the live database after the restore.

use super::*;

/// A copy of the setup's backup with `sql` run on it.
fn tampered(setup: &Setup, name: &str, sql: &str) -> PathBuf {
    let copy = setup.copy_of_backup(name);
    Connection::open(&copy).unwrap().execute_batch(sql).unwrap();
    copy
}

/// Restores `backup` over the setup's database, expects the schema error
/// for `object_type` `name`, and checks nothing changed. Returns the
/// message.
fn refused(setup: &Setup, backup: &Path, expected: SchemaProblem, object: (&str, &str)) -> String {
    let before = fs::read(&setup.path).unwrap();

    let err = restore(&setup.path, backup).unwrap_err();

    match &err {
        Error::Schema {
            problem,
            object_type,
            name,
            version,
            ..
        } => {
            assert_eq!(*problem, expected, "{err}");
            assert_eq!((object_type.as_str(), name.as_str()), object, "{err}");
            assert_eq!(*version, Database::supported_schema_version());
        }
        other => panic!("expected a schema error, got {other:?}"),
    }
    setup.assert_untouched(&before, 1);
    assert!(matches!(
        check(&setup.path, backup),
        Err(Error::Schema { .. })
    ));
    setup.assert_untouched(&before, 1);
    err.to_string()
}

#[test]
fn a_backup_with_an_extra_trigger_is_refused() {
    let setup = Setup::new();
    let cases = [
        (
            "client.db",
            "CREATE TRIGGER evil AFTER UPDATE ON mcp_client \
             BEGIN UPDATE mcp_client SET enabled = 1, access_level = 'standard'; END;",
        ),
        (
            "setting.db",
            "CREATE TRIGGER evil AFTER DELETE ON setting \
             BEGIN INSERT OR REPLACE INTO setting (key, value) VALUES ('mcp.enabled', 'on'); END;",
        ),
        // Fires on the first refused call after the restore, and widens AI
        // access again.
        (
            "log.db",
            "CREATE TRIGGER evil AFTER INSERT ON mcp_access_log BEGIN \
             UPDATE mcp_client SET enabled = 1, access_level = 'standard'; \
             INSERT OR REPLACE INTO setting (key, value) VALUES ('mcp.enabled', 'on'); END;",
        ),
    ];
    for (file, sql) in cases {
        let backup = tampered(&setup, file, sql);
        let message = refused(&setup, &backup, SchemaProblem::Extra, ("trigger", "evil"));
        assert!(
            message.contains(r#"has an extra trigger "evil""#),
            "{message}"
        );
        assert!(
            message.contains("nothing was restored and the current database is unchanged"),
            "{message}"
        );
        // The trigger's SQL isn't shown.
        assert!(!message.contains("UPDATE"), "{message}");
        assert!(!message.contains("access_level"), "{message}");
    }
}

#[test]
fn a_backup_with_an_extra_view_is_refused() {
    let setup = Setup::new();
    let backup = tampered(
        &setup,
        "view.db",
        "CREATE VIEW peek AS SELECT * FROM setting;",
    );
    refused(&setup, &backup, SchemaProblem::Extra, ("view", "peek"));
}

#[test]
fn a_backup_with_an_extra_table_or_index_is_refused() {
    let setup = Setup::new();
    let backup = tampered(&setup, "table.db", "CREATE TABLE stash (x TEXT);");
    refused(&setup, &backup, SchemaProblem::Extra, ("table", "stash"));
    let backup = tampered(
        &setup,
        "index.db",
        "CREATE INDEX setting_value ON setting (value);",
    );
    refused(
        &setup,
        &backup,
        SchemaProblem::Extra,
        ("index", "setting_value"),
    );
}

#[test]
fn a_backup_with_a_changed_trigger_is_refused() {
    let setup = Setup::new();
    // Same name and table, a body that no longer guards the key.
    let backup = tampered(
        &setup,
        "changed.db",
        "DROP TRIGGER category_read_only;
         CREATE TRIGGER category_read_only BEFORE UPDATE ON category
         WHEN 0 BEGIN SELECT RAISE(ABORT, 'never'); END;",
    );
    refused(
        &setup,
        &backup,
        SchemaProblem::Changed,
        ("trigger", "category_read_only"),
    );
}

#[test]
fn a_backup_missing_a_trigger_is_refused() {
    let setup = Setup::new();
    let backup = tampered(&setup, "missing.db", "DROP TRIGGER category_read_only;");
    refused(
        &setup,
        &backup,
        SchemaProblem::Missing,
        ("trigger", "category_read_only"),
    );
}

#[test]
fn a_clean_backup_at_the_current_version_restores() {
    let setup = Setup::new();
    let restored = restore(&setup.path, &setup.backup).unwrap();
    assert_eq!(restored.from_version, Database::supported_schema_version());
    assert_eq!(account_names(&setup.path), ["Backed up"]);
}

/// The strongest check that the comparison never refuses a real backup:
/// for every embedded version, a database built through the real migration
/// path, holding a row, backed up the way Orre backs up and restored. The
/// backup and the reference are both written by the SQLite bundled with
/// this build, including FTS5's shadow tables from V24 on.
#[test]
fn a_backup_made_by_this_build_at_every_version_restores() {
    let latest = Database::supported_schema_version();
    let mut with_full_text = 0;
    for version in 1..=latest {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("old").join("orre.db");
        {
            let db = db::open_at_version(&path, version).unwrap();
            db.conn()
                .execute(
                    "INSERT INTO account (name, kind, currency) VALUES ('Old', 'current', 'GBP')",
                    [],
                )
                .unwrap();
        }
        let backup = create_from_path(&path, Reason::Manual).unwrap();
        let target = db_path(&dir);

        let restored =
            restore(&target, &backup.path).unwrap_or_else(|err| panic!("version {version}: {err}"));

        assert_eq!(restored.from_version, version);
        assert_eq!(account_names(&target), ["Old"], "version {version}");
        if !shadow_names(&backup.path).is_empty() {
            assert!(version >= 24, "version {version}");
            with_full_text += 1;
        }
    }
    assert_eq!(
        with_full_text,
        latest - 23,
        "the full-text index was exercised"
    );
}

// FTS5's shadow tables: their `CREATE` text is written by SQLite's fts5
// module, not by V24. Each must be a plain `CREATE TABLE` in the
// reference's form (the text fts5 writes for that suffix); only the quotes
// around the table name may differ, and a virtual table by that name is
// refused.

/// The names of the full-text index's shadow tables in the file at `path`.
fn shadow_names(path: &Path) -> Vec<String> {
    let conn = Connection::open(path).unwrap();
    let mut statement = conn
        .prepare(
            "SELECT name FROM sqlite_master \
             WHERE type = 'table' AND name LIKE 'transaction\\_text\\_%' ESCAPE '\\' \
             ORDER BY name",
        )
        .unwrap();
    statement
        .query_map([], |row| row.get(0))
        .unwrap()
        .collect::<Result<_, _>>()
        .unwrap()
}

#[test]
fn a_shadow_table_whose_sql_reads_differently_is_accepted() {
    let setup = Setup::new();
    assert_eq!(
        shadow_names(&setup.backup),
        [
            "transaction_text_config",
            "transaction_text_data",
            "transaction_text_docsize",
            "transaction_text_idx"
        ]
    );
    // The same table with its name double-quoted, the one difference the
    // check allows in fts5's form.
    let backup = setup.copy_of_backup("requoted.db");
    {
        let conn = Connection::open(&backup).unwrap();
        conn.execute_batch(
            "PRAGMA writable_schema = ON;
             UPDATE sqlite_master
                SET sql = 'CREATE TABLE \"transaction_text_config\"(k PRIMARY KEY, v) WITHOUT ROWID'
              WHERE name = 'transaction_text_config';
             PRAGMA writable_schema = OFF;",
        )
        .unwrap();
    }
    let conn = Connection::open(&backup).unwrap();
    let sql: String = conn
        .query_row(
            "SELECT sql FROM sqlite_master WHERE name = 'transaction_text_config'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert!(sql.contains(r#""transaction_text_config""#), "{sql}");
    let integrity: String = conn
        .query_row("PRAGMA integrity_check", [], |row| row.get(0))
        .unwrap();
    assert_eq!(integrity, "ok");
    drop(conn);

    restore(&setup.path, &backup).unwrap();

    assert_eq!(account_names(&setup.path), ["Backed up"]);
}

#[test]
fn an_extra_table_named_like_a_shadow_table_is_refused() {
    let setup = Setup::new();
    for name in ["transaction_text_extra", "transaction_text_data2"] {
        let backup = tampered(
            &setup,
            &format!("{name}.db"),
            &format!("CREATE TABLE {name} (x TEXT);"),
        );
        refused(&setup, &backup, SchemaProblem::Extra, ("table", name));
    }
}

#[test]
fn a_missing_shadow_table_is_refused() {
    let setup = Setup::new();
    let backup = tampered(
        &setup,
        "no-docsize.db",
        "DROP TABLE transaction_text_docsize;",
    );
    refused(
        &setup,
        &backup,
        SchemaProblem::Missing,
        ("table", "transaction_text_docsize"),
    );
}

#[test]
fn a_changed_full_text_table_is_refused() {
    let setup = Setup::new();
    // Another tokenizer: the virtual table's own SQL is the migration's,
    // and compared in full.
    let backup = tampered(
        &setup,
        "tokenizer.db",
        "DROP TABLE transaction_text;
         CREATE VIRTUAL TABLE transaction_text USING fts5 (
             merchant_display, merchant, description_raw,
             content = 'transaction', content_rowid = 'id',
             tokenize = 'unicode61'
         );",
    );
    refused(
        &setup,
        &backup,
        SchemaProblem::Changed,
        ("table", "transaction_text"),
    );
}

// The SQL is compared exactly (SLI-50 review): a compare that folded
// whitespace let these through.

/// A copy of the setup's backup with `from` replaced by `to` in the stored
/// SQL of the object `name`, written with `writable_schema`.
fn rewritten(setup: &Setup, file: &str, name: &str, from: &str, to: &str) -> PathBuf {
    let copy = setup.copy_of_backup(file);
    let conn = Connection::open(&copy).unwrap();
    conn.execute_batch("PRAGMA writable_schema = ON;").unwrap();
    let updated = conn
        .execute(
            "UPDATE sqlite_master SET sql = replace(sql, ?2, ?3) \
             WHERE name = ?1 AND instr(sql, ?2) > 0",
            rusqlite::params![name, from, to],
        )
        .unwrap();
    assert_eq!(updated, 1, "{name}: {from:?} not found");
    conn.execute_batch("PRAGMA writable_schema = OFF;").unwrap();
    copy
}

/// A newline after a `--` comment turned into a space: the comment now
/// runs on over the pot `CHECK`, so a pot without a parent would be
/// allowed. Folding whitespace made the two texts the same.
#[test]
fn a_comment_that_hides_a_check_is_refused() {
    let setup = Setup::new();
    let backup = rewritten(
        &setup,
        "comment.db",
        "account",
        "(triggers under \"Account structure\").\n    CHECK ((kind = 'pot')",
        "(triggers under \"Account structure\"). CHECK ((kind = 'pot')",
    );
    refused(
        &setup,
        &backup,
        SchemaProblem::Changed,
        ("table", "account"),
    );
}

/// A no-break space (U+00A0) where the migrations have a space. SQLite
/// reads it as part of a word, so it only loads inside a string or a
/// comment: here a trigger's message and a comment in the view.
#[test]
fn a_no_break_space_in_a_trigger_or_view_is_refused() {
    let setup = Setup::new();
    let backup = rewritten(
        &setup,
        "nbsp-trigger.db",
        "category_read_only",
        "a category''s key",
        "a\u{a0}category''s key",
    );
    refused(
        &setup,
        &backup,
        SchemaProblem::Changed,
        ("trigger", "category_read_only"),
    );
    let backup = rewritten(
        &setup,
        "nbsp-view.db",
        "ledger_line",
        "-- 1. A transaction",
        "-- 1.\u{a0}A transaction",
    );
    refused(
        &setup,
        &backup,
        SchemaProblem::Changed,
        ("view", "ledger_line"),
    );
}

/// No index has a string or a comment, so a no-break space in one is part
/// of a name, and the schema doesn't load: the integrity check refuses it
/// before the schema is compared.
#[test]
fn a_no_break_space_in_an_index_doesnt_load() {
    let setup = Setup::new();
    let backup = rewritten(
        &setup,
        "nbsp-index.db",
        "account_parent",
        "ON account (parent_id)",
        "ON account\u{a0}(parent_id)",
    );
    let before = fs::read(&setup.path).unwrap();
    let err = restore(&setup.path, &backup).unwrap_err();
    assert!(matches!(err, Error::Integrity { .. }), "{err:?}");
    setup.assert_untouched(&before, 1);
}

/// A shadow table's name with another kind of table behind it.
#[test]
fn a_shadow_table_of_another_type_is_refused() {
    let setup = Setup::new();
    let backup = setup.copy_of_backup("dbstat.db");
    {
        let conn = Connection::open(&backup).unwrap();
        conn.execute_batch(
            "DROP TABLE transaction_text_docsize;
             PRAGMA writable_schema = ON;
             INSERT INTO sqlite_master (type, name, tbl_name, rootpage, sql)
             VALUES ('table', 'transaction_text_docsize', 'transaction_text_docsize', 0,
                     'CREATE VIRTUAL TABLE transaction_text_docsize USING dbstat');
             PRAGMA writable_schema = OFF;",
        )
        .unwrap();
    }
    refused(
        &setup,
        &backup,
        SchemaProblem::Changed,
        ("table", "transaction_text_docsize"),
    );
}

/// Only `sqlite_stat1` and `sqlite_stat4` as `ANALYZE` writes them are left
/// out of the comparison.
#[test]
fn statistics_tables_are_left_out_only_as_analyze_writes_them() {
    let setup = Setup::new();
    // Any other `sqlite_stat` name (made with `writable_schema`: SQLite
    // keeps `sqlite_` names to itself).
    let backup = setup.copy_of_backup("stat9.db");
    Connection::open(&backup)
        .unwrap()
        .execute_batch(
            "CREATE TABLE stat9 (tbl, idx, stat);
             PRAGMA writable_schema = ON;
             UPDATE sqlite_master
                SET name = 'sqlite_stat9', tbl_name = 'sqlite_stat9',
                    sql = 'CREATE TABLE sqlite_stat9(tbl,idx,stat)'
              WHERE name = 'stat9';
             PRAGMA writable_schema = OFF;",
        )
        .unwrap();
    refused(
        &setup,
        &backup,
        SchemaProblem::Extra,
        ("table", "sqlite_stat9"),
    );

    // A virtual table by the name `sqlite_stat1`.
    let backup = setup.copy_of_backup("virtual-stat1.db");
    Connection::open(&backup)
        .unwrap()
        .execute_batch(
            "PRAGMA writable_schema = ON;
             INSERT INTO sqlite_master (type, name, tbl_name, rootpage, sql)
             VALUES ('table', 'sqlite_stat1', 'sqlite_stat1', 0,
                     'CREATE VIRTUAL TABLE sqlite_stat1 USING dbstat');
             PRAGMA writable_schema = OFF;",
        )
        .unwrap();
    refused(
        &setup,
        &backup,
        SchemaProblem::Extra,
        ("table", "sqlite_stat1"),
    );

    // What `ANALYZE` writes is accepted.
    let backup = tampered(&setup, "analyzed.db", "ANALYZE;");
    let names: Vec<String> = Connection::open(&backup)
        .unwrap()
        .prepare("SELECT name FROM sqlite_master WHERE name LIKE 'sqlite_stat%' ORDER BY name")
        .unwrap()
        .query_map([], |row| row.get(0))
        .unwrap()
        .collect::<Result<_, _>>()
        .unwrap();
    assert_eq!(names, ["sqlite_stat1", "sqlite_stat4"]);
    restore(&setup.path, &backup).unwrap();
    assert_eq!(account_names(&setup.path), ["Backed up"]);
}

/// A copy of the setup's backup holding transactions, so the full-text
/// index has something in it, then `sql`.
fn with_transactions(setup: &Setup, file: &str, sql: &str) -> PathBuf {
    tampered(
        setup,
        file,
        &format!(
            "INSERT INTO \"transaction\" (account_id, date, amount_minor, currency, \
                 description_raw, description_normalized, origin) \
             VALUES (1, '2026-11-01', -340, 'GBP', 'SYNTHETIC CAFE', 'SYNTHETIC CAFE', 'manual'), \
                    (1, '2026-11-02', -120, 'GBP', 'SYNTHETIC BAKERY', 'SYNTHETIC BAKERY', 'manual'); \
             {sql}"
        ),
    )
}

/// The full-text index must match the text it indexes: a backup whose
/// index forgot a row (here told to delete it) is refused. SQLite's own
/// integrity check only checks the index against itself.
#[test]
fn a_full_text_index_that_doesnt_match_its_text_is_refused() {
    let setup = Setup::new();
    let healthy = with_transactions(&setup, "healthy.db", "");
    let stray = with_transactions(
        &setup,
        "stray.db",
        "INSERT INTO transaction_text (transaction_text, rowid, merchant_display, merchant, \
             description_raw) \
         SELECT 'delete', id, merchant_display, merchant, description_raw \
         FROM \"transaction\" WHERE description_raw = 'SYNTHETIC CAFE';",
    );
    let before = fs::read(&setup.path).unwrap();

    let err = restore(&setup.path, &stray).unwrap_err();

    assert!(matches!(err, Error::Integrity { .. }), "{err:?}");
    assert!(err.to_string().contains("transaction_text"), "{err}");
    setup.assert_untouched(&before, 1);

    restore(&setup.path, &healthy).unwrap();
    assert_eq!(account_names(&setup.path), ["Backed up"]);
}

/// The header's `user_version` and `application_id` must be the
/// migrations' (Orre sets neither, so both are 0).
#[test]
fn a_backup_with_another_user_version_or_application_id_is_refused() {
    let setup = Setup::new();
    let conn = Connection::open(&setup.backup).unwrap();
    for pragma in ["user_version", "application_id"] {
        let value: i64 = conn
            .pragma_query_value(None, pragma, |row| row.get(0))
            .unwrap();
        assert_eq!(value, 0, "{pragma}");
    }
    drop(conn);
    for pragma in ["user_version", "application_id"] {
        let backup = tampered(
            &setup,
            &format!("{pragma}.db"),
            &format!("PRAGMA {pragma} = 7;"),
        );
        refused(&setup, &backup, SchemaProblem::Changed, ("pragma", pragma));
    }
    restore(&setup.path, &setup.backup).unwrap();
}
