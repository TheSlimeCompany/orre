use crate::exclusion::ExclusionReason;
use crate::money::Currency;

use super::super::summary::monthly_summary;
use super::super::test_ledger::{TestLedger, date};
use super::*;

fn period(text: &str) -> Period {
    text.parse().unwrap()
}

fn gbp(minor: i64) -> Money {
    Money::new(minor, Currency::GBP)
}

fn query(month: &str) -> OverviewQuery {
    OverviewQuery {
        month: period(month),
        account: None,
        today: date("2026-11-20"),
    }
}

fn read(ledger: &TestLedger, query: OverviewQuery) -> Overview {
    overview(&ledger.reader(), &query).unwrap()
}

fn keys(overview: &Overview) -> Vec<String> {
    overview
        .categories
        .iter()
        .map(|entry| match &entry.group {
            SpendingGroup::Named { key, .. } => key.as_str().to_owned(),
            SpendingGroup::Other => "(other)".to_owned(),
            SpendingGroup::Uncategorised => "(uncategorised)".to_owned(),
        })
        .collect()
}

#[test]
fn the_window_is_twelve_months_to_today_when_the_selection_is_inside_it() {
    let mut ledger = TestLedger::new();
    let account = ledger.account("Current", Currency::GBP);
    ledger.row(account, "2026-10-02", -1_000, None);
    for selected in ["2026-11", "2026-10", "2025-12"] {
        let read = read(&ledger, query(selected));
        let periods: Vec<String> = read.months.iter().map(|m| m.period.key()).collect();
        assert_eq!(periods.len(), 12, "{selected}");
        assert_eq!(periods.first().unwrap(), "2025-12", "{selected}");
        assert_eq!(periods.last().unwrap(), "2026-11", "{selected}");
        assert_eq!(read.selected.period, period(selected));
    }
    let read = read(&ledger, query("2026-10"));
    assert_eq!(read.months[10].spent, gbp(1_000));
}

#[test]
fn the_window_ends_at_the_selection_when_it_is_before_the_window() {
    let mut ledger = TestLedger::new();
    let account = ledger.account("Current", Currency::GBP);
    ledger.row(account, "2025-06-02", -2_000, None);
    ledger.row(account, "2025-05-02", -1_500, None);
    let read = read(&ledger, query("2025-06"));
    assert_eq!(read.months.first().unwrap().period, period("2024-07"));
    assert_eq!(read.months.last().unwrap().period, period("2025-06"));
    assert_eq!(read.months.len(), 12);
    assert_eq!(read.selected.spent, gbp(2_000));
    // The first month of that window is itself before today's window.
    let read = self::read(&ledger, query("2024-07"));
    assert_eq!(read.months.first().unwrap().period, period("2023-08"));
    assert_eq!(read.months.last().unwrap().period, period("2024-07"));
    assert_eq!(read.previous.period, period("2024-06"));
}

#[test]
fn the_previous_month_may_be_outside_the_window() {
    let mut ledger = TestLedger::new();
    let account = ledger.account("Current", Currency::GBP);
    ledger.row(account, "2025-11-05", -4_000, None);
    ledger.row(account, "2025-12-05", -1_000, None);
    let read = read(&ledger, query("2025-12"));
    assert_eq!(read.months.first().unwrap().period, period("2025-12"));
    assert_eq!(read.previous.period, period("2025-11"));
    assert_eq!(read.previous.spent, gbp(4_000));
    assert_eq!(read.spent_change_percent, Some(-75));
}

#[test]
fn a_month_after_today_keeps_the_window() {
    let mut ledger = TestLedger::new();
    let account = ledger.account("Current", Currency::GBP);
    ledger.row(account, "2026-12-05", -1_000, None);
    let read = read(&ledger, query("2026-12"));
    assert_eq!(read.months.last().unwrap().period, period("2026-11"));
    assert_eq!(read.selected.spent, gbp(1_000));
}

