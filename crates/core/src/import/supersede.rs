//! A bank's correction of a stored row (ROADMAP R.4, L.21;
//! `docs/architecture/import.md` → "Decision record: a bank's correction
//! supersedes the stored row").
//!
//! A file row whose source ID the account already holds, for another date
//! or amount, is the bank's new version of that row: a tip added, a fuel
//! hold or a foreign-currency amount settled, a pending row posted under the
//! same ID. Source fields are never edited, so the import takes the new
//! version as a new row and excludes the stored one as `superseded`, naming
//! the row that replaced it (`migrations/V21__superseded.sql`).
//!
//! - **The same payment.** The source ID alone doesn't make a correction:
//!   a bank (or a saved preset) that reuses IDs across unrelated rows would
//!   turn them into corrections. The file row must also be dated within
//!   [`CORRECTION_WINDOW_DAYS`] of the stored row, either way, and move
//!   money the same way (both out or both in; a zero amount on either side,
//!   a pending authorisation that posts, fits either). The description
//!   isn't compared: a pending row's descriptor often differs from the
//!   posted one's. Otherwise the row stays flagged
//!   ([`CorrectionBlocked::NotSamePayment`]).
//! - **When it can.** The stored row (the live one: not superseded itself)
//!   must be an imported row that isn't excluded, isn't in a suggested or
//!   confirmed transfer pair, has no suggested or confirmed refund,
//!   reimbursement or settlement link, and neither it nor the correction
//!   may be dated on or before the account's lock. Otherwise the row stays
//!   out, flagged ([`super::Flag::SourceIdConflict`] with its
//!   [`CorrectionBlocked`]): each of those ties says something about the old
//!   amount or date that the new row would silently break, so the user
//!   resolves it first.
//! - **A scheduled payment** the stored row fulfils doesn't stop it: the
//!   payment is the same, only the bank's figures changed, so the
//!   `schedule_match` moves to the new row, as the user's fields do
//!   (`schedule::move_match`), even when the new amount or date is outside
//!   the schedule's tolerance: the user or the matcher already decided this
//!   payment fulfils that occurrence. A correction whose stored row had no
//!   match is matched like a new row (a pending row for 0 that posts as
//!   the scheduled amount). Undoing the import moves the correction's match,
//!   whichever it is, to the restored row (`removal`).
//! - **An older version again.** A file row equal (date and amount) to a
//!   version the account already replaced is already imported: an older
//!   export never undoes a correction.
//! - **What moves across** ([`carry_over`]), after the rules have run on the
//!   new row, so a field the user set wins over a rule's: a category the
//!   user set (or approved), a merchant name the user set, the note, the
//!   tags (added), `reimbursable`, the share when it still fits the new
//!   amount and no rule set one, and the split when the amount is
//!   unchanged. A category or display name a rule or the preset set isn't
//!   copied: the rules work it out again for the new row. The stored row
//!   keeps its own values; what couldn't move is listed per row
//!   ([`NotCarried`]) in the preview (what the amount decides) and the
//!   commit summary (all of it).

use std::collections::BTreeSet;
use std::fmt;

use chrono::NaiveDate;
use rusqlite::{Connection, OptionalExtension, params};

use crate::account;
use crate::change::{self, CategorySource, CategoryValue, Field, NameSource, ShareValue, Value};
use crate::id::{AccountId, TransactionId};
use crate::money::Money;
use crate::split;

/// How many days apart, either way, a correction may be dated from the
/// stored row it replaces. A card hold (a hotel, a fuel pump, a car hire)
/// can settle weeks after it was authorised, so the window is a month;
/// unrelated rows that reuse an ID are usually further apart than that, or
/// move money the other way ([`CorrectionBlocked::NotSamePayment`]).
pub const CORRECTION_WINDOW_DAYS: i64 = 31;

/// Why the bank's correction can't replace the stored row. The row is then
/// flagged and left out, as before corrections existed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CorrectionBlocked {
    /// The file row is dated more than [`CORRECTION_WINDOW_DAYS`] from the
    /// stored row, or moves money the other way: the ID was reused, it
    /// isn't the same payment.
    NotSamePayment,
    /// The stored row is excluded already.
    Excluded,
    /// It is in a suggested or confirmed transfer pair.
    InTransfer,
    /// A suggested or confirmed refund, reimbursement or settlement link
    /// has it as its money in or its purchase.
    Linked,
    /// It, or the correction, is dated on or before the account's lock.
    Locked { through: NaiveDate },
    /// It isn't an imported row: the pot side of a pot transfer.
    NotImported,
}

