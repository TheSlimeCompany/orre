//! `register`: the Transactions table's read model (ROADMAP M3.3;
//! `DESIGN.md` → "Data table (Transactions)"; `ARCHITECTURE.md` →
//! "Decision record: the Transactions view").
//!
//! The reports read the [`Ledger`], which keeps only what
//! counts: no transfers, no excluded rows, one line per split. The table is
//! the other view of the same rows: one [`RegisterRow`] per transaction,
//! transfers and excluded rows included, with its split lines inside it.
//!
//! - [`Register::load`] reads every transaction once, newest first, with
//!   the accounts and categories the table names. A person's ledger fits
//!   in memory (tens of thousands of rows), so the app reads it again only
//!   after a change, never per keystroke.
//! - [`Register::select`] is pure: it filters ([`Filter`]: account,
//!   category, dates, uncategorised, text, excluded), groups the rows into
//!   days with each day's net, newest first. Sorting by a column
//!   ([`Sort`]) drops the days: one flat list in that order, ties newest
//!   first ([`Listing::flat`]).
//! - [`Register::category_counts`] counts what each top-level group holds,
//!   all time, as the Category filter keeps rows: the accounts panel's
//!   CATEGORIES.
//! - **Category colours** ([`Colour`]) are ranked when the register is read
//!   (user, 2026-11-14): the six top-level groups with the most spending
//!   over the 12 months to `today` take `chart-1` … `chart-6`, their
//!   subcategories share the colour, and everything else (income,
//!   transfers, the `other` group, groups with no spending) is
//!   [`Colour::Other`]. Nothing is stored yet; M4.1 stores the colours so
//!   charts and table agree. Amounts in different currencies are never
//!   added: the ranking uses the currency with the most spending lines.
//!
//! - **Source and exclusion** (M3.10): each row says where it came from
//!   ([`RowSource`]: its import batch's file and day, or the day the user
//!   added it) and, when excluded, the user's details and the day. Days
//!   are local, read from the stored UTC times through SQLite's
//!   `localtime`, as [`crate::schedule::local_today`] reads today.
//! - **What a row takes**: a category of its own
//!   ([`RegisterRow::takes_category`], core's one rule,
//!   [`crate::category::assign`], read from the row), a share
//!   ([`RegisterRow::takes_share`]) and links ([`RegisterRow::takes_links`]),
//!   so the app offers an edit only where core takes it.
//!
//! Text search matches the display merchant, the bank's merchant and the
//! raw description, ignoring case. That text is the user's own data and
//! untrusted: it is only ever matched and shown, and nothing here logs it.

use std::cmp::Ordering;
use std::collections::{BTreeMap, HashMap};
use std::ops::Range;

use chrono::{Datelike as _, Days, Months, NaiveDate, NaiveTime, Weekday};
use rusqlite::Connection;

use crate::account::{self, AccountKind};
use crate::category::assign::{CategoryFacts, NoOwnCategory};
use crate::category::{CategoryKey, CategoryKind, CategoryTree};
use crate::db::Reader;
use crate::exclusion::ExclusionReason;
use crate::id::{AccountId, BatchId, LinkId, RuleId, TagId, TransactionId, TransferGroupId};
use crate::link::{LinkKind, LinkStatus, PersonOwed};
use crate::manual::Origin;
use crate::money::{Currency, Money};
use crate::transfer::TransferStatus;

use super::{DateRange, Error, Ledger};

/// The most category groups that get a colour of their own (`DESIGN.md` →
/// "Data visualisation": at most six named series).
pub const NAMED_COLOURS: usize = 6;

/// The key of the group that always draws as "Other", whatever it holds.
const OTHER_GROUP: &str = "other";

/// A category's marker colour.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Colour {
    /// `chart-1` … `chart-6`: the slot, from 1.
    Chart(u8),
    /// The "Other" marker: an outline, not a seventh colour.
    Other,
}

/// A category as the table shows it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RegisterCategory {
    /// `category.id`.
    pub id: i64,
    pub key: CategoryKey,
    pub label: String,
    pub kind: CategoryKind,
    /// Its group's ID; `None` for a group.
    pub group: Option<i64>,
    pub colour: Colour,
    /// The user hid it, or its group (M3.7): left out of the menus that
    /// set a category, kept in the Category filter and on its rows.
    pub hidden: bool,
}

/// An account as the table names it (closed ones too: their rows stay).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RegisterAccount {
    pub id: AccountId,
    pub name: String,
    pub kind: AccountKind,
    /// What its amounts are in: a new manual row's (M3.10).
    pub currency: Currency,
}

/// A tag as the inspector shows it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RegisterTag {
    pub id: TagId,
    pub name: String,
    /// Never leaves the app (`tag` module docs): drawn with a lock.
    pub private: bool,
}

/// One line of a split transaction.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SplitPart {
    pub amount: Money,
    /// `category.id`; `None` while the line is uncategorised.
    pub category: Option<i64>,
    /// The user's part of the line; `None` means all of it. Kept so an edit
    /// in the split editor doesn't drop it.
    pub my_share: Option<Money>,
    /// Who shares the line's cost: user text, never leaves the app.
    pub share_label: Option<String>,
    /// The user's note on the line: user text, never leaves the app.
    pub user_note: Option<String>,
}

impl SplitPart {
    /// A line of `amount` in `category`, all of it the user's, no note.
    pub fn new(amount: Money, category: Option<i64>) -> SplitPart {
        SplitPart {
            amount,
            category,
            my_share: None,
            share_label: None,
            user_note: None,
        }
    }
}

/// A row's category.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RowCategory {
    Uncategorised,
    /// `category.id`.
    Category(i64),
    /// Split into lines, in line order.
    Split(Vec<SplitPart>),
}

/// Which side of a link a row is on.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum LinkSide {
    /// The money in.
    Inflow,
    /// The purchase it pays back.
    Purchase,
}

/// What is on the other side of a link.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum LinkOther {
    Transaction(TransactionId),
    /// A settlement's person: a local label, never leaves the app.
    Person(String),
}

