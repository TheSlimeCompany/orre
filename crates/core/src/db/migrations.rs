//! Versioned migrations: the SQL files in `crates/core/migrations/`,
//! embedded at compile time and applied by `refinery`, which records each
//! one with a checksum in `refinery_schema_history`.

use refinery::{Migration, Runner};
use rusqlite::Connection;

use super::{Error, HistoryProblem, SchemaVersion};

mod embedded {
    refinery::embed_migrations!("migrations");
}

/// refinery's default history table.
const HISTORY_TABLE: &str = "refinery_schema_history";

/// The migrations built into this binary, in version order. refinery
/// embeds them in file-name order, where `V10__…` comes before `V1__…`;
/// it sorts by version itself only when it runs them.
pub(super) fn embedded() -> Vec<Migration> {
    let mut migrations = embedded::migrations::runner().get_migrations().clone();
    migrations.sort();
    migrations
}

/// The highest version in `migrations`, or 0 when there are none.
pub(super) fn latest_version(migrations: &[Migration]) -> SchemaVersion {
    migrations
        .iter()
        .map(|migration| SchemaVersion::from(migration.version()))
        .max()
        .unwrap_or(0)
}

/// The last version applied to the database, or 0 when none has been.
pub(super) fn current_version(conn: &Connection) -> rusqlite::Result<SchemaVersion> {
    if !has_history(conn)? {
        return Ok(0);
    }
    conn.query_row(
        &format!("SELECT COALESCE(MAX(version), 0) FROM {HISTORY_TABLE}"),
        [],
        |row| row.get(0),
    )
}

/// The first line of a migration that needs foreign keys off: one that
/// rebuilds a table other tables reference (SQLite's 12-step procedure).
/// `PRAGMA foreign_keys` can't change inside a transaction, so [`run`]
/// turns them off around the whole run.
pub(super) const FOREIGN_KEYS_OFF: &str = "-- coffer: foreign_keys=off";

/// The temporary trigger that checks foreign keys before the run commits.
const FOREIGN_KEY_CHECK_TRIGGER: &str = "coffer_migration_foreign_key_check";

/// Whether `migration` asks for foreign keys off ([`FOREIGN_KEYS_OFF`]).
fn needs_foreign_keys_off(migration: &Migration) -> bool {
    migration
        .sql()
        .and_then(|sql| sql.lines().next())
        .is_some_and(|line| line.trim_end() == FOREIGN_KEYS_OFF)
}

/// Applies every pending migration in one transaction.
///
/// Refuses a database from a newer Coffer, and (through refinery) one whose
/// applied migrations differ from `migrations`: an edited, missing or
/// skipped migration.
///
/// When a pending migration needs foreign keys off ([`FOREIGN_KEYS_OFF`]),
/// they are off for the whole run, and every foreign key in the database
/// is checked inside its transaction, just before it commits: a violation
/// fails the run and rolls every pending migration back. Foreign keys are on
/// again afterwards, whatever happened.
pub(super) fn run(conn: &mut Connection, migrations: &[Migration]) -> Result<(), Error> {
    refuse_newer(conn, migrations)?;
    let current = current_version(conn)?;
    let foreign_keys_off = migrations.iter().any(|migration| {
        SchemaVersion::from(migration.version()) > current && needs_foreign_keys_off(migration)
    });
    if !foreign_keys_off {
        return apply(conn, migrations);
    }

    // The checks refinery makes, before anything here writes.
    check(conn, migrations)?;
    // Outside any transaction: there the pragma does nothing.
    conn.pragma_update(None, "foreign_keys", false)?;
    let result =
        check_foreign_keys_before_commit(conn, migrations).and_then(|()| apply(conn, migrations));
    let cleanup = conn
        .execute_batch(&format!(
            "DROP TRIGGER IF EXISTS temp.{FOREIGN_KEY_CHECK_TRIGGER}"
        ))
        .and_then(|()| conn.pragma_update(None, "foreign_keys", true));
    result?;
    cleanup?;
    Ok(())
}

