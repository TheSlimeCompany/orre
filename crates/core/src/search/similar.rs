//! A transaction's nearest neighbours: other transactions from the same
//! merchant, nearest in amount (`docs/architecture/reporting.md` → "Decision
//! record: similar transactions", ROADMAP FT.4).
//!
//! # The query
//!
//! The text is the row's *stem*, cut exactly as a rule proposal cuts it
//! (`rule::stem` over `merchant::clean`): over the bank's merchant column
//! when the row has one, else over its raw description. The index holds the
//! raw column, whose whitespace `merchant::clean` has collapsed, so the stem
//! isn't matched as one phrase. Each of its words of three characters or more
//! is a literal phrase ([`phrase`]), the phrases are ANDed, and an FTS5
//! column filter restricts them to the one column the stem came from:
//! `{merchant} : ("AMAZON" AND "PRIME")`. Shorter words, which trigrams
//! can't match, are left out of the query. A row with no stem, or no word of
//! three characters, has no neighbours. The filter and the `AND`s are written
//! here; the text stays inside the quoted phrases and is never FTS5 syntax.
//!
//! The index narrows; the words decide. It matches substrings and skips the
//! short words, so a match is kept only when its column, cleaned and
//! case-folded as the stem is, holds every word of the stem, short ones too,
//! as a word of its own: `WH SMITH` doesn't find `SMITHSON`, nor `EE LIMITED`
//! every other `LIMITED`. The text is only compared, never kept or logged.
//!
//! # Who counts
//!
//! A neighbour is another row of the same currency and the same sign (both
//! money out or both money in; a zero amount has none), not excluded as a
//! `duplicate` or `superseded`. [`Neighbours::Categorised`] also leaves out
//! rows with no category of their own: uncategorised rows, split rows (whose
//! lines hold the categories), the legs of a confirmed transfer and the
//! inflow of a confirmed refund link (they keep a stale category).
//!
//! # The order
//!
//! Best match first (`bm25`), then the amount nearest the row's, then the
//! newest date, then the highest ID, so the answer is the same for the same
//! database. Amounts stay integers.
//!
//! # One query for many rows
//!
//! The match score depends on the matched row and the query only, so every
//! subject with the same `PoolKey` (column, words, currency, sign, filter)
//! sees the same order of matches. `Pool::load` runs the one SQL query for
//! a key and ranks the matches with an integer `dense_rank()` of `bm25`;
//! `Pool::nearest` then orders them for one subject by (rank, distance in
//! amount, newest date, highest ID) in Rust. [`similar`] builds a one-off
//! pool; a caller with many rows (`inbox::similar`) keeps the pools.
//!
//! Reads only; opens no transaction of its own.

use std::cmp::Reverse;

use rusqlite::{Connection, OptionalExtension, params};

use super::phrase;
use crate::category::CategoryKey;
use crate::id::TransactionId;
use crate::merchant;
use crate::rule;

/// Which rows may be neighbours.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Neighbours {
    /// Any row.
    Any,
    /// Only rows with a category of their own: not uncategorised, not split,
    /// not a confirmed transfer leg or refund inflow.
    Categorised,
}

/// One neighbour.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Neighbour {
    pub transaction: TransactionId,
    /// Its own category; `None` for a row without one.
    pub category: Option<CategoryKey>,
}

/// What the matching rows of subjects share: the pool they are ranked in.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub(crate) struct PoolKey {
    column: &'static str,
    /// The FTS5 query, column filter included.
    query: String,
    currency: String,
    negative: bool,
    categorised: bool,
    /// Every word of the stem, short ones too, case-folded: a match must
    /// hold each as a word of its own ([`has_words`]).
    words: Vec<String>,
}

/// The row being matched, with the pool its neighbours are in.
#[derive(Debug)]
pub(crate) struct Subject {
    id: TransactionId,
    amount_minor: i64,
    pub(crate) key: PoolKey,
}

impl Subject {
    /// Reads `transaction`; `None` when it doesn't exist, has a zero amount
    /// or has no query (module docs).
    pub(crate) fn load(
        conn: &Connection,
        transaction: TransactionId,
        filter: Neighbours,
    ) -> rusqlite::Result<Option<Subject>> {
        let row = conn
            .prepare_cached(
                "SELECT amount_minor, currency, merchant, description_raw \
                 FROM \"transaction\" WHERE id = ?1",
            )?
            .query_row([transaction], |row| {
                Ok((
                    row.get::<_, i64>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, Option<String>>(2)?,
                    row.get::<_, String>(3)?,
                ))
            })
            .optional()?;
        let Some((amount_minor, currency, merchant, description)) = row else {
            return Ok(None);
        };
        if amount_minor == 0 {
            return Ok(None);
        }
        let (column, text) = match merchant
            .as_deref()
            .filter(|merchant| !merchant.trim().is_empty())
        {
            Some(bank_merchant) => ("merchant", bank_merchant),
            None => ("description_raw", description.as_str()),
        };
        let Some(stem) = rule::stem(&merchant::clean(text)) else {
            return Ok(None);
        };
        let phrases: Vec<String> = stem.split_whitespace().filter_map(phrase).collect();
        if phrases.is_empty() {
            return Ok(None);
        }
        Ok(Some(Subject {
            id: transaction,
            amount_minor,
            key: PoolKey {
                column,
                query: format!("{{{column}}} : ({})", phrases.join(" AND ")),
                currency,
                negative: amount_minor < 0,
                categorised: filter == Neighbours::Categorised,
                words: stem.split_whitespace().map(rule::fold).collect(),
            },
        }))
    }
}

