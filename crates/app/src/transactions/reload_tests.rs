//! Tests of [`TransactionsView::reload`] for a read nobody in the app asked
//! for: another process (an MCP client, `orre import`, `undo-import`) wrote
//! the database, and the user's place must survive it (ROADMAP LU.4). The
//! scratch database is written through a separate core connection, as that
//! process would.

use chrono::{Days, NaiveDate};
use gpui_kit::test::TestWindowExt as _;
use gpui_kit::{SharedString, TestAppContext, VisualTestContext};
use orre_core::account::{self, AccountKind, NewAccount};
use orre_core::db::Database;
use orre_core::id::{AccountId, TransactionId};
use orre_core::manual::{self, ManualTransaction};
use orre_core::money::{Currency, Money};
use tempfile::TempDir;

use super::describe::InspectorDescription;
use super::*;

/// Rows on the scratch database: more than a window shows.
const ROWS: usize = 40;

struct Scratch {
    _dir: TempDir,
    path: std::path::PathBuf,
    account: AccountId,
    /// "SHOP 00" (the oldest day) to "SHOP 39", one a day.
    rows: Vec<TransactionId>,
}

impl Scratch {
    fn new() -> Self {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("orre.db");
        let mut db = Database::open(&path).unwrap();
        let account = account::create(
            &mut db,
            &NewAccount::new("Current account", AccountKind::Current, Currency::GBP),
        )
        .unwrap();
        let rows = (0..ROWS)
            .map(|day| add(&mut db, account, day, &format!("SHOP {day:02}")))
            .collect();
        Self {
            _dir: dir,
            path,
            account,
            rows,
        }
    }

    fn db(&self) -> Database {
        Database::open(&self.path).unwrap()
    }

    /// What another process does: adds a row to the newest day and more.
    fn add_elsewhere(&self, description: &str) -> TransactionId {
        add(&mut self.db(), self.account, ROWS, description)
    }
}

fn add(db: &mut Database, account: AccountId, day: usize, description: &str) -> TransactionId {
    let date = NaiveDate::from_ymd_opt(2026, 9, 1).unwrap() + Days::new(day as u64);
    let entry =
        ManualTransaction::new(account, date, Money::new(-1000, Currency::GBP), description);
    manual::create(db, &entry).unwrap()
}

