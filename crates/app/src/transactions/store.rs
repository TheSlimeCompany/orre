//! Opening the database for the Transactions view (`docs/ARCHITECTURE.md`
//! → "Decision record: the Transactions view").
//!
//! Like [`crate::accounts::store::AccountStore`], the store holds no
//! connection: [`TransactionStore::open`] opens the database at its path
//! for one call (creating and migrating it as the CLI does) through
//! [`gate::open`], which checks the restore lock before and after opening.
//! It does nothing else: each place that reads or changes something names
//! core's function itself, on the database the store opened. Changes go
//! through the view's edit pipeline (`TransactionsView::write`), which
//! opens the database for its `op`; reads that only fill a dialog or a
//! prompt go through `TransactionsView::query` or call
//! [`TransactionStore::open`] themselves. Reading the register runs on a
//! background thread; a change is one small write on the main thread.
//!
//! Every change is core's: [`own::recategorize`] sets the categories as the
//! user's, in one DB transaction recorded as one undoable change batch (the
//! CLI's `orre categorize` takes the same path), and [`change::undo`]
//! reverts it. The inspector's split, tags, note and merchant name go
//! through [`edit`] (M3.9), each one undoable change batch too, and so do
//! its links and shares (M3.11: link, confirm, unlink or reject, share,
//! rename a person). Nothing touches an imported source field.
//!
//! M3.10 adds what the Source section and the dialogs do: excluding and
//! including again ([`exclusion`]: fields on the row, not a change batch;
//! "Include again" is the undo), undoing an import ([`import::undo`], after
//! [`import::undo_preview`] said what goes), and manual rows ([`manual`]:
//! create, edit, delete).
//!
//! M3.8 adds transfer pairs: confirming, rejecting and unpairing
//! ([`inbox::decide_transfer`], which also settles an agent's pending
//! suggestion for the same pair) and undoing that ([`transfer::undo`]):
//! a status change, not a change batch.
//!
//! [`own::recategorize`]: orre_core::inbox::own::recategorize
//! [`change::undo`]: orre_core::change::undo
//! [`edit`]: orre_core::edit
//! [`exclusion`]: orre_core::exclusion
//! [`import::undo`]: orre_core::import::undo
//! [`import::undo_preview`]: orre_core::import::undo_preview
//! [`manual`]: orre_core::manual
//! [`inbox::decide_transfer`]: orre_core::inbox::decide_transfer
//! [`transfer::undo`]: orre_core::transfer::undo

use std::path::PathBuf;

use orre_core::backup::gate;
use orre_core::db::Database;

use crate::accounts::store::StoreError;

/// The transactions in the database at `database`.
#[derive(Clone, Debug)]
pub struct TransactionStore {
    database: PathBuf,
}

impl TransactionStore {
    pub fn new(database: PathBuf) -> Self {
        Self { database }
    }

    /// Opens the database for one call ([`gate::open`]): refused while a
    /// restore holds its lock.
    pub fn open(&self) -> Result<Database, StoreError> {
        Ok(gate::open(&self.database, Database::open)?)
    }
}

#[cfg(test)]
mod tests {
    use chrono::NaiveDate;
    use orre_core::account::{self, AccountKind, NewAccount};
    use orre_core::category::CategoryKey;
    use orre_core::change;
    use orre_core::edit;
    use orre_core::inbox::own;
    use orre_core::manual::{self, ManualTransaction};
    use orre_core::money::{Currency, Money};
    use orre_core::reporting::register::{Register, RowCategory};
    use orre_core::split::SplitLine;
    use tempfile::TempDir;

    use super::*;

    /// The register, read as the view reads it: on a database the store
    /// opened.
    fn load(store: &TransactionStore, today: NaiveDate) -> Register {
        let db = store.open().unwrap();
        Register::load(&db.reader().unwrap(), today).unwrap()
    }

    #[test]
    fn categorises_reads_back_and_undoes() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("orre.db");
        let id = {
            let mut db = Database::open(&path).unwrap();
            let current = account::create(
                &mut db,
                &NewAccount::new("Current account", AccountKind::Current, Currency::GBP),
            )
            .unwrap();
            let day = NaiveDate::from_ymd_opt(2026, 11, 9).unwrap();
            manual::create(
                &mut db,
                &ManualTransaction::new(current, day, Money::new(-420, Currency::GBP), "COFFEE"),
            )
            .unwrap()
        };
        let store = TransactionStore::new(path);
        let today = NaiveDate::from_ymd_opt(2026, 11, 14).unwrap();
        assert_eq!(
            load(&store, today).row(id).unwrap().category,
            RowCategory::Uncategorised
        );

