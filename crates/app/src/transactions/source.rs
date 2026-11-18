//! Where a transaction came from and whether it counts (ROADMAP M3.10;
//! `DESIGN.md` → "Transactions additions": Split and tags' Source section,
//! Excluded `3FE-0`, Manual (cash account) `4D7-0`; `ARCHITECTURE.md` →
//! "Decision record: Transactions additions", point 1).
//!
//! - **Source section**, on every row between Split and Rule: an imported
//!   row's raw description (mono 11/14 `text-secondary`), its batch
//!   ("monzo-november.csv · imported 9 Nov", caption `text-muted`), outline
//!   "Exclude…" and "Undo import…"; an excluded row keeps only "Undo
//!   import…". A manual row reads "Added by you on 7 Nov" with the small
//!   destructive outline "Delete transaction…": only manual rows can be
//!   deleted, imported ones are excluded instead.
//! - **The exclusion card**, under an excluded row's header: `float`, 1px
//!   `hairline`, `radius-surface`, 12px padding; the 12px `excluded` icon,
//!   "Excluded from totals" (or "… and balance" for a duplicate, which
//!   also leaves the account's balance) and the day it was excluded; the
//!   reason (body `text-strong`), the user's details (caption `text-muted`,
//!   wrapping), then outline "Include again" and "Edit reason…".
//!
//! The buttons ask the shell for their dialogs ([`super::TransactionsEvent`]);
//! "Include again" is the undo of excluding and runs at once. The user's
//! details are their own text, shown plain and never acted on.

use coffer_core::exclusion::ExclusionReason;
use coffer_core::reporting::register::{RegisterRow, RowSource};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::{
    AnyElement, Context, InteractiveElement as _, IntoElement, ParentElement as _, Styled as _, div,
};

use crate::format;
use crate::theme::tokens::{color, radius, size, spacing, typography};
use crate::ui::button::{Button, ButtonVariant};
use crate::ui::icon::Glyph;
use crate::ui::inspector::{section, section_title};
use crate::ui::text::TextRoleExt as _;

use super::editor::EditPlace;
use super::{TransactionsEvent, TransactionsView};

/// Why the last change at `place` wasn't made, in `danger`.
fn error_at(view: &TransactionsView, place: EditPlace) -> Option<AnyElement> {
    view.editor
        .error_at(place)
        .map(|error| crate::ui::field::error_caption(error).into_any_element())
}

/// A reason as the table, the card and the Reason select name it.
pub fn reason_label(reason: ExclusionReason) -> &'static str {
    match reason {
        ExclusionReason::Duplicate => "Duplicate",
        ExclusionReason::BankError => "Bank error",
        ExclusionReason::Disputed => "Disputed",
        ExclusionReason::Other => "Other",
    }
}

/// The card's title: a duplicate also leaves the account's balance.
pub fn card_title(reason: ExclusionReason) -> &'static str {
    if reason.leaves_balance() {
        "Excluded from totals and balance"
    } else {
        "Excluded from totals"
    }
}

/// The batch caption, "monzo-november.csv · imported 9 Nov", or "Added by
/// you on 7 Nov" for a manual row.
pub fn source_caption(source: &RowSource, today: chrono::NaiveDate) -> String {
    match source {
        RowSource::Import {
            file, imported_on, ..
        } => format!(
            "{file} · imported {}",
            format::list_date(*imported_on, today)
        ),
        RowSource::Manual { created_on } => {
            format!("Added by you on {}", format::list_date(*created_on, today))
        }
    }
}

/// The exclusion card for an excluded `row`; `None` when it counts.
pub(super) fn exclusion_card(
    view: &TransactionsView,
    row: &RegisterRow,
    cx: &mut Context<TransactionsView>,
) -> Option<AnyElement> {
    let reason = row.excluded?;
    let id = row.id;
    let details = row
        .excluded_note
        .clone()
        .filter(|note| !note.trim().is_empty());
    let day = row
        .excluded_on
        .map(|day| format::list_date(day, view.today))
        .unwrap_or_default();
    let card = div()
        .id("exclusion-card")
        .flex()
        .flex_col()
        .gap(spacing::S2)
        .p(spacing::S3)
        .bg(color::FLOAT)
        .border(size::HAIRLINE)
        .border_color(color::HAIRLINE)
        .rounded(radius::SURFACE)
        .child(
            div()
                .flex()
                .items_center()
                .gap(spacing::S2)
                .child(Glyph::Excluded.render().text_color(color::TEXT_MUTED))
                .child(section_title(card_title(reason)).flex_1().min_w_0())
                .child(
                    div()
                        .flex_none()
                        .text_role(typography::MONO_VALUE)
                        .text_color(color::TEXT_MUTED)
                        .child(day),
                ),
        )
        .child(
            div()
                .text_role(typography::BODY)
                .text_color(color::TEXT_STRONG)
                .child(reason_label(reason)),
        )
        .when_some(details, |this, details| {
            this.child(
                div()
                    .text_role(typography::CAPTION)
                    .text_color(color::TEXT_MUTED)
                    .child(details),
            )
        })
        .child(
            div()
                .flex()
                .gap(spacing::S2)
                .child(
                    Button::new("include-again", "Include again")
                        .with_variant(ButtonVariant::Outline)
                        .with_surface(color::FLOAT)
                        .on_click(cx.listener(move |this, _, _, cx| this.include_again(id, cx))),
                )
                .child(
                    Button::new("edit-exclusion", "Edit reason…")
                        .with_variant(ButtonVariant::Outline)
                        .with_surface(color::FLOAT)
                        .on_click(cx.listener(move |_, _, _, cx| {
                            cx.emit(TransactionsEvent::ExcludeRequested(id))
                        })),
                ),
        )
        .children(error_at(view, EditPlace::Exclusion));
    // 8px below the header's caption.
    Some(div().pt(spacing::S2).child(card).into_any_element())
}

