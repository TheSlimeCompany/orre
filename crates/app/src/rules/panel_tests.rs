//! Tests of the Rules panel's keyboard cursor (ROADMAP U.9; `DESIGN.md` →
//! "Keyboard paths"): Tab reaches the panel, ↑↓ move the cursor through the
//! sections and the open section's filters without changing what's open,
//! and Enter or Space act on the cursor row as a click does. On the scratch
//! database from [`super::tests`], with the keys pressed as the user would;
//! the cursor and what's open are read through the test-only `describe()`.

use gpui_kit::{AppContext as _, Entity, TestAppContext, VisualTestContext, WindowOptions};

use super::describe::{PanelDescription, describe_window};
use super::store::RulesStore;
use super::tests::{Scratch, redraw};
use super::*;
use crate::accounts::{Accounts, fixture as accounts_fixture};
use crate::inbox::Inbox;
use crate::shell::{ShellView, View};
use crate::transactions::{self, store::TransactionStore};

/// The shell on Rules, as [`super::tests::open`] opens it, but in a real
/// window with `Root` (which binds Tab), focus on the Rules canvas.
fn open(
    cx: &mut TestAppContext,
    scratch: &Scratch,
) -> (Entity<ShellView>, Entity<Rules>, VisualTestContext) {
    cx.update(|cx| {
        gpui_kit::init(cx);
        crate::theme::component::install(cx);
        crate::shell::init(cx);
    });
    let store = RulesStore::new(scratch.path.clone());
    let transactions = TransactionStore::new(scratch.path.clone());
    let (window, shell) = cx
        .update(|cx| {
            gpui_kit::open_window(WindowOptions::default(), cx, |window, cx| {
                cx.new(|cx| {
                    let accounts = cx.new(|_| Accounts::fixture(accounts_fixture::accounts()));
                    let inbox = cx.new(|_| Inbox::empty());
                    let settings = cx.new(crate::settings::SettingsView::fixture);
                    let mut shell = ShellView::with_all(
                        View::Rules,
                        accounts,
                        transactions::Source::Database(transactions),
                        inbox,
                        settings,
                        window,
                        cx,
                    );
                    let rules = cx.new(|_| Rules::from_store(store));
                    shell.install_rules(rules, window, cx);
                    shell
                })
            })
        })
        .expect("open test window");
    let mut cx = VisualTestContext::from_window(window, cx);
    let rules = cx.update(|window, cx| {
        let canvas = shell.read(cx).rules_view().read(cx).focus_handle().clone();
        canvas.focus(window, cx);
        shell.read(cx).rules().clone()
    });
    redraw(&mut cx);
    (shell, rules, cx)
}

fn press(keys: &str, cx: &mut VisualTestContext) {
    cx.simulate_keystrokes(keys);
    redraw(cx);
}

/// The panel as `describe()` shows it, its keyboard cursor included.
fn panel(shell: &Entity<ShellView>, cx: &mut VisualTestContext) -> PanelDescription {
    describe_window(shell, cx).panel
}

/// The row the panel's keyboard cursor is on, if it has keyboard focus.
fn cursor(shell: &Entity<ShellView>, cx: &mut VisualTestContext) -> Option<&'static str> {
    panel(shell, cx).cursor
}

/// Presses Tab until the panel has focus: the cursor starts on the open
/// section, Rules.
fn tab_to_panel(shell: &Entity<ShellView>, cx: &mut VisualTestContext) {
    for _ in 0..40 {
        if cursor(shell, cx) == Some("Rules") {
            return;
        }
        press("tab", cx);
    }
    panic!("Tab never reached the Rules panel");
}

/// What's open and on (sections and filters), and the selected row: what
/// ↑↓ mustn't change.
fn opened(
    shell: &Entity<ShellView>,
    cx: &mut VisualTestContext,
) -> (Vec<&'static str>, Option<String>) {
    let shown = describe_window(shell, cx);
    let on = shown
        .panel
        .sections
        .iter()
        .chain(&shown.panel.filters)
        .filter(|row| row.selected)
        .map(|row| row.label)
        .collect();
    (on, shown.selected)
}

