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
//! Whether a transaction still *needs* a category is decided from the same
//! facts ([`CategoryFacts::needs_category`]): it takes one of its own and has
//! none, or it is split and a line has none. A confirmed transfer leg or a
//! confirmed refund's money in never does. `needs_category_in_batch` counts
//! a batch's rows for the import summary; the register's Uncategorised chip
//! reads the same rule from its rows (`RegisterRow::is_uncategorised`).
//!
//! Other writers of a category (rules at import and applied to history,
//! duplicate merges, removing a split, undo) don't ask: what they write is
//! theirs to decide.

use std::fmt;

use rusqlite::{Connection, OptionalExtension};

use crate::id::{BatchId, TransactionId};

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
    /// It has a category of its own (not through split lines).
    pub has_category: bool,
    /// Some split line has no category.
    pub split_line_uncategorised: bool,
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

    /// Whether a transaction with these facts still needs a category: it
    /// takes one of its own and has none; or it is split and some line has
    /// none. A confirmed transfer leg or a confirmed refund's money in never
    /// does.
    pub fn needs_category(self) -> bool {
        match self.no_own_category() {
            None => !self.has_category,
            Some(NoOwnCategory::Split) => self.split_line_uncategorised,
            Some(NoOwnCategory::Transfer | NoOwnCategory::Refund) => false,
        }
    }
}

/// The columns of the facts, in the order [`facts_from_row`] reads them,
/// over `"transaction" t` joined to its transfer group `g`
/// ([`FACTS_FROM`]). The one SQL for the facts, shared by [`facts_in`] and
/// [`needs_category_in_batch`].
const FACTS_COLUMNS: &str = "\
    EXISTS (SELECT 1 FROM transaction_split s WHERE s.transaction_id = t.id), \
    coalesce(g.status = 'confirmed', 0), \
    EXISTS (SELECT 1 FROM transaction_link k \
             WHERE k.inflow_id = t.id AND k.kind = 'refund' \
               AND k.status = 'confirmed'), \
    (t.category_id IS NOT NULL \
       AND NOT EXISTS (SELECT 1 FROM transaction_split s WHERE s.transaction_id = t.id)), \
    EXISTS (SELECT 1 FROM transaction_split s \
             WHERE s.transaction_id = t.id AND s.category_id IS NULL)";

/// What [`FACTS_COLUMNS`] reads from.
const FACTS_FROM: &str =
    "\"transaction\" t LEFT JOIN transfer_group g ON g.id = t.transfer_group_id";

/// Reads the facts from a row selected with [`FACTS_COLUMNS`] first.
fn facts_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<CategoryFacts> {
    Ok(CategoryFacts {
        split: row.get(0)?,
        confirmed_transfer: row.get(1)?,
        confirmed_refund_in: row.get(2)?,
        has_category: row.get(3)?,
        split_line_uncategorised: row.get(4)?,
    })
}

/// The facts of stored `transaction`; `None` when there is no such
/// transaction. Reads only; opens no transaction of its own.
pub fn facts_in(
    conn: &Connection,
    transaction: TransactionId,
) -> rusqlite::Result<Option<CategoryFacts>> {
    conn.prepare_cached(&format!(
        "SELECT {FACTS_COLUMNS} FROM {FACTS_FROM} WHERE t.id = ?1"
    ))?
    .query_row([transaction], facts_from_row)
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

/// How many of the imported rows of `batch` still need a category
/// ([`CategoryFacts::needs_category`]), for the import summary. Reads only.
pub(crate) fn needs_category_in_batch(
    conn: &Connection,
    batch: BatchId,
) -> rusqlite::Result<usize> {
    let mut statement = conn.prepare_cached(&format!(
        "SELECT {FACTS_COLUMNS} FROM {FACTS_FROM} \
          WHERE t.batch_id = ?1 AND t.origin = 'imported'"
    ))?;
    let mut count = 0;
    for facts in statement.query_map([batch], facts_from_row)? {
        if facts?.needs_category() {
            count += 1;
        }
    }
    Ok(count)
}

#[cfg(test)]
#[path = "assign_tests.rs"]
mod tests;
