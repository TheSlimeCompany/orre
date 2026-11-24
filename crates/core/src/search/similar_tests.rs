//! Tests for [`similar`]: which rows are neighbours and in what order.

use rusqlite::params;

use super::{Neighbours, similar};
use crate::id::TransactionId;
use crate::import;
use crate::inbox::testing::{Fixture, day};
use crate::money::{Currency, Money};
use crate::statement::{SourceFormat, SourceLocation, Statement, StatementLine};

/// The IDs of the neighbours of `row`, best first.
fn ids(f: &Fixture, row: TransactionId, filter: Neighbours) -> Vec<TransactionId> {
    similar(f.conn(), row, filter, 50)
        .unwrap()
        .into_iter()
        .map(|neighbour| neighbour.transaction)
        .collect()
}

fn exclude(f: &Fixture, row: TransactionId, reason: &str) {
    f.conn()
        .execute(
            "UPDATE \"transaction\" SET excluded_reason = ?1, \
             excluded_at = '2026-11-20T00:00:00.000Z' WHERE id = ?2",
            params![reason, row],
        )
        .unwrap();
}

/// Marks imported `row` as superseded by `replacement`, as an import's
/// correction does. The source fields are read-only, so the guard is dropped
/// in this scratch database to give both rows one source ID.
fn supersede(f: &Fixture, row: TransactionId, replacement: TransactionId) {
    f.conn()
        .execute_batch("DROP TRIGGER transaction_source_fields_read_only")
        .unwrap();
    f.conn()
        .execute(
            "UPDATE \"transaction\" SET excluded_reason = 'superseded', superseded_by = ?2, \
             excluded_at = '2026-11-20T00:00:00.000Z', \
             external_id = (SELECT external_id FROM \"transaction\" WHERE id = ?2) \
             WHERE id = ?1",
            params![row, replacement],
        )
        .unwrap();
}

/// Imports one Bean Cafe row with source ID `source`.
fn import_row(f: &mut Fixture, day_of_month: u32, source: &str) -> TransactionId {
    let mut statement = Statement::new(SourceFormat::Ofx, Currency::GBP);
    let mut line = StatementLine::new(
        day(day_of_month),
        Money::new(-500, Currency::GBP),
        "CARD PAYMENT BEAN CAFE".to_owned(),
    );
    line.external_id = Some(source.to_owned());
    line.merchant = Some("Bean Cafe".to_owned());
    statement.push(SourceLocation::Entry(1), Ok(line));
    let preview = import::preview(&f.db, f.account, "statement.ofx", statement).unwrap();
    let confirm = preview.needs_confirmation();
    import::commit(&mut f.db, &preview, confirm).unwrap();
    f.conn()
        .query_row(
            "SELECT id FROM \"transaction\" WHERE external_id = ?1",
            [source],
            |row| row.get(0),
        )
        .unwrap()
}

fn set(f: &Fixture, row: TransactionId, column: &str, value: Option<&str>) {
    f.conn()
        .execute(
            &format!("UPDATE \"transaction\" SET {column} = ?1 WHERE id = ?2"),
            params![value, row],
        )
        .unwrap();
}

#[test]
fn neighbours_are_ranked_by_match_then_amount_closeness_then_newest() {
    let f = Fixture::new();
    let subject = f.row(day(1), -500, "Bean Cafe", None);
    // A longer text matches the phrase less well than an exact one.
    let long = f.row(
        day(9),
        -500,
        "Bean Cafe Central Station London Branch",
        None,
    );
    let far = f.row(day(8), -900, "Bean Cafe", None);
    let near_old = f.row(day(2), -510, "Bean Cafe", None);
    let near_new = f.row(day(5), -510, "Bean Cafe", None);
    let nearest = f.row(day(3), -499, "BEAN CAFE", None);

    assert_eq!(
        ids(&f, subject, Neighbours::Any),
        [nearest, near_new, near_old, far, long]
    );
    let two = similar(f.conn(), subject, Neighbours::Any, 2).unwrap();
    assert_eq!(
        two.iter().map(|n| n.transaction).collect::<Vec<_>>(),
        [nearest, near_new]
    );
}

