use chrono::NaiveDate;

use crate::exclusion::ExclusionReason;
use crate::money::{Currency, Money};
use crate::reporting::DateRange;
use crate::reporting::test_ledger::{TestLedger, date};

use super::*;

const TODAY: &str = "2026-11-14";

fn load(ledger: &TestLedger) -> Register {
    Register::load(&ledger.reader(), date(TODAY)).unwrap()
}

fn category_id(register: &Register, key: &str) -> i64 {
    register
        .categories()
        .iter()
        .find(|category| category.key.as_str() == key)
        .unwrap()
        .id
}

/// The listed transactions' IDs, in order.
fn ids(register: &Register, listing: &Listing) -> Vec<TransactionId> {
    listing
        .rows
        .iter()
        .map(|&index| register.rows()[index].id)
        .collect()
}

#[test]
fn reads_one_row_per_transaction_newest_first() {
    let mut ledger = TestLedger::new();
    let current = ledger.account("Current account", Currency::GBP);
    let pot = ledger.pot(current, "Rainy day");
    let older = ledger.row_at(
        current,
        "2026-11-08",
        -1862,
        Some("food.groceries"),
        Some("Sainsbury's"),
        "SAINSBURYS S/MKT",
    );
    let split = ledger.row(current, "2026-11-09", -5216, None);
    ledger.split(
        split,
        &[(-4000, Some("food.groceries")), (-1216, Some("shopping"))],
    );
    let out = ledger.row(current, "2026-11-09", -25_000, None);
    let into = ledger.row(pot, "2026-11-09", 25_000, None);
    ledger.transfer(out, into);
    let bare = ledger.row_at(current, "2026-11-09", -420, None, None, "SQ *BLUE BOTTLE");

    let register = load(&ledger);
    let order: Vec<_> = register.rows().iter().map(|row| row.id).collect();
    assert_eq!(order, [bare, into, out, split, older]);

    let groceries = category_id(&register, "food.groceries");
    let row = register.row(older).unwrap();
    assert_eq!(row.merchant, "Sainsbury's");
    assert_eq!(row.description, "SAINSBURYS S/MKT");
    assert_eq!(row.category, RowCategory::Category(groceries));
    assert_eq!(row.amount, Money::new(-1862, Currency::GBP));

    // With no merchant, the cleaned description names the row.
    let bare_row = register.row(bare).unwrap();
    assert_eq!(bare_row.merchant, "BLUE BOTTLE");
    assert_eq!(bare_row.description, "SQ *BLUE BOTTLE");

    let RowCategory::Split(parts) = &register.row(split).unwrap().category else {
        panic!("a split row");
    };
    assert_eq!(parts.len(), 2);
    assert_eq!(parts[0].amount.minor(), -4000);
    assert_eq!(parts[0].category, Some(groceries));
    let labels: Vec<_> = register
        .row_categories(register.row(split).unwrap())
        .iter()
        .map(|category| category.label.clone())
        .collect();
    assert_eq!(labels, ["Groceries", "Shopping"]);

    // A confirmed transfer names the other side's account.
    assert_eq!(register.row(out).unwrap().transfer, Some(vec![pot]));
    assert_eq!(register.row(into).unwrap().transfer, Some(vec![current]));
    assert!(!register.row(out).unwrap().is_uncategorised());
    assert_eq!(register.account(pot).unwrap().kind, AccountKind::Pot);
}

