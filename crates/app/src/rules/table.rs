//! The Rules view's canvas (`docs/DESIGN.md` → "Rules"): the toolbar and
//! the open section's table, or the apply preview.
//!
//! - **Rules** (`B1J-0`): # (24) · WHEN (flex) · ACCOUNT (140) · THEN
//!   (180) · MATCHES (96), 28px rows, the selected one `molten-soft`; a row
//!   is dragged onto another to take its place. A never-reached rule says
//!   so in `warning` before a `text-muted` 0.
//! - **Apply to past transactions** (`EFT-0`): MERCHANT · ACCOUNT · NOW ·
//!   AFTER · DATE · AMOUNT; "Will change", then "Category you set" with
//!   "Change these too"; the footer's buttons and caption.
//! - **Merchant names** (`G2O-0`): BANK TEXT · SHOWS AS (180) · LAST SEEN
//!   (72) · MATCHES (96).
//! - **Change history** (`GZT-0`): WHEN (96) · CHANGE · KIND (140) ·
//!   TRANSACTIONS (96); an undone change struck through, its KIND "Undone
//!   28 Oct".
//! - **Categories** (composed from Merchant names' table): CATEGORY (flex:
//!   the marker and label, a group's label 500, a subcategory indented so
//!   its marker lines up with the group's label, a hidden one `text-muted`
//!   with "Hidden" in caption) · KIND (140) · RULES (72) · TRANSACTIONS (96:
//!   transactions and split lines filed under it); zero counts in
//!   `text-faint`.
//!
//! Bank text, names and patterns are plain text ([`text::plain`]).

use coffer_core::category::CategoryKey;
use coffer_core::change::{BatchStatus, HistoryEntry};
use coffer_core::merchant::AliasSummary;
use coffer_core::rule::{PreviewAfter, PreviewRow, RuleSummary};
use gpui_kit::base::TestSupportExt as _;
use gpui_kit::component::Disableable as _;
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::{
    AnyElement, AppContext as _, Context, Entity, InteractiveElement as _, IntoElement,
    ParentElement as _, Render, SharedString, StatefulInteractiveElement as _, Styled as _, Window,
    div,
};

use crate::format::{self, Sign};
use crate::theme::tokens::{color, radius, shadow, size, spacing, typography};
use crate::ui::button::{Button, ButtonVariant, IconButton, IconButtonSize};
use crate::ui::icon::Glyph;
use crate::ui::kbd::Kbd;
use crate::ui::marker::Marker;
use crate::ui::text::TextRoleExt as _;
use crate::ui::toggle::Toggle;
use crate::ui::tracked::Tracked;

use super::store::RulesData;
use super::text::{self, Part, ThenLead, rich};
use super::view::RulesView;
use super::{Rules, Section, Target};

/// The LAST SEEN and history WHEN lanes: 72px (`spacing-6` × 3).
const LAST_SEEN_LANE: gpui_kit::Pixels = gpui_kit::px(72.);
/// The preview's DATE lane: 48px (`spacing-6` × 2).
const DATE_LANE: gpui_kit::Pixels = gpui_kit::px(48.);

/// A category's marker: its palette colour, or "Other".
pub fn marker(data: &RulesData, key: &CategoryKey) -> Marker {
    match data.palette.slot(key) {
        Some(slot) => color::CHART
            .get(usize::from(slot).saturating_sub(1))
            .copied()
            .map_or(Marker::Other, Marker::Colour),
        None => Marker::Other,
    }
}

fn category_label(data: &RulesData, key: &CategoryKey) -> String {
    data.category(key)
        .map_or_else(|| key.as_str().to_owned(), |option| option.label.clone())
}

/// A fixed-width lane of a row.
fn lane(width: gpui_kit::Pixels) -> gpui_kit::Div {
    div()
        .flex()
        .flex_none()
        .items_center()
        .w(width)
        .overflow_hidden()
        .whitespace_nowrap()
}

/// A caps lane label: 11/14 600 `text-muted`.
fn lane_label(label: &'static str) -> impl IntoElement {
    div()
        .flex()
        .text_color(color::TEXT_MUTED)
        .child(Tracked::new(typography::SECTION_LABEL, label))
}

/// A table row: 28px, 12px gaps, 8px inline padding, radius 6; the
/// selected one `molten-soft`, the others `hover` under the pointer.
fn row(id: impl Into<SharedString>, selected: bool) -> gpui_kit::Stateful<gpui_kit::Div> {
    div()
        .id(id.into())
        .flex()
        .flex_none()
        .items_center()
        .gap(spacing::S3)
        .h(size::CONTROL)
        .px(spacing::S2)
        .rounded(radius::MD)
        .map(|this| {
            if selected {
                this.bg(color::MOLTEN_SOFT)
            } else {
                this.hover(|style| style.bg(color::HOVER))
            }
        })
}

fn header_row() -> gpui_kit::Div {
    div()
        .flex()
        .flex_none()
        .items_center()
        .gap(spacing::S3)
        .h(size::CONTROL)
        .px(spacing::S2)
}

