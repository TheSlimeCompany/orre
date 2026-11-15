//! Confirming, rejecting and unpairing transfer pairs (M3.8): the user's
//! side of [`detect_for_batch`](super::detect_for_batch), called by the
//! Transactions inspector.
//!
//! - [`confirm`]: a `suggested` pair becomes `confirmed`, so both rows leave
//!   income and spending. Refused when either row is excluded, split or
//!   linked ([`Blocked`]), as an Inbox approval refuses.
//! - [`reject`] ("Not a transfer"): a `suggested` pair becomes `rejected`.
//!   The group keeps its rows, so detection never suggests the pair again.
//! - [`unpair`]: a `confirmed` pair becomes `rejected`, the same state.
//!   Refused for a pot's pair: the import mirrored the pot's row from the
//!   parent's, so the two are one movement of money, not a guess
//!   ([`Blocked::PotPair`]).
//!
//! Each function re-checks the pair's current status and rows inside a
//! `BEGIN IMMEDIATE` transaction and refuses on a mismatch, so a decision
//! made on a stale screen changes nothing. Each returns a [`Decision`];
//! [`undo`] reverses it by setting the previous status back, and refuses
//! ([`Error::Changed`]) when the pair or its rows have changed since. No
//! change batch is involved: only `transfer_group.status` moves, and every
//! status change here has the opposite change as its inverse.
//!
//! Nothing here touches categories, amounts, dates or balances, and no lock
//! date applies: a transaction stays in its account with its amount, and
//! only reports (income and spending) read the status.
//!
//! [`pair_of`] is the inspector's read model: the pair a transaction is in,
//! its other side, and whether each of the three actions is allowed now.
//! The `_in` functions run inside the caller's transaction, to compose with
//! other writes.

use chrono::NaiveDate;
use rusqlite::{Connection, TransactionBehavior, params};

use super::{Error, TransferStatus, group_of, load_group};
use crate::account;
use crate::db::Database;
use crate::id::{AccountId, TransactionId, TransferGroupId};
use crate::link;
use crate::money::Money;

/// Why an action on a pair isn't allowed now.
#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
pub enum Blocked {
    /// The pair isn't in the status the action starts from.
    #[error("the pair is {}", .found.as_str())]
    WrongStatus { found: TransferStatus },

    /// The group doesn't hold exactly two transactions.
    #[error("the group doesn't hold exactly two transactions")]
    NotTwoSided,

    /// An excluded row isn't spending or income to leave out.
    #[error("transaction {0} is excluded")]
    Excluded(TransactionId),

    /// A split row is categorised by its lines, so it can't be a transfer.
    #[error("transaction {0} is split")]
    Split(TransactionId),

    /// A row that is the money in or the purchase of a confirmed link.
    #[error("transaction {0} is linked")]
    Linked(TransactionId),

    /// A pot's transfer, mirrored from the parent's row at import.
    #[error("the pair is a pot's transfer, mirrored by the import")]
    PotPair,
}

/// What a user can do to a pair.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Action {
    /// `suggested` to `confirmed`.
    Confirm,
    /// `suggested` to `rejected`: "Not a transfer".
    Reject,
    /// `confirmed` to `rejected`.
    Unpair,
}

impl Action {
    /// The status the pair must be in.
    pub const fn from(self) -> TransferStatus {
        match self {
            Action::Confirm | Action::Reject => TransferStatus::Suggested,
            Action::Unpair => TransferStatus::Confirmed,
        }
    }

    /// The status the pair is left in.
    pub const fn to(self) -> TransferStatus {
        match self {
            Action::Confirm => TransferStatus::Confirmed,
            Action::Reject | Action::Unpair => TransferStatus::Rejected,
        }
    }
}

/// A change made to a pair, kept to [`undo`] it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Decision {
    pub group: TransferGroupId,
    pub action: Action,
    /// The pair's two transactions, lower ID first.
    pub transactions: [TransactionId; 2],
}

/// Whether an action is allowed now, and if not, why.
pub type Availability = Result<(), Blocked>;

/// The other side of a pair, for "Savings → Current account · 9 Nov ·
/// £250.00".
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Partner {
    pub transaction: TransactionId,
    pub account: AccountId,
    pub account_name: String,
    pub date: NaiveDate,
    /// With its sign: the opposite of the transaction asked about.
    pub amount: Money,
}

