//! Tests of the Rules edit pipeline ([`Rules::write`]) through the view's
//! own methods: where an error is said, what the undo bar offers again, and
//! which saves tell the shell [`RulesEvent::Changed`]. The database is the
//! scratch one from [`super::tests`].

use std::cell::RefCell;
use std::rc::Rc;

use coffer_core::change as core_change;
use coffer_core::id::{RuleId, TransactionId};
use coffer_core::reporting::register::Register;
use gpui_kit::{AppContext as _, Entity, TestAppContext, VisualTestContext};

use super::tests::{Scratch, open, redraw};
use super::write::NO_DATABASE;
use super::*;

/// Every event `rules` sends from now on.
fn capture(rules: &Entity<Rules>, cx: &mut VisualTestContext) -> Rc<RefCell<Vec<RulesEvent>>> {
    let events = Rc::new(RefCell::new(Vec::new()));
    let sink = events.clone();
    cx.update(|_, cx| {
        cx.subscribe(rules, move |_, event: &RulesEvent, _| {
            sink.borrow_mut().push(*event)
        })
        .detach()
    });
    events
}

/// The transactions `rule` filed, from the register.
fn filed_by(scratch: &Scratch, rule: RuleId) -> Vec<TransactionId> {
    Register::load(
        &scratch.db().reader().unwrap(),
        chrono::NaiveDate::from_ymd_opt(2026, 11, 14).unwrap(),
    )
    .unwrap()
    .rows()
    .iter()
    .filter(|row| row.rule == Some(rule))
    .map(|row| row.id)
    .collect()
}

/// Applies the DISHOOM rule to its one past row, with the bar dismissed
/// after; returns the batch.
fn apply_dishoom(
    scratch: &Scratch,
    rules: &Entity<Rules>,
    cx: &mut VisualTestContext,
) -> ChangeBatchId {
    let dishoom = scratch.seeded.rules[12];
    cx.update(|_, cx| {
        rules.update(cx, |rules, cx| {
            rules.select_rule(Some(dishoom), cx);
            rules.apply_selected(cx);
            rules.commit(cx);
        })
    });
    redraw(cx);
    let outcome = cx.update(|_, cx| rules.read(cx).outcome().cloned());
    let Some(Outcome::Done {
        undo: Some(batch), ..
    }) = outcome
    else {
        panic!("the apply offers Undo");
    };
    batch
}

#[gpui_kit::test]
fn deleting_a_rule_tells_the_shell_and_leaves_no_rule_on_its_rows(cx: &mut TestAppContext) {
    let scratch = Scratch::new();
    let (_, rules, cx) = open(cx, &scratch);
    let events = capture(&rules, cx);
    let (rule, filed) = scratch
        .seeded
        .rules
        .iter()
        .map(|rule| (*rule, filed_by(&scratch, *rule)))
        .find(|(_, filed)| !filed.is_empty())
        .expect("a rule that filed rows");
    assert!(!filed.is_empty());

    let deleted = cx.update(|_, cx| rules.update(cx, |rules, cx| rules.delete_rule(rule, cx)));
    redraw(cx);

    assert_eq!(deleted, Ok(()));
    assert_eq!(*events.borrow(), [RulesEvent::Changed]);
    assert!(
        filed_by(&scratch, rule).is_empty(),
        "the rows lose the rule"
    );
    assert!(cx.update(|_, cx| rules.read(cx).outcome().is_none()));
}

#[gpui_kit::test]
fn without_a_database_the_toolbar_says_so_and_a_dialog_is_only_told(cx: &mut TestAppContext) {
    let rules = cx.new(|_| Rules::empty());

    rules.update(cx, |rules, cx| rules.move_rule(RuleId::new(1), 0, cx));
    assert_eq!(
        rules.read_with(cx, |rules, _| rules.outcome().cloned()),
        Some(Outcome::Refused(NO_DATABASE.into())),
        "a toolbar write says it under the toolbar"
    );

    rules.update(cx, |rules, cx| rules.dismiss_outcome(cx));
    let deleted = rules.update(cx, |rules, cx| rules.delete_rule(RuleId::new(1), cx));
    assert_eq!(deleted, Err(NO_DATABASE.into()));
    let undone = rules.update(cx, |rules, cx| rules.undo_batch(ChangeBatchId::new(1), cx));
    assert_eq!(undone, Err(NO_DATABASE.into()));
    assert_eq!(
        rules.read_with(cx, |rules, _| rules.outcome().cloned()),
        None,
        "a dialog write says nothing under the toolbar"
    );
}

#[gpui_kit::test]
fn a_name_saved_without_a_database_is_refused_under_the_toolbar(cx: &mut TestAppContext) {
    let rules = cx.new(|_| Rules::empty());
    rules.update(cx, |rules, cx| {
        rules.new_name(cx);
        rules.edit_name(
            true,
            |form| {
                form.pattern = "ACME".into();
                form.name = "Acme".into();
            },
            cx,
        );
    });
    assert_eq!(
        rules.read_with(cx, |rules, _| rules.outcome().cloned()),
        Some(Outcome::Refused(NO_DATABASE.into()))
    );
}

