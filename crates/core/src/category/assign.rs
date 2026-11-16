//! Whether a transaction takes a category of its own (`ARCHITECTURE.md` →
//! "Decision record: the Transactions view", point 3; "Decision record:
//! refunds, reimbursements and settlements (built)", point 5).
//!
//! A transaction takes one unless it is
//!
//! - **split**: each line takes its own, in the split editor (M3.9);
//! - **a leg of a confirmed transfer pair**: money moved between the
//!   user's accounts counts as neither spending nor income (M3.8); a pair
//!   Coffer only suggested doesn't count;
//! - **the money in of a confirmed refund**: it counts in its purchase's
//!   category until it's unlinked (M3.11); a suggested refund doesn't
//!   count, and nor does the purchase side.
//!
//! The rule is [`CategoryFacts::no_own_category`], one function over three
//! facts. The facts are read in two places: from the database here
//! ([`no_own_category_in`]), for the user's edits (`inbox::own::recategorize`)
//! and an approved categorisation (`inbox::approve`); and from the
//! Transactions table's rows in memory
//! (`reporting::register::RegisterRow::no_own_category`), so the app offers
//! the edit only where core takes it. A test runs the same rows through
//! both.
//!
//! Other writers of a category (rules at import and applied to history,
//! duplicate merges, removing a split, undo) don't ask: what they write is
//! theirs to decide.

use std::fmt;

use rusqlite::{Connection, OptionalExtension};

use crate::id::TransactionId;

/// Why a transaction takes no category of its own. Its words name no
/// transaction text.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum NoOwnCategory {
    /// Split: its lines take categories.
    Split,
    /// A leg of a confirmed transfer pair.
    Transfer,
    /// The money in of a confirmed refund: its purchase's category counts.
    Refund,
}

impl fmt::Display for NoOwnCategory {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            NoOwnCategory::Split => "it is split, so its lines take categories",
            NoOwnCategory::Transfer => "it is in a confirmed transfer, which takes no category",
            NoOwnCategory::Refund => {
                "it is a linked refund, which counts in its purchase's category"
            }
        })
    }
}

/// What decides whether a transaction takes a category of its own.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct CategoryFacts {
    /// It has split lines.
    pub split: bool,
    /// It is in a `confirmed` transfer group.
    pub confirmed_transfer: bool,
    /// It is the money in of a `confirmed` refund link.
    pub confirmed_refund_in: bool,
}

impl CategoryFacts {
    /// Why a transaction with these facts takes no category of its own;
    /// `None` when it takes one. The rule (module docs): a transfer first,
    /// then a split, then a refund, though the schema keeps them apart.
    pub fn no_own_category(self) -> Option<NoOwnCategory> {
        if self.confirmed_transfer {
            Some(NoOwnCategory::Transfer)
        } else if self.split {
            Some(NoOwnCategory::Split)
        } else if self.confirmed_refund_in {
            Some(NoOwnCategory::Refund)
        } else {
            None
        }
    }
}

/// The facts of stored `transaction`; `None` when there is no such
/// transaction. Reads only; opens no transaction of its own.
pub fn facts_in(
    conn: &Connection,
    transaction: TransactionId,
) -> rusqlite::Result<Option<CategoryFacts>> {
    conn.prepare_cached(
        "SELECT EXISTS (SELECT 1 FROM transaction_split s WHERE s.transaction_id = t.id), \
                coalesce(g.status = 'confirmed', 0), \
                EXISTS (SELECT 1 FROM transaction_link k \
                         WHERE k.inflow_id = t.id AND k.kind = 'refund' \
                           AND k.status = 'confirmed') \
           FROM \"transaction\" t LEFT JOIN transfer_group g ON g.id = t.transfer_group_id \
          WHERE t.id = ?1",
    )?
    .query_row([transaction], |row| {
        Ok(CategoryFacts {
            split: row.get(0)?,
            confirmed_transfer: row.get(1)?,
            confirmed_refund_in: row.get(2)?,
        })
    })
    .optional()
}

/// Why stored `transaction` takes no category of its own, if it doesn't;
/// `None` when it takes one or there is no such transaction (the caller's
/// write reports that). Reads only; opens no transaction of its own.
pub fn no_own_category_in(
    conn: &Connection,
    transaction: TransactionId,
) -> rusqlite::Result<Option<NoOwnCategory>> {
    Ok(facts_in(conn, transaction)?.and_then(CategoryFacts::no_own_category))
}

#[cfg(test)]
#[path = "assign_tests.rs"]
mod tests;
