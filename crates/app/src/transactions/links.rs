//! The inspector's Share and Links sections (ROADMAP M3.11; `docs/DESIGN.md`
//! → "Refunds, reimbursements and shared costs"; Paper `9WS-0`, `BAT-0`,
//! `EB0-0`; `ARCHITECTURE.md` → "Decision record: refunds, reimbursements
//! and settlements (built)").
//!
//! - **Share** (money out, outside a transfer), between Details and Links:
//!   Just me / Shared / Claim back. Shared shows Yours (the user's part, a
//!   `£` field "of £31.80") and Owed by (the person: a local label, with the
//!   lock); it's saved once a person is chosen, and again when Yours is
//!   left or Enter is pressed. Claim back shows what is still owed. A split
//!   purchase is shared line by line, which isn't drawn yet, so its Shared
//!   segment is disabled.
//! - **Links** (every transaction outside a transfer): "Link…" opens the
//!   link dialog (the shell owns it); each confirmed link is a row with ✕,
//!   which unlinks it with the undo bar; Coffer's suggestion is a card
//!   ("Is this a refund?") with Link and Not a refund.
//!
//! Every change goes through the store to core's `edit`: one undoable
//! change batch each. A link, an unlink or a rejected suggestion shows the
//! D6.3 undo bar; a share change, like M3.9's edits, doesn't.

use coffer_core::id::{LinkId, TransactionId};
use coffer_core::link::{LinkKind, NewLink, ShareLabel, Sharing};
use coffer_core::money::{Currency, Money};
use coffer_core::reporting::linking as core_linking;
use coffer_core::reporting::register::{RegisterRow, RowCategory, RowLink};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::{
    AnyElement, Context, IntoElement, ParentElement as _, SharedString, Styled as _, Window, div,
};

use crate::format::{self, Sign};
use crate::inbox::text::{Part, rich, unsigned};
use crate::theme::tokens::{color, font, radius, size, spacing, typography};
use crate::ui::button::{Button, ButtonVariant, IconButton, IconButtonSize};
use crate::ui::field::{self as fields, FieldRow, SelectField, error_caption, text_input};
use crate::ui::icon::Glyph;
use crate::ui::inspector::{section, section_title_row};
use crate::ui::marker::Marker;
use crate::ui::menu::{MenuGroup, MenuOption, Select, filter_groups};
use crate::ui::segmented::Segmented;
use crate::ui::text::TextRoleExt as _;

use super::editor::EditPlace;
use super::linking;
use super::{Menu, Source, TransactionsEvent, TransactionsView, Undo, UndoAction, present};

/// The Share control's keys.
const JUST_ME: &str = "just-me";
const SHARED: &str = "shared";
const CLAIM: &str = "claim";

/// The Owed by menu's "Add “…”" option.
pub const NEW_PERSON: &str = "new-person";

/// What the link dialog starts from.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum LinkFrom {
    /// A transaction's "Link…": money in or a purchase.
    Transaction(TransactionId),
    /// A person's "Link a payment…" on Owed to you.
    Person(ShareLabel, Currency),
}

/// Whether `row` has the Share section: money out outside a transfer.
pub fn has_share(row: &RegisterRow) -> bool {
    row.amount.is_negative() && !row.in_transfer
}

/// Whether `row` has the Links section: outside a transfer.
pub fn has_links(row: &RegisterRow) -> bool {
    !row.in_transfer && !row.amount.is_zero()
}

/// The person's key in the Owed by menu.
fn person_key(label: &str) -> String {
    format!("person-{label}")
}