#[test]
fn days_net_what_counts_and_hide_excluded_rows() {
    let mut ledger = TestLedger::new();
    let current = ledger.account("Current account", Currency::GBP);
    let savings = ledger.account("Savings", Currency::GBP);
    let coffee = ledger.row(current, "2026-11-09", -420, Some("food.eating_out"));
    let lunch = ledger.row(current, "2026-11-09", -645, Some("food.eating_out"));
    let out = ledger.row(current, "2026-11-09", -10_000, None);
    let into = ledger.row(savings, "2026-11-09", 10_000, None);
    ledger.transfer(out, into);
    let twice = ledger.row(current, "2026-11-09", -645, Some("food.eating_out"));
    ledger.exclude(twice, ExclusionReason::Duplicate);
    let earlier = ledger.row(current, "2026-11-07", 3400, Some("shopping"));

    let register = load(&ledger);
    let listing = register.select(&Filter::default(), Sort::Default);
    assert_eq!(
        ids(&register, &listing),
        [into, out, lunch, coffee, earlier]
    );
    assert_eq!(listing.days.len(), 2);
    let day = &listing.days[0];
    assert_eq!(day.date, date("2026-11-09"));
    assert_eq!(day.rows, 0..4);
    assert_eq!(day.net, [Money::new(-1065, Currency::GBP)]);
    assert_eq!(listing.days[1].net, [Money::new(3400, Currency::GBP)]);

    // The Excluded filter shows the row, still left out of the net.
    let filter = Filter {
        include_excluded: true,
        ..Filter::default()
    };
    let listing = register.select(&filter, Sort::Default);
    assert_eq!(listing.len(), 6);
    assert_eq!(listing.days[0].net, [Money::new(-1065, Currency::GBP)]);
}

#[test]
fn a_day_in_two_currencies_nets_each_apart() {
    let mut ledger = TestLedger::new();
    let pounds = ledger.account("Current account", Currency::GBP);
    let euros = ledger.account("Euro account", Currency::EUR);
    ledger.row(pounds, "2026-11-09", -500, None);
    ledger.row(euros, "2026-11-09", -1200, None);
    let register = load(&ledger);
    let listing = register.select(&Filter::default(), Sort::Default);
    assert_eq!(
        listing.days[0].net,
        [
            Money::new(-1200, Currency::EUR),
            Money::new(-500, Currency::GBP)
        ]
    );
}

#[test]
fn filters_by_account_category_dates_and_text() {
    let mut ledger = TestLedger::new();
    let current = ledger.account("Current account", Currency::GBP);
    let joint = ledger.account("Joint account", Currency::GBP);
    let groceries = ledger.row_at(
        current,
        "2026-11-09",
        -1862,
        Some("food.groceries"),
        Some("Sainsbury's"),
        "SAINSBURYS S/MKT CLAPHAM",
    );
    let eating_out = ledger.row_at(
        joint,
        "2026-11-08",
        -2745,
        Some("food.eating_out"),
        Some("Deliveroo"),
        "DELIVEROO.CO.UK",
    );
    let split = ledger.row(joint, "2026-10-30", -5216, None);
    ledger.split(split, &[(-4000, Some("shopping")), (-1216, Some("food"))]);
    let bills = ledger.row(current, "2026-10-01", -8640, Some("bills.energy"));

    let register = load(&ledger);
    let select = |filter: Filter| ids(&register, &register.select(&filter, Sort::Default));

    assert_eq!(
        select(Filter {
            account: Some(joint),
            ..Filter::default()
        }),
        [eating_out, split]
    );
    // A group includes its subcategories and the split's line in it.
    let food = category_id(&register, "food");
    assert_eq!(
        select(Filter {
            category: Some(food),
            ..Filter::default()
        }),
        [groceries, eating_out, split]
    );
    let shopping = category_id(&register, "shopping");
    assert_eq!(
        select(Filter {
            category: Some(shopping),
            ..Filter::default()
        }),
        [split]
    );
    assert_eq!(
        select(Filter {
            dates: Some(DateRange::new(date("2026-11-01"), date("2026-11-30")).unwrap()),
            ..Filter::default()
        }),
        [groceries, eating_out]
    );
    // Text matches the merchant and the raw description, ignoring case.
    assert_eq!(
        select(Filter {
            text: "  clapham ".into(),
            ..Filter::default()
        }),
        [groceries]
    );
    assert_eq!(
        select(Filter {
            text: "DELIVEROO".into(),
            account: Some(current),
            ..Filter::default()
        }),
        Vec::<TransactionId>::new()
    );
    assert_eq!(
        select(Filter::default()),
        [groceries, eating_out, split, bills]
    );
}

