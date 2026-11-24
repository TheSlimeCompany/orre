//! The ranked bar list (`docs/DESIGN.md` → "Data visualisation" → "Ranked bar
//! list"; Paper `2M5-0`): one entry per category, a label line and below it a
//! 3px bar in the category's colour. The list a donut would otherwise be.
//!
//! Entries are 12px apart. The label line is the category's marker, its name
//! (12/16 `text-secondary`), its share (mono 11/14 `text-muted`, a 24px lane)
//! and its amount (mono 12/16 `text`, the 96px amount lane); the bar sits 6px
//! below. Bars have no track: the largest positive amount fills the row and
//! the rest scale to it. An amount of zero or less draws no bar, but keeps
//! the bar's space so the entries stay evenly spaced. "Other" and
//! "Uncategorised" repeat their markers in their bars ([`Marker`]). The
//! caller orders the entries.

use gpui_kit::base::TestSupportExt as _;
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::{
    App, InteractiveElement as _, IntoElement, ParentElement as _, RenderOnce, SharedString,
    Styled as _, Window, div, relative,
};

use crate::theme::tokens::{color, radius, size, spacing, typography};
use crate::ui::marker::Marker;
use crate::ui::text::TextRoleExt as _;

/// The share lane's width.
const SHARE_LANE: gpui_kit::Pixels = spacing::S6;

/// The marker for chart slot `slot`, 1 to 6 (`--color-chart-1` …), as
/// categories are ranked by `reporting::palette`. A slot outside 1 to 6 is
/// "Other".
pub fn slot_marker(slot: usize) -> Marker {
    slot.checked_sub(1)
        .and_then(|at| color::CHART.get(at))
        .map_or(Marker::Other, |&fill| Marker::Colour(fill))
}

/// One category in the list.
#[derive(Clone, Debug, PartialEq)]
pub struct RankedEntry {
    marker: Marker,
    label: SharedString,
    share: SharedString,
    amount: SharedString,
    minor: i64,
}

impl RankedEntry {
    /// An entry named `label` with its `marker`: [`slot_marker`] for a
    /// palette colour, [`Marker::Other`] for the rest, [`Marker::None`] for
    /// uncategorised. `share` ("25%") and `amount` ("£612.40") are the
    /// owner's text; `minor` is the amount in minor units, which sizes the
    /// bar.
    pub fn new(
        marker: Marker,
        label: impl Into<SharedString>,
        share: impl Into<SharedString>,
        amount: impl Into<SharedString>,
        minor: i64,
    ) -> Self {
        Self {
            marker,
            label: label.into(),
            share: share.into(),
            amount: amount.into(),
            minor,
        }
    }

    /// The id of the entry's bar, for tests and the dev driver.
    fn bar_id(&self) -> SharedString {
        format!("ranked-bar-{}", self.label).into()
    }
}

/// How much of the row each entry's bar fills, in order: `None` for an
/// amount of zero or less, otherwise its share of the largest amount.
/// Everything is `None` when no amount is positive.
pub fn bar_fractions(amounts: &[i64]) -> Vec<Option<f32>> {
    let largest = amounts.iter().copied().max().filter(|&top| top > 0);
    amounts
        .iter()
        .map(|&minor| match largest {
            Some(top) if minor > 0 => Some((minor as f64 / top as f64) as f32),
            _ => None,
        })
        .collect()
}

/// The ranked bar list, as wide as its parent.
#[derive(IntoElement)]
pub struct RankedBars {
    entries: Vec<RankedEntry>,
}

impl RankedBars {
    pub fn new(entries: Vec<RankedEntry>) -> Self {
        Self { entries }
    }
}

impl RenderOnce for RankedBars {
    fn render(self, _: &mut Window, _: &mut App) -> impl IntoElement {
        let amounts: Vec<i64> = self.entries.iter().map(|entry| entry.minor).collect();
        let fractions = bar_fractions(&amounts);
        div().flex().flex_col().gap(spacing::S3).children(
            self.entries
                .into_iter()
                .zip(fractions)
                .map(|(entry, fraction)| {
                    let uncategorised = entry.marker == Marker::None;
                    let label = div()
                        .flex()
                        .items_center()
                        .gap(spacing::S2)
                        .child(entry.marker.render())
                        .child(
                            div()
                                .flex_1()
                                .min_w_0()
                                .overflow_hidden()
                                .whitespace_nowrap()
                                .text_ellipsis()
                                .text_role(typography::BODY)
                                .text_color(if uncategorised {
                                    color::TEXT_MUTED
                                } else {
                                    color::TEXT_SECONDARY
                                })
                                .child(entry.label.clone()),
                        )
                        .child(
                            div()
                                .flex_none()
                                .w(SHARE_LANE)
                                .text_right()
                                .whitespace_nowrap()
                                .text_role(typography::MONO_VALUE)
                                .text_color(color::TEXT_MUTED)
                                .child(entry.share.clone()),
                        )
                        .child(
                            div()
                                .flex_none()
                                .w(size::COLUMN_AMOUNT)
                                .text_right()
                                .whitespace_nowrap()
                                .text_role(typography::MONO_AMOUNT)
                                .text_color(color::TEXT)
                                .child(entry.amount.clone()),
                        );
                    let track = div()
                        .flex_none()
                        .h(size::PROGRESS)
                        .mt(spacing::S1_5)
                        .when_some(fraction, |this, fraction| this.child(bar(&entry, fraction)));
                    div().flex().flex_col().child(label).child(track)
                }),
        )
    }
}

