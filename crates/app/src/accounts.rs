//! Accounts management (ROADMAP M3.2; `docs/DESIGN.md` → "Accounts panel";
//! `docs/ARCHITECTURE.md` → "Decision record: accounts management").
//!
//! - [`panel`]: the left panel of the money views: net worth, the accounts
//!   with their pots nested under them, and the categories.
//! - [`inspector`]: the account inspector, shown on Transactions while an
//!   account is selected and no transaction is: its details (name, bank,
//!   kind, currency, preset), entered balances, pots and imports.
//! - [`new_account`] and [`add_balance`]: the New account and Add balance
//!   dialogs; Add balance is also "Add valuation" for an asset and "Add a
//!   balance" (what's owed) for a liability (M3.17).
//! - [`valuations`]: what replaces the transaction table while an asset or
//!   liability account is selected: its values, as a step-line chart and a
//!   table (M3.17).
//! - [`close`] and [`delete`]: the Close and Delete account dialogs (M3.17).
//!
//! After closing, the undo bar says so and Undo reopens the account
//! ([`notice_layer`]); a closed account is reopened later with `coffer
//! accounts reopen` (decided with the user, 2026-11-14).
//!
//! [`Accounts`] holds what they show and which account is selected, and
//! makes every change through [`store::AccountStore`], which opens the
//! database for each call; core decides what's allowed. Screens drawn
//! without a database (snapshots, tests) use [`fixture`] data, which can't
//! change.

pub mod add_balance;
pub mod close;
pub mod data;
pub mod delete;
pub mod fixture;
pub mod inspector;
pub mod new_account;
pub mod panel;
pub mod store;
pub mod valuations;

gpui_kit::actions!(
    coffer_accounts,
    [
        /// ⌘N while an asset or liability account is selected: add a
        /// valuation (`DESIGN.md` → "Accounts you keep yourself"). On other
        /// accounts the key goes on to whoever binds it there (M3.10).
        AddValuation,
    ]
);

/// Binds the accounts panel's and the valuations canvas's keys. Call once,
/// before the first panel.
pub fn init(cx: &mut gpui_kit::App) {
    use std::rc::Rc;

    use gpui_kit::base::actions::{Cancel, SelectDown, SelectUp};
    use gpui_kit::{KeyBinding, KeyBindingContextPredicate};
    cx.bind_keys([
        KeyBinding::new("up", SelectUp, Some(panel::CONTEXT)),
        KeyBinding::new("down", SelectDown, Some(panel::CONTEXT)),
        KeyBinding::new("escape", Cancel, Some(panel::CONTEXT)),
    ]);
    // ⌘N (Ctrl elsewhere): `KeyBinding::new` can't resolve `secondary-`,
    // so `load` asks the platform, as the shell's shortcuts do.
    let bindings = [panel::CONTEXT, valuations::CONTEXT].map(|context| {
        let context =
            Rc::new(KeyBindingContextPredicate::parse(context).expect("the key context parses"));
        KeyBinding::load(
            "secondary-n",
            Box::new(AddValuation),
            Some(context),
            false,
            None,
            cx,
        )
        .expect("⌘N parses")
    });
    cx.bind_keys(bindings);
}

use chrono::NaiveDate;
use coffer_core::account::{AccountEdit, EnteredBalance, NewAccount};
use coffer_core::id::AccountId;
use coffer_core::reporting::account_tree::AccountNode;
use gpui_kit::{Context, EventEmitter, SharedString};

use data::AccountsData;
use store::AccountStore;

/// Today, where the computer's clock can be read.
fn today() -> chrono::NaiveDate {
    coffer_core::schedule::local_today().unwrap_or_else(|_| fixture::today())
}

/// Where the accounts come from.
enum Source {
    /// The database: what `coffer` opens.
    Database(AccountStore),
    /// Sample data that can't change.
    Fixture,
}

