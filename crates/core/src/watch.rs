//! Noticing that another process changed the database (ROADMAP LU.2;
//! `docs/architecture/app.md` → "Decision record: live updates
//! (2026-11-21)").
//!
//! The app, the CLI and `orre mcp` all write the one database file, so what
//! the app shows can go stale without the app having done anything. A
//! [`Watcher`] answers "what changed since I last looked?" in two steps, so
//! that asking often costs nothing when nothing happened:
//!
//! 1. **Stamp.** The size and modification time of the database file and
//!    of its `-wal` file (absent is a valid state: the file is checkpointed
//!    away), and the database file's identity (device and inode on Unix, the
//!    creation time elsewhere). Reading it is `stat` calls: nothing is
//!    opened. An unchanged stamp ends the check.
//! 2. **Fingerprints.** A changed stamp is not yet a change (a checkpoint
//!    rewrites the files and no data), so the database is opened once, with
//!    [`gate::open`] and [`Database::open_existing`] (it creates and migrates
//!    nothing), and a few queries per [`Topic`] are hashed. The topics whose
//!    hash differs from the last check's are reported. The stamp is read
//!    again after the connection closes. If it is the stamp from before the
//!    open, it is kept. If not (another process wrote while the check read,
//!    or the check's own close checkpointed and removed the `-wal`), the
//!    stamp from before is kept, so the next check opens and compares again
//!    instead of absorbing a write it never read. A check whose open leaves
//!    the files as they were settles it, so the extra open is one tick.
//!
//! - **The first check is the baseline.** It stores the stamp and
//!   fingerprints and reports no topic. Take it before the views' first
//!   reads start, so a write landing between the two is reported. If
//!   the database file doesn't exist yet, nothing is checked, and the first
//!   check that finds it is the baseline.
//! - **A restore reports every topic.** [`restore`](crate::backup::restore)
//!   swaps a new file in, so the identity changes, and nothing about the old
//!   fingerprints can be trusted.
//! - **While a restore holds the restore lock** the check is
//!   [`Check::Skipped`] and the stored state is left alone, so the next check
//!   after the restore compares against what was last seen and catches up.
//! - **The app's own changes** are reported like any other. Before a view
//!   starts the reload that follows one, the app calls [`Watcher::rebase`]
//!   for its topics, so the next check doesn't report them again. Rebasing
//!   first is what keeps it sound: a write another process makes after the
//!   rebase is reported, and one made before it is in the reload's read.
//!   Rebasing after the reload would absorb a write that landed between the
//!   two unseen.
//! - **After the app's own restore**, rebasing every topic also takes the
//!   restored file's identity, so the next check compares instead of
//!   reporting every topic a second time.
//! - **Timestamps are only as fine as the file system's.** Two commits in
//!   one modification-time tick that leave both files' sizes as they were
//!   look like one stamp. On file systems with nanosecond times (ext4,
//!   APFS, NTFS) that doesn't happen in practice; on coarse ones (exFAT,
//!   some network file systems) the second commit waits for the next
//!   change to be seen.
//!
//! Fingerprints are hashes kept in memory and compared for equality: never
//! stored, logged or sent anywhere, though they are computed from rows that
//! hold user text (rule patterns, names). They read the primary key or a
//! count where a table can grow, and hash whole rows (as `group_concat`
//! strings over an explicit column list) only for tables that stay
//! small by nature.
//!
//! What no topic sees, and what it would take:
//!
//! - Budgets, schedules (skips and matches), exchange rates, tags'
//!   links to transactions, and the MCP access log: no topic covers them.
//!   Each is a table of its own that a view could be reloaded for; adding
//!   one is a query here.
//! - A change to a transaction's own columns that records no change batch
//!   and changes no count or newest ID. Core records one for every edit of
//!   a user-facing field ([`crate::change`]) and undoing it changes the
//!   batch's status, which the fingerprint holds. Catching an unrecorded one
//!   would need an index or column to read cheaply (a migration, LU.6).
//! - The excluded columns: `created_at` of the rule tables, a client's
//!   `last_seen_at` and the window state setting. They change without the
//!   user having changed anything the views show.

#[cfg(test)]
mod tests;