#[test]
fn changes_are_whole_percents_rounded_half_away_from_zero() {
    assert_eq!(ratio_percent(105, 200), Some(53));
    assert_eq!(ratio_percent(-105, 200), Some(-53));
    assert_eq!(ratio_percent(1, 3), Some(33));
    assert_eq!(ratio_percent(2, 3), Some(67));
    assert_eq!(ratio_percent(-2, 3), Some(-67));
    assert_eq!(ratio_percent(1, 200), Some(1));
    assert_eq!(ratio_percent(-1, 200), Some(-1));
    assert_eq!(ratio_percent(0, 5), Some(0));
    assert_eq!(ratio_percent(1, 0), None);

    let mut ledger = TestLedger::new();
    let account = ledger.account("Current", Currency::GBP);
    // September 200 spent, October 305: +52.5% rounds to +53%.
    ledger.row(account, "2026-09-10", -20_000, None);
    ledger.row(account, "2026-10-10", -30_500, None);
    // Income 400 then 190: -52.5% rounds to -53%.
    ledger.row(account, "2026-09-11", 40_000, None);
    ledger.row(account, "2026-10-11", 19_000, None);
    let read = read(&ledger, query("2026-10"));
    assert_eq!(read.spent_change_percent, Some(53));
    assert_eq!(read.income_change_percent, Some(-53));
}

#[test]
fn a_previous_month_of_zero_has_no_percentage() {
    let mut ledger = TestLedger::new();
    let account = ledger.account("Current", Currency::GBP);
    ledger.row(account, "2026-10-10", -3_000, None);
    ledger.row(account, "2026-10-11", 5_000, None);
    let read = read(&ledger, query("2026-10"));
    assert_eq!(read.spent_change_percent, None);
    assert_eq!(read.income_change_percent, None);
    assert_eq!(read.net_change, gbp(2_000));
}

#[test]
fn a_previous_month_below_zero_is_compared_by_its_size() {
    let mut ledger = TestLedger::new();
    let account = ledger.account("Current", Currency::GBP);
    // September: a 100 purchase refunded in full plus a 50 refund of one
    // from August: spent -50 for the month.
    let august = ledger.row(account, "2026-08-10", -5_000, Some("shopping"));
    let refund = ledger.row(account, "2026-09-10", 5_000, Some("shopping"));
    ledger.refund(refund, august, 5_000);
    ledger.row(account, "2026-10-10", -5_000, None);
    let read = read(&ledger, query("2026-10"));
    assert_eq!(read.previous.spent, gbp(-5_000));
    assert_eq!(read.selected.spent, gbp(5_000));
    assert_eq!(read.spent_change_percent, Some(200));
}

#[test]
fn net_change_is_a_signed_difference() {
    let mut ledger = TestLedger::new();
    let account = ledger.account("Current", Currency::GBP);
    // September net +300, October net -100.
    ledger.row(account, "2026-09-01", 50_000, Some("income.salary"));
    ledger.row(account, "2026-09-02", -20_000, None);
    ledger.row(account, "2026-10-01", 10_000, Some("income.salary"));
    ledger.row(account, "2026-10-02", -20_000, None);
    let read = read(&ledger, query("2026-10"));
    assert_eq!(read.previous.net, gbp(30_000));
    assert_eq!(read.selected.net, gbp(-10_000));
    assert_eq!(read.net_change, gbp(-40_000));
    let read = self::read(&ledger, query("2026-09"));
    assert_eq!(read.net_change, gbp(30_000));
}

