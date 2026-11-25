//! The words the Overview screen shows, as plain functions of what core
//! read (`docs/DESIGN.md` → "Overview"): the month names, the changes
//! against last month, the notes under the figures and the pending
//! summary. The view draws with these and `describe()` reads them, so the
//! two can't disagree.

use chrono::NaiveDate;
use orre_core::inbox::{ORRE_SOURCE, SuggestionKind};
use orre_core::money::{Currency, Money};
use orre_core::recurring::Cadence;
use orre_core::reporting::Period;
use orre_core::reporting::overview::{Overview, SpendingGroup};
use orre_core::reporting::period::MonthStart;

use crate::format::{self, Sign};
use crate::inbox::Inbox;
use crate::inbox::text::{NAME_SHOWN, plain};
use crate::ui::marker::Marker;
use crate::ui::ranked_bars::slot_marker;

/// A day in `month`'s calendar month, to format its name from: a month is
/// named by its year and month, whatever days the user's month start gives it.
fn name_day(month: Period) -> NaiveDate {
    month.first_day(MonthStart::CALENDAR)
}

/// "October 2026": the switcher's title and the chart's tooltips.
pub fn month_title(month: Period) -> String {
    name_day(month).format("%B %Y").to_string()
}

/// "October": the spending section's caption.
pub fn month_name(month: Period) -> String {
    name_day(month).format("%B").to_string()
}

/// "Oct": a month's label on the chart and in the changes.
pub fn month_short(month: Period) -> String {
    name_day(month).format("%b").to_string()
}

/// The chart's caption: "Last 12 months" while its window ends at the
/// current month, else "12 months to Mar 2025".
pub fn chart_caption(last: Period, today: NaiveDate, start: MonthStart) -> String {
    if last == Period::containing(today, start) {
        "Last 12 months".to_owned()
    } else {
        format!("12 months to {}", name_day(last).format("%b %Y"))
    }
}

/// The caption under Spent and Income: "+8% vs Sep", "Same as Sep" when it
/// rounds to nothing, "−16% vs Sep", and "None in Sep" when `previous` had
/// none to compare with.
pub fn change_caption(percent: Option<i64>, previous: Period) -> String {
    let month = month_short(previous);
    match percent {
        None => format!("None in {month}"),
        Some(0) => format!("Same as {month}"),
        Some(percent) => {
            let sign = if percent > 0 { "+" } else { "\u{2212}" };
            format!("{sign}{}% vs {month}", percent.unsigned_abs())
        }
    }
}

/// The caption under Net: the signed difference from `previous`'s net,
/// "+£212.40 vs Sep", or "Same as Sep".
pub fn net_caption(change: Money, previous: Period) -> String {
    let month = month_short(previous);
    if change.is_zero() {
        format!("Same as {month}")
    } else {
        format!("{} vs {month}", format::money(change, Sign::Always))
    }
}

/// "Amounts in EUR aren't included", for the currencies the figures leave
/// out; `None` when there are none.
pub fn other_currencies_note(currencies: &[Currency]) -> Option<String> {
    let codes: Vec<&str> = currencies.iter().map(|currency| currency.code()).collect();
    let (last, rest) = codes.split_last()?;
    let list = if rest.is_empty() {
        (*last).to_owned()
    } else {
        format!("{} and {last}", rest.join(", "))
    };
    Some(format!("Amounts in {list} aren't included"))
}

/// A recurring payment's cadence, as the list shows it.
pub fn cadence_label(cadence: Cadence) -> &'static str {
    match cadence {
        Cadence::Weekly => "Weekly",
        Cadence::Fortnightly => "Fortnightly",
        Cadence::FourWeekly => "Every 4 weeks",
        Cadence::Monthly => "Monthly",
        Cadence::Quarterly => "Quarterly",
        Cadence::Yearly => "Yearly",
    }
}

/// The kinds the compact agent card counts, in the order it lists them.
pub const SUMMARY_KINDS: [SuggestionKind; 5] = [
    SuggestionKind::Categorization,
    SuggestionKind::Rule,
    SuggestionKind::TransferPair,
    SuggestionKind::MerchantAlias,
    SuggestionKind::RefundLink,
];

/// "9 categorisations", "1 transfer pair": `count` of `kind`.
pub fn kind_count(kind: SuggestionKind, count: usize) -> String {
    let (one, many) = match kind {
        SuggestionKind::Categorization => ("categorisation", "categorisations"),
        SuggestionKind::Rule => ("rule", "rules"),
        SuggestionKind::TransferPair => ("transfer pair", "transfer pairs"),
        SuggestionKind::MerchantAlias => ("merchant name", "merchant names"),
        SuggestionKind::RefundLink => ("refund link", "refund links"),
    };
    format!("{count} {}", if count == 1 { one } else { many })
}