use std::collections::BTreeSet;
use std::collections::hash_map::DefaultHasher;
use std::fs::Metadata;
use std::hash::{Hash, Hasher};
use std::io;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use rusqlite::Connection;
use rusqlite::types::ValueRef;

use crate::backup::gate;
use crate::db::{self, Database};

/// A part of what the app shows that can be reloaded on its own.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Topic {
    /// Suggestions (filing, and deciding: the pending count moves) and
    /// saved reports.
    Inbox,
    /// What the Transactions, Accounts and Checks views draw on: the
    /// transactions, accounts, balance snapshots, import and change batches
    /// (and their undo), duplicate candidates and links.
    Ledger,
    /// Rules and their actions, merchant aliases and categories: what
    /// decides how a transaction is named and filed.
    Rules,
    /// MCP clients (name, enabled, access level, status) and the settings.
    Settings,
}

impl Topic {
    /// Every topic, in the order of the enum.
    pub const ALL: [Topic; 4] = [Topic::Inbox, Topic::Ledger, Topic::Rules, Topic::Settings];
}

/// What one [`Watcher::check`] found.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Check {
    /// The stamp is as it was, or there is no database file yet. Nothing was
    /// opened.
    Unchanged,
    /// A restore holds the restore lock. Nothing was compared or stored: the
    /// next check catches up.
    Skipped,
    /// The database was opened and compared. The set is empty when the
    /// stamp changed but no data did (a checkpoint), and on the baseline.
    Changed(BTreeSet<Topic>),
}

impl Check {
    /// The topics that changed: empty unless [`Check::Changed`].
    pub fn topics(&self) -> BTreeSet<Topic> {
        match self {
            Check::Changed(topics) => topics.clone(),
            Check::Unchanged | Check::Skipped => BTreeSet::new(),
        }
    }
}

/// Why a check failed. The stored state is left as it was, so the next check
/// tries again.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// The database file or its `-wal` couldn't be inspected.
    #[error("couldn't inspect {path}: {source}", path = path.display())]
    Stat { path: PathBuf, source: io::Error },
    /// The database couldn't be opened, or the restore lock couldn't be
    /// checked ([`gate::open`]). A restore in progress is
    /// [`Check::Skipped`], not this.
    #[error(transparent)]
    Open(gate::Error),
    /// A fingerprint query failed.
    #[error("database error: {0}")]
    Database(#[from] rusqlite::Error),
}

/// Who a file is, apart from its name, so a restore that put another file
/// at the same path is noticed even if size and time happen to match.
type Identity = Option<(u64, u64)>;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct FileStamp {
    len: u64,
    modified: Option<SystemTime>,
}

impl FileStamp {
    fn of(metadata: &Metadata) -> FileStamp {
        FileStamp {
            len: metadata.len(),
            modified: metadata.modified().ok(),
        }
    }
}

/// The database file's stamp and its `-wal`'s (`None`: absent).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Stamp {
    database: FileStamp,
    identity: Identity,
    wal: Option<FileStamp>,
}

#[cfg(unix)]
fn identity(metadata: &Metadata) -> Identity {
    use std::os::unix::fs::MetadataExt;
    Some((metadata.dev(), metadata.ino()))
}

/// Where there is no inode, the creation time: a file swapped in is created
/// anew. `None` where the platform doesn't say, which leaves size and time.
#[cfg(not(unix))]
fn identity(metadata: &Metadata) -> Identity {
    let created = metadata.created().ok()?;
    let since_epoch = created.duration_since(SystemTime::UNIX_EPOCH).ok()?;
    Some((since_epoch.as_secs(), u64::from(since_epoch.subsec_nanos())))
}

/// `None` when there is no file.
fn metadata(path: &Path) -> Result<Option<Metadata>, Error> {
    match std::fs::metadata(path) {
        Ok(metadata) => Ok(Some(metadata)),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(source) => Err(Error::Stat {
            path: path.to_path_buf(),
            source,
        }),
    }
}

impl Stamp {
    /// `None` when there is no database file.
    fn read(db_path: &Path) -> Result<Option<Stamp>, Error> {
        let Some(database) = metadata(db_path)? else {
            return Ok(None);
        };
        let wal = metadata(&db::sidecar(db_path, "-wal"))?;
        Ok(Some(Stamp {
            database: FileStamp::of(&database),
            identity: identity(&database),
            wal: wal.as_ref().map(FileStamp::of),
        }))
    }
}

