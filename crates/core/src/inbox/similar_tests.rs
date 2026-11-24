//! Orre's own category suggestions after an import (ROADMAP FT.4).

use rusqlite::params;
use serde_json::Value;

use super::super::testing::{Fixture, day};
use super::super::{ORRE_SOURCE, reject};
use super::{MAX_PENDING_CATEGORY_SUGGESTIONS, suggest_for_batch};
use crate::id::{BatchId, TransactionId};
use crate::import::{self, Committed};
use crate::money::{Currency, Money};
use crate::statement::{SourceFormat, SourceLocation, Statement, StatementLine};

fn statement(rows: &[(u32, i64, &str, &str)]) -> Statement {
    let mut statement = Statement::new(SourceFormat::Ofx, Currency::GBP);
    for (index, &(d, minor, id, merchant)) in rows.iter().enumerate() {
        let mut line = StatementLine::new(
            day(d),
            Money::new(minor, Currency::GBP),
            format!("CARD PAYMENT {merchant}"),
        );
        line.external_id = Some(id.to_owned());
        line.merchant = Some(merchant.to_owned());
        statement.push(SourceLocation::Entry(index as u64 + 1), Ok(line));
    }
    statement
}

/// Imports `rows` (day, amount, source ID, bank merchant); `commit` files
/// the suggestions.
fn import(f: &mut Fixture, rows: &[(u32, i64, &str, &str)]) -> (BatchId, Vec<TransactionId>) {
    let preview = import::preview(&f.db, f.account, "statement.ofx", statement(rows)).unwrap();
    let confirm = preview.needs_confirmation();
    let Committed::Batch(summary) = import::commit(&mut f.db, &preview, confirm).unwrap() else {
        panic!("expected a batch");
    };
    let ids = rows.iter().map(|row| live(f, row.2)).collect();
    (summary.batch, ids)
}

fn live(f: &Fixture, external_id: &str) -> TransactionId {
    f.conn()
        .query_row(
            "SELECT id FROM \"transaction\" WHERE external_id = ?1",
            [external_id],
            |row| row.get(0),
        )
        .unwrap()
}

/// Every categorization suggestion: (source, status, rationale, items as
/// `(transaction_id, category)`), in filing order.
type Filed = (String, String, Option<String>, Vec<(i64, String)>);

fn filed(f: &Fixture) -> Vec<Filed> {
    let mut statement = f
        .conn()
        .prepare(
            "SELECT source_client, status, rationale, payload_json FROM suggestion \
             WHERE kind = 'categorization' ORDER BY id",
        )
        .unwrap();
    statement
        .query_map([], |row| {
            let json: String = row.get(3)?;
            let json: Value = serde_json::from_str(&json).unwrap();
            let items = json["items"]
                .as_array()
                .unwrap()
                .iter()
                .map(|item| {
                    (
                        item["transaction_id"].as_i64().unwrap(),
                        item["category"].as_str().unwrap().to_owned(),
                    )
                })
                .collect();
            Ok((row.get(0)?, row.get(1)?, row.get(2)?, items))
        })
        .unwrap()
        .collect::<Result<_, _>>()
        .unwrap()
}

/// Five categorised rows for `merchant`: `of` of them in `category`, the
/// rest in `other`.
fn history(f: &Fixture, merchant: &str, category: &str, of: usize, other: &str) {
    for n in 0..5 {
        let category = if n < of { category } else { other };
        f.row(day(1 + n as u32), -400 - n as i64, merchant, Some(category));
    }
}

const CAFE: &str = "Bean Cafe";

#[test]
fn three_of_five_neighbours_agreeing_file_one_suggestion_from_orre() {
    let mut f = Fixture::new();
    history(&f, CAFE, "food.eating_out", 3, "food.groceries");

    let (_, rows) = import(&mut f, &[(10, -450, "n1", CAFE)]);

    assert_eq!(
        filed(&f),
        [(
            ORRE_SOURCE.to_owned(),
            "pending".to_owned(),
            Some("Similar transactions have this category.".to_owned()),
            vec![(rows[0].get(), "food.eating_out".to_owned())],
        )]
    );
}

