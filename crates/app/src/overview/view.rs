//! Drawing the Overview (`docs/DESIGN.md` → "Overview", Paper `22V-0`): the
//! month switcher, the figures with the compact agent card, the income and
//! spending chart, and the lower row of spending by category and recurring
//! payments. What it says is [`super::text::screen_text`]'s; this lays it
//! out.
//!
//! The canvas takes the whole width (nothing on Overview selects a record,
//! so there is no inspector) with 16px above, 24px beside and below, and
//! 24px between sections; it scrolls when the window is short.
//!
//! Element ids tests and the dev driver use: `overview-previous-month` and
//! `overview-next-month` (the switcher), `overview-figure-spent`,
//! `overview-figure-income`, `overview-figure-net`, the chart's
//! `chart-month-{index}` bands, `overview-categories`, `overview-recurring`
//! and its `overview-recurring-row-{index}` rows, `overview-review` (the agent card) and
//! `overview-canvas` (the scrolling canvas).

use gpui_kit::base::TestSupportExt as _;
use gpui_kit::component::Disableable as _;
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::{
    AnyElement, Context, Div, InteractiveElement as _, IntoElement, ParentElement as _, Render,
    SharedString, StatefulInteractiveElement as _, Styled as _, Window, div,
};
use orre_core::money::Currency;
use orre_core::reporting::Period;

use super::OverviewView;
use super::text::{
    AgentCardText, CategoryText, FigureText, RecurringText, ScreenText, agent_card_text,
    screen_text,
};
use crate::theme::tokens::typography::TextRole;
use crate::theme::tokens::{color, size, spacing, typography, weight};
use crate::ui::agent_card::CompactAgentCard;
use crate::ui::button::{Button, ButtonVariant, IconButton};
use crate::ui::empty_state::EmptyState;
use crate::ui::icon::Glyph;
use crate::ui::income_chart::{ChartMonth, IncomeChart, legend};
use crate::ui::ranked_bars::{RankedBars, RankedEntry};
use crate::ui::text::TextRoleExt as _;

/// The figures' labels ("Spent"): caption type at medium weight.
const FIGURE_LABEL: TextRole = TextRole {
    weight: weight::MEDIUM,
    ..typography::CAPTION
};

/// What a section with nothing in it says, in `text-muted`.
const NOTHING_SPENT: &str = "No spending in";
const NOTHING_DUE: &str = "Nothing due in the next 30 days";

/// The cadence lane of a recurring row.
const CADENCE_LANE: gpui_kit::Pixels = size::COLUMN_AMOUNT;
/// The date lane of a recurring row.
const DATE_LANE: gpui_kit::Pixels = gpui_kit::px(48.);

impl Render for OverviewView {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let canvas = div()
            .flex()
            .flex_col()
            .flex_1()
            .min_w_0()
            .min_h_0()
            .bg(color::CANVAS);
        if let Some(error) = &self.load_error {
            return canvas.child(centred(
                EmptyState::new("Couldn't read the overview").body(error.clone()),
            ));
        }
        let Some(data) = &self.data else {
            return canvas;
        };
        if !data.has_transactions {
            return canvas.child(centred(
                EmptyState::new("No transactions yet").shortcut("⌘I", "Import a statement"),
            ));
        }
        // The month the data is for: while a read for another month is in
        // flight, the title and captions stay with the figures they head.
        let text = screen_text(data, data.selected.period, self.today);
        let currency = data.currency;
        let agent = self
            .inbox
            .as_ref()
            .and_then(|inbox| agent_card_text(inbox.read(cx)));
        let content = div()
            .flex()
            .flex_col()
            .gap(spacing::S6)
            .pt(spacing::S4)
            .px(spacing::S6)
            .pb(spacing::S6)
            .child(self.render_switcher(&text, cx))
            .child(figures(&text, agent, cx))
            .child(self.render_chart(&text, currency, cx))
            .child(
                div()
                    .flex()
                    .gap(spacing::S6 * 2.)
                    .child(categories(&text))
                    .child(recurring(&text)),
            );
        canvas.child(
            div()
                .id("overview-canvas")
                .test_support()
                .flex_1()
                .min_h_0()
                .overflow_y_scroll()
                .child(content),
        )
    }
}

/// `content` centred in the canvas.
fn centred(content: impl IntoElement) -> Div {
    div()
        .flex()
        .flex_1()
        .items_center()
        .justify_center()
        .child(content)
}