/// One of the three figures: its label ("Spent"), the amount and the change
/// against last month under it.
#[derive(Clone, Debug, PartialEq)]
pub struct FigureText {
    pub label: &'static str,
    pub value: String,
    pub caption: String,
}

/// One month on the chart: the strings for its axis label and tooltip, and
/// the amounts in minor units that size its bars.
#[derive(Clone, Debug, PartialEq)]
pub struct ChartMonthText {
    pub label: String,
    pub title: String,
    pub spending: (i64, String),
    pub income: (i64, String),
    pub net: (i64, String),
    /// Before the earliest month with data: no net point on the line.
    pub before_data: bool,
}

/// One entry of "Spending by category".
#[derive(Clone, Debug, PartialEq)]
pub struct CategoryText {
    pub marker: Marker,
    pub label: String,
    pub share: String,
    pub amount: String,
    pub minor: i64,
}

/// One row of "Recurring".
#[derive(Clone, Debug, PartialEq)]
pub struct RecurringText {
    pub marker: Marker,
    pub merchant: String,
    pub cadence: &'static str,
    pub next: String,
    pub amount: String,
}

/// What the screen says for one read of core's overview (module docs): the
/// view draws it and `describe()` reads it.
#[derive(Clone, Debug, PartialEq)]
pub struct ScreenText {
    /// The switcher's month, "October 2026".
    pub title: String,
    /// Spent, Income and Net.
    pub figures: [FigureText; 3],
    pub other_currencies: Option<String>,
    pub chart_caption: String,
    pub months: Vec<ChartMonthText>,
    /// The index of the shown month on the chart, when it's in the window.
    pub selected_month: Option<usize>,
    /// The month's name, "October".
    pub category_caption: String,
    pub categories: Vec<CategoryText>,
    /// The recurring list's total.
    pub recurring_total: String,
    pub recurring: Vec<RecurringText>,
}

/// An amount the way a figure or a list shows it, never signed unless it's
/// below zero.
fn unsigned(amount: Money) -> String {
    format::money(amount, Sign::Negative)
}

/// What the screen says for `overview`, with `month` selected, as of
/// `today`.
pub fn screen_text(overview: &Overview, month: Period, today: NaiveDate) -> ScreenText {
    let previous = overview.previous.period;
    let figures = [
        FigureText {
            label: "Spent",
            value: unsigned(overview.selected.spent),
            caption: change_caption(overview.spent_change_percent, previous),
        },
        FigureText {
            label: "Income",
            value: unsigned(overview.selected.income),
            caption: change_caption(overview.income_change_percent, previous),
        },
        FigureText {
            label: "Net",
            value: format::money(overview.selected.net, Sign::Always),
            caption: net_caption(overview.net_change, previous),
        },
    ];
    let months: Vec<ChartMonthText> = overview
        .months
        .iter()
        .map(|figures| ChartMonthText {
            label: month_short(figures.period),
            title: month_title(figures.period),
            spending: (figures.spent.minor(), unsigned(figures.spent)),
            income: (figures.income.minor(), unsigned(figures.income)),
            net: (
                figures.net.minor(),
                format::money(figures.net, Sign::Always),
            ),
            before_data: overview
                .earliest_month
                .is_none_or(|earliest| figures.period < earliest),
        })
        .collect();
    let categories = overview
        .categories
        .iter()
        .map(|entry| {
            let (marker, label) = match &entry.group {
                SpendingGroup::Named { label, slot, .. } => {
                    (slot_marker(usize::from(*slot)), plain(label, NAME_SHOWN))
                }
                SpendingGroup::Other => (Marker::Other, "Other".to_owned()),
                SpendingGroup::Uncategorised => (Marker::None, "Uncategorised".to_owned()),
            };
            CategoryText {
                marker,
                label,
                share: format!("{}%", entry.share_percent),
                amount: unsigned(entry.amount),
                minor: entry.amount.minor(),
            }
        })
        .collect();
    let recurring = overview
        .recurring
        .iter()
        .map(|payment| RecurringText {
            // As on Transactions: a slot's colour, "Other" for a category
            // without one, the dashed marker for none.
            marker: match (payment.slot, &payment.category) {
                (Some(slot), _) => slot_marker(usize::from(slot)),
                (None, Some(_)) => Marker::Other,
                (None, None) => Marker::None,
            },
            merchant: plain(&payment.merchant, NAME_SHOWN),
            cadence: cadence_label(payment.cadence),
            next: format::list_date(payment.next, today),
            amount: format::money(payment.amount, Sign::Always),
        })
        .collect();
    ScreenText {
        title: month_title(month),
        figures,
        other_currencies: other_currencies_note(&overview.other_currencies),
        chart_caption: overview.months.last().map_or_else(String::new, |last| {
            chart_caption(last.period, today, overview.month_start)
        }),
        selected_month: overview
            .months
            .iter()
            .position(|figures| figures.period == month),
        months,
        category_caption: month_name(month),
        categories,
        // Payments only, so the total is shown unsigned, as Paper `2NY-0`.
        recurring_total: format::money(
            overview
                .recurring_total
                .checked_neg()
                .unwrap_or(overview.recurring_total),
            Sign::Negative,
        ),
        recurring,
    }
}

