//! Tests of the Rules edit pipeline ([`Rules::write`]) through the view's
//! own methods: where an error is said, what the undo bar offers again, and
//! which saves tell the shell [`RulesEvent::Changed`]. The database is the
//! scratch one from [`super::tests`]; what the view says is read through its
//! test-only `describe()`.

use std::cell::RefCell;
use std::rc::Rc;

use gpui_kit::{AppContext as _, Entity, TestAppContext, VisualTestContext};
use orre_core::change as core_change;
use orre_core::id::{RuleId, TransactionId};
use orre_core::reporting::register::Register;
use orre_core::rule::{self as core_rule, MatchKind};

use super::describe::{BarDescription, CanvasDescription};
use super::tests::{Scratch, describe, item, open, redraw};
use super::view::Input;
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

/// Whether a read is in flight: asked for, not landed.
fn reading(rules: &Entity<Rules>, cx: &mut VisualTestContext) -> bool {
    cx.update(|_, cx| rules.read(cx).loading.is_some())
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
    let (shell, rules, cx) = open(cx, &scratch);
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
    let shown = describe(&shell, cx);
    assert_eq!(
        (shown.bar, shown.refused),
        (None, None),
        "a delete says nothing after"
    );
    assert_eq!(shown.panel.sections[0].count, 13);
}