/// A suggested or confirmed link of a row (rejected ones aren't listed).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RowLink {
    pub id: LinkId,
    pub kind: LinkKind,
    pub status: LinkStatus,
    /// This row's side.
    pub side: LinkSide,
    pub other: LinkOther,
    /// Positive.
    pub amount: Money,
}

/// How a row is shared and what it can still take in links.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RowShare {
    /// The user's part; `None` means all of it.
    pub my_share: Option<Money>,
    /// Who owes the rest: a local label, never leaves the app.
    pub label: Option<String>,
    /// Claimed back as a whole (`reimbursable`).
    pub claim: bool,
    /// Money out: what refunds and reimbursements can still take (for a
    /// claim, what is still owed); money in: what is left to link. Never
    /// negative.
    pub left: Money,
}

impl RowShare {
    /// All the user's, nothing linked: `left` is the whole amount.
    pub fn whole(amount: Money) -> RowShare {
        RowShare {
            my_share: None,
            label: None,
            claim: false,
            left: Money::new(amount.minor().saturating_abs(), amount.currency()),
        }
    }
}

/// Where a row came from: the inspector's Source section (M3.10). Dates
/// are local days, read from the stored UTC times.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RowSource {
    /// From an import: the bank's row (`imported`), or the pot side an
    /// import created for it (`mirrored`). Undoing the batch removes it.
    Import {
        batch: BatchId,
        /// The file's name, without its directory.
        file: String,
        imported_on: NaiveDate,
        origin: Origin,
    },
    /// Entered by the user (`manual`): every field can change, and it can
    /// be deleted.
    Manual { created_on: NaiveDate },
}

impl RowSource {
    /// Whether the user entered it.
    pub fn is_manual(&self) -> bool {
        matches!(self, RowSource::Manual { .. })
    }

    /// The import batch it came from, if any.
    pub fn batch(&self) -> Option<BatchId> {
        match self {
            RowSource::Import { batch, .. } => Some(*batch),
            RowSource::Manual { .. } => None,
        }
    }
}

/// The transfer pair a row is in (M3.8).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RowPair {
    pub group: TransferGroupId,
    /// `Suggested` or `Confirmed`.
    pub status: TransferStatus,
}

/// One transaction in the table.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RegisterRow {
    pub id: TransactionId,
    pub account: AccountId,
    pub date: NaiveDate,
    pub time: Option<NaiveTime>,
    /// Signed: negative is money out.
    pub amount: Money,
    /// The amount in the currency spent, when it differs (a card abroad).
    pub original: Option<Money>,
    /// What the Merchant column shows: the display merchant, else the
    /// bank's merchant, else the raw description.
    pub merchant: String,
    /// The bank's merchant text, as imported.
    pub bank_merchant: Option<String>,
    /// The raw description, as imported (the inspector's "Raw description").
    pub description: String,
    /// The bank's kind of payment ("Card payment"), when the export has it.
    pub payment_type: Option<String>,
    pub category: RowCategory,
    /// The rule that set the category.
    pub rule: Option<RuleId>,
    /// In a confirmed transfer: the other side's accounts. Transfers are
    /// left out of the day nets, and the table names them by the other side.
    pub transfer: Option<Vec<AccountId>>,
    pub excluded: Option<ExclusionReason>,
    /// The user's details of the exclusion: user text, like the note.
    /// `None` when there are none or the row isn't excluded (M3.10).
    pub excluded_note: Option<String>,
    /// The local day it was excluded; `None` when it isn't (M3.10).
    pub excluded_on: Option<NaiveDate>,
    /// Where it came from (M3.10).
    pub source: RowSource,
    /// The user's own note.
    pub note: Option<String>,
    /// The bank's note, as imported: read-only.
    pub bank_note: Option<String>,
    /// Whether the user named the merchant (`merchant_display_source =
    /// user`); otherwise the name is derived or a rule's.
    pub merchant_set: bool,
    /// The row's tags, by name ignoring case.
    pub tags: Vec<TagId>,
    /// Whether it can be split: not in a suggested or confirmed transfer
    /// and not the money in of a confirmed link (`split` module docs).
    pub splittable: bool,
    /// In a suggested or confirmed transfer: no links, no share (M3.11).
    pub in_transfer: bool,
    /// The suggested or confirmed transfer pair it's in: the inspector's
    /// Transfer section (M3.8, [`super::pairing`]). `None` outside a pair
    /// and for a pair the user rejected.
    pub pair: Option<RowPair>,
    /// Its suggested and confirmed links, oldest first (M3.11).
    pub links: Vec<RowLink>,
    /// How it is shared and what it can still take (M3.11).
    pub share: RowShare,
}

impl RegisterRow {
    /// Whether the row is in a confirmed transfer.
    pub fn is_transfer(&self) -> bool {
        self.transfer.is_some()
    }

    /// Whether it needs a category: it takes one of its own and has none,
    /// or it is split and a line has none; never a confirmed transfer leg
    /// or a confirmed refund's money in
    /// ([`CategoryFacts::needs_category`]).
    pub fn is_uncategorised(&self) -> bool {
        self.category_facts().needs_category()
    }

    /// Whether it counts in the day's net: not a transfer, not excluded.
    pub fn counts_in_net(&self) -> bool {
        !self.is_transfer() && self.excluded.is_none()
    }

    /// The confirmed refund this row is the money in of (the oldest, if
    /// there were ever more than one): its purchase sets how it counts.
    pub fn confirmed_refund(&self) -> Option<&RowLink> {
        self.links.iter().find(|link| {
            link.side == LinkSide::Inflow
                && link.kind == LinkKind::Refund
                && link.status == LinkStatus::Confirmed
        })
    }

    /// Whether its category is its linked purchase's (M3.11): the money in
    /// of a confirmed refund, read-only until it's unlinked.
    pub fn category_follows_purchase(&self) -> bool {
        self.confirmed_refund().is_some()
    }

