//! Closing, reopening and deleting accounts (ROADMAP M3.17;
//! `docs/ARCHITECTURE.md` → "Decision record: accounts you keep yourself",
//! points 3–4, and "Decision record: asset and liability accounts
//! (built)").
//!
//! - **Closing** sets `account.closed_on` and deletes nothing: the account
//!   leaves the accounts panel and counts toward net worth up to and
//!   including that day ([`crate::reporting`]), and its transactions,
//!   balances and imports stay. Its open pots close with it, on the same
//!   day. Closing an asset or liability also writes a £0 valuation on that
//!   day, so it counts as nothing from then.
//! - **When**: never before what the account (or a pot closing with it)
//!   holds. A bank, cash or pot account can close on the day of its last
//!   transaction or balance; an asset or liability only after its last
//!   valuation, so the £0 never replaces a value the user entered and
//!   reopening loses nothing ([`Closing::earliest`]). The lock-date guard
//!   (M1 wave 4, point 21): never before `reconciled_through`, since the
//!   locked days' net worth would change ([`Error::CloseLocked`]).
//! - **Reopening** clears `closed_on`, on the account and on the pots that
//!   closed with it (the same day), and removes the £0 valuation closing
//!   wrote: the valuation on the closing day, which the date rule makes the
//!   one closing wrote. A pot whose parent is closed can't reopen alone.
//! - **Deleting** is for an account added by mistake: one with no
//!   transactions, imports, pots, rules or schedules, and no lock
//!   ([`Deletion`]). Its entered balances or valuations go with it. It
//!   can't be undone, so [`deletion`] says how many values go first.

use chrono::{Days, NaiveDate};
use rusqlite::{Connection, params};

use super::{Account, Error, date_key, find, from_row, load, optional_date};
use crate::db::Database;
use crate::id::AccountId;

/// What closing an account involves ([`closing`]).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Closing {
    pub account: Account,
    /// Its open pots, which close with it.
    pub pots: Vec<Account>,
    /// The earliest day it can close on (module docs); `None` when it holds
    /// nothing, so any day will do.
    pub earliest: Option<NaiveDate>,
}

impl Closing {
    /// Whether it can close on `date`: on or after [`Closing::earliest`].
    pub fn allows(&self, date: NaiveDate) -> bool {
        self.earliest.is_none_or(|earliest| date >= earliest)
    }
}

/// What closing `account` involves, if it is open.
pub fn closing(db: &Database, account: AccountId) -> Result<Closing, Error> {
    plan(db.conn(), account)
}

fn plan(conn: &Connection, id: AccountId) -> Result<Closing, Error> {
    let account = load(conn, id)?;
    if account.closed_on.is_some() {
        return Err(Error::AlreadyClosed);
    }
    let pots = open_pots(conn, id)?;
    let mut earliest = None;
    for held in std::iter::once(&account).chain(&pots) {
        earliest = earliest.max(earliest_for(conn, held)?);
    }
    Ok(Closing {
        account,
        pots,
        earliest,
    })
}

/// The earliest day `account` alone can close on (module docs), the lock
/// left out: [`close`] says so separately.
fn earliest_for(conn: &Connection, account: &Account) -> Result<Option<NaiveDate>, Error> {
    let last = |sql: &str| -> rusqlite::Result<Option<NaiveDate>> {
        conn.query_row(sql, [account.id], |row| optional_date(row, "date"))
    };
    let last_balance =
        last("SELECT max(date) AS date FROM balance_snapshot WHERE account_id = ?1")?;
    if !account.kind.holds_transactions() {
        // After the last valuation, so the £0 never replaces one.
        return Ok(last_balance.and_then(|date| date.checked_add_days(Days::new(1))));
    }
    let last_transaction =
        last("SELECT max(date) AS date FROM \"transaction\" WHERE account_id = ?1")?;
    let opening = account.opening_balance.map(|(date, _)| date);
    Ok(last_transaction.max(last_balance).max(opening))
}

fn open_pots(conn: &Connection, parent: AccountId) -> rusqlite::Result<Vec<Account>> {
    let mut statement = conn.prepare(&format!(
        "SELECT {} FROM account WHERE parent_id = ?1 AND closed_on IS NULL ORDER BY id",
        super::COLUMNS
    ))?;
    statement.query_map([parent], from_row)?.collect()
}

/// Closes `account` on `on` (module docs), with its open pots. Returns
/// every account it closed, the account first.
pub fn close(
    db: &mut Database,
    account: AccountId,
    on: NaiveDate,
) -> Result<Vec<AccountId>, Error> {
    let tx = db.conn_mut().transaction()?;
    let plan = plan(&tx, account)?;
    for held in std::iter::once(&plan.account).chain(&plan.pots) {
        if let Some(through) = held.reconciled_through
            && on < through
        {
            return Err(Error::CloseLocked { date: on, through });
        }
    }
    if let Some(earliest) = plan.earliest
        && on < earliest
    {
        return Err(Error::CloseTooEarly { earliest });
    }
    let day = date_key(on);
    let mut closed = Vec::new();
    for held in std::iter::once(&plan.account).chain(&plan.pots) {
        tx.execute(
            "UPDATE account SET closed_on = ?1 WHERE id = ?2",
            params![day, held.id],
        )?;
        if !held.kind.holds_transactions() {
            tx.execute(
                "INSERT INTO balance_snapshot (account_id, date, balance_minor, source) \
                 VALUES (?1, ?2, 0, 'valuation')",
                params![held.id, day],
            )?;
        }
        closed.push(held.id);
    }
    tx.commit()?;
    Ok(closed)
}