/// One hash per topic, indexed by the topic's position in [`Topic::ALL`].
type Fingerprints = [u64; Topic::ALL.len()];

#[derive(Debug)]
struct Seen {
    stamp: Stamp,
    fingerprints: Fingerprints,
}

/// Watches one database file for changes made by any process
/// (module docs). Synchronous: the app calls [`check`](Watcher::check) off
/// the UI thread, on a timer.
#[derive(Debug)]
pub struct Watcher {
    db_path: PathBuf,
    seen: Option<Seen>,
    /// How many times a check or rebase opened the database.
    #[cfg(test)]
    opens: usize,
    /// Runs after a check's queries and before it reads the stamp again:
    /// a write from another process landing in that window.
    #[cfg(test)]
    between: Option<fn(&Path)>,
}

impl Watcher {
    /// A watcher of the database at `db_path`. Nothing is read until the
    /// first [`check`](Watcher::check), which is the baseline.
    pub fn new(db_path: &Path) -> Watcher {
        Watcher {
            db_path: db_path.to_path_buf(),
            seen: None,
            #[cfg(test)]
            opens: 0,
            #[cfg(test)]
            between: None,
        }
    }

    /// Opens the database through the restore gate and reads every
    /// fingerprint `read` asks for, closing the connection before it
    /// returns. [`gate::Error::Restoring`] is `Ok(None)`.
    fn read<T>(
        &mut self,
        read: impl FnOnce(&Connection) -> rusqlite::Result<T>,
    ) -> Result<Option<T>, Error> {
        #[cfg(test)]
        {
            self.opens += 1;
        }
        let opened = gate::open(&self.db_path, |path| {
            let db = Database::open_existing(path)?;
            let reader = db.reader()?;
            Ok((db, reader))
        });
        match opened {
            Ok((_db, reader)) => Ok(Some(read(reader.conn())?)),
            Err(gate::Error::Restoring) => Ok(None),
            Err(error) => Err(Error::Open(error)),
        }
    }

    /// Looks for changes since the last check that compared (module docs).
    pub fn check(&mut self) -> Result<Check, Error> {
        let Some(stamp) = Stamp::read(&self.db_path)? else {
            return Ok(Check::Unchanged);
        };
        if self.seen.as_ref().is_some_and(|seen| seen.stamp == stamp) {
            return Ok(Check::Unchanged);
        }
        let Some(fingerprints) = self.read(|conn| fingerprints(conn, &Topic::ALL))? else {
            return Ok(Check::Skipped);
        };

        let changed = match &self.seen {
            None => BTreeSet::new(),
            Some(seen) if seen.stamp.identity != stamp.identity => Topic::ALL.into_iter().collect(),
            Some(seen) => Topic::ALL
                .into_iter()
                .filter(|topic| seen.fingerprints[*topic as usize] != fingerprints[*topic as usize])
                .collect(),
        };
        #[cfg(test)]
        if let Some(between) = self.between {
            between(&self.db_path);
        }
        // The stamp after the connection closed is kept only when it is the
        // one before: then nothing touched the files while the check read.
        // Otherwise the stamp before is kept, so the next check opens again
        // and compares. That covers both a write that landed after the
        // queries and the check's own effect on the files (closing the last
        // connection checkpoints and removes the `-wal`); a check whose open
        // leaves the files as they were then settles it.
        let after = Stamp::read(&self.db_path)?;
        let stamp = match after {
            Some(after) if after == stamp => after,
            _ => stamp,
        };
        self.seen = Some(Seen {
            stamp,
            fingerprints,
        });
        Ok(Check::Changed(changed))
    }