    /// What decides whether it takes a category of its own, read from the
    /// row as the database's rule reads it from the tables
    /// ([`crate::category::assign`]).
    pub fn category_facts(&self) -> CategoryFacts {
        CategoryFacts {
            split: matches!(self.category, RowCategory::Split(_)),
            confirmed_transfer: self.is_transfer(),
            confirmed_refund_in: self.category_follows_purchase(),
            has_category: matches!(self.category, RowCategory::Category(_)),
            split_line_uncategorised: matches!(
                &self.category,
                RowCategory::Split(parts) if parts.iter().any(|part| part.category.is_none())
            ),
        }
    }

    /// Why it takes no category of its own, if it doesn't: split, a leg of
    /// a confirmed transfer, or a linked refund's money in. Core's writes
    /// refuse (`inbox::own::recategorize`) or skip (an approved
    /// categorisation) the same rows.
    pub fn no_own_category(&self) -> Option<NoOwnCategory> {
        self.category_facts().no_own_category()
    }

    /// Whether its category can be set as a whole (module docs of
    /// [`crate::category::assign`]).
    pub fn takes_category(&self) -> bool {
        self.no_own_category().is_none()
    }

    /// Whether it can be shared or claimed back (the inspector's Share
    /// section, M3.11): money out outside a suggested or confirmed
    /// transfer.
    pub fn takes_share(&self) -> bool {
        self.amount.is_negative() && !self.in_transfer
    }

    /// Whether it can take links (the inspector's Links section, M3.11):
    /// money in or out, outside a suggested or confirmed transfer. Wider
    /// than what the link dialog offers ([`super::linking::kinds_from`]):
    /// the section also lists the links a row already has.
    pub fn takes_links(&self) -> bool {
        !self.in_transfer && !self.amount.is_zero()
    }
}

/// A preset date range for the Date filter.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum DatePreset {
    /// Monday to Sunday (ISO 8601) around today.
    ThisWeek,
    /// The Monday to Sunday before this week.
    LastWeek,
    ThisMonth,
    LastMonth,
    /// This month and the two before it.
    LastThreeMonths,
    ThisYear,
    #[default]
    AllTime,
}

impl DatePreset {
    /// In menu order.
    pub const ALL: [DatePreset; 7] = [
        DatePreset::ThisWeek,
        DatePreset::LastWeek,
        DatePreset::ThisMonth,
        DatePreset::LastMonth,
        DatePreset::LastThreeMonths,
        DatePreset::ThisYear,
        DatePreset::AllTime,
    ];

    /// The days it covers as of `today`; `None` for all time. Ranges run to
    /// the end of the period, so a row dated later this month (or this week)
    /// still shows. Weeks start on Monday.
    pub fn range(self, today: NaiveDate) -> Option<DateRange> {
        let month_start = today.with_day(1)?;
        let month_end = month_start.checked_add_months(Months::new(1))?.pred_opt()?;
        let week = today.week(Weekday::Mon);
        let (from, to) = match self {
            DatePreset::ThisWeek => (week.first_day(), week.last_day()),
            DatePreset::LastWeek => (
                week.first_day().checked_sub_days(Days::new(7))?,
                week.first_day().pred_opt()?,
            ),
            DatePreset::ThisMonth => (month_start, month_end),
            DatePreset::LastMonth => (
                month_start.checked_sub_months(Months::new(1))?,
                month_start.pred_opt()?,
            ),
            DatePreset::LastThreeMonths => {
                (month_start.checked_sub_months(Months::new(2))?, month_end)
            }
            DatePreset::ThisYear => (
                NaiveDate::from_ymd_opt(today.year(), 1, 1)?,
                NaiveDate::from_ymd_opt(today.year(), 12, 31)?,
            ),
            DatePreset::AllTime => return None,
        };
        DateRange::new(from, to).ok()
    }
}

/// What the table shows. Every part is optional and they AND together.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Filter {
    /// One account (a pot is its own account).
    pub account: Option<AccountId>,
    /// A category's ID; a group includes its subcategories. A split row
    /// matches when any of its lines does.
    pub category: Option<i64>,
    pub dates: Option<DateRange>,
    /// Only rows that need a category ([`RegisterRow::is_uncategorised`]).
    pub uncategorised: bool,
    /// Text in the merchant, the bank's merchant or the raw description,
    /// ignoring case. Blank matches everything.
    pub text: String,
    /// Excluded rows are hidden unless this is set (the Excluded filter,
    /// M3.10).
    pub include_excluded: bool,
}

/// A column the rows can be sorted by.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum SortColumn {
    Merchant,
    Category,
    Account,
    Amount,
}

/// How the rows are ordered; the default is by day, newest first.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum Sort {
    /// As imported: newest first.
    #[default]
    Default,
    /// By `column`, A–Z or smallest first.
    Ascending(SortColumn),
    /// By `column`, Z–A or largest first.
    Descending(SortColumn),
}

impl Sort {
    /// What a click on `column`'s header does: sort by it, then reverse,
    /// then back to the default order.
    pub fn next(self, column: SortColumn) -> Sort {
        match self {
            Sort::Ascending(current) if current == column => Sort::Descending(column),
            Sort::Descending(current) if current == column => Sort::Default,
            _ => Sort::Ascending(column),
        }
    }

    /// The column it sorts by, if any.
    pub fn column(self) -> Option<SortColumn> {
        match self {
            Sort::Default => None,
            Sort::Ascending(column) | Sort::Descending(column) => Some(column),
        }
    }
}

/// One day of a [`Listing`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ListingDay {
    pub date: NaiveDate,
    /// The net of the day's rows that count ([`RegisterRow::counts_in_net`]),
    /// per currency, never added across currencies. Empty when none count.
    pub net: Vec<Money>,
    /// The day's rows: a range of [`Listing::rows`].
    pub rows: Range<usize>,
}

/// What [`Register::select`] picked.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Listing {
    /// Indices into [`Register::rows`]: day by day, newest day first, when
    /// grouped; in the sort's order when [`Listing::flat`].
    pub rows: Vec<usize>,
    /// The days of [`Listing::rows`]; empty when [`Listing::flat`].
    pub days: Vec<ListingDay>,
    /// Whether the rows are one list in the sort's order, with no days
    /// (any sort but [`Sort::Default`]).
    pub flat: bool,
    /// How many rows match every part of the filter but `uncategorised`
    /// and need a category: the Uncategorised chip's count.
    pub uncategorised: usize,
}

