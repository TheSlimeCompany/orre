//! Reading rules, merchant names and the change history, and changing them,
//! the way [`crate::inbox::store`] does suggestions: no connection is held
//! between calls. Each call opens the database at its path after checking
//! the restore lock, so a restore can swap the file.
//!
//! Everything is core's: the Rules table is [`rule::overview`], a rule is
//! edited with [`rule::update`], moved with [`rule::move_to`] and deleted
//! with [`rule::delete`]; a new rule is previewed with
//! [`rule::preview_draft`] and saved (and applied) with [`rule::save_draft`];
//! a stored rule is applied with [`rule::preview_apply`] then
//! [`rule::commit_apply`], one undoable change batch; names are
//! [`merchant::overview`], [`merchant::add`], [`merchant::edit`] and
//! [`merchant::delete`], which apply at once with no batch; the history is
//! [`change::history`], undone with [`change::undo`]; categories are
//! [`category::overview`], changed with [`category::add`],
//! [`category::rename`], [`category::set_hidden`] and [`category::delete`],
//! which apply at once with no batch (no transaction changes).

use std::collections::{BTreeSet, HashMap};
use std::path::PathBuf;

use chrono::{NaiveDate, NaiveDateTime};
use coffer_core::account;
use coffer_core::backup;
use coffer_core::category::{self, Category, CategoryKey, GroupOverview, NewCategory};
use coffer_core::change::{self, HistoryEntry, HistoryItem, UndoSummary};
use coffer_core::clock;
use coffer_core::db::Database;
use coffer_core::id::{AccountId, ChangeBatchId, MerchantAliasId, RuleId, TransactionId};
use coffer_core::inbox::{Suggestion, preview as inbox_preview};
use coffer_core::merchant::{self, AliasDefinition, AliasSummary};
use coffer_core::money::Currency;
use coffer_core::reporting::palette::{Palette, category_palette};
use coffer_core::rule::{
    self, Applied, ApplyPreview, Draft, DraftPreview, PreviewRow, RuleDefinition, RuleSummary,
    Saved,
};
use coffer_core::schedule;
use coffer_core::tag;

use crate::accounts::store::StoreError;

/// A restore is replacing the database.
#[derive(Debug)]
struct Restoring;

impl std::fmt::Display for Restoring {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("A backup is being restored; try again when it's done.")
    }
}

impl std::error::Error for Restoring {}

/// A category as the selects and the table show it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CategoryOption {
    pub key: CategoryKey,
    pub label: String,
    /// The group's label for a subcategory; `None` for a group.
    pub group: Option<String>,
    /// Hidden, itself or with its group: pickers leave it out unless it's
    /// already chosen.
    pub hidden: bool,
}

/// An account as the Account select and lane show it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AccountOption {
    pub id: AccountId,
    pub name: String,
    pub currency: Currency,
}

/// A tag the Tag action's menu offers.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TagOption {
    pub name: String,
    pub private: bool,
}

/// What the Rules view shows.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct RulesData {
    /// Every rule in evaluation order, with what it decides today.
    pub rules: Vec<RuleSummary>,
    /// When each rule was stored, local.
    pub rule_created: HashMap<RuleId, NaiveDate>,
    pub names: Vec<AliasSummary>,
    /// When each name was added, local.
    pub name_added: HashMap<MerchantAliasId, NaiveDate>,
    /// Every change batch, newest first.
    pub history: Vec<HistoryEntry>,
    /// When each batch was made and undone, local.
    pub history_at: HashMap<ChangeBatchId, (NaiveDateTime, Option<NaiveDateTime>)>,
    /// Every category, groups first with their subcategories after them.
    pub categories: Vec<CategoryOption>,
    /// Every group with its subcategories and what uses each (the
    /// Categories section).
    pub category_overview: Vec<GroupOverview>,
    pub palette: Palette,
    pub accounts: Vec<AccountOption>,
    pub tags: Vec<TagOption>,
    /// Today, local: dates in this year are written without it.
    pub today: Option<NaiveDate>,
}

impl RulesData {
    pub fn category(&self, key: &CategoryKey) -> Option<&CategoryOption> {
        self.categories.iter().find(|option| &option.key == key)
    }

    pub fn account(&self, id: AccountId) -> Option<&AccountOption> {
        self.accounts.iter().find(|account| account.id == id)
    }

    /// The currency amounts are typed in for a rule on `account` (any
    /// account: the first account's, the home currency until settings
    /// name one).
    pub fn currency(&self, account: Option<AccountId>) -> Currency {
        account
            .and_then(|id| self.account(id))
            .or_else(|| self.accounts.first())
            .map_or(Currency::GBP, |account| account.currency)
    }

