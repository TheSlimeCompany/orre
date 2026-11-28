//! `overview`: everything the app's Overview screen shows for one month,
//! read in one call (ROADMAP M4.1).
//!
//! The screen shows one currency, the [display currency](Overview::currency):
//! the one with the most counted lines (income and spending, the user's
//! share, not zero) in the chart's 12 months for the account filter, ties
//! broken by currency code. With no such line it is the filtered account's
//! currency, else the first account's, else GBP. Amounts in different
//! currencies are never added, so lines in any other currency are left out
//! of every figure and listed in [`Overview::other_currencies`], for the app
//! to say so.
//!
//! Lines are classified as [`super::summary`] does (`Basis::Personal`, then
//! [`Side`]): income positive, spending positive with refunds reducing it,
//! net income minus spending. For all accounts the months agree with
//! `monthly_summary`.
//!
//! # The chart's window
//!
//! The 12 months ending at the month of `today`, unless the selected month
//! is earlier than the first of them: then the 12 months ending at the
//! selected month. A selected month after `today` leaves the window alone.
//! The previous month's figures are read whether or not the window has it.
//!
//! Months come from [`Period`] only (`containing`, `range`, `next`,
//! `previous`, `through`), so the month's cut is defined in one place:
//! the user's month start ([`setting::month_start`]), read once per call
//! inside its read transaction and returned as [`Overview::month_start`],
//! so the app cuts the same months without reading it again. Each month's
//! figures carry its dates ([`MonthFigures::range`]).
//!
//! The screen opens on [`landing_month`]: today's month, or the latest
//! month with data when today's has none yet (the statements lag). The
//! month is chosen together with the display currency: it has counted lines
//! in the currency the overview for that month displays, so it never opens
//! on a month of zeros because its only lines are in a minority currency.
//! [`opening_overview`] reads the landing month and its overview together.
//!
//! Each public read runs in one read transaction, so its statements agree
//! even when an import commits meanwhile.
//!
//! # Changes
//!
//! Spent and income are compared with the previous month as a whole-number
//! percentage of it, rounded half away from zero, in integer arithmetic;
//! `None` when the previous month's figure is zero. Net is compared as a
//! signed amount: a percentage of a number that changes sign misleads.
//!
//! # Categories
//!
//! The selected month's spending, one entry per top-level group that has a
//! colour slot ([`super::palette`]); every other group (the `other` group
//! included) adds into one [`SpendingGroup::Other`] entry, and uncategorised
//! money out is [`SpendingGroup::Uncategorised`]. Named entries come
//! largest first, then Other, then Uncategorised. An entry with lines is
//! listed even when its refunds outweigh its purchases (amount at or below
//! zero); one with no lines is not.
//!
//! # Recurring
//!
//! The detected series of money out ([`crate::recurring`]) that haven't
//! lapsed, on the account filter, in the display currency, due from `today`
//! through 30 days after it, whichever month is selected. Recurring income
//! (a salary) isn't a payment, so it isn't listed. A series whose expected
//! date has passed without an import showing it (the statements lag) is
//! rolled forward along its cadence to its first date on or after `today`,
//! so a late import doesn't hide a payment that is still coming; but only
//! when it is at most 30 days overdue. Longer, and the account more likely
//! stopped being imported (an old card) than lags, so the series isn't
//! listed: detection judges `lapsed` from the account's own rows, which a
//! dormant account never has.
//!
//! Nothing here logs transaction text.

use std::collections::{BTreeMap, HashMap};

use chrono::{Days, NaiveDate};
use rusqlite::{Transaction, TransactionBehavior};

use crate::account::Account;
use crate::cancel::Cancel;
use crate::category::{CategoryKey, CategoryTree};
use crate::db::Reader;
use crate::id::AccountId;
use crate::money::{Currency, Money, MoneyError};
use crate::recurring::{self, Cadence, Direction};
use crate::setting;

use super::palette::{Palette, category_palette};
use super::{Basis, DateRange, Error, Ledger, LedgerLine, MonthStart, Period, Side, balance};

/// How many months the chart shows.
pub const WINDOW_MONTHS: usize = 12;

/// How many days ahead of today the recurring list looks, inclusive.
pub const RECURRING_DAYS: u64 = 30;

