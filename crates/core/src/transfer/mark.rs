//! Marking two transactions as a transfer by hand: the Transactions
//! bulk-action bar's "Mark as transfer", for a pair detection didn't
//! suggest (dates further apart than the window, or rows it can't see as a
//! pair) or one the user rejected and has changed their mind about.
//!
//! [`markable`] is the rule, on plain values so the view can answer it
//! from the register without a database: the two are on different
//! accounts, in one currency, with opposite amounts that aren't zero;
//! neither is in a suggested or confirmed pair, excluded, split or linked
//! (what [`super::confirm`] refuses). Dates, closed accounts and the
//! transfer window aren't looked at: the user knows the pair.
//!
//! A row in a `rejected` group may be marked. The register doesn't show
//! rejected groups, and "Not a transfer" or Unpair shouldn't be final:
//!
//! - when the two are the rows of one rejected pair, that group is
//!   confirmed again;
//! - otherwise a new confirmed group is recorded, and each row leaves the
//!   rejected group it was in (its old partner stays there, so detection
//!   still doesn't suggest it).
//!
//! [`mark`] checks again inside a `BEGIN IMMEDIATE` transaction and returns
//! a [`Marked`]; [`unmark`] reverses it exactly (the pair's group back to
//! `rejected`, or the new group removed and each row back in the group it
//! left), and refuses ([`Error::Changed`]) when the pair has changed since.
//! As with [`super::Decision`], no change batch is involved: only
//! `transfer_group` and `transaction.transfer_group_id` move. Nothing
//! touches categories, amounts, dates or balances.

use rusqlite::{Connection, OptionalExtension, TransactionBehavior, params};

use super::{Error, TransferStatus, load_group};
use crate::db::Database;
use crate::id::{AccountId, TransactionId, TransferGroupId};
use crate::link;
use crate::money::Money;

/// What [`markable`] needs of one side.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MarkSide {
    pub id: TransactionId,
    pub account: AccountId,
    /// Signed.
    pub amount: Money,
    /// The status of the transfer group it's in, if any.
    pub group: Option<TransferStatus>,
    pub excluded: bool,
    pub split: bool,
    /// The money in or the purchase of a confirmed link.
    pub linked: bool,
}

/// Why two transactions can't be marked as a transfer.
#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
pub enum NotMarkable {
    #[error("both are on one account")]
    SameAccount,

    #[error("they are in different currencies")]
    Currencies,

    /// Not the same amount out and in, or both zero.
    #[error("they aren't the same amount out and in")]
    NotOpposite,

    /// In a suggested or confirmed pair already.
    #[error("transaction {0} is in a transfer already")]
    InTransfer(TransactionId),

    #[error("transaction {0} is excluded")]
    Excluded(TransactionId),

    #[error("transaction {0} is split")]
    Split(TransactionId),

    #[error("transaction {0} is linked")]
    Linked(TransactionId),
}

/// Whether `a` and `b` can be marked as one transfer (module docs).
pub fn markable(a: &MarkSide, b: &MarkSide) -> Result<(), NotMarkable> {
    if a.account == b.account {
        return Err(NotMarkable::SameAccount);
    }
    if a.amount.currency() != b.amount.currency() {
        return Err(NotMarkable::Currencies);
    }
    if a.amount.minor() == 0 || a.amount.minor().checked_neg() != Some(b.amount.minor()) {
        return Err(NotMarkable::NotOpposite);
    }
    for side in [a, b] {
        if matches!(
            side.group,
            Some(TransferStatus::Suggested | TransferStatus::Confirmed)
        ) {
            return Err(NotMarkable::InTransfer(side.id));
        }
    }
    for side in [a, b] {
        if side.excluded {
            return Err(NotMarkable::Excluded(side.id));
        }
        if side.split {
            return Err(NotMarkable::Split(side.id));
        }
        if side.linked {
            return Err(NotMarkable::Linked(side.id));
        }
    }
    Ok(())
}

/// What the pair's group was before [`mark`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Previous {
    /// The group is new. Each row's rejected group before, if it was in
    /// one, in [`Marked::transactions`] order.
    New([Option<TransferGroupId>; 2]),
    /// The group is the two rows' own rejected pair, confirmed again.
    Rejected,
}

/// Two transactions marked as a transfer, kept to [`unmark`] it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Marked {
    /// The confirmed group.
    pub group: TransferGroupId,
    /// The pair's two transactions, lower ID first.
    pub transactions: [TransactionId; 2],
    pub previous: Previous,
}

/// Marks `a` and `b` as one confirmed transfer (module docs), in a DB
/// transaction of its own.
pub fn mark(db: &mut Database, a: TransactionId, b: TransactionId) -> Result<Marked, Error> {
    let tx = db
        .conn_mut()
        .transaction_with_behavior(TransactionBehavior::Immediate)?;
    let marked = mark_in(&tx, a, b)?;
    tx.commit()?;
    Ok(marked)
}

