//! Merchant names' read model (`DESIGN.md` → "Rules", "Merchant names",
//! `G2O-0`): every alias with the transactions that show its name.
//!
//! A transaction shows an alias's name when its name is derived
//! (`merchant_display_source` is NULL) and its cleaned bank text resolves
//! to that alias ([`AliasSet::resolve`]): a name a rule or the user set
//! isn't counted, since the alias doesn't decide it. Resolution is the same
//! code that names rows, so the counts can't drift from what's shown.

use std::collections::HashMap;

use chrono::NaiveDate;
use rusqlite::{Transaction, TransactionBehavior};

use super::{AliasSet, Error, MerchantAlias, clean, source_text, store};
use crate::db::Database;
use crate::id::MerchantAliasId;

/// One cleaned bank text an alias catches.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Caught {
    /// The cleaned bank text (untrusted: shown as plain text only).
    pub text: String,
    /// How many transactions have it.
    pub transactions: usize,
    /// The newest of their dates.
    pub last: NaiveDate,
}

/// One alias as the Merchant names table shows it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AliasSummary {
    pub alias: MerchantAlias,
    /// When it was added (UTC, RFC 3339 with milliseconds).
    pub created_at: String,
    /// The transactions that show its name.
    pub matches: usize,
    /// The newest of their dates.
    pub last: Option<NaiveDate>,
    /// The cleaned bank texts it catches, newest first (then A–Z).
    pub catches: Vec<Caught>,
}

/// Every alias, sorted by the name it gives (ignoring case, then by ID),
/// with what it catches today. Reads only.
pub fn overview(db: &Database) -> Result<Vec<AliasSummary>, Error> {
    let tx = Transaction::new_unchecked(db.conn(), TransactionBehavior::Deferred)?;
    let aliases = store::load_all(&tx)?;
    let created: HashMap<MerchantAliasId, String> = {
        let mut statement = tx.prepare("SELECT id, created_at FROM merchant_alias")?;
        statement
            .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))?
            .collect::<Result<_, _>>()?
    };
    let set = AliasSet::new(aliases.iter().cloned());
    // Per alias, per cleaned text: count and newest date.
    let mut caught: HashMap<MerchantAliasId, HashMap<String, (usize, NaiveDate)>> = HashMap::new();
    if !set.is_empty() {
        let mut statement = tx.prepare(
            "SELECT merchant, description_raw, date FROM \"transaction\" \
             WHERE merchant_display_source IS NULL",
        )?;
        let mut rows = statement.query([])?;
        while let Some(row) = rows.next()? {
            let merchant: Option<String> = row.get(0)?;
            let description: String = row.get(1)?;
            let date = crate::account::date(row, "date")?;
            let Some(text) = source_text(merchant.as_deref(), &description) else {
                continue;
            };
            let cleaned = clean(text);
            let Some(alias) = set.resolve(&cleaned).alias else {
                continue;
            };
            let entry = caught
                .entry(alias)
                .or_default()
                .entry(cleaned)
                .or_insert((0, date));
            entry.0 += 1;
            entry.1 = entry.1.max(date);
        }
    }
    drop(tx);

    let mut summaries: Vec<AliasSummary> = aliases
        .into_iter()
        .map(|alias| {
            let mut catches: Vec<Caught> = caught
                .remove(&alias.id())
                .unwrap_or_default()
                .into_iter()
                .map(|(text, (transactions, last))| Caught {
                    text,
                    transactions,
                    last,
                })
                .collect();
            catches.sort_by(|a, b| b.last.cmp(&a.last).then_with(|| a.text.cmp(&b.text)));
            AliasSummary {
                created_at: created.get(&alias.id()).cloned().unwrap_or_default(),
                matches: catches.iter().map(|caught| caught.transactions).sum(),
                last: catches.iter().map(|caught| caught.last).max(),
                catches,
                alias,
            }
        })
        .collect();
    summaries.sort_by(|a, b| {
        let name = |s: &AliasSummary| s.alias.definition().name().as_str().to_lowercase();
        name(a).cmp(&name(b)).then(a.alias.id().cmp(&b.alias.id()))
    });
    Ok(summaries)
}
