//! UI tests for M3.10: the Excluded chip, the Exclude dialog, "Include
//! again" and the undo bar, the undo-import confirmation, and manual rows
//! (the New-transaction dialog, the inspector's Date, Amount and Account,
//! Delete transaction), against a scratch database through
//! [`TransactionStore`], as `coffer` runs them.

use chrono::NaiveDate;
use coffer_core::account::{self, AccountKind, EnteredBalance, EnteredSource, NewAccount};
use coffer_core::db::Database;
use coffer_core::exclusion::{self, ExclusionReason};
use coffer_core::id::{AccountId, BatchId, TransactionId};
use coffer_core::import::{self, Committed};
use coffer_core::manual::{self, ManualTransaction};
use coffer_core::money::{Currency, Money};
use coffer_core::reconciliation;
use coffer_core::statement::{SourceFormat, SourceLocation, Statement, StatementLine};
use coffer_core::transfer;
use gpui_kit::test::TestWindowExt as _;
use gpui_kit::{SharedString, TestAppContext, VisualTestContext};
use tempfile::TempDir;

use super::confirm::{DeleteTransactionDialog, UndoImportDialog};
use super::exclude_dialog::ExcludeDialog;
use super::new_transaction::{Direction, NewTransactionDialog};
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

fn row_id(id: TransactionId) -> String {
    format!("txn-{id}")
}

fn day(day: u32) -> NaiveDate {
    NaiveDate::from_ymd_opt(2026, 11, day).unwrap()
}

fn gbp(minor: i64) -> Money {
    Money::new(minor, Currency::GBP)
}

/// A scratch database: Current account with three imported rows (one
/// import, `test-november.csv`), and Cash with one manual row.
struct Scratch {
    _dir: TempDir,
    path: std::path::PathBuf,
    current: AccountId,
    cash: AccountId,
    batch: BatchId,
    /// Odeon, Pret, Tesco: the imported rows, oldest first.
    imported: Vec<TransactionId>,
    /// The manual row on Cash.
    corner_shop: TransactionId,
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
        let cash = account::create(
            &mut db,
            &NewAccount::new("Cash", AccountKind::Cash, Currency::GBP),
        )
        .unwrap();
        let mut statement = Statement::new(SourceFormat::Csv, Currency::GBP);
        for (line, (on, minor, text)) in [
            (7, -3180, "ODEON LUXE LEICESTER SQ"),
            (8, -645, "PRET A MANGER"),
            (9, -985, "TESCO EXPRESS"),
        ]
        .into_iter()
        .enumerate()
        {
            statement.push(
                SourceLocation::Line(line as u64 + 2),
                Ok(StatementLine::new(day(on), gbp(minor), text)),
            );
        }
        let preview = import::preview(&db, current, "test-november.csv", statement).unwrap();
        let Committed::Batch(summary) =
            import::commit(&mut db, &preview, preview.needs_confirmation()).unwrap()
        else {
            panic!("a batch");
        };
        let mut entry = ManualTransaction::new(cash, day(8), gbp(-420), "Corner shop");
        entry.merchant = Some("Corner shop".to_owned());
        let corner_shop = manual::create(&mut db, &entry).unwrap();
        drop(db);
        let db = TransactionStore::new(path.clone()).open().unwrap();
        let register = Register::load(&db.reader().unwrap(), day(14)).unwrap();
        drop(db);
        let mut imported: Vec<_> = register
            .rows()
            .iter()
            .filter(|row| row.source.batch() == Some(summary.batch))
            .map(|row| (row.date, row.id))
            .collect();
        imported.sort();
        let imported = imported.into_iter().map(|(_, id)| id).collect();
        Self {
            _dir: dir,
            path,
            current,
            cash,
            batch: summary.batch,
            imported,
            corner_shop,
        }
    }

    fn db(&self) -> Database {
        Database::open(&self.path).unwrap()
    }

    fn excluded(&self, id: TransactionId) -> Option<exclusion::Exclusion> {
        exclusion::get(&self.db(), id).unwrap()
    }

    fn odeon(&self) -> TransactionId {
        self.imported[0]
    }
}

fn open_database<'a>(
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

fn listed(view: &Entity<TransactionsView>, cx: &mut VisualTestContext) -> Vec<TransactionId> {
    cx.update(|_, cx| {
        let view = view.read(cx);
        view.listing()
            .rows
            .iter()
            .map(|&index| view.register().rows()[index].id)
            .collect()
    })
}