/// What to read.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct OverviewQuery {
    /// The selected month.
    pub month: Period,
    /// One account (a pot is its own account), or all of them.
    pub account: Option<AccountId>,
    /// The current day: ends the chart's window and starts the recurring
    /// list.
    pub today: NaiveDate,
}

/// One month's figures in the display currency.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MonthFigures {
    pub period: Period,
    /// The month's days under [`Overview::month_start`].
    pub range: DateRange,
    /// Money earned or received, positive.
    pub income: Money,
    /// Money spent, positive; refunds reduce it, so it can go below zero.
    pub spent: Money,
    /// Income minus spent.
    pub net: Money,
}

/// How a spending entry is named.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SpendingGroup {
    /// A top-level group with a colour slot.
    Named {
        key: CategoryKey,
        /// The user's label.
        label: String,
        /// Its chart colour, 1 to [`super::palette::SLOTS`].
        slot: u8,
    },
    /// Every group without a slot, the `other` group included.
    Other,
    /// Money out with no category.
    Uncategorised,
}

/// One entry of the selected month's spending by category.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CategorySpend {
    pub group: SpendingGroup,
    /// Spent, positive; at or below zero when refunds outweigh purchases.
    pub amount: Money,
    /// `amount` as a whole-number percentage of the month's spending,
    /// rounded half away from zero; 0 when that is not above zero.
    pub share_percent: i64,
}

/// A recurring payment (money out) due in the next 30 days.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct UpcomingPayment {
    /// The display merchant of the series' latest payment.
    pub merchant: String,
    pub cadence: Cadence,
    pub next: NaiveDate,
    /// The typical amount, signed: money out is negative.
    pub amount: Money,
    /// The series' category, if it has one.
    pub category: Option<CategoryKey>,
    /// The category's colour slot, when it has one.
    pub slot: Option<u8>,
}

/// Everything the Overview screen shows (module docs).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Overview {
    /// The month start every month here is cut by, read with the figures.
    pub month_start: MonthStart,
    /// The one currency every amount is in.
    pub currency: Currency,
    /// Other currencies with counted lines in the window, by code. Their
    /// lines are in no figure.
    pub other_currencies: Vec<Currency>,
    /// Whether the database has any transaction at all (the empty state).
    pub has_transactions: bool,
    /// The earliest month with a counted line on the account filter, in any
    /// currency; `None` when there is none.
    pub earliest_month: Option<Period>,
    /// The chart's 12 months, oldest first.
    pub months: Vec<MonthFigures>,
    /// The selected month (in the window or not).
    pub selected: MonthFigures,
    /// The month before it (in the window or not).
    pub previous: MonthFigures,
    /// Spent against the previous month, in whole percent; `None` when the
    /// previous month spent nothing.
    pub spent_change_percent: Option<i64>,
    /// Income against the previous month, in whole percent; `None` when the
    /// previous month earned nothing.
    pub income_change_percent: Option<i64>,
    /// Net minus the previous month's net.
    pub net_change: Money,
    /// The selected month's spending by category (module docs).
    pub categories: Vec<CategorySpend>,
    /// Recurring payments due from today, earliest first, ties by merchant.
    pub recurring: Vec<UpcomingPayment>,
    /// The sum of the amounts in `recurring`: at or below zero.
    pub recurring_total: Money,
}

/// Reads the overview for `query` (module docs). A selected month with no
/// month before it (0001-01) is [`Error::InvalidPeriod`]; an unknown
/// account is [`Error::UnknownAccount`].
pub fn overview(reader: &Reader, query: &OverviewQuery) -> Result<Overview, Error> {
    overview_cancellable(reader, query, &Cancel::never())
}

/// [`overview`], stopping with [`Error::Cancelled`] soon after `cancel` is
/// cancelled (SLI-37: the Overview drops a read it no longer wants). It
/// checks between the read's steps and inside the recurring search.
pub fn overview_cancellable(
    reader: &Reader,
    query: &OverviewQuery,
    cancel: &Cancel,
) -> Result<Overview, Error> {
    let tx = Transaction::new_unchecked(reader.conn(), TransactionBehavior::Deferred)?;
    let start = setting::month_start(reader)?;
    let overview = overview_in_transaction(reader, query, start, cancel)?;
    drop(tx);
    Ok(overview)
}

