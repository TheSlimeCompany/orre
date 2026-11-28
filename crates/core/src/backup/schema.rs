//! The restore's schema check: a backup's tables, indexes, views and
//! triggers must be exactly those this build's migrations create at the
//! backup's version, its header's `user_version` and `application_id` the
//! same, and its full-text index must match the text it indexes
//! (`settings-and-backups.md` → "Decision record: restore checks the
//! backup's schema"). Anything else would come into the live database with
//! the restore: a trigger there runs on the next write.

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use rusqlite::Connection;

use super::{Error, SchemaProblem};
use crate::db::{self, SchemaVersion};

/// The longest object name [`Error::Schema`] shows, in characters.
const NAME_CAP: usize = 64;

/// A schema's objects: `(type, name)` to `(table, SQL)`, the SQL as stored
/// but for `\r\n` read as `\n`.
type Objects = BTreeMap<(String, String), (String, Option<String>)>;

/// The suffixes of the shadow tables FTS5 creates for a virtual table `V`:
/// `V_data`, `V_idx` and so on.
const FTS5_SHADOW_SUFFIXES: &[&str] = &["data", "idx", "content", "docsize", "config"];

/// The statistics tables `ANALYZE` may add, exactly as SQLite creates them.
/// They hold no code and come and go with the data, so they are left out
/// of the comparison, but only in this form.
const STATISTICS_TABLES: &[(&str, &str)] = &[
    ("sqlite_stat1", "CREATE TABLE sqlite_stat1(tbl,idx,stat)"),
    (
        "sqlite_stat4",
        "CREATE TABLE sqlite_stat4(tbl,idx,neq,nlt,ndlt,sample)",
    ),
];

/// The header values compared with the reference's. Orre sets neither.
const HEADER_PRAGMAS: &[&str] = &["user_version", "application_id"];

/// Refuses the backup (named `backup` in the error) open on `conn` unless
/// its schema is the one the migrations create at `version`. The
/// migrations run on an empty in-memory database, never on the backup.
///
/// - Every object's SQL must be the reference's exactly, but for `\r\n`
///   read as `\n`: the reference ran the same migration text on the same
///   SQLite.
/// - A virtual table's shadow tables (which ones is read from the
///   reference, never from the backup) are written by SQLite's module, not
///   the migrations, so the only difference allowed is the quotes around
///   the table's name.
/// - `sqlite_stat1` and `sqlite_stat4` are left out when they are what
///   `ANALYZE` creates.
/// - `user_version` and `application_id` must be the reference's.
/// - Then each FTS5 table must match the text it indexes (its
///   `integrity-check` command with rank 1), or the backup is refused as
///   [`Error::Integrity`]. SQLite's `PRAGMA integrity_check` checks the
///   index only against itself.
///
/// `conn` must have `trusted_schema` off, as `check_copy`'s has.
pub(super) fn check(conn: &Connection, version: SchemaVersion, backup: &Path) -> Result<(), Error> {
    let reference = db::reference_at_version(version)?;
    let expected = objects(&reference)?;
    let shadows = shadow_tables(&expected);
    let found = objects(conn)?;
    let refused = |problem, object_type: &str, name: &str| Error::Schema {
        path: backup.to_path_buf(),
        version,
        problem,
        object_type: object_type.chars().take(NAME_CAP).collect(),
        name: name.chars().take(NAME_CAP).collect(),
    };

    let keys: BTreeSet<_> = expected.keys().chain(found.keys()).collect();
    for key in keys {
        let problem = match (expected.get(key), found.get(key)) {
            (None, Some(_)) => SchemaProblem::Extra,
            (Some(_), None) => SchemaProblem::Missing,
            (Some(want), Some(have)) => {
                let same = if shadows.contains(key) {
                    want.0 == have.0
                        && quotes_normalised(&want.1, &key.1) == quotes_normalised(&have.1, &key.1)
                } else {
                    want == have
                };
                if same {
                    continue;
                }
                SchemaProblem::Changed
            }
            (None, None) => continue,
        };
        return Err(refused(problem, &key.0, &key.1));
    }

    for pragma in HEADER_PRAGMAS {
        let read =
            |conn: &Connection| conn.pragma_query_value(None, pragma, |row| row.get::<_, i64>(0));
        if read(&reference)? != read(conn)? {
            return Err(refused(SchemaProblem::Changed, "pragma", pragma));
        }
    }

    for table in fts5_tables(&expected) {
        let quoted = format!("\"{}\"", table.replace('"', "\"\""));
        let checked = conn.execute(
            &format!("INSERT INTO {quoted} ({quoted}, rank) VALUES ('integrity-check', 1)"),
            [],
        );
        match checked {
            Ok(_) => {}
            Err(err) if err.sqlite_error_code() == Some(rusqlite::ErrorCode::DatabaseCorrupt) => {
                return Err(Error::Integrity {
                    path: backup.to_path_buf(),
                    count: 1,
                    first: format!("the full-text index {table} doesn't match its text: {err}"),
                });
            }
            Err(err) => return Err(Error::Sqlite(err)),
        }
    }
    Ok(())
}

