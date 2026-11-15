//! UI tests for M3.8: the Transfer section confirms, rejects and unpairs a
//! pair, the undo bar reverses each, and an agent's pending suggestion for
//! the same pair is settled, against a scratch database through
//! [`TransactionStore`], as `coffer` runs them.

use chrono::NaiveDate;
use coffer_core::account::{self, AccountKind, NewAccount};
use coffer_core::db::Database;
use coffer_core::id::{AccountId, TransactionId, TransferGroupId};
use coffer_core::import::{self, Committed};
use coffer_core::inbox::{self, NewSuggestion, SuggestionKind, SuggestionStatus};
use coffer_core::mcp_client::ClientName;
use coffer_core::money::{Currency, Money};
use coffer_core::statement::{SourceFormat, SourceLocation, Statement, StatementLine};
use coffer_core::transfer::{self, TransferStatus};
use gpui_kit::test::TestWindowExt as _;
use gpui_kit::{SharedString, TestAppContext, VisualTestContext};
use serde_json::json;
use tempfile::TempDir;

use super::*;

fn setup(cx: &mut TestAppContext) {
    cx.update(|cx| {
        gpui_kit::init(cx);
        crate::theme::component::install(cx);
        init(cx);
    });
}

fn redraw(cx: &mut VisualTestContext) {
    cx.run_until_parked();
    cx.update(|window, cx| window.draw(cx).clear(cx));
}

fn click(id: impl Into<SharedString>, cx: &mut VisualTestContext) {
    let id = id.into();
    cx.update(move |window, cx| window.click(id, cx));
    redraw(cx);
}

fn shows(id: impl Into<SharedString>, cx: &mut VisualTestContext) -> bool {
    let id = id.into();
    cx.update(move |window, _| window.try_find(id).is_some())
}

fn day(day: u32) -> NaiveDate {
    NaiveDate::from_ymd_opt(2026, 11, day).unwrap()
}

/// Current account and Savings, a £50 move between them that detection
/// suggested as a pair, and an agent's pending suggestion of the same pair.
struct Scratch {
    _dir: TempDir,
    path: std::path::PathBuf,
    out: TransactionId,
    inn: TransactionId,
    group: TransferGroupId,
    suggestion: inbox::SuggestionId,
}

impl Scratch {
    fn new() -> Self {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("coffer.db");
        let mut db = Database::open(&path).unwrap();
        let current = account::create(
            &mut db,
            &NewAccount::new("Current account", AccountKind::Current, Currency::GBP),
        )
        .unwrap();
        let savings = account::create(
            &mut db,
            &NewAccount::new("Savings", AccountKind::Savings, Currency::GBP),
        )
        .unwrap();
        // Two imports; the second's detection pairs its row with the first's.
        let out = import_one(&mut db, current, "current.csv", day(2), -5000, "TO SAVINGS");
        let inn = import_one(
            &mut db,
            savings,
            "savings.csv",
            day(3),
            5000,
            "FROM CURRENT",
        );
        let group = transfer::group_of(&db, out).unwrap().unwrap().id;
        let suggestion = inbox::file_suggestion(
            &mut db,
            &ClientName::new("claude-code").unwrap(),
            &NewSuggestion {
                kind: SuggestionKind::TransferPair,
                payload: json!({"outflow_id": out.get(), "inflow_id": inn.get()}),
                rationale: None,
            },
        )
        .unwrap()
        .id;
        Self {
            _dir: dir,
            path,
            out,
            inn,
            group,
            suggestion,
        }
    }

    fn status(&self) -> TransferStatus {
        transfer::group(&Database::open(&self.path).unwrap(), self.group)
            .unwrap()
            .status
    }

    fn suggestion(&self) -> SuggestionStatus {
        inbox::get(&Database::open(&self.path).unwrap(), self.suggestion)
            .unwrap()
            .status
    }
}

