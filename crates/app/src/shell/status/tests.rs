use chrono::NaiveDate;
use orre_core::import::BatchStatus;
use orre_core::money::Currency;

use super::*;
use crate::accounts::fixture;

fn at(stored: &str) -> DateTime<Utc> {
    clock::parse_stored(stored).unwrap()
}

fn counts(transactions: usize, excluded: usize, sample: bool) -> Counts {
    Counts {
        transactions,
        excluded,
        sample,
    }
}

/// The status bar on `accounts` from the database, AI access on.
fn read(accounts: &AccountsData, enabled_clients: usize, has_checks: bool) -> Status {
    Status::read(&Inputs {
        ai_on: true,
        enabled_clients,
        failing: 0,
        has_checks,
        accounts,
        counts: counts(0, 0, false),
        now: at("2026-11-09T08:14:30.000Z"),
    })
}

fn empty() -> AccountsData {
    AccountsData::empty(Vec::new(), NaiveDate::from_ymd_opt(2026, 11, 17).unwrap())
}

/// A new database: nothing imported, no checks, no clients. Only what's
/// true is said (the bug: Paper's sample showed on an empty database).
#[test]
fn an_empty_database_shows_only_the_transactions_count() {
    let status = read(&empty(), 0, false);
    assert_eq!(
        status,
        Status {
            ai: Ai::None,
            checks: ChecksStatus::None,
            import: None,
            transactions: "0 transactions".to_owned(),
            new: None,
            currency: None,
        }
    );
}

#[test]
fn the_ai_item_counts_enabled_clients_while_access_is_on() {
    assert_eq!(read(&empty(), 1, false).ai, Ai::Connected(1));
    assert_eq!(read(&empty(), 3, false).ai, Ai::Connected(3));
    assert_eq!(agents_text(1), "1 agent connected");
    assert_eq!(agents_text(3), "3 agents connected");
    let off = Status::read(&Inputs {
        ai_on: false,
        enabled_clients: 2,
        failing: 0,
        has_checks: false,
        accounts: &empty(),
        counts: counts(0, 0, false),
        now: at("2026-11-09T08:14:30.000Z"),
    });
    assert_eq!(off.ai, Ai::Off);
}

#[test]
fn reconciled_needs_a_check_and_a_failure_replaces_it() {
    assert_eq!(read(&empty(), 0, true).checks, ChecksStatus::Reconciled);
    let failing = Status::read(&Inputs {
        ai_on: true,
        enabled_clients: 0,
        failing: 2,
        has_checks: true,
        accounts: &empty(),
        counts: counts(0, 0, false),
        now: at("2026-11-09T08:14:30.000Z"),
    });
    assert_eq!(failing.checks, ChecksStatus::Failing(2));
    assert_eq!(failing_text(1), "1 balance check failing");
    assert_eq!(failing_text(2), "2 balance checks failing");
}

/// The fixture's newest import is `monzo-november.csv` at 08:12.
#[test]
fn the_import_line_names_the_newest_import_still_in_an_account() {
    let mut accounts = fixture::accounts();
    let status = read(&accounts, 0, true);
    assert_eq!(
        status.import.as_deref(),
        Some("Imported 2 min ago from monzo-november.csv")
    );

    // Undone, it no longer counts: the October one is the newest left.
    let detail = accounts.details.get_mut(&fixture::CURRENT).unwrap();
    detail.imports[0].status = BatchStatus::Undone;
    assert_eq!(
        read(&accounts, 0, true).import.as_deref(),
        Some("Imported on 2 Nov from monzo-october.csv")
    );
}

#[test]
fn ago_reads_minutes_hours_then_the_date() {
    let now = at("2026-11-14T12:00:00.000Z");
    assert_eq!(ago(at("2026-11-14T11:59:30.000Z"), now), "just now");
    // A import a moment after the time was last read.
    assert_eq!(ago(at("2026-11-14T12:00:20.000Z"), now), "just now");
    assert_eq!(ago(at("2026-11-14T11:58:00.000Z"), now), "2 min ago");
    assert_eq!(ago(at("2026-11-14T10:59:00.000Z"), now), "1 hour ago");
    assert_eq!(ago(at("2026-11-14T09:00:00.000Z"), now), "3 hours ago");
    assert_eq!(ago(at("2026-11-04T10:00:00.000Z"), now), "on 4 Nov");
}

#[test]
fn the_currency_shows_when_every_open_account_shares_one() {
    let mut accounts = fixture::accounts();
    assert_eq!(read(&accounts, 0, true).currency.as_deref(), Some("GBP"));

    accounts.tree.accounts[0].account.currency = Currency::EUR;
    assert_eq!(read(&accounts, 0, true).currency, None);
    assert_eq!(read(&empty(), 0, false).currency, None);
}

/// The sample data keeps Paper's artboard (`1CU-0`).
#[test]
fn the_sample_data_keeps_papers_status_bar() {
    let status = Status::read(&Inputs {
        ai_on: true,
        enabled_clients: 0,
        failing: 0,
        has_checks: false,
        accounts: &empty(),
        counts: counts(23, 0, true),
        now: at("2026-11-17T10:00:00.000Z"),
    });
    assert_eq!(
        status,
        Status {
            ai: Ai::Connected(1),
            checks: ChecksStatus::Reconciled,
            import: Some("Imported 2 min ago from monzo-november.csv".to_owned()),
            transactions: "328 transactions".to_owned(),
            new: Some("3 new".to_owned()),
            currency: Some("GBP".to_owned()),
        }
    );
}

#[test]
fn the_status_bar_counts_excluded_rows_when_there_are_some() {
    assert_eq!(transactions_count(counts(12, 0, false)), "12 transactions");
    assert_eq!(transactions_count(counts(1, 0, false)), "1 transaction");
    assert_eq!(
        transactions_count(counts(12, 2, false)),
        "12 transactions · 2 excluded"
    );
    // The sample data keeps the artboard's figure (D6.3, `3FE-0`).
    assert_eq!(
        transactions_count(counts(23, 2, true)),
        "328 transactions · 2 excluded"
    );
}
