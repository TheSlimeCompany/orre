//! Changing the category tree (M3.7, the categories editor): adding,
//! renaming, hiding and deleting a category, and the editor's read model,
//! each category with what uses it. `docs/ARCHITECTURE.md` → "Decision
//! record: categories" (points 2–5) and "Decision record: the categories
//! editor (built)".
//!
//! - **Add**: a new group with a kind, or a subcategory under a group,
//!   taking the group's kind. Its key comes from its label once, when it's
//!   added ([`derive_key`]), and never follows a rename.
//! - **Rename** changes the label only; keys and parents are read-only (V1
//!   trigger), so moving a category between groups isn't possible.
//! - Labels are trimmed and can't be blank, and two categories side by side
//!   (two groups, or two subcategories of one group) can't share a label,
//!   ignoring case: a picker would show them the same.
//! - **Hide** keeps a category and everything filed under it, and takes it
//!   out of pickers ([`CategoryTree::visible`]). A hidden group hides its
//!   subcategories with it; each keeps its own flag, so showing the group
//!   again brings them back as they were.
//! - **Delete** is refused while a transaction, split line, budget or
//!   schedule points at the category, a rule's `set_category` or
//!   `split_template` names its key, or a pending Inbox suggestion (a
//!   categorisation or a rule, [`crate::inbox`]) names it
//!   ([`Error::InUse`], with the counts), and for a group while it has
//!   subcategories ([`Error::HasSubcategories`]). A pending suggestion
//!   counts because approving it needs the category; deleting it away would
//!   leave a suggestion that can only be refused. The user approves or
//!   rejects it in the Inbox first: Coffer doesn't decide it for them, and a
//!   categorisation's other items may still be wanted. Decided suggestions
//!   don't count, nor do change batches that recorded the category: undoing
//!   one leaves a row whose category is gone as it is (`change::undo`).
//!
//! Errors name categories by their labels, never by transaction text.

use std::collections::HashMap;
use std::fmt;

use rusqlite::{Connection, params};

use super::{Category, CategoryKey, CategoryKind, CategoryTree};
use crate::db::Database;
use crate::inbox::{Payload, SuggestionKind};

/// The key part used when a label has no ASCII letter or digit to make one
/// from ("☕", "Café" keeps `caf`).
const FALLBACK_KEY: &str = "custom";

/// Where a new category goes.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Placement {
    /// A new group of this kind.
    Group(CategoryKind),
    /// A subcategory of the group with this key; it takes the group's kind.
    Under(CategoryKey),
}

/// A category to add.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NewCategory {
    /// What the user sees; trimmed, and the source of the key.
    pub label: String,
    pub placement: Placement,
}

/// What points at a category: why it can't be deleted while any count is
/// above zero.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct CategoryUsage {
    /// Transactions filed under it directly.
    pub transactions: usize,
    /// Split lines in it.
    pub split_lines: usize,
    /// Rules whose `set_category` or `split_template` names its key.
    pub rules: usize,
    /// Budget rows for it (the standing budget and monthly overrides).
    pub budgets: usize,
    /// Schedules with it as their category.
    pub schedules: usize,
    /// Pending Inbox suggestions that name its key.
    pub suggestions: usize,
}

impl CategoryUsage {
    /// Nothing uses it, so it can be deleted.
    pub fn is_unused(&self) -> bool {
        *self == CategoryUsage::default()
    }

    /// Only pending Inbox suggestions use it: approving or rejecting them
    /// frees it.
    pub fn only_suggestions(&self) -> bool {
        self.suggestions > 0
            && CategoryUsage {
                suggestions: 0,
                ..*self
            }
            .is_unused()
    }

    /// What frees it, after "so it can't be deleted; ".
    fn remedy(&self) -> &'static str {
        if self.only_suggestions() {
            "approve or reject them in the Inbox first"
        } else {
            "hide it instead"
        }
    }
}

