//! The Overview's sample data (`docs/DESIGN.md` → "Overview", Paper
//! `22V-0`): what the screen shows with no data folder, in the snapshot
//! example and in tests that don't need a database.
//!
//! October 2026 spent £2,481.90: Groceries £612.40, Eating out £438.15,
//! Bills £402.60, Shopping £356.98, Transport £214.30, Entertainment
//! £98.67, Other £300.27 and Uncategorised £58.53. Income is £3,450 a month
//! (£4,280 in March); the month's spending runs from £1,980.10 (January) to
//! £3,120.75 (December). Each function answers a query as core's `overview`
//! would, so a month picked on the screen changes the figures: any other
//! month spends in proportion to October across the same categories, and
//! the account filter changes nothing.

use chrono::{Days, NaiveDate};
use orre_core::category::CategoryKey;
use orre_core::money::{Currency, Money};
use orre_core::recurring::Cadence;
use orre_core::reporting::Period;
use orre_core::reporting::overview::{
    CategorySpend, MonthFigures, Overview, OverviewQuery, SpendingGroup, UpcomingPayment,
    WINDOW_MONTHS,
};
use orre_core::reporting::period::MonthStart;

const GBP: Currency = Currency::GBP;

/// Spending by calendar month, January first, in pence.
const SPENDING: [i64; 12] = [
    198_010, 214_560, 267_340, 231_980, 245_320, 259_870, 302_410, 276_520, 229_806, 248_190,
    226_540, 312_075,
];

/// Income by calendar month: £3,450, and £4,280 in March.
const INCOME: [i64; 12] = [
    345_000, 345_000, 428_000, 345_000, 345_000, 345_000, 345_000, 345_000, 345_000, 345_000,
    345_000, 345_000,
];

/// October's categories, named ones first: the key, the label, the colour
/// slot and the pence.
const NAMED: [(&str, &str, u8, i64); 6] = [
    ("food", "Groceries", 1, 61_240),
    ("eating_out", "Eating out", 2, 43_815),
    ("bills", "Bills", 5, 40_260),
    ("shopping", "Shopping", 3, 35_698),
    ("transport", "Transport", 4, 21_430),
    ("entertainment", "Entertainment", 6, 9_867),
];
const OTHER: i64 = 30_027;
const UNCATEGORISED: i64 = 5_853;
const OCTOBER_SPENT: i64 = 248_190;

/// The month `period`'s spending and income in pence.
fn figures_in(period: Period) -> (i64, i64) {
    let at = (period.month() - 1) as usize;
    (SPENDING[at], INCOME[at])
}

fn gbp(minor: i64) -> Money {
    Money::new(minor, GBP)
}

fn month_figures(period: Period) -> MonthFigures {
    let (spent, income) = figures_in(period);
    MonthFigures {
        period,
        range: period.range(MonthStart::CALENDAR),
        income: gbp(income),
        spent: gbp(spent),
        net: gbp(income - spent),
    }
}

/// The 12 months ending at `end`.
fn window(end: Period) -> Vec<Period> {
    let mut start = end;
    for _ in 1..WINDOW_MONTHS {
        start = start.previous().expect("a month before");
    }
    start.through(end).collect()
}

/// `change` as a whole-number percentage of `base`, half away from zero.
fn percent(now: i64, base: i64) -> Option<i64> {
    (base != 0).then(|| {
        let difference = i128::from(now - base) * 100;
        let base = i128::from(base);
        let half = base / 2;
        let rounded = if difference >= 0 {
            (difference + half) / base
        } else {
            (difference - half) / base
        };
        rounded as i64
    })
}

/// The selected month's spending by category: October's mix scaled to the
/// month, the rounding left in Uncategorised.
fn categories(spent: i64) -> Vec<CategorySpend> {
    let scale = |minor: i64| minor * spent / OCTOBER_SPENT;
    let share = |amount: i64| {
        if spent > 0 {
            (amount * 100 + spent / 2) / spent
        } else {
            0
        }
    };
    let entry = |group, amount| CategorySpend {
        group,
        amount: gbp(amount),
        share_percent: share(amount),
    };
    let mut rest = spent;
    let mut entries = Vec::new();
    for (key, label, slot, minor) in NAMED {
        let amount = scale(minor);
        rest -= amount;
        entries.push(entry(
            SpendingGroup::Named {
                key: CategoryKey::new(key).expect("a category key"),
                label: label.to_owned(),
                slot,
            },
            amount,
        ));
    }
    let other = scale(OTHER);
    rest -= other;
    entries.push(entry(SpendingGroup::Other, other));
    entries.push(entry(SpendingGroup::Uncategorised, rest.max(0)));
    // Uncategorised is October's £58.53 in October itself.
    debug_assert!(spent != OCTOBER_SPENT || rest == UNCATEGORISED);
    entries
}

