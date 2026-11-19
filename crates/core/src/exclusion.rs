//! Excluding a transaction, and including it again (M1.22).
//!
//! Exclusion is how a wrong row is corrected without editing it: the row
//! stays in the database, visible under an "excluded" filter, with a
//! recorded reason (`docs/ARCHITECTURE.md` → "Decision record: Transactions
//! additions", point 1). Any row can be excluded, whatever its origin: the
//! exclusion fields are user-facing, so an imported row's source fields stay
//! as they are.
//!
//! - Every excluded row leaves spending, income, budgets and reports.
//!   [`ExclusionReason::Duplicate`] and [`ExclusionReason::Superseded`] also
//!   leave the account's computed balance: a duplicate never happened at the
//!   bank, and a superseded row is replaced by the bank's correction of it.
//!   The other reasons record money that really moved.
//! - `superseded` is the import's, never the user's
//!   ([`ExclusionReason::CHOOSABLE`] leaves it out): an import that takes a
//!   bank's correction sets it, with the row that replaced it
//!   ([`Exclusion::superseded_by`]), and only undoing that import clears it.
//!   [`exclude`] and [`include_again`] refuse a superseded row
//!   ([`Error::Superseded`]).
//! - A lock guards balances only (`docs/architecture/links-and-checks.md` →
//!   "Decision record: a lock guards balances only"), so only a change to
//!   whether a row counts in the balance meets the account's lock
//!   ([`moves_balance`]): into or out of a reason that leaves the balance,
//!   on a row dated on or before its account's `reconciled_through`, is
//!   refused ([`Error::Locked`]). Every other exclusion is allowed there.
//!   [`moves_balance`] is the one rule for every writer of an exclusion:
//!   this module, and change batches (rule applies, merges, their undo;
//!   [`crate::change`]).
//! - The reason of an excluded row can change; each [`exclude`] records a
//!   new `excluded_at`. [`include_again`] clears the reason, the note and
//!   the date together.
//!
//! The note is the user's text, like `user_note`: it is never logged and
//! never quoted in an error.

use chrono::NaiveDate;
use rusqlite::{Connection, OptionalExtension, params};

use crate::account;
use crate::db::Database;
use crate::id::TransactionId;

/// Why a transaction is excluded (`transaction.excluded_reason`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ExclusionReason {
    /// The same transaction is in the account twice. It never happened at
    /// the bank, so it also leaves the balance.
    Duplicate,
    /// The bank's mistake, such as a double charge, until the refund
    /// arrives.
    BankError,
    /// A charge being contested, including fraud.
    Disputed,
    /// Anything else; needs a note saying what.
    Other,
    /// Replaced by the bank's correction of it: a later import brought the
    /// same source ID with another date or amount. It leaves the balance,
    /// since the new row counts instead. Set only by an import, and cleared
    /// only by undoing it.
    Superseded,
}

impl ExclusionReason {
    /// Every key `transaction.excluded_reason` takes.
    pub const ALL: [ExclusionReason; 5] = [
        ExclusionReason::Duplicate,
        ExclusionReason::BankError,
        ExclusionReason::Disputed,
        ExclusionReason::Other,
        ExclusionReason::Superseded,
    ];

    /// The reasons a user (or a rule) can choose: all but
    /// [`ExclusionReason::Superseded`], which only an import sets.
    pub const CHOOSABLE: [ExclusionReason; 4] = [
        ExclusionReason::Duplicate,
        ExclusionReason::BankError,
        ExclusionReason::Disputed,
        ExclusionReason::Other,
    ];

