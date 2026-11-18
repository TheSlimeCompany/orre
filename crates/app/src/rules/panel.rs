//! The Rules view's left panel (`docs/DESIGN.md` → "Rules"; Paper `B1J-0`,
//! `G2O-0`, `GZT-0`): RULES (Rules, Merchant names, Change history,
//! Categories, each with its count), then the open list's filter: ACTION
//! for rules, MATCH for merchant names, KIND for the history and for
//! categories (with Hidden). A second click on a filter clears it. Zero
//! counts stay listed, in mono `text-faint`.
//!
//! A Tab stop (`DESIGN.md` → "Keyboard paths"): ↑↓ move a keyboard cursor
//! through every row, sections and filters, and Enter or Space act on its
//! row as a click does ([`crate::ui::panel_cursor`]). The keys don't change
//! what's open or filtered; the cursor starts on the open section.

use coffer_core::change::ChangeKind;
use coffer_core::merchant::AliasKind;
use coffer_core::rule::ActionKind;
use gpui_kit::{
    App, Entity, InteractiveElement as _, IntoElement, ParentElement as _, RenderOnce,
    SharedString, StatefulInteractiveElement as _, Styled as _, Window, div,
};

use crate::theme::tokens::{color, size, spacing, typography};
use crate::ui::icon::Glyph;
use crate::ui::list_row::{ListRow, TrailingTone};
use crate::ui::panel_cursor::{PanelCursor, PanelCursorExt as _, PanelKeys};
use crate::ui::section_label::SectionLabelRow;
use crate::ui::text::TextRoleExt as _;

use super::text;
use super::{CategoryFilter, Rules, Section};

/// The panel's key context, where its keyboard cursor moves.
pub const CONTEXT: &str = "CofferRulesPanel";

/// The panel's keys: ↑↓ move the cursor, Enter and Space act on its row.
pub const KEYS: PanelKeys = PanelKeys::new(CONTEXT);

/// A row of the panel, by what a click (or Enter or Space on the cursor)
/// does with it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PanelRow {
    /// Opens a section.
    Section(Section),
    /// Toggles ACTION (rules).
    Action(ActionKind),
    /// Toggles MATCH (merchant names).
    Match(AliasKind),
    /// Toggles KIND (change history).
    Kind(ChangeKind),
    /// Toggles KIND or Hidden (categories).
    Category(CategoryFilter),
}

/// The sections under RULES, in order.
const SECTIONS: [Section; 4] = [
    Section::Rules,
    Section::Names,
    Section::History,
    Section::Categories,
];

/// The Categories section's filters: each kind, then Hidden.
fn category_filters() -> impl Iterator<Item = CategoryFilter> {
    text::CATEGORY_KINDS
        .map(CategoryFilter::Kind)
        .into_iter()
        .chain([CategoryFilter::Hidden])
}

impl PanelRow {
    /// Every row of the panel showing `open`, in panel order: the sections,
    /// then the open section's filters.
    pub fn all(open: Section) -> Vec<Self> {
        let filters: Vec<Self> = match open {
            Section::Rules => text::PANEL_ACTIONS.map(Self::Action).to_vec(),
            Section::Names => text::ALIAS_KINDS.map(Self::Match).to_vec(),
            Section::History => text::CHANGE_KINDS.map(Self::Kind).to_vec(),
            Section::Categories => category_filters().map(Self::Category).collect(),
        };
        SECTIONS
            .map(Self::Section)
            .into_iter()
            .chain(filters)
            .collect()
    }

    /// Does what clicking the row does.
    pub fn act(self, rules: &mut Rules, cx: &mut gpui_kit::Context<Rules>) {
        match self {
            Self::Section(section) => rules.show(section, cx),
            Self::Action(kind) => rules.toggle_action_filter(kind, cx),
            Self::Match(kind) => rules.toggle_match_filter(kind, cx),
            Self::Kind(kind) => rules.toggle_kind_filter(kind, cx),
            Self::Category(filter) => rules.toggle_category_filter(filter, cx),
        }
    }
}

/// A section: 8px (the first) or 4px above, 6px inline, 10px below.
fn section(first: bool, label: &str) -> gpui_kit::Div {
    div()
        .flex()
        .flex_col()
        .flex_none()
        .pt(if first { spacing::S2 } else { spacing::S1 })
        .px(spacing::S1_5)
        .pb(spacing::S2_5)
        .child(SectionLabelRow::new(SharedString::from(label.to_owned())))
}

/// The panel. It draws itself when laid out, where it has the window its
/// keyboard cursor needs.
pub fn render(rules: &Entity<Rules>, _: &App) -> impl IntoElement {
    RulesPanel {
        rules: rules.clone(),
    }
}

#[derive(IntoElement)]
struct RulesPanel {
    rules: Entity<Rules>,
}