/// One match in a pool.
struct Match {
    id: TransactionId,
    /// `dense_rank()` of `bm25` among the pool: 1 is the best match.
    rank: i64,
    amount_minor: i64,
    date: String,
    category: Option<CategoryKey>,
}

/// Every row that may be a neighbour of the subjects of one [`PoolKey`].
pub(crate) struct Pool {
    matches: Vec<Match>,
}

impl Pool {
    /// The rows matching `key`'s query that pass its filters, in one query,
    /// keeping those whose matched column holds every word of the stem
    /// ([`has_words`]).
    pub(crate) fn load(conn: &Connection, key: &PoolKey) -> rusqlite::Result<Pool> {
        // `column` is one of the two names `Subject::load` writes, never text.
        let column = key.column;
        let mut statement = conn.prepare_cached(&format!(
            "WITH hits AS MATERIALIZED ( \
                 SELECT rowid AS id, bm25(transaction_text) AS score \
                 FROM transaction_text WHERE transaction_text MATCH ?1) \
             SELECT t.id, dense_rank() OVER (ORDER BY h.score), t.amount_minor, t.date, c.key, \
                    t.{column} \
             FROM hits h \
             JOIN \"transaction\" t ON t.id = h.id \
             LEFT JOIN category c ON c.id = t.category_id \
             LEFT JOIN transfer_group g ON g.id = t.transfer_group_id \
             WHERE t.currency = ?2 \
               AND ((?3 AND t.amount_minor < 0) OR (NOT ?3 AND t.amount_minor > 0)) \
               AND (t.excluded_reason IS NULL OR t.excluded_reason NOT IN ('duplicate', 'superseded')) \
               AND (NOT ?4 OR (t.category_id IS NOT NULL \
                    AND NOT EXISTS (SELECT 1 FROM transaction_split s WHERE s.transaction_id = t.id) \
                    AND coalesce(g.status = 'confirmed', 0) = 0 \
                    AND NOT EXISTS (SELECT 1 FROM transaction_link k \
                         WHERE k.inflow_id = t.id AND k.kind = 'refund' AND k.status = 'confirmed')))"
        ))?;
        let mut matches = Vec::new();
        let mut rows = statement.query(params![
            key.query,
            key.currency,
            key.negative,
            key.categorised
        ])?;
        while let Some(row) = rows.next()? {
            let text: Option<String> = row.get(5)?;
            if !text.is_some_and(|text| has_words(&text, &key.words)) {
                continue;
            }
            let category: Option<String> = row.get(4)?;
            matches.push(Match {
                id: row.get(0)?,
                rank: row.get(1)?,
                amount_minor: row.get(2)?,
                date: row.get(3)?,
                category: category.and_then(|key| CategoryKey::new(&key).ok()),
            });
        }
        Ok(Pool { matches })
    }

    /// Up to `limit` nearest neighbours of `subject`, best first (module
    /// docs). `subject` must have the key the pool was loaded for.
    pub(crate) fn nearest(&self, subject: &Subject, limit: usize) -> Vec<Neighbour> {
        if limit == 0 {
            return Vec::new();
        }
        let amount = subject.amount_minor;
        // The best `limit` so far, nearest first.
        let mut candidates: Vec<&Match> = Vec::with_capacity(limit.saturating_add(1).min(64));
        for m in self.matches.iter().filter(|m| m.id != subject.id) {
            let key = nearness(m, amount);
            if candidates.len() >= limit
                && candidates
                    .last()
                    .is_some_and(|worst| key >= nearness(worst, amount))
            {
                continue;
            }
            let at = candidates.partition_point(|best| nearness(best, amount) < key);
            candidates.insert(at, m);
            candidates.truncate(limit);
        }
        candidates
            .into_iter()
            .map(|m| Neighbour {
                transaction: m.id,
                category: m.category.clone(),
            })
            .collect()
    }
}

/// Whether `text`, cleaned and case-folded as a stem is, holds every one of
/// `words` (folded) as a word of its own. Words are split at whitespace and
/// at `*`, where a stem is cut, so `NEAR*` holds `near`. The index matches
/// substrings and leaves out words under three characters; this is what
/// makes `WH SMITH` miss `SMITHSON` and `EE LIMITED` miss `THAMES WATER
/// LIMITED`. The text is only compared, never kept or logged.
fn has_words(text: &str, words: &[String]) -> bool {
    let folded = rule::fold(&merchant::clean(text));
    let held: Vec<&str> = folded
        .split(|c: char| c.is_whitespace() || c == '*')
        .filter(|word| !word.is_empty())
        .collect();
    words.iter().all(|word| held.contains(&word.as_str()))
}

/// The order of matches for a subject of `amount_minor`: smaller is nearer.
fn nearness(m: &Match, amount_minor: i64) -> (i64, i128, Reverse<&str>, Reverse<TransactionId>) {
    (
        m.rank,
        (i128::from(m.amount_minor) - i128::from(amount_minor)).abs(),
        Reverse(m.date.as_str()),
        Reverse(m.id),
    )
}

/// Up to `limit` nearest neighbours of `transaction`, best first (module
/// docs). Empty when the transaction doesn't exist or has no query or sign.
pub fn similar(
    conn: &Connection,
    transaction: TransactionId,
    filter: Neighbours,
    limit: usize,
) -> rusqlite::Result<Vec<Neighbour>> {
    let Some(subject) = Subject::load(conn, transaction, filter)? else {
        return Ok(Vec::new());
    };
    Ok(Pool::load(conn, &subject.key)?.nearest(&subject, limit))
}

#[cfg(test)]
#[path = "similar_tests.rs"]
mod similar_tests;