impl Listing {
    pub fn len(&self) -> usize {
        self.rows.len()
    }

    pub fn is_empty(&self) -> bool {
        self.rows.is_empty()
    }
}

/// A top-level group and how many rows it holds
/// ([`Register::category_counts`]).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CategoryCount {
    /// The group's category ID, as [`Filter::category`] takes it.
    pub category: i64,
    pub rows: usize,
}

/// What the accounts panel's CATEGORIES lists ([`Register::category_counts`]).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct CategoryCounts {
    /// The groups that hold rows: most first, the `other` group last.
    pub groups: Vec<CategoryCount>,
    /// Rows that need a category, excluded ones left out: what the
    /// Uncategorised filter keeps with nothing else set.
    pub uncategorised: usize,
}

/// Every transaction, with the accounts and categories it names.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Register {
    rows: Vec<RegisterRow>,
    /// Each row's searchable text, lowercased, by row index.
    haystacks: Vec<String>,
    by_id: HashMap<TransactionId, usize>,
    accounts: Vec<RegisterAccount>,
    categories: Vec<RegisterCategory>,
    category_index: HashMap<i64, usize>,
    /// Every tag, by name ignoring case.
    tags: Vec<RegisterTag>,
    /// What each person owes, per currency, sorted by name (M3.11): the
    /// Settlement choices and the Owed by menu.
    people: Vec<PersonOwed>,
}

impl Register {
    /// A register of `rows` (any order: kept newest first, then by ID
    /// descending), naming `accounts` and `categories` (in menu order:
    /// each group, then its subcategories).
    pub fn new(
        mut rows: Vec<RegisterRow>,
        accounts: Vec<RegisterAccount>,
        categories: Vec<RegisterCategory>,
    ) -> Register {
        rows.sort_by(|a, b| b.date.cmp(&a.date).then(b.id.cmp(&a.id)));
        let haystacks = rows.iter().map(haystack).collect();
        let by_id = rows
            .iter()
            .enumerate()
            .map(|(index, row)| (row.id, index))
            .collect();
        let category_index = categories
            .iter()
            .enumerate()
            .map(|(index, category)| (category.id, index))
            .collect();
        Register {
            rows,
            haystacks,
            by_id,
            accounts,
            categories,
            category_index,
            tags: Vec::new(),
            people: Vec::new(),
        }
    }

    /// The register with `people` (what each owes; any order: kept by name,
    /// then currency).
    pub fn with_people(mut self, mut people: Vec<PersonOwed>) -> Register {
        people.sort_by(|a, b| {
            a.label
                .cmp(&b.label)
                .then_with(|| a.owed.currency().cmp(&b.owed.currency()))
        });
        self.people = people;
        self
    }

    /// What each person owes, per currency, sorted by name.
    pub fn people(&self) -> &[PersonOwed] {
        &self.people
    }

    /// The register with `tags` (any order: kept by name ignoring case,
    /// then ID), which its rows' [`RegisterRow::tags`] name.
    pub fn with_tags(mut self, mut tags: Vec<RegisterTag>) -> Register {
        tags.sort_by(|a, b| compare_text(&a.name, &b.name).then(a.id.cmp(&b.id)));
        self.tags = tags;
        self
    }

    /// Reads every transaction, account and category, with the colours
    /// ranked over the 12 months to `today` (module docs).
    pub fn load(reader: &Reader, today: NaiveDate) -> Result<Register, Error> {
        let conn = reader.conn();
        let categories = load_categories(reader, today)?;
        let accounts = load_accounts(conn)?;
        let rows = load_rows(conn)?;
        let tags = crate::tag::list_in(conn)?
            .into_iter()
            .map(|tag| RegisterTag {
                id: tag.id,
                name: tag.name.as_str().to_owned(),
                private: tag.private,
            })
            .collect();
        let people = crate::link::owed_by_label_in(conn)?;
        Ok(Register::new(rows, accounts, categories)
            .with_tags(tags)
            .with_people(people))
    }

    /// Every row, newest first.
    pub fn rows(&self) -> &[RegisterRow] {
        &self.rows
    }

    /// Whether row `id`'s merchant, bank merchant or raw description holds
    /// `text`, ignoring case (the table's search); blank matches all.
    pub fn row_matches(&self, id: TransactionId, text: &str) -> bool {
        let text = text.trim().to_lowercase();
        text.is_empty()
            || self
                .by_id
                .get(&id)
                .is_some_and(|&index| self.haystacks[index].contains(&text))
    }

    pub fn row(&self, id: TransactionId) -> Option<&RegisterRow> {
        self.by_id.get(&id).map(|&index| &self.rows[index])
    }

    pub fn accounts(&self) -> &[RegisterAccount] {
        &self.accounts
    }

    pub fn account(&self, id: AccountId) -> Option<&RegisterAccount> {
        self.accounts.iter().find(|account| account.id == id)
    }

    /// Every tag, by name ignoring case.
    pub fn tags(&self) -> &[RegisterTag] {
        &self.tags
    }

    pub fn tag(&self, id: TagId) -> Option<&RegisterTag> {
        self.tags.iter().find(|tag| tag.id == id)
    }

    /// Every category in menu order: each group, then its subcategories.
    pub fn categories(&self) -> &[RegisterCategory] {
        &self.categories
    }

    pub fn category(&self, id: i64) -> Option<&RegisterCategory> {
        self.category_index
            .get(&id)
            .map(|&index| &self.categories[index])
    }

    /// The categories of `row`, for its label and markers: one for a
    /// categorised row, each line's (in line order, uncategorised lines
    /// left out) for a split one.
    pub fn row_categories(&self, row: &RegisterRow) -> Vec<&RegisterCategory> {
        match &row.category {
            RowCategory::Uncategorised => Vec::new(),
            RowCategory::Category(id) => self.category(*id).into_iter().collect(),
            RowCategory::Split(parts) => parts
                .iter()
                .filter_map(|part| part.category.and_then(|id| self.category(id)))
                .collect(),
        }
    }

