//! The Source section's confirmations (ROADMAP M3.10), `--container-dialog`
//! wide: undoing an import and deleting a manual transaction.
//!
//! Composed for M3.10 from the dialog spec and the CLI's `orre
//! undo-import`, not drawn (`DESIGN.md` says "Undo import… opens the
//! existing batch-undo confirmation", and the app had none: the import
//! notification's Undo runs at once, right after importing):
//!
//! - **Undo import**: "Undo the import of monzo-november.csv?", then what
//!   goes, from core's [`import::undo_preview`]: the transactions (and those
//!   it created in pots), balances and flags; the user's edits on those
//!   rows; transfer pairs dissolved; rows merged into them, which stay
//!   excluded; later imports into the account that relied on it. Then
//!   "Importing the file again brings the rows back, without your edits.",
//!   Cancel and the destructive solid "Undo import". A refusal (a locked
//!   period, already undone) is said in `danger`, and then there's nothing
//!   to confirm.
//! - **Delete transaction**: "Delete “Farmers' market”?", "It's removed
//!   from Cash. This can't be undone.", Cancel and the destructive solid
//!   "Delete transaction". Only manual rows have it; core refuses others.
//!
//! [`import::undo_preview`]: orre_core::import::undo_preview

use gpui_kit::component::Disableable as _;
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::{
    App, Context, Entity, EventEmitter, FocusHandle, IntoElement, ParentElement as _, Render,
    SharedString, Styled as _, Subscription, Window, div,
};
use orre_core::id::{BatchId, TransactionId};
use orre_core::import::UndoPreview;

use crate::accounts::DialogClosed;
use crate::theme::tokens::{color, spacing, typography};
use crate::ui::button::{Button, ButtonVariant};
use crate::ui::dialog::Dialog;
use crate::ui::field::{caption, error_caption};
use crate::ui::text::TextRoleExt as _;

use super::TransactionsView;
use super::arrived::DIALOG_CAPTION;
use super::linking;

fn plural(count: usize, one: &str, many: &str) -> String {
    if count == 1 {
        format!("1 {one}")
    } else {
        format!("{count} {many}")
    }
}

/// What undoing the import removes, one sentence per line, from `preview`.
/// `account` names the account it was imported into.
pub fn undo_lines(preview: &UndoPreview, account: &str) -> Vec<String> {
    let mut removed = vec![plural(preview.transactions, "transaction", "transactions")];
    if preview.balances > 0 {
        removed.push(plural(preview.balances, "balance", "balances"));
    }
    if preview.flags > 0 {
        removed.push(plural(preview.flags, "flagged row", "flagged rows"));
    }
    let removed = match removed.as_slice() {
        [only] => only.clone(),
        [rest @ .., last] => format!("{} and {last}", rest.join(", ")),
        [] => String::new(),
    };
    let mut lines = vec![format!("This removes {removed} from {account}.")];
    if preview.mirrored > 0 {
        lines.push(format!(
            "{} it added to pots {} too.",
            plural(preview.mirrored, "transaction", "transactions"),
            if preview.mirrored == 1 { "goes" } else { "go" }
        ));
    }
    if preview.edited > 0 {
        lines.push(format!(
            "{} your edits (categories, notes, tags, splits, shares, exclusions or links), \
             which go with them.",
            if preview.edited == 1 {
                "1 of them has".to_owned()
            } else {
                format!("{} of them have", preview.edited)
            }
        ));
    }
    if preview.transfers > 0 {
        lines.push(format!(
            "{} with other rows {} dissolved; the other side stays as an ordinary transaction.",
            plural(preview.transfers, "transfer pair", "transfer pairs"),
            if preview.transfers == 1 { "is" } else { "are" }
        ));
    }
    if preview.merged_into > 0 {
        lines.push(format!(
            "{} merged into these as duplicates {} excluded.",
            plural(preview.merged_into, "transaction", "other transactions"),
            if preview.merged_into == 1 {
                "stays"
            } else {
                "stay"
            }
        ));
    }
    if !preview.restored.is_empty() {
        let restored = preview.restored.len();
        lines.push(format!(
            "{} replaced by the bank's corrections in this import {} back as {} {}.",
            plural(restored, "transaction", "transactions"),
            if restored == 1 { "comes" } else { "come" },
            if restored == 1 { "it" } else { "they" },
            if restored == 1 { "was" } else { "were" },
        ));
    }
    if preview.superseded > 0 {
        lines.push(format!(
            "{} replaced by a later import's correction {} too; the correction stays.",
            plural(preview.superseded, "transaction", "transactions"),
            if preview.superseded == 1 {
                "goes"
            } else {
                "go"
            }
        ));
    }
    if !preview.later_batches.is_empty() {
        lines.push(
            "Later imports into this account skipped rows this one had, so those go too; \
             import those files again to bring them back."
                .to_owned(),
        );
    }
    lines
}