#[test]
fn filter_showing_clears_only_the_parts_that_hide_a_row() {
    let mut ledger = TestLedger::new();
    let current = ledger.account("Current account", Currency::GBP);
    let cash = ledger.account("Cash", Currency::GBP);
    let groceries = ledger.row_at(
        current,
        "2026-11-09",
        -1862,
        Some("food.groceries"),
        Some("Sainsbury's"),
        "SAINSBURYS S/MKT CLAPHAM",
    );
    let market = ledger.row_at(
        cash,
        "2026-10-20",
        -1280,
        None,
        Some("Farmers' market"),
        "Farmers' market",
    );
    let register = load(&ledger);
    let food = category_id(&register, "food");
    let november = DateRange::new(date("2026-11-01"), date("2026-11-30")).unwrap();
    let shown_in =
        |filter: &Filter, id| ids(&register, &register.select(filter, Sort::Default)).contains(&id);

    // Every part hides the market row: each is cleared.
    let hiding = Filter {
        account: Some(current),
        category: Some(food),
        dates: Some(november),
        uncategorised: false,
        text: "sainsbury".into(),
        include_excluded: false,
    };
    assert!(!shown_in(&hiding, market));
    let shown = register.filter_showing(&hiding, market).unwrap();
    assert_eq!(shown, Filter::default());
    assert!(shown_in(&shown, market));

    // Parts that already show a row stay.
    let showing = register.filter_showing(&hiding, groceries).unwrap();
    assert_eq!(showing, hiding);
    let uncategorised = Filter {
        uncategorised: true,
        account: Some(cash),
        ..Filter::default()
    };
    assert_eq!(
        register.filter_showing(&uncategorised, market).unwrap(),
        uncategorised
    );
    assert_eq!(
        register.filter_showing(&uncategorised, groceries).unwrap(),
        Filter::default()
    );
    assert_eq!(
        register.filter_showing(&Filter::default(), TransactionId::new(999_999)),
        None
    );

    // An excluded row turns the Excluded filter on.
    ledger.exclude(groceries, ExclusionReason::BankError);
    let register = load(&ledger);
    assert_eq!(
        register
            .filter_showing(&Filter::default(), groceries)
            .unwrap(),
        Filter {
            include_excluded: true,
            ..Filter::default()
        }
    );
}

#[test]
fn uncategorised_rows_are_counted_and_filtered() {
    let mut ledger = TestLedger::new();
    let current = ledger.account("Current account", Currency::GBP);
    let savings = ledger.account("Savings", Currency::GBP);
    let bare = ledger.row(current, "2026-11-09", -420, None);
    ledger.row(current, "2026-11-09", -645, Some("food.eating_out"));
    let half = ledger.row(current, "2026-11-08", -1000, None);
    ledger.split(half, &[(-600, Some("shopping")), (-400, None)]);
    let out = ledger.row(current, "2026-11-08", -5000, None);
    let into = ledger.row(savings, "2026-11-08", 5000, None);
    ledger.transfer(out, into);
    let other_account = ledger.row(savings, "2026-11-07", -100, None);

    let register = load(&ledger);
    let listing = register.select(&Filter::default(), Sort::Default);
    assert_eq!(listing.uncategorised, 3, "transfers don't need a category");

    let filter = Filter {
        uncategorised: true,
        ..Filter::default()
    };
    let listing = register.select(&filter, Sort::Default);
    assert_eq!(ids(&register, &listing), [bare, half, other_account]);
    assert_eq!(listing.uncategorised, 3);

    // The count follows the other filters.
    let filter = Filter {
        account: Some(current),
        ..Filter::default()
    };
    assert_eq!(register.select(&filter, Sort::Default).uncategorised, 2);
}