/// Something the panel or inspector asked for that the shell carries out.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AccountsEvent {
    /// The selected account changed (`None`: all accounts).
    Selected(Option<AccountId>),
    /// The `+` on the Accounts section: open New account.
    NewAccountRequested,
    /// The `+` on the Assets and liabilities section: open New account
    /// with Kind set to Asset.
    NewValuedAccountRequested,
    /// "Add balance…" in the inspector (or "Add valuation…", ⌘N): open
    /// Add balance for the account.
    AddBalanceRequested(AccountId),
    /// "Close account…" in the inspector.
    CloseRequested(AccountId),
    /// "Delete account…" in the inspector.
    DeleteRequested(AccountId),
    /// The flagged dot in the panel: open the account's failing balance
    /// check in the Inbox's Balance checks (`DESIGN.md` → "Review queues").
    ChecksRequested(AccountId),
}

/// A dialog is done: the change was saved, or the user cancelled. The
/// shell closes it and puts focus back.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DialogClosed;

/// An account just closed: the undo bar's message, and what Undo reopens.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Closed {
    pub account: AccountId,
    /// "House closed".
    pub message: SharedString,
}

/// The accounts the app shows, and which one is selected.
pub struct Accounts {
    source: Source,
    data: AccountsData,
    selected: Option<AccountId>,
    /// Why the accounts couldn't be read, shown in the panel.
    load_error: Option<SharedString>,
    /// The account closed last, until the bar is dismissed.
    closed: Option<Closed>,
    /// Why Undo couldn't reopen it, said in the bar.
    closed_error: Option<SharedString>,
}

impl EventEmitter<AccountsEvent> for Accounts {}

impl Accounts {
    /// The accounts in `store`, read now.
    pub fn from_store(store: AccountStore) -> Self {
        let mut accounts = Self {
            data: AccountsData::empty(Vec::new(), today()),
            source: Source::Database(store),
            selected: None,
            load_error: None,
            closed: None,
            closed_error: None,
        };
        accounts.load();
        accounts
    }

    /// No accounts, because they can't be read: `reason` shows in the
    /// panel.
    pub fn unavailable(reason: impl Into<SharedString>) -> Self {
        Self {
            source: Source::Fixture,
            data: AccountsData::empty(Vec::new(), today()),
            selected: None,
            load_error: Some(reason.into()),
            closed: None,
            closed_error: None,
        }
    }

    /// Sample data, which can't change.
    pub fn fixture(data: AccountsData) -> Self {
        Self {
            source: Source::Fixture,
            data,
            selected: None,
            load_error: None,
            closed: None,
            closed_error: None,
        }
    }

    /// Shows the undo bar for `closed`, as closing does (snapshots).
    pub fn with_closed(mut self, closed: Closed) -> Self {
        self.closed = Some(closed);
        self
    }

    /// The account closed last, while its undo bar shows.
    pub fn closed(&self) -> Option<&Closed> {
        self.closed.as_ref()
    }

    /// Why Undo couldn't reopen the account.
    pub fn closed_error(&self) -> Option<&SharedString> {
        self.closed_error.as_ref()
    }

    pub fn data(&self) -> &AccountsData {
        &self.data
    }

    pub fn selected(&self) -> Option<AccountId> {
        self.selected
    }

    /// The selected account, if it's still open.
    pub fn selected_node(&self) -> Option<&AccountNode> {
        self.selected.and_then(|id| self.data.node(id))
    }

    pub fn load_error(&self) -> Option<&SharedString> {
        self.load_error.as_ref()
    }

    /// Selects `account` (`None`: all accounts).
    pub fn select(&mut self, account: Option<AccountId>, cx: &mut Context<Self>) {
        if self.selected == account {
            return;
        }
        self.selected = account;
        cx.emit(AccountsEvent::Selected(account));
        cx.notify();
    }

    /// Selects `account`, or all accounts if it's selected already: what a
    /// click on a panel row does.
    pub fn toggle(&mut self, account: AccountId, cx: &mut Context<Self>) {
        let next = (self.selected != Some(account)).then_some(account);
        self.select(next, cx);
    }