/// The overview for `query` with its month replaced by the
/// [landing month](landing_month) for its account and `today`, both read in
/// one read transaction, so the month always matches the figures.
pub fn opening_overview(reader: &Reader, query: &OverviewQuery) -> Result<Overview, Error> {
    opening_overview_cancellable(reader, query, &Cancel::never())
}

/// [`opening_overview`], stopping with [`Error::Cancelled`] soon after
/// `cancel` is cancelled, as [`overview_cancellable`].
pub fn opening_overview_cancellable(
    reader: &Reader,
    query: &OverviewQuery,
    cancel: &Cancel,
) -> Result<Overview, Error> {
    let tx = Transaction::new_unchecked(reader.conn(), TransactionBehavior::Deferred)?;
    let start = setting::month_start(reader)?;
    let month = landing_month_in_transaction(reader, query.account, query.today, start, cancel)?;
    let overview =
        overview_in_transaction(reader, &OverviewQuery { month, ..*query }, start, cancel)?;
    drop(tx);
    Ok(overview)
}

/// [`overview`], inside a read transaction the caller holds.
fn overview_in_transaction(
    reader: &Reader,
    query: &OverviewQuery,
    start: MonthStart,
    cancel: &Cancel,
) -> Result<Overview, Error> {
    cancel.check()?;
    let selected = query.month;
    let previous = selected.previous().ok_or(Error::InvalidPeriod)?;
    let account = query
        .account
        .map(|id| balance::account(reader, id))
        .transpose()?;

    let window_end = window_end(selected, query.today, start);
    let window_start = window_start(window_end);
    let first = window_start.min(previous);
    let last = window_end.max(selected);
    let accounts: Vec<AccountId> = query.account.into_iter().collect();
    let lines = Ledger::new()
        .within(DateRange::months(first, last, start)?)
        .accounts(accounts.iter().copied())
        .lines(reader)?;
    cancel.check()?;

    let window = DateRange::months(window_start, window_end, start)?;
    let (currency, other_currencies) = display_currency(reader, &lines, window, account.as_ref())?;

    // Per month: income, and spending as the signed sum (below zero).
    let mut sums: BTreeMap<Period, (i64, i64)> = BTreeMap::new();
    for line in lines.iter().filter(|line| counts(line, Some(currency))) {
        let (income, spending) = sums
            .entry(Period::containing(line.date, start))
            .or_default();
        let counted = line.counted(Basis::Personal).minor();
        match line.side(Basis::Personal) {
            Side::Income => *income = add(*income, counted)?,
            Side::Spending => *spending = add(*spending, counted)?,
            Side::Neither => {}
        }
    }
    let figures = |period: Period| -> Result<MonthFigures, Error> {
        let (income, signed_spending) = sums.get(&period).copied().unwrap_or_default();
        let spent = Money::new(signed_spending, currency).checked_neg()?;
        let income = Money::new(income, currency);
        Ok(MonthFigures {
            period,
            range: period.range(start),
            income,
            spent,
            net: income.checked_sub(spent)?,
        })
    };
    let months = window_start
        .through(window_end)
        .map(figures)
        .collect::<Result<Vec<_>, _>>()?;
    let selected_figures = figures(selected)?;
    let previous_figures = figures(previous)?;

    let earliest_month = earliest_month(reader, &lines, first, &accounts, start)?;
    cancel.check()?;
    let has_transactions: bool =
        reader
            .conn()
            .query_row("SELECT EXISTS (SELECT 1 FROM \"transaction\")", [], |row| {
                row.get(0)
            })?;

    let palette = category_palette(reader, query.today)?;
    let categories = category_spending(
        reader,
        &lines,
        selected_figures.range,
        &palette,
        selected_figures.spent,
    )?;
    cancel.check()?;
    let (recurring, recurring_total) =
        upcoming_payments(reader, query, currency, &palette, cancel)?;

    Ok(Overview {
        month_start: start,
        currency,
        other_currencies,
        has_transactions,
        earliest_month,
        months,
        selected: selected_figures,
        previous: previous_figures,
        spent_change_percent: change_percent(selected_figures.spent, previous_figures.spent),
        income_change_percent: change_percent(selected_figures.income, previous_figures.income),
        net_change: selected_figures.net.checked_sub(previous_figures.net)?,
        categories,
        recurring,
        recurring_total,
    })
}