    pub fn rule(&self, id: RuleId) -> Option<&RuleSummary> {
        self.rules.iter().find(|summary| summary.rule.id() == id)
    }

    /// A rule's position, 0 first.
    pub fn position(&self, id: RuleId) -> Option<usize> {
        self.rules
            .iter()
            .position(|summary| summary.rule.id() == id)
    }

    pub fn name(&self, id: MerchantAliasId) -> Option<&AliasSummary> {
        self.names.iter().find(|summary| summary.alias.id() == id)
    }

    pub fn entry(&self, id: ChangeBatchId) -> Option<&HistoryEntry> {
        self.history.iter().find(|entry| entry.batch.id == id)
    }

    /// The category with row ID `id` and its group's overview (its own, for
    /// a group).
    pub fn category_entry(&self, id: i64) -> Option<(&category::CategorySummary, &GroupOverview)> {
        self.category_overview.iter().find_map(|group| {
            std::iter::once(&group.group)
                .chain(&group.children)
                .find(|summary| summary.category.id() == id)
                .map(|summary| (summary, group))
        })
    }

    /// The group with key `key`.
    pub fn category_group(&self, key: &CategoryKey) -> Option<&GroupOverview> {
        self.category_overview
            .iter()
            .find(|group| group.group.category.key() == key)
    }
}

/// The rules, names and history in the database at `database`.
#[derive(Clone, Debug)]
pub struct RulesStore {
    database: PathBuf,
}

impl RulesStore {
    pub fn new(database: PathBuf) -> Self {
        Self { database }
    }

    fn open(&self) -> Result<Database, StoreError> {
        if backup::restoring(&self.database)? {
            return Err(Box::new(Restoring));
        }
        Ok(Database::open(&self.database)?)
    }

    pub fn load(&self) -> Result<RulesData, StoreError> {
        load(&self.open()?)
    }

    /// Pending agent rule suggestions that would take over rows `rule`
    /// filed (the inspector's sparkle line).
    pub fn touching(&self, rule: RuleId) -> Result<Vec<Suggestion>, StoreError> {
        Ok(inbox_preview::touching_rule(&self.open()?, rule)?)
    }

    /// Saves an edit to `rule`. Edits don't re-run on history.
    pub fn update(&self, rule: RuleId, definition: &RuleDefinition) -> Result<(), StoreError> {
        Ok(rule::update(&mut self.open()?, rule, definition)?)
    }

    pub fn move_to(&self, rule: RuleId, position: usize) -> Result<(), StoreError> {
        Ok(rule::move_to(&mut self.open()?, rule, position)?)
    }

    pub fn delete(&self, rule: RuleId) -> Result<(), StoreError> {
        Ok(rule::delete(&mut self.open()?, rule)?)
    }

    /// Applying stored rule `rule` to past transactions, with its rows.
    pub fn preview_rule(
        &self,
        rule: RuleId,
    ) -> Result<(ApplyPreview, Vec<PreviewRow>), StoreError> {
        let db = self.open()?;
        let preview = rule::preview_apply(&db, rule)?;
        let rows = rule::preview_rows(&db, &preview)?;
        Ok((preview, rows))
    }

    /// Saving and applying `draft`, with its rows.
    pub fn preview_draft(
        &self,
        draft: &Draft,
    ) -> Result<(DraftPreview, Vec<PreviewRow>), StoreError> {
        let db = self.open()?;
        let preview = rule::preview_draft(&db, draft)?;
        let rows = rule::preview_rows(&db, &preview.preview)?;
        Ok((preview, rows))
    }

    /// Applies `preview` of a stored rule as one change batch.
    pub fn apply(
        &self,
        preview: &ApplyPreview,
        included: &BTreeSet<TransactionId>,
    ) -> Result<Applied, StoreError> {
        Ok(rule::commit_apply(&mut self.open()?, preview, included)?)
    }

    /// Saves `draft`, and applies `apply`'s preview with it when given.
    pub fn save_draft(
        &self,
        draft: &Draft,
        apply: Option<(&ApplyPreview, &BTreeSet<TransactionId>)>,
    ) -> Result<Saved, StoreError> {
        Ok(rule::save_draft(&mut self.open()?, draft, apply)?)
    }

    pub fn add_name(&self, alias: &AliasDefinition) -> Result<MerchantAliasId, StoreError> {
        Ok(merchant::add(&mut self.open()?, alias)?.0)
    }

    pub fn edit_name(
        &self,
        id: MerchantAliasId,
        alias: &AliasDefinition,
    ) -> Result<(), StoreError> {
        merchant::edit(&mut self.open()?, id, alias)?;
        Ok(())
    }

