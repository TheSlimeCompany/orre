//! Change history's read model (`DESIGN.md` → "Rules", "Change history",
//! `GZT-0`): every change batch, newest first, with what it did in a form
//! the app can say in a line, and each batch's items as "before → after".
//!
//! Descriptions are made when shown from the batch's items and, for a rule
//! apply, from the rule it names (`change_batch.rule_id`, V16): a rule edited
//! since is described as it is now, and a deleted one as deleted.

use std::collections::{BTreeSet, HashMap};

use chrono::NaiveDate;
use rusqlite::{Connection, OptionalExtension};

use super::{BatchInfo, ChangeKind, Error, Field, Item, Value, list, load_batch, load_items};
use crate::category::CategoryKey;
use crate::db::Database;
use crate::exclusion::ExclusionReason;
use crate::id::{ChangeBatchId, RuleId, TagId, TransactionId};
use crate::merchant;
use crate::rule::{self, RuleDefinition};

/// A field's value as Change history shows it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Shown {
    /// A category, or none.
    Category(Option<CategoryKey>),
    /// Split into this many lines (0: not split).
    Split(usize),
    /// A display name set by a rule or the user, or the derived one.
    Merchant(Option<String>),
    /// Whether there is a note. The note itself isn't shown here.
    Note(bool),
    /// Tag names, A–Z.
    Tags(Vec<String>),
    /// Excluded for this reason, or counted.
    Excluded(Option<ExclusionReason>),
    /// The user's share in minor units (`None`: all of it).
    Share(Option<i64>),
    /// Whether the whole amount is claimed back.
    Reimbursable(bool),
}

/// One field of one transaction a batch changed.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HistoryItem {
    pub transaction: TransactionId,
    /// The transaction's name now: its display merchant, else its cleaned
    /// bank text. Untrusted bank text: shown as plain text only.
    pub name: String,
    pub date: NaiveDate,
    pub field: Field,
    pub old: Shown,
    pub new: Shown,
}

/// What a batch did, for its one-line description.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Summary {
    /// A rule applied to past transactions: the rule as it is now, `None`
    /// when it has been deleted (or wasn't recorded).
    Rule {
        rule: Option<RuleId>,
        definition: Option<RuleDefinition>,
    },
    /// Every item set the same category (`None`: uncategorised).
    Category(Option<CategoryKey>),
    /// Every item added these tags.
    Tags(Vec<String>),
    /// A duplicate merge: the name of the row excluded as the duplicate.
    Merged { name: String },
    /// Anything else: the fields it changed.
    Fields(Vec<Field>),
}

/// One change batch as Change history lists it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HistoryEntry {
    pub batch: BatchInfo,
    pub summary: Summary,
}

/// What `conn` knows about the names in values: category keys, tag names.
struct Names<'c> {
    conn: &'c Connection,
    categories: HashMap<i64, Option<CategoryKey>>,
    tags: HashMap<TagId, String>,
}

impl<'c> Names<'c> {
    fn new(conn: &'c Connection) -> Self {
        Names {
            conn,
            categories: HashMap::new(),
            tags: HashMap::new(),
        }
    }

    fn category(&mut self, id: Option<i64>) -> rusqlite::Result<Option<CategoryKey>> {
        let Some(id) = id else { return Ok(None) };
        if let Some(key) = self.categories.get(&id) {
            return Ok(key.clone());
        }
        let key: Option<String> = self
            .conn
            .query_row("SELECT key FROM category WHERE id = ?1", [id], |row| {
                row.get(0)
            })
            .optional()?;
        let key = key.and_then(|key| CategoryKey::new(&key).ok());
        self.categories.insert(id, key.clone());
        Ok(key)
    }

    fn tag(&mut self, id: TagId) -> rusqlite::Result<Option<String>> {
        if let Some(name) = self.tags.get(&id) {
            return Ok(Some(name.clone()));
        }
        let name: Option<String> = self
            .conn
            .query_row("SELECT name FROM tag WHERE id = ?1", [id], |row| row.get(0))
            .optional()?;
        if let Some(name) = &name {
            self.tags.insert(id, name.clone());
        }
        Ok(name)
    }

    fn tags(&mut self, ids: &[TagId]) -> rusqlite::Result<Vec<String>> {
        let mut names = Vec::new();
        for id in ids {
            names.extend(self.tag(*id)?);
        }
        names.sort_by_key(|name| name.to_lowercase());
        Ok(names)
    }

    fn shown(&mut self, value: &Value) -> rusqlite::Result<Shown> {
        Ok(match value {
            Value::Category(category) => Shown::Category(self.category(category.category_id)?),
            Value::Merchant(name) => Shown::Merchant(name.name.clone()),
            Value::UserNote(note) => Shown::Note(note.is_some()),
            Value::Tags(ids) => Shown::Tags(self.tags(ids)?),
            Value::Exclusion(exclusion) => {
                Shown::Excluded(exclusion.as_ref().map(|value| value.reason))
            }
            Value::Split(lines) => Shown::Split(lines.len()),
            Value::Share(share) => Shown::Share(share.my_share_minor),
            Value::Reimbursable(on) => Shown::Reimbursable(*on),
        })
    }
}