#[gpui_kit::test]
fn the_undo_dialog_gets_the_error_and_the_toolbar_stays_quiet(cx: &mut TestAppContext) {
    let scratch = Scratch::new();
    let (_, rules, cx) = open(cx, &scratch);
    let batch = apply_dishoom(&scratch, &rules, cx);
    cx.update(|_, cx| rules.update(cx, |rules, cx| rules.dismiss_outcome(cx)));
    core_change::undo(&mut scratch.db(), batch).unwrap();
    let events = capture(&rules, cx);

    let result = cx.update(|_, cx| rules.update(cx, |rules, cx| rules.undo_batch(batch, cx)));

    assert!(result.is_err(), "an undone change can't be undone again");
    assert_eq!(cx.update(|_, cx| rules.read(cx).outcome().cloned()), None);
    assert!(events.borrow().is_empty());
}

#[gpui_kit::test]
fn the_bars_undo_that_fails_offers_undo_again_with_the_reason(cx: &mut TestAppContext) {
    let scratch = Scratch::new();
    let (_, rules, cx) = open(cx, &scratch);
    let batch = apply_dishoom(&scratch, &rules, cx);
    core_change::undo(&mut scratch.db(), batch).unwrap();

    cx.update(|_, cx| rules.update(cx, |rules, cx| rules.undo(cx)));

    let outcome = cx.update(|_, cx| rules.read(cx).outcome().cloned());
    let Some(Outcome::Done { message, undo }) = outcome else {
        panic!("the bar stays, offering Undo again");
    };
    assert!(message.starts_with("Couldn't undo: "), "{message}");
    assert_eq!(undo, Some(batch));
}

#[gpui_kit::test]
fn an_apply_tells_the_shell_but_moving_and_editing_a_rule_do_not(cx: &mut TestAppContext) {
    let scratch = Scratch::new();
    let (_, rules, cx) = open(cx, &scratch);
    let events = capture(&rules, cx);

    // Moving a rule changes no transaction.
    let uber = scratch.seeded.rules[1];
    cx.update(|_, cx| rules.update(cx, |rules, cx| rules.move_rule(uber, 0, cx)));
    assert_eq!(
        cx.update(|_, cx| rules.read(cx).data().position(uber)),
        Some(0)
    );
    assert!(events.borrow().is_empty(), "{:?}", events.borrow());

    // Nor does editing one.
    let pret = scratch.seeded.rules[3];
    cx.update(|_, cx| {
        rules.update(cx, |rules, cx| {
            rules.select_rule(Some(pret), cx);
            rules.edit(true, |form| form.text = "PRET".into(), cx);
        })
    });
    let stored = coffer_core::rule::get(&scratch.db(), pret).unwrap();
    assert_eq!(stored.definition().pattern().as_str(), "PRET");
    assert!(events.borrow().is_empty(), "{:?}", events.borrow());

    // An apply that changes a transaction does.
    apply_dishoom(&scratch, &rules, cx);
    assert_eq!(*events.borrow(), [RulesEvent::Changed]);
}

#[gpui_kit::test]
fn editing_a_merchant_name_tells_the_shell(cx: &mut TestAppContext) {
    let scratch = Scratch::new();
    let (_, rules, cx) = open(cx, &scratch);
    let events = capture(&rules, cx);
    cx.update(|_, cx| {
        rules.update(cx, |rules, cx| {
            rules.show(Section::Names, cx);
            rules.edit_name(true, |form| form.name = "Renamed".into(), cx);
        })
    });

    assert_eq!(*events.borrow(), [RulesEvent::Changed]);
    let shown = cx.update(|_, cx| {
        rules
            .read(cx)
            .names()
            .iter()
            .any(|name| name.alias.definition().name().as_str() == "Renamed")
    });
    assert!(shown, "the view read the name again");
    let (selected, form) = cx.update(|_, cx| {
        let rules = rules.read(cx);
        (
            rules.selected_name(),
            rules.name_form().map(|form| form.name.clone()),
        )
    });
    assert!(
        matches!(selected, Some(NameSelection::Stored(_))),
        "the saved name stays selected: {selected:?}"
    );
    assert_eq!(form.as_deref(), Some("Renamed"), "with its fields");
}

#[gpui_kit::test]
fn moving_a_rule_keeps_what_was_typed_for_the_selected_one(cx: &mut TestAppContext) {
    let scratch = Scratch::new();
    let (_, rules, cx) = open(cx, &scratch);
    let pret = scratch.seeded.rules[3];
    let uber = scratch.seeded.rules[1];
    cx.update(|_, cx| {
        rules.update(cx, |rules, cx| {
            rules.select_rule(Some(pret), cx);
            // Typed, not yet blurred: not saved.
            rules.edit(false, |form| form.text = "PRET".into(), cx);
            rules.move_rule(uber, 0, cx);
            rules.select_rule(Some(uber), cx);
        })
    });

    let stored = coffer_core::rule::get(&scratch.db(), pret).unwrap();
    assert_eq!(stored.definition().pattern().as_str(), "PRET");
    assert_eq!(
        cx.update(|_, cx| rules.read(cx).data().position(uber)),
        Some(0)
    );
}
