//! Tests of reordering rules from the keyboard (ROADMAP U.8; `DESIGN.md` →
//! "Rules" → "Keyboard reorder"): ⌥↑⌥↓ on the selected rule while the table
//! has focus, through the drop's write, with the undo bar saying where the
//! rule went. On the scratch database from [`super::tests`], with the keys
//! pressed as the user would, and what the view shows read through its
//! test-only `describe()`.

use coffer_core::id::RuleId;
use coffer_core::rule as core_rule;
use gpui_kit::test::TestWindowExt as _;
use gpui_kit::{Entity, TestAppContext, VisualTestContext};

use super::describe::{Action, BarDescription, RulesDescription};
use super::tests::{Scratch, describe, item, open, redraw, rule_rows};
use super::view::Input;
use super::*;
use crate::shell::ShellView;
use crate::theme::focus::RING_SELECTOR;
use crate::theme::tokens::size;

/// The fixture's rule `n` (0 first): 1 UBER, 2 UBER TRIP, 5 WAITROSE, 11
/// JUSTGIVING, 13 the rent (last).
fn rule(scratch: &Scratch, n: usize) -> RuleId {
    scratch.seeded.rules[n]
}

/// The order the database keeps.
fn stored_order(scratch: &Scratch) -> Vec<RuleId> {
    core_rule::list(&scratch.db())
        .unwrap()
        .iter()
        .map(|rule| rule.id())
        .collect()
}

/// Where `id` is in the database's order (0 first).
fn stored_at(scratch: &Scratch, id: RuleId) -> usize {
    stored_order(scratch)
        .iter()
        .position(|rule| *rule == id)
        .unwrap()
}

/// Selects `id` and puts focus on the table, as clicking its row does.
fn select(
    shell: &Entity<ShellView>,
    rules: &Entity<Rules>,
    id: RuleId,
    cx: &mut VisualTestContext,
) {
    cx.update(|_, cx| rules.update(cx, |rules, cx| rules.select_rule(Some(id), cx)));
    focus_table(shell, cx);
}

fn focus_table(shell: &Entity<ShellView>, cx: &mut VisualTestContext) {
    cx.update(|window, cx| {
        let view = shell.read(cx).rules_view().clone();
        let focus = view.read(cx).focus_handle().clone();
        focus.focus(window, cx);
    });
    redraw(cx);
}

fn press(keys: &str, cx: &mut VisualTestContext) {
    cx.simulate_keystrokes(keys);
    redraw(cx);
}

fn outcome(rules: &Entity<Rules>, cx: &mut VisualTestContext) -> Option<Outcome> {
    cx.update(|_, cx| rules.read(cx).outcome().cloned())
}

/// The bar `describe()` lists after a move: the line, with Undo.
fn moved(line: &str) -> Option<BarDescription> {
    Some(BarDescription {
        message: line.to_owned(),
        undo: true,
    })
}

fn selected(rules: &Entity<Rules>, cx: &mut VisualTestContext) -> Option<RuleId> {
    cx.update(|_, cx| rules.read(cx).selected_rule_id())
}

fn shows(id: &str, cx: &mut VisualTestContext) -> bool {
    let id = gpui_kit::SharedString::from(id.to_owned());
    cx.update(move |window, _| window.try_find(id).is_some())
}

/// Whether `describe()` says ⌥↑ (`down` false) or ⌥↓ would move the
/// selected rule now.
fn can_move(shown: &RulesDescription, down: bool) -> bool {
    let key = if down { "alt-down" } else { "alt-up" };
    shown
        .actions
        .iter()
        .find(|action| action.key == Some(key))
        .is_some_and(|action: &Action| action.enabled)
}

