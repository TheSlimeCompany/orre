//! The income and spending chart (`docs/DESIGN.md` → "Data visualisation" →
//! "Income vs spending (monthly overview)" and "Chart chrome"; Paper `2HP-0`):
//! a spending bar and an income bar for each month, the net as a line through
//! the month centres, a hover tooltip and the selected month's overlay.
//!
//! Custom, on `div`s and one painted path (decided in D4.1: the stock charts
//! can't draw grouped bars, a 1.5px haloed line or the selection). The owner
//! passes plain presentation data ([`ChartMonth`]) and keeps the selection;
//! the chart reports a click on a month through
//! [`on_select`](IncomeChart::on_select). Its arithmetic is in
//! [`chart`](super::chart).
//!
//! Each month's band is a hit area with the element id `chart-month-{index}`
//! (tests and the dev driver click it). While a month is hovered the chart
//! shows the net point (`chart-net-point`) and the tooltip (`chart-tooltip`); the selected month
//! shows its overlay (`chart-selection`), at least a bar's width tall so an
//! empty month's outline isn't flattened onto the axis. Months before any
//! data ([`ChartMonth::before_data`]) have no net point: the line starts at
//! the first month with data. Hover is the chart's own keyed
//! state; the selection is the owner's.
//!
//! Not built: the `motion-chart` growth and value animations, and round line
//! joins (GPUI's stroke options need `lyon`'s join type, which isn't
//! re-exported): the net line joins with GPUI's default miter.

use gpui_kit::base::TestSupportExt as _;
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::{
    App, Div, ElementId, InteractiveElement as _, IntoElement, ParentElement as _, PathBuilder,
    RenderOnce, Rgba, SharedString, StatefulInteractiveElement as _, Styled as _, Window, canvas,
    div, point, px, relative,
};
use orre_core::money::Currency;

use crate::theme::tokens::typography::TextRole;
use crate::theme::tokens::{color, radius, shadow, size, spacing, typography, weight};
use crate::ui::chart::{
    Axis, BarExtent, PLOT_HEIGHT, Side, compact_label, label_edge, tooltip_side,
};
use crate::ui::text::TextRoleExt as _;

/// A bar's width: `spacing-4`.
const BAR_WIDTH: f32 = 16.;
/// The gap between a month's two bars: `spacing-0_5`.
const BAR_GAP: f32 = 2.;
/// The pair's width.
const PAIR_WIDTH: f32 = BAR_WIDTH * 2. + BAR_GAP;
/// The y labels' column: 32px, 8px from the plot.
const LABEL_COLUMN: f32 = 32.;
/// The selection outline sits this far outside the bars.
const OUTLINE_GAP: f32 = 2.;
/// The selection label sits this far above the outline.
const LABEL_RISE: f32 = 12.;
/// The least height the selection frames, from the zero line up: a bar's
/// width, so a month with no bars (or tiny ones) gets a square to frame
/// rather than an outline flattened onto the axis, its handles overlapping.
const OUTLINE_MIN_HEIGHT: f32 = BAR_WIDTH;
/// The line's halo: 4.5px, so 1.5px of `canvas` shows each side of the line.
const HALO: f32 = 4.5;
/// The tooltip's distance from the month's centre.
const TOOLTIP_OFFSET: f32 = 24.;
/// The tooltip's width.
const TOOLTIP_WIDTH: f32 = 176.;

/// The selection label: 10/12 mono 600, white on `molten`.
const SELECTION_LABEL: TextRole = TextRole {
    weight: weight::STRONG,
    ..typography::MONO_MICRO
};

/// One value on the chart: minor units for the geometry, the text the owner
/// formatted for the tooltip and the selection label.
#[derive(Clone, Debug, PartialEq)]
pub struct ChartValue {
    minor: i64,
    text: SharedString,
}

impl ChartValue {
    pub fn new(minor: i64, text: impl Into<SharedString>) -> Self {
        Self {
            minor,
            text: text.into(),
        }
    }
}