/// What the compact agent card says.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AgentCardText {
    /// The client with the most pending suggestions.
    pub agent: String,
    /// The Inbox tab's count: pending suggestions from AI clients.
    pub pending: String,
    /// The pending suggestions by kind, in [`SUMMARY_KINDS`]' order: "9
    /// categorisations", "2 rules".
    pub summary: Vec<String>,
}

/// The agent card's words while AI clients have suggestions pending; `None`
/// when there are none (the card isn't shown). Orre's own suggestions don't
/// show it, as they don't count on the Inbox tab; the kinds count every
/// pending suggestion, which the Inbox lists.
pub fn agent_card_text(inbox: &Inbox) -> Option<AgentCardText> {
    let pending = inbox.badge();
    if pending == 0 {
        return None;
    }
    let counts = &inbox.data().counts;
    let mut busiest: Option<(&String, usize)> = None;
    for (source, &count) in &counts.by_source {
        if source != ORRE_SOURCE && count > 0 && busiest.is_none_or(|(_, most)| count > most) {
            busiest = Some((source, count));
        }
    }
    let (agent, _) = busiest?;
    Some(AgentCardText {
        agent: plain(agent, NAME_SHOWN),
        pending: pending.to_string(),
        summary: SUMMARY_KINDS
            .into_iter()
            .filter_map(|kind| {
                let count = counts.pending_by_kind.get(&kind).copied()?;
                (count > 0).then(|| kind_count(kind, count))
            })
            .collect(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn month(year: i32, month: u32) -> Period {
        Period::new(year, month).unwrap()
    }

    fn gbp(minor: i64) -> Money {
        Money::new(minor, Currency::GBP)
    }

    #[test]
    fn names_a_month_three_ways() {
        let october = month(2026, 10);
        assert_eq!(month_title(october), "October 2026");
        assert_eq!(month_name(october), "October");
        assert_eq!(month_short(october), "Oct");
    }

    #[test]
    fn the_chart_says_where_its_window_ends() {
        let today = NaiveDate::from_ymd_opt(2026, 11, 14).unwrap();
        assert_eq!(
            chart_caption(month(2026, 11), today, MonthStart::CALENDAR),
            "Last 12 months"
        );
        assert_eq!(
            chart_caption(month(2025, 3), today, MonthStart::CALENDAR),
            "12 months to Mar 2025"
        );
    }

    #[test]
    fn a_change_is_signed_with_a_true_minus() {
        let sep = month(2026, 9);
        assert_eq!(change_caption(Some(8), sep), "+8% vs Sep");
        assert_eq!(change_caption(Some(-16), sep), "\u{2212}16% vs Sep");
        assert_eq!(change_caption(Some(0), sep), "Same as Sep");
        assert_eq!(change_caption(None, sep), "None in Sep");
    }

    #[test]
    fn net_changes_are_money() {
        let sep = month(2026, 9);
        assert_eq!(net_caption(gbp(21_240), sep), "+£212.40 vs Sep");
        assert_eq!(net_caption(gbp(-18_384), sep), "\u{2212}£183.84 vs Sep");
        assert_eq!(net_caption(gbp(0), sep), "Same as Sep");
    }

    #[test]
    fn other_currencies_are_listed() {
        let eur = Currency::EUR;
        let usd = Currency::new("USD").unwrap();
        let chf = Currency::new("CHF").unwrap();
        assert_eq!(other_currencies_note(&[]), None);
        assert_eq!(
            other_currencies_note(&[eur]).as_deref(),
            Some("Amounts in EUR aren't included")
        );
        assert_eq!(
            other_currencies_note(&[eur, usd]).as_deref(),
            Some("Amounts in EUR and USD aren't included")
        );
        assert_eq!(
            other_currencies_note(&[chf, eur, usd]).as_deref(),
            Some("Amounts in CHF, EUR and USD aren't included")
        );
    }

    #[test]
    fn kinds_are_counted_in_the_singular_and_plural() {
        assert_eq!(
            kind_count(SuggestionKind::Categorization, 9),
            "9 categorisations"
        );
        assert_eq!(
            kind_count(SuggestionKind::TransferPair, 1),
            "1 transfer pair"
        );
        assert_eq!(kind_count(SuggestionKind::Rule, 2), "2 rules");
    }
}