impl fmt::Display for CorrectionBlocked {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            CorrectionBlocked::NotSamePayment => f.write_str(
                "its date or direction is too far from the stored row's to be the bank's \
                 correction of it",
            ),
            CorrectionBlocked::Excluded => f.write_str("it is excluded"),
            CorrectionBlocked::InTransfer => f.write_str("it is in a transfer pair"),
            CorrectionBlocked::Linked => {
                f.write_str("it has a refund, reimbursement or settlement link")
            }
            CorrectionBlocked::Locked { through } => write!(
                f,
                "it or the new version is dated on or before {through}, which is locked"
            ),
            CorrectionBlocked::NotImported => f.write_str("it is a pot's mirrored row"),
        }
    }
}

/// Something the user set on the stored row that the correction doesn't
/// take. It stays on the stored row, which stays visible under the
/// excluded filter.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum NotCarried {
    /// The split: the amount changed, or the new row can't be split (it is
    /// a pot transfer).
    Split,
    /// The share: it doesn't fit the new amount, or a rule set one on the
    /// new row.
    Share,
    /// The category the user set: a rule split the new row.
    Category,
}

impl fmt::Display for NotCarried {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            NotCarried::Split => "split",
            NotCarried::Share => "share",
            NotCarried::Category => "category",
        })
    }
}

/// A row of the file that is the bank's correction of a stored row.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Correction {
    /// The stored row it replaces: excluded as `superseded` by the commit.
    pub existing: TransactionId,
    /// The stored row's date and amount.
    pub was_date: NaiveDate,
    pub was_amount: Money,
    /// The new row's fingerprint.
    pub fingerprint: String,
    /// What the user set on the stored row that the new amount rules out
    /// moving. The commit may find more ([`AppliedCorrection`]).
    pub not_carried: Vec<NotCarried>,
}

/// A correction the commit made.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AppliedCorrection {
    /// The stored row, now excluded as `superseded`.
    pub replaced: TransactionId,
    /// The new row that replaced it.
    pub by: TransactionId,
    /// What stayed on the stored row.
    pub not_carried: Vec<NotCarried>,
}

/// The account's live row with a source ID, and what deciding on a
/// correction needs to know of it.
#[derive(Clone, Debug)]
pub(super) struct Stored {
    pub id: TransactionId,
    pub date: NaiveDate,
    pub amount_minor: i64,
    imported: bool,
    excluded: bool,
    in_transfer: bool,
    linked: bool,
    split: bool,
    my_share_minor: Option<i64>,
}

/// The account's live (not superseded) row with source ID `source_id`.
pub(super) fn live_row(
    conn: &Connection,
    account: AccountId,
    source_id: &str,
) -> rusqlite::Result<Option<Stored>> {
    let mut statement = conn.prepare_cached(
        "SELECT t.id, t.date, t.amount_minor, t.origin = 'imported' AS imported, \
         t.excluded_reason IS NOT NULL AS excluded, \
         EXISTS (SELECT 1 FROM transfer_group g \
                 WHERE g.id = t.transfer_group_id AND g.status <> 'rejected') AS in_transfer, \
         EXISTS (SELECT 1 FROM transaction_link l \
                 WHERE (l.inflow_id = t.id OR l.target_id = t.id) \
                   AND l.status <> 'rejected') AS linked, \
         EXISTS (SELECT 1 FROM transaction_split s WHERE s.transaction_id = t.id) AS split, \
         t.my_share_minor \
         FROM \"transaction\" t \
         WHERE t.account_id = ?1 AND t.external_id = ?2 AND t.superseded_by IS NULL",
    )?;
    statement
        .query_row(params![account, source_id], |row| {
            Ok(Stored {
                id: row.get("id")?,
                date: account::date(row, "date")?,
                amount_minor: row.get("amount_minor")?,
                imported: row.get("imported")?,
                excluded: row.get("excluded")?,
                in_transfer: row.get("in_transfer")?,
                linked: row.get("linked")?,
                split: row.get("split")?,
                my_share_minor: row.get("my_share_minor")?,
            })
        })
        .optional()
}

