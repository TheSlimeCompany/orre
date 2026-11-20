//! Reading what the Rules view shows, the way [`crate::inbox::store`] does
//! suggestions: no connection is held between calls. Each call opens the
//! database at its path through [`gate::open`], which checks the restore
//! lock, so a restore can swap the file.
//!
//! Only the read model is here: [`RulesStore::load`] builds [`RulesData`]
//! from core's overviews ([`rule::overview`], [`merchant::overview`],
//! [`change::history`], [`category::overview`]). At 100,000 transactions
//! that takes far longer than a frame, so the view runs it on a background
//! thread ([`Rules::reload`](super::Rules::reload)); the store is `Clone`
//! and holds only a path, so it can go there. Every change and every
//! other read is a call to core's own function through
//! [`Rules::write`](super::Rules) and its `query`, which open the database
//! with `RulesStore::open` (`ARCHITECTURE.md` → "Decision record: one gate
//! for opening the database, and the Transactions edit pipeline").

use std::collections::HashMap;
use std::path::PathBuf;

use chrono::{NaiveDate, NaiveDateTime};
use orre_core::account;
use orre_core::backup::gate;
use orre_core::category::{self, CategoryKey, GroupOverview};
use orre_core::change::{self, HistoryEntry};
use orre_core::clock;
use orre_core::db::Database;
use orre_core::id::{AccountId, ChangeBatchId, MerchantAliasId, RuleId};
use orre_core::merchant::{self, AliasSummary};
use orre_core::money::Currency;
use orre_core::reporting::palette::{Palette, category_palette};
use orre_core::rule::{self, RuleSummary};
use orre_core::schedule;
use orre_core::tag;

use crate::accounts::store::StoreError;

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
    /// Keys of deleted categories, never handed out again (R.11): the new
    /// category form's key preview skips them as `category::add` does.
    pub retired_keys: Vec<CategoryKey>,
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

    /// Opens the database through the gate, which checks the restore lock.
    pub(super) fn open(&self) -> Result<Database, StoreError> {
        Ok(gate::open(&self.database, Database::open)?)
    }

    pub fn load(&self) -> Result<RulesData, StoreError> {
        load(&self.open()?)
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
        retired_keys: category::retired_keys(db)?,
        palette: category_palette(&db.reader()?, today)?,
        accounts,
        tags,
        today: Some(today),
    })
}
