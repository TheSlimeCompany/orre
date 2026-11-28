//! A golden test of the schema text SQLite stores: every `sqlite_master`
//! row of [`super::reference_at_version`] at the latest embedded version,
//! against `schema_snapshot.txt` beside this file.
//!
//! `backup::restore` compares each object's SQL exactly with that
//! reference (`settings-and-backups.md` → "Decision record: restore checks
//! the backup's schema"). Some of the text is SQLite's own: `ALTER TABLE …
//! RENAME TO` (V14, V17, V20, V21) and `ADD COLUMN` rewrite what is stored.
//! If a bundled-SQLite update changes it, older backups would be refused,
//! so this test fails first and someone decides.
//!
//! The format, one block per object, sorted by type and name:
//!
//! ```text
//! #### <type> <name> on <tbl_name>
//! <sql>
//!
//! ```
//!
//! `\r\n` is read as `\n` (as the restore does); a backslash is written
//! `\\`, a tab `\t`, a lone carriage return `\r`, a space at the end of a
//! line and any other non-printing or non-ASCII character `\u{…}`; spaces
//! in names are `\u{20}`. A row without SQL (an automatic index) has
//! `(no SQL)` after its header line instead of a body.

use std::fs;
use std::path::PathBuf;

use super::{SchemaVersion, migrations, reference_at_version};

/// Rewrites the snapshot when set to exactly `1`.
const UPDATE_VAR: &str = "ORRE_UPDATE_SCHEMA_SNAPSHOT";

/// Starts every block's header line; no line of SQL may start with it.
const HEADER: &str = "#### ";

/// The longest line the failure message quotes, in characters.
const LINE_CAP: usize = 120;

const PREAMBLE: &str = "\
# The schema text SQLite stores for Orre's migrations at the latest version
# (crates/core/src/db/schema_snapshot_tests.rs, which describes the format).
# Don't edit by hand. Regenerate, after deciding about older backups, with
#   ORRE_UPDATE_SCHEMA_SNAPSHOT=1 cargo test -p orre-core --lib \
db::schema_snapshot_tests::the_stored_schema_text_matches_the_snapshot

";

fn snapshot_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("src/db/schema_snapshot.txt")
}

/// `text` with everything but printable ASCII and `\n` escaped, and a
/// space at the end of a line too; `name` also escapes every space.
fn escaped(text: &str, name: bool) -> String {
    let text = text.replace("\r\n", "\n");
    let chars: Vec<char> = text.chars().collect();
    let mut out = String::with_capacity(text.len());
    for (i, &c) in chars.iter().enumerate() {
        let line_end = chars.get(i + 1).is_none_or(|next| *next == '\n');
        match c {
            '\\' => out.push_str("\\\\"),
            '\t' => out.push_str("\\t"),
            '\r' => out.push_str("\\r"),
            '\n' if !name => out.push('\n'),
            ' ' if name || line_end => out.push_str("\\u{20}"),
            ' '..='~' => out.push(c),
            _ => out.push_str(&format!("\\u{{{:x}}}", u32::from(c))),
        }
    }
    out
}

/// The blocks for the reference at `version`: header line and body.
fn blocks(version: SchemaVersion) -> Vec<(String, String)> {
    let conn = reference_at_version(version).unwrap();
    let mut statement = conn
        .prepare("SELECT type, name, tbl_name, sql FROM sqlite_master ORDER BY type, name")
        .unwrap();
    statement
        .query_map([], |row| {
            let (kind, name, table): (String, String, String) =
                (row.get(0)?, row.get(1)?, row.get(2)?);
            let sql: Option<String> = row.get(3)?;
            let header = format!(
                "{HEADER}{} {} on {}",
                escaped(&kind, true),
                escaped(&name, true),
                escaped(&table, true)
            );
            let body = sql.map_or_else(|| "(no SQL)".to_owned(), |sql| escaped(&sql, false));
            Ok((header, body))
        })
        .unwrap()
        .collect::<Result<_, _>>()
        .unwrap()
}

fn render(blocks: &[(String, String)]) -> String {
    let mut out = PREAMBLE.to_owned();
    for (header, body) in blocks {
        assert!(
            !body.lines().any(|line| line.starts_with(HEADER)),
            "{header}: a line of SQL starts with {HEADER:?}; change the format's header"
        );
        out.push_str(&format!("{header}\n{body}\n\n"));
    }
    out
}