fn selected(view: &Entity<TransactionsView>, cx: &mut VisualTestContext) -> Vec<TransactionId> {
    cx.update(|_, cx| view.read(cx).selected().iter().map(|row| row.id).collect())
}

/// `build`'s dialog in a window of its own, beside the view's.
fn open_dialog<D: 'static + gpui_kit::Render>(
    cx: &mut VisualTestContext,
    build: impl FnOnce(&mut Window, &mut Context<D>) -> D + 'static,
) -> (Entity<D>, &mut VisualTestContext) {
    let (dialog, cx) = cx.add_window_view(build);
    redraw(cx);
    (dialog, cx)
}

// The fixture ----------------------------------------------------------------

#[gpui_kit::test]
fn the_excluded_chip_shows_excluded_rows_in_their_days(cx: &mut TestAppContext) {
    setup(cx);
    let (view, cx) =
        cx.add_window_view(|window, cx| TransactionsView::new(Source::Fixture, window, cx));
    cx.update(|_, cx| {
        view.update(cx, |view, cx| {
            view.show_fixture(fixture::excluded_register(), cx)
        })
    });
    redraw(cx);
    let odeon = fixture::id("txn-odeon-07");
    let saturday_net = |cx: &mut VisualTestContext| {
        cx.update(|_, cx| {
            let view = view.read(cx);
            let listing = view.listing();
            listing
                .days
                .iter()
                .find(|day| day.date == NaiveDate::from_ymd_opt(2026, 11, 7).unwrap())
                .map(|day| day.net.clone())
                .unwrap()
        })
    };
    assert!(!listed(&view, cx).contains(&odeon), "hidden by default");
    let net = saturday_net(cx);
    assert_eq!(net, [gbp(-16_054)]);
    assert_eq!(
        cx.update(|_, cx| view.read(cx).counts()),
        Counts {
            transactions: 23,
            excluded: 2,
            sample: true
        }
    );

    click("filter-excluded", cx);
    assert!(listed(&view, cx).contains(&odeon));
    assert_eq!(
        saturday_net(cx),
        net,
        "excluded rows don't count in the day"
    );
    assert!(shows(row_id(odeon), cx));

    // The inspector: the card, with Include again and Edit reason…; Source
    // keeps only Undo import….
    click(row_id(odeon), cx);
    assert!(shows("include-again", cx));
    assert!(shows("edit-exclusion", cx));
    assert!(shows("undo-import", cx));
    assert!(!shows("exclude", cx));

    // Sample data can't change: Include again says so in the card.
    click("include-again", cx);
    assert!(cx.update(|_, cx| {
        view.read(cx)
            .editor
            .error_at(editor::EditPlace::Exclusion)
            .is_some()
    }));

    // Escape clears the selection, then the filters, the chip with them.
    cx.update(|_, cx| view.update(cx, |view, cx| view.clear_selection(cx)));
    cx.update(|window, cx| {
        view.update(cx, |view, cx| view.cancel(window, cx));
    });
    redraw(cx);
    assert!(!listed(&view, cx).contains(&odeon));
}

// Excluding ------------------------------------------------------------------

#[gpui_kit::test]
fn the_exclude_dialog_needs_a_reason_and_details_for_other(cx: &mut TestAppContext) {
    let scratch = Scratch::new();
    let odeon = scratch.odeon();
    let (view, cx) = open_database(cx, &scratch);
    click(row_id(odeon), cx);
    assert!(shows("exclude", cx), "an imported row can be excluded");
    assert!(!shows("delete-transaction", cx), "and not deleted");

    let target = view.clone();
    let (dialog, cx) = open_dialog(cx, move |window, cx| {
        ExcludeDialog::new(target, odeon, window, cx)
    });
    cx.update(|_, cx| dialog.update(cx, |dialog, cx| dialog.submit(cx)));
    assert_eq!(
        cx.update(|_, cx| dialog.read(cx).error().cloned()),
        Some("Choose a reason.".into())
    );
    cx.update(|window, cx| {
        dialog.update(cx, |dialog, cx| {
            dialog.choose_reason(ExclusionReason::Other, window, cx);
            dialog.submit(cx);
        })
    });
    assert!(cx.update(|_, cx| dialog.read(cx).error().is_some()));
    assert_eq!(scratch.excluded(odeon), None, "nothing saved yet");

    cx.update(|window, cx| {
        dialog.update(cx, |dialog, cx| {
            dialog.set_details("  Test charge ", window, cx);
            dialog.submit(cx);
        })
    });
    redraw(cx);
    let stored = scratch.excluded(odeon).unwrap();
    assert_eq!(stored.reason, ExclusionReason::Other);
    assert_eq!(stored.note.as_deref(), Some("Test charge"));
}