/// Undo import, for one batch.
pub struct UndoImportDialog {
    transactions: Entity<TransactionsView>,
    batch: BatchId,
    title: SharedString,
    lines: Vec<String>,
    /// The preview failed: nothing to confirm.
    refused: bool,
    focus: FocusHandle,
    error: Option<SharedString>,
}

impl EventEmitter<DialogClosed> for UndoImportDialog {}

impl UndoImportDialog {
    /// The dialog for `batch`, with what undoing it removes read now.
    pub fn new(
        transactions: Entity<TransactionsView>,
        batch: BatchId,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let preview = transactions.read(cx).undo_import_preview(batch);
        let lines = preview.as_ref().ok().map(|preview| {
            let account = transactions
                .read(cx)
                .register()
                .account(preview.batch.account)
                .map_or_else(|| "the account".to_owned(), |account| account.name.clone());
            undo_lines(preview, &account)
        });
        let focus = cx.focus_handle();
        focus.focus(window, cx);
        match preview {
            Ok(preview) => Self::preview(transactions, &preview, lines.unwrap_or_default(), focus),
            Err(error) => Self {
                transactions,
                batch,
                title: "Undo import?".into(),
                lines: Vec::new(),
                refused: true,
                focus,
                error: Some(error),
            },
        }
    }

    /// The dialog over `preview` (snapshots: the fixture has no batches).
    pub fn preview(
        transactions: Entity<TransactionsView>,
        preview: &UndoPreview,
        lines: Vec<String>,
        focus: FocusHandle,
    ) -> Self {
        Self {
            transactions,
            batch: preview.batch.id,
            title: format!(
                "Undo the import of {}?",
                linking::name(&preview.batch.source_file)
            )
            .into(),
            lines,
            refused: false,
            focus,
            error: None,
        }
    }

    pub fn error(&self) -> Option<&SharedString> {
        self.error.as_ref()
    }

    /// Undoes the import, or says why it can't.
    pub fn submit(&mut self, cx: &mut Context<Self>) {
        if self.refused {
            return;
        }
        let batch = self.batch;
        match self
            .transactions
            .update(cx, |view, cx| view.undo_import(batch, cx))
        {
            Ok(()) => cx.emit(DialogClosed),
            Err(error) => {
                self.error = Some(error);
                cx.notify();
            }
        }
    }

    pub fn cancel(&mut self, cx: &mut Context<Self>) {
        cx.emit(DialogClosed);
    }
}

impl Render for UndoImportDialog {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let on_cancel = cx.entity().downgrade();
        let lines = div()
            .flex()
            .flex_col()
            .gap(spacing::S1)
            .children(self.lines.iter().map(|line| {
                div()
                    .text_role(typography::BODY)
                    .text_color(color::TEXT_SECONDARY)
                    .child(line.clone())
            }));
        Dialog::new(self.title.clone(), &self.focus)
            .when(!self.lines.is_empty(), |this| {
                this.body(lines).body(caption(
                    "Importing the file again brings the rows back, without your edits.",
                ))
            })
            .when_some(self.error.clone(), |this, error| {
                this.body(error_caption(error))
            })
            .action(
                Button::new("undo-import-cancel", "Cancel")
                    .with_variant(ButtonVariant::Secondary)
                    .with_surface(color::FLOAT)
                    .on_click(cx.listener(|this, _, _, cx| this.cancel(cx))),
            )
            .when(!self.refused, |this| {
                this.action(
                    Button::new("undo-import-confirm", "Undo import")
                        .with_variant(ButtonVariant::DestructiveSolid)
                        .with_surface(color::FLOAT)
                        .on_click(cx.listener(|this, _, _, cx| this.submit(cx))),
                )
            })
            .on_cancel(move |_, cx| {
                let _ = on_cancel.update(cx, |this, cx| this.cancel(cx));
            })
    }
}

/// Delete transaction, for one manual row.
pub struct DeleteTransactionDialog {
    transactions: Entity<TransactionsView>,
    transaction: TransactionId,
    title: SharedString,
    body: String,
    focus: FocusHandle,
    error: Option<SharedString>,
    _observing: Subscription,
}

impl EventEmitter<DialogClosed> for DeleteTransactionDialog {}