    /// The key stored in `transaction.excluded_reason`.
    pub const fn as_str(self) -> &'static str {
        match self {
            ExclusionReason::Duplicate => "duplicate",
            ExclusionReason::BankError => "bank_error",
            ExclusionReason::Disputed => "disputed",
            ExclusionReason::Other => "other",
            ExclusionReason::Superseded => "superseded",
        }
    }

    pub fn from_key(key: &str) -> Option<ExclusionReason> {
        Self::ALL.into_iter().find(|reason| reason.as_str() == key)
    }

    /// Whether a row excluded for this reason leaves the account's computed
    /// balance: a duplicate and a superseded row do.
    pub const fn leaves_balance(self) -> bool {
        matches!(
            self,
            ExclusionReason::Duplicate | ExclusionReason::Superseded
        )
    }

    /// Whether a user (or a rule) can choose this reason.
    pub const fn is_choosable(self) -> bool {
        !matches!(self, ExclusionReason::Superseded)
    }

    /// Whether the reason needs a note.
    pub const fn needs_note(self) -> bool {
        matches!(self, ExclusionReason::Other)
    }
}

/// Whether changing a row's exclusion from `old` to `new` (`None`: not
/// excluded) changes whether it counts in its account's balance: one side
/// leaves the balance ([`ExclusionReason::leaves_balance`]) and the other
/// doesn't. Only such a change meets the account's lock (module docs).
pub fn moves_balance(old: Option<ExclusionReason>, new: Option<ExclusionReason>) -> bool {
    old.is_some_and(ExclusionReason::leaves_balance)
        != new.is_some_and(ExclusionReason::leaves_balance)
}

/// A stored exclusion.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Exclusion {
    pub reason: ExclusionReason,
    /// The user's text; `None` when there is none.
    pub note: Option<String>,
    /// When it was excluded: a UTC timestamp, `YYYY-MM-DDTHH:MM:SS.SSSZ`.
    pub excluded_at: String,
    /// The row that replaced it, when it is
    /// [`ExclusionReason::Superseded`].
    pub superseded_by: Option<TransactionId>,
}

/// Why excluding or including a transaction failed. Messages never contain
/// transaction text or the note.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("there is no transaction {0}")]
    TransactionNotFound(TransactionId),

    #[error("excluding a transaction as \"other\" needs a note saying why")]
    NoteRequired,

    #[error("transaction {0} isn't excluded")]
    NotExcluded(TransactionId),

    #[error("\"superseded\" is set only by importing the bank's correction")]
    NotChoosable,

    #[error(
        "transaction {id} was replaced by the bank's correction, transaction {by}; undo that \
         import to bring it back"
    )]
    Superseded {
        id: TransactionId,
        by: TransactionId,
    },

    #[error(
        "transaction {id} is dated {date}, on or before {through}, which is locked, and this \
         would change the account's balance there; unlock the account first"
    )]
    Locked {
        id: TransactionId,
        date: NaiveDate,
        through: NaiveDate,
    },

    #[error("database error: {0}")]
    Database(#[from] rusqlite::Error),
}

/// Excludes transaction `id` for `reason`, or changes the reason and note of
/// an excluded one. `note` is the user's; a blank one counts as none, and
/// [`ExclusionReason::Other`] needs one.
pub fn exclude(
    db: &mut Database,
    id: TransactionId,
    reason: ExclusionReason,
    note: Option<&str>,
) -> Result<Exclusion, Error> {
    let note = note.filter(|note| !note.trim().is_empty());
    if !reason.is_choosable() {
        return Err(Error::NotChoosable);
    }
    if reason.needs_note() && note.is_none() {
        return Err(Error::NoteRequired);
    }
    let tx = db.conn_mut().transaction()?;
    let stored = load(&tx, id)?;
    stored.check_not_superseded(id)?;
    check_lock(&stored, id, moves_balance(stored.reason, Some(reason)))?;
    tx.execute(
        "UPDATE \"transaction\" SET excluded_reason = ?1, excluded_note = ?2, \
         excluded_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now') WHERE id = ?3",
        params![reason.as_str(), note, id],
    )?;
    let exclusion = exclusion_of(&tx, id)?.ok_or(Error::NotExcluded(id))?;
    tx.commit()?;
    tracing::info!(transaction = %id, reason = reason.as_str(), "excluded a transaction");
    Ok(exclusion)
}