#[gpui_kit::test]
fn excluding_hides_the_row_and_undo_includes_it_again(cx: &mut TestAppContext) {
    let scratch = Scratch::new();
    let odeon = scratch.odeon();
    let (view, cx) = open_database(cx, &scratch);
    click(row_id(odeon), cx);
    cx.update(|_, cx| {
        view.update(cx, |view, cx| {
            view.exclude(odeon, ExclusionReason::BankError, None, cx)
        })
    })
    .unwrap();
    redraw(cx);
    assert_eq!(
        scratch.excluded(odeon).map(|exclusion| exclusion.reason),
        Some(ExclusionReason::BankError)
    );
    assert!(!listed(&view, cx).contains(&odeon), "the chip is off");
    assert_eq!(
        cx.update(|_, cx| view.read(cx).undo_message().cloned()),
        Some("Excluded as bank error".into())
    );
    assert_eq!(cx.update(|_, cx| view.read(cx).counts().excluded), 1);

    click("undo-change", cx);
    assert_eq!(scratch.excluded(odeon), None);
    assert!(listed(&view, cx).contains(&odeon));
    assert_eq!(selected(&view, cx), [odeon], "back where the user left it");

    // With the chip on, the card's Include again includes it too.
    cx.update(|_, cx| {
        view.update(cx, |view, cx| {
            view.exclude(odeon, ExclusionReason::Disputed, None, cx)
        })
    })
    .unwrap();
    redraw(cx);
    click("filter-excluded", cx);
    click(row_id(odeon), cx);
    click("include-again", cx);
    assert_eq!(scratch.excluded(odeon), None);
}

#[gpui_kit::test]
fn a_duplicate_in_a_locked_period_is_refused_and_said(cx: &mut TestAppContext) {
    let scratch = Scratch::new();
    let odeon = scratch.odeon();
    // Balances that check the first two rows, locked through 8 Nov.
    {
        let mut db = scratch.db();
        for (on, minor) in [(6, 0), (8, -3825)] {
            account::add_balance(
                &mut db,
                scratch.current,
                &EnteredBalance {
                    date: day(on),
                    balance: gbp(minor),
                    source: EnteredSource::Statement,
                    note: None,
                },
            )
            .unwrap();
        }
        reconciliation::lock(&mut db, scratch.current, day(8)).unwrap();
    }
    let (view, cx) = open_database(cx, &scratch);
    let target = view.clone();
    let (dialog, cx) = open_dialog(cx, move |window, cx| {
        ExcludeDialog::new(target, odeon, window, cx)
    });
    cx.update(|window, cx| {
        dialog.update(cx, |dialog, cx| {
            dialog.choose_reason(ExclusionReason::Duplicate, window, cx);
            dialog.submit(cx);
        })
    });
    let error = cx.update(|_, cx| dialog.read(cx).error().cloned()).unwrap();
    assert!(error.contains("locked"), "{error}");
    assert_eq!(scratch.excluded(odeon), None);

    // A bank error doesn't move the balance, so the lock doesn't stop it.
    cx.update(|window, cx| {
        dialog.update(cx, |dialog, cx| {
            dialog.choose_reason(ExclusionReason::BankError, window, cx);
            dialog.submit(cx);
        })
    });
    assert!(scratch.excluded(odeon).is_some());
}

// Undoing an import ------------------------------------------------------------

#[gpui_kit::test]
fn undo_import_confirms_then_removes_the_batch(cx: &mut TestAppContext) {
    let scratch = Scratch::new();
    let (view, cx) = open_database(cx, &scratch);
    click(row_id(scratch.odeon()), cx);
    assert!(shows("undo-import", cx));

    let (target, batch) = (view.clone(), scratch.batch);
    let (dialog, cx) = open_dialog(cx, move |window, cx| {
        UndoImportDialog::new(target, batch, window, cx)
    });
    assert!(shows("undo-import-confirm", cx));
    cx.update(|_, cx| dialog.update(cx, |dialog, cx| dialog.submit(cx)));
    redraw(cx);
    assert_eq!(cx.update(|_, cx| dialog.read(cx).error().cloned()), None);
    assert_eq!(listed(&view, cx), [scratch.corner_shop]);

    // Undone already: the dialog says so, with nothing to confirm.
    let target = view.clone();
    let (again, cx) = open_dialog(cx, move |window, cx| {
        UndoImportDialog::new(target, batch, window, cx)
    });
    assert!(cx.update(|_, cx| again.read(cx).error().is_some()));
    assert!(!shows("undo-import-confirm", cx));
}