    /// Moves the selection `step` rows down the panel (up when negative),
    /// stopping at the ends; with nothing selected, down selects the first
    /// row and up the last. The arrow keys in the panel.
    pub fn select_next(&mut self, step: isize, cx: &mut Context<Self>) {
        let rows: Vec<AccountId> = panel::order(&self.data);
        if rows.is_empty() {
            return;
        }
        let next = match self
            .selected
            .and_then(|id| rows.iter().position(|row| *row == id))
        {
            Some(at) => at.saturating_add_signed(step).min(rows.len() - 1),
            None if step < 0 => rows.len() - 1,
            None => 0,
        };
        self.select(Some(rows[next]), cx);
    }

    /// Asks the shell to open New account.
    pub fn request_new_account(&mut self, cx: &mut Context<Self>) {
        cx.emit(AccountsEvent::NewAccountRequested);
    }

    /// Asks the shell to open New account with Kind set to Asset.
    pub fn request_new_valued_account(&mut self, cx: &mut Context<Self>) {
        cx.emit(AccountsEvent::NewValuedAccountRequested);
    }

    /// Asks the shell to open Add balance for `account`.
    pub fn request_add_balance(&mut self, account: AccountId, cx: &mut Context<Self>) {
        cx.emit(AccountsEvent::AddBalanceRequested(account));
    }

    /// ⌘N: asks for Add valuation when the selected account is an asset or
    /// liability. Returns whether it did, so the key can go on otherwise.
    pub fn request_add_valuation(&mut self, cx: &mut Context<Self>) -> bool {
        match self.selected_node() {
            Some(node) if !node.account.kind.holds_transactions() => {
                let id = node.account.id;
                self.request_add_balance(id, cx);
                true
            }
            _ => false,
        }
    }

    /// Asks the shell to open Close account for `account`.
    pub fn request_close(&mut self, account: AccountId, cx: &mut Context<Self>) {
        cx.emit(AccountsEvent::CloseRequested(account));
    }

    /// Asks the shell to open Delete account for `account`.
    pub fn request_delete(&mut self, account: AccountId, cx: &mut Context<Self>) {
        cx.emit(AccountsEvent::DeleteRequested(account));
    }

    /// The flagged dot: ask the shell to open the account's balance checks.
    pub fn request_checks(&mut self, account: AccountId, cx: &mut Context<Self>) {
        cx.emit(AccountsEvent::ChecksRequested(account));
    }

    /// Reads the accounts again. A selected account that has gone (closed
    /// or deleted) is let go, and the views filtering by it are told.
    pub fn reload(&mut self, cx: &mut Context<Self>) {
        let before = self.selected;
        self.load();
        if before.is_some() && self.selected.is_none() {
            cx.emit(AccountsEvent::Selected(None));
        }
        cx.notify();
    }

    fn load(&mut self) {
        let Source::Database(store) = &self.source else {
            return;
        };
        match store.load(store.presets()) {
            Ok(data) => {
                self.data = data;
                self.load_error = None;
            }
            Err(error) => {
                tracing::warn!(%error, "couldn't read the accounts");
                self.load_error = Some(error.to_string().into());
            }
        }
        if self.selected.is_some_and(|id| !self.data.contains(id)) {
            self.selected = None;
        }
    }

    fn store(&self) -> Result<&AccountStore, SharedString> {
        match &self.source {
            Source::Database(store) => Ok(store),
            Source::Fixture => Err("This is sample data, which can't be changed.".into()),
        }
    }

    /// Adds an account (and its first balance), then selects it. The error
    /// says why it wasn't added.
    pub fn create(
        &mut self,
        account: &NewAccount,
        balance: Option<&EnteredBalance>,
        cx: &mut Context<Self>,
    ) -> Result<AccountId, SharedString> {
        let id = self
            .store()?
            .create(account, balance)
            .map_err(|error| SharedString::from(error.to_string()))?;
        self.reload(cx);
        self.select(Some(id), cx);
        Ok(id)
    }