/// Includes excluded transaction `id` again: clears its reason, note and
/// date.
pub fn include_again(db: &mut Database, id: TransactionId) -> Result<(), Error> {
    let tx = db.conn_mut().transaction()?;
    let stored = load(&tx, id)?;
    let Some(reason) = stored.reason else {
        return Err(Error::NotExcluded(id));
    };
    stored.check_not_superseded(id)?;
    check_lock(&stored, id, moves_balance(Some(reason), None))?;
    tx.execute(
        "UPDATE \"transaction\" SET excluded_reason = NULL, excluded_note = NULL, \
         excluded_at = NULL WHERE id = ?1",
        [id],
    )?;
    tx.commit()?;
    tracing::info!(transaction = %id, "included a transaction again");
    Ok(())
}

/// Transaction `id`'s exclusion; `None` when it isn't excluded.
pub fn get(db: &Database, id: TransactionId) -> Result<Option<Exclusion>, Error> {
    load(db.conn(), id)?;
    exclusion_of(db.conn(), id)
}

/// What a change needs to know of the row.
struct Stored {
    reason: Option<ExclusionReason>,
    date: NaiveDate,
    /// Its account's lock date.
    locked_through: Option<NaiveDate>,
    superseded_by: Option<TransactionId>,
}

impl Stored {
    /// Refuses a change to a superseded row's exclusion: only undoing the
    /// import that superseded it clears it.
    fn check_not_superseded(&self, id: TransactionId) -> Result<(), Error> {
        match self.superseded_by {
            Some(by) => Err(Error::Superseded { id, by }),
            None => Ok(()),
        }
    }
}

fn load(conn: &Connection, id: TransactionId) -> Result<Stored, Error> {
    conn.query_row(
        "SELECT t.excluded_reason, t.date, a.reconciled_through, t.superseded_by \
         FROM \"transaction\" t JOIN account a ON a.id = t.account_id WHERE t.id = ?1",
        [id],
        |row| {
            Ok(Stored {
                reason: reason(row)?,
                date: account::date(row, "date")?,
                locked_through: account::optional_date(row, "reconciled_through")?,
                superseded_by: row.get("superseded_by")?,
            })
        },
    )
    .optional()?
    .ok_or(Error::TransactionNotFound(id))
}

/// Refuses a change to whether the row counts in its account's balance,
/// when the row is in the account's locked period.
fn check_lock(stored: &Stored, id: TransactionId, changes_balance: bool) -> Result<(), Error> {
    match stored.locked_through {
        Some(through) if changes_balance && stored.date <= through => Err(Error::Locked {
            id,
            date: stored.date,
            through,
        }),
        _ => Ok(()),
    }
}

fn exclusion_of(conn: &Connection, id: TransactionId) -> Result<Option<Exclusion>, Error> {
    let exclusion = conn.query_row(
        "SELECT excluded_reason, excluded_note, excluded_at, superseded_by FROM \"transaction\" \
         WHERE id = ?1",
        [id],
        |row| {
            let Some(reason) = reason(row)? else {
                return Ok(None);
            };
            Ok(Some(Exclusion {
                reason,
                note: row.get("excluded_note")?,
                excluded_at: row.get("excluded_at")?,
                superseded_by: row.get("superseded_by")?,
            }))
        },
    )?;
    Ok(exclusion)
}

fn reason(row: &rusqlite::Row<'_>) -> rusqlite::Result<Option<ExclusionReason>> {
    let key: Option<String> = row.get("excluded_reason")?;
    key.map(|key| {
        ExclusionReason::from_key(&key).ok_or_else(|| account::invalid(row, "excluded_reason"))
    })
    .transpose()
}

#[cfg(test)]
mod tests;