/// The caption under a table (16px above).
fn note(words: &'static str) -> impl IntoElement {
    div()
        .px(spacing::S2)
        .pt(spacing::S4)
        .text_role(typography::CAPTION)
        .text_color(color::TEXT_MUTED)
        .child(words)
}

fn body() -> gpui_kit::Stateful<gpui_kit::Div> {
    div()
        .id("rules-body")
        .flex()
        .flex_col()
        .flex_1()
        .min_h_0()
        .overflow_y_scroll()
        .px(spacing::S2)
        .pt(spacing::S2)
        .pb(spacing::S4)
}

// Toolbar ------------------------------------------------------------------------

/// The toolbar (52px): the title, its caption, and the section's actions.
pub fn toolbar(entity: &Entity<Rules>, window: &Window, cx: &mut Context<RulesView>) -> AnyElement {
    let rules = entity.read(cx);
    let data = rules.data();
    let caption_text = |words: String| {
        div()
            .flex_1()
            .min_w_0()
            .whitespace_nowrap()
            .text_ellipsis()
            .text_role(typography::CAPTION)
            .text_color(color::TEXT_MUTED)
            .child(words)
            .into_any_element()
    };
    let (back, title, caption, trailing): (bool, &str, AnyElement, Option<AnyElement>) =
        match (rules.section(), rules.apply()) {
            (Section::Rules, Some(apply)) => {
                let caption = match rules.form() {
                    Some(form) => {
                        let lead = match apply.target {
                            Target::New => "New rule: ".to_owned(),
                            Target::Stored(id) => {
                                format!("Rule {}: ", data.position(id).map_or(0, |at| at + 1))
                            }
                        };
                        let mut parts = vec![Part::text(lead)];
                        match form.definition(data.currency(form.account)) {
                            Ok(rule) => {
                                parts.push(Part::text(format!("{} ", text::condition(&rule))));
                                parts.push(
                                    Part::mono(text::pattern(&rule)).color(color::TEXT_SECONDARY),
                                );
                                parts.push(Part::text(format!(
                                    " → {}",
                                    text::then_words(&rule, data)
                                )));
                            }
                            Err(_) => parts.push(Part::text("not finished")),
                        }
                        parts.push(Part::text(" · nothing changes until you apply"));
                        div()
                            .flex_1()
                            .min_w_0()
                            .overflow_hidden()
                            .whitespace_nowrap()
                            .text_role(typography::CAPTION)
                            .child(rich(window, typography::CAPTION, color::TEXT_MUTED, parts))
                            .into_any_element()
                    }
                    None => caption_text(String::new()),
                };
                (true, "Apply to past transactions", caption, None)
            }
            (Section::Rules, None) => {
                let new_rule = {
                    let entity = entity.clone();
                    Button::new("rules-new-rule", "New rule")
                        .icon(Glyph::Plus)
                        .with_variant(ButtonVariant::Outline)
                        .on_click(move |_, _, cx| entity.update(cx, |rules, cx| rules.new_rule(cx)))
                };
                let keys = div()
                    .flex()
                    .flex_none()
                    .items_center()
                    .gap(spacing::S3)
                    .when(!data.rules.is_empty(), |this| {
                        this.child(
                            div()
                                .flex()
                                .items_center()
                                .gap(spacing::S1_5)
                                .child(Kbd::new("↑↓"))
                                .child(
                                    div()
                                        .text_role(typography::CAPTION)
                                        .text_color(color::TEXT_MUTED)
                                        .child("Move"),
                                ),
                        )
                    })
                    .child(new_rule);
                (
                    false,
                    "Rules",
                    caption_text(format!(
                        "{} · checked top to bottom on every import",
                        text::count(data.rules.len(), "rule", "rules")
                    )),
                    Some(keys.into_any_element()),
                )
            }
            (Section::Names, _) => {
                let entity = entity.clone();
                (
                    false,
                    "Merchant names",
                    caption_text(format!(
                        "{} · how a bank’s merchant text is shown",
                        text::count(data.names.len(), "name", "names")
                    )),
                    Some(
                        Button::new("rules-new-name", "New name")
                            .icon(Glyph::Plus)
                            .with_variant(ButtonVariant::Outline)
                            .on_click(move |_, _, cx| {
                                entity.update(cx, |rules, cx| rules.new_name(cx))
                            })
                            .into_any_element(),
                    ),
                )
            }
            (Section::History, _) => (
                false,
                "Change history",
                caption_text(format!(
                    "{} to existing transactions · newest first",
                    text::count(data.history.len(), "change", "changes")
                )),
                None,
            ),
            (Section::Categories, _) => {
                let entity = entity.clone();
                let hidden = data.categories.iter().filter(|c| c.hidden).count();
                let hidden_words = if hidden > 0 {
                    format!(" · {hidden} hidden")
                } else {
                    String::new()
                };
                (
                    false,
                    "Categories",
                    caption_text(format!(
                        "{}{hidden_words} · what transactions are filed under",
                        text::count(data.categories.len(), "category", "categories")
                    )),
                    Some(
                        Button::new("rules-new-category", "New category")
                            .icon(Glyph::Plus)
                            .with_variant(ButtonVariant::Outline)
                            .on_click(move |_, _, cx| {
                                entity.update(cx, |rules, cx| rules.new_category(cx))
                            })
                            .into_any_element(),
                    ),
                )
            }
        };
    div()
        .flex()
        .flex_none()
        .items_center()
        .gap(spacing::S2)
        .h(size::CONTROL + spacing::S6)
        .py(spacing::S3)
        .pl(if back { spacing::S2 } else { spacing::S4 })
        .pr(spacing::S4)
        .border_b(size::HAIRLINE)
        .border_color(color::HAIRLINE)
        .when(back, |this| {
            let entity = entity.clone();
            this.child(
                IconButton::new("rules-back", Glyph::Back, "Back to rules")
                    .with_size(IconButtonSize::Small)
                    .on_click(move |_, _, cx| entity.update(cx, |rules, cx| rules.back(cx))),
            )
        })
        .child(
            div()
                .flex()
                .flex_none()
                .text_color(color::TEXT)
                .child(Tracked::new(typography::TITLE, title)),
        )
        .child(caption)
        .children(trailing)
        .into_any_element()
}