impl fmt::Display for CategoryUsage {
    /// The non-zero counts, "3 transactions, 1 rule and 1 budget".
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let parts: Vec<String> = [
            (self.transactions, "transaction", "transactions"),
            (self.split_lines, "split line", "split lines"),
            (self.rules, "rule", "rules"),
            (self.budgets, "budget", "budgets"),
            (self.schedules, "schedule", "schedules"),
            (
                self.suggestions,
                "pending Inbox suggestion",
                "pending Inbox suggestions",
            ),
        ]
        .into_iter()
        .filter(|(count, _, _)| *count > 0)
        .map(|(count, one, many)| format!("{count} {}", if count == 1 { one } else { many }))
        .collect();
        match parts.as_slice() {
            [] => f.write_str("nothing"),
            [one] => f.write_str(one),
            [rest @ .., last] => write!(f, "{} and {last}", rest.join(", ")),
        }
    }
}

/// A category and what uses it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CategorySummary {
    pub category: Category,
    pub usage: CategoryUsage,
}

/// A group in the editor: its own category and its subcategories, each with
/// what uses it, in [`CategoryTree`]'s order.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GroupOverview {
    pub group: CategorySummary,
    pub children: Vec<CategorySummary>,
}

/// Why a category change was refused. Nothing changed.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("there is no category {0}")]
    NotFound(CategoryKey),

    /// A subcategory can't hold subcategories: the tree has two levels.
    #[error("“{label}” is a subcategory, so it can't have subcategories of its own")]
    NotAGroup { label: String },

    #[error("a category needs a name")]
    BlankLabel,

    #[error("there is already a category called “{label}” there")]
    LabelTaken { label: String },

    #[error("“{label}” is used by {usage}, so it can't be deleted; {}", usage.remedy())]
    InUse { label: String, usage: CategoryUsage },

    #[error(
        "“{label}” has {count} {}; delete or hide them first",
        if *count == 1 { "subcategory" } else { "subcategories" }
    )]
    HasSubcategories { label: String, count: usize },

    #[error("database error: {0}")]
    Database(#[from] rusqlite::Error),
}

/// The key a category labelled `label` gets: the label's ASCII letters and
/// digits in lower case, every other run as one `_` (none at either end),
/// under `parent`'s key for a subcategory. When nothing is left the part is
/// `custom`. While `taken` says a key is in use, `_2`, `_3`, … is added.
pub fn derive_key(
    label: &str,
    parent: Option<&CategoryKey>,
    taken: impl Fn(&CategoryKey) -> bool,
) -> CategoryKey {
    let mut part = String::new();
    let mut gap = false;
    for c in label.chars() {
        if c.is_ascii_alphanumeric() {
            if gap && !part.is_empty() {
                part.push('_');
            }
            gap = false;
            part.push(c.to_ascii_lowercase());
        } else {
            gap = true;
        }
    }
    if part.is_empty() {
        part.push_str(FALLBACK_KEY);
    }
    let make = |part: &str| {
        let text = match parent {
            Some(parent) => format!("{parent}.{part}"),
            None => part.to_owned(),
        };
        CategoryKey::new(&text).expect("lowercase letters, digits and `_` make a key")
    };
    let first = make(&part);
    if !taken(&first) {
        return first;
    }
    (2u64..)
        .map(|n| make(&format!("{part}_{n}")))
        .find(|key| !taken(key))
        .expect("some suffix is free")
}

/// Adds a category (module docs) and returns it as stored.
pub fn add(db: &mut Database, new: &NewCategory) -> Result<Category, Error> {
    let label = clean_label(&new.label)?;
    let tx = db.conn_mut().transaction()?;
    let tree = CategoryTree::load(&tx)?;
    let (parent, kind) = match &new.placement {
        Placement::Group(kind) => (None, *kind),
        Placement::Under(key) => {
            let group = tree.get(key).ok_or_else(|| Error::NotFound(key.clone()))?;
            if !group.is_group() {
                return Err(Error::NotAGroup {
                    label: group.label().to_owned(),
                });
            }
            (Some(group), group.kind())
        }
    };
    check_label_free(&tree, parent.map(Category::id), &label, None)?;
    let key = derive_key(&label, parent.map(Category::key), |key| {
        tree.get(key).is_some()
    });
    tx.execute(
        "INSERT INTO category (parent_id, key, label, kind) VALUES (?1, ?2, ?3, ?4)",
        params![parent.map(Category::id), key, label, kind],
    )?;
    let category = CategoryTree::load(&tx)?
        .get(&key)
        .cloned()
        .ok_or_else(|| Error::NotFound(key.clone()))?;
    tx.commit()?;
    Ok(category)
}