#[test]
fn category_counts_match_what_the_category_filter_keeps() {
    let mut ledger = TestLedger::new();
    let current = ledger.account("Current account", Currency::GBP);
    let savings = ledger.account("Savings", Currency::GBP);
    ledger.row(current, "2026-11-09", -420, Some("food.eating_out"));
    ledger.row(current, "2026-11-09", -645, Some("food.groceries"));
    ledger.row(savings, "2023-01-02", -900, Some("food"));
    // One split row in food twice and shopping once: once in each group.
    let split = ledger.row(current, "2026-11-08", -3000, None);
    ledger.split(
        split,
        &[
            (-1000, Some("food.groceries")),
            (-1000, Some("food.eating_out")),
            (-1000, Some("shopping.clothing")),
        ],
    );
    ledger.row(current, "2026-11-08", -500, Some("other"));
    ledger.row(current, "2026-11-07", -100, Some("other"));
    ledger.row(current, "2026-11-07", -100, Some("other"));
    ledger.row(current, "2026-11-07", -250, Some("transport.taxi"));
    let twice = ledger.row(current, "2026-11-07", -250, Some("transport.taxi"));
    ledger.exclude(twice, ExclusionReason::Duplicate);
    let skipped = ledger.row(current, "2026-11-06", -75, None);
    ledger.exclude(skipped, ExclusionReason::Duplicate);
    ledger.row(current, "2026-11-06", -75, None);
    let out = ledger.row(current, "2026-11-06", -5000, None);
    let into = ledger.row(savings, "2026-11-06", 5000, None);
    ledger.transfer(out, into);

    let register = load(&ledger);
    let counts = register.category_counts();
    let key = |id: i64| register.category(id).unwrap().key.as_str().to_owned();
    let listed: Vec<(String, usize)> = counts
        .groups
        .iter()
        .map(|count| (key(count.category), count.rows))
        .collect();
    // Most first, ties in menu order (transport before shopping), `other`
    // last whatever it holds; empty groups left out.
    assert_eq!(
        listed,
        [
            ("food".to_owned(), 4),
            ("transport".to_owned(), 1),
            ("shopping".to_owned(), 1),
            ("other".to_owned(), 3),
        ]
    );
    assert_eq!(counts.uncategorised, 1, "excluded rows and transfers left out");

    // Each count is exactly what picking the group shows.
    for count in &counts.groups {
        let filter = Filter {
            category: Some(count.category),
            ..Filter::default()
        };
        assert_eq!(register.select(&filter, Sort::Default).len(), count.rows);
    }
    let filter = Filter {
        uncategorised: true,
        ..Filter::default()
    };
    assert_eq!(
        register.select(&filter, Sort::Default).len(),
        counts.uncategorised
    );

    // Nothing imported: no groups, nothing uncategorised.
    let empty = load(&TestLedger::new()).category_counts();
    assert_eq!(empty, CategoryCounts::default());
}

#[test]
fn sorting_orders_rows_within_each_day() {
    let mut ledger = TestLedger::new();
    let current = ledger.account("Current account", Currency::GBP);
    let joint = ledger.account("Joint account", Currency::GBP);
    let pret = ledger.row_at(
        current,
        "2026-11-09",
        -645,
        None,
        Some("Pret A Manger"),
        "PRET",
    );
    let amazon = ledger.row_at(
        joint,
        "2026-11-09",
        -2399,
        Some("shopping"),
        Some("amazon"),
        "AMZN",
    );
    let tfl = ledger.row_at(
        current,
        "2026-11-09",
        -310,
        Some("transport"),
        Some("Transport for London"),
        "TFL",
    );
    let refund = ledger.row_at(joint, "2026-11-08", 3400, None, Some("ASOS"), "ASOS");
    let bills = ledger.row_at(
        current,
        "2026-11-08",
        -8640,
        Some("bills"),
        Some("Octopus"),
        "OCTOPUS",
    );

    let register = load(&ledger);
    let sorted = |sort: Sort| ids(&register, &register.select(&Filter::default(), sort));

    assert_eq!(sorted(Sort::Default), [tfl, amazon, pret, bills, refund]);
    assert_eq!(
        sorted(Sort::Ascending(SortColumn::Merchant)),
        [amazon, pret, tfl, refund, bills],
        "ignoring case, and never across days"
    );
    assert_eq!(
        sorted(Sort::Descending(SortColumn::Merchant)),
        [tfl, pret, amazon, bills, refund]
    );
    assert_eq!(
        sorted(Sort::Ascending(SortColumn::Amount)),
        [amazon, pret, tfl, bills, refund]
    );
    assert_eq!(
        sorted(Sort::Ascending(SortColumn::Account)),
        [tfl, pret, amazon, bills, refund]
    );
    // Uncategorised rows sort first.
    assert_eq!(
        sorted(Sort::Ascending(SortColumn::Category)),
        [pret, amazon, tfl, refund, bills]
    );
    let listing = register.select(&Filter::default(), Sort::Ascending(SortColumn::Amount));
    assert_eq!(listing.days.len(), 2);
    assert_eq!(listing.days[1].rows, 3..5);
}