// Canvas -------------------------------------------------------------------------

/// The open section's canvas.
pub fn render(
    entity: &Entity<Rules>,
    window: &mut Window,
    cx: &mut Context<RulesView>,
) -> AnyElement {
    let rules = entity.read(cx);
    match (rules.section(), rules.apply().is_some()) {
        (Section::Rules, true) => apply_body(entity, rules),
        (Section::Rules, false) => rules_body(entity, rules, window),
        (Section::Names, _) => names_body(entity, rules, window),
        (Section::History, _) => history_body(entity, rules, window),
        (Section::Categories, _) => categories_body(entity, rules),
    }
}

/// An empty section, centred (Paper `HFL-0`): the icon in a 36px `field`
/// well, the title and a caption.
fn empty(glyph: Glyph, title: &'static str, caption: &'static str) -> AnyElement {
    div()
        .flex()
        .flex_1()
        .flex_col()
        .items_center()
        .justify_center()
        .gap(spacing::S2)
        .child(
            div()
                .flex()
                .flex_none()
                .items_center()
                .justify_center()
                .size(size::CONTROL_XL)
                .rounded(radius::SURFACE)
                .bg(color::FIELD)
                .child(glyph.render().text_color(color::TEXT_MUTED)),
        )
        .child(
            div()
                .flex()
                .pt(spacing::S1)
                .text_color(color::TEXT)
                .child(Tracked::new(typography::TITLE, title)),
        )
        .child(
            div()
                .w(size::PANEL + spacing::S6 * 2.)
                .text_center()
                .text_role(typography::CAPTION)
                .text_color(color::TEXT_MUTED)
                .child(caption),
        )
        .into_any_element()
}

/// What's dragged when a rule row moves.
#[derive(Clone)]
struct DraggedRule {
    id: coffer_core::id::RuleId,
    label: SharedString,
}

/// The row under the pointer while it's dragged (composed: the row's WHEN
/// on a `float` surface with the Toolbar shadow).
struct DragGhost {
    label: SharedString,
}

impl Render for DragGhost {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div()
            .flex()
            .items_center()
            .h(size::CONTROL)
            .px(spacing::S2)
            .rounded(radius::MD)
            .bg(color::FLOAT)
            .border(size::HAIRLINE)
            .border_color(color::BORDER)
            .shadow(vec![shadow::TOOLBAR.box_shadow()])
            .text_role(typography::BODY)
            .text_color(color::TEXT)
            .child(self.label.clone())
    }
}

fn rules_body(entity: &Entity<Rules>, rules: &Rules, window: &Window) -> AnyElement {
    let data = rules.data();
    if data.rules.is_empty() {
        return empty(
            Glyph::ArrowLarge,
            "No rules yet",
            "Set a category on a transaction and Coffer offers to turn it into a rule once the \
             merchant comes back. Or start one with New rule.",
        );
    }
    let header = header_row()
        .child(lane(size::CONTROL_SM).child(lane_label("#")))
        .child(div().flex().flex_1().min_w_0().child(lane_label("When")))
        .child(lane(size::COLUMN_ACCOUNT).child(lane_label("Account")))
        .child(lane(size::COLUMN_CATEGORY).child(lane_label("Then")))
        .child(
            lane(size::COLUMN_AMOUNT)
                .justify_end()
                .child(lane_label("Matches")),
        );
    let selected = rules.selected_rule_id();
    let rows: Vec<AnyElement> = rules
        .rules()
        .into_iter()
        .map(|summary| rule_row(entity, data, summary, selected, window))
        .collect();
    body()
        .child(header)
        .children(rows)
        .child(note(
            "Each field is set by the first rule that matches, so put a specific rule above a \
             broader one; drag a row to reorder. Rules run on new imports. Past transactions \
             change only through Apply to past transactions, which shows every change first.",
        ))
        .into_any_element()
}