#[test]
fn categories_are_named_groups_then_other_then_uncategorised() {
    let mut ledger = TestLedger::new();
    let account = ledger.account("Current", Currency::GBP);
    // Six ranked groups in the last year; the seventh and `other` have no
    // slot. Spending rank decides the slots: food first.
    for (category, minor) in [
        ("food.groceries", -9_000),
        ("bills", -8_000),
        ("transport.public", -7_000),
        ("shopping", -6_000),
        ("entertainment", -5_000),
        ("health", -4_000),
        ("travel", -3_000),
    ] {
        ledger.row(account, "2026-10-01", minor, Some(category));
    }
    ledger.row(account, "2026-10-02", -20_000, Some("other"));
    ledger.row(account, "2026-10-03", -1_000, None);
    // Refunds outweigh purchases in health this month; the ranking over the
    // year keeps it sixth, ahead of travel.
    ledger.row(account, "2026-11-01", -1_000, Some("health"));
    ledger.row(account, "2026-11-02", 1_500, Some("health"));
    ledger.row(account, "2026-11-03", -500, Some("bills"));
    ledger.row(account, "2026-11-05", -700, Some("other"));
    ledger.row(account, "2026-11-06", -300, None);

    let read = read(&ledger, query("2026-10"));
    // Travel has no slot (seventh); `other` neither: both are Other.
    assert_eq!(
        keys(&read),
        [
            "food",
            "bills",
            "transport",
            "shopping",
            "entertainment",
            "health",
            "(other)",
            "(uncategorised)"
        ]
    );
    let amounts: Vec<i64> = read.categories.iter().map(|e| e.amount.minor()).collect();
    assert_eq!(
        amounts,
        [9_000, 8_000, 7_000, 6_000, 5_000, 4_000, 23_000, 1_000]
    );
    let slots: Vec<u8> = read
        .categories
        .iter()
        .filter_map(|e| match &e.group {
            SpendingGroup::Named { slot, .. } => Some(*slot),
            _ => None,
        })
        .collect();
    assert_eq!(slots, [1, 2, 3, 4, 5, 6]);
    let SpendingGroup::Named { label, .. } = &read.categories[0].group else {
        panic!("food is named");
    };
    assert!(!label.is_empty());
    // 63,000 spent: 9,000 is 14.28%, 23,000 is 36.5% (rounds up).
    assert_eq!(read.selected.spent, gbp(63_000));
    assert_eq!(read.categories[0].share_percent, 14);
    assert_eq!(read.categories[6].share_percent, 37);

    // November: health is below zero, listed in amount order, with no
    // share above zero; Other and Uncategorised stay last.
    let read = self::read(&ledger, query("2026-11"));
    assert_eq!(read.selected.spent, gbp(1_000));
    assert_eq!(
        keys(&read),
        ["bills", "health", "(other)", "(uncategorised)"]
    );
    let health = &read.categories[1];
    assert_eq!(health.amount, gbp(-500));
    assert_eq!(health.share_percent, -50);
    assert_eq!(read.categories[2].amount, gbp(700));
}

#[test]
fn a_spending_total_at_or_below_zero_gives_shares_of_zero() {
    assert_eq!(share_percent(gbp(-2_000), gbp(-500)), 0);
    assert_eq!(share_percent(gbp(500), gbp(0)), 0);
    assert_eq!(share_percent(gbp(1), gbp(3)), 33);
}

#[test]
fn categories_leave_out_what_has_no_lines() {
    let mut ledger = TestLedger::new();
    let account = ledger.account("Current", Currency::GBP);
    ledger.row(account, "2026-10-01", -9_000, Some("food.groceries"));
    // Income and a disputed row are not spending.
    ledger.row(account, "2026-10-02", 50_000, Some("income.salary"));
    let disputed = ledger.row(account, "2026-10-03", -1_000, Some("bills"));
    ledger.exclude(disputed, ExclusionReason::Disputed);
    let read = read(&ledger, query("2026-10"));
    assert_eq!(keys(&read), ["food"]);
    assert_eq!(read.categories[0].share_percent, 100);
    let read = self::read(&ledger, query("2026-09"));
    assert!(read.categories.is_empty());
}

#[test]
fn the_account_filter_limits_every_figure() {
    let mut ledger = TestLedger::new();
    let current = ledger.account("Current", Currency::GBP);
    let savings = ledger.account("Savings", Currency::GBP);
    ledger.row(current, "2026-10-02", -1_000, Some("food.groceries"));
    ledger.row(savings, "2026-10-03", -4_000, Some("bills"));
    ledger.row(savings, "2026-09-03", -2_000, Some("bills"));
    ledger.row(savings, "2025-01-03", 100, None);

    let all = read(&ledger, query("2026-10"));
    assert_eq!(all.selected.spent, gbp(5_000));
    assert_eq!(all.earliest_month, Some(period("2025-01")));

    let mut one = query("2026-10");
    one.account = Some(current);
    let read = read(&ledger, one);
    assert_eq!(read.selected.spent, gbp(1_000));
    assert_eq!(read.previous.spent, gbp(0));
    assert_eq!(read.spent_change_percent, None);
    assert_eq!(keys(&read), ["food"]);
    assert_eq!(read.earliest_month, Some(period("2026-10")));

    one.account = Some(savings);
    let read = self::read(&ledger, one);
    assert_eq!(read.selected.spent, gbp(4_000));
    assert_eq!(read.spent_change_percent, Some(100));
    assert_eq!(read.earliest_month, Some(period("2025-01")));
}