/// Renames the category with `key`: its label only. Renaming to its own
/// label (or a change of case) is allowed.
pub fn rename(db: &mut Database, key: &CategoryKey, label: &str) -> Result<(), Error> {
    let label = clean_label(label)?;
    let tx = db.conn_mut().transaction()?;
    let tree = CategoryTree::load(&tx)?;
    let category = tree.get(key).ok_or_else(|| Error::NotFound(key.clone()))?;
    if category.label() == label {
        return Ok(());
    }
    check_label_free(&tree, category.parent_id(), &label, Some(category.id()))?;
    tx.execute(
        "UPDATE category SET label = ?1 WHERE id = ?2",
        params![label, category.id()],
    )?;
    tx.commit()?;
    Ok(())
}

/// Hides the category with `key` from pickers, or shows it again. Its
/// transactions, history and reports are unchanged.
pub fn set_hidden(db: &mut Database, key: &CategoryKey, hidden: bool) -> Result<(), Error> {
    let changed = db.conn().execute(
        "UPDATE category SET hidden = ?1 WHERE key = ?2",
        params![hidden, key],
    )?;
    if changed == 0 {
        return Err(Error::NotFound(key.clone()));
    }
    Ok(())
}

/// What uses the category with `key`.
pub fn usage(db: &Database, key: &CategoryKey) -> Result<CategoryUsage, Error> {
    let tree = CategoryTree::load(db.conn())?;
    let category = tree.get(key).ok_or_else(|| Error::NotFound(key.clone()))?;
    Ok(load_usage(db.conn(), &tree)?
        .remove(&category.id())
        .unwrap_or_default())
}

/// Deletes the category with `key` when nothing uses it (module docs).
pub fn delete(db: &mut Database, key: &CategoryKey) -> Result<(), Error> {
    let tx = db.conn_mut().transaction()?;
    let tree = CategoryTree::load(&tx)?;
    let category = tree.get(key).ok_or_else(|| Error::NotFound(key.clone()))?;
    if category.is_group() {
        let count = tree.group_of(key).map_or(0, |group| group.children().len());
        if count > 0 {
            return Err(Error::HasSubcategories {
                label: category.label().to_owned(),
                count,
            });
        }
    }
    let usage = load_usage(&tx, &tree)?
        .remove(&category.id())
        .unwrap_or_default();
    if !usage.is_unused() {
        return Err(Error::InUse {
            label: category.label().to_owned(),
            usage,
        });
    }
    tx.execute("DELETE FROM category WHERE id = ?1", [category.id()])?;
    tx.commit()?;
    Ok(())
}

/// Every group with its subcategories, hidden ones included, each with what
/// uses it: the categories editor's table.
pub fn overview(db: &Database) -> Result<Vec<GroupOverview>, Error> {
    let tree = CategoryTree::load(db.conn())?;
    let mut usage = load_usage(db.conn(), &tree)?;
    let mut summary = |category: &Category| CategorySummary {
        category: category.clone(),
        usage: usage.remove(&category.id()).unwrap_or_default(),
    };
    Ok(tree
        .groups()
        .iter()
        .map(|group| GroupOverview {
            group: summary(group.category()),
            children: group.children().iter().map(&mut summary).collect(),
        })
        .collect())
}

/// `label` trimmed; refused when blank.
fn clean_label(label: &str) -> Result<String, Error> {
    let label = label.trim();
    if label.is_empty() {
        return Err(Error::BlankLabel);
    }
    Ok(label.to_owned())
}