#[gpui_kit::test]
fn without_a_database_the_toolbar_says_so_and_a_dialog_is_only_told(cx: &mut TestAppContext) {
    let rules = cx.new(|_| Rules::empty());

    rules.update(cx, |rules, cx| rules.move_rule(RuleId::new(1), 0, cx));
    let shown = rules.read_with(cx, |rules, _| rules.describe());
    assert_eq!(
        shown.refused.as_deref(),
        Some(NO_DATABASE),
        "a toolbar write says it under the toolbar"
    );
    assert_eq!(shown.bar, None);

    rules.update(cx, |rules, cx| rules.dismiss_outcome(cx));
    let deleted = rules.update(cx, |rules, cx| rules.delete_rule(RuleId::new(1), cx));
    assert_eq!(deleted, Err(NO_DATABASE.into()));
    let undone = rules.update(cx, |rules, cx| rules.undo_batch(ChangeBatchId::new(1), cx));
    assert_eq!(undone, Err(NO_DATABASE.into()));
    assert_eq!(
        rules.read_with(cx, |rules, _| rules.describe().refused),
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
    let shown = rules.read_with(cx, |rules, _| rules.describe());
    assert_eq!(shown.refused.as_deref(), Some(NO_DATABASE));
    assert_eq!(
        item(&shown).caption.as_deref(),
        Some("new name, saved once both fields are filled in"),
        "still the new name"
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
    let (shell, rules, cx) = open(cx, &scratch);
    let batch = apply_dishoom(&scratch, &rules, cx);
    core_change::undo(&mut scratch.db(), batch).unwrap();

    cx.update(|_, cx| rules.update(cx, |rules, cx| rules.undo(cx)));

    let shown = describe(&shell, cx);
    let Some(BarDescription {
        message,
        undo: true,
    }) = shown.bar
    else {
        panic!("the bar stays, offering Undo again: {:?}", shown.bar);
    };
    assert!(message.starts_with("Couldn't undo: "), "{message}");
    assert_eq!(
        shown.refused, None,
        "said in the bar, not under the toolbar"
    );
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
    let stored = orre_core::rule::get(&scratch.db(), pret).unwrap();
    assert_eq!(stored.definition().pattern().as_str(), "PRET");
    assert!(events.borrow().is_empty(), "{:?}", events.borrow());

    // An apply that changes a transaction does.
    apply_dishoom(&scratch, &rules, cx);
    assert_eq!(*events.borrow(), [RulesEvent::Changed]);
}

#[gpui_kit::test]
fn editing_a_merchant_name_tells_the_shell(cx: &mut TestAppContext) {
    let scratch = Scratch::new();
    let (shell, rules, cx) = open(cx, &scratch);
    let events = capture(&rules, cx);
    cx.update(|_, cx| {
        rules.update(cx, |rules, cx| {
            rules.show(Section::Names, cx);
            rules.edit_name(true, |form| form.name = "Renamed".into(), cx);
        })
    });
    redraw(cx);

    assert_eq!(*events.borrow(), [RulesEvent::Changed]);
    let shown = describe(&shell, cx);
    let CanvasDescription::Names(names) = &shown.canvas else {
        panic!("the names table: {:?}", shown.canvas);
    };
    let renamed: Vec<bool> = names
        .iter()
        .filter(|name| name.shows_as == "Renamed")
        .map(|name| name.selected)
        .collect();
    assert_eq!(
        renamed,
        [true],
        "the view read the name again, and it stays selected"
    );
    assert_eq!(item(&shown).title, "Renamed", "with its fields");
    let form = cx.update(|_, cx| rules.read(cx).name_form().map(|form| form.name.clone()));
    assert_eq!(form.as_deref(), Some("Renamed"));
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

    let stored = orre_core::rule::get(&scratch.db(), pret).unwrap();
    assert_eq!(stored.definition().pattern().as_str(), "PRET");
    assert_eq!(
        cx.update(|_, cx| rules.read(cx).data().position(uber)),
        Some(0)
    );
}

#[gpui_kit::test]
fn a_read_that_lands_keeps_what_was_typed_since_it_was_asked_for(cx: &mut TestAppContext) {
    let scratch = Scratch::new();
    let (shell, rules, cx) = open(cx, &scratch);
    let pret = scratch.seeded.rules[3];
    cx.update(|_, cx| rules.update(cx, |rules, cx| rules.select_rule(Some(pret), cx)));
    redraw(cx);
    let view = cx.update(|_, cx| shell.read(cx).rules_view().clone());

    // A select saves at once and asks for a read; before it lands, a valid
    // pattern is typed and not saved.
    cx.update(|_, cx| {
        rules.update(cx, |rules, cx| {
            rules.edit(true, |form| form.kind = MatchKind::StartsWith, cx)
        })
    });
    cx.update(|window, cx| {
        view.update(cx, |view, cx| {
            view.type_into(Input::Pattern, "PRET", window, cx)
        })
    });
    assert!(reading(&rules, cx));
    redraw(cx);

    let text = cx.update(|_, cx| rules.read(cx).form().map(|form| form.text.clone()));
    assert_eq!(text.as_deref(), Some("PRET"), "the read didn't refill it");
    let stored = orre_core::rule::get(&scratch.db(), pret).unwrap();
    assert_eq!(
        (
            stored.definition().pattern().kind(),
            stored.definition().pattern().as_str()
        ),
        (MatchKind::StartsWith, "PRET A MANGER"),
        "the select saved; the typing didn't"
    );

    // The same when the shell asks for a read before the save's lands: the
    // newer read stands in for the older, and the typing came after that.
    cx.update(|_, cx| {
        rules.update(cx, |rules, cx| {
            rules.edit(true, |form| form.kind = MatchKind::Contains, cx)
        })
    });
    cx.update(|window, cx| {
        view.update(cx, |view, cx| {
            view.type_into(Input::Pattern, "PRET A", window, cx)
        })
    });
    cx.update(|_, cx| rules.update(cx, |rules, cx| rules.reload(cx)));
    redraw(cx);
    let text = cx.update(|_, cx| rules.read(cx).form().map(|form| form.text.clone()));
    assert_eq!(text.as_deref(), Some("PRET A"));
}

#[gpui_kit::test]
fn a_save_while_its_read_is_pending_is_not_made_again(cx: &mut TestAppContext) {
    let scratch = Scratch::new();
    let (shell, rules, cx) = open(cx, &scratch);
    let events = capture(&rules, cx);

    // A name: saved once, then the same save again (a blur, a selection
    // moving) before the read lands is skipped, so the shell isn't told
    // twice. Typed back to what it was, it saves again.
    let original = cx.update(|_, cx| {
        rules.update(cx, |rules, cx| {
            rules.show(Section::Names, cx);
            let original = rules.name_form().map(|form| form.name.clone());
            rules.edit_name(true, |form| form.name = "Renamed".into(), cx);
            rules.edit_name(true, |_| {}, cx);
            original.expect("the first name opens")
        })
    });
    assert_eq!(*events.borrow(), [RulesEvent::Changed]);
    assert!(reading(&rules, cx));
    cx.update(|_, cx| {
        rules.update(cx, |rules, cx| {
            rules.edit_name(true, |form| form.name = original.clone(), cx)
        })
    });
    assert_eq!(*events.borrow(), [RulesEvent::Changed, RulesEvent::Changed]);
    redraw(cx);
    assert_eq!(item(&describe(&shell, cx)).title, original);

    // A rule: saved, then deleted behind the view's back; saving the same
    // again before the read lands would be refused, so it wasn't tried.
    let pret = scratch.seeded.rules[3];
    cx.update(|_, cx| {
        rules.update(cx, |rules, cx| {
            rules.select_rule(Some(pret), cx);
            rules.edit(true, |form| form.kind = MatchKind::StartsWith, cx);
        })
    });
    core_rule::delete(&mut scratch.db(), pret).unwrap();
    assert!(reading(&rules, cx));
    cx.update(|_, cx| rules.update(cx, |rules, cx| rules.save_rule(cx)));
    assert_eq!(cx.update(|_, cx| rules.read(cx).outcome().cloned()), None);

    // A category's name, likewise: one rename, told once.
    events.borrow_mut().clear();
    cx.update(|_, cx| {
        rules.update(cx, |rules, cx| {
            rules.show(Section::Categories, cx);
            rules.edit_category(true, |form| form.name = "Food shop".into(), cx);
            rules.save_category_name(cx);
        })
    });
    assert_eq!(*events.borrow(), [RulesEvent::Changed]);
    assert!(reading(&rules, cx));
    redraw(cx);
    assert_eq!(*events.borrow(), [RulesEvent::Changed]);
}

#[gpui_kit::test]
fn reopening_a_rule_before_its_read_lands_keeps_what_was_saved(cx: &mut TestAppContext) {
    let scratch = Scratch::new();
    let (_shell, rules, cx) = open(cx, &scratch);
    let pret = scratch.seeded.rules[3];
    let other = scratch.seeded.rules[4];
    cx.update(|_, cx| rules.update(cx, |rules, cx| rules.select_rule(Some(pret), cx)));
    redraw(cx);
    cx.update(|_, cx| {
        rules.update(cx, |rules, cx| {
            rules.edit(true, |form| form.kind = MatchKind::StartsWith, cx)
        })
    });
    // Away and back, then away again, before the read lands: the form opens
    // as saved, so leaving it doesn't save the older rule back.
    let reopened = cx.update(|_, cx| {
        rules.update(cx, |rules, cx| {
            rules.select_rule(Some(other), cx);
            rules.select_rule(Some(pret), cx);
            let kind = rules.form().map(|form| form.kind);
            rules.select_rule(Some(other), cx);
            kind
        })
    });
    assert!(reading(&rules, cx));
    assert_eq!(reopened, Some(MatchKind::StartsWith));
    let stored = core_rule::get(&scratch.db(), pret).unwrap();
    assert_eq!(stored.definition().pattern().kind(), MatchKind::StartsWith);
}

#[gpui_kit::test]
fn open_rule_on_a_rule_made_elsewhere_selects_it_when_the_read_lands(cx: &mut TestAppContext) {
    let scratch = Scratch::new();
    let (shell, rules, cx) = open(cx, &scratch);
    cx.update(|window, cx| {
        shell.update(cx, |shell, cx| {
            shell.show(crate::shell::View::Transactions, window, cx)
        })
    });
    redraw(cx);
    // A rule made outside the Rules view (an approved Inbox suggestion).
    let proposal = core_rule::propose(&scratch.db(), scratch.seeded.vinted)
        .unwrap()
        .expect("VINTED proposes a rule");
    let made = core_rule::create(&mut scratch.db(), &proposal.definition).unwrap();
    let transactions = cx.update(|_, cx| shell.read(cx).transactions().clone());
    cx.update(|_, cx| {
        transactions.update(cx, |_, cx| {
            cx.emit(crate::transactions::TransactionsEvent::OpenRule(made))
        })
    });
    redraw(cx);
    assert!(cx.update(|_, cx| shell.read(cx).view() == crate::shell::View::Rules));
    assert_eq!(
        cx.update(|_, cx| rules.read(cx).selected_rule_id()),
        Some(made)
    );
}

#[gpui_kit::test]
fn reopening_a_name_or_category_before_its_read_lands_keeps_what_was_saved(
    cx: &mut TestAppContext,
) {
    let scratch = Scratch::new();
    let (_shell, rules, cx) = open(cx, &scratch);
    let events = capture(&rules, cx);
    let reopened = cx.update(|_, cx| {
        rules.update(cx, |rules, cx| {
            rules.show(Section::Names, cx);
            let ids: Vec<_> = rules.names().iter().map(|n| n.alias.id()).collect();
            let (first, second) = (ids[0], ids[1]);
            assert_eq!(rules.selected_name(), Some(NameSelection::Stored(first)));
            rules.edit_name(true, |form| form.name = "Renamed".into(), cx);
            rules.select_name(Some(NameSelection::Stored(second)), cx);
            rules.select_name(Some(NameSelection::Stored(first)), cx);
            let name = rules.name_form().map(|form| form.name.clone());
            rules.select_name(Some(NameSelection::Stored(second)), cx);
            name
        })
    });
    assert!(reading(&rules, cx));
    assert_eq!(
        reopened.as_deref(),
        Some("Renamed"),
        "the name form opens as saved"
    );
    assert_eq!(
        *events.borrow(),
        [RulesEvent::Changed],
        "the name is saved once"
    );
    redraw(cx);
    events.borrow_mut().clear();

    let (id, reopened) = cx.update(|_, cx| {
        rules.update(cx, |rules, cx| {
            rules.show(Section::Categories, cx);
            let ids: Vec<i64> = rules
                .category_rows()
                .iter()
                .map(|row| row.summary.category.id())
                .collect();
            let Some(CategorySelection::Stored(first)) = rules.selected_category() else {
                panic!("a category is selected")
            };
            let other = *ids.iter().find(|id| **id != first).unwrap();
            rules.edit_category(true, |form| form.name = "Food shop".into(), cx);
            rules.select_category(Some(CategorySelection::Stored(other)), cx);
            rules.select_category(Some(CategorySelection::Stored(first)), cx);
            let name = rules.category_form().map(|form| form.name.clone());
            rules.select_category(Some(CategorySelection::Stored(other)), cx);
            (first, name)
        })
    });
    assert!(reading(&rules, cx));
    assert_eq!(
        reopened.as_deref(),
        Some("Food shop"),
        "the category form opens as saved"
    );
    assert_eq!(
        *events.borrow(),
        [RulesEvent::Changed],
        "the category is saved once"
    );
    let label = scratch
        .db()
        .categories()
        .unwrap()
        .iter()
        .find(|category| category.id() == id)
        .map(|category| category.label().to_owned());
    assert_eq!(
        label.as_deref(),
        Some("Food shop"),
        "the stored label stays"
    );
}
