//! UI integration tests for accounts management: the shell's panel,
//! inspector and dialogs in a headless window, against a scratch database
//! through [`AccountStore`], as `coffer` runs them.

use chrono::NaiveDate;
use coffer_core::account::{self, AccountKind, EnteredBalance, EnteredSource, NewAccount};
use coffer_core::db::Database;
use coffer_core::id::AccountId;
use coffer_core::money::{Currency, Money};
use gpui_kit::test::TestWindowExt as _;
use gpui_kit::{AppContext as _, Entity, TestAppContext, VisualTestContext};
use tempfile::TempDir;

use super::*;
use crate::shell::{ShellView, View};

/// A scratch database with Current account (an entered balance, a pot) and
/// the store reading it.
struct Scratch {
    _dir: TempDir,
    path: std::path::PathBuf,
    current: AccountId,
    pot: AccountId,
}

impl Scratch {
    fn new() -> Self {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("coffer.db");
        let mut db = Database::open(&path).unwrap();
        let mut current = NewAccount::new("Current account", AccountKind::Current, Currency::GBP);
        current.institution = Some("Example Bank".into());
        let balance = EnteredBalance {
            date: NaiveDate::from_ymd_opt(2026, 11, 1).unwrap(),
            balance: Money::new(10_000, Currency::GBP),
            source: EnteredSource::Manual,
            note: None,
        };
        let current = account::create_with_balance(&mut db, &current, Some(&balance)).unwrap();
        let mut pot = NewAccount::new("Holiday", AccountKind::Pot, Currency::GBP);
        pot.parent = Some(current);
        let pot = account::create(&mut db, &pot).unwrap();
        Self {
            _dir: dir,
            path,
            current,
            pot,
        }
    }

    fn db(&self) -> Database {
        Database::open(&self.path).unwrap()
    }
}

fn open<'a>(
    cx: &'a mut TestAppContext,
    scratch: &Scratch,
) -> (
    Entity<ShellView>,
    Entity<Accounts>,
    &'a mut VisualTestContext,
) {
    cx.update(|cx| {
        gpui_kit::init(cx);
        crate::theme::component::install(cx);
        crate::shell::init(cx);
    });
    let store = AccountStore::new(scratch.path.clone());
    let (shell, cx) = cx.add_window_view(|window, cx| {
        let accounts = cx.new(|_| Accounts::from_store(store));
        ShellView::with_accounts(View::Transactions, accounts, window, cx)
    });
    let accounts = cx.update(|_, cx| shell.read(cx).accounts().clone());
    redraw(cx);
    (shell, accounts, cx)
}

fn redraw(cx: &mut VisualTestContext) {
    cx.run_until_parked();
    cx.update(|window, cx| window.draw(cx).clear(cx));
}

fn shows(id: &str, cx: &mut VisualTestContext) -> bool {
    let id = gpui_kit::SharedString::from(id.to_owned());
    cx.update(move |window, _| window.try_find(id).is_some())
}

fn click(id: &str, cx: &mut VisualTestContext) {
    let id = gpui_kit::SharedString::from(id.to_owned());
    cx.update(move |window, cx| window.click(id, cx));
    redraw(cx);
}

/// Clicks the field `id` and types `text` into it.
fn type_into(id: &str, text: &str, cx: &mut VisualTestContext) {
    click(id, cx);
    let text = text.to_owned();
    cx.update(move |window, cx| window.input(&text, cx));
    redraw(cx);
}

fn press(key: &str, cx: &mut VisualTestContext) {
    let key = key.to_owned();
    cx.update(move |window, cx| window.press(&key, cx));
    redraw(cx);
}

fn selected(accounts: &Entity<Accounts>, cx: &mut VisualTestContext) -> Option<AccountId> {
    cx.update(|_, cx| accounts.read(cx).selected())
}

fn row(id: AccountId) -> String {
    format!("account-{id}")
}