/// The Owed by menu (composed for M3.11 from the tag menu, user,
/// 2026-11-14): everyone with a share or a settlement, A–Z with the lock,
/// narrowed by `query`; when `query` isn't anyone's name, "Add “…”".
pub fn person_groups(view: &TransactionsView, query: &str) -> Vec<MenuGroup> {
    let mut names: Vec<&str> = view
        .register
        .people()
        .iter()
        .map(|person| person.label.as_str())
        .collect();
    if let Some(row) = view.single_row()
        && let Some(label) = row.share.label.as_deref()
    {
        names.push(label);
    }
    names.sort_unstable();
    names.dedup();
    let chosen = view.chosen_person();
    let options: Vec<MenuOption> = names
        .iter()
        .map(|name| {
            MenuOption::new(person_key(name), linking::name(name))
                .icon(Glyph::Lock)
                .checked(chosen.as_deref() == Some(*name))
        })
        .collect();
    let mut groups = filter_groups(&[MenuGroup::new(None, options)], query);
    let typed = query.trim();
    if !typed.is_empty() && !names.contains(&typed) {
        let option = match ShareLabel::new(typed) {
            Ok(label) => {
                MenuOption::new(NEW_PERSON, format!("Add “{}”", label.as_str())).icon(Glyph::Plus)
            }
            Err(_) => MenuOption::new(NEW_PERSON, "Add a person")
                .icon(Glyph::Plus)
                .hint("at most 64 characters, one line")
                .disabled(true),
        };
        groups.push(MenuGroup::new(None, vec![option]));
    }
    groups
}

/// The person a menu key names.
pub fn person_from_key(key: &str) -> Option<&str> {
    key.strip_prefix("person-")
}

impl TransactionsView {
    /// The person the selected row is shared with, if any.
    pub(super) fn chosen_person(&self) -> Option<String> {
        self.single_row().and_then(|row| row.share.label.clone())
    }