    /// Changes an account's details. The error says why they weren't.
    pub fn update(
        &mut self,
        account: AccountId,
        edit: &AccountEdit,
        cx: &mut Context<Self>,
    ) -> Result<(), SharedString> {
        self.store()?
            .update(account, edit)
            .map_err(|error| SharedString::from(error.to_string()))?;
        self.reload(cx);
        Ok(())
    }

    /// Closes `account` on `on`, with its open pots, and shows the undo
    /// bar. The error says why it wasn't closed.
    pub fn close(
        &mut self,
        account: AccountId,
        on: NaiveDate,
        cx: &mut Context<Self>,
    ) -> Result<(), SharedString> {
        let name = self
            .data
            .node(account)
            .map(|node| node.account.name.clone())
            .unwrap_or_default();
        self.store()?
            .close(account, on)
            .map_err(|error| SharedString::from(error.to_string()))?;
        self.closed = Some(Closed {
            account,
            message: format!("{name} closed").into(),
        });
        self.closed_error = None;
        self.reload(cx);
        Ok(())
    }

    /// Undo on the bar: reopens the account just closed and selects it.
    pub fn undo_close(&mut self, cx: &mut Context<Self>) {
        let Some(closed) = self.closed.clone() else {
            return;
        };
        let reopened = self.store().and_then(|store| {
            store
                .reopen(closed.account)
                .map_err(|error| SharedString::from(error.to_string()))
        });
        match reopened {
            Ok(_) => {
                self.closed = None;
                self.closed_error = None;
                self.reload(cx);
                self.select(Some(closed.account), cx);
            }
            Err(error) => {
                self.closed_error = Some(new_account::sentence(&error));
                cx.notify();
            }
        }
    }

    /// Closes the undo bar; the account stays closed.
    pub fn dismiss_closed(&mut self, cx: &mut Context<Self>) {
        self.closed = None;
        self.closed_error = None;
        cx.notify();
    }

    /// Deletes `account` and its values. The error says why it wasn't.
    pub fn delete(
        &mut self,
        account: AccountId,
        cx: &mut Context<Self>,
    ) -> Result<(), SharedString> {
        self.store()?
            .delete(account)
            .map_err(|error| SharedString::from(error.to_string()))?;
        self.reload(cx);
        Ok(())
    }

    /// Records a balance for `account`. The error says why it wasn't.
    pub fn add_balance(
        &mut self,
        account: AccountId,
        balance: &EnteredBalance,
        cx: &mut Context<Self>,
    ) -> Result<(), SharedString> {
        self.store()?
            .add_balance(account, balance)
            .map_err(|error| SharedString::from(error.to_string()))?;
        self.reload(cx);
        Ok(())
    }
}

/// The undo bar after closing an account, centred over the foot of the
/// window's body like the import notification: "House closed" and Undo,
/// which reopens it. `None` when no account was just closed.
pub fn notice_layer(
    accounts: &gpui_kit::Entity<Accounts>,
    cx: &gpui_kit::App,
) -> Option<gpui_kit::AnyElement> {
    use gpui_kit::{IntoElement as _, ParentElement as _, Styled as _, div};

    use crate::theme::tokens::spacing;
    use crate::ui::undo_bar::UndoBar;

    let state = accounts.read(cx);
    let closed = state.closed()?;
    let message = state
        .closed_error()
        .cloned()
        .unwrap_or_else(|| closed.message.clone());
    let (undo, dismiss) = (accounts.downgrade(), accounts.downgrade());
    Some(
        div()
            .absolute()
            .left_0()
            .right_0()
            .bottom(spacing::S6)
            .flex()
            .justify_center()
            .child(
                UndoBar::new(message)
                    .on_undo(move |_, _, cx| {
                        let _ = undo.update(cx, |accounts, cx| accounts.undo_close(cx));
                    })
                    .on_dismiss(move |_, _, cx| {
                        let _ = dismiss.update(cx, |accounts, cx| accounts.dismiss_closed(cx));
                    }),
            )
            .into_any_element(),
    )
}

#[cfg(test)]
mod tests;