#[test]
fn a_header_click_sorts_reverses_then_resets() {
    let column = SortColumn::Amount;
    let first = Sort::Default.next(column);
    assert_eq!(first, Sort::Ascending(column));
    assert_eq!(first.next(column), Sort::Descending(column));
    assert_eq!(first.next(column).next(column), Sort::Default);
    assert_eq!(
        Sort::Descending(SortColumn::Merchant).next(column),
        Sort::Ascending(column)
    );
    assert_eq!(first.column(), Some(column));
    assert_eq!(Sort::Default.column(), None);
}

#[test]
fn colours_go_to_the_six_groups_with_most_spending() {
    let mut ledger = TestLedger::new();
    let current = ledger.account("Current account", Currency::GBP);
    // Seven spending groups, most first; plus income, `other`, and spending
    // older than 12 months.
    let groups = [
        ("food.groceries", -9000),
        ("bills", -8000),
        ("transport.public", -7000),
        ("shopping", -6000),
        ("entertainment", -5000),
        ("health", -4000),
        ("travel", -3000),
    ];
    for (key, minor) in groups {
        ledger.row(current, "2026-10-01", minor, Some(key));
    }
    ledger.row(current, "2026-10-01", -100_000, Some("other"));
    ledger.row(current, "2026-10-01", 500_000, Some("income.salary"));
    ledger.row(current, "2025-11-14", -100_000, Some("giving"));
    ledger.row(current, "2025-11-15", -1000, Some("home"));

    let register = load(&ledger);
    let colour = |key: &str| {
        register
            .categories()
            .iter()
            .find(|category| category.key.as_str() == key)
            .unwrap()
            .colour
    };
    assert_eq!(colour("food"), Colour::Chart(1));
    assert_eq!(
        colour("food.eating_out"),
        Colour::Chart(1),
        "shared by the group"
    );
    assert_eq!(colour("bills.energy"), Colour::Chart(2));
    assert_eq!(colour("transport"), Colour::Chart(3));
    assert_eq!(colour("shopping"), Colour::Chart(4));
    assert_eq!(colour("entertainment"), Colour::Chart(5));
    assert_eq!(colour("health"), Colour::Chart(6));
    assert_eq!(colour("travel"), Colour::Other, "only six colours");
    assert_eq!(colour("other"), Colour::Other);
    assert_eq!(colour("income"), Colour::Other);
    assert_eq!(colour("giving"), Colour::Other, "more than 12 months ago");
    assert_eq!(colour("home"), Colour::Other, "beaten by the six");

    // The menu order: each group, then its subcategories.
    let food = register
        .categories()
        .iter()
        .position(|category| category.key.as_str() == "food")
        .unwrap();
    assert_eq!(
        register.categories()[food + 1].key.as_str(),
        "food.groceries"
    );
    assert_eq!(
        register.categories()[food + 1].group,
        Some(register.categories()[food].id)
    );
}

#[test]
fn with_no_spending_every_category_is_other() {
    let ledger = TestLedger::new();
    let register = load(&ledger);
    assert!(!register.categories().is_empty());
    assert!(
        register
            .categories()
            .iter()
            .all(|category| category.colour == Colour::Other)
    );
    assert!(
        register
            .select(&Filter::default(), Sort::Default)
            .is_empty()
    );
}