    /// The Share control's choice for `row`: what's stored, or Shared while
    /// a share waits for its person.
    fn share_key(&self, row: &RegisterRow) -> &'static str {
        if self.editor.shared_draft || row.share.label.is_some() {
            SHARED
        } else if row.share.claim {
            CLAIM
        } else {
            JUST_ME
        }
    }

    /// The user's part as typed in Yours: negative for money out; `None`
    /// when it isn't an amount.
    fn typed_share(&self, row: &RegisterRow, cx: &Context<Self>) -> Option<Money> {
        let text = self.editor.yours.read(cx).value();
        let typed = format::parse_amount(&text, row.amount.currency()).ok()?;
        Some(Money::new(
            -typed.minor().saturating_abs(),
            row.amount.currency(),
        ))
    }

    /// Runs one link or share edit through the store and reads the
    /// register again; `undo` is the undo bar's message, if it shows one.
    fn run_link_edit(
        &mut self,
        place: EditPlace,
        undo: Option<String>,
        cx: &mut Context<Self>,
        edit: impl FnOnce(
            &super::store::TransactionStore,
        ) -> Result<
            Option<coffer_core::id::ChangeBatchId>,
            crate::accounts::store::StoreError,
        >,
    ) -> bool {
        let Source::Database(store) = &self.source else {
            self.editor.error =
                Some((place, "This is sample data, which can't be changed.".into()));
            cx.notify();
            return false;
        };
        match edit(store) {
            Ok(batch) => {
                self.editor.error = None;
                if let (Some(batch), Some(message)) = (batch, undo) {
                    self.undo = Some(Undo {
                        action: UndoAction::Batch(batch),
                        message: message.into(),
                    });
                }
                self.reload(cx);
                cx.emit(TransactionsEvent::Changed);
                cx.notify();
                true
            }
            Err(error) => {
                tracing::warn!(%error, "couldn't save a link or share");
                self.editor.error = Some((place, error.to_string().into()));
                cx.notify();
                false
            }
        }
    }

    /// Just me, Shared or Claim back: Just me and Claim back save now;
    /// Shared waits for a person unless one is stored.
    pub(super) fn choose_share(&mut self, key: &str, window: &mut Window, cx: &mut Context<Self>) {
        let Some(row) = self.single_row().cloned() else {
            return;
        };
        self.editor.shared_draft = false;
        let sharing = match key {
            JUST_ME => Sharing::JustMe,
            CLAIM => Sharing::ClaimBack,
            _ => {
                if row.share.label.is_none() {
                    // Half each until the user says otherwise.
                    let half = Money::new(
                        (row.amount.minor() / 2).saturating_abs(),
                        row.amount.currency(),
                    );
                    let text = SharedString::from(half.to_decimal_string());
                    self.editor
                        .yours
                        .update(cx, |input, cx| input.set_value(text, window, cx));
                    self.editor.shared_draft = true;
                }
                cx.notify();
                return;
            }
        };
        self.run_link_edit(EditPlace::Share, None, cx, |store| {
            store.set_sharing(row.id, &sharing)
        });
    }

    /// Shares the selected row with `person` (a name chosen or typed),
    /// keeping Yours.
    pub(super) fn choose_person(&mut self, person: &str, cx: &mut Context<Self>) {
        let Some(row) = self.single_row().cloned() else {
            return;
        };
        let label = match ShareLabel::new(person) {
            Ok(label) => label,
            Err(error) => {
                self.editor.error = Some((EditPlace::Share, error.to_string().into()));
                cx.notify();
                return;
            }
        };
        let Some(my_share) = self.typed_share(&row, cx) else {
            self.editor.error = Some((
                EditPlace::Share,
                "Type your part as an amount, like 15.90.".into(),
            ));
            cx.notify();
            return;
        };
        if self.run_link_edit(EditPlace::Share, None, cx, |store| {
            store.set_sharing(row.id, &Sharing::Shared { my_share, label })
        }) {
            self.editor.shared_draft = false;
        }
    }

    /// Yours was left or Enter pressed: saves the share when it has a
    /// person and the amount changed.
    pub(super) fn commit_yours(&mut self, cx: &mut Context<Self>) {
        let Some(row) = self
            .editor
            .shown
            .and_then(|id| self.register.row(id))
            .cloned()
        else {
            return;
        };
        let Some(label) = row.share.label.clone() else {
            return;
        };
        let Some(my_share) = self.typed_share(&row, cx) else {
            self.editor.error = Some((
                EditPlace::Share,
                "Type your part as an amount, like 15.90.".into(),
            ));
            cx.notify();
            return;
        };
        if Some(my_share) == row.share.my_share {
            return;
        }
        let Ok(label) = ShareLabel::new(&label) else {
            return;
        };
        self.run_link_edit(EditPlace::Share, None, cx, |store| {
            store.set_sharing(row.id, &Sharing::Shared { my_share, label })
        });
    }

    /// Confirms Coffer's suggestion on the selected row.
    pub(super) fn confirm_suggestion(&mut self, ids: Vec<LinkId>, cx: &mut Context<Self>) {
        let Some(row) = self.single_row().cloned() else {
            return;
        };
        let suggested: Vec<&RowLink> = row
            .links
            .iter()
            .filter(|link| ids.contains(&link.id))
            .collect();
        let Some(kind) = suggested.first().map(|link| link.kind) else {
            return;
        };
        let purchases: Vec<&RegisterRow> = suggested
            .iter()
            .filter_map(|link| match link.other {
                coffer_core::reporting::register::LinkOther::Transaction(id) => {
                    self.register.row(id)
                }
                _ => None,
            })
            .collect();
        let message = linking::linked_message(kind, &row, &purchases, None, self.today);
        self.run_link_edit(EditPlace::Links, Some(message), cx, |store| {
            store.confirm_links(&ids)
        });
    }

    /// Confirms every suggestion on the selected row, as its card's Link
    /// button does (snapshots).
    pub fn confirm_all_suggestions(&mut self, cx: &mut Context<Self>) {
        let ids: Vec<LinkId> = self
            .single_row()
            .map(|row| {
                linking::suggestions(row)
                    .iter()
                    .map(|link| link.id)
                    .collect()
            })
            .unwrap_or_default();
        if !ids.is_empty() {
            self.confirm_suggestion(ids, cx);
        }
    }

    /// Rejects links: ✕ on a link (unlink), or "Not a refund" on Coffer's
    /// suggestion.
    pub(super) fn reject_links(&mut self, ids: Vec<LinkId>, cx: &mut Context<Self>) {
        let Some(row) = self.single_row().cloned() else {
            return;
        };
        let Some(link) = row.links.iter().find(|link| ids.contains(&link.id)) else {
            return;
        };
        let message = linking::unlinked_message(
            link.kind,
            link.status == coffer_core::link::LinkStatus::Suggested,
        );
        self.run_link_edit(EditPlace::Links, Some(message.into()), cx, |store| {
            store.reject_links(&ids)
        });
    }

    /// Makes a link from the link dialog. Returns why it couldn't, for the
    /// dialog to say.
    pub fn link(
        &mut self,
        link: &NewLink,
        person: Option<&str>,
        cx: &mut Context<Self>,
    ) -> Result<(), SharedString> {
        let Source::Database(store) = &self.source else {
            return Err("This is sample data, which can't be changed.".into());
        };
        let inflow = self.register.row(link.inflow).cloned();
        let purchase = link
            .target
            .purchase()
            .and_then(|id| self.register.row(id))
            .cloned();
        match store.link(link) {
            Ok(batch) => {
                if let (Some(batch), Some(inflow)) = (batch, inflow) {
                    let purchases: Vec<&RegisterRow> = purchase.iter().collect();
                    self.undo = Some(Undo {
                        action: UndoAction::Batch(batch),
                        message: linking::linked_message(
                            link.kind, &inflow, &purchases, person, self.today,
                        )
                        .into(),
                    });
                }
                self.editor.error = None;
                self.reload(cx);
                cx.emit(TransactionsEvent::Changed);
                cx.notify();
                Ok(())
            }
            Err(error) => {
                tracing::warn!(%error, "couldn't link");
                Err(error.to_string().into())
            }
        }
    }

    /// Renames a person everywhere (Owed to you's Name).
    pub fn rename_person(
        &mut self,
        from: &ShareLabel,
        to: &str,
        cx: &mut Context<Self>,
    ) -> Result<(), SharedString> {
        let Source::Database(store) = &self.source else {
            return Err("This is sample data, which can't be changed.".into());
        };
        let to = ShareLabel::new(to).map_err(|error| SharedString::from(error.to_string()))?;
        if &to == from {
            return Ok(());
        }
        match store.rename_person(from, &to) {
            Ok(_) => {
                self.reload(cx);
                cx.emit(TransactionsEvent::Changed);
                Ok(())
            }
            Err(error) => Err(error.to_string().into()),
        }
    }

    /// Asks the shell for the link dialog.
    pub(super) fn request_link(&mut self, from: LinkFrom, cx: &mut Context<Self>) {
        cx.emit(TransactionsEvent::LinkRequested(from));
    }
}