/// One month on the chart.
#[derive(Clone, Debug, PartialEq)]
pub struct ChartMonth {
    label: SharedString,
    title: SharedString,
    spending: ChartValue,
    income: ChartValue,
    net: ChartValue,
    /// Whether the month is before any data ([`ChartMonth::before_data`]).
    before_data: bool,
}

impl ChartMonth {
    /// A month with its axis label ("Oct") and tooltip title ("October
    /// 2026"), no spending, income or net yet.
    pub fn new(label: impl Into<SharedString>, title: impl Into<SharedString>) -> Self {
        Self {
            label: label.into(),
            title: title.into(),
            spending: ChartValue::new(0, ""),
            income: ChartValue::new(0, ""),
            net: ChartValue::new(0, ""),
            before_data: false,
        }
    }

    /// The month's spending, as a positive amount.
    pub fn spending(mut self, minor: i64, text: impl Into<SharedString>) -> Self {
        self.spending = ChartValue::new(minor, text);
        self
    }

    /// The month's income, as a positive amount.
    pub fn income(mut self, minor: i64, text: impl Into<SharedString>) -> Self {
        self.income = ChartValue::new(minor, text);
        self
    }

    /// The month's net, signed: income less spending.
    pub fn net(mut self, minor: i64, text: impl Into<SharedString>) -> Self {
        self.net = ChartValue::new(minor, text);
        self
    }

    /// The month is before the first month with any data: the net line
    /// starts after it and hovering it shows no net point, so months with
    /// nothing imported don't read as months of breaking even. Only leading
    /// months are marked; the line joins the rest in order.
    pub fn before_data(mut self) -> Self {
        self.before_data = true;
        self
    }
}

type Select = Box<dyn Fn(usize, &mut Window, &mut App)>;

/// The chart: months left to right, in the order given.
#[derive(IntoElement)]
pub struct IncomeChart {
    id: ElementId,
    months: Vec<ChartMonth>,
    currency: Currency,
    selected: Option<usize>,
    on_select: Option<Select>,
}

impl IncomeChart {
    /// A chart of `months` with axis labels in `currency`. `id` keys the
    /// chart's hover state.
    pub fn new(id: impl Into<ElementId>, currency: Currency, months: Vec<ChartMonth>) -> Self {
        Self {
            id: id.into(),
            months,
            currency,
            selected: None,
            on_select: None,
        }
    }

    /// Shows the month at `index` as selected. An index past the last month
    /// shows none.
    pub fn selected(mut self, index: Option<usize>) -> Self {
        self.selected = index;
        self
    }

    /// Runs `handler` with the month's index when its band is clicked. The
    /// owner shows the selection by passing it back to
    /// [`selected`](Self::selected).
    pub fn on_select(mut self, handler: impl Fn(usize, &mut Window, &mut App) + 'static) -> Self {
        self.on_select = Some(Box::new(handler));
        self
    }
}

/// The legend that sits right of the chart title: 8×8 markers for the bars
/// and a 12×1.5px line for the net, 11/14 `text-secondary` labels. Spending
/// and income markers follow the bars: solid `text-strong`, and a 1px
/// `text-muted` outline on `chart-income-fill`.
pub fn legend() -> Div {
    let item = |swatch: Div, name: &'static str| {
        div()
            .flex()
            .flex_none()
            .items_center()
            .gap(spacing::S1_5)
            .child(swatch)
            .child(
                div()
                    .whitespace_nowrap()
                    .text_role(typography::CAPTION)
                    .text_color(color::TEXT_SECONDARY)
                    .child(name),
            )
    };
    div()
        .flex()
        .flex_none()
        .items_center()
        .gap(spacing::S3)
        .child(item(spending_swatch(), "Spending"))
        .child(item(income_swatch(), "Income"))
        .child(item(
            div()
                .flex_none()
                .w(spacing::S3)
                .h(size::STROKE)
                .bg(color::TEXT),
            "Net",
        ))
}

/// The id of month `index`'s hit area.
fn month_id(index: usize) -> SharedString {
    format!("chart-month-{index}").into()
}