    /// The purchase `row`'s category follows: the other side of its
    /// [`RegisterRow::confirmed_refund`], when the register has it.
    pub fn refund_purchase(&self, row: &RegisterRow) -> Option<&RegisterRow> {
        match row.confirmed_refund()?.other {
            LinkOther::Transaction(id) => self.row(id),
            LinkOther::Person(_) => None,
        }
    }

    /// The rows `filter` keeps: grouped by day, newest first, under
    /// [`Sort::Default`]; under any other sort, one flat list in that order
    /// with no days (ties newest first, then by ID).
    pub fn select(&self, filter: &Filter, sort: Sort) -> Listing {
        let text = filter.text.trim().to_lowercase();
        let mut uncategorised = 0;
        let mut kept: Vec<usize> = Vec::new();
        for (index, row) in self.rows.iter().enumerate() {
            if !self.matches(row, filter) {
                continue;
            }
            if !text.is_empty() && !self.haystacks[index].contains(&text) {
                continue;
            }
            let needs_category = row.is_uncategorised();
            if needs_category {
                uncategorised += 1;
            }
            if filter.uncategorised && !needs_category {
                continue;
            }
            kept.push(index);
        }

        if sort != Sort::Default {
            kept.sort_by(|&a, &b| self.compare(a, b, sort));
            return Listing {
                rows: kept,
                days: Vec::new(),
                flat: true,
                uncategorised,
            };
        }

        // `kept` is newest first, so each day's rows are together.
        let mut rows = Vec::with_capacity(kept.len());
        let mut days = Vec::new();
        for day_rows in kept.chunk_by(|&a, &b| self.rows[a].date == self.rows[b].date) {
            let start = rows.len();
            rows.extend_from_slice(day_rows);
            days.push(ListingDay {
                date: self.rows[day_rows[0]].date,
                net: net(day_rows.iter().map(|&index| &self.rows[index])),
                rows: start..rows.len(),
            });
        }
        Listing {
            rows,
            days,
            flat: false,
            uncategorised,
        }
    }

    /// `filter` with the parts that hide row `id` cleared, so the table
    /// shows it: a new row the user just added (M3.10), or one they asked
    /// to see again. Parts that already show it stay as they are; an
    /// excluded row turns the Excluded filter on. `None` when the register
    /// has no such row.
    pub fn filter_showing(&self, filter: &Filter, id: TransactionId) -> Option<Filter> {
        let index = *self.by_id.get(&id)?;
        let row = &self.rows[index];
        let mut shown = filter.clone();
        if row.excluded.is_some() {
            shown.include_excluded = true;
        }
        if filter.account.is_some_and(|account| account != row.account) {
            shown.account = None;
        }
        if filter
            .dates
            .is_some_and(|range| !(range.from() <= row.date && row.date <= range.to()))
        {
            shown.dates = None;
        }
        if filter
            .category
            .is_some_and(|wanted| !self.in_category(row, wanted))
        {
            shown.category = None;
        }
        if filter.uncategorised && !row.is_uncategorised() {
            shown.uncategorised = false;
        }
        if !self.row_matches(id, &filter.text) {
            shown.text.clear();
        }
        Some(shown)
    }

    /// How many rows each top-level group holds, all time and in every
    /// account: the accounts panel's CATEGORIES. A group's count is what
    /// the Category filter keeps for it with nothing else set
    /// ([`Register::select`]), so it always matches what picking it shows:
    /// its subcategories' rows included, a split row once however many of
    /// its lines are in the group, excluded rows left out as the table
    /// hides them. Groups that hold nothing are left out; a hidden group
    /// that holds something stays, as the Category filter keeps it. Most
    /// rows first, ties in menu order, the `other` group last.
    pub fn category_counts(&self) -> CategoryCounts {
        let mut counts: HashMap<i64, usize> = HashMap::new();
        let mut uncategorised = 0;
        let mut groups: Vec<i64> = Vec::new();
        for row in self.rows.iter().filter(|row| row.excluded.is_none()) {
            if row.is_uncategorised() {
                uncategorised += 1;
            }
            let ids: Vec<i64> = match &row.category {
                RowCategory::Uncategorised => Vec::new(),
                RowCategory::Category(id) => vec![*id],
                RowCategory::Split(parts) => {
                    parts.iter().filter_map(|part| part.category).collect()
                }
            };
            groups.clear();
            for id in ids {
                let Some(category) = self.category(id) else {
                    continue;
                };
                let group = category.group.unwrap_or(category.id);
                if !groups.contains(&group) {
                    groups.push(group);
                }
            }
            for group in &groups {
                *counts.entry(*group).or_default() += 1;
            }
        }
        let mut groups: Vec<(&RegisterCategory, usize)> = self
            .categories
            .iter()
            .filter(|category| category.group.is_none())
            .filter_map(|category| {
                let rows = counts.get(&category.id).copied().unwrap_or(0);
                (rows > 0).then_some((category, rows))
            })
            .collect();
        // Stable: ties keep menu order.
        groups.sort_by_key(|&(category, rows)| {
            (
                category.key.as_str() == OTHER_GROUP,
                std::cmp::Reverse(rows),
            )
        });
        CategoryCounts {
            groups: groups
                .into_iter()
                .map(|(category, rows)| CategoryCount {
                    category: category.id,
                    rows,
                })
                .collect(),
            uncategorised,
        }
    }

    /// Whether `row`, or a line of it when it's split, is in category
    /// `wanted` or, for a group, one of its subcategories.
    fn in_category(&self, row: &RegisterRow, wanted: i64) -> bool {
        let in_wanted = |id: i64| {
            id == wanted
                || self
                    .category(id)
                    .is_some_and(|category| category.group == Some(wanted))
        };
        match &row.category {
            RowCategory::Uncategorised => false,
            RowCategory::Category(id) => in_wanted(*id),
            RowCategory::Split(parts) => parts
                .iter()
                .any(|part| part.category.is_some_and(in_wanted)),
        }
    }