#[test]
fn two_of_five_agreeing_files_nothing() {
    let mut f = Fixture::new();
    for (n, category) in [
        "food.eating_out",
        "food.eating_out",
        "food.groceries",
        "food.groceries",
        "shopping.clothing",
    ]
    .into_iter()
    .enumerate()
    {
        f.row(day(1 + n as u32), -400 - n as i64, CAFE, Some(category));
    }

    import(&mut f, &[(10, -450, "n1", CAFE)]);

    assert!(filed(&f).is_empty());
}

#[test]
fn only_the_five_nearest_in_amount_vote() {
    let mut f = Fixture::new();
    // Three far rows in one category, the five nearest split 2 and 3.
    for n in 0..3 {
        f.row(
            day(1 + n),
            -9_000 - i64::from(n),
            CAFE,
            Some("shopping.clothing"),
        );
    }
    for n in 0..5u32 {
        let category = if n < 2 {
            "food.groceries"
        } else {
            "food.eating_out"
        };
        f.row(day(1 + n), -440 - i64::from(n), CAFE, Some(category));
    }

    import(&mut f, &[(10, -450, "n1", CAFE)]);

    let filed = filed(&f);
    assert_eq!(filed.len(), 1);
    assert_eq!(filed[0].3[0].1, "food.eating_out");
}

#[test]
fn rows_of_two_categories_file_two_suggestions() {
    let mut f = Fixture::new();
    history(&f, CAFE, "food.eating_out", 5, "food.eating_out");
    history(&f, "Green Grocer", "food.groceries", 5, "food.groceries");

    let (_, rows) = import(
        &mut f,
        &[
            (10, -450, "n1", CAFE),
            (11, -460, "n2", "Green Grocer"),
            (12, -470, "n3", CAFE),
        ],
    );

    let mut filed = filed(&f);
    filed.sort_by(|a, b| a.3.cmp(&b.3));
    let items: Vec<_> = filed.into_iter().map(|f| f.3).collect();
    assert_eq!(
        items,
        [
            vec![
                (rows[0].get(), "food.eating_out".to_owned()),
                (rows[2].get(), "food.eating_out".to_owned())
            ],
            vec![(rows[1].get(), "food.groceries".to_owned())],
        ]
    );
}

/// Rows that take no category of their own, or have one, get nothing.
#[test]
fn split_transfer_and_categorised_rows_get_nothing() {
    let mut f = Fixture::new();
    history(&f, CAFE, "food.eating_out", 5, "food.eating_out");
    let (batch, rows) = import(
        &mut f,
        &[
            (10, -450, "n1", CAFE),
            (11, -450, "n2", CAFE),
            (12, -450, "n3", CAFE),
            (13, -450, "n4", CAFE),
        ],
    );
    assert_eq!(filed(&f)[0].3.len(), 4);
    f.conn().execute("DELETE FROM suggestion", []).unwrap();

    f.split(rows[0], &[-200, -250]);
    f.transfer(rows[1], f.row(day(11), 450, "Other side", None), false);
    f.conn()
        .execute(
            "UPDATE \"transaction\" SET category_id = (SELECT id FROM category \
             WHERE key = 'food.groceries'), category_source = 'user' WHERE id = ?1",
            [rows[2]],
        )
        .unwrap();

    assert_eq!(suggest_for_batch(f.conn(), batch).unwrap(), 1);
    assert_eq!(
        filed(&f)[0].3,
        [(rows[3].get(), "food.eating_out".to_owned())]
    );
}

#[test]
fn a_row_named_in_a_decided_suggestion_isnt_suggested_again() {
    let mut f = Fixture::new();
    history(&f, CAFE, "food.eating_out", 5, "food.eating_out");
    let (batch, _) = import(&mut f, &[(10, -450, "n1", CAFE)]);
    let id: i64 = f
        .conn()
        .query_row("SELECT id FROM suggestion", [], |row| row.get(0))
        .unwrap();
    reject(&mut f.db, super::super::SuggestionId::new(id), None).unwrap();

    assert_eq!(suggest_for_batch(f.conn(), batch).unwrap(), 0);
    assert_eq!(filed(&f).len(), 1);
    assert_eq!(filed(&f)[0].1, "rejected");
}