/// An 8×8 series marker: a solid fill, or a 1px outline on a fill.
fn series_swatch(fill: Rgba, outline: Option<Rgba>) -> Div {
    let swatch = div()
        .flex_none()
        .size(size::MARKER)
        .rounded(radius::MARKER)
        .bg(fill);
    match outline {
        Some(outline) => swatch.border(size::HAIRLINE).border_color(outline),
        None => swatch,
    }
}

/// The spending marker: solid `text-strong`.
fn spending_swatch() -> Div {
    series_swatch(color::TEXT_STRONG, None)
}

/// The income marker: 1px `text-muted` outline on `chart-income-fill`.
fn income_swatch() -> Div {
    series_swatch(color::CHART_INCOME_FILL, Some(color::TEXT_MUTED))
}

/// A bar of `extent` at `left` in the pair: radius 2 on the value end only.
/// `outline` is a 1px border in that colour, used for income bars; the
/// treatment matches "Other" in the ranked bar list (`size::HAIRLINE` on the
/// same box).
fn bar(left: f32, extent: BarExtent, fill: Rgba, outline: Option<Rgba>) -> Div {
    let bar = div()
        .absolute()
        .left(px(left))
        .top(px(extent.top))
        .w(px(BAR_WIDTH))
        .h(px(extent.height))
        .bg(fill);
    let bar = match outline {
        Some(outline) => bar.border(size::HAIRLINE).border_color(outline),
        None => bar,
    };
    if extent.rises {
        bar.rounded_tl(radius::MARKER).rounded_tr(radius::MARKER)
    } else {
        bar.rounded_bl(radius::MARKER).rounded_br(radius::MARKER)
    }
}

/// A 1px gridline `top` from the plot's top.
fn line(top: f32, fill: Rgba) -> Div {
    div()
        .absolute()
        .left_0()
        .right_0()
        .top(px(top) - size::HAIRLINE / 2.)
        .h(size::HAIRLINE)
        .bg(fill)
}

/// How far across a plot of `count` months month `index`'s centre is.
fn centre(index: usize, count: usize) -> f32 {
    (index as f32 + 0.5) / count as f32
}

/// The tooltip (`DESIGN.md` → "Tooltip": `hover` fill, 1px `tooltip-border`,
/// radius `surface`, the tooltip shadow) with the month's title and one row
/// per series, values in mono.
fn tooltip(month: &ChartMonth) -> Div {
    let row = |swatch: Div, name: &'static str, value: &SharedString| {
        div()
            .flex()
            .items_center()
            .gap(spacing::S2)
            .child(swatch)
            .child(
                div()
                    .flex_1()
                    .text_role(typography::CAPTION)
                    .text_color(color::TEXT_SECONDARY)
                    .child(name),
            )
            .child(
                div()
                    .flex_none()
                    .whitespace_nowrap()
                    .text_role(typography::MONO_VALUE)
                    .text_color(color::TEXT)
                    .child(value.clone()),
            )
    };
    // The net's point, as it is drawn on the chart, in the 8px marker lane.
    let net_point = div()
        .flex()
        .flex_none()
        .justify_center()
        .w(size::MARKER)
        .child(div().size(size::DOT).rounded_full().bg(color::TEXT));
    div()
        .flex()
        .flex_col()
        .gap(spacing::S1)
        .w(px(TOOLTIP_WIDTH))
        .px(spacing::S2_5)
        .py(spacing::S2)
        .bg(color::HOVER)
        .border_1()
        .border_color(color::TOOLTIP_BORDER)
        .rounded(radius::SURFACE)
        .shadow(vec![shadow::TOOLTIP.box_shadow()])
        .child(
            div()
                .pb(spacing::S0_5)
                .text_role(typography::CAPTION)
                .font_weight(weight::STRONG)
                .text_color(color::TEXT)
                .child(month.title.clone()),
        )
        .child(row(spending_swatch(), "Spending", &month.spending.text))
        .child(row(income_swatch(), "Income", &month.income.text))
        .child(row(net_point, "Net", &month.net.text))
}