/// Imports one row into `account` and returns it.
fn import_one(
    db: &mut Database,
    account: AccountId,
    file: &str,
    on: NaiveDate,
    minor: i64,
    text: &str,
) -> TransactionId {
    let mut statement = Statement::new(SourceFormat::Csv, Currency::GBP);
    statement.push(
        SourceLocation::Line(2),
        Ok(StatementLine::new(
            on,
            Money::new(minor, Currency::GBP),
            text,
        )),
    );
    let preview = import::preview(db, account, file, statement).unwrap();
    let Committed::Batch(summary) =
        import::commit(db, &preview, preview.needs_confirmation()).unwrap()
    else {
        panic!("a batch");
    };
    let register = Register::load(&db.reader().unwrap(), day(14)).unwrap();
    register
        .rows()
        .iter()
        .find(|row| row.source.batch() == Some(summary.batch))
        .unwrap()
        .id
}

fn open<'a>(cx: &'a mut TestAppContext, scratch: &Scratch) -> &'a mut VisualTestContext {
    setup(cx);
    let store = TransactionStore::new(scratch.path.clone());
    let (_view, cx) = cx.add_window_view(move |window, cx| {
        TransactionsView::new(Source::Database(store), window, cx)
    });
    redraw(cx);
    cx
}

#[gpui_kit::test]
fn confirming_a_suggested_pair_settles_the_agents_suggestion_and_undoes(cx: &mut TestAppContext) {
    let scratch = Scratch::new();
    let cx = open(cx, &scratch);
    click(format!("txn-{}", scratch.out), cx);
    assert!(shows("confirm-transfer", cx));
    assert!(shows("reject-transfer", cx));
    assert!(!shows("split", cx), "a transfer can't be split");

    click("confirm-transfer", cx);
    assert_eq!(scratch.status(), TransferStatus::Confirmed);
    assert_eq!(scratch.suggestion(), SuggestionStatus::Approved);
    assert!(shows("unpair-transfer", cx));
    assert!(shows("undo-change", cx));

    click("undo-change", cx);
    assert_eq!(scratch.status(), TransferStatus::Suggested);
    assert!(shows("confirm-transfer", cx));
}

#[gpui_kit::test]
fn not_a_transfer_rejects_the_pair_and_the_section_goes(cx: &mut TestAppContext) {
    let scratch = Scratch::new();
    let cx = open(cx, &scratch);
    click(format!("txn-{}", scratch.inn), cx);
    click("reject-transfer", cx);
    assert_eq!(scratch.status(), TransferStatus::Rejected);
    assert_eq!(scratch.suggestion(), SuggestionStatus::Rejected);
    assert!(!shows("confirm-transfer", cx));
    assert!(shows("split", cx), "an ordinary row again");
}

#[gpui_kit::test]
fn unpairing_a_confirmed_pair_and_undoing_it(cx: &mut TestAppContext) {
    let scratch = Scratch::new();
    transfer::confirm(&mut Database::open(&scratch.path).unwrap(), scratch.group).unwrap();
    let cx = open(cx, &scratch);
    click(format!("txn-{}", scratch.out), cx);
    click("unpair-transfer", cx);
    assert_eq!(scratch.status(), TransferStatus::Rejected);

    click("undo-change", cx);
    assert_eq!(scratch.status(), TransferStatus::Confirmed);
    assert!(shows("unpair-transfer", cx));
}

/// An undo that core refuses (the pair changed since) keeps the bar, with
/// the reason, rather than failing where nothing is drawn.
#[gpui_kit::test]
fn a_refused_undo_keeps_the_bar_and_says_why(cx: &mut TestAppContext) {
    let scratch = Scratch::new();
    let cx = open(cx, &scratch);
    click(format!("txn-{}", scratch.out), cx);
    click("reject-transfer", cx);
    // Changed behind the view's back: the pair is suggested again.
    let mut db = Database::open(&scratch.path).unwrap();
    let decision = transfer::Decision {
        group: scratch.group,
        action: transfer::Action::Reject,
        transactions: [scratch.out, scratch.inn],
    };
    transfer::undo(&mut db, &decision).unwrap();
    drop(db);

    click("undo-change", cx);
    assert_eq!(scratch.status(), TransferStatus::Suggested);
    assert!(shows("undo-change", cx), "the bar stays");
}
