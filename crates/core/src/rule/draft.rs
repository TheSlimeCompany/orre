//! A rule being made in the Rules view, not saved yet (`DESIGN.md` →
//! "Rules", "Apply to past transactions", `EFT-0`).
//!
//! [`preview_draft`] stores the rule at its position in a DB transaction
//! that is rolled back ([`change::dry_run`]) and plans applying it there with
//! [`preview_apply`](super::preview_apply)'s own code, so the preview is
//! what saving and applying then do. [`save_draft`] stores it for real and,
//! when asked, applies the preview in the same DB transaction ("Create rule
//! and change 4"), or only stores it ("Create rule only").

use std::collections::BTreeSet;

use chrono::NaiveDate;
use rusqlite::{Connection, TransactionBehavior};

use super::apply::{
    Applied, ApplyError, ApplyPreview, Target, commit_apply_in, load_candidates, plan_history,
};
use super::engine::RuleSet;
use super::store::{self, create_at_in};
use super::{RuleDefinition, RuleOrigin};
use crate::category::CategoryKey;
use crate::change;
use crate::db::Database;
use crate::id::{AccountId, RuleId, TransactionId};
use crate::money::{Currency, Money};

/// A rule not saved yet: what it is, where it goes, and where it came from.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Draft {
    /// What it matches and does.
    pub definition: RuleDefinition,
    /// Its place in the list, 0 first (a new rule goes first: decision
    /// record "rules", point 2). Past the end means last.
    pub position: usize,
    /// Where it came from.
    pub origin: RuleOrigin,
}

/// What saving a draft and applying it would do.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DraftPreview {
    /// The apply, planned with the draft stored at its position. Its rule
    /// ID is the one the draft got in the rolled-back transaction.
    pub preview: ApplyPreview,
    /// The transactions the draft's conditions match, whatever they hold.
    pub matches: usize,
    /// The stored rules that also match at least one of those
    /// transactions, in evaluation order ("This one overlaps none of your
    /// 14 rules").
    pub overlaps: Vec<RuleId>,
}

/// What [`save_draft`] did.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Saved {
    /// The new rule.
    pub rule: RuleId,
    /// The apply, when one was asked for.
    pub applied: Option<Applied>,
}

/// Stores `draft` in the caller's DB transaction, clamping its position to
/// the list.
fn stage(conn: &Connection, draft: &Draft) -> Result<RuleId, ApplyError> {
    let count = store::load_all(conn)?.len();
    Ok(create_at_in(
        conn,
        &draft.definition,
        draft.position.min(count),
        draft.origin,
    )?)
}

/// What saving `draft` and applying it to past transactions would change,
/// and which stored rules it overlaps. Reads only: the rule is stored in a
/// transaction that is rolled back.
pub fn preview_draft(db: &Database, draft: &Draft) -> Result<DraftPreview, ApplyError> {
    change::dry_run(db, |conn| {
        let id = stage(conn, draft)?;
        let preview = plan_history(conn, id)?;
        let rules = RuleSet::load(conn)?;
        let mut matches = 0;
        let mut overlapping = BTreeSet::new();
        for row in load_candidates(conn, "1", [])? {
            let matching = rules.matching(&row.input());
            if matching.contains(&id) {
                matches += 1;
                overlapping.extend(matching.into_iter().filter(|rule| *rule != id));
            }
        }
        let overlaps = rules
            .ids()
            .filter(|rule| overlapping.contains(rule))
            .collect();
        Ok(DraftPreview {
            preview,
            matches,
            overlaps,
        })
    })
}

/// `preview` as planned for rule `rule` rather than the rule it names.
fn renamed(preview: &ApplyPreview, rule: RuleId) -> ApplyPreview {
    let from = preview.rule;
    let swap = |id: RuleId| if id == from { rule } else { id };
    let mut renamed = preview.clone();
    renamed.rule = rule;
    for row in &mut renamed.rows {
        for change in &mut row.changes {
            change.rule = swap(change.rule);
        }
    }
    for skipped in &mut renamed.skipped_splits {
        skipped.rule = swap(skipped.rule);
    }
    renamed
}