impl OverviewView {
    /// The month switcher: chevron buttons round the month in title type,
    /// pulled 8px left so the first glyph lines up with the content edge.
    fn render_switcher(&self, text: &ScreenText, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .flex()
            .items_center()
            .gap(spacing::S1)
            .ml(-spacing::S2)
            .child(
                IconButton::new("overview-previous-month", Glyph::Back, "Previous month")
                    .disabled(self.previous_month().is_none())
                    .on_click(cx.listener(|this, _, _, cx| this.show_previous(cx))),
            )
            .child(
                div()
                    .whitespace_nowrap()
                    .text_role(typography::TITLE)
                    .text_color(color::TEXT)
                    .child(SharedString::from(text.title.clone())),
            )
            .child(
                IconButton::new("overview-next-month", Glyph::Forward, "Next month")
                    .disabled(self.next_month().is_none())
                    .on_click(cx.listener(|this, _, _, cx| this.show_next(cx))),
            )
    }

    /// "Income and spending": title, caption and legend, then the chart.
    fn render_chart(
        &self,
        text: &ScreenText,
        currency: Currency,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let months: Vec<ChartMonth> = text
            .months
            .iter()
            .map(|month| {
                let chart_month = ChartMonth::new(month.label.clone(), month.title.clone())
                    .spending(month.spending.0, month.spending.1.clone())
                    .income(month.income.0, month.income.1.clone())
                    .net(month.net.0, month.net.1.clone());
                if month.before_data {
                    chart_month.before_data()
                } else {
                    chart_month
                }
            })
            .collect();
        let periods: Vec<Period> = self
            .data
            .iter()
            .flat_map(|data| data.months.iter().map(|figures| figures.period))
            .collect();
        let this = cx.entity().downgrade();
        let chart = IncomeChart::new("overview-chart", currency, months)
            .selected(text.selected_month)
            .on_select(move |index, _, cx| {
                let Some(&period) = periods.get(index) else {
                    return;
                };
                this.update(cx, |this, cx| this.select_month(period, cx))
                    .ok();
            });
        div()
            .flex()
            .flex_col()
            .gap(spacing::S3)
            .child(
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .child(section_heading("Income and spending", &text.chart_caption))
                    .child(legend()),
            )
            .child(chart)
    }
}

/// Spent, Income and Net, 48px apart and top-aligned, the other currencies'
/// note under them, and the compact agent card at the right end, centred on
/// the row.
fn figures(
    text: &ScreenText,
    agent: Option<AgentCardText>,
    cx: &mut Context<OverviewView>,
) -> impl IntoElement {
    let [spent, income, net] = &text.figures;
    let row = div()
        .flex()
        .items_start()
        .gap(spacing::S6 * 2.)
        .child(figure(
            "overview-figure-spent",
            spent,
            typography::DISPLAY_FIGURE,
        ))
        .child(figure("overview-figure-income", income, typography::FIGURE))
        .child(figure("overview-figure-net", net, typography::FIGURE));
    div()
        .flex()
        .flex_col()
        .gap(spacing::S2)
        .child(
            div()
                .flex()
                .items_center()
                .justify_between()
                .child(row)
                .children(agent.map(|agent| agent_card(agent, cx))),
        )
        .children(text.other_currencies.clone().map(|note| {
            div()
                .text_role(typography::CAPTION)
                .text_color(color::TEXT_MUTED)
                .child(SharedString::from(note))
        }))
}

fn figure(id: &'static str, figure: &FigureText, role: TextRole) -> impl IntoElement {
    div()
        .id(id)
        .test_support()
        .flex()
        .flex_col()
        .gap(spacing::S1)
        .child(
            div()
                .text_role(FIGURE_LABEL)
                .text_color(color::TEXT_MUTED)
                .child(figure.label),
        )
        .child(
            div()
                .whitespace_nowrap()
                .text_role(role)
                .text_color(color::TEXT)
                .child(SharedString::from(figure.value.clone())),
        )
        .child(
            div()
                .whitespace_nowrap()
                .text_role(typography::CAPTION)
                .text_color(color::TEXT_MUTED)
                .child(SharedString::from(figure.caption.clone())),
        )
}

fn agent_card(agent: AgentCardText, cx: &mut Context<OverviewView>) -> impl IntoElement {
    CompactAgentCard::new(agent.agent)
        .pending(agent.pending)
        .summary(agent.summary.into_iter().map(SharedString::from))
        .action(
            Button::new("overview-review", "Review")
                .with_variant(ButtonVariant::AgentPrimary)
                .with_surface(color::MOLDAVITE_SURFACE)
                .on_click(cx.listener(|this, _, _, cx| this.request_review(cx))),
        )
}

