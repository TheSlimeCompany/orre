//! UI tests for the bulk-action bar's Mark as transfer and Create rule
//! ([`super::bulk`]), against a scratch database through
//! [`TransactionStore`], as `orre` runs them.

use std::cell::RefCell;
use std::rc::Rc;

use chrono::NaiveDate;
use gpui_kit::test::TestWindowExt as _;
use gpui_kit::{SharedString, TestAppContext, VisualTestContext};
use orre_core::account::{self, AccountKind, NewAccount};
use orre_core::category::CategoryKey;
use orre_core::db::Database;
use orre_core::id::{AccountId, TransactionId};
use orre_core::inbox::own;
use orre_core::manual::{self, ManualTransaction};
use orre_core::money::{Currency, Money};
use orre_core::rule::{self, RuleOrigin};
use orre_core::transfer::{self, TransferStatus};
use tempfile::TempDir;

use super::bulk::{DIFFERENT_RULES, NO_RULE};
use super::describe::{self, BarDescription, InspectorDescription, TransactionsDescription};
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

fn day(day: u32) -> NaiveDate {
    NaiveDate::from_ymd_opt(2026, 11, day).unwrap()
}

/// Current account, Savings; £50 out of the current account on the 2nd
/// and into savings on the 13th, too far apart for detection; rows that
/// can't pair with it; and three categorised payments for rules.
struct Scratch {
    _dir: TempDir,
    path: std::path::PathBuf,
    out: TransactionId,
    into: TransactionId,
    /// £40 into savings.
    less: TransactionId,
    /// £50 into the current account.
    same_account: TransactionId,
    pret: TransactionId,
    pret_again: TransactionId,
    tesco: TransactionId,
}

impl Scratch {
    fn new() -> Self {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("orre.db");
        let mut db = Database::open(&path).unwrap();
        let current = account(&mut db, "Current account", AccountKind::Current);
        let savings = account(&mut db, "Savings", AccountKind::Savings);
        let mut row = |account, on, minor, text| {
            let entry =
                ManualTransaction::new(account, day(on), Money::new(minor, Currency::GBP), text);
            manual::create(&mut db, &entry).unwrap()
        };
        let out = row(current, 2, -5000, "TO SAVINGS");
        let into = row(savings, 13, 5000, "FROM CURRENT");
        let less = row(savings, 12, 4000, "FROM CURRENT");
        let same_account = row(current, 11, 5000, "REFUND");
        let pret = row(current, 5, -450, "PRET A MANGER");
        let pret_again = row(current, 6, -520, "PRET A MANGER");
        let tesco = row(current, 7, -2000, "TESCO STORES");
        let key = |key| CategoryKey::new(key).unwrap();
        own::recategorize(
            &mut db,
            &[
                (pret, key("food.eating_out")),
                (pret_again, key("food.eating_out")),
                (tesco, key("food.groceries")),
            ],
        )
        .unwrap();
        Self {
            _dir: dir,
            path,
            out,
            into,
            less,
            same_account,
            pret,
            pret_again,
            tesco,
        }
    }

    fn db(&self) -> Database {
        Database::open(&self.path).unwrap()
    }

    fn group_of(&self, id: TransactionId) -> Option<transfer::TransferGroup> {
        transfer::group_of(&self.db(), id).unwrap()
    }
}

fn account(db: &mut Database, name: &str, kind: AccountKind) -> AccountId {
    account::create(db, &NewAccount::new(name, kind, Currency::GBP)).unwrap()
}

fn open<'a>(
    cx: &'a mut TestAppContext,
    scratch: &Scratch,
) -> (Entity<TransactionsView>, &'a mut VisualTestContext) {
    setup(cx);
    let store = TransactionStore::new(scratch.path.clone());
    let (view, cx) = cx.add_window_view(move |window, cx| {
        TransactionsView::new(Source::Database(store), window, cx)
    });
    redraw(cx);
    (view, cx)
}

fn select(view: &Entity<TransactionsView>, ids: &[TransactionId], cx: &mut VisualTestContext) {
    let ids = ids.to_vec();
    cx.update(|_, cx| view.update(cx, |view, cx| view.select(ids, cx)));
    redraw(cx);
}

fn describe(
    view: &Entity<TransactionsView>,
    cx: &mut VisualTestContext,
) -> TransactionsDescription {
    cx.update(|_, cx| view.read(cx).describe(cx))
}

/// Whether the bulk-action bar's Mark as transfer is enabled, as
/// described.
fn mark_enabled(view: &Entity<TransactionsView>, cx: &mut VisualTestContext) -> bool {
    match describe(view, cx).bar {
        Some(BarDescription::Bulk { actions, .. }) => {
            actions
                .iter()
                .find(|action| action.label == "Mark as transfer")
                .unwrap()
                .enabled
        }
        other => panic!("expected the bulk-action bar, got {other:?}"),
    }
}

/// The error the several-rows inspector says.
fn several_error(view: &Entity<TransactionsView>, cx: &mut VisualTestContext) -> Option<String> {
    match describe(view, cx).inspector {
        InspectorDescription::Several(several) => several.category_error,
        other => panic!("expected several rows, got {other:?}"),
    }
}

/// Everything the view sends the shell from now on.
fn events(
    view: &Entity<TransactionsView>,
    cx: &mut VisualTestContext,
) -> Rc<RefCell<Vec<TransactionsEvent>>> {
    let events = Rc::new(RefCell::new(Vec::new()));
    let sink = events.clone();
    cx.update(|_, cx| {
        cx.subscribe(view, move |_, event: &TransactionsEvent, _| {
            sink.borrow_mut().push(event.clone());
        })
        .detach();
    });
    events
}