/// The Source section for `row`.
pub(super) fn source_section(
    view: &TransactionsView,
    row: &RegisterRow,
    cx: &mut Context<TransactionsView>,
) -> AnyElement {
    let id = row.id;
    let caption = div()
        .text_role(typography::CAPTION)
        .text_color(color::TEXT_MUTED)
        .child(source_caption(&row.source, view.today));
    let section = section(true)
        .gap(spacing::S2)
        .child(section_title(super::inspector::Section::Source.title()));
    let section = match &row.source {
        RowSource::Manual { .. } => section.child(caption).child(
            div().flex().child(
                Button::new("delete-transaction", "Delete transaction…")
                    .with_variant(ButtonVariant::DestructiveCompact)
                    .with_surface(color::CHROME)
                    .on_click(cx.listener(move |_, _, _, cx| {
                        cx.emit(TransactionsEvent::DeleteRequested(id))
                    })),
            ),
        ),
        RowSource::Import { batch, .. } => {
            let batch = *batch;
            section
                .child(
                    div()
                        .text_role(typography::MONO_VALUE)
                        .text_color(color::TEXT_SECONDARY)
                        .child(row.description.clone()),
                )
                .child(caption)
                .child(
                    div()
                        .flex()
                        .gap(spacing::S2)
                        .when(row.excluded.is_none(), |this| {
                            this.child(
                                Button::new("exclude", "Exclude…")
                                    .with_variant(ButtonVariant::Outline)
                                    .with_surface(color::CHROME)
                                    .on_click(cx.listener(move |_, _, _, cx| {
                                        cx.emit(TransactionsEvent::ExcludeRequested(id))
                                    })),
                            )
                        })
                        .child(
                            Button::new("undo-import", "Undo import…")
                                .with_variant(ButtonVariant::Outline)
                                .with_surface(color::CHROME)
                                .on_click(cx.listener(move |_, _, _, cx| {
                                    cx.emit(TransactionsEvent::UndoImportRequested(batch))
                                })),
                        ),
                )
        }
    };
    section
        .children(error_at(view, EditPlace::Source))
        .into_any_element()
}

#[cfg(test)]
mod tests {
    use chrono::NaiveDate;
    use coffer_core::id::BatchId;
    use coffer_core::manual::Origin;

    use super::*;

    #[test]
    fn the_source_caption_names_the_file_or_the_user() {
        let today = NaiveDate::from_ymd_opt(2026, 11, 14).unwrap();
        let imported = RowSource::Import {
            batch: BatchId::new(2),
            file: "monzo-joint-november.csv".to_owned(),
            imported_on: NaiveDate::from_ymd_opt(2026, 11, 9).unwrap(),
            origin: Origin::Imported,
        };
        assert_eq!(
            source_caption(&imported, today),
            "monzo-joint-november.csv · imported 9 Nov"
        );
        let manual = RowSource::Manual {
            created_on: NaiveDate::from_ymd_opt(2026, 11, 7).unwrap(),
        };
        assert_eq!(source_caption(&manual, today), "Added by you on 7 Nov");
    }

    #[test]
    fn only_a_duplicate_leaves_the_balance() {
        assert_eq!(
            card_title(ExclusionReason::Duplicate),
            "Excluded from totals and balance"
        );
        for reason in [
            ExclusionReason::BankError,
            ExclusionReason::Disputed,
            ExclusionReason::Other,
        ] {
            assert_eq!(card_title(reason), "Excluded from totals");
        }
        assert_eq!(reason_label(ExclusionReason::BankError), "Bank error");
    }
}