    /// Every part of `filter` but the text and `uncategorised`.
    fn matches(&self, row: &RegisterRow, filter: &Filter) -> bool {
        if row.excluded.is_some() && !filter.include_excluded {
            return false;
        }
        if filter.account.is_some_and(|account| account != row.account) {
            return false;
        }
        if let Some(range) = filter.dates
            && !(range.from() <= row.date && row.date <= range.to())
        {
            return false;
        }
        if let Some(wanted) = filter.category
            && !self.in_category(row, wanted)
        {
            return false;
        }
        true
    }

    /// The order of two rows under `sort`; ties keep the
    /// default order (newest first, then by ID).
    fn compare(&self, a: usize, b: usize, sort: Sort) -> Ordering {
        let (column, descending) = match sort {
            Sort::Default => return a.cmp(&b),
            Sort::Ascending(column) => (column, false),
            Sort::Descending(column) => (column, true),
        };
        let (row_a, row_b) = (&self.rows[a], &self.rows[b]);
        let order = match column {
            SortColumn::Merchant => compare_text(&row_a.merchant, &row_b.merchant),
            SortColumn::Category => {
                compare_text(&self.category_label(row_a), &self.category_label(row_b))
            }
            SortColumn::Account => compare_text(
                self.account_name(row_a.account),
                self.account_name(row_b.account),
            ),
            SortColumn::Amount => row_a
                .amount
                .currency()
                .code()
                .cmp(row_b.amount.currency().code())
                .then(row_a.amount.minor().cmp(&row_b.amount.minor())),
        };
        let order = if descending { order.reverse() } else { order };
        order.then(a.cmp(&b))
    }

    fn account_name(&self, id: AccountId) -> &str {
        self.account(id).map_or("", |account| account.name.as_str())
    }

    /// The text a row sorts by in the Category column: its category's
    /// label, its lines' labels, "Transfer", or nothing (uncategorised
    /// rows sort first).
    fn category_label(&self, row: &RegisterRow) -> String {
        if row.is_transfer() {
            return "Transfer".to_owned();
        }
        self.row_categories(row)
            .iter()
            .map(|category| category.label.as_str())
            .collect::<Vec<_>>()
            .join(", ")
    }
}

/// Text compared ignoring case, then as written.
fn compare_text(a: &str, b: &str) -> Ordering {
    a.to_lowercase()
        .cmp(&b.to_lowercase())
        .then_with(|| a.cmp(b))
}

/// The searchable text of a row, lowercased.
fn haystack(row: &RegisterRow) -> String {
    let mut text = row.merchant.to_lowercase();
    if let Some(merchant) = &row.bank_merchant {
        text.push('\n');
        text.push_str(&merchant.to_lowercase());
    }
    text.push('\n');
    text.push_str(&row.description.to_lowercase());
    text
}

/// The net of the rows that count, per currency (in code order).
fn net<'a>(rows: impl Iterator<Item = &'a RegisterRow>) -> Vec<Money> {
    let mut sums: BTreeMap<String, (Currency, i128)> = BTreeMap::new();
    for row in rows.filter(|row| row.counts_in_net()) {
        let currency = row.amount.currency();
        sums.entry(currency.code().to_owned())
            .or_insert((currency, 0))
            .1 += i128::from(row.amount.minor());
    }
    sums.into_values()
        .map(|(currency, sum)| {
            // A day's net can't leave i64 in practice; clamp rather than fail.
            let minor = i64::try_from(sum).unwrap_or(if sum < 0 { i64::MIN } else { i64::MAX });
            Money::new(minor, currency)
        })
        .collect()
}

fn load_accounts(conn: &Connection) -> Result<Vec<RegisterAccount>, Error> {
    let mut statement = conn.prepare("SELECT id, name, kind, currency FROM account ORDER BY id")?;
    let accounts = statement
        .query_map([], |row| {
            let kind: String = row.get("kind")?;
            Ok(RegisterAccount {
                id: row.get("id")?,
                name: row.get("name")?,
                kind: AccountKind::from_key(&kind).ok_or_else(|| account::invalid(row, "kind"))?,
                currency: row.get("currency")?,
            })
        })?
        .collect::<Result<_, _>>()?;
    Ok(accounts)
}

/// The categories in menu order, with their colours ranked over the 12
/// months to `today`.
fn load_categories(reader: &Reader, today: NaiveDate) -> Result<Vec<RegisterCategory>, Error> {
    let tree = CategoryTree::load(reader.conn())?;
    let ranked = ranked_groups(reader, &tree, today)?;
    let colour_of = |group: i64| {
        ranked
            .iter()
            .position(|&id| id == group)
            .map_or(Colour::Other, |slot| Colour::Chart(slot as u8 + 1))
    };
    let mut categories = Vec::with_capacity(tree.len());
    for group in tree.groups() {
        let parent = group.category();
        let colour = colour_of(parent.id());
        categories.push(RegisterCategory {
            id: parent.id(),
            key: parent.key().clone(),
            label: parent.label().to_owned(),
            kind: parent.kind(),
            group: None,
            colour,
            hidden: parent.is_hidden(),
        });
        for child in group.children() {
            categories.push(RegisterCategory {
                id: child.id(),
                key: child.key().clone(),
                label: child.label().to_owned(),
                kind: child.kind(),
                group: Some(parent.id()),
                colour,
                hidden: !tree.is_visible(child.key()),
            });
        }
    }
    Ok(categories)
}

/// The IDs of `tree`'s groups that take the six colours, most spending
/// first (module docs). The Inbox's [`super::palette`] uses the same
/// ranking, so a category has one colour everywhere.
pub(crate) fn ranked_groups(
    reader: &Reader,
    tree: &CategoryTree,
    today: NaiveDate,
) -> Result<Vec<i64>, Error> {
    rank_groups(reader, today, |id| {
        tree.groups()
            .iter()
            .find(|group| {
                group.category().id() == id || group.children().iter().any(|child| child.id() == id)
            })
            .map(|group| group.category().id())
    })
}