/// The net line through the month centres, over a `canvas` halo.
fn net_line(fractions: Vec<(f32, f32)>) -> impl IntoElement {
    canvas(
        |_, _, _| (),
        move |bounds, (), window, _| {
            if fractions.len() < 2 {
                return;
            }
            let at = |(x, y): (f32, f32)| {
                point(
                    bounds.origin.x + bounds.size.width * x,
                    bounds.origin.y + bounds.size.height * y,
                )
            };
            // The halo first, the same path wider, then the line itself.
            for (width, fill) in [(px(HALO), color::CANVAS), (size::STROKE, color::TEXT)] {
                let mut path = PathBuilder::stroke(width);
                path.move_to(at(fractions[0]));
                for &next in &fractions[1..] {
                    path.line_to(at(next));
                }
                if let Ok(path) = path.build() {
                    window.paint_path(path, fill);
                }
            }
        },
    )
    .absolute()
    .size_full()
}

impl IncomeChart {
    /// The month's selection overlay: the outline round the pair, four
    /// handles and the molten label.
    fn selection(&self, index: usize, axis: &Axis) -> Div {
        let month = &self.months[index];
        let extents = [month.spending.minor, month.income.minor]
            .map(|minor| BarExtent::new(minor, axis, PLOT_HEIGHT));
        let zero = PLOT_HEIGHT * axis.fraction(0);
        let top = extents
            .iter()
            .flatten()
            .map(|extent| extent.top)
            .fold(zero, f32::min);
        let bottom = extents
            .iter()
            .flatten()
            .map(BarExtent::bottom)
            .fold(zero, f32::max);
        let top = top.min(bottom - OUTLINE_MIN_HEIGHT);
        let reach = OUTLINE_GAP + f32::from(size::STROKE);
        let half = PAIR_WIDTH / 2. + reach;
        let handle = |left: bool, upper: bool| {
            let offset = -f32::from(size::HANDLE) / 2.;
            div()
                .absolute()
                .when(left, |this| this.left(px(offset)))
                .when(!left, |this| this.right(px(offset)))
                .when(upper, |this| this.top(px(offset)))
                .when(!upper, |this| this.bottom(px(offset)))
                .size(size::HANDLE)
                .bg(color::WHITE)
                .border(size::STROKE)
                .border_color(color::MOLTEN)
        };
        let outline = div()
            .id("chart-selection")
            .test_support()
            .absolute()
            .left(px(-half))
            .top(px(top - reach))
            .w(px(half * 2.))
            .h(px(bottom - top + reach * 2.))
            .border(size::STROKE)
            .border_color(color::MOLTEN)
            .child(handle(true, true))
            .child(handle(false, true))
            .child(handle(true, false))
            .child(handle(false, false));
        let label = div()
            .absolute()
            .top(px(top
                - reach
                - LABEL_RISE
                - f32::from(SELECTION_LABEL.line_height)))
            .map(|this| match label_edge(index, self.months.len()) {
                Side::Left => this.left(px(-half)),
                Side::Right => this.right(px(-half)),
            })
            .px(spacing::S1)
            .rounded(radius::LABEL)
            .bg(color::MOLTEN)
            .whitespace_nowrap()
            .text_role(SELECTION_LABEL)
            .text_color(color::WHITE)
            // "Oct · £2,481.90", as Paper `2HP-0`.
            .child(SharedString::from(format!(
                "{} · {}",
                month.label, month.spending.text
            )));
        div()
            .absolute()
            .left(relative(centre(index, self.months.len())))
            .top_0()
            .size_0()
            .child(outline)
            .child(label)
    }

    /// The hovered month's net point and tooltip.
    fn hover(&self, index: usize, axis: &Axis) -> Div {
        let month = &self.months[index];
        let net_top = PLOT_HEIGHT * axis.fraction(month.net.minor);
        let ring = spacing::S0_5;
        let point = (!month.before_data).then(|| {
            div()
                .id("chart-net-point")
                .test_support()
                .absolute()
                .left(-(size::DOT / 2. + ring))
                .top(px(net_top) - (size::DOT / 2. + ring))
                .size(size::DOT + ring * 2.)
                .rounded_full()
                .bg(color::TEXT)
                .border(ring)
                .border_color(color::CANVAS)
        });
        let tooltip = tooltip(month)
            .id("chart-tooltip")
            .test_support()
            .absolute()
            .top_0()
            .map(
                |this| match tooltip_side(index, self.selected, self.months.len()) {
                    Side::Right => this.left(px(TOOLTIP_OFFSET)),
                    Side::Left => this.right(px(TOOLTIP_OFFSET)),
                },
            );
        div()
            .absolute()
            .left(relative(centre(index, self.months.len())))
            .top_0()
            .size_0()
            .children(point)
            .child(tooltip)
    }
}