// Mark as transfer ------------------------------------------------------------

#[gpui_kit::test]
fn marking_two_rows_confirms_a_pair_and_undo_unpairs_it(cx: &mut TestAppContext) {
    let scratch = Scratch::new();
    let (view, cx) = open(cx, &scratch);
    let events = events(&view, cx);
    // Money in first: the bar still names the money out first.
    select(&view, &[scratch.into, scratch.out], cx);
    assert!(mark_enabled(&view, cx));

    click("bulk-transfer", cx);

    let group = scratch.group_of(scratch.out).expect("paired");
    assert_eq!(group.status, TransferStatus::Confirmed);
    let mut pair = [scratch.out, scratch.into];
    pair.sort();
    assert_eq!(group.transactions, pair);
    let description = describe(&view, cx);
    assert_eq!(
        description.undo_message(),
        Some("Marked as transfer: Current account → Savings")
    );
    assert!(description.row(scratch.out).unwrap().transfer);
    assert!(description.row(scratch.into).unwrap().transfer);
    assert!(events.borrow().contains(&TransactionsEvent::Changed));

    click("undo-change", cx);

    assert_eq!(scratch.group_of(scratch.out), None);
    assert_eq!(scratch.group_of(scratch.into), None);
    let description = describe(&view, cx);
    assert!(!description.row(scratch.out).unwrap().transfer);
    assert_eq!(description.undo_message(), None);
}

#[gpui_kit::test]
fn marking_is_disabled_unless_two_rows_make_one_transfer(cx: &mut TestAppContext) {
    let scratch = Scratch::new();
    let (view, cx) = open(cx, &scratch);
    for rows in [
        // Three rows.
        vec![scratch.out, scratch.into, scratch.less],
        // One account.
        vec![scratch.out, scratch.same_account],
        // Different amounts.
        vec![scratch.out, scratch.less],
        // Both money out.
        vec![scratch.pret, scratch.tesco],
    ] {
        select(&view, &rows, cx);
        assert!(!mark_enabled(&view, cx), "{rows:?}");
        click("bulk-transfer", cx);
        assert!(rows.iter().all(|id| scratch.group_of(*id).is_none()));
    }
    // Already in a pair: marked, it can't be marked again.
    transfer::mark(&mut scratch.db(), scratch.out, scratch.into).unwrap();
    cx.update(|_, cx| view.update(cx, |view, cx| view.reload(cx)));
    redraw(cx);
    select(&view, &[scratch.out, scratch.into], cx);
    assert!(!mark_enabled(&view, cx));
}

// Create rule -------------------------------------------------------------------

#[gpui_kit::test]
fn rows_that_give_one_rule_open_it_in_rules(cx: &mut TestAppContext) {
    let scratch = Scratch::new();
    let expected = rule::rule_from(&scratch.db(), scratch.pret)
        .unwrap()
        .expect("a rule");
    assert_eq!(
        rule::rule_from(&scratch.db(), scratch.pret_again).unwrap(),
        Some(expected.clone())
    );
    let (view, cx) = open(cx, &scratch);
    let events = events(&view, cx);
    select(&view, &[scratch.pret, scratch.pret_again], cx);

    click("bulk-rule", cx);

    assert_eq!(
        *events.borrow(),
        [TransactionsEvent::CreateRule(expected, RuleOrigin::Edit)]
    );
    assert_eq!(several_error(&view, cx), None);
}

#[gpui_kit::test]
fn rows_that_give_different_rules_or_none_say_so(cx: &mut TestAppContext) {
    let scratch = Scratch::new();
    let (view, cx) = open(cx, &scratch);
    let events = events(&view, cx);

    select(&view, &[scratch.pret, scratch.tesco], cx);
    click("bulk-rule", cx);
    assert_eq!(several_error(&view, cx).as_deref(), Some(DIFFERENT_RULES));

    // An uncategorised row gives no rule.
    select(&view, &[scratch.pret, scratch.out], cx);
    click("bulk-rule", cx);
    assert_eq!(several_error(&view, cx).as_deref(), Some(NO_RULE));

    assert!(events.borrow().is_empty());
}

#[gpui_kit::test]
fn a_rule_found_takes_away_the_earlier_error(cx: &mut TestAppContext) {
    let scratch = Scratch::new();
    let (view, cx) = open(cx, &scratch);
    let events = events(&view, cx);
    select(&view, &[scratch.pret, scratch.tesco], cx);
    click("bulk-rule", cx);
    assert_eq!(several_error(&view, cx).as_deref(), Some(DIFFERENT_RULES));

    select(&view, &[scratch.pret, scratch.pret_again], cx);
    click("bulk-rule", cx);

    assert_eq!(events.borrow().len(), 1);
    assert_eq!(several_error(&view, cx), None);
}

#[gpui_kit::test]
fn the_bar_describes_create_rule_as_enabled(cx: &mut TestAppContext) {
    let scratch = Scratch::new();
    let (view, cx) = open(cx, &scratch);
    select(&view, &[scratch.pret, scratch.tesco], cx);
    let Some(BarDescription::Bulk { actions, .. }) = describe(&view, cx).bar else {
        panic!("the bulk-action bar");
    };
    assert_eq!(
        actions,
        [
            describe::Action::button("Categorise", true),
            describe::Action::button("Mark as transfer", false),
            describe::Action::button("Create rule", true),
            describe::Action::button("Clear selection", true),
        ]
    );
}
