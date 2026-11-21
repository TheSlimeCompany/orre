//! The Delete backup dialog, opened by "Delete backup…" in the backup
//! inspector (R.12).
//!
//! Composed from the dialog spec as the Remove client dialog is
//! (`DESIGN.md` → "Settings"): a destructive confirmation, "Delete the
//! backup from `10 Nov, 14:02`?", that the file is deleted for good,
//! Cancel / "Delete backup" (destructive solid).

use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::{
    Context, Entity, EventEmitter, FocusHandle, IntoElement, Render, SharedString, Styled as _,
    Window,
};

use crate::accounts::DialogClosed;
use crate::theme::tokens::{color, typography};
use crate::ui::button::{Button, ButtonVariant};
use crate::ui::dialog::Dialog;
use crate::ui::field::error_caption;

use super::table::{caption, rich};
use super::{DeleteTarget, SettingsView, backups};

/// The Delete backup dialog.
pub struct DeleteBackupDialog {
    settings: Entity<SettingsView>,
    target: DeleteTarget,
    focus: FocusHandle,
    /// Why the file couldn't be deleted.
    error: Option<SharedString>,
}

impl EventEmitter<DialogClosed> for DeleteBackupDialog {}

impl DeleteBackupDialog {
    pub fn new(
        settings: Entity<SettingsView>,
        target: DeleteTarget,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let focus = cx.focus_handle();
        focus.focus(window, cx);
        Self {
            settings,
            target,
            focus,
            error: None,
        }
    }

    /// Deletes the backup, or says why it can't.
    pub fn confirm(&mut self, cx: &mut Context<Self>) {
        let path = self.target.path.clone();
        match self
            .settings
            .update(cx, |settings, cx| settings.delete_backup(&path, cx))
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

    /// What the dialog shows, as plain data (test-only, ROADMAP U.3).
    #[cfg(any(test, feature = "dev-driver"))]
    pub fn describe(&self) -> crate::ui::describe::Dialog {
        use crate::ui::describe::{Action, Dialog};
        Dialog {
            title: super::table::Run::joined(&backups::delete_title(&self.target.taken)),
            fields: Vec::new(),
            error: self.error.as_ref().map(ToString::to_string),
            actions: vec![
                Action::button("Cancel", true),
                Action::button("Delete backup", true),
            ],
        }
    }
}

impl Render for DeleteBackupDialog {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let title = rich(
            typography::TITLE,
            color::TEXT,
            backups::delete_title(&self.target.taken),
        )
        .whitespace_nowrap();
        let on_cancel = cx.entity().downgrade();
        Dialog::with_title(title, &self.focus)
            .body(caption(backups::delete_note(self.target.reason)))
            .when_some(self.error.clone(), |this, error| {
                this.body(error_caption(error))
            })
            .action(
                Button::new("delete-backup-cancel", "Cancel")
                    .with_variant(ButtonVariant::Secondary)
                    .with_surface(color::FLOAT)
                    .on_click(cx.listener(|this, _, _, cx| this.cancel(cx))),
            )
            .action(
                Button::new("delete-backup-confirm", "Delete backup")
                    .with_variant(ButtonVariant::DestructiveSolid)
                    .with_surface(color::FLOAT)
                    .on_click(cx.listener(|this, _, _, cx| this.confirm(cx))),
            )
            .on_cancel(move |_, cx| {
                let _ = on_cancel.update(cx, |this, cx| this.cancel(cx));
            })
    }
}
