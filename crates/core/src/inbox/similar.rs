//! Orre's own category suggestions after an import (ROADMAP FT.4): an
//! imported row with no category that sits among rows the user already
//! categorised gets a `categorization` suggestion from [`ORRE_SOURCE`].
//!
//! # When
//!
//! [`suggest_for_batch`] runs after an import has committed, in a DB
//! transaction of its own (`import::commit`), never inside the import and
//! never in a rehearsal. If it fails the import still stands.
//!
//! # Which rows
//!
//! Rows of the batch (`origin = 'imported'`) that take a category of their
//! own ([`CategoryFacts::no_own_category`]) and have none, are not excluded
//! as a `duplicate` or `superseded` copy of another row (a rule can exclude
//! one as it is imported), and are not in a `suggested` or `confirmed`
//! transfer. A row already named in any
//! `categorization` suggestion, whatever its status or source, is left
//! alone: the user or a client has already been asked. The named IDs are read
//! once per call.
//!
//! # The vote
//!
//! Each row's five nearest categorised neighbours (`search::similar`) vote
//! with their categories, from any source. Rows that share a query share one
//! SQL query: the batch keeps a [`Pool`] per [`PoolKey`], so a merchant with
//! thousands of categorised rows is read once for all the batch's rows. A category held by at least
//! [`MIN_AGREEING`] of them is proposed for the row; with five voters at most
//! one category can.
//!
//! # Filing
//!
//! One `categorization` suggestion per category, its items in transaction ID
//! order, split at [`payload::MAX_ITEMS`], through `file_in`. The rationale is a
//! fixed sentence, never a merchant or bank text. Orre's rule suggestions
//! (`inbox::own`) share its pending cap ([`super::MAX_PENDING_PER_SOURCE`]), so
//! filing stops once Orre has `MAX_PENDING_CATEGORY_SUGGESTIONS` pending
//! suggestions of any kind, leaving room for them. When it already has that
//! many, no neighbour is read at all. A payload that doesn't
//! check is skipped. Only
//! database errors propagate. Logs name IDs and counts, never text.
//!
//! The same database gives the same suggestions: neighbours and rows are
//! ordered, and a second run names no row twice.

use std::collections::{BTreeMap, HashMap, HashSet};

use rusqlite::Connection;

use super::payload::{self, CategorizationItem, Target};
use super::{Error, ORRE_SOURCE, Payload, file_in};
use crate::category::CategoryKey;
use crate::category::assign;
use crate::id::{BatchId, TransactionId};
use crate::search::similar::{Neighbours, Pool, PoolKey, Subject};

#[cfg(test)]
#[path = "similar_tests.rs"]
mod similar_tests;

/// How many nearest categorised neighbours vote.
const NEIGHBOURS: usize = 5;

/// How many of them must hold one category to propose it.
const MIN_AGREEING: usize = 3;

/// Orre stops filing category suggestions at this many pending suggestions of
/// its own, of any kind: the rest of [`super::MAX_PENDING_PER_SOURCE`] is for the
/// rule suggestions.
const MAX_PENDING_CATEGORY_SUGGESTIONS: usize = 150;

/// The rationale of every suggestion made here.
const RATIONALE: &str = "Similar transactions have this category.";

