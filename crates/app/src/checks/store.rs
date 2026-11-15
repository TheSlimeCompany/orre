//! Reading the review queues and acting on them in the database, the way
//! [`crate::inbox::store`] does suggestions: no connection is held between
//! calls. Each call opens the database at its path through [`gate::open`],
//! which checks the restore lock, so a restore can swap the file.
//!
//! Everything it reads and every change is core's: the queues are
//! [`coffer_core::checks::review`]; merging is
//! [`duplicate::merge_preview`] then [`duplicate::merge`] (which plans
//! again and refuses a stale preview or a locked row); dismissing and
//! reopening are [`duplicate::dismiss`] and [`duplicate::reopen`]; undoing a
//! merge is [`change::undo`]; locking is [`reconciliation::lock`] and
//! [`reconciliation::unlock`]. Owed to you (M3.11) is
//! [`coffer_core::link::owed::queue`]; confirming or rejecting a payment's
//! claims and renaming a person go through [`edit`], one undoable change
//! batch each.

use std::path::PathBuf;

use chrono::NaiveDate;
use coffer_core::backup::gate;
use coffer_core::change::{self, UndoSummary};
use coffer_core::checks::{self, Pair, Review};
use coffer_core::db::Database;
use coffer_core::duplicate::{self, MergeRequest, Merged};
use coffer_core::edit;
use coffer_core::id::{AccountId, ChangeBatchId, DuplicateCandidateId, LinkId, TransactionId};
use coffer_core::link::ShareLabel;
use coffer_core::link::owed::{self, OwedQueue};
use coffer_core::reconciliation;
use coffer_core::reporting::palette::{Palette, category_palette};
use coffer_core::schedule;

use crate::accounts::store::StoreError;

/// What the queues show.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ChecksData {
    pub review: Review,
    /// Owed to you (M3.11).
    pub owed: OwedQueue,
    pub palette: Palette,
    /// Today, local: dates in this year are written without it.
    pub today: Option<NaiveDate>,
}

/// The review queues in the database at `database`.
#[derive(Clone, Debug)]
pub struct ChecksStore {
    database: PathBuf,
}

impl ChecksStore {
    pub fn new(database: PathBuf) -> Self {
        Self { database }
    }

    fn open(&self) -> Result<Database, StoreError> {
        Ok(gate::open(&self.database, Database::open)?)
    }

    /// Both queues, read now.
    pub fn load(&self) -> Result<ChecksData, StoreError> {
        load(&self.open()?)
    }

    /// Pending pair `candidate`, planned keeping `keep`.
    pub fn pair(
        &self,
        candidate: DuplicateCandidateId,
        keep: TransactionId,
    ) -> Result<Pair, StoreError> {
        Ok(checks::pair(&self.open()?, candidate, Some(keep))?)
    }

    /// Merges `candidate` keeping `keep`, as one `duplicate_merge` batch.
    pub fn merge(
        &self,
        candidate: DuplicateCandidateId,
        keep: TransactionId,
    ) -> Result<Merged, StoreError> {
        let mut db = self.open()?;
        let preview = duplicate::merge_preview(&db, &[MergeRequest::keeping(candidate, keep)])?;
        Ok(duplicate::merge(&mut db, &preview)?)
    }

    /// "Not duplicates": the pair is never raised again.
    pub fn dismiss(&self, candidate: DuplicateCandidateId) -> Result<(), StoreError> {
        Ok(duplicate::dismiss(&mut self.open()?, candidate)?)
    }

    /// Undoes a dismissal: the pair is pending again.
    pub fn reopen(&self, candidate: DuplicateCandidateId) -> Result<(), StoreError> {
        Ok(duplicate::reopen(&mut self.open()?, candidate)?)
    }

    /// Undoes a merge's batch: the row is included again, the moved values
    /// go back, and the pair is pending again.
    pub fn undo(&self, batch: ChangeBatchId) -> Result<UndoSummary, StoreError> {
        Ok(change::undo(&mut self.open()?, batch)?)
    }

    pub fn lock(&self, account: AccountId, through: NaiveDate) -> Result<(), StoreError> {
        Ok(reconciliation::lock(&mut self.open()?, account, through)?)
    }

    pub fn unlock(&self, account: AccountId) -> Result<(), StoreError> {
        reconciliation::unlock(&mut self.open()?, account)?;
        Ok(())
    }

    /// "Link as reimbursement": confirms a payment's suggested claims.
    pub fn confirm_links(&self, links: &[LinkId]) -> Result<Option<ChangeBatchId>, StoreError> {
        Ok(edit::confirm_links(&mut self.open()?, links)?)
    }

    /// "Not this": rejects them; they're never suggested again.
    pub fn reject_links(&self, links: &[LinkId]) -> Result<Option<ChangeBatchId>, StoreError> {
        Ok(edit::reject_links(&mut self.open()?, links)?)
    }

    /// Renames a person everywhere.
    pub fn rename_person(
        &self,
        from: &ShareLabel,
        to: &ShareLabel,
    ) -> Result<Option<ChangeBatchId>, StoreError> {
        Ok(edit::rename_person(&mut self.open()?, from, to)?)
    }
}

/// [`ChecksStore::load`] on an open database: also what tests and the
/// snapshot use.
pub fn load(db: &Database) -> Result<ChecksData, StoreError> {
    let today = schedule::local_today()?;
    Ok(ChecksData {
        review: checks::review(db)?,
        owed: owed::queue(db)?,
        palette: category_palette(&db.reader()?, today)?,
        today: Some(today),
    })
}