/// Makes the run fail, inside its transaction, if the database has a
/// foreign key violation once the last migration has run: refinery records
/// each migration in its history table in the same transaction as the
/// migration, so a temporary trigger on that table checks every foreign key
/// when the last one is recorded (`pragma_foreign_key_check` reports
/// violations whether or not foreign keys are on).
fn check_foreign_keys_before_commit(
    conn: &mut Connection,
    migrations: &[Migration],
) -> Result<(), Error> {
    <Connection as refinery::Migrate>::assert_migrations_table(conn, HISTORY_TABLE)
        .map_err(|err| Error::Migration(Box::new(err)))?;
    let last = latest_version(migrations);
    conn.execute_batch(&format!(
        "CREATE TEMP TRIGGER {FOREIGN_KEY_CHECK_TRIGGER} \
         BEFORE INSERT ON main.{HISTORY_TABLE} \
         WHEN NEW.version = {last} AND EXISTS (SELECT 1 FROM pragma_foreign_key_check) \
         BEGIN \
             SELECT RAISE(ABORT, 'the migrations leave a foreign key that points at nothing'); \
         END"
    ))?;
    Ok(())
}

/// Runs refinery over `migrations`, all pending ones in one transaction.
fn apply(conn: &mut Connection, migrations: &[Migration]) -> Result<(), Error> {
    let runner = Runner::new(migrations).set_grouped(true);
    let report = match runner.run(conn) {
        Ok(report) => report,
        // Two processes opening a fresh database at once can both see the
        // same migrations as pending; the second's apply then fails once the
        // first has committed. Running again finds nothing left to do. A real
        // failure fails the same way again, and its transaction rolls back.
        Err(_) => {
            refuse_newer(conn, migrations)?;
            runner
                .run(conn)
                .map_err(|err| Error::Migration(Box::new(err)))?
        }
    };

    for migration in report.applied_migrations() {
        tracing::info!(%migration, "applied database migration");
    }
    Ok(())
}

/// Checks, without writing, that the database may be used with
/// `migrations`, and returns its schema version.
///
/// Refuses a database from a newer Coffer ([`Error::SchemaTooNew`]) and one
/// whose recorded history differs from `migrations`
/// ([`Error::MigrationHistory`]): the same checks refinery makes when it
/// runs, but without creating its history table, so it works on a database
/// that must not be written. Assumes versioned migrations only (see the
/// `embedded_migrations_are_numbered_without_gaps` test).
pub(super) fn check(conn: &Connection, migrations: &[Migration]) -> Result<SchemaVersion, Error> {
    refuse_newer(conn, migrations)?;
    let applied = applied(conn)?;

    for (version, name, checksum) in &applied {
        let problem = match migrations
            .iter()
            .find(|migration| SchemaVersion::from(migration.version()) == *version)
        {
            None => HistoryProblem::Unknown,
            Some(migration)
                if migration.name() != name
                    || checksum.parse::<u64>().ok() != Some(migration.checksum()) =>
            {
                HistoryProblem::Changed
            }
            Some(_) => continue,
        };
        return Err(Error::MigrationHistory {
            version: *version,
            problem,
        });
    }

    let current = applied
        .iter()
        .map(|(version, ..)| *version)
        .max()
        .unwrap_or(0);
    for migration in migrations {
        let version = SchemaVersion::from(migration.version());
        if version <= current && !applied.iter().any(|(applied, ..)| *applied == version) {
            return Err(Error::MigrationHistory {
                version,
                problem: HistoryProblem::Skipped,
            });
        }
    }
    Ok(current)
}

/// The recorded migrations as `(version, name, checksum)`; none when the
/// history table doesn't exist.
fn applied(conn: &Connection) -> rusqlite::Result<Vec<(SchemaVersion, String, String)>> {
    if !has_history(conn)? {
        return Ok(Vec::new());
    }
    let mut statement = conn.prepare(&format!(
        "SELECT version, name, checksum FROM {HISTORY_TABLE}"
    ))?;
    statement
        .query_map([], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)))?
        .collect()
}

fn has_history(conn: &Connection) -> rusqlite::Result<bool> {
    conn.query_row(
        "SELECT EXISTS (SELECT 1 FROM sqlite_schema WHERE type = 'table' AND name = ?1)",
        [HISTORY_TABLE],
        |row| row.get(0),
    )
}