impl RenderOnce for RulesPanel {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let cursor = PanelCursor::<PanelRow>::keyed("rules-panel-cursor", window, cx);
        let rules = &self.rules;
        let state = rules.read(cx);
        let data = state.data();
        let open = state.section();
        let rows = PanelRow::all(open);
        let home = Some(PanelRow::Section(open));
        let frame = cursor.read(cx).frame(&rows, home.as_ref(), window);
        // Every row: its look, the cursor's, and what a click does.
        let row = |key: PanelRow, list_row: ListRow, selected: bool| {
            let handle = rules.clone();
            frame
                .mark(&key, list_row)
                .trailing_tone(TrailingTone::Faint)
                .selected(selected)
                .on_click(move |_, _, cx| handle.update(cx, |rules, cx| key.act(rules, cx)))
        };

        let lists = SECTIONS.map(|section| {
            let (id, label, glyph, count) = match section {
                Section::Rules => (
                    "rules-section-rules",
                    "Rules",
                    Glyph::Arrow,
                    data.rules.len(),
                ),
                Section::Names => (
                    "rules-section-names",
                    "Merchant names",
                    Glyph::NameTag,
                    data.names.len(),
                ),
                Section::History => (
                    "rules-section-history",
                    "Change history",
                    Glyph::History,
                    data.history.len(),
                ),
                Section::Categories => (
                    "rules-section-categories",
                    "Categories",
                    Glyph::SetCategory,
                    data.categories.len(),
                ),
            };
            row(
                PanelRow::Section(section),
                ListRow::new(id, label)
                    .icon(glyph)
                    .trailing(count.to_string()),
                open == section,
            )
        });

        let filters = match open {
            Section::Rules => section(false, "Action").children(text::PANEL_ACTIONS.map(|kind| {
                row(
                    PanelRow::Action(kind),
                    ListRow::new(
                        SharedString::from(format!("rules-action-{}", kind.as_str())),
                        text::action_filter_label(kind),
                    )
                    .icon(text::action_glyph(kind))
                    .trailing(state.count_with_action(kind).to_string()),
                    state.action_filter() == Some(kind),
                )
            })),
            Section::Names => section(false, "Match").children(text::ALIAS_KINDS.map(|kind| {
                row(
                    PanelRow::Match(kind),
                    ListRow::new(
                        SharedString::from(format!("rules-match-{}", kind.as_str())),
                        text::alias_kind(kind),
                    )
                    .icon(text::alias_glyph(kind))
                    .trailing(state.count_names(kind).to_string()),
                    state.match_filter() == Some(kind),
                )
            })),
            Section::History => section(false, "Kind").children(text::CHANGE_KINDS.map(|kind| {
                row(
                    PanelRow::Kind(kind),
                    ListRow::new(
                        SharedString::from(format!("rules-kind-{}", kind.as_str())),
                        text::change_kind(kind),
                    )
                    .icon(text::change_glyph(kind))
                    .trailing(state.count_history(kind).to_string()),
                    state.kind_filter() == Some(kind),
                )
            })),
            // Composed: the kinds have no icon of their own, so their rows
            // keep the icon's slot empty; Hidden is the categories pickers
            // leave out.
            Section::Categories => {
                section(false, "Kind").children(category_filters().map(|filter| {
                    let (id, label) = match filter {
                        CategoryFilter::Kind(kind) => (
                            format!("rules-category-kind-{}", kind.as_str()),
                            text::category_kind_filter(kind),
                        ),
                        CategoryFilter::Hidden => ("rules-category-hidden".to_owned(), "Hidden"),
                    };
                    row(
                        PanelRow::Category(filter),
                        ListRow::new(SharedString::from(id), label)
                            .blank_slot()
                            .dimmed(filter == CategoryFilter::Hidden)
                            .trailing(state.count_categories(filter).to_string()),
                        state.category_filter() == Some(filter),
                    )
                }))
            }
        };

        let load_error = state.load_error().cloned();
        let act = rules.clone();
        div()
            .id("rules-panel")
            .panel_cursor(
                &KEYS,
                &cursor,
                rows,
                home,
                move |row, _, cx| act.update(cx, |rules, cx| row.act(rules, cx)),
                cx,
            )
            .flex()
            .flex_col()
            .flex_none()
            .w(size::PANEL)
            .overflow_y_scroll()
            .bg(color::CHROME)
            .border_r(size::HAIRLINE)
            .border_color(color::HAIRLINE)
            .child(section(true, "Rules").children(lists))
            .child(filters)
            .children(load_error.map(|error| {
                div()
                    .px(spacing::S3_5)
                    .text_role(typography::CAPTION)
                    .text_color(color::DANGER)
                    .child(error)
            }))
    }
}