/// Stores `draft` and, with `apply`, applies its preview to past
/// transactions in the same DB transaction: every change that needs no
/// inclusion, and on the included rows the ones that overwrite a value the
/// user set. Refuses a stale preview ([`ApplyError::Stale`]) or one that
/// meets a lock ([`ApplyError::Locked`]), as
/// [`commit_apply`](super::commit_apply) does; then nothing is stored.
pub fn save_draft(
    db: &mut Database,
    draft: &Draft,
    apply: Option<(&ApplyPreview, &BTreeSet<TransactionId>)>,
) -> Result<Saved, ApplyError> {
    let tx = db
        .conn_mut()
        .transaction_with_behavior(TransactionBehavior::Immediate)?;
    let rule = stage(&tx, draft)?;
    let applied = match apply {
        Some((preview, included)) => Some(commit_apply_in(&tx, &renamed(preview, rule), included)?),
        None => None,
    };
    tx.commit()?;
    tracing::info!(rule = %rule, applied = applied.is_some(), "saved a new rule");
    Ok(Saved { rule, applied })
}

/// What a row of the apply preview gets as its category ("AFTER").
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PreviewAfter {
    /// This category.
    Category(CategoryKey),
    /// Split into this many lines.
    Split(usize),
    /// Its category stays; the rule changes another field (a tag, a
    /// name, a note, a share or an exclusion).
    Unchanged,
}

/// One row of the apply preview (`EFT-0`), as the canvas lists it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PreviewRow {
    pub transaction: TransactionId,
    /// Its name now: the display merchant, else the cleaned bank text.
    /// Untrusted bank text: shown as plain text only.
    pub name: String,
    pub account: AccountId,
    pub date: NaiveDate,
    pub amount: Money,
    /// Its category now (`None`: uncategorised, or split).
    pub now: Option<CategoryKey>,
    pub after: PreviewAfter,
    /// Some change overwrites a value the user set: made only if the user
    /// includes the row ("Category you set", "Change these too").
    pub user_set: bool,
}

/// The rows of `preview` with what the canvas shows of each, newest first
/// (then by ID). Reads only.
pub fn preview_rows(db: &Database, preview: &ApplyPreview) -> Result<Vec<PreviewRow>, ApplyError> {
    let conn = db.conn();
    let mut statement = conn.prepare_cached(
        "SELECT t.merchant_display, t.merchant, t.description_raw, t.account_id, t.date, \
         t.amount_minor, t.currency, c.key \
         FROM \"transaction\" t LEFT JOIN category c ON c.id = t.category_id WHERE t.id = ?1",
    )?;
    let mut rows = Vec::with_capacity(preview.rows.len());
    for plan in &preview.rows {
        let row = statement.query_row([plan.transaction], |row| {
            let display: Option<String> = row.get(0)?;
            let bank: Option<String> = row.get(1)?;
            let description: String = row.get(2)?;
            let name = display
                .filter(|name| !name.trim().is_empty())
                .unwrap_or_else(|| {
                    crate::merchant::clean(
                        crate::merchant::source_text(bank.as_deref(), &description)
                            .unwrap_or_default(),
                    )
                });
            let currency: String = row.get(6)?;
            let currency =
                Currency::new(&currency).map_err(|_| crate::account::invalid(row, "currency"))?;
            let key: Option<String> = row.get(7)?;
            Ok(PreviewRow {
                transaction: plan.transaction,
                name,
                account: row.get(3)?,
                date: crate::account::date(row, "date")?,
                amount: Money::new(row.get(5)?, currency),
                now: key.and_then(|key| CategoryKey::new(&key).ok()),
                after: PreviewAfter::Unchanged,
                user_set: plan.needs_inclusion(),
            })
        })?;
        let after = plan
            .changes
            .iter()
            .find_map(|change| match &change.new {
                Target::Category { key, .. } => Some(PreviewAfter::Category(key.clone())),
                Target::Split { lines, .. } => Some(PreviewAfter::Split(lines.len())),
                _ => None,
            })
            .unwrap_or(PreviewAfter::Unchanged);
        rows.push(PreviewRow { after, ..row });
    }
    rows.sort_by(|a, b| b.date.cmp(&a.date).then(b.transaction.cmp(&a.transaction)));
    Ok(rows)
}