#[test]
fn date_presets_cover_whole_periods() {
    let today = date(TODAY);
    let range = |preset: DatePreset| preset.range(today).map(|range| (range.from(), range.to()));
    let day = |text: &str| NaiveDate::parse_from_str(text, "%Y-%m-%d").unwrap();
    assert_eq!(
        range(DatePreset::ThisMonth),
        Some((day("2026-11-01"), day("2026-11-30")))
    );
    assert_eq!(
        range(DatePreset::LastMonth),
        Some((day("2026-10-01"), day("2026-10-31")))
    );
    assert_eq!(
        range(DatePreset::LastThreeMonths),
        Some((day("2026-09-01"), day("2026-11-30")))
    );
    assert_eq!(
        range(DatePreset::ThisYear),
        Some((day("2026-01-01"), day("2026-12-31")))
    );
    assert_eq!(range(DatePreset::AllTime), None);
    // January's last month is the previous December.
    assert_eq!(
        DatePreset::LastMonth
            .range(day("2027-01-15"))
            .map(|range| (range.from(), range.to())),
        Some((day("2026-12-01"), day("2026-12-31")))
    );
}

#[test]
fn rows_carry_tags_notes_the_users_name_and_whether_they_can_be_split() {
    let mut ledger = TestLedger::new();
    let current = ledger.account("Current account", Currency::GBP);
    let savings = ledger.account("Savings", Currency::GBP);
    let shop = ledger.row(current, "2026-11-07", -5216, None);
    ledger.tag(shop, "weekly shop", false);
    ledger.tag(shop, "Mum's birthday", true);
    ledger
        .db
        .conn()
        .execute(
            "UPDATE \"transaction\" SET note = 'Flowers + weekly shop', user_note = 'Card', \
             merchant_display = 'Waitrose', merchant_display_source = 'user' WHERE id = ?1",
            [shop],
        )
        .unwrap();
    let out = ledger.row(current, "2026-11-08", -25_000, None);
    let into = ledger.row(savings, "2026-11-08", 25_000, None);
    ledger.transfer(out, into);
    let purchase = ledger.row(current, "2026-11-01", -3400, Some("shopping"));
    let refund = ledger.row(current, "2026-11-06", 3400, None);
    ledger.refund(refund, purchase, 3400);

    let register = load(&ledger);
    let names: Vec<_> = register
        .tags()
        .iter()
        .map(|tag| tag.name.as_str())
        .collect();
    assert_eq!(names, ["Mum's birthday", "weekly shop"]);
    assert!(register.tags()[0].private);

    let row = register.row(shop).unwrap();
    let on_row: Vec<_> = row
        .tags
        .iter()
        .map(|id| register.tag(*id).unwrap().name.as_str())
        .collect();
    assert_eq!(on_row, ["Mum's birthday", "weekly shop"]);
    assert_eq!(row.bank_note.as_deref(), Some("Flowers + weekly shop"));
    assert_eq!(row.note.as_deref(), Some("Card"));
    assert!(row.merchant_set);
    assert_eq!(row.merchant, "Waitrose");
    assert!(row.splittable);

    // A transfer and the money in of a confirmed refund can't be split.
    assert!(!register.row(out).unwrap().splittable);
    assert!(!register.row(refund).unwrap().splittable);
    let purchase_row = register.row(purchase).unwrap();
    assert!(purchase_row.splittable);
    assert!(purchase_row.tags.is_empty());
    assert!(!purchase_row.merchant_set);
}