        let key = CategoryKey::new("food.eating_out").unwrap();
        let recategorize = |items: &[_]| own::recategorize(&mut store.open().unwrap(), items);
        let batch = recategorize(&[(id, key.clone())]).unwrap().unwrap();
        let register = load(&store, today);
        let RowCategory::Category(category) = register.row(id).unwrap().category else {
            panic!("categorised");
        };
        assert_eq!(register.category(category).unwrap().key, key);
        // The same category again changes nothing.
        assert_eq!(recategorize(&[(id, key)]).unwrap(), None);

        change::undo(&mut store.open().unwrap(), batch).unwrap();
        assert_eq!(
            load(&store, today).row(id).unwrap().category,
            RowCategory::Uncategorised
        );
    }

    #[test]
    fn splits_tags_notes_and_names_read_back_and_undo() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("orre.db");
        let id = {
            let mut db = Database::open(&path).unwrap();
            let current = account::create(
                &mut db,
                &NewAccount::new("Joint account", AccountKind::Current, Currency::GBP),
            )
            .unwrap();
            let day = NaiveDate::from_ymd_opt(2026, 11, 7).unwrap();
            manual::create(
                &mut db,
                &ManualTransaction::new(current, day, Money::new(-5216, Currency::GBP), "WAITROSE"),
            )
            .unwrap()
        };
        let store = TransactionStore::new(path);
        let today = NaiveDate::from_ymd_opt(2026, 11, 14).unwrap();
        let gbp = |minor| Money::new(minor, Currency::GBP);
        let groceries = CategoryKey::new("food.groceries").unwrap();
        let db = || store.open().unwrap();

        let batch = edit::set_split(
            &mut db(),
            id,
            &[
                SplitLine::new(gbp(-3866), Some(groceries)),
                SplitLine::new(gbp(-1350), None),
            ],
        )
        .unwrap()
        .unwrap();
        let register = load(&store, today);
        let RowCategory::Split(parts) = &register.row(id).unwrap().category else {
            panic!("split");
        };
        assert_eq!(parts.len(), 2);
        change::undo(&mut db(), batch).unwrap();
        assert_eq!(
            load(&store, today).row(id).unwrap().category,
            RowCategory::Uncategorised
        );

        edit::add_new_tag(&mut db(), id, "Mum's birthday", true)
            .unwrap()
            .1
            .unwrap();
        edit::set_note(&mut db(), id, "Card for Mum")
            .unwrap()
            .unwrap();
        edit::set_merchant(&mut db(), id, "Waitrose & Partners")
            .unwrap()
            .unwrap();
        let register = load(&store, today);
        let row = register.row(id).unwrap();
        let tag = register.tag(row.tags[0]).unwrap();
        assert_eq!((tag.name.as_str(), tag.private), ("Mum's birthday", true));
        assert_eq!(row.note.as_deref(), Some("Card for Mum"));
        assert_eq!(row.merchant, "Waitrose & Partners");
        assert!(row.merchant_set);

        let tag = row.tags[0];
        edit::remove_tag(&mut db(), id, tag).unwrap().unwrap();
        assert!(load(&store, today).row(id).unwrap().tags.is_empty());
        edit::add_tag(&mut db(), id, tag).unwrap().unwrap();
        assert_eq!(load(&store, today).row(id).unwrap().tags, [tag]);
        assert_eq!(edit::remove_split(&mut db(), id).unwrap(), None);
    }

    #[test]
    fn opening_is_refused_while_a_restore_holds_the_lock() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("orre.db");
        let store = TransactionStore::new(path.clone());
        drop(store.open().unwrap());

        // A restore holds an exclusive lock on `<database>-restoring`.
        let lock = std::fs::File::create(orre_core::backup::restore_lock_path(&path)).unwrap();
        lock.lock().unwrap();
        assert_eq!(
            store.open().unwrap_err().to_string(),
            "A backup is being restored; try again when it's done."
        );
        lock.unlock().unwrap();
        store.open().unwrap();
    }
}