#[test]
fn an_unknown_account_is_refused() {
    let ledger = TestLedger::new();
    let mut unknown = query("2026-10");
    unknown.account = Some(AccountId::new(99));
    assert!(matches!(
        overview(&ledger.reader(), &unknown),
        Err(Error::UnknownAccount(_))
    ));
}

#[test]
fn the_display_currency_is_the_one_with_most_lines_and_the_rest_are_listed() {
    let mut ledger = TestLedger::new();
    let sterling = ledger.account("Current", Currency::GBP);
    let euros = ledger.account("Euros", Currency::EUR);
    let dollars = ledger.account("Dollars", Currency::USD);
    ledger.row(sterling, "2026-10-01", -1_000, None);
    ledger.row(euros, "2026-10-01", -2_000, None);
    ledger.row(euros, "2026-10-02", -3_000, None);
    ledger.row(euros, "2026-09-02", 9_000, None);
    ledger.row(dollars, "2026-10-02", -4_000, None);
    // Before the window: doesn't count towards the choice.
    for day in ["2024-01-01", "2024-01-02", "2024-01-03", "2024-01-04"] {
        ledger.row(sterling, day, -100, None);
    }

    let read = read(&ledger, query("2026-10"));
    assert_eq!(read.currency, Currency::EUR);
    assert_eq!(read.other_currencies, [Currency::GBP, Currency::USD]);
    assert_eq!(read.selected.spent, Money::new(5_000, Currency::EUR));
    assert_eq!(read.selected.income, Money::zero(Currency::EUR));
    assert_eq!(read.previous.income, Money::new(9_000, Currency::EUR));
    assert_eq!(read.recurring_total, Money::zero(Currency::EUR));

    // A filter on one account shows its currency alone.
    let mut one = query("2026-10");
    one.account = Some(dollars);
    let read = self::read(&ledger, one);
    assert_eq!(read.currency, Currency::USD);
    assert!(read.other_currencies.is_empty());
}

#[test]
fn display_currency_ties_go_to_the_lower_code() {
    let mut ledger = TestLedger::new();
    let sterling = ledger.account("Current", Currency::GBP);
    let euros = ledger.account("Euros", Currency::EUR);
    ledger.row(sterling, "2026-10-01", -1_000, None);
    ledger.row(euros, "2026-10-01", -2_000, None);
    let read = read(&ledger, query("2026-10"));
    assert_eq!(read.currency, Currency::EUR);
    assert_eq!(read.other_currencies, [Currency::GBP]);
}

#[test]
fn without_counted_lines_the_currency_is_the_accounts_first_or_gbp() {
    let mut ledger = TestLedger::new();
    let read_empty = read(&ledger, query("2026-10"));
    assert_eq!(read_empty.currency, Currency::GBP);
    assert!(!read_empty.has_transactions);
    assert_eq!(read_empty.earliest_month, None);
    assert_eq!(read_empty.months.len(), 12);
    assert!(read_empty.categories.is_empty());
    assert!(read_empty.recurring.is_empty());

    let euros = ledger.account("Euros", Currency::EUR);
    let dollars = ledger.account("Dollars", Currency::USD);
    let read_accounts = read(&ledger, query("2026-10"));
    assert_eq!(read_accounts.currency, Currency::EUR);
    assert!(!read_accounts.has_transactions);

    let mut dollars_only = query("2026-10");
    dollars_only.account = Some(dollars);
    assert_eq!(read(&ledger, dollars_only).currency, Currency::USD);

    // A line that is zero or neither side doesn't count, but is a transaction.
    ledger.row(euros, "2026-10-05", -100, Some("transfer.savings"));
    let read = read(&ledger, query("2026-10"));
    assert!(read.has_transactions);
    assert_eq!(read.earliest_month, None);
}

#[test]
fn the_earliest_month_looks_before_the_window_too() {
    let mut ledger = TestLedger::new();
    let account = ledger.account("Current", Currency::GBP);
    ledger.row(account, "2023-04-30", -100, None);
    ledger.row(account, "2026-10-01", -100, None);
    let read = read(&ledger, query("2026-10"));
    assert_eq!(read.earliest_month, Some(period("2023-04")));
    assert!(read.has_transactions);
}

/// Four monthly payments to `merchant` on the 5th, the last in `last`.
fn subscription(
    ledger: &mut TestLedger,
    account: AccountId,
    merchant: &str,
    minor: i64,
    category: &str,
    days: [&str; 4],
) {
    for day in days {
        ledger.row_at(
            account,
            day,
            minor,
            Some(category),
            Some(merchant),
            merchant,
        );
    }
}