fn rule_row(
    entity: &Entity<Rules>,
    data: &RulesData,
    summary: &RuleSummary,
    selected: Option<coffer_core::id::RuleId>,
    window: &Window,
) -> AnyElement {
    let rule = summary.rule.definition();
    let id = summary.rule.id();
    let position = data.position(id).unwrap_or(0);
    let selected = selected == Some(id);
    let (strong, pattern, secondary, faint) = if selected {
        (
            color::WHITE,
            color::WHITE,
            color::TEXT_STRONG,
            color::TEXT_SECONDARY,
        )
    } else {
        (
            color::TEXT,
            color::TEXT,
            color::TEXT_SECONDARY,
            color::TEXT_FAINT,
        )
    };
    let currency = data.currency(rule.account_id().map(coffer_core::id::AccountId::new));
    let when = rich(
        window,
        typography::BODY,
        color::TEXT_SECONDARY,
        text::when_parts(rule, currency, pattern, color::TEXT_MUTED),
    );
    let account = match rule
        .account_id()
        .and_then(|id| data.account(coffer_core::id::AccountId::new(id)))
    {
        Some(account) => div()
            .text_color(secondary)
            .child(text::plain(&account.name, text::NAME_SHOWN)),
        None => div()
            .text_color(if selected {
                color::TEXT_STRONG
            } else {
                color::TEXT_MUTED
            })
            .child("Any account"),
    };
    let (lead, suffixes) = text::then(rule, data);
    let then_color = if selected {
        color::WHITE
    } else {
        color::TEXT_STRONG
    };
    let suffix = suffixes
        .iter()
        .map(|word| format!(" + {word}"))
        .collect::<String>();
    let then = match lead {
        ThenLead::Category(key) => div()
            .flex()
            .items_center()
            .gap(spacing::S2)
            .child(marker(data, &key).render())
            .child(rich(
                window,
                typography::BODY,
                then_color,
                vec![
                    Part::text(category_label(data, &key)),
                    Part::text(suffix).color(color::TEXT_MUTED),
                ],
            )),
        ThenLead::Action { kind, word, value } => div()
            .flex()
            .items_center()
            .gap(spacing::S2)
            .child(
                div()
                    .flex()
                    .flex_none()
                    .items_center()
                    .justify_center()
                    .size(size::MARKER)
                    .child(
                        text::action_glyph(kind)
                            .render()
                            .size(size::MARKER)
                            .text_color(color::TEXT_MUTED),
                    ),
            )
            .child(rich(
                window,
                typography::BODY,
                then_color,
                vec![
                    Part::text(format!("{word} ")).color(color::TEXT_MUTED),
                    Part::text(value),
                    Part::text(suffix).color(color::TEXT_MUTED),
                ],
            )),
    };
    let matches = if summary.never_reached() {
        div()
            .flex()
            .items_center()
            .justify_end()
            .gap(spacing::S2)
            .child(
                div()
                    .text_role(typography::CAPTION)
                    .text_color(color::WARNING)
                    .child("Never reached"),
            )
            .child(
                div()
                    .text_role(typography::MONO_AMOUNT)
                    .text_color(color::TEXT_MUTED)
                    .child("0"),
            )
    } else {
        div()
            .text_role(typography::MONO_AMOUNT)
            .text_color(strong)
            .child(summary.decides.to_string())
    };
    let label = SharedString::from(format!("{} {}", text::condition(rule), text::pattern(rule)));
    let (select, dropped) = (entity.clone(), entity.clone());
    row(format!("rule-{id}"), selected)
        .on_click(move |_, _, cx| select.update(cx, |rules, cx| rules.toggle_rule(id, cx)))
        .on_drag(DraggedRule { id, label }, |dragged, _, _, cx| {
            let label = dragged.label.clone();
            cx.new(|_| DragGhost { label })
        })
        .drag_over::<DraggedRule>(|style, _, _, _| style.bg(color::HOVER))
        .on_drop(move |dragged: &DraggedRule, _, cx| {
            dropped.update(cx, |rules, cx| rules.move_rule(dragged.id, position, cx))
        })
        .child(
            lane(size::CONTROL_SM)
                .text_role(typography::MONO_VALUE)
                .text_color(faint)
                .child((position + 1).to_string()),
        )
        .child(
            div()
                .flex()
                .flex_1()
                .min_w_0()
                .overflow_hidden()
                .whitespace_nowrap()
                .text_role(typography::BODY)
                .child(when),
        )
        .child(
            lane(size::COLUMN_ACCOUNT)
                .text_role(typography::BODY)
                .child(account),
        )
        .child(lane(size::COLUMN_CATEGORY).child(then))
        .child(lane(size::COLUMN_AMOUNT).justify_end().child(matches))
        .test_support()
        .into_any_element()
}