#[gpui_kit::test]
fn the_panel_lists_the_database_and_a_click_shows_the_account(cx: &mut TestAppContext) {
    let scratch = Scratch::new();
    let (shell, accounts, cx) = open(cx, &scratch);
    assert!(shows(&row(scratch.current), cx));
    assert!(shows(&row(scratch.pot), cx), "the pot is listed under it");
    // The Transactions fixture opens with a transaction selected.
    assert!(!shows("add-balance", cx));

    click(&row(scratch.current), cx);
    assert_eq!(selected(&accounts, cx), Some(scratch.current));
    let transactions_selected =
        cx.update(|_, cx| shell.read(cx).transactions().read(cx).selected().len());
    assert_eq!(transactions_selected, 0, "the account takes the inspector");
    assert!(shows("add-balance", cx));
    assert!(shows("account-name", cx));

    click(&row(scratch.current), cx);
    assert_eq!(
        selected(&accounts, cx),
        None,
        "a second click: all accounts"
    );
    assert!(!shows("add-balance", cx));
}

#[gpui_kit::test]
fn new_account_adds_and_selects_it(cx: &mut TestAppContext) {
    let scratch = Scratch::new();
    let (_shell, accounts, cx) = open(cx, &scratch);
    click(panel::ADD_ACCOUNT, cx);
    assert!(shows("new-account-add", cx));

    type_into("new-account-name", "Amex", cx);
    type_into("new-account-bank", "American Express", cx);
    type_into("new-account-balance", "-42.50", cx);
    click("new-account-add", cx);

    assert!(!shows("new-account-add", cx), "the dialog closes");
    let added = account::list(&scratch.db())
        .unwrap()
        .into_iter()
        .find(|account| account.name == "Amex")
        .expect("Amex was added");
    assert_eq!(added.institution.as_deref(), Some("American Express"));
    assert_eq!(selected(&accounts, cx), Some(added.id));
    let balances = account::entered_balances(&scratch.db(), added.id).unwrap();
    assert_eq!(balances.len(), 1);
    assert_eq!(balances[0].balance.minor(), -4_250);
    assert!(shows(&row(added.id), cx));
}

#[gpui_kit::test]
fn new_account_says_what_is_wrong_and_adds_nothing(cx: &mut TestAppContext) {
    let scratch = Scratch::new();
    let (shell, _accounts, cx) = open(cx, &scratch);
    click(panel::ADD_ACCOUNT, cx);

    // No name.
    click("new-account-add", cx);
    let error = cx.update(|_, cx| dialog_error(&shell, cx));
    assert_eq!(error.as_deref(), Some("An account needs a name."));

    // A balance that would need rounding.
    type_into("new-account-name", "Amex", cx);
    type_into("new-account-balance", "12.345", cx);
    click("new-account-add", cx);
    let error = cx.update(|_, cx| dialog_error(&shell, cx)).unwrap();
    assert!(error.starts_with("The balance isn't an amount"), "{error}");
    assert_eq!(account::list(&scratch.db()).unwrap().len(), 2);

    // Escape cancels.
    press("escape", cx);
    assert!(!shows("new-account-add", cx));
    assert_eq!(account::list(&scratch.db()).unwrap().len(), 2);
}

/// The open New account dialog's error.
fn dialog_error(shell: &Entity<ShellView>, cx: &gpui_kit::App) -> Option<String> {
    let dialog = shell.read(cx).dialog()?.clone();
    let dialog = dialog.downcast::<new_account::NewAccountDialog>().ok()?;
    dialog.read(cx).error().map(ToString::to_string)
}

#[gpui_kit::test]
fn the_inspector_saves_a_rename_and_refuses_a_blank_name(cx: &mut TestAppContext) {
    let scratch = Scratch::new();
    let (_shell, accounts, cx) = open(cx, &scratch);
    click(&row(scratch.current), cx);

    click("account-name", cx);
    press("end", cx);
    cx.update(|window, cx| window.input(" (Monzo)", cx));
    press("enter", cx);
    let name = account::get(&scratch.db(), scratch.current).unwrap().name;
    assert_eq!(name, "Current account (Monzo)");
    let shown = cx.update(|_, cx| {
        accounts
            .read(cx)
            .selected_node()
            .unwrap()
            .account
            .name
            .clone()
    });
    assert_eq!(shown, "Current account (Monzo)", "the panel reads it back");

    // Clear the field: core refuses, and the stored name comes back.
    click("account-name", cx);
    press("secondary-a", cx);
    press("backspace", cx);
    press("enter", cx);
    assert_eq!(
        account::get(&scratch.db(), scratch.current).unwrap().name,
        "Current account (Monzo)"
    );
    let field = cx.update(|window, _| window.find("account-name").value().map(ToString::to_string));
    assert_eq!(field.as_deref(), Some("Current account (Monzo)"));
}