    pub fn delete_name(&self, id: MerchantAliasId) -> Result<(), StoreError> {
        merchant::delete(&mut self.open()?, id)?;
        Ok(())
    }

    /// Adds a category; it's returned as stored.
    pub fn add_category(&self, new: &NewCategory) -> Result<Category, StoreError> {
        Ok(category::add(&mut self.open()?, new)?)
    }

    /// Renames a category: its label only.
    pub fn rename_category(&self, key: &CategoryKey, label: &str) -> Result<(), StoreError> {
        Ok(category::rename(&mut self.open()?, key, label)?)
    }

    /// Hides a category from pickers, or shows it again.
    pub fn set_category_hidden(&self, key: &CategoryKey, hidden: bool) -> Result<(), StoreError> {
        Ok(category::set_hidden(&mut self.open()?, key, hidden)?)
    }

    /// Deletes a category nothing uses.
    pub fn delete_category(&self, key: &CategoryKey) -> Result<(), StoreError> {
        Ok(category::delete(&mut self.open()?, key)?)
    }

    /// The items of `batch`, as "before → after".
    pub fn history_items(&self, batch: ChangeBatchId) -> Result<Vec<HistoryItem>, StoreError> {
        Ok(change::history_items(&self.open()?, batch)?)
    }

    /// What undoing `batch` would restore, and what stays.
    pub fn undo_preview(&self, batch: ChangeBatchId) -> Result<UndoSummary, StoreError> {
        Ok(change::undo_preview(&self.open()?, batch)?)
    }

    pub fn undo(&self, batch: ChangeBatchId) -> Result<UndoSummary, StoreError> {
        Ok(change::undo(&mut self.open()?, batch)?)
    }
}

/// `stored` (UTC) in the computer's time zone; UTC when that can't be read.
fn locals(stored: &[&str]) -> Vec<Option<NaiveDateTime>> {
    let parsed: Vec<_> = stored
        .iter()
        .map(|text| clock::parse_stored(text))
        .collect();
    let present: Vec<_> = parsed.iter().flatten().copied().collect();
    let converted = clock::local(&present)
        .unwrap_or_else(|_| present.iter().map(|time| time.naive_utc()).collect());
    let mut converted = converted.into_iter();
    parsed
        .into_iter()
        .map(|time| time.and_then(|_| converted.next()))
        .collect()
}

/// [`RulesStore::load`] on an open database: also what tests and the
/// snapshot use.
pub fn load(db: &Database) -> Result<RulesData, StoreError> {
    let today = schedule::local_today()?;
    let rules = rule::overview(db)?;
    let names = merchant::overview(db)?;
    let history = change::history(db)?;

    let mut stored: Vec<&str> = Vec::new();
    stored.extend(rules.iter().map(|summary| summary.rule.created_at()));
    stored.extend(names.iter().map(|summary| summary.created_at.as_str()));
    for entry in &history {
        stored.push(&entry.batch.created_at);
        stored.push(entry.batch.undone_at.as_deref().unwrap_or(""));
    }
    let mut times = locals(&stored).into_iter();
    let rule_created = rules
        .iter()
        .filter_map(|summary| Some((summary.rule.id(), times.next()??.date())))
        .collect();
    let name_added = names
        .iter()
        .filter_map(|summary| Some((summary.alias.id(), times.next()??.date())))
        .collect();
    let history_at = history
        .iter()
        .filter_map(|entry| {
            let made = times.next().flatten();
            let undone = times.next().flatten();
            Some((entry.batch.id, (made?, undone)))
        })
        .collect();

    let tree = db.categories()?;
    let mut categories = Vec::new();
    for group in tree.groups() {
        let parent = group.category();
        categories.push(CategoryOption {
            key: parent.key().clone(),
            label: parent.label().to_owned(),
            group: None,
            hidden: !tree.is_visible(parent.key()),
        });
        categories.extend(group.children().iter().map(|child| CategoryOption {
            key: child.key().clone(),
            label: child.label().to_owned(),
            group: Some(parent.label().to_owned()),
            hidden: !tree.is_visible(child.key()),
        }));
    }
    let accounts = account::list(db)?
        .into_iter()
        .filter(|account| account.closed_on.is_none())
        .map(|account| AccountOption {
            id: account.id,
            name: account.name,
            currency: account.currency,
        })
        .collect();
    let tags = tag::list(db)?
        .into_iter()
        .map(|tag| TagOption {
            name: tag.name.as_str().to_owned(),
            private: tag.private,
        })
        .collect();
    Ok(RulesData {
        rules,
        rule_created,
        names,
        name_added,
        history,
        history_at,
        categories,
        category_overview: category::overview(db)?,
        palette: category_palette(&db.reader()?, today)?,
        accounts,
        tags,
        today: Some(today),
    })
}