const MONTHLY_TO_NOVEMBER: [&str; 4] = ["2026-08-05", "2026-09-05", "2026-10-05", "2026-11-05"];

#[test]
fn recurring_looks_thirty_days_ahead_inclusive() {
    let mut ledger = TestLedger::new();
    let account = ledger.account("Current", Currency::GBP);
    // Next on 2026-12-05.
    subscription(
        &mut ledger,
        account,
        "Netflix",
        -1_299,
        "bills",
        MONTHLY_TO_NOVEMBER,
    );
    let series = recurring::find_with_reader(&ledger.reader()).unwrap();
    assert_eq!(series.len(), 1);
    assert_eq!(series[0].next_expected, date("2026-12-05"));

    let at = |today: &str| {
        let mut q = query("2026-11");
        q.today = date(today);
        read(&ledger, q)
    };
    // Today is the next date.
    let read = at("2026-12-05");
    assert_eq!(read.recurring.len(), 1);
    assert_eq!(read.recurring[0].merchant, "Netflix");
    assert_eq!(read.recurring[0].cadence, Cadence::Monthly);
    assert_eq!(read.recurring[0].next, date("2026-12-05"));
    assert_eq!(read.recurring[0].amount, gbp(-1_299));
    assert_eq!(read.recurring_total, gbp(-1_299));
    // Thirty days before it is the last day in.
    assert_eq!(at("2026-11-05").recurring.len(), 1);
    // Thirty-one days before it is out.
    assert!(at("2026-11-04").recurring.is_empty());
    assert_eq!(at("2026-11-04").recurring_total, gbp(0));
    // The day after it, with no import showing the payment, it is rolled
    // on to its next month.
    let late = at("2026-12-06");
    assert_eq!(late.recurring.len(), 1);
    assert_eq!(late.recurring[0].next, date("2027-01-05"));
    // A weekly series rolls on by weeks.
    let mut weekly = TestLedger::new();
    let account = weekly.account("Current", Currency::GBP);
    subscription(
        &mut weekly,
        account,
        "Veg box",
        -2_000,
        "food",
        ["2026-11-02", "2026-11-09", "2026-11-16", "2026-11-23"],
    );
    let mut q = query("2026-11");
    q.today = date("2026-12-10");
    let rolled = super::overview(&weekly.reader(), &q).unwrap();
    assert_eq!(rolled.recurring.len(), 1);
    assert_eq!(rolled.recurring[0].next, date("2026-12-14"));
    // Thirty days overdue still rolls on; thirty-one is an account no
    // longer imported, whose series isn't listed.
    // (5 Dec to 4 Jan is 30 days; to 5 Jan, 31.)
    assert_eq!(at("2027-01-04").recurring[0].next, date("2027-01-05"));
    assert!(at("2027-01-05").recurring.is_empty());
    assert!(at("2028-11-24").recurring.is_empty());
}

