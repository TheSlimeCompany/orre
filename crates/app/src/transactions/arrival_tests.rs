//! Tests of what a read from another process shows (ROADMAP LU.5): the
//! event with what changed, the row marks, and the note for selected rows
//! that went (an `undo-import` elsewhere). Against a scratch database
//! through [`TransactionStore`], written by a separate core connection as
//! that process would. LU.4's tests (`reload_tests`) keep the user's place.

use std::cell::RefCell;
use std::rc::Rc;

use gpui_kit::test::TestWindowExt as _;
use gpui_kit::{TestAppContext, VisualTestContext};
use orre_core::exclusion::ExclusionReason;
use orre_core::id::TransactionId;
use orre_core::import;
use orre_core::manual::{self, ManualTransaction};
use orre_core::money::{Currency, Money};

use crate::accounts::DialogClosed;

use super::confirm::DeleteTransactionDialog;
use super::describe::{InspectorDescription, TransactionsDescription};
use super::exclude_dialog::ExcludeDialog;
use super::source_tests::{Scratch, open_database, open_dialog, redraw};
use super::*;

fn reload(view: &Entity<TransactionsView>, cx: &mut VisualTestContext) {
    cx.update(|_, cx| view.update(cx, |view, cx| view.reload(cx)));
    redraw(cx);
}

fn describe(
    view: &Entity<TransactionsView>,
    cx: &mut VisualTestContext,
) -> TransactionsDescription {
    cx.update(|_, cx| view.read(cx).describe(cx))
}

fn select(view: &Entity<TransactionsView>, ids: &[TransactionId], cx: &mut VisualTestContext) {
    let ids = ids.to_vec();
    cx.update(|_, cx| view.update(cx, |view, cx| view.select(ids, cx)));
    redraw(cx);
}

/// What another process does: undoes the import.
fn undo_elsewhere(scratch: &Scratch) {
    import::undo(&mut scratch.db(), scratch.batch).unwrap();
}

/// The events the view emits for a read, from now on.
fn watch(
    view: &Entity<TransactionsView>,
    cx: &mut VisualTestContext,
) -> Rc<RefCell<Vec<RegisterChanged>>> {
    let seen = Rc::new(RefCell::new(Vec::new()));
    let log = seen.clone();
    cx.update(|_, cx| {
        cx.subscribe(view, move |_, event: &RegisterChanged, _| {
            log.borrow_mut().push(event.clone())
        })
        .detach()
    });
    seen
}

fn removed_note(describe: &TransactionsDescription) -> Option<(String, String)> {
    match &describe.inspector {
        InspectorDescription::Removed { title, body } => Some((title.clone(), body.clone())),
        _ => None,
    }
}

// The event ------------------------------------------------------------------

#[gpui_kit::test]
fn a_read_after_another_process_wrote_reports_what_changed(cx: &mut TestAppContext) {
    let scratch = Scratch::new();
    let (view, cx) = open_database(cx, &scratch);
    let seen = watch(&view, cx);

    // Nothing differs: no event.
    reload(&view, cx);
    assert!(seen.borrow().is_empty());

    let entry = ManualTransaction::new(
        scratch.cash,
        chrono::NaiveDate::from_ymd_opt(2026, 11, 10).unwrap(),
        Money::new(-250, Currency::GBP),
        "Bakery",
    );
    let added = manual::create(&mut scratch.db(), &entry).unwrap();
    reload(&view, cx);

    let seen = seen.borrow();
    assert_eq!(seen.len(), 1);
    assert_eq!(seen[0].change.new, [added]);
    assert!(seen[0].change.changed.is_empty() && seen[0].change.removed.is_empty());
    assert!(!seen[0].after_own_write);
}

#[gpui_kit::test]
fn a_read_after_the_views_own_write_says_so(cx: &mut TestAppContext) {
    let scratch = Scratch::new();
    let (view, cx) = open_database(cx, &scratch);
    let seen = watch(&view, cx);
    cx.update(|_, cx| {
        view.update(cx, |view, cx| {
            view.exclude(scratch.odeon(), ExclusionReason::BankError, None, cx)
                .unwrap()
        })
    });
    redraw(cx);
    let seen = seen.borrow();
    assert_eq!(seen.len(), 1);
    assert!(seen[0].after_own_write);
    assert_eq!(seen[0].change.changed, [scratch.odeon()]);
}

// The row mark ---------------------------------------------------------------