#[test]
fn only_rows_of_the_same_sign_and_currency_count() {
    let mut f = Fixture::new();
    let subject = f.row(day(1), -500, "Bean Cafe", None);
    let same = f.row(day(2), -700, "Bean Cafe", None);
    f.row(day(3), 700, "Bean Cafe", None);
    f.row(day(3), 0, "Bean Cafe", None);
    let usd = f.account("Dollars", Currency::USD);
    f.row_on(usd, day(4), -500, "Bean Cafe", None);

    assert_eq!(ids(&f, subject, Neighbours::Any), [same]);

    let money_in = f.row(day(5), 300, "Bean Cafe", None);
    let refund = f.row(day(6), 250, "Bean Cafe", None);
    let found = ids(&f, money_in, Neighbours::Any);
    assert!(found.contains(&refund));
    assert!(found.iter().all(|id| *id != subject && *id != same));

    let zero = f.row(day(7), 0, "Bean Cafe", None);
    assert!(ids(&f, zero, Neighbours::Any).is_empty());
}

#[test]
fn the_row_itself_and_duplicates_and_superseded_rows_are_left_out() {
    let mut f = Fixture::new();
    let subject = f.row(day(1), -500, "Bean Cafe", None);
    let kept = f.row(day(2), -500, "Bean Cafe", None);
    let duplicate = f.row(day(3), -500, "Bean Cafe", None);
    let manual = f.row(day(5), -500, "Bean Cafe", None);
    exclude(&f, duplicate, "duplicate");
    exclude(&f, manual, "disputed");
    let old = import_row(&mut f, 6, "old");
    let new = import_row(&mut f, 7, "new");
    supersede(&f, old, new);

    let mut found = ids(&f, subject, Neighbours::Any);
    found.sort();
    assert_eq!(found, [kept, manual, new]);
}

#[test]
fn a_stem_under_three_characters_has_no_neighbours() {
    let f = Fixture::new();
    let short = f.row(day(1), -500, "Ab", None);
    f.row(day(2), -500, "Ab", None);
    let digits = f.row(day(3), -500, "7 Eleven", None);
    f.row(day(4), -500, "7 Eleven", None);

    assert!(ids(&f, short, Neighbours::Any).is_empty());
    assert!(ids(&f, digits, Neighbours::Any).is_empty());
    assert!(ids(&f, TransactionId::new(9_999), Neighbours::Any).is_empty());
}

#[test]
fn a_row_with_no_bank_merchant_is_matched_on_its_description() {
    let f = Fixture::new();
    let subject = f.row(day(1), -500, "x", None);
    set(&f, subject, "merchant", None);
    set(&f, subject, "description_raw", Some("Bean Cafe London"));
    let same_description = f.row(day(2), -500, "x", None);
    set(&f, same_description, "merchant", None);
    set(
        &f,
        same_description,
        "description_raw",
        Some("BEAN CAFE LONDON 12"),
    );
    // Its bank merchant has the text, its description doesn't: another column.
    let merchant_only = f.row(day(3), -500, "Bean Cafe", None);
    set(&f, merchant_only, "description_raw", Some("card payment"));

    assert_eq!(ids(&f, subject, Neighbours::Any), [same_description]);
    // And the other way round: a bank merchant is matched against merchants.
    let with_merchant = f.row(day(4), -500, "Bean Cafe", None);
    assert_eq!(ids(&f, with_merchant, Neighbours::Any), [merchant_only]);
}

#[test]
fn fts_syntax_in_bank_text_is_matched_literally() {
    let f = Fixture::new();
    let text = "ACME\" OR NEAR* \"X";
    let subject = f.row(day(1), -500, text, None);
    let same = f.row(day(2), -500, text, None);
    f.row(day(3), -500, "ACME", None);
    f.row(day(4), -500, "X", None);
    let colon = f.row(day(5), -500, "merchant : Bean (Cafe) -x ^y", None);
    let colon_same = f.row(day(6), -500, "merchant : Bean (Cafe) -x ^y", None);

    assert_eq!(ids(&f, subject, Neighbours::Any), [same]);
    assert_eq!(ids(&f, colon, Neighbours::Any), [colon_same]);
}

#[test]
fn categorised_only_leaves_out_uncategorised_and_split_rows() {
    let mut f = Fixture::new();
    let subject = f.row(day(1), -500, "Bean Cafe", None);
    let coffee = f.row(day(2), -500, "Bean Cafe", Some("food.eating_out"));
    f.row(day(3), -500, "Bean Cafe", None);
    let split = f.row(day(4), -500, "Bean Cafe", Some("food.eating_out"));
    f.split(split, &[-200, -300]);

    let found = similar(f.conn(), subject, Neighbours::Categorised, 10).unwrap();
    assert_eq!(found.len(), 1);
    assert_eq!(found[0].transaction, coffee);
    assert_eq!(
        found[0].category.as_ref().unwrap().as_str(),
        "food.eating_out"
    );
    assert_eq!(ids(&f, subject, Neighbours::Any).len(), 3);
}