/// The payments due in the 30 days from `today`.
fn recurring(today: NaiveDate) -> Vec<UpcomingPayment> {
    let due =
        |days: u64, merchant: &str, cadence, minor: i64, category: &str, slot| UpcomingPayment {
            merchant: merchant.to_owned(),
            cadence,
            next: today.checked_add_days(Days::new(days)).expect("a date"),
            amount: gbp(minor),
            category: Some(CategoryKey::new(category).expect("a key")),
            slot,
        };
    vec![
        due(
            1,
            "Spotify",
            Cadence::Monthly,
            -1_199,
            "entertainment",
            Some(6),
        ),
        due(
            3,
            "Flat 2B rent",
            Cadence::Monthly,
            -125_000,
            "bills",
            Some(5),
        ),
        due(
            6,
            "Netflix",
            Cadence::Monthly,
            -1_299,
            "entertainment",
            Some(6),
        ),
        due(
            9,
            "Octopus Energy",
            Cadence::Monthly,
            -8_640,
            "bills",
            Some(5),
        ),
        due(
            21,
            "Home insurance",
            Cadence::Yearly,
            -31_200,
            "insurance",
            None,
        ),
    ]
}

fn build(query: &OverviewQuery, has_transactions: bool, other: Vec<Currency>) -> Overview {
    let selected = query.month;
    let previous = selected.previous().expect("a month before");
    let current = Period::containing(query.today, MonthStart::CALENDAR);
    let earliest = *window(current).first().expect("12 months");
    let end = if selected < earliest {
        selected
    } else {
        current
    };
    let shown = |period| {
        if has_transactions {
            month_figures(period)
        } else {
            MonthFigures {
                period,
                range: period.range(MonthStart::CALENDAR),
                income: gbp(0),
                spent: gbp(0),
                net: gbp(0),
            }
        }
    };
    let (selected_figures, previous_figures) = (shown(selected), shown(previous));
    let upcoming = if has_transactions {
        recurring(query.today)
    } else {
        Vec::new()
    };
    let total = upcoming.iter().map(|payment| payment.amount.minor()).sum();
    Overview {
        month_start: MonthStart::CALENDAR,
        currency: GBP,
        other_currencies: other,
        has_transactions,
        earliest_month: has_transactions.then_some(earliest),
        months: window(end).into_iter().map(shown).collect(),
        selected: selected_figures,
        previous: previous_figures,
        spent_change_percent: percent(
            selected_figures.spent.minor(),
            previous_figures.spent.minor(),
        ),
        income_change_percent: percent(
            selected_figures.income.minor(),
            previous_figures.income.minor(),
        ),
        net_change: gbp(selected_figures.net.minor() - previous_figures.net.minor()),
        categories: if has_transactions {
            categories(selected_figures.spent.minor())
        } else {
            Vec::new()
        },
        recurring: upcoming,
        recurring_total: gbp(total),
    }
}

/// The overview of the sample data for `query`.
pub fn sample(query: &OverviewQuery) -> Overview {
    build(query, true, Vec::new())
}

/// The overview of a database with no transactions.
pub fn empty(query: &OverviewQuery) -> Overview {
    build(query, false, Vec::new())
}

/// The sample data with euro lines the figures leave out.
pub fn with_euros(query: &OverviewQuery) -> Overview {
    build(query, true, vec![Currency::EUR])
}

/// The day the sample data is drawn on, and October 2026 its selected month.
pub fn today() -> NaiveDate {
    crate::accounts::fixture::today()
}

pub fn october() -> Period {
    Period::new(2026, 10).expect("a month")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn query(month: Period) -> OverviewQuery {
        OverviewQuery {
            month,
            account: None,
            today: today(),
        }
    }

    #[test]
    fn october_is_the_designs_month() {
        let overview = sample(&query(october()));
        assert_eq!(overview.selected.spent, gbp(248_190));
        assert_eq!(overview.selected.income, gbp(345_000));
        let amounts: Vec<i64> = overview
            .categories
            .iter()
            .map(|entry| entry.amount.minor())
            .collect();
        assert_eq!(
            amounts,
            [61_240, 43_815, 40_260, 35_698, 21_430, 9_867, 30_027, 5_853]
        );
        assert_eq!(overview.spent_change_percent, Some(8));
        assert_eq!(overview.income_change_percent, Some(0));
    }

    #[test]
    fn the_year_runs_from_the_lowest_january_to_the_highest_december() {
        let overview = sample(&query(october()));
        assert_eq!(overview.months.len(), WINDOW_MONTHS);
        let spent: Vec<i64> = overview.months.iter().map(|m| m.spent.minor()).collect();
        assert_eq!(spent.iter().min(), Some(&198_010));
        assert_eq!(spent.iter().max(), Some(&312_075));
        assert_eq!(overview.months[0].period, Period::new(2025, 12).unwrap());
        assert_eq!(overview.months[11].period, Period::new(2026, 11).unwrap());
    }

    #[test]
    fn every_month_adds_up() {
        for month in window(Period::new(2026, 11).unwrap()) {
            let overview = sample(&query(month));
            let total: i64 = overview
                .categories
                .iter()
                .map(|entry| entry.amount.minor())
                .sum();
            assert_eq!(total, overview.selected.spent.minor(), "{month}");
        }
    }
}