#[gpui_kit::test]
fn only_listed_rows_wear_the_mark_and_nothing_else_moves(cx: &mut TestAppContext) {
    let scratch = Scratch::new();
    let (view, cx) = open_database(cx, &scratch);
    let chosen = scratch.imported[1];
    select(&view, &[chosen], cx);
    let before = describe(&view, cx);
    assert!(before.marked().is_empty());

    let marks = BTreeSet::from([scratch.imported[0], scratch.corner_shop]);
    cx.update(|_, cx| {
        view.update(cx, |view, cx| {
            view.set_marks(marks.clone(), MarkPhase::Shown, cx)
        })
    });
    redraw(cx);
    let after = describe(&view, cx);
    assert_eq!(
        after.marked().into_iter().collect::<BTreeSet<_>>(),
        marks,
        "the marked rows, and only they"
    );
    assert_eq!(after.selected(), [chosen], "the selection stays");
    assert_eq!(after.listed(), before.listed(), "no row moved");

    // A row a search hides isn't marked; it is again when listed.
    cx.update(|_, cx| {
        view.update(cx, |view, cx| {
            view.filter.text = "odeon".to_owned();
            view.refilter(cx);
        })
    });
    redraw(cx);
    assert_eq!(describe(&view, cx).marked(), [scratch.imported[0]]);
    cx.update(|_, cx| {
        view.update(cx, |view, cx| {
            view.filter.text.clear();
            view.refilter(cx);
        })
    });
    assert_eq!(describe(&view, cx).marked().len(), 2);

    // Fading out keeps them listed until the shell clears the set.
    cx.update(|_, cx| {
        view.update(cx, |view, cx| {
            view.set_marks(marks.clone(), MarkPhase::FadingOut, cx)
        })
    });
    redraw(cx);
    assert_eq!(describe(&view, cx).marked().len(), 2);
    cx.update(|_, cx| {
        view.update(cx, |view, cx| {
            view.set_marks(BTreeSet::new(), MarkPhase::Shown, cx)
        })
    });
    redraw(cx);
    assert!(describe(&view, cx).marked().is_empty());
}

// Rows that went -------------------------------------------------------------

#[gpui_kit::test]
fn an_undo_import_elsewhere_shows_the_note_when_every_selected_row_went(cx: &mut TestAppContext) {
    let scratch = Scratch::new();
    let (view, cx) = open_database(cx, &scratch);
    select(&view, &[scratch.imported[1]], cx);
    assert!(removed_note(&describe(&view, cx)).is_none());

    undo_elsewhere(&scratch);
    reload(&view, cx);

    let after = describe(&view, cx);
    assert!(after.selected().is_empty());
    let (title, body) = removed_note(&after).expect("the inspector says the row was removed");
    assert_eq!(title, "This transaction was removed");
    assert!(body.contains("came from test-november.csv."), "{body}");
    assert!(
        body.ends_with("That import was undone outside this window."),
        "{body}"
    );
    assert!(body.contains("PRET"), "names the row as it was: {body}");

    // It stays until the next selection.
    reload(&view, cx);
    assert!(removed_note(&describe(&view, cx)).is_some());
    select(&view, &[scratch.corner_shop], cx);
    let after = describe(&view, cx);
    assert!(removed_note(&after).is_none());
    assert!(matches!(after.inspector, InspectorDescription::One(_)));
}

/// Selecting an account (which clears the row selection) shows it in the
/// inspector, not the note (review of LU.5).
#[gpui_kit::test]
fn clearing_the_selection_drops_the_removed_note(cx: &mut TestAppContext) {
    let scratch = Scratch::new();
    let (view, cx) = open_database(cx, &scratch);
    select(&view, &[scratch.imported[1]], cx);
    undo_elsewhere(&scratch);
    reload(&view, cx);
    assert!(removed_note(&describe(&view, cx)).is_some());

    cx.update(|_, cx| view.update(cx, |view, cx| view.clear_selection(cx)));
    assert!(removed_note(&describe(&view, cx)).is_none());
}

#[gpui_kit::test]
fn several_rows_going_say_so_in_the_plural(cx: &mut TestAppContext) {
    let scratch = Scratch::new();
    let (view, cx) = open_database(cx, &scratch);
    select(&view, &scratch.imported, cx);
    undo_elsewhere(&scratch);
    reload(&view, cx);
    let (title, body) = removed_note(&describe(&view, cx)).unwrap();
    assert_eq!(title, "These 3 transactions were removed");
    assert_eq!(body, "They came from test-november.csv.");
}

#[gpui_kit::test]
fn one_of_two_selected_rows_going_keeps_the_other_and_shows_no_note(cx: &mut TestAppContext) {
    let scratch = Scratch::new();
    let (view, cx) = open_database(cx, &scratch);
    select(&view, &[scratch.imported[0], scratch.corner_shop], cx);
    undo_elsewhere(&scratch);
    reload(&view, cx);
    let after = describe(&view, cx);
    assert_eq!(after.selected(), [scratch.corner_shop]);
    assert!(removed_note(&after).is_none());
    assert!(matches!(after.inspector, InspectorDescription::One(_)));
}