#[test]
fn a_whitespace_run_in_the_merchant_doesnt_hide_identical_twins() {
    let f = Fixture::new();
    let first = f.row(day(1), -500, "AMAZON PRIME    LONDON", None);
    let second = f.row(day(2), -500, "AMAZON PRIME    LONDON", None);
    let single = f.row(day(3), -500, "AMAZON PRIME LONDON", None);
    // The words are ANDed, whatever order or spacing the text has.
    let reordered = f.row(day(4), -500, "LONDON  AMAZON\tPRIME", None);
    f.row(day(5), -500, "AMAZON LONDON", None);

    let mut found = ids(&f, first, Neighbours::Any);
    found.sort();
    assert_eq!(found, [second, single, reordered]);
    assert!(ids(&f, second, Neighbours::Any).contains(&first));
}

#[test]
fn only_the_matched_column_counts() {
    let f = Fixture::new();
    let subject = f.row(day(1), -500, "Bean Cafe", None);
    // Bean Cafe only in the display name.
    let display_only = f.row(day(2), -500, "Other One", None);
    set(&f, display_only, "merchant_display", Some("Bean Cafe"));
    // Bean Cafe only in the raw description.
    let description_only = f.row(day(3), -500, "Other Two", None);
    set(&f, description_only, "description_raw", Some("Bean Cafe"));
    let real = f.row(day(4), -500, "Bean Cafe", None);

    assert_eq!(ids(&f, subject, Neighbours::Any), [real]);
}

#[test]
fn confirmed_transfer_legs_and_refund_inflows_arent_categorised_neighbours() {
    let f = Fixture::new();
    let subject = f.row(day(1), -500, "Bean Cafe", None);
    let kept = f.row(day(2), -500, "Bean Cafe", Some("food.eating_out"));
    let leg = f.row(day(3), -500, "Bean Cafe", Some("food.eating_out"));
    f.transfer(
        leg,
        f.row_on(f.savings, day(3), 500, "Elsewhere", None),
        true,
    );
    let suggested_leg = f.row(day(4), -500, "Bean Cafe", Some("food.eating_out"));
    f.transfer(
        suggested_leg,
        f.row_on(f.savings, day(4), 500, "Elsewhere", None),
        false,
    );

    let found = ids(&f, subject, Neighbours::Categorised);
    assert!(found.contains(&kept) && found.contains(&suggested_leg));
    assert!(!found.contains(&leg));

    let money_in = f.row(day(5), 300, "Bean Cafe", None);
    let kept_in = f.row(day(6), 300, "Bean Cafe", Some("food.eating_out"));
    let refund = f.row(day(7), 300, "Bean Cafe", Some("food.eating_out"));
    f.refund(refund, f.row(day(1), -300, "Shop", None), 300, true);
    let suggested = f.row(day(8), 300, "Bean Cafe", Some("food.eating_out"));
    f.refund(suggested, f.row(day(1), -300, "Shop", None), 300, false);

    let found = ids(&f, money_in, Neighbours::Categorised);
    assert!(found.contains(&kept_in) && found.contains(&suggested));
    assert!(!found.contains(&refund));
}

#[test]
fn every_stem_word_must_be_a_word_of_the_match_short_ones_too() {
    let f = Fixture::new();
    // `EE` is too short for the index, so the query is `LIMITED` alone.
    let ee = f.row(day(1), -500, "EE LIMITED", None);
    let ee_again = f.row(day(2), -500, "ee  limited", None);
    f.row(day(3), -500, "THAMES WATER LIMITED", None);
    // `SMITH` is inside `SMITHSON`, but not a word of it.
    let smith = f.row(day(4), -700, "WH SMITH", None);
    let smith_again = f.row(day(5), -700, "WH SMITH STATION", None);
    f.row(day(6), -700, "WH SMITHSON", None);
    f.row(day(7), -700, "JOHN SMITH", None);

    assert_eq!(ids(&f, ee, Neighbours::Any), [ee_again]);
    assert_eq!(ids(&f, smith, Neighbours::Any), [smith_again]);
}