#[gpui_kit::test]
fn add_balance_records_an_entered_balance(cx: &mut TestAppContext) {
    let scratch = Scratch::new();
    let (_shell, accounts, cx) = open(cx, &scratch);
    click(&row(scratch.current), cx);
    click("add-balance", cx);
    assert!(shows("add-balance-add", cx));

    type_into("add-balance-amount", "1,284.10", cx);
    // Replace today's date with one in the past.
    click("add-balance-date", cx);
    press("secondary-a", cx);
    cx.update(|window, cx| window.input("9 Nov 2026", cx));
    type_into("add-balance-note", "From the app", cx);
    click("add-balance-add", cx);

    assert!(!shows("add-balance-add", cx), "the dialog closes");
    let balances = account::entered_balances(&scratch.db(), scratch.current).unwrap();
    assert_eq!(balances.len(), 2);
    assert_eq!(
        balances[0].date,
        NaiveDate::from_ymd_opt(2026, 11, 9).unwrap()
    );
    assert_eq!(balances[0].balance.minor(), 128_410);
    assert_eq!(balances[0].note.as_deref(), Some("From the app"));
    // The inspector lists it.
    let listed = cx.update(|_, cx| {
        accounts
            .read(cx)
            .data()
            .detail(scratch.current)
            .unwrap()
            .balances
            .len()
    });
    assert_eq!(listed, 2);
}

#[gpui_kit::test]
fn sample_data_cant_change(cx: &mut TestAppContext) {
    cx.update(|cx| {
        let accounts = cx.new(|_| Accounts::fixture(fixture::accounts()));
        let result = accounts.update(cx, |accounts, cx| {
            accounts.create(
                &NewAccount::new("X", AccountKind::Cash, Currency::GBP),
                None,
                cx,
            )
        });
        assert!(result.is_err());
    });
}

#[gpui_kit::test]
fn the_arrow_keys_move_through_the_panel(cx: &mut TestAppContext) {
    let scratch = Scratch::new();
    let (_shell, accounts, cx) = open(cx, &scratch);
    click(&row(scratch.current), cx);
    press("down", cx);
    assert_eq!(
        selected(&accounts, cx),
        Some(scratch.pot),
        "the pot is next"
    );
    press("down", cx);
    assert_eq!(
        selected(&accounts, cx),
        Some(scratch.pot),
        "it stops at the end"
    );
    press("up", cx);
    assert_eq!(selected(&accounts, cx), Some(scratch.current));
    press("escape", cx);
    assert_eq!(selected(&accounts, cx), None, "Escape: all accounts");
}

#[gpui_kit::test]
fn the_kind_select_changes_the_kind_by_pointer_and_keyboard(cx: &mut TestAppContext) {
    let scratch = Scratch::new();
    // An account with nothing in it, so every kind can be chosen.
    let empty = account::create(
        &mut scratch.db(),
        &NewAccount::new("Card", AccountKind::Current, Currency::GBP),
    )
    .unwrap();
    let (_shell, accounts, cx) = open(cx, &scratch);
    click(&row(empty), cx);

    click("inspector-kind", cx);
    assert!(shows("option-credit_card", cx), "the menu is open");
    click("option-credit_card", cx);
    assert!(!shows("option-credit_card", cx), "choosing closes it");
    assert_eq!(
        account::get(&scratch.db(), empty).unwrap().kind,
        AccountKind::CreditCard
    );

    // From the keyboard: the focused trigger opens on Enter, the arrows
    // move, Enter chooses (Credit card → Cash), Escape would close.
    press("enter", cx);
    assert!(shows("option-cash", cx));
    press("down", cx);
    press("enter", cx);
    assert_eq!(
        account::get(&scratch.db(), empty).unwrap().kind,
        AccountKind::Cash
    );
    let shown = cx.update(|_, cx| accounts.read(cx).selected_node().unwrap().account.kind);
    assert_eq!(shown, AccountKind::Cash);
    assert!(!shows("option-cash", cx), "choosing closes it");
    // Focus is back on the select: Enter opens it again, Escape closes it.
    press("enter", cx);
    assert!(shows("option-cash", cx));
    press("escape", cx);
    assert!(!shows("option-cash", cx));

    // Pressing the field while the menu is open closes it.
    click("inspector-kind", cx);
    assert!(shows("option-cash", cx));
    click("inspector-kind", cx);
    assert!(!shows("option-cash", cx), "a second press closes it");
}