#[gpui_kit::test]
fn alt_up_moves_the_selected_rule_and_the_bar_says_where(cx: &mut TestAppContext) {
    let scratch = Scratch::new();
    let (shell, rules, cx) = open(cx, &scratch);
    let trip = rule(&scratch, 2);
    select(&shell, &rules, trip, cx);

    press("alt-up", cx);
    assert_eq!(stored_at(&scratch, trip), 1, "above UBER");
    assert_eq!(
        outcome(&rules, cx),
        Some(Outcome::Moved(Moved {
            rule: trip,
            from: 2
        }))
    );
    let shown = describe(&shell, cx);
    assert_eq!(
        shown.bar,
        moved("Moved “Merchant contains UBER TRIP” to rule 2 of 14")
    );
    assert!(shows("undo-change", cx), "the bar offers Undo");
    // The moved row stays selected, renumbered, and keeps focus, with the
    // ring; the inspector follows it.
    assert_eq!(selected(&rules, cx), Some(trip));
    let row = &rule_rows(&shown)[1];
    assert_eq!(
        (row.number, row.when.as_str(), row.selected),
        (2, "Merchant contains UBER TRIP", true)
    );
    assert_eq!(
        item(&shown)
            .caption
            .as_deref()
            .map(|c| c.ends_with("rule 2 of 14")),
        Some(true)
    );
    let ring = cx
        .debug_bounds(RING_SELECTOR)
        .expect("the selected row draws the focus ring");
    assert_eq!(ring.size.height, size::CONTROL, "on the 28px row");
    // It decides UBER TRIP's rows now (the inspector follows).
    assert!(
        !row.never_reached && row.matches != "0",
        "no longer never reached"
    );
    assert_eq!(item(&shown).never_reached, None);

    // Pressed again, the same bar says the new place.
    press("alt-up", cx);
    assert_eq!(stored_at(&scratch, trip), 0);
    let shown = describe(&shell, cx);
    assert_eq!(
        shown.bar,
        moved("Moved “Merchant contains UBER TRIP” to rule 1 of 14")
    );
    // At the top, ⌥↑ has nothing to do: pressed, the bar stays as it was.
    assert!(!can_move(&shown, false) && can_move(&shown, true));
    press("alt-up", cx);
    assert_eq!(stored_at(&scratch, trip), 0);
    assert_eq!(
        outcome(&rules, cx),
        Some(Outcome::Moved(Moved {
            rule: trip,
            from: 2
        }))
    );

    // One Undo returns it to where the presses began.
    let id = gpui_kit::SharedString::from("undo-change");
    cx.update(move |window, cx| window.click(id, cx));
    redraw(cx);
    assert_eq!(
        stored_order(&scratch),
        scratch.seeded.rules,
        "back as seeded"
    );
    let shown = describe(&shell, cx);
    assert_eq!(shown.bar, None, "the bar goes");
    assert_eq!(
        shown.selected.as_deref(),
        Some("Merchant contains UBER TRIP"),
        "still selected"
    );
    assert_eq!(rule_rows(&shown)[2].number, 3);
}

#[gpui_kit::test]
fn alt_down_moves_down_and_the_edges_do_nothing(cx: &mut TestAppContext) {
    let scratch = Scratch::new();
    let (shell, rules, cx) = open(cx, &scratch);
    let uber = rule(&scratch, 1);
    select(&shell, &rules, uber, cx);
    press("alt-down", cx);
    assert_eq!(stored_at(&scratch, uber), 2, "below UBER TRIP");
    assert_eq!(
        describe(&shell, cx).bar,
        moved("Moved “Merchant contains UBER” to rule 3 of 14")
    );

    // ⌥↓ on the last rule and ⌥↑ on the first: nothing moves, no bar.
    let first = stored_order(&scratch)[0];
    let last = rule(&scratch, 13);
    select(&shell, &rules, last, cx);
    cx.update(|_, cx| rules.update(cx, |rules, cx| rules.dismiss_outcome(cx)));
    let before = stored_order(&scratch);
    let shown = describe(&shell, cx);
    assert!(
        can_move(&shown, false) && !can_move(&shown, true),
        "the last rule"
    );
    press("alt-down", cx);
    select(&shell, &rules, first, cx);
    let shown = describe(&shell, cx);
    assert!(
        !can_move(&shown, false) && can_move(&shown, true),
        "the first rule"
    );
    press("alt-up", cx);
    assert_eq!(stored_order(&scratch), before);
    assert_eq!(describe(&shell, cx).bar, None, "no bar at the edges");
}

#[gpui_kit::test]
fn another_rule_or_another_change_starts_a_new_bar(cx: &mut TestAppContext) {
    let scratch = Scratch::new();
    let (shell, rules, cx) = open(cx, &scratch);
    let (uber, trip) = (rule(&scratch, 1), rule(&scratch, 2));
    select(&shell, &rules, trip, cx);
    press("alt-up", cx);
    // ↓ selects UBER (now rule 3); moving it starts its own bar.
    press("down", cx);
    assert_eq!(selected(&rules, cx), Some(uber));
    press("alt-down", cx);
    assert_eq!(
        outcome(&rules, cx),
        Some(Outcome::Moved(Moved {
            rule: uber,
            from: 2
        }))
    );
    // Its Undo moves only UBER back: UBER TRIP stays above it.
    cx.update(|_, cx| rules.update(cx, |rules, cx| rules.undo(cx)));
    redraw(cx);
    let order = stored_order(&scratch);
    assert_eq!((order[1], order[2]), (trip, uber));

    // Any other change (here a drop) takes the bar away, so the next press
    // starts from where the rule is then.
    select(&shell, &rules, trip, cx);
    press("alt-down", cx);
    let tfl = rule(&scratch, 0);
    cx.update(|_, cx| rules.update(cx, |rules, cx| rules.move_rule(tfl, 1, cx)));
    assert_eq!(outcome(&rules, cx), None);
    focus_table(&shell, cx);
    let at = stored_at(&scratch, trip);
    press("alt-up", cx);
    assert_eq!(
        outcome(&rules, cx),
        Some(Outcome::Moved(Moved {
            rule: trip,
            from: at
        }))
    );
}

