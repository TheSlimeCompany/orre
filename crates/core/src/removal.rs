//! Removing transactions: the one place a transaction is deleted.
//!
//! Deleting a manual row ([`crate::manual::delete`]) and undoing an import
//! ([`crate::import::undo`]) both remove transactions, and several tables
//! point at a transaction with no `ON DELETE`. [`remove`] dissolves the
//! rows' transfer groups, clears every row that refers to them
//! ([`REFERENCES`], which a test compares with the live schema, so a table
//! that starts pointing at a transaction can't be forgotten) and deletes the
//! transactions, so each caller keeps only its own checks and steps.
//!
//! What a clear means for each table: split lines and tag links go (the tags
//! stay); change items go (the batches stay, with what is left); a link goes
//! whichever side the row is on, so money in from another import is unlinked
//! again; a duplicate candidate naming the row goes, whatever its status; a
//! schedule match goes, which reopens its occurrence. A row that one of the
//! removed rows superseded (`superseded_by`, the bank's correction of it)
//! isn't deleted: its `superseded` exclusion is cleared, which restores it
//! exactly as it was before the correction came in, and the correction's
//! schedule match (usually the one it took from that row) moves to it
//! (`schedule::move_match`) rather than going, so its occurrence stays
//! fulfilled.
//!
//! Nothing here opens a DB transaction: the caller's wraps the whole removal,
//! so a failure leaves everything as it was. Nothing is logged.

use rusqlite::types::ToSql;
use rusqlite::{Connection, params_from_iter};

use crate::id::{BatchId, TransactionId, TransferGroupId};
use crate::schedule;

/// Every column that references `"transaction"`, as `(table, column)`; each
/// is cleared by [`remove`] before the transactions go.
const REFERENCES: [(&str, &str); 8] = [
    ("transaction_split", "transaction_id"),
    ("transaction_tag", "transaction_id"),
    ("change_item", "transaction_id"),
    ("transaction_link", "inflow_id"),
    ("transaction_link", "target_id"),
    ("duplicate_candidate", "transaction_a"),
    ("duplicate_candidate", "transaction_b"),
    ("schedule_match", "transaction_id"),
];

/// The one column of `"transaction"` itself that references a transaction:
/// the row that superseded it. [`remove`] restores such a row rather than
/// clearing a reference to it.
const SUPERSEDED_BY: (&str, &str) = ("transaction", "superseded_by");

/// Which transactions [`remove`] removes.
#[derive(Debug, Clone, Copy)]
pub(crate) enum Scope {
    /// One transaction.
    Row(TransactionId),
    /// Every transaction of an import batch, mirrored pot rows included.
    Import(BatchId),
}

impl Scope {
    /// A query for the IDs of the transactions in scope, and its one
    /// parameter.
    fn rows(&self) -> (&'static str, &dyn ToSql) {
        match self {
            Scope::Row(id) => ("SELECT id FROM \"transaction\" WHERE id = ?1", id),
            Scope::Import(batch) => ("SELECT id FROM \"transaction\" WHERE batch_id = ?1", batch),
        }
    }
}

/// What [`remove`] did beyond deleting the transactions.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Removed {
    /// The transfer groups it dissolved, because a row in scope belonged to
    /// one. Their rows outside the scope are ordinary transactions now.
    pub dissolved_groups: Vec<TransferGroupId>,
}

/// Removes the transactions in `scope` with everything that refers to them,
/// inside the caller's DB transaction. Partner rows of a transfer pair stay
/// as ordinary transactions. Anything the caller keeps about the rows
/// (balances, flags, batch status) is the caller's to clear.
pub(crate) fn remove(conn: &Connection, scope: Scope) -> rusqlite::Result<Removed> {
    let (rows, parameter) = scope.rows();

    let dissolved_groups: Vec<TransferGroupId> = {
        let mut statement = conn.prepare(&format!(
            "SELECT DISTINCT transfer_group_id FROM \"transaction\" \
             WHERE id IN ({rows}) AND transfer_group_id IS NOT NULL"
        ))?;
        statement
            .query_map([parameter], |row| row.get(0))?
            .collect::<Result<_, _>>()?
    };
    for group in &dissolved_groups {
        dissolve_transfer_group(conn, *group)?;
    }

    // The rows the removed ones superseded, which count again once they
    // are gone (and not before: one live row per source ID), with the row
    // that superseded each.
    let (table, column) = SUPERSEDED_BY;
    let restored: Vec<(TransactionId, TransactionId)> = {
        let mut statement = conn.prepare(&format!(
            "SELECT id, {column} FROM \"{table}\" WHERE {column} IN ({rows}) AND id NOT IN ({rows})"
        ))?;
        statement
            .query_map([parameter], |row| Ok((row.get(0)?, row.get(1)?)))?
            .collect::<Result<_, _>>()?
    };
    // A correction's schedule match goes back to the row it replaced,
    // before the matches of removed rows are deleted. It is the one the
    // correction took from that row, one the import made for it, or one
    // the user made on it since; each names the same payment, so it
    // belongs on the restored row and the occurrence stays fulfilled.
    for &(id, by) in &restored {
        schedule::move_match(conn, by, id)?;
    }

    for (table, column) in REFERENCES {
        conn.execute(
            &format!("DELETE FROM {table} WHERE {column} IN ({rows})"),
            params_from_iter([parameter]),
        )?;
    }
    conn.execute(
        &format!("DELETE FROM \"transaction\" WHERE id IN ({rows})"),
        params_from_iter([parameter]),
    )?;
    let mut restore = conn.prepare(&format!(
        "UPDATE \"{table}\" SET excluded_reason = NULL, excluded_note = NULL, \
         excluded_at = NULL, {column} = NULL WHERE id = ?1"
    ))?;
    for (id, _) in restored {
        restore.execute([id])?;
    }
    Ok(Removed { dissolved_groups })
}

/// Deletes a transfer group, leaving its rows as ordinary transactions.
pub(crate) fn dissolve_transfer_group(
    conn: &Connection,
    group: TransferGroupId,
) -> rusqlite::Result<()> {
    conn.execute(
        "UPDATE \"transaction\" SET transfer_group_id = NULL WHERE transfer_group_id = ?1",
        [group],
    )?;
    conn.execute("DELETE FROM transfer_group WHERE id = ?1", [group])?;
    Ok(())
}

#[cfg(test)]
mod tests;