// Rendering -------------------------------------------------------------------

/// A caption from parts, in caption type, `text-muted`.
fn rich_caption(window: &Window, parts: Vec<Part>) -> AnyElement {
    div()
        .text_role(typography::CAPTION)
        .child(rich(window, typography::CAPTION, color::TEXT_MUTED, parts))
        .into_any_element()
}

/// The refund's Category row while its purchase sets it (Paper `FGU-0`):
/// marker and name in `text-secondary`, the 10px link in `icon-faint`.
pub(super) fn following_category(view: &TransactionsView, row: &RegisterRow) -> AnyElement {
    let purchase = linking::refund_purchase(&view.register, row);
    let (label, marker) = purchase
        .map(|purchase| {
            (
                present::category_label(&view.register, purchase),
                present::row_marker(&view.register, purchase),
            )
        })
        .unwrap_or_else(|| ("Uncategorised".to_owned(), Marker::None));
    let value = div()
        .flex()
        .flex_1()
        .min_w_0()
        .items_center()
        .gap(spacing::S2)
        .h(size::CONTROL)
        .px(spacing::S2)
        .child(marker.render())
        .child(
            div()
                .flex_1()
                .min_w_0()
                .whitespace_nowrap()
                .text_ellipsis()
                .text_role(typography::BODY)
                .text_color(color::TEXT_SECONDARY)
                .child(label),
        )
        .child(
            Glyph::Link
                .render_at(size::ICON_XS)
                .text_color(color::ICON_FAINT),
        );
    div()
        .flex()
        .flex_col()
        .gap(spacing::S2)
        .child(FieldRow::new("Category", value))
        .child(fields::caption(linking::FOLLOWS_CAPTION))
        .into_any_element()
}