#[gpui_kit::test]
fn an_edit_left_by_clicking_another_account_stays_with_its_account(cx: &mut TestAppContext) {
    let scratch = Scratch::new();
    let (_shell, accounts, cx) = open(cx, &scratch);
    click(&row(scratch.current), cx);
    click("account-name", cx);
    press("end", cx);
    cx.update(|window, cx| window.input(" 2", cx));
    // No Enter: the next click takes focus and changes the selection.
    click(&row(scratch.pot), cx);
    assert_eq!(selected(&accounts, cx), Some(scratch.pot));

    let db = scratch.db();
    assert_eq!(
        account::get(&db, scratch.current).unwrap().name,
        "Current account 2"
    );
    assert_eq!(
        account::get(&db, scratch.pot).unwrap().name,
        "Holiday",
        "the pot is untouched"
    );
    let field = cx.update(|window, _| window.find("account-name").value().map(ToString::to_string));
    assert_eq!(field.as_deref(), Some("Holiday"));
}

// Assets, liabilities, closing and deleting (M3.17) ---------------------------

/// Adds a valued account to the scratch database, with one value.
fn valued(scratch: &Scratch, name: &str, kind: AccountKind, minor: i64) -> AccountId {
    let value = EnteredBalance {
        date: NaiveDate::from_ymd_opt(2026, 10, 12).unwrap(),
        balance: Money::new(minor, Currency::GBP),
        source: EnteredSource::Valuation,
        note: None,
    };
    account::create_with_balance(
        &mut scratch.db(),
        &NewAccount::new(name, kind, Currency::GBP),
        Some(&value),
    )
    .unwrap()
}

/// Replaces the date in the field `id` with `date`.
fn retype_date(id: &str, date: &str, cx: &mut VisualTestContext) {
    click(id, cx);
    press("secondary-a", cx);
    let date = date.to_owned();
    cx.update(move |window, cx| window.input(&date, cx));
    redraw(cx);
}

/// The open dialog's error, if it is a `D`.
fn error_of<D: gpui_kit::Render>(
    shell: &Entity<ShellView>,
    cx: &mut VisualTestContext,
    error: impl Fn(&D) -> Option<String> + 'static,
) -> Option<String> {
    cx.update(|_, cx| {
        let dialog = shell.read(cx).dialog()?.clone();
        let dialog = dialog.downcast::<D>().ok()?;
        error(dialog.read(cx))
    })
}

#[gpui_kit::test]
fn the_assets_plus_adds_an_asset_shown_by_its_values(cx: &mut TestAppContext) {
    let scratch = Scratch::new();
    let (_shell, accounts, cx) = open(cx, &scratch);
    click(panel::ADD_VALUED_ACCOUNT, cx);
    // Kind is Asset: no Bank, and a Note.
    assert!(shows("new-account-add", cx));
    assert!(!shows("new-account-bank", cx));
    assert!(shows("new-account-note", cx));
    type_into("new-account-name", "House", cx);
    type_into("new-account-balance", "385,000", cx);
    retype_date("new-account-date", "12 Oct 2026", cx);
    type_into("new-account-note", "Remortgage survey", cx);
    click("new-account-add", cx);
    assert!(!shows("new-account-add", cx), "the dialog closes");

    let house = account::list(&scratch.db())
        .unwrap()
        .into_iter()
        .find(|account| account.name == "House")
        .expect("House was added");
    assert_eq!(house.kind, AccountKind::Asset);
    let values = account::entered_balances(&scratch.db(), house.id).unwrap();
    assert_eq!(values.len(), 1);
    assert_eq!(values[0].balance.minor(), 38_500_000);
    assert_eq!(
        values[0].source,
        coffer_core::reconciliation::BalanceSource::Valuation
    );
    assert_eq!(values[0].note.as_deref(), Some("Remortgage survey"));
    assert_eq!(selected(&accounts, cx), Some(house.id));

    // Its values replace the transaction table, and ⌘N adds one.
    assert!(shows("add-valuation", cx));
    // Clicking the row twice lets it go and selects it again, with the
    // panel focused: ⌘N works there.
    click(&row(house.id), cx);
    click(&row(house.id), cx);
    assert_eq!(selected(&accounts, cx), Some(house.id));
    press("secondary-n", cx);
    assert!(shows("add-balance-add", cx), "⌘N opens Add valuation");
    type_into("add-balance-amount", "390,000", cx);
    retype_date("add-balance-date", "1 Nov 2026", cx);
    click("add-balance-add", cx);
    assert!(!shows("add-balance-add", cx));
    let values = account::entered_balances(&scratch.db(), house.id).unwrap();
    assert_eq!(values[0].balance.minor(), 39_000_000);

    // A bank account goes back to the table.
    click(&row(scratch.current), cx);
    assert!(!shows("add-valuation", cx));
}