/// [`mark`] inside the caller's transaction.
pub fn mark_in(conn: &Connection, a: TransactionId, b: TransactionId) -> Result<Marked, Error> {
    let (first, second) = (load_side(conn, a)?, load_side(conn, b)?);
    markable(&first.side, &second.side)?;
    let transactions = [a.min(b), a.max(b)];
    let groups = if a < b {
        [first.group, second.group]
    } else {
        [second.group, first.group]
    };
    let own_pair = match groups {
        [Some(x), Some(y)] if x == y => load_group(conn, x)?
            .is_some_and(|group| group.transactions == transactions)
            .then_some(x),
        _ => None,
    };
    let marked = match own_pair {
        Some(group) => {
            let changed = conn.execute(
                "UPDATE transfer_group SET status = ?1 WHERE id = ?2 AND status = ?3",
                params![
                    TransferStatus::Confirmed.as_str(),
                    group,
                    TransferStatus::Rejected.as_str()
                ],
            )?;
            if changed != 1 {
                return Err(Error::Changed(group));
            }
            Marked {
                group,
                transactions,
                previous: Previous::Rejected,
            }
        }
        None => {
            conn.execute(
                "INSERT INTO transfer_group (status) VALUES (?1)",
                [TransferStatus::Confirmed.as_str()],
            )?;
            let group = TransferGroupId::new(conn.last_insert_rowid());
            let mut assign = conn.prepare_cached(
                "UPDATE \"transaction\" SET transfer_group_id = ?1 \
                 WHERE id = ?2 AND transfer_group_id IS ?3",
            )?;
            for (id, before) in transactions.into_iter().zip(groups) {
                if assign.execute(params![group, id, before])? != 1 {
                    return Err(Error::Changed(group));
                }
            }
            Marked {
                group,
                transactions,
                previous: Previous::New(groups),
            }
        }
    };
    tracing::info!(group = %marked.group, "marked a transfer pair");
    Ok(marked)
}

/// Reverses `marked`, if the pair is as it left it: confirmed, with the
/// same two transactions. Its own rejected pair goes back to `rejected`;
/// a new group is removed and each row goes back to the group it left
/// (none, if that group has gone since).
pub fn unmark(db: &mut Database, marked: &Marked) -> Result<(), Error> {
    let tx = db
        .conn_mut()
        .transaction_with_behavior(TransactionBehavior::Immediate)?;
    unmark_in(&tx, marked)?;
    tx.commit()?;
    Ok(())
}

/// [`unmark`] inside the caller's transaction.
pub fn unmark_in(conn: &Connection, marked: &Marked) -> Result<(), Error> {
    let group = load_group(conn, marked.group)?.ok_or(Error::Changed(marked.group))?;
    if group.status != TransferStatus::Confirmed || group.transactions != marked.transactions {
        return Err(Error::Changed(marked.group));
    }
    match marked.previous {
        Previous::Rejected => {
            conn.execute(
                "UPDATE transfer_group SET status = ?1 WHERE id = ?2",
                params![TransferStatus::Rejected.as_str(), marked.group],
            )?;
        }
        Previous::New(before) => {
            let mut restore = conn.prepare_cached(
                "UPDATE \"transaction\" SET transfer_group_id = \
                     (SELECT id FROM transfer_group WHERE id = ?1) \
                 WHERE id = ?2",
            )?;
            for (id, before) in marked.transactions.into_iter().zip(before) {
                restore.execute(params![before, id])?;
            }
            conn.execute("DELETE FROM transfer_group WHERE id = ?1", [marked.group])?;
        }
    }
    tracing::info!(group = %marked.group, "unmarked a transfer pair");
    Ok(())
}

/// One side as stored, and the group it's in.
struct Loaded {
    side: MarkSide,
    group: Option<TransferGroupId>,
}

fn load_side(conn: &Connection, id: TransactionId) -> Result<Loaded, Error> {
    let loaded = conn
        .prepare_cached(
            "SELECT t.account_id, t.amount_minor, t.currency, t.transfer_group_id, g.status, \
                    t.excluded_reason IS NOT NULL, \
                    EXISTS (SELECT 1 FROM transaction_split s WHERE s.transaction_id = t.id) \
             FROM \"transaction\" t LEFT JOIN transfer_group g ON g.id = t.transfer_group_id \
             WHERE t.id = ?1",
        )?
        .query_row([id], |row| {
            let status: Option<String> = row.get(4)?;
            let status = status
                .map(|key| {
                    TransferStatus::from_key(&key).ok_or_else(|| {
                        rusqlite::Error::InvalidColumnType(
                            4,
                            "status".to_owned(),
                            rusqlite::types::Type::Text,
                        )
                    })
                })
                .transpose()?;
            Ok(Loaded {
                side: MarkSide {
                    id,
                    account: row.get(0)?,
                    amount: Money::new(row.get(1)?, row.get(2)?),
                    group: status,
                    excluded: row.get(5)?,
                    split: row.get(6)?,
                    linked: false,
                },
                group: row.get(3)?,
            })
        })
        .optional()?;
    let mut loaded = loaded.ok_or(Error::TransactionNotFound(id))?;
    loaded.side.linked = link::is_linked(conn, id)?;
    Ok(loaded)
}
