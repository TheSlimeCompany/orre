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

    /// What the row says.
    pub fn label(self) -> &'static str {
        match self {
            Self::Section(Section::Rules) => "Rules",
            Self::Section(Section::Names) => "Merchant names",
            Self::Section(Section::History) => "Change history",
            Self::Section(Section::Categories) => "Categories",
            Self::Action(kind) => text::action_filter_label(kind),
            Self::Match(kind) => text::alias_kind(kind),
            Self::Kind(kind) => text::change_kind(kind),
            Self::Category(CategoryFilter::Kind(kind)) => text::category_kind_filter(kind),
            Self::Category(CategoryFilter::Hidden) => "Hidden",
        }
    }

    /// The row's element ID.
    pub fn id(self) -> SharedString {
        match self {
            Self::Section(Section::Rules) => "rules-section-rules".into(),
            Self::Section(Section::Names) => "rules-section-names".into(),
            Self::Section(Section::History) => "rules-section-history".into(),
            Self::Section(Section::Categories) => "rules-section-categories".into(),
            Self::Action(kind) => format!("rules-action-{}", kind.as_str()).into(),
            Self::Match(kind) => format!("rules-match-{}", kind.as_str()).into(),
            Self::Kind(kind) => format!("rules-kind-{}", kind.as_str()).into(),
            Self::Category(CategoryFilter::Kind(kind)) => {
                format!("rules-category-kind-{}", kind.as_str()).into()
            }
            Self::Category(CategoryFilter::Hidden) => "rules-category-hidden".into(),
        }
    }

    /// The count after its label: a section's rows, or the rows a filter
    /// keeps.
    pub fn count(self, rules: &Rules) -> usize {
        let data = rules.data();
        match self {
            Self::Section(Section::Rules) => data.rules.len(),
            Self::Section(Section::Names) => data.names.len(),
            Self::Section(Section::History) => data.history.len(),
            Self::Section(Section::Categories) => data.categories.len(),
            Self::Action(kind) => rules.count_with_action(kind),
            Self::Match(kind) => rules.count_names(kind),
            Self::Kind(kind) => rules.count_history(kind),
            Self::Category(filter) => rules.count_categories(filter),
        }
    }

    /// Whether it's selected: the open section, or a filter that's on.
    pub fn selected(self, rules: &Rules) -> bool {
        match self {
            Self::Section(section) => rules.section() == section,
            Self::Action(kind) => rules.action_filter() == Some(kind),
            Self::Match(kind) => rules.match_filter() == Some(kind),
            Self::Kind(kind) => rules.kind_filter() == Some(kind),
            Self::Category(filter) => rules.category_filter() == Some(filter),
        }
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

/// The title of `open`'s filters.
pub fn filter_title(open: Section) -> &'static str {
    match open {
        Section::Rules => "Action",
        Section::Names => "Match",
        Section::History | Section::Categories => "Kind",
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
            let glyph = match section {
                Section::Rules => Glyph::Arrow,
                Section::Names => Glyph::NameTag,
                Section::History => Glyph::History,
                Section::Categories => Glyph::SetCategory,
            };
            let key = PanelRow::Section(section);
            row(
                key,
                ListRow::new(key.id(), key.label())
                    .icon(glyph)
                    .trailing(key.count(state).to_string()),
                key.selected(state),
            )
        });

        // A filter row: its icon, or (Categories' KIND) an empty slot, with
        // Hidden dimmed.
        let filter = |key: PanelRow| {
            let list_row = ListRow::new(key.id(), key.label());
            let list_row = match key {
                PanelRow::Action(kind) => list_row.icon(text::action_glyph(kind)),
                PanelRow::Match(kind) => list_row.icon(text::alias_glyph(kind)),
                PanelRow::Kind(kind) => list_row.icon(text::change_glyph(kind)),
                // Composed: the kinds have no icon of their own, so their
                // rows keep the icon's slot empty; Hidden is the categories
                // pickers leave out.
                PanelRow::Category(filter) => list_row
                    .blank_slot()
                    .dimmed(filter == CategoryFilter::Hidden),
                PanelRow::Section(_) => list_row,
            };
            row(
                key,
                list_row.trailing(key.count(state).to_string()),
                key.selected(state),
            )
        };
        let filters = section(false, filter_title(open)).children(
            rows.iter()
                .copied()
                .filter(|key| !matches!(key, PanelRow::Section(_)))
                .map(filter),
        );

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
