//! The Rules view's left panel (`docs/DESIGN.md` → "Rules"; Paper `B1J-0`,
//! `G2O-0`, `GZT-0`): RULES (Rules, Merchant names, Change history, each
//! with its count), then the open list's filter: ACTION for rules, MATCH
//! for merchant names, KIND for the history. A second click on a filter
//! clears it. Zero counts stay listed, in mono `text-faint`.

use gpui_kit::{
    App, Entity, InteractiveElement as _, IntoElement, ParentElement as _, SharedString,
    StatefulInteractiveElement as _, Styled as _, div,
};

use crate::theme::tokens::{color, size, spacing, typography};
use crate::ui::icon::Glyph;
use crate::ui::list_row::{ListRow, TrailingTone};
use crate::ui::section_label::SectionLabelRow;
use crate::ui::text::TextRoleExt as _;

use super::text;
use super::{Rules, Section};

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

/// The panel.
pub fn render(rules: &Entity<Rules>, cx: &App) -> impl IntoElement {
    let state = rules.read(cx);
    let data = state.data();
    let open = state.section();
    let list = |section: Section, id: &'static str, label: &'static str, glyph, count: usize| {
        let handle = rules.clone();
        ListRow::new(id, label)
            .icon(glyph)
            .trailing(count.to_string())
            .trailing_tone(TrailingTone::Faint)
            .selected(open == section)
            .on_click(move |_, _, cx| handle.update(cx, |rules, cx| rules.show(section, cx)))
    };
    let lists = [
        list(
            Section::Rules,
            "rules-section-rules",
            "Rules",
            Glyph::Arrow,
            data.rules.len(),
        ),
        list(
            Section::Names,
            "rules-section-names",
            "Merchant names",
            Glyph::NameTag,
            data.names.len(),
        ),
        list(
            Section::History,
            "rules-section-history",
            "Change history",
            Glyph::History,
            data.history.len(),
        ),
    ];

    let filters = match open {
        Section::Rules => section(false, "Action").children(text::PANEL_ACTIONS.map(|kind| {
            let handle = rules.clone();
            ListRow::new(
                SharedString::from(format!("rules-action-{}", kind.as_str())),
                text::action_filter_label(kind),
            )
            .icon(text::action_glyph(kind))
            .trailing(state.count_with_action(kind).to_string())
            .trailing_tone(TrailingTone::Faint)
            .selected(state.action_filter() == Some(kind))
            .on_click(move |_, _, cx| {
                handle.update(cx, |rules, cx| rules.toggle_action_filter(kind, cx))
            })
        })),
        Section::Names => section(false, "Match").children(text::ALIAS_KINDS.map(|kind| {
            let handle = rules.clone();
            let count = data
                .names
                .iter()
                .filter(|name| name.alias.definition().kind() == kind)
                .count();
            ListRow::new(
                SharedString::from(format!("rules-match-{}", kind.as_str())),
                text::alias_kind(kind),
            )
            .icon(text::alias_glyph(kind))
            .trailing(count.to_string())
            .trailing_tone(TrailingTone::Faint)
            .selected(state.match_filter() == Some(kind))
            .on_click(move |_, _, cx| {
                handle.update(cx, |rules, cx| rules.toggle_match_filter(kind, cx))
            })
        })),
        Section::History => section(false, "Kind").children(text::CHANGE_KINDS.map(|kind| {
            let handle = rules.clone();
            let count = data
                .history
                .iter()
                .filter(|entry| entry.batch.kind == kind)
                .count();
            ListRow::new(
                SharedString::from(format!("rules-kind-{}", kind.as_str())),
                text::change_kind(kind),
            )
            .icon(text::change_glyph(kind))
            .trailing(count.to_string())
            .trailing_tone(TrailingTone::Faint)
            .selected(state.kind_filter() == Some(kind))
            .on_click(move |_, _, cx| {
                handle.update(cx, |rules, cx| rules.toggle_kind_filter(kind, cx))
            })
        })),
    };

    div()
        .id("rules-panel")
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
        .children(state.load_error().cloned().map(|error| {
            div()
                .px(spacing::S3_5)
                .text_role(typography::CAPTION)
                .text_color(color::DANGER)
                .child(error)
        }))
}
