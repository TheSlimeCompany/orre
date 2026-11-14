//! The Rules view's inspector (`docs/DESIGN.md` → "Rules"; Paper `B1J-0`,
//! `EFT-0`, `G2O-0`, `GZT-0`, `HFH-0`): 14px sections divided by hairlines.
//!
//! - **A rule**: the rule as title, what it decides as the figure; Never
//!   reached (the card naming the rule that catches its matches); When
//!   (Field, Test, Text, Account, Amount); Then (one row per action, "+ Add
//!   action"); Past transactions (Apply, and an agent's rule over this one
//!   from the Inbox); the footer (where it came from, Delete rule…). A new
//!   rule has Order (Position) instead of Past transactions and no footer.
//! - **A merchant name**: Match (Test, Text, Shows as), the bank text it
//!   catches, the footer (Added on, Delete name…).
//! - **A change**: what changed, row by row, and Undo this change….
//!
//! Composed, not drawn (`DESIGN.md` → "Rules" → "Built in M3.13"): the
//! Amount condition's second row, the Split action's lines, the Exclude
//! reason row, the Tag action's pills, New rule and New name from scratch.

use coffer_core::change::BatchStatus;
use coffer_core::rule::{ActionKind, RuleOrigin};
use gpui_kit::component::Disableable as _;
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::{
    AnyElement, Context, Entity, InteractiveElement as _, IntoElement, ParentElement as _,
    SharedString, StatefulInteractiveElement as _, Styled as _, Window, div,
};

use crate::theme::focus::{FocusRingExt as _, FocusRingPlacement};
use crate::theme::tokens::{color, font, radius, size, spacing, typography, weight};
use crate::ui::button::{Button, ButtonVariant, IconButton, IconButtonSize};
use crate::ui::field::{self as fields, FieldRow, SelectField, error_caption, text_input};
use crate::ui::icon::Glyph;
use crate::ui::inspector::{caption, section, section_title_row};
use crate::ui::marker::Marker;
use crate::ui::menu::{self, Select};
use crate::ui::segmented::Segmented;
use crate::ui::tag::{AddTagPill, TagPill};
use crate::ui::text::TextRoleExt as _;
use crate::ui::tracked::Tracked;

use super::form::{ActionForm, AmountChoice, Direction, FormError, NameError};
use super::text::{self, Part, rich};
use super::view::{Input, Menu, RulesView, amount_label};
use super::{NameSelection, Rules, Section};

/// The inspector for the open section.
pub fn render(
    view: &RulesView,
    entity: &Entity<Rules>,
    window: &mut Window,
    cx: &mut Context<RulesView>,
) -> AnyElement {
    let section_kind = entity.read(cx).section();
    let sections: Vec<AnyElement> = match section_kind {
        Section::Rules => rule_sections(view, entity, window, cx),
        Section::Names => name_sections(view, entity, window, cx),
        Section::History => change_sections(entity, window, cx),
    };
    div()
        .id("rules-inspector")
        .flex()
        .flex_col()
        .flex_none()
        .w(size::INSPECTOR)
        .h_full()
        .overflow_y_scroll()
        .bg(color::CHROME)
        .border_l(size::HAIRLINE)
        .border_color(color::HAIRLINE)
        .children(sections)
        .into_any_element()
}

/// The header: a title with mono parts, the figure and a caption.
fn header(
    window: &Window,
    title: Vec<Part>,
    figure: Option<(String, bool)>,
    caption_text: Option<AnyElement>,
) -> AnyElement {
    section(false)
        .gap(spacing::S1)
        .child(
            div()
                .flex()
                .min_w_0()
                .overflow_hidden()
                .whitespace_nowrap()
                .child(rich(window, typography::TITLE, color::TEXT, title)),
        )
        .when_some(figure, |this, (figure, faint)| {
            this.child(
                div()
                    .flex()
                    .text_color(if faint {
                        color::TEXT_FAINT
                    } else {
                        color::TEXT
                    })
                    .child(Tracked::new(typography::DISPLAY_FIGURE, figure)),
            )
        })
        .children(caption_text)
        .into_any_element()
}

fn caption_line(words: impl Into<SharedString>) -> AnyElement {
    caption(div().child(words.into())).into_any_element()
}

/// Nothing selected: one caption.
fn nothing(words: &'static str) -> Vec<AnyElement> {
    vec![
        section(false)
            .child(fields::caption(words))
            .into_any_element(),
    ]
}