// Manual rows ------------------------------------------------------------------

#[gpui_kit::test]
fn a_new_transaction_is_added_through_core_and_selected(cx: &mut TestAppContext) {
    let scratch = Scratch::new();
    let (view, cx) = open_database(cx, &scratch);
    let (target, cash) = (view.clone(), scratch.cash);
    let (dialog, cx) = open_dialog(cx, move |window, cx| {
        NewTransactionDialog::new(target, Some(cash), window, cx)
    });
    assert_eq!(cx.update(|_, cx| dialog.read(cx).account()), Some(cash));

    // Nothing typed: the amount is asked for.
    cx.update(|_, cx| dialog.update(cx, |dialog, cx| dialog.submit(cx)));
    assert!(cx.update(|_, cx| dialog.read(cx).error().is_some()));

    let groceries = cx.update(|_, cx| {
        view.read(cx)
            .register()
            .categories()
            .iter()
            .find(|category| category.key.as_str() == "food.groceries")
            .unwrap()
            .id
    });
    cx.update(|window, cx| {
        dialog.update(cx, |dialog, cx| {
            dialog.fill("12.80", "Farmers' market", "Eggs and bread", window, cx);
            dialog.set_category(Some(groceries), cx);
            dialog.submit(cx);
        })
    });
    redraw(cx);
    let added = selected(&view, cx);
    assert_eq!(added.len(), 1, "the new row is selected");
    let row = cx.update(|_, cx| view.read(cx).register().row(added[0]).unwrap().clone());
    assert_eq!(row.amount, gbp(-1280), "Out is money out");
    assert_eq!(row.account, cash);
    assert_eq!(row.merchant, "Farmers' market");
    assert_eq!(row.note.as_deref(), Some("Eggs and bread"));
    assert!(row.source.is_manual());

    // Money in dated after the account closed is refused by core.
    let wallet = {
        let mut db = scratch.db();
        let wallet = account::create(
            &mut db,
            &NewAccount::new("Old wallet", AccountKind::Cash, Currency::GBP),
        )
        .unwrap();
        account::close::close(&mut db, wallet, day(10)).unwrap();
        wallet
    };
    cx.update(|_, cx| view.update(cx, |view, cx| view.reload(cx)));
    redraw(cx);
    let target = view.clone();
    let (later, cx) = open_dialog(cx, move |window, cx| {
        NewTransactionDialog::new(target, Some(wallet), window, cx)
    });
    cx.update(|window, cx| {
        later.update(cx, |dialog, cx| {
            dialog.set_direction(Direction::In, cx);
            dialog.fill("5.00", "Found", "", window, cx);
        })
    });
    cx.update(|window, cx| {
        later.update(cx, |dialog, cx| {
            dialog.set_date("12 Nov 2026", window, cx);
            dialog.submit(cx);
        })
    });
    let error = cx.update(|_, cx| later.read(cx).error().cloned()).unwrap();
    assert!(error.contains("closed"), "{error}");
}