/// A transaction's name now and its date.
fn transaction_name(conn: &Connection, id: TransactionId) -> rusqlite::Result<(String, NaiveDate)> {
    conn.query_row(
        "SELECT merchant_display, merchant, description_raw, date FROM \"transaction\" \
         WHERE id = ?1",
        [id],
        |row| {
            let display: Option<String> = row.get(0)?;
            let bank: Option<String> = row.get(1)?;
            let description: String = row.get(2)?;
            let name = display
                .filter(|name| !name.trim().is_empty())
                .unwrap_or_else(|| {
                    merchant::clean(
                        merchant::source_text(bank.as_deref(), &description).unwrap_or_default(),
                    )
                });
            Ok((name, crate::account::date(row, "date")?))
        },
    )
}

/// The rule a batch applied: the recorded one, else (for batches from
/// before V16) the one its category items name.
fn applied_rule(batch: &BatchInfo, items: &[Item]) -> Option<RuleId> {
    batch.rule.or_else(|| {
        items.iter().find_map(|item| match &item.new {
            Value::Category(category) => category.rule_id,
            _ => None,
        })
    })
}

fn summarise(
    conn: &Connection,
    names: &mut Names<'_>,
    rules: &HashMap<RuleId, RuleDefinition>,
    batch: &BatchInfo,
    items: &[Item],
) -> Result<Summary, Error> {
    if batch.kind == ChangeKind::RuleApply {
        let rule = applied_rule(batch, items);
        let definition = rule.and_then(|id| rules.get(&id).cloned());
        return Ok(Summary::Rule { rule, definition });
    }
    if batch.kind == ChangeKind::DuplicateMerge
        && let Some(item) = items.iter().find(|item| {
            matches!(&item.new, Value::Exclusion(Some(value))
                if value.reason == ExclusionReason::Duplicate)
        })
    {
        let (name, _) = transaction_name(conn, item.transaction)?;
        return Ok(Summary::Merged { name });
    }
    let fields: BTreeSet<Field> = items.iter().map(Item::field).collect();
    if fields.len() == 1 && fields.contains(&Field::Category) {
        let categories: BTreeSet<Option<i64>> = items
            .iter()
            .map(|item| match &item.new {
                Value::Category(category) => category.category_id,
                _ => None,
            })
            .collect();
        if categories.len() == 1 {
            let id = categories.into_iter().next().flatten();
            return Ok(Summary::Category(names.category(id)?));
        }
    }
    if fields.len() == 1 && fields.contains(&Field::Tags) {
        let mut added: Option<BTreeSet<TagId>> = None;
        for item in items {
            if let (Value::Tags(old), Value::Tags(new)) = (&item.old, &item.new) {
                let these: BTreeSet<TagId> =
                    new.iter().filter(|id| !old.contains(id)).copied().collect();
                added = Some(match added {
                    Some(so_far) => so_far.intersection(&these).copied().collect(),
                    None => these,
                });
            }
        }
        if let Some(added) = added.filter(|added| !added.is_empty()) {
            let ids: Vec<TagId> = added.into_iter().collect();
            return Ok(Summary::Tags(names.tags(&ids)?));
        }
    }
    Ok(Summary::Fields(fields.into_iter().collect()))
}

/// Every change batch, newest first, with what it did (module docs).
/// Imports aren't change batches, so they aren't here.
pub fn history(db: &Database) -> Result<Vec<HistoryEntry>, Error> {
    let conn = db.conn();
    let mut names = Names::new(conn);
    // A stored rule that no longer validates is an error everywhere else
    // (the Rules view says so); here its batches only lose their rule's text.
    let rules: HashMap<RuleId, RuleDefinition> = rule::load_all(conn)
        .map(|rules| {
            rules
                .into_iter()
                .map(|rule| (rule.id(), rule.definition().clone()))
                .collect()
        })
        .unwrap_or_default();
    list(db)?
        .into_iter()
        .map(|batch| {
            let items = load_items(conn, batch.id)?;
            let summary = summarise(conn, &mut names, &rules, &batch, &items)?;
            Ok(HistoryEntry { batch, summary })
        })
        .collect()
}

/// The items of `batch` as "before → after", in the order made, each with
/// its transaction's name and date.
pub fn history_items(db: &Database, batch: ChangeBatchId) -> Result<Vec<HistoryItem>, Error> {
    let conn = db.conn();
    load_batch(conn, batch)?;
    let mut names = Names::new(conn);
    load_items(conn, batch)?
        .into_iter()
        .map(|item| {
            let (name, date) = transaction_name(conn, item.transaction)?;
            Ok(HistoryItem {
                transaction: item.transaction,
                name,
                date,
                field: item.field(),
                old: names.shown(&item.old)?,
                new: names.shown(&item.new)?,
            })
        })
        .collect()
}