/// The pair a transaction is in, as the inspector shows it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PairView {
    pub group: TransferGroupId,
    pub status: TransferStatus,
    /// The group's other transactions: one for a pair.
    pub partners: Vec<Partner>,
    pub confirm: Availability,
    pub reject: Availability,
    pub unpair: Availability,
}

impl PairView {
    /// Whether `action` is allowed now.
    pub fn allows(&self, action: Action) -> Availability {
        match action {
            Action::Confirm => self.confirm,
            Action::Reject => self.reject,
            Action::Unpair => self.unpair,
        }
    }
}

/// The pair `transaction` is in, or `None` when it's in no group. A
/// `rejected` pair is returned too (the inspector shows nothing for it, but
/// can say so); none of its actions are allowed.
pub fn pair_of(db: &Database, transaction: TransactionId) -> Result<Option<PairView>, Error> {
    let Some(group) = group_of(db, transaction)? else {
        return Ok(None);
    };
    let conn = db.conn();
    let rows = load_rows(conn, group.id)?;
    let allowed = |action| availability(group.status, &rows, action);
    let mut statement = conn.prepare(
        "SELECT t.id, t.account_id, a.name, t.date, t.amount_minor, t.currency \
         FROM \"transaction\" t JOIN account a ON a.id = t.account_id \
         WHERE t.transfer_group_id = ?1 AND t.id <> ?2 ORDER BY t.id",
    )?;
    let partners = statement
        .query_map(params![group.id, transaction], |row| {
            Ok(Partner {
                transaction: row.get(0)?,
                account: row.get(1)?,
                account_name: row.get(2)?,
                date: account::date(row, "date")?,
                amount: Money::new(row.get(4)?, row.get(5)?),
            })
        })?
        .collect::<Result<_, _>>()?;
    Ok(Some(PairView {
        group: group.id,
        status: group.status,
        partners,
        confirm: allowed(Action::Confirm),
        reject: allowed(Action::Reject),
        unpair: allowed(Action::Unpair),
    }))
}

/// Confirms a suggested pair.
pub fn confirm(db: &mut Database, group: TransferGroupId) -> Result<Decision, Error> {
    decide(db, group, Action::Confirm)
}

/// Rejects a suggested pair: "Not a transfer".
pub fn reject(db: &mut Database, group: TransferGroupId) -> Result<Decision, Error> {
    decide(db, group, Action::Reject)
}

/// Unpairs a confirmed pair.
pub fn unpair(db: &mut Database, group: TransferGroupId) -> Result<Decision, Error> {
    decide(db, group, Action::Unpair)
}

/// [`confirm`] inside the caller's transaction.
pub fn confirm_in(conn: &Connection, group: TransferGroupId) -> Result<Decision, Error> {
    decide_in(conn, group, Action::Confirm)
}

/// [`reject`] inside the caller's transaction.
pub fn reject_in(conn: &Connection, group: TransferGroupId) -> Result<Decision, Error> {
    decide_in(conn, group, Action::Reject)
}

/// [`unpair`] inside the caller's transaction.
pub fn unpair_in(conn: &Connection, group: TransferGroupId) -> Result<Decision, Error> {
    decide_in(conn, group, Action::Unpair)
}

/// Reverses `decision`, if the pair is as it left it: in the status the
/// decision set, with the same two transactions. The previous status is
/// restored under the checks that status needs now: a pair confirmed again
/// can't hold an excluded, split or linked row, and a suggested one can't
/// hold a split or linked row.
pub fn undo(db: &mut Database, decision: &Decision) -> Result<(), Error> {
    let tx = db
        .conn_mut()
        .transaction_with_behavior(TransactionBehavior::Immediate)?;
    undo_in(&tx, decision)?;
    tx.commit()?;
    Ok(())
}

/// [`undo`] inside the caller's transaction.
pub fn undo_in(conn: &Connection, decision: &Decision) -> Result<(), Error> {
    let group = load_group(conn, decision.group)?.ok_or(Error::Changed(decision.group))?;
    if group.status != decision.action.to() || group.transactions != decision.transactions {
        return Err(Error::Changed(decision.group));
    }
    let rows = load_rows(conn, decision.group)?;
    let restored = decision.action.from();
    if restored != TransferStatus::Rejected {
        for row in &rows {
            if row.split {
                return Err(Blocked::Split(row.id).into());
            }
            if row.linked {
                return Err(Blocked::Linked(row.id).into());
            }
            if restored == TransferStatus::Confirmed && row.excluded {
                return Err(Blocked::Excluded(row.id).into());
            }
        }
    }
    set_status(conn, decision.group, decision.action.to(), restored)?;
    tracing::info!(group = %decision.group, to = restored.as_str(), "undid a transfer decision");
    Ok(())
}