#[gpui_kit::test]
fn a_liability_stores_what_is_owed_below_zero(cx: &mut TestAppContext) {
    let scratch = Scratch::new();
    let mortgage = valued(&scratch, "Mortgage", AccountKind::Liability, -21_691_000);
    let (shell, _accounts, cx) = open(cx, &scratch);
    click(&row(mortgage), cx);
    click("add-balance", cx);
    // A minus is refused: Coffer stores what's owed below zero itself.
    type_into("add-balance-amount", "-1", cx);
    retype_date("add-balance-date", "1 Nov 2026", cx);
    click("add-balance-add", cx);
    let error = error_of(&shell, cx, |dialog: &add_balance::AddBalanceDialog| {
        dialog.error().map(ToString::to_string)
    });
    assert!(error.unwrap().contains("without a minus"));

    click("add-balance-amount", cx);
    press("secondary-a", cx);
    cx.update(|window, cx| window.input("212,640", cx));
    click("add-balance-add", cx);
    assert!(!shows("add-balance-add", cx));
    let values = account::entered_balances(&scratch.db(), mortgage).unwrap();
    assert_eq!(values[0].balance.minor(), -21_264_000);
}

#[gpui_kit::test]
fn closing_an_asset_hides_it_and_undo_reopens_it(cx: &mut TestAppContext) {
    let scratch = Scratch::new();
    let house = valued(&scratch, "House", AccountKind::Asset, 38_500_000);
    let (shell, accounts, cx) = open(cx, &scratch);
    click(&row(house), cx);
    click("close-account", cx);
    assert!(shows("close-account-close", cx));

    // Not on or before its last value.
    retype_date("close-account-date", "12 Oct 2026", cx);
    click("close-account-close", cx);
    let error = error_of(&shell, cx, |dialog: &close::CloseAccountDialog| {
        dialog.error().map(ToString::to_string)
    })
    .expect("refused");
    assert!(error.contains("2026-10-13"), "{error}");
    assert_eq!(account::get(&scratch.db(), house).unwrap().closed_on, None);

    retype_date("close-account-date", "6 Nov 2026", cx);
    click("close-account-close", cx);
    assert!(!shows("close-account-close", cx), "the dialog closes");
    assert_eq!(
        account::get(&scratch.db(), house).unwrap().closed_on,
        NaiveDate::from_ymd_opt(2026, 11, 6)
    );
    assert!(!shows(&row(house), cx), "it leaves the panel");
    assert_eq!(selected(&accounts, cx), None);
    let message = cx.update(|_, cx| {
        accounts
            .read(cx)
            .closed()
            .map(|closed| closed.message.clone())
    });
    assert_eq!(message.as_deref(), Some("House closed"));

    // Undo reopens it, without the £0 value closing added.
    click("undo-change", cx);
    assert_eq!(account::get(&scratch.db(), house).unwrap().closed_on, None);
    assert_eq!(
        account::entered_balances(&scratch.db(), house)
            .unwrap()
            .len(),
        1
    );
    assert!(shows(&row(house), cx));
    assert_eq!(selected(&accounts, cx), Some(house));
    assert!(!shows("undo-change", cx));
}

#[gpui_kit::test]
fn delete_shows_only_where_core_allows_it(cx: &mut TestAppContext) {
    let scratch = Scratch::new();
    let house = valued(&scratch, "House", AccountKind::Asset, 38_500_000);
    let (_shell, accounts, cx) = open(cx, &scratch);

    // Current account has a pot: close only.
    click(&row(scratch.current), cx);
    assert!(shows("close-account", cx));
    assert!(!shows("delete-account", cx));

    click(&row(house), cx);
    click("delete-account", cx);
    assert!(shows("delete-account-delete", cx));
    click("delete-account-delete", cx);
    assert!(!shows("delete-account-delete", cx));
    assert!(account::get(&scratch.db(), house).is_err(), "deleted");
    assert!(!shows(&row(house), cx));
    assert_eq!(selected(&accounts, cx), None);
}