/// A section's title (12/16 600) and its caption (11/14 `text-muted`).
fn section_heading(title: &str, caption: &str) -> impl IntoElement {
    div()
        .flex()
        .items_baseline()
        .gap(spacing::S2)
        .child(
            div()
                .text_role(typography::SECTION_TITLE)
                .text_color(color::TEXT)
                .child(SharedString::from(title.to_owned())),
        )
        .child(
            div()
                .text_role(typography::CAPTION)
                .text_color(color::TEXT_MUTED)
                .child(SharedString::from(caption.to_owned())),
        )
}

/// A line saying a section is empty.
fn nothing(words: String) -> impl IntoElement {
    div()
        .text_role(typography::CAPTION)
        .text_color(color::TEXT_MUTED)
        .child(SharedString::from(words))
}

/// "Spending by category · October" as a ranked bar list.
fn categories(text: &ScreenText) -> impl IntoElement {
    let body: AnyElement = if text.categories.is_empty() {
        nothing(format!("{NOTHING_SPENT} {}", text.category_caption)).into_any_element()
    } else {
        RankedBars::new(
            text.categories
                .iter()
                .map(|entry: &CategoryText| {
                    RankedEntry::new(
                        entry.marker,
                        entry.label.clone(),
                        entry.share.clone(),
                        entry.amount.clone(),
                        entry.minor,
                    )
                })
                .collect(),
        )
        .into_any_element()
    };
    div()
        .id("overview-categories")
        .test_support()
        .flex()
        .flex_col()
        .flex_1()
        .min_w_0()
        .gap(spacing::S3)
        .child(section_heading(
            "Spending by category",
            &text.category_caption,
        ))
        .child(body)
}

/// "Recurring · Next 30 days", the period total on the right, then a 28px
/// row for each payment, inset 8px like the table body.
fn recurring(text: &ScreenText) -> impl IntoElement {
    let body: AnyElement = if text.recurring.is_empty() {
        nothing(NOTHING_DUE.to_owned()).into_any_element()
    } else {
        div()
            .flex()
            .flex_col()
            .children(
                text.recurring
                    .iter()
                    .enumerate()
                    .map(|(at, row)| recurring_row(at, row)),
            )
            .into_any_element()
    };
    div()
        .id("overview-recurring")
        .test_support()
        .flex()
        .flex_col()
        .flex_1()
        .min_w_0()
        .gap(spacing::S3)
        .child(
            div()
                .flex()
                .items_baseline()
                .justify_between()
                .child(section_heading("Recurring", "Next 30 days"))
                .when(!text.recurring.is_empty(), |this| {
                    this.child(
                        div()
                            .whitespace_nowrap()
                            .text_role(typography::MONO_VALUE)
                            .text_color(color::TEXT_MUTED)
                            .child(SharedString::from(text.recurring_total.clone())),
                    )
                }),
        )
        .child(body)
}

fn recurring_row(at: usize, row: &RecurringText) -> impl IntoElement {
    div()
        .id(SharedString::from(format!("overview-recurring-row-{at}")))
        .test_support()
        .flex()
        .items_center()
        .h(size::CONTROL)
        .px(spacing::S2)
        .gap(spacing::S2)
        .child(row.marker.render())
        .child(
            div()
                .flex_1()
                .min_w_0()
                .overflow_hidden()
                .whitespace_nowrap()
                .text_ellipsis()
                .text_role(typography::BODY)
                .text_color(color::TEXT)
                .child(SharedString::from(row.merchant.clone())),
        )
        .child(
            div()
                .flex_none()
                .w(CADENCE_LANE)
                .whitespace_nowrap()
                .text_role(typography::BODY)
                .text_color(color::TEXT_MUTED)
                .child(row.cadence),
        )
        .child(
            div()
                .flex_none()
                .w(DATE_LANE)
                .text_right()
                .whitespace_nowrap()
                .text_role(typography::MONO_VALUE)
                .text_color(color::TEXT_MUTED)
                .child(SharedString::from(row.next.clone())),
        )
        .child(
            div()
                .flex_none()
                .w(size::COLUMN_AMOUNT)
                .text_right()
                .whitespace_nowrap()
                .text_role(typography::MONO_AMOUNT)
                .text_color(color::TEXT)
                .child(SharedString::from(row.amount.clone())),
        )
}