/// Reopens `account` (module docs), with the pots that closed with it.
/// Returns every account it reopened, the account first.
pub fn reopen(db: &mut Database, account: AccountId) -> Result<Vec<AccountId>, Error> {
    let tx = db.conn_mut().transaction()?;
    let stored = load(&tx, account)?;
    let Some(closed_on) = stored.closed_on else {
        return Err(Error::NotClosed);
    };
    if let Some(parent) = stored.parent
        && find(&tx, parent)?.is_some_and(|parent| parent.closed_on.is_some())
    {
        return Err(Error::ParentClosed);
    }
    let day = date_key(closed_on);
    let mut reopened = vec![stored.clone()];
    let mut statement = tx.prepare(&format!(
        "SELECT {} FROM account WHERE parent_id = ?1 AND closed_on = ?2 ORDER BY id",
        super::COLUMNS
    ))?;
    let pots = statement
        .query_map(params![account, day], from_row)?
        .collect::<Result<Vec<_>, _>>()?;
    drop(statement);
    reopened.extend(pots);
    for held in &reopened {
        if !held.kind.holds_transactions() {
            tx.execute(
                "DELETE FROM balance_snapshot \
                 WHERE account_id = ?1 AND date = ?2 AND source = 'valuation' \
                   AND balance_minor = 0 AND batch_id IS NULL",
                params![held.id, day],
            )?;
        }
        tx.execute(
            "UPDATE account SET closed_on = NULL WHERE id = ?1",
            [held.id],
        )?;
    }
    tx.commit()?;
    Ok(reopened.into_iter().map(|account| account.id).collect())
}

/// Every closed account, as they were created.
pub fn list_closed(db: &Database) -> Result<Vec<Account>, Error> {
    let mut statement = db.conn().prepare(&format!(
        "SELECT {} FROM account WHERE closed_on IS NOT NULL ORDER BY id",
        super::COLUMNS
    ))?;
    let accounts = statement
        .query_map([], from_row)?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(accounts)
}

/// Why an account can't be deleted ([`Deletion::refused`]).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum DeleteLock {
    Transactions,
    Imports,
    Pots,
    Rules,
    Schedules,
    /// Its history is locked through a date.
    Locked,
}

impl DeleteLock {
    /// Why, in a few words: "has transactions".
    pub const fn reason(self) -> &'static str {
        match self {
            DeleteLock::Transactions => "has transactions",
            DeleteLock::Imports => "has imports",
            DeleteLock::Pots => "has pots",
            DeleteLock::Rules => "has rules",
            DeleteLock::Schedules => "has schedules",
            DeleteLock::Locked => "is locked",
        }
    }
}

/// What deleting an account would remove ([`deletion`]).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Deletion {
    /// Its entered balances or valuations, and its opening balance: what
    /// goes with it.
    pub values: usize,
    /// Why it can't be deleted; `None` when it can.
    pub refused: Option<DeleteLock>,
}

/// What deleting `account` would remove, or why it can't be.
pub fn deletion(db: &Database, account: AccountId) -> Result<Deletion, Error> {
    assess(db.conn(), account)
}

fn assess(conn: &Connection, id: AccountId) -> Result<Deletion, Error> {
    let account = load(conn, id)?;
    let count =
        |sql: &str| -> rusqlite::Result<i64> { conn.query_row(sql, [id], |row| row.get(0)) };
    let refused = if count("SELECT COUNT(*) FROM \"transaction\" WHERE account_id = ?1")? > 0 {
        Some(DeleteLock::Transactions)
    } else if count("SELECT COUNT(*) FROM import_batch WHERE account_id = ?1")? > 0 {
        Some(DeleteLock::Imports)
    } else if count("SELECT COUNT(*) FROM account WHERE parent_id = ?1")? > 0 {
        Some(DeleteLock::Pots)
    } else if count("SELECT COUNT(*) FROM rule WHERE account_id = ?1")? > 0 {
        Some(DeleteLock::Rules)
    } else if count("SELECT COUNT(*) FROM schedule WHERE account_id = ?1")? > 0 {
        Some(DeleteLock::Schedules)
    } else if account.reconciled_through.is_some() {
        Some(DeleteLock::Locked)
    } else {
        None
    };
    let snapshots = count("SELECT COUNT(*) FROM balance_snapshot WHERE account_id = ?1")?;
    Ok(Deletion {
        values: usize::try_from(snapshots).unwrap_or_default()
            + usize::from(account.opening_balance.is_some()),
        refused,
    })
}

/// Deletes `account` and its entered balances or valuations, if nothing
/// else refers to it ([`Deletion`]). Can't be undone. Returns how many
/// values went with it.
pub fn delete(db: &mut Database, account: AccountId) -> Result<usize, Error> {
    let tx = db.conn_mut().transaction()?;
    let deletion = assess(&tx, account)?;
    if let Some(why) = deletion.refused {
        return Err(Error::Undeletable(why));
    }
    tx.execute(
        "DELETE FROM balance_snapshot WHERE account_id = ?1",
        [account],
    )?;
    tx.execute("DELETE FROM account WHERE id = ?1", [account])?;
    tx.commit()?;
    Ok(deletion.values)
}

#[cfg(test)]
mod tests;