/// Refuses `label` when another category beside it (a group among the
/// groups, a subcategory among its group's) has it, ignoring case.
fn check_label_free(
    tree: &CategoryTree,
    parent: Option<i64>,
    label: &str,
    except: Option<i64>,
) -> Result<(), Error> {
    let folded = label.to_lowercase();
    let mut siblings: Box<dyn Iterator<Item = &Category>> = match parent {
        None => Box::new(tree.groups().iter().map(|group| group.category())),
        Some(parent) => Box::new(
            tree.groups()
                .iter()
                .filter(move |group| group.category().id() == parent)
                .flat_map(|group| group.children()),
        ),
    };
    if siblings.any(|other| Some(other.id()) != except && other.label().to_lowercase() == folded) {
        return Err(Error::LabelTaken {
            label: label.to_owned(),
        });
    }
    Ok(())
}

/// What uses each category, by row ID; categories nothing uses are left
/// out.
fn load_usage(
    conn: &Connection,
    tree: &CategoryTree,
) -> rusqlite::Result<HashMap<i64, CategoryUsage>> {
    let mut usage: HashMap<i64, CategoryUsage> = HashMap::new();
    let mut count_by_id =
        |sql: &str, field: fn(&mut CategoryUsage) -> &mut usize| -> rusqlite::Result<()> {
            let mut statement = conn.prepare(sql)?;
            let rows = statement
                .query_map([], |row| Ok((row.get::<_, i64>(0)?, row.get::<_, i64>(1)?)))?;
            for row in rows {
                let (id, count) = row?;
                *field(usage.entry(id).or_default()) = usize::try_from(count).unwrap_or(0);
            }
            Ok(())
        };
    count_by_id(
        "SELECT category_id, COUNT(*) FROM \"transaction\" \
         WHERE category_id IS NOT NULL GROUP BY category_id",
        |usage| &mut usage.transactions,
    )?;
    count_by_id(
        "SELECT category_id, COUNT(*) FROM transaction_split \
         WHERE category_id IS NOT NULL GROUP BY category_id",
        |usage| &mut usage.split_lines,
    )?;
    count_by_id(
        "SELECT category_id, COUNT(*) FROM budget GROUP BY category_id",
        |usage| &mut usage.budgets,
    )?;
    count_by_id(
        "SELECT category_id, COUNT(*) FROM schedule \
         WHERE category_id IS NOT NULL GROUP BY category_id",
        |usage| &mut usage.schedules,
    )?;

    // Rules name keys, in their actions' JSON (`rule::action`).
    let mut statement = conn.prepare(
        "SELECT key, COUNT(DISTINCT rule_id) FROM ( \
             SELECT json_extract(value_json, '$') AS key, rule_id FROM rule_action \
             WHERE kind = 'set_category' \
             UNION ALL \
             SELECT json_extract(line.value, '$.category') AS key, rule_action.rule_id \
             FROM rule_action, json_each(rule_action.value_json) AS line \
             WHERE rule_action.kind = 'split_template' \
         ) WHERE key IS NOT NULL GROUP BY key",
    )?;
    let rows = statement.query_map([], |row| {
        Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?))
    })?;
    for row in rows {
        let (key, count) = row?;
        // A key no category has (deleted outside Coffer) has nothing to
        // count against.
        let Some(category) = CategoryKey::new(&key).ok().and_then(|key| tree.get(&key)) else {
            continue;
        };
        usage.entry(category.id()).or_default().rules = usize::try_from(count).unwrap_or(0);
    }

    // Pending suggestions name keys in their payloads, parsed as approving
    // reads them. One that no longer parses names nothing.
    let mut statement = conn.prepare(
        "SELECT kind, payload_json FROM suggestion \
         WHERE status = 'pending' AND kind IN ('categorization', 'rule')",
    )?;
    let rows = statement.query_map([], |row| {
        Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
    })?;
    for row in rows {
        let (kind, json) = row?;
        let Some(payload) = kind
            .parse::<SuggestionKind>()
            .ok()
            .and_then(|kind| Payload::from_stored(kind, &json).ok())
        else {
            continue;
        };
        for key in payload.category_keys() {
            if let Some(category) = tree.get(key) {
                usage.entry(category.id()).or_default().suggestions += 1;
            }
        }
    }
    Ok(usage)
}

#[cfg(test)]
mod tests;