/// The IDs of the groups that take the six colours, most spending first:
/// spending lines over the 12 months to `today`, the user's share, in the
/// currency with the most such lines. `group_of` maps a category ID to its
/// group's. The `other` group is never ranked.
fn rank_groups(
    reader: &Reader,
    today: NaiveDate,
    group_of: impl Fn(i64) -> Option<i64>,
) -> Result<Vec<i64>, Error> {
    let from = today
        .checked_sub_months(Months::new(12))
        .and_then(|day| day.succ_opt())
        .unwrap_or(today);
    let lines = Ledger::new()
        .within(DateRange::new(from, today)?)
        .lines(reader)?;
    let mut by_currency: HashMap<Currency, usize> = HashMap::new();
    let mut spent: HashMap<(Currency, i64), i128> = HashMap::new();
    for line in &lines {
        let Some(category) = &line.category else {
            continue;
        };
        // The `other` group stays outlined, however much it holds.
        let in_other =
            category.key.as_str() == OTHER_GROUP || category.key.parent() == Some(OTHER_GROUP);
        if category.kind != CategoryKind::Spending || in_other {
            continue;
        }
        let Some(group) = group_of(category.id) else {
            continue;
        };
        let code = line.personal.currency();
        *by_currency.entry(code).or_default() += 1;
        *spent.entry((code, group)).or_default() -= i128::from(line.personal.minor());
    }
    let Some(currency) = by_currency
        .iter()
        .max_by(|a, b| a.1.cmp(b.1).then(b.0.code().cmp(a.0.code())))
        .map(|(code, _)| *code)
    else {
        return Ok(Vec::new());
    };
    let mut ranked: Vec<(i64, i128)> = spent
        .into_iter()
        .filter(|((code, _), amount)| *code == currency && *amount > 0)
        .map(|((_, group), amount)| (group, amount))
        .collect();
    // Most spending first; ties by group ID, so the order is stable.
    ranked.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
    Ok(ranked
        .into_iter()
        .take(NAMED_COLOURS)
        .map(|(group, _)| group)
        .collect())
}

