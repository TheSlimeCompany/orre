//! Tests of the edit pipeline ([`TransactionsView::write`]) at its
//! interface: a policy and a call on the database, on the Paper fixture and
//! on a scratch database, checking where errors are said and how they're
//! worded, the undo bar, the events the shell gets and the register being
//! read again.

use std::cell::RefCell;
use std::rc::Rc;

use coffer_core::db::Database;
use gpui_kit::{TestAppContext, VisualTestContext};
use tempfile::TempDir;

use crate::accounts::store::StoreError;

use super::editor::EditPlace;
use super::write::{Clears, Offer, Policy, SAMPLE_DATA, Said};
use super::*;

/// What a failing store call says.
const REFUSED: &str = "the store refused it";

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

fn fixture_view(cx: &mut TestAppContext) -> (Entity<TransactionsView>, &mut VisualTestContext) {
    setup(cx);
    let (view, cx) =
        cx.add_window_view(|window, cx| TransactionsView::new(Source::Fixture, window, cx));
    redraw(cx);
    (view, cx)
}

/// The view over an empty scratch database, read once.
fn database_view<'a>(
    cx: &'a mut TestAppContext,
    dir: &TempDir,
) -> (Entity<TransactionsView>, &'a mut VisualTestContext) {
    setup(cx);
    let path = dir.path().join("coffer.db");
    drop(Database::open(&path).unwrap());
    let store = TransactionStore::new(path);
    let (view, cx) = cx.add_window_view(move |window, cx| {
        TransactionsView::new(Source::Database(store), window, cx)
    });
    redraw(cx);
    (view, cx)
}

/// Runs `op` through the pipeline under `policy`, then lets the register's
/// read finish.
fn write<T: 'static>(
    view: &Entity<TransactionsView>,
    cx: &mut VisualTestContext,
    policy: Policy<'static, T>,
    op: impl FnOnce(&mut Database) -> Result<T, StoreError> + 'static,
) -> Result<T, SharedString> {
    let result = cx.update(|_, cx| view.update(cx, |view, cx| view.write(policy, cx, op)));
    redraw(cx);
    result
}

fn refuse(_: &mut Database) -> Result<(), StoreError> {
    Err(REFUSED.into())
}