#[test]
fn running_it_again_files_nothing_more() {
    let mut f = Fixture::new();
    history(&f, CAFE, "food.eating_out", 5, "food.eating_out");
    let (batch, _) = import(&mut f, &[(10, -450, "n1", CAFE), (11, -455, "n2", CAFE)]);
    assert_eq!(filed(&f).len(), 1);

    assert_eq!(suggest_for_batch(f.conn(), batch).unwrap(), 0);
    assert_eq!(filed(&f).len(), 1);
}

#[test]
fn a_suggestion_from_another_source_counts_as_naming_the_row() {
    let mut f = Fixture::new();
    history(&f, CAFE, "food.eating_out", 5, "food.eating_out");
    let (batch, rows) = import(&mut f, &[(10, -450, "n1", CAFE)]);
    f.conn().execute("DELETE FROM suggestion", []).unwrap();
    f.conn()
        .execute(
            "INSERT INTO suggestion (kind, payload_json, source_client, status) \
             VALUES ('categorization', ?1, 'claude-code', 'pending')",
            params![format!(
                r#"{{"items":[{{"transaction_id":{},"category":"food.groceries"}}]}}"#,
                rows[0]
            )],
        )
        .unwrap();
    f.conn()
        .execute(
            "UPDATE suggestion SET status = 'approved', decided_at = '2026-11-20T00:00:00.000Z'",
            [],
        )
        .unwrap();

    assert_eq!(suggest_for_batch(f.conn(), batch).unwrap(), 0);
}

#[test]
fn the_rationale_carries_no_bank_text() {
    let mut f = Fixture::new();
    history(&f, CAFE, "food.eating_out", 5, "food.eating_out");
    import(&mut f, &[(10, -450, "n1", CAFE)]);

    let rationale = filed(&f)[0].2.clone().unwrap();
    for text in ["Bean", "Cafe", "CARD PAYMENT", "n1"] {
        assert!(!rationale.contains(text), "{rationale}");
    }
}

#[test]
fn a_rehearsal_files_nothing() {
    let mut f = Fixture::new();
    history(&f, CAFE, "food.eating_out", 5, "food.eating_out");
    let preview = import::preview(
        &f.db,
        f.account,
        "statement.ofx",
        statement(&[(10, -450, "n1", CAFE)]),
    )
    .unwrap();

    import::rehearse(&f.db, &preview).unwrap();
    assert!(filed(&f).is_empty());

    import::commit(&mut f.db, &preview, preview.needs_confirmation()).unwrap();
    assert_eq!(filed(&f).len(), 1);
}

#[test]
fn rows_with_unmatched_text_get_nothing() {
    let mut f = Fixture::new();
    history(&f, CAFE, "food.eating_out", 5, "food.eating_out");

    import(&mut f, &[(10, -450, "n1", "Noodle Bar")]);

    assert!(filed(&f).is_empty());
}

#[test]
fn a_hundred_and_one_rows_of_one_category_file_two_suggestions() {
    let mut f = Fixture::new();
    history(&f, CAFE, "food.eating_out", 5, "food.eating_out");
    let rows: Vec<(u32, i64, String)> = (0..101)
        .map(|n| (1 + n % 28, -450 - i64::from(n), format!("n{n}")))
        .collect();
    let rows: Vec<(u32, i64, &str, &str)> = rows
        .iter()
        .map(|(d, minor, id)| (*d, *minor, id.as_str(), CAFE))
        .collect();

    let (_, ids) = import(&mut f, &rows);

    let filed = filed(&f);
    assert_eq!(
        filed.iter().map(|f| f.3.len()).collect::<Vec<_>>(),
        [100, 1]
    );
    let mut named: Vec<i64> = filed.iter().flat_map(|f| f.3.iter().map(|i| i.0)).collect();
    named.sort();
    let mut expected: Vec<i64> = ids.iter().map(|id| id.get()).collect();
    expected.sort();
    assert_eq!(named, expected);
}

/// `n` pending suggestions from Orre that name no transaction of ours.
fn pending_from_orre(f: &Fixture, n: usize) {
    for n in 0..n {
        let n = i64::try_from(n).unwrap();
        f.conn()
            .execute(
                "INSERT INTO suggestion (kind, payload_json, source_client) \
                 VALUES ('transfer_pair', json_object('outflow_id', ?1, 'inflow_id', ?2), ?3)",
                params![n * 2 + 100_001, n * 2 + 100_002, ORRE_SOURCE],
            )
            .unwrap();
    }
}