fn load_rows(conn: &Connection) -> Result<Vec<RegisterRow>, Error> {
    // Split lines and transfer partners first, by transaction.
    let mut splits: HashMap<TransactionId, Vec<SplitPart>> = HashMap::new();
    {
        let mut statement = conn.prepare(
            "SELECT s.transaction_id, s.amount_minor, s.category_id, t.currency, \
                    s.my_share_minor, s.share_label, s.user_note \
               FROM transaction_split s JOIN \"transaction\" t ON t.id = s.transaction_id \
              ORDER BY s.transaction_id, s.line_no",
        )?;
        let rows = statement.query_map([], |row| {
            let currency: Currency = row.get(3)?;
            Ok((
                row.get::<_, TransactionId>(0)?,
                SplitPart {
                    amount: Money::new(row.get(1)?, currency),
                    category: row.get(2)?,
                    my_share: row
                        .get::<_, Option<i64>>(4)?
                        .map(|minor| Money::new(minor, currency)),
                    share_label: row.get(5)?,
                    user_note: row.get(6)?,
                },
            ))
        })?;
        for row in rows {
            let (transaction, part) = row?;
            splits.entry(transaction).or_default().push(part);
        }
    }
    let mut partners: HashMap<TransactionId, Vec<AccountId>> = HashMap::new();
    {
        let mut statement = conn.prepare(
            "SELECT t.id, o.account_id FROM \"transaction\" t \
               JOIN transfer_group g ON g.id = t.transfer_group_id AND g.status = 'confirmed' \
               JOIN \"transaction\" o ON o.transfer_group_id = t.transfer_group_id \
                                      AND o.id <> t.id \
              ORDER BY t.id, o.id",
        )?;
        let rows = statement.query_map([], |row| {
            Ok((row.get::<_, TransactionId>(0)?, row.get::<_, AccountId>(1)?))
        })?;
        for row in rows {
            let (transaction, account) = row?;
            partners.entry(transaction).or_default().push(account);
        }
    }

    // Tags by name ignoring case, as the inspector lists them.
    let mut tags: HashMap<TransactionId, Vec<TagId>> = HashMap::new();
    {
        let mut statement = conn.prepare(
            "SELECT l.transaction_id, l.tag_id FROM transaction_tag l \
               JOIN tag ON tag.id = l.tag_id \
              ORDER BY l.transaction_id, tag.name, tag.id",
        )?;
        let rows = statement.query_map([], |row| {
            Ok((row.get::<_, TransactionId>(0)?, row.get::<_, TagId>(1)?))
        })?;
        for row in rows {
            let (transaction, tag) = row?;
            tags.entry(transaction).or_default().push(tag);
        }
    }

    // Suggested and confirmed links, on both sides.
    let mut links: HashMap<TransactionId, Vec<RowLink>> = HashMap::new();
    {
        let mut statement = conn.prepare(
            "SELECT l.id, l.kind, l.status, l.inflow_id, l.target_id, l.share_label, \
                    l.amount_minor, t.currency \
               FROM transaction_link l JOIN \"transaction\" t ON t.id = l.inflow_id \
              WHERE l.status <> 'rejected' ORDER BY l.id",
        )?;
        let rows = statement.query_map([], |row| {
            let kind: String = row.get(1)?;
            let status: String = row.get(2)?;
            Ok((
                row.get::<_, LinkId>(0)?,
                LinkKind::from_key(&kind).ok_or_else(|| account::invalid(row, "kind"))?,
                LinkStatus::from_key(&status).ok_or_else(|| account::invalid(row, "status"))?,
                row.get::<_, TransactionId>(3)?,
                row.get::<_, Option<TransactionId>>(4)?,
                row.get::<_, Option<String>>(5)?,
                Money::new(row.get(6)?, row.get(7)?),
            ))
        })?;
        for row in rows {
            let (id, kind, status, inflow, target, label, amount) = row?;
            let other = match (target, label) {
                (Some(target), _) => {
                    links.entry(target).or_default().push(RowLink {
                        id,
                        kind,
                        status,
                        side: LinkSide::Purchase,
                        other: LinkOther::Transaction(inflow),
                        amount,
                    });
                    LinkOther::Transaction(target)
                }
                (None, Some(label)) => LinkOther::Person(label),
                (None, None) => continue,
            };
            links.entry(inflow).or_default().push(RowLink {
                id,
                kind,
                status,
                side: LinkSide::Inflow,
                other,
                amount,
            });
        }
    }

    let mut statement = conn.prepare(
        "SELECT t.id, t.account_id, t.date, t.time, t.amount_minor, t.currency, \
                t.original_amount_minor, t.original_currency, t.merchant, \
                t.merchant_display, t.merchant_display_source, t.description_raw, \
                t.source_type, t.category_id, t.rule_id, t.excluded_reason, t.user_note, \
                t.note, g.status AS transfer_status, t.transfer_group_id, t.my_share_minor, t.share_label, \
                t.reimbursable, t.excluded_note, t.origin, t.batch_id, b.source_file, \
                date(t.excluded_at, 'localtime') AS excluded_on, \
                date(t.created_at, 'localtime') AS created_on, \
                date(b.imported_at, 'localtime') AS imported_on, \
                EXISTS (SELECT 1 FROM transaction_link k \
                         WHERE k.inflow_id = t.id AND k.status = 'confirmed') AS linked_in \
           FROM \"transaction\" t \
           LEFT JOIN transfer_group g ON g.id = t.transfer_group_id \
           LEFT JOIN import_batch b ON b.id = t.batch_id \
          ORDER BY t.date DESC, t.id DESC",
    )?;
    let rows = statement.query_map([], |row| {
        let id: TransactionId = row.get("id")?;
        let currency: Currency = row.get("currency")?;
        let original = match (
            row.get::<_, Option<i64>>("original_amount_minor")?,
            row.get::<_, Option<Currency>>("original_currency")?,
        ) {
            (Some(minor), Some(currency)) => Some(Money::new(minor, currency)),
            _ => None,
        };
        let time = row
            .get::<_, Option<String>>("time")?
            .map(|text| {
                NaiveTime::parse_from_str(&text, "%H:%M:%S")
                    .map_err(|_| account::invalid(row, "time"))
            })
            .transpose()?;
        let excluded = row
            .get::<_, Option<String>>("excluded_reason")?
            .map(|key| {
                ExclusionReason::from_key(&key)
                    .ok_or_else(|| account::invalid(row, "excluded_reason"))
            })
            .transpose()?;
        let origin: String = row.get("origin")?;
        let origin = Origin::from_key(&origin).ok_or_else(|| account::invalid(row, "origin"))?;
        let source = match (origin, row.get::<_, Option<BatchId>>("batch_id")?) {
            (Origin::Manual, _) | (_, None) => RowSource::Manual {
                created_on: account::date(row, "created_on")?,
            },
            (origin, Some(batch)) => RowSource::Import {
                batch,
                file: row.get("source_file")?,
                imported_on: account::date(row, "imported_on")?,
                origin,
            },
        };
        let excluded_on = match excluded {
            Some(_) => Some(account::date(row, "excluded_on")?),
            None => None,
        };
        let bank_merchant: Option<String> = row.get("merchant")?;
        let display: Option<String> = row.get("merchant_display")?;
        let description: String = row.get("description_raw")?;
        let merchant = [display.as_deref(), bank_merchant.as_deref()]
            .into_iter()
            .flatten()
            .map(str::trim)
            .find(|name| !name.is_empty())
            .unwrap_or_else(|| description.trim())
            .to_owned();
        let transfer_status: Option<String> = row.get("transfer_status")?;
        let confirmed = transfer_status.as_deref() == Some("confirmed");
        let in_transfer = matches!(transfer_status.as_deref(), Some("suggested" | "confirmed"));
        let pair = match transfer_status
            .as_deref()
            .and_then(TransferStatus::from_key)
        {
            Some(status @ (TransferStatus::Suggested | TransferStatus::Confirmed)) => row
                .get::<_, Option<TransferGroupId>>("transfer_group_id")?
                .map(|group| RowPair { group, status }),
            _ => None,
        };
        let linked_in: bool = row.get("linked_in")?;
        let category = match row.get::<_, Option<i64>>("category_id")? {
            Some(category) => RowCategory::Category(category),
            None => RowCategory::Uncategorised,
        };
        let amount = Money::new(row.get("amount_minor")?, currency);
        let share = RowShare {
            my_share: row
                .get::<_, Option<i64>>("my_share_minor")?
                .map(|minor| Money::new(minor, currency)),
            label: row.get("share_label")?,
            claim: row.get("reimbursable")?,
            ..RowShare::whole(amount)
        };
        Ok(RegisterRow {
            id,
            account: row.get("account_id")?,
            date: account::date(row, "date")?,
            time,
            amount,
            original,
            merchant,
            bank_merchant,
            description,
            payment_type: row.get("source_type")?,
            category,
            rule: row.get("rule_id")?,
            transfer: confirmed.then(Vec::new),
            excluded,
            excluded_note: row.get("excluded_note")?,
            excluded_on,
            source,
            note: row.get("user_note")?,
            bank_note: row.get("note")?,
            merchant_set: row
                .get::<_, Option<String>>("merchant_display_source")?
                .as_deref()
                == Some("user"),
            tags: Vec::new(),
            splittable: !in_transfer && !linked_in,
            in_transfer,
            pair,
            links: Vec::new(),
            share,
        })
    })?;
    let mut out = Vec::new();
    for row in rows {
        let mut row = row?;
        if let Some(parts) = splits.remove(&row.id) {
            row.category = RowCategory::Split(parts);
        }
        if let Some(row_tags) = tags.remove(&row.id) {
            row.tags = row_tags;
        }
        if let Some(transfer) = &mut row.transfer {
            *transfer = partners.remove(&row.id).unwrap_or_default();
        }
        if let Some(row_links) = links.remove(&row.id) {
            let taken: i64 = row_links
                .iter()
                .filter(|link| link.status == LinkStatus::Confirmed)
                .map(|link| link.amount.minor())
                .sum();
            let left = row
                .amount
                .minor()
                .saturating_abs()
                .saturating_sub(taken)
                .max(0);
            row.share.left = Money::new(left, row.amount.currency());
            row.links = row_links;
        }
        out.push(row);
    }
    Ok(out)
}

#[cfg(test)]
mod tests;