#[test]
fn recurring_is_ordered_filtered_and_summed() {
    let mut ledger = TestLedger::new();
    let current = ledger.account("Current", Currency::GBP);
    let other = ledger.account("Other", Currency::GBP);
    let euros = ledger.account("Euros", Currency::EUR);
    subscription(
        &mut ledger,
        current,
        "Spotify",
        -1_000,
        "bills",
        MONTHLY_TO_NOVEMBER,
    );
    subscription(
        &mut ledger,
        current,
        "Netflix",
        -1_299,
        "bills",
        MONTHLY_TO_NOVEMBER,
    );
    subscription(
        &mut ledger,
        current,
        "Gym",
        -3_000,
        "health",
        ["2026-08-02", "2026-09-02", "2026-10-02", "2026-11-02"],
    );
    subscription(
        &mut ledger,
        other,
        "Cloud",
        -500,
        "bills",
        MONTHLY_TO_NOVEMBER,
    );
    subscription(
        &mut ledger,
        euros,
        "Zeitung",
        -700,
        "bills",
        MONTHLY_TO_NOVEMBER,
    );
    // Income in the same series machinery: salary on the 28th.
    subscription(
        &mut ledger,
        current,
        "Employer",
        200_000,
        "income.salary",
        ["2026-08-28", "2026-09-28", "2026-10-28", "2026-11-28"],
    );
    // A lapsed series: stopped in the summer, the account carries on.
    subscription(
        &mut ledger,
        current,
        "Old Club",
        -900,
        "bills",
        ["2026-02-05", "2026-03-05", "2026-04-05", "2026-05-05"],
    );
    let mut q = query("2026-11");
    q.today = date("2026-11-30");
    let all = read(&ledger, q);
    // GBP wins on lines. Gym 12-02, Netflix and Spotify 12-05 (by name),
    // Cloud 12-05; the salary (money in), the euro series and the lapsed one
    // are out.
    let merchants: Vec<&str> = all.recurring.iter().map(|p| p.merchant.as_str()).collect();
    assert_eq!(merchants, ["Gym", "Cloud", "Netflix", "Spotify"]);
    assert_eq!(all.recurring_total, gbp(-3_000 - 500 - 1_299 - 1_000));
    // Each payment carries its category's slot, as its category's entry
    // would.
    let palette = category_palette(&ledger.reader(), q.today).unwrap();
    for payment in &all.recurring {
        let key = payment.category.as_ref().expect("categorised");
        assert!(palette.slot(key).is_some(), "{}", key.as_str());
        assert_eq!(payment.slot, palette.slot(key), "{}", payment.merchant);
    }

    q.account = Some(current);
    let filtered = read(&ledger, q);
    let merchants: Vec<&str> = filtered
        .recurring
        .iter()
        .map(|p| p.merchant.as_str())
        .collect();
    assert_eq!(merchants, ["Gym", "Netflix", "Spotify"]);
}

#[test]
fn months_agree_with_monthly_summary_for_all_accounts() {
    let mut ledger = TestLedger::new();
    let current = ledger.account("Current", Currency::GBP);
    let savings = ledger.account("Savings", Currency::GBP);
    ledger.row(current, "2026-09-01", 200_000, Some("income.salary"));
    let shop = ledger.row(current, "2026-09-03", -5_000, None);
    ledger.split(shop, &[(-3_000, Some("food.groceries")), (-2_000, None)]);
    let shoes = ledger.row(current, "2026-09-28", -6_000, Some("shopping.clothing"));
    let back = ledger.row(current, "2026-10-04", 6_500, Some("income.gifts"));
    ledger.refund(back, shoes, 6_000);
    let dinner = ledger.row(current, "2026-10-06", -4_000, Some("food.eating_out"));
    ledger.share(dinner, -2_000);
    let laptop = ledger.row(current, "2026-10-07", -3_000, Some("shopping.electronics"));
    ledger.claim_back(laptop);
    let disputed = ledger.row(current, "2026-10-08", -1_000, Some("food.groceries"));
    ledger.exclude(disputed, ExclusionReason::Disputed);
    let out = ledger.row(current, "2026-10-09", -10_000, None);
    let into = ledger.row(savings, "2026-10-09", 10_000, None);
    ledger.transfer(out, into);
    ledger.row(current, "2026-10-10", -500, Some("transfer.savings"));
    ledger.row(current, "2026-10-11", 700, None);
    ledger.row(savings, "2026-11-12", -1_234, Some("food"));

    let read = read(&ledger, query("2026-10"));
    let summary = monthly_summary(&ledger.reader(), period("2025-12"), period("2026-11")).unwrap();
    assert_eq!(read.months.len(), summary.items.len());
    for (month, summary) in read.months.iter().zip(&summary.items) {
        assert_eq!(month.period, summary.period);
        let get = |totals: &Totals| totals.get(Currency::GBP).unwrap_or(gbp(0));
        assert_eq!(month.income, get(&summary.income), "{}", month.period);
        assert_eq!(month.spent, get(&summary.spending), "{}", month.period);
        assert_eq!(month.net, get(&summary.net), "{}", month.period);
    }
    assert!(read.months.iter().any(|m| m.spent.minor() > 0));
}

use super::super::Totals;