#[test]
fn filing_stops_at_the_category_suggestions_pending_cap() {
    let mut f = Fixture::new();
    history(&f, CAFE, "food.eating_out", 5, "food.eating_out");
    pending_from_orre(&f, MAX_PENDING_CATEGORY_SUGGESTIONS);

    import(&mut f, &[(10, -450, "n1", CAFE)]);
    assert!(filed(&f).is_empty());
}

#[test]
fn one_pending_below_the_cap_files_one_chunk_and_leaves_room_for_rules() {
    let mut f = Fixture::new();
    history(&f, CAFE, "food.eating_out", 5, "food.eating_out");
    pending_from_orre(&f, MAX_PENDING_CATEGORY_SUGGESTIONS - 1);
    let rows: Vec<(u32, i64, String)> = (0..101)
        .map(|n| (1 + n % 28, -450 - i64::from(n), format!("n{n}")))
        .collect();
    let rows: Vec<(u32, i64, &str, &str)> = rows
        .iter()
        .map(|(d, minor, id)| (*d, *minor, id.as_str(), CAFE))
        .collect();

    import(&mut f, &rows);

    assert_eq!(filed(&f).len(), 1);
    let pending: i64 = f
        .conn()
        .query_row(
            "SELECT COUNT(*) FROM suggestion WHERE status = 'pending'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(pending, MAX_PENDING_CATEGORY_SUGGESTIONS as i64);
    const { assert!(MAX_PENDING_CATEGORY_SUGGESTIONS < crate::inbox::MAX_PENDING_PER_SOURCE) };
}

#[test]
fn a_failing_suggestion_step_leaves_the_import_standing() {
    let mut f = Fixture::new();
    history(&f, CAFE, "food.eating_out", 5, "food.eating_out");
    f.conn()
        .execute_batch(
            "CREATE TRIGGER scratch_refuse_suggestions BEFORE INSERT ON suggestion \
             BEGIN SELECT RAISE(ABORT, 'refused'); END",
        )
        .unwrap();
    let preview = import::preview(
        &f.db,
        f.account,
        "statement.ofx",
        statement(&[(10, -450, "n1", CAFE)]),
    )
    .unwrap();

    let committed = import::commit(&mut f.db, &preview, preview.needs_confirmation()).unwrap();

    let Committed::Batch(summary) = committed else {
        panic!("expected a batch");
    };
    let stored: i64 = f
        .conn()
        .query_row(
            "SELECT COUNT(*) FROM \"transaction\" WHERE batch_id = ?1",
            [summary.batch],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(stored, 1);
    assert!(filed(&f).is_empty());
}

#[test]
fn confirmed_transfer_legs_and_refund_inflows_dont_vote() {
    let mut f = Fixture::new();
    // Three stale-category rows that take none of their own, two honest ones.
    for n in 0..3 {
        let leg = f.row(
            day(1 + n),
            -400 - i64::from(n),
            CAFE,
            Some("food.groceries"),
        );
        f.transfer(
            leg,
            f.row_on(f.savings, day(1 + n), 400 + i64::from(n), "Elsewhere", None),
            true,
        );
    }
    for n in 3..5 {
        f.row(
            day(1 + n),
            -400 - i64::from(n),
            CAFE,
            Some("food.eating_out"),
        );
    }
    import(&mut f, &[(10, -450, "n1", CAFE)]);
    assert!(filed(&f).is_empty());

    for n in 0..3 {
        let inflow = f.row(
            day(1 + n),
            400 + i64::from(n),
            "Bean Cafe Refunds",
            Some("food.groceries"),
        );
        let purchase = f.row(day(1 + n), -400 - i64::from(n), "Shop", None);
        f.refund(inflow, purchase, 400 + i64::from(n), true);
    }
    for n in 3..5 {
        f.row(
            day(1 + n),
            400 + i64::from(n),
            "Bean Cafe Refunds",
            Some("food.eating_out"),
        );
    }
    import(&mut f, &[(11, 450, "n2", "Bean Cafe Refunds")]);
    assert!(filed(&f).is_empty());
}