/// Every row of `sqlite_master` but the statistics tables in their
/// `ANALYZE` form ([`STATISTICS_TABLES`]).
fn objects(conn: &Connection) -> rusqlite::Result<Objects> {
    let mut statement = conn.prepare("SELECT type, name, tbl_name, sql FROM main.sqlite_master")?;
    let rows = statement.query_map([], |row| {
        let sql: Option<String> = row.get(3)?;
        Ok((
            (row.get::<_, String>(0)?, row.get::<_, String>(1)?),
            (row.get(2)?, sql.map(|sql| sql.replace("\r\n", "\n"))),
        ))
    })?;
    let mut objects = Objects::new();
    for row in rows {
        let ((kind, name), (table, sql)) = row?;
        let statistics = kind == "table"
            && STATISTICS_TABLES.iter().any(|(stat, form)| {
                name == *stat && table == *stat && sql.as_deref() == Some(*form)
            });
        if !statistics {
            objects.insert((kind, name), (table, sql));
        }
    }
    Ok(objects)
}

/// Whether `sql` creates a virtual table.
fn is_virtual(sql: &Option<String>) -> bool {
    sql.as_deref().is_some_and(|sql| {
        sql.get(..20)
            .is_some_and(|start| start.eq_ignore_ascii_case("CREATE VIRTUAL TABLE"))
    })
}

/// The shadow tables of the virtual tables in `objects`: each table named
/// `V_<suffix>` for an FTS5 suffix, where `V` is a virtual table there.
fn shadow_tables(objects: &Objects) -> BTreeSet<(String, String)> {
    objects
        .iter()
        .filter(|((kind, _), (_, sql))| kind == "table" && is_virtual(sql))
        .flat_map(|((_, name), _)| {
            FTS5_SHADOW_SUFFIXES
                .iter()
                .map(move |suffix| ("table".to_owned(), format!("{name}_{suffix}")))
        })
        .filter(|key| objects.contains_key(key))
        .collect()
}

/// The FTS5 tables in `objects`.
fn fts5_tables(objects: &Objects) -> Vec<&str> {
    objects
        .iter()
        .filter(|((kind, _), (_, sql))| {
            kind == "table"
                && is_virtual(sql)
                && sql
                    .as_deref()
                    .is_some_and(|sql| sql.to_ascii_lowercase().contains(" using fts5"))
        })
        .map(|((_, name), _)| name.as_str())
        .collect()
}

/// A shadow table's SQL with the quotes around its name made single:
/// fts5 writes `CREATE TABLE 'V_data'(…)`, and the same table may read
/// `CREATE TABLE "V_data"(…)`. Nothing else is changed.
fn quotes_normalised(sql: &Option<String>, name: &str) -> Option<String> {
    let sql = sql.as_deref()?;
    let double = format!("CREATE TABLE \"{name}\"(");
    Some(match sql.strip_prefix(&double) {
        Some(rest) => format!("CREATE TABLE '{name}'({rest}"),
        None => sql.to_owned(),
    })
}