    /// Keeps the current fingerprints of `topics`, so the next check doesn't
    /// report them as changed: for a view about to reload (after the app's
    /// own change, say), called before the reload's read starts (module
    /// docs). With every topic it also takes the database file's identity as
    /// it is now, so after the app's own restore the next check compares
    /// rather than reporting every topic.
    ///
    /// The stored stamp is left as it is, so the next check still compares
    /// every other topic: a change another process made to one of them in
    /// the meantime is reported, not absorbed. That costs the next check one
    /// open even when nothing else changed. Does nothing before the baseline
    /// or while a restore holds the restore lock (the next check then reports
    /// whatever the restore changed).
    pub fn rebase(&mut self, topics: &[Topic]) -> Result<(), Error> {
        if self.seen.is_none() {
            return Ok(());
        }
        // Read before the open: a restore that lands after it leaves the old
        // identity kept, and the next check reports every topic, which is
        // redundant, not wrong.
        let every = Topic::ALL.iter().all(|topic| topics.contains(topic));
        let identity = if every {
            Stamp::read(&self.db_path)?.map(|stamp| stamp.identity)
        } else {
            None
        };
        let Some(now) = self.read(|conn| fingerprints(conn, topics))? else {
            return Ok(());
        };
        if let Some(seen) = &mut self.seen {
            for topic in topics {
                seen.fingerprints[*topic as usize] = now[*topic as usize];
            }
            if let Some(identity) = identity {
                seen.stamp.identity = identity;
            }
        }
        Ok(())
    }
}

// Fingerprints -----------------------------------------------------------------

/// A table [`rows`] hashes whole: the columns it reads and the ones it
/// leaves out (module docs). A test compares the two lists with the live
/// schema, so a column added by a migration has to be put in one of them.
struct Rows {
    table: &'static str,
    columns: &'static [&'static str],
    /// The table's other columns.
    #[cfg_attr(not(test), allow(dead_code))]
    left_out: &'static [&'static str],
    /// A `WHERE` condition, or `""`.
    filter: &'static str,
    /// The `ORDER BY`, which makes the string stable.
    order: &'static str,
}

const ACCOUNT: Rows = Rows {
    table: "account",
    columns: &[
        "id",
        "parent_id",
        "name",
        "institution",
        "kind",
        "currency",
        "opening_balance_minor",
        "opening_date",
        "reconciled_through",
        "closed_on",
        "created_at",
        "preset_id",
        "sub_account_key",
    ],
    left_out: &[],
    filter: "",
    order: "id",
};

const RULE: Rows = Rows {
    table: "rule",
    columns: &[
        "id",
        "priority",
        "match_field",
        "match_kind",
        "pattern",
        "amount_min",
        "amount_max",
        "account_id",
        "origin",
    ],
    left_out: &["created_at"],
    filter: "",
    order: "id",
};

const RULE_ACTION: Rows = Rows {
    table: "rule_action",
    columns: &["id", "rule_id", "kind", "value_json"],
    left_out: &[],
    filter: "",
    order: "id",
};

const MERCHANT_ALIAS: Rows = Rows {
    table: "merchant_alias",
    columns: &["id", "match_kind", "pattern", "display_name"],
    left_out: &["created_at"],
    filter: "",
    order: "rowid",
};

const CATEGORY: Rows = Rows {
    table: "category",
    columns: &["id", "parent_id", "key", "label", "kind", "hidden"],
    left_out: &[],
    filter: "",
    order: "id",
};

const MCP_CLIENT: Rows = Rows {
    table: "mcp_client",
    columns: &["name", "enabled", "access_level", "status"],
    left_out: &["created_at", "last_seen_at"],
    filter: "",
    order: "name",
};

const SETTING: Rows = Rows {
    table: "setting",
    columns: &["key", "value"],
    left_out: &[],
    filter: "key <> 'app.window'",
    order: "key",
};

/// Every table a topic hashes whole, with the columns it hashes (for the
/// schema test).
#[cfg(test)]
const ROWS: [&Rows; 7] = [
    &ACCOUNT,
    &RULE,
    &RULE_ACTION,
    &MERCHANT_ALIAS,
    &CATEGORY,
    &MCP_CLIENT,
    &SETTING,
];

/// One row, one string: the table's rows in order, each one the SQL
/// literals of its columns joined by `char(31)`, the rows by `char(30)`.
fn rows(rows: &Rows) -> String {
    let columns: Vec<String> = rows
        .columns
        .iter()
        .map(|column| format!("quote({column})"))
        .collect();
    let filter = if rows.filter.is_empty() {
        String::new()
    } else {
        format!(" WHERE {}", rows.filter)
    };
    format!(
        "SELECT coalesce(group_concat(row, char(30)), '') FROM \
         (SELECT {} AS row FROM {}{filter} ORDER BY {})",
        columns.join(" || char(31) || "),
        rows.table,
        rows.order,
    )
}