/// A select field in a row, wired to the view's menus.
fn select(
    view: &RulesView,
    menu: Menu,
    label: &'static str,
    field: SelectField,
    cx: &mut Context<RulesView>,
) -> AnyElement {
    let groups = if view.menu == Some(menu) {
        view.visible_groups(cx)
    } else {
        view.menu_groups(menu, cx)
    };
    let (toggled, chosen, moved) = (
        cx.entity().downgrade(),
        cx.entity().downgrade(),
        cx.entity().downgrade(),
    );
    let mut select = Select::new(
        SharedString::from(format!("rules-select-{menu:?}")),
        label,
        field,
        groups,
        view.chosen_key(menu, cx),
        view.state(menu),
    )
    .on_toggle(move |open, window, cx| {
        toggled
            .update(cx, |this, cx| {
                if open != (this.menu == Some(menu)) {
                    if open {
                        this.open(menu, window, cx);
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
    });
    if matches!(menu, Menu::Category | Menu::SplitLine(_)) {
        select = select
            .filter(&view.menu_filter)
            .max_height(size::CONTROL * super::view::MENU_ROWS);
    }
    if menu == Menu::Position {
        select = select.max_height(size::CONTROL * super::view::MENU_ROWS);
    }
    select.into_any_element()
}

/// A text field, mono when it holds a pattern or a number.
fn field(view: &RulesView, input: Input, mono: bool) -> AnyElement {
    match view.input(input) {
        Some(state) => text_input(state)
            .w_full()
            .when(mono, |this| this.font_family(font::MONO))
            .into_any_element(),
        None => div().into_any_element(),
    }
}

/// The ghost "+ Add action" / "+ Add line" (24px, radius 6, 8px padding
/// pulled 8px left, a 12px plus, caption 500 `text-secondary`), as M3.9's
/// "Add line".
fn ghost(
    id: &'static str,
    label: &'static str,
    focus: &gpui_kit::FocusHandle,
    open: bool,
    window: &Window,
) -> gpui_kit::Stateful<gpui_kit::Div> {
    div()
        .id(id)
        .track_focus(focus)
        .flex()
        .flex_none()
        .items_center()
        .gap(spacing::S1_5)
        .h(size::CONTROL_SM)
        .px(spacing::S2)
        .ml(-spacing::S2)
        .rounded(radius::MD)
        .text_color(color::TEXT_SECONDARY)
        .when(open, |this| this.bg(color::CONTROL_ACTIVE))
        .hover(|style| style.bg(color::HOVER))
        .active(|style| style.bg(color::CONTROL_ACTIVE))
        .child(Glyph::Plus.render().text_color(color::TEXT_SECONDARY))
        .child(
            div()
                .whitespace_nowrap()
                .text_role(typography::CAPTION)
                .font_weight(weight::MEDIUM)
                .child(label),
        )
        .focus_visible_ring(focus, FocusRingPlacement::Outside, window)
}

// A rule -------------------------------------------------------------------------

fn rule_sections(
    view: &RulesView,
    entity: &Entity<Rules>,
    window: &mut Window,
    cx: &mut Context<RulesView>,
) -> Vec<AnyElement> {
    let rules = entity.read(cx);
    let Some(form) = rules.form().cloned() else {
        return nothing(
            "Select a rule to see what it does and edit it, or start one with New rule.",
        );
    };
    let data = rules.data().clone();
    let is_new = rules.is_new();
    let summary = rules.selected_rule().cloned();
    let apply = rules.apply().cloned();
    let error = rules.form_error();
    let touching = rules.touching().to_vec();
    let new_position = rules.new_position();
    let origin = rules.new_origin();
    let today = rules.today();
    let total = data.rules.len();

    // Header.
    let title = match form.definition(data.currency(form.account)) {
        Ok(rule) => vec![
            Part::text(format!("{} ", text::condition(&rule))),
            Part::mono(text::pattern(&rule)).weight(weight::MEDIUM),
        ],
        Err(_) if form.text.is_empty() => vec![Part::text("New rule")],
        Err(_) => vec![
            Part::text(format!(
                "{} {} ",
                text::field(form.field),
                text::test(form.kind)
            )),
            Part::mono(text::plain(&form.text, text::PATTERN_SHOWN)).weight(weight::MEDIUM),
        ],
    };
    let mut sections = Vec::new();
    if is_new {
        let rows = apply.as_ref().map_or(0, |apply| apply.rows.len());
        let lead = if origin == RuleOrigin::Edit {
            "earlier transactions match"
        } else {
            "transactions match"
        };
        sections.push(header(
            window,
            title,
            Some((rows.to_string(), false)),
            Some(caption_line(format!("{lead} · new rule, not saved yet"))),
        ));
    } else if let Some(summary) = &summary {
        let position = data.position(summary.rule.id()).map_or(0, |at| at + 1);
        let words = if summary.never_reached() {
            format!("transactions decided by this rule · rule {position} of {total}")
        } else {
            let last = summary
                .last
                .map(|date| format!(" · last on {}", text::date(date, today)))
                .unwrap_or_default();
            format!("transactions match{last} · rule {position} of {total}")
        };
        sections.push(header(
            window,
            title,
            Some((summary.decides.to_string(), summary.never_reached())),
            Some(caption_line(words)),
        ));
        if let Some(catch) = summary.caught_by
            && summary.never_reached()
            && let Some(above) = data.rule(catch.rule)
        {
            sections.push(never_reached(
                entity, &data, summary, above, catch.rows, window,
            ));
        }
    }

    sections.push(when_section(view, &form, &data, error.as_ref(), cx));
    sections.push(then_section(view, &form, &data, error.as_ref(), window, cx));

    if is_new {
        let overlaps = apply.as_ref().map_or(0, |apply| apply.overlaps.len());
        let position_label = match new_position {
            0 => "First".to_owned(),
            at if at >= total => "Last".to_owned(),
            at => format!("After rule {at}"),
        };
        let field = SelectField::new(
            "rules-position-field",
            format!(
                "{position_label} · rule {} of {}",
                new_position.min(total) + 1,
                total + 1
            ),
        );
        let overlap_words = match overlaps {
            0 => format!(
                "This one overlaps none of your {}.",
                text::count(total, "rule", "rules")
            ),
            1 => "It overlaps 1 of your rules: the first to match a field sets it.".to_owned(),
            n => format!("It overlaps {n} of your rules: the first to match a field sets it."),
        };
        sections.push(
            section(true)
                .gap(spacing::S2)
                .child(section_title_row("Order", None))
                .child(FieldRow::new(
                    "Position",
                    select(view, Menu::Position, "Position", field, cx),
                ))
                .child(fields::caption(format!(
                    "A new rule goes first, so it wins over broader rules. {overlap_words}"
                )))
                .into_any_element(),
        );
    } else if let Some(summary) = &summary {
        sections.push(past_section(entity, &touching, apply.is_some(), window));
        sections.push(rule_footer(entity, summary, &data, today));
    }
    sections
}

fn never_reached(
    entity: &Entity<Rules>,
    data: &super::store::RulesData,
    summary: &coffer_core::rule::RuleSummary,
    above: &coffer_core::rule::RuleSummary,
    rows: usize,
    window: &Window,
) -> AnyElement {
    let above_rule = above.rule.definition();
    let position = data.position(above.rule.id()).map_or(0, |at| at + 1);
    let all = if rows >= summary.matches {
        format!(
            "all {}",
            text::count(summary.matches, "transaction", "transactions")
        )
    } else {
        format!("{rows} of the {} transactions", summary.matches)
    };
    let (moved, shown) = (entity.clone(), entity.clone());
    let (id, above_id, target) = (summary.rule.id(), above.rule.id(), position - 1);
    section(true)
        .child(
            div()
                .flex()
                .flex_col()
                .gap(spacing::S2)
                .p(spacing::S3)
                .rounded(radius::SURFACE)
                .bg(color::FLOAT)
                .border(size::HAIRLINE)
                .border_color(color::HAIRLINE)
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap(spacing::S2)
                        .child(Glyph::Alert.render().text_color(color::WARNING))
                        .child(
                            div()
                                .text_role(typography::SECTION_TITLE)
                                .text_color(color::TEXT)
                                .child("Never reached"),
                        ),
                )
                .child(div().text_role(typography::BODY).child(rich(
                    window,
                    typography::BODY,
                    color::TEXT_STRONG,
                    vec![
                        Part::text(format!("Rule {position}, {} ", text::condition(above_rule))),
                        Part::mono(text::pattern(above_rule)),
                        Part::text(format!(
                            ", comes first and catches {all} this rule would match."
                        )),
                    ],
                )))
                .child(fields::caption(format!(
                    "Move this rule above rule {position} to let it decide them, or delete it."
                )))
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap(spacing::S1_5)
                        .pt(spacing::S1)
                        .child(
                            Button::new("rules-move-above", format!("Move above rule {position}"))
                                .with_variant(ButtonVariant::Outline)
                                .on_click(move |_, _, cx| {
                                    moved.update(cx, |rules, cx| rules.move_rule(id, target, cx))
                                }),
                        )
                        .child(
                            Button::new("rules-show-above", format!("Show rule {position}"))
                                .with_variant(ButtonVariant::Outline)
                                .on_click(move |_, _, cx| {
                                    shown.update(cx, |rules, cx| {
                                        rules.select_rule(Some(above_id), cx)
                                    })
                                }),
                        ),
                ),
        )
        .into_any_element()
}

fn when_section(
    view: &RulesView,
    form: &super::form::RuleForm,
    data: &super::store::RulesData,
    error: Option<&FormError>,
    cx: &mut Context<RulesView>,
) -> AnyElement {
    let account = form.account.and_then(|id| data.account(id)).map_or_else(
        || "Any account".to_owned(),
        |account| text::plain(&account.name, text::NAME_SHOWN),
    );
    let pattern_error = error.filter(|error| error.is_pattern() && !form.text.is_empty());
    let pattern_label = if form.kind == coffer_core::rule::MatchKind::Regex {
        "Pattern"
    } else {
        "Text"
    };
    let text_field = match view.input(Input::Pattern) {
        Some(state) => text_input(state)
            .w_full()
            .font_family(font::MONO)
            .when(pattern_error.is_some(), |this| {
                this.border(size::HAIRLINE)
                    .border_color(color::DANGER)
                    .suffix(Glyph::Alert.render().text_color(color::DANGER))
            })
            .into_any_element(),
        None => div().into_any_element(),
    };
    let mut section = section(true)
        .gap(spacing::S2)
        .child(section_title_row("When", None))
        .child(FieldRow::new(
            "Field",
            select(
                view,
                Menu::Field,
                "Field",
                SelectField::new("rules-field-field", text::field(form.field)),
                cx,
            ),
        ))
        .child(FieldRow::new(
            "Test",
            select(
                view,
                Menu::Test,
                "Test",
                SelectField::new("rules-test-field", text::test(form.kind)),
                cx,
            ),
        ))
        .child(FieldRow::new(pattern_label, text_field));
    if let Some(FormError::Pattern(problem)) = pattern_error {
        section = section.child(
            div()
                .pl(size::FIELD_LABEL + spacing::S2)
                .child(error_caption(text::pattern_error(problem))),
        );
    }
    section = section
        .child(FieldRow::new(
            "Account",
            select(
                view,
                Menu::Account,
                "Account",
                SelectField::new("rules-account-field", account),
                cx,
            ),
        ))
        .child(FieldRow::new(
            "Amount",
            select(
                view,
                Menu::Amount,
                "Amount",
                SelectField::new("rules-amount-field", amount_label(form.amount.choice)),
                cx,
            ),
        ));
    if matches!(
        form.amount.choice,
        AmountChoice::Over | AmountChoice::Under | AmountChoice::Between
    ) {
        let direction = Segmented::new(
            "rules-amount-direction",
            match form.amount.direction {
                Direction::Out => "out",
                Direction::In => "in",
            },
        )
        .segment("out", "Out")
        .segment("in", "In")
        .on_choose({
            let view = cx.entity().downgrade();
            move |key, _, cx| {
                let direction = if key.as_ref() == "in" {
                    Direction::In
                } else {
                    Direction::Out
                };
                let _ = view.update(cx, |this, cx| this.set_direction(direction, cx));
            }
        });
        let currency = data.currency(form.account);
        let symbol = crate::format::currency_prefix(currency);
        let amount = |input: Input| match view.input(input) {
            Some(state) => crate::ui::field::amount_input(state, symbol.clone(), cx)
                .flex_1()
                .min_w_0()
                .into_any_element(),
            None => div().into_any_element(),
        };
        let mut fields_row = div()
            .flex()
            .flex_1()
            .min_w_0()
            .items_center()
            .gap(spacing::S1_5)
            .child(direction)
            .child(amount(Input::Low));
        if form.amount.choice == AmountChoice::Between {
            fields_row = fields_row
                .child(
                    div()
                        .flex_none()
                        .text_role(typography::BODY)
                        .text_color(color::TEXT_MUTED)
                        .child("and"),
                )
                .child(amount(Input::High));
        }
        section = section.child(FieldRow::new("", fields_row));
    }
    if form.amount.choice == AmountChoice::Custom {
        section = section.child(fields::caption(
            "This rule has an amount range the choices above can’t say. Choosing one replaces \
             it.",
        ));
    }
    if let Some(FormError::Amount(problem)) = error {
        section = section.child(
            div()
                .pl(size::FIELD_LABEL + spacing::S2)
                .child(error_caption(problem.clone())),
        );
    }
    let caption_words = match form.kind {
        coffer_core::rule::MatchKind::Regex => {
            "Matches and the preview update as you type once the pattern is valid. Matching \
             always finishes quickly, whatever the pattern."
        }
        _ => "Matching ignores case. Merchant is the cleaned name, before any rename.",
    };
    section
        .child(fields::caption(caption_words))
        .into_any_element()
}

#[allow(clippy::too_many_arguments)]
fn then_section(
    view: &RulesView,
    form: &super::form::RuleForm,
    data: &super::store::RulesData,
    error: Option<&FormError>,
    window: &mut Window,
    cx: &mut Context<RulesView>,
) -> AnyElement {
    let mut section = section(true)
        .gap(spacing::S2)
        .child(section_title_row("Then", None));
    for action in &form.actions {
        let kind = action.kind();
        let control: AnyElement = match action {
            ActionForm::Category(value) => {
                let field = match value {
                    Some(key) => SelectField::new(
                        "rules-category-field",
                        data.category(key)
                            .map_or_else(|| key.as_str().to_owned(), |c| c.label.clone()),
                    )
                    .marker(super::table::marker(data, key)),
                    None => SelectField::new("rules-category-field", "Choose a category")
                        .marker(Marker::None)
                        .placeholder(true),
                };
                select(view, Menu::Category, "Category", field, cx)
            }
            ActionForm::Merchant(_) => field(view, Input::Merchant, false),
            ActionForm::Note(_) => field(view, Input::Note, false),
            ActionForm::Share(_) => match view.input(Input::Share) {
                Some(state) => text_input(state)
                    .w_full()
                    .font_family(font::MONO)
                    .suffix(
                        div()
                            .text_role(typography::MONO_AMOUNT)
                            .text_color(color::TEXT_FAINT)
                            .child("%"),
                    )
                    .into_any_element(),
                None => div().into_any_element(),
            },
            ActionForm::Exclude(reason) => {
                let field = match reason {
                    Some(reason) => SelectField::new("rules-exclude-field", text::reason(*reason)),
                    None => {
                        SelectField::new("rules-exclude-field", "Choose a reason").placeholder(true)
                    }
                };
                select(view, Menu::Exclude, "Reason", field, cx)
            }
            ActionForm::Tags(tags) => tag_control(view, tags, cx),
            ActionForm::Split(lines) => split_control(view, lines, data, window, cx),
        };
        let remove = IconButton::new(
            SharedString::from(format!("rules-remove-{}", kind.as_str())),
            Glyph::Reject,
            format!("Remove {}", text::action_label(kind)),
        )
        .with_size(IconButtonSize::Small)
        .on_click(cx.listener(move |this, _, _, cx| this.remove_action(kind, cx)));
        let tall = matches!(action, ActionForm::Split(_) | ActionForm::Tags(_));
        section = section.child(
            div()
                .flex()
                .flex_none()
                .gap(spacing::S2)
                .w_full()
                .map(|this| {
                    if tall {
                        this.items_start()
                    } else {
                        this.items_center().h(size::CONTROL)
                    }
                })
                .child(
                    div()
                        .flex()
                        .flex_none()
                        .items_center()
                        .h(size::CONTROL)
                        .w(size::FIELD_LABEL)
                        .whitespace_nowrap()
                        .text_role(typography::BODY)
                        .text_color(color::TEXT_MUTED)
                        .child(text::action_label(kind)),
                )
                .child(div().flex().flex_1().min_w_0().child(control))
                .child(
                    div()
                        .flex()
                        .flex_none()
                        .items_center()
                        .h(size::CONTROL)
                        .child(remove),
                ),
        );
    }
    let open = view.menu == Some(Menu::AddAction);
    let add = ghost(
        "rules-add-action",
        "Add action",
        &view.add_action_focus,
        open,
        window,
    )
    .on_click(cx.listener(RulesView::keyboard_open(Menu::AddAction)));
    let add = div()
        .relative()
        .flex_none()
        .child(view.popup_trigger(Menu::AddAction, add, cx))
        .when(open, |this| {
            this.child(menu::below(
                size::CONTROL_SM + spacing::S1,
                view.popup_list(Menu::AddAction, cx),
            ))
        });
    section = section.child(div().flex().child(add));
    match error {
        Some(FormError::Action { message, .. }) => {
            section = section.child(error_caption(message.clone()));
        }
        Some(FormError::NoActions) => {
            section = section.child(fields::caption(
                "Add what the rule does: a category, a name, a tag, a note, a share, a split or \
                 an exclusion.",
            ));
        }
        _ => {}
    }
    if form.actions.len() > 1 {
        section = section.child(fields::caption(
            "Each kind of action once per rule. A rule sets either Category or Split, not \
             both: a split sets a category on each line.",
        ));
    }
    section.into_any_element()
}

/// The Tag action: the tags as pills, then "+ Tag" opening the tag menu
/// (D6.3's tag row).
fn tag_control(view: &RulesView, tags: &[String], cx: &mut Context<RulesView>) -> AnyElement {
    let private = |name: &str| {
        view.rules()
            .read(cx)
            .data()
            .tags
            .iter()
            .any(|tag| tag.private && tag.name.eq_ignore_ascii_case(name))
    };
    let pills: Vec<AnyElement> = tags
        .iter()
        .enumerate()
        .map(|(index, name)| {
            let removed = name.clone();
            TagPill::new(
                SharedString::from(format!("rules-tag-{index}")),
                text::plain(name, text::NAME_SHOWN),
            )
            .private(private(name))
            .on_remove(cx.listener(move |this, _, _, cx| {
                let removed = removed.clone();
                this.rules().clone().update(cx, |rules, cx| {
                    rules.edit(
                        true,
                        |form| {
                            if let Some(ActionForm::Tags(tags)) =
                                form.action_mut(ActionKind::AddTag)
                            {
                                tags.retain(|tag| *tag != removed);
                            }
                        },
                        cx,
                    )
                });
            }))
            .into_any_element()
        })
        .collect();
    let open = view.menu == Some(Menu::Tags);
    let add = AddTagPill::new("rules-add-tag", &view.tag_focus)
        .open(open)
        .on_click(cx.listener(RulesView::keyboard_open(Menu::Tags)));
    let add = div()
        .relative()
        .flex_none()
        .child(view.popup_trigger(Menu::Tags, add, cx))
        .when(open, |this| {
            this.child(menu::below(
                size::CONTROL_SM + spacing::S1,
                view.popup_list(Menu::Tags, cx),
            ))
        });
    div()
        .flex()
        .flex_1()
        .min_w_0()
        .flex_wrap()
        .items_center()
        .gap(spacing::S1)
        .min_h(size::CONTROL)
        .py(spacing::S0_5)
        .children(pills)
        .child(add)
        .into_any_element()
}

/// The Split action: D6.3's lines without amounts, each a category and a
/// share in percent, "+ Add line" and the total.
fn split_control(
    view: &RulesView,
    lines: &[super::form::SplitLineForm],
    data: &super::store::RulesData,
    window: &Window,
    cx: &mut Context<RulesView>,
) -> AnyElement {
    let rows: Vec<AnyElement> = lines
        .iter()
        .enumerate()
        .map(|(index, line)| {
            let field = match &line.category {
                Some(key) => SelectField::new(
                    SharedString::from(format!("rules-split-category-{index}")),
                    data.category(key)
                        .map_or_else(|| key.as_str().to_owned(), |c| c.label.clone()),
                )
                .marker(super::table::marker(data, key)),
                None => SelectField::new(
                    SharedString::from(format!("rules-split-category-{index}")),
                    "Category",
                )
                .marker(Marker::None)
                .placeholder(true),
            };
            let percent = match view.input(Input::SplitPercent(index)) {
                Some(state) => text_input(state)
                    .flex_none()
                    .w(size::CONTROL_XL + spacing::S6)
                    .font_family(font::MONO)
                    .text_right()
                    .suffix(
                        div()
                            .text_role(typography::MONO_AMOUNT)
                            .text_color(color::TEXT_FAINT)
                            .child("%"),
                    )
                    .into_any_element(),
                None => div().into_any_element(),
            };
            div()
                .flex()
                .flex_none()
                .items_center()
                .gap(spacing::S1_5)
                .h(size::CONTROL)
                .child(div().flex().flex_1().min_w_0().child(select(
                    view,
                    Menu::SplitLine(index),
                    "Line category",
                    field,
                    cx,
                )))
                .child(percent)
                .when(lines.len() > 2, |this| {
                    this.child(
                        IconButton::new(
                            SharedString::from(format!("rules-split-remove-{index}")),
                            Glyph::Close,
                            format!("Remove line {}", index + 1),
                        )
                        .with_size(IconButtonSize::Small)
                        .on_click(cx.listener(
                            move |this, _, window, cx| this.remove_split_line(index, window, cx),
                        )),
                    )
                })
                .into_any_element()
        })
        .collect();
    let total: u64 = lines
        .iter()
        .filter_map(|line| super::form::parse_percent(&line.percent))
        .sum();
    let balanced = total == 10_000;
    let focus = view.add_action_focus.clone();
    let add_line = ghost("rules-split-add-line", "Add line", &focus, false, window)
        .on_click(cx.listener(|this, _, window, cx| this.add_split_line(window, cx)));
    div()
        .flex()
        .flex_col()
        .flex_1()
        .min_w_0()
        .gap(spacing::S1_5)
        .children(rows)
        .child(
            div()
                .flex()
                .items_center()
                .h(size::CONTROL_SM)
                .child(add_line)
                .child(div().flex_1())
                .child(
                    div()
                        .text_role(typography::MONO_VALUE)
                        .text_color(if balanced {
                            color::TEXT_MUTED
                        } else {
                            color::WARNING
                        })
                        .child(format!(
                            "{}%",
                            super::form::percent_text(
                                coffer_core::rule::Share::new(total.min(10_000))
                                    .unwrap_or(coffer_core::rule::Share::WHOLE)
                            )
                        )),
                ),
        )
        .into_any_element()
}

fn past_section(
    entity: &Entity<Rules>,
    touching: &[coffer_core::inbox::Suggestion],
    applying: bool,
    window: &Window,
) -> AnyElement {
    let apply = {
        let entity = entity.clone();
        Button::new("rules-apply", "Apply to past transactions…")
            .with_variant(ButtonVariant::Outline)
            .disabled(applying)
            .on_click(move |_, _, cx| entity.update(cx, |rules, cx| rules.apply_selected(cx)))
    };
    let mut section = section(true)
        .gap(spacing::S2)
        .child(section_title_row("Past transactions", None))
        .child(fields::caption(
            "Changes to this rule apply to new imports. To change past transactions too, apply \
             it: you see every change before anything is saved.",
        ))
        .child(
            div()
                .flex()
                .items_center()
                .gap(spacing::S1_5)
                .pt(spacing::S1)
                .child(apply),
        );
    if let Some(suggestion) = touching.first() {
        let pattern = match &suggestion.payload {
            coffer_core::inbox::Payload::Rule(rule) => text::pattern(rule),
            _ => String::new(),
        };
        let (client, _) = crate::inbox::text::source_name(&suggestion.source_client);
        let id = suggestion.id;
        let review = entity.clone();
        section = section.child(
            div()
                .flex()
                .items_start()
                .gap(spacing::S1_5)
                .pt(spacing::S1)
                .child(
                    div()
                        .flex()
                        .flex_none()
                        .items_center()
                        .h(crate::theme::tokens::leading::CAPTION)
                        .child(
                            Glyph::Sparkle
                                .render()
                                .size(size::ICON_XS)
                                .text_color(color::MOLDAVITE),
                        ),
                )
                .child(
                    div()
                        .id("rules-review-in-inbox")
                        .flex_1()
                        .min_w_0()
                        .text_role(typography::CAPTION)
                        .cursor_pointer()
                        .on_click(move |_, _, cx| {
                            review.update(cx, |rules, cx| rules.review_suggestion(id, cx))
                        })
                        .child(rich(
                            window,
                            typography::CAPTION,
                            color::MOLDAVITE_TEXT_MUTED,
                            vec![
                                Part::text(format!("{client} suggests a rule for ")),
                                Part::mono(pattern).color(color::MOLDAVITE_TEXT),
                                Part::text(" above this one. "),
                                Part::text("Review in the Inbox").color(color::MOLDAVITE_TEXT),
                            ],
                        )),
                ),
        );
    }
    section.into_any_element()
}

fn rule_footer(
    entity: &Entity<Rules>,
    summary: &coffer_core::rule::RuleSummary,
    data: &super::store::RulesData,
    today: chrono::NaiveDate,
) -> AnyElement {
    let lead = match summary.rule.origin() {
        Some(RuleOrigin::Edit) => "Created from an edit on ",
        Some(RuleOrigin::Inbox) => "Created from a suggestion on ",
        _ => "Created on ",
    };
    let created = data
        .rule_created
        .get(&summary.rule.id())
        .map(|date| text::date(*date, today));
    let delete = {
        let entity = entity.clone();
        Button::new("rules-delete", "Delete rule…")
            .with_variant(ButtonVariant::DestructiveCompact)
            .on_click(move |_, _, cx| entity.update(cx, |rules, cx| rules.request_delete_rule(cx)))
    };
    footer(lead, created, delete.into_any_element())
}

/// The footer: "Created on 12 Mar" (the date in mono) and a destructive
/// action.
fn footer(lead: &'static str, date: Option<String>, action: AnyElement) -> AnyElement {
    section(true)
        .flex_row()
        .items_center()
        .justify_between()
        .gap(spacing::S2)
        .child(
            div()
                .flex()
                .items_center()
                .text_role(typography::CAPTION)
                .text_color(color::TEXT_MUTED)
                .when_some(date, |this, date| {
                    this.child(lead)
                        .child(div().font_family(font::MONO).child(date))
                }),
        )
        .child(action)
        .into_any_element()
}

// A merchant name ----------------------------------------------------------------

fn name_sections(
    view: &RulesView,
    entity: &Entity<Rules>,
    window: &mut Window,
    cx: &mut Context<RulesView>,
) -> Vec<AnyElement> {
    let rules = entity.read(cx);
    let (Some(selection), Some(form)) = (rules.selected_name(), rules.name_form().cloned()) else {
        return nothing("Select a name to edit it, or start one with New name.");
    };
    let data = rules.data().clone();
    let today = rules.today();
    let error = rules.name_error();
    let summary = match selection {
        NameSelection::Stored(id) => data.name(id).cloned(),
        NameSelection::New => None,
    };
    let mut sections = Vec::new();
    let title = if form.name.trim().is_empty() {
        "New name".to_owned()
    } else {
        text::plain(&form.name, text::NAME_SHOWN)
    };
    sections.push(match &summary {
        Some(summary) => {
            let last = summary
                .last
                .map(|date| format!(" · last on {}", text::date(date, today)))
                .unwrap_or_default();
            header(
                window,
                vec![Part::text(title)],
                Some((summary.matches.to_string(), false)),
                Some(caption_line(format!("transactions show this name{last}"))),
            )
        }
        None => header(
            window,
            vec![Part::text(title)],
            None,
            Some(caption_line(
                "new name, saved once both fields are filled in",
            )),
        ),
    });
    let pattern_error = match &error {
        Some(NameError::Pattern(Some(problem))) if !form.pattern.is_empty() => {
            Some(problem.clone())
        }
        _ => None,
    };
    let text_field = match view.input(Input::NamePattern) {
        Some(state) => text_input(state)
            .w_full()
            .font_family(font::MONO)
            .when(pattern_error.is_some(), |this| {
                this.border(size::HAIRLINE)
                    .border_color(color::DANGER)
                    .suffix(Glyph::Alert.render().text_color(color::DANGER))
            })
            .into_any_element(),
        None => div().into_any_element(),
    };
    let mut matching = section(true)
        .gap(spacing::S2)
        .child(section_title_row("Match", None))
        .child(FieldRow::new(
            "Test",
            select(
                view,
                Menu::NameTest,
                "Test",
                SelectField::new("rules-name-test-field", text::alias_test(form.kind)),
                cx,
            ),
        ))
        .child(FieldRow::new("Text", text_field));
    if let Some(problem) = pattern_error {
        matching = matching.child(
            div()
                .pl(size::FIELD_LABEL + spacing::S2)
                .child(error_caption(text::pattern_error(&problem))),
        );
    }
    matching = matching
        .child(FieldRow::new(
            "Shows as",
            field(view, Input::NameShows, false),
        ))
        .child(fields::caption(
            "Matches the bank’s merchant text after clean-up, ignoring case. A name you typed on \
             a transaction isn’t replaced.",
        ));
    sections.push(matching.into_any_element());

    if let Some(summary) = &summary {
        const SHOWN: usize = 3;
        let caught: Vec<AnyElement> = summary
            .catches
            .iter()
            .take(SHOWN)
            .map(|caught| {
                div()
                    .flex()
                    .flex_none()
                    .items_center()
                    .gap(spacing::S2)
                    .h(size::CONTROL_SM)
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .overflow_hidden()
                            .whitespace_nowrap()
                            .text_ellipsis()
                            .text_role(typography::MONO_VALUE)
                            .text_color(color::TEXT_SECONDARY)
                            .child(text::plain(&caught.text, text::NAME_SHOWN)),
                    )
                    .child(
                        div()
                            .flex_none()
                            .text_role(typography::MONO_VALUE)
                            .text_color(color::TEXT_MUTED)
                            .child(text::date(caught.last, today)),
                    )
                    .into_any_element()
            })
            .collect();
        let more = summary.catches.len().saturating_sub(SHOWN);
        let mut catches = section(true)
            .gap(spacing::S2)
            .child(section_title_row("Bank text it catches", None));
        if caught.is_empty() {
            catches = catches.child(fields::caption("No transaction shows this name yet."));
        } else {
            catches = catches.child(div().flex().flex_col().gap(spacing::S1).children(caught));
        }
        if more > 0 {
            catches = catches.child(
                div()
                    .pt(spacing::S1)
                    .text_role(typography::CAPTION)
                    .text_color(color::TEXT_MUTED)
                    .child(format!("and {more} more")),
            );
        }
        sections.push(catches.into_any_element());
        let added = data
            .name_added
            .get(&summary.alias.id())
            .map(|date| text::date(*date, today));
        let delete = {
            let entity = entity.clone();
            Button::new("rules-delete-name", "Delete name…")
                .with_variant(ButtonVariant::DestructiveCompact)
                .on_click(move |_, _, cx| {
                    entity.update(cx, |rules, cx| rules.request_delete_name(cx))
                })
        };
        sections.push(footer("Added on ", added, delete.into_any_element()));
    }
    sections
}

// A change -----------------------------------------------------------------------

fn change_sections(
    entity: &Entity<Rules>,
    window: &mut Window,
    cx: &mut Context<RulesView>,
) -> Vec<AnyElement> {
    let rules = entity.read(cx);
    let Some(entry) = rules.selected_batch() else {
        return nothing("Select a change to see what it did and undo it.");
    };
    let data = rules.data();
    let today = rules.today();
    let undone = entry.batch.status == BatchStatus::Undone;
    let (made, undone_at) = data
        .history_at
        .get(&entry.batch.id)
        .copied()
        .map_or((None, None), |(made, undone)| (Some(made), undone));
    let title = text::change_parts(entry, data);
    let caption_words = match made {
        Some(at) => format!("transactions changed · {}", text::when_at(at, today)),
        None => "transactions changed".to_owned(),
    };
    let mut sections = vec![header(
        window,
        title,
        Some((entry.batch.transactions.to_string(), false)),
        Some(caption_line(caption_words)),
    )];
    const SHOWN: usize = 12;
    let currency = data.currency(None);
    let items: Vec<AnyElement> = rules
        .batch_items()
        .iter()
        .take(SHOWN)
        .map(|item| {
            div()
                .flex()
                .flex_none()
                .items_center()
                .gap(spacing::S2)
                .h(size::CONTROL_SM)
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .overflow_hidden()
                        .whitespace_nowrap()
                        .text_ellipsis()
                        .text_role(typography::BODY)
                        .text_color(color::TEXT_SECONDARY)
                        .child(text::plain(&item.name, text::NAME_SHOWN)),
                )
                .child(
                    div()
                        .flex_none()
                        .whitespace_nowrap()
                        .text_role(typography::CAPTION)
                        .child(rich(
                            window,
                            typography::CAPTION,
                            color::TEXT_MUTED,
                            vec![
                                Part::text(format!(
                                    "{} → ",
                                    text::shown(&item.old, data, currency)
                                )),
                                Part::text(text::shown(&item.new, data, currency))
                                    .color(color::TEXT),
                            ],
                        )),
                )
                .into_any_element()
        })
        .collect();
    let more = rules.batch_items().len().saturating_sub(SHOWN);
    let mut changed = section(true)
        .gap(spacing::S2)
        .child(section_title_row("What changed", None))
        .child(div().flex().flex_col().gap(spacing::S1).children(items));
    if more > 0 {
        changed = changed.child(
            div()
                .text_role(typography::CAPTION)
                .text_color(color::TEXT_MUTED)
                .child(format!("and {more} more")),
        );
    }
    if undone {
        let when = undone_at.map_or(String::new(), |at| {
            format!(" on {}", text::when_at(at, today))
        });
        changed = changed.child(fields::caption(format!(
            "Undone{when}. Each value went back where it still held this change’s."
        )));
    } else {
        let rule_words = if entry.batch.kind == coffer_core::change::ChangeKind::RuleApply {
            " The rule itself stays."
        } else {
            ""
        };
        let undo = {
            let entity = entity.clone();
            Button::new("rules-undo-change", "Undo this change…")
                .with_variant(ButtonVariant::Outline)
                .on_click(move |_, _, cx| entity.update(cx, |rules, cx| rules.request_undo(cx)))
        };
        changed = changed
            .child(fields::caption(format!(
                "Undo puts each one back to the value it had before. A transaction you have \
                 edited since keeps your edit.{rule_words}"
            )))
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(spacing::S1_5)
                    .pt(spacing::S1)
                    .child(undo),
            );
    }
    sections.push(changed.into_any_element());
    sections
}