/// The bar for `entry`: `fraction` of the row, 3px, radius 2, in the marker's
/// treatment.
fn bar(entry: &RankedEntry, fraction: f32) -> impl IntoElement {
    let bar = div()
        .id(entry.bar_id())
        .test_support()
        .h_full()
        .w(relative(fraction))
        .rounded(radius::MARKER);
    match entry.marker {
        Marker::Colour(fill) | Marker::Split(fill, _) => bar.bg(fill),
        Marker::Other => bar
            .bg(color::FIELD)
            .border(size::HAIRLINE)
            .border_color(color::TEXT_MUTED),
        Marker::None => bar
            .border(size::HAIRLINE)
            .border_dashed()
            .border_color(color::TEXT_MUTED),
    }
}

#[cfg(test)]
mod tests {
    use gpui_kit::test::TestWindowExt as _;
    use gpui_kit::{Context, Render, TestAppContext, VisualTestContext, px};

    use super::*;

    #[test]
    fn the_largest_amount_fills_the_row_and_the_rest_scale_to_it() {
        assert_eq!(
            bar_fractions(&[61_240, 43_815, 30_027]),
            [Some(1.), Some(43_815. / 61_240.), Some(30_027. / 61_240.)]
        );
    }

    #[test]
    fn an_amount_of_zero_or_less_draws_no_bar() {
        assert_eq!(
            bar_fractions(&[1_000, 0, -500]),
            [Some(1.), None, None],
            "a refund-heavy group is listed but has no bar"
        );
        // The scale comes from the largest positive amount, wherever it is.
        assert_eq!(
            bar_fractions(&[-500, 200, 100]),
            [None, Some(1.), Some(0.5)]
        );
    }

    #[test]
    fn with_no_positive_amount_nothing_is_drawn() {
        assert_eq!(bar_fractions(&[]), []);
        assert_eq!(bar_fractions(&[0, 0]), [None, None]);
        assert_eq!(bar_fractions(&[-1]), [None]);
    }

    #[test]
    fn slots_one_to_six_are_the_palette() {
        assert_eq!(slot_marker(1), Marker::Colour(color::CHART_1));
        assert_eq!(slot_marker(6), Marker::Colour(color::CHART_6));
        assert_eq!(slot_marker(0), Marker::Other);
        assert_eq!(slot_marker(7), Marker::Other);
    }

    struct Harness {
        entries: Vec<RankedEntry>,
    }

    impl Render for Harness {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            div()
                .w(px(500.))
                .child(RankedBars::new(self.entries.clone()))
        }
    }

    fn open(cx: &mut TestAppContext, entries: Vec<RankedEntry>) -> &mut VisualTestContext {
        cx.update(|cx| {
            gpui_kit::init(cx);
            crate::theme::component::install(cx);
        });
        let (_, cx) = cx.add_window_view(move |_, _| Harness { entries });
        cx.run_until_parked();
        cx.update(|window, cx| window.draw(cx).clear(cx));
        cx
    }

    fn entry(marker: Marker, label: &str, minor: i64) -> RankedEntry {
        RankedEntry::new(marker, label, "10%", format!("£{}", minor / 100), minor)
    }

    #[gpui_kit::test]
    fn bars_are_3px_and_scale_to_the_largest(cx: &mut TestAppContext) {
        let cx = open(
            cx,
            vec![
                entry(slot_marker(1), "Groceries", 60_000),
                entry(slot_marker(2), "Eating out", 30_000),
                entry(Marker::Other, "Other", 15_000),
                entry(Marker::None, "Uncategorised", 6_000),
                entry(slot_marker(3), "Refunds", -100),
            ],
        );
        let width = |id: &'static str, cx: &mut VisualTestContext| {
            cx.update(|window, _| window.find(id).bounds())
        };
        let groceries = width("ranked-bar-Groceries", cx);
        assert_eq!(groceries.size.width, px(500.), "the largest fills the row");
        assert_eq!(groceries.size.height, size::PROGRESS);
        let eating = width("ranked-bar-Eating out", cx);
        assert!((eating.size.width - px(250.)).abs() < px(0.01));
        assert_eq!(eating.left(), groceries.left());
        assert!((width("ranked-bar-Other", cx).size.width - px(125.)).abs() < px(0.01));
        assert!((width("ranked-bar-Uncategorised", cx).size.width - px(50.)).abs() < px(0.01));
        assert!(
            cx.update(|window, _| window.try_find("ranked-bar-Refunds").is_none()),
            "no bar for an amount below zero"
        );
        // A label line (16), 6px, the bar (3), then the 12px between entries.
        let next = eating.top() - groceries.top();
        assert_eq!(next, px(16. + 6. + 3. + 12.));
    }

    #[gpui_kit::test]
    fn an_empty_list_renders(cx: &mut TestAppContext) {
        open(cx, Vec::new());
    }

    #[gpui_kit::test]
    fn all_zero_amounts_render_without_bars(cx: &mut TestAppContext) {
        let cx = open(
            cx,
            vec![
                entry(slot_marker(1), "Groceries", 0),
                entry(Marker::None, "Uncategorised", 0),
            ],
        );
        assert!(cx.update(|window, _| window.try_find("ranked-bar-Groceries").is_none()));
        assert!(cx.update(|window, _| window.try_find("ranked-bar-Uncategorised").is_none()));
    }
}