#[test]
fn the_landing_month_is_the_latest_month_with_a_counted_line() {
    let mut ledger = TestLedger::new();
    let current = ledger.account("Current", Currency::GBP);
    let card = ledger.account("Card", Currency::GBP);
    let today = date("2026-11-20");
    let landing =
        |ledger: &TestLedger, account| landing_month(&ledger.reader(), account, today).unwrap();
    // Nothing at all: today's month.
    assert_eq!(landing(&ledger, None), period("2026-11"));

    // Only an old month, before the 12 months to today.
    ledger.row(current, "2024-03-09", -1_000, None);
    assert_eq!(landing(&ledger, None), period("2024-03"));

    // A lagging statement: the latest month with data, not today's.
    ledger.row(current, "2026-10-13", -2_000, None);
    assert_eq!(landing(&ledger, None), period("2026-10"));
    // A line dated after today's month doesn't count.
    ledger.row(current, "2026-12-01", -500, None);
    assert_eq!(landing(&ledger, None), period("2026-10"));

    // Another account's later month counts for all accounts, not this one.
    ledger.row(card, "2026-11-02", -300, None);
    assert_eq!(landing(&ledger, None), period("2026-11"));
    assert_eq!(landing(&ledger, Some(current)), period("2026-10"));
    assert_eq!(landing(&ledger, Some(card)), period("2026-11"));
}

#[test]
fn the_landing_month_has_lines_in_the_displayed_currency() {
    let mut ledger = TestLedger::new();
    let sterling = ledger.account("Current", Currency::GBP);
    let euros = ledger.account("Euros", Currency::EUR);
    for day in ["2026-10-02", "2026-10-09", "2026-10-16"] {
        ledger.row(sterling, day, -1_000, None);
    }
    ledger.row(euros, "2026-11-03", -700, None);
    let today = date("2026-11-20");
    assert_eq!(
        landing_month(&ledger.reader(), None, today).unwrap(),
        period("2026-10")
    );

    let opened = opening_overview(&ledger.reader(), &query("2026-11")).unwrap();
    assert_eq!(opened.currency, Currency::GBP);
    assert_eq!(opened.selected.period, period("2026-10"));
    assert_eq!(opened.selected.spent, gbp(3_000));
}

#[test]
fn an_older_landing_month_is_in_the_busiest_currency_of_its_window() {
    let mut ledger = TestLedger::new();
    let sterling = ledger.account("Current", Currency::GBP);
    let euros = ledger.account("Euros", Currency::EUR);
    // The latest month has only a euro line; the 12 months to it are mostly
    // sterling, last in March.
    ledger.row(euros, "2024-06-05", -700, None);
    ledger.row(sterling, "2024-03-05", -1_000, None);
    ledger.row(sterling, "2024-02-05", -1_000, None);
    ledger.row(sterling, "2023-12-05", -1_000, None);
    let landed = landing_month(&ledger.reader(), None, date("2026-11-20")).unwrap();
    assert_eq!(landed, period("2024-03"));

    let opened = opening_overview(&ledger.reader(), &query("2026-11")).unwrap();
    assert_eq!(opened.selected.period, period("2024-03"));
    assert_eq!(opened.currency, Currency::GBP);
    assert_eq!(opened.selected.spent, gbp(1_000));
}

#[test]
fn an_older_landing_month_is_the_latest_whose_own_window_shows_it() {
    let mut ledger = TestLedger::new();
    let sterling = ledger.account("Current", Currency::GBP);
    let euros = ledger.account("Euros", Currency::EUR);
    // June 2024's window is mostly sterling, but May's (June 2023 to May
    // 2024) is mostly euros, and May has a euro line: May, not a month the
    // sterling majority skips to.
    ledger.row(euros, "2024-06-05", -100, None);
    ledger.row(euros, "2024-05-05", -200, None);
    for day in ["2024-04-03", "2024-04-04", "2024-04-05"] {
        ledger.row(sterling, day, -1_000, None);
    }
    for day in [
        "2023-06-01",
        "2023-06-02",
        "2023-06-03",
        "2023-06-04",
        "2023-06-05",
    ] {
        ledger.row(euros, day, -500, None);
    }
    let landed = landing_month(&ledger.reader(), None, date("2026-11-20")).unwrap();
    assert_eq!(landed, period("2024-05"));
    let opened = opening_overview(&ledger.reader(), &query("2026-11")).unwrap();
    assert_eq!(opened.selected.period, period("2024-05"));
    assert_eq!(opened.currency, Currency::EUR);
    assert!(!opened.selected.spent.is_zero());
}