#[gpui_kit::test]
fn a_manual_rows_date_amount_and_account_save_through_core(cx: &mut TestAppContext) {
    let scratch = Scratch::new();
    let shop = scratch.corner_shop;
    let (view, cx) = open_database(cx, &scratch);
    click(row_id(shop), cx);
    assert!(shows("manual-amount", cx));
    assert!(shows("delete-transaction", cx));
    assert!(
        !shows("exclude", cx),
        "manual rows are deleted, not excluded"
    );

    cx.update(|window, cx| {
        let (date, amount) = {
            let editor = &view.read(cx).editor;
            (editor.date.clone(), editor.amount.clone())
        };
        date.update(cx, |input, cx| input.set_value("6 Nov 2026", window, cx));
        amount.update(cx, |input, cx| input.set_value("-5.10", window, cx));
    });
    redraw(cx);
    // Leaving the row saves what was typed.
    click(row_id(scratch.imported[2]), cx);
    let stored = manual::get(&scratch.db(), shop).unwrap();
    assert_eq!(stored.date, day(6));
    assert_eq!(stored.amount, gbp(-510));

    // The Account select moves it.
    click(row_id(shop), cx);
    cx.update(|window, cx| {
        view.update(cx, |view, cx| {
            view.show_menu(Menu::ManualAccount, window, cx);
            view.choose(&scratch.current.to_string().into(), window, cx);
        })
    });
    redraw(cx);
    assert_eq!(
        manual::get(&scratch.db(), shop).unwrap().account,
        scratch.current
    );

    // A date that can't be read is said, and nothing changes.
    cx.update(|window, cx| {
        let date = view.read(cx).editor.date.clone();
        date.update(cx, |input, cx| input.set_value("31 Feb", window, cx));
        view.update(cx, |view, cx| view.commit_manual_date(window, cx));
    });
    assert!(cx.update(|_, cx| {
        view.read(cx)
            .editor
            .error_at(editor::EditPlace::Details)
            .is_some()
    }));
    assert_eq!(manual::get(&scratch.db(), shop).unwrap().date, day(6));
}

#[gpui_kit::test]
fn a_new_transaction_hidden_by_the_filters_is_shown_and_selected(cx: &mut TestAppContext) {
    let scratch = Scratch::new();
    let (view, cx) = open_database(cx, &scratch);
    // Current account's uncategorised rows that mention Tesco: a new
    // uncategorised row on Cash is hidden by the account and the search,
    // not by "Uncategorised".
    cx.update(|_, cx| {
        view.update(cx, |view, cx| {
            view.set_account(Some(scratch.current), cx);
            view.set_uncategorised(true, cx);
        })
    });
    click("search", cx);
    cx.update(move |window, cx| window.input("tesco", cx));
    redraw(cx);
    assert_eq!(listed(&view, cx), [scratch.imported[2]]);

    let (target, cash) = (view.clone(), scratch.cash);
    let (dialog, cx) = open_dialog(cx, move |window, cx| {
        NewTransactionDialog::new(target, Some(cash), window, cx)
    });
    cx.update(|window, cx| {
        dialog.update(cx, |dialog, cx| {
            dialog.fill("12.80", "Farmers' market", "", window, cx);
            dialog.submit(cx);
        })
    });
    redraw(cx);

    let added = selected(&view, cx);
    assert_eq!(added.len(), 1, "the new row is selected");
    assert!(listed(&view, cx).contains(&added[0]));
    let (filter, search) = cx.update(|_, cx| {
        let view = view.read(cx);
        (view.filter().clone(), view.search.read(cx).value())
    });
    assert_eq!(filter.account, None, "the account that hid it is cleared");
    assert!(filter.text.is_empty());
    assert_eq!(search.as_ref(), "", "and the search field with it");
    assert!(filter.uncategorised, "a filter that shows it stays");
}

#[gpui_kit::test]
fn an_edit_that_fails_as_the_row_is_left_is_said_in_the_failure_bar(cx: &mut TestAppContext) {
    let scratch = Scratch::new();
    let shop = scratch.corner_shop;
    let (view, cx) = open_database(cx, &scratch);
    let failure =
        |cx: &mut VisualTestContext| cx.update(|_, cx| view.read(cx).edit_failure_message());
    let bad_date = |cx: &mut VisualTestContext| {
        cx.update(|window, cx| {
            let date = view.read(cx).editor.date.clone();
            date.update(cx, |input, cx| input.set_value("31 Feb", window, cx));
        });
        redraw(cx);
    };

    // A date that can't be read, typed and left by clicking another row:
    // the fields show that row now, so the bar says the change didn't save.
    click(row_id(shop), cx);
    bad_date(cx);
    click(row_id(scratch.imported[2]), cx);
    assert_eq!(
        failure(cx).as_deref(),
        Some("Couldn't save the change to Corner shop")
    );
    assert!(shows("edit-failure-show", cx));
    assert_eq!(manual::get(&scratch.db(), shop).unwrap().date, day(8));
    assert_eq!(selected(&view, cx), [scratch.imported[2]]);

    // Show goes back to the row and says why under its fields.
    click("edit-failure-show", cx);
    assert_eq!(selected(&view, cx), [shop]);
    assert_eq!(failure(cx), None);
    let error = cx.update(|_, cx| {
        view.read(cx)
            .editor
            .error_at(editor::EditPlace::Details)
            .unwrap()
    });
    assert!(error.contains("date"), "{error}");

    // The fields show what's stored again, so leaving now saves nothing and
    // says nothing.
    click(row_id(scratch.imported[2]), cx);
    assert_eq!(failure(cx), None);

    // The bar can be dismissed, and goes when the selection moves.
    click(row_id(shop), cx);
    bad_date(cx);
    click(row_id(scratch.imported[2]), cx);
    assert!(failure(cx).is_some());
    click("notification-dismiss", cx);
    assert_eq!(failure(cx), None);
    click(row_id(shop), cx);
    bad_date(cx);
    click(row_id(scratch.imported[2]), cx);
    click(row_id(scratch.imported[1]), cx);
    assert_eq!(failure(cx), None);
}

