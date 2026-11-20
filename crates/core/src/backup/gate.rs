//! Opening the database for one call, never across a restore: the other
//! half of the restore lock ([`super::restore_lock_path`];
//! `docs/ARCHITECTURE.md` → "Decision record: M2: MCP server", F.1).
//!
//! Every Orre process except [`super::restore`] opens the database per
//! call and keeps no connection between calls: the MCP server for each tool
//! call, the app's views for each read and change. Each opens it here.
//! [`open`] checks the restore lock ([`super::restoring`]), opens the
//! database, and checks the lock again, refusing the call if a restore held
//! it at either check. So a connection is never used across the swap: one
//! opened before a restore took the lock is one [`super::restore`] waits
//! for (it changes nothing until the connections opened earlier close), and
//! one opened after is refused and closed before it's used.
//!
//! A lock file that nobody holds (a stopped restore's, or the empty file a
//! finished one leaves) doesn't refuse: the database is complete whichever
//! step that restore stopped at.
//!
//! The lock is checked for an instant each time, not held for the call: a
//! restore that takes it after the second check waits for this call's
//! connections to close ([`super::RELEASE_WAIT`]).

use std::io;
use std::path::Path;

use super::restoring;
use crate::db;

/// Why [`open`] didn't open the database.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// A restore holds the restore lock: at the check before opening, or
    /// at the one after.
    #[error("A backup is being restored; try again when it's done.")]
    Restoring,
    /// The restore lock file exists but can't be opened or locked (its
    /// permissions, or a file system without locks), so whether a restore
    /// is running is unknown. [`super::restoring`]'s error, as it is.
    #[error(transparent)]
    Lock(io::Error),
    /// Opening failed: the opener's own error, as it is.
    #[error(transparent)]
    Open(db::Error),
}

/// Opens the database at `db_path` for one call with `open`, unless a
/// restore holds the restore lock before or after it does.
///
/// `open` is [`Database::open`](db::Database::open) (creates and migrates:
/// the app and the CLI) or [`Database::open_existing`](db::Database::open_existing)
/// (neither: the MCP server), or one of them followed by other connections
/// the call needs ([`Database::reader`](db::Database::reader)): everything
/// it opens is opened between the two checks. It should do nothing else.
/// What it opened is dropped (closed) when the second check refuses.
pub fn open<T>(
    db_path: &Path,
    open: impl FnOnce(&Path) -> Result<T, db::Error>,
) -> Result<T, Error> {
    check(db_path)?;
    let opened = open(db_path).map_err(Error::Open)?;
    check(db_path)?;
    Ok(opened)
}

/// [`Error::Restoring`] while a restore holds the lock.
fn check(db_path: &Path) -> Result<(), Error> {
    match restoring(db_path) {
        Ok(false) => Ok(()),
        Ok(true) => Err(Error::Restoring),
        Err(error) => Err(Error::Lock(error)),
    }
}

#[cfg(test)]
mod tests {
    use std::fs::{self, File, OpenOptions};
    use std::path::PathBuf;

    use tempfile::TempDir;

    use super::*;
    use crate::backup::{RestoreLock, restore_lock_path};
    use crate::db::Database;

    fn db_path(dir: &TempDir) -> PathBuf {
        dir.path().join("data").join("orre.db")
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

    fn assert_restoring<T: std::fmt::Debug>(result: Result<T, Error>) {
        match result {
            Err(Error::Restoring) => {}
            other => panic!("expected Restoring, got {other:?}"),
        }
    }

    #[test]
    fn opens_creating_and_migrating_or_only_an_existing_database() {
        let dir = TempDir::new().unwrap();
        let path = db_path(&dir);

        // `open_existing` creates nothing: no file is the opener's error.
        match open(&path, Database::open_existing) {
            Err(Error::Open(db::Error::NoDatabase { path: missing })) => {
                assert_eq!(missing, path)
            }
            other => panic!("expected NoDatabase, got {other:?}"),
        }
        assert!(!path.exists());

        let db = open(&path, Database::open).unwrap();
        assert_eq!(
            db.schema_version().unwrap(),
            Database::supported_schema_version()
        );
        drop(db);
        let db = open(&path, Database::open_existing).unwrap();
        assert_eq!(db.path(), path);
    }

    #[test]
    fn refuses_while_a_restore_holds_the_lock() {
        let dir = TempDir::new().unwrap();
        let path = db_path(&dir);
        drop(Database::open(&path).unwrap());

        let lock = hold_restore_lock(&path);
        let mut opened = false;
        assert_restoring(open(&path, |path| {
            opened = true;
            Database::open(path)
        }));
        assert!(!opened, "nothing is opened while the lock is held");
        assert_restoring(open(&path, Database::open_existing));

        drop(lock);
        open(&path, Database::open_existing).unwrap();
    }

    #[test]
    fn refuses_when_a_restore_takes_the_lock_while_opening() {
        let dir = TempDir::new().unwrap();
        let path = db_path(&dir);
        drop(Database::open(&path).unwrap());

        let mut lock = None;
        assert_restoring(open(&path, |path| {
            let db = Database::open(path)?;
            // A restore in another process takes the lock now.
            lock = Some(hold_restore_lock(path));
            Ok(db)
        }));
        assert!(lock.is_some());
    }

    #[test]
    fn opens_when_the_lock_file_is_there_but_nobody_holds_it() {
        let dir = TempDir::new().unwrap();
        let path = db_path(&dir);
        drop(Database::open(&path).unwrap());

        // The empty file a finished restore leaves.
        drop(hold_restore_lock(&path));
        assert!(restore_lock_path(&path).exists());
        open(&path, Database::open_existing).unwrap();

        // One a crashed restore left, with its marker.
        RestoreLock::acquire(&path, &dir.path().join("backup.db"))
            .unwrap()
            .crash();
        assert!(!fs::read(restore_lock_path(&path)).unwrap().is_empty());
        open(&path, Database::open).unwrap();
    }

    #[test]
    fn says_a_restore_is_running_in_the_apps_words() {
        assert_eq!(
            Error::Restoring.to_string(),
            "A backup is being restored; try again when it's done."
        );
    }

    #[cfg(unix)]
    #[test]
    fn a_lock_file_it_cant_open_is_the_lock_checks_own_error() {
        use std::os::unix::fs::PermissionsExt;

        let dir = TempDir::new().unwrap();
        let path = db_path(&dir);
        drop(Database::open(&path).unwrap());
        let lock = restore_lock_path(&path);
        fs::write(&lock, "").unwrap();
        fs::set_permissions(&lock, fs::Permissions::from_mode(0o000)).unwrap();
        // Root reads it anyway: nothing to check then.
        if File::open(&lock).is_ok() {
            return;
        }

        let expected = restoring(&path).unwrap_err().to_string();
        match open(&path, Database::open_existing) {
            Err(error @ Error::Lock(_)) => assert_eq!(error.to_string(), expected),
            other => panic!("expected Lock, got {other:?}"),
        }
    }
}