#[gpui_kit::test]
fn the_views_own_undo_import_shows_no_note(cx: &mut TestAppContext) {
    let scratch = Scratch::new();
    let (view, cx) = open_database(cx, &scratch);
    select(&view, &[scratch.imported[1]], cx);
    cx.update(|_, cx| view.update(cx, |view, cx| view.undo_import(scratch.batch, cx).unwrap()));
    redraw(cx);
    let after = describe(&view, cx);
    assert!(after.selected().is_empty());
    assert!(removed_note(&after).is_none());
    assert!(matches!(after.inspector, InspectorDescription::Nothing(_)));
}

#[gpui_kit::test]
fn an_inline_edit_on_a_removed_row_closes_and_shows_the_note(cx: &mut TestAppContext) {
    let scratch = Scratch::new();
    let (view, cx) = open_database(cx, &scratch);
    let edited = scratch.imported[0];
    cx.update(|window, cx| {
        view.update(cx, |view, cx| {
            view.selection.set([edited]);
            view.open(Menu::Row(edited), window, cx);
        })
    });
    redraw(cx);
    assert_eq!(
        describe(&view, cx).menu.map(|menu| menu.menu),
        Some(Menu::Row(edited))
    );

    undo_elsewhere(&scratch);
    reload(&view, cx);

    let after = describe(&view, cx);
    assert!(after.menu.is_none(), "the inline edit closed");
    assert!(removed_note(&after).is_some());
}

#[gpui_kit::test]
fn a_dialog_about_a_removed_row_stays_open_with_its_primary_disabled(cx: &mut TestAppContext) {
    let scratch = Scratch::new();
    let (view, cx) = open_database(cx, &scratch);
    let (target, odeon) = (view.clone(), scratch.odeon());
    let (dialog, dialog_cx) = open_dialog(cx, move |window, cx| {
        ExcludeDialog::new(target, odeon, window, cx)
    });
    dialog_cx.update(|window, cx| {
        dialog.update(cx, |dialog, cx| {
            dialog.choose_reason(ExclusionReason::BankError, window, cx)
        })
    });
    redraw(dialog_cx);
    assert!(!dialog_cx.update(|_, cx| dialog.read(cx).removed(cx)));
    assert_ne!(
        dialog_cx.update(|window, _| window.find("exclude-confirm").disabled()),
        Some(true)
    );
    let closed = Rc::new(RefCell::new(0));
    let count = closed.clone();
    dialog_cx.update(|_, cx| {
        cx.subscribe(&dialog, move |_, _: &DialogClosed, _| {
            *count.borrow_mut() += 1
        })
        .detach()
    });

    undo_elsewhere(&scratch);
    dialog_cx.update(|_, cx| view.update(cx, |view, cx| view.reload(cx)));
    redraw(dialog_cx);

    assert!(dialog_cx.update(|_, cx| dialog.read(cx).removed(cx)));
    assert_eq!(
        dialog_cx.update(|window, _| window.find("exclude-confirm").disabled()),
        Some(true),
        "the primary is disabled"
    );
    dialog_cx.update(|_, cx| dialog.update(cx, |dialog, cx| dialog.submit(cx)));
    assert_eq!(*closed.borrow(), 0, "and it does nothing");
    assert_eq!(
        dialog_cx.update(|_, cx| dialog.read(cx).error().cloned()),
        None
    );

    // Cancel closes it.
    dialog_cx.update(|_, cx| dialog.update(cx, |dialog, cx| dialog.cancel(cx)));
    assert_eq!(*closed.borrow(), 1);
}

#[gpui_kit::test]
fn the_delete_dialog_for_a_row_deleted_elsewhere_cannot_be_confirmed(cx: &mut TestAppContext) {
    let scratch = Scratch::new();
    let (view, cx) = open_database(cx, &scratch);
    let (target, shop) = (view.clone(), scratch.corner_shop);
    let (dialog, dialog_cx) = open_dialog(cx, move |window, cx| {
        DeleteTransactionDialog::new(target, shop, window, cx)
    });
    assert_ne!(
        dialog_cx.update(|window, _| window.find("delete-transaction-confirm").disabled()),
        Some(true)
    );
    manual::delete(&mut scratch.db(), shop).unwrap();
    dialog_cx.update(|_, cx| view.update(cx, |view, cx| view.reload(cx)));
    redraw(dialog_cx);
    assert!(dialog_cx.update(|_, cx| dialog.read(cx).removed(cx)));
    assert_eq!(
        dialog_cx.update(|window, _| window.find("delete-transaction-confirm").disabled()),
        Some(true)
    );
    dialog_cx.update(|_, cx| dialog.update(cx, |dialog, cx| dialog.submit(cx)));
    assert_eq!(
        dialog_cx.update(|_, cx| dialog.read(cx).error().cloned()),
        None
    );
}