impl DeleteTransactionDialog {
    pub fn new(
        transactions: Entity<TransactionsView>,
        transaction: TransactionId,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let register = transactions.read(cx).register();
        let row = register.row(transaction);
        let title = row.map_or_else(
            || "Delete this transaction?".to_owned(),
            |row| format!("Delete “{}”?", linking::name(&row.merchant)),
        );
        let account = row
            .and_then(|row| register.account(row.account))
            .map_or_else(|| "its account".to_owned(), |account| account.name.clone());
        let focus = cx.focus_handle();
        focus.focus(window, cx);
        // The row may go in a read from another process: the primary and
        // the caption follow the view.
        let observing = cx.observe(&transactions, |_, _, cx| cx.notify());
        Self {
            transactions,
            transaction,
            title: title.into(),
            body: format!("It's removed from {account}. This can't be undone."),
            focus,
            error: None,
            _observing: observing,
        }
    }

    pub fn error(&self) -> Option<&SharedString> {
        self.error.as_ref()
    }

    /// Whether the row went in a read from another process: the primary is
    /// disabled and the caption says so; Cancel still closes.
    pub fn removed(&self, cx: &App) -> bool {
        self.transactions.read(cx).row_removed(self.transaction)
    }

    pub fn submit(&mut self, cx: &mut Context<Self>) {
        if self.removed(cx) {
            return;
        }
        let transaction = self.transaction;
        match self
            .transactions
            .update(cx, |view, cx| view.delete_manual(transaction, cx))
        {
            Ok(()) => cx.emit(DialogClosed),
            Err(error) => {
                self.error = Some(error);
                cx.notify();
            }
        }
    }

    pub fn cancel(&mut self, cx: &mut Context<Self>) {
        cx.emit(DialogClosed);
    }
}

impl Render for DeleteTransactionDialog {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let on_cancel = cx.entity().downgrade();
        let removed = self.removed(cx);
        Dialog::new(self.title.clone(), &self.focus)
            .body(caption(self.body.clone()))
            .when_some(self.error.clone(), |this, error| {
                this.body(error_caption(error))
            })
            .when(removed, |this| this.body(caption(DIALOG_CAPTION)))
            .action(
                Button::new("delete-transaction-cancel", "Cancel")
                    .with_variant(ButtonVariant::Secondary)
                    .with_surface(color::FLOAT)
                    .on_click(cx.listener(|this, _, _, cx| this.cancel(cx))),
            )
            .action(
                Button::new("delete-transaction-confirm", "Delete transaction")
                    .with_variant(ButtonVariant::DestructiveSolid)
                    .with_surface(color::FLOAT)
                    .disabled(removed)
                    .on_click(cx.listener(|this, _, _, cx| this.submit(cx))),
            )
            .on_cancel(move |_, cx| {
                let _ = on_cancel.update(cx, |this, cx| this.cancel(cx));
            })
    }
}

#[cfg(test)]
mod tests {
    use orre_core::id::AccountId;
    use orre_core::import::{BatchInfo, BatchStatus};
    use orre_core::statement::SourceFormat;

    use super::*;

    fn preview() -> UndoPreview {
        UndoPreview {
            batch: BatchInfo {
                id: BatchId::new(2),
                account: AccountId::new(1),
                source_file: "monzo-november.csv".to_owned(),
                format: SourceFormat::Csv,
                preset_id: None,
                imported_at: "2026-11-09T08:12:00.000Z".to_owned(),
                status: BatchStatus::Committed,
                transactions: 296,
            },
            transactions: 296,
            mirrored: 0,
            balances: 1,
            flags: 0,
            edited: 0,
            transfers: 0,
            merged_into: 0,
            later_batches: Vec::new(),
            restored: Vec::new(),
            superseded: 0,
        }
    }

    #[test]
    fn the_undo_says_what_goes() {
        assert_eq!(
            undo_lines(&preview(), "Current account"),
            ["This removes 296 transactions and 1 balance from Current account."]
        );
        let mut more = preview();
        more.flags = 2;
        more.edited = 1;
        more.merged_into = 1;
        let lines = undo_lines(&more, "Current account");
        assert_eq!(
            lines[0],
            "This removes 296 transactions, 1 balance and 2 flagged rows from Current account."
        );
        assert!(lines[1].starts_with("1 of them has your edits"));
        assert_eq!(
            lines[2],
            "1 transaction merged into these as duplicates stays excluded."
        );

        let mut corrections = preview();
        corrections.restored = vec![orre_core::id::TransactionId::new(7)];
        corrections.superseded = 2;
        let lines = undo_lines(&corrections, "Current account");
        assert_eq!(
            lines[1..],
            [
                "1 transaction replaced by the bank's corrections in this import comes back as \
                 it was.",
                "2 transactions replaced by a later import's correction go too; the correction \
                 stays.",
            ]
        );
    }
}
