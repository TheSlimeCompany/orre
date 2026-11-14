//! The Delete account dialog, opened by "Delete account…" in the account
//! inspector, which shows only on an account core lets go
//! ([`coffer_core::account::close::deletion`]: no transactions, imports,
//! pots, rules or schedules, and no lock; `ARCHITECTURE.md` → "Decision
//! record: accounts you keep yourself", point 4).
//!
//! Composed for M3.17, not drawn (decided with the user, 2026-11-14), on
//! `--container-dialog`: "Delete House?", the body "Its 5 values go with
//! it. This can't be undone; to keep its history, close it instead.", then
//! Cancel and the destructive solid "Delete account". A refusal is said in
//! `danger` under the body. Nothing shows after deleting: there's nothing
//! to undo.

use coffer_core::account::{AccountKind, Deletion};
use coffer_core::id::AccountId;
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::{
    Context, Entity, EventEmitter, FocusHandle, IntoElement, Render, SharedString, Window,
};

use crate::theme::tokens::color;
use crate::ui::button::{Button, ButtonVariant};
use crate::ui::dialog::Dialog;
use crate::ui::field::{caption, error_caption};

use super::new_account::sentence;
use super::{Accounts, DialogClosed};

/// The body: what goes with the account.
pub fn body(kind: AccountKind, deletion: Deletion) -> String {
    let (one, many) = if kind.holds_transactions() {
        ("balance", "balances")
    } else {
        ("value", "values")
    };
    let goes = match deletion.values {
        0 => String::new(),
        1 => format!("Its {one} goes with it. "),
        n => format!("Its {n} {many} go with it. "),
    };
    format!("{goes}This can't be undone; to keep its history, close it instead.")
}

/// Delete account, for one account.
pub struct DeleteAccountDialog {
    accounts: Entity<Accounts>,
    account: AccountId,
    title: SharedString,
    body: String,
    focus: FocusHandle,
    error: Option<SharedString>,
}

impl EventEmitter<DialogClosed> for DeleteAccountDialog {}

impl DeleteAccountDialog {
    /// The dialog for `account`, which must be open.
    pub fn new(
        accounts: Entity<Accounts>,
        account: AccountId,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let data = accounts.read(cx).data();
        let (title, body) = match data.node(account) {
            Some(node) => (
                format!("Delete {}?", node.account.name).into(),
                body(
                    node.account.kind,
                    data.detail(account)
                        .map(|detail| detail.deletion)
                        .unwrap_or_default(),
                ),
            ),
            None => ("Delete account?".into(), String::new()),
        };
        let focus = cx.focus_handle();
        focus.focus(window, cx);
        Self {
            accounts,
            account,
            title,
            body,
            focus,
            error: None,
        }
    }

    pub fn error(&self) -> Option<&SharedString> {
        self.error.as_ref()
    }

    /// Deletes the account, or says why it can't.
    pub fn submit(&mut self, cx: &mut Context<Self>) {
        let account = self.account;
        let result = self
            .accounts
            .update(cx, |accounts, cx| accounts.delete(account, cx));
        match result {
            Ok(()) => cx.emit(DialogClosed),
            Err(error) => {
                self.error = Some(sentence(&error));
                cx.notify();
            }
        }
    }

    pub fn cancel(&mut self, cx: &mut Context<Self>) {
        cx.emit(DialogClosed);
    }
}

impl Render for DeleteAccountDialog {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let cancel = cx.listener(|this, _, _, cx| this.cancel(cx));
        let delete = cx.listener(|this, _, _, cx| this.submit(cx));
        let on_cancel = cx.entity().downgrade();
        Dialog::new(self.title.clone(), &self.focus)
            .body(caption(self.body.clone()))
            .when_some(self.error.clone(), |this, error| {
                this.body(error_caption(error))
            })
            .action(
                Button::new("delete-account-cancel", "Cancel")
                    .with_variant(ButtonVariant::Secondary)
                    .with_surface(color::FLOAT)
                    .on_click(cancel),
            )
            .action(
                Button::new("delete-account-delete", "Delete account")
                    .with_variant(ButtonVariant::DestructiveSolid)
                    .with_surface(color::FLOAT)
                    .on_click(delete),
            )
            .on_cancel(move |_, cx| {
                let _ = on_cancel.update(cx, |this, cx| this.cancel(cx));
            })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_body_counts_what_goes() {
        let deletion = |values| Deletion {
            values,
            refused: None,
        };
        assert_eq!(
            body(AccountKind::Asset, deletion(5)),
            "Its 5 values go with it. This can't be undone; to keep its history, close it instead."
        );
        assert_eq!(
            body(AccountKind::Current, deletion(1)),
            "Its balance goes with it. This can't be undone; to keep its history, close it instead."
        );
        assert_eq!(
            body(AccountKind::Cash, deletion(0)),
            "This can't be undone; to keep its history, close it instead."
        );
    }
}
