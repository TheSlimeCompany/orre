//! "Create a rule from this edit?" (`DESIGN.md` → "Transactions additions",
//! `3WT-0`): after the user sets a transaction's category, the rule Orre
//! proposes, and how many earlier transactions it would match.
//!
//! # When
//!
//! Only after a category the user set (`category_source = 'user'`), on a row
//! that isn't split and isn't in a suggested or confirmed transfer, when no
//! rule decides its category today. Never for a pattern the user turned down
//! ([`dismiss`]) or one a stored rule already has with the same category,
//! and only when the proposed rule matches at least one other transaction.
//!
//! # The rule
//!
//! `merchant contains <stem>` → the row's category, any account, any
//! amount. The stem is the cleaned bank merchant ([`merchant::clean`], the
//! text merchant rules match) cut before its reference: at the first `*`,
//! and before the first word with a digit (`VINTED *KX8F2Q` → `VINTED`,
//! `TESCO STORES 3297` → `TESCO STORES`). A row with no bank merchant gets a
//! `description contains` rule from its cleaned description. A stem shorter
//! than three characters proposes nothing: it would match too much.
//!
//! The counts come from the draft's own preview ([`preview_draft`]), with the
//! rule first, so they are exactly what "Create rule…" then shows: the rows
//! it would change, split into uncategorised ones, ones the user set (left
//! out unless included) and the rest.

use rusqlite::{OptionalExtension, params};

use super::draft::{Draft, preview_draft};
use super::engine::{MatchInput, RuleSet};
use super::{
    AmountBounds, ApplyError, MatchField, MatchKind, Pattern, RuleAction, RuleDefinition,
    RuleOrigin, fold, store,
};
use crate::category::CategoryKey;
use crate::change::{CategoryValue, Value};
use crate::db::Database;
use crate::id::{AccountId, TransactionId};
use crate::merchant;

/// The shortest stem worth a rule.
pub const MIN_STEM_LEN: usize = 3;

/// The rule proposed after a category edit, and what it would change.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Proposal {
    /// The edited transaction.
    pub transaction: TransactionId,
    /// The proposed rule.
    pub definition: RuleDefinition,
    /// The category it sets: the one the user just chose.
    pub category: CategoryKey,
    /// Other transactions it would change ("6 earlier transactions match").
    pub matches: usize,
    /// Of those, the uncategorised ones.
    pub uncategorized: usize,
    /// Of those, the ones with a category the user set: changed only if the
    /// user includes them.
    pub user_set: usize,
}

/// The stem of a cleaned merchant or description: cut at the first `*` and
/// before the first word with a digit, whitespace collapsed. `None` when
/// less than [`MIN_STEM_LEN`] characters are left.
pub fn stem(cleaned: &str) -> Option<String> {
    let head = cleaned.split('*').next().unwrap_or_default();
    let words: Vec<&str> = head
        .split_whitespace()
        .take_while(|word| !word.chars().any(|c| c.is_ascii_digit()))
        .collect();
    let stem = words.join(" ");
    (stem.chars().count() >= MIN_STEM_LEN).then_some(stem)
}

/// The transaction as the proposal reads it.
struct Row {
    account_id: AccountId,
    amount_minor: i64,
    merchant: Option<String>,
    description: String,
    source_category: Option<String>,
    source_type: Option<String>,
    category: Option<String>,
    user_set: bool,
    split: bool,
    in_transfer: bool,
}

impl Row {
    fn input(&self) -> MatchInput<'_> {
        MatchInput {
            account_id: self.account_id,
            amount_minor: self.amount_minor,
            merchant: self.merchant.as_deref(),
            description: &self.description,
            source_category: self.source_category.as_deref(),
            source_type: self.source_type.as_deref(),
        }
    }
}

fn load(db: &Database, transaction: TransactionId) -> rusqlite::Result<Option<Row>> {
    db.conn()
        .query_row(
            "SELECT t.account_id, t.amount_minor, t.merchant, t.description_raw, \
             t.source_category, t.source_type, c.key, \
             coalesce(t.category_source = 'user', 0), \
             EXISTS (SELECT 1 FROM transaction_split s WHERE s.transaction_id = t.id), \
             coalesce(g.status IN ('suggested', 'confirmed'), 0) \
             FROM \"transaction\" t \
             LEFT JOIN category c ON c.id = t.category_id \
             LEFT JOIN transfer_group g ON g.id = t.transfer_group_id \
             WHERE t.id = ?1",
            [transaction],
            |row| {
                Ok(Row {
                    account_id: row.get(0)?,
                    amount_minor: row.get(1)?,
                    merchant: row.get(2)?,
                    description: row.get(3)?,
                    source_category: row.get(4)?,
                    source_type: row.get(5)?,
                    category: row.get(6)?,
                    user_set: row.get(7)?,
                    split: row.get(8)?,
                    in_transfer: row.get(9)?,
                })
            },
        )
        .optional()
}