impl RenderOnce for IncomeChart {
    fn render(mut self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let count = self.months.len();
        let lowest = self
            .months
            .iter()
            .flat_map(|month| [month.spending.minor, month.income.minor, month.net.minor])
            .min()
            .unwrap_or(0);
        let highest = self
            .months
            .iter()
            .flat_map(|month| [month.spending.minor, month.income.minor, month.net.minor])
            .max()
            .unwrap_or(0);
        let axis = Axis::new(lowest, highest, self.currency);
        let selected = self.selected.filter(|&index| index < count);
        let hovered_state = window.use_keyed_state(self.id.clone(), cx, |_, _| None::<usize>);
        let hovered = (*hovered_state.read(cx)).filter(|&index| index < count);
        let on_select = self.on_select.take().map(std::rc::Rc::new);

        let ticks = axis.ticks();
        let labels = ticks.iter().enumerate().map(|(at, &tick)| {
            div()
                .absolute()
                .right_0()
                .top(
                    px(axis.gridline_y(at as u32, PLOT_HEIGHT))
                        - typography::MONO_MICRO.line_height / 2.,
                )
                .whitespace_nowrap()
                .text_role(typography::MONO_MICRO)
                .text_color(color::TEXT_MUTED)
                .child(compact_label(tick, self.currency))
        });
        let gridlines = ticks.iter().enumerate().map(|(at, &tick)| {
            line(
                axis.gridline_y(at as u32, PLOT_HEIGHT),
                if tick == 0 {
                    color::BORDER_STRONG
                } else {
                    color::HAIRLINE
                },
            )
        });

        let bands = self.months.iter().enumerate().map(|(index, month)| {
            let spending = BarExtent::new(month.spending.minor, &axis, PLOT_HEIGHT);
            let income = BarExtent::new(month.income.minor, &axis, PLOT_HEIGHT);
            let pair = div()
                .relative()
                .flex_none()
                .w(px(PAIR_WIDTH))
                .h_full()
                .when_some(spending, |this, extent| {
                    this.child(bar(0., extent, color::TEXT_STRONG, None))
                })
                .when_some(income, |this, extent| {
                    this.child(bar(
                        BAR_WIDTH + BAR_GAP,
                        extent,
                        color::CHART_INCOME_FILL,
                        Some(color::TEXT_MUTED),
                    ))
                });
            let on_select = on_select.clone();
            let hover = hovered_state.clone();
            div()
                .id(month_id(index))
                .test_support()
                .flex()
                .flex_1()
                .justify_center()
                .h_full()
                .cursor_pointer()
                .on_hover(move |entered, _, cx| {
                    hover.update(cx, |hovered, cx| {
                        let next = match (*entered, *hovered) {
                            (true, _) => Some(index),
                            (false, Some(current)) if current != index => Some(current),
                            (false, _) => None,
                        };
                        if *hovered != next {
                            *hovered = next;
                            cx.notify();
                        }
                    });
                })
                .when_some(on_select, move |this, on_select| {
                    this.on_click(move |_, window, cx| on_select(index, window, cx))
                })
                .child(pair)
        });

        let fractions: Vec<(f32, f32)> = self
            .months
            .iter()
            .enumerate()
            .filter(|(_, month)| !month.before_data)
            .map(|(index, month)| (centre(index, count), axis.fraction(month.net.minor)))
            .collect();

        let plot = div()
            .relative()
            .flex_1()
            .min_w_0()
            .h(px(PLOT_HEIGHT))
            .children(gridlines)
            .child(div().absolute().size_full().flex().children(bands))
            .child(net_line(fractions))
            .when_some(selected, |this, index| {
                this.child(self.selection(index, &axis))
            })
            .when_some(hovered, |this, index| this.child(self.hover(index, &axis)));

        let x_labels = self.months.iter().enumerate().map(|(index, month)| {
            div()
                .flex()
                .flex_1()
                .justify_center()
                .whitespace_nowrap()
                .text_role(typography::MONO_MICRO)
                .text_color(if selected == Some(index) {
                    color::TEXT
                } else {
                    color::TEXT_MUTED
                })
                .child(month.label.clone())
        });

        div()
            .flex()
            .flex_col()
            .gap(spacing::S2)
            .child(
                div()
                    .flex()
                    .gap(spacing::S2)
                    .child(
                        div()
                            .relative()
                            .flex_none()
                            .w(px(LABEL_COLUMN))
                            .h(px(PLOT_HEIGHT))
                            .children(labels),
                    )
                    .child(plot),
            )
            .child(
                div()
                    .flex()
                    .gap(spacing::S2)
                    .child(div().flex_none().w(px(LABEL_COLUMN)))
                    .child(div().flex().flex_1().children(x_labels)),
            )
    }
}