fn save(_: &mut Database) -> Result<(), StoreError> {
    Ok(())
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

/// What the view says, wherever it says it.
#[derive(Debug, PartialEq, Eq)]
struct Shown {
    category: Option<SharedString>,
    inspector: Option<(EditPlace, SharedString)>,
    undo: Option<SharedString>,
}

fn said(view: &Entity<TransactionsView>, cx: &mut VisualTestContext) -> Shown {
    cx.update(|_, cx| {
        let view = view.read(cx);
        Shown {
            category: view.edit_error().cloned(),
            inspector: view.editor.error.clone(),
            undo: view.undo_message().cloned(),
        }
    })
}

fn generation(view: &Entity<TransactionsView>, cx: &mut VisualTestContext) -> u64 {
    cx.update(|_, cx| view.read(cx).generation)
}

fn set_bar(view: &Entity<TransactionsView>, cx: &mut VisualTestContext, action: UndoAction) {
    cx.update(|_, cx| {
        view.update(cx, |view, _| {
            view.undo = Some(Undo {
                action,
                message: "Excluded as a bank error".into(),
            });
        });
    });
}

fn include(name: &str) -> UndoAction {
    UndoAction::Include(fixture::id(name))
}

// Sample data ----------------------------------------------------------------

#[gpui_kit::test]
fn sample_data_is_refused_where_the_policy_says(cx: &mut TestAppContext) {
    let (view, cx) = fixture_view(cx);
    let events = events(&view, cx);
    let before = generation(&view, cx);
    let called = Rc::new(RefCell::new(false));
    let store_call = |called: &Rc<RefCell<bool>>| {
        let called = called.clone();
        move |_: &mut Database| -> Result<(), StoreError> {
            *called.borrow_mut() = true;
            Ok(())
        }
    };

    let result = write(
        &view,
        cx,
        Policy::new(Said::Category).emit(TransactionsEvent::Changed),
        store_call(&called),
    );
    assert_eq!(result, Err(SAMPLE_DATA.into()));
    assert_eq!(
        said(&view, cx),
        Shown {
            category: Some(SAMPLE_DATA.into()),
            inspector: None,
            undo: None,
        }
    );

    let result = write(
        &view,
        cx,
        Policy::edit(EditPlace::Split),
        store_call(&called),
    );
    assert_eq!(result, Err(SAMPLE_DATA.into()));
    assert_eq!(
        said(&view, cx).inspector,
        Some((EditPlace::Split, SAMPLE_DATA.into()))
    );

    // A dialog says it itself; the view says nothing new.
    cx.update(|_, cx| {
        view.update(cx, |view, _| {
            view.edit_error = None;
            view.editor.error = None;
        })
    });
    let result = write(&view, cx, Policy::ledger(), store_call(&called));
    assert_eq!(result, Err(SAMPLE_DATA.into()), "not made a sentence");
    let result = write(
        &view,
        cx,
        Policy::new(Said::Logged).log("couldn't remember Not now"),
        store_call(&called),
    );
    assert_eq!(result, Err(SAMPLE_DATA.into()));
    assert_eq!(
        said(&view, cx),
        Shown {
            category: None,
            inspector: None,
            undo: None,
        }
    );

    assert!(!*called.borrow(), "the store is never called");
    assert!(events.borrow().is_empty(), "the shell isn't told");
    assert_eq!(generation(&view, cx), before);
}

#[gpui_kit::test]
fn sample_data_refused_from_the_bar_is_offered_again(cx: &mut TestAppContext) {
    let (view, cx) = fixture_view(cx);
    let events = events(&view, cx);
    let policy = Policy::new(Said::Inspector(EditPlace::Transfer))
        .sentence()
        .emit(TransactionsEvent::Changed)
        .retry(include("txn-amazon-09"))
        .always();
    assert!(write(&view, cx, policy, save).is_err());
    assert_eq!(
        said(&view, cx),
        Shown {
            category: None,
            inspector: Some((EditPlace::Transfer, SAMPLE_DATA.into())),
            undo: Some(format!("Couldn't undo: {SAMPLE_DATA}").into()),
        }
    );
    assert_eq!(
        *events.borrow(),
        [TransactionsEvent::Changed],
        "an undo from the bar tells the shell either way"
    );
}

#[gpui_kit::test]
fn a_query_is_refused_on_sample_data(cx: &mut TestAppContext) {
    let (view, cx) = fixture_view(cx);
    let result = cx.update(|_, cx| view.read(cx).query(|_| Ok::<_, StoreError>(1)));
    assert_eq!(result, Err(SAMPLE_DATA.into()));
}

#[gpui_kit::test]
fn a_query_says_the_store_error_as_a_sentence(cx: &mut TestAppContext) {
    let dir = TempDir::new().unwrap();
    let (view, cx) = database_view(cx, &dir);
    let result: Result<(), _> = cx.update(|_, cx| {
        view.read(cx)
            .query(|_| Err::<(), StoreError>(REFUSED.into()))
    });
    assert_eq!(result, Err("The store refused it.".into()));
    let result = cx.update(|_, cx| view.read(cx).query(|_| Ok::<_, StoreError>(1)));
    assert_eq!(result, Ok(1));
}

#[gpui_kit::test]
fn a_restore_refuses_writes_and_queries_before_core_is_called(cx: &mut TestAppContext) {
    let dir = TempDir::new().unwrap();
    let (view, cx) = database_view(cx, &dir);
    let path = dir.path().join("coffer.db");
    let called = Rc::new(RefCell::new(false));

    // A restore holds an exclusive lock on `<database>-restoring`.
    let lock = std::fs::File::create(coffer_core::backup::restore_lock_path(&path)).unwrap();
    lock.lock().unwrap();
    let restoring = "A backup is being restored; try again when it's done.";
    let flag = called.clone();
    let result = write(&view, cx, Policy::new(Said::Category), move |_| {
        *flag.borrow_mut() = true;
        Ok(())
    });
    assert_eq!(result, Err(restoring.into()));
    assert_eq!(said(&view, cx).category, Some(restoring.into()));
    let flag = called.clone();
    let result: Result<(), _> = cx.update(|_, cx| {
        view.read(cx).query(move |_| {
            *flag.borrow_mut() = true;
            Ok::<_, StoreError>(())
        })
    });
    assert_eq!(result, Err(restoring.into()), "a sentence already");
    assert!(!*called.borrow(), "core is never called");

    lock.unlock().unwrap();
    assert_eq!(write(&view, cx, Policy::new(Said::Category), save), Ok(()));
}

// Errors -----------------------------------------------------------------------

#[gpui_kit::test]
fn a_store_error_is_said_where_and_as_the_policy_says(cx: &mut TestAppContext) {
    let dir = TempDir::new().unwrap();
    let (view, cx) = database_view(cx, &dir);

    let result = write(&view, cx, Policy::new(Said::Category), refuse);
    assert_eq!(result, Err(REFUSED.into()), "as it is");
    assert_eq!(said(&view, cx).category, Some(REFUSED.into()));
    assert_eq!(said(&view, cx).inspector, None);

    let result = write(&view, cx, Policy::link(EditPlace::Links), refuse);
    assert_eq!(result, Err(REFUSED.into()));
    assert_eq!(
        said(&view, cx).inspector,
        Some((EditPlace::Links, REFUSED.into()))
    );

    let policy = Policy::new(Said::Inspector(EditPlace::Exclusion)).sentence();
    let result = write(&view, cx, policy, refuse);
    assert_eq!(result, Err("The store refused it.".into()));
    assert_eq!(
        said(&view, cx).inspector,
        Some((EditPlace::Exclusion, "The store refused it.".into()))
    );

    // A dialog's error is only returned: what the view said stays.
    let result = write(&view, cx, Policy::ledger(), refuse);
    assert_eq!(result, Err("The store refused it.".into()));
    let result = write(&view, cx, Policy::new(Said::Logged), refuse);
    assert_eq!(result, Err(REFUSED.into()));
    assert_eq!(
        said(&view, cx),
        Shown {
            category: Some(REFUSED.into()),
            inspector: Some((EditPlace::Exclusion, "The store refused it.".into())),
            undo: None,
        }
    );
}

#[gpui_kit::test]
fn a_save_takes_away_the_error_the_policy_clears(cx: &mut TestAppContext) {
    let dir = TempDir::new().unwrap();
    let (view, cx) = database_view(cx, &dir);
    let say_both = |view: &Entity<TransactionsView>, cx: &mut VisualTestContext| {
        cx.update(|_, cx| {
            view.update(cx, |view, _| {
                view.edit_error = Some("before".into());
                view.editor.error = Some((EditPlace::Details, "before".into()));
            })
        });
    };

    say_both(&view, cx);
    write(&view, cx, Policy::new(Said::Category), save).unwrap();
    assert_eq!(said(&view, cx).category, None);
    assert!(said(&view, cx).inspector.is_some());

    say_both(&view, cx);
    write(&view, cx, Policy::edit(EditPlace::Split), save).unwrap();
    assert_eq!(said(&view, cx).inspector, None, "wherever it was said");
    assert!(said(&view, cx).category.is_some());

    say_both(&view, cx);
    write(&view, cx, Policy::ledger(), save).unwrap();
    write(&view, cx, Policy::new(Said::Logged), save).unwrap();
    let policy = Policy::new(Said::Category).clears(Clears::Nothing);
    write(&view, cx, policy, save).unwrap();
    assert!(said(&view, cx).category.is_some());
    assert!(said(&view, cx).inspector.is_some());

    let policy = Policy::new(Said::Dialog).clears(Clears::Inspector);
    write(&view, cx, policy, save).unwrap();
    assert_eq!(said(&view, cx).inspector, None);
    assert!(said(&view, cx).category.is_some());
}

// The undo bar -------------------------------------------------------------------

#[gpui_kit::test]
fn the_undo_bar_follows_the_offer(cx: &mut TestAppContext) {
    let dir = TempDir::new().unwrap();
    let (view, cx) = database_view(cx, &dir);
    let amazon = include("txn-amazon-09");
    let pret = include("txn-pret-09");
    let showing = Some(SharedString::from("Excluded as a bank error"));

    // Single-row edits leave it as it is (the M3.3 rule).
    set_bar(&view, cx, amazon);
    write(&view, cx, Policy::edit(EditPlace::Details), save).unwrap();
    assert_eq!(said(&view, cx).undo, showing);

    write(
        &view,
        cx,
        Policy::new(Said::Category).undo(|_| Offer::Clear),
        save,
    )
    .unwrap();
    assert_eq!(said(&view, cx).undo, None);

    // From what the store returned.
    let policy = Policy::link(EditPlace::Links).undo(|count: &usize| {
        Offer::Show(
            include("txn-pret-09"),
            format!("Linked {count} payments").into(),
        )
    });
    write(&view, cx, policy, |_| Ok(2)).unwrap();
    assert_eq!(said(&view, cx).undo, Some("Linked 2 payments".into()));

    set_bar(&view, cx, amazon);
    let policy = Policy::ledger().undo(move |_| Offer::ClearIf(pret));
    write(&view, cx, policy, save).unwrap();
    assert_eq!(said(&view, cx).undo, showing, "it offers something else");
    let policy = Policy::ledger().undo(move |_| Offer::ClearIf(amazon));
    write(&view, cx, policy, save).unwrap();
    assert_eq!(said(&view, cx).undo, None);

    // A failure offers nothing new, unless it was the bar's own Undo.
    set_bar(&view, cx, amazon);
    let policy = Policy::link(EditPlace::Links).undo(move |_| Offer::Show(pret, "Linked".into()));
    write(&view, cx, policy, refuse).unwrap_err();
    assert_eq!(said(&view, cx).undo, showing);
    let policy = Policy::new(Said::Inspector(EditPlace::Exclusion))
        .sentence()
        .retry(amazon);
    write(&view, cx, policy, refuse).unwrap_err();
    assert_eq!(
        said(&view, cx).undo,
        Some("Couldn't undo: The store refused it.".into())
    );
    let action = cx.update(|_, cx| view.read(cx).undo.as_ref().map(|undo| undo.action));
    assert_eq!(action, Some(amazon), "Undo tries the same again");

    // A retry is only for failures.
    write(
        &view,
        cx,
        Policy::edit(EditPlace::Details).retry(pret),
        save,
    )
    .unwrap();
    assert_eq!(
        said(&view, cx).undo,
        Some("Couldn't undo: The store refused it.".into())
    );
}

// Events and the register ----------------------------------------------------------

#[gpui_kit::test]
fn the_shell_is_told_what_the_policy_names(cx: &mut TestAppContext) {
    let dir = TempDir::new().unwrap();
    let (view, cx) = database_view(cx, &dir);
    let events = events(&view, cx);

    write(&view, cx, Policy::edit(EditPlace::Details), save).unwrap();
    assert!(
        events.borrow().is_empty(),
        "an inspector edit tells nothing"
    );

    write(&view, cx, Policy::link(EditPlace::Share), save).unwrap();
    write(&view, cx, Policy::ledger(), save).unwrap();
    let policy = Policy::edit(EditPlace::Details).emit(TransactionsEvent::LedgerChanged);
    write(&view, cx, policy, save).unwrap();
    assert_eq!(
        *events.borrow(),
        [
            TransactionsEvent::Changed,
            TransactionsEvent::LedgerChanged,
            TransactionsEvent::LedgerChanged,
        ]
    );
    events.borrow_mut().clear();

    write(&view, cx, Policy::link(EditPlace::Links), refuse).unwrap_err();
    write(&view, cx, Policy::ledger(), refuse).unwrap_err();
    assert!(events.borrow().is_empty(), "not after a failure");

    let policy = Policy::new(Said::Category)
        .emit(TransactionsEvent::Changed)
        .always();
    write(&view, cx, policy, refuse).unwrap_err();
    assert_eq!(
        *events.borrow(),
        [TransactionsEvent::Changed],
        "unless always"
    );
}

#[gpui_kit::test]
fn the_register_is_read_again_after_a_save(cx: &mut TestAppContext) {
    let dir = TempDir::new().unwrap();
    let (view, cx) = database_view(cx, &dir);
    let start = generation(&view, cx);

    write(&view, cx, Policy::edit(EditPlace::Details), save).unwrap();
    assert_eq!(generation(&view, cx), start + 1);

    write(&view, cx, Policy::new(Said::Logged).no_reload(), save).unwrap();
    assert_eq!(generation(&view, cx), start + 1, "nothing to read again");

    write(&view, cx, Policy::edit(EditPlace::Details), refuse).unwrap_err();
    assert_eq!(generation(&view, cx), start + 1, "not after a failure");

    let policy = Policy::new(Said::Category).always();
    write(&view, cx, policy, refuse).unwrap_err();
    assert_eq!(generation(&view, cx), start + 2, "unless always");
}

#[gpui_kit::test]
fn nothing_happens_when_nothing_was_written(cx: &mut TestAppContext) {
    let dir = TempDir::new().unwrap();
    let (view, cx) = database_view(cx, &dir);
    let events = events(&view, cx);
    let start = generation(&view, cx);
    set_bar(&view, cx, include("txn-amazon-09"));
    cx.update(|_, cx| view.update(cx, |view, _| view.edit_error = Some("before".into())));

    let policy = Policy::new(Said::Category)
        .emit(TransactionsEvent::Changed)
        .undo(|_| Offer::Clear)
        .wrote(|renamed| *renamed);
    assert_eq!(write(&view, cx, policy, |_| Ok(false)), Ok(false));
    assert_eq!(
        said(&view, cx),
        Shown {
            category: Some("before".into()),
            inspector: None,
            undo: Some("Excluded as a bank error".into()),
        }
    );
    assert!(events.borrow().is_empty());
    assert_eq!(generation(&view, cx), start);

    let policy = Policy::new(Said::Category)
        .emit(TransactionsEvent::Changed)
        .wrote(|renamed| *renamed);
    assert_eq!(write(&view, cx, policy, |_| Ok(true)), Ok(true));
    assert_eq!(said(&view, cx).category, None);
    assert_eq!(*events.borrow(), [TransactionsEvent::Changed]);
    assert_eq!(generation(&view, cx), start + 1);
}

// Leaving a row ----------------------------------------------------------------------

#[gpui_kit::test]
fn an_error_on_a_row_left_goes_to_the_failure_bar(cx: &mut TestAppContext) {
    let (view, cx) = fixture_view(cx);
    let amazon = fixture::id("txn-amazon-09");
    set_bar(&view, cx, include("txn-pret-09"));

    // Nothing failed: nothing moves.
    cx.update(|_, cx| view.update(cx, |view, _| view.say_on_failure_bar(amazon)));
    assert!(cx.update(|_, cx| view.read(cx).edit_failure.is_none()));
    assert!(said(&view, cx).undo.is_some());

    write(&view, cx, Policy::edit(EditPlace::Details), save).unwrap_err();
    cx.update(|_, cx| view.update(cx, |view, _| view.say_on_failure_bar(amazon)));
    let failure = cx.update(|_, cx| view.read(cx).edit_failure.clone());
    assert_eq!(
        failure,
        Some(EditFailure {
            row: amazon,
            place: EditPlace::Details,
            reason: SAMPLE_DATA.into(),
        })
    );
    assert_eq!(
        said(&view, cx),
        Shown {
            category: None,
            inspector: None,
            undo: None,
        },
        "the failure bar takes the undo bar's place"
    );
    assert!(cx.update(|_, cx| view.read(cx).edit_failure_message().is_some()));
}