#[test]
fn rows_say_where_they_came_from_and_why_they_are_excluded() {
    use crate::exclusion;
    use crate::import::{self, Committed};
    use crate::statement::{SourceFormat, SourceLocation, Statement, StatementLine};

    let mut ledger = TestLedger::new();
    let current = ledger.account("Current account", Currency::GBP);
    let mut statement = Statement::new(SourceFormat::Csv, Currency::GBP);
    statement.push(
        SourceLocation::Line(2),
        Ok(StatementLine::new(
            date("2026-11-07"),
            Money::new(-3180, Currency::GBP),
            "ODEON LUXE LEICESTER SQ",
        )),
    );
    let preview = import::preview(&ledger.db, current, "test-november.csv", statement).unwrap();
    let Committed::Batch(summary) =
        import::commit(&mut ledger.db, &preview, preview.needs_confirmation()).unwrap()
    else {
        panic!("a batch");
    };
    let cash = ledger.row(current, "2026-11-08", -420, None);
    let imported: TransactionId = ledger
        .db
        .conn()
        .query_row(
            "SELECT id FROM \"transaction\" WHERE origin = 'imported'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    exclusion::exclude(
        &mut ledger.db,
        imported,
        ExclusionReason::BankError,
        Some("Charged twice"),
    )
    .unwrap();
    // The local days of the stored UTC times, as the register reads them.
    let local = |sql: &str, id: TransactionId| -> NaiveDate {
        let text: String = ledger
            .db
            .conn()
            .query_row(sql, [id], |row| row.get(0))
            .unwrap();
        date(&text)
    };
    let imported_on = local(
        "SELECT date(b.imported_at, 'localtime') FROM \"transaction\" t \
         JOIN import_batch b ON b.id = t.batch_id WHERE t.id = ?1",
        imported,
    );
    let excluded_on = local(
        "SELECT date(excluded_at, 'localtime') FROM \"transaction\" WHERE id = ?1",
        imported,
    );
    let created_on = local(
        "SELECT date(created_at, 'localtime') FROM \"transaction\" WHERE id = ?1",
        cash,
    );

    let register = load(&ledger);
    let row = register.row(imported).unwrap();
    assert_eq!(
        row.source,
        RowSource::Import {
            batch: summary.batch,
            file: "test-november.csv".to_owned(),
            imported_on,
            origin: Origin::Imported,
        }
    );
    assert_eq!(row.source.batch(), Some(summary.batch));
    assert!(!row.source.is_manual());
    assert_eq!(row.excluded, Some(ExclusionReason::BankError));
    assert_eq!(row.excluded_note.as_deref(), Some("Charged twice"));
    assert_eq!(row.excluded_on, Some(excluded_on));

    let row = register.row(cash).unwrap();
    assert_eq!(row.source, RowSource::Manual { created_on });
    assert!(row.source.is_manual());
    assert_eq!((row.excluded, row.excluded_on), (None, None));
    assert_eq!(row.excluded_note, None);
}

#[test]
fn shares_and_links_are_for_rows_outside_a_transfer() {
    let mut ledger = TestLedger::new();
    let current = ledger.account("Current account", Currency::GBP);
    let savings = ledger.account("Savings", Currency::GBP);
    let spent = ledger.row(current, "2026-11-02", -1200, Some("shopping"));
    let earned = ledger.row(current, "2026-11-02", 5000, Some("income"));
    let nothing = ledger.row(current, "2026-11-02", 0, None);
    let out = ledger.row(current, "2026-11-03", -25_000, None);
    let into = ledger.row(savings, "2026-11-03", 25_000, None);
    ledger.transfer(out, into);
    let maybe_out = ledger.row(current, "2026-11-04", -700, None);
    let maybe_in = ledger.row(savings, "2026-11-04", 700, None);
    ledger.suggested_transfer(maybe_out, maybe_in);

    let register = load(&ledger);
    let takes = |id| {
        let row = register.row(id).unwrap();
        (row.takes_share(), row.takes_links())
    };
    // Money out: both; money in: links; nothing moved: neither.
    assert_eq!(takes(spent), (true, true));
    assert_eq!(takes(earned), (false, true));
    assert_eq!(takes(nothing), (false, false));
    // A pair, confirmed or only suggested: neither.
    for id in [out, into, maybe_out, maybe_in] {
        assert_eq!(takes(id), (false, false), "{id}");
    }
}