fn open<'a>(
    cx: &'a mut TestAppContext,
    scratch: &Scratch,
) -> (Entity<TransactionsView>, &'a mut VisualTestContext) {
    cx.update(|cx| {
        gpui_kit::init(cx);
        crate::theme::component::install(cx);
        init(cx);
    });
    let store = TransactionStore::new(scratch.path.clone());
    let (view, cx) = cx.add_window_view(move |window, cx| {
        TransactionsView::new(Source::Database(store), window, cx)
    });
    redraw(cx);
    (view, cx)
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

fn reload(view: &Entity<TransactionsView>, cx: &mut VisualTestContext) {
    cx.update(|_, cx| view.update(cx, |view, cx| view.reload(cx)));
    redraw(cx);
}

fn describe(
    view: &Entity<TransactionsView>,
    cx: &mut VisualTestContext,
) -> super::describe::TransactionsDescription {
    cx.update(|_, cx| view.read(cx).describe(cx))
}

/// Scrolls the list so item `index` is at its top.
fn scroll_to_item(view: &Entity<TransactionsView>, index: usize, cx: &mut VisualTestContext) {
    cx.update(|_, cx| {
        view.update(cx, |view, _| {
            view.list.scroll_to(ListOffset {
                item_ix: index,
                offset_in_item: gpui_kit::px(0.),
            })
        })
    });
    redraw(cx);
}

/// What is at the top of the list, by identity.
fn top(view: &Entity<TransactionsView>, cx: &mut VisualTestContext) -> Option<ScrolledTo> {
    cx.update(|_, cx| view.read(cx).scroll_anchor().map(|anchor| anchor.item))
}

fn merchant_field(view: &Entity<TransactionsView>, cx: &mut VisualTestContext) -> String {
    cx.update(|_, cx| {
        let input = view.read(cx).editor.merchant.clone();
        input.read(cx).value().to_string()
    })
}

#[gpui_kit::test]
fn a_reload_from_outside_keeps_the_place_and_what_was_typed(cx: &mut TestAppContext) {
    let scratch = Scratch::new();
    let (view, cx) = open(cx, &scratch);

    // Mid-task: a search, a selected row, a half-typed merchant name, and the
    // list scrolled well down.
    click("search", cx);
    cx.update(|window, cx| window.input("shop", cx));
    redraw(cx);
    let chosen = scratch.rows[20];
    cx.update(|_, cx| view.update(cx, |view, cx| view.select([chosen], cx)));
    redraw(cx);
    cx.update(|window, cx| {
        let merchant = view.read(cx).editor.merchant.clone();
        merchant.update(cx, |input, cx| {
            input.set_value("Half a nam", window, cx);
            input.focus(window, cx);
        });
    });
    redraw(cx);
    scroll_to_item(&view, 30, cx);
    let before = describe(&view, cx);
    let place = top(&view, cx).expect("scrolled below the top");
    assert_eq!(before.toolbar.search, "shop");
    assert_eq!(before.selected(), [chosen]);
    let listed_before = before.listed();
    assert_eq!(listed_before.len(), ROWS);

    // Elsewhere: a newer row that matches the search, and one that doesn't.
    let added = scratch.add_elsewhere("SHOP NEW");
    scratch.add_elsewhere("UNRELATED");
    reload(&view, cx);

    let after = describe(&view, cx);
    assert_eq!(after.toolbar.search, "shop", "search text stays");
    assert!(
        cx.update(|_, cx| view.read(cx).filter().text == "shop"),
        "and so does the filter"
    );
    assert_eq!(after.selected(), [chosen], "the selection stays");
    assert_eq!(after.keyboard_row(), Some(chosen));
    assert_eq!(
        merchant_field(&view, cx),
        "Half a nam",
        "the typed text isn't put back"
    );
    let mut expected = vec![added];
    expected.extend(listed_before);
    assert_eq!(after.listed(), expected, "the new matching row is listed");
    assert_eq!(
        top(&view, cx),
        Some(place),
        "the same item is at the top, though two items came in above it"
    );
}

#[gpui_kit::test]
fn a_reload_from_outside_leaves_the_top_of_the_list_at_the_top(cx: &mut TestAppContext) {
    let scratch = Scratch::new();
    let (view, cx) = open(cx, &scratch);
    assert_eq!(top(&view, cx), None);
    scratch.add_elsewhere("SHOP NEW");
    reload(&view, cx);
    assert_eq!(top(&view, cx), None);
    assert_eq!(describe(&view, cx).listed().len(), ROWS + 1);
}

#[gpui_kit::test]
fn a_reload_from_outside_keeps_filters_an_open_menu_and_the_selection(cx: &mut TestAppContext) {
    let scratch = Scratch::new();
    let (view, cx) = open(cx, &scratch);
    let chosen = scratch.rows[5];
    cx.update(|_, cx| view.update(cx, |view, cx| view.select([chosen], cx)));
    redraw(cx);
    cx.update(|_, cx| view.update(cx, |view, cx| view.set_excluded(true, cx)));
    click("filter-date", cx);
    let menu = |cx: &mut VisualTestContext| describe(&view, cx).menu.map(|menu| menu.menu);
    assert_eq!(menu(cx), Some(Menu::Date));

    scratch.add_elsewhere("SHOP NEW");
    reload(&view, cx);

    assert_eq!(menu(cx), Some(Menu::Date), "the menu stays open");
    assert_eq!(describe(&view, cx).selected(), [chosen]);
    assert!(cx.update(|_, cx| view.read(cx).filter().include_excluded));
    assert_eq!(describe(&view, cx).listed().len(), ROWS + 1);
}

#[gpui_kit::test]
fn a_selected_row_that_goes_elsewhere_is_let_go_and_nothing_else_is_selected(
    cx: &mut TestAppContext,
) {
    let scratch = Scratch::new();
    let (view, cx) = open(cx, &scratch);
    click("search", cx);
    cx.update(|window, cx| window.input("shop", cx));
    redraw(cx);
    let (kept, gone) = (scratch.rows[10], scratch.rows[11]);
    cx.update(|_, cx| view.update(cx, |view, cx| view.select([kept, gone], cx)));
    redraw(cx);
    scroll_to_item(&view, 30, cx);

    manual::delete(&mut scratch.db(), gone).unwrap();
    reload(&view, cx);

    let after = describe(&view, cx);
    assert_eq!(after.selected(), [kept], "only the row that went is let go");
    assert!(!after.listed().contains(&gone));
    assert_eq!(after.toolbar.search, "shop");

    // The only selected row going leaves nothing selected, not a neighbour.
    manual::delete(&mut scratch.db(), kept).unwrap();
    reload(&view, cx);
    let after = describe(&view, cx);
    assert!(after.selected().is_empty());
    assert!(!after.listed().contains(&kept));
    // LU.5: and the inspector says it went (`arrival_tests`).
    assert!(matches!(
        after.inspector,
        InspectorDescription::Removed { .. }
    ));
    assert_eq!(after.toolbar.search, "shop");
    assert_eq!(after.listed().len(), ROWS - 2);
}

#[gpui_kit::test]
fn a_reload_after_rows_went_elsewhere_keeps_the_same_item_at_the_top(cx: &mut TestAppContext) {
    let scratch = Scratch::new();
    let (view, cx) = open(cx, &scratch);
    // A day heading and a row at the top, in turn: the place is read from
    // the register the list was drawn from, not the one just read. One row
    // goes each time, above the top, so the top's index in the register
    // shifts;
    // few enough that the list still scrolls that far.
    for (index, gone) in [(30, 39..40), (31, 38..39)] {
        scroll_to_item(&view, index, cx);
        let place = top(&view, cx).expect("scrolled below the top");
        let mut db = scratch.db();
        for row in &scratch.rows[gone] {
            manual::delete(&mut db, *row).unwrap();
        }
        drop(db);
        reload(&view, cx);
        assert_eq!(top(&view, cx), Some(place), "item {index} stays at the top");
    }
}