#[gpui_kit::test]
fn a_paired_manual_rows_amount_waits_until_it_is_unpaired(cx: &mut TestAppContext) {
    let scratch = Scratch::new();
    let tesco = scratch.imported[2];
    // Cash taken out with the Tesco shop: a confirmed pair with a manual
    // side on Cash.
    let (cashback, group) = {
        let mut db = scratch.db();
        let mut entry = ManualTransaction::new(scratch.cash, day(9), gbp(985), "Cashback");
        entry.merchant = Some("Cashback".to_owned());
        let cashback = manual::create(&mut db, &entry).unwrap();
        transfer::suggest_for_batch(&mut db, scratch.batch, transfer::TransferWindow::DEFAULT)
            .unwrap();
        let group = transfer::group_of(&db, cashback).unwrap().unwrap().id;
        transfer::confirm(&mut db, group).unwrap();
        (cashback, group)
    };
    let (view, cx) = open_database(cx, &scratch);
    let set_amount = |text: &'static str, cx: &mut VisualTestContext| {
        cx.update(|window, cx| {
            let amount = view.read(cx).editor.amount.clone();
            amount.update(cx, |input, cx| input.set_value(text, window, cx));
            view.update(cx, |view, cx| view.commit_manual_amount(window, cx));
        });
        redraw(cx);
    };

    // Refused under Details while it's a transfer, and nothing changes.
    click(row_id(cashback), cx);
    set_amount("12.00", cx);
    let error = cx.update(|_, cx| {
        view.read(cx)
            .editor
            .error_at(editor::EditPlace::Details)
            .unwrap()
    });
    assert!(error.contains("transfer pair"), "{error}");
    assert_eq!(
        manual::get(&scratch.db(), cashback).unwrap().amount,
        gbp(985)
    );
    assert_eq!(
        transfer::group_of(&scratch.db(), cashback)
            .unwrap()
            .map(|g| g.id),
        Some(group)
    );

    // Unpaired (an undoable decision), the same edit saves.
    transfer::unpair(&mut scratch.db(), group).unwrap();
    cx.update(|_, cx| view.update(cx, |view, cx| view.reload(cx)));
    redraw(cx);
    set_amount("12.00", cx);
    assert_eq!(
        cx.update(|_, cx| view.read(cx).editor.error_at(editor::EditPlace::Details)),
        None
    );
    assert_eq!(
        manual::get(&scratch.db(), cashback).unwrap().amount,
        gbp(1200)
    );
    assert!(transfer::group_of(&scratch.db(), tesco).unwrap().is_none());
}

#[gpui_kit::test]
fn only_manual_rows_can_be_deleted(cx: &mut TestAppContext) {
    let scratch = Scratch::new();
    let shop = scratch.corner_shop;
    let (view, cx) = open_database(cx, &scratch);
    let refused =
        cx.update(|_, cx| view.update(cx, |view, cx| view.delete_manual(scratch.odeon(), cx)));
    assert!(refused.unwrap_err().contains("exclude it instead"));

    let target = view.clone();
    let (dialog, cx) = open_dialog(cx, move |window, cx| {
        DeleteTransactionDialog::new(target, shop, window, cx)
    });
    cx.update(|_, cx| dialog.update(cx, |dialog, cx| dialog.submit(cx)));
    redraw(cx);
    assert!(matches!(
        manual::get(&scratch.db(), shop),
        Err(manual::Error::TransactionNotFound(_))
    ));
    assert!(!listed(&view, cx).contains(&shop));
}