/// The window's last month (module docs).
fn window_end(selected: Period, today: NaiveDate, start: MonthStart) -> Period {
    let current = Period::containing(today, start);
    if selected < window_start(current) {
        selected
    } else {
        current
    }
}

/// The first month of the 12 ending at `end`; fewer only at the calendar's
/// start.
fn window_start(end: Period) -> Period {
    let mut start = end;
    for _ in 1..WINDOW_MONTHS {
        match start.previous() {
            Some(previous) => start = previous,
            None => break,
        }
    }
    start
}

/// Whether a line counts in the figures: money that counts, on a side, and
/// in `currency` when given.
fn counts(line: &LedgerLine, currency: Option<Currency>) -> bool {
    let counted = line.counted(Basis::Personal);
    !counted.is_zero()
        && line.side(Basis::Personal) != Side::Neither
        && currency.is_none_or(|currency| counted.currency() == currency)
}

fn add(left: i64, right: i64) -> Result<i64, Error> {
    left.checked_add(right)
        .ok_or(Error::Money(MoneyError::Overflow))
}

/// How many counted lines each currency has among `lines`.
fn counted_by_currency<'a>(
    lines: impl Iterator<Item = &'a LedgerLine>,
) -> BTreeMap<Currency, usize> {
    let mut counted = BTreeMap::new();
    for line in lines.filter(|line| counts(line, None)) {
        *counted
            .entry(line.counted(Basis::Personal).currency())
            .or_default() += 1;
    }
    counted
}

/// The currency with the most lines; the map's order (by code) breaks ties.
fn busiest_currency(counted: &BTreeMap<Currency, usize>) -> Option<Currency> {
    let mut busiest: Option<(Currency, usize)> = None;
    for (&currency, &lines) in counted {
        if busiest.is_none_or(|(_, most)| lines > most) {
            busiest = Some((currency, lines));
        }
    }
    busiest.map(|(currency, _)| currency)
}

/// The display currency and the other currencies counted in `window`.
fn display_currency(
    reader: &Reader,
    lines: &[LedgerLine],
    window: DateRange,
    account: Option<&Account>,
) -> Result<(Currency, Vec<Currency>), Error> {
    let counted = counted_by_currency(lines.iter().filter(|line| window.contains(line.date)));
    let busiest = busiest_currency(&counted);
    let currency = match (busiest, account) {
        (Some(currency), _) => currency,
        (None, Some(account)) => account.currency,
        (None, None) => balance::accounts(reader)?
            .first()
            .map_or(Currency::GBP, |account| account.currency),
    };
    let others = counted
        .keys()
        .copied()
        .filter(|&other| other != currency)
        .collect();
    Ok((currency, others))
}

/// The earliest month with a counted line: in `lines` (read from `first`
/// on) or before `first`.
fn earliest_month(
    reader: &Reader,
    lines: &[LedgerLine],
    first: Period,
    accounts: &[AccountId],
    start: MonthStart,
) -> Result<Option<Period>, Error> {
    let older = match first.previous() {
        Some(last) => Ledger::new()
            .within(DateRange::months(Period::MIN, last, start)?)
            .accounts(accounts.iter().copied())
            .lines(reader)?,
        None => Vec::new(),
    };
    Ok(older
        .iter()
        .chain(lines)
        .filter(|line| counts(line, None))
        .map(|line| Period::containing(line.date, start))
        .min())
}

/// The month the Overview opens on, chosen with its display currency.
///
/// Today's month, or the latest earlier month, that has a counted line on
/// the account filter in the busiest currency of its chart window (the
/// currency [`overview`] will display for it); the month of `today` when
/// there is no counted line at all.
///
/// Statements lag: an export made on the 21st may stop at the end of the
/// month before, and a screen that opens on a month of zeros says nothing.
/// The 12 months ending at `today`'s month are read first; older lines
/// only when those have none. A month older than today's window ends its
/// own chart window, so it is the latest month whose window's busiest
/// currency has a line in it.
pub fn landing_month(
    reader: &Reader,
    account: Option<AccountId>,
    today: NaiveDate,
) -> Result<Period, Error> {
    let tx = Transaction::new_unchecked(reader.conn(), TransactionBehavior::Deferred)?;
    let start = setting::month_start(reader)?;
    let month = landing_month_in_transaction(reader, account, today, start, &Cancel::never())?;
    drop(tx);
    Ok(month)
}