fn refuse_newer(conn: &Connection, migrations: &[Migration]) -> Result<(), Error> {
    let found = current_version(conn)?;
    let supported = latest_version(migrations);
    if found > supported {
        return Err(Error::SchemaTooNew { found, supported });
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every embedded migration is versioned (`V`), and versions run 1, 2, 3…
    /// with no gaps, so "the last applied version" is a complete description
    /// of a schema.
    #[test]
    fn embedded_migrations_are_numbered_without_gaps() {
        let migrations = embedded();
        for (index, migration) in migrations.iter().enumerate() {
            assert_eq!(
                SchemaVersion::from(migration.version()),
                index as SchemaVersion + 1,
                "{migration}"
            );
            // refinery doesn't export its migration type enum.
            assert_eq!(
                migration.prefix().to_string(),
                "V",
                "{migration} must be a V migration"
            );
        }
    }

    #[test]
    fn embedded_migrations_apply_to_an_empty_database() {
        let mut conn = Connection::open_in_memory().unwrap();
        let migrations = embedded();

        run(&mut conn, &migrations).unwrap();

        assert_eq!(current_version(&conn).unwrap(), latest_version(&migrations));
        assert_eq!(
            check(&conn, &migrations).unwrap(),
            latest_version(&migrations)
        );
    }

    fn migration(name: &str, sql: &str) -> Migration {
        Migration::unapplied(name, sql).unwrap()
    }

    fn v1() -> Migration {
        migration("V1__first", "CREATE TABLE first (id INTEGER PRIMARY KEY);")
    }

    fn v2() -> Migration {
        migration("V2__second", "CREATE TABLE second (id INTEGER);")
    }

    fn v3() -> Migration {
        migration("V3__third", "CREATE TABLE third (id INTEGER);")
    }

    fn migrated(migrations: &[Migration]) -> Connection {
        let mut conn = Connection::open_in_memory().unwrap();
        run(&mut conn, migrations).unwrap();
        conn
    }

    fn history_problem(result: Result<SchemaVersion, Error>) -> (SchemaVersion, HistoryProblem) {
        match result {
            Err(Error::MigrationHistory { version, problem }) => (version, problem),
            other => panic!("expected a history error, got {other:?}"),
        }
    }

    #[test]
    fn check_accepts_a_matching_or_older_history() {
        let conn = migrated(&[v1()]);
        assert_eq!(check(&conn, &[v1()]).unwrap(), 1);
        assert_eq!(check(&conn, &[v1(), v2()]).unwrap(), 1);
        assert_eq!(
            check(&Connection::open_in_memory().unwrap(), &[v1()]).unwrap(),
            0
        );
    }

    #[test]
    fn check_refuses_a_newer_schema() {
        let conn = migrated(&[v1(), v2()]);
        assert!(matches!(
            check(&conn, &[v1()]),
            Err(Error::SchemaTooNew {
                found: 2,
                supported: 1
            })
        ));
    }

    #[test]
    fn check_refuses_an_edited_or_renamed_migration() {
        let conn = migrated(&[v1(), v2()]);
        let edited = migration("V2__second", "CREATE TABLE second (id TEXT);");
        let renamed = migration("V2__renamed", "CREATE TABLE second (id INTEGER);");

        assert_eq!(
            history_problem(check(&conn, &[v1(), edited])),
            (2, HistoryProblem::Changed)
        );
        assert_eq!(
            history_problem(check(&conn, &[v1(), renamed])),
            (2, HistoryProblem::Changed)
        );
    }

    #[test]
    fn check_refuses_an_unknown_or_skipped_migration() {
        let conn = migrated(&[v1(), v3()]);
        assert_eq!(
            history_problem(check(&conn, &[v1(), v2(), v3()])),
            (2, HistoryProblem::Skipped)
        );

        let conn = migrated(&[v1(), v2(), v3()]);
        assert_eq!(
            history_problem(check(&conn, &[v1(), v3()])),
            (2, HistoryProblem::Unknown)
        );
    }

    fn parent_and_child() -> Migration {
        migration(
            "V1__parent_and_child",
            "CREATE TABLE parent (id INTEGER PRIMARY KEY, name TEXT);
             CREATE TABLE child (id INTEGER PRIMARY KEY, parent_id INTEGER REFERENCES parent (id));
             INSERT INTO parent (id, name) VALUES (1, 'a'), (2, 'b');
             INSERT INTO child (id, parent_id) VALUES (1, 1), (2, 2);",
        )
    }

    /// Rebuilds `parent`, which `child` references: only possible with
    /// foreign keys off.
    fn rebuild_parent(sql_after: &str) -> Migration {
        migration(
            "V2__rebuild_parent",
            &format!(
                "{FOREIGN_KEYS_OFF}
                 CREATE TABLE parent_new (id INTEGER PRIMARY KEY, name TEXT NOT NULL);
                 INSERT INTO parent_new (id, name) SELECT id, name FROM parent;
                 DROP TABLE parent;
                 ALTER TABLE parent_new RENAME TO parent;
                 {sql_after}"
            ),
        )
    }

    fn foreign_keys_on(conn: &Connection) -> bool {
        conn.query_row("PRAGMA foreign_keys", [], |row| row.get(0))
            .unwrap()
    }

    fn count(conn: &Connection, table: &str) -> i64 {
        conn.query_row(&format!("SELECT count(*) FROM {table}"), [], |row| {
            row.get(0)
        })
        .unwrap()
    }

    #[test]
    fn a_migration_asks_for_foreign_keys_off_on_its_first_line() {
        assert!(needs_foreign_keys_off(&rebuild_parent("")));
        assert!(!needs_foreign_keys_off(&parent_and_child()));
        let later = migration(
            "V3__later",
            &format!("SELECT 1;\n{FOREIGN_KEYS_OFF}\nSELECT 2;"),
        );
        assert!(!needs_foreign_keys_off(&later));
        let superseded = embedded()
            .into_iter()
            .find(|migration| migration.name() == "superseded")
            .unwrap();
        assert!(needs_foreign_keys_off(&superseded));
    }

    /// The rebuild runs with foreign keys off, with every pending migration
    /// in one transaction, and they are on again afterwards.
    #[test]
    fn a_rebuild_runs_with_foreign_keys_off_and_turns_them_back_on() {
        let mut conn = Connection::open_in_memory().unwrap();
        conn.pragma_update(None, "foreign_keys", true).unwrap();
        run(&mut conn, &[parent_and_child()]).unwrap();

        run(&mut conn, &[parent_and_child(), rebuild_parent("")]).unwrap();

        assert_eq!(current_version(&conn).unwrap(), 2);
        assert!(foreign_keys_on(&conn));
        assert_eq!(count(&conn, "parent"), 2);
        assert_eq!(count(&conn, "child"), 2);
        let refused = conn.execute("DELETE FROM parent WHERE id = 1", []);
        assert!(refused.is_err(), "{refused:?}");

        // From an empty database too: both in one run.
        let mut fresh = Connection::open_in_memory().unwrap();
        fresh.pragma_update(None, "foreign_keys", true).unwrap();
        run(&mut fresh, &[parent_and_child(), rebuild_parent("")]).unwrap();
        assert_eq!(current_version(&fresh).unwrap(), 2);
        assert!(foreign_keys_on(&fresh));
    }

    /// A rebuild that leaves a reference to nothing fails, and the database
    /// stays at its previous version with every row.
    #[test]
    fn a_foreign_key_violation_rolls_the_run_back() {
        let mut conn = Connection::open_in_memory().unwrap();
        conn.pragma_update(None, "foreign_keys", true).unwrap();
        run(&mut conn, &[parent_and_child()]).unwrap();

        let error = run(
            &mut conn,
            &[
                parent_and_child(),
                rebuild_parent("DELETE FROM parent WHERE id = 2;"),
                v3(),
            ],
        )
        .unwrap_err();

        assert!(matches!(error, Error::Migration(_)), "{error:?}");
        assert_eq!(current_version(&conn).unwrap(), 1);
        assert!(foreign_keys_on(&conn));
        assert_eq!(count(&conn, "parent"), 2);
        let third: bool = conn
            .query_row(
                "SELECT EXISTS (SELECT 1 FROM sqlite_schema WHERE name = 'third')",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert!(!third);
        // The check's trigger is gone with the run.
        let triggers: i64 = conn
            .query_row("SELECT count(*) FROM temp.sqlite_schema", [], |row| {
                row.get(0)
            })
            .unwrap();
        assert_eq!(triggers, 0);
    }

    /// A newer or diverged database is refused before the rebuild's run
    /// writes anything.
    #[test]
    fn a_rebuild_is_refused_on_a_diverged_history_before_writing() {
        let mut conn = Connection::open_in_memory().unwrap();
        conn.pragma_update(None, "foreign_keys", true).unwrap();
        run(&mut conn, &[parent_and_child()]).unwrap();
        let edited = migration("V1__parent_and_child", "CREATE TABLE other (id INTEGER);");

        let error = run(&mut conn, &[edited, rebuild_parent("")]).unwrap_err();

        assert!(
            matches!(
                error,
                Error::MigrationHistory {
                    version: 1,
                    problem: HistoryProblem::Changed
                }
            ),
            "{error:?}"
        );
        assert!(foreign_keys_on(&conn));
        assert_eq!(current_version(&conn).unwrap(), 1);
    }

    /// `check` agrees with refinery: whatever it accepts, `run` accepts.
    #[test]
    fn run_accepts_what_check_accepts() {
        let mut conn = migrated(&[v1()]);
        check(&conn, &[v1(), v2()]).unwrap();
        run(&mut conn, &[v1(), v2()]).unwrap();
        assert_eq!(current_version(&conn).unwrap(), 2);
    }
}