/// Whether the user turned down a prompt for `field contains pattern`.
fn dismissed(db: &Database, field: MatchField, pattern: &str) -> rusqlite::Result<bool> {
    db.conn()
        .prepare_cached(
            "SELECT 1 FROM rule_prompt_dismissed WHERE match_field = ?1 AND pattern = ?2",
        )?
        .exists(params![field.as_str(), fold(pattern)])
}

/// The rule a row suggests: `contains <stem>` on its cleaned merchant (or
/// description), setting its category. `None` without a category, for a
/// split row or one in a transfer, or when no stem is long enough.
fn suggested(row: &Row) -> Option<(RuleDefinition, CategoryKey)> {
    let category = CategoryKey::new(row.category.as_deref()?).ok()?;
    if row.split || row.in_transfer {
        return None;
    }
    let (field, cleaned) = match row.merchant.as_deref() {
        Some(merchant) => (MatchField::Merchant, merchant::clean(merchant)),
        None => (MatchField::Description, merchant::clean(&row.description)),
    };
    let stem = stem(&cleaned)?;
    let pattern = Pattern::new(MatchKind::Contains, &stem).ok()?;
    let definition = RuleDefinition::new(
        field,
        pattern,
        AmountBounds::ANY,
        None,
        vec![RuleAction::SetCategory(category.clone())],
    )
    .expect("one set_category action is a valid rule");
    // The rule must catch the row it came from.
    let alone = RuleSet::new([store::StoredRule::unsaved(
        crate::id::RuleId::new(0),
        definition.clone(),
    )]);
    if alone.matching(&row.input()).is_empty() {
        return None;
    }
    Some((definition, category))
}

/// The rule "Create rule from this" (the Transactions inspector's Rule
/// section) starts from: the same rule the prompt proposes, for any row
/// with a category, whoever set it. Reads only.
pub fn rule_from(
    db: &Database,
    transaction: TransactionId,
) -> Result<Option<RuleDefinition>, ApplyError> {
    Ok(load(db, transaction)?
        .as_ref()
        .and_then(suggested)
        .map(|(definition, _)| definition))
}

/// The rule Orre proposes after the user set `transaction`'s category,
/// if any (module docs). Reads only.
pub fn propose(db: &Database, transaction: TransactionId) -> Result<Option<Proposal>, ApplyError> {
    let Some(row) = load(db, transaction)? else {
        return Ok(None);
    };
    if !row.user_set {
        return Ok(None);
    }
    let stored = store::load_all(db.conn())?;
    let rules = RuleSet::new(stored.iter().cloned());
    let outcome = rules.evaluate(&row.input());
    if outcome.category.is_some() || outcome.split.is_some() {
        return Ok(None);
    }
    let Some((definition, category)) = suggested(&row) else {
        return Ok(None);
    };
    if dismissed(db, definition.field(), definition.pattern().as_str())?
        || stored.iter().any(|rule| *rule.definition() == definition)
    {
        return Ok(None);
    }
    let draft = Draft {
        definition: definition.clone(),
        position: 0,
        origin: RuleOrigin::Edit,
    };
    let preview = preview_draft(db, &draft)?;
    // Its only action sets a category, so every planned row changes one.
    let others: Vec<_> = preview
        .preview
        .rows
        .iter()
        .filter(|plan| plan.transaction != transaction)
        .collect();
    if others.is_empty() {
        return Ok(None);
    }
    let uncategorized = others
        .iter()
        .filter(|plan| {
            plan.changes.iter().any(|change| {
                matches!(
                    change.old,
                    Value::Category(CategoryValue {
                        category_id: None,
                        ..
                    })
                )
            })
        })
        .count();
    let user_set = others.iter().filter(|plan| plan.needs_inclusion()).count();
    Ok(Some(Proposal {
        transaction,
        definition,
        category,
        matches: others.len(),
        uncategorized,
        user_set,
    }))
}

/// Remembers "Not now" for `definition`'s pattern, so the prompt doesn't
/// propose it again. Only its field and its text, folded, are kept.
pub fn dismiss(db: &mut Database, definition: &RuleDefinition) -> rusqlite::Result<()> {
    db.conn().execute(
        "INSERT INTO rule_prompt_dismissed (match_field, pattern) VALUES (?1, ?2) \
         ON CONFLICT DO NOTHING",
        params![
            definition.field().as_str(),
            fold(definition.pattern().as_str())
        ],
    )?;
    Ok(())
}