/// [`landing_month`], inside a read transaction the caller holds.
fn landing_month_in_transaction(
    reader: &Reader,
    account: Option<AccountId>,
    today: NaiveDate,
    start: MonthStart,
    cancel: &Cancel,
) -> Result<Period, Error> {
    cancel.check()?;
    let current = Period::containing(today, start);
    let window = window_start(current);
    let read = |range: DateRange| -> Result<Vec<LedgerLine>, Error> {
        Ok(Ledger::new()
            .within(range)
            .accounts(account)
            .lines(reader)?
            .into_iter()
            .filter(|line| counts(line, None))
            .collect())
    };
    let recent = read(DateRange::months(window, current, start)?)?;
    cancel.check()?;
    if !recent.is_empty() {
        return Ok(latest_in_busiest_currency(&recent, window, current, start).unwrap_or(current));
    }
    let Some(last) = window.previous() else {
        return Ok(current);
    };
    let older = read(DateRange::months(Period::MIN, last, start)?)?;
    cancel.check()?;
    let mut months: Vec<Period> = older
        .iter()
        .map(|line| Period::containing(line.date, start))
        .collect();
    months.sort_unstable();
    months.dedup();
    // A month before today's window ends its own chart window, so it lands
    // when its window's busiest currency has a line in it. The latest such
    // month; the earliest month with data always is one (its window has no
    // other month's lines).
    Ok(months
        .iter()
        .rev()
        .copied()
        .find(|&month| {
            latest_in_busiest_currency(&older, window_start(month), month, start) == Some(month)
        })
        .unwrap_or(current))
}

/// The latest month in `first..=last` with a line in the busiest currency
/// of the lines in those months. `lines` are all counted.
fn latest_in_busiest_currency(
    lines: &[LedgerLine],
    first: Period,
    last: Period,
    start: MonthStart,
) -> Option<Period> {
    let in_window = || {
        lines
            .iter()
            .filter(move |line| (first..=last).contains(&Period::containing(line.date, start)))
    };
    let currency = busiest_currency(&counted_by_currency(in_window()))?;
    in_window()
        .filter(|line| counts(line, Some(currency)))
        .map(|line| Period::containing(line.date, start))
        .max()
}

/// The selected month's spending by category (module docs), over its days
/// `range`; `total` is the month's spending, the base of the shares.
fn category_spending(
    reader: &Reader,
    lines: &[LedgerLine],
    range: DateRange,
    palette: &Palette,
    total: Money,
) -> Result<Vec<CategorySpend>, Error> {
    let tree = CategoryTree::load(reader.conn())?;
    let currency = total.currency();
    // Signed sums (money out below zero); present once a line is added.
    let mut named: HashMap<CategoryKey, i64> = HashMap::new();
    let mut other: Option<i64> = None;
    let mut uncategorised: Option<i64> = None;
    for line in lines.iter().filter(|line| {
        range.contains(line.date)
            && counts(line, Some(currency))
            && line.side(Basis::Personal) == Side::Spending
    }) {
        let minor = line.counted(Basis::Personal).minor();
        let sum = match &line.category {
            None => uncategorised.get_or_insert(0),
            Some(category) => {
                let group = tree
                    .group_of(&category.key)
                    .map(|group| group.category().key())
                    .filter(|group| palette.slot(group).is_some());
                match group {
                    Some(group) => named.entry(group.clone()).or_insert(0),
                    None => other.get_or_insert(0),
                }
            }
        };
        *sum = add(*sum, minor)?;
    }

    let spend = |signed: i64, group: SpendingGroup| -> Result<CategorySpend, Error> {
        let amount = Money::new(signed, currency).checked_neg()?;
        Ok(CategorySpend {
            group,
            amount,
            share_percent: share_percent(amount, total),
        })
    };
    let mut entries = Vec::new();
    for (key, signed) in named {
        let label = tree
            .get(&key)
            .map_or_else(|| key.as_str().to_owned(), |group| group.label().to_owned());
        let slot = palette.slot(&key).unwrap_or_default();
        entries.push(spend(signed, SpendingGroup::Named { key, label, slot })?);
    }
    entries.sort_by(|a, b| {
        b.amount
            .minor()
            .cmp(&a.amount.minor())
            .then_with(|| group_key(&a.group).cmp(group_key(&b.group)))
    });
    if let Some(signed) = other {
        entries.push(spend(signed, SpendingGroup::Other)?);
    }
    if let Some(signed) = uncategorised {
        entries.push(spend(signed, SpendingGroup::Uncategorised)?);
    }
    Ok(entries)
}