/// The snapshot's blocks, read back from its text.
fn parse(text: &str) -> Vec<(String, String)> {
    let mut blocks = Vec::new();
    for chunk in text.split(&format!("\n{HEADER}")).skip(1) {
        let (header, body) = chunk.split_once('\n').unwrap_or((chunk, ""));
        // A block ends "\n\n"; the split took the last "\n" of all but the last.
        let body = body
            .strip_suffix("\n\n")
            .or_else(|| body.strip_suffix('\n'))
            .unwrap_or(body);
        blocks.push((format!("{HEADER}{header}"), body.to_owned()));
    }
    blocks
}

/// A line for the failure message: capped, with any raw character the
/// format would have escaped (a hand edit, say) shown as `\u{…}`.
fn cap(line: Option<&str>) -> String {
    let Some(line) = line else {
        return "(no line)".to_owned();
    };
    let trimmed = line.trim_end_matches(' ');
    let mut shown: String = trimmed
        .chars()
        .map(|c| match c {
            ' '..='~' => c.to_string(),
            _ => format!("\\u{{{:x}}}", u32::from(c)),
        })
        .collect();
    shown.push_str(&"\\u{20}".repeat(line.len() - trimmed.len()));
    if shown.chars().count() > LINE_CAP {
        shown = format!("{}…", shown.chars().take(LINE_CAP).collect::<String>());
    }
    shown
}

/// The first difference between the snapshot's blocks and the actual ones.
fn first_difference(expected: &[(String, String)], actual: &[(String, String)]) -> String {
    for i in 0..expected.len().max(actual.len()) {
        match (expected.get(i), actual.get(i)) {
            (Some((want, _)), Some((have, _))) if want != have => {
                return format!(
                    "the objects differ at position {i}:\n  snapshot: {}\n  actual:   {}",
                    cap(Some(want)),
                    cap(Some(have))
                );
            }
            (Some((header, want)), Some((_, have))) if want != have => {
                let want_lines: Vec<&str> = want.split('\n').collect();
                let have_lines: Vec<&str> = have.split('\n').collect();
                let line = (0..)
                    .find(|&n| want_lines.get(n) != have_lines.get(n))
                    .unwrap_or(0);
                return format!(
                    "the SQL of `{}` differs at line {}:\n  snapshot: {}\n  actual:   {}",
                    header.trim_start_matches(HEADER),
                    line + 1,
                    cap(want_lines.get(line).copied()),
                    cap(have_lines.get(line).copied())
                );
            }
            (Some((header, _)), None) => {
                return format!(
                    "`{}` is in the snapshot but not in the schema",
                    header.trim_start_matches(HEADER)
                );
            }
            (None, Some((header, _))) => {
                return format!(
                    "`{}` is in the schema but not in the snapshot",
                    header.trim_start_matches(HEADER)
                );
            }
            _ => {}
        }
    }
    "the preamble differs".to_owned()
}

#[test]
fn the_stored_schema_text_matches_the_snapshot() {
    let latest = migrations::latest_version(&migrations::embedded());
    let actual = blocks(latest);
    let rendered = render(&actual);
    // The format reads back to exactly what was rendered.
    assert_eq!(parse(&rendered), actual);
    let path = snapshot_path();
    if std::env::var(UPDATE_VAR).as_deref() == Ok("1") {
        fs::write(&path, &rendered).unwrap();
        return;
    }
    let snapshot = fs::read_to_string(&path).unwrap_or_default();
    if snapshot == rendered {
        return;
    }
    panic!(
        "The schema text SQLite stores no longer matches {} (version {latest}).\n\
         First difference: {}\n\n\
         This is most likely a bundled-SQLite update (rusqlite / libsqlite3-sys) or a new or \
         changed migration. `backup::restore` compares each object's SQL exactly, so a user's \
         older backups, made by an Orre built with the old text, may now be refused as \
         `Changed`. Decide what to do about them (for example, accept the known old text in \
         the restore's comparison, or re-baseline) BEFORE updating the snapshot.\n\
         Then regenerate it with:\n  \
         {UPDATE_VAR}=1 cargo test -p orre-core --lib \
         db::schema_snapshot_tests::the_stored_schema_text_matches_the_snapshot",
        path.display(),
        first_difference(&parse(&snapshot), &actual),
    );
}