// Apply preview ------------------------------------------------------------------

fn preview_row(
    data: &RulesData,
    row: &PreviewRow,
    today: chrono::NaiveDate,
    included: bool,
) -> AnyElement {
    let left_out = row.user_set && !included;
    let tone = if left_out {
        color::TEXT_SECONDARY
    } else {
        color::TEXT
    };
    let now = match &row.now {
        Some(key) => div()
            .flex()
            .items_center()
            .gap(spacing::S2)
            .child(marker(data, key).render())
            .child(
                div()
                    .text_color(if row.user_set {
                        color::TEXT_STRONG
                    } else {
                        color::TEXT
                    })
                    .child(category_label(data, key)),
            ),
        None => div()
            .flex()
            .items_center()
            .gap(spacing::S2)
            .child(Marker::None.render())
            .child(div().text_color(color::TEXT_MUTED).child("Uncategorised")),
    };
    let after = if left_out {
        div().text_color(color::TEXT_MUTED).child("No change")
    } else {
        match &row.after {
            PreviewAfter::Category(key) => div()
                .flex()
                .items_center()
                .gap(spacing::S2)
                .child(marker(data, key).render())
                .child(
                    div()
                        .text_color(color::TEXT)
                        .child(category_label(data, key)),
                ),
            PreviewAfter::Split(lines) => div()
                .text_color(color::TEXT)
                .child(format!("Split in {lines}")),
            PreviewAfter::Unchanged => div().text_color(color::TEXT_MUTED).child("Other change"),
        }
    };
    let account = data
        .account(row.account)
        .map_or(String::new(), |account| account.name.clone());
    row_plain(row.transaction)
        .child(
            div()
                .flex_1()
                .min_w_0()
                .overflow_hidden()
                .whitespace_nowrap()
                .text_ellipsis()
                .text_role(typography::BODY)
                .text_color(tone)
                .child(text::plain(&row.name, text::NAME_SHOWN)),
        )
        .child(
            lane(size::COLUMN_ACCOUNT)
                .text_role(typography::BODY)
                .text_color(color::TEXT_SECONDARY)
                .child(text::plain(&account, text::NAME_SHOWN)),
        )
        .child(
            lane(size::COLUMN_ACCOUNT)
                .text_role(typography::BODY)
                .child(now),
        )
        .child(
            lane(size::COLUMN_ACCOUNT)
                .text_role(typography::BODY)
                .child(after),
        )
        .child(
            lane(DATE_LANE)
                .justify_end()
                .text_role(typography::MONO_VALUE)
                .text_color(color::TEXT_MUTED)
                .child(text::date(row.date, today)),
        )
        .child(
            lane(size::COLUMN_AMOUNT)
                .justify_end()
                .text_role(typography::MONO_AMOUNT)
                .text_color(tone)
                .child(format::money(row.amount, Sign::Always)),
        )
        .into_any_element()
}

fn row_plain(id: coffer_core::id::TransactionId) -> gpui_kit::Stateful<gpui_kit::Div> {
    div()
        .id(SharedString::from(format!("preview-{id}")))
        .flex()
        .flex_none()
        .items_center()
        .gap(spacing::S3)
        .h(size::CONTROL)
        .px(spacing::S2)
        .rounded(radius::MD)
}

/// A section's header in the preview: its name in caption 600
/// `text-secondary` and its count in mono `text-muted`.
fn group_title(title: &'static str, count: usize) -> gpui_kit::Div {
    div()
        .flex()
        .flex_1()
        .min_w_0()
        .items_center()
        .gap(spacing::S2)
        .child(
            div()
                .text_role(typography::CAPTION)
                .font_weight(crate::theme::tokens::weight::STRONG)
                .text_color(color::TEXT_SECONDARY)
                .child(title),
        )
        .child(
            div()
                .text_role(typography::MONO_VALUE)
                .text_color(color::TEXT_MUTED)
                .child(count.to_string()),
        )
}