fn group_key(group: &SpendingGroup) -> &str {
    match group {
        SpendingGroup::Named { key, .. } => key.as_str(),
        SpendingGroup::Other | SpendingGroup::Uncategorised => "",
    }
}

/// The recurring payments due in the next 30 days and their sum.
fn upcoming_payments(
    reader: &Reader,
    query: &OverviewQuery,
    currency: Currency,
    palette: &Palette,
    cancel: &Cancel,
) -> Result<(Vec<UpcomingPayment>, Money), Error> {
    let until = query
        .today
        .checked_add_days(Days::new(RECURRING_DAYS))
        .unwrap_or(NaiveDate::MAX);
    let series =
        recurring::find_with_reader_cancellable(reader, cancel).map_err(|error| match error {
            recurring::Error::Cancelled(cancelled) => Error::Cancelled(cancelled),
            other => Error::Recurring(other),
        })?;
    let mut payments: Vec<UpcomingPayment> = series
        .into_iter()
        .filter(|series| {
            !series.lapsed
                && series.direction == Direction::Out
                && query
                    .account
                    .is_none_or(|account| series.account == account)
                && series.typical.currency() == currency
        })
        .filter_map(|series| {
            let next = next_due(&series, query.today)?;
            (next <= until).then(|| UpcomingPayment {
                slot: series.category.as_ref().and_then(|key| palette.slot(key)),
                category: series.category,
                merchant: series.merchant_display,
                cadence: series.cadence,
                next,
                amount: series.typical,
            })
        })
        .collect();
    payments.sort_by(|a, b| {
        a.next
            .cmp(&b.next)
            .then_with(|| a.merchant.cmp(&b.merchant))
    });
    let mut total = Money::zero(currency);
    for payment in &payments {
        total = total.checked_add(payment.amount)?;
    }
    Ok((payments, total))
}

/// How many occurrences [`next_due`] steps through at most: a weekly series
/// over a century.
const MAX_OCCURRENCES: u32 = 5_300;

/// The series' next date on or after `today`: its expected date, or, when
/// that has passed (the statements lag), its first occurrence along the
/// cadence on or after `today`, when it is at most [`RECURRING_DAYS`]
/// overdue (module docs). `None` when it is more, or past chrono's dates.
fn next_due(series: &recurring::RecurringSeries, today: NaiveDate) -> Option<NaiveDate> {
    if series.next_expected >= today {
        return Some(series.next_expected);
    }
    let overdue_from = today.checked_sub_days(Days::new(RECURRING_DAYS))?;
    if series.next_expected < overdue_from {
        return None;
    }
    (0..=MAX_OCCURRENCES)
        .map_while(|k| series.cadence.nth(series.anchor, k))
        .find(|&date| date >= today && date > series.next_expected)
}

/// `numerator / denominator` in whole percent, rounded half away from zero;
/// `None` unless the denominator is above zero.
fn ratio_percent(numerator: i128, denominator: i128) -> Option<i64> {
    if denominator <= 0 {
        return None;
    }
    let scaled = numerator * 100;
    let magnitude = (scaled.abs() * 2 + denominator) / (2 * denominator);
    let signed = if scaled < 0 { -magnitude } else { magnitude };
    // Only a tiny denominator against an enormous numerator leaves i64.
    Some(i64::try_from(signed).unwrap_or(if signed < 0 { i64::MIN } else { i64::MAX }))
}

/// How `current` compares with `previous`, in whole percent of `previous`
/// (module docs). A previous figure below zero (refunds outweighing
/// purchases) is taken by its size, so an increase is always positive.
fn change_percent(current: Money, previous: Money) -> Option<i64> {
    let difference = i128::from(current.minor()) - i128::from(previous.minor());
    ratio_percent(difference, i128::from(previous.minor()).abs())
}

/// `amount` as a share of `total`; 0 unless `total` is above zero.
fn share_percent(amount: Money, total: Money) -> i64 {
    if !total.is_positive() {
        return 0;
    }
    ratio_percent(i128::from(amount.minor()), i128::from(total.minor())).unwrap_or(0)
}

#[cfg(test)]
mod tests;