/// The labels of the rows that are on: the open section, then any filter.
fn on(shell: &Entity<ShellView>, cx: &mut VisualTestContext) -> Vec<&'static str> {
    opened(shell, cx).0
}

#[gpui_kit::test]
fn tab_reaches_the_panel_and_the_arrows_move_the_cursor_across_sections(cx: &mut TestAppContext) {
    let scratch = Scratch::new();
    let (shell, _, mut cx) = open(cx, &scratch);
    let cx = &mut cx;
    let before = opened(&shell, cx);
    assert_eq!(before.0, ["Rules"]);
    assert!(before.1.is_some(), "a rule is selected");
    assert_eq!(cursor(&shell, cx), None, "no cursor until focused");

    tab_to_panel(&shell, cx);
    press("down down down", cx);
    assert_eq!(cursor(&shell, cx), Some("Categories"));
    // On past the ACTION label to its first filter.
    press("down", cx);
    assert_eq!(
        cursor(&shell, cx),
        Some(text::action_filter_label(text::PANEL_ACTIONS[0])),
        "the label is skipped"
    );
    press("up up", cx);
    assert_eq!(cursor(&shell, cx), Some("Change history"));
    assert_eq!(
        opened(&shell, cx),
        before,
        "the arrows change nothing that's open, filtered or selected"
    );
}

#[gpui_kit::test]
fn enter_opens_a_section_and_space_toggles_a_filter(cx: &mut TestAppContext) {
    let scratch = Scratch::new();
    let (shell, rules, mut cx) = open(cx, &scratch);
    let cx = &mut cx;
    tab_to_panel(&shell, cx);

    // Enter on Merchant names opens it, as a click does; the cursor stays.
    press("down enter", cx);
    assert_eq!(on(&shell, cx), ["Merchant names"]);
    assert_eq!(cursor(&shell, cx), Some("Merchant names"));

    // On Categories, its KIND filters follow it.
    press("down down space", cx);
    let shown = panel(&shell, cx);
    assert_eq!(on(&shell, cx), ["Categories"]);
    assert_eq!(shown.filter_title, "Kind");
    press("down", cx);
    assert_eq!(cursor(&shell, cx), Some("Spending"));
    press("space", cx);
    assert_eq!(
        on(&shell, cx),
        ["Categories", "Spending"],
        "Space turns the filter on"
    );
    assert_eq!(
        cx.update(|_, cx| rules.read(cx).category_filter()),
        Some(CategoryFilter::Kind(text::CATEGORY_KINDS[0]))
    );
    assert_eq!(
        cursor(&shell, cx),
        Some("Spending"),
        "the cursor stays on the filter"
    );
    press("enter", cx);
    assert_eq!(
        on(&shell, cx),
        ["Categories"],
        "and Enter turns it off again"
    );

    // The last row is Hidden: the cursor stops there.
    press("down down down down down", cx);
    assert_eq!(cursor(&shell, cx), Some("Hidden"));
    assert_eq!(on(&shell, cx), ["Categories"], "moving toggles nothing");
}

#[gpui_kit::test]
fn an_action_filter_toggles_from_the_keyboard(cx: &mut TestAppContext) {
    let scratch = Scratch::new();
    let (shell, rules, mut cx) = open(cx, &scratch);
    let cx = &mut cx;
    tab_to_panel(&shell, cx);
    press("down down down down", cx);
    press("enter", cx);
    let first = text::action_filter_label(text::PANEL_ACTIONS[0]);
    assert_eq!(on(&shell, cx), ["Rules", first]);
    assert_eq!(
        cx.update(|_, cx| rules.read(cx).action_filter()),
        Some(text::PANEL_ACTIONS[0])
    );
    press("space", cx);
    assert_eq!(on(&shell, cx), ["Rules"]);
}
