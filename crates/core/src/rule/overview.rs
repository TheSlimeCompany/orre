//! The Rules table's read model (`DESIGN.md` → "Rules", `B1J-0`): every rule
//! in evaluation order with what it decides today.
//!
//! **Matches** counts the transactions a rule decides today: it matches them
//! and wins at least one field there, because no rule above already set the
//! fields it sets (decision record "rules", point 1). This is the rule set
//! evaluated against every stored transaction now, as an import would, not
//! what the transactions hold: rule edits don't re-run on history (point 4).
//! A rule that matches transactions but decides none is **never reached**,
//! and [`RuleSummary::caught_by`] names the rule above that matches most of
//! them.

use std::collections::{BTreeSet, HashMap};

use chrono::NaiveDate;
use rusqlite::{Transaction, TransactionBehavior};

use super::apply::load_candidates;
use super::engine::{Outcome, RuleSet};
use super::store::{self, StoreError, StoredRule};
use crate::db::Database;
use crate::id::RuleId;

/// The rule above a never-reached rule that catches its transactions.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Catch {
    /// The rule.
    pub rule: RuleId,
    /// How many of the never-reached rule's matches it also matches.
    pub rows: usize,
}

/// One rule as the Rules table shows it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RuleSummary {
    /// The rule.
    pub rule: StoredRule,
    /// The transactions its conditions match.
    pub matches: usize,
    /// The transactions it decides today: it matches them and wins a field.
    pub decides: usize,
    /// The newest of those transactions' dates.
    pub last: Option<NaiveDate>,
    /// For a never-reached rule, the rule above that matches most of its
    /// transactions (the earlier one on a tie).
    pub caught_by: Option<Catch>,
}

impl RuleSummary {
    /// Whether it matches transactions but always loses to rules above.
    pub fn never_reached(&self) -> bool {
        self.matches > 0 && self.decides == 0
    }
}

/// The rules that won a field in `outcome`.
fn winners(outcome: &Outcome) -> BTreeSet<RuleId> {
    let mut rules = BTreeSet::new();
    rules.extend(outcome.category.as_ref().map(|set| set.rule));
    rules.extend(outcome.split.as_ref().map(|set| set.rule));
    rules.extend(outcome.merchant.as_ref().map(|set| set.rule));
    rules.extend(outcome.tags.iter().map(|set| set.rule));
    rules.extend(outcome.note.as_ref().map(|set| set.rule));
    rules.extend(outcome.exclude.as_ref().map(|set| set.rule));
    rules.extend(outcome.share.as_ref().map(|set| set.rule));
    rules
}

/// Every rule in evaluation order, with what it decides today (module
/// docs). A rule's index is its position. Reads only.
pub fn overview(db: &Database) -> Result<Vec<RuleSummary>, StoreError> {
    let tx = Transaction::new_unchecked(db.conn(), TransactionBehavior::Deferred)?;
    let stored = store::load_all(&tx)?;
    let rules = RuleSet::new(stored.iter().cloned());
    let rows = load_candidates(&tx, "1", [])?;
    drop(tx);

    let position: HashMap<RuleId, usize> = stored
        .iter()
        .enumerate()
        .map(|(at, rule)| (rule.id(), at))
        .collect();
    let mut matches = vec![0usize; stored.len()];
    let mut decides = vec![0usize; stored.len()];
    let mut last: Vec<Option<NaiveDate>> = vec![None; stored.len()];
    // For each rule, how many of its matches each rule above also matches.
    let mut above: Vec<HashMap<RuleId, usize>> = vec![HashMap::new(); stored.len()];
    for row in &rows {
        let input = row.input();
        let matching = rules.matching(&input);
        if matching.is_empty() {
            continue;
        }
        for (at, rule) in matching.iter().enumerate() {
            let index = position[rule];
            matches[index] += 1;
            for earlier in &matching[..at] {
                *above[index].entry(*earlier).or_default() += 1;
            }
        }
        for rule in winners(&rules.evaluate(&input)) {
            let index = position[&rule];
            decides[index] += 1;
            last[index] = last[index].max(Some(row.date));
        }
    }

    Ok(stored
        .into_iter()
        .enumerate()
        .map(|(index, rule)| {
            let never_reached = matches[index] > 0 && decides[index] == 0;
            let caught_by = never_reached
                .then(|| {
                    above[index]
                        .iter()
                        .max_by_key(|(rule, rows)| (**rows, std::cmp::Reverse(position[rule])))
                        .map(|(rule, rows)| Catch {
                            rule: *rule,
                            rows: *rows,
                        })
                })
                .flatten();
            RuleSummary {
                rule,
                matches: matches[index],
                decides: decides[index],
                last: last[index],
                caught_by,
            }
        })
        .collect())
}

/// How many transactions rule `rule` filed (their `rule_id`) that
/// `definition` also matches: whether a rule not stored yet (an agent's
/// suggestion) would take over rows this rule decides. Reads only.
pub fn overlap_with_rows_of(
    conn: &rusqlite::Connection,
    definition: &super::RuleDefinition,
    rule: RuleId,
) -> Result<usize, StoreError> {
    // Any ID works: the set holds this one rule.
    let alone = RuleSet::new([StoredRule::unsaved(RuleId::new(0), definition.clone())]);
    Ok(load_candidates(conn, "t.rule_id = ?1", [rule])?
        .iter()
        .filter(|row| !alone.matching(&row.input()).is_empty())
        .count())
}