/// The queries hashed for `topic`, each returning one row. IDs are the
/// primary key and counts read a covering index, but for `"transaction"`'s
/// count, the smallest index end to end (O(rows): milliseconds at 10^5, once
/// per changed stamp).
fn queries(topic: Topic) -> Vec<String> {
    match topic {
        Topic::Inbox => vec![
            // New suggestion, or a decision: the pending count moves.
            "SELECT coalesce(max(id), 0), \
                    (SELECT count(*) FROM suggestion WHERE status = 'pending') \
             FROM suggestion"
                .to_string(),
            // A saved report, and one deleted (`orre inbox delete-report`)
            // below the newest.
            "SELECT count(*), coalesce(max(id), 0) FROM report".to_string(),
        ],
        Topic::Ledger => vec![
            "SELECT coalesce(max(id), 0) FROM \"transaction\"".to_string(),
            // A manual transaction deleted below the newest changes neither
            // the newest ID nor any batch.
            "SELECT count(*) FROM \"transaction\"".to_string(),
            // Undo keeps the row and its ID and sets the status.
            "SELECT count(*), coalesce(max(id), 0), coalesce(sum(status = 'undone'), 0) \
             FROM import_batch"
                .to_string(),
            "SELECT count(*), coalesce(max(id), 0), coalesce(sum(status = 'undone'), 0) \
             FROM change_batch"
                .to_string(),
            "SELECT count(*), coalesce(max(id), 0) FROM balance_snapshot".to_string(),
            "SELECT count(*), coalesce(max(id), 0), \
                    (SELECT count(*) FROM duplicate_candidate WHERE status = 'pending') \
             FROM duplicate_candidate"
                .to_string(),
            // Confirming or rejecting a link moves no ID and no count.
            "SELECT count(*), coalesce(max(id), 0), coalesce(sum(status = 'confirmed'), 0), \
                    coalesce(sum(status = 'rejected'), 0) \
             FROM transaction_link"
                .to_string(),
            // The same for transfers: an Inbox approval of a transfer pair
            // confirms or adds a group and records no change batch.
            "SELECT count(*), coalesce(max(id), 0), coalesce(sum(status = 'confirmed'), 0), \
                    coalesce(sum(status = 'rejected'), 0) \
             FROM transfer_group"
                .to_string(),
            rows(&ACCOUNT),
        ],
        Topic::Rules => [&RULE, &RULE_ACTION, &MERCHANT_ALIAS, &CATEGORY]
            .into_iter()
            .map(rows)
            .collect(),
        Topic::Settings => [&MCP_CLIENT, &SETTING].into_iter().map(rows).collect(),
    }
}

fn fingerprints(conn: &Connection, topics: &[Topic]) -> rusqlite::Result<Fingerprints> {
    let mut all = [0; Topic::ALL.len()];
    for topic in topics {
        let mut hasher = DefaultHasher::new();
        for query in queries(*topic) {
            hash_rows(&mut hasher, conn, &query)?;
        }
        all[*topic as usize] = hasher.finish();
    }
    Ok(all)
}

/// Feeds every value of every row of `query` to `hasher`, tagged by type so
/// `1` and `'1'` differ, after the query's own text so two queries that
/// return nothing don't alias.
fn hash_rows(hasher: &mut DefaultHasher, conn: &Connection, query: &str) -> rusqlite::Result<()> {
    query.hash(hasher);
    let mut statement = conn.prepare_cached(query)?;
    let columns = statement.column_count();
    let mut rows = statement.query([])?;
    while let Some(row) = rows.next()? {
        for column in 0..columns {
            match row.get_ref(column)? {
                ValueRef::Null => 0u8.hash(hasher),
                ValueRef::Integer(value) => (1u8, value).hash(hasher),
                ValueRef::Real(value) => (2u8, value.to_bits()).hash(hasher),
                ValueRef::Text(value) => (3u8, value).hash(hasher),
                ValueRef::Blob(value) => (4u8, value).hash(hasher),
            }
        }
        u8::MAX.hash(hasher);
    }
    Ok(())
}