fn decide(db: &mut Database, group: TransferGroupId, action: Action) -> Result<Decision, Error> {
    let tx = db
        .conn_mut()
        .transaction_with_behavior(TransactionBehavior::Immediate)?;
    let decision = decide_in(&tx, group, action)?;
    tx.commit()?;
    Ok(decision)
}

fn decide_in(conn: &Connection, id: TransferGroupId, action: Action) -> Result<Decision, Error> {
    let group = load_group(conn, id)?.ok_or(Error::GroupNotFound(id))?;
    let rows = load_rows(conn, id)?;
    if let Some(blocked) = blocker(group.status, &rows, action) {
        return Err(blocked.into());
    }
    set_status(conn, id, action.from(), action.to())?;
    tracing::info!(
        group = %id,
        from = action.from().as_str(),
        to = action.to().as_str(),
        "decided a transfer pair"
    );
    Ok(Decision {
        group: id,
        action,
        transactions: [rows[0].id, rows[1].id],
    })
}

/// Moves a group from `from` to `to`; the status is checked again in the
/// statement, so a change since the caller read it is an error.
fn set_status(
    conn: &Connection,
    group: TransferGroupId,
    from: TransferStatus,
    to: TransferStatus,
) -> Result<(), Error> {
    let changed = conn.execute(
        "UPDATE transfer_group SET status = ?1 WHERE id = ?2 AND status = ?3",
        params![to.as_str(), group, from.as_str()],
    )?;
    if changed == 1 {
        Ok(())
    } else {
        Err(Error::Changed(group))
    }
}

/// What the checks need of a row in a group: read from the database here,
/// or from the register by the inspector's read model
/// ([`crate::reporting::pairing`]), so both answer with [`availability`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PairRow {
    pub id: TransactionId,
    pub excluded: bool,
    pub split: bool,
    /// The pot's side of a sub-account transfer.
    pub mirrored: bool,
    /// The money in or the purchase of a confirmed link.
    pub linked: bool,
}

fn load_rows(conn: &Connection, group: TransferGroupId) -> Result<Vec<PairRow>, Error> {
    let mut statement = conn.prepare(
        "SELECT t.id, t.excluded_reason IS NOT NULL, \
                EXISTS (SELECT 1 FROM transaction_split s WHERE s.transaction_id = t.id), \
                t.origin = 'mirrored' \
         FROM \"transaction\" t WHERE t.transfer_group_id = ?1 ORDER BY t.id",
    )?;
    let mut rows = statement
        .query_map([group], |row| {
            Ok(PairRow {
                id: row.get(0)?,
                excluded: row.get(1)?,
                split: row.get(2)?,
                mirrored: row.get(3)?,
                linked: false,
            })
        })?
        .collect::<Result<Vec<_>, _>>()?;
    for row in &mut rows {
        row.linked = link::is_linked(conn, row.id)?;
    }
    Ok(rows)
}

/// Why `action` isn't allowed on a group in `status` with `rows`.
/// Whether `action` is allowed on a group in `status` holding `rows`: the
/// checks every decision runs.
pub fn availability(status: TransferStatus, rows: &[PairRow], action: Action) -> Availability {
    blocker(status, rows, action).map_or(Ok(()), Err)
}

fn blocker(status: TransferStatus, rows: &[PairRow], action: Action) -> Option<Blocked> {
    if status != action.from() {
        return Some(Blocked::WrongStatus { found: status });
    }
    if rows.len() != 2 {
        return Some(Blocked::NotTwoSided);
    }
    match action {
        Action::Confirm => rows.iter().find_map(|row| {
            if row.excluded {
                Some(Blocked::Excluded(row.id))
            } else if row.split {
                Some(Blocked::Split(row.id))
            } else if row.linked {
                Some(Blocked::Linked(row.id))
            } else {
                None
            }
        }),
        Action::Reject => None,
        Action::Unpair => rows
            .iter()
            .any(|row| row.mirrored)
            .then_some(Blocked::PotPair),
    }
}