fn apply_body(entity: &Entity<Rules>, rules: &Rules) -> AnyElement {
    let data = rules.data();
    let today = rules.today();
    let Some(apply) = rules.apply() else {
        return div().into_any_element();
    };
    let will: Vec<&PreviewRow> = apply.will_change().collect();
    let user_set: Vec<&PreviewRow> = apply.user_set().collect();
    let valid = apply.preview.is_some();
    let header = header_row()
        .child(group_title("Will change", will.len()))
        .child(lane(size::COLUMN_ACCOUNT).child(lane_label("Account")))
        .child(lane(size::COLUMN_ACCOUNT).child(lane_label("Now")))
        .child(lane(size::COLUMN_ACCOUNT).child(lane_label("After")))
        .child(lane(DATE_LANE).justify_end().child(lane_label("Date")))
        .child(
            lane(size::COLUMN_AMOUNT)
                .justify_end()
                .child(lane_label("Amount")),
        );
    let mut body = body().child(header);
    if !valid {
        body = body.child(
            div()
                .px(spacing::S2)
                .h(size::CONTROL)
                .flex()
                .items_center()
                .text_role(typography::CAPTION)
                .text_color(color::TEXT_MUTED)
                .child("Finish the rule in the inspector to see what it would change."),
        );
    } else if will.is_empty() && user_set.is_empty() {
        body = body.child(
            div()
                .px(spacing::S2)
                .h(size::CONTROL)
                .flex()
                .items_center()
                .text_role(typography::CAPTION)
                .text_color(color::TEXT_MUTED)
                .child("No past transaction would change."),
        );
    }
    body = body.children(
        will.iter()
            .map(|row| preview_row(data, row, today, apply.include)),
    );
    if !user_set.is_empty() {
        let toggle = {
            let entity = entity.clone();
            Toggle::new("rules-change-these-too", apply.include, "Change these too").on_change(
                move |on, _, cx| {
                    let on = *on;
                    entity.update(cx, |rules, cx| rules.set_include(on, cx))
                },
            )
        };
        body = body.child(
            div()
                .flex()
                .flex_col()
                .flex_none()
                .pt(spacing::S2)
                .border_t(size::HAIRLINE)
                .border_color(color::HAIRLINE)
                .child(
                    header_row()
                        .child(
                            group_title("Category you set", user_set.len()).child(
                                div()
                                    .text_role(typography::CAPTION)
                                    .text_color(color::TEXT_MUTED)
                                    .child("· left as they are unless you include them"),
                            ),
                        )
                        .child(
                            div()
                                .flex()
                                .flex_none()
                                .items_center()
                                .gap(spacing::S2)
                                .child(
                                    div()
                                        .text_role(typography::CAPTION)
                                        .text_color(color::TEXT_SECONDARY)
                                        .child("Change these too"),
                                )
                                .child(toggle),
                        ),
                )
                .children(
                    user_set
                        .iter()
                        .map(|row| preview_row(data, row, today, apply.include)),
                ),
        );
    }
    let changing = apply.changing();
    let (primary, secondary) = match apply.target {
        Target::New => (
            format!("Create rule and change {changing}"),
            Some("Create rule only"),
        ),
        Target::Stored(_) => (
            format!(
                "Apply to {}",
                text::count(changing, "transaction", "transactions")
            ),
            None,
        ),
    };
    let commit = {
        let entity = entity.clone();
        Button::new("rules-commit", primary)
            .with_variant(ButtonVariant::Primary)
            .disabled(!valid || changing == 0)
            .on_click(move |_, _, cx| entity.update(cx, |rules, cx| rules.commit(cx)))
    };
    let create_only = secondary.map(|label| {
        let entity = entity.clone();
        Button::new("rules-create-only", label)
            .with_variant(ButtonVariant::Secondary)
            .disabled(!valid)
            .on_click(move |_, _, cx| entity.update(cx, |rules, cx| rules.create_only(cx)))
    });
    body.child(
        div()
            .flex()
            .flex_none()
            .items_center()
            .gap(spacing::S1_5)
            .px(spacing::S2)
            .pt(spacing::S3_5)
            .border_t(size::HAIRLINE)
            .border_color(color::HAIRLINE)
            .child(commit)
            .children(create_only)
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .pl(spacing::S2)
                    .text_role(typography::CAPTION)
                    .text_color(color::TEXT_MUTED)
                    .child(
                        "Saved as one change. Undo it from the bar that follows, or later from \
                         Change history.",
                    ),
            ),
    )
    .into_any_element()
}

// Merchant names -----------------------------------------------------------------

fn name_row(
    entity: &Entity<Rules>,
    summary: &AliasSummary,
    selected: bool,
    today: chrono::NaiveDate,
    window: &Window,
) -> AnyElement {
    let id = summary.alias.id();
    let definition = summary.alias.definition();
    let (strong, pattern, muted) = if selected {
        (color::WHITE, color::WHITE, color::TEXT_SECONDARY)
    } else {
        (color::TEXT, color::TEXT, color::TEXT_MUTED)
    };
    let bank = rich(
        window,
        typography::BODY,
        color::TEXT_SECONDARY,
        vec![
            Part::text(format!("{} ", text::alias_kind(definition.kind()))),
            Part::mono(text::plain(definition.pattern(), text::PATTERN_SHOWN)).color(pattern),
        ],
    );
    let select = entity.clone();
    row(format!("name-{id}"), selected)
        .on_click(move |_, _, cx| select.update(cx, |rules, cx| rules.toggle_name(id, cx)))
        .child(
            div()
                .flex()
                .flex_1()
                .min_w_0()
                .overflow_hidden()
                .whitespace_nowrap()
                .text_role(typography::BODY)
                .child(bank),
        )
        .child(
            lane(size::COLUMN_CATEGORY)
                .text_role(typography::BODY)
                .text_color(strong)
                .child(text::plain(definition.name().as_str(), text::NAME_SHOWN)),
        )
        .child(
            lane(LAST_SEEN_LANE)
                .justify_end()
                .text_role(typography::MONO_VALUE)
                .text_color(muted)
                .child(
                    summary
                        .last
                        .map_or(String::new(), |date| text::date(date, today)),
                ),
        )
        .child(
            lane(size::COLUMN_AMOUNT)
                .justify_end()
                .text_role(typography::MONO_AMOUNT)
                .text_color(strong)
                .child(summary.matches.to_string()),
        )
        .test_support()
        .into_any_element()
}