#[test]
fn the_opening_overview_is_the_overview_at_the_landing_month() {
    let mut ledger = TestLedger::new();
    let account = ledger.account("Current", Currency::GBP);
    ledger.row(account, "2026-09-10", -2_000, None);
    ledger.row(account, "2026-10-10", -3_000, None);
    ledger.row(account, "2026-10-11", 5_000, None);
    let reader = ledger.reader();
    let today = date("2026-11-20");
    let landed = landing_month(&reader, None, today).unwrap();
    assert_eq!(landed, period("2026-10"));
    let opened = opening_overview(&reader, &query("2026-11")).unwrap();
    assert_eq!(opened, overview(&reader, &query("2026-10")).unwrap());
}

#[test]
fn months_follow_the_month_start() {
    use crate::reporting::{StartDay, Weekends};

    let mut ledger = TestLedger::new();
    let current = ledger.account("Current", Currency::GBP);
    ledger.row(current, "2026-11-24", -1_000, Some("food.groceries"));
    ledger.row(current, "2026-11-25", -2_000, Some("food.groceries"));
    let start = MonthStart::new(StartDay::day(25).unwrap(), Weekends::Keep);
    crate::setting::set_month_start(&mut ledger.db, start).unwrap();

    // Today, 26 Nov, is in December's month under the 25th.
    let today = date("2026-11-26");
    let reader = ledger.reader();
    assert_eq!(
        landing_month(&reader, None, today).unwrap(),
        period("2026-12")
    );
    let opened = opening_overview(
        &reader,
        &OverviewQuery {
            today,
            ..query("2026-01")
        },
    )
    .unwrap();
    assert_eq!(opened.month_start, start);
    assert_eq!(opened.selected.period, period("2026-12"));
    assert_eq!(
        (opened.selected.range.from(), opened.selected.range.to()),
        (date("2026-11-25"), date("2026-12-24"))
    );
    assert_eq!(opened.selected.spent, gbp(2_000));
    assert_eq!(opened.previous.period, period("2026-11"));
    assert_eq!(opened.previous.spent, gbp(1_000));
    assert_eq!(opened.months.last().unwrap().period, period("2026-12"));
    for month in &opened.months {
        assert_eq!(month.range, month.period.range(start));
    }
    assert_eq!(opened.earliest_month, Some(period("2026-11")));
}

/// One merchant's dense rows, as SLI-36's repro: 12 a day for 50 days to
/// today, about -£1 each, so the recurring search is most of the read.
fn dense_merchant() -> TestLedger {
    let mut ledger = TestLedger::new();
    let account = ledger.account("Current", Currency::GBP);
    let first = date("2026-10-02");
    for n in 0..600_i64 {
        let day = (first + chrono::Days::new((n % 50) as u64)).to_string();
        let minor = -(98 + n % 5);
        ledger.row_at(account, &day, minor, None, Some("Bench merchant"), "BENCH");
    }
    ledger
}

/// SLI-37: a cancelled read stops with `Cancelled`, having looked at
/// almost nothing (under 1% of the rows a whole read's recurring search
/// looks at); an uncancelled one is `overview`'s.
#[test]
fn a_cancelled_read_stops_and_an_uncancelled_one_is_the_overview() {
    let ledger = dense_merchant();
    let reader = ledger.reader();
    let query = query("2026-11");

    recurring::work::take();
    let whole = overview_cancellable(&reader, &query, &Cancel::new()).unwrap();
    let full = recurring::work::take();
    assert_eq!(whole, overview(&reader, &query).unwrap());
    assert_eq!(
        opening_overview_cancellable(&reader, &query, &Cancel::new()).unwrap(),
        opening_overview(&reader, &query).unwrap()
    );
    recurring::work::take();

    let cancelled = Cancel::new();
    cancelled.cancel();
    type Read = fn(&Reader, &OverviewQuery, &Cancel) -> Result<Overview, Error>;
    let reads: [(&str, Read); 2] = [
        ("overview", overview_cancellable),
        ("opening", opening_overview_cancellable),
    ];
    for (read, run) in reads {
        let outcome = run(&reader, &query, &cancelled);
        let stopped = recurring::work::take();
        assert!(
            matches!(outcome, Err(Error::Cancelled(_))),
            "{read}: {:?}",
            outcome.as_ref().map(|_| "an overview")
        );
        assert!(
            stopped * 100 < full,
            "{read}: rows examined: whole read {full}, cancelled {stopped}"
        );
    }
}