/// An earlier version of `live` (a row it, or a row before it, replaced)
/// dated `date` for `amount_minor`: the file holds a version the account
/// has already moved past.
pub(super) fn earlier_version(
    conn: &Connection,
    live: TransactionId,
    date: NaiveDate,
    amount_minor: i64,
) -> rusqlite::Result<Option<TransactionId>> {
    let mut statement = conn.prepare_cached(
        "WITH RECURSIVE version (id) AS ( \
             SELECT id FROM \"transaction\" WHERE superseded_by = ?1 \
             UNION \
             SELECT t.id FROM \"transaction\" t JOIN version v ON t.superseded_by = v.id) \
         SELECT t.id FROM \"transaction\" t JOIN version v ON v.id = t.id \
         WHERE t.date = ?2 AND t.amount_minor = ?3 ORDER BY t.id DESC LIMIT 1",
    )?;
    statement
        .query_row(
            params![live, account::date_key(date), amount_minor],
            |row| row.get(0),
        )
        .optional()
}

/// Why `stored` can't be replaced by a correction dated `date` for
/// `amount_minor`, if it can't. `locked_through` is the account's lock.
///
/// A mirrored row is never the bank's row for the ID, so that comes first.
/// Then whether the file row is the same payment at all: if it isn't, the
/// ties below (which say what a correction would break) don't apply to it.
pub(super) fn blocked(
    stored: &Stored,
    date: NaiveDate,
    amount_minor: i64,
    locked_through: Option<NaiveDate>,
) -> Option<CorrectionBlocked> {
    if !stored.imported {
        Some(CorrectionBlocked::NotImported)
    } else if !same_payment(stored, date, amount_minor) {
        Some(CorrectionBlocked::NotSamePayment)
    } else if let Some(through) =
        locked_through.filter(|through| stored.date <= *through || date <= *through)
    {
        Some(CorrectionBlocked::Locked { through })
    } else if stored.excluded {
        Some(CorrectionBlocked::Excluded)
    } else if stored.in_transfer {
        Some(CorrectionBlocked::InTransfer)
    } else if stored.linked {
        Some(CorrectionBlocked::Linked)
    } else {
        None
    }
}

/// Whether a row dated `date` for `amount_minor` can be the bank's new
/// version of `stored` (module docs): within [`CORRECTION_WINDOW_DAYS`]
/// of it, and not moving money the other way.
fn same_payment(stored: &Stored, date: NaiveDate, amount_minor: i64) -> bool {
    (date - stored.date).num_days().abs() <= CORRECTION_WINDOW_DAYS
        && stored.amount_minor.signum() * amount_minor.signum() >= 0
}

/// What the new amount alone rules out moving from `stored` to a row of
/// `amount_minor`: a split when the amount changed, a share that no longer
/// fits.
pub(super) fn predicted_not_carried(stored: &Stored, amount_minor: i64) -> Vec<NotCarried> {
    let mut not_carried = Vec::new();
    if stored.split && stored.amount_minor != amount_minor {
        not_carried.push(NotCarried::Split);
    }
    if stored
        .my_share_minor
        .is_some_and(|share| !share_fits(share, amount_minor))
    {
        not_carried.push(NotCarried::Share);
    }
    not_carried
}

/// Whether `share` is part of `amount`, with its sign (the schema's CHECK).
fn share_fits(share: i64, amount: i64) -> bool {
    if amount >= 0 {
        (0..=amount).contains(&share)
    } else {
        (amount..=0).contains(&share)
    }
}

/// The ID the next transaction gets: one past the AUTOINCREMENT sequence
/// (and past every stored ID). The commit points the stored row at it
/// before it inserts the correction under it.
pub(super) fn next_transaction_id(conn: &Connection) -> rusqlite::Result<TransactionId> {
    conn.query_row(
        "SELECT max(coalesce((SELECT seq FROM sqlite_sequence WHERE name = 'transaction'), 0), \
                    coalesce((SELECT max(id) FROM \"transaction\"), 0)) + 1",
        [],
        |row| row.get(0),
    )
}