/// The Owed by select (Paper `CBH-0`).
fn owed_by(
    view: &TransactionsView,
    row: &RegisterRow,
    cx: &mut Context<TransactionsView>,
) -> Select {
    let field = match row.share.label.as_deref() {
        Some(label) => {
            let theirs = row
                .share
                .my_share
                .map(|mine| Money::new(mine.minor() - row.amount.minor(), mine.currency()));
            let field = SelectField::new("owed-by-field", linking::name(label)).private();
            match theirs {
                Some(theirs) => field.amount(unsigned(theirs)),
                None => field,
            }
        }
        None => SelectField::new("owed-by-field", "Choose a person").placeholder(true),
    };
    let groups = if view.menu == Some(Menu::Person) {
        person_groups(view, &view.menu_filter.read(cx).value())
    } else {
        person_groups(view, "")
    };
    let (toggled, chosen, moved) = (
        cx.entity().downgrade(),
        cx.entity().downgrade(),
        cx.entity().downgrade(),
    );
    Select::new(
        "owed-by",
        "Owed by",
        field,
        groups,
        view.chosen_person().map(|label| person_key(&label).into()),
        &view.editor.person_select,
    )
    .filter(&view.menu_filter)
    .max_height(size::CONTROL * super::MENU_ROWS)
    .on_toggle(move |open, window, cx| {
        toggled
            .update(cx, |this, cx| {
                if open != (this.menu == Some(Menu::Person)) {
                    if open {
                        this.open(Menu::Person, window, cx);
                    } else {
                        this.close(window, cx);
                    }
                }
            })
            .ok();
    })
    .on_choose(move |key, window, cx| {
        chosen
            .update(cx, |this, cx| this.choose(key, window, cx))
            .ok();
    })
    .on_move(move |step, _, cx| {
        moved
            .update(cx, |this, cx| this.move_highlight(step, cx))
            .ok();
    })
}

/// The Share section (Paper `C3B-0`, `FJ4-0`).
pub(super) fn share_section(
    view: &TransactionsView,
    row: &RegisterRow,
    window: &mut Window,
    cx: &mut Context<TransactionsView>,
) -> AnyElement {
    let key = view.share_key(row);
    let split = matches!(row.category, RowCategory::Split(_));
    let chooser = cx.entity().downgrade();
    let control = Segmented::new("share", key)
        .fill()
        .segment(JUST_ME, "Just me")
        .segment(SHARED, "Shared")
        .segment(CLAIM, "Claim back")
        .disabled(SHARED, split)
        .on_choose(move |key, window, cx| {
            chooser
                .update(cx, |this, cx| this.choose_share(key, window, cx))
                .ok();
        });
    let spent = Money::new(row.amount.minor().saturating_abs(), row.amount.currency());
    let mut section = section(true)
        .gap(spacing::S2)
        .child(section_title_row("Share", None))
        .child(control);
    match key {
        SHARED => {
            let of = div()
                .flex()
                .flex_none()
                .text_role(typography::CAPTION)
                .text_color(color::TEXT_MUTED)
                .child(div().font_family(font::UI).child("of\u{a0}"))
                .child(div().font_family(font::MONO).child(unsigned(spent)));
            let yours = text_input(&view.editor.yours)
                .w_full()
                .gap(spacing::S1)
                .font_family(font::MONO)
                .prefix(
                    div()
                        .text_role(typography::MONO_AMOUNT)
                        .text_color(color::TEXT_MUTED)
                        .child(format::currency_prefix(row.amount.currency())),
                )
                .suffix(of);
            let mine = view
                .typed_share(row, cx)
                .or(row.share.my_share)
                .unwrap_or(row.amount);
            section = section
                .child(FieldRow::new("Yours", yours))
                .child(FieldRow::new("Owed by", owed_by(view, row, cx)))
                .child(rich_caption(
                    window,
                    linking::shared_caption(mine, row.share.label.as_deref()),
                ));
        }
        CLAIM => {
            let still = div()
                .flex()
                .flex_1()
                .min_w_0()
                .items_baseline()
                .px(spacing::S2)
                .whitespace_nowrap()
                .child(
                    div()
                        .text_role(typography::MONO_AMOUNT)
                        .text_color(color::TEXT)
                        .child(unsigned(row.share.left)),
                )
                .child(
                    div()
                        .text_role(typography::CAPTION)
                        .text_color(color::TEXT_MUTED)
                        .child("\u{a0}of\u{a0}")
                        .child(div().font_family(font::MONO).child(unsigned(spent)))
                        .flex(),
                );
            section = section
                .child(FieldRow::new("Still owed", still))
                .child(fields::caption(linking::CLAIM_CAPTION));
        }
        _ => {}
    }
    section
        .children(
            view.editor
                .error_at(EditPlace::Share)
                .map(|error| error_caption(error).into_any_element()),
        )
        .into_any_element()
}