fn names_body(entity: &Entity<Rules>, rules: &Rules, window: &Window) -> AnyElement {
    let today = rules.today();
    if rules.data().names.is_empty() && rules.selected_name().is_none() {
        return empty(
            Glyph::NameTag,
            "No merchant names yet",
            "A name shows a bank’s merchant text the way you want it. Start one with New name.",
        );
    }
    let selected = match rules.selected_name() {
        Some(super::NameSelection::Stored(id)) => Some(id),
        _ => None,
    };
    let header = header_row()
        .child(
            div()
                .flex()
                .flex_1()
                .min_w_0()
                .child(lane_label("Bank text")),
        )
        .child(lane(size::COLUMN_CATEGORY).child(lane_label("Shows as")))
        .child(
            lane(LAST_SEEN_LANE)
                .justify_end()
                .child(lane_label("Last seen")),
        )
        .child(
            lane(size::COLUMN_AMOUNT)
                .justify_end()
                .child(lane_label("Matches")),
        );
    body()
        .child(header)
        .children(rules.names().into_iter().map(|summary| {
            name_row(
                entity,
                summary,
                Some(summary.alias.id()) == selected,
                today,
                window,
            )
        }))
        .child(note(
            "Coffer first cleans the bank’s text (card numbers, “CONTACTLESS”, payment-processor \
             prefixes), then applies these names. A name you type on a single transaction wins \
             over both. Renaming changes only what you see: rules still match the bank’s text, \
             and the bank’s text is never changed.",
        ))
        .into_any_element()
}

// Change history -----------------------------------------------------------------

fn history_row(
    entity: &Entity<Rules>,
    data: &RulesData,
    entry: &HistoryEntry,
    selected: bool,
    today: chrono::NaiveDate,
    window: &Window,
) -> AnyElement {
    let id = entry.batch.id;
    let undone = entry.batch.status == BatchStatus::Undone;
    let (made, undone_at) = data
        .history_at
        .get(&id)
        .copied()
        .map_or((None, None), |(made, undone)| (Some(made), undone));
    let (when_tone, change_tone, kind_tone, count_tone) = match (selected, undone) {
        (_, true) => (
            color::TEXT_FAINT,
            color::TEXT_MUTED,
            color::TEXT_MUTED,
            color::TEXT_MUTED,
        ),
        (true, false) => (
            color::TEXT_SECONDARY,
            color::WHITE,
            color::TEXT_STRONG,
            color::WHITE,
        ),
        (false, false) => (
            color::TEXT_MUTED,
            color::TEXT,
            color::TEXT_SECONDARY,
            color::TEXT,
        ),
    };
    let parts = text::change_parts(entry, data);
    let change = div()
        .flex()
        .flex_1()
        .min_w_0()
        .overflow_hidden()
        .whitespace_nowrap()
        .text_role(typography::BODY)
        .map(|this| {
            if undone {
                this.child(text::struck(window, typography::BODY, change_tone, parts))
            } else {
                this.child(rich(window, typography::BODY, change_tone, parts))
            }
        });
    let kind = if undone {
        match undone_at {
            Some(at) => format!("Undone {}", text::date(at.date(), today)),
            None => "Undone".to_owned(),
        }
    } else {
        text::change_kind(entry.batch.kind).to_owned()
    };
    let select = entity.clone();
    row(format!("change-{id}"), selected)
        .on_click(move |_, _, cx| select.update(cx, |rules, cx| rules.toggle_batch(id, cx)))
        .child(
            lane(size::COLUMN_AMOUNT)
                .text_role(typography::MONO_VALUE)
                .text_color(when_tone)
                .child(made.map_or(String::new(), |at| text::when(at, today))),
        )
        .child(change)
        .child(
            lane(size::COLUMN_ACCOUNT)
                .text_role(typography::BODY)
                .text_color(kind_tone)
                .child(kind),
        )
        .child(
            lane(size::COLUMN_AMOUNT)
                .justify_end()
                .text_role(typography::MONO_AMOUNT)
                .text_color(count_tone)
                .child(entry.batch.transactions.to_string()),
        )
        .test_support()
        .into_any_element()
}