/// Files Orre's category suggestions for the imported rows of `batch` (module
/// docs); returns how many suggestions it filed.
pub(crate) fn suggest_for_batch(conn: &Connection, batch: BatchId) -> rusqlite::Result<usize> {
    // Nothing can be filed: don't read the neighbours under the write lock.
    if pending(conn)? >= MAX_PENDING_CATEGORY_SUGGESTIONS {
        tracing::debug!(batch = %batch, "Orre's category suggestions stopped at their pending cap");
        return Ok(0);
    }
    let named = already_named(conn)?;
    let mut pools: HashMap<PoolKey, Pool> = HashMap::new();
    let mut by_category: BTreeMap<CategoryKey, Vec<TransactionId>> = BTreeMap::new();
    for row in candidates(conn, batch)? {
        if named.contains(&row.get()) {
            continue;
        }
        let Some(subject) = Subject::load(conn, row, Neighbours::Categorised)? else {
            continue;
        };
        if !pools.contains_key(&subject.key) {
            pools.insert(subject.key.clone(), Pool::load(conn, &subject.key)?);
        }
        let neighbours = pools[&subject.key].nearest(&subject, NEIGHBOURS);
        let mut votes: BTreeMap<CategoryKey, usize> = BTreeMap::new();
        for category in neighbours.into_iter().filter_map(|n| n.category) {
            *votes.entry(category).or_default() += 1;
        }
        if let Some((category, _)) = votes.into_iter().find(|(_, n)| *n >= MIN_AGREEING) {
            by_category.entry(category).or_default().push(row);
        }
    }

    let mut filed = 0;
    for (category, rows) in by_category {
        for chunk in rows.chunks(payload::MAX_ITEMS) {
            if pending(conn)? >= MAX_PENDING_CATEGORY_SUGGESTIONS {
                tracing::debug!(batch = %batch, "Orre's category suggestions stopped at their pending cap");
                return Ok(filed);
            }
            let payload = Payload::Categorization(
                chunk
                    .iter()
                    .map(|row| CategorizationItem {
                        target: Target::Transaction(*row),
                        category: category.clone(),
                    })
                    .collect(),
            );
            match file_in(conn, ORRE_SOURCE, &payload, Some(RATIONALE)) {
                Ok(result) => filed += usize::from(!result.duplicate),
                Err(Error::Database(error)) => return Err(error),
                Err(Error::TooManyPending) => {
                    tracing::debug!(batch = %batch, "Orre's category suggestions stopped at the pending cap");
                    return Ok(filed);
                }
                Err(error) => {
                    tracing::debug!(batch = %batch, %error, "Orre's category suggestion wasn't filed");
                }
            }
        }
    }
    Ok(filed)
}

/// The batch's imported rows that need a category of their own, aren't
/// excluded as a `duplicate` or `superseded` copy of another row and aren't
/// in a suggested or confirmed transfer, by ID.
fn candidates(conn: &Connection, batch: BatchId) -> rusqlite::Result<Vec<TransactionId>> {
    let mut statement = conn.prepare_cached(
        "SELECT t.id FROM \"transaction\" t \
         LEFT JOIN transfer_group g ON g.id = t.transfer_group_id \
         WHERE t.batch_id = ?1 AND t.origin = 'imported' AND t.category_id IS NULL \
           AND (t.excluded_reason IS NULL OR t.excluded_reason NOT IN ('duplicate', 'superseded')) \
           AND coalesce(g.status IN ('suggested', 'confirmed'), 0) = 0 \
           AND NOT EXISTS (SELECT 1 FROM transaction_split s WHERE s.transaction_id = t.id) \
         ORDER BY t.id",
    )?;
    let ids = statement
        .query_map([batch], |row| row.get::<_, TransactionId>(0))?
        .collect::<Result<Vec<_>, _>>()?;
    let mut rows = Vec::with_capacity(ids.len());
    for id in ids {
        let takes_category = assign::facts_in(conn, id)?
            .is_some_and(|facts| facts.no_own_category().is_none() && !facts.has_category);
        if takes_category {
            rows.push(id);
        }
    }
    Ok(rows)
}

/// How many suggestions Orre has pending, of any kind.
fn pending(conn: &Connection) -> rusqlite::Result<usize> {
    let count: i64 = conn
        .prepare_cached(
            "SELECT COUNT(*) FROM suggestion WHERE source_client = ?1 AND status = 'pending'",
        )?
        .query_row([ORRE_SOURCE], |row| row.get(0))?;
    Ok(usize::try_from(count).unwrap_or(usize::MAX))
}

/// The IDs of the transactions any `categorization` suggestion, of any
/// status or source, names.
fn already_named(conn: &Connection) -> rusqlite::Result<HashSet<i64>> {
    conn.prepare_cached(
        "SELECT DISTINCT json_extract(j.value, '$.transaction_id') FROM suggestion s, \
            json_each(s.payload_json, '$.items') j \
          WHERE s.kind = 'categorization' \
            AND json_type(j.value, '$.transaction_id') = 'integer'",
    )?
    .query_map([], |row| row.get::<_, i64>(0))?
    .collect()
}