/// One Links row (Paper `FEC-0`): the 12px link, what it is with its date
/// in mono caption `text-muted`, the mono amount and the 24px ✕.
fn link_row(
    view: &TransactionsView,
    link: &RowLink,
    cx: &mut Context<TransactionsView>,
) -> AnyElement {
    let line = linking::link_line(&view.register, link);
    let id = link.id;
    div()
        .flex()
        .flex_none()
        .items_center()
        .gap(spacing::S2)
        .h(size::CONTROL)
        .child(Glyph::Link.render().text_color(color::TEXT_MUTED))
        .child(
            div()
                .flex()
                .flex_1()
                .min_w_0()
                .items_baseline()
                .whitespace_nowrap()
                .overflow_hidden()
                .text_role(typography::BODY)
                .text_color(color::TEXT)
                .child(
                    div()
                        .min_w_0()
                        .text_ellipsis()
                        .child(if line.date.is_some() {
                            // The space is in UI type, as Paper sets it.
                            format!("{}\u{a0}", line.what)
                        } else {
                            line.what
                        }),
                )
                .when_some(line.date, |this, date| {
                    this.child(
                        div()
                            .flex_none()
                            .text_role(typography::MONO_VALUE)
                            .text_color(color::TEXT_MUTED)
                            .child(format::list_date(date, view.today)),
                    )
                }),
        )
        .child(
            div()
                .flex_none()
                .text_role(typography::MONO_AMOUNT)
                .text_color(color::TEXT)
                .child(line.amount),
        )
        .child(
            IconButton::new(
                SharedString::from(format!("unlink-{id}")),
                Glyph::Close,
                "Unlink",
            )
            .with_size(IconButtonSize::Small)
            .on_click(cx.listener(move |this, _, _, cx| this.reject_links(vec![id], cx))),
        )
        .into_any_element()
}

/// The 28px `field` strip naming a purchase (Paper `APV-0`): mono date,
/// merchant, mono amount.
fn purchase_strip(view: &TransactionsView, purchase: &RegisterRow) -> AnyElement {
    div()
        .flex()
        .flex_none()
        .items_center()
        .gap(spacing::S2)
        .h(size::CONTROL)
        .px(spacing::S2)
        .rounded(radius::MD)
        .bg(color::FIELD)
        .child(
            div()
                .flex_none()
                .text_role(typography::MONO_VALUE)
                .text_color(color::TEXT_MUTED)
                .child(format::list_date(purchase.date, view.today)),
        )
        .child(
            div()
                .flex_1()
                .min_w_0()
                .whitespace_nowrap()
                .text_ellipsis()
                .text_role(typography::BODY)
                .text_color(color::TEXT)
                .child(linking::name(&purchase.merchant)),
        )
        .child(
            div()
                .flex_none()
                .text_role(typography::MONO_AMOUNT)
                .text_color(color::TEXT)
                .child(format::money(purchase.amount, Sign::Always)),
        )
        .into_any_element()
}