#[cfg(test)]
mod tests {
    use std::cell::RefCell;
    use std::rc::Rc;

    use gpui_kit::test::TestWindowExt as _;
    use gpui_kit::{Context, Render, TestAppContext, VisualTestContext};

    use super::*;

    fn month(index: usize, spending: i64, income: i64) -> ChartMonth {
        let names = [
            "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
        ];
        ChartMonth::new(names[index], format!("{} 2026", names[index]))
            .spending(spending, format!("£{}", spending / 100))
            .income(income, format!("£{}", income / 100))
            .net(income - spending, format!("£{}", (income - spending) / 100))
    }

    fn year(spending: i64, income: i64) -> Vec<ChartMonth> {
        (0..12)
            .map(|index| month(index, spending, income))
            .collect()
    }

    /// A chart with the selection kept as an owner would.
    struct Harness {
        months: Vec<ChartMonth>,
        selected: Option<usize>,
        selections: Rc<RefCell<Vec<usize>>>,
    }

    impl Render for Harness {
        fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
            let this = cx.entity();
            let selections = self.selections.clone();
            div().w(px(1000.)).p(px(24.)).child(
                IncomeChart::new("chart", Currency::GBP, self.months.clone())
                    .selected(self.selected)
                    .on_select(move |index, _, cx| {
                        selections.borrow_mut().push(index);
                        this.update(cx, |harness, cx| {
                            harness.selected = Some(index);
                            cx.notify();
                        });
                    }),
            )
        }
    }

    fn open(
        cx: &mut TestAppContext,
        months: Vec<ChartMonth>,
        selected: Option<usize>,
    ) -> (&mut VisualTestContext, Rc<RefCell<Vec<usize>>>) {
        cx.update(|cx| {
            gpui_kit::init(cx);
            crate::theme::component::install(cx);
        });
        let selections = Rc::new(RefCell::new(Vec::new()));
        let (_, cx) = cx.add_window_view({
            let selections = selections.clone();
            move |_, _| Harness {
                months,
                selected,
                selections,
            }
        });
        redraw(cx);
        (cx, selections)
    }

    fn redraw(cx: &mut VisualTestContext) {
        cx.run_until_parked();
        cx.update(|window, cx| window.draw(cx).clear(cx));
    }

    fn shows(id: &'static str, cx: &mut VisualTestContext) -> bool {
        cx.update(|window, _| window.try_find(id).is_some())
    }

    #[gpui_kit::test]
    fn clicking_a_month_selects_it(cx: &mut TestAppContext) {
        let (cx, selections) = open(cx, year(250_000, 345_000), Some(9));
        assert!(shows("chart-selection", cx));
        cx.update(|window, cx| window.click("chart-month-3", cx));
        redraw(cx);
        assert_eq!(*selections.borrow(), [3]);
        // The owner passed the new selection back: the overlay moved.
        let (overlay, band) = cx.update(|window, _| {
            (
                window.find("chart-selection").bounds(),
                window.find("chart-month-3").bounds(),
            )
        });
        assert!(overlay.center().x > band.left() && overlay.center().x < band.right());
    }

    #[gpui_kit::test]
    fn the_twelve_bands_share_the_plot_equally(cx: &mut TestAppContext) {
        let (cx, _) = open(cx, year(250_000, 345_000), None);
        let bounds: Vec<_> = (0..12)
            .map(|index| cx.update(|window, _| window.find(month_id(index)).bounds()))
            .collect();
        for pair in bounds.windows(2) {
            assert_eq!(pair[0].size.width, pair[1].size.width);
            assert!((pair[0].right() - pair[1].left()).abs() < px(0.01));
        }
        assert_eq!(bounds[0].size.height, px(PLOT_HEIGHT));
    }

    #[gpui_kit::test]
    fn hovering_a_month_shows_its_tooltip_until_it_leaves(cx: &mut TestAppContext) {
        let (cx, _) = open(cx, year(250_000, 345_000), Some(9));
        assert!(!shows("chart-tooltip", cx));
        cx.update(|window, cx| window.hover("chart-month-6", cx));
        redraw(cx);
        assert!(shows("chart-tooltip", cx));
        let (tooltip, band) = cx.update(|window, _| {
            (
                window.find("chart-tooltip").bounds(),
                window.find("chart-month-6").bounds(),
            )
        });
        // July is before the selected October: the tooltip opens to its left.
        assert!(tooltip.right() < band.center().x);
        cx.update(|window, cx| window.hover("chart-month-9", cx));
        redraw(cx);
        assert!(shows("chart-tooltip", cx), "now October's");
    }

    #[gpui_kit::test]
    fn an_empty_chart_renders_its_axis(cx: &mut TestAppContext) {
        let (cx, selections) = open(cx, Vec::new(), Some(3));
        assert!(!shows("chart-month-0", cx));
        assert!(!shows("chart-selection", cx), "no month to select");
        assert!(selections.borrow().is_empty());
    }

    #[gpui_kit::test]
    fn all_zero_values_draw_no_bars_but_still_take_clicks(cx: &mut TestAppContext) {
        let (cx, selections) = open(cx, year(0, 0), Some(0));
        assert!(shows("chart-selection", cx));
        cx.update(|window, cx| window.click("chart-month-11", cx));
        assert_eq!(*selections.borrow(), [11]);
    }

    #[gpui_kit::test]
    fn an_empty_months_selection_still_frames_a_bar(cx: &mut TestAppContext) {
        let mut months = year(250_000, 345_000);
        months[11] = month(11, 0, 0);
        let (cx, _) = open(cx, months, Some(11));
        let outline = cx.update(|window, _| window.find("chart-selection").bounds());
        let reach = OUTLINE_GAP + f32::from(size::STROKE);
        // Not flattened onto the axis: a bar's width tall, plus the gap.
        assert!(outline.size.height >= px(OUTLINE_MIN_HEIGHT + reach * 2.) - px(0.01));
    }

    #[gpui_kit::test]
    fn a_month_before_any_data_has_no_net_point(cx: &mut TestAppContext) {
        let mut months = year(0, 0);
        months[0] = months[0].clone().before_data();
        months[11] = month(11, 250_000, 345_000);
        let (cx, _) = open(cx, months, Some(11));
        // Hovering still shows the month's tooltip, but no point: the point
        // is the line's, and the line starts at the first month with data.
        cx.update(|window, cx| window.hover("chart-month-0", cx));
        redraw(cx);
        assert!(shows("chart-tooltip", cx));
        assert!(!shows("chart-net-point", cx));
        cx.update(|window, cx| window.hover("chart-month-1", cx));
        redraw(cx);
        assert!(shows("chart-net-point", cx), "a month with data");
    }

    #[gpui_kit::test]
    fn a_negative_net_renders(cx: &mut TestAppContext) {
        let mut months = year(250_000, 345_000);
        months[4] = month(4, 450_000, 100_000);
        let (cx, _) = open(cx, months, Some(4));
        cx.update(|window, cx| window.hover("chart-month-4", cx));
        redraw(cx);
        assert!(shows("chart-tooltip", cx));
        assert!(shows("chart-selection", cx));
    }
}