fn history_body(entity: &Entity<Rules>, rules: &Rules, window: &Window) -> AnyElement {
    let data = rules.data();
    let today = rules.today();
    if data.history.is_empty() {
        return empty(
            Glyph::History,
            "No changes yet",
            "Applying a rule, a bulk edit or merging duplicates is listed here, and each can be \
             undone. Imports have their own history on each account.",
        );
    }
    let selected = rules.selected_batch().map(|entry| entry.batch.id);
    let header = header_row()
        .child(lane(size::COLUMN_AMOUNT).child(lane_label("When")))
        .child(div().flex().flex_1().min_w_0().child(lane_label("Change")))
        .child(lane(size::COLUMN_ACCOUNT).child(lane_label("Kind")))
        .child(
            lane(size::COLUMN_AMOUNT)
                .justify_end()
                .child(lane_label("Transactions")),
        );
    body()
        .child(header)
        .children(rules.history().into_iter().map(|entry| {
            history_row(
                entity,
                data,
                entry,
                Some(entry.batch.id) == selected,
                today,
                window,
            )
        }))
        .child(note(
            "A change is anything that edits many existing transactions at once: applying a \
             rule, a bulk edit, merging duplicates. Imports have their own history on each \
             account. Undo puts back the values from before the change.",
        ))
        .into_any_element()
}

// Categories ---------------------------------------------------------------------

/// A count in a table lane: mono, `text-faint` when it's zero.
fn count_lane(width: gpui_kit::Pixels, count: usize, tone: gpui_kit::Rgba) -> gpui_kit::Div {
    lane(width)
        .justify_end()
        .text_role(typography::MONO_AMOUNT)
        .text_color(if count == 0 { color::TEXT_FAINT } else { tone })
        .child(count.to_string())
}

fn category_row(
    entity: &Entity<Rules>,
    data: &RulesData,
    category_row: &super::CategoryRow<'_>,
    selected: bool,
) -> AnyElement {
    let category = &category_row.summary.category;
    let usage = category_row.summary.usage;
    let (child, hidden) = (category_row.child, category_row.hidden);
    let id = category.id();
    let (label_tone, secondary, count_tone) = match (selected, hidden) {
        (true, true) => (
            color::TEXT_SECONDARY,
            color::TEXT_SECONDARY,
            color::TEXT_STRONG,
        ),
        (true, false) => (color::WHITE, color::TEXT_STRONG, color::WHITE),
        (false, true) => (color::TEXT_MUTED, color::TEXT_MUTED, color::TEXT_SECONDARY),
        (false, false) => (color::TEXT, color::TEXT_SECONDARY, color::TEXT),
    };
    let name = div()
        .flex()
        .flex_1()
        .min_w_0()
        .items_center()
        .gap(spacing::S2)
        // A subcategory's marker lines up with its group's label.
        .when(child, |this| this.pl(size::MARKER + spacing::S2))
        .child(marker(data, category.key()).render())
        .child(
            div()
                .min_w_0()
                .overflow_hidden()
                .whitespace_nowrap()
                .text_ellipsis()
                .text_role(typography::BODY)
                .when(!child, |this| {
                    this.font_weight(crate::theme::tokens::weight::MEDIUM)
                })
                .text_color(label_tone)
                .child(category.label().to_owned()),
        )
        .when(hidden, |this| {
            this.child(
                div()
                    .flex_none()
                    .text_role(typography::CAPTION)
                    .text_color(color::TEXT_MUTED)
                    .child("Hidden"),
            )
        });
    let select = entity.clone();
    row(format!("category-{id}"), selected)
        .on_click(move |_, _, cx| select.update(cx, |rules, cx| rules.toggle_category(id, cx)))
        .child(name)
        .child(
            lane(size::COLUMN_ACCOUNT)
                .text_role(typography::BODY)
                .text_color(secondary)
                .child(text::category_kind(category.kind())),
        )
        .child(count_lane(LAST_SEEN_LANE, usage.rules, count_tone))
        .child(count_lane(
            size::COLUMN_AMOUNT,
            usage.transactions + usage.split_lines,
            count_tone,
        ))
        .test_support()
        .into_any_element()
}

fn categories_body(entity: &Entity<Rules>, rules: &Rules) -> AnyElement {
    let data = rules.data();
    let selected = match rules.selected_category() {
        Some(super::CategorySelection::Stored(id)) => Some(id),
        _ => None,
    };
    let header = header_row()
        .child(
            div()
                .flex()
                .flex_1()
                .min_w_0()
                .child(lane_label("Category")),
        )
        .child(lane(size::COLUMN_ACCOUNT).child(lane_label("Kind")))
        .child(
            lane(LAST_SEEN_LANE)
                .justify_end()
                .child(lane_label("Rules")),
        )
        .child(
            lane(size::COLUMN_AMOUNT)
                .justify_end()
                .child(lane_label("Transactions")),
        );
    let rows: Vec<AnyElement> = rules
        .category_rows()
        .iter()
        .map(|row| {
            category_row(
                entity,
                data,
                row,
                Some(row.summary.category.id()) == selected,
            )
        })
        .collect();
    body()
        .child(header)
        .children(rows)
        .child(note(
            "A category’s name is what you see; rules, presets and AI clients use its key, which \
             never changes, so renaming is safe. A category in use can’t be deleted: hide it to \
             stop offering it, and its transactions and reports stay as they are.",
        ))
        .into_any_element()
}