/// Coffer's suggestion card (Paper `APQ-0`): neutral, `float`, 1px
/// `hairline`, `radius-surface`, 12px padding.
fn suggestion_card(
    view: &TransactionsView,
    row: &RegisterRow,
    suggestions: &[&RowLink],
    window: &mut Window,
    cx: &mut Context<TransactionsView>,
) -> Option<AnyElement> {
    let first = *suggestions.first()?;
    let ids: Vec<LinkId> = suggestions.iter().map(|link| link.id).collect();
    let purchases: Vec<&RegisterRow> = suggestions
        .iter()
        .filter_map(|link| match link.other {
            coffer_core::reporting::register::LinkOther::Transaction(id) => view.register.row(id),
            _ => None,
        })
        .collect();
    let (title, why, effect, confirm, reject) = if first.kind == LinkKind::Refund {
        let why = core_linking::refund_why(&view.register, row, first)?;
        let purchase = purchases.first()?;
        (
            "Is this a refund?".to_owned(),
            linking::refund_why(&why),
            linking::refund_effect(&view.register, purchase, first.amount),
            "Link as refund",
            "Not a refund",
        )
    } else {
        let (title, why) = linking::reimbursement_why(purchases.len());
        (
            title.to_owned(),
            why,
            vec![Part::text(linking::reimbursement_effect(purchases.len()))],
            "Link as reimbursement",
            "Not this",
        )
    };
    let (confirm_ids, reject_ids) = (ids.clone(), ids);
    Some(
        div()
            .flex()
            .flex_col()
            .gap(spacing::S2_5)
            .p(spacing::S3)
            .rounded(radius::SURFACE)
            .border(size::HAIRLINE)
            .border_color(color::HAIRLINE)
            .bg(color::FLOAT)
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap(spacing::S1)
                    .child(
                        div()
                            .text_role(typography::SECTION_TITLE)
                            .text_color(color::TEXT)
                            .child(title),
                    )
                    .child(
                        div()
                            .text_role(typography::CAPTION)
                            .text_color(color::TEXT_MUTED)
                            .child(why),
                    ),
            )
            .children(
                purchases
                    .iter()
                    .map(|purchase| purchase_strip(view, purchase)),
            )
            .child(rich_caption(window, effect))
            .child(
                div()
                    .flex()
                    .gap(spacing::S1_5)
                    .child(
                        Button::new("link-suggestion", confirm)
                            .with_variant(ButtonVariant::Outline)
                            .with_surface(color::FLOAT)
                            .on_click(cx.listener(move |this, _, _, cx| {
                                this.confirm_suggestion(confirm_ids.clone(), cx)
                            })),
                    )
                    .child(
                        Button::new("reject-suggestion", reject)
                            .with_variant(ButtonVariant::Ghost)
                            .with_surface(color::FLOAT)
                            .on_click(cx.listener(move |this, _, _, cx| {
                                this.reject_links(reject_ids.clone(), cx)
                            })),
                    ),
            )
            .into_any_element(),
    )
}

/// The Links section (Paper `AN1-0`, `FE7-0`).
pub(super) fn links_section(
    view: &TransactionsView,
    row: &RegisterRow,
    window: &mut Window,
    cx: &mut Context<TransactionsView>,
) -> AnyElement {
    let can_link = !core_linking::kinds_from(row).is_empty();
    let id = row.id;
    let link_button = can_link.then(|| {
        Button::new("link", "Link…")
            .with_variant(ButtonVariant::Outline)
            .on_click(
                cx.listener(move |this, _, _, cx| this.request_link(LinkFrom::Transaction(id), cx)),
            )
            .into_any_element()
    });
    let rows: Vec<AnyElement> = linking::confirmed(row)
        .map(|link| link_row(view, link, cx))
        .collect();
    let suggestions = linking::suggestions(row);
    let card = suggestion_card(view, row, &suggestions, window, cx);
    let caption = if card.is_none() {
        linking::links_caption(&view.register, row)
    } else {
        None
    };
    section(true)
        .gap(spacing::S2)
        .child(section_title_row("Links", link_button))
        .children(rows)
        .children(card)
        .children(caption.map(|parts| rich_caption(window, parts)))
        .children(
            view.editor
                .error_at(EditPlace::Links)
                .map(|error| error_caption(error).into_any_element()),
        )
        .into_any_element()
}