#[gpui_kit::test]
fn with_a_filter_on_the_rule_jumps_past_hidden_rows(cx: &mut TestAppContext) {
    let scratch = Scratch::new();
    let (shell, rules, cx) = open(cx, &scratch);
    let (waitrose, justgiving) = (rule(&scratch, 5), rule(&scratch, 11));
    cx.update(|_, cx| {
        rules.update(cx, |rules, cx| {
            rules.toggle_action_filter(ActionKind::AddTag, cx)
        })
    });
    select(&shell, &rules, justgiving, cx);

    press("alt-up", cx);
    let order = stored_order(&scratch);
    assert_eq!(
        (order[5], order[6]),
        (justgiving, waitrose),
        "just above WAITROSE"
    );
    let shown = describe(&shell, cx);
    assert_eq!(
        shown.bar,
        moved("Moved “Merchant contains JUSTGIVING” to rule 6 of 14")
    );
    let numbers: Vec<usize> = rule_rows(&shown).iter().map(|row| row.number).collect();
    assert_eq!(numbers, [6, 7], "the filter's rows, renumbered");
    // It's the first row shown now: ⌥↑ does nothing more.
    assert!(!can_move(&shown, false));
    press("alt-up", cx);
    assert_eq!(stored_at(&scratch, justgiving), 5);
}

#[gpui_kit::test]
fn the_keys_are_ignored_in_the_inspector_and_for_a_new_rule(cx: &mut TestAppContext) {
    let scratch = Scratch::new();
    let (shell, rules, cx) = open(cx, &scratch);
    let trip = rule(&scratch, 2);
    select(&shell, &rules, trip, cx);

    // In the Text field, ⌥↑ stays with the field.
    cx.update(|window, cx| {
        let view = shell.read(cx).rules_view().clone();
        let field = view.read(cx).input(Input::Pattern).unwrap().clone();
        field.update(cx, |field, cx| field.focus(window, cx));
    });
    redraw(cx);
    press("alt-up", cx);
    assert_eq!(stored_order(&scratch), scratch.seeded.rules);
    assert_eq!(outcome(&rules, cx), None);

    // A new rule has the Position select instead.
    cx.update(|_, cx| rules.update(cx, |rules, cx| rules.new_rule(cx)));
    focus_table(&shell, cx);
    let shown = describe(&shell, cx);
    assert!(!can_move(&shown, false) && !can_move(&shown, true));
    press("alt-up alt-down", cx);
    assert_eq!(stored_order(&scratch), scratch.seeded.rules);
    assert!(cx.update(|_, cx| rules.read(cx).is_new()));
}

#[gpui_kit::test]
fn a_refused_move_is_said_as_a_drops_is(cx: &mut TestAppContext) {
    let scratch = Scratch::new();
    let (shell, rules, cx) = open(cx, &scratch);
    let trip = rule(&scratch, 2);
    select(&shell, &rules, trip, cx);
    // Deleted behind the view's back: core refuses to move it.
    core_rule::delete(&mut scratch.db(), trip).unwrap();

    press("alt-up", cx);
    let said = outcome(&rules, cx);
    assert!(
        matches!(said, Some(Outcome::Refused(_))),
        "said under the toolbar: {said:?}"
    );
    let shown = describe(&shell, cx);
    assert!(shown.refused.is_some(), "said under the toolbar");
    assert_eq!(shown.bar, None, "no bar");
    assert!(!shows("undo-change", cx), "no bar");

    // The drop says the same.
    cx.update(|_, cx| rules.update(cx, |rules, cx| rules.move_rule(trip, 0, cx)));
    assert_eq!(outcome(&rules, cx), said);
}

#[gpui_kit::test]
fn a_reload_that_finds_another_order_drops_the_bar(cx: &mut TestAppContext) {
    let scratch = Scratch::new();
    let (shell, rules, cx) = open(cx, &scratch);
    let trip = rule(&scratch, 2);
    select(&shell, &rules, trip, cx);
    press("alt-up", cx);
    assert!(matches!(outcome(&rules, cx), Some(Outcome::Moved(_))));

    // Read again with nothing changed (back from another view): the bar
    // stays, and Undo still knows where the rule began.
    cx.update(|_, cx| rules.update(cx, |rules, cx| rules.reload(cx)));
    assert!(matches!(outcome(&rules, cx), Some(Outcome::Moved(_))));

    // Moved elsewhere (another rule put first, as an approved suggestion
    // could): the place Undo would use is stale, so the bar goes.
    let last = rule(&scratch, 13);
    core_rule::move_to(&mut scratch.db(), last, 0).unwrap();
    cx.update(|_, cx| rules.update(cx, |rules, cx| rules.reload(cx)));
    assert_eq!(outcome(&rules, cx), None);
    assert_eq!(describe(&shell, cx).bar, None);
}
