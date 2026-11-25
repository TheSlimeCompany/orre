//! The user's month start (ROADMAP M4.11) in the month tools and the
//! `monthly_review` prompt: each month result gives its `from` and `to`,
//! months are cut by the rule, and the dates are the same at every level.

mod support;

use orre_core::money::Currency;
use orre_core::reporting::period::{MonthStart, StartDay, Weekends};
use orre_core::setting;
use rmcp::model::ContentBlock;
use serde_json::{Value, json};
use support::reports::{add_account, add_row, amounts, writable};
use support::*;

/// A month starting on the 25th, named after the month it ends in.
fn the_25th() -> MonthStart {
    MonthStart::new(StartDay::day(25).unwrap(), Weekends::Keep)
}

/// The fixture under the 25th, with a euro row on 24 Nov and one on 25 Nov
/// (euros: no fixture row is in them).
fn paid_on_the_25th() -> Fixture {
    let fixture = Fixture::new();
    setting::set_month_start(&mut writable(&fixture), the_25th()).unwrap();
    let account = add_account(&fixture, "Euro", Currency::EUR);
    add_row(&fixture, account, "2026-11-24", -1_000, None, "before");
    add_row(&fixture, account, "2026-11-25", -2_000, None, "after");
    fixture
}

fn euros(value: &Value) -> Vec<i64> {
    amounts(value)
        .into_iter()
        .filter(|(currency, _)| currency == "EUR")
        .map(|(_, minor)| minor)
        .collect()
}

fn dates(value: &Value) -> (&str, &str) {
    (
        value["from"].as_str().unwrap(),
        value["to"].as_str().unwrap(),
    )
}

#[tokio::test]
async fn monthly_summary_cuts_months_on_the_users_start_day() {
    let fixture = paid_on_the_25th();
    for &level in LEVELS {
        let client = fixture.connect("claude-code", level).await;
        let result = client
            .call_ok(
                "monthly_summary",
                json!({ "from": "2026-11", "to": "2026-12" }),
            )
            .await;
        let months = result["months"].as_array().unwrap();
        assert_eq!(
            dates(&months[0]),
            ("2026-10-25", "2026-11-24"),
            "at {level}"
        );
        assert_eq!(
            dates(&months[1]),
            ("2026-11-25", "2026-12-24"),
            "at {level}"
        );
        assert_eq!(euros(&months[0]["spending"]), [1_000], "at {level}");
        assert_eq!(euros(&months[1]["spending"]), [2_000], "at {level}");
        client.close().await;
    }
}

#[tokio::test]
async fn the_other_month_tools_give_the_same_dates() {
    let fixture = paid_on_the_25th();
    let client = fixture.connect("claude-code", AccessLevel::Aggregate).await;

    let spending = client
        .call_ok("spending_by_category", json!({ "period": "2026-12" }))
        .await;
    assert_eq!(dates(&spending), ("2026-11-25", "2026-12-24"));
    assert_eq!(euros(&spending["total"]), [2_000]);

    let budget = client
        .call_ok("budget_status", json!({ "month": "2026-11" }))
        .await;
    assert_eq!(dates(&budget), ("2026-10-25", "2026-11-24"));

    let worth = client
        .call_ok(
            "net_worth_history",
            json!({ "from": "2026-11", "to": "2026-12" }),
        )
        .await;
    let months = worth["months"].as_array().unwrap();
    assert_eq!(dates(&months[0]), ("2026-10-25", "2026-11-24"));
    assert_eq!(months[0]["date"], "2026-11-24");
    assert_eq!(dates(&months[1]), ("2026-11-25", "2026-12-24"));
    client.close().await;
}

#[tokio::test]
async fn the_dates_are_calendar_months_without_a_setting() {
    let fixture = Fixture::new();
    let client = fixture.connect("claude-code", AccessLevel::Aggregate).await;
    let result = client
        .call_ok(
            "monthly_summary",
            json!({ "from": "2026-11", "to": "2026-11" }),
        )
        .await;
    assert_eq!(dates(&result["months"][0]), ("2026-11-01", "2026-11-30"));
    client.close().await;
}

#[tokio::test]
async fn monthly_review_names_the_dates_of_the_month_under_the_rule() {
    let fixture = paid_on_the_25th();
    let client = fixture.connect("claude-code", AccessLevel::Standard).await;
    let result = client
        .prompt("monthly_review", json!({ "month": "2026-11" }))
        .await
        .unwrap();
    let text: String = result
        .messages
        .iter()
        .map(|message| match &message.content {
            ContentBlock::Text(text) => text.text.clone(),
            other => panic!("not text: {other:?}"),
        })
        .collect();
    assert!(
        text.contains("2026-11 runs from 2026-10-25 to 2026-11-24"),
        "{text}"
    );
    // The anomalies search covers the same days.
    assert!(
        text.contains("`from` 2026-10-25 and `to` 2026-11-24"),
        "{text}"
    );
    assert!(text.contains("covers 2026-10-25 to 2026-11-24"), "{text}");
    client.close().await;
}
