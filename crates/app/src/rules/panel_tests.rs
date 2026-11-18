//! Tests of the Rules panel's keyboard cursor (ROADMAP U.9; `DESIGN.md` →
//! "Keyboard paths"): Tab reaches the panel, ↑↓ move the cursor through the
//! sections and the open section's filters without changing what's open,
//! and Enter or Space act on the cursor row as a click does. On the scratch
//! database from [`super::tests`], with the keys pressed as the user would.

use gpui_kit::test::TestWindowExt as _;
use gpui_kit::{
    AppContext as _, Entity, SharedString, TestAppContext, VisualTestContext, WindowOptions,
};

use super::store::RulesStore;
use super::tests::{Scratch, redraw};
use super::*;
use crate::accounts::{Accounts, fixture as accounts_fixture};
use crate::inbox::Inbox;
use crate::shell::{ShellView, View};
use crate::theme::focus::RING_SELECTOR;
use crate::transactions::{self, store::TransactionStore};

/// The shell on Rules, as [`super::tests::open`] opens it, but in a real
/// window with `Root` (which binds Tab), focus on the Rules canvas.
fn open(cx: &mut TestAppContext, scratch: &Scratch) -> (Entity<Rules>, VisualTestContext) {
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
    (rules, cx)
}

fn press(keys: &str, cx: &mut VisualTestContext) {
    cx.simulate_keystrokes(keys);
    redraw(cx);
}

/// Whether the focus ring is drawn on the row `id`.
fn ring_on(id: &str, cx: &mut VisualTestContext) -> bool {
    let Some(ring) = cx.debug_bounds(RING_SELECTOR) else {
        return false;
    };
    let id = SharedString::from(id.to_owned());
    cx.update(move |window, _| window.try_find(id).map(|row| row.bounds())) == Some(ring)
}

/// Presses Tab until the panel has focus: the cursor starts on the open
/// section, Rules.
fn tab_to_panel(cx: &mut VisualTestContext) {
    for _ in 0..40 {
        if ring_on("rules-section-rules", cx) {
            return;
        }
        press("tab", cx);
    }
    panic!("Tab never reached the Rules panel");
}

/// What's open, filtered and selected: what ↑↓ mustn't change.
fn opened(
    rules: &Entity<Rules>,
    cx: &mut VisualTestContext,
) -> (
    Section,
    Option<ActionKind>,
    Option<CategoryFilter>,
    Option<RuleId>,
) {
    cx.update(|_, cx| {
        let rules = rules.read(cx);
        (
            rules.section(),
            rules.action_filter(),
            rules.category_filter(),
            rules.selected_rule_id(),
        )
    })
}

#[gpui_kit::test]
fn tab_reaches_the_panel_and_the_arrows_move_the_cursor_across_sections(cx: &mut TestAppContext) {
    let scratch = Scratch::new();
    let (rules, mut cx) = open(cx, &scratch);
    let cx = &mut cx;
    let before = opened(&rules, cx);
    assert!(before.3.is_some(), "a rule is selected");
    assert!(
        !ring_on("rules-section-rules", cx),
        "no cursor until focused"
    );

    tab_to_panel(cx);
    press("down down down", cx);
    assert!(ring_on("rules-section-categories", cx));
    // On past the ACTION label to its first filter.
    press("down", cx);
    let first = format!("rules-action-{}", text::PANEL_ACTIONS[0].as_str());
    assert!(ring_on(&first, cx), "the label is skipped");
    press("up up", cx);
    assert!(ring_on("rules-section-history", cx));
    assert_eq!(
        opened(&rules, cx),
        before,
        "the arrows change nothing that's open, filtered or selected"
    );
}

#[gpui_kit::test]
fn enter_opens_a_section_and_space_toggles_a_filter(cx: &mut TestAppContext) {
    let scratch = Scratch::new();
    let (rules, mut cx) = open(cx, &scratch);
    let cx = &mut cx;
    tab_to_panel(cx);

    // Enter on Merchant names opens it, as a click does; the cursor stays.
    press("down enter", cx);
    assert_eq!(opened(&rules, cx).0, Section::Names);
    assert!(ring_on("rules-section-names", cx));

    // On Categories, its KIND filters follow it.
    press("down down space", cx);
    assert_eq!(opened(&rules, cx).0, Section::Categories);
    press("down", cx);
    let kind = text::CATEGORY_KINDS[0];
    let id = format!("rules-category-kind-{}", kind.as_str());
    assert!(ring_on(&id, cx));
    press("space", cx);
    assert_eq!(
        opened(&rules, cx).2,
        Some(CategoryFilter::Kind(kind)),
        "Space turns the filter on"
    );
    assert!(ring_on(&id, cx), "the cursor stays on the filter");
    press("enter", cx);
    assert_eq!(opened(&rules, cx).2, None, "and Enter turns it off again");

    // The last row is Hidden: the cursor stops there.
    press("down down down down down", cx);
    assert!(ring_on("rules-category-hidden", cx));
    assert_eq!(opened(&rules, cx).2, None, "moving toggles nothing");
}

#[gpui_kit::test]
fn an_action_filter_toggles_from_the_keyboard(cx: &mut TestAppContext) {
    let scratch = Scratch::new();
    let (rules, mut cx) = open(cx, &scratch);
    let cx = &mut cx;
    tab_to_panel(cx);
    press("down down down down", cx);
    press("enter", cx);
    assert_eq!(opened(&rules, cx).1, Some(text::PANEL_ACTIONS[0]));
    press("space", cx);
    assert_eq!(opened(&rules, cx).1, None);
}