/// Excludes `existing` as `superseded` by `by`, which the caller inserts
/// next, in the same DB transaction (the foreign key is deferred).
pub(super) fn mark_superseded(
    conn: &Connection,
    existing: TransactionId,
    by: TransactionId,
) -> rusqlite::Result<usize> {
    conn.execute(
        "UPDATE \"transaction\" SET excluded_reason = 'superseded', excluded_note = NULL, \
         excluded_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now'), superseded_by = ?1 \
         WHERE id = ?2 AND excluded_reason IS NULL",
        params![by, existing],
    )
}

/// Copies what the user set on `old` to `new`, the row that replaced it,
/// after the rules have run on `new` (module docs). Returns what stayed on
/// `old`. The writes aren't recorded as a change batch: undoing the import
/// deletes `new`.
pub(super) fn carry_over(
    conn: &Connection,
    old: TransactionId,
    new: TransactionId,
) -> Result<Vec<NotCarried>, change::Error> {
    let read = |id: TransactionId, field: Field| change::read(conn, id, field);
    let users = |source: Option<CategorySource>| {
        matches!(
            source,
            Some(CategorySource::User | CategorySource::AiApproved)
        )
    };
    let amounts: (i64, i64) = conn.query_row(
        "SELECT o.amount_minor, n.amount_minor FROM \"transaction\" o, \"transaction\" n \
         WHERE o.id = ?1 AND n.id = ?2",
        params![old, new],
        |row| Ok((row.get(0)?, row.get(1)?)),
    )?;
    let in_transfer: bool = conn.query_row(
        "SELECT EXISTS (SELECT 1 FROM \"transaction\" t \
         JOIN transfer_group g ON g.id = t.transfer_group_id \
         WHERE t.id = ?1 AND g.status <> 'rejected')",
        [new],
        |row| row.get(0),
    )?;
    let mut not_carried = BTreeSet::new();

    // The split first: it replaces whatever category or share the rules
    // gave the new row (a split row has neither of its own).
    if let Value::Split(lines) = read(old, Field::Split)?
        && !lines.is_empty()
    {
        if amounts.0 != amounts.1 || in_transfer {
            not_carried.insert(NotCarried::Split);
        } else if read(new, Field::Split)? != Value::Split(lines.clone()) {
            change::write(conn, new, Value::Category(CategoryValue::NONE))?;
            change::write(conn, new, Value::Share(ShareValue::WHOLE))?;
            change::write(conn, new, Value::Split(lines))?;
        }
    }

    if let Value::Category(theirs) = read(old, Field::Category)?
        && theirs.category_id.is_some()
        && users(theirs.source)
    {
        if split::is_split(conn, new)? {
            not_carried.insert(NotCarried::Category);
        } else {
            change::write(conn, new, Value::Category(theirs))?;
        }
    }

    if let Value::Share(theirs) = read(old, Field::Share)?
        && theirs != ShareValue::WHOLE
    {
        let ours = read(new, Field::Share)?;
        if ours != Value::Share(theirs.clone()) {
            let fits = theirs
                .my_share_minor
                .is_none_or(|share| share_fits(share, amounts.1));
            if fits && ours == Value::Share(ShareValue::WHOLE) && !split::is_split(conn, new)? {
                change::write(conn, new, Value::Share(theirs))?;
            } else {
                not_carried.insert(NotCarried::Share);
            }
        }
    }

    if let Value::Merchant(theirs) = read(old, Field::Merchant)?
        && theirs.source == Some(NameSource::User)
    {
        change::write(conn, new, Value::Merchant(theirs))?;
    }

    if let Value::UserNote(Some(theirs)) = read(old, Field::UserNote)? {
        change::write(conn, new, Value::UserNote(Some(theirs)))?;
    }

    if let Value::Tags(theirs) = read(old, Field::Tags)?
        && let Value::Tags(ours) = read(new, Field::Tags)?
    {
        let all: BTreeSet<_> = ours.iter().chain(&theirs).copied().collect();
        if all.len() > ours.len() {
            change::write(conn, new, Value::Tags(all.into_iter().collect()))?;
        }
    }

    if read(old, Field::Reimbursable)? == Value::Reimbursable(true) {
        change::write(conn, new, Value::Reimbursable(true))?;
    }

    Ok(not_carried.into_iter().collect())
}
